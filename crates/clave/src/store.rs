//! The clave state store (spec §5): one JSON file, read-modified-written under
//! an advisory lock held on a SEPARATE, never-renamed lockfile, with the data
//! write done as temp-file + atomic rename.
//!
//! WHY the separate lockfile: locking the data file itself would be a bug —
//! the atomic rename swaps the data file's inode out from under a second
//! writer's lock, and concurrent hooks (Claude's global hook fan-in) would
//! silently lose updates. The lockfile never gets renamed, so its inode is a
//! stable lock anchor. The flip side: readers that only need a consistent
//! (possibly slightly stale) view can skip the lock entirely — an
//! atomic-rename reader always sees a whole file. That lock-free path is what
//! keeps `clave hook` from ever delaying an UNTRACKED Claude session (§6.5).

use std::collections::BTreeMap;
use std::fs;
use std::io::Write;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result};
use clave_types::{Agent, AgentSnapshot, Status};
use fs4::fs_std::FileExt;
use serde::{Deserialize, Serialize};

/// Where an agent's label came from (§6.4). While `FirstPrompt`, `clave hook`
/// keeps tail-scanning the jsonl for a session summary; once `Summary`, it
/// stops re-scanning forever (the label only meaningfully changes once).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LabelSource {
    FirstPrompt,
    Summary,
}

/// One store row (spec §5's agent record, minus the deleted `archived`).
/// Mirrors `clave_types::Agent` plus store-only fields (`worktree`,
/// `label_source`) that the plugin never needs to see.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentRecord {
    /// Minted session UUID — the join key (invariant #3).
    pub uuid: String,
    /// PHYSICAL (canonicalized) working directory — see munge.rs / S0b.
    pub cwd: String,
    /// git toplevel of cwd; keys the §6.3 add/resume picker.
    pub repo_root: String,
    pub branch: String,
    /// `dir · branch · summary-or-first-prompt` (§6.4).
    pub label: String,
    pub status: Status,
    /// unix s; bumped on UserPromptSubmit → drives the bar's recency order.
    pub last_interacted: u64,
    /// unix s; bumped on focus (`clave focus`) → clears done-unread.
    pub last_visited: u64,
    /// Worktree path if `clave add --worktree` created one (§6.3), else None.
    pub worktree: Option<String>,
    pub label_source: LabelSource,
}

/// The whole store file. `seq` is the monotonic snapshot counter of the §5
/// pipe contract — persisted HERE so pushes stay monotonic across processes
/// (every hook invocation is a fresh process).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Store {
    #[serde(default)]
    pub seq: u64,
    /// Keyed by uuid. BTreeMap for stable iteration order (deterministic
    /// snapshots and `ls` output).
    #[serde(default)]
    pub agents: BTreeMap<String, AgentRecord>,
}

pub struct StorePaths {
    pub dir: PathBuf,
    pub data: PathBuf,
    pub lock: PathBuf,
}

/// Spec §5 names the literal path `~/.local/state/clave/`. Built from $HOME
/// rather than `dirs::state_dir()` because the latter is `None` on macOS and
/// we want one path on every platform.
pub fn store_paths() -> Result<StorePaths> {
    let home = dirs::home_dir().context("cannot determine home directory")?;
    let dir = home.join(".local").join("state").join("clave");
    Ok(StorePaths {
        data: dir.join("agents.json"),
        lock: dir.join("agents.lock"),
        dir,
    })
}

/// Lock-free read. Safe without the lock because writers replace the file by
/// atomic rename — a reader opens either the old whole file or the new whole
/// file, never a torn write. Missing file = empty store (first run).
pub fn read_store(paths: &StorePaths) -> Result<Store> {
    match fs::read(&paths.data) {
        Ok(bytes) => Ok(serde_json::from_slice(&bytes)?),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Store::default()),
        Err(e) => Err(e).context("reading store"),
    }
}

/// Locked read-modify-write. The exclusive flock on the lockfile is held
/// across the WHOLE read→mutate→write, so concurrent hook events serialize
/// instead of clobbering each other. The closure's return value passes
/// through so callers can extract data computed under the lock.
pub fn with_store_mut<T>(paths: &StorePaths, f: impl FnOnce(&mut Store) -> T) -> Result<T> {
    fs::create_dir_all(&paths.dir).context("creating store dir")?;
    let lock = fs::OpenOptions::new()
        .create(true)
        .write(true)
        .open(&paths.lock)
        .context("opening lockfile")?;
    lock.lock_exclusive().context("locking store")?;
    // From here to the end of the function we hold the lock; any early return
    // (via ?) drops `lock`, which releases the flock — no poisoned state.
    let mut store = read_store(paths)?;
    let out = f(&mut store);
    let tmp = paths.dir.join("agents.json.tmp");
    {
        let mut t = fs::File::create(&tmp).context("creating temp store")?;
        t.write_all(&serde_json::to_vec_pretty(&store)?)?;
        // fsync BEFORE the rename so a crash can't leave a renamed-but-empty
        // file (rename is atomic, buffered content is not).
        t.sync_all()?;
    }
    fs::rename(&tmp, &paths.data).context("atomic store swap")?;
    FileExt::unlock(&lock).context("unlocking store")?;
    Ok(out)
}

/// Store → pipe snapshot (§5): drop the store-only fields, keep the order.
pub fn snapshot_from(store: &Store) -> AgentSnapshot {
    AgentSnapshot {
        seq: store.seq,
        agents: store
            .agents
            .values()
            .map(|r| Agent {
                uuid: r.uuid.clone(),
                cwd: r.cwd.clone(),
                repo_root: r.repo_root.clone(),
                branch: r.branch.clone(),
                label: r.label.clone(),
                status: r.status,
                last_interacted: r.last_interacted,
                last_visited: r.last_visited,
            })
            .collect(),
    }
}

/// `clave focus <uuid>` (§6.5): persist the "user looked at it" transition
/// and hand back a seq-bumped snapshot for the caller to push. Zellij only
/// delivers TabUpdate to the ACTIVE tab's bar instance (C3 live finding), so
/// exactly one instance repainted locally — the pipe push is how every other
/// instance learns the flip. Unknown uuid returns None (no bump, no push):
/// the plugin can race an agent whose tab just closed.
pub fn apply_focus(paths: &StorePaths, uuid: &str, now: u64) -> Result<Option<AgentSnapshot>> {
    with_store_mut(paths, |s| {
        let r = s.agents.get_mut(uuid)?;
        r.last_visited = now;
        if r.status == Status::Done {
            r.status = Status::Idle; // green "done & unread" → dim
        }
        s.seq += 1; // monotonic pipe contract (§5)
        Some(snapshot_from(s))
    })
}

/// Seconds since the epoch — the store's one timestamp format.
pub fn now_unix() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp_paths(dir: &std::path::Path) -> StorePaths {
        StorePaths {
            dir: dir.to_path_buf(),
            data: dir.join("agents.json"),
            lock: dir.join("agents.lock"),
        }
    }

    fn rec(uuid: &str) -> AgentRecord {
        AgentRecord {
            uuid: uuid.into(),
            cwd: "/x".into(),
            repo_root: "/x".into(),
            branch: "main".into(),
            label: "x · main".into(),
            status: Status::Idle,
            last_interacted: 0,
            last_visited: 0,
            worktree: None,
            label_source: LabelSource::FirstPrompt,
        }
    }

    #[test]
    fn missing_file_reads_as_default() {
        let d = tempfile::tempdir().unwrap();
        let s = read_store(&tmp_paths(d.path())).unwrap();
        assert_eq!(s.seq, 0);
        assert!(s.agents.is_empty());
    }

    #[test]
    fn rmw_roundtrips_and_bumps_seq() {
        let d = tempfile::tempdir().unwrap();
        let p = tmp_paths(d.path());
        with_store_mut(&p, |s| {
            s.agents.insert("u1".into(), rec("u1"));
            s.seq += 1;
        })
        .unwrap();
        // A second, separate RMW sees the first one's write (file-backed).
        with_store_mut(&p, |s| {
            assert_eq!(s.seq, 1);
            s.agents.get_mut("u1").unwrap().status = Status::Working;
            s.seq += 1;
        })
        .unwrap();
        let s = read_store(&p).unwrap();
        assert_eq!(s.seq, 2);
        assert_eq!(s.agents["u1"].status, Status::Working);
        // The write went through temp+rename: no stray temp file remains.
        assert!(!p.dir.join("agents.json.tmp").exists());
        // The lockfile exists and was NOT renamed away (it is the lock anchor).
        assert!(p.lock.exists());
    }

    #[test]
    fn focus_clears_done_to_idle_and_stamps_visit() {
        let d = tempfile::tempdir().unwrap();
        let p = tmp_paths(d.path());
        with_store_mut(&p, |s| {
            let mut r = rec("u1");
            r.status = Status::Done;
            s.agents.insert("u1".into(), r);
        })
        .unwrap();
        // Returns a seq-bumped snapshot for the pipe push: only the focused
        // tab's bar instance repaints locally (zellij starves hidden
        // instances of TabUpdates — C3 finding), so the flip must broadcast.
        let snap = apply_focus(&p, "u1", 999).unwrap().expect("row changed");
        let s = read_store(&p).unwrap();
        assert_eq!(s.agents["u1"].status, Status::Idle); // unread cleared
        assert_eq!(s.agents["u1"].last_visited, 999);
        assert!(s.seq > 0); // §5 pipe contract: the push must be strictly newer
        assert_eq!(snap.seq, s.seq);
        assert_eq!(snap.agents[0].status, Status::Idle);
        // Unknown uuid: silently fine (plugin may race a just-closed agent) —
        // no snapshot, seq untouched.
        assert!(apply_focus(&p, "ghost", 1000).unwrap().is_none());
        assert_eq!(read_store(&p).unwrap().seq, s.seq);
    }

    #[test]
    fn snapshot_mirrors_store_rows() {
        let mut s = Store::default();
        s.seq = 7;
        s.agents.insert("u1".into(), rec("u1"));
        let snap = snapshot_from(&s);
        assert_eq!(snap.seq, 7);
        assert_eq!(snap.agents.len(), 1);
        assert_eq!(snap.agents[0].uuid, "u1");
        assert_eq!(snap.agents[0].label, "x · main");
    }
}
