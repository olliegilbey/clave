---
description: Fugu-style dry-run review — this session briefs 3 blind reviewers (haiku/sonnet/opus) + an opus verifier, who consolidate findings on a plan or implementation without touching code
argument-hint: [what to review — a plan, "the current diff", or a path/description] [--focus "..."]
allowed-tools: Workflow, Read, Grep, Glob, Bash(git diff:*), Bash(git status:*), Bash(git log:*), Bash(git show:*)
---

Run a Fugu-style multi-agent review of a coding plan or implementation. It is a **dry run** — the agents are *instructed* to produce findings only and not edit code (prompt-enforced, not tool-sandboxed), and this session must not apply changes on their behalf.

**You are the conductor.** This session holds the spec and intent, so you scope the review — but your job is to *launch* the reviewers, not to brief them.

## The rule: pass a launch thread, not a dossier

Each reviewer runs in its **own isolated context** and explores the repo itself. Hand it only what it needs to *start*:
- the **task** (what to evaluate), stated **neutrally** — no verdict, no "I think this is fine/broken", no leading;
- **one or two entry files** to open and traverse out from — the discovery starting thread, *not* a summary of what's there;
- a **light** per-agent focus weight (optional).

Do **not** paste the whole spec, your analysis, or your conclusions. That anchors all three to your view and kills the independent coverage that makes this worth running. Minimal and neutral wins — the agents reconstruct everything else by exploring.

## 1. Scope from `$ARGUMENTS` + the session

- A pasted/described **plan** → the plan is the task; name the entry file(s) it touches.
- "the diff" / "my changes" / nothing → `git diff --stat` (+ `git status`) to find the entry files; the task is "review these changes."
- A named **path or feature** → that path is the entry point.

Fold any `--focus "..."` into the task neutrally.

## 2. Invoke the workflow (this spends tokens — it is the opt-in)

Call `Workflow` by name (it is vendored in this repo at `.claude/workflows/fugu-review.js`, so it resolves for anyone who has cloned the repo, including cloud agents with no home-directory config):

```
Workflow({
  name: "fugu-review",
  args: {
    restated_goal: "<one neutral sentence: what is under review>",
    shared_brief:  "<the review task, neutrally — what to evaluate, not your verdict. Keep it lean.>",
    key_context:   "<the entry point(s): first file(s) to open and traverse from; branch/diff if relevant. A thread to pull, not a summary.>",
    addenda: {                     // OPTIONAL light per-model lean; omit any model to use its default lens.
      haiku: "", sonnet: "", opus: ""   // "" = that tier's DEFAULT_LENS (divergent by design — good for coverage),
    },                                  // NOT "fully aligned". Set a string to override; keep it light.
    focus: "<focus note or ''>",
    cli_reviewers: true            // OPTIONAL — set when reviewing an IMPLEMENTATION (a git diff): adds
                                   // CodeRabbit CLI + Codex CLI as extra blind lanes. Omit for plan reviews
                                   // (CLIs can't review a document). Defaults: base "main", type "committed";
                                   // override with { base: "...", type: "committed|uncommitted|all",
                                   // tools: ["coderabbit","codex"] }. A lane whose CLI is missing or
                                   // unauthenticated returns zero findings with the failure noted — harmless.
  }
})
```

If name resolution ever fails, fall back to `scriptPath` with the path to `.claude/workflows/fugu-review.js`, resolved from the repo root (not `~` — this copy is vendored per-repo, not global).

Three **blind reviewers** (haiku · sonnet · opus, high effort, parallel) each explore from the entry point and return findings — plus, with `cli_reviewers`, **CodeRabbit CLI** and **Codex CLI** lanes whose output is translated verbatim into the same schema; an opus **consolidator** verifies each finding against the codebase and returns one report. CLI lanes never saw the shared brief, so their overlap with model reviewers is an extra-strong agreement signal. (Cold chat with no context? Pass just `{ target, focus }` and they scope it themselves.)

## 3. Relay the report

Present the returned object concisely:
- **Summary** + **recommendation** (go / no-go / go-with-changes).
- **Findings** ranked, each with severity, `verdict` (CONFIRMED / PLAUSIBLE / REFUTED), who raised it (`raised_by`), location, suggested fix. Lead with CONFIRMED + high severity; keep REFUTED ones in a short "considered and dismissed" note.
- **Agreements** (multi-model, high-confidence) and **disputes** (single-model or contested).
- **Blind spots** to verify manually.

**Failure/degenerate case:** the return always carries `raw_reviews` (the raw per-model reviewer findings). If `findings` is empty but `raw_reviews` contains findings, the consolidator was truncated or failed — say so, then relay the raw reviewer findings directly (they're unverified) rather than reporting a clean bill of health. Don't trust an empty `findings` as "all clear" unless the `summary` clearly says the verifier deliberately refuted everything.

Do **not** apply fixes — this is a dry run. Offer to act on specific findings only if the user asks.

## Known caveats (CLI reviewer lanes)

- **In a cloud / remote agent environment, do not opt into `cli_reviewers` at
  all.** The external lanes shell out to `coderabbit`, `codex` and `gemini` —
  third-party CLIs that are almost never installed in a cloud container, and
  that need interactive auth (OAuth, API keys) even when they are. The harness
  degrades honestly — a lane whose CLI is missing or unauthenticated returns
  zero findings *with the failure noted*, never a fabricated review — but the
  consolidator then reports a smaller pool than the brief implies, and a reader
  skimming the result sees three model lanes plus three silent ones and reads
  reassurance that was never earned. **A lane that did not run is not a lane
  that passed.** In cloud environments: run the model lanes only (they need
  nothing but the repo), and satisfy the second review requirement with an
  independent adversarial reviewer agent instead — see AGENTS.md. If you *do*
  attempt a CLI lane remotely, state in the PR whether it actually executed.
- **Empty-diff base bug (was clave issue #20 — FIXED in this vendored copy).**
  The `codex` lane used to run `git diff <base>...HEAD` (triple-dot,
  committed-only) whatever `type` said, so a review of **uncommitted** work —
  the common case for "the diff" / "my changes" — diffed nothing and the lane
  silently returned zero findings that read as a clean pass. `diffExpr()` in
  the workflow now derives the range from `type` (`committed` →
  `<base>...HEAD`, `uncommitted` → `git diff HEAD`, `all` → `git diff <base>`),
  and both the codex and gemini prompts instruct the lane to **report an empty
  diff as a finding** rather than as a clean review, so the failure is
  self-announcing. Note the maintainer's own global copy outside this repo may
  still carry the original bug. General rule regardless: **do not read CLI-lane
  silence as a clean review** — confirm the lane saw a non-empty diff.
- **CodeRabbit cloud vs CLI.** CodeRabbit's cloud/PR reviews are rate-limited
  (roughly 1/hour) and unsuitable for repeated dry runs; the `coderabbit` CLI
  lane above is reliable and not subject to that limit.
- **No `--plain` flag.** As of CodeRabbit CLI v0.7.0, `--plain` was removed —
  plain text is the default output now. Do not add `--plain` to the
  `coderabbit` command in the workflow; it is not a valid flag on current
  versions.
