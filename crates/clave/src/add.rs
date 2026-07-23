//! `clave add` (§6.3): pick a directory, then new-or-resume an agent in a new
//! tab. The INTERACTIVE weave (fzf) lives in run_add; everything decidable
//! is a pure function above it so it can be unit-tested.

use std::io::Write as _;
use std::path::Path;
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

/// §6.3 liveness (issue #6): the uuids the STORE currently binds to a live
/// tab. The bind is set by the agent tab's own bar via a pane-id join (S2) and
/// PRUNED when the tab closes (§6.6 / `clave prune-tabs`), so a Some tab_id
/// tracks the live tab SET — the "join binds against the live tab set" the
/// issue asks for, materialized in the store. This is authoritative because
/// serialized commands go BLIND under an MCP server: zellij serializes the
/// pane's CHILD process (`uv … run main.py`), not `claude` (C7 corollary,
/// 2026-07-21). `live_uuids` survives only as an ADDITIVE fallback for a
/// non-MCP agent whose bind hasn't landed — it can ADD a uuid, never mask one,
/// so it can't reintroduce the double-attach the bind-based signal fixes.
pub fn bound_live_uuids(store: &Store) -> Vec<String> {
    store
        .agents
        .values()
        .filter(|r| r.tab_id.is_some())
        .map(|r| r.uuid.clone())
        .collect()
}

/// Labels get interpolated into KDL string literals and fzf menu lines —
/// strip the things that break them: quotes, control chars, and backslash
/// (KDL's escape introducer — a raw `\` is a parse error at whichever seam
/// bakes the label, worst case launch.kdl; fugu 2026-07-21, guardrail test
/// `backslash_label_is_guarded_through_real_parser` proves it on the real
/// zellij parser).
pub fn sanitize_label(s: &str) -> String {
    s.chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .filter(|c| *c != '"' && *c != '\\')
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

/// Format the immutable `clave spawn` argument snapshot shared by every KDL
/// shape. `run_add` creates the tab before its locked store write, so the
/// requested profile must be baked here and copied into the row afterwards;
/// rediscovery at either side of that add/store race could make resume replay a
/// different command than the pane already launched.
fn spawn_args_kdl(uuid: &str, label: &str, cwd: &str, claude_codex: bool) -> String {
    let mut args = format!(r#"args "spawn" "{uuid}" "--name" "{label}" "--cwd" "{cwd}""#);
    if claude_codex {
        args.push_str(r#" "--claude-codex""#);
    }
    args
}

/// Preflight the optional `claude-codex` wrapper before a `--codex` launch,
/// with the SAME hold-open-on-TTY treatment as run_add's base tool preflight
/// (add.rs base preflight). `clave add --codex` is a manual CLI flag that a
/// user may run inside a `close_on_exit` floating pane; a bare `?` there would
/// flash-and-vanish the missing-wrapper guidance before it can be read (fugu
/// review, 2026-07-23). Both run_add arms call this so the two paths cannot
/// drift. The hold-open is a TTY side-effect — tier-3, not unit-covered — and
/// intentionally mirrors the untested base-preflight idiom above it.
fn preflight_codex_wrapper() -> Result<()> {
    if let Err(e) = crate::doctor::preflight(
        &[crate::discover::ToolId::ClaudeCodex],
        "clave add --codex can't launch — missing wrapper:",
    ) {
        eprintln!("{e}");
        eprintln!("You can install it from another tab without leaving the session.");
        hold_open_if_tty();
        anyhow::bail!("missing claude-codex wrapper for `clave add --codex`");
    }
    Ok(())
}

/// The agent-tab KDL node WITH its own bar pane — for one-shot
/// `zellij action new-tab --layout` files ONLY, which do NOT pass through
/// the session's default_tab_template. A layout that HAS the template must
/// use `tab_node_bare` instead: zellij wraps explicit tab nodes with the
/// template too, so a bar-carrying node there renders a DOUBLE bar (live
/// finding, c8-cold-start 2026-07-18 — the eager tab loaded two plugin
/// instances in the same second and broke executor election).
///
/// `claude_codex` is baked into the spawn args via `spawn_args_kdl` — the
/// launch profile is an immutable KDL snapshot, not re-read from the store.
pub fn tab_node(
    binary: &str,
    wasm: &str,
    label: &str,
    uuid: &str,
    cwd: &str,
    claude_codex: bool,
) -> String {
    // split_direction="vertical" is REQUIRED for a LEFT bar: zellij stacks
    // sibling panes horizontally (rows) by default (Task 9 C1 finding; same
    // wrapper as setup::layout_kdl and the S2 spike layout). size="15%" not
    // size=30: fixed panes refuse resizes — see setup::layout_kdl.
    // `command` bakes the environment's clave (§2 binary split): the
    // versioned copy's absolute path in a stable session, bare `clave` in
    // dev/sandbox — so the resurrected pane re-execs the SAME binary.
    let spawn_args = spawn_args_kdl(uuid, label, cwd, claude_codex);
    format!(
        r#"    tab name="{label}" focus=true {{
        pane split_direction="vertical" {{
            pane size="15%" borderless=true {{
                plugin location="file:{wasm}"
            }}
            pane cwd="{cwd}" command="{binary}" {{
                {spawn_args}
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
///
/// `claude_codex` is baked into the spawn args via `spawn_args_kdl`, exactly
/// as in `tab_node` — the two builders must agree on the launch profile.
pub fn tab_node_bare(
    binary: &str,
    label: &str,
    uuid: &str,
    cwd: &str,
    claude_codex: bool,
) -> String {
    // `command` bakes the environment's clave — see tab_node.
    let spawn_args = spawn_args_kdl(uuid, label, cwd, claude_codex);
    format!(
        r#"    tab name="{label}" focus=true {{
        pane cwd="{cwd}" command="{binary}" {{
            {spawn_args}
        }}
    }}
"#
    )
}

/// Reject a cwd that can't be safely interpolated into generated KDL.
/// `label` goes through `sanitize_label`, but a cwd is a REAL filesystem
/// path — munging it (dropping a quote) would point the baked `--cwd`/
/// `cwd="…"` at a directory that doesn't exist, so `clave spawn` would
/// canonicalize-fail on the lie. A `"` or control char (legal-but-rare on
/// unix) is the only thing that breaks the KDL string literal AND the spawn
/// args; reject loudly at layout-assembly time so tab creation fails with a
/// clear cause instead of emitting malformed KDL zellij silently rejects.
/// Called at every seam that bakes a cwd (add/open/launch eager row).
pub fn validate_cwd(cwd: &str) -> Result<()> {
    // Backslash included (fugu 2026-07-21): it introduces a KDL escape, so a
    // raw `\` in a baked cwd string is a layout parse error like a quote is.
    anyhow::ensure!(
        !cwd.contains('"') && !cwd.contains('\\') && !cwd.chars().any(char::is_control),
        "cwd {cwd:?} contains a double-quote, backslash, or control char — refusing to bake unsafe KDL (rename the directory)"
    );
    Ok(())
}

/// The one-shot temp layout (§6.3): Zellij KDL has no variable substitution,
/// so the uuid/label/cwd are baked in, the file is passed to
/// `zellij action new-tab --layout`, then deleted. Baking the command in
/// makes tab creation IDEMPOTENT — resurrection is clave's job, not
/// zellij's (§6.8, C8 redesign 2026-07-17).
pub fn tab_layout(
    binary: &str,
    wasm: &str,
    label: &str,
    uuid: &str,
    cwd: &str,
    claude_codex: bool,
) -> String {
    format!(
        "layout {{\n{}}}\n",
        tab_node(binary, wasm, label, uuid, cwd, claude_codex)
    )
}

pub struct ResumeCandidate {
    pub uuid: String,
    pub label: String,
    /// The worktree/checkout dir this transcript belongs to (§6.3 revised
    /// 2026-07-21: `claude --resume` is project-dir-scoped, so resuming from
    /// any other cwd fails with "No conversation found"). The tab must open
    /// HERE, not in the picked repo dir — run_add bakes this into the layout.
    pub cwd: String,
    /// The branch of `cwd`'s worktree (None = detached). Carried so a resumed
    /// jsonl-only candidate records/labels its OWN branch, not the picker's.
    pub branch: Option<String>,
    /// Currently on screen (uuid found in dump-layout): picking it JUMPS to
    /// its tab — resuming a live session duplicates it (C7, round 7).
    pub live: bool,
}

/// One munged project dir's scan result, tagged with the worktree it maps to
/// (§6.3 worktree-aware resume). `run_add` builds one per `git worktree list`
/// entry (plus the picked dir); `resume_candidates` folds them into a single
/// uuid-deduped, recency-sorted list with per-candidate cwd attribution.
pub struct DirScan {
    /// The (canonicalized) worktree/checkout path — the transcript's true cwd.
    pub cwd: String,
    /// The worktree's branch (None = detached HEAD).
    pub branch: Option<String>,
    /// False for the repo's main checkout; true for a registered worktree.
    /// Drives the branch-suffixed `(wt)` label so picker rows are glanceable.
    pub is_worktree: bool,
    /// `(uuid, mtime)` from listing `~/.claude/projects/<munged cwd>/*.jsonl`.
    pub stems: Vec<(String, u64)>,
}

/// The MAIN working tree's path from parsed porcelain output: git documents
/// the main tree as the FIRST `worktree list` entry. This — not the picked
/// dir's `rev-parse --show-toplevel`, which inside a LINKED worktree returns
/// the worktree's own root — is the stable root that keys the store's
/// `repo_root` and the `(wt)` marker (fugu 2026-07-21, HIGH).
pub fn main_worktree_path(worktrees: &[WorktreeEntry]) -> Option<&str> {
    worktrees.first().map(|w| w.path.as_str())
}

/// A `git worktree list --porcelain` record: the worktree path and its branch
/// (None when the record is `detached`).
pub struct WorktreeEntry {
    pub path: String,
    pub branch: Option<String>,
}

/// Parse `git worktree list --porcelain` (§6.3 worktree-aware resume,
/// 2026-07-21 finding). Records are blank-line separated; each opens with
/// `worktree <path>` and may carry `branch refs/heads/<b>` or `detached`.
/// Everything else (`HEAD <sha>`, `bare`, `locked`, blank, garbage) is
/// ignored — porcelain is append-only, so unknown keys are forward-compatible.
/// Pure over the string output; the shell-out stays in `run_add`.
pub fn parse_worktrees(porcelain: &str) -> Vec<WorktreeEntry> {
    let mut out: Vec<WorktreeEntry> = Vec::new();
    for line in porcelain.lines() {
        if let Some(path) = line.strip_prefix("worktree ") {
            // A new `worktree` line starts a new record (blank-line separator
            // is implicit — we never need to see it).
            out.push(WorktreeEntry {
                path: path.to_string(),
                branch: None,
            });
        } else if let Some(branch) = line.strip_prefix("branch refs/heads/") {
            // strip past `refs/heads/` so slashed branches (`feat/x`) survive.
            if let Some(cur) = out.last_mut() {
                cur.branch = Some(branch.to_string());
            }
        }
    }
    out
}

/// §6.3 resume picker input: this repo's store rows + jsonl-discovered sessions
/// across EVERY worktree (`dirs`), not just the picked dir's — `claude
/// --resume` is project-dir-scoped (2026-07-21), so a worktree session is
/// invisible unless its own dir is scanned. Each candidate carries the cwd it
/// belongs to. Live agents are included but MARKED (§6.3 revised 2026-07-14:
/// many agents per repo; live = jump, dead = resume). Recency (mtime) first
/// across all dirs; store labels beat bare uuids; dedup by uuid.
pub fn resume_candidates(
    store: &Store,
    repo_root: &str,
    dirs: &[DirScan],
    live: &[String],
) -> Vec<ResumeCandidate> {
    // value = (mtime, store_label, cwd, branch, is_worktree).
    type Row = (u64, Option<String>, String, Option<String>, bool);
    let mut by_uuid: std::collections::BTreeMap<String, Row> = Default::default();
    for d in dirs {
        for (uuid, mtime) in &d.stems {
            by_uuid
                .entry(uuid.clone())
                // A uuid's jsonl is cwd-scoped, so cross-dir collisions are
                // near-impossible; if one occurs, keep the most-recent copy
                // and the cwd/branch IT belongs to.
                .and_modify(|e| {
                    if *mtime > e.0 {
                        e.0 = *mtime;
                        e.2 = d.cwd.clone();
                        e.3 = d.branch.clone();
                        e.4 = d.is_worktree;
                    }
                })
                .or_insert((*mtime, None, d.cwd.clone(), d.branch.clone(), d.is_worktree));
        }
    }
    for r in store.agents.values().filter(|r| r.repo_root == repo_root) {
        let e = by_uuid.entry(r.uuid.clone()).or_insert((
            r.last_interacted,
            None,
            r.cwd.clone(),
            Some(r.branch.clone()),
            r.worktree.is_some(),
        ));
        // The store row is authoritative for label + cwd/branch: the cwd was
        // canonicalized at creation and is worktree-aware (see
        // merge_resume_record). is_worktree from disk (if the jsonl was found)
        // is left intact — it reflects the same session.
        e.1 = Some(r.label.clone());
        e.2 = r.cwd.clone();
        e.3 = Some(r.branch.clone());
    }
    let mut list: Vec<(u64, ResumeCandidate)> = by_uuid
        .into_iter()
        .map(|(uuid, (mtime, store_label, cwd, branch, is_worktree))| {
            let base = store_label.clone().unwrap_or_else(|| uuid.clone());
            // §6.3 step 4: worktree candidates are branch-suffixed so the
            // picker distinguishes them. A bare uuid gains ` · <branch>`
            // (which worktree?); an EARNED store label already encodes its
            // branch (hook.rs writes `dir · branch · words`), so it is NOT
            // re-appended — only the `(wt)` marker is. Main-checkout
            // candidates keep their label unchanged.
            let label = if is_worktree {
                if store_label.is_some() {
                    sanitize_label(&format!("{base} (wt)"))
                } else {
                    let br = branch.as_deref().unwrap_or("-");
                    sanitize_label(&format!("{base} · {br} (wt)"))
                }
            } else {
                base
            };
            let live = live.contains(&uuid);
            (
                mtime,
                ResumeCandidate {
                    uuid,
                    label,
                    cwd,
                    branch,
                    live,
                },
            )
        })
        .collect();
    list.sort_by_key(|c| std::cmp::Reverse(c.0));
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
/// resetting. Re-adding a resumed agent means "it is on screen again under
/// this requested launch profile" — status/bind reset, the requested profile
/// follows the already-baked KDL, and every historical field is preserved.
///
/// No existing row (a jsonl-only candidate — discovered on disk, never
/// tracked): no better info exists, store the fresh record as-is.
pub fn merge_resume_record(existing: Option<&AgentRecord>, fresh: AgentRecord) -> AgentRecord {
    match existing {
        Some(row) => AgentRecord {
            // The tab was already created from `fresh` before this locked write;
            // copy only that requested launch choice so the persisted replay
            // matches the immutable KDL despite the add/store race.
            claude_codex: fresh.claude_codex,
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
    let mut child = Command::new(tool_path(crate::discover::ToolId::Fzf))
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

fn cmd_stdout(cmd: impl AsRef<std::ffi::OsStr>, args: &[&str]) -> Result<String> {
    let cmd = cmd.as_ref();
    let out = Command::new(cmd)
        .args(args)
        .output()
        .with_context(|| format!("running {}", cmd.to_string_lossy()))?;
    anyhow::ensure!(
        out.status.success(),
        "{} {args:?} failed",
        cmd.to_string_lossy()
    );
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

// tool_path moved to discover::tool_path (codex P2 on PR #29): hook and open
// need the same discovered-or-bare resolution, so it lives with discovery.
use crate::discover::tool_path;

/// The Alt+a keybind runs add in a floating pane with close_on_exit=true —
/// an abort's message would flash and VANISH (spec §Preflight pane-hold).
/// Block on Enter so the guidance is readable; TTY-gated so scripted
/// invocations never hang.
fn hold_open_if_tty() {
    use std::io::IsTerminal;
    if std::io::stdin().is_terminal() {
        eprintln!("\npress Enter to close");
        let _ = std::io::stdin().read_line(&mut String::new());
    }
}

/// List a munged project dir's `<uuid>.jsonl` stems + mtimes (§6.3). Factored
/// out of `run_add` so the worktree-aware resume loop can scan EVERY worktree's
/// dir with the same rule. A missing dir (worktree never hosted a session)
/// yields an empty vec — not an error.
fn scan_jsonl_stems(proj_dir: &Path) -> Vec<(String, u64)> {
    let mut stems = Vec::new();
    if let Ok(rd) = std::fs::read_dir(proj_dir) {
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
    stems
}

/// Branch to RECORD on a fresh store row (§6.3 worktree-aware resume; review
/// finding 2026-07-21). `cand_branch == None` means two different things and
/// must not be conflated: with `resumed == true` it is a candidate in a
/// DETACHED worktree → record `-` (matching the picker row's display and the
/// non-repo fallback), never the picked dir's HEAD — else an adopted detached
/// worktree session claims e.g. `main`. Only a `new` agent (`resumed ==
/// false`, no candidate) inherits the picked dir's branch.
pub fn record_branch(resumed: bool, cand_branch: Option<&str>, picked_branch: &str) -> String {
    match (resumed, cand_branch) {
        (true, Some(b)) => b.to_string(),
        (true, None) => "-".to_string(),
        (false, _) => picked_branch.to_string(),
    }
}

pub fn run_add(worktree: bool, claude_codex: bool) -> Result<()> {
    // Preflight (spec §Preflight): the fzf weave, git/claude, and zellij
    // itself are all needed before any tab exists — abort BEFORE creating
    // anything. Zellij included (coderabbit CLI, 2026-07-22): run_add execs
    // `zellij action new-tab`, so an undiscoverable zellij would otherwise
    // fail AFTER the picker and the store row, leaving orphaned state.
    if let Err(e) = crate::doctor::preflight(
        &[
            crate::discover::ToolId::Fzf,
            crate::discover::ToolId::Zoxide,
            crate::discover::ToolId::Git,
            crate::discover::ToolId::Claude,
            crate::discover::ToolId::Zellij,
        ],
        "clave add needs tools that are missing:",
    ) {
        eprintln!("{e}");
        eprintln!("You can install them from another tab without leaving the session.");
        hold_open_if_tty();
        anyhow::bail!("missing dependencies for `clave add`");
    }

    // 1) Pick a directory: fzf over zoxide's ranked list, current dir first
    //    (§6.3 — fzf+zoxide are verified present on the target machine).
    let cwd = std::env::current_dir()?.to_string_lossy().into_owned();
    let mut dirs: Vec<String> = vec![cwd.clone()];
    dirs.extend(
        cmd_stdout(tool_path(crate::discover::ToolId::Zoxide), &["query", "-l"])?
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
    // Route every git/zellij invocation through discovery (review 2026-07-22,
    // Fix 2): doctor promises off-PATH tools are used by absolute path, so
    // add must not fall back to bare `git`/`zellij` — an off-PATH git (SSH,
    // ~/.local/bin) would break repo detection preflight already passed.
    let git = tool_path(crate::discover::ToolId::Git);
    let repo_root = cmd_stdout(&git, &["-C", &physical_str, "rev-parse", "--show-toplevel"])
        .map(|s| s.trim().to_string())
        .unwrap_or_else(|_| physical_str.clone()); // non-repo dirs are fine
    let branch = cmd_stdout(
        &git,
        &["-C", &physical_str, "rev-parse", "--abbrev-ref", "HEAD"],
    )
    .map(|s| s.trim().to_string())
    .unwrap_or_else(|_| "-".to_string());

    // 3) Liveness input (§6.3 revised 2026-07-14: MANY agents per repo, so
    //    no auto-jump here — live agents surface as jump entries in the
    //    resume picker instead; the old first-live-agent jump made a second
    //    agent in the same repo impossible).
    let zellij = tool_path(crate::discover::ToolId::Zellij); // Fix 2 (review 2026-07-22)
    let dump = cmd_stdout(&zellij, &["action", "dump-layout"]).unwrap_or_default();
    let paths = store_paths()?;
    let store = crate::store::read_store(&paths)?;
    // Issue #6: liveness from the store's binds (authoritative — command
    // strings go blind under MCP servers), with the dump-layout scan folded in
    // as an additive fallback for a bind that hasn't landed. Union: a uuid live
    // by EITHER signal is a jump, never a resume (never a double-attach).
    let mut live = bound_live_uuids(&store);
    for u in live_uuids(&dump) {
        if !live.contains(&u) {
            live.push(u);
        }
    }

    // 4) new vs resume.
    let Some(choice) = fzf_pick(&["new".into(), "resume".into()], "agent> ")? else {
        return Ok(());
    };
    // resume_root: the main tree's root when the resume arm computed one —
    // it keys the store row so future picks from ANY dir of this repo find
    // the earned label (fugu 2026-07-21). `new` keeps the picked toplevel.
    let (uuid, worktree_path, existing, cand_cwd, cand_branch, resume_root) = if choice == "resume"
    {
        // clave owns the picker (§6.3 — claude --resume's own picker would
        // break resurrection). Candidates: store rows + jsonl scan across
        // EVERY worktree (2026-07-21 finding: `claude --resume` is
        // project-dir-scoped, and real work spreads across git worktrees, so
        // scanning only the picked dir hides worktree sessions). A non-repo
        // dir has no worktrees → the loop below falls back to the picked dir
        // alone, behavior unchanged.
        let porcelain = cmd_stdout(
            &git, // discovered path (review 2026-07-22) — same promise as every git call here
            &["-C", &repo_root, "worktree", "list", "--porcelain"],
        )
        .unwrap_or_default();
        let worktrees = parse_worktrees(&porcelain);
        // The MAIN tree's root, not the picked dir's toplevel (fugu
        // 2026-07-21, HIGH): `rev-parse --show-toplevel` inside a linked
        // worktree returns the WORKTREE's root, which would invert every
        // (wt) marker and miss every store row keyed by the real repo_root.
        // Non-repo pick → no porcelain → fall back to the picked dir.
        let main_root = main_worktree_path(&worktrees)
            .and_then(|p| std::fs::canonicalize(p).ok())
            .and_then(|p| p.to_str().map(String::from))
            .unwrap_or_else(|| repo_root.clone());
        let root_canon = std::fs::canonicalize(&main_root).ok();
        let projects = crate::env::claude_config_dir()?.join("projects");
        let mut dirs: Vec<DirScan> = Vec::new();
        let mut seen: std::collections::BTreeSet<String> = Default::default();
        for w in &worktrees {
            // Canonicalize to match munge_cwd's physical-path rule (S0b) and
            // to compare against the (physical) repo root. A vanished worktree
            // dir canonicalize-fails → skip it.
            let Ok(canon) = std::fs::canonicalize(&w.path) else {
                continue;
            };
            let cwd = canon.to_string_lossy().into_owned();
            if !seen.insert(cwd.clone()) {
                continue;
            }
            let is_worktree = root_canon.as_deref().is_none_or(|r| canon != r);
            let proj = projects.join(crate::munge::munge_cwd(&cwd));
            dirs.push(DirScan {
                cwd,
                branch: w.branch.clone(),
                is_worktree,
                stems: scan_jsonl_stems(&proj),
            });
        }
        // Always scan the picked dir too: a subdir of the repo (not a worktree
        // root) is not in `worktree list`, and pre-worktree behavior scanned
        // exactly this dir. Its is_worktree stays false (it lives in the main
        // checkout). physical_str is already canonical (step 2).
        if seen.insert(physical_str.clone()) {
            let proj = projects.join(crate::munge::munge_cwd(&physical_str));
            dirs.push(DirScan {
                cwd: physical_str.clone(),
                branch: Some(branch.clone()),
                is_worktree: false,
                stems: scan_jsonl_stems(&proj),
            });
        }
        let candidates = resume_candidates(&store, &main_root, &dirs, &live);
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
            let _ = Command::new(&zellij) // discovered above (Fix 2)
                .args(["pipe", "--name", "clave-nav", "--", &payload])
                .status();
            return Ok(());
        }
        if claude_codex {
            preflight_codex_wrapper()?;
        }
        // Carry the picked candidate's OWN cwd/branch (2026-07-21): a
        // jsonl-only worktree session must resume in its worktree dir, not the
        // picked repo dir, or `claude --resume` fails "No conversation found".
        let cand = candidates.iter().find(|c| c.uuid == uuid);
        let cand_cwd = cand.map(|c| c.cwd.clone());
        let cand_branch = cand.and_then(|c| c.branch.clone());
        // The lock-free store copy is fine for DERIVING the tab's cwd/label
        // (worst case one beat stale); the AUTHORITATIVE update-or-insert
        // happens under the lock in step 7.
        let existing = store.agents.get(&uuid).cloned();
        (uuid, None, existing, cand_cwd, cand_branch, Some(main_root))
    } else {
        if claude_codex {
            preflight_codex_wrapper()?;
        }
        let uuid = uuid::Uuid::new_v4().to_string();
        // Worktree opt-in (§6.3): clave shells out itself (never claude -w)
        // so it OWNS the path — needed for the munged jsonl existence check
        // and the store record.
        let wt = if worktree {
            let short = &uuid[..8];
            let path = format!("{repo_root}/.claude-worktrees/{short}");
            cmd_stdout(
                &git, // discovered above (Fix 2)
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
        (uuid, wt, None, None, None, None)
    };

    // 5) The agent's cwd for the TAB LAYOUT:
    //    - resume WITH a store row → the ROW's cwd. It was canonicalized when
    //      stored and is worktree-aware; recomputing from the picked dir would
    //      bake the wrong `--cwd` for a worktree agent and break spawn's
    //      jsonl-keyed resume (see merge_resume_record).
    //    - fresh worktree → canonicalize AGAIN (it's brand new — S0b applies).
    //    - else → the picked dir (already canonical from step 2).
    //    - resume of a jsonl-only candidate → the CANDIDATE's cwd (the
    //      worktree the transcript belongs to), not the picked dir: resume is
    //      project-dir-scoped (2026-07-21). cand_cwd is None only in the `new`
    //      branch, so the final arm keeps the picked-dir behavior there.
    let agent_cwd = match (&existing, &worktree_path) {
        (Some(row), _) => row.cwd.clone(),
        (None, Some(w)) => std::fs::canonicalize(w)?
            .to_str()
            .context("wt path")?
            .to_string(),
        (None, None) => cand_cwd.clone().unwrap_or_else(|| physical_str.clone()),
    };
    // The branch recorded/labelled for a jsonl-only resume must be the
    // candidate's worktree branch — `-` when its worktree is detached — not
    // the picked dir's HEAD (else a worktree session gets the main checkout's
    // branch; review finding 2026-07-21). An existing row keeps its own branch
    // via merge_resume_record; a fresh worktree uses the picked branch.
    // `cand_cwd.is_some()` ⇔ this is a resume pick (set for every candidate).
    let agent_branch = record_branch(cand_cwd.is_some(), cand_branch.as_deref(), &branch);
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
            // to gate the first-prompt upgrade. Uses agent_branch (the
            // candidate's worktree branch) so the record's branch and the
            // reconstructed prefix agree for a resumed worktree session.
            sanitize_label(&format!("{dir_name} · {agent_branch}"))
        }
    };

    // 6) One-shot temp layout → new tab (§6.3). $TMPDIR, deleted after.
    //    Guard the cwd before it's baked: canonicalize accepts paths with a
    //    `"`/control char that would break the generated KDL (see validate_cwd).
    validate_cwd(&agent_cwd)?;
    let wasm = wasm_path()?.to_str().context("wasm path")?.to_string();
    let binary = crate::release::runtime_binary();
    let layout = tab_layout(&binary, &wasm, &label, &uuid, &agent_cwd, claude_codex);
    let tmp = std::env::temp_dir().join(format!("clave-{uuid}.kdl"));
    std::fs::write(&tmp, layout)?;
    let status = Command::new(&zellij) // discovered above (Fix 2)
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
            repo_root: resume_root.clone().unwrap_or_else(|| repo_root.clone()),
            branch: agent_branch.clone(),
            label: label.clone(),
            status: clave_types::Status::Idle,
            last_interacted: now_unix(),
            last_visited: 0,
            worktree: worktree_path.clone(),
            label_source: LabelSource::FirstPrompt,
            claude_codex,
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
            claude_codex: false,
            tab_id: None,
            stale: false,
        }
    }

    #[test]
    fn bound_live_uuids_derives_liveness_from_store_binds() {
        // Issue #6 (2026-07-21): liveness must come from the store's uuid→tab_id
        // binds, NOT serialized commands. dump-layout serializes an agent
        // pane's CHILD process under an MCP server (`uv … run main.py`), not
        // `claude`, so live_uuids was BLIND while three agents were live —
        // `clave add` then offered a LIVE session as resume → double-attach.
        // A bound agent (tab_id Some, kept honest by clave prune-tabs) is live;
        // a dormant/closed-and-pruned one (tab_id None) is not.
        let mut s = Store::default();
        let mut bound = rec("u-live");
        bound.tab_id = Some(7);
        s.agents.insert("u-live".into(), bound);
        s.agents.insert("u-dormant".into(), rec("u-dormant")); // tab_id None
        assert_eq!(bound_live_uuids(&s), vec!["u-live".to_string()]);
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
        let kdl = tab_layout(
            "clave",
            "/data/clave-bar.wasm",
            "x · main",
            "u-1",
            "/x",
            false,
        );
        // The bar pane, the baked spawn (idempotent resurrection, §6.3/S4),
        // and the cwd all present:
        assert!(kdl.contains("location=\"file:/data/clave-bar.wasm\""));
        assert!(kdl.contains("\"spawn\" \"u-1\""));
        assert!(kdl.contains("cwd=\"/x\""));
        assert!(kdl.contains("name=\"x · main\""));
        // Regression (Task 9 C1): the bar must be a LEFT column, not a top strip.
        assert!(kdl.contains("split_direction=\"vertical\""));
        // §2 binary split: the pane command is the passed binary. A stable
        // session bakes the versioned copy's absolute path instead of bare.
        assert!(kdl.contains("command=\"clave\""));
        let abs = tab_layout("/data/clave/bin/clave-v0.1.0", "/w", "l", "u", "/x", false);
        assert!(abs.contains("command=\"/data/clave/bin/clave-v0.1.0\""));
        assert!(!abs.contains("command=\"clave\""));
    }

    #[test]
    fn tab_layout_codex_diff_is_only_spawn_flag() {
        // The launch profile is an immutable snapshot baked before the store
        // write. Keep the KDL delta to one token so Task 3 cannot accidentally
        // perturb the proven C8 layout while closing the add/store race.
        let plain = tab_layout("/bin/clave", "/bar.wasm", "label", "u", "/repo", false);
        let codex = tab_layout("/bin/clave", "/bar.wasm", "label", "u", "/repo", true);

        assert!(codex.contains(r#""--claude-codex""#));
        assert_eq!(codex.replace(r#" "--claude-codex""#, ""), plain);
    }

    #[test]
    fn sanitize_label_strips_kdl_breakers() {
        assert_eq!(sanitize_label("fix \"auth\"\nflow"), "fix auth flow");
    }

    #[test]
    fn validate_cwd_rejects_kdl_breakers_but_passes_normal_paths() {
        // A cwd is interpolated RAW into generated KDL (cwd="{cwd}" and the
        // spawn args) — unlike label, it cannot be munged (a mangled path
        // points nowhere, so `clave spawn` would canonicalize-fail on a lie).
        // A `"` or control char is legal-but-rare on unix and breaks the KDL
        // string literal, silently failing tab creation. Reject, don't munge.
        assert!(validate_cwd("/repo/worktrees/ab").is_ok());
        assert!(validate_cwd("/home/o/a b/dir").is_ok()); // spaces are fine
        assert!(validate_cwd("/repo/\"evil\"").is_err()); // double-quote
        assert!(validate_cwd("/repo/a\nb").is_err()); // control char
        assert!(validate_cwd("/repo/a\tb").is_err());
    }

    #[test]
    fn merge_resume_preserves_existing_row_and_resets_status() {
        // The resume-clobber defect (plan-review fix): an existing WORKTREE
        // row must survive a resume untouched except status/bind and the newly
        // requested launch profile. The latter must come from `fresh`: the tab
        // is created before the locked store write, so preserving the old bit
        // would make the row disagree with the immutable command already baked.
        let mut existing = rec("u-wt");
        existing.cwd = "/repo/.claude-worktrees/abc12345".into();
        existing.worktree = Some("/repo/.claude-worktrees/abc12345".into());
        existing.repo_root = "/repo".into();
        existing.branch = "clave/abc12345".into();
        existing.label = "abc12345 · clave/abc12345 · fix auth".into();
        existing.label_source = LabelSource::Summary;
        existing.status = Status::Working; // stale — the pane is gone
        existing.last_interacted = 77;
        existing.last_visited = 42;
        existing.tab_id = Some(3); // the DEAD tab that hosted it last time
        existing.stale = true;
        existing.claude_codex = false;
        let mut fresh = rec("u-wt"); // what the weave derives from the PICKED dir
        fresh.claude_codex = true;
        let merged = merge_resume_record(Some(&existing), fresh.clone());
        assert!(merged.claude_codex);
        assert_eq!(merged.status, Status::Idle);
        // The resumed agent lands in a brand-new tab: the old bind is stale
        // by definition — reset, the new tab's bar re-binds on join (§6.6 B).
        assert_eq!(merged.tab_id, None);
        assert_eq!(merged.label, existing.label);
        assert_eq!(merged.cwd, existing.cwd); // worktree cwd NOT relocated
        assert_eq!(merged.repo_root, existing.repo_root);
        assert_eq!(merged.branch, existing.branch);
        assert_eq!(merged.worktree, existing.worktree);
        assert_eq!(merged.last_interacted, existing.last_interacted);
        assert_eq!(merged.last_visited, existing.last_visited);
        assert_eq!(merged.label_source, existing.label_source);
        assert_eq!(merged.stale, existing.stale);

        let mut previous_codex = existing.clone();
        previous_codex.claude_codex = true;
        let mut requested_plain = fresh.clone();
        requested_plain.claude_codex = false;
        let switched_plain = merge_resume_record(Some(&previous_codex), requested_plain);
        assert!(!switched_plain.claude_codex);
        assert_eq!(switched_plain.cwd, previous_codex.cwd);
        assert_eq!(switched_plain.worktree, previous_codex.worktree);

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
        // Single-dir scan (the main checkout): the pre-worktree shape.
        let dirs = vec![DirScan {
            cwd: "/repo".into(),
            branch: Some("main".into()),
            is_worktree: false,
            stems: vec![("u-old".into(), 100u64), ("u-disk".into(), 200u64)],
        }];
        let live = vec!["u-live".to_string()];
        let c = resume_candidates(&s, "/repo", &dirs, &live);
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

    #[test]
    fn parse_worktrees_extracts_paths_and_branches() {
        // §6.3 worktree-aware resume (2026-07-21 finding: `claude --resume` is
        // project-dir-scoped). `git worktree list --porcelain` records are
        // blank-line separated; each opens with `worktree <path>` and carries
        // `branch refs/heads/<b>` or `detached`. Unknown lines are ignored.
        let porcelain = "\
worktree /Users/o/code/clave
HEAD abc123
branch refs/heads/main

worktree /Users/o/code/clave/.claude/worktrees/feat-x
HEAD def456
branch refs/heads/feat/x

worktree /Users/o/code/clave/.claude/worktrees/loose
HEAD 999aaa
detached

garbage that should be ignored
";
        let w = parse_worktrees(porcelain);
        assert_eq!(w.len(), 3);
        assert_eq!(w[0].path, "/Users/o/code/clave");
        assert_eq!(w[0].branch.as_deref(), Some("main"));
        assert_eq!(w[1].path, "/Users/o/code/clave/.claude/worktrees/feat-x");
        assert_eq!(w[1].branch.as_deref(), Some("feat/x")); // slash preserved
        assert_eq!(w[2].path, "/Users/o/code/clave/.claude/worktrees/loose");
        assert_eq!(w[2].branch, None); // detached → no branch
        assert!(parse_worktrees("").is_empty());
    }

    #[test]
    fn sanitize_label_strips_backslash() {
        // Fugu 2026-07-21 (HIGH): backslash is KDL's escape introducer — a
        // raw `\` in a label is a parse error at the seam that bakes it
        // (worst case launch.kdl → bricked cold start). Filtered like `"`.
        assert_eq!(sanitize_label(r"fix the \d regex"), "fix the d regex");
    }

    #[test]
    fn validate_cwd_rejects_backslash() {
        // Same KDL hazard as above, cwd side: refuse rather than bake.
        assert!(validate_cwd(r"/home/o/we\ird").is_err());
        assert!(validate_cwd("/home/o/fine").is_ok());
    }

    #[test]
    fn main_worktree_path_is_first_porcelain_entry() {
        // Fugu 2026-07-21 (HIGH): `rev-parse --show-toplevel` inside a
        // LINKED worktree returns the worktree's own root, so it cannot key
        // the store or the (wt) marker. `git worktree list` documents the
        // main working tree as the FIRST entry — that is the stable root.
        let w = parse_worktrees(
            "worktree /repo\nbranch refs/heads/main\n\nworktree /repo/wt\nbranch refs/heads/f\n",
        );
        assert_eq!(main_worktree_path(&w), Some("/repo"));
        assert_eq!(main_worktree_path(&[]), None);
    }

    #[test]
    fn record_branch_detached_resume_is_dash_not_picker_head() {
        // Review finding (2026-07-21): `cand_branch == None` conflates "no
        // candidate (new agent)" with "candidate in a DETACHED worktree".
        // A detached resume must record `-` (what the picker row shows, and
        // the non-repo fallback), never the picked dir's HEAD — else the
        // adopted lifeline worktree (recreated with `--detach`) claims `main`.
        assert_eq!(record_branch(true, None, "main"), "-");
        assert_eq!(record_branch(true, Some("feat/x"), "main"), "feat/x");
        // A `new` agent (no candidate) inherits the picked dir's branch.
        assert_eq!(record_branch(false, None, "main"), "main");
    }

    #[test]
    fn resume_candidates_attributes_cwd_and_marks_worktrees() {
        // §6.3 worktree-aware resume: candidates from EVERY worktree dir, each
        // carrying its OWN cwd/branch so the tab resumes in its true dir.
        let mut s = Store::default();
        let mut main_row = rec("u-main");
        main_row.repo_root = "/repo".into();
        main_row.cwd = "/repo".into();
        main_row.label = "repo · main · fix things".into();
        main_row.last_interacted = 50;
        s.agents.insert("u-main".into(), main_row);

        let dirs = vec![
            DirScan {
                cwd: "/repo".into(),
                branch: Some("main".into()),
                is_worktree: false,
                stems: vec![("u-main".into(), 50), ("u-disk-main".into(), 100)],
            },
            DirScan {
                cwd: "/repo/.claude/worktrees/wt".into(),
                branch: Some("feat/x".into()),
                is_worktree: true,
                stems: vec![("u-wt".into(), 300)],
            },
        ];
        let live = vec!["u-wt".to_string()];
        let c = resume_candidates(&s, "/repo", &dirs, &live);
        // Recency desc ACROSS dirs: u-wt(300) > u-disk-main(100) > u-main(50).
        assert_eq!(c.len(), 3);
        // Worktree candidate: its OWN cwd carried, bare uuid gets branch + (wt).
        assert_eq!(c[0].uuid, "u-wt");
        assert_eq!(c[0].cwd, "/repo/.claude/worktrees/wt");
        assert_eq!(c[0].branch.as_deref(), Some("feat/x"));
        assert!(c[0].live);
        assert_eq!(c[0].label, "u-wt · feat/x (wt)");
        // Main-checkout jsonl-only: repo-root cwd, bare uuid, no (wt) marker.
        assert_eq!(c[1].uuid, "u-disk-main");
        assert_eq!(c[1].cwd, "/repo");
        assert_eq!(c[1].label, "u-disk-main");
        assert!(!c[1].live);
        // Store row: earned label wins over uuid, store cwd kept.
        assert_eq!(c[2].uuid, "u-main");
        assert_eq!(c[2].cwd, "/repo");
        assert_eq!(c[2].label, "repo · main · fix things");
    }

    #[test]
    fn resume_candidates_store_label_beats_uuid_on_worktree_and_dedups() {
        // Dedup by uuid across store + disk (§6.3 step 5). A worktree agent
        // present BOTH as a store row and an on-disk jsonl collapses to ONE
        // candidate: the earned store label wins (and already encodes the
        // branch, so it is NOT re-appended), only the (wt) marker is added.
        let mut s = Store::default();
        let mut row = rec("u-wt");
        row.repo_root = "/repo".into();
        row.cwd = "/repo/wt".into();
        row.worktree = Some("/repo/wt".into());
        row.branch = "feat/x".into();
        row.label = "wt · feat/x · earned".into();
        row.last_interacted = 10;
        s.agents.insert("u-wt".into(), row);
        let dirs = vec![DirScan {
            cwd: "/repo/wt".into(),
            branch: Some("feat/x".into()),
            is_worktree: true,
            stems: vec![("u-wt".into(), 999)],
        }];
        let c = resume_candidates(&s, "/repo", &dirs, &[]);
        assert_eq!(c.len(), 1); // deduped, not doubled
        assert_eq!(c[0].uuid, "u-wt");
        assert_eq!(c[0].cwd, "/repo/wt"); // store cwd kept
        assert_eq!(c[0].label, "wt · feat/x · earned (wt)");
    }

    #[test]
    fn picked_candidate_cwd_bakes_into_tab_layout() {
        // §6.3 worktree-aware resume: the tab must open in the CANDIDATE's cwd
        // (the worktree the transcript belongs to), NOT the picked repo dir —
        // `claude --resume` is project-dir-scoped (2026-07-21). This pins the
        // resume_candidates → tab_layout seam that run_add wires up.
        let dirs = vec![DirScan {
            cwd: "/repo/.claude/worktrees/wt".into(),
            branch: Some("feat/x".into()),
            is_worktree: true,
            stems: vec![("u-wt".into(), 1)],
        }];
        let c = resume_candidates(&Store::default(), "/repo", &dirs, &[]);
        let picked = &c[0];
        let kdl = tab_layout(
            "clave",
            "/w.wasm",
            &picked.label,
            &picked.uuid,
            &picked.cwd,
            false,
        );
        assert!(kdl.contains("cwd=\"/repo/.claude/worktrees/wt\""));
        assert!(!kdl.contains("cwd=\"/repo\"")); // NOT the picker/root dir
        assert!(kdl.contains("\"--cwd\" \"/repo/.claude/worktrees/wt\""));
    }
}
