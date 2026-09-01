//! `clave pr-sync <uuid>` (#232): the PR-number lookup, detached from the
//! hook path.
//!
//! Starship-style discipline (maintainer ruling): an external command that
//! hits a git host must never ride the hook's hot path (§6.5) — every hook
//! event fires for every tool call of every tracked agent, and `gh` can be
//! slow, absent, or rate-limited. So `run_hook` (hook.rs) does ONLY an
//! integer comparison (`pr_is_stale`) at the end of its store write, and
//! when the row is stale it spawns THIS binary's `pr-sync <uuid>` fully
//! detached — the hook returns before `gh` even starts. `pr-sync` is the
//! only place that runs `gh`, under a hard-killed 5s timeout, and it writes
//! a miss down as an answer (`pr_number: None`) so the miss does not
//! retrigger for a full TTL. A miss is an answer.

use crate::store::{self, AgentRecord};

/// How long a resolved (or missed) PR number is trusted before `pr-sync`
/// asks again. 5 minutes: long enough that a fleet of agents does not
/// hammer `gh`, short enough that a freshly opened PR shows up within a
/// coffee sip.
pub const PR_TTL_SECS: u64 = 300;

/// Pure over an injected runner so the parsing/degradation rules are
/// unit-testable without a `gh` binary or the network. `run` returns `None`
/// for "the command produced nothing usable" (missing binary, non-zero
/// exit, timeout) — every one of those degrades to `None` here, never a
/// panic and never surfaced to a caller.
///
/// `repo_root` is not folded into the `gh` invocation itself — the real
/// runner pins the repo via the child process's cwd (`run_pr_sync`) — but an
/// empty `repo_root` means "we don't know where this checkout lives," which
/// is a hard no rather than a `gh` call left to guess.
pub fn resolve_pr(
    run: &dyn Fn(&[&str]) -> Option<String>,
    repo_root: &str,
    branch: &str,
) -> Option<u32> {
    if repo_root.is_empty() || branch.is_empty() {
        return None;
    }
    let out = run(&[
        "pr",
        "list",
        "--head",
        branch,
        "--json",
        "number",
        "--jq",
        ".[0].number",
    ])?;
    out.trim().parse().ok()
}

/// TTL-or-branch-change staleness. A branch switch invalidates the cached
/// number immediately (the cache is now for the wrong question) even inside
/// the TTL window; `pr_checked == 0` (never looked) is always stale.
pub fn pr_is_stale(rec: &AgentRecord, now: u64) -> bool {
    rec.pr_checked == 0
        || now.saturating_sub(rec.pr_checked) > PR_TTL_SECS
        || rec.pr_branch != rec.branch
}

/// Spawn `clave pr-sync <uuid>` fully detached: stdin/stdout/stderr null,
/// `spawn()` and drop the child — the caller (the hook) returns immediately
/// and never learns whether the spawn even succeeded. Resolves the running
/// binary via `current_exe`, never PATH `clave` — the #43/#44 leak class: a
/// pane's PATH is not guaranteed to hold the same `clave` running this hook.
/// A failed resolve or spawn is silent: the cache simply stays stale a bit
/// longer, which is exactly the degradation `pr_is_stale` already promises.
pub fn spawn_pr_sync(uuid: &str) {
    let Ok(exe) = std::env::current_exe() else {
        return;
    };
    let _ = std::process::Command::new(exe)
        .args(["pr-sync", uuid])
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn();
}

/// The checkout `pr-sync` should run `gh` from: the worktree if `clave add
/// --worktree` made one, else the repo root — same choice `add::run_add`'s
/// `agent_cwd` makes for a fresh worktree row.
fn checkout_dir(rec: &AgentRecord) -> String {
    rec.worktree
        .clone()
        .unwrap_or_else(|| rec.repo_root.clone())
}

/// Re-checks staleness UNDER the store lock and hands back exactly what a
/// caller needs to run `gh` (checkout dir, repo root, branch) — `None` means
/// "nothing to do," whether because the uuid is gone or a racing `pr-sync`
/// already answered. Split out from [`run_pr_sync`] so the double-spawn race
/// (two hooks both observing staleness before either writes) is testable
/// without a `gh` binary.
fn pr_sync_target(s: &store::Store, uuid: &str, now: u64) -> Option<(String, String, String)> {
    let rec = s.agents.get(uuid)?;
    pr_is_stale(rec, now).then(|| (checkout_dir(rec), rec.repo_root.clone(), rec.branch.clone()))
}

/// The real `gh` runner: a process-level 5s timeout via a watchdog thread
/// that `kill -9`s the child's pid if `gh` has not finished by then. Kept to
/// the minimum — this is the one surface here that needs a live `gh` to
/// exercise, so it stays untested; everything it calls (`resolve_pr`,
/// `pr_is_stale`) is covered above. A cancel channel (not a bare sleep +
/// kill) keeps `gh` finishing under 5s — success OR error — from leaving a
/// `kill -9` timer armed against a pid the OS may since have reused: the
/// cancel is sent right after `wait_with_output` returns, before either
/// outcome is handled, so no exit path can skip it.
fn gh_runner(cwd: String) -> impl Fn(&[&str]) -> Option<String> {
    move |args: &[&str]| {
        // Discovered path, never bare `gh` (same class as push_snapshot's
        // zellij fix, hook.rs ~610-614): pr-sync runs detached from a hook
        // that inherited Claude's env, whose PATH may lack homebrew. A miss
        // here degrades silently — tool_path falls back to the bare name,
        // which then simply fails to spawn and resolve_pr returns None.
        let child =
            std::process::Command::new(crate::discover::tool_path(crate::discover::ToolId::Gh))
                .args(args)
                .current_dir(&cwd)
                .stdin(std::process::Stdio::null())
                .stdout(std::process::Stdio::piped())
                .stderr(std::process::Stdio::null())
                .spawn()
                .ok()?;
        let pid = child.id();
        let (cancel_tx, cancel_rx) = std::sync::mpsc::channel::<()>();
        let watchdog = std::thread::spawn(move || {
            if cancel_rx
                .recv_timeout(std::time::Duration::from_secs(5))
                .is_err()
            {
                // Absolute path, not PATH `kill`: same off-PATH hazard as
                // `gh` above, and /bin/kill is universal on macOS/Linux so
                // there is no discover() case to make for it.
                let _ = std::process::Command::new("/bin/kill")
                    .args(["-9", &pid.to_string()])
                    .status();
            }
        });
        // Send the cancel BEFORE handling the result, on every exit path: an
        // early `?` on a wait error used to skip straight past the send,
        // leaving the watchdog armed to `kill -9` a pid the OS is free to
        // have already reused (finding 4, #232 final review).
        let output = child.wait_with_output();
        let _ = cancel_tx.send(()); // this pid is free to be reused; stand down
        let _ = watchdog.join();
        let output = output.ok()?;
        output
            .status
            .success()
            .then(|| String::from_utf8_lossy(&output.stdout).trim().to_string())
    }
}

/// `clave pr-sync <uuid>` (main.rs's hidden subcommand): loads the store,
/// re-checks staleness under the lock, and — only if still stale — runs the
/// real `gh` lookup OUTSIDE the lock (§6.5: nothing that shells out or hits
/// the network runs under the store flock) and writes the answer back,
/// including a miss, bumping seq and pushing the snapshot exactly like every
/// other locked writer in this crate. Every failure (unreadable store,
/// vanished uuid, no `gh`) degrades silently — this runs detached from the
/// hook with nobody watching its exit code.
pub fn run_pr_sync(uuid: &str) {
    let Ok(paths) = store::store_paths() else {
        return;
    };
    let now = store::now_unix();
    let target = store::with_store_mut(&paths, |s| pr_sync_target(s, uuid, now))
        .ok()
        .flatten();
    let Some((cwd, repo_root, branch)) = target else {
        return;
    };

    let run = gh_runner(cwd);
    let pr_number = resolve_pr(&run, &repo_root, &branch);

    let snap = store::with_store_mut(&paths, |s| {
        let rec = s.agents.get_mut(uuid)?;
        rec.pr_number = pr_number; // a miss (None) IS the answer — no retrigger for a TTL
        rec.pr_checked = store::now_unix();
        rec.pr_branch = branch.clone();
        s.seq += 1; // monotonic pipe contract (§5)
        Some(store::snapshot_from(s))
    });
    if let Ok(Some(snap)) = snap {
        crate::hook::push_snapshot(&snap);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::{LabelSource, StorePaths};
    use clave_types::Status;
    use std::collections::BTreeMap;

    // Local copy of the Task 2 `rec()` shape (store.rs/hook.rs carry their
    // own): label "x · main", cwd "/x", branch "main", source FirstPrompt.
    fn rec(uuid: &str) -> AgentRecord {
        AgentRecord {
            uuid: uuid.into(),
            cwd: "/x".into(),
            repo_root: "/x".into(),
            branch: "main".into(),
            label: "x · main".into(),
            status: Status::Idle,
            last_interacted: 0,
            commit_ord: 0,
            last_visited: 0,
            worktree: None,
            label_source: LabelSource::FirstPrompt,
            tab_id: None,
            pane_id: None,
            stale: false,
            title: None,
            summary: String::new(),
            default_branch: None,
            context_tokens: None,
            context_level: None,
            live_session: None,
            buckets: BTreeMap::new(),
            model: None,
            provider: None,
            effort: None,
            pr_number: None,
            pr_checked: 0,
            pr_branch: String::new(),
        }
    }

    fn tmp_paths(dir: &std::path::Path) -> StorePaths {
        StorePaths {
            dir: dir.to_path_buf(),
            data: dir.join("agents.json"),
            lock: dir.join("agents.lock"),
        }
    }

    #[test]
    fn resolve_pr_parses_the_number_and_degrades_silently() {
        let hit = |_: &[&str]| Some("204\n".to_string());
        assert_eq!(resolve_pr(&hit, "/r", "drive-launch"), Some(204));
        let empty = |_: &[&str]| Some("".to_string());
        assert_eq!(resolve_pr(&empty, "/r", "drive-launch"), None);
        let dead = |_: &[&str]| None; // gh missing / timed out / non-zero
        assert_eq!(resolve_pr(&dead, "/r", "drive-launch"), None);
        let junk = |_: &[&str]| Some("not a number".to_string());
        assert_eq!(resolve_pr(&junk, "/r", "drive-launch"), None);
    }

    /// Either half of "we don't know what to ask" is a hard no, and neither
    /// reaches the runner: a detached checkout has no branch to head-match, and
    /// an unknown repo root would leave `gh` guessing from whatever cwd it
    /// inherited. The runner panics so the guard cannot pass by returning
    /// `None` for the wrong reason.
    #[test]
    fn resolve_pr_asks_nothing_when_it_does_not_know_what_to_ask() {
        let never = |_: &[&str]| -> Option<String> { panic!("gh must not run") };
        assert_eq!(resolve_pr(&never, "", "drive-launch"), None);
        assert_eq!(resolve_pr(&never, "/r", ""), None);
        assert_eq!(resolve_pr(&never, "", ""), None);
    }

    #[test]
    fn pr_staleness_is_ttl_or_branch_change() {
        let mut r = rec("u"); // store test fixture
        r.branch = "drive-launch".into();
        r.pr_checked = 1000;
        r.pr_branch = "drive-launch".into();
        assert!(!pr_is_stale(&r, 1000 + PR_TTL_SECS - 1));
        // The boundary itself: an answer exactly `PR_TTL_SECS` old is still
        // trusted — the TTL is how long it is good FOR, not the last instant
        // it is good AT.
        assert!(!pr_is_stale(&r, 1000 + PR_TTL_SECS));
        assert!(pr_is_stale(&r, 1000 + PR_TTL_SECS + 1));
        r.pr_branch = "old-branch".into(); // branch moved: cache is for the wrong question
        assert!(pr_is_stale(&r, 1001));
        let fresh = rec("u2"); // pr_checked = 0: never looked
        assert!(pr_is_stale(&fresh, 1));
    }

    /// `pr-sync` runs `gh` from the row's OWN checkout — the worktree when
    /// `clave add --worktree` made one, the repo root otherwise. A lookup from
    /// the wrong directory asks a different repo's question, or none at all,
    /// and the miss it writes back is indistinguishable from "no PR".
    #[test]
    fn pr_sync_targets_the_rows_own_checkout() {
        let d = tempfile::tempdir().unwrap();
        let paths = tmp_paths(d.path());
        let mut plain = rec("plain");
        plain.repo_root = "/repos/clave".into();
        plain.branch = "drive-launch".into();
        let mut wt = rec("wt");
        wt.repo_root = "/repos/clave".into();
        wt.worktree = Some("/repos/clave-wt/drive-launch".into());
        wt.branch = "drive-launch".into();
        store::with_store_mut(&paths, |s| {
            s.agents.insert("plain".into(), plain);
            s.agents.insert("wt".into(), wt);
        })
        .unwrap();

        let target =
            |uuid: &str| store::with_store_mut(&paths, |s| pr_sync_target(s, uuid, 1_000)).unwrap();
        let triple =
            |a: &str, b: &str, c: &str| Some((a.to_string(), b.to_string(), c.to_string()));
        assert_eq!(
            target("plain"),
            triple("/repos/clave", "/repos/clave", "drive-launch"),
            "an ordinary checkout asks from its repo root"
        );
        assert_eq!(
            target("wt"),
            triple(
                "/repos/clave-wt/drive-launch",
                "/repos/clave",
                "drive-launch"
            ),
            "a worktree row asks from the worktree"
        );
        assert_eq!(target("gone"), None, "a vanished uuid is nothing to do");
    }

    /// The double-spawn race the design calls out: two hooks can both
    /// observe a stale row before either has written back (each ran its own
    /// lock-free-ish window before spawning `pr-sync`). The SECOND `pr-sync`
    /// to acquire the store must see the first's answer and back off rather
    /// than issuing a second `gh` call.
    #[test]
    fn a_second_pr_sync_sees_the_firsts_answer_and_backs_off() {
        let d = tempfile::tempdir().unwrap();
        let paths = tmp_paths(d.path());
        let mut r = rec("u");
        r.branch = "drive-launch".into();
        store::with_store_mut(&paths, |s| {
            s.agents.insert("u".into(), r.clone());
        })
        .unwrap();

        // First pr-sync observes staleness (never checked).
        let now = 1_000;
        let first = store::with_store_mut(&paths, |s| pr_sync_target(s, "u", now)).unwrap();
        assert!(first.is_some(), "a never-checked row is stale");

        // It resolves and writes its answer (standing in for the gh round trip).
        store::with_store_mut(&paths, |s| {
            let rec = s.agents.get_mut("u").unwrap();
            rec.pr_number = Some(7);
            rec.pr_checked = now;
            rec.pr_branch = "drive-launch".into();
        })
        .unwrap();

        // A second pr-sync racing the same staleness window re-checks under
        // the lock and finds nothing left to do.
        let second = store::with_store_mut(&paths, |s| pr_sync_target(s, "u", now)).unwrap();
        assert!(
            second.is_none(),
            "the second racer must back off, not re-ask gh"
        );
    }
}
