<!--
This is a VERIFICATION DOSSIER, not a summary. `main` is guaranteed green,
reviewed and hermetically verified — it is NOT guaranteed live-validated, so the
maintainer needs to know exactly what was checked, by what, and what was left
unchecked. Delete nothing: if a section does not apply, write "n/a" and why.

Taxonomy and tiers: docs/dev/TESTING.md. Agent contract: AGENTS.md.
-->

## What changed

<!-- The change in a few lines, and WHY. Link the issue: Closes #___ -->

Closes #

**Risk class(es)** from the taxonomy (tick all that apply — they are cumulative):

- [ ] Pure logic / model
- [ ] Generated artifacts (`config.kdl` / `layout.kdl` / `launch.kdl`)
- [ ] CLI surface (new subcommand or flag)
- [ ] Cross-process / IPC (pipes, plugin shellouts, multi-writer store paths)
- [ ] Install / environment (release mechanics, dev-install, `PATH`, doctor) → label `needs-live-validation`
- [ ] Visual / UX (glyphs, colours, widths, fonts) → label `host-untestable`

## Verified automatically

The three gates, with actual results (paste the summary lines, not "passing"):

| Command | Result |
|---|---|
| `cargo test --workspace` | <!-- e.g. 164 passed; 0 failed --> |
| `cargo build -p clave-bar --target wasm32-wasip1` | |
| `cargo clippy --workspace --all-targets -- -D warnings` | |

`--workspace` is load-bearing: a bare `cargo test` silently skips every
`clave-bar` model test and still exits 0.

**Class-specific evidence** (required by the taxonomy row(s) ticked above):

- Tests added, red-first? <!-- name them; say which failed before the fix -->
- Proptests extended if a new branch became reachable? <!-- or why not -->
- Generated artifacts: parser guardrail + version-coherence / path-existence
  assertions? <!-- name the test -->
- CLI: `Cli::try_parse_from` pin + one sandboxed **debug** end-to-end run?
  <!-- paste the command -->
- Cross-process: the ordering / idempotency argument
  <!-- state it here: what two writers can arrive out of order, and why the
       payload is safe under any interleaving -->

## Review lanes run

| Lane | Ran? | Findings |
|---|---|---|
| Vendored fugu review (`.claude/commands/fugu-review.md`) | | |
| Independent adversarial reviewer | | |
| CodeRabbit CLI (`coderabbit review --committed --base main`) | | |
| Other (Codex, PR bots) | | |

At least one lane must be **independent of whoever wrote the code** — on this
repo external lanes have caught defects the implementer and a single reviewer
both missed (the prune ordering race, the clap `ArgAction` bug, the stale
width-seek anchor).

**Findings DECLINED**, with reasoning:

<!-- One line each. A declined finding recorded is fine; a declined finding
     dropped is not. -->

## Could NOT be verified, and why

<!-- Be explicit and specific. "Tier 2 does not exist yet (#47), so nothing
     asserts that a real session ends up with one bar per tab" is the shape.
     This is the section that earns trust — an empty one is a red flag on any
     change touching a seam. -->

## Live steps for the maintainer

<!-- REQUIRED if `needs-live-validation`. Numbered, sandbox-first
     (`clave dev reset` → `clave dev scenario <name>` → the human runs
     `clave dev launch`), each step with its EXPECTED OBSERVATION. Never ask the
     maintainer to launch or kill a session on your behalf — print the command.
     Remember `Alt+w` is the tab-close path, not Ctrl+D. -->

1.
2.

## Handoff and links

- Status handoff: `docs/status/YYYY-MM-DD-HHMM-clave-orchestrator.md`
  <!-- tracked per the #22 ruling; it rides this PR -->
- Related issues / ledger sections touched:
