//! `clave open <uuid>` (§6.3, C8 2026-07-17): the non-interactive sibling of
//! `add` — open a known store row's tab. Invoked by the bar's executor
//! instance when a dormant row's focus settles (0.4s dwell) or on an
//! explicit pick (click / Alt+N). No picker: the row IS the choice.

use anyhow::{Context, Result};

use crate::store::AgentRecord;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OpenDecision {
    /// uuid already in dump-layout: do nothing (double-fire guard #2 —
    /// the bar's in-flight set is #1; `live_uuids` can transiently miss a
    /// mid-tool-call agent, §10, so BOTH exist).
    AlreadyLive,
    /// Row cwd missing on disk (deleted worktree / moved repo): no tab;
    /// the caller records `stale` so the bar shows ✗. Recovery is manual.
    Stale,
    /// Create the tab (baked idempotent spawn → jsonl check resumes).
    Open,
}

/// Liveness for `clave open` (issue #6): the STORE bind is authoritative —
/// `row.tab_id` is Some iff the bar has it bound to a live (un-pruned) tab —
/// with the dump-layout command scan kept only as an ADDITIVE fallback for a
/// non-MCP agent whose bind hasn't landed. Command parsing alone went blind
/// under MCP servers (zellij serializes the `uv … run main.py` child, not
/// `claude`, C7 corollary 2026-07-21), so a live agent read as dead and a
/// dwell-open spawned a DUPLICATE tab. The intended dwell path opens a DORMANT
/// row (tab_id None), so the bind check is a pure safety add there.
///
/// The scan goes through `add::resolve_scan_token`'s canonicalization (#226),
/// which folds in the two translations this function used to do by hand and
/// one it could not: a rotated pane's `--resume <live-id>` lands on its row
/// (#99), and a name-resume's `--resume <title>` lands on the title's
/// most-recently-interacted holder — read literally either would call a live
/// agent dormant and dwell-open a SECOND tab on it, the double-attach this
/// function was written to prevent. `add::live_uuid_union` is the same
/// translation for the picker.
pub fn open_is_live(store: &crate::store::Store, row: &AgentRecord, dump_layout: &str) -> bool {
    // `pane_id` counts too (#226): an adopted session is registered by its
    // first hook and bound only when the owning bar's join lands — keying on
    // the bind alone would read the row dead inside that window.
    row.tab_id.is_some()
        || row.pane_id.is_some()
        || crate::add::resolved_scan_uuids(store, dump_layout).contains(&row.uuid)
}

pub fn open_decision(_row: &AgentRecord, is_live: bool, cwd_exists: bool) -> OpenDecision {
    if is_live {
        OpenDecision::AlreadyLive
    } else if !cwd_exists {
        OpenDecision::Stale
    } else {
        OpenDecision::Open
    }
}

/// `collapsed` comes from the BAR (D36): the new tab must be born in the mode
/// the fleet is in, or it flashes wide and then snaps. A hand-run `clave open`
/// passes nothing and is born expanded.
pub fn run_open(uuid: &str, collapsed: bool) -> Result<()> {
    let paths = crate::store::store_paths()?;
    let store = crate::store::read_store(&paths)?;
    let Some(row) = store.agents.get(uuid) else {
        crate::evlog::log_event("open", &format!("{uuid}: unknown uuid"));
        anyhow::bail!("clave open: unknown uuid {uuid}");
    };
    // All zellij invocations are EXPLICITLY session-scoped (§6.9 / the
    // sanctioned-commands rule): run_command children inherit the server's
    // env, but never bet on ambient state.
    let session = crate::env::session_name();
    // Discovered path (codex P2 on PR #29): open runs via the bar's
    // run_command, whose env may lack the interactive PATH.
    let zellij = crate::discover::tool_path(crate::discover::ToolId::Zellij);
    let output = std::process::Command::new(&zellij)
        .env("ZELLIJ_SESSION_NAME", &session)
        .args(["action", "dump-layout"])
        .output();
    // A COMMAND failure (spawn error or non-zero exit) must be loud, not
    // silently read as "no tabs live": swallowing it makes `is_live` false
    // for a genuinely live agent and risks spawning a duplicate tab, which
    // (unlike a bail here) is not retryable from the bar's dwell timer. A
    // SUCCESSFUL-but-empty dump is different and expected — a bar-less
    // session or the §10 mid-tool-call miss both legitimately read as
    // empty — so only command failure bails; empty success flows through.
    let dump = match output {
        Ok(o) if o.status.success() => String::from_utf8_lossy(&o.stdout).into_owned(),
        Ok(o) => {
            let stderr = String::from_utf8_lossy(&o.stderr);
            crate::evlog::log_event(
                "open",
                &format!("{uuid}: dump-layout exited non-zero: {stderr}"),
            );
            anyhow::bail!("clave open: dump-layout failed: {stderr}");
        }
        Err(e) => {
            crate::evlog::log_event("open", &format!("{uuid}: dump-layout spawn failed: {e}"));
            anyhow::bail!("clave open: dump-layout spawn failed: {e}");
        }
    };
    // Issue #6: bind-first liveness (dump-layout scan is the additive
    // fallback) — `open_is_live` fixes the MCP-blind duplicate-tab spawn.
    let is_live = open_is_live(&store, row, &dump);
    // A missing baked cwd is not stale when the conversation demonstrably
    // MOVED somewhere spawnable (#139, #143 review): the removed-worktree
    // wake was otherwise rejected here before spawn's relocation recovery
    // could ever run. Open only decides tab creation; spawn re-runs the
    // search and repoints the row.
    let relocated = crate::env::claude_config_dir()
        .ok()
        .and_then(|d| crate::spawn::relocated_cwd(&d, &row.uuid, row.live_session.as_deref()));
    let cwd_exists = std::path::Path::new(&row.cwd).is_dir() || relocated.is_some();
    match open_decision(row, is_live, cwd_exists) {
        OpenDecision::AlreadyLive => {
            crate::evlog::log_event("open", &format!("{uuid}: already live, no-op"));
            Ok(())
        }
        OpenDecision::Stale => {
            crate::evlog::log_event("open", &format!("{uuid}: cwd missing → stale"));
            if let Some(snap) = crate::store::apply_open_result(&paths, uuid, true)? {
                crate::hook::push_snapshot(&snap);
            }
            Ok(())
        }
        OpenDecision::Open => {
            // Bake the cwd the pane can actually RUN from (#143 review): a
            // recovered relocation must not bake the missing row.cwd — the
            // pane would be born in a dead dir before `clave spawn` could
            // ever follow the move.
            let open_cwd = relocated.as_deref().unwrap_or(&row.cwd);
            // Guard the baked cwd before it reaches KDL (see
            // add::validate_cwd) — a `"`/control char breaks the layout.
            crate::add::validate_cwd(open_cwd)?;
            let wasm = crate::setup::wasm_path()?;
            let binary = crate::release::runtime_binary();
            let label = crate::add::sanitize_label(&row.label);
            // The SESSION's own mode, not the store's (finding 2, #232
            // review) — see add.rs's identical read for the full rationale:
            // a mid-session `clave rows` write only takes effect at the next
            // launch, so a tab minted now must match the plugin identity the
            // running config.kdl's keybinds already address.
            let row_height = crate::setup::session_row_height(&crate::setup::data_dir()?);
            let layout = crate::add::tab_layout(
                &binary,
                wasm.to_str().context("wasm path")?,
                &label,
                uuid,
                open_cwd,
                collapsed,
                row_height,
            );
            let tmp = std::env::temp_dir().join(format!("clave-open-{uuid}.kdl"));
            std::fs::write(&tmp, layout)?;
            let status = std::process::Command::new(&zellij)
                .env("ZELLIJ_SESSION_NAME", &session)
                .args([
                    "action",
                    "new-tab",
                    "--layout",
                    tmp.to_str().context("tmp")?,
                ])
                .status()?;
            let _ = std::fs::remove_file(&tmp);
            anyhow::ensure!(status.success(), "zellij action new-tab failed");
            crate::evlog::log_event("open", &format!("{uuid}: tab created (resume via spawn)"));
            // A previously-stale row that opens fine heals (§5).
            if let Some(snap) = crate::store::apply_open_result(&paths, uuid, false)? {
                crate::hook::push_snapshot(&snap);
            }
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::{AgentRecord, LabelSource};
    use clave_types::Status;
    use std::collections::BTreeMap;

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
            metered_at: 0,
            buckets: BTreeMap::new(),
            model: None,
            provider: None,
            effort: None,
            pr_number: None,
            pr_checked: 0,
            pr_branch: String::new(),
        }
    }

    #[test]
    fn open_is_live_prefers_the_store_bind_over_command_scan() {
        // Issue #6: the STORE bind is the authoritative liveness signal —
        // row.tab_id is Some iff the bar has it bound to a live (un-pruned)
        // tab. A bound row reads live even when the command scan is BLIND (the
        // MCP-child serialization that made live_uuids miss a live agent and
        // dwell-open spawn a duplicate tab). The dump-layout scan stays only as
        // an ADDITIVE fallback for a non-MCP agent whose bind hasn't landed.
        let mut s = crate::store::Store::default();
        let mut bound = rec("u1");
        bound.tab_id = Some(3);
        s.agents.insert("u1".into(), bound.clone());
        assert!(open_is_live(&s, &bound, "layout { tab { pane } }")); // bind wins, scan empty
        let unbound = rec("u2"); // tab_id None
        s.agents.insert("u2".into(), unbound.clone());
        assert!(!open_is_live(&s, &unbound, "layout { tab { pane } }"));
        // Fallback: an unbound row still visible in dump-layout (non-MCP agent,
        // bind lag) reads live via the command scan (args on its own line, the
        // real dump-layout shape live_uuids parses).
        let dump = "tab {\n  pane command=\"claude\" {\n    args \"--resume\" \"u2\"\n  }\n}";
        assert!(open_is_live(&s, &unbound, dump));
        // …and the fallback survives ROTATION (#99): the resurrected pane runs
        // `--resume <live-id>`, which is not the row's uuid. Read literally,
        // this unbound-but-live row would be dwell-opened a SECOND time.
        let mut rotated = rec("u3");
        rotated.live_session = Some("rot-3".into());
        s.agents.insert("u3".into(), rotated.clone());
        let dump = "tab {\n  pane command=\"claude\" {\n    args \"--resume\" \"rot-3\"\n  }\n}";
        assert!(open_is_live(&s, &rotated, dump));
        assert!(
            !open_is_live(&s, &rec("u9"), dump),
            "a stranger's id is not this row"
        );
    }

    /// #226: `claude --resume DJ` resolves a NAME — the dump token must reach
    /// the title's most-recently-interacted holder, and ONLY that holder: the
    /// newest reads live (no dwell-open double-attach in the pre-hook window),
    /// while an older row sharing the title stays dead and resumable.
    #[test]
    fn open_is_live_resolves_a_name_resumed_pane_to_the_newest_title_holder() {
        let mut s = crate::store::Store::default();
        let mut dj_old = rec("u-old");
        dj_old.title = Some("DJ".into());
        dj_old.last_interacted = 100;
        s.agents.insert("u-old".into(), dj_old.clone());
        let mut dj_new = rec("u-new");
        dj_new.title = Some("DJ".into());
        dj_new.last_interacted = 200;
        s.agents.insert("u-new".into(), dj_new.clone());
        let dump = "tab {\n  pane command=\"claude\" {\n    args \"--resume\" \"DJ\"\n  }\n}";
        assert!(open_is_live(&s, &dj_new, dump));
        assert!(
            !open_is_live(&s, &dj_old, dump),
            "older holder stays resumable"
        );
    }

    /// #226: a hook-registered pane is live BEFORE the bar's bind lands —
    /// `open_is_live` keyed on `tab_id` alone would dwell-open a second tab
    /// on an adopted session inside the register→bind window.
    #[test]
    fn open_is_live_counts_a_registered_pane_before_the_bind_lands() {
        let mut adopted = rec("u4");
        adopted.pane_id = Some(12); // registered, not yet bound
        let mut s = crate::store::Store::default();
        s.agents.insert("u4".into(), adopted.clone());
        assert!(open_is_live(&s, &adopted, "layout { tab { pane } }"));
    }

    #[test]
    fn open_decision_is_noop_for_live_stale_for_missing_cwd() {
        // §6.3 clave open guards, in priority order:
        // 1. liveness no-op — dwell-timer/click double-fire protection
        //    (second guard; the bar's in-flight set is the first).
        // 2. staleness — missing cwd (deleted worktree) → no tab, bar ✗.
        let r = rec("u1");
        assert_eq!(open_decision(&r, true, true), OpenDecision::AlreadyLive);
        assert_eq!(open_decision(&r, true, false), OpenDecision::AlreadyLive);
        assert_eq!(open_decision(&r, false, false), OpenDecision::Stale);
        assert_eq!(open_decision(&r, false, true), OpenDecision::Open);
    }
}
