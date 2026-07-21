# Status — clave orchestrator (FOUNDATION + ALL SPIKES COMPLETE; next = subsystem plan)

_2026-07-03 13:44 · repo github.com/olliegilbey/clave (public) · branch `main` · tree clean_

Predecessor handoff (S0/S0b/S1 detail + design provenance):
@docs/status/2026-07-02-1257-clave-orchestrator.md — read only if you need the
pre-S2 spike detail or the brainstorm/fugu-review history. Most is now executed as code.

## Task Overview
Build **clave**: a terminal-native orchestrator for multiple Claude Code agents in a
dedicated Zellij session; one agent = one Zellij tab running the real Claude TUI; a
first-party WASM plugin (`clave-bar`) renders a left sidebar (repo-grouped,
recency-sorted, colour-coded status glyph) from clave's **pushed** model.

**This thread finished the foundation+spikes plan.** It executed the last task
(Task 6 / spike **S2**) via superpowers:subagent-driven-development, ran the
human-in-the-loop interactive validation, then did the SDD **final whole-branch
review** + applied its fixes. **All four gating spikes (S0, S0b, S1, S2) PASS** →
the idempotency join key AND the plugin architecture are proven. **The foundation+
spikes phase is DONE.** The next phase is the **subsystem plan** (a fresh
`/superpowers:writing-plans` pass) — no code from that phase exists yet.

## Reference Docs
- **SDD progress ledger** `.superpowers/sdd/progress.md` — **READ THIS FIRST.**
  Gitignored scratch (won't show in `git status`, still on disk). Per-task record,
  the S2 findings, the final-review outcome, and carried-forward decisions.
- **Canonical spec** `docs/superpowers/specs/2026-06-30-clave-orchestrator-design.md`:
  - §2 invariants (~:67–105), §4 verified knowledge base incl. canonicalize-cwd (~:129–229),
  - **§6 subsystem specs (~:274–438) — all Decided; the NEXT plan builds these.**
  - §9 spike plan (~:492–540) — S0/S0b/S1/S2 all done.
- **Spike S2 findings** `docs/superpowers/spikes/S2.md` — the nav mechanism + all
  S2 gotchas (permissions, keybind vs CLI pipe, focus_pane_with_id). Feeds §6.6.
- **Foundation+spikes plan** `docs/superpowers/plans/2026-07-01-clave-foundation-and-spikes.md`
  — now fully executed; Global Constraints (~:13–29) still bind the workspace.

## Current State
Tree clean; everything committed to `main`. `git status` shows only `.claude/`
(intentional untracked) and the predecessor status file (untracked). NO uncommitted
code. Verify with `git status` / `git log`.

Commits this thread (newest→oldest):
- `dd38ace` fix(clave-bar): unblock CLI pipe on every path + `cargo fmt` (final-review fixes).
- `46f1eed` spike(s2): **PASS** — uuid→pane→focus join validated (focus_pane_with_id).
- `a17c505` spike(s2): author-only (go_to_tab, pending) — superseded by 46f1eed's approach.

Key production files (all reviewed, tests green, fmt clean):
- `crates/clave/src/{main.rs,lib.rs,munge.rs}` (+ `examples/munge.rs`) — CLI shell + `munge_cwd` join key (3 tests).
- `crates/clave-types/src/lib.rs` — `Status`/`Agent`/`AgentSnapshot`/`Register` pipe schema (4 tests).
- `crates/clave-bar/src/main.rs` — the plugin, FINAL S2 shape: `load` requests
  `ReadCliPipes`+`ChangeApplicationState`, no event subscription; `pipe()` →
  `handle_pipe()` (clave-status render-gate, clave-register→uuid_to_pane,
  clave-nav→`focus_pane_with_id(Terminal(pane_id))`) then unconditional
  `unblock_cli_pipe_input`; `render()` colored glyphs; NO `fn main`.
- Spike artifacts: `spikes/{s0-create-and-munge.sh, s1-msgs/*.json, s2-register.sh,
  layouts/{s1.kdl,s2.kdl}, s2-config.kdl}`; `docs/superpowers/spikes/{S0-S0b,S1,S2}.md`.
- `justfile`, workspace `Cargo.toml` (resolver 3, edition 2024, default-members excludes clave-bar).

**7/7 tests pass; `cargo fmt --all --check` clean; `cargo build -p clave-bar --target wasm32-wasip1` clean.**

## Important Discoveries
(S2's — ordered by cost to re-learn. S0/S0b/S1 discoveries are in the predecessor file + ledger.)

1. **Nav is `focus_pane_with_id(PaneId::Terminal(pane_id), false, false)`, NOT `go_to_tab`.**
   The plan's `go_to_tab(pane_to_tab[pane]+1)` was called with the CORRECT value from a
   real keybind (attached-client) context and was a **silent no-op** (0-/1-based tab-index
   mismatch — not a client-context issue; that was ruled out by driving it from a keybind).
   `focus_pane_with_id` focuses the registered TERMINAL pane directly; Zellij pulls its tab
   forward. It needs ONLY `uuid→pane_id`, so `pane_to_tab`/`PaneUpdate`/`ReadApplicationState`
   were all dropped (user chose "simplify"). **§6.6 nav must use focus_pane_with_id.**
2. **Zellij plugin permissions are ALL-OR-NOTHING per plugin.** Requesting a set grants
   only if the cache holds the ENTIRE set under the plugin's location key; a partial match
   raises a prompt (UNANSWERABLE in a narrow bar pane) and withholds ALL → every `zellij pipe`
   times out (`Action CliPipe did not complete within 1s`). Fix = pre-seed
   `~/Library/Caches/org.Zellij-Contributors.Zellij/permissions.kdl` with the EXACT set
   under BOTH `"file:<abs>.wasm"` AND `"<abs>.wasm"` key forms. Current on-disk grant for
   clave-bar = `ReadCliPipes ChangeApplicationState` (both keys). **`clave setup` (§6.5/§6.8/§7)
   MUST seed this — do not rely on the prompt.**
3. **Production nav = a `MessagePlugin` KEYBIND** → `pipe()` with `source=Keybind` (attached-
   client context). `spikes/s2-config.kdl` demonstrates it (Ctrl+y, launched via
   `zellij --config`). The `zellij pipe --name clave-nav` shell form (`source=Cli`) also works
   but is only a spike driver.
4. **`zellij pipe` triggers a harmless CLI-only flood** — every invocation logs `CliPipe did
   not complete within 1s timeout` + `1000 consecutive unknown messages … logging client out`,
   even WITH `unblock_cli_pipe_input`. The effect (register/nav) still happens; the KEYBIND path
   is completely clean. Not a plugin bug — a `zellij pipe` artifact. Did NOT chase further.
   `unblock_cli_pipe_input(pipe_id)` was kept because it makes `zellij pipe` return to the prompt.
5. **Layout needs a visible tab strip for validation** — clave-bar draws no chrome, so a minimal
   layout shows no tabs/focus. Added `zellij:tab-bar` (built-in, auto-permitted, `is_plugin` so
   filtered) PER-TAB in `spikes/layouts/s2.kdl` (per-tab, not `default_tab_template` which is
   parse-fragile — S1). **Launch with `-n`, NOT `--layout`** (with `--layout`, `--session` means
   "add tabs to an EXISTING session" and errors if absent).
6. **Debugging technique that worked:** `eprintln!` in a zellij plugin lands in the zellij log
   (`$TMPDIR/zellij-<uid>/zellij-log/zellij.log`, `CLAVE-DBG` prefix). This is how the join was
   proven from the plugin's side. (All debug eprintlns were removed for the final commit.)

**Failed approaches (do NOT retry):**
- `go_to_tab((tab_index)+1)` and `go_to_tab(tab_index)` — go_to_tab from a plugin did not focus
  the expected tab regardless of the client context (keybind or CLI). Use focus_pane_with_id.
- Assuming the interactive permission prompt is answerable in the bar pane — it is not; pre-seed
  the cache instead (this cost S1 many rounds and re-bit S2 when the requested set changed).
- Chasing the "1000 unknown messages" flood as a plugin bug — it is a `zellij pipe` CLI artifact,
  irrelevant to the production keybind path.

## Next Steps
1. **Resume: `cat .superpowers/sdd/progress.md` FIRST**, then spec §6. Trust the ledger + `git log`;
   do NOT re-run any spike or re-dispatch Tasks 1–6.
2. **Write the SUBSYSTEM plan** (fresh `/superpowers:brainstorming` if needed, then
   `/superpowers:writing-plans`) in spec dependency order:
   `clave spawn` (§6.1, uses canonicalize+munge_cwd) → state store + `ls` (§6.2) →
   `hook` + status state machine (§6.5) → **full `clave-bar` bar (§6.6)** → `add` (§6.3) →
   naming (§6.4) → archiving (§6.7) → **session/config + keybinds (§6.8)**.
3. **Fold these into the plan (from S2):**
   - §6.6 nav uses `focus_pane_with_id(Terminal(pane_id))` + a `MessagePlugin` keybind (§6.8).
   - §6.6 sidebar rows should be **mouse-clickable** to switch to that agent's tab (user request).
   - §6.5/§6.8/§7 `clave setup` must seed `permissions.kdl` (exact set, both key forms).
   - §6.6 may re-add `PaneUpdate`/`ReadApplicationState` for dropping CLOSED agents from the bar
     (NOT for nav).
   - The clave-bar `pipe()` handlers should gain `eprintln!` logging on dropped/malformed payloads
     (deferred Task 6 minor).
4. **Deferred minors** (in ledger, low-risk, sweep during subsystem work): Task 1 crate-manifest
   "why" comments; Task 2 clave-types doc/test asymmetry. (Task 3 doc + Task 6 fmt already fixed.)

**Where work stopped — verbatim last exchange:**
> **User:** "commit then /handoff"

(Immediately prior, I asked whether to commit the two final-review fixes — the clave-bar
unconditional-unblock fix + a workspace `cargo fmt` — then hand off. The user approved both.
Both are now committed in `dd38ace`.)

## Context to Preserve
- **User prefs:** extremely concise, signal over noise; explain while doing; **MORE code comments
  than normal** (heavily-commented, the *why*); conventional commits; **commit messages end with
  `Claude-Session: https://claude.ai/code/session_<id>`** — each executing agent uses ITS OWN
  session URL. **Ask before commits/PRs and before architecture decisions with multiple valid
  approaches** (this thread: asked before every commit and before the "simplify plugin" call).
  Greybeard shell/dev tone.
- **Commit signing:** commits are SSH-signed via 1Password (`op-ssh-sign`). If a commit fails with
  `1Password: failed to fill whole buffer`, the agent is LOCKED — ask the user to unlock 1Password,
  then retry (staging is preserved).
- **Branch policy:** solo public repo → commit straight to `main` (confirmed). Public repo: no
  secrets, no machine-specific abs paths in committed code — EXCEPT spike layouts/config under
  `spikes/` (the sanctioned exception; `s2.kdl`/`s2-config.kdl` carry the absolute wasm path).
- **SDD staging discipline:** stage EXPLICIT paths (never `git add -A`). `.claude/` is deliberately
  untracked (open question: should clave ship with superpowers enabled? don't commit without asking).
  `.superpowers/` is gitignored scratch. Stage `Cargo.lock` only when deps actually change.
- **Dual-repo (for §6.5/§6.8):** `~/.claude` is a symlink → `~/dotfiles/src/.claude`; editing
  `~/.claude/settings.json` edits the dotfiles source. `clave setup` should do additive/idempotent merges.
- **Env (verified):** Zellij 0.44.3 (homebrew, single binary), Claude 2.1.197, rustc 1.96.1,
  `wasm32-wasip1` installed, zellij-tile 0.44.3, fzf/zoxide/jq present. Pre-commit secret-scan
  (gitleaks/trufflehog/ripsecrets) runs on every commit.
- **Re-running S2** (if ever needed): `zellij --config "$PWD/spikes/s2-config.kdl" -s clave-s2
  -n "$PWD/spikes/layouts/s2.kdl"`, then Ctrl+y (keybind) or `zellij pipe --name clave-nav --
  '{"uuid":"u2"}'`; teardown `zellij delete-session clave-s2 --force`. permissions.kdl already seeded.

## Restart Hint
Tree clean, all committed, all four spikes pass, final review done + fixes applied — **safe to
/clear.** Resume: read `.superpowers/sdd/progress.md` + spec §6, then start the SUBSYSTEM plan
(brainstorm → writing-plans) in §6 dependency order. No spike or Task 1–6 rework needed.
