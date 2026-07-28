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
    /// Zellij tab id hosting this agent (§6.6 Design B), bound by the agent
    /// tab's own bar via `clave bind`. Keys the hook's prompt→timeline stamp
    /// and the bar's glyph join. Session-scoped: None until bound, reset on
    /// session recreate (see clear_tab_timeline).
    #[serde(default)]
    pub tab_id: Option<usize>,
    /// §5 (2026-07-17): `clave open` found the row's cwd missing → the bar
    /// renders ✗ instead of ◌. A row flag, NOT a status (statuses are hook
    /// lifecycle); cleared by a later successful open. `default` keeps
    /// pre-field payloads parseable.
    #[serde(default)]
    pub stale: bool,
    /// Claude's session rename, from the transcript's `custom-title` line.
    /// Store-side home for the wire field of the same name (#69). Written by
    /// S4 (#59); nothing populates it yet, so it stays None. `default` keeps
    /// pre-field store files loading — a missing key is a whole-store parse
    /// failure, not a blank field.
    #[serde(default)]
    pub title: Option<String>,
    /// The words segment, held structurally rather than only inside `label`
    /// (design-lock §7.1). Seeded once from existing labels by
    /// `backfill_summaries`; thereafter written by S4 (#59) from `ai-title`,
    /// the `type:"summary"` tier being extinct (#79). `default` keeps
    /// pre-field store files loading.
    #[serde(default)]
    pub summary: String,
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
    /// tab_id → unix seconds of the last user commitment (§6.6 row order).
    /// Kept HERE, not per bar instance: instance-local copies fed by
    /// fire-and-forget pipe deltas diverged live (C5 round 5) — the store
    /// RMW is the one writer, and the map rides every snapshot push.
    /// tab_ids are session-scoped: cleared on session (re)create.
    #[serde(default)]
    pub tab_timeline: BTreeMap<usize, u64>,
    /// Bar collapse mode (issue #5): plugin-side per-instance memory synced
    /// only by the toggle broadcast desynced live (C8 parity-desync — a
    /// reload or missed pipe flips one instance forever). Same doctrine as
    /// tab_timeline above: the store RMW is the one writer and the flag
    /// rides every snapshot push, so instances hydrate at birth and heal on
    /// every push. `default` (expanded) keeps pre-field store files loading.
    #[serde(default)]
    pub collapsed: bool,
}

pub struct StorePaths {
    pub dir: PathBuf,
    pub data: PathBuf,
    pub lock: PathBuf,
}

/// Spec §5 names the literal path `~/.local/state/clave/`. Built from $HOME
/// rather than `dirs::state_dir()` because the latter is `None` on macOS and
/// we want one path on every platform. `$CLAVE_STATE_DIR` overrides the whole
/// dir (spec §6.9: the dev harness sandboxes the store).
pub fn store_paths() -> Result<StorePaths> {
    let home = dirs::home_dir().context("cannot determine home directory")?;
    let default = home.join(".local").join("state").join("clave");
    let dir = crate::env::dir_from(std::env::var("CLAVE_STATE_DIR").ok(), default);
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
        // Contents are never read or written — this file exists only to hold
        // an flock (spec §5 store locking, fugu 2026-07-01: the lock is a
        // SEPARATE file, never the renamed-over data file, else concurrent
        // hooks lose updates). `truncate(false)` spells out the existing
        // default explicitly (no behavior change) — suspicious_open_options.
        .truncate(false)
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
        tab_timeline: store.tab_timeline.clone(),
        collapsed: store.collapsed,
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
                tab_id: r.tab_id,
                stale: r.stale,
                // Projected now — `AgentRecord` has carried this since §6.3
                // and the wire simply never did (S6 #61 §2.4).
                worktree: r.worktree.clone(),
                title: r.title.clone(),
                summary: r.summary.clone(),
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

/// `clave touch <tab_id>` (§6.6): stamp a user commitment on the STORE's
/// tab timeline and hand back a seq-bumped snapshot for the pipe push.
/// Max-merge so a late/duplicate older stamp can never regress the order
/// (concurrent birth touches from multiple bar instances are expected).
pub fn apply_touch(paths: &StorePaths, tab_id: usize, now: u64) -> Result<AgentSnapshot> {
    with_store_mut(paths, |s| {
        let e = s.tab_timeline.entry(tab_id).or_insert(0);
        *e = (*e).max(now);
        s.seq += 1; // monotonic pipe contract (§5)
        snapshot_from(s)
    })
}

/// `clave bind <uuid> <tab_id>` (§6.6 Design B): persist the uuid→tab join
/// reported by the agent tab's own bar instance (the only one whose data is
/// reliably fresh — it is active at spawn time). Snapshot back only on
/// CHANGE, so a bar re-reporting an existing bind costs no push. Unknown
/// uuid returns None (bar may race a pruned agent).
pub fn apply_bind(paths: &StorePaths, uuid: &str, tab_id: usize) -> Result<Option<AgentSnapshot>> {
    with_store_mut(paths, |s| {
        if !s.agents.contains_key(uuid) {
            return None; // bar may race a pruned agent
        }
        // Evict any OTHER agent still bound to this tab_id. zellij reuses
        // tab_ids (get_new_tab_id = max-key+1, screen.rs:1617), so a reused id
        // could otherwise leave a dead agent AND the new one both claiming the
        // tab — the bar's glyph join (agent_in_tab) would decorate it with
        // whichever uuid sorts first. One tab hosts one agent: the freshest
        // bind wins. (Prune_tabs is the primary cleaner; this closes the
        // close-then-immediately-reuse window where the id never went absent.)
        let mut evicted = false;
        for r in s.agents.values_mut() {
            if r.uuid != uuid && r.tab_id == Some(tab_id) {
                r.tab_id = None;
                evicted = true;
            }
        }
        let already = s.agents.get(uuid).and_then(|r| r.tab_id) == Some(tab_id);
        if already && !evicted {
            return None; // re-report of an existing bind, no collision → free
        }
        if let Some(r) = s.agents.get_mut(uuid) {
            r.tab_id = Some(tab_id);
        }
        s.seq += 1; // monotonic pipe contract (§5)
        Some(snapshot_from(s))
    })
}

/// `clave prune-tabs <stale tab ids…>` (#6/F3): drop tab_timeline entries and
/// clear agent tab_id binds for EXACTLY the ids listed — the ones the bar
/// observed die on a close. Session recreate wiped these wholesale
/// (clear_tab_timeline); mid-session tab CLOSE left them to grow unbounded —
/// and, since zellij REUSES tab_ids (get_new_tab_id = max-key+1, screen.rs:1617),
/// a survivor entry would let a reused-id tab inherit a dead agent's glyph/order.
///
/// REMOVE-LISTED, not retain-carried: the payload is the DEAD ids, so two
/// fire-and-forget prunes (no arrival-order guarantee — the collapse
/// pending-write class) COMMUTE and are idempotent; a late prune can only
/// re-remove ids already judged dead, never strip a bind for a tab it never
/// observed close. (A retain-only-live payload would clobber the bind of ANY
/// tab created after the prune was computed → live agent rendered dormant → the
/// #6 double-attach via a race.) Change-gated (no push when nothing matched).
/// Self-heal lives in the bar's detection (staleness re-derived each set
/// change), so a lost push is re-emitted while the entry persists. Residual: if
/// a listed id is REUSED within the subprocess-latency window a late removal
/// could unbind the new tenant — `apply_bind` eviction is the backstop and the
/// window is milliseconds. Empty payload = no-op (nothing observed dead).
/// None = no change.
pub fn apply_prune_tabs(paths: &StorePaths, stale_ids: &[usize]) -> Result<Option<AgentSnapshot>> {
    with_store_mut(paths, |s| {
        if stale_ids.is_empty() {
            return None; // nothing observed dead
        }
        let before = s.tab_timeline.len();
        s.tab_timeline.retain(|id, _| !stale_ids.contains(id));
        let mut changed = s.tab_timeline.len() != before;
        for r in s.agents.values_mut() {
            if r.tab_id.is_some_and(|id| stale_ids.contains(&id)) {
                r.tab_id = None;
                changed = true;
            }
        }
        if !changed {
            return None; // §5: no no-op pushes
        }
        s.seq += 1; // monotonic pipe contract (§5)
        Some(snapshot_from(s))
    })
}

/// `clave collapse <true|false>` (issue #5): persist the bar collapse mode
/// as an ABSOLUTE value — never a flip, so broadcast races and duplicate
/// executor writes stay idempotent — and hand back a seq-bumped snapshot
/// for the pipe push that heals any instance the `clave-toggle` broadcast
/// missed. Snapshot back only on CHANGE (like apply_bind): a re-assert of
/// the current mode must not generate pipe traffic (round 11: storms).
pub fn apply_collapse(paths: &StorePaths, collapsed: bool) -> Result<Option<AgentSnapshot>> {
    with_store_mut(paths, |s| {
        if s.collapsed == collapsed {
            return None;
        }
        s.collapsed = collapsed;
        s.seq += 1; // monotonic pipe contract (§5)
        Some(snapshot_from(s))
    })
}

/// `clave open` outcome (§6.3, 2026-07-17): record whether the row's cwd was
/// missing. Snapshot back only on CHANGE (a repeated stale open must not
/// generate pipe traffic); None for unknown uuids.
pub fn apply_open_result(
    paths: &StorePaths,
    uuid: &str,
    stale: bool,
) -> Result<Option<AgentSnapshot>> {
    with_store_mut(paths, |s| {
        let r = s.agents.get_mut(uuid)?;
        if r.stale == stale {
            return None;
        }
        r.stale = stale;
        s.seq += 1; // monotonic pipe contract (§5)
        Some(snapshot_from(s))
    })
}

/// Seed `summary` for rows written before the field existed, by lifting the
/// words segment out of the composed label (`dir · branch · words`).
///
/// `splitn(3)` so a summary that itself contains the separator survives
/// whole. Matches only EMPTY summaries, so it is idempotent and self-limiting
/// — after one pass nothing matches again. Same shape as S1 §3.6's
/// `commit_ord` backfill.
///
/// WHY it is needed at all, given S4 (#59) will keep summaries live:
/// `refresh_label` returns early forever once `label_source == Summary`
/// (`hook.rs:155`), and dormant rows receive no hook events by definition —
/// so without this they render a blank 17-column field indefinitely.
///
/// Returns whether anything changed, so the caller can gate its `seq` bump:
/// §5 forbids no-op pushes.
pub fn backfill_summaries(s: &mut Store) -> bool {
    let mut changed = false;
    for r in s.agents.values_mut() {
        if !r.summary.is_empty() {
            continue;
        }
        if let Some(words) = r.label.splitn(3, clave_types::LABEL_SEP).nth(2)
            && !words.is_empty()
        {
            r.summary = words.to_string();
            changed = true;
        }
    }
    changed
}

/// Session (re)create hygiene: tab_ids are SESSION-scoped, so a fresh
/// session must inherit neither dead tabs' commitments (reused ids) nor
/// stale uuid→tab binds. No push — no bar instance exists yet at launch
/// time; hydration reads the store.
pub fn clear_tab_timeline(paths: &StorePaths) -> Result<()> {
    with_store_mut(paths, |s| {
        let bound = s.agents.values().any(|r| r.tab_id.is_some());
        let mut changed = false;
        if !s.tab_timeline.is_empty() || bound {
            s.tab_timeline.clear();
            s.agents.values_mut().for_each(|r| r.tab_id = None);
            changed = true;
        }
        // Session create is the one locked pass that runs at every launch,
        // so it is where the one-shot backfill rides (#69). Accepted cost: a
        // MID-session upgrade leaves dormant rows blank until the next
        // launch. The alternative is a migration hook on every store open —
        // more machinery than a cosmetic gap on unused rows justifies.
        changed |= backfill_summaries(s);
        if changed {
            s.seq += 1; // content changed ⇒ seq changed (§5)
        }
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
            tab_id: None,
            stale: false,
            title: None,
            summary: String::new(),
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
        let mut s = Store {
            seq: 7,
            ..Store::default()
        };
        s.agents.insert("u1".into(), rec("u1"));
        s.tab_timeline.insert(4, 1700);
        s.agents.get_mut("u1").unwrap().tab_id = Some(4);
        let snap = snapshot_from(&s);
        assert_eq!(snap.seq, 7);
        assert_eq!(snap.agents.len(), 1);
        assert_eq!(snap.agents[0].uuid, "u1");
        assert_eq!(snap.agents[0].label, "x · main");
        // §6.6 store-timeline: order rides every snapshot.
        assert_eq!(snap.tab_timeline.get(&4), Some(&1700));
        // §6.6 Design B: the uuid→tab bind rides it too (glyph join key).
        assert_eq!(snap.agents[0].tab_id, Some(4));
    }

    #[test]
    fn collapse_persists_absolute_value_and_dedupes() {
        // `clave collapse` (issue #5): absolute value, seq-bumped RMW,
        // change-gated push — the store copy of the C8 parity-desync fix.
        let d = tempfile::tempdir().unwrap();
        let p = tmp_paths(d.path());
        let snap = apply_collapse(&p, true).unwrap().expect("changed");
        assert!(snap.collapsed, "snapshot must carry the new mode");
        // The FILE is the durable truth (CodeRabbit CLI, PR #13): assert the
        // persisted store too, not just the in-memory snapshot projection.
        assert!(read_store(&p).unwrap().collapsed);
        assert_eq!(snap.seq, 1);
        // Re-asserting the same mode: no change, no seq bump, no push —
        // duplicate executor writes after a broadcast are free (round 11).
        assert!(apply_collapse(&p, true).unwrap().is_none());
        assert_eq!(read_store(&p).unwrap().seq, 1);
        // Toggling back is a change again.
        let snap = apply_collapse(&p, false).unwrap().expect("changed back");
        assert!(!snap.collapsed);
        assert!(!read_store(&p).unwrap().collapsed);
        assert_eq!(snap.seq, 2);
    }

    #[test]
    fn bind_records_tab_id_once_and_ignores_unknown_or_unchanged() {
        // `clave bind <uuid> <tab_id>` (§6.6 Design B): the agent tab's own
        // bar reports its join to the store so every OTHER instance can key
        // glyphs/order off the snapshot instead of local joins (round 6:
        // register pipes don't replay; hidden manifests go stale).
        let d = tempfile::tempdir().unwrap();
        let p = tmp_paths(d.path());
        with_store_mut(&p, |s| {
            s.agents.insert("u1".into(), rec("u1"));
        })
        .unwrap();
        let snap = apply_bind(&p, "u1", 4).unwrap().expect("bound");
        assert_eq!(snap.agents[0].tab_id, Some(4));
        assert_eq!(snap.seq, 1);
        // Same bind again: no change, no seq bump, no push.
        assert!(apply_bind(&p, "u1", 4).unwrap().is_none());
        assert_eq!(read_store(&p).unwrap().seq, 1);
        // A MOVED agent (pane broken out to a new tab) re-binds.
        let snap = apply_bind(&p, "u1", 9).unwrap().expect("rebound");
        assert_eq!(snap.agents[0].tab_id, Some(9));
        // Unknown uuid: silently none (bar may race a pruned agent).
        assert!(apply_bind(&p, "ghost", 1).unwrap().is_none());
    }

    #[test]
    fn prune_tabs_removes_listed_stale_ids_order_safe_and_change_gated() {
        // #6/F3: mid-session tab CLOSE left binds + tab_timeline entries to grow
        // unbounded, and — with tab_id reuse (screen.rs:1617) — a survivor
        // decorates a reused-id tab. The bar reports the DEAD ids it observed;
        // the store REMOVES exactly those (not "retain the live set") so
        // out-of-order prunes commute and can't unbind a tab they never saw die.
        let d = tempfile::tempdir().unwrap();
        let p = tmp_paths(d.path());
        with_store_mut(&p, |s| {
            let mut live = rec("u-live");
            live.tab_id = Some(10);
            s.agents.insert("u-live".into(), live);
            let mut dead = rec("u-dead");
            dead.tab_id = Some(11); // its tab just closed
            s.agents.insert("u-dead".into(), dead);
        })
        .unwrap();
        apply_touch(&p, 10, 100).unwrap();
        apply_touch(&p, 11, 200).unwrap(); // stale timeline entry
        // Stale set is {11}: remove EXACTLY 11's bind + timeline entry; 10 (a
        // tab the prune never observed die) is untouched — the order-safety.
        let snap = apply_prune_tabs(&p, &[11]).unwrap().expect("pruned");
        let s = read_store(&p).unwrap();
        assert_eq!(s.agents["u-live"].tab_id, Some(10), "live bind untouched");
        assert_eq!(s.agents["u-dead"].tab_id, None, "dead bind cleared");
        assert!(s.tab_timeline.contains_key(&10));
        assert!(!s.tab_timeline.contains_key(&11), "stale timeline dropped");
        assert!(snap.agents.iter().all(|a| a.tab_id != Some(11)));
        // Idempotent late arrival: re-removing an already-dead id → no change,
        // no push, no seq bump (this is what makes two out-of-order prunes safe).
        let seq = read_store(&p).unwrap().seq;
        assert!(apply_prune_tabs(&p, &[11]).unwrap().is_none());
        assert_eq!(read_store(&p).unwrap().seq, seq);
        // Empty payload (nothing observed dead) → no-op.
        assert!(apply_prune_tabs(&p, &[]).unwrap().is_none());
        assert_eq!(read_store(&p).unwrap().agents["u-live"].tab_id, Some(10));
    }

    #[test]
    fn bind_evicts_a_reused_tab_id_from_the_previous_agent() {
        // zellij reuses tab_ids (get_new_tab_id = max-key+1, screen.rs:1617):
        // if a dead agent's bind survived a close, a NEW agent binding the
        // REUSED id must EVICT it — one tab hosts one agent, else the bar's
        // glyph join (agent_in_tab) decorates the tab with whichever uuid sorts
        // first (a dead agent's colour). Belt-and-suspenders with prune_tabs.
        // Fixture (CodeRabbit MINOR): u-new ALREADY holds Some(11) too, so the
        // call hits the `already && evicted` path — it must NOT short-circuit
        // the no-op return (that would leave the dead bind in place); it must
        // evict u-dead AND emit a snapshot.
        let d = tempfile::tempdir().unwrap();
        let p = tmp_paths(d.path());
        with_store_mut(&p, |s| {
            let mut dead = rec("u-dead");
            dead.tab_id = Some(11); // survived a close, still bound to 11
            s.agents.insert("u-dead".into(), dead);
            let mut newer = rec("u-new");
            newer.tab_id = Some(11); // already bound to the reused id → `already`
            s.agents.insert("u-new".into(), newer);
        })
        .unwrap();
        // `already && evicted`: u-new is unchanged but u-dead IS evicted, so the
        // write is real — a snapshot must come back (not the no-op None).
        let snap = apply_bind(&p, "u-new", 11)
            .unwrap()
            .expect("collision eviction must push even when the binder is unchanged");
        let s = read_store(&p).unwrap();
        assert_eq!(s.agents["u-new"].tab_id, Some(11), "new agent stays bound");
        assert_eq!(
            s.agents["u-dead"].tab_id, None,
            "reused id evicted the dead bind"
        );
        assert!(snap.agents.iter().filter(|a| a.tab_id == Some(11)).count() == 1);
    }

    #[test]
    fn clear_session_state_wipes_timeline_and_binds() {
        // Both maps are keyed by session-scoped tab_ids — a recreated
        // session must inherit neither.
        let d = tempfile::tempdir().unwrap();
        let p = tmp_paths(d.path());
        with_store_mut(&p, |s| {
            s.agents.insert("u1".into(), rec("u1"));
        })
        .unwrap();
        apply_touch(&p, 4, 1700).unwrap();
        apply_bind(&p, "u1", 4).unwrap();
        clear_tab_timeline(&p).unwrap();
        let s = read_store(&p).unwrap();
        assert!(s.tab_timeline.is_empty());
        assert_eq!(s.agents["u1"].tab_id, None);
    }

    #[test]
    fn touch_stamps_timeline_bumps_seq_and_never_regresses() {
        // `clave touch <tab_id>` (§6.6): the ONE writer of tab order. Locked
        // RMW here — per-instance pipe-delta merges diverged live (C5 rd 5).
        let d = tempfile::tempdir().unwrap();
        let p = tmp_paths(d.path());
        let snap = apply_touch(&p, 4, 1700).unwrap();
        assert_eq!(snap.tab_timeline.get(&4), Some(&1700));
        assert_eq!(snap.seq, 1); // §5: every push strictly newer
        // Later commitment moves it forward…
        let snap = apply_touch(&p, 4, 2000).unwrap();
        assert_eq!(snap.tab_timeline.get(&4), Some(&2000));
        // …but a late/duplicate OLDER stamp can't regress it (max-merge).
        let snap = apply_touch(&p, 4, 100).unwrap();
        assert_eq!(snap.tab_timeline.get(&4), Some(&2000));
        assert_eq!(snap.seq, 3);
        // Persisted: a fresh read sees the same map.
        assert_eq!(read_store(&p).unwrap().tab_timeline.get(&4), Some(&2000));
    }

    #[test]
    fn open_result_sets_and_clears_stale_on_change_only() {
        // §6.3 `clave open`: cwd missing → stale=true (bar ✗); a later
        // successful open clears it. Snapshot back only on CHANGE (§5: no
        // no-op pushes), None for unknown uuids (row may have been pruned).
        let d = tempfile::tempdir().unwrap();
        let p = tmp_paths(d.path());
        with_store_mut(&p, |s| {
            s.agents.insert("u1".into(), rec("u1"));
        })
        .unwrap();
        let snap = apply_open_result(&p, "u1", true).unwrap().expect("changed");
        assert!(snap.agents[0].stale);
        assert_eq!(snap.seq, 1);
        // Same value again: no change, no seq bump, no push.
        assert!(apply_open_result(&p, "u1", true).unwrap().is_none());
        assert_eq!(read_store(&p).unwrap().seq, 1);
        // Successful open clears it.
        let snap = apply_open_result(&p, "u1", false)
            .unwrap()
            .expect("cleared");
        assert!(!snap.agents[0].stale);
        // Unknown uuid: silently none.
        assert!(apply_open_result(&p, "ghost", true).unwrap().is_none());
    }

    #[test]
    fn clear_tab_timeline_wipes_session_scoped_ids() {
        // tab_ids are SESSION-scoped: a recreated session reuses ids, so a
        // stale timeline would order new tabs by dead tabs' commitments.
        let d = tempfile::tempdir().unwrap();
        let p = tmp_paths(d.path());
        apply_touch(&p, 4, 1700).unwrap();
        clear_tab_timeline(&p).unwrap();
        let s = read_store(&p).unwrap();
        assert!(s.tab_timeline.is_empty());
        assert_eq!(s.seq, 2); // content changed ⇒ seq changed (§5 invariant)
        // Idempotent: clearing an empty timeline changes nothing.
        clear_tab_timeline(&p).unwrap();
        assert_eq!(read_store(&p).unwrap().seq, 2);
    }

    #[test]
    fn snapshot_projects_title_summary_and_worktree_from_the_record() {
        // One producer, one consumer (§5): snapshot_from is the only place
        // a record becomes a wire Agent, so this is the whole contract.
        let mut s = Store::default();
        let mut r = rec("u1");
        r.title = Some("CLA-MAIN".into());
        r.summary = "fix the flaky auth".into();
        r.worktree = Some("/x/.claude/worktrees/wt".into());
        s.agents.insert("u1".into(), r);

        let snap = snapshot_from(&s);
        let a = &snap.agents[0];
        assert_eq!(a.title.as_deref(), Some("CLA-MAIN"));
        assert_eq!(a.summary, "fix the flaky auth");
        assert_eq!(a.worktree.as_deref(), Some("/x/.claude/worktrees/wt"));
    }

    #[test]
    fn agent_record_title_and_summary_default_on_pre_field_store_files() {
        // The first run of a new binary reads the EXISTING agents.json, which
        // has neither key. Without #[serde(default)] that is a whole-store
        // parse failure and every agent vanishes — not a blank field.
        let json = serde_json::to_value(rec("u1")).unwrap();
        let mut o = json.as_object().unwrap().clone();
        o.remove("title");
        o.remove("summary");
        let back: AgentRecord = serde_json::from_value(serde_json::Value::Object(o)).unwrap();
        assert_eq!(back.title, None);
        assert!(back.summary.is_empty());
    }

    #[test]
    fn backfill_lifts_the_words_segment_out_of_an_existing_label() {
        // Rows written before `summary` existed carry it only inside `label`.
        // refresh_label returns early forever once label_source == Summary
        // (hook.rs:155), and dormant rows get no hook events at all — so
        // without this they would render a blank 17-column field for good.
        let mut s = Store::default();
        let mut r = rec("u1");
        r.label = "clave \u{00b7} main \u{00b7} fix the flaky auth".into();
        r.summary = String::new();
        s.agents.insert("u1".into(), r);

        assert!(backfill_summaries(&mut s));
        assert_eq!(s.agents["u1"].summary, "fix the flaky auth");
    }

    #[test]
    fn backfill_keeps_a_separator_inside_the_summary_text() {
        // splitn(3) — a summary that itself contains the separator survives
        // whole. A plain split() would truncate it at the first occurrence.
        let mut s = Store::default();
        let mut r = rec("u1");
        r.label = "clave \u{00b7} main \u{00b7} a \u{00b7} b".into();
        r.summary = String::new();
        s.agents.insert("u1".into(), r);

        backfill_summaries(&mut s);
        assert_eq!(s.agents["u1"].summary, "a \u{00b7} b");
    }

    #[test]
    fn backfill_is_idempotent_and_skips_labels_without_a_words_segment() {
        // Self-limiting: it matches only EMPTY summaries, so a second pass
        // changes nothing. Same shape as S1 §3.6's commit_ord backfill.
        let mut s = Store::default();
        let mut earned = rec("u1");
        earned.label = "clave \u{00b7} main \u{00b7} fix the flaky auth".into();
        earned.summary = "already set by S4".into();
        s.agents.insert("u1".into(), earned);
        let mut bare = rec("u2");
        bare.label = "clave \u{00b7} main".into(); // never earned any words
        bare.summary = String::new();
        s.agents.insert("u2".into(), bare);

        assert!(!backfill_summaries(&mut s), "nothing to do");
        assert_eq!(s.agents["u1"].summary, "already set by S4");
        assert!(s.agents["u2"].summary.is_empty());

        // And a real pass must not re-fire on a second run.
        let mut t = Store::default();
        let mut r = rec("u3");
        r.label = "clave \u{00b7} main \u{00b7} words".into();
        r.summary = String::new();
        t.agents.insert("u3".into(), r);
        assert!(backfill_summaries(&mut t));
        assert!(!backfill_summaries(&mut t), "second pass is a no-op");
    }
}
