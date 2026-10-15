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
//! retrigger for a full TTL. A miss is an answer. A failed `gh` is not a
//! miss: it is [`PrLookup::Failed`], and it keeps a number that answers the
//! row's current question.

use crate::store::{self, AgentRecord};

/// How long a resolved (or missed) PR number is trusted before `pr-sync`
/// asks again. 5 minutes: long enough that a fleet of agents does not
/// hammer `gh`, short enough that a freshly opened PR shows up within a
/// coffee sip.
pub const PR_TTL_SECS: u64 = 300;

/// What one `gh` lookup learned. A lookup that failed is its own case, so
/// it cannot read as "no PR".
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PrLookup {
    /// `gh` names an open PR for the branch.
    Open(u32),
    /// `gh` answered cleanly and lists no open PR, or the row has no branch
    /// that a PR could head.
    NoPr,
    /// `gh` could not answer: it is missing, it was killed at 5s, it exited
    /// non-zero, or it printed something that is not a number.
    Failed,
}

/// Pure over an injected runner so the parsing/degradation rules are
/// unit-testable without a `gh` binary or the network. `run` returns `None`
/// for "the command produced nothing usable" (missing binary, non-zero
/// exit, timeout), which is [`PrLookup::Failed`] here, never a panic.
///
/// `repo_root` is not folded into the `gh` invocation itself — the real
/// runner pins the repo via the child process's cwd (`run_pr_sync`) — but an
/// empty `repo_root` means "we don't know where this checkout lives," which
/// is a hard no rather than a `gh` call left to guess.
pub fn resolve_pr(
    run: &dyn Fn(&[&str]) -> Option<String>,
    repo_root: &str,
    branch: &str,
) -> PrLookup {
    // `-` is the host's "asked and could not tell" branch (`add::record_branch`,
    // and `hook::take_checkout` for a session that moved into a detached head).
    // It is a sentinel, not a name: `gh pr list --head -` is a question about a
    // branch nobody has.
    if repo_root.is_empty() || branch.is_empty() || branch == "-" {
        return PrLookup::NoPr;
    }
    let Some(out) = run(&[
        "pr",
        "list",
        "--head",
        branch,
        "--json",
        "number",
        "--jq",
        ".[0].number",
    ]) else {
        return PrLookup::Failed;
    };
    // Measured with gh 2.102.0 (2026-10-14): a branch with no open PR prints
    // one empty line and exits 0. A repo with no GitHub remote exits 1.
    match out.trim() {
        "" => PrLookup::NoPr,
        n => n.parse().map_or(PrLookup::Failed, PrLookup::Open),
    }
}

/// TTL-or-branch-change staleness. A branch switch invalidates the cached
/// number immediately (the cache is now for the wrong question) even inside
/// the TTL window; `pr_checked == 0` (never looked, or the checkout moved)
/// is always stale.
pub fn pr_is_stale(rec: &AgentRecord, now: u64) -> bool {
    !answers_current_question(rec) || now.saturating_sub(rec.pr_checked) > PR_TTL_SECS
}

/// The row's number answers the question the row asks now: it was answered
/// (`pr_checked != 0`) for the branch the row is on. A move zeroes the stamp
/// (`hook::take_checkout`, `store::apply_relocation`), so a number asked from
/// another directory fails this too.
fn answers_current_question(rec: &AgentRecord) -> bool {
    rec.pr_checked != 0 && rec.pr_branch == rec.branch
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

/// The directory `pr-sync` runs `gh` from: the one the session is actually in.
///
/// `gh` resolves the repository from its own working directory, so asking from
/// the row's `cwd` keeps the repository and the branch consistent BY
/// CONSTRUCTION. That is what makes the pairing safe as a session moves, and
/// sessions do move: `hook::take_checkout` follows them, and 76 of 1219
/// transcripts change cwd mid-session (2026-09-15) — 6% of the corpus, and 30%
/// of the conversations over 1 MiB. Asking from a recorded ROOT instead can
/// pair the old repository with the new branch, and a branch name both repos
/// carry then answers with another project's PR number — a wrong number on the
/// card, which is worse than a blank one.
///
/// The worktree and the root remain the fallback for a row whose cwd is not
/// known. A cwd that no longer exists cannot spawn `gh` at all (`gh_runner`
/// ends in `.spawn().ok()?`), so the lookup is [`PrLookup::Failed`]: the row
/// keeps a number asked from this same checkout, and never gains one that
/// belongs to another project.
fn checkout_dir(rec: &AgentRecord) -> String {
    if !rec.cwd.is_empty() {
        return rec.cwd.clone();
    }
    rec.worktree
        .clone()
        .unwrap_or_else(|| rec.repo_root.clone())
}

/// One lookup a `pr-sync` owes: where `gh` runs, and what it asks.
#[derive(Debug, PartialEq, Eq)]
struct PrAsk {
    /// [`checkout_dir`] when the sync started.
    cwd: String,
    repo_root: String,
    branch: String,
}

/// The lookup a stale row owes. `None` means "nothing to do": the uuid is
/// gone, or the row is fresh because another `pr-sync` answered first.
///
/// It writes nothing, so `pr_checked` and `pr_branch` mean "answered for this
/// branch, at this time". A stamp written here would outlive a sync that dies
/// (a closed pane), and make an old branch's number read as the answer for
/// the new one (review of PR #284). Two syncs that start before either
/// answers both ask `gh`, but the second answer is the same, so it pushes
/// nothing ([`apply_pr_answer`]). No lock: writers replace the store by
/// atomic rename (`store::read_store`).
fn pr_sync_target(s: &store::Store, uuid: &str, now: u64) -> Option<PrAsk> {
    let rec = s.agents.get(uuid)?;
    pr_is_stale(rec, now).then(|| PrAsk {
        cwd: checkout_dir(rec),
        repo_root: rec.repo_root.clone(),
        branch: rec.branch.clone(),
    })
}

/// Records `gh`'s answer to `ask`, a miss included, and bumps `seq` only
/// when the bar renders something new. It returns that decision, and the
/// caller pushes on it.
///
/// An answer to a question the row no longer asks is dropped: the branch or
/// the directory moved while `gh` was out. The row keeps what it had, which
/// answers its own question or is stale, so the next hook asks again.
///
/// A failed lookup keeps a number that answers the row's current question
/// ([`answers_current_question`]) and writes only the time, so a failing
/// `gh` waits one TTL as a miss does, and is not asked again on every hook.
/// Before 0.5.6 a flaky network blanked the `#N` cell and brought it back:
/// two pushes, and the one cell that names the PR flickered. Any other number
/// is blanked on a failure, because a wrong number is worse than a blank one
/// ([`checkout_dir`]).
///
/// Of the answer, only the number reaches the wire (`store::snapshot_from`
/// keeps `pr_checked` and `pr_branch` host-side). A gone PR is a change: the
/// card drops the `#N` cell and gives its columns to the branch (card.rs,
/// `pr_extra`). The same number again is not. On the live 0.5.5 fleet, 25 of
/// 27 `pr-sync` pushes in 21 minutes changed nothing on screen (2026-10-14),
/// and each push costs every bar a decode and a render.
///
/// A write with no seq bump is safe, the slim push included, because it
/// changes nothing in the snapshot. The stamps are not on the wire, so the
/// dormant tag cannot move (`store::dormant_tag`). A bar takes only a newer
/// seq (clave-bar model.rs `rebuild`), so it must already hold every rendered
/// field at this seq, and it does. `hook::apply_hook_event` writes
/// `live_session` the same way.
fn apply_pr_answer(
    s: &mut store::Store,
    uuid: &str,
    ask: &PrAsk,
    lookup: PrLookup,
    now: u64,
) -> bool {
    let Some(rec) = s.agents.get_mut(uuid) else {
        return false;
    };
    if rec.branch != ask.branch || checkout_dir(rec) != ask.cwd {
        return false;
    }
    let pr = match lookup {
        PrLookup::Open(n) => Some(n),
        PrLookup::NoPr => None,
        PrLookup::Failed if answers_current_question(rec) => {
            rec.pr_checked = now;
            return false;
        }
        PrLookup::Failed => None,
    };
    rec.pr_checked = now; // a miss is an answer too: no new ask for a TTL
    rec.pr_branch = ask.branch.clone();
    if rec.pr_number == pr {
        return false;
    }
    rec.pr_number = pr;
    s.seq += 1; // monotonic pipe contract (§5)
    true
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
        // which then simply fails to spawn: `PrLookup::Failed`.
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

/// `clave pr-sync <uuid>` (main.rs's hidden subcommand): reads the lookup
/// it owes ([`pr_sync_target`]), runs the real `gh` lookup OUTSIDE the
/// lock (§6.5: nothing that shells out or hits the network runs under the
/// store flock), and writes the answer back ([`apply_pr_answer`]). It pushes
/// only an answer the bar renders differently. Every failure (unreadable
/// store, vanished uuid, no `gh`) degrades silently — this runs detached from
/// the hook with nobody watching its exit code.
pub fn run_pr_sync(uuid: &str) {
    let Ok(paths) = store::store_paths() else {
        return;
    };
    let now = store::now_unix();
    let target = store::read_store(&paths).ok();
    let Some(ask) = target.and_then(|s| pr_sync_target(&s, uuid, now)) else {
        return;
    };

    let run = gh_runner(ask.cwd.clone());
    let lookup = resolve_pr(&run, &ask.repo_root, &ask.branch);

    let snap = store::with_store_mut(&paths, |s| {
        apply_pr_answer(s, uuid, &ask, lookup, store::now_unix()).then(|| store::snapshot_from(s))
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
            standby_stamp: None,
            title: None,
            summary: String::new(),
            default_branch: None,
            context_tokens: None,
            context_level: None,
            live_session: None,
            metered_at: 0,
            buckets: BTreeMap::new(),
            model: None,
            provider: None,
            effort: None,
            pr_number: None,
            pr_checked: 0,
            pr_branch: String::new(),
            wants: None,
            subagents: false,
        }
    }

    fn tmp_paths(dir: &std::path::Path) -> StorePaths {
        StorePaths {
            dir: dir.to_path_buf(),
            data: dir.join("agents.json"),
            lock: dir.join("agents.lock"),
        }
    }

    /// Only a clean empty answer is "no PR". A `gh` that could not answer is
    /// `Failed`, and it must stay apart from `NoPr`: merged, a flaky network
    /// blanks the card's `#N` cell and brings it back.
    #[test]
    fn resolve_pr_parses_the_number_and_degrades_silently() {
        let hit = |_: &[&str]| Some("204\n".to_string());
        assert_eq!(resolve_pr(&hit, "/r", "drive-launch"), PrLookup::Open(204));
        // gh 2.102.0 prints one empty line for a branch with no open PR.
        let empty = |_: &[&str]| Some("\n".to_string());
        assert_eq!(resolve_pr(&empty, "/r", "drive-launch"), PrLookup::NoPr);
        let dead = |_: &[&str]| None; // gh missing / timed out / non-zero
        assert_eq!(resolve_pr(&dead, "/r", "drive-launch"), PrLookup::Failed);
        let junk = |_: &[&str]| Some("not a number".to_string());
        assert_eq!(resolve_pr(&junk, "/r", "drive-launch"), PrLookup::Failed);
    }

    /// Either half of "we don't know what to ask" is a hard no, and neither
    /// reaches the runner: a detached checkout has no branch to head-match, and
    /// an unknown repo root would leave `gh` guessing from whatever cwd it
    /// inherited. The runner panics so the guard cannot pass by returning
    /// `NoPr` for the wrong reason.
    #[test]
    fn resolve_pr_asks_nothing_when_it_does_not_know_what_to_ask() {
        let never = |_: &[&str]| -> Option<String> { panic!("gh must not run") };
        assert_eq!(resolve_pr(&never, "", "drive-launch"), PrLookup::NoPr);
        assert_eq!(resolve_pr(&never, "/r", ""), PrLookup::NoPr);
        assert_eq!(resolve_pr(&never, "", ""), PrLookup::NoPr);
        assert_eq!(resolve_pr(&never, "/r", "-"), PrLookup::NoPr);
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
    fn pr_sync_asks_from_the_directory_the_session_is_in() {
        let d = tempfile::tempdir().unwrap();
        let paths = tmp_paths(d.path());

        // An ordinary checkout: cwd and root are the same place.
        let mut plain = rec("plain");
        plain.cwd = "/repos/clave".into();
        plain.repo_root = "/repos/clave".into();
        plain.branch = "drive-launch".into();

        // A worktree row: the session is in the worktree, and that is where the
        // question belongs.
        let mut wt = rec("wt");
        wt.cwd = "/repos/clave-wt/drive-launch".into();
        wt.repo_root = "/repos/clave".into();
        wt.worktree = Some("/repos/clave-wt/drive-launch".into());
        wt.branch = "drive-launch".into();

        // A row whose session has MOVED. `hook::take_checkout` follows it and
        // deliberately leaves `repo_root` alone, because only git could settle
        // that. So the recorded root is stale — and asking from it would pair
        // the old repository with the new branch, which is how another
        // project's PR number reaches this card.
        let mut moved = rec("moved");
        moved.cwd = "/repos/other".into();
        moved.repo_root = "/repos/clave".into();
        moved.branch = "drive-launch".into();

        // A row that does not know where it is falls back to what it does know.
        let mut unknown = rec("unknown");
        unknown.cwd = String::new();
        unknown.repo_root = "/repos/clave".into();
        unknown.branch = "drive-launch".into();

        store::with_store_mut(&paths, |s| {
            s.agents.insert("plain".into(), plain);
            s.agents.insert("wt".into(), wt);
            s.agents.insert("moved".into(), moved);
            s.agents.insert("unknown".into(), unknown);
        })
        .unwrap();

        let s = store::read_store(&paths).unwrap();
        let target = |uuid: &str| pr_sync_target(&s, uuid, 1_000);
        let asks_from = |uuid: &str| target(uuid).map(|ask| ask.cwd);

        assert_eq!(
            asks_from("plain"),
            Some("/repos/clave".to_string()),
            "an ordinary checkout asks from itself"
        );
        assert_eq!(
            asks_from("wt"),
            Some("/repos/clave-wt/drive-launch".to_string()),
            "a worktree row asks from the worktree it is in"
        );
        assert_eq!(
            asks_from("moved"),
            Some("/repos/other".to_string()),
            "the question follows the session, so `gh` resolves the repository \
             the branch actually belongs to"
        );
        assert_eq!(
            asks_from("unknown"),
            Some("/repos/clave".to_string()),
            "and a row with no cwd still asks from what it knows"
        );
        assert_eq!(target("gone"), None, "a vanished uuid is nothing to do");
    }

    /// The double-spawn race: every hook that fires while `gh` is out sees a
    /// stale row and spawns a `pr-sync`, and each one asks `gh`. That costs a
    /// second call, not a second push: the second answer is the same (two
    /// syncs after one double prompt, live fleet 2026-10-14). A sync that
    /// starts after an answer landed backs off.
    #[test]
    fn racing_syncs_ask_twice_but_push_once() {
        let mut s = store_with(None);
        s.agents.get_mut("u").unwrap().pr_checked = 0; // never asked
        let first = pr_sync_target(&s, "u", 1_000).expect("a never-asked row is stale");
        let second = pr_sync_target(&s, "u", 1_000).expect("and still stale: nothing was written");
        assert!(apply_pr_answer(
            &mut s,
            "u",
            &first,
            PrLookup::Open(7),
            1_001
        ));
        assert!(!apply_pr_answer(
            &mut s,
            "u",
            &second,
            PrLookup::Open(7),
            1_002
        ));
        assert_eq!(s.seq, 11, "two answers, one push");
        assert_eq!(
            pr_sync_target(&s, "u", 1_003),
            None,
            "a later sync backs off"
        );
    }

    /// A store with one row on `drive-launch` at `/x`, holding `pr`, at seq
    /// 10. Its cache holds the answer the last sync wrote at 1_000.
    fn store_with(pr: Option<u32>) -> store::Store {
        let mut s = store::Store::default();
        let mut r = rec("u");
        r.branch = "drive-launch".into();
        r.pr_number = pr;
        r.pr_checked = 1_000;
        r.pr_branch = "drive-launch".into();
        s.agents.insert("u".into(), r);
        s.seq = 10;
        s
    }

    /// The lookup a sync started on `branch`, from the row's `/x`.
    fn ask_for(branch: &str) -> PrAsk {
        PrAsk {
            cwd: "/x".into(),
            repo_root: "/x".into(),
            branch: branch.into(),
        }
    }

    fn ask() -> PrAsk {
        ask_for("drive-launch")
    }

    /// 25 of 27 `pr-sync` pushes on the live fleet changed nothing on screen
    /// (2026-10-14). The same answer again must not bump seq, so the caller
    /// does not push. The stamps still land, or the next hook asks `gh` again.
    #[test]
    fn the_same_answer_again_moves_no_seq_but_still_stamps_the_cache() {
        for (lookup, pr) in [(PrLookup::Open(232), Some(232)), (PrLookup::NoPr, None)] {
            let mut s = store_with(pr);
            let moved = apply_pr_answer(&mut s, "u", &ask(), lookup, 5_000);
            assert!(!moved, "{pr:?} again renders nothing new");
            assert_eq!(s.seq, 10, "{pr:?} again must not bump seq");
            let r = &s.agents["u"];
            assert_eq!(r.pr_number, pr);
            assert_eq!(
                (r.pr_checked, r.pr_branch.as_str()),
                (5_000, "drive-launch")
            );
            assert!(!pr_is_stale(r, 5_001), "the answer holds for a TTL");
        }
    }

    #[test]
    fn a_new_pr_number_bumps_seq_once() {
        let mut s = store_with(None);
        let answer =
            |s: &mut store::Store, n| apply_pr_answer(s, "u", &ask(), PrLookup::Open(n), 5_000);
        assert!(answer(&mut s, 281));
        assert_eq!(s.seq, 11, "a new number is one store write");
        assert_eq!(s.agents["u"].pr_number, Some(281));
        assert!(!answer(&mut s, 281));
        assert_eq!(s.seq, 11, "and finding it again is none");
        assert!(answer(&mut s, 282));
        assert_eq!(s.seq, 12, "another number is another write");
    }

    /// The first answer after a branch switch is the answer FOR the new
    /// branch. Without the branch written down, the row stays stale and every
    /// hook asks `gh` again.
    #[test]
    fn the_first_answer_after_a_branch_switch_is_recorded_for_the_new_branch() {
        let mut s = store_with(Some(5));
        s.agents.get_mut("u").unwrap().pr_branch = "old-branch".into();
        assert!(
            pr_is_stale(&s.agents["u"], 1_001),
            "a switch is stale at once"
        );
        assert!(apply_pr_answer(
            &mut s,
            "u",
            &ask(),
            PrLookup::Open(9),
            1_002
        ));
        let r = &s.agents["u"];
        assert_eq!(
            (r.pr_number, r.pr_branch.as_str()),
            (Some(9), "drive-launch")
        );
        assert!(!pr_is_stale(r, 1_003), "the new branch's answer holds");
    }

    /// A PR that went away (merged or closed: `gh pr list` lists open PRs)
    /// changes the card. It drops the `#N` cell and gives its columns to the
    /// branch (card.rs, `pr_extra`), so the bar must hear about it.
    #[test]
    fn a_pr_that_went_away_blanks_the_cell_and_bumps_seq() {
        let mut s = store_with(Some(232));
        assert!(apply_pr_answer(&mut s, "u", &ask(), PrLookup::NoPr, 5_000));
        assert_eq!(s.seq, 11);
        assert_eq!(s.agents["u"].pr_number, None);
    }

    /// A flaky network must not blank the cell and bring it back. A failed
    /// `gh` on a row with a known PR keeps the number and bumps nothing. It
    /// writes the time, so the row waits one TTL as a miss does, and a
    /// failing `gh` is not asked again on every hook.
    #[test]
    fn a_failed_gh_keeps_a_known_pr_and_moves_no_seq() {
        let mut s = store_with(Some(232));
        assert!(!apply_pr_answer(
            &mut s,
            "u",
            &ask(),
            PrLookup::Failed,
            5_000
        ));
        assert_eq!(s.seq, 10, "nothing on screen changed");
        let r = &s.agents["u"];
        assert_eq!(r.pr_number, Some(232), "the number stands");
        assert_eq!(r.pr_checked, 5_000, "and waits a TTL from now");
        assert!(!pr_is_stale(r, 5_000 + PR_TTL_SECS));
    }

    /// But a number asked for another branch, or from another directory,
    /// does not answer this question. A failed `gh` blanks it, because a
    /// wrong number is worse than a blank one. This is also a sync that died
    /// after a branch switch: it wrote nothing, so the next one still sees
    /// the old branch's number as the old branch's.
    #[test]
    fn a_failed_gh_blanks_a_number_asked_for_another_question() {
        let mut s = store_with(Some(100));
        s.agents.get_mut("u").unwrap().pr_branch = "feat-a".into();
        let died = pr_sync_target(&s, "u", 1_010).expect("a switch is stale");
        assert_eq!(died, ask(), "the sync asks about the branch the row is on");
        assert!(
            pr_is_stale(&s.agents["u"], 1_011),
            "a sync that died left it stale"
        );
        assert!(apply_pr_answer(
            &mut s,
            "u",
            &ask(),
            PrLookup::Failed,
            1_311
        ));
        assert_eq!((s.seq, s.agents["u"].pr_number), (11, None));

        let mut s = store_with(Some(42));
        s.agents.get_mut("u").unwrap().pr_checked = 0; // the checkout moved
        assert!(apply_pr_answer(
            &mut s,
            "u",
            &ask(),
            PrLookup::Failed,
            1_311
        ));
        assert_eq!((s.seq, s.agents["u"].pr_number), (11, None));

        let mut s = store_with(None);
        s.agents.get_mut("u").unwrap().pr_checked = 0;
        assert!(!apply_pr_answer(
            &mut s,
            "u",
            &ask(),
            PrLookup::Failed,
            1_311
        ));
        assert_eq!(s.seq, 10, "a blank cell stays blank: no push");
        assert_eq!(
            s.agents["u"].pr_checked, 1_311,
            "and a failing gh waits a TTL"
        );
    }

    /// An answer that lands after the row moved answers a question the row no
    /// longer asks: another branch, or another directory. It writes nothing,
    /// and the row stays stale, so the next hook asks the right question.
    #[test]
    fn a_late_answer_for_an_old_question_writes_nothing() {
        let late_branch = ask_for("feat-a");
        let mut late_dir = ask();
        late_dir.cwd = "/old".into();
        for (late, what) in [(late_branch, "branch"), (late_dir, "directory")] {
            let mut s = store_with(Some(9));
            s.agents.get_mut("u").unwrap().pr_checked = 0; // the move made it stale
            let before = s.agents["u"].clone();
            assert!(!apply_pr_answer(
                &mut s,
                "u",
                &late,
                PrLookup::Open(5),
                1_304
            ));
            assert_eq!(s.seq, 10, "an old {what}'s answer pushes nothing");
            let r = &s.agents["u"];
            assert_eq!((r.pr_number, r.pr_checked), (before.pr_number, 0), "{what}");
            assert!(
                pr_is_stale(r, 1_305),
                "the row still owes a lookup ({what})"
            );
        }
    }

    /// A resume that relocates the row to another directory zeroes the stamp,
    /// as a `cd` does (`hook::take_checkout`). Without it, a failing `gh`
    /// keeps the old repo's number beside the new repo (review of PR #284,
    /// case S2).
    #[test]
    fn a_relocation_makes_the_pr_cache_stale() {
        let d = tempfile::tempdir().unwrap();
        let paths = tmp_paths(d.path());
        let now = store::now_unix();
        store::with_store_mut(&paths, |s| {
            let mut r = rec("u");
            (r.cwd, r.branch, r.pr_number) = ("/repo-x".into(), "fix".into(), Some(42));
            (r.pr_checked, r.pr_branch) = (now, "fix".into());
            s.agents.insert("u".into(), r);
        })
        .unwrap();
        store::apply_relocation(&paths, "u", "/repo-x", None).unwrap();
        let r = &store::read_store(&paths).unwrap().agents["u"];
        assert!(
            !pr_is_stale(r, now),
            "a relocation in place keeps the answer"
        );
        store::apply_relocation(&paths, "u", "/repo-y", None).unwrap();
        let s = store::read_store(&paths).unwrap();
        assert!(
            pr_is_stale(&s.agents["u"], now),
            "a new directory asks again"
        );
        let ask = pr_sync_target(&s, "u", now).unwrap();
        let mut s = s;
        assert!(apply_pr_answer(&mut s, "u", &ask, PrLookup::Failed, now));
        assert_eq!(s.agents["u"].pr_number, None, "and a failure blanks #42");
    }

    #[test]
    fn an_answer_for_a_vanished_row_writes_nothing() {
        let mut s = store_with(None);
        assert!(!apply_pr_answer(
            &mut s,
            "gone",
            &ask(),
            PrLookup::Open(7),
            5_000
        ));
        assert_eq!(s.seq, 10);
        assert!(!s.agents.contains_key("gone"));
    }
}
