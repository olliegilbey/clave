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
pub fn open_is_live(row: &AgentRecord, dump_layout: &str) -> bool {
    row.tab_id.is_some() || crate::add::live_uuids(dump_layout).contains(&row.uuid)
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

pub fn run_open(uuid: &str) -> Result<()> {
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
    let output = std::process::Command::new("zellij")
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
    let is_live = open_is_live(row, &dump);
    let cwd_exists = std::path::Path::new(&row.cwd).is_dir();
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
            // Guard the stored cwd before baking it into KDL (see
            // add::validate_cwd) — a `"`/control char breaks the layout.
            crate::add::validate_cwd(&row.cwd)?;
            let wasm = crate::setup::wasm_path()?;
            let binary = crate::release::runtime_binary();
            let label = crate::add::sanitize_label(&row.label);
            let layout = crate::add::tab_layout(
                &binary,
                wasm.to_str().context("wasm path")?,
                &label,
                uuid,
                &row.cwd,
            );
            let tmp = std::env::temp_dir().join(format!("clave-open-{uuid}.kdl"));
            std::fs::write(&tmp, layout)?;
            let status = std::process::Command::new("zellij")
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
    fn open_is_live_prefers_the_store_bind_over_command_scan() {
        // Issue #6: the STORE bind is the authoritative liveness signal —
        // row.tab_id is Some iff the bar has it bound to a live (un-pruned)
        // tab. A bound row reads live even when the command scan is BLIND (the
        // MCP-child serialization that made live_uuids miss a live agent and
        // dwell-open spawn a duplicate tab). The dump-layout scan stays only as
        // an ADDITIVE fallback for a non-MCP agent whose bind hasn't landed.
        let mut bound = rec("u1");
        bound.tab_id = Some(3);
        assert!(open_is_live(&bound, "layout { tab { pane } }")); // bind wins, scan empty
        let unbound = rec("u2"); // tab_id None
        assert!(!open_is_live(&unbound, "layout { tab { pane } }"));
        // Fallback: an unbound row still visible in dump-layout (non-MCP agent,
        // bind lag) reads live via the command scan (args on its own line, the
        // real dump-layout shape live_uuids parses).
        let dump = "tab {\n  pane command=\"claude\" {\n    args \"--resume\" \"u2\"\n  }\n}";
        assert!(open_is_live(&unbound, dump));
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
