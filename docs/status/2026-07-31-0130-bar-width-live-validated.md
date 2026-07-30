# Status — the sidebar reaches its target width, and four bugs are filed

_2026-07-31 · branch `feat/bar-width-clip`, **PR #90 open**, gates green, 280
tests, mutation clean, every commit Ollie-signed. Run `git log --oneline
main..HEAD` for the real count; a number here goes stale on the next commit._

## Read this first

**`docs/ux/LEDGER.md` is the authority, not this file.** It carries 37 numbered
decisions with their reasoning and overrides any spec. This handoff is
orientation; the ledger is the record. Read its **operating rule**, then
**D31–D37** (everything this session decided), then the **task table** at the
bottom — the only statement of what has shipped.

Runnable target render, look at it before designing anything:

```bash
cargo run -p clave-bar --example bar-preview
```

**Ollie's goal now: get to another version release quickly**, building out the
remaining pieces. That framing should drive triage — prefer finishing what is
started over opening new fronts.

## Your role

**Standing coordinator and principal engineer for the UX workstream.** Subagents
implement; you brief them, review each result, absorb what it finds, brief the
next. The operating rule that broke a four-session loop and must not be relaxed:

> **Specs are an OUTPUT, not an input.** Nothing under `docs/superpowers/specs/`
> gets amended during the build. Discoveries land in the ledger. Subagents **may
> read** the specs; your overrides travel in their brief.

## FIRST THING TO DO — the PR review comments

**PR #90 has four unresolved review threads** from `chatgpt-codex-connector` and
`coderabbitai`. Triage them before anything else; two are quick, and one is a
genuine defect in a test this session wrote. **Fix and reply before resolving —
never silent-resolve.** My assessment, which you should verify rather than trust:

1. **Codex, P1, `crates/clave/src/setup.rs:868` — VALID, fix it.** A test calls
   `launch_layout_kdl` (the TTY-reading wrapper) and asserts the fallback
   percent. It passes only because `cargo test` has no TTY. **Under an
   interactive PTY it fails** — at 142 columns the real function emits 38%, not
   27%. The fix is one word: call `launch_layout_kdl_for(..., None, false)`, the
   parameterised form that exists precisely for this. I flagged this exact
   hazard in my own commit message and then left one site un-migrated.
2. **CodeRabbit, `docs/dev/LIVE-INTERACTION-CHECKLIST.md:275` — I believe this
   is WRONG; verify then reply, do not accept.** It says the expanded floor is
   27, but `min_intact_cols() = 13 + title + repo`, and D33 moved `EXPANDED` to
   `(9, 7)`, so the floor is **29**. 27 was the pre-D33 value. Check
   `min_intact_cols_is_thirteen_plus_title_plus_repo` in `render.rs` — it
   asserts 29 — then reply with that reference.
3. **CodeRabbit, `docs/dev/TESTING.md:588` — probably VALID.** The golden was
   renamed to `golden_bar_at_fifty_four_columns` by a `sed`, but the surrounding
   derivation may still say 44 and a 17-cell summary. Read the passage; if it
   does, fix the arithmetic too.
4. **CodeRabbit, `FOOTGUNS.md:49` — VALID and cheap.** The `dump-layout` entry
   says it reports an MCP child *unconditionally*; it should say *may*. Without
   MCP children the dump can legitimately show the agent process, and the
   absolute phrasing would make someone dismiss a valid dump.

`CodeRabbit` **reports `pass` while rate-limited** (FOOTGUNS, #68). Read the
check detail, not the colour.

## What shipped this session

Five commits on `feat/bar-width-clip`, plus a merge of `main`. Each was verified
at a real terminal, not only by tests.

| commit | what |
|---|---|
| `4df8fb6` | **The wrap fix (D31).** Rows are clipped to the pane instead of over-running into a terminal that wraps them. |
| `2f8dd5a` | **Expanded 44 → 54 (D33).** Profile `(9, 7)`, summary 25. |
| `332116d` | **Birth from the real terminal (D35).** What makes 54 actually reachable. |
| `e508b7a` | **Birth respects the collapse mode (D36)** + the partial seek gate (D37). |
| `6713bc0` | Docs: link the residual flash to #89. |

**The headline result, measured live:** expanded now rests at **53 columns with a
24-cell summary** on Ollie's display. Before this branch it was **17 cells**. The
44 → 54 change alone delivered *nothing* — see D34.

## The two findings that justify the whole live-testing exercise

**Both were invisible to every automated tier in the repo.**

- **D34 — a target is a suggestion, not a width.** The seek accepts anything
  within half a resize step, and reachable widths are a lattice anchored at
  birth. Moving the target 44 → 54 left the bar resting at 47 on a 280-column
  display, so the widened title bought *two fewer* summary cells than before.
  Caught by measuring through the real `width_seek`, not by reading.
- **D36 — birth ignored the collapse mode.** The mode persists in the store, so
  a fleet left collapsed was born at 54 and shrank to 30. **No test could have
  caught it**: the mode was not an input to the function, so there was nothing to
  vary, nothing to fail, and no mutant to kill. A human at a terminal was the
  only instrument that worked.

## Open bugs, all filed with full reasoning

| # | what | state |
|---|---|---|
| **#89** | Sidebar flashes wide at launch when the store says `collapsed: true` | Reduced by D37, **not fixed**. Next step is named in the issue. |
| **#91** | New agents born with a title chip containing clave's own label | Diagnosed to the exact mechanism; three candidate fixes weighed. |
| **#92** | Unread green clears on a fly-by; wants a dwell | Ollie's call, shape described. |
| **#93** | `/rename` not reflected until the next prompt | Cosmetic, self-healing. |

**#89 and #91 are the two worth fixing before a release.** #91 is a visible
correctness bug on every new agent; #89 is cosmetic but happens constantly.

**On #89, do NOT guess a fourth time.** Two rounds were lost reasoning about
render ordering instead of measuring it. The issue names the discriminating test:
dump the layout immediately on tab birth and again once settled, and see whether
the pane's *percent* changes. If it does not, nothing resized it and no
plugin-side gate can ever fix it — which would change the fix entirely.

## Live validation — what passed, what was never run

Driven in the `ux-gate1` sandbox across three monitors and several widths.

**Passed:**

- **Item 1, collapse/expand** — six-plus toggles, clean every time, across three
  display widths. D21's bug is dead and D26's fix holds live.
- **Item 5, navigation** — wrap both ways, explicit picks open immediately,
  walk-through leaves rows dormant, the dead row shows `↻` then `✗` with no new
  tab, nav survives `Alt+w` (#23 stays fixed), mouse works both ways.
- **Item 3, the hook** — status transitions, summary tier 3, `/rename`, and
  `/clear` not blanking the chip (#24). Tier 1 correctly did not fire: that
  session had **0 `ai-title` lines**, so there was nothing to show.
- **No wrapping** in either profile across three monitors. D31 confirmed live.

**Never run — these are what a release still owes:**

- **Item 4, provenance.** Part (b) needs a repo whose default branch is neither
  `main` nor `master`, created fresh. It wants a `zoxide add` so `Alt+a` can
  reach it; Ollie has not yet okayed that, so **ask before running it**.
- **Item 7, terminal tabs**, including the recycled-tab-id sort trap.
- **Item 6, resize drift.** Partially exercised by accident and it healed, but
  never run deliberately.

## Known limitation, confirmed live and not yet fixed

**Task 7b′ — `clave open` / `add` tabs are born on the fiction.** They build
one-shot `new-tab --layout` files that bypass the session template *and* run
inside zellij, so the TTY is the wrong width. Measured on a live session: the
launch tab's bar sits at **28%** (terminal-derived) while every dwell-opened tab
sits at **27%** (`BAR_BIRTH_PERCENT`), so they rest one column apart and the
difference is visible when switching tabs.

The fix: the bar passes its own `get_tab_info().display_area_columns` — a
synchronous host call, verified present in `zellij-tile-0.44.3/src/shim.rs:307` —
down as a CLI argument. That is a CLI-surface change, so the risk taxonomy wants
a `Cli::try_parse_from` pin plus one sandboxed **debug** run.

## Traps this session paid for

- **`zellij action dump-layout` reports a pane's MCP CHILD, not the agent.** A
  tab running `clave spawn` dumped as `uv … whatsapp-mcp-server`. Reads exactly
  like clave generated the wrong layout; `launch.kdl` was correct. Cost a round.
  Now in FOOTGUNS.
- **A correct mechanism at the wrong call site passes every gate.** D37's gate
  was first armed in the `PermissionRequestResult` arm, but `load()` only
  *requests* permission — the grant is an event and zellij renders before it. Both
  D36 and D37's misses lived in `clave-bar/src/main.rs`, which is `test = false`
  and mutation-excluded. **This is an argument for moving decisions OUT of
  `main.rs`, not for adding tests to it.**
- **A fixture is not a specification** (again — this is D23's lesson recurring).
  #91 exists because every title test used a fixture whose `custom-title` was a
  user rename, so nothing exercised the value clave itself writes at spawn.
- **Seeded scenario statuses are fiction.** `ux-gate1` writes `needs_you`,
  `failed`, `working` straight into the store with no hook involved, so a red dot
  on a seeded row says nothing about reality. Only a genuinely new agent tests
  the hook→status path.
- **Instrument before theorising.** `docs/dev/TESTING.md`'s instrumentation
  recipe exists for exactly this and reaching for it after the FIRST failed
  hypothesis would have been cheaper than the second.

## Rules that are not negotiable

- **Never kill, launch, or run a bare `zellij` command**, never `just
  dev-install` / `just release` / `dev launch`, never write
  `~/.local/state/clave/` or anything under `~/.claude/`. `just sandbox` is
  yours to run; the session lifecycle is Ollie's — **print, never run**.
- **`just sandbox` runs `dev reset` first**, which wipes the store. If a bug
  needs specific store state (e.g. #89 needs `collapsed: true`), **rebuild and
  copy the wasm into the sandbox data dir instead** — that is the documented
  instrumentation path and it preserves state.
- **`cargo test --workspace`, always.** Bare `cargo test` skips tests and exits 0.
- **GLYPH RULE:** every non-ASCII glyph in Rust source and test literals is a
  `\u{...}` escape.
- **Ollie signs every commit** — `git commit` pauses on a 1Password prompt. Wait
  for it. Never `--no-gpg-sign`. Prefer `git merge` over `git rebase` when
  updating the branch: a rebase re-signs every commit and makes him clear one
  prompt each.
- **The repo is PUBLIC.** No home-directory paths, transcript content or personal
  data. The pre-commit PII hook does not cover `gh`.
- **Fix review findings and reply before resolving. Never silent-resolve.**

## Suggested order from here

1. **Clear PR #90's four review threads** (above). One is a real test defect.
2. **Fix #91** — visible on every new agent, and the mechanism is fully
   diagnosed, so it is a contained change.
3. **Finish the live checklist** — items 4, 7, 6. Ollie drives; you read
   observability and never puppet the session.
4. **Merge #90**, then decide whether #89 blocks the tag. It is cosmetic, so
   probably not — but it is constant, so it may be worth the diagnostic first.
5. **Then D28's release sequence**: confidence in the testing, `just release`,
   switch the daily driver. His hands for the last two.

Task 7b′, S5 (store-backed ink allocation — the reason most chips are blue), and
S4 proper remain queued after that.
