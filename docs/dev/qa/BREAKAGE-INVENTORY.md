# The breakage inventory — index

_Compiled 2026-08-12 from FOOTGUNS.md, TESTING.md, the SUBSYSTEM-VALIDATION
C-sections, RELEASE-RUNBOOK.md, the status archive, and the full GitHub issue
history. **101 distinct breakage classes**, each expanded in its plane file
below into a self-contained spec (seam · preconditions · reproduce · healthy ·
broken · drive assertion · guard · verified refs) an engineer can build a test
from without hunting other docs. This is the evidence base for
[QA-DRIVE.md](../QA-DRIVE.md). Add new classes to the plane file AND bump the
counts here; anchors in the plane files were verified 2026-08-12 (many
FOOTGUNS anchors had drifted — trust the plane files)._

| Plane | Items | File |
|---|---|---|
| Store/hooks | S1–S17 | [inventory/store-hooks.md](inventory/store-hooks.md) |
| Pipe delivery (register/bind/nav) | P1–P18 | [inventory/pipe-delivery.md](inventory/pipe-delivery.md) |
| Bar model/render | B1–B22 | [inventory/bar-render.md](inventory/bar-render.md) |
| Zellij tab-pane truth | Z1–Z15 | [inventory/zellij-truth.md](inventory/zellij-truth.md) |
| Spawn/resume/resurrection | R1–R4 | [inventory/spawn-resume.md](inventory/spawn-resume.md) |
| PATH/version coherence (release) | V1–V17 | [inventory/version-coherence.md](inventory/version-coherence.md) |
| Keybinds/config | K1–K8 | [inventory/keybinds-config.md](inventory/keybinds-config.md) |

Markers used throughout: **[FIELD]** hit the maintainer's live fleet ·
**[SANDBOX]** live in a dev sandbox · **[NEAR-MISS]** shipped-shaped, caught
in review/CI.

## Currently unguarded and open

- **P9 / #178** — wake binds never land beyond a session's early tabs
  (v0.1.3 held on it; QA-DRIVE phase 2 is its harness). Scope extended
  2026-08-12: the resume/auto-heal path fails identically.
- **S17 / #180** — out-of-band resume orphans the row; the PidGate is
  correctly fail-closed but has no re-adoption path (Ctrl-Z/no-`fg` is the
  field driver; validated manual heal in the spec).
- **B22 / #181** — width runaway recurs on v0.1.3 (~90%), on a tab in
  #178's failure state; causality vs coincidence is the open question.
- **B4 / #153** — external mover walks the bar off target.
- **V6 / #48** — doctor is blind to PATH; runbook Steps 1–3 manual because
  of it.
- **P8 / #45** — pipe noise, unguarded AND load-bearing: the EOF-twin drop
  lines are the only proof of broadcast pipe delivery.

## Corrections found during the 2026-08-12 verification pass

- **K2** guard was overstated: the layout-as-config-layer fix is a ruling
  only — #114 is open, `launch_session` still passes `--config`, and the
  "7/7 asserts" were a deleted scratch probe. The guardrail is owed.
- **S9** is now structurally closed on the live path: ordinals are minted
  inside the store lock (`Store::mint_ord`, test-pinned); residue is only
  the pre-ordinal backfill sort.
- **K3** is conditional: the hot-reload-drops-overlay class exists only once
  #114's layout route lands; today the slot belongs to K4.
- **B14** is semi-automatable after all: the birth touch lands in the store,
  so a `dev status | jq` recency probe machine-checks it.
- **P15**'s field renamed: `tab_timeline` → `tab_order` (ordinals).
- "#128" in P3/P5 is a **PR** (for issue #100), not an issue.
- The twins ambiguity is resolved in the specs: EOF-twins are corroborating
  telemetry, never *failure* evidence — and, since #182, never *delivery*
  evidence either. They are empty-payload control drops carrying no session
  identity in a user-global log, so the count arithmetic corroborates at best;
  delivery is gated on the payload's own observable (the store bind). P1's
  discriminator is no focus change AND no log line, twins present either way.

## Known-liar detectors (never trust these alone)

- `dump-layout` width — normalises to 33%/67% whatever the live geometry.
- `dump-screen` — empty for plugin panes.
- The model's own `cols` belief — not pane truth. A maintainer screenshot is
  the only reliable width oracle.
- "pass / Review rate limited" (CodeRabbit) — reviewed nothing.
- Empty grep output — meaning is the probe's, not the emptiness's: nothing
  from `clave_unversioned` is the expected PASS, nothing from `clave_versions`
  is a STOP. Same blank line, opposite verdicts, which is why a report has to
  say which probe printed nothing.
- `dropped … pipe with empty payload` — a control present in health, not
  evidence of failure (#162's day-long misdiagnosis).
