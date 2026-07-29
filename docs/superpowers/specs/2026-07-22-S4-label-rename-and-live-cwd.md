# S4 — the label: Claude's rename, live cwd/branch, and a width-aware grammar (RC-F, slices #24)

_2026-07-22 · workstream **S4**, root cause **RC-F** of
[`2026-07-22-ux-defect-dossier.md`](2026-07-22-ux-defect-dossier.md) · main `50fa26a`_

_**Amended 2026-07-28** against
[`2026-07-28-agentsnapshot-v2-design.md`](2026-07-28-agentsnapshot-v2-design.md)
§3–§4 (#79), the AgentSnapshot v2 wire shape (#69, PR #81) and S4's own issue
(#59). This is a living document: the amendments are folded **in place**, and
where a superseded claim was load-bearing it is quoted and corrected rather than
deleted, because the old reasoning is unsafe to rebuild on. Four things changed
substantively — §1.3 / §3.3 (the summary tier's source was **extinct**), §3.2
(the "the transcript never moves" invariant was **false**, and the freeze built
on it caused the staleness it existed to prevent), §4.3
(`payload.transcript_path` is **mandatory**, not deferred), and §3.6 (an older
binary's read-modify-write **silently strips** earned fields — S4 owns the
fix). §3.1 and §3.6 also stop proposing `title` / `summary` and start consuming
them._

**The requirement, verbatim from the maintainer:**

> "the tab name inside the clave sidebar is `issue-10-kdl-guardrail …` — the old
> worktree we were in, even though the cwd has actually changed back to clave as
> the root."
>
> "I want to detect the renames and have tabs be named based on the rename. This
> works for the top of the pane — this one is called F-CLA for example."

**Row format, ruled 2026-07-22. Binding. Supersedes both the earlier
`repo · title · branch · summary` composition and #24's 2026-07-21
`● F-CLA · clave · 𖣂 · <summary>` comment:**

```text
● 󰁼 𖣂 F-CLA · clave · <summary>
└──┬──┘ └──────────┬──────────┘
 GUTTER            TEXT LABEL
  (S6)              (S4, this spec)
```

Two halves, two owners:

| Half | Cells | Contents | Owner |
|---|---|---|---|
| **gutter** | left, fixed | status dot `●` (exists), context-battery slot `󰁼` (**#24 item 4 — not this batch; the slot renders blank**), worktree marker `𖣂` | **S6** |
| **text label** | the rest | `title · repo · summary` — **three** segments, joined by ` · ` | **S4** |

**Branch is dropped from the rendered label entirely.** It is still derived and
still stored (§3.2) — it is a wire field, a picker input, and the natural source
for S6's worktree marker — but it consumes **no width budget** and appears in no
rendered row.

Two adjacent workstreams this spec is written against and does not touch:

- **S6 owns the gutter.** S4 composes a **text-only** label and knows nothing
  about glyph cells. The width budget is therefore a **parameter**, not
  `cols - 3` (§4.8).
- **S8 widens the sidebar from 30 to ~38 columns** (it touches the C6 width-seek
  machinery). S4's fitter must be correct at **any** width and hardcodes none;
  tests pin 30 and 38 explicitly (§5.1).

Read RC-F in the dossier first (`2026-07-22-ux-defect-dossier.md:371-425`). The
mechanism is not re-derived here.

---

## 1. Problem

### 1.1 The repo segment is a frozen worktree directory name

`clave add` composes the label once, from the *directory basename of the agent's
cwd* — `crates/clave/src/add.rs:699-711`:

```rust
let label = match &existing {
    Some(row) => sanitize_label(&row.label),
    None => {
        let dir_name = agent_cwd.rsplit('/').next().unwrap_or(&agent_cwd);
        sanitize_label(&format!("{dir_name} · {agent_branch}"))
    }
};
```

`cwd` is written exactly once, at record creation (`add.rs:743`). `refresh_label`
rebuilds the prefix on every hook event (`hook.rs:158-170`) but always from that
frozen `rec.cwd`:

```rust
let dir = rec
    .cwd
    .rsplit('/')
    .next()
    .filter(|s| !s.is_empty())
    .unwrap_or(&rec.cwd);
```

So an agent created in `…/clave/.claude-worktrees/issue-10-kdl-guardrail` is
named `issue-10-kdl-guardrail` for the rest of its life, whatever it does next.
That is the reported string.

**Two independent defects are folded into that one symptom, and they need
different fixes:**

| | Defect | Fix |
|---|---|---|
| (a) | the segment is sourced from **cwd**, when what a human wants to see is the **repo** | source it from `repo_root` — `clave-types/src/lib.rs:44-45`, *"git toplevel of cwd; the grouping key in the bar"*, and the **main** worktree root since #19 (`add.rs:553-561`) |
| (b) | cwd itself is never re-read after creation | deserialize Claude's `cwd` from the hook payload |

**Fix (a) alone resolves the reported string.** Cd-ing between a worktree and the
main checkout of the *same* repo does not change `repo_root`, so
`basename(repo_root)` reads `clave` from both.

**Fix (b) therefore has no rendered observable under the new format, and this
spec says so plainly.** With branch dropped from the label, a mid-session `cd`
changes `rec.live_cwd` and `rec.branch` in the store and changes **nothing** on
screen. It is kept because it is cheap, because `Agent.branch` is a wire field
and a picker input that is currently wrong for any agent that has moved, and
because it is the natural source for S6's `𖣂` worktree marker. It is
infrastructure, not the fix — and §6 Step 2 is rewritten around a store-only
observable accordingly.

**Nothing re-reads cwd mid-session — confirmed absent.** `current_dir()` appears
once, in the picker (`add.rs:481`). No assignment to `rec.cwd` outside record
construction. `HookPayload` (`hook.rs:23-34`) deserializes exactly three fields —
`session_id`, `prompt`, `message` — and **not** `cwd`, which every Claude Code
hook event carries. That absent field is the whole seam.

### 1.2 Claude's rename is written to disk and never read

Verified on disk (#24 comment, 2026-07-21, on the principal session's own
transcript): a session rename is appended to the jsonl as

```json
{"type":"custom-title","customTitle":"F-CLA","sessionId":"…"}
```

re-appended over the session's life (61 records in one transcript), **latest
wins**. Grep for `customTitle|custom-title|custom_title` across `crates/` returns
**exactly one hit, and it is a comment** — `hook.rs:79-82`, naming this as future
work. No code reads it. Claude's rename also updates `PaneInfo.title`, which the
bar discards at `clave-bar/src/main.rs:458-463`.

**The read is already there.** `refresh_label` tails the same file for summaries:
`summary_from_tail` (`hook.rs:116-130`) over `read_tail`'s last 64 KiB
(`hook.rs:135-143`), invoked from `run_hook` at `hook.rs:279-287`. A
`custom-title` tier is one more line-wise `serde_json::from_str` over a buffer
already in memory.

### 1.3 Two obstacles: the tier freezes the label, and its source is extinct

**Amended 2026-07-28 (#79, spike §3.1/§3.2).** This section originally named one
obstacle — the freeze. A live spike over the maintainer's own tree found a
second, underneath it, and the second is the one that has actually been hurting:

> `summary_from_tail` (`hook.rs:116`) scans for `{"type":"summary", …}`. That
> line type appears in **0 of 919 transcripts** under `~/.claude/projects/`, and
> all 14 rows of the live store read `label_source: first_prompt`.

**No agent has ever reached `LabelSource::Summary`.** Every label in the live
sidebar is a truncated prompt fragment — exactly the labelling the summary tier
was written to replace. Claude Code writes `ai-title` (373 occurrences across the
40 newest transcripts) where it once wrote `summary`; clave listens for a line
type that is no longer emitted. The tier's **source** is the primary defect, and
it is a live bug independent of this workstream. §3.3 retargets it.

The freeze below is therefore real in code but has **never executed in
production**. It still has to go — a retargeted tier that fires would hit it on
its first event — but it is downstream of a state nothing reaches, and no
reasoning in this spec may treat "we observed the freeze" as evidence of
anything.

`hook.rs:154-157`:

```rust
// Once a summary named the session, it stays (§6.4: stop re-scanning).
if rec.label_source == LabelSource::Summary {
    return false;
}
```

and the read that feeds it is gated the same way — `hook.rs:279-287`:

```rust
let tail = s.agents.get(&uuid).and_then(|rec| {
    if rec.label_source == LabelSource::FirstPrompt
        && matches!(event, "Stop" | "UserPromptSubmit")
    {
        read_tail(&jsonl_path(&claude_dir, &rec.cwd, &uuid), 64 * 1024)
    } else {
        None
    }
});
```

Titles change repeatedly and branches change on every `git switch`. Both gates
must move: the *whole-function* stop becomes a *summary-branch-only* stop, and
the tail read stops being conditioned on `label_source` at all. **And the
extractor those gates feed must be retargeted from `type:"summary"` to
`ai-title`** — moving a gate in front of an extinct tier changes nothing
observable.

A third thing must move with them, for a reason nobody suspected when this
section was written: the file `read_tail` opens. §3.2 records the correction.

### 1.4 At 30 columns, the width eats exactly the distinctive part

The clamp is a dumb tail-truncate — `clave-bar/src/main.rs:546-553`:

```rust
let budget = cols.saturating_sub(3); // gutter + margin
let name: String = if row.name.chars().count() > budget {
    let mut n: String = row.name.chars().take(budget.saturating_sub(1)).collect();
    n.push('…');
    n
} else {
    row.name.clone()
};
```

`issue-10-kdl-guardrail · -` is 26 chars and the budget at 30 cols is 27 — the
shared, least-distinctive prefix consumes the entire row before a summary can
appear. This is the maintainer's own #24 finding ("the width problem and the
label problem are ONE problem").

Three things move the arithmetic, and only the third is S4's:

- **S8** takes the pane from 30 to ~38 columns — more room, not a policy.
- **S6** takes cells *back* for the gutter (status + battery + worktree marker),
  so the text budget is **not** `cols - 3` and is not S4's to compute.
- **S4** drops branch from the render and gives the surviving three segments a
  give-way order (§3.4), so the distinctive part survives whatever budget it is
  handed.

The `cols.saturating_sub(3)` above is therefore replaced by a **budget
parameter**, and no width constant appears anywhere in this spec.

---

## 2. What this workstream closes, and what it leaves

Slice of **#24** (sidebar distinctiveness epic).

| #24 item | S4 |
|---|---|
| 1. worktree provenance in the name, from `repo_root` | **intent closed, by relocation.** The *name* half lands here: the row names the **repo** (`basename(repo_root)`), never the worktree directory — which is the whole of the reported defect. The *provenance* half moves out of the name entirely and becomes S6's `𖣂` gutter marker. The `<repo> » <worktree-dir>` rendering the item originally proposed is **not** built and is now superseded: at 30–38 columns a worktree directory name is exactly the string that was crowding out everything distinctive |
| 2. per-repo colour coding | **not S4** — S5 / RC-G. S4 guarantees the contract S5 needs (§3.5) |
| 3. pull in Claude's own session summaries; revisit what the label shows at each width | **closed, and wider than first scoped** — the summary tier is *retargeted* to `ai-title` (§3.3), because the `type:"summary"` source it was built on is extinct and the tier has never once fired (§1.3, #79); a higher `custom-title` tier is added; and §3.4 gives a width policy that is correct at any budget. The earlier claim here — "the summary tier is verified to keep upgrading" — was **false**, and is corrected rather than deleted because it was load-bearing for §3.3's state machine |
| 4. context battery per row | **not S4** — two seams are opened for it and neither is built: the extended tail scan (§4.3b) is where the token estimate is computed, and S6 reserves its gutter cell |
| 5. model badge | **not S4** — same tail-scan seam, same note |
| 6. uncollapsed width (30 → ~38) | **not S4 — S8.** §3.4 is correct at any budget and hardcodes none; the C6 width-seek machinery is untouched here |
| 7. collapsed 4-col design | **not S4** — needs the gutter (S6) and the colour channel (S5) to say anything at all |
| the gutter glyphs (`● 󰁼 𖣂`) | **not S4 — S6.** S4's contract to it: the composed label is text-only and carries no glyph, and the budget is a parameter S6 sets (§4.8) |
| "render from structured FIELDS, not the stored label string" | **partially, and the ceiling is now known.** The give-way policy is structured (§3.4) but `Row` stays a single `String`, because live rows render the *zellij tab name* and never see `Agent.label` (§3.5). Structured render is reachable only if the live-row path stops using the tab name — a much larger change, and nobody's this batch |

---

## 3. Design

### 3.1 The label grammar

One rule, one function, both writers. Segments are joined by
`LABEL_SEP = " · "` (U+0020 U+00B7 U+0020 — verified `20 c2 b7 20`), and an
**absent segment contributes no separator**.

| # | Segment | Source | When it refreshes | When absent |
|---|---|---|---|---|
| 0 | **title** | latest **non-empty** `{"type":"custom-title","customTitle":…}` in the jsonl tail, held in `rec.title` — the **user's own rename**, the highest-authority signal on the row | every `Stop` / `UserPromptSubmit` — the tail is re-scanned and `rec.title` updated on any new non-empty value | omitted entirely (no separator), and **repo shifts to index 0** — see the positional caveat in §3.5. This is the *common* case: most sessions are never renamed |
| 1 | **repo** | `basename(rec.repo_root)`; falls back to `rec.repo_root` whole if it has no `/`, then to `rec.cwd`'s basename if `repo_root` is empty | never — `repo_root` is immutable after creation (§3.6) | **never absent.** A non-repo dir gets its own basename (`add.rs:502-504` falls back to the picked dir) |
| 2 | **summary** | `rec.summary`: Claude's rolling `{"type":"ai-title","aiTitle":…}` if the tail carries one, else the first 4 words of the first real user prompt | `ai-title` re-reads on every `Stop` / `UserPromptSubmit` and **keeps upgrading** — it is a rolling description, ~9 per transcript (§3.3, §5.3) | `""`. A brand-new agent has none until its first prompt |
| — | ~~branch~~ | `rec.branch`, re-derived from the **live** cwd's `HEAD` (§3.2). Stored, on the wire (`Agent.branch`), used by the picker and `clave ls` | every `Stop` / `UserPromptSubmit` | **never rendered.** Zero width budget. A detached or unreadable `HEAD` still stores `-` (`add.rs:305,510`) |

**The two earned fields are not S4's to declare.** `title: Option<String>` and
`summary: String` land **structurally** with the AgentSnapshot v2 wire shape
(#69, PR #81) — on `clave_types::Agent` *and* `store::AgentRecord`, both
`#[serde(default)]`, both projected by `snapshot_from`
([`2026-07-28-agentsnapshot-v2-design.md`](2026-07-28-agentsnapshot-v2-design.md)
§2). S4 **consumes** them; it does not re-propose them. Two consequences that
propagate through this spec:

- the earlier draft's field name `words` is retired in favour of the landed
  `summary`, and
- `summary` is a **`String`, not an `Option`** — "not earned yet" is `""`, not
  `None`. Every guard written as `words.is_none()` becomes `summary.is_empty()`.

`live_cwd` (§3.2) remains S4's own field, and is the only record field this spec
still adds.

Composition is a method on the record so `add.rs` and `hook.rs` cannot drift:

```rust
impl AgentRecord {
    pub fn compose_label(&self) -> String { … }   // store.rs
}
```

Each segment passes `sanitize_segment` (§3.7) before joining, and the joined
result is **not** re-sanitized (that would collapse the separator).

Worked examples at the reported case (the gutter, S6's, is shown for context but
is not part of the string S4 composes):

| State | Composed label (S4) | Full row |
|---|---|---|
| fresh `clave add --worktree`, no prompt yet | `clave` | `● 󰁼 𖣂 clave` |
| after the first prompt | `clave · fix the flaky auth` | `● 󰁼 𖣂 clave · fix the flaky auth` |
| after Claude's first `ai-title` | `clave · Fix auth flow` | `● 󰁼 𖣂 clave · Fix auth flow` |
| after the maintainer renames the session `F-CLA` | `F-CLA · clave · Fix auth flow` | `● 󰁼 𖣂 F-CLA · clave · Fix auth flow` |
| after `cd` back to the main checkout | `F-CLA · clave · Fix auth flow` | **unchanged** — branch does not render (§1.1); the store changes, the screen does not |

Note the first row: an un-prompted, un-renamed agent composes to a **single
segment**. That is intended — the gutter already carries status and provenance,
and there is genuinely nothing else known about it yet.

### 3.2 Live cwd, the transcript path, and what gets re-derived

> **CORRECTION, 2026-07-28 (#79, spike §3.3).** This section previously asserted,
> as a hard invariant, that *"Claude keys the transcript directory on the cwd the
> session was **created** in and never moves it"*, and built the whole
> freeze-`rec.cwd` argument on it. **That invariant is false, and it was
> load-bearing — the reasoning below is rewritten, not footnoted.** Measured:
> a session created in `…/code/clave` and then moved into a worktree has its
> **entire `.jsonl` relocated** by Claude into a project directory keyed on the
> **new** cwd, carrying its full history (33 lines stamped with the old cwd, 169
> with the new), and logs a `{"type":"relocated","relocatedCwd":…}` line.
> `find ~/.claude/projects -name '<uuid>.jsonl'` returns exactly one hit, under
> the **new** path. The old directory holds nothing.
>
> **The failure mode was therefore inverted.** Freezing `rec.cwd` *to protect the
> tail read* means that after any relocation the tail read opens an abandoned
> directory and silently finds nothing — **the freeze caused the exact staleness
> it existed to prevent**, and it would have been invisible: no error, just a
> label that stops upgrading. Any future reasoning of the form "the transcript
> lives where the session was born" is unsafe. See
> [`2026-07-28-agentsnapshot-v2-design.md`](2026-07-28-agentsnapshot-v2-design.md)
> §3.3 for the evidence and the reproduction.

**The tail read stops deriving a path and starts being told one.**
`payload.transcript_path` is **mandatory** in this spec (§4.3), not the deferred
optimisation §3.8 once listed it as. Claude reports the transcript's location on
**every hook event**; deriving it from a cwd is guessing at something we are
told, and relocation is precisely where the guess is wrong. Consuming the
reported path dissolves relocation for the label pipeline entirely — there is no
cwd→munge derivation left on the hot path to be stale.

**`rec.cwd` is still not mutated** — but for two reasons now, not three. The
transcript-join-key reason is dead:

| Reason | Still holds? |
|---|---|
| ~~`jsonl_path(claude_dir, &rec.cwd, &uuid)` (`spawn.rs:28-33`) → `munge_cwd` (`munge.rs:20-25`) locates the transcript~~ | **no.** The path moves; the label pipeline no longer derives it (§4.3d) |
| `spawn_mode` (`spawn.rs:35-41`) tests that path to choose resume-vs-create. A wrong path means `claude --session-id` on an in-use id | yes — and see the hazard below |
| `claude --resume` is **project-dir-scoped** (`add.rs:242-244`), so the resume tab must open in `rec.cwd` | yes |

**A hazard this correction exposes, which S4 does not fix.** `spawn_mode` still
probes `jsonl_path(claude_dir, &rec.cwd, uuid)`. After a relocation that probe
misses, `spawn_mode` chooses *create*, and `clave spawn` mints a fresh session
rather than resuming — the history is orphaned, silently. That is a **spawn/resume
defect, pre-existing and independent of the label**, newly *visible* because the
relocation is now known. It is filed as a follow-up (§7, limitation 5) rather than
fixed here: `live_cwd` gives it a second path to probe, but changing
resume-vs-create arbitration is not a label change and must not ride this diff.

`merge_resume_record` (`add.rs:343-354`) still preserves cwd, pinned by
`merge_resume_preserves_existing_row_and_resets_status` (`add.rs:889-922`).
**That test and that behaviour are unchanged by this spec** — nothing here
touches `rec.cwd`.

`AgentRecord` gains `live_cwd: Option<String>`: the cwd Claude last reported,
purely an *observation*. It has exactly two jobs — feed the branch derivation,
and be visible in the raw store so the maintainer can diagnose a divergence
(`live_cwd != cwd` ⇒ the agent has moved, and — now — its transcript has moved
with it). It has explicitly **lost** the job it never had: it is not, and must
not become, an input to the transcript path. That input is `transcript_path`.

**Branch is re-derived; nothing else is. It is derived but never rendered.**
Rationale, decision by decision:

- **Branch, yes — even though it does not render.** Three reasons it survives the
  format change: (1) it is a **wire field** (`Agent.branch`,
  `clave-types/src/lib.rs:46-47`) and a **store field** that is simply *wrong*
  today for any agent that has moved, and a wrong stored value is a latent
  defect regardless of who reads it; (2) `resume_candidates` labels worktree
  picker rows with it (`add.rs:301-310`, and §4.5(d) below now makes that the
  only place it appears to a human); (3) it is the obvious source for S6's `𖣂`
  marker (`rec.worktree.is_some()` is the direct test, but branch is what
  disambiguates *which* worktree). It costs two file reads (below), so the bar
  for keeping it is low. It changes on `git switch` *without* a cd, so the
  derivation runs on every label-bearing event, not only when `live_cwd` moves.
  Two derivations per turn, worst case.
- **Branch consumes no width budget.** It is absent from `compose_label`
  (§4.4c), so it cannot crowd out the title, the repo or the summary. This is
  the single largest width win in the change: `clave/ab12cd34` was 13 of 27
  cells.
- **`worktree-state` first, the filesystem second (added 2026-07-28, #79, spike
  §3.4).** For any session Claude has placed in a worktree, the transcript
  already carries `{"type":"worktree-state","worktreePath":…,"worktreeName":…,
  "worktreeBranch":…,"originalCwd":…}` — 384 occurrences across the 40 newest
  transcripts. That is the branch, the worktree path **and** S6's entire
  provenance input, **for zero additional cost**: the tail is already in memory
  for the title and `ai-title` tiers, so this is one more line-wise
  `from_str` over a buffer we have already read, with **no filesystem walk and no
  git subprocess at all**. The tier order is therefore:

  1. `worktree-state` in the tail → `worktreeBranch` (and `worktreePath` /
     `worktreeName` for §7's S6 hand-off);
  2. failing that, `head::head_branch` over `live_cwd` (below);
  3. failing that, `rec.branch` unchanged.

  **`head.rs` is not made redundant and is not dropped.** The line is emitted
  only for sessions Claude itself put in a worktree; a plain checkout — and a
  worktree created by `clave add --worktree` rather than by Claude — never
  carries it. `head.rs` remains the universal fallback and keeps its full test
  matrix (§5.1). What changes is that the common worktree case stops touching
  the filesystem.
- **No `git` subprocess.** The hook is a global Claude Code hook whose prime
  directive is DO NO HARM (`hook.rs:1-5`). A subprocess on the turn path can
  block Claude, and `Command` has no timeout in this codebase. Where the
  `worktree-state` tier does not apply, branch is read **from the filesystem**:

  ```
  walk up from live_cwd for a `.git` entry
    ├─ `.git` is a directory → read `.git/HEAD`
    └─ `.git` is a file      → it contains `gitdir: <path>` (a linked worktree)
                               → read `<path>/HEAD`
  HEAD contains `ref: refs/heads/<branch>` → branch = <branch>
  HEAD contains a raw 40-hex sha          → detached → "-"
  anything else / unreadable / no .git    → None → keep rec.branch
  ```

  Two file reads and a bounded upward walk. No subprocess, no index lock, no
  network filesystem stall beyond a `stat`. It is also **hermetically testable
  with `tempfile`, with no `git` binary on the box** — which matters, because
  Tier 2 does not exist (#47).
- **`repo_root`, no.** Refreshing it is *possible* from the same walk (a linked
  worktree's `gitdir` is `<main>/.git/worktrees/<name>`, so the main root is two
  parents up) but it is **declined**: `repo_root` is the picker's grouping key —
  `resume_candidates` filters `store.agents.values().filter(|r| r.repo_root == repo_root)`
  (`add.rs:275`). Rewriting it when an agent cd's into another repo would drop
  the row out of the picker of the repo whose project dir still holds its
  transcript and which is the only cwd `claude --resume` accepts. The row would
  be listed under a repo it cannot be resumed into. Accepted consequence: an
  agent that cd's **across repos** keeps naming the repo it was born in — which
  is also the repo it is bound to for resume. Recorded as a known limitation
  (§7) with a follow-up.

### 3.3 The `LabelSource` state machine — and the tier it points at

`LabelSource` currently means *"which tier produced the whole label, and has it
frozen"*. It is re-scoped to mean **"which tier produced the `summary`
segment"**, and nothing else. Segments 0 and 1 are outside it and always live.

**The tier is retargeted (2026-07-28, #79, spike §3.1/§3.2).** `LabelSource::Summary`
no longer means "a `{"type":"summary"}` line was found" — that line type is
extinct, **0 of 919 transcripts**, and the state has never been entered by any
agent in production (§1.3). It now means **"an `ai-title` was found"**:

| Transcript line | Count, 40 newest transcripts | Feeds |
|---|---|---|
| `custom-title` — `{"customTitle":"CLA-MAIN"}` | 1057 | `rec.title`, segment 0. **The user's own rename: a distinct and higher-authority signal**, and never conflated with the tiers below |
| **`ai-title`** — `{"aiTitle":"Get approval to proceed"}` | **373** | **`rec.summary`, segment 2 — the retargeted tier.** Claude's rolling auto-description |
| `last-prompt` — `{"lastPrompt":…}`, maintained live | 1340 | the **fallback** for segment 2, when no `ai-title` exists yet |
| `summary` | **0** | nothing. Extinct |

`ai-title` is what the maintainer independently predicted the tier should read
("claude often computes a recap … that recap might be a good field to hold onto,
and it might actually be better to use in the text of the summary when it
exists"). It exists, it is automatic, and it was already in the transcript the
whole time.

**The variant name `Summary` is kept, deliberately.** Renaming it to `AiTitle`
changes the serialized string `"summary"` → `"ai_title"`, which is the *same*
whole-store parse failure on an older binary that the rejected third variant
would cause (below). The wire value is a stable token; the tier it names is an
implementation fact. §4.4(a)'s doc comment carries the mapping so the next reader
does not have to infer it.

**The two wire values are unchanged.** `LabelSource` keeps exactly
`FirstPrompt | Summary` (`store.rs:28-33`, `#[serde(rename_all = "snake_case")]`).
Adding a third variant (`None`) was rejected: an older `clave` binary reading a
store containing `"label_source":"none"` fails `serde_json::from_slice` in
`read_store` (`store.rs:123-129`) — a **whole-store parse failure**, in exactly
the mixed-binary window #43/#44 makes routine. "Not earned yet" is instead
expressed as `summary.is_empty()`, on a `#[serde(default)]` field (#69) an old
binary ignores.

**State S stops being terminal for the segment.** The old design froze the text
the first time a summary landed, on the premise that "the label only meaningfully
changes once". `ai-title` falsifies the premise: it is a *rolling* description,
re-emitted roughly nine times per transcript, and freezing it on first sight
throws away the whole reason for consuming it. S self-loops **with updates**, and
§5.3 re-does the churn arithmetic for it.

States are the pair `(summary, label_source)`:

```text
                     ┌───────────────────────────────────────┐
                     │  U  ·  UNEARNED                       │
                     │     summary      = ""                 │
                     │     label_source = FirstPrompt        │
                     └───┬───────────────────────────┬───────┘
   UserPromptSubmit      │                           │  tail yields an ai-title
   ∧ prompt non-empty    │                           │  ∧ not harness-injected
   ∧ not harness-injected│                           │
                         ▼                           │
        ┌────────────────────────────────┐           │
        │  P  ·  PROMPT-EARNED           │           │
        │     summary      = first 4     │           │
        │                    words       │           │
        │     label_source = FirstPrompt │           │
        │  self-loop: a LATER prompt does │          │
        │  NOT overwrite (earn-once)      │          │
        └────────────────┬───────────────┘           │
                         │  tail yields an ai-title  │
                         │  ∧ not harness-injected   │
                         ▼                           ▼
        ┌──────────────────────────────────────────────────┐
        │  A  ·  AI-TITLE-EARNED   (label_source = Summary) │
        │     summary      = clamp(latest aiTitle)          │
        │     label_source = Summary                        │
        │   self-loop WITH UPDATES: a NEW ai-title replaces  │
        │   the old one (it is a rolling description); the   │
        │   prompt tier never writes again; the TITLE        │
        │   segment keeps refreshing independently           │
        └──────────────────────────────────────────────────┘
```

| From | Event | Guard | To | `refresh_label` returns |
|---|---|---|---|---|
| U | `UserPromptSubmit` | prompt non-empty ∧ `!is_harness_injected` | P | `true` (label changed) |
| U | `UserPromptSubmit` | prompt injected, or empty/absent | U | whatever the title/repo segments decide |
| U | `Stop` \| `UserPromptSubmit` | no `ai-title`, but the tail has a `last-prompt` ∧ `!is_harness_injected` | P | `true` — the fallback tier (below) |
| U, P | `Stop` \| `UserPromptSubmit` | tail has an `ai-title` ∧ `!is_harness_injected` | A | `true` |
| P | `UserPromptSubmit` | any later prompt | P | segment unchanged — **earn-once**; a new title may still change the label |
| A | `Stop` \| `UserPromptSubmit` | tail has a **different** `ai-title` | A | `true`, and the segment is replaced |
| A | `Stop` \| `UserPromptSubmit` | same `ai-title`, or none in the tail | A | `false` — the held-last-value rule, exactly as for the title (below) |
| any | `Notification` \| `SessionEnd` | — | unchanged | no tail read at all |

**The `last-prompt` fallback, and why it is earn-once.** `payload.prompt` reaches
`refresh_label` only on `UserPromptSubmit`; an agent clave began tracking
mid-life, or one whose first tracked event is a `Stop`, has no prompt to earn
from and sits at the bare repo segment indefinitely. `last-prompt` is the same
text, in the tail we are already reading, available on every event — so it is the
natural source for the **U → P** transition when the payload cannot supply one.
It is deliberately **not** allowed to overwrite an earned segment: `lastPrompt` is
maintained *live* (1340 occurrences), so a tier that re-read it every turn would
rewrite the label on every single prompt and fire a tab rename with it. The guard
is the same `summary.is_empty()` that gates the payload path, and #17's
`is_harness_injected` check applies to it identically — a `last-prompt` holding a
`<task-notification>` is the same leak from a different door.

The transition **U → P** is where #17's guard lives, and its gate changes. Today
it is a byte-for-byte string comparison — `hook.rs:190-197`:

```rust
if event == "UserPromptSubmit"
    && rec.label == prefix
    && let Some(p) = payload.prompt.as_deref().filter(|p| !p.trim().is_empty())
    && !is_harness_injected(p)
```

`rec.label == prefix` is the fragile bit: it is the "Task 5 cross-task coupling"
that forces `add.rs:709` to build the label byte-identically. With a title
segment that can appear between two events, and a branch that can change, the
reconstruction can no longer be relied upon. It becomes an explicit state test:

```rust
if event == "UserPromptSubmit" && rec.summary.is_empty() && …
```

**This deletes the byte-for-byte coupling entirely** — `add.rs`'s comment at
`add.rs:703-708` goes with it.

**Title is a separate, non-terminal sub-machine.** It is not part of
`LabelSource` because it can change any number of times:

```text
title = None      ──(tail: non-empty customTitle T, not injected)──► title = Some(T)
title = Some(A)   ──(tail: non-empty customTitle B ≠ A)───────────► title = Some(B)
title = Some(A)   ──(tail: empty, absent, or injected)────────────► title = Some(A)   [HELD]
```

The **held** arm is load-bearing twice over:

1. **The maintainer's `/clear` ruling** (#24 comment, 2026-07-21): *"clave
   persists the latest NON-EMPTY across `/clear` (Claude clears its own); empty
   records ignored."* Claude appends an empty `customTitle` on `/clear`; clave
   keeps the last real one. Clave's bar and Claude's own `✻ F-CLA` pane header
   may legitimately disagree after a `/clear` — clave's is the deliberate one.
2. **The 64 KiB tail window.** `read_tail` (`hook.rs:135-143`) reads the last
   64 KiB. A rename made early in a long session scrolls out of that window. If
   the title were recomputed from the tail every time, it would **flicker off**
   the moment the transcript grew past the window and back on at the next
   rename. Holding the last non-empty value in `rec.title` makes the tail an
   *update channel*, not a *source of truth*, and the flicker cannot happen.

### 3.4 Truncation: give-way order, and where it lives

**Where.** Bar-side, at render, as a pure function in
`crates/clave-bar/src/model.rs`, called from the clamp site at
`clave-bar/src/main.rs:546-553`.

Why not CLI-side: the CLI knows neither `cols` nor S6's gutter width, and the
same string is the picker line, the `clave ls` line and the `launch.kdl` tab
name — all of which want full fidelity. Truncating at composition corrupts four
consumers to serve one.

Why bar-side works for **both** row kinds despite the dossier's split (live rows
render `t.name`, the *zellij tab name*, at `model.rs:757`; dormant rows render
`a.label` at `model.rs:783`): both are strings, both are produced by
`compose_label` (the tab name via `Effect::RenameTab`, `model.rs:569-582`), so
the fitter takes a `&str` and needs no knowledge of which branch produced it.
When a human has manually renamed a zellij tab, the string has no separators,
`split` yields one segment, and the function degrades to exactly today's
tail-truncate. No regression, no special case.

**The policy, and why it needs no positional inference.**

```rust
fit_label_str(name: &str, budget: usize) -> String
```

The grammar (§3.1) puts the **summary last, always** — it is the only optional
trailing segment. So the give-way rule needs to know nothing about *meaning*:

> **Drop trailing segments while the joined string exceeds the budget and more
> than two segments remain. Then tail-truncate with `…`.**

That single rule implements the required priority exactly:

| Give-way rank | Segment | Why |
|---|---|---|
| 1st (and only) to go | **summary** | the give-way segment, per the ruling. It is the longest, the most compressible, and the one Claude will re-derive |
| never dropped | **repo** | identity-bearing *and* colour-bearing (S5) |
| never dropped | **title** | identity-bearing *and* the maintainer's most-important element; also colour-adjacent, being the row's human name |

Worked over every shape `compose_label` can emit:

| Composed | Over budget → |
|---|---|
| `title · repo · summary` | → `title · repo` (2 segments, floor reached) → tail-truncate |
| `repo · summary` (no title — the majority) | 2 segments already: **no drop**, tail-truncate. `clave · Fix the auth fl…` — the summary degrades gracefully instead of vanishing, which is the right answer now that repo is short (branch is gone) |
| `title · repo` (no summary earned) | 2 segments: no drop, tail-truncate |
| `repo` (nothing earned, never renamed) | 1 segment: tail-truncate |
| `some-hand-renamed-tab` (no separators) | 1 segment: tail-truncate, exactly as today |

**When even `title · repo` overflows** (the case the ruling asks about
explicitly): **no further segment is dropped.** The joined string is
tail-truncated, so the **title survives whole and the repo truncates from the
right** — `F-CLA · cla…`. Three reasons that is the right resolution rather than
dropping the repo:

1. The repo is **redundantly encoded in colour** (S5) and in the gutter's
   worktree marker (S6). It is the one segment that can afford to lose
   characters, because its channel is not only textual.
2. A truncated repo still **starts with** `basename(repo_root)`'s leading
   characters, so S5's value-match (§3.5) works with `starts_with` and does not
   need the whole string.
3. Dropping it instead would leave a bare title, and the title is *the same
   width problem one level down* — a user-chosen string with no upper bound
   below the 32-char clamp (§3.7). Truncation is honest; dropping is not.

The counter-case is bounded: `title · repo` overflows only when the title is
long, which is the user's own choice, and never for the observed shape
(`F-CLA · clave` = 13 cells).

Worked at the two widths S8 brackets, with a nominal 4-cell gutter (S6's number,
not S4's — the point is the fitter is correct for any of them):

| Budget | `F-CLA · clave · Fix the auth flow` (33) |
|---|---|
| 30 cols → ~25 | drop summary → `F-CLA · clave` (13) ✓ |
| 38 cols → ~33 | fits whole ✓ |

Per-segment shrinking (abbreviating a repo, eliding a summary's middle) is
deliberately **not** built (§3.8).

### 3.5 The S5 contract — four obligations, three held verbatim

**Superseded — S5 does NOT split a rendered string.** This section previously
handed S5 a `String` to split on ` · `, on the reasoning that live rows render
the *zellij tab name* (`model.rs:757`) rather than `Agent.label`, so a segmented
value computed in `rows()` could not reach them. Design-lock §7.1 overturned
that: a live row renders from the **STORE**, and #69 landed `title`, `summary`
and `worktree` on `Agent` as values precisely so the bar never parses positions
out of a composed string. Following the old contract would resurrect the deleted
span/index mechanism and bypass those fields.

**The obligation is now inverted.** S4 owes S5 *values*, not a splittable
string: populate `AgentRecord.title` and `AgentRecord.summary`, which
`snapshot_from` already projects. S5 lays its own fixed-width columns from them
(design-lock §2). `fit_label_str` survives only for the **zellij tab name**,
which is genuinely one opaque string.

| # | Obligation | Held? |
|---|---|---|
| 1 | **title is segment 0** | ✅ **when a title exists.** `compose_label` emits it first, and `fit_label_str` never drops a leading segment |
| 2 | **repo is segment 1** | ⚠️ **when a title exists.** When Claude has never renamed the session — *the majority of rows* — there is no title segment and **repo is segment 0**. See the caveat below; it is the one thing S4 cannot make true by construction |
| 3 | **separator is exactly ` · `** | ✅ U+0020 U+00B7 U+0020, exported as `clave_types::LABEL_SEP` so both crates reference one constant instead of two literals |
| 4 | **U+00B7 stripped from segment content** | ✅ `sanitize_segment` (§3.7) replaces the raw character in every segment, so the split is unambiguous and a Claude-authored title cannot forge a boundary |

**The caveat on obligation 2, stated precisely because it will otherwise be
found at runtime.** Purely positional indexing is **ambiguous at two segments**:
`title · repo` and `repo · summary` are indistinguishable by splitting alone, and
both are common. Making the title always present (an empty leading slot) was
considered and rejected (§3.8): it burns three cells on every un-renamed row and
`sanitize_segment` would strip it anyway.

This is also the answer to CodeRabbit's "keep composition and paint-span indexing
based on identical emitted fields" (2026-07-22): S5 does **not** re-derive or
re-index the segment sequence — it matches the *emitted, sanitized* repo text by
value. Composition (`compose_label`) and paint (`InkSpan`) therefore share the
same post-sanitization string, so they cannot drift apart even if the segment
count changes under truncation.

**Recommended resolution, which costs S5 nothing:** locate the repo segment **by
value**, not by index. S5 already resolves the row's `Agent` to read
`repo_root` — for dormant rows the agent *is* the row (`model.rs:769-789`), and
for live rows `rows()` already calls `agent_in_tab(t.tab_id)` to pick the glyph
(`model.rs:746-753`). So S5 has `basename(repo_root)` in hand and can match it
against the split segments. S4's guarantee that makes this exact:

> **The repo segment's text is byte-for-byte
> `sanitize_segment(basename(repo_root))`** — no abbreviation, no decoration, no
> case change. When the row was tail-truncated, the surviving repo text is a
> **prefix** of that string, so a `starts_with` match is sufficient and no other
> segment can collide with it (a title or summary that happens to equal the repo
> basename colours identically, which is harmless).

The collision surface between the two workstreams stays small by construction:
**S4 touches neither `rows()` nor `Row`** (S5's seam), and S5 touches none of
`hook.rs` / `add.rs` / `store.rs` (S4's). The only shared file is
`clave-bar/src/main.rs:539-557`, where S4's edit is one line (§4.8).

### 3.6 Compatibility and migration

**Amended 2026-07-28.** S4 now adds **one** `AgentRecord` field, not three.
`title` and `summary` land with the v2 wire shape (#69, PR #81) on both `Agent`
and `AgentRecord`, and `snapshot_from` (`store.rs:185`) already projects them —
design-lock §7.1 rules that the bar renders from the store, so every field it
lays a column from arrives as a value rather than as a position inside `label`.
The earlier claim here that these were **store-only and deliberately off the
wire** is superseded; do not restore it.

| Field | Type | Owner | On the wire? |
|---|---|---|---|
| `title` | `Option<String>` | **#69** — consumed, not proposed | yes, `Agent.title` |
| `summary` | `String` (`""` = unearned) | **#69** — consumed, not proposed | yes, `Agent.summary` |
| `worktree` | `Option<String>` | **#69** — already stored, now projected | yes, `Agent.worktree` |
| `live_cwd` | `Option<String>` | **S4** | **no** — a diagnostic, not a render input |

Consequences:

- **`clave_types` gains only `LABEL_SEP` from S4** — a `pub const`, no schema
  change. The struct changes are #69's and carry #69's roundtrip obligations.
- **Old store → new binary**: `#[serde(default)]` covers it. `summary == ""` puts
  a long-lived agent back in state **U**, so its next `ai-title` — or its next
  real prompt — re-earns the segment. A previously-`Summary` row that arrives
  with an empty `summary` would otherwise be stuck, because the prompt tier is
  gated on state U: **re-derive on read** — when
  `summary.is_empty() && label_source == Summary`, reset `label_source` to
  `FirstPrompt` so the next event re-earns from the tail. One line, in
  `refresh_label`, and testable. It is also the self-heal for the hazard below,
  which is why it is not optional.

#### The mixed-version write hazard — S4's to fix

**This is not the same "degrades gracefully" story the earlier draft told, and
the difference is the whole point.** From `FOOTGUNS.md` ("Rust and codebase
specifics", #69 whole-branch review / #59):

> **An OLDER `clave` binary writing the store SILENTLY STRIPS fields it does not
> know.** `with_store_mut` is read-modify-**write**: serde drops unknown keys on
> deserialize, so the whole row is re-serialized without them. Harmless for
> `title`/`summary` today — the #69 backfill re-seeds them from `label` at the
> next session create — but the moment S4 (#59) writes an *earned* summary from
> `ai-title`, a single old-binary write in a mixed-version window destroys it
> with no self-heal and no error. Any field holding earned state needs either a
> version guard or a re-derivation path.

Restated as it applies here. `AgentRecord` has no `deny_unknown_fields`, so an
old binary *parses* a new store — but it does not merely ignore the new fields,
it **drops them on the next write**. Today that is benign, because nothing
populates `title`/`summary` except #69's backfill, which re-seeds from `label` at
the next session create. **S4 is exactly the change that makes it malign**: once
`rec.summary` holds an earned `ai-title` and `rec.title` holds the user's rename,
one hook event from a stale binary erases both, silently, with no error and — for
`title` — no source to re-seed from, because `label` no longer encodes it
unambiguously.

**The fix S4 adopts: a re-derivation path, keyed on `label_source`.** Not a
version guard — #69 §2.1 rules out a version field explicitly ("no version field,
no migration framework"), and a guard that refuses to write is worse than a write
that heals. The mechanism:

1. `label_source` is a **pre-existing** field, known to every binary since #17,
   so an old binary preserves it through a read-modify-write. `Summary` is a
   value **only a post-S4 binary ever writes** (the `type:"summary"` tier it used
   to mean is extinct — §3.3). It therefore survives as a reliable marker of
   *"this row had earned an `ai-title` summary"* even when the value itself has
   been stripped.
2. The §3.6 re-derivation above reads exactly that signal —
   `summary.is_empty() && label_source == Summary` — and puts the row back in
   state U, so the next `Stop` re-earns from the transcript tail.
3. `title` needs no marker: the held-last-non-empty rule (§3.3) already rewrites
   it from the tail on the next label-bearing event.

**The residual, stated rather than hidden.** Both recoveries read the 64 KiB
tail. If the last `custom-title` / `ai-title` line has scrolled out of that window
before the stale write happens, the value is gone until Claude emits a fresh one —
which, for `ai-title`, is a matter of turns, and for `custom-title` may be never.
That is a genuinely lossy corner. It is **bounded to the mixed-version window**
(#43/#44), it produces a stale label rather than a wrong one, and closing it
properly means a store schema version, which is #69's ruled-out territory. Filed
as a follow-up (§7, limitation 6), pinned by
`stale_binary_strip_is_re_derived_from_the_tail` (§5.1), and named in the
adversarial reviewer brief (§5.4).

### 3.7 Sanitization — `sanitize_label` covers a title, with one gap

`sanitize_label` (`add.rs:80-88`) maps control chars → space, filters `"` and
`\`, then collapses whitespace. Against a Claude-authored title that is
**sufficient for KDL safety**: `"` closes the string literal and `\` is KDL's
escape introducer, and those are the two characters the real-parser guardrail
`backslash_label_is_guarded_through_real_parser` exists to prove
(`crates/clave/tests/kdl_guardrail.rs`). Labels reach KDL at three sites —
`tab_node` (`add.rs:106`), `tab_node_bare` (`add.rs:127`) and the launch layout's
eager row (`setup.rs:199-205`) — so this is **generated-artifacts class**.

**The gap this design introduces:** `sanitize_label` does not touch U+00B7. A
title of `evil · injected` would fabricate a segment boundary, shifting every
downstream segment and — worse — breaking S5's "leading segment is the repo"
rule for that row. New helper, in `store.rs` beside `compose_label`:

```rust
fn sanitize_segment(s: &str) -> String {
    crate::add::sanitize_label(&s.replace('\u{b7}', "-"))
}
```

Replace the raw character, not the `" · "` sequence: whitespace is collapsed
*after* the replacement, so `"a  ·  b"` would otherwise reconstitute a
separator. Applied to **every rendered segment** — title, repo and summary. The
threat model is only really the title (Claude-authored, user-chosen) and the
summary (Claude-authored), but the rule holds at the type level rather than by
luck: `compose_label` cannot emit an unsanitized segment because every push goes
through the same function.

`rec.branch` is **not** passed through `sanitize_segment`, because it is no
longer a rendered segment. It keeps its existing treatment wherever it is
displayed today (`resume_candidates` sanitizes the composed picker line at
`add.rs:303,306`).

Title also gets the same length clamp the summary tier has — `first_words`'
char-boundary-safe 32-char cap (`hook.rs:107-110`) — factored into
`clamp_chars(s, 32)` and reused. A user-chosen title is otherwise unbounded and
would monopolise the row after §3.4 protects it from being dropped.

### 3.8 Rejected alternatives

| Rejected | Why |
|---|---|
| **Mutate `rec.cwd` from the payload** | it still scopes `spawn_mode` and project-dir-scoped resume (§3.2). **The "it is the transcript join key" half of this rationale is dead** — the transcript relocates and the label pipeline no longer derives its path (#79, spike §3.3) — but the surviving half is sufficient, and it is also why `merge_resume_record` is left alone |
| **Mutate `rec.repo_root` when the live cwd's repo changes** | drops the row out of `resume_candidates`' `repo_root ==` filter (`add.rs:275`) for the only repo it can be resumed into |
| **Add `LabelSource::None`** | a new wire string breaks `read_store` on any older binary — a *whole-store* parse failure, in exactly the window #43/#44 makes routine |
| **Shell out to `git rev-parse --abbrev-ref HEAD`** | a subprocess on Claude's turn path, with no timeout in this codebase, in a hook whose contract is DO NO HARM. The `HEAD`-file read is two `read_to_string`s and is unit-testable without `git` installed |
| **Recompute the title from the tail every event (no `rec.title`)** | flickers off when the last rename scrolls out of the 64 KiB window, and violates the `/clear` persistence ruling |
| **Truncate CLI-side, in the composed label** | the CLI knows neither `cols` nor S6's gutter width; the same string is the picker line, the `clave ls` line and the `launch.kdl` tab name |
| **Render from structured `Row` fields (the #24 "stop rendering the label string" reframe)** | correct destination, and **no longer unreachable** — design-lock §7.1 ruled a live row renders from the STORE, and #69 landed the fields. Still out of scope *for S4*, which owes the values; S5/S6 own the render (§3.5) |
| **`fit_label -> Vec<String>` handed to S5** | **withdrawn.** S5 declined it correctly: the segmented value never reaches a live row (§3.5). S4 ships `String` only |
| **Always emit a title slot (empty when unrenamed) so repo is invariably segment 1** | burns three cells on the majority of rows, and `parts.retain(\|p\| !p.is_empty())` / `sanitize_segment` would strip it anyway. §3.5's value-match resolves the ambiguity for free |
| **Keep branch as a fourth rendered segment** | superseded by the 2026-07-22 ruling. Independently justified: `clave/ab12cd34` was 13 of 27 cells and is the least discriminating string on the row — every sibling worktree of a repo shares its shape |
| **Per-segment shrinking (abbreviate a repo, elide a summary's middle)** | gold-plating. The drop rule already fits both 30 and 38 cols; abbreviation rules are a design question for #24's brainstorm, not a defect fix |
| ~~**Use `payload.transcript_path` instead of `jsonl_path(claude_dir, &rec.cwd, uuid)`**~~ | **WITHDRAWN — this is now MANDATORY (§4.3a/d), not an alternative.** The deferral read *"strictly better … but it is a fourth change; the derived path is proven in production today"*. Both halves were wrong: it is a **correctness** requirement, not an optimisation, and the derived path is proven **broken** the moment a session relocates (#79, spike §3.3). Recorded rather than deleted because "the derived path is proven in production" is exactly the kind of claim a later reader would re-adopt |
| **Track the transcript by `live_cwd` instead of by `transcript_path`** | it re-derives what we are told, one relocation later. The payload names the file; munging a cwd only guesses at it, and §3.2's correction is the price of the guess. `live_cwd` feeds branch derivation and diagnosis, nothing else |
| **Rename `LabelSource::Summary` to `AiTitle` now the tier is retargeted** | the serialized string changes `"summary"` → `"ai_title"`, which is the same whole-store `read_store` failure on an older binary as the rejected `None` variant. The wire token stays; §4.4(a)'s doc comment carries the mapping (§3.3) |
| **Let `last-prompt` overwrite an already-earned summary** | `lastPrompt` is maintained live (1340 occurrences across 40 transcripts), so the label would change on **every** prompt and fire a tab rename with it — reintroducing exactly the churn §5.3 exists to bound. Earn-once, same guard as the payload path (§3.3) |
| **Drop `head.rs` now that `worktree-state` carries the branch** | the line is emitted only for sessions **Claude** placed in a worktree. A plain checkout, and a worktree created by `clave add --worktree`, never carry it. `worktree-state` is a fast path in front of `head.rs`, not a replacement for it (§3.2, spike §3.4) |
| **A store schema version to guard the mixed-version strip (§3.6)** | #69 §2.1 rules out a version field and a migration framework outright, and a guard that refuses to write is worse than a write that heals. S4 takes the re-derivation path instead, and states its residual (§3.6) |
| **Read the title from `PaneInfo.title`** | the bar discards pane titles (`clave-bar/src/main.rs:458-463`), the value is Claude's TUI chrome not a record, and nothing would reach a *dormant* row |

---

## 4. Implementation

Change class: **generated artifacts** (labels reach `launch.kdl` and the one-shot
tab layout) **+ pure logic**. Per the taxonomy (`docs/dev/TESTING.md`), that
means TDD red-first, the real-parser guardrail, and `cargo test --workspace`.

### 4.1 `crates/clave-types/src/lib.rs` — the separator constant

Add, above `Status`:

```rust
/// The one label segment separator (§6.4 / S4). Space, U+00B7 MIDDLE DOT,
/// space. Exported here so the CLI's composer and the bar's width fitter
/// reference ONE constant — two string literals is how the byte-for-byte
/// label coupling rotted the first time.
pub const LABEL_SEP: &str = " \u{b7} ";
```

No struct changes. `Agent`/`AgentSnapshot` are untouched.

### 4.2 `crates/clave/src/head.rs` — new module, branch from `HEAD`

`pub mod head;` in `lib.rs` (alphabetical, between `evlog` and `hook`).

```rust
//! Branch from the filesystem, with no `git` subprocess.
//!
//! The hook runs on Claude's turn path and its contract is DO NO HARM
//! (hook.rs:1-5): a `Command` with no timeout can block Claude indefinitely.
//! `.git/HEAD` is two reads and a bounded upward walk, and it is testable in
//! CI with no `git` binary on the box — which matters while Tier 2 does not
//! exist (#47).

/// The branch of the worktree containing `cwd`, or `None` when it cannot be
/// determined. Detached HEAD returns `Some("-")`, matching the convention at
/// add.rs:305,510.
pub fn head_branch(cwd: &std::path::Path) -> Option<String>;

/// The `.git` entry governing `cwd`, resolved through a linked worktree's
/// `gitdir:` pointer. Split out so it is testable on its own.
fn git_dir_for(cwd: &std::path::Path) -> Option<std::path::PathBuf>;
```

Behaviour, spelled out because the tests pin it:

1. Walk `cwd` and its ancestors (bounded by the filesystem root) for a `.git`
   entry. First hit wins.
2. `.git` is a **directory** → git dir = that directory.
3. `.git` is a **file** → read it, expect `gitdir: <path>` (git's linked-worktree
   pointer). A relative `<path>` resolves against the `.git` file's parent. Git
   dir = the resolved path.
4. Read `<gitdir>/HEAD`. `ref: refs/heads/<name>` → `Some(name.trim())`.
   40 hex chars → `Some("-")` (detached). Anything else → `None`.
5. Any I/O error at any step → `None`. Never panics, never blocks.

Note for a reviewer: git's `HEAD` may name a ref outside `refs/heads/` (a
`ref: refs/remotes/…` during some operations). Strip the `refs/heads/` prefix
when present; otherwise return the ref path's last component. The caller only
displays it.

### 4.3 `crates/clave/src/hook.rs` — the payload, the tiers, the gates

**(a) `HookPayload` gains `cwd` and `transcript_path`** — replacing `hook.rs:23-34`:

```rust
#[derive(Debug, Default, Deserialize)]
pub struct HookPayload {
    #[serde(default)]
    pub session_id: Option<String>,
    #[serde(default)]
    pub prompt: Option<String>,
    #[serde(default)]
    pub message: Option<String>,
    /// EVERY Claude Code hook event carries `cwd` — the agent's CURRENT
    /// working directory, which may differ from the cwd the session was
    /// created in (the maintainer cd's between worktrees mid-session, RC-F).
    /// Observation only: `rec.cwd` is the transcript join key and is never
    /// rewritten from this (S4 §3.2). Optional like every other field — a
    /// payload without it degrades to "branch unchanged".
    #[serde(default)]
    pub cwd: Option<String>,
    /// The transcript Claude is writing RIGHT NOW. MANDATORY input, not an
    /// optimisation (#79): Claude RELOCATES a session's .jsonl into a project
    /// directory keyed on the new cwd when the session changes directory, so
    /// `jsonl_path(claude_dir, &rec.cwd, uuid)` reads an abandoned directory
    /// from that moment on — silently, forever (S4 §3.2). Deriving the path
    /// guesses at something every hook event tells us. Absent → no tail read
    /// this event; the held title/summary values stand.
    #[serde(default)]
    pub transcript_path: Option<String>,
}
```

`transcript_path` is a Claude Code payload field, deserialized like every other:
optional, and a `None` degrades to "no update this event" rather than to a wrong
value.

**(b) four tail extractors**, replacing and joining `summary_from_tail`
(`hook.rs:116-130`). All four share one shape — `.lines().rev().find_map(…)` over
a local `#[derive(Deserialize)] struct Line` matching on `type` and filtering
`!s.trim().is_empty()` — so they are one generic helper plus four thin callers,
not four parsers:

```rust
/// Scan a jsonl TAIL for the LAST NON-EMPTY
/// `{"type":"custom-title","customTitle":…}` line — Claude's session rename,
/// re-appended latest-wins (61 records in one observed transcript). Empty
/// values are SKIPPED, not returned: `/clear` appends an empty one and the
/// maintainer's ruling (#24, 2026-07-21) is that clave holds the last real
/// rename across it.
pub fn custom_title_from_tail(tail: &str) -> Option<String>;

/// The LAST NON-EMPTY `{"type":"ai-title","aiTitle":…}` — Claude's rolling
/// auto-description, and the summary tier's real source (#79). REPLACES
/// `summary_from_tail`, whose `{"type":"summary"}` line type appears in 0 of
/// 919 transcripts and has never once fired in production (S4 §1.3).
pub fn ai_title_from_tail(tail: &str) -> Option<String>;

/// The LAST NON-EMPTY `{"type":"last-prompt","lastPrompt":…}` — the fallback
/// source for the summary segment when no `ai-title` exists yet and the event
/// carried no `payload.prompt` (S4 §3.3). EARN-ONCE at the call site: this
/// value is maintained live and would otherwise rewrite the label every turn.
pub fn last_prompt_from_tail(tail: &str) -> Option<String>;

/// The LAST `{"type":"worktree-state", …}` — `worktreePath`, `worktreeName`,
/// `worktreeBranch`, `originalCwd`. Present for any session CLAUDE placed in a
/// worktree (384 occurrences across the 40 newest transcripts), absent for a
/// plain checkout and for a `clave add --worktree` tree. Zero-cost provenance:
/// the tail is already in memory (#79, spike §3.4). Feeds branch derivation
/// ahead of `head::head_branch`, and hands S6 its marker inputs.
pub fn worktree_state_from_tail(tail: &str) -> Option<WorktreeState>;

pub struct WorktreeState {
    pub path: Option<String>,
    pub name: Option<String>,
    pub branch: Option<String>,
}
```

`summary_from_tail` is **deleted, not kept as a third tier.** Keeping a scanner
for a line type that provably does not exist is dead code that reads as a working
feature — it is what made §1.3's defect survive this long. Its test
(`summary_from_tail_takes_last_summary_line`, `hook.rs:454-464`) is re-pointed at
`ai_title_from_tail`, not deleted (§5.2): it asserts *last-non-empty-wins over a
tail*, which is still the contract; only the line type it feeds changed. Every
`Summary`-tier assertion in the suite is in the same position — hand-written
fixtures of a shape Claude no longer writes, so they were green against fiction.
**Re-point them; do not delete them.**

**(c) `refresh_label` is rewritten.** Replacing `hook.rs:148-199` in full. The
replaced hard-stop and prefix rebuild:

```rust
    // Once a summary named the session, it stays (§6.4: stop re-scanning).
    if rec.label_source == LabelSource::Summary {
        return false;
    }
    let dir = rec
        .cwd
        .rsplit('/')
        .next()
        .filter(|s| !s.is_empty())
        .unwrap_or(&rec.cwd);
    let prefix = crate::add::sanitize_label(&format!("{dir} · {}", rec.branch));
```

The new body, in order:

```rust
fn refresh_label(rec, event, payload, jsonl_tail) -> bool {
    let before = rec.label.clone();

    // §3.6 re-derivation: a row whose `summary` is empty while label_source
    // says Summary either predates the field or was stripped by an OLDER
    // BINARY's read-modify-write (FOOTGUNS, #69/#59). Put it back in state U
    // so the next tail re-earns it. This is the self-heal, not a nicety.
    if rec.summary.is_empty() && rec.label_source == LabelSource::Summary {
        rec.label_source = LabelSource::FirstPrompt;
    }

    // NOT a rendered segment — live cwd → branch, stored only (§3.2).
    // Label-bearing events only.
    if matches!(event, "Stop" | "UserPromptSubmit") {
        if let Some(cwd) = payload.cwd.as_deref() {
            rec.live_cwd = Some(cwd.to_string());
        }
        // Tier 1: the transcript already states it, for free (§3.2, #79).
        let wt = jsonl_tail.and_then(worktree_state_from_tail);
        if let Some(b) = wt.as_ref().and_then(|w| w.branch.clone()) {
            rec.branch = b;
        // Tier 2: two file reads, no subprocess. Plain checkouts land here.
        } else {
            let probe = rec.live_cwd.as_deref().unwrap_or(&rec.cwd);
            if let Some(b) = crate::head::head_branch(Path::new(probe)) {
                rec.branch = b;
            }
        }
        // Tier 3 is doing nothing: `rec.branch` is left as it was.
    }

    // segment 0 — title. Held on absent/empty/injected (§3.3).
    if let Some(t) = jsonl_tail
        .and_then(custom_title_from_tail)
        .filter(|t| !is_harness_injected(t))
    {
        rec.title = Some(clamp_chars(&t, 32));
    }

    // segment 2 — summary. The §3.3 state machine, retargeted to `ai-title`.
    // NOT gated on label_source: ai-title ROLLS, and freezing it on first
    // sight discards the only reason to read it.
    if let Some(ai) = jsonl_tail
        .and_then(ai_title_from_tail)
        .filter(|s| !is_harness_injected(s))
    {
        rec.summary = first_words(&ai);
        rec.label_source = LabelSource::Summary;
    } else if rec.summary.is_empty() {
        // EARN-ONCE fallbacks, in payload-then-tail order. label_source stays
        // FirstPrompt for both — neither is an ai-title.
        let candidate = payload
            .prompt
            .as_deref()
            .filter(|p| !p.trim().is_empty())
            .map(str::to_owned)
            .filter(|_| event == "UserPromptSubmit")
            .or_else(|| jsonl_tail.and_then(last_prompt_from_tail));
        if let Some(p) = candidate.filter(|p| !is_harness_injected(p)) {
            rec.summary = first_words(&p);
        }
    }

    rec.label = rec.compose_label();
    rec.label != before          // ← the churn firewall (§5.3)
}
```

**Three things to hold onto in that body**, because each replaces a claim an
earlier draft made:

1. **The `ai-title` branch is unconditional on `label_source`.** State A
   self-loops with updates (§3.3). `label_source` is written as a *marker* — it
   is what §3.6's re-derivation reads after a stale-binary strip — not as a
   freeze.
2. **`is_harness_injected` guards all three earn paths**, not two. The title
   tier, the `ai-title` tier and the prompt/`last-prompt` fallback each pass a
   Claude-authored or harness-authored string, and #17's leak is that *any*
   unguarded path bakes a tag in forever (`FOOTGUNS.md`: "Earned labels stick,
   with no self-heal").
3. **`worktree-state` is read before `head_branch`, and its absence costs
   nothing** — the tail is already in memory for the two tiers above it.

**The return value is `rec.label != before`, not "did any input get re-read".**
That single line is what keeps §5.3's rename-churn bounded: re-deriving an
unchanged branch, or re-reading the same title, produces `false`, so
`apply_hook_event` does not bump `seq` (`hook.rs:254-257`), does not push a
snapshot (`hook.rs:291-293`), and no `Effect::RenameTab` is ever considered.

**A branch-only change therefore returns `false` — and that is correct, not a
lost write.** `with_store_mut` writes the store unconditionally at the end of
the closure (`store.rs:151-161`); the return value gates only the `seq` bump and
the snapshot push. So `rec.branch` and `rec.live_cwd` **persist to disk**, and
the bar's in-memory `Agent.branch` simply stays stale until some *other* change
pushes a snapshot. Nothing renders `branch` (§3.1), so no row is ever wrong on
screen. Pinned by `branch_change_alone_does_not_change_the_label` (§5.1) — which
asserts both halves: the record mutated, and the return was `false`.

**(d) the tail-read gate loses its `label_source` condition.** Replacing
`hook.rs:279-287`:

```rust
        let tail = s.agents.get(&uuid).and_then(|rec| {
            if rec.label_source == LabelSource::FirstPrompt
                && matches!(event, "Stop" | "UserPromptSubmit")
            {
                read_tail(&jsonl_path(&claude_dir, &rec.cwd, &uuid), 64 * 1024)
            } else {
                None
            }
        });
```

becomes an event-only gate **over the reported path**:

```rust
        // S4: the TITLE tier is live for the session's whole life, so the tail
        // is read on every label-bearing event regardless of label_source —
        // the old `== FirstPrompt` gate froze renames the moment a summary
        // landed. Cost: one 64 KiB read on Stop/UserPromptSubmit for TRACKED
        // agents only (the untracked fast path at hook.rs:270-272 already
        // returned).
        //
        // The path comes from the PAYLOAD, not from rec.cwd (#79). Claude
        // RELOCATES a transcript when the session changes directory — the
        // whole .jsonl moves into a project dir keyed on the NEW cwd, history
        // and all — so the munged rec.cwd derivation reads an abandoned
        // directory from that moment on, silently and forever. Deriving a path
        // we are handed on every event was never worth the failure mode.
        let tail = matches!(event, "Stop" | "UserPromptSubmit")
            .then(|| payload.transcript_path.as_deref())
            .flatten()
            .and_then(|p| read_tail(Path::new(p), 64 * 1024));
```

Two consequences worth naming:

- **The gate no longer consults the store at all.** The `s.agents.get(&uuid)`
  lookup existed only to reach `rec.cwd`; the untracked fast path at
  `hook.rs:270-272` has already returned by this point, so nothing is lost.
- **`jsonl_path` / `munge_cwd` leave the label pipeline entirely.** They remain in
  `spawn.rs` for resume-vs-create arbitration, which is where §3.2's newly
  exposed relocation hazard now lives — not here.

**(e) `clamp_chars`** — extract the tail of `first_words` (`hook.rs:107-110`) so
the title tier reuses it:

```rust
pub fn clamp_chars(s: &str, max: usize) -> String   // char-boundary safe
```

`first_words` becomes `clamp_chars(&joined, 32)`; behaviour identical, which is
why `first_words_clamps` (`hook.rs:444-452`) still passes unmodified.

### 4.4 `crates/clave/src/store.rs` — the record, the composer

**(a) `LabelSource`'s doc comment** — replacing `store.rs:25-27`:

```rust
/// Where an agent's label came from (§6.4). While `FirstPrompt`, `clave hook`
/// keeps tail-scanning the jsonl for a session summary; once `Summary`, it
/// stops re-scanning forever (the label only meaningfully changes once).
```

becomes:

```rust
/// Which tier produced the label's **summary segment** (§6.4, re-scoped by S4).
/// It says NOTHING about the repo/title/branch segments, which are recomposed
/// on every label-bearing hook event and are never frozen. `summary == ""`
/// means "not earned yet" — deliberately expressed on the defaulted #69 field
/// rather than a third variant here, because a new wire string would fail
/// `read_store` on any older binary (#43/#44 mixed-binary window).
///
/// `FirstPrompt`: the segment came from the first real user prompt (payload) or
/// the transcript's `last-prompt`, and the tail is still scanned for an
/// `ai-title` that would replace it.
/// `Summary`: the segment came from Claude's `ai-title`. NOT terminal — an
/// ai-title is a ROLLING description and a newer one replaces the held value.
///
/// NAMING, deliberate (#79): the variant is still spelled `Summary` and still
/// serializes as `"summary"`, but the `{"type":"summary"}` transcript line it
/// was named after is EXTINCT — 0 of 919 transcripts, and no agent ever
/// reached this state before S4. Renaming it would change the serialized token
/// and fail `read_store` on an older binary, which is the same defect the
/// third variant was rejected for. Read it as "the Claude-authored tier".
///
/// It also serves as a MARKER: `Summary` is a value only a post-S4 binary
/// writes, so `summary.is_empty() && label_source == Summary` is the reliable
/// signal that an older binary's read-modify-write stripped an earned value
/// (S4 §3.6) — and the trigger for re-deriving it from the tail.
```

**(b) `AgentRecord` gains ONE field** (after `summary`, `store.rs:83`). `title`
and `summary` are already there — #69 landed them (`store.rs:70-83`) along with
their `Agent` counterparts and the `snapshot_from` projection. S4 **fills** them;
it does not declare them. Their doc comments already point here ("written by S4
(#59) from `ai-title`, the `type:"summary"` tier being extinct (#79)"), so S4's
only obligation to those two is to make the comments true.

```rust
    /// The cwd Claude LAST REPORTED in a hook payload. An OBSERVATION, and
    /// deliberately NOT a path source: `cwd` above scopes `spawn_mode` and
    /// `claude --resume`, and the TRANSCRIPT path comes from the payload's
    /// `transcript_path` because Claude relocates transcripts on a cwd change
    /// (#79). This field feeds the branch re-derivation where `worktree-state`
    /// is absent, and makes a mid-session `cd` — and therefore a relocation —
    /// visible in the raw store for diagnosis (S4 §3.2).
    #[serde(default)]
    pub live_cwd: Option<String>,
```

**(c) `compose_label`** — new `impl AgentRecord`, beside the struct:

```rust
impl AgentRecord {
    /// The §6.4 / S4 label grammar, ruled 2026-07-22: `title · repo · summary`.
    /// Absent segments contribute no separator. TEXT ONLY — the status /
    /// battery / worktree glyphs are the GUTTER's (S6) and never appear here,
    /// because this string is also the zellij TAB NAME and the launch.kdl tab
    /// node.
    ///
    /// `branch` is deliberately ABSENT: it is still derived and stored (it is
    /// a wire field and a picker input) but it renders nowhere and consumes no
    /// width.
    ///
    /// THE one composer — add.rs and hook.rs both call it, which is what
    /// retires the byte-for-byte prefix coupling add.rs:703-708 used to demand.
    pub fn compose_label(&self) -> String {
        // repo segment: basename(repo_root) → repo_root → cwd basename, each
        // taken only if non-empty. The UUID fallback below is the final
        // backstop that makes `compose_label_never_returns_empty` actually
        // hold (CodeRabbit 2026-07-22: the earlier draft fell back only to
        // repo_root, so an empty/control-only repo_root produced an empty tab
        // name — an invalid KDL `tab name=""`).
        let repo_basename = self.repo_root.rsplit('/').next().unwrap_or("");
        let repo: &str = [repo_basename, self.repo_root.as_str()]
            .into_iter()
            .find(|s| !s.trim().is_empty())
            .unwrap_or("");
        let mut parts: Vec<String> = Vec::with_capacity(3);
        if let Some(t) = &self.title { parts.push(sanitize_segment(t)); }
        parts.push(sanitize_segment(repo));
        if !self.summary.is_empty() { parts.push(sanitize_segment(&self.summary)); }
        parts.retain(|p| !p.is_empty());
        if parts.is_empty() {
            // Everything sanitized to empty (control-only inputs, or no repo at
            // all). Guarantee a non-empty, KDL-safe name from the uuid prefix —
            // `sanitize_segment` cannot empty a hex prefix.
            parts.push(sanitize_segment(&self.uuid.chars().take(8).collect::<String>()));
        }
        parts.join(clave_types::LABEL_SEP)
    }
}

/// Segment-safe sanitization: `sanitize_label`'s KDL rules PLUS the segment
/// separator itself. A Claude-authored title containing U+00B7 would forge a
/// segment boundary — shifting every downstream segment and breaking S5's
/// "leading segment is the repo" colour rule. Replace the raw character, not
/// the ` · ` sequence: sanitize_label collapses whitespace afterwards, so
/// `"a  ·  b"` would otherwise reconstitute a separator.
pub fn sanitize_segment(s: &str) -> String {
    crate::add::sanitize_label(&s.replace('\u{b7}', "-"))
}
```

Degenerate case to guard: if `repo` sanitizes to empty (a `repo_root` of only
control characters — pathological but not impossible), `parts` is empty and the
label is `""`. `compose_label` falls back to `self.uuid`'s first 8 characters
rather than returning an empty tab name, which zellij renders as a nameless tab.
Pinned by `compose_label_never_returns_empty` (§5.1).

Placement note: this is display logic in the serde/IO module. It lives here
because `AgentRecord` lives here and both callers are outside; the alternative
(a fourth new module) buys nothing. `store.rs` already reaches into `add.rs`'s
sibling namespace in the same direction `hook.rs` does (`crate::add::sanitize_label`).

**(d) every `AgentRecord` literal gains `live_cwd`.** `title` and `summary` were
added to every literal by #69; only S4's one new field remains. `AgentRecord`
derives no `Default`, so this is mechanical and exhaustive — the compiler
enumerates it (line numbers are pre-#69 and will have moved):

```text
crates/clave/src/store.rs:372          (test rec)
crates/clave/src/hook.rs:304           (test rec)
crates/clave/src/add.rs:741            (run_add's `fresh` — PRODUCTION)
crates/clave/src/add.rs:773            (test rec)
crates/clave/src/lsview.rs:35          (test rec)
crates/clave/src/open.rs:145           (test rec)
crates/clave/src/dev.rs:229            (scenario seeding — PRODUCTION)
crates/clave/src/setup.rs:792,828,989,1255   (tests)
crates/clave/tests/kdl_guardrail.rs:60 (eager_record)
```

All test literals take `live_cwd: None` (on top of #69's `title: None,
summary: String::new()`). `add.rs:741` and `dev.rs:229` are covered in §4.5 /
§4.6.

`merge_resume_record` (`add.rs:343-354`) uses `..row.clone()`, so `title`,
`summary` and `live_cwd` are preserved on resume **for free** and with the same
rationale as cwd/label — the earned title and summary are state a re-add has no
business resetting. `merge_resume_preserves_existing_row_and_resets_status`
(`add.rs:889-922`) is extended to assert it (§5.2), not changed in intent.

### 4.5 `crates/clave/src/add.rs` — one composer, coupling deleted

**(a)** replace the label derivation at `add.rs:695-711`:

```rust
    let label = match &existing {
        Some(row) => sanitize_label(&row.label),
        None => {
            let dir_name = agent_cwd.rsplit('/').next().unwrap_or(&agent_cwd);
            // CROSS-TASK COUPLING (Task 5): a NEW agent's label MUST be
            // exactly `<dir_name> · <branch>` (space-middot-space) —
            // hook.rs::refresh_label reconstructs this prefix byte-for-byte
            // to gate the first-prompt upgrade. …
            sanitize_label(&format!("{dir_name} · {agent_branch}"))
        }
    };
```

The `existing` arm is unchanged in effect (the row's earned label is reused).
The `None` arm becomes: build the `fresh` record first, then
`fresh.compose_label()`. Structurally this means moving the `AgentRecord`
construction (currently inside the `with_store_mut` closure at `add.rs:741-754`)
*above* the layout write at `add.rs:713-731`, because the layout bakes the label.
The record is still **inserted** inside the lock — only its construction moves,
and `merge_resume_record`'s authoritative in-lock `existing` lookup
(`add.rs:738-739`) is untouched.

New `fresh` fields: `live_cwd: None` (alongside #69's `title: None`,
`summary: String::new()`, already present). A `--worktree`
agent is now born `clave` (one segment) instead of `ab12cd34 · clave/ab12cd34` —
**that is the reported fix**, visible at creation and not only after the first
hook event.

**(b)** the Task-5 coupling comment (`add.rs:703-708`) is deleted. Say so in the
PR: it is the load-bearing rationale for a constraint this spec removes, and a
reviewer who sees it vanish without explanation will (correctly) flag it.

**(c) `resume_candidates`' worktree branch suffix must be restored — a real
regression, caught by the format change.** `add.rs:301-310` reads:

```rust
            let label = if is_worktree {
                if store_label.is_some() {
                    sanitize_label(&format!("{base} (wt)"))
                } else {
                    let br = branch.as_deref().unwrap_or("-");
                    sanitize_label(&format!("{base} · {br} (wt)"))
                }
            } else {
                base
            };
```

and its rationale at `add.rs:296-300` is explicit: *"an EARNED store label
already encodes its branch (hook.rs writes `dir · branch · summary`), so it is NOT
re-appended — only the `(wt)` marker is."* **That premise is now false.** With
branch out of the grammar, an earned label encodes no branch, so every worktree
candidate in the fzf picker would read `clave (wt)`, `clave (wt)`, `clave (wt)` —
indistinguishable, which is precisely the fugu 2026-07-21 HIGH this arm was
written to fix.

The two arms collapse into one:

```rust
            let label = if is_worktree {
                // S4 (2026-07-22): the stored label no longer carries the
                // branch (the row format moved worktree provenance to the
                // gutter marker), so an EARNED label needs the branch appended
                // exactly like a bare-uuid candidate does — otherwise every
                // worktree of a repo picks as an identical `<repo> (wt)`.
                let br = branch.as_deref().unwrap_or("-");
                sanitize_label(&format!("{base} · {br} (wt)"))
            } else {
                base
            };
```

The picker is the **only** human-facing place `branch` still appears, which is
the second half of why §3.2 keeps deriving it.

**(d)** nothing else in `add.rs` changes. `merge_resume_record`,
`sanitize_label`, `validate_cwd` and the picker weave are untouched.

**(e) `lsview.rs:17-21`** prints `"{glyph} {label}  ({repo_root})"`. The repo now
appears in both columns. Cosmetic, not load-bearing — **left alone**, and named
here so a reviewer sees it was considered rather than missed.

### 4.6 `crates/clave/src/dev.rs` — scenario records

`dev.rs:229-243` seeds scenario rows. Add `live_cwd: None` (#69 already added
`title` / `summary`). The
scenario labels are currently literal strings; leave them — the first hook event
in a `clave dev launch` recomposes them, and that recomposition is itself worth
watching during sandbox validation.

### 4.7 `crates/clave-bar/src/model.rs` — the width fitter

Pure addition; **no change to `Row`, `rows()`, `apply_snapshot` or the rename
block** (`model.rs:563-582`). This is what keeps S4 and S5 out of each other's
diffs.

```rust
/// Fit a composed label (S4 §3.4) into `budget` display cells.
///
/// The grammar (`title · repo · summary`) puts the SUMMARY last and makes it
/// the only optional trailing segment, so the give-way rule needs no
/// knowledge of what any segment MEANS:
///
///   drop trailing segments while the join exceeds `budget`
///   AND more than two segments remain; then tail-truncate with `…`.
///
/// Consequences, all intended:
///   * the summary is the give-way segment (the 2026-07-22 ruling);
///   * the title and the repo are never dropped — both are identity-bearing,
///     and the repo additionally anchors S5's colour match (§3.5);
///   * when even `title · repo` overflows, the TITLE survives whole and the
///     repo truncates from the right (`F-CLA · cla…`) — the repo's identity
///     is redundantly carried by colour (S5) and the gutter marker (S6), and
///     the surviving text is still a PREFIX of basename(repo_root).
///
/// `budget` is a PARAMETER, never a constant: S6 owns the gutter width and
/// S8 is moving the pane from 30 to ~38 columns. This function is correct at
/// every budget including 0.
///
/// Robust to non-label input: a hand-renamed zellij tab has no separators,
/// splits to one segment, and falls straight through to the tail-truncate —
/// exactly today's behaviour (model.rs:1326-1353 pins that we do not fight a
/// manual rename by re-issuing one).
pub fn fit_label_str(name: &str, budget: usize) -> String;
```

The whole implementation:

```text
if budget == 0 { return "" }               // explicit zero branch — see below
segments = name.split(LABEL_SEP)
while segments.len() > 2 && join(segments).chars().count() > budget:
    segments.pop()
tail_truncate(join(segments), budget)
```

**The `budget == 0` branch is explicit, not incidental (CodeRabbit 2026-07-22).**
The old `main.rs:547-553` body is `.chars().take(budget - 1)`; with `budget: usize`
and `budget == 0`, `budget - 1` **underflows to `usize::MAX`**, so `take` returns
the *whole* string — the opposite of the `""` the contract requires. The guarded
form returns `""` for budget 0, and for `budget ≥ 1` uses
`.chars().take(budget.saturating_sub(1))` + `…` (char-boundary safe), giving
budget 1 → `"…"` and budget ≥ 2 → `budget-1` content chars + `…`. This is the one
budget-zero contract; S5's `clamp_name` and S6's collapsed geometry match it
(S6 §2.8's `const_assert` is `<=` so budget 0 is a valid collapsed outcome).

**Budget-zero rendering contract — one rule, applied by every workstream
(CodeRabbit 2026-07-22).** To stop S4's `fit_label_str` and S5's `clamp_name` /
collapsed expectations from disagreeing, the single contract is: **budget 0 →
`""` (empty, no ellipsis); budget 1 → `"…"`; budget ≥ 2 → up to `budget−1`
content chars plus `…`.** S5's `clamp_name` and S6's collapsed-mode geometry both
adopt this verbatim (S6 §2.8 already relaxed its `const_assert` from `==` to `<=`
so a budget of 0 is a valid collapsed outcome, not a build failure). Any span
paint runs against the *clamped* string, so a zero budget paints nothing rather
than leaking an escape.

**No positional inference of meaning is required, and none is performed.** That
is a deliberate simplification over the previous four-segment design, where
`repo · title · branch` and `repo · branch · summary` were indistinguishable at
three segments. The new grammar has no such collision, because the only optional
*trailing* segment is the summary. `title` being optional creates an ambiguity
only for a **consumer that needs to identify** the repo — which is S5, and §3.5
resolves it by value-match rather than by index.

Return type is `String`, not `Vec<String>`: the segmented form is withdrawn
(§3.5).

### 4.8 `crates/clave-bar/src/main.rs` — the render call site

Replacing `main.rs:544-553`:

```rust
            // Clamp the NAME to what's left after the 2-cell gutter, with a
            // trailing … (char-boundary safe; labels can be multibyte).
            let budget = cols.saturating_sub(3); // gutter + margin
            let name: String = if row.name.chars().count() > budget {
                let mut n: String = row.name.chars().take(budget.saturating_sub(1)).collect();
                n.push('…');
                n
            } else {
                row.name.clone()
            };
```

with:

```rust
            // Fit the NAME to the text budget. S4: drop the give-way segment
            // BEFORE truncating, so the distinctive part survives a narrow
            // pane (#24). Falls back to the old `…` tail-truncate for
            // non-label strings and once the floor is reached.
            //
            // GUTTER INTERFACE (S6): `gutter_cells` is the width of the glyph
            // column — status dot, context-battery slot, worktree marker —
            // which S6 owns entirely. S4 composes a TEXT-ONLY label and knows
            // nothing about it. Until S6 lands this stays the current
            // `2 + 1 margin`, so nothing regresses; S6 changes THIS EXPRESSION
            // and nothing else in the fit path.
            let budget = cols.saturating_sub(gutter_cells + 1); // + right margin
            let name = model::fit_label_str(&row.name, budget);
```

with, for now, `let gutter_cells = 2;` — the existing `cols.saturating_sub(3)`
restated so the seam is named rather than assumed. **S4 introduces no width
constant of its own and asserts none in any test.**

Three interfaces this call site now carries, all stated so the adjacent
workstreams can land independently:

| To | S4 guarantees | S4 assumes |
|---|---|---|
| **S6** (gutter) | the composed label is text-only — no glyph, no leading space, no colour | S6 sets `gutter_cells`; S4 never reads or renders a glyph |
| **S8** (width) | `fit_label_str` is correct at every budget, 0 upward | nothing — no width constant is read. (The old "tests pin 30 **and** 38" is dead: the ratified target is **44**, design-lock §2) |
| **S5** (colour) | `AgentRecord.title` and `.summary` are populated, so `snapshot_from` projects real values; repo text is `sanitize_segment(basename(repo_root))` | S5 lays its own fixed-width columns from those values (design-lock §2, §7.1) and never splits a rendered string — see §3.5 |

This is a two-line edit in the file S5 and S6 also edit (`main.rs:539-557`). The
rebase is mechanical; S5's colouring wraps the `name` this produces, S6's gutter
precedes it.

### 4.9 Documentation

- `docs/superpowers/specs/2026-06-30-clave-orchestrator-design.md` §6.4 — the
  label rule changes shape. Amend in place with a dated note, matching how §6.5
  and §6.6 carry their revisions.
- `docs/superpowers/spikes/SUBSYSTEM-VALIDATION.md` — per AGENTS.md, record the
  `custom-title` / `ai-title` / `worktree-state` / `.git/HEAD` findings in the
  ledger **in the same commit**, and the dead end explicitly: *a tier scanning
  `{"type":"summary"}` cannot fire; verify a transcript line type exists before
  building a state on it* (#79).
- `FOOTGUNS.md` already carries the three entries this spec depends on — the
  extinct `summary` tier, transcript relocation on a cwd change, and the
  older-binary read-modify-write strip. S4 does not re-document them; it links
  them (§3.2, §3.6).
- `docs/status/YYYY-MM-DD-HHMM-clave-orchestrator.md` — handoff duty.

---

## 5. Test plan

Risk taxonomy (`docs/dev/TESTING.md`): **generated artifacts** (labels reach
`launch.kdl` via `setup.rs:199-205` and the one-shot layout via `add.rs:106,127`)
**+ pure logic / model**. Required: TDD red-first, `cargo test --workspace`,
**the real-parser guardrail**, and — because Tier 2 does not exist (#47) — a
written argument plus an adversarial reviewer for anything crossing a seam. The
one seam S4 crosses is *reading a file Claude writes*, addressed in §5.4.

### 5.1 Tier 1 — new tests

**`crates/clave/src/head.rs`** (all with `tempfile`, no `git` binary required):

| Test | Pins |
|---|---|
| `head_branch_reads_plain_repo` | `.git/HEAD` = `ref: refs/heads/main` → `Some("main")` |
| `head_branch_reads_linked_worktree_gitdir_pointer` | `.git` **file** = `gitdir: <dir>/.git/worktrees/ab12` → reads that dir's `HEAD` → the worktree's branch, not the main one. **This is the reported bug's exact shape** |
| `head_branch_resolves_relative_gitdir` | a relative `gitdir:` resolves against the `.git` file's parent |
| `head_branch_walks_up_from_a_subdirectory` | `cwd = <repo>/crates/clave/src` finds `<repo>/.git` |
| `head_branch_detached_is_dash` | 40-hex `HEAD` → `Some("-")`, matching `add.rs:305,510` |
| `head_branch_outside_a_repo_is_none` | a bare tempdir → `None` (caller keeps `rec.branch`) |
| `head_branch_never_panics_on_garbage` | empty `HEAD`, binary `HEAD`, `.git` file with no `gitdir:`, a `gitdir:` pointing nowhere → all `None` |

**`crates/clave/src/hook.rs`**:

| Test | Pins |
|---|---|
| `custom_title_from_tail_takes_last_non_empty` | three `custom-title` lines, the last empty → the **middle** value wins. The `/clear` ruling, directly |
| `custom_title_from_tail_ignores_other_record_types` | `ai-title` / `agent-name` / `user` lines are not titles. **`custom-title` is the user's rename and outranks them** (§3.3) |
| `ai_title_from_tail_takes_last_non_empty` | the retargeted tier's extractor, over a realistic tail. Re-pointed from `summary_from_tail_takes_last_summary_line` (§5.2) |
| `ai_title_tier_keeps_upgrading` | two `ai-title` values in sequence → the segment tracks the **second**. State A self-loops with updates (§3.3); the old design froze here |
| `last_prompt_earns_only_when_unearned` | a tail with a `last-prompt` and an already-earned `summary` → **unchanged**. The churn guard on the live-maintained fallback (§3.3) |
| `last_prompt_earns_on_a_stop_with_no_payload_prompt` | state U + `Stop` + a tail `last-prompt` → state P. The gap the fallback exists for |
| `ai_title_outranks_last_prompt_in_the_same_tail` | both present → `ai-title` wins and `label_source` becomes `Summary` |
| `title_survives_a_tail_without_one` | `rec.title = Some("F-CLA")`, a tail with **no** `custom-title` → still `Some("F-CLA")`. The 64 KiB-window flicker guard (§3.3) |
| `title_tier_stays_live_after_summary` | drive to state **A**, then feed a tail with a new title → the label changes. **The §1.3 obstacle, red-first** |
| `branch_refreshes_from_the_payload_cwd` | `payload.cwd` in a temp worktree, tail with **no** `worktree-state` → `rec.branch` and `rec.live_cwd` update via `head.rs`, `rec.cwd` **unchanged** |
| `worktree_state_branch_wins_over_head_walk` | a tail carrying `worktreeBranch` **and** a `live_cwd` whose `.git/HEAD` says something else → the transcript value wins, and `head_branch` is not consulted. The zero-cost tier (§3.2, spike §3.4) |
| `worktree_state_absent_falls_back_to_head` | plain-checkout tail → tier 2. **Pins that `head.rs` is not dead** |
| `branch_refresh_is_skipped_on_non_label_events` | `Notification` / `SessionEnd` → no `live_cwd` write, no branch read |
| `refresh_label_returns_false_when_nothing_changed` | same event twice → second returns `false`. **The churn firewall (§5.3)** — without it `seq` bumps and a snapshot pushes on every turn |
| `stale_binary_strip_is_re_derived_from_the_tail` | `summary: "", label_source: Summary` — the shape an older binary's read-modify-write leaves behind (§3.6) — → source resets to `FirstPrompt` and the next tail re-earns it. **The self-heal, and the only thing standing between an earned summary and silent loss in a mixed-version window** |
| `label_is_repo_not_cwd_basename` | `cwd = /r/.claude-worktrees/issue-10-kdl-guardrail`, `repo_root = /r` → the label names `r`. **The reported symptom, as a unit test** |
| `branch_change_alone_does_not_change_the_label` | change the branch via a temp worktree, keep title/summary fixed → `rec.branch` updates, `refresh_label` returns **`false`**. Branch is derived and stored but not rendered (§3.1) — and this is also the churn firewall's tightest case |
| `injected_custom_title_does_not_earn` | `customTitle = "<system-reminder …>"` → `rec.title` stays `None` |
| `injected_ai_title_does_not_earn` | same, through the retargeted tier — the earn path that did not exist when #17 was written |
| `tail_is_read_from_the_payload_transcript_path` | `payload.transcript_path` points at a file under a **different** munged project dir than `rec.cwd` would derive → the tail is read and the label upgrades. **Transcript relocation, as a unit test (§3.2)** |
| `absent_transcript_path_is_a_no_op` | no `transcript_path` in the payload → no tail read, held values stand, `refresh_label` returns `false` |

**`crates/clave/src/store.rs`**:

| Test | Pins |
|---|---|
| `compose_label_omits_absent_segments_without_double_separators` | all four presence combinations of (`title`, `summary`) — with `summary == ""` as the absent case, not `None` (§3.1) |
| `compose_label_orders_title_repo_summary` | the **binding** order, asserted literally: `F-CLA · clave · fix auth` |
| `compose_label_never_renders_the_branch` | a record with `branch = "clave/ab12cd34"` produces a label containing neither `clave/` nor `ab12cd34`. **This is the ruling, as a test** |
| `compose_label_puts_repo_first_when_there_is_no_title` | the majority shape — `clave · fix auth`. The §3.5 obligation-2 caveat, pinned so S5 cannot be surprised by it |
| `compose_label_repo_segment_is_verbatim_basename_of_repo_root` | S5's value-match guarantee (§3.5) |
| `sanitize_segment_strips_the_separator_character` | title `"evil · injected"` → one segment. **The S5 contract, and the forged-boundary attack (§3.7)** |
| `compose_label_survives_kdl_metacharacters_in_every_segment` | `"` and `\` in title and summary |
| `compose_label_never_returns_empty` | a pathological all-control-char `repo_root` falls back to a uuid prefix, never `""` (§4.4c) |
| `agent_record_loads_a_pre_s4_store_file` | JSON without `live_cwd` deserializes with `None`. (`title`/`summary` defaulting is #69's roundtrip test, `clave-types`, not re-asserted here) |

**`crates/clave-bar/src/model.rs`**:

Every case is asserted at **both** widths S8 brackets. The suite defines
`const NARROW_BUDGET` / `const WIDE_BUDGET` derived from 30 and 38 minus a
nominal gutter, with a comment saying they are *fixture* values, not contract —
S8 may move the pane and S6 may move the gutter without touching this file.

| Test | Pins |
|---|---|
| `fit_label_keeps_everything_when_it_fits` | `F-CLA · clave · Fix auth flow` at `WIDE_BUDGET` → unchanged |
| `fit_label_drops_the_summary_first` | the same string at `NARROW_BUDGET` → `F-CLA · clave`, summary gone, title and repo intact. **The ruling's give-way order** |
| `fit_label_keeps_the_summary_when_there_is_no_title` | `clave · Fix the auth flow now` at `NARROW_BUDGET` → 2 segments, **no drop**, tail-truncated (`clave · Fix the auth fl…`). The majority shape, and #24's original complaint |
| `fit_label_truncates_the_repo_not_the_title_when_two_segments_overflow` | a long title + repo at a tiny budget → the title survives whole, the repo is cut from the right. **The case the ruling asked about explicitly** |
| `fit_label_never_drops_below_two_segments` | 3-segment input at budget 1 → still 2 segments before the tail-truncate; the truncate does the rest |
| `fit_label_passes_through_a_hand_renamed_tab` | `"some-manual-name"` (no separators) → today's tail-truncate, at both budgets |
| `fit_label_str_is_char_boundary_safe` | multibyte segments, every budget `0..=48`, no panic, no partial code point |
| `fit_label_str_at_budget_zero_is_empty` | the collapsed-mode floor; no panic, no stray `…` wider than the budget |

**Proptest** (`clave-bar` has `proptest` as a dev-dep; the taxonomy demands a new
property when a new branch becomes reachable):

```rust
∀ segments (1..=3 arbitrary strings, none containing U+00B7), ∀ budget ∈ 0..80:
    let joined = segments.join(LABEL_SEP);
    let out    = fit_label_str(&joined, budget);
    out.chars().count() <= budget                          // never overflows the pane
 ∧ joined.starts_with(out.trim_end_matches('…'))           // output is always a HEAD of the input
 ∧ (budget >= joined.chars().count() → out == joined)      // no gratuitous truncation
 ∧ no panic
```

The second property is the one that matters for S5: the fitted string is always
a **prefix-preserving** reduction of the composed label, so the leading segment
(title when present, repo otherwise) is never something the composer did not
write, and a `starts_with` match against `basename(repo_root)` cannot be fooled.

### 5.2 Tier 1 — existing tests that change, and why

> **Read this first (2026-07-28, #79).** Every existing assertion about
> `LabelSource::Summary` in this suite was green against a **hand-written
> fixture** of a line type Claude does not emit — `{"type":"summary"}`, 0 of 919
> transcripts (§1.3). They were testing fiction, faithfully. **Re-point them at
> `ai-title`; do not delete them.** What they pin — last-non-empty-wins, the
> injected-prefix guard, tier precedence — is all still the contract; only the
> line type feeding it changed. A deleted test here is coverage lost for a
> behaviour that still exists.

| Test | File:line | Change | Why |
|---|---|---|---|
| `first_words_clamps` | `hook.rs:444-452` | **none** | `first_words` is refactored onto `clamp_chars` with identical behaviour. It passing unmodified is the evidence the refactor is behaviour-preserving |
| `summary_from_tail_takes_last_summary_line` | `hook.rs:454-464` | **re-pointed and renamed** → `ai_title_from_tail_takes_last_non_empty` | `summary_from_tail` is deleted with its extinct tier (§4.3b). The test's *contract* — last non-empty wins over a tail of mixed line types — is unchanged and still needed; only its fixture's `"type"` moves from `summary` to `ai-title` and its `"summary"` key to `"aiTitle"`. **Do not delete it**: it is the only place that pins the extractor's reverse scan |
| `refresh_label_upgrades_first_prompt_then_summary_then_stops` | `hook.rs:466-486` | **rewritten, re-pointed, and renamed** → `refresh_label_upgrades_first_prompt_then_ai_title_and_keeps_upgrading` | three things change. (i) Its fixture's `{"type":"summary"}` line becomes `{"type":"ai-title","aiTitle":…}` — the old one matched nothing Claude writes (§1.3). (ii) Its step 3 asserts `assert!(!refresh_label(&mut r, "Stop", &p, Some(tail)))` — "once Summary, we STOP re-deriving". That claim is now **wrong, not merely narrowed**: `ai-title` rolls, so a *second, different* ai-title must return `true` and replace the segment (§3.3). Step 3 becomes three assertions — identical tail → `false`; a **new ai-title** → `true` with a changed segment; a **new title** → `true` with a changed label. (iii) Its expected strings change from `"x · main · fix the flaky auth"` to `"x · fix the flaky auth"` — **the branch is gone from the grammar**. The `rec()` fixture has `cwd == repo_root == "/x"`, so the repo segment stays `x` and the diff isolates exactly the branch removal. **This is the single most important diff in the suite** — it encoded the frozen-label behaviour, the extinct tier *and* the four-segment grammar, and a reviewer must see all three change deliberately |
| `injected_task_notification_does_not_earn_label_but_next_prompt_does` | `hook.rs:488-515` | gate assertion updated **and** expected strings change | it asserts `r.label == "x · main"` and `r.label_source == FirstPrompt` as proxies for "still eligible". The proxy is now `r.summary.is_empty()` — the byte-compare gate is gone (§3.3) — and the bare label is now `"x"`, not `"x · main"`, since branch no longer renders. #17's property is unchanged and is now re-asserted **directly** rather than through a string proxy, which is strictly stronger |
| `every_injected_prefix_is_blocked_on_both_earn_paths` | `hook.rs:517-552` | extended → `..._on_all_four_earn_paths` | S4 opens **three** new doors on a guard written for one. CodeRabbit's original point on PR #25 was that a prefix guarding only one path is a latent re-leak; a *tier* guarding none is worse. Every prefix now drives the payload prompt, `ai-title`, `last-prompt` **and** `custom-title`, asserting `summary.is_empty()` **and** `title == None`. Its `assert_eq!(r.label, "x · main")` lines become `"x"` |
| `refresh_label_sanitizes_kdl_metacharacters` | `hook.rs:554-573` | extended | keep the prompt case; add a hostile **title** through the same assertion. Titles are Claude-authored text on the same KDL path |
| `merge_resume_preserves_existing_row_and_resets_status` | `add.rs:889-922` | **extended, not changed in intent** | `..row.clone()` already preserves them; the test must *say so*. Add `row.title = Some("F-CLA")`, `row.summary = "fix auth".into()`, `row.live_cwd = Some(…)` and assert all three survive. **The cwd/label/label_source assertions at `add.rs:913-916` are unchanged** — §3.2 does not touch `rec.cwd`, so no stated intentional decision is required here |
| `store_rows_without_live_tabs_render_dormant` | `model.rs:2085-2110` | **none** | it builds `a.label = "repo · main · fix"` by hand and asserts `d.name` round-trips it verbatim. `rows()` is untouched by S4 and the fitter runs at *render*, not in `rows()`. That this passes **unmodified — with a label in the old four-segment shape** — is the evidence that S4 stayed out of S5's and S6's seam and that `rows()` remains format-agnostic |
| `resume_candidates_*` worktree-label tests | `add.rs:925+`, `add.rs:1039`, `add.rs:1088` | expected picker strings change | §4.5(c) collapses the two `is_worktree` arms, so an earned-label worktree candidate now picks as `<repo> · <branch> (wt)` instead of `<repo> (wt)`. **Red-first here specifically** — the old assertion passing after the grammar change would mean the picker had silently gone ambiguous |
| `rename_only_when_label_changes_not_when_tab_name_differs` | `model.rs:1326-1353` | **none** | S4 does not touch the rename block (`model.rs:563-582`). §5.3 explains why more-frequent label changes do not invalidate it |
| every `AgentRecord` literal | §4.4(d) | one `live_cwd: None` | mechanical; the compiler enumerates them. `title` / `summary` were added by #69 |
| `eager_record` | `kdl_guardrail.rs:59-73` | `live_cwd` + populated `title`/`summary` + a new case | see §5.5 |

### 5.3 Rename churn — the analysis, and why it is bounded

`Effect::RenameTab` is emitted only when the label differs from what **this
instance last wrote** (`model.rs:574`, against `self.renamed`), and it is
executor-gated to the active instance (`main.rs:137`). The question is how often
the label now changes.

| New change source | Frequency | Bounded by |
|---|---|---|
| title | a handful of times per session (a rename is a deliberate human act) | — |
| ~~branch~~ | **never** — branch is not in the label (§3.1). A `git switch` updates the store and produces **no rename at all** | the grammar itself |
| summary, `ai-title` tier | **revised 2026-07-28**: no longer "at most twice". `ai-title` rolls — **373 occurrences across the 40 newest transcripts, ≈ 9 per session** — and §3.3 deliberately lets each new one replace the held value | the emission rate itself, plus the `first_words` 32-char clamp: many ai-titles differ only past the clamp and compose to the *same* label, which the firewall below turns into `false` |
| summary, prompt / `last-prompt` fallback | at most **once** — earn-once (§3.3) | `summary.is_empty()` |
| **re-derivation with no change** | every `Stop` / `UserPromptSubmit` | **`refresh_label` returns `rec.label != before`** (§4.3c) |

Dropping branch from the grammar removed the *only* per-`git switch` change
source this design would have introduced. The retargeted summary tier adds one
back, at a rate the earlier draft did not have to reason about — so state it
plainly: **~9 tab renames per session, worst case, spread across its whole life.**
That is the same order as the title tier and an order below a per-turn rename;
it is the price of a label that actually tracks what the agent is doing, which is
#24 item 3's entire ask. If it proves visible in live validation (§6 Step 7), the
lever is a *debounce on the composed label*, not a re-freeze of the tier — the
freeze is what §1.3 exists to remove.

That last row is the whole answer. An unchanged branch, an unchanged title and an
unchanged summary produce `false`, so `apply_hook_event` does not bump `seq`
(`hook.rs:254-257`) and `run_hook` does not push a snapshot (`hook.rs:291-293`).
No snapshot, no `apply_snapshot`, no rename considered. **The steady-state churn
is exactly zero**, and it is pinned by
`refresh_label_returns_false_when_nothing_changed` (§5.1).

When the label genuinely changes, one `rename_tab_with_id` fires, from one
instance.

Two interactions worth stating rather than discovering:

- **`model.rs:1326-1353` stays correct.** It pins "do not re-rename when the tab
  name diverged from the label" — i.e. clave does not fight a manual `zellij`
  rename by re-issuing the same name. Under S4 a manual rename still survives
  until the *label itself* changes, at which point clave overwrites it. That is
  the right resolution now that Claude's own rename is the sanctioned rename
  channel, and it is unchanged behaviour — only its frequency moves.
- **`self.renamed` is per-instance and is not cleared on `apply_snapshot`**
  (`model.rs:193`, initialised `model.rs:304`). A plugin hot-reload therefore
  reincarnates the model from scratch and re-renames every bound tab once
  with the same name. Pre-existing, unchanged, and worth knowing during live
  validation so it is not misread as churn.

### 5.4 The seam argument (required — Tier 2 does not exist, #47)

S4 crosses one seam: it reads a file **Claude Code writes**, and it trusts one
field of a JSON payload **Claude Code sends**. Nothing automated can execute that
today. The written argument, for the PR dossier:

1. **The read is not new.** `read_tail` + line-wise serde over the same 64 KiB of
   the same file has been in production since the summary tier. S4 adds one more
   record type to a parse that already tolerates every other line silently.
2. **Every failure degrades to "no change".** A missing file, a truncated tail, a
   mid-line split at the 64 KiB boundary, an unknown record shape, a `null`
   `customTitle` — all produce `None`, which leaves `rec.title` held and
   `refresh_label` returning `false`. There is no path from a malformed
   transcript to a wrong label, only to an unchanged one.
3. **`payload.cwd` is optional and only ever an observation.** Absent → branch
   unchanged. Present but nonsense → `head_branch` returns `None` → branch
   unchanged. It is never written to `rec.cwd`, so it cannot reach `munge_cwd`,
   `spawn_mode` or the resume path.
4. **`payload.transcript_path` is optional and fails closed.** Absent → no tail
   read this event → held title/summary stand → `false`. Present but pointing at
   a missing or unreadable file → `read_tail` yields `None` → same. It is used
   **only** to open a file for reading; it is never stored, never munged, never
   handed to `spawn`. A hostile value is a failed read.
5. **Every line type this spec reads is verified on disk, with counts**, not
   assumed — `custom-title` (#24 comment 2026-07-21, 61 records in one
   transcript; 1057 across 40), `ai-title` (373), `last-prompt` (1340),
   `worktree-state` (384), and the *absence* of `summary` (0 of 919). See
   [`2026-07-28-agentsnapshot-v2-design.md`](2026-07-28-agentsnapshot-v2-design.md)
   §3.6 for the reproduction commands. **That measurement is itself the lesson**:
   the extinct tier survived a full design round because nobody counted, so a
   reviewer should treat "Claude writes X" as a claim requiring a count.
6. **The remaining unverified assumptions are two**, and both are Live Step 2's:
   that every hook event carries `cwd`, and that every hook event carries
   `transcript_path`. `cwd`'s absence is a no-op. **`transcript_path`'s absence is
   not** — it would leave the label pipeline with no path at all, and the
   fallback would have to be the derived path this spec just proved unsafe after
   a relocation. Step 2 must report it explicitly.
7. **No subprocess is added to the hook.** §3.8 rejects the `git` shellout for
   exactly this reason, and the `worktree-state` tier removes even the filesystem
   walk for the common worktree case.

Adversarial reviewer brief: attack (a) the `gitdir:` resolution against real git
worktree and submodule layouts, (b) whether any path can make `refresh_label`
return `true` on an unchanged label, (c) whether any composed label can contain
a U+00B7 that `sanitize_segment` did not put there, (d) the §4.5(c) picker
regression — specifically whether any *other* consumer assumed the label encoded
a branch (grep for the assumption, do not reason about it), (e) **whether any
code path still derives a transcript path from a cwd** and could therefore go
stale on relocation (§3.2 — `spawn.rs` legitimately does; the label pipeline must
not), and (f) **whether an older binary's read-modify-write can destroy an earned
`title` or `summary` in a way §3.6's re-derivation does not heal.** (f) is the
one with no automated coverage below the unit level and the one whose failure is
silent.

### 5.5 Real-parser guardrail (generated-artifacts class — mandatory)

`crates/clave/tests/kdl_guardrail.rs` currently proves a `dir · branch · summary`
label survives the real zellij 0.44.3 parser. Extend it:

- `eager_record()` (`kdl_guardrail.rs:59-73`) gains `live_cwd`, and sets
  `title: Some("F-CLA")` and `summary: "fix the KDL guardrail".into()` so the
  **full three-segment** label is the one the existing cases already exercise.
  Its literal `label:` field is replaced by `compose_label()` output so the
  guardrail parses what the composer actually emits, not a hand-written
  approximation of it.
- New case `hostile_title_label_is_guarded_through_real_parser`, alongside the
  existing `backslash_label_is_guarded_through_real_parser`: a record whose
  `title` contains `"`, `\`, a control char and a U+00B7, run through
  `compose_label` → `tab_node` / `tab_node_bare` / `launch_layout_kdl`, asserting
  both **validity** (`Layout::from_str` parses) and **structure** (the parsed
  label has exactly one U+00B7 per real boundary, i.e. the forged boundary was
  neutralised).

Rationale, restated from TESTING.md: substring tests assert *content*; this
asserts *validity*, and a label failure surfaces at **session launch**, where a
dead `attach` blocks forever.

### 5.6 Gate

```bash
cargo test --workspace          # --workspace is load-bearing (clave-bar is not a default member)
cargo build -p clave-bar --target wasm32-wasip1
cargo clippy --workspace --all-targets -- -D warnings
```

Plus one sandboxed end-to-end, debug build, no live session touched:

```bash
CLAVE_STATE_DIR="$TMPDIR/clave-s4" cargo run -p clave -- ls --json | jq .
```

(No new CLI surface is added, so no `Cli::try_parse_from` pin is required — say
that explicitly in the dossier rather than leaving it inferred.)

---

## 6. Live validation

**Contract** (AGENTS.md, TESTING.md "The interaction contract"): the maintainer
executes every step. The driving agent **prints** commands and never runs them
against a live session — no `zellij action`, no launch, no kill, no reload, no
pipe. Every diagnostic below is read-only; `read_store` is lock-free-safe because
writers use temp+atomic-rename (`store.rs:153-161`).

Paths are genericized (`$HOME`, `$TMPDIR`) — the pre-commit PII blocklist rejects
private local paths in staged lines, and it has fired twice.

Two facts the agent must hold while reading reports:

- `clave ls --json` emits an `AgentSnapshot`, which since #69 **carries**
  `title`, `summary` and `worktree` but still **drops** `label_source` and
  `live_cwd` (`store.rs:185`). Two of S4's three diagnostics are therefore
  snapshot-only, and every step below still reads the **raw store** so one
  command covers all five fields.
- `clave dev status` is the **wrong tool** here: `run_status` calls
  `enter_sandbox` first (`dev.rs:262-265`), so it reads the sandbox store.

The one command used throughout, printed once:

```bash
jq '[.agents[] | {label, label_source, title, summary, branch, cwd, live_cwd, repo_root, tab_id}]' \
  "$HOME/.local/state/clave/agents.json"
```

Call it **`STORE`** below.

### Step 0 — pre-flight (issue #44 is unfixed; skip this and every reading below is suspect)

**(a) Run:**
```bash
command -v clave && clave --version
grep -n 'clave-bar' "$TMPDIR/zellij-$(id -u)/zellij-log/zellij.log" | tail -5
```

**(b) Look at:** the `clave --version` string versus the version in the most
recent `clave-bar: loaded vX.Y.Z` line.

**(c) Report:** both strings verbatim.

| Report | Conclusion | Next |
|---|---|---|
| the two match | the fleet is coherent | Step 1 |
| they differ | **#44/#43** — the plugin shells out to a different binary than the one installed | **stop.** No observation below can be trusted. Report and abandon the run |
| no `clave-bar: loaded` line today | the log is stale or filtered wrong (it is shared by every zellij session on the machine) | re-run with `tail -50`; still nothing → report and stop |

### Step 1 — baseline: does any row still show a worktree directory name?

**(a) Run:** `STORE`

**(b) Look at:** the sidebar rows top-to-bottom. For each, check three things:
does a **repo** name appear (never a worktree directory name); does a **branch**
appear anywhere (it must **not**); and is the branch nonetheless correct in the
JSON.

**(c) Report:** every row's label verbatim, and the JSON.

| Report | Conclusion | Next |
|---|---|---|
| every label contains `basename(repo_root)` and **no** branch | the §1.1(a) fix and the format change both landed | Step 2 |
| any label still contains a branch (`clave/ab12cd34`, `main`, …) | the loaded wasm or binary predates the change, **or** `compose_label` still emits branch | re-check Step 0's version match before calling it a defect |
| a label contains a worktree dir name **and** its `repo_root` is that same worktree path | the record's `repo_root` is wrong at the source — `clave add → new` run from *inside* a worktree (`add.rs:502`, §7 limitation), **not** an S4 regression | record it; continue to Step 2 |
| a label contains a worktree dir name but `repo_root` is the main root | **S4 regression** — `compose_label` is not being called, or the row has not seen a hook event since the upgrade | report the JSON; prompt that agent once and re-run `STORE` |
| a never-renamed, never-prompted agent shows a **single** segment (just the repo) | correct — §3.1's first worked example | Step 2 |
| `label` disagrees with what the sidebar shows for a **live** row | the tab name is stale — `Effect::RenameTab` never fired for it | check that row's `tab_id` is non-null; if null → **RC-A/RC-B, S0's territory**, not S4 |

### Step 2 — an agent's cwd changes mid-session (store-only observable)

> **Read this before running it.** Under the new format a mid-session `cd`
> produces **no visible change** — branch does not render (§1.1). The sidebar
> staying still is the **pass** condition, not a failure. The observable is the
> raw store, and this step exists to prove the payload seam works at all, and to
> catch the one catastrophic outcome (`cwd` being overwritten).
>
> **Amended 2026-07-28 (#79).** This step now carries a second, heavier job. A
> cwd change makes Claude **relocate the whole transcript** into a project
> directory keyed on the new cwd (§3.2), so this is also the live check that the
> label pipeline followed it — that is, that `payload.transcript_path` is
> present and is what the tail read uses. **Do not stop at the `live_cwd`
> reading.** After the `cd`, send one more prompt and confirm the label still
> upgrades; a label that goes quiet after a `cd` is the derived path still being
> used somewhere.

**(a) Do:** in a Claude session running in a **worktree** (e.g. one of the
`.claude-worktrees/…` agents), type into Claude:

> `cd` to the main checkout of this repo and tell me the branch.

Let it finish (wait for the green `Done` glyph — the `Stop` hook is what carries
the payload).

**(b) Look at:** `STORE` before and after, and the row's sidebar text before and
after.

**(c) Report:** both `STORE` outputs and both sidebar strings.

| Report | Conclusion | Next |
|---|---|---|
| `live_cwd` is now the main checkout, `cwd` is still the worktree, `branch` is the main branch, **and the sidebar is unchanged** | **the seam works and the grammar is correct.** `payload.cwd` is confirmed present (§5.4 assumption 5) | Step 3 |
| all of the above **except** the sidebar changed | the branch leaked into the label after all | report both strings; §4.4(c) regression |
| `live_cwd` is `null` after a completed turn | Claude's hook payload does **not** carry `cwd` on this CLI version — the live-branch tier is inert. Nothing rendered depends on it, so this is not user-visible | **report.** Not a merge blocker under the new format, but it invalidates §3.2's seam and S6 must know before it builds the worktree marker on it |
| no `title` or `summary` has ever changed on **any** row since the upgrade | `payload.transcript_path` may be absent on this CLI version, which leaves the tail read with no path at all (§5.4 assumption 6) | **merge blocker, and it is the one assumption with no fallback.** Report one raw hook payload — the maintainer can capture it by adding a `tee` to the hook command line, printed by the driving agent, never run by it |
| `live_cwd` updated but `branch` did not | `head_branch` failed on that path — the `.git` walk or the `gitdir:` pointer | report `live_cwd` verbatim plus `ls -la "<live_cwd>/.git"` output |
| `cwd` changed too | **stop-the-line defect.** `rec.cwd` is still frozen (§3.2) and still scopes `spawn_mode` and `claude --resume`; rewriting it breaks resume | report immediately; do not continue |
| `seq` jumped on this turn with no label change | the churn firewall leaked — a branch-only change must return `false` (§5.3) | report; also covered by Step 7 |
| **after one further prompt, the label still upgrades** (a new `ai-title` or a rename lands) | **the transcript followed the relocation and the tail read followed it too** — `transcript_path` is present and wired. This is the check §3.2's correction exists for | Step 3 |
| the label goes **silent** after the `cd` — `title` and `summary` stop changing while Claude is clearly still working | the tail read is still opening the **pre-relocation** path. Either `payload.transcript_path` is absent on this CLI version, or `jsonl_path(claude_dir, &rec.cwd, uuid)` survived somewhere in the label pipeline | **merge blocker.** Report `find "$HOME/.claude/projects" -name '<uuid>.jsonl'` — one hit under the *new* munged dir confirms relocation; then grep the diff for `jsonl_path` |
| `find` returns the jsonl under the **old** munged dir after a completed `cd` turn | relocation did not happen on this CLI version — §3.2's correction still holds for the versions where it does, and nothing here regresses | note the CLI version and continue |

### Step 3 — rename a Claude session and watch the sidebar

**(a) Do:** in any tracked Claude session, rename it — the same action that
produced `F-CLA` at the top of the pane. Then send it any one-word prompt (a
rename alone writes the record; the **prompt** is what makes clave read it).

**(b) Look at:** that row in the sidebar, and Claude's own `✻ <name>` pane
header.

**(c) Run and report:** `STORE`, the sidebar row verbatim, and the pane header.

| Report | Conclusion | Next |
|---|---|---|
| `title` holds the new name and the sidebar shows `<name> · <repo> · …` — **title first** | **the title tier works and the ruled order is correct** | Step 4 |
| the sidebar shows `<repo> · <name> · …` — repo first | the composer is emitting the **superseded** order | report; §4.4(c) regression |
| `title` is `null` | the tail scan found no `custom-title`. Report `grep -c custom-title` on the transcript (path printed in Step 5) — zero means this CLI version writes renames elsewhere; non-zero means the extractor is broken | report and stop the title steps |
| `title` is right but the sidebar still shows the old name | the rename did not reach the tab — same fork as Step 2 (`tab_id` null ⇒ S0) | record |
| the row shows the title but **lost the summary** | expected at a narrow pane — §3.4 makes the summary the give-way segment | confirm by comparing against a wider row; then Step 4 |
| that agent's `label_source` is `summary` and the title still appeared | **§1.3's obstacle is genuinely fixed** — note it explicitly, it is the headline behaviour change | Step 4 |
| **any** row anywhere reaches `label_source: summary` | **the retargeted tier fires.** Before this change no agent in the fleet had ever left `first_prompt` (14/14 rows, §1.3) — reaching `summary` at all is the proof `ai-title` is being read | note it; it is a merge-report headline |
| every row is still `first_prompt` after several turns on a busy agent | the `ai-title` extractor is not matching. Report `grep -c '"type":"ai-title"' <transcript>` — zero means this CLI version does not emit it either, non-zero means the extractor is broken | report; do not merge on a still-inert tier |

### Step 4 — `/clear` must not erase the held rename

**(a) Do:** in the session renamed in Step 3, run `/clear`, then send one
one-word prompt.

**(b) Look at:** the sidebar row, and Claude's own pane header.

**(c) Report:** `STORE`, plus both.

| Report | Conclusion | Next |
|---|---|---|
| clave still shows the title; Claude's pane header no longer does | **the #24 ruling is implemented** — the disagreement is deliberate | Step 5 |
| clave's title went `null` | the empty-`customTitle` filter is not working | report the last 20 lines of the transcript's `custom-title` records |
| clave's title changed to something else | an unrelated record type is being matched | report |

### Step 5 — an injected/system prompt must NOT earn a label

**(a) Do:** find a *fresh* agent with no earned summary (`summary: ""` in
`STORE`) — or make one with `clave add` and give it no prompt. Then trigger a
harness-injected turn: the reliable shape from #17 is **resuming a session that
died with a pending background task**, which auto-fires a `<task-notification>`
turn. If none is available, the read-only substitute is to inspect an existing
transcript for one:

```bash
# find the transcript BY UUID, not by munging a cwd — it may have relocated (§3.2)
T=$(find "$HOME/.claude/projects" -name '<uuid>.jsonl')
grep -o '"type":"[a-z-]*"' "$T" | sort | uniq -c | sort -rn
grep -o '<task-notification\|<system-reminder\|<local-command-caveat\|<command-name' \
  "$T" | sort | uniq -c
```

(**do not** reconstruct the path by munging `cwd` — that is exactly the derivation
§3.2 corrects, and it silently misses a relocated transcript. Project directory
names begin with `-`, which bare `ls` parses as flags; `find` sidesteps it.)

**(b) Look at:** whether that agent's row gained a summary segment.

**(c) Report:** `STORE` for that agent, specifically `summary`, `title` and
`label_source`.

| Report | Conclusion | Next |
|---|---|---|
| `summary` is still `""` and the label is the bare `repo` | **#17's guard holds through the rewrite** — this is the regression this step exists for | Step 6 |
| `summary` contains `<task-notification` or any other tag | **#17 has re-leaked.** The byte-compare gate was replaced by `summary.is_empty()` (§3.3) and the guard was lost | report immediately; this is a merge blocker |
| `title` contains a tag | one of the **three new** earn paths leaked — the title tier is missing `is_harness_injected` | report immediately; merge blocker |
| `summary` contains a tag **and** `label_source` is `summary` | the leak came through the retargeted `ai-title` tier — a path that did not exist when #17 was written | report immediately; merge blocker |
| the agent never took an injected turn | inconclusive, not a pass | say so plainly; the Tier-1 table-driven test (§5.2) is the standing guard |

### Step 6 — width behaviour at the real bar

> **Width caveat.** S4 sets no width and no gutter size. Whatever the pane is at
> the time of this run — 30 today, ~38 once **S8** lands, minus whatever **S6**
> takes for the gutter — the expectations below are the same, because the policy
> is budget-independent. Record the observed width so the report is
> interpretable later; do **not** treat a particular number as the contract.

**(a) Do:** with at least one three-segment row on screen (Step 3 produced one)
and at least one no-title row, read the sidebar at its normal width. Do **not**
toggle collapse if the maintainer is mid-work — this step is observational.

**(b) Look at:** which segments survive on each row, and whether any row lost
everything but the repo.

**(c) Report:** every row verbatim, and the bar's approximate column width.

| Report | Conclusion | Next |
|---|---|---|
| three-segment rows show `title · repo` (summary dropped) and no-title rows show `repo · <truncated summary>` | **§3.4 works as specified** | Step 7 |
| a no-title row shows a bare repo with the summary **dropped** rather than truncated | the two-segment floor failed — `fit_label_str` dropped below two segments | report the row and the width; merge blocker |
| a titled row lost its **title** and kept the repo | the give-way order is inverted | report; merge blocker |
| a titled row shows `title · <truncated repo>` | correct, and it is the §3.4 "even `title · repo` overflows" case — note the title length | expected; Step 7 |
| rows are still tail-truncated mid-word with nothing dropped | `fit_label_str` is not wired at `main.rs:546` — or the loaded wasm predates the change | re-check Step 0's version match first |
| a row shows a stray `·` at either end | a joined empty segment — `parts.retain(\|p\| !p.is_empty())` is missing | report |
| a row's text starts hard against the gutter glyph with no space | a gutter-width mismatch — **S6's**, not S4's; the budget expression at `main.rs:546` is off by one | record and hand to S6 |

### Step 7 — steady-state churn

**(a) Do:** pick a settled agent and leave it alone for two full turns
(prompt → done → prompt → done) with no rename, no `cd`, no `git switch`.

**(b) Look at:** whether its tab name flickers or its row visibly re-renders at
each turn boundary.

**(c) Run and report:**
```bash
jq -r '.seq' "$HOME/.local/state/clave/agents.json"
```
before and after the two turns, plus whether anything flickered.

| Report | Conclusion | Next |
|---|---|---|
| `seq` advanced by roughly the number of *status* changes and nothing flickered | **the churn firewall holds** (§5.3) | done |
| `seq` advanced on every hook event with no visible change | `refresh_label` is returning `true` on an unchanged label — the `rec.label != before` comparison is wrong or something below it mutates unconditionally | report the delta; this is the §5.3 defect |
| the tab name visibly flickers between two values | two writers disagree on the label — likely a mixed-binary window (Step 0) or an old-binary recompose (§3.6) | re-run Step 0 |

---

## 7. Risks, limitations and out of scope

### Format provenance (settled)

The row format was ruled on **2026-07-22**: `● 󰁼 𖣂 F-CLA · clave · <summary>`.
It supersedes two earlier statements, both now dead and recorded here only so a
future reader does not resurrect one:

| Superseded | By |
|---|---|
| #24 comment, 2026-07-21: `● F-CLA · clave · 𖣂 · <summary>` (marker inline in the text) | the marker moves into the **gutter** (S6) |
| the earlier S4 draft: `repo · title · branch · summary` | title leads; **branch is dropped entirely** |

No open format question remains. What *is* open is §3.5's obligation-2 caveat —
repo is segment 1 only when a title exists — which is an S5 integration detail
with a recommended resolution, not a format question.

### Risks

| Risk | Mitigation |
|---|---|
| `payload.cwd` is absent on this Claude CLI version | the field is `Option`; absent → branch unchanged, everything else still works, and **nothing rendered depends on it** under the new format. **Live Step 2 is the check**, and it distinguishes "absent" from "broken". S6 must know the answer before building the worktree marker on it |
| **S5 indexes segments positionally and mis-colours every un-renamed row** | the highest-probability integration failure in this batch. §3.5 states the caveat, gives the value-match resolution, and `compose_label_puts_repo_first_when_there_is_no_title` pins the shape so S5's own tests will hit it |
| **S6 and S8 land in either order and the budget arithmetic drifts** | S4 introduces **no width constant** and asserts none. `budget` is a parameter; `gutter_cells` is one named expression at `main.rs:546` (§4.8). Whoever lands changes that one line |
| the picker regression (§4.5c) is missed | it is the reason `resume_candidates_*` tests are on the red-first list (§5.2), and it is item (d) in the adversarial reviewer brief (§5.4) |
| the 64 KiB tail is smaller than the distance to the last rename | `rec.title` holds the last non-empty value; the tail is an update channel, not a source of truth (§3.3) |
| the tail read now runs on every `Stop`/`UserPromptSubmit` instead of stopping at `Summary` | 64 KiB, for **tracked agents only** (the untracked fast path at `hook.rs:270-272` already returned). Measured cost is a single `seek`+`read`; no subprocess, and no path derivation either — `payload.transcript_path` (§4.3) removed it |
| **`payload.transcript_path` is absent on this Claude CLI version** | the label pipeline has **no path at all** and every tier goes inert. Unlike `payload.cwd` this has no benign degradation: the alternative is the derived path, which #79 proved silently wrong after a relocation. **Live Step 2 is the check**, and it is a merge blocker if it fails |
| **an older `clave` binary strips an earned `title` / `summary` on write** | §3.6's re-derivation heals it from the next tail read, keyed on `label_source == Summary` surviving the strip. Residual: a value that has scrolled out of the 64 KiB window before the strip is lost until Claude emits a fresh one. Bounded to the #43/#44 mixed-version window; pinned by `stale_binary_strip_is_re_derived_from_the_tail`; item (f) in the §5.4 reviewer brief |
| **the retargeted `ai-title` tier is itself extinct on a future CLI version** | the failure is *inert*, not wrong — the segment falls back to the prompt tier and the label stops upgrading. That is exactly how the `type:"summary"` tier died unnoticed for months (§1.3), so the detection is a **count, not an assumption**: Live Step 3's `grep -c '"type":"ai-title"'` row, and `FOOTGUNS.md`'s entry on how to spot an extinct tier |
| a Claude-authored title forges a segment boundary | `sanitize_segment` strips U+00B7 (§3.7), tested at unit level and through the real KDL parser (§5.5) |
| a very long title crowds the whole row | bounded by the 32-char clamp (§3.7) and then by the tail-truncate; the title is the user's own choice and §3.4 deliberately does not second-guess it |
| an old `clave` binary in a mixed-version window recomposes labels under the old rule | degrades to old-style labels until the next event from the new binary; no data loss (§3.6). This is #43/#44's blast radius, not S4's |
| more frequent label changes churn `Effect::RenameTab` | `refresh_label` returns `label != before`; steady-state churn is zero, pinned by a test (§5.3) |

### Known limitations, accepted

1. **An agent that `cd`s across *repos* keeps naming the repo it was born in.**
   `repo_root` is deliberately not refreshed (§3.2) because it is the picker's
   grouping key and the transcript's home. Under the new format this is now
   **invisible** — the branch, which would have exposed the mismatch, no longer
   renders — so the row simply reads `clave · …` throughout. Defensible:
   `claude --resume` only works from the birth repo's cwd, so `clave` is the
   session's true home.
2. **`clave add → new`, run from inside a worktree, records `repo_root` as the
   worktree path** (`add.rs:502-504`; the main-root resolution at
   `add.rs:553-561` exists only on the *resume* arm). Such a row's segment 1 will
   still be a worktree directory name. **This is a pre-existing #19-shaped defect
   in `add.rs`, not something S4 introduces** — and Live Step 1's second branch
   distinguishes it. `head::git_dir_for` (§4.2) makes the fix a few lines
   (`<gitdir>` of a linked worktree is `<main>/.git/worktrees/<name>`); file it.
3. **Collapsed mode is unaddressed.** At a budget of 0–1 everything becomes `""`
   or `…`. #24 item 7, and it needs the gutter (S6) and the colour channel (S5)
   before it can say anything at all.
4. **Worktree provenance is invisible until S6 lands.** Branch left the label and
   the `𖣂` marker has not arrived, so between S4 and S6 two worktrees of one
   repo are distinguishable only by their title and summary. The **picker** is
   unaffected (§4.5c restores its branch suffix), and that is where it matters
   most. Stated because it is a real, temporary regression in the sidebar and it
   should not be discovered on screen.
5. **`spawn_mode` still derives the transcript path from the frozen `rec.cwd`,
   and a relocated session therefore resumes as a *new* one** (§3.2, #79). The
   probe misses, `spawn_mode` chooses create, and the history is orphaned —
   silently. **Pre-existing and independent of the label**; newly *visible*
   because relocation is now known, and newly *diagnosable* because `live_cwd`
   records the move. Not fixed here: resume-vs-create arbitration is not a label
   change and must not ride this diff. **File it**, and note that `live_cwd`
   gives the fix a second path to probe.
6. **An older `clave` binary can permanently lose an earned `title`** in the
   mixed-version window, if the last `custom-title` line has already scrolled out
   of the 64 KiB tail (§3.6). `summary` self-heals within a few turns because
   `ai-title` re-emits; `custom-title` may never re-emit, because a rename is a
   one-off human act. Closing it properly needs a store schema version, which
   #69 §2.1 rules out. **File it** against the #43/#44 mixed-binary work, not
   against S4.

### Out of scope

| | Why |
|---|---|
| the gutter — status dot, battery slot, `𖣂` marker (#24 items 4, and the marker) | **S6.** S4's contract to it: text-only label, budget is a parameter (§4.8) |
| widening the bar 30 → ~38 (#24 item 6) | **S8.** It touches the C6 width-seek machinery. §3.4 is correct at any budget and asserts none |
| per-repo colour (#24 item 2) | **S5 / RC-G.** §3.5 states the four obligations and the one caveat |
| context battery (#24 item 4), model badge (#24 item 5) | the extended tail scan in §4.3(b) is the seam both plug into — **noted as the integration point**, not built |
| collapsed 4-col design (#24 item 7) | needs both the gutter and the colour channel |
| `Row` gains structured fields ("stop rendering the label string") | out of scope for S4, but **reachable**: design-lock §7.1 plus #69's `title`/`summary`/`worktree` make it S5/S6's work, not a blocked one (§3.5) |
| ~~`payload.transcript_path` replacing `jsonl_path(claude_dir, &rec.cwd, uuid)`~~ | **NO LONGER OUT OF SCOPE — it is mandatory (§4.3, §3.8).** The deferral rested on "the derived path is proven in production", which #79 falsified: the derived path is wrong from the first relocation onward, silently |
| fixing `spawn_mode`'s derived transcript path (limitation 5) | resume-vs-create arbitration, not a label change. **The relocation finding exposes it; S4 files it** |
| a store schema version to close the mixed-version strip (limitation 6) | #69 §2.1 rules it out; S4 ships the re-derivation path instead (§3.6) |
| **S6's worktree marker, even though `worktree-state` now hands S4 its inputs** | `worktreePath` / `worktreeName` reach `refresh_label` for free (§3.2) and S4 stores none of them beyond `branch`. **Do NOT read this as "S6 decides"** — S6 read it as "S4 writes", so the obligation fell between the two and nothing filled `AgentRecord.worktree` from the transcript. Broken on #82: the tier is **explicitly deferred** behind S6 §5 Step 5's measurement, and whoever takes it owns the write into the existing wire-backed field |
| `lsview.rs` printing the repo twice (§4.5e) | cosmetic; named so it is seen to have been considered |
| dwell-open on dormant rows (#24's last comment) | unrelated nav question |
| anything in `model.rs` ordering, `apply_tabs`, or the executor election | S0/S1/S3 |

### Sequencing

S4 is **logically independent** — it depends on no other workstream's *design*.
It touches `hook.rs`, `add.rs`, `store.rs`, `head.rs` (new), `clave-types` (one
const) and `model.rs` (one pure function). Two of those files — `store.rs` and
`model.rs` — are also edited by S1 (ordering) and S5 (ink allocation), but at
disjoint sites (S4 adds record fields and one pure `fit_label_str`; S1 rewrites
the ordering key; S5 adds allocation state and the `compose_row` seam). So the
coupling is **file-level rebase, not a design dependency** (CodeRabbit
2026-07-22): whichever of S4/S1/S5 lands second re-runs `cargo test --workspace`
and resolves textual merge conflicts, but no spec has to change. This matches the
dossier's dependency table, which lists S4's design dependency as `—`.

The one shared file is `clave-bar/src/main.rs:539-557`, and three workstreams
converge on it:

| | Edits | Conflict shape |
|---|---|---|
| **S4** | two lines: `budget` and `fit_label_str` | trivial |
| **S5** | wraps `name` in colour after the fit | additive, downstream of S4's line |
| **S6** | sets `gutter_cells` and renders the glyph column | additive, upstream of S4's line |

They compose in one direction — gutter → budget → fit → colour → print — so any
landing order works and each rebase is mechanical. S4 deliberately touches
neither `Row` nor `rows()`, which is what keeps it out of S5's and S6's way.

Two live-validation caveats that are not S4's to fix:

- Steps 1–3 all have a branch that lands on **"`tab_id` is null ⇒ RC-A/RC-B"**.
  Until S0 lands, a rename that does not reach a tab may be S0's defect wearing
  S4's clothes. The tables distinguish the two by reading `tab_id` from the raw
  store first — do not skip that read.
- Step 6 reads a width that S6 and S8 both move. Record the observed width with
  the report; a later reader cannot reconstruct it.
