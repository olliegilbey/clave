//! `clave add` (§6.3): pick a directory, then new-or-resume an agent in a new
//! tab. The INTERACTIVE weave (fzf) lives in run_add; everything decidable
//! is a pure function above it so it can be unit-tested.

use std::collections::BTreeSet;
use std::io::Write as _;
use std::path::Path;
use std::process::{Command, Stdio};

use anyhow::{Context, Result};

use crate::hook::push_snapshot;
use crate::setup::{data_dir, session_row_height, wasm_path};
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

/// Canonicalize one dump-layout token to a row uuid (#226). A `--resume`
/// token can be a row's minted uuid, a rotated conversation's live id (#99) —
/// or, since `claude --resume <name>` resolves a NAME on Claude's side, a
/// session TITLE, which is not unique: the original DJ bug read a title as a
/// session id, matched nothing, and the false-dead row invited a
/// double-attach. Titles resolve to their most-recently-interacted holder
/// (ratified #226); a token naming nothing returns None and contributes
/// nothing — never a false-dead, never a false-live. All three scan
/// consumers (`live_uuid_union`, `open_is_live`, `protected_from_dump`) go
/// through here so the translation can't skew.
pub fn resolve_scan_token(store: &Store, token: &str) -> Option<String> {
    if store.agents.contains_key(token) {
        return Some(token.to_string());
    }
    if let Some(r) = store
        .agents
        .values()
        .find(|r| r.live_session.as_deref() == Some(token))
    {
        return Some(r.uuid.clone());
    }
    store
        .agents
        .values()
        .filter(|r| r.title.as_deref() == Some(token))
        .max_by_key(|r| r.last_interacted)
        .map(|r| r.uuid.clone())
}

/// Every scan token of a dump, canonicalized — the deduped set the liveness
/// consumers share (#226). A token the resolver cannot place is carried RAW,
/// not dropped: it can name a jsonl-only session the store has never minted,
/// and the picker marks those live by that very id — dropping it would read
/// the pane dead and re-open the double-attach. A carried title that matched
/// no row is equally harmless: nothing downstream is keyed by titles.
pub fn resolved_scan_uuids(store: &Store, dump_layout: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for t in live_uuids(dump_layout) {
        let u = resolve_scan_token(store, &t).unwrap_or(t);
        if !out.contains(&u) {
            out.push(u);
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

/// Every uuid the picker must treat as LIVE (a jump, never a resume — a resume
/// would double-attach). Issue #6: the store's binds are authoritative, because
/// serialized command strings go blind under an MCP server, and the
/// `dump-layout` scan is folded in as an ADDITIVE fallback for a bind that has
/// not landed yet. A uuid live by either signal is live.
///
/// The scan's uuids are translated back through `live_session` (#99). A
/// resurrected rotated pane runs `claude --resume <live-id>`, so what
/// `live_uuids` reads out of the dump is the CONVERSATION's id, not the row's —
/// unmapped it would name nothing the picker knows and the fallback would go
/// silently blind for exactly the panes #99 fixed.
pub fn live_uuid_union(store: &Store, dump_layout: &str) -> Vec<String> {
    let mut live = bound_live_uuids(store);
    // Scan tokens arrive canonicalized (#226): live-session AND title tokens
    // land on their row's minted uuid; a token naming nothing is dropped
    // rather than carried as an id that can never match.
    for u in resolved_scan_uuids(store, dump_layout) {
        if !live.contains(&u) {
            live.push(u);
        }
    }
    live
}

/// The rows `clave prune` (#149) must never retire: the ones the RUNNING
/// zellij session is actually hosting, per its layout dump.
///
/// Deliberately NOT [`live_uuid_union`], and the difference is the whole
/// point: binds are session-scoped and cleared only at the NEXT launch, so
/// between a kill and a relaunch every row still carries its dead session's
/// `tab_id` and a bind-based guard would make the entire fleet unprunable
/// (#150 review). Only presence in the live dump protects.
///
/// The `live_session` leg is the same #99 translation [`live_uuid_union`]
/// does, for the same reason and it is load-bearing here in a harsher way: a
/// pane whose conversation rotated runs `claude --resume <live-id>`, so the
/// dump names the CONVERSATION and not the row. Matched on the minted uuid
/// alone, a rotated agent sitting open in a tab is invisible to this — and an
/// invisible row is an unprotected one, so `clave prune` deletes the sidebar
/// row of an agent the user is looking at.
pub fn protected_from_dump(store: &Store, dump_layout: &str) -> BTreeSet<String> {
    // Same canonicalization as the union (#226): a name-resumed pane's row is
    // hosted by the running session too, and unprotected meant deletable.
    // Intersected with the store — this set's contract is ROWS never to
    // retire, so carried-raw tokens that name no row stay out of it.
    resolved_scan_uuids(store, dump_layout)
        .into_iter()
        .filter(|u| store.agents.contains_key(u))
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

/// The agent-tab KDL node WITH its own bar pane — for one-shot
/// `zellij action new-tab --layout` files ONLY, which do NOT pass through
/// the session's default_tab_template. A layout that HAS the template must
/// use `tab_node_bare` instead: zellij wraps explicit tab nodes with the
/// template too, so a bar-carrying node there renders a DOUBLE bar (live
/// finding, c8-cold-start 2026-07-18 — the eager tab loaded two plugin
/// instances in the same second and broke executor election).
///
/// `collapsed` is the mode the tab must be born in (LEDGER D36, task 7b′): a
/// tab born expanded into a collapsed fleet flashes wide and then snaps. The
/// width itself is not an input any more — the bar pane is a fixed column
/// count taken from `row_height.target_cols` (#232; formerly the single-mode
/// `target_cols_for`), which zellij applies exactly whatever the window is,
/// so nothing here needs to know the display.
pub fn tab_node(
    binary: &str,
    wasm: &str,
    label: &str,
    uuid: &str,
    cwd: &str,
    collapsed: bool,
    row_height: clave_types::RowHeight,
) -> String {
    // split_direction="vertical" is REQUIRED for a LEFT bar: zellij stacks
    // sibling panes horizontally (rows) by default (Task 9 C1 finding; same
    // wrapper as setup::layout_kdl and the S2 spike layout). The bar pane
    // itself — fixed-cols, see its doc — comes from the one place that
    // emits it (`setup::bar_pane_kdl`).
    // `command` bakes the environment's clave (§2 binary split): the
    // versioned copy's absolute path in a stable session, bare `clave` in
    // dev/sandbox — so the resurrected pane re-execs the SAME binary.
    format!(
        r#"    tab name="{label}" focus=true {{
        pane split_direction="vertical" {{
{pane}            pane cwd="{cwd}" command="{binary}" {{
                args "spawn" "{uuid}" "--name" "{label}" "--cwd" "{cwd}"
            }}
        }}
    }}
"#,
        pane = crate::setup::bar_pane_kdl(
            binary,
            wasm,
            row_height.target_cols(collapsed),
            "            ",
            row_height
        ),
    )
}

/// The bar-LESS agent-tab node for layouts that carry
/// default_tab_template (the §6.8 launch layout): the template supplies
/// the bar + vertical split, and this node's pane fills its `children`
/// slot. Same baked idempotent spawn — only the bar pane differs.
pub fn tab_node_bare(binary: &str, label: &str, uuid: &str, cwd: &str) -> String {
    // `command` bakes the environment's clave — see tab_node.
    format!(
        r#"    tab name="{label}" focus=true {{
        pane cwd="{cwd}" command="{binary}" {{
            args "spawn" "{uuid}" "--name" "{label}" "--cwd" "{cwd}"
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
    collapsed: bool,
    row_height: clave_types::RowHeight,
) -> String {
    // #181: the new tab carries the same two swap geometries every other tab
    // has, so Alt+c works in a dwell-opened tab exactly as it does in a
    // template-born one. This file has NO default_tab_template (that is the
    // whole point of the one-shot path), so the explicit tab node below is used
    // verbatim and the swap layouts sit alongside it.
    format!(
        "layout {{\n{swaps}{tab}}}\n",
        swaps = crate::setup::swap_layouts_kdl(binary, wasm, collapsed, row_height),
        tab = tab_node(binary, wasm, label, uuid, cwd, collapsed, row_height)
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
        // A ROTATED row owns two stems on disk, and the scan above cannot tell:
        // it joins by `stem == uuid`, so the live transcript matched no row and
        // stood as its own UNATTACHED candidate — the picker offering a second
        // copy of a session already open in a tab, and resuming it would give a
        // live agent a second store row (#99). The row is the one that knows,
        // so its live stem is folded in here, keeping the FRESHER mtime: that
        // file is the conversation actually being typed into, so it is also the
        // truer recency for this row.
        let rotated = r
            .live_session
            .as_deref()
            .and_then(|l| by_uuid.remove(l))
            .map(|row| row.0);
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
        if let Some(mtime) = rotated {
            e.0 = e.0.max(mtime);
        }
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

/// #139: the Alt+a dir picker's candidate list — zoxide's ranked entries
/// (order preserved, current dir first) unioned with every worktree of every
/// repo the store already tracks. A worktree never `cd`'d into does not exist
/// for zoxide, yet it is a first-class fleet location; visiting it first is an
/// unreasonable precondition. Dedup by exact path keeps zoxide's ranking for
/// dirs it knows; a zoxide-known LINKED worktree still gains the `(wt)` mark.
/// Returns `(path, mark_wt)`.
pub fn dir_candidates(zoxide: Vec<String>, worktrees: Vec<(String, bool)>) -> Vec<(String, bool)> {
    let mut out: Vec<(String, bool)> = zoxide.into_iter().map(|d| (d, false)).collect();
    for (path, linked) in worktrees {
        match out.iter_mut().find(|(d, _)| *d == path) {
            Some((_, mark)) => *mark |= linked,
            None => out.push((path, linked)),
        }
    }
    out
}

/// One dir-picker fzf line: the path, tab-suffixed with the `(wt)` marker for
/// a linked worktree — the same marker convention the resume picker uses.
/// `picked_dir` is the inverse; the tab keeps the marker out of path space
/// (a literal tab in a path is already rejected by `validate_cwd`).
pub fn dir_line(path: &str, wt: bool) -> String {
    if wt {
        format!("{path}\t(wt)")
    } else {
        path.to_string()
    }
}

/// The path half of a picked dir line: strips exactly the generated
/// `\t(wt)` suffix, nothing else. A zoxide path with a literal tab in it
/// must pass through INTACT so `validate_cwd` rejects it loudly — splitting
/// on the first tab silently truncated such a path, and a truncated prefix
/// that happens to be a directory launches the agent in the wrong checkout
/// (#143 review).
pub fn picked_dir(line: &str) -> &str {
    line.strip_suffix("\t(wt)").unwrap_or(line)
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
            pane_id: None,
            ..row.clone()
        },
        None => fresh,
    }
}

/// The fields `run_add` has already resolved by the time it locks the store
/// for the record write (step 7) — everything the fresh-record literal needs
/// beyond what the lock itself supplies (the ordinal, any existing row).
pub(crate) struct FreshRecordInputs<'a> {
    pub uuid: &'a str,
    pub cwd: &'a str,
    pub repo_root: &'a str,
    pub branch: &'a str,
    pub label: &'a str,
    pub worktree: Option<String>,
    pub default_branch: Option<String>,
    /// A RESUMED conversation's buckets, derived from its own transcript
    /// (maintainer ruling 2026-08-20: the jsonl is the source of truth, so a
    /// row with real history enters at its real weight, not its opener's
    /// echo). `None` — no transcript yet — is the brand-new case, which
    /// keeps the ratified opener inheritance.
    pub own_buckets: Option<std::collections::BTreeMap<u32, u32>>,
}

/// The S1 mint + merge-resume-or-insert sequence `run_add` runs under the
/// store lock (§6.3 step 7). Extracted from the `with_store_mut` closure so
/// this — the same path a real `clave add` takes — is directly unit-testable
/// without fzf/zellij: `run_add` and the tests below both drive it.
///
/// Spec (2026-08-19, amended 2026-08-20): a brand-new conversation inherits
/// the opener's buckets — an exact copy, so identical scores + the position
/// tiebreak put it directly below its opener until real commitments diverge
/// them. A conversation resumed FROM ITS TRANSCRIPT enters at its own
/// derived weight instead (`own_buckets`), and a resumed row already in the
/// store keeps its earned buckets via `merge_resume_record`'s
/// `..row.clone()`.
pub(crate) fn mint_record(s: &mut Store, inputs: FreshRecordInputs) -> AgentRecord {
    let ord = s.mint_ord();
    let seeded = inputs
        .own_buckets
        .unwrap_or_else(|| crate::store::opener_buckets(s));
    let fresh = AgentRecord {
        uuid: inputs.uuid.to_string(),
        cwd: inputs.cwd.to_string(),
        repo_root: inputs.repo_root.to_string(),
        branch: inputs.branch.to_string(),
        label: inputs.label.to_string(),
        status: clave_types::Status::Idle,
        last_interacted: now_unix(),
        commit_ord: ord,
        last_visited: 0,
        worktree: inputs.worktree,
        label_source: LabelSource::FirstPrompt,
        tab_id: None,
        pane_id: None,
        stale: false,
        title: None,
        summary: String::new(),
        default_branch: inputs.default_branch,
        context_tokens: None,
        context_level: None,
        // A fresh row is by definition still on its minted uuid. An
        // EXISTING row's live id survives this via `merge_resume_record`'s
        // `..row.clone()`, which is what keeps a re-added rotated agent
        // pointing at its live conversation (#99).
        live_session: None,
        buckets: seeded,
        model: None,
        provider: None,
        effort: None,
        pr_number: None,
        pr_checked: 0,
        pr_branch: String::new(),
    };
    // Note `merge_resume_record` PRESERVES an existing row's
    // `default_branch` along with everything else, so a row written before
    // #86 keeps `None` and the bar keeps its heuristic for it. Deliberate:
    // the merge's whole contract is that a re-add means "it is on screen
    // again" and resets nothing but status, and the fallback already makes
    // that row no worse than it was.
    let mut merged = merge_resume_record(s.agents.get(inputs.uuid), fresh);
    // A resume opens a brand-new tab, which birth-touches to the top
    // anyway; giving the ROW the same ordinal keeps the two consistent and
    // stops the row plunging if that tab is closed before any prompt.
    merged.commit_ord = ord;
    s.agents.insert(inputs.uuid.to_string(), merged.clone());
    merged
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

pub(crate) fn cmd_stdout(cmd: impl AsRef<std::ffi::OsStr>, args: &[&str]) -> Result<String> {
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

/// The REPOSITORY's default branch — the branch a plain checkout of it sits on,
/// and the only branch design-lock §5.1 lets the bar render with NO provenance
/// mark. Review finding 2026-07-29 (#86): the bar decided this by NAME, treating
/// `main` and `master` as exhaustive, so an ordinary default checkout of a repo
/// whose default is `trunk`, `develop` or `dev` took the branch glyph —
/// mislabelling a valid repository on naming convention alone. Resolved HERE,
/// where git can actually be asked, and carried on the record.
///
/// Two sources, most authoritative first, and both strictly LOCAL — `add` runs
/// interactively in front of the user, so nothing here may touch the network:
///
/// 1. `symbolic-ref --short refs/remotes/origin/HEAD` — what the remote itself
///    declared at clone time. Missing on a repo with no remote, and on old
///    clones made before git wrote the ref.
/// 2. `init.defaultBranch`, accepted ONLY when `refs/heads/<it>` exists in this
///    repo. A local-only repo has no canonical default to read; the branch the
///    user's own git would have created is the best answer available, and the
///    existence gate stops a config left over from another project from
///    inventing a branch this repo does not have.
///
/// `None` is a real answer, not a failure — the bar falls back to its
/// `main`/`master` heuristic for it, so behaviour is never WORSE than before
/// this field existed.
pub(crate) fn resolve_default_branch(git: &Path, repo_root: &str) -> Option<String> {
    let head = cmd_stdout(
        git,
        &[
            "-C",
            repo_root,
            "symbolic-ref",
            "--short",
            "refs/remotes/origin/HEAD",
        ],
    )
    .ok()
    .and_then(|s| strip_origin_prefix(&s));
    if head.is_some() {
        return head;
    }
    let configured = cmd_stdout(
        git,
        &["-C", repo_root, "config", "--get", "init.defaultBranch"],
    )
    .ok()?
    .trim()
    .to_string();
    if configured.is_empty() {
        return None;
    }
    // cmd_stdout errors on a non-zero exit, which is exactly what
    // `rev-parse --verify` returns for a ref that is not there.
    cmd_stdout(
        git,
        &[
            "-C",
            repo_root,
            "rev-parse",
            "--verify",
            "--quiet",
            &format!("refs/heads/{configured}"),
        ],
    )
    .is_ok()
    .then_some(configured)
}

/// `symbolic-ref --short refs/remotes/origin/HEAD` prints `origin/<branch>`.
/// The remote has to come off: the record's `branch` is a plain branch name, and
/// provenance compares the two for equality. Split ONCE so a slashed default
/// (`release/v1`) survives intact.
fn strip_origin_prefix(out: &str) -> Option<String> {
    let s = out.trim();
    let b = s.strip_prefix("origin/").unwrap_or(s);
    (!b.is_empty()).then(|| b.to_string())
}

/// #139 (io half of `dir_candidates`): every worktree of every distinct
/// `repo_root` the store tracks, canonicalized (S0b), vanished paths skipped,
/// linked-vs-main decided per repo the same way the resume scan decides it
/// (first porcelain entry is the main tree — see `main_worktree_path`).
/// A repo_root that is gone or not a repo yields nothing (`cmd_stdout` fails
/// → empty porcelain). BTreeSet roots keep the output deterministic.
fn store_worktree_dirs(git: &Path, store: &Store) -> Vec<(String, bool)> {
    let roots: std::collections::BTreeSet<&str> = store
        .agents
        .values()
        .map(|r| r.repo_root.as_str())
        .collect();
    let mut seen: std::collections::BTreeSet<String> = Default::default();
    let mut out = Vec::new();
    for root in roots {
        let porcelain =
            cmd_stdout(git, &["-C", root, "worktree", "list", "--porcelain"]).unwrap_or_default();
        let worktrees = parse_worktrees(&porcelain);
        let main = main_worktree_path(&worktrees).and_then(|p| std::fs::canonicalize(p).ok());
        for w in &worktrees {
            let Ok(canon) = std::fs::canonicalize(&w.path) else {
                continue; // vanished worktree — nothing to open there
            };
            let linked = main.as_deref().is_none_or(|m| canon != m);
            let Some(path) = canon.to_str() else {
                continue;
            };
            if seen.insert(path.to_string()) {
                out.push((path.to_string(), linked));
            }
        }
    }
    out
}

pub fn run_add(worktree: bool) -> Result<()> {
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
    //    (§6.3 — fzf+zoxide are verified present on the target machine),
    //    UNIONED with every store-known repo's worktrees (#139): a worktree
    //    zoxide has never seen is still a first-class fleet location. The
    //    store read here is lock-free and only feeds the candidate list; the
    //    authoritative read happens in step 3 as before.
    // Route every git/zellij invocation through discovery (review 2026-07-22,
    // Fix 2): doctor promises off-PATH tools are used by absolute path, so
    // add must not fall back to bare `git`/`zellij` — an off-PATH git (SSH,
    // ~/.local/bin) would break repo detection preflight already passed.
    let git = tool_path(crate::discover::ToolId::Git);
    let cwd = std::env::current_dir()?.to_string_lossy().into_owned();
    let mut zx: Vec<String> = vec![cwd.clone()];
    zx.extend(
        cmd_stdout(tool_path(crate::discover::ToolId::Zoxide), &["query", "-l"])?
            .lines()
            .map(String::from),
    );
    zx.dedup();
    let wt_dirs = store_paths()
        .and_then(|p| crate::store::read_store(&p))
        .map(|s| store_worktree_dirs(&git, &s))
        .unwrap_or_default();
    let lines: Vec<String> = dir_candidates(zx, wt_dirs)
        .iter()
        .map(|(d, wt)| dir_line(d, *wt))
        .collect();
    let Some(picked_line) = fzf_pick(&lines, "agent dir> ")? else {
        return Ok(());
    };
    let dir = picked_dir(&picked_line).to_string();

    // 2) Canonicalize FIRST (S0b) — everything downstream keys off the
    //    physical path: repo_root, munged jsonl dir, the spawn command.
    let physical = std::fs::canonicalize(&dir).with_context(|| format!("canonicalizing {dir}"))?;
    let physical_str = physical.to_str().context("non-UTF8 dir")?.to_string();
    let repo_root = cmd_stdout(&git, &["-C", &physical_str, "rev-parse", "--show-toplevel"])
        .map(|s| s.trim().to_string())
        .unwrap_or_else(|_| physical_str.clone()); // non-repo dirs are fine
    let branch = cmd_stdout(
        &git,
        &["-C", &physical_str, "rev-parse", "--abbrev-ref", "HEAD"],
    )
    .map(|s| s.trim().to_string())
    .unwrap_or_else(|_| "-".to_string());
    // Resolved against the picked dir's toplevel, which is correct even inside
    // a linked worktree: `refs/remotes` and `config` are SHARED across a repo's
    // worktrees, so every one of them answers with the same default (#86).
    let default_branch = resolve_default_branch(&git, &repo_root);

    // 3) Liveness input (§6.3 revised 2026-07-14: MANY agents per repo, so
    //    no auto-jump here — live agents surface as jump entries in the
    //    resume picker instead; the old first-live-agent jump made a second
    //    agent in the same repo impossible).
    let zellij = tool_path(crate::discover::ToolId::Zellij); // Fix 2 (review 2026-07-22)
    // Session-hard (#183 review, P1): every zellij leg below names its target
    // explicitly. The env vars alone FAIL OPEN — with no name supplied, a
    // dead target session lets zellij serve the sole remaining live one
    // (FOOTGUNS, the 2026-08-07 incident; `send_action_to_session`'s
    // `ActiveSession::One` arm) — where `--session` makes the same race exit
    // 1 instead. `session_name()` is `$CLAVE_SESSION` or the dedicated
    // "clave" session (§6.8), so the field path names the session it is
    // already inside.
    let session = crate::env::session_name();
    // Fail closed like open.rs does: an empty dump under-reports live agents
    // in `live_uuid_union`, and a live agent misread as dormant turns a
    // resume pick into a second tab instead of a jump.
    let dump = cmd_stdout(
        &zellij,
        &["--session", session.as_str(), "action", "dump-layout"],
    )
    .with_context(|| format!("zellij dump-layout for session {session}"))?;
    let paths = store_paths()?;
    let store = crate::store::read_store(&paths)?;
    let live = live_uuid_union(&store, &dump);
    // The one layout input the new tab's bar is SIZED by. Derived here,
    // lock-free, for the same reason `existing` is: it feeds the layout only.
    // `collapsed` is the mode the fleet is in — a tab born expanded into a
    // collapsed fleet flashes wide and then snaps, which is the jank D36
    // removed everywhere except here.
    let collapsed = store.collapsed;

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
            let status = Command::new(&zellij) // discovered above (Fix 2)
                .args([
                    "--session",
                    &session,
                    "pipe",
                    "--name",
                    "clave-nav",
                    "--",
                    &payload,
                ])
                .status()
                .context("sending clave-nav pipe")?;
            anyhow::ensure!(status.success(), "zellij pipe to {session} failed");
            return Ok(());
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
    // The SESSION's own mode, not the store's (finding 2, #232 review):
    // `clave rows` only takes effect at the next launch, so a tab minted
    // mid-session must match the plugin identity the running config.kdl's
    // keybinds already address — reading `store.row_height` here would bake
    // a tab whose bar registers under a configuration no keybind matches
    // (a deaf bar; second-bar if every launch-era tab has since closed).
    let row_height = session_row_height(&data_dir()?);
    let layout = tab_layout(
        &binary, &wasm, &label, &uuid, &agent_cwd, collapsed, row_height,
    );
    let tmp = std::env::temp_dir().join(format!("clave-{uuid}.kdl"));
    std::fs::write(&tmp, layout)?;
    let status = Command::new(&zellij) // discovered above (Fix 2)
        .args([
            "--session",
            &session,
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
    // A resumed conversation's own history, read OUTSIDE the store lock (a
    // transcript can be tens of MB; the lock protects the store, not this
    // read). Probes the transcript's possible homes (#139: not always the
    // cwd); None = brand-new = opener inheritance in `mint_record`.
    let own_buckets = crate::env::claude_config_dir().ok().and_then(|dir| {
        crate::backfill::derive_for_row(
            &dir,
            &[
                agent_cwd.as_str(),
                resume_root.as_deref().unwrap_or(&repo_root),
            ],
            &uuid,
            None,
            crate::store::unix_hour(now_unix()),
        )
    });
    let snap = with_store_mut(&paths, |s| {
        // S1: a new row is a user commitment, so it is minted an ordinal from
        // this same locked write and enters at the top. Before S1 a new row
        // inherited no order at all and could sink below every dormant row.
        // See `mint_record` for the ordinal mint, the newborn's inherited
        // buckets, and the resume-preserving merge.
        mint_record(
            s,
            FreshRecordInputs {
                uuid: &uuid,
                cwd: &agent_cwd,
                repo_root: resume_root.as_deref().unwrap_or(&repo_root),
                branch: &agent_branch,
                label: &label,
                worktree: worktree_path.clone(),
                default_branch: default_branch.clone(),
                own_buckets: own_buckets.clone(),
            },
        );
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
    use std::collections::BTreeMap;

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

    /// #226: `claude --resume <name>` resolves a NAME on Claude's side, so a
    /// dump-layout token can be a session TITLE, which collides — the original
    /// DJ bug read "DJ" as a session id, matched nothing, and the false-dead
    /// row invited a double-attach. Resolution order: minted uuid, live
    /// session id (#99), then title with the most-recently-interacted holder
    /// winning; a token naming nothing contributes nothing.
    #[test]
    fn scan_tokens_resolve_uuid_then_live_session_then_title_by_recency() {
        let mut s = Store::default();
        s.agents.insert("u1".into(), rec("u1"));
        let mut rotated = rec("u2");
        rotated.live_session = Some("rot-2".into());
        s.agents.insert("u2".into(), rotated);
        let mut dj_old = rec("u3");
        dj_old.title = Some("DJ".into());
        dj_old.last_interacted = 100;
        s.agents.insert("u3".into(), dj_old);
        let mut dj_new = rec("u4");
        dj_new.title = Some("DJ".into());
        dj_new.last_interacted = 200;
        s.agents.insert("u4".into(), dj_new);

        assert_eq!(resolve_scan_token(&s, "u1").as_deref(), Some("u1"));
        assert_eq!(resolve_scan_token(&s, "rot-2").as_deref(), Some("u2"));
        assert_eq!(
            resolve_scan_token(&s, "DJ").as_deref(),
            Some("u4"),
            "title collision: most-recently-interacted holder wins"
        );
        assert_eq!(resolve_scan_token(&s, "nobody"), None);
    }

    /// #226: the picker's live set and prune's protected set both see a
    /// name-resumed pane — "DJ" in the dump marks the title's most-recent
    /// holder live (a jump, not a second attach) and protects it from prune.
    #[test]
    fn a_name_resumed_pane_reaches_the_union_and_the_protected_set() {
        let mut s = Store::default();
        let mut dj = rec("u-dj");
        dj.title = Some("DJ".into());
        s.agents.insert("u-dj".into(), dj);
        let dump = "tab {\n  pane command=\"claude\" {\n    args \"--resume\" \"DJ\"\n  }\n}";
        assert_eq!(live_uuid_union(&s, dump), vec!["u-dj".to_string()]);
        assert!(protected_from_dump(&s, dump).contains("u-dj"));
        // A token naming no row is CARRIED in the union (it can be a
        // jsonl-only session the picker knows by exactly that id) but never
        // enters the protected set, whose contract is store rows only.
        let dump = "tab {\n  pane command=\"claude\" {\n    args \"--resume\" \"ghost\"\n  }\n}";
        assert_eq!(live_uuid_union(&s, dump), vec!["ghost".to_string()]);
        assert!(protected_from_dump(&s, dump).is_empty());
    }

    /// Drive `mint_record` — the same record-mint path `run_add` uses — with
    /// only a uuid, filling every other input with the `rec()` shape's
    /// placeholders (irrelevant to the buckets assertions below).
    fn mint_record_under_test(s: &mut Store, uuid: &str) -> AgentRecord {
        mint_record(
            s,
            FreshRecordInputs {
                uuid,
                cwd: "/x",
                repo_root: "/x",
                branch: "main",
                label: "x · main",
                worktree: None,
                default_branch: None,
                own_buckets: None,
            },
        )
    }

    /// Spec: newborn initialisation. A fresh row inherits the opener's
    /// buckets — exact copy, so the tie + position tiebreak lands it
    /// directly below its opener in frecency mode.
    #[test]
    fn a_fresh_row_inherits_the_openers_buckets() {
        let mut s = Store::default();
        let mut opener = rec("u-opener");
        opener.tab_id = Some(4);
        opener.buckets = [(100, 6)].into();
        s.agents.insert("u-opener".into(), opener);
        s.tab_order = [(4, 90)].into();
        let merged = mint_record_under_test(&mut s, "u-new");
        assert_eq!(merged.buckets, [(100u32, 6u32)].into());
    }

    /// Amendment (2026-08-20): a conversation resumed from its transcript
    /// enters at its OWN derived weight — the opener's echo is only for
    /// rows with no history of their own.
    #[test]
    fn own_transcript_buckets_beat_the_openers_echo() {
        let mut s = Store::default();
        let mut opener = rec("u-opener");
        opener.tab_id = Some(4);
        opener.buckets = [(100, 6)].into();
        s.agents.insert("u-opener".into(), opener);
        s.tab_order = [(4, 90)].into();
        let merged = mint_record(
            &mut s,
            FreshRecordInputs {
                uuid: "u-resumed",
                cwd: "/x",
                repo_root: "/x",
                branch: "main",
                label: "x · main",
                worktree: None,
                default_branch: None,
                own_buckets: Some([(99, 3)].into()),
            },
        );
        assert_eq!(merged.buckets, [(99u32, 3u32)].into());
    }

    /// Resume must never overwrite earned history with an inherited copy.
    #[test]
    fn merge_resume_record_keeps_the_existing_rows_buckets() {
        let mut existing = rec("u1");
        existing.buckets = [(99, 42)].into();
        let mut fresh = rec("u1");
        fresh.buckets = [(100, 1)].into(); // whatever add would seed
        let merged = merge_resume_record(Some(&existing), fresh);
        assert_eq!(merged.buckets, [(99u32, 42u32)].into());
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

    /// The liveness fallback survives rotation (#99). A resurrected rotated
    /// pane runs `claude --resume <live-id>`, so the dump names the
    /// CONVERSATION, not the row — read literally it is a uuid nobody has
    /// heard of, and the row it actually belongs to looks dormant.
    #[test]
    fn the_dump_scan_translates_a_rotated_id_back_to_its_row() {
        let mut s = Store::default();
        let mut row = rec("minted");
        row.tab_id = None; // the bind has not landed — this IS the fallback case
        row.live_session = Some("rotated".into());
        s.agents.insert("minted".into(), row);
        let dump = r#"
            layout {
                tab name="a" {
                    pane command="claude" {
                        args "--resume" "rotated"
                    }
                }
                tab name="b" {
                    pane command="claude" {
                        args "--resume" "stranger"
                    }
                }
            }
        "#;
        assert_eq!(
            live_uuid_union(&s, dump),
            vec!["minted".to_string(), "stranger".to_string()]
        );
    }

    /// `clave prune` retires idle rows, and the only thing standing between it
    /// and a row the user is looking at is this predicate. Two ways it goes
    /// wrong, and both delete a live agent's sidebar row:
    ///
    /// - a pane whose conversation ROTATED (`/clear`) is resurrected as
    ///   `claude --resume <live-id>`, so the dump names the conversation and
    ///   not the row — matched on the minted uuid alone it protects nothing;
    /// - a row bound to a tab of a session that is no longer running must NOT
    ///   be protected, or the fleet becomes unprunable forever (#150 review).
    ///
    /// This lived as a closure inside `main.rs` until the 2026-08-15 mutation
    /// sweep, where `main` could be replaced wholesale with `Ok(())` and every
    /// operator inside the closure flipped, all green — `main.rs` has no test
    /// harness beyond its argument-parsing pins. Moved beside its
    /// `live_uuid_union` sibling, which carries the same translation.
    #[test]
    fn the_idle_pruner_protects_a_rotated_pane_and_only_the_running_session() {
        let mut s = Store::default();
        // Open in a tab, conversation rotated: the dump names "rotated".
        let mut spun = rec("u-rotated");
        spun.live_session = Some("rotated".into());
        s.agents.insert("u-rotated".into(), spun);
        // Open in a tab under its own minted id.
        s.agents.insert("u-plain".into(), rec("u-plain"));
        // Bound — but to a tab of a session that has since been killed, so it
        // is absent from the dump. Prunable.
        let mut ghost = rec("u-deadbind");
        ghost.tab_id = Some(4);
        s.agents.insert("u-deadbind".into(), ghost);
        let dump = r#"
            layout {
                tab name="a" {
                    pane command="claude" {
                        args "--resume" "rotated"
                    }
                }
                tab name="b" {
                    pane command="claude" {
                        args "--session-id" "u-plain"
                    }
                }
            }
        "#;
        let protected = protected_from_dump(&s, dump);
        assert!(
            protected.contains("u-rotated"),
            "a rotated pane must be protected under its ROW's uuid, not the \
             conversation id the dump names"
        );
        assert!(protected.contains("u-plain"));
        assert!(
            !protected.contains("u-deadbind"),
            "a bind from a session that is not running protects nothing"
        );
        // An empty dump protects nothing at all — the caller reads a failed
        // `dump-layout` as "no session", and that is the same shape.
        assert!(protected_from_dump(&s, "layout { tab { pane } }").is_empty());
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
            clave_types::RowHeight::Double,
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
        let abs = tab_layout(
            "/data/clave/bin/clave-v0.1.0",
            "/w",
            "l",
            "u",
            "/x",
            false,
            clave_types::RowHeight::Double,
        );
        assert!(abs.contains("command=\"/data/clave/bin/clave-v0.1.0\""));
        assert!(!abs.contains("command=\"clave\""));
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
        row.commit_ord = 88;
        row.tab_id = Some(3); // the DEAD tab that hosted it last time
        row.pane_id = Some(9); // and the dead pane inside it (#178)
        let fresh = rec("u-wt"); // what the weave derives from the PICKED dir
        let merged = merge_resume_record(Some(&row), fresh.clone());
        assert_eq!(merged.status, Status::Idle);
        // The merge preserves the row's ordinal like everything else. `add`
        // then deliberately OVERRIDES it with a freshly minted one (S1 §4.6),
        // because a resume opens a new tab that birth-touches to the top
        // anyway; what must not happen is the merge silently dropping it to 0.
        assert_eq!(merged.commit_ord, 88);
        // The resumed agent lands in a brand-new tab: the old bind is stale
        // by definition — reset, the new tab's bar re-binds on join (§6.6 B).
        // pane_id travels with it: a surviving stale pane_id is #178's class
        // (a row keyed to a pane that no longer exists joins nothing, silently).
        assert_eq!(merged.tab_id, None);
        // And so is the pane inside it. A pane id carried over from a dead
        // tab rides every later snapshot as "this row announced a pane" while
        // no bar can see it — the permanent false stall #178 is hunting, and a
        // uuid-directed jump chases a pane that no longer exists. (cargo
        // mutants 2026-08-15: deleting `pane_id: None` from the merge survived
        // while its `tab_id` twin one line above was pinned.)
        assert_eq!(merged.pane_id, None);
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

    /// #139: the dir picker must reach worktrees zoxide has never seen —
    /// store-known repos' worktrees union in AFTER zoxide's ranked list
    /// (ranking preserved), deduped by exact path, marked when linked.
    #[test]
    fn dir_candidates_unions_worktrees_after_zoxide_and_dedups() {
        let zoxide = vec![
            "/here".to_string(),                         // current dir, always first
            "/repo".to_string(),                         // zoxide knows the main checkout
            "/repo/.claude/worktrees/known".to_string(), // and ONE worktree
        ];
        let worktrees = vec![
            ("/repo".to_string(), false), // main checkout: never marked
            ("/repo/.claude/worktrees/known".to_string(), true),
            ("/repo/.claude/worktrees/unseen".to_string(), true),
        ];
        let c = dir_candidates(zoxide, worktrees);
        assert_eq!(
            c,
            vec![
                ("/here".to_string(), false),
                ("/repo".to_string(), false),
                // zoxide's copy survives (its rank), but gains the mark:
                ("/repo/.claude/worktrees/known".to_string(), true),
                // the one zoxide never saw appends:
                ("/repo/.claude/worktrees/unseen".to_string(), true),
            ]
        );
        // No store repos → exactly the zoxide list, unmarked.
        assert_eq!(
            dir_candidates(vec!["/a".into()], vec![]),
            vec![("/a".to_string(), false)]
        );
    }

    #[test]
    fn dir_lines_round_trip_through_the_picker() {
        assert_eq!(dir_line("/repo/wt", true), "/repo/wt\t(wt)");
        assert_eq!(dir_line("/plain", false), "/plain");
        assert_eq!(picked_dir(&dir_line("/repo/wt", true)), "/repo/wt");
        assert_eq!(picked_dir("/plain"), "/plain");
        // Spaces in paths survive — only the TAB separates the marker.
        assert_eq!(picked_dir("/a b/dir\t(wt)"), "/a b/dir");
        // A zoxide path with a LITERAL tab passes through intact for
        // validate_cwd to reject — truncating at the tab could land in a
        // wrong-but-existing prefix directory (#143 review).
        assert_eq!(picked_dir("/weird\tpath"), "/weird\tpath");
        assert_eq!(picked_dir("/weird\tpath\t(wt)"), "/weird\tpath");
    }

    /// #139 (io half of the union): a REAL repo with a linked worktree,
    /// reached through a store row's repo_root — the main tree unions in
    /// unmarked, the linked worktree marked, nothing else invented. Real
    /// `git` via discovery, exactly like production; identity flags and
    /// signing off so a dev machine's config cannot interfere.
    #[test]
    fn store_worktree_dirs_lists_main_and_linked_from_a_real_repo() {
        let git = tool_path(crate::discover::ToolId::Git);
        let tmp = tempfile::tempdir().unwrap();
        let repo = tmp.path().join("repo");
        std::fs::create_dir_all(&repo).unwrap();
        let repo_s = repo.to_str().unwrap();
        cmd_stdout(&git, &["-C", repo_s, "init", "-q"]).unwrap();
        cmd_stdout(
            &git,
            &[
                "-C",
                repo_s,
                "-c",
                "user.email=t@t",
                "-c",
                "user.name=t",
                "-c",
                "commit.gpgsign=false",
                "commit",
                "--allow-empty",
                "-q",
                "-m",
                "x",
            ],
        )
        .unwrap();
        let wt = tmp.path().join("wt");
        cmd_stdout(
            &git,
            &[
                "-C",
                repo_s,
                "worktree",
                "add",
                "-q",
                wt.to_str().unwrap(),
                "-b",
                "t-wt",
            ],
        )
        .unwrap();

        let mut store = Store::default();
        let mut row = rec("u");
        row.repo_root = repo_s.into();
        store.agents.insert("u".into(), row);

        let dirs = store_worktree_dirs(&git, &store);
        let main_c = std::fs::canonicalize(&repo)
            .unwrap()
            .to_string_lossy()
            .into_owned();
        let wt_c = std::fs::canonicalize(&wt)
            .unwrap()
            .to_string_lossy()
            .into_owned();
        assert!(
            dirs.contains(&(main_c, false)),
            "main tree unmarked: {dirs:?}"
        );
        assert!(
            dirs.contains(&(wt_c, true)),
            "linked worktree marked: {dirs:?}"
        );
        assert_eq!(dirs.len(), 2);
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
    fn origin_head_yields_a_bare_branch_name() {
        // #86: `symbolic-ref --short refs/remotes/origin/HEAD` prints
        // `origin/<branch>`, and provenance compares the result against the
        // record's `branch`, which is bare — leaving the remote on would make
        // EVERY default checkout look like a side branch, the exact bug the
        // field exists to fix.
        assert_eq!(
            strip_origin_prefix("origin/trunk\n").as_deref(),
            Some("trunk")
        );
        assert_eq!(
            strip_origin_prefix("origin/main\n").as_deref(),
            Some("main")
        );
        // Split ONCE: a slashed default survives whole.
        assert_eq!(
            strip_origin_prefix("origin/release/v1\n").as_deref(),
            Some("release/v1")
        );
        // Nothing to read is None, never `Some("")` — an empty default_branch
        // would compare unequal to every real branch and mark the default
        // checkout as a branch.
        assert_eq!(strip_origin_prefix(""), None);
        assert_eq!(strip_origin_prefix("  \n"), None);
        assert_eq!(strip_origin_prefix("origin/"), None);
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

    /// #99's picker half: a rotated row must not be offered TWICE.
    ///
    /// After a `/clear` the live transcript is `<rotated>.jsonl`, whose stem
    /// matches no row, so the scan stood it up as its own candidate — clave
    /// offering a duplicate of a session already open in a tab, and picking it
    /// would give that live agent a second store row. The row's `live_session`
    /// is what links the two, and the fold also takes the live file's mtime,
    /// which is the recency the user actually experienced.
    #[test]
    fn a_rotated_transcript_folds_into_its_row_rather_than_standing_alone() {
        let mut s = Store::default();
        let mut row = rec("minted");
        row.repo_root = "/repo".into();
        row.cwd = "/repo".into();
        row.label = "repo · main · earned".into();
        row.last_interacted = 10;
        row.live_session = Some("rotated".into());
        s.agents.insert("minted".into(), row);

        let dirs = vec![DirScan {
            cwd: "/repo".into(),
            branch: Some("main".into()),
            is_worktree: false,
            // Both files exist on disk: the minted one frozen at the clear, the
            // rotated one still being written.
            stems: vec![("minted".into(), 100), ("rotated".into(), 900)],
        }];
        let c = resume_candidates(&s, "/repo", &dirs, &[]);
        assert_eq!(c.len(), 1, "the rotated stem is the row, not a candidate");
        assert_eq!(c[0].uuid, "minted"); // the row's identity, not the live id
        assert_eq!(c[0].label, "repo · main · earned");

        // …and it sorts on the LIVE transcript's recency: without the fold the
        // row would rank on its frozen stem (100) and sit below a stranger.
        let dirs = vec![DirScan {
            cwd: "/repo".into(),
            branch: Some("main".into()),
            is_worktree: false,
            stems: vec![
                ("minted".into(), 100),
                ("rotated".into(), 900),
                ("stranger".into(), 500),
            ],
        }];
        let c = resume_candidates(&s, "/repo", &dirs, &[]);
        assert_eq!(
            c.iter().map(|c| c.uuid.as_str()).collect::<Vec<_>>(),
            ["minted", "stranger"]
        );
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
            clave_types::RowHeight::Double,
        );
        assert!(kdl.contains("cwd=\"/repo/.claude/worktrees/wt\""));
        assert!(!kdl.contains("cwd=\"/repo\"")); // NOT the picker/root dir
        assert!(kdl.contains("\"--cwd\" \"/repo/.claude/worktrees/wt\""));
    }
}
