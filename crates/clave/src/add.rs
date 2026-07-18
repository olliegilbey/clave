//! `clave add` (§6.3): pick a directory, then new-or-resume an agent in a new
//! tab. The INTERACTIVE weave (fzf) lives in run_add; everything decidable
//! is a pure function above it so it can be unit-tested.

use std::io::Write as _;
use std::process::{Command, Stdio};

use anyhow::{Context, Result};

use crate::hook::push_snapshot;
// Only `wasm_path` is consumed here (the layout needs the bar's wasm location);
// `data_dir` from the same module is unused, so it is intentionally NOT
// imported — an unused import would be a warning (see task-7 adaptation note).
use crate::setup::wasm_path;
use crate::store::{
    AgentRecord, LabelSource, Store, now_unix, snapshot_from, store_paths, with_store_mut,
};

/// Parse `zellij action dump-layout` for live agent uuids (§6.3 liveness).
/// Zellij serializes the LIVE pane process, not the baked layout command
/// (C7 finding): after `clave spawn` execs, the pane serializes as
/// `claude --session-id <uuid> …` or `claude --resume <uuid>`. The baked
/// `clave` + `"spawn" "<uuid>"` form is matched too (pre-exec window, and
/// zellij's fallback when the live read fails).
pub fn live_uuids(dump_layout: &str) -> Vec<String> {
    let mut out = Vec::new();
    for line in dump_layout.lines() {
        let Some(rest) = line.trim().strip_prefix("args ") else {
            continue;
        };
        // Tokenize the quoted args: "spawn" "<uuid>" "--name" …
        let tokens: Vec<&str> = rest
            .split('"')
            .enumerate()
            .filter(|(i, _)| i % 2 == 1) // odd indices are inside quotes
            .map(|(_, t)| t)
            .collect();
        match tokens.as_slice() {
            ["spawn", uuid, ..] => out.push((*uuid).to_string()),
            _ => {
                if let Some(uuid) = tokens
                    .windows(2)
                    .find(|w| w[0] == "--session-id" || w[0] == "--resume")
                    .map(|w| w[1])
                {
                    out.push(uuid.to_string());
                }
            }
        }
    }
    out
}

/// Labels get interpolated into KDL string literals and fzf menu lines —
/// strip the two things that break them (quotes, control chars).
pub fn sanitize_label(s: &str) -> String {
    s.chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .filter(|c| *c != '"')
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

/// The agent-tab KDL node WITH its own bar pane — for one-shot
/// `zellij action new-tab --layout` files ONLY, which do NOT pass through
/// the session's default_tab_template. A layout that HAS the template must
/// use `tab_node_bare` instead: zellij wraps explicit tab nodes with the
/// template too, so a bar-carrying node there renders a DOUBLE bar (live
/// finding, c8-cold-start 2026-07-18 — the eager tab loaded two plugin
/// instances in the same second and broke executor election).
pub fn tab_node(wasm: &str, label: &str, uuid: &str, cwd: &str) -> String {
    // split_direction="vertical" is REQUIRED for a LEFT bar: zellij stacks
    // sibling panes horizontally (rows) by default (Task 9 C1 finding; same
    // wrapper as setup::layout_kdl and the S2 spike layout).
    format!(
        r#"    tab name="{label}" focus=true {{
        pane split_direction="vertical" {{
            pane size=30 borderless=true {{
                plugin location="file:{wasm}"
            }}
            pane cwd="{cwd}" command="clave" {{
                args "spawn" "{uuid}" "--name" "{label}" "--cwd" "{cwd}"
            }}
        }}
    }}
"#
    )
}

/// The bar-LESS agent-tab node for layouts that carry
/// default_tab_template (the §6.8 launch layout): the template supplies
/// the bar + vertical split, and this node's pane fills its `children`
/// slot. Same baked idempotent spawn — only the bar pane differs.
pub fn tab_node_bare(label: &str, uuid: &str, cwd: &str) -> String {
    format!(
        r#"    tab name="{label}" focus=true {{
        pane cwd="{cwd}" command="clave" {{
            args "spawn" "{uuid}" "--name" "{label}" "--cwd" "{cwd}"
        }}
    }}
"#
    )
}

/// The one-shot temp layout (§6.3): Zellij KDL has no variable substitution,
/// so the uuid/label/cwd are baked in, the file is passed to
/// `zellij action new-tab --layout`, then deleted. Baking the command in
/// makes tab creation IDEMPOTENT — resurrection is clave's job, not
/// zellij's (§6.8, C8 redesign 2026-07-17).
pub fn tab_layout(wasm: &str, label: &str, uuid: &str, cwd: &str) -> String {
    format!("layout {{\n{}}}\n", tab_node(wasm, label, uuid, cwd))
}

pub struct ResumeCandidate {
    pub uuid: String,
    pub label: String,
    /// Currently on screen (uuid found in dump-layout): picking it JUMPS to
    /// its tab — resuming a live session duplicates it (C7, round 7).
    pub live: bool,
}

/// §6.3 resume picker input: this repo's store rows + jsonl-discovered
/// sessions (`jsonl_stems` = (uuid, mtime) from listing the munged project
/// dir). Live agents are included but MARKED (§6.3 revised 2026-07-14:
/// many agents per repo; live = jump, dead = resume). Recency (mtime)
/// first; store labels beat bare uuids.
pub fn resume_candidates(
    store: &Store,
    repo_root: &str,
    jsonl_stems: &[(String, u64)],
    live: &[String],
) -> Vec<ResumeCandidate> {
    let mut by_uuid: std::collections::BTreeMap<String, (u64, Option<String>)> = Default::default();
    for (uuid, mtime) in jsonl_stems {
        by_uuid.insert(uuid.clone(), (*mtime, None));
    }
    for r in store.agents.values().filter(|r| r.repo_root == repo_root) {
        let e = by_uuid
            .entry(r.uuid.clone())
            .or_insert((r.last_interacted, None));
        e.1 = Some(r.label.clone());
    }
    let mut list: Vec<(u64, ResumeCandidate)> = by_uuid
        .into_iter()
        .map(|(uuid, (mtime, label))| {
            let label = label.unwrap_or_else(|| uuid.clone());
            let live = live.contains(&uuid);
            (mtime, ResumeCandidate { uuid, label, live })
        })
        .collect();
    list.sort_by(|a, b| b.0.cmp(&a.0));
    list.into_iter().map(|(_, c)| c).collect()
}

/// What to store when (re)recording an agent (plan-review fix to §6.3 step 7).
///
/// Resuming an agent that already has a store row must NOT clobber it: the
/// row's `cwd` was canonicalized at creation and is WORKTREE-AWARE — replacing
/// it with the picked dir would silently relocate a worktree agent to the repo
/// root, bake the wrong `--cwd` into the tab layout, and make `clave spawn`
/// munge the wrong project dir: it would miss the worktree-keyed jsonl and
/// CREATE a fresh session (hard `--session-id` collision risk) instead of
/// resuming. The row also carries earned state (label/label_source from
/// hook.rs, last_visited/last_interacted) that a re-add has no business
/// resetting. Re-adding a resumed agent only means "it is on screen again" —
/// so status resets to Idle and EVERYTHING else is preserved.
///
/// No existing row (a jsonl-only candidate — discovered on disk, never
/// tracked): no better info exists, store the fresh record as-is.
pub fn merge_resume_record(existing: Option<&AgentRecord>, fresh: AgentRecord) -> AgentRecord {
    match existing {
        Some(row) => AgentRecord {
            status: clave_types::Status::Idle,
            // The resume opens a brand-new tab: the old bind is stale by
            // definition. The new tab's bar re-binds on join (§6.6 B).
            tab_id: None,
            ..row.clone()
        },
        None => fresh,
    }
}

// ── interactive weave ───────────────────────────────────────────────────────
// Every subprocess below is commented with *why*. None of this is unit-tested
// (it needs fzf + a live zellij + a TTY); Task 9 validates it live.

/// Run `fzf` over `lines`, return the picked line. fzf opens /dev/tty itself,
/// so this works inside the Alt+a floating pane; stdin carries the menu,
/// stdout the choice. None = user aborted (Esc) → caller exits quietly.
fn fzf_pick(lines: &[String], prompt: &str) -> Result<Option<String>> {
    let mut child = Command::new("fzf")
        .args(["--prompt", prompt, "--height", "100%"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .context("launching fzf (is it installed?)")?;
    child
        .stdin
        .take()
        .context("fzf stdin")?
        .write_all(lines.join("\n").as_bytes())?;
    let out = child.wait_with_output()?;
    if !out.status.success() {
        return Ok(None); // Esc/ctrl-c in fzf — a normal abort, not an error
    }
    let picked = String::from_utf8_lossy(&out.stdout).trim().to_string();
    Ok((!picked.is_empty()).then_some(picked))
}

fn cmd_stdout(cmd: &str, args: &[&str]) -> Result<String> {
    let out = Command::new(cmd)
        .args(args)
        .output()
        .with_context(|| format!("running {cmd}"))?;
    anyhow::ensure!(out.status.success(), "{cmd} {args:?} failed");
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

pub fn run_add(worktree: bool) -> Result<()> {
    // 1) Pick a directory: fzf over zoxide's ranked list, current dir first
    //    (§6.3 — fzf+zoxide are verified present on the target machine).
    let cwd = std::env::current_dir()?.to_string_lossy().into_owned();
    let mut dirs: Vec<String> = vec![cwd.clone()];
    dirs.extend(
        cmd_stdout("zoxide", &["query", "-l"])?
            .lines()
            .map(String::from),
    );
    dirs.dedup();
    let Some(dir) = fzf_pick(&dirs, "agent dir> ")? else {
        return Ok(());
    };

    // 2) Canonicalize FIRST (S0b) — everything downstream keys off the
    //    physical path: repo_root, munged jsonl dir, the spawn command.
    let physical = std::fs::canonicalize(&dir).with_context(|| format!("canonicalizing {dir}"))?;
    let physical_str = physical.to_str().context("non-UTF8 dir")?.to_string();
    let repo_root = cmd_stdout(
        "git",
        &["-C", &physical_str, "rev-parse", "--show-toplevel"],
    )
    .map(|s| s.trim().to_string())
    .unwrap_or_else(|_| physical_str.clone()); // non-repo dirs are fine
    let branch = cmd_stdout(
        "git",
        &["-C", &physical_str, "rev-parse", "--abbrev-ref", "HEAD"],
    )
    .map(|s| s.trim().to_string())
    .unwrap_or_else(|_| "-".to_string());

    // 3) Liveness input (§6.3 revised 2026-07-14: MANY agents per repo, so
    //    no auto-jump here — live agents surface as jump entries in the
    //    resume picker instead; the old first-live-agent jump made a second
    //    agent in the same repo impossible).
    let dump = cmd_stdout("zellij", &["action", "dump-layout"]).unwrap_or_default();
    let live = live_uuids(&dump);
    let paths = store_paths()?;
    let store = crate::store::read_store(&paths)?;

    // 4) new vs resume.
    let Some(choice) = fzf_pick(&["new".into(), "resume".into()], "agent> ")? else {
        return Ok(());
    };
    let (uuid, worktree_path, existing) = if choice == "resume" {
        // clave owns the picker (§6.3 — claude --resume's own picker would
        // break resurrection). Candidates: store rows + jsonl scan.
        let proj_dir = crate::env::claude_config_dir()?
            .join("projects")
            .join(crate::munge::munge_cwd(&physical_str));
        let mut stems: Vec<(String, u64)> = Vec::new();
        if let Ok(rd) = std::fs::read_dir(&proj_dir) {
            for e in rd.flatten() {
                let p = e.path();
                if p.extension().is_some_and(|x| x == "jsonl") {
                    let stem = p
                        .file_stem()
                        .unwrap_or_default()
                        .to_string_lossy()
                        .into_owned();
                    let mtime = e
                        .metadata()
                        .and_then(|m| m.modified())
                        .ok()
                        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                        .map(|d| d.as_secs())
                        .unwrap_or(0);
                    stems.push((stem, mtime));
                }
            }
        }
        let candidates = resume_candidates(&store, &repo_root, &stems, &live);
        anyhow::ensure!(
            !candidates.is_empty(),
            "no resumable sessions for {repo_root}"
        );
        let lines: Vec<String> = candidates
            .iter()
            // label shown (live agents marked), uuid carried in the line.
            .map(|c| {
                let marker = if c.live { "▶ " } else { "  " };
                format!("{marker}{}\t{}", c.label, c.uuid)
            })
            .collect();
        let Some(picked) = fzf_pick(&lines, "resume> ")? else {
            return Ok(());
        };
        let uuid = picked
            .rsplit('\t')
            .next()
            .context("picker line")?
            .to_string();
        // A LIVE pick is a JUMP, not a resume — resuming an on-screen
        // session opens it twice (C7, round 7). clave-nav via the CLI pipe:
        // works (S2), and `add` runs INSIDE the session so the env targets
        // the right zellij.
        if candidates.iter().any(|c| c.uuid == uuid && c.live) {
            let payload = format!("{{\"uuid\":\"{uuid}\"}}");
            let _ = Command::new("zellij")
                .args(["pipe", "--name", "clave-nav", "--", &payload])
                .status();
            return Ok(());
        }
        // The lock-free store copy is fine for DERIVING the tab's cwd/label
        // (worst case one beat stale); the AUTHORITATIVE update-or-insert
        // happens under the lock in step 7.
        let existing = store.agents.get(&uuid).cloned();
        (uuid, None, existing)
    } else {
        let uuid = uuid::Uuid::new_v4().to_string();
        // Worktree opt-in (§6.3): clave shells out itself (never claude -w)
        // so it OWNS the path — needed for the munged jsonl existence check
        // and the store record.
        let wt = if worktree {
            let short = &uuid[..8];
            let path = format!("{repo_root}/.claude-worktrees/{short}");
            cmd_stdout(
                "git",
                &[
                    "-C",
                    &repo_root,
                    "worktree",
                    "add",
                    "-b",
                    &format!("clave/{short}"),
                    &path,
                ],
            )?;
            Some(path)
        } else {
            None
        };
        (uuid, wt, None)
    };

    // 5) The agent's cwd for the TAB LAYOUT:
    //    - resume WITH a store row → the ROW's cwd. It was canonicalized when
    //      stored and is worktree-aware; recomputing from the picked dir would
    //      bake the wrong `--cwd` for a worktree agent and break spawn's
    //      jsonl-keyed resume (see merge_resume_record).
    //    - fresh worktree → canonicalize AGAIN (it's brand new — S0b applies).
    //    - else → the picked dir (already canonical from step 2).
    let agent_cwd = match (&existing, &worktree_path) {
        (Some(row), _) => row.cwd.clone(),
        (None, Some(w)) => std::fs::canonicalize(w)?
            .to_str()
            .context("wt path")?
            .to_string(),
        (None, None) => physical_str.clone(),
    };
    // The tab label: an existing row's label is real, possibly already the
    // earned `dir · branch · words` from hook.rs — reopening the tab must not
    // regress it to the base form. Sanitize at interpolation time (the stored
    // label is preserved verbatim; only the KDL string needs to be safe).
    let label = match &existing {
        Some(row) => sanitize_label(&row.label),
        None => {
            let dir_name = agent_cwd.rsplit('/').next().unwrap_or(&agent_cwd);
            // CROSS-TASK COUPLING (Task 5): a NEW agent's label MUST be
            // exactly `<dir_name> · <branch>` (space-middot-space) —
            // hook.rs::refresh_label reconstructs this prefix byte-for-byte
            // to gate the first-prompt upgrade.
            sanitize_label(&format!("{dir_name} · {branch}"))
        }
    };

    // 6) One-shot temp layout → new tab (§6.3). $TMPDIR, deleted after.
    let wasm = wasm_path()?.to_str().context("wasm path")?.to_string();
    let layout = tab_layout(&wasm, &label, &uuid, &agent_cwd);
    let tmp = std::env::temp_dir().join(format!("clave-{uuid}.kdl"));
    std::fs::write(&tmp, layout)?;
    let status = Command::new("zellij")
        .args([
            "action",
            "new-tab",
            "--layout",
            tmp.to_str().context("tmp path")?,
        ])
        .status()?;
    let _ = std::fs::remove_file(&tmp);
    anyhow::ensure!(status.success(), "zellij action new-tab failed");

    // 7) Record + push (§6.3): the row exists BEFORE the first hook event so
    //    the hook's untracked fast path doesn't drop this agent's events.
    //    UPDATE-OR-INSERT, not blind insert (plan-review fix): resuming an
    //    agent with an existing row must preserve it — merge_resume_record
    //    keeps everything and resets only status. The authoritative
    //    existing-row lookup happens HERE, inside the lock (the step-4 copy
    //    was lock-free and only derived layout inputs).
    let snap = with_store_mut(&paths, |s| {
        let fresh = AgentRecord {
            uuid: uuid.clone(),
            cwd: agent_cwd.clone(),
            repo_root: repo_root.clone(),
            branch: branch.clone(),
            label: label.clone(),
            status: clave_types::Status::Idle,
            last_interacted: now_unix(),
            last_visited: 0,
            worktree: worktree_path.clone(),
            label_source: LabelSource::FirstPrompt,
            tab_id: None,
            stale: false,
        };
        let merged = merge_resume_record(s.agents.get(&uuid), fresh);
        s.agents.insert(uuid.clone(), merged);
        s.seq += 1;
        snapshot_from(s)
    })?;
    push_snapshot(&snap);
    crate::evlog::log_event("add", &format!("{uuid}: recorded ({choice})"));
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use clave_types::Status;

    // Local copy of the Task 2 `rec()` shape (see store.rs / hook.rs): the
    // pre-labelled starting record. Tests override the fields they care about.
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
        }
    }

    #[test]
    fn live_uuids_finds_baked_spawn_commands() {
        // Shape of `zellij action dump-layout` output for an agent pane —
        // the args are quoted tokens after "spawn".
        let dump = r#"
            tab name="x · main" {
                pane command="clave" {
                    args "spawn" "3f2a-uuid-1" "--name" "x · main" "--cwd" "/x"
                }
            }
            tab name="plain" { pane }
            tab name="y" {
                pane command="clave" {
                    args "spawn" "9b1c-uuid-2" "--name" "y · dev" "--cwd" "/y"
                }
            }
        "#;
        assert_eq!(live_uuids(dump), vec!["3f2a-uuid-1", "9b1c-uuid-2"]);
        assert!(live_uuids("layout { tab { pane } }").is_empty());
        // Zellij serializes the LIVE pane process, not the baked layout
        // (C7 live finding): after `clave spawn` execs, the pane reads
        // `claude --session-id <uuid> …` (create) or `claude --resume
        // <uuid>` (resume). Both must count as live.
        let dump = r#"
            layout {
                tab name="a" {
                    pane command="claude" {
                        args "--session-id" "exec-uuid-1" "--name" "x · main"
                    }
                }
                tab name="b" {
                    pane command="claude" {
                        args "--resume" "exec-uuid-2"
                    }
                }
                tab name="c" {
                    pane command="<defunct>" {
                        start_suspended true
                    }
                }
            }
        "#;
        assert_eq!(live_uuids(dump), vec!["exec-uuid-1", "exec-uuid-2"]);
    }

    #[test]
    fn tab_layout_bakes_the_idempotent_spawn() {
        let kdl = tab_layout("/data/clave-bar.wasm", "x · main", "u-1", "/x");
        // The bar pane, the baked spawn (idempotent resurrection, §6.3/S4),
        // and the cwd all present:
        assert!(kdl.contains("location=\"file:/data/clave-bar.wasm\""));
        assert!(kdl.contains("\"spawn\" \"u-1\""));
        assert!(kdl.contains("cwd=\"/x\""));
        assert!(kdl.contains("name=\"x · main\""));
        // Regression (Task 9 C1): the bar must be a LEFT column, not a top strip.
        assert!(kdl.contains("split_direction=\"vertical\""));
    }

    #[test]
    fn sanitize_label_strips_kdl_breakers() {
        assert_eq!(sanitize_label("fix \"auth\"\nflow"), "fix auth flow");
    }

    #[test]
    fn merge_resume_preserves_existing_row_and_resets_status() {
        // The resume-clobber defect (plan-review fix): an existing WORKTREE
        // row must survive a resume untouched except status → Idle. Blindly
        // storing the fresh record would relocate the agent to the picked
        // dir, so spawn's munged jsonl check would miss the worktree-keyed
        // transcript and CREATE a colliding session instead of resuming.
        let mut row = rec("u-wt");
        row.cwd = "/repo/.claude-worktrees/abc12345".into();
        row.worktree = Some("/repo/.claude-worktrees/abc12345".into());
        row.repo_root = "/repo".into();
        row.branch = "clave/abc12345".into();
        row.label = "abc12345 · clave/abc12345 · fix auth".into();
        row.label_source = LabelSource::Summary;
        row.status = Status::Working; // stale — the pane is gone
        row.last_interacted = 77;
        row.last_visited = 42;
        row.tab_id = Some(3); // the DEAD tab that hosted it last time
        let fresh = rec("u-wt"); // what the weave derives from the PICKED dir
        let merged = merge_resume_record(Some(&row), fresh.clone());
        assert_eq!(merged.status, Status::Idle);
        // The resumed agent lands in a brand-new tab: the old bind is stale
        // by definition — reset, the new tab's bar re-binds on join (§6.6 B).
        assert_eq!(merged.tab_id, None);
        assert_eq!(merged.cwd, row.cwd); // worktree cwd NOT relocated
        assert_eq!(merged.worktree, row.worktree);
        assert_eq!(merged.label, row.label); // earned label survives
        assert_eq!(merged.label_source, LabelSource::Summary);
        assert_eq!(merged.last_interacted, 77);
        assert_eq!(merged.last_visited, 42);
        // No store row (jsonl-only candidate): no better info exists — the
        // fresh record is stored as-is.
        assert_eq!(merge_resume_record(None, fresh.clone()), fresh);
    }

    #[test]
    fn resume_candidates_mark_live_and_sort_by_mtime() {
        // §6.3 revised (C7, 2026-07-14): many agents per repo. Live agents
        // are NOT excluded — they appear MARKED so picking one JUMPS to its
        // tab instead of duplicating the session (the round-7 defect: a live
        // session resumed twice).
        let mut s = Store::default();
        let mut r = rec("u-live");
        r.repo_root = "/repo".into();
        r.last_interacted = 300;
        s.agents.insert("u-live".into(), r);
        let mut r2 = rec("u-old");
        r2.repo_root = "/repo".into();
        r2.label = "repo · main · old thing".into();
        s.agents.insert("u-old".into(), r2);
        let jsonls = vec![
            ("u-old".to_string(), 100u64),
            ("u-disk".to_string(), 200u64),
        ];
        let live = vec!["u-live".to_string()];
        let c = resume_candidates(&s, "/repo", &jsonls, &live);
        // mtime/interacted desc: u-live (300) > u-disk (200) > u-old (100);
        // store label wins over the bare uuid when we have one.
        assert_eq!(c.len(), 3);
        assert_eq!(c[0].uuid, "u-live");
        assert!(c[0].live);
        assert_eq!(c[1].uuid, "u-disk");
        assert!(!c[1].live);
        assert_eq!(c[2].uuid, "u-old");
        assert_eq!(c[2].label, "repo · main · old thing");
        assert!(!c[2].live);
    }
}
