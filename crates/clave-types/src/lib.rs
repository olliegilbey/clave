//! Shared pipe schema between the `clave` binary and the `clave-bar` plugin.
//! serde-only and target-agnostic (compiles for host AND wasm) — this is the
//! anti-drift guarantee (invariant #9): both artifacts serialize the SAME
//! structs.

use serde::{Deserialize, Serialize};

/// Per-agent status. This is a *latest-wins state machine* (spec §6.5), not a
/// priority-max: a later event can downgrade an earlier one.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Status {
    Idle,
    Working,
    NeedsYou,
    Done,
    Failed,
}

/// One agent row as the plugin renders it. Mirrors the store record's
/// display-relevant fields (spec §5); the plugin never sees the store, only
/// this snapshot.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Agent {
    /// Minted session UUID — the join key (invariant #3).
    pub uuid: String,
    pub cwd: String,
    /// git toplevel of `cwd`; the grouping key in the bar.
    pub repo_root: String,
    pub branch: String,
    /// `cwd · branch · summary` (spec §6.4).
    pub label: String,
    pub status: Status,
    /// unix seconds; bumped on UserPromptSubmit → drives recency sort.
    pub last_interacted: u64,
    /// unix seconds; bumped on focus → `unread = done && !visited`.
    pub last_visited: u64,
    pub archived: bool,
}

/// The full-replace snapshot `clave` pushes to `clave-bar` on every change
/// (spec §5 pipe contract). `seq` is monotonic; a consumer applies only the
/// highest `seq` it has seen and discards stale/out-of-order messages.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentSnapshot {
    pub seq: u64,
    pub agents: Vec<Agent>,
}

/// The `clave-register` payload a pane's `clave spawn` pipes to the plugin so it
/// can map uuid → pane_id → live tab position (spec §6.1 / spike S2).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Register {
    pub uuid: String,
    pub pane_id: u32,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn status_serializes_as_spec_snake_case() {
        // Exactly the strings the spec (§5/§6.5) mandates.
        assert_eq!(serde_json::to_string(&Status::Idle).unwrap(), "\"idle\"");
        assert_eq!(serde_json::to_string(&Status::Working).unwrap(), "\"working\"");
        assert_eq!(serde_json::to_string(&Status::NeedsYou).unwrap(), "\"needs_you\"");
        assert_eq!(serde_json::to_string(&Status::Done).unwrap(), "\"done\"");
        assert_eq!(serde_json::to_string(&Status::Failed).unwrap(), "\"failed\"");
    }

    #[test]
    fn status_deserializes_from_snake_case() {
        let s: Status = serde_json::from_str("\"needs_you\"").unwrap();
        assert_eq!(s, Status::NeedsYou);
    }

    #[test]
    fn snapshot_roundtrips() {
        let snap = AgentSnapshot {
            seq: 7,
            agents: vec![Agent {
                uuid: "u1".into(),
                cwd: "/Users/x/code/clave".into(),
                repo_root: "/Users/x/code/clave".into(),
                branch: "main".into(),
                label: "clave · main · hello".into(),
                status: Status::Working,
                last_interacted: 1000,
                last_visited: 0,
                archived: false,
            }],
        };
        let json = serde_json::to_string(&snap).unwrap();
        let back: AgentSnapshot = serde_json::from_str(&json).unwrap();
        assert_eq!(snap, back);
    }

    #[test]
    fn register_roundtrips() {
        let reg = Register { uuid: "u1".into(), pane_id: 42 };
        let back: Register = serde_json::from_str(&serde_json::to_string(&reg).unwrap()).unwrap();
        assert_eq!(reg, back);
    }
}
