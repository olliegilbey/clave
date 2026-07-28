# AgentSnapshot v2 — the wire shape, and four transcript findings

_2026-07-28 · resolves **#69** — but by **ruling two of its five fields out**
(§2.2), not by landing all five · **corrects** S4 §3.2, where a stated invariant
is false · obligates amendments to S4, S5, S6 and the design lock (§4)_

Two deliverables, one document:

1. **The wire shape** (§2) — the merged `AgentSnapshot` five workstreams each
   proposed independently. Genuinely unowned; this spec owns it and ships it.
2. **Four findings from a live spike** (§3) — measured against the maintainer's
   own transcript tree. One of them **falsifies a hard invariant in S4**, and
   promotes an optimisation S4 deferred into a correctness requirement.

## 1. What this owns, and what it deliberately does not

**Owns:** the structural fields on `Agent` / `AgentRecord`, their defaults,
their exclusions, and a one-time backfill. Delivered as one small, inert PR that
**unblocks S5 and S6**.

**Does not own: liveness.** `2026-07-22-S4-label-rename-and-live-cwd.md` already
designs it across 1787 lines — the `LabelSource` state machine (§3.3), live cwd
(§3.2), a `head.rs` branch reader (§4.2), a test plan (§5) and a live-validation
SOP (§6) including the exact `/clear` case. **S4 remains the liveness owner.**
This spec hands it corrections (§4), not a replacement.

That boundary is the whole point of #69, applied to this document as much as to
the workstreams:

> "we need to build the plumbing under the hood that more than one feature
> relies on, so that they are built from a shared place - rather than having
> each agent working on a feature building its own implementation."

---

## 2. The wire shape

`clave_types::Agent` gains three fields, each `#[serde(default)]`:

| field | type | why |
|---|---|---|
| `title` | `Option<String>` | design-lock §7.1 structural need; `None` = never renamed, the majority case |
| `summary` | `String` | the 17-column field; today reachable only by parsing `label` |
| `worktree` | `Option<String>` | S6 §2.4 — already stored at `store.rs:54`, simply never projected |

`store::AgentRecord` gains `title` and `summary`; it already has `worktree`.
`snapshot_from` (`store.rs:167`) projects all three — it is the single producer,
and `apply_snapshot` in `clave-bar/src/model.rs` the single consumer.

**Why structural rather than parsed.** Design-lock §2 replaces one composed
string with three fixed-width columns (title 7 · repo 7 · summary 17), and §7.1
rules that a live row renders from the store. A single blob cannot be slotted
into three columns without the bar reverse-engineering it by splitting on ` · `
— the mechanism §7.1 deleted. The host sends the values instead.

### 2.1 Why `#[serde(default)]`

Not legacy compatibility. Serde requires every key to be **present**; absence is
a whole-document parse failure, not an empty value. Without it, the first run of
the new binary against the existing `~/.local/state/clave/agents.json` — which
has no `summary` key — fails to parse and clave sees **zero agents**. The fleet
index vanishes on upgrade.

It also covers the skew that matters in practice: a running zellij session keeps
the bar it loaded at launch, so a mid-session release means a new CLI writing to
an old bar. The maintainer restarts his dogfood session willingly, but **the dev
sandbox hot-reloads the bar without relaunching** — the same skew, in the place
the project iterates.

Everything heavier is out — no version field, no migration framework. Ruling:

> "if there is a worry about breaking older clave sessions, don't worry about
> that till we have got to a full v1."

### 2.2 What is deliberately excluded

Both exclusions share one shape: **a field whose name promises what its values
cannot yet deliver.**

- **`inks` / `repo_ink` / `title_ink`.** Design-lock §7.1 deletes `InkSpan`
  outright; the replacement is two palette *indices*. But those come from S5's
  four-map store ledger, and `u8` has no "unset" — `0` is a real palette entry
  (crystalBlue). Shipping them early paints every row one colour until S5 lands.
  **S5 lands the fields together with the ledger that fills them.**
- **`tab_timeline` → `tab_order`, `commit_ord`.** S1 §3.6 rejects
  rename-without-semantics explicitly: the rename exists *to discard* unix-second
  values (~1.7 × 10⁹) that would outrank every ordinal forever under a max-merge.
  It is inseparable from `mint_ord` and the `clear_session_order` backfill.
  **S1 lands it.**

### 2.3 Backfill

`summary` is seeded once where empty, by splitting the existing label —
`label.splitn(3, " · ").nth(2)` — inside `clear_tab_timeline`'s existing locked
RMW (`store.rs:340`, called from `setup.rs:708`; S1 renames it
`clear_session_order`). `splitn(3)` holds even when the summary text itself
contains ` · `.

Precedent: S1 §3.6 item 3 does the same for its ordinals — idempotent,
self-limiting, ~10 lines, matching nothing once it has run.

**Stated honestly: that call site fires at session *create*, not at every
binary upgrade.** A maintainer who upgrades mid-session sees blank summaries on
dormant rows until his next session launch. Accepted: the alternative is a
migration hook on every store open, which is more machinery than a cosmetic gap
on rows nobody is currently using justifies.

**Why it is not made redundant by S4's liveness work.** S4 refills any agent that
receives a hook event. **Dormant rows receive none, by definition** — without
the backfill they sit in the dormant list with a blank 17-column field
indefinitely.

This parses a composed label **once, host-side, to escape the string** — the
opposite of the per-render parsing §7.1 deleted.

---

## 3. The spike

The maintainer proposed testing against his own tree: this session is titled
`CLA-MAIN` and has been `/clear`ed several times, so the transcript should show
what `/clear` does to a summary. It answered that and three things nobody asked.

All findings measured 2026-07-28 on the maintainer's machine; §3.6 reproduces
them.

### 3.1 FINDING — the `summary` tier is extinct and has never fired

`summary_from_tail` (`hook.rs:116`) scans for `{"type":"summary", …}`. That line
type appears in **0 of 919 transcripts** under `~/.claude/projects/`.

The live store confirms the consequence:

```
agents: 14      label_source: {'first_prompt': 14}
  first_prompt | clave · main
  first_prompt | clave · main · There was a session
  first_prompt | clave · main · You're picking up clave
  first_prompt | issue-10-kdl-guardrail · - · Okay, it's really broken.
```

**Fourteen of fourteen rows are `FirstPrompt`. No agent has ever reached
`LabelSource::Summary`.** Every label in the live sidebar is a truncated prompt
fragment — exactly the labelling the summary tier was written to replace.

Two consequences:

1. A **live bug independent of #69**. Clave's logic is not wrong; it listens for
   a line type Claude Code stopped writing.
2. S4 §1.3 frames the obstacle as *"`Summary` freezes the label forever"*. The
   freeze is real but **has never executed in production**. The tier's *source*
   is the defect; the freeze is downstream of a state nothing reaches.

### 3.2 FINDING — what Claude actually writes

Line types across the 40 newest transcripts:

| type | count | content |
|---|---|---|
| `custom-title` | 1057 | the user's rename — `{"customTitle":"CLA-MAIN"}` |
| **`ai-title`** | **373** | **Claude's rolling description — `{"aiTitle":"Get approval to proceed"}`** |
| `last-prompt` | 1340 | `{"lastPrompt":…}`, maintained live |
| `worktree-state` | 384 | `worktreePath`, `worktreeName`, `worktreeBranch`, `originalCwd` |
| `relocated` | 131 | `{"relocatedCwd":…}` |
| `agent-name` | 564 | subagent identity |
| `summary` | **0** | extinct |

`ai-title` is the natural replacement for the extinct tier, and it is what the
maintainer independently predicted:

> "claude often computes a recap, it does it automatically every now and then,
> or I can invoke it with forward-slash 'recap' - that recap might be a good
> field to hold onto, and it might actually be better to use in the text of the
> summary when it exists."

It exists, it is automatic, and it is already in the transcript. **S4's tier
table does not know about it.**

### 3.3 FINDING — transcripts RELOCATE, so S4 §3.2's invariant is false

S4 §3.2 states, as a hard invariant:

> "Claude keys the transcript directory on the cwd the session was **created**
> in and never moves it. Repointing `rec.cwd` would make the hook tail-read a
> file that does not exist — killing the summary tier and the new title tier at
> the same stroke."

**This is false.** This session began in `/Users/olliegilbey/code/clave`. After
entering a worktree, its transcript is at
`projects/-Users-olliegilbey-code-clave--claude-worktrees-agentsnapshot-v2/<uuid>.jsonl`,
and `find` returns **nothing** for that uuid under the original project
directory. The file moved and carried its history — 33 lines stamped with the
old cwd, 169 with the new. Claude logs a `relocated` line when it happens.

**The failure mode is therefore inverted.** S4 freezes `rec.cwd` *to protect* the
tail read; after any relocation the frozen value points at an abandoned
directory, and the tail read dies silently — the exact outcome the freeze exists
to prevent, caused by the freeze.

**The fix is S4's own deferred item**, which the spike promotes from optimisation
to requirement. S4 §3.8:

> "**Use `payload.transcript_path` instead of `jsonl_path(claude_dir, &rec.cwd,
> uuid)`** | strictly better (it removes the cwd→munge derivation from the hot
> path) but it is a fourth change. Deferred."

Claude reports the transcript's location on **every hook event**. Deriving it
from a cwd is guessing at something we are told. Using `transcript_path`
dissolves relocation entirely — no cwd tracking is needed for the tail read at
all, and S4's `live_cwd` field survives untouched for its other job, feeding
branch derivation.

**Why the payload cwd is a coarse signal, not jitter:** a shell `cd` inside a
tool call relocates nothing — the harness resets it. Relocation is a
Claude-level operation.

### 3.4 FINDING — `worktree-state` carries provenance for free

For any session inside a worktree the transcript already states `worktreePath`,
`worktreeName`, `worktreeBranch` and `originalCwd` — the entire input to the
design-lock §5 provenance glyph, **with no git subprocess**. Plain checkouts
still need S4 §4.2's `head.rs`; worktree sessions may not.

### 3.5 FINDING — `/clear` keeps the session, and the rename survives it

The transcript spans 12:46→14:16 across several `/clear`s under one uuid, and
`custom-title` holds `CLA-MAIN` throughout — 16 occurrences, all identical,
spanning the clears. So `/clear` continues the same jsonl rather than minting a
session, and a rename **persists in the transcript** even though it disappears
from the live session UI. This is the maintainer's requirement:

> "this also applies to the rename field that sets the label - that also
> disappears from the claude session on clear, but it should persist in the tab
> text."

S4 §3.3's hold-last-non-empty machine already delivers it, and S4 §6 Step 4
already validates it. **No change owed — this finding confirms S4.**

### 3.6 Reproducing

```bash
# 3.1 — the extinct tier
grep -rl '"type":"summary"' --include='*.jsonl' ~/.claude/projects | wc -l   # 0
# 3.2 — what is actually written
grep -ho '"type":"[a-z-]*"' ~/.claude/projects/*/*.jsonl | sort | uniq -c | sort -rn
# 3.3 — relocation: one hit, under the CURRENT cwd
find ~/.claude/projects -name '<session-uuid>.jsonl'
```

`ls` is aliased to `eza`, and project directory names begin with `-`, which bare
`ls` parses as flags. Use `/bin/ls --` or `find`.

---

## 4. Amendments owed

> "we'll need to go and update the specs for all the ux changes to bring
> everything into alignment after all these discoveries."

**S4** — `2026-07-22-S4-label-rename-and-live-cwd.md`, the heaviest:

- §3.2's "never moves it" invariant is **false** (§3.3). Correct it.
- Promote `payload.transcript_path` from deferred (§3.8, §7) to **mandatory**.
  It is the fix for relocation, not an optimisation.
- Retarget the `Summary` tier from the extinct `type:"summary"` to `ai-title`
  (§3.1, §3.2). The state machine currently has a state nothing can enter.
  `last-prompt` is the natural fallback tier.
- Add `worktree-state` (§3.4) as a zero-cost provenance source ahead of
  `head.rs` for worktree sessions.
- `title` and `summary` arrive structurally from this spec — consume, don't
  re-propose.

**S6** — `2026-07-22-S6-gutter-glyphs.md`: §2.4's `worktree` projection is
delivered here; re-point it. Add `worktree-state` as the cheapest provenance
source.

**S5** — `2026-07-22-S5-per-repo-colour.md`: already owed a revision by
design-lock §9 item 3 (`InkSpan` deleted, 8 hues, chip not tint). Add: ink fields
land **with** the ledger, never before (§2.2).

**S1** — `2026-07-22-S1-ordering-semantics.md`: unaffected by the spike; see §6
on pinning.

**Design lock** — §7.1 records that `Agent` has "no `title` and no `summary`
field". This spec makes that stale; amend to point here.

**`FOOTGUNS.md`** — three entries: the extinct `summary` tier and how to detect
it; transcript relocation on cwd change; project directories beginning with `-`
breaking bare `ls`.

---

## 5. Verification

This is a **cross-process/IPC change** under `docs/dev/TESTING.md`'s risk
taxonomy: it owes an ordering/idempotency argument and an independent
adversarial reviewer. The PR must state which lanes actually ran — a lane that
did not run is not a lane that passed.

**Tier 1 — unit.** The `clave-types` round-trip tests are extended, not
rewritten: a v1 payload (no new keys) deserialises with correct defaults, and a
v2 payload round-trips. The backfill gets an idempotency test — running it twice
changes nothing. The `snap()` helper in `model.rs`'s tests is updated once.

**Tier 2 — sandbox.** `just sandbox`. The change is inert by construction, so
the assertion is that **nothing renders differently** and the store loads
cleanly with and without the new keys present.

**Tier 3 — live.** Not automatable: no test observes a real bar consuming a real
snapshot in a live zellij session (#47). Confidence rests on the round-trip
tests plus the single producer/consumer pair.

**A regression trap this creates for S4, recorded here.** §3.1 means no existing
test can ever have covered the summary tier in reality — anything asserting
`LabelSource::Summary` behaviour asserts against a fixture. Those tests must be
**re-pointed at `ai-title`, not deleted**: they encode the right state machine
against the wrong source.

---

## 6. Deferred — do not design these out

- **Pinned tabs.** Planned: pin rows to the top so they hold position relative to
  each other while unpinned rows flow past. This is a **second ordering axis**.
  An ordinal key handles it (pinned rows sort in their own band); a design
  assuming one total order needs rework. S1 should keep the door open and build
  nothing.
- **`/recap`.** `ai-title` is already the automatic form (§3.2). An explicit
  recap may warrant its own tier above it. S4's tier table should be able to take
  a new row without structural change.
- **Theme-tracking colour.** S5/S6 keep role→value colour resolution swappable so
  the palette can follow the zellij theme (#60, #61). Snapshot fields carry
  **identity, never resolved RGB** — §2.2 preserves this by keeping ink fields
  out until S5.
- **Terminal tab identity.** Design-lock §7.2 — `Tab #16` is a placeholder.
  Unaffected here; the fields added are agent-scoped.
