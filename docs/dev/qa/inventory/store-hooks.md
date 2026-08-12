# Store/hooks — per-item test specs (S1–S17)

The store is the on-disk JSON (`<state-dir>/agents.json`) that Claude Code
lifecycle **hooks** write and the CLI reads; the sidebar plugin never reads it
directly, it receives full-replace **snapshots** carrying a monotonic `seq`.
Two ids matter everywhere here: the **minted uuid** (the id `clave add`
generates, the store's stable join key) and the **live session id** (the id
Claude reports right now — rotates on `/clear`, not on `--resume`). See
UBIQUITOUS_LANGUAGE.md §1 for the full vocabulary. `<state-dir>` below is
`~/.local/state/clave/` for the stable install and
`$(clave dev instance --field root)/state/` for your per-worktree sandbox;
all reproduction is sandbox-side (`clave dev …`, `scripts/ct.sh …`), never
the maintainer's live session.

### S1 — Session-id rotation orphans the row (#97, #87, #99) [FIELD]
**Seam:** hook payload's `session_id` joined against the store's minted-uuid key — the two silently diverge on `/clear`.
**Preconditions:** one spawned agent whose row exists in the store; sandbox scenario (any `c8-*`) with a live pane, or a seeded row plus manual hook fire.
**Reproduce:**
1. `just sandbox c8-cold-start`; human launches; confirm the row via `clave dev status`.
2. `/clear` in the agent's pane (human gesture — rotates the live session id AND starts a new transcript).
3. Type a prompt in that pane (fires `UserPromptSubmit` with the rotated id).
4. Probe: `jq '.agents["<uuid>"] | {last_interacted, live_session}' <state-dir>/agents.json`.
**Healthy:** `last_interacted` rises on the prompt; `live_session` becomes the rotated id (non-null once they disagree).
**Broken:** `last_interacted` frozen while the transcript appends (measured 5.9 days stale on the maintainer's daily driver, 2026-07-31); status/title/summary never change again.
**Drive assertion:** record `last_interacted` before step 3; within 10s after, assert it increased AND `live_session != null`.
**Guard today:** `hook.rs::resolve_row` env-uuid fallback, unit tests `a_rotated_payload_id_is_remembered_and_an_agreeing_one_clears_it` (hook.rs:1006), `a_rotated_session_id_resolves_only_for_the_agents_own_claude` (hook.rs:1103).
**Refs:** #97 #99 #87; `crates/clave/src/hook.rs:751` (`resolve_row`), `:650-653` (`live_session` write); FOOTGUNS.md "Claude Code ROTATES its session id".

### S2 — Resurrection reopens the pre-rotation conversation (#99) [FIELD]
**Seam:** the resurrection exec (`claude --resume <id>`) vs which conversation that id names — `--resume <superseded-id>` appends to the OLD transcript, it does not re-chain.
**Preconditions:** a row whose `live_session` differs from its minted uuid (i.e. `/clear`ed at least once), its tab closed.
**Reproduce:**
1. From S1's end state (rotated row), close the tab: `Alt+w` (human) or `scripts/ct.sh close-tab` with it focused.
2. Wake the dormant row: select it, `Alt+Enter` (human gesture).
3. Probe the resumed process: `ps aux | grep 'claude --resume'` (in the sandbox this is unambiguous).
**Healthy:** argv shows `--resume <live_session value>`; the resumed agent knows post-`/clear` content (human check).
**Broken:** argv shows `--resume <minted uuid>`; the agent knows only pre-clear content — the old conversation reopened.
**Drive assertion:** `ps -eo args | grep -o 'claude --resume [0-9a-f-]*'` → the id equals the row's `live_session` (jq probe), never the minted uuid when the two differ.
**Guard today:** `spawn.rs::resume_target` (spawn.rs:83) prefers `live_session`; cold restart is the migration for pre-field rows (never backfill — see #106).
**Refs:** #99; `crates/clave/src/spawn.rs:57-89`; FOOTGUNS.md rotation entry; UBIQUITOUS_LANGUAGE.md "live session id".

### S3 — `CLAVE_AGENT_UUID` inherited by nested `claude` [NEAR-MISS]
**Seam:** ambient env identity vs process identity — env set before the `exec` into Claude reaches every descendant, including a `claude` the agent shells out.
**Preconditions:** one live agent pane in the sandbox; its row's current field values recorded.
**Reproduce:**
1. In the agent's pane, ask the agent to run `claude -p 'say hi'` (a nested Claude, inheriting `CLAVE_AGENT_UUID` but re-exporting its own `CLAUDE_PID`).
2. Probe the parent row before/after: `jq '.agents["<uuid>"] | {status, last_interacted, label}' <state-dir>/agents.json`.
**Healthy:** parent row unchanged by the nested run's hooks (fails closed: gate takes the untracked fast path).
**Broken:** nested Claude's hooks write the parent's row — its status, ordering and prose flip on the child's lifecycle.
**Drive assertion:** snapshot the row JSON, run the nested `claude -p`, assert the row bytes are unchanged (allowing only writes attributable to the parent's own events).
**Guard today:** `PidGate` (hook.rs:774-801) — `CLAVE_AGENT_PID` vs Claude's `CLAUDE_PID`, fails closed when either is missing; unit-tested (hook.rs:1073-1103 region).
**Refs:** #97 (caught in review); `crates/clave/src/hook.rs:774` (`PidGate`); FOOTGUNS.md "Env set before an `exec`".

### S4 — Older binary strips unknown store fields (#69, #86, #111)
**Seam:** serde read-modify-write in `with_store_mut` — deserialize drops unknown keys, the whole row re-serializes without them.
**Preconditions:** a store written by a NEWER binary (rows carrying fields the old binary predates, e.g. `title`/`summary`/`live_session`), plus any older `clave-v*` binary that fires a hook or CLI write.
**Reproduce:**
1. Against a scratch store (`CLAVE_STATE_DIR=<tmp>`), write a row with the current binary.
2. Run any store-mutating command with an older release binary (`~/.local/share/clave/bin/clave-v<OLD> hook Stop < payload.json` or similar) against the same `CLAVE_STATE_DIR`.
3. Probe: `jq '.agents["<uuid>"] | keys' <tmp>/agents.json`.
**Healthy:** every field present before step 2 is still present.
**Broken:** earned fields silently gone — chip/summary blank out in the bar; a null `live_session` is then indistinguishable from "never rotated" (#106's exact problem).
**Drive assertion:** key-set diff of the row before/after the old-binary write is empty. Requires an old binary artifact on the machine; without one, detection only: field presence audit across the store after any mixed-version window.
**Guard today:** nothing structural — #106 (versioned store schema) is OPEN. Cold-restart-is-the-migration is the operational rule.
**Refs:** #69 #86 #111 #106; `crates/clave/src/store.rs:246` (`with_store_mut` RMW); FOOTGUNS.md "An OLDER `clave` binary writing the store".

### S5 — Re-record clobbers a worktree cwd
**Seam:** the `Alt+a` resume path re-recording a row it should merge into — replacing a canonicalized, worktree-aware `cwd` with the picked dir.
**Preconditions:** `c8-worktree` scenario (one agent seeded inside a real `git worktree`); its row's `cwd` recorded.
**Reproduce:**
1. `just sandbox c8-worktree`; human launches.
2. `Alt+a` → same repo → resume → pick the worktree agent (human gesture; the picker is a floating pane).
3. Probe: `jq '.agents["<uuid>"].cwd' <state-dir>/agents.json`.
**Healthy:** `cwd` still the worktree path; resumed Claude runs in the worktree; no fresh session created.
**Broken:** row relocates to the repo root; `clave spawn` misses the worktree-keyed transcript and CREATES a fresh conversation.
**Drive assertion:** `cwd` before == `cwd` after the resume pick; `find ~/.claude/projects -name '<uuid>.jsonl' | wc -l` stays 1 (no fresh jsonl minted).
**Guard today:** `merge_resume_record` resets only `status`/`tab_id` (add.rs:461), unit-tested.
**Refs:** `crates/clave/src/add.rs:461` (`merge_resume_record`); FOOTGUNS.md "Re-recording a resumed agent".

### S6 — Data-file lock loses concurrent hook writes
**Seam:** flock target vs atomic-rename — locking the data file locks an inode the rename swaps away, so a second writer holds a lock on a dead file.
**Preconditions:** any store; two or more concurrent writers (hooks race constantly in a real fleet).
**Reproduce:**
1. `CLAVE_STATE_DIR=<tmp>` scratch store with N seeded rows.
2. Fire N store-mutating writes concurrently (e.g. N parallel `clave touch <i>` / hook invocations with `&`, then `wait`). Run from a terminal OUTSIDE any zellij session (S11 — snapshot pushes go to the ambient session).
3. Probe: `jq '.seq, (.tab_order | length)' <tmp>/agents.json`.
**Healthy:** all N updates present; `seq` advanced once per write.
**Broken:** silent lost updates — fewer entries/bumps than writes, no error anywhere.
**Drive assertion:** after `wait`, `seq` delta == N and every written key is present.
**Guard today:** separate never-renamed `agents.lock` (store.rs:226; rationale store.rs:2-9); `ordinals_are_minted_strictly_increasing_under_the_lock` (store.rs:950) pins the in-lock property.
**Refs:** `crates/clave/src/store.rs:2-9`, `:226`, `:246-258`; FOOTGUNS.md "The store lock must be a SEPARATE lockfile".

### S7 — `type:"summary"` label tier never fired (#79) [SHIPPED-DEAD]
**Seam:** transcript-line parser vs what the field actually emits — Claude Code stopped writing `{"type":"summary"}` lines entirely.
**Preconditions:** none — this is a liveness property of the transcript format, not a runtime state.
**Reproduce:** `Repro unknown — detection only:` the failure is an absence. Detection:
1. `grep -rl '"type":"summary"' --include='*.jsonl' ~/.claude/projects | wc -l` → measured 0 of 919 (2026-07-28), 0 of 153 (07-29), 0 of 770 (07-31).
2. Inventory what does exist: `grep -ho '"type":"[a-z-]*"' ~/.claude/projects/*/*.jsonl | sort | uniq -c | sort -rn`.
**Healthy:** row `summary` populated via `away_summary_from_tail` → `ai_title_from_tail` → prompt seed (hook.rs:545-547); the label tier's extinct-line scan is a known-dead fallback, not the live path.
**Broken (the shipped state):** every label a truncated first-prompt fragment; a green test suite asserting a tier that fired 0 times in production.
**Drive assertion:** the dated field measurement above, re-run and recorded with date+counts whenever the tail parsers change (TESTING.md habit 2's field half). Hermetic half: precedence test `ai_title_beats_the_extinct_summary_line_and_the_prompt_seed` (hook.rs).
**Guard today:** `summary` retargeted to `ai_title_from_tail` (hook.rs:249); the LABEL tier still points at the extinct line by explicit deferral (hook.rs:278-283 comment); dated-measurement habit.
**Refs:** #79 #111; `crates/clave/src/hook.rs:249`, `:279`, `:483`; FOOTGUNS.md "Claude transcripts"; TESTING.md shape 5.

### S8 — Transcript relocates when cwd changes (#59, #69) [FIELD]
**Seam:** a cwd frozen at spawn used as the transcript path anchor vs Claude moving the whole `.jsonl` to a project dir keyed on the NEW cwd.
**Preconditions:** one live agent; its conversation's cwd then changes (Claude `cd`s, or a worktree move); a `relocated` line lands in the transcript.
**Reproduce:**
1. Sandbox agent live; note its transcript path: `find ~/.claude/projects -name '<live-id>.jsonl'`.
2. Have the agent change its working directory and complete a turn.
3. Re-run the `find`; probe the row: `jq '.agents["<uuid>"].summary' <state-dir>/agents.json` across two further turns.
**Healthy:** exactly one jsonl hit, under the CURRENT cwd's munged dir; row fields keep rolling.
**Broken:** tail read silently empty (path rebuilt from stale `rec.cwd` + uuid); row frozen with no error.
**Drive assertion:** `find … -name '<id>.jsonl' | wc -l` == 1 AND its parent dir corresponds to the current cwd; `summary`/`last_interacted` advance on the next turn within a bounded wait.
**Guard today:** `resolve_transcript` prefers `payload.transcript_path` (hook.rs:838, field at :59), unit-tested.
**Refs:** #59 #69; `crates/clave/src/hook.rs:59`, `:838`; FOOTGUNS.md "A transcript RELOCATES when the session's cwd changes".

### S9 — Ordering ties resolve on tab position
**Seam:** whole-second timestamps as an ordering key — two commitments inside one wall second had no order, so the row comparator's tiebreak (tab position) won regardless of recency.
**Preconditions:** two agents prompted <1s apart (or two tab touches in one second).
**Reproduce:**
1. Sandbox with 2 live agents; prompt both within one second (scriptable: two hook `UserPromptSubmit` fires back-to-back).
2. Probe: `jq '[.agents[] | {uuid, commit_ord}]' <state-dir>/agents.json`.
**Healthy (current code):** the two rows carry DISTINCT `commit_ord` values — ordinals are minted inside the store lock (`Store::mint_ord`, strictly increasing), so wall-clock ties can no longer occur on the live path. The inventory row's "documented constraint only" understates the current guard; the residue is the one-shot backfill of pre-ordinal rows, which seeds ordinals by sorting `last_interacted` whole seconds (store.rs:597-612) — a tie THERE still resolves arbitrarily, once, at first launch of a new binary over an old store.
**Broken (the original class):** lower-positioned tab wins regardless of who was touched last; a prompt silently swallowed by the sort.
**Drive assertion:** after two same-second prompts, `commit_ord` values are distinct and their order matches fire order.
**Guard today:** in-lock ordinal minting (store.rs:363-381, hook.rs:693); `ordinals_are_minted_strictly_increasing_under_the_lock` (store.rs:950); comparator `rank_desc` (clave-bar model.rs:238).
**Refs:** FOOTGUNS.md "`now_unix()` stamps whole seconds" (its store.rs:352-357 anchor has drifted — that region is now `apply_focus`); `crates/clave/src/store.rs:378`, `:597-612`.

### S10 — evlog re-resolves state dir under `cargo test`
**Seam:** ambient env resolution inside a library path — `log_event` re-reads `$CLAVE_STATE_DIR`/`$HOME` instead of the `StorePaths` the caller holds, so test-context writes land in the LIVE log.
**Preconditions:** a `log_event` call reachable from any unit test; the developer's real `~/.local/state/clave/clave.log` present.
**Reproduce:**
1. `wc -l ~/.local/state/clave/clave.log` (mark).
2. `cargo test --workspace`.
3. `wc -l` again.
**Healthy:** delta 0 — no stray lines in the live evlog (the JSON-lines decision log).
**Broken:** stray test-run lines appended to the maintainer's live log (117 found once), and sandbox events routed to the stable log.
**Drive assertion:** the line-count sandwich above, exact-equal. Cheap enough to run in CI-adjacent tooling.
**Guard today:** convention only — use `log_event_in(&paths.dir, …)` from anything holding a store (evlog.rs:24); ambient `log_event` (evlog.rs:12) reserved for CLI entry points.
**Refs:** `crates/clave/src/evlog.rs:12`, `:24`; FOOTGUNS.md "`evlog::log_event` re-resolves the state dir".

### S11 — `CLAVE_STATE_DIR` sandboxes the store, not the session [FIELD]
**Seam:** store override vs pipe destination — `push_snapshot` pipes into whatever zellij session the process is standing in; the env var only moves the files.
**Preconditions:** a store-mutating clave command run from a shell INSIDE a live clave session, with `CLAVE_STATE_DIR` pointed at a scratch store.
**Reproduce:** do NOT reproduce against the live fleet (it flashed a three-row sandbox fleet onto the maintainer's real sidebar, 2026-08-06). The mechanism is fully deterministic; the safe demonstration is inside the sandbox session itself: from a sandbox pane, run a snapshot-pushing command against a second scratch `CLAVE_STATE_DIR` and watch the sandbox bar repaint with the scratch fleet until the next hook event heals it.
**Healthy:** e2e store checks run with `env -u ZELLIJ -u ZELLIJ_SESSION_NAME …` or from a terminal outside any session — no pipe lands anywhere.
**Broken:** the ambient session's bars render the scratch store's fleet.
**Drive assertion:** procedural, not a probe — every scripted e2e that mutates a store MUST carry `env -u ZELLIJ -u ZELLIJ_SESSION_NAME`; the drive script greps its own command log for a bare store-mutating invocation and fails if one exists. Visual confirmation is HUMAN-ONLY.
**Guard today:** documented only (FOOTGUNS + TESTING); `scripts/ct.sh` clears the ambient session for zellij actions but does not wrap clave CLI calls.
**Refs:** `crates/clave/src/hook.rs:569` (`push_snapshot`); FOOTGUNS.md "`CLAVE_STATE_DIR` sandboxes the STORE"; #149 validation incident.

### S12 — `dev reset` swept scenario transcripts machine-wide [NEAR-MISS]
**Seam:** deterministic scenario uuids (byte-identical `…-c85c<n>` across every sandbox instance) vs a uuid-keyed global delete under `~/.claude/projects`.
**Preconditions:** two per-worktree sandbox instances, both with seeded scenarios (so both own `c85c-*` transcripts in the shared real Claude tree).
**Reproduce:**
1. Seed instance A and instance B (`clave dev scenario c8-cold-start` from two worktrees).
2. Count B's transcripts: `find ~/.claude/projects -name '*c85c*.jsonl' | grep <B's munged repos path> | wc -l`.
3. `clave dev reset` in A.
4. Re-count B's.
**Healthy:** B's count unchanged — reset names the EXACT munged project dirs it finds by walking its own `repos/` tree, nothing else.
**Broken:** every `c85c-*` jsonl on the machine deleted, including other agents' in-flight scenario evidence (prefix-matching munged dirs is also broken: `munge_cwd` is lossy, `…-clave-dev` prefixes `…-clave-dev-wt-a`).
**Drive assertion:** the count sandwich above, exact-equal across another instance's reset.
**Guard today:** `scenario_project_dirs` exact-dir match (dev.rs:892); tests `a_reset_sweeps_only_its_own_instances_transcripts` (dev.rs:1149), `scenario_project_dirs_names_both_plain_and_worktree_cwds` (dev.rs:1121).
**Refs:** `crates/clave/src/dev.rs:892`; FOOTGUNS.md "A sandbox root name is a PREFIX"; #161 (per-worktree instances).

### S13 — `dev reset` deleted the sandbox wasm [SANDBOX]
**Seam:** scenario state vs build artifacts sharing one sandbox root — the old `remove_dir_all(root)` took `data/clave-bar.wasm` with it.
**Preconditions:** a staged sandbox (`just sandbox` has populated `data/`).
**Reproduce:**
1. `test -f "$(clave dev instance --field data)/clave-bar.wasm"` (expect present).
2. `clave dev reset`.
3. Re-test.
**Healthy:** the wasm and generated config in `data/` survive; the reset → scenario → launch loop needs no rebuild.
**Broken:** launch demands a rebuild — the reset silently ate a build artifact.
**Drive assertion:** the wasm file exists after reset (`test -f`, plus `state/` and `repos/` gone).
**Guard today:** reset wipes only `SCENARIO_STATE_DIRS = ["state", "repos"]` (dev.rs:853), test-pinned (dev.rs:1069-1092).
**Refs:** `crates/clave/src/dev.rs:853`, `:1069-1092`; FOOTGUNS.md "`clave dev reset` must wipe only".

### S14 — Scenario worktree branch tags collided [SANDBOX]
**Seam:** branch-tag derivation vs the scenario uuid shape — slicing the FRONT 8 chars of a uuid that always starts `00000000-` gave every scenario agent the same tag.
**Preconditions:** a scenario placing 2+ worktree agents in ONE repo (the `ux-gate1` shape).
**Reproduce:**
1. Seed such a scenario (`clave dev scenario <name>` where the fixture has two worktree agents sharing a repo).
2. Probe: `git -C <state-root>/repos/<repo> branch --list 'clave/*'`.
**Healthy:** one distinct `clave/<tag>` branch per worktree agent (tag from the uuid TAIL, which is the minted `{n:08}` suffix).
**Broken:** second `git worktree add -b clave/00000000` fails closed — "branch already exists"; the scenario seed aborts.
**Drive assertion:** distinct branch count == worktree-agent count; seeding exits 0.
**Guard today:** `uuid_tag` takes the tail (dev.rs:620); test `ux_gate1_worktree_agents_get_distinct_branch_tags` (dev.rs:1530).
**Refs:** `crates/clave/src/dev.rs:620`; FOOTGUNS.md "Scenario worktree branches must NOT be tagged from the FRONT".

### S15 — `--session-id` refuses reuse, breaking re-seed [SANDBOX]
**Seam:** `claude --session-id <uuid>` CREATES and refuses an existing jsonl — so unconditional seeding worked exactly once per scenario, ever.
**Preconditions:** a scenario already seeded once (its `c85c` transcripts exist in `~/.claude/projects`), no reset in between.
**Reproduce:**
1. `clave dev scenario c8-cold-start` (first seed).
2. Run the same command again.
**Healthy:** second run detects the existing jsonl as the GOAL state (`seed_needed` false), skips seeding, exits 0.
**Broken:** `Session ID <uuid> is already in use` — the re-seed fails permanently until a reset.
**Drive assertion:** second invocation exits 0 and its output contains no "already in use"; transcript count unchanged.
**Guard today:** resume-or-create via `seed_needed` (dev.rs:1039), test `seeding_skips_an_already_seeded_session` (dev.rs:1051).
**Refs:** `crates/clave/src/dev.rs:1039`; FOOTGUNS.md "`claude --session-id <uuid>` CREATES a session".

### S16 — `munge_cwd` on non-canonical cwd misses the jsonl
**Seam:** clave's transcript-dir join key vs Claude's — Claude munges the PHYSICAL `getcwd()` (macOS `/var` → `/private/var`, `/tmp` → `/private/tmp`), so an unresolved symlinked cwd derives a different project dir.
**Preconditions:** an agent whose recorded cwd sits under a symlink (`/tmp/...`, `/var/...` on macOS).
**Reproduce:**
1. Seed or record a row with cwd `/tmp/<dir>` (unresolved form).
2. `clave spawn <uuid>` against it.
**Healthy:** cwd canonicalized before munging → the join finds the existing jsonl → resume, not create.
**Broken:** the munged key misses, spawn attempts `--session-id` create against an id whose jsonl exists elsewhere → the same `Session ID … is already in use` error as S15, from a different root.
**Drive assertion:** spawn against a `/tmp`-symlinked cwd exits into a `--resume` (S2's `ps` probe), with no "already in use" emitted.
**Guard today:** canonicalize-before-munge enforced at the call site (main.rs:263-268), rationale in `munge.rs:11`; covered by unit tests around `munge_cwd` (munge.rs:20).
**Refs:** `crates/clave/src/munge.rs:11-20`; `crates/clave/src/main.rs:263-268`; FOOTGUNS.md "`munge_cwd` must be fed the CANONICAL cwd".

### S17 — Out-of-band resume orphans the row: the PidGate has no re-adoption path (#180) [FIELD, OPEN]
**Seam:** `hook.rs::resolve_row`'s two admission routes vs a Claude that clave did not exec — a manual `claude --resume` carries a rotated session id (not a store key) and none of the spawn-set env (`CLAVE_AGENT_UUID`/`CLAVE_AGENT_PID`), so the PidGate fails closed and every hook declines, forever.
**Preconditions:** a spawned agent whose pane Claude was replaced out of band. The field driver is the Ctrl-Z trap: `clave spawn` execs claude as the pane's own process, so a suspended claude has no shell to `fg` it — Ctrl-C plus manual `claude --resume <name>` is the only exit, and it is exactly the resume clave cannot see.
**Reproduce:**
1. Sandbox row live in a tab (any `c8-*` scenario).
2. In the pane: Ctrl-C claude; from the surviving shell run bare `claude --resume <name>` (no clave env).
3. Drive a turn; probe `jq '.agents["<uuid>"] | {context_tokens, last_interacted, status}' <state-dir>/agents.json`.
**Healthy (desired, undesigned):** the row re-adopts the resumed conversation — tokens/status/title resume tracking.
**Broken (current, by design):** the row freezes at its pre-suspend values while the transcript appends; renames fired while orphaned never land. Measured on the daily driver 2026-08-12: frozen at 28,606 tokens while the session ran past 65k.
**Drive assertion:** after step 3, `last_interacted` must rise within 10s — red today; goes green with whatever re-adoption path #180 rules in.
**Guard today:** nothing — the fail-closed gate is CORRECT (it exists so a nested claude can never write the wrong row, S3); what is missing is any sanctioned re-adoption. Validated manual heal: `CLAVE_AGENT_UUID=<uuid> CLAVE_AGENT_PID=$$ exec claude --resume <name>` (exec preserves the pid, the gate passes, and the missed `custom-title` re-stamps from the transcript tail on the first accepted hook).
**Refs:** #180; `crates/clave/src/hook.rs:751` (`resolve_row` + its "worst case is the old freeze" doc), `:774` (`PidGate`); S1 (the guarded rotation class this is the residual of); S3 (why the gate must stay closed); FOOTGUNS.md title re-stamp measurement.
