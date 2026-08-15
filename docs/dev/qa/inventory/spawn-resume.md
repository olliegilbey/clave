# Spawn/resume/resurrection — per-item test specs (R1–R4)

This plane covers the paths that turn a store row back into a running Claude:
the `Alt+a` resume **picker** (a floating pane listing resumable conversations
for a repo), the dormant-row **wake** (`Alt+Enter`, the only gesture that
launches a dormant row), and **resurrection** (relaunching a fleet's
conversations after the zellij session died). Vocabulary — **minted uuid**
vs **live session id**, **agent session** vs **zellij session** — is in
UBIQUITOUS_LANGUAGE.md §1. Reproduction is sandbox-side against your
per-worktree instance (`clave dev instance`; zellij actions via
`scripts/ct.sh` only); session launch/kill and every picker gesture are the
human's.

### R1 — Resume-picking an on-screen session opened it twice (C7 r8)
**Seam:** the picker's candidate list vs zellij pane truth — a conversation already open in a tab was offered as resumable, so picking it spawned a second tab on the same uuid.
**Preconditions:** one agent LIVE in a tab; same repo pickable via `Alt+a`. Liveness detection must survive two liars: `dump-layout` serializes a pane's deepest child (MCP/LSP, or `<defunct>` pre-C7), and a rotated row is only joinable via `live_session`.
**Reproduce:**
1. `just sandbox c8-cold-start`; human launches; one agent resumed live.
2. `Alt+a` → same repo → resume (human gesture): the picker lists candidates.
3. Pick the row marked live.
**Healthy:** the live candidate carries the `▶` marker (add.rs:845); picking it JUMPS to the existing tab — no new tab, no second `claude` process on that uuid.
**Broken:** same uuid in two tabs — two panes appending to one conversation.
**Drive assertion:** before/after the pick: `scripts/ct.sh list-panes -t -j` tab count unchanged; `ps -eo args | grep -c "claude --resume <live-id>"` == 1; store `tab_id` for the row unchanged. The pick itself is a human gesture (picker is interactive), so the drive stages and probes; the press is HUMAN.
**Guard today:** picker marks live rows `▶` and pick-jumps (C7 r8 fix); rotated sessions join the live set via `live_session` (`add.rs::resume_candidates` :310, `add.rs::live_uuid_union` :85, `open.rs::open_is_live`).
**Refs:** SUBSYSTEM-VALIDATION.md C7 (lines "Findings 2026-07-14, round 8"); `crates/clave/src/add.rs:85`, `:310`, `:845`; #99 (the rotation blind spot); #100 (the second-tab commit path).

### R2 — Serialized `--session-id` would collide on resurrection (C8) [NEAR-MISS]
**Seam:** zellij session serialization vs Claude's session-id semantics — serialization replays the DISCOVERED pane command, and a replayed `claude --session-id <uuid>` refuses the existing jsonl (creates are create-only, S15's same refusal).
**Preconditions:** a clave session with agent tabs, killed and resurrected — IF serialization were on. Also note zellij serializes the deepest child, so with MCP servers the replayed command might not even be `claude` (Z10/Z11).
**Reproduce:** `Repro unknown — detection only:` the class is designed out rather than guarded at runtime; reproducing it would require hand-enabling serialization. Detection that the guard holds:
1. `grep 'session_serialization false' "$(clave dev instance --field data)/config.kdl"` → exactly one hit.
2. Cold-start the `c8-cold-start` scenario: kill+relaunch (human), then confirm the most-recent agent resumes focused with history and dormant rows sit `○` — resurrection is clave-owned and lazy, not zellij-replayed.
**Healthy:** config carries `session_serialization false`; cold start resumes via `clave spawn` → `claude --resume`.
**Broken:** a resurrected pane re-runs a serialized `claude --session-id` and fails against its own transcript.
**Drive assertion:** the config grep above (== 1), plus post-relaunch `ps` shows `--resume`, never `--session-id`, on resurrected panes.
**Guard today:** serialization OFF in every generated config (setup.rs:183), pinned by `config_disables_session_serialization` (setup.rs:1136); lazy clave-owned resurrection (C8 redesign, 2026-07-17).
**Refs:** SUBSYSTEM-VALIDATION.md C8 (redesign preamble + serialization findings); `crates/clave/src/setup.rs:183`, `:1136`; FOOTGUNS.md "zellij serializes the LIVE discovered pane process".

### R3 — Harness-injected prompt earned a permanent bad label (#17) [FIELD]
**Seam:** `UserPromptSubmit` payload text vs its author — resuming a session that died with pending background work makes the Claude harness auto-fire a turn whose "prompt" is an orphaned task-notification tag, and earned labels stick forever.
**Preconditions:** a resumable conversation that ended with pending background tasks (so its first resumed turn is harness-injected, not human); the row's label not yet earned.
**Reproduce:** hard to stage deterministically — the injection depends on harness state at death. Detection + hermetic guard carry this one:
1. Hermetic: the prefix blocklist test drives both earn paths — `every_injected_prefix_is_blocked_on_both_earn_paths` (hook.rs:1968).
2. Live detection: after adopting/resuming any session, probe `jq '.agents["<uuid>"].label' <state-dir>/agents.json`.
**Healthy:** label derives from a real user prompt; a harness-shaped first turn earns NOTHING (the guard never strips-and-uses the remainder — it refuses the whole line).
**Broken:** the tab name and row label read task-notification boilerplate forever — no self-heal, one leak bakes it.
**Drive assertion:** after every scripted resume in a drive, assert the label does not start with any entry of `HARNESS_INJECTED_PREFIXES` (mirror the list, or shell out to a tiny probe); full reproduction stays HUMAN-adjacent.
**Guard today:** `HARNESS_INJECTED_PREFIXES` start-of-string match (hook.rs:108-121), unit-tested on both earn paths.
**Refs:** #17; `crates/clave/src/hook.rs:108`, `:1968`; FOOTGUNS.md "Harness-injected text reaches `UserPromptSubmit`".

### R4 — Waking a remote-controlled conversation claims it locally (2026-08-11) [FIELD]
**Seam:** `claude --resume` vs a conversation currently driven from another surface — resume claims the conversation for the local process, and the remote controller is disconnected.
**Preconditions:** a store row whose conversation is at that moment being driven remotely (e.g. claude.ai remote control of the same session); the row rendered dormant locally.
**Reproduce:** noted as a side-finding on the #178 drive, not yet isolated. `Repro unknown — detection only:` the shape is: select the remotely-driven row, `Alt+Enter` (wake) → local `claude --resume <live-id>` starts → the remote control session drops. Staging it requires a second controlling surface, which only the human has — HUMAN-ONLY end to end.
**Healthy (desired, undesigned):** a wake either detects the remote claim and refuses/warns, or coexists — no ruling exists yet.
**Broken (current, by default):** the remote controller disconnects the moment the local resume lands; no local signal that it happened.
**Drive assertion:** `HUMAN-ONLY: the human drives the same conversation from a remote surface, wakes the row locally, and observes whether the remote session survives.` Nothing store-side distinguishes a remotely-driven row today.
**Guard today:** nothing — recorded on the v0.1.3 Part C drive handoff; no issue of its own yet (noted alongside #178).
**Refs:** docs/status/2026-08-11-1830-v013-part-c-drive.md ("Side-finding"); #178 (the drive it surfaced on); `crates/clave/src/spawn.rs:83` (`resume_target`, the claiming path).
