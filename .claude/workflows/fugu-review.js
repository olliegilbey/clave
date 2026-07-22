export const meta = {
  name: 'fugu-review',
  description: 'Fugu-style review: the calling session briefs parallel blind reviewers (haiku/sonnet/opus, plus optional coderabbit/codex/gemini CLI lanes), then an opus verifier consolidates and checks findings against the codebase',
  whenToUse: 'Dry-run review of a coding plan or an implementation. Read-only: produces ranked, verified findings; never edits code. The caller supplies the review brief (it is the conductor).',
  phases: [
    { title: 'Review', detail: 'haiku + sonnet + opus each review blind, in parallel, high effort; optional coderabbit/codex/gemini CLI lanes join when the caller opts in' },
    { title: 'Consolidate', detail: 'opus dedupes, adversarially verifies each finding vs the codebase, ranks' },
  ],
}

// ---------------------------------------------------------------------------
// Fugu, cheaply. Sakana's Fugu = a trained Conductor that briefs a diverse LLM
// pool playing Thinker/Worker/Verifier roles; the win is COVERAGE from diversity
// plus a verification stage, not one smarter model. We reproduce the shape:
//   Conductor  -> the CALLING SESSION. The chat that ran /fugu-review already
//                 holds the spec, intent, and code, so it writes the brief
//                 directly (see commands/fugu-review.md). No subagent conductor:
//                 a fresh one could only lose the session's intent, never add it.
//   Workers    -> blind reviewers on different model tiers (plus optional
//                 external CLI lanes), high effort.
//   Verifier   -> one opus consolidator that checks every finding vs ground truth.
// Reviewers mostly share the brief (so agreement is a real confidence signal)
// with a small per-model addendum to play to each tier's strengths.
// ---------------------------------------------------------------------------

// args is the brief the calling session assembled. Shape (all optional except
// that *something* describing the target should be present):
//   { restated_goal, shared_brief, key_context, addenda:{haiku,sonnet,opus},
//     target, focus, cli_reviewers }
// A bare string is treated as the target — unless it parses as a JSON object
// (a conductor that stringified its args by mistake: without this, options
// like cli_reviewers would silently drop and the CLI lanes would never run;
// happened live 2026-07-21).
const A = (() => {
  if (typeof args !== 'string') return args || {}
  const s = args.trim()
  if (s.startsWith('{')) {
    try { return JSON.parse(s) } catch { /* not JSON — treat as target */ }
  }
  return { target: args }
})()
const target = (A.target || A.task || A.plan || '').toString().trim()
const focus = (A.focus || '').toString().trim()

const MODELS = ['haiku', 'sonnet', 'opus']

// Optional external CLI reviewer lanes (CodeRabbit CLI, Codex CLI). Off by
// default: they only make sense when the target is a git diff (they cannot
// review a plan document), so the CONDUCTOR opts in with cli_reviewers: true
// (defaults below) or an object overriding { base, type, tools }. Each lane is
// a wrapper agent that runs the CLI via Bash and translates its findings into
// FINDINGS_SCHEMA — a lane whose CLI is missing/unauthenticated returns zero
// findings with the failure noted, never a fabricated review.
const CLI_DEFAULTS = { base: 'main', type: 'committed', tools: ['coderabbit', 'codex', 'gemini'] }
const CLI = A.cli_reviewers
  ? { ...CLI_DEFAULTS, ...(typeof A.cli_reviewers === 'object' ? A.cli_reviewers : {}) }
  : null

// The git range a lane must actually read, DERIVED from `type` (clave #20).
// The codex lane used to hardcode `base...HEAD` whatever `type` said, so a
// review of UNCOMMITTED work — the default target shape, since the conductor
// usually points at the working diff — compared two commits and saw nothing.
// The lane then reported "nothing significant" and that silence read as a
// clean pass. A review lane that can only ever agree is worse than no lane:
// it manufactures confidence. `type` is now load-bearing, not decorative.
//   committed   → merge-base range: what the branch adds on top of base
//   uncommitted → working tree + index against HEAD
//   all         → two-dot: everything in the tree relative to base
const diffExpr = (c) =>
  c.type === 'uncommitted'
    ? 'git diff HEAD'
    : c.type === 'all'
      ? `git diff ${c.base}`
      : `git diff ${c.base}...HEAD`

// Shell command per CLI lane. Both are read-only review modes: coderabbit
// `--agent` emits structured findings; codex runs sandboxed read-only.
const CLI_COMMANDS = {
  coderabbit: (c) => `coderabbit review --agent --type ${c.type} --base ${c.base}`,
  codex: (c) => `codex exec --sandbox read-only 'Review the ${c.type} git changes relative to ${c.base} (${diffExpr(c)}; explore the repo for context). If that diff is EMPTY, say so explicitly as your finding instead of reporting a clean review — an empty diff means the range is wrong, not that the code is good. Findings only - do not edit files. For each finding: severity (critical/major/minor/nit), file:line, a one-sentence claim, the evidence, and a suggested fix. If nothing significant, say so explicitly.'`,
  // Gemini CLI lane: non-interactive `-p` run from the repo root. Gemini's
  // default (non-yolo) approval mode cannot approve write tools in
  // non-interactive mode, and the prompt forbids edits — findings only.
  gemini: (c) => `gemini -p 'Review the ${c.type} git changes relative to ${c.base} in this repository (see them with \`${diffExpr(c)}\`; read surrounding files for context). If that diff is EMPTY, report that as your finding rather than a clean review — an empty diff means the range is wrong, not that the code is good. Findings only - do NOT edit or create any files. For each finding report: severity (critical/major/minor/nit), file:line, a one-sentence claim, the evidence grounded in code you read, and a suggested fix. If nothing significant is wrong, say so explicitly.'`,
}

// Default emphasis per tier, used when the caller's addenda omit one.
const DEFAULT_LENS = {
  haiku:  'fast pattern-level defects: obvious bugs, typos, off-by-one, missing null/error handling, style and convention breaks',
  sonnet: 'logic and correctness: edge cases, boundary conditions, concurrency/ordering, contract mismatches, incorrect assumptions',
  opus:   'architecture and subtlety: design fit with the existing codebase, hidden coupling, wrong abstractions, security, long-term maintainability',
}

// The brief. Prefer what the calling session supplied; fall back so the workflow
// still runs if it was handed only a raw target.
const fallbackBrief = `Review the target below against the ACTUAL codebase. ${focus ? `Caller focus: ${focus}. ` : ''}Hunt for correctness bugs, design/fit problems, missing edge cases, and risks. Read the real code before asserting anything.\n\nTARGET:\n${target || '(none supplied — inspect the current uncommitted diff via `git diff` / `git diff --staged`)'}`

// Prefer the caller's brief; fold `focus` into it (the fallback already carries focus).
const suppliedBrief = (A.shared_brief || A.brief || '').toString()
const plan = {
  restated_goal: (A.restated_goal || A.goal || target || '(the change under review)').toString(),
  shared_brief:  suppliedBrief
    ? (focus ? `${suppliedBrief}\n\nCaller focus: ${focus}` : suppliedBrief)
    : fallbackBrief,
  key_context:   (A.key_context || A.context || '(No pointers supplied — reviewers should locate the relevant files themselves.)').toString(),
  addenda:       (A.addenda && typeof A.addenda === 'object') ? A.addenda : {},
}

// --- Schemas ---------------------------------------------------------------

// Lenient by design: only `findings` is required at the top level, and only the
// core fields per finding. Smaller models (haiku) reliably drop optional sibling
// fields and mis-escape big code payloads under a strict schema — relaxing this
// lets partial-but-valid output through instead of failing the whole reviewer.
const FINDINGS_SCHEMA = {
  type: 'object',
  required: ['findings'],
  properties: {
    findings: {
      type: 'array',
      items: {
        type: 'object',
        required: ['title', 'severity', 'claim', 'evidence'],
        properties: {
          title:         { type: 'string' },
          severity:      { type: 'string', enum: ['critical', 'high', 'medium', 'low', 'nit'] },
          location:      { type: 'string', description: 'file:line or area; empty if general' },
          claim:         { type: 'string', description: 'What is wrong / the concern' },
          evidence:      { type: 'string', description: 'Why — grounded in the actual code or plan you read' },
          suggested_fix: { type: 'string' },
          confidence:    { type: 'string', enum: ['high', 'medium', 'low'] },
        },
      },
    },
    overall_take: { type: 'string', description: '1-3 sentence verdict on the plan/implementation' },
    blind_spots:  { type: 'string', description: 'What you could not check, and assumptions you made' },
  },
}

// Same leniency as FINDINGS_SCHEMA: no additionalProperties:false (a stray field
// must not hard-fail and trigger a retry death-spiral), and only the fields that
// carry the report's core value are required.
const CONSOLIDATED_SCHEMA = {
  type: 'object',
  required: ['summary', 'findings'],
  properties: {
    summary:        { type: 'string', description: 'Overall verdict on the plan/implementation' },
    recommendation: { type: 'string', description: 'Short go / no-go / go-with-changes call' },
    findings: {
      type: 'array',
      items: {
        type: 'object',
        required: ['title', 'severity', 'claim', 'verdict'],
        properties: {
          title:         { type: 'string' },
          severity:      { type: 'string', enum: ['critical', 'high', 'medium', 'low', 'nit'] },
          location:      { type: 'string' },
          claim:         { type: 'string' },
          evidence:      { type: 'string', description: 'Your own verification against the codebase, not just the reviewer note' },
          verdict:       { type: 'string', enum: ['CONFIRMED', 'PLAUSIBLE', 'REFUTED'] },
          raised_by:     { type: 'string', description: 'Which reviewers raised it, e.g. "haiku, opus (2/3)"' },
          suggested_fix: { type: 'string' },
        },
      },
    },
    agreements:  { type: 'string', description: 'Where reviewers converged — high-confidence signal' },
    disputes:    { type: 'string', description: 'Where they disagreed or only one flagged something' },
    blind_spots: { type: 'string', description: 'What the user should still verify manually' },
  },
}

// --- Prompts ---------------------------------------------------------------

function reviewerPrompt(model) {
  const addendum = (plan.addenda && plan.addenda[model]) || DEFAULT_LENS[model]
  // Count every lane that will actually run so the prompt never understates
  // the pool (model reviewers + any opted-in CLI lanes).
  const laneCount = MODELS.length + (CLI ? CLI.tools.filter((t) => CLI_COMMANDS[t]).length : 0)
  return `You are one of ${laneCount} independent reviewers doing a DRY RUN. You are blind to all the others — do not assume they cover anything; review thoroughly yourself. Do NOT modify any files; produce findings only.

WHAT WE ARE REVIEWING:
${plan.restated_goal}

SHARED REVIEW BRIEF (all reviewers run this):
${plan.shared_brief}

STARTING POINT — begin here, then investigate on your own:
${plan.key_context}
This is a launch thread, not a summary. Do not assume it is complete or that anything has been pre-checked for you.

YOUR EXTRA EMPHASIS (a light lean on top of the shared brief — an emphasis, not a limit; challenge the approach itself if it deserves it):
${addendum}

You have full read/search/git access — run wild. Open the entry point, follow imports, callers, callees, tests and neighbours, and pull in whatever you need to judge this yourself. Ground every finding in code you actually read. Prefer a few well-evidenced findings over a long speculative list. If the plan/implementation is sound, say so plainly.

OUTPUT DISCIPLINE (so your structured result stays valid): cap at your ~8 highest-value findings. In \`evidence\`/\`suggested_fix\`, describe and CITE file:line — do not paste large code blocks (big embedded code causes malformed JSON). Keep each field to a few sentences. Also include a short \`overall_take\` and \`blind_spots\` if you can.`
}

// Wrapper lane: run an external CLI reviewer and translate its output. The
// wrapper must NOT review the code itself — the external tool's independent
// perspective is the whole point of the lane.
function cliReviewerPrompt(tool) {
  const cmd = CLI_COMMANDS[tool](CLI)
  return `You are a WRAPPER lane in a multi-reviewer dry run: run the ${tool} CLI code reviewer and translate its output into the findings schema faithfully. Do NOT review the code yourself, do NOT add, drop, or editorialize findings, and do NOT modify any files — the external tool's independent perspective is the product.

WHAT IS UNDER REVIEW:
${plan.restated_goal}

RUN THIS via Bash from the repo root (it is slow — use a generous timeout, up to 10 minutes):
${cmd}

Then convert EVERY finding the tool reported into the schema: title; severity mapped onto critical/high/medium/low/nit; location as file:line; claim = the tool's assertion; evidence = the tool's own rationale (quote or closely paraphrase it); suggested_fix if it offered one; confidence: high (it is the tool's claim, faithfully relayed). Put the tool's own overall summary, if any, in overall_take, and note in blind_spots that this lane is a verbatim translation of ${tool}'s output.

If the CLI errors or cannot run (not installed, not authenticated, usage/rate limits, no diff to review), return an EMPTY findings array and state exactly what failed in blind_spots. Never fabricate findings and never substitute your own review for the tool's.`
}

function consolidatorPrompt(labeledReviews) {
  const n = labeledReviews.length
  const who = labeledReviews.map((x) => x.model).join(', ')
  return `You are the CONSOLIDATOR / VERIFIER (Fugu's Verifier role). ${n} reviewer(s) on different models/tools (${who}) reviewed the same target mostly-blind. Lanes named "coderabbit", "codex", or "gemini" are external CLI reviewers relayed verbatim — they did not share the model reviewers' brief, so overlap with them is an even stronger agreement signal. Turn the raw findings into one trustworthy report.

WHAT WAS REVIEWED:
${plan.restated_goal}

CONTEXT:
${plan.key_context}

RAW REVIEWER OUTPUT (labeled by model):
${JSON.stringify(labeledReviews, null, 2)}

Do this:
1. Merge findings that are the same underlying issue. Record who raised each (e.g. "sonnet, opus (2/${n})"). Agreement across models is a real confidence signal because they shared a brief.
2. ADVERSARIALLY VERIFY every merged finding against the ACTUAL codebase — open the files, confirm or refute. Set verdict CONFIRMED / PLAUSIBLE / REFUTED. Do not launder a claim you could not confirm.
3. Drop pure noise; downgrade weakly-evidenced items. Rank by severity then confidence.
4. Call out where reviewers agreed (trust these), where they disagreed or a single model stood alone, and what still needs manual verification.

OUTPUT DISCIPLINE (the whole report must fit ONE StructuredOutput call — too large a payload gets truncated at the tool boundary and the report is lost): cap at your ~12 highest-severity findings; in every field describe and CITE file:line rather than pasting code blocks; keep each field to a few sentences. Emit your real verdict in ONE call; never return placeholder/stub text just to satisfy the schema — a partial-but-real report beats a valid-but-empty one.

Do not modify any files. This is a dry run: recommend, do not change code.`
}

// --- Orchestration ---------------------------------------------------------
// The barrier is correct: the consolidator needs ALL three reviews before it
// can dedupe and verify.

phase('Review')
const cliTools = CLI ? CLI.tools.filter((t) => CLI_COMMANDS[t]) : []
const laneNames = [...MODELS, ...cliTools]
log(`Blind reviewers running in parallel: ${laneNames.join(' · ')}…`)
const rawReviews = await parallel([
  ...MODELS.map((model) => () =>
    agent(reviewerPrompt(model), {
      model, effort: 'high', schema: FINDINGS_SCHEMA,
      label: `review:${model}`, phase: 'Review',
    })
  ),
  // CLI wrapper lanes: cheap model, low effort — the work is running one
  // command and transcribing, not reasoning.
  ...cliTools.map((tool) => () =>
    agent(cliReviewerPrompt(tool), {
      model: 'sonnet', effort: 'low', schema: FINDINGS_SCHEMA,
      label: `review:${tool}`, phase: 'Review',
    })
  ),
])

// Label each review by its lane; drop any reviewer that died/was skipped.
const labeledReviews = rawReviews
  .map((review, i) => ({ model: laneNames[i], review }))
  .filter((x) => x.review)

if (labeledReviews.length === 0) {
  return {
    summary: 'No reviewer returned findings — every review agent failed or was skipped.',
    recommendation: 'Re-run /fugu-review; if it persists, the target may be too large or the models unavailable.',
    findings: [], agreements: '', disputes: '', blind_spots: 'Nothing was reviewed.',
  }
}
if (labeledReviews.length < laneNames.length) {
  log(`Only ${labeledReviews.length}/${laneNames.length} reviewers returned — consolidating what we have.`)
}

phase('Consolidate')
log('Consolidator (opus) verifying findings against the codebase…')
// Bump effort to 'xhigh' here if you want the heaviest verification pass.
const consolidated = await agent(consolidatorPrompt(labeledReviews), {
  model: 'opus', effort: 'high', schema: CONSOLIDATED_SCHEMA,
  label: 'consolidator', phase: 'Consolidate',
})

// NEVER lose reviewer work: the raw labeled reviews ride along on every return,
// success or not, so a failed/degenerate consolidation can always be triaged.
const reviewersHadFindings = labeledReviews.some(
  (x) => Array.isArray(x.review?.findings) && x.review.findings.length > 0,
)
const consolidatorProducedNothing = !consolidated || !(consolidated.findings?.length > 0)

// Guard keys on OUTCOME, not just null: a truncated/degenerate consolidator that
// returns a valid-but-empty stub (the real failure mode — the final StructuredOutput
// payload is too large and gets truncated) must trigger the fallback too.
if (!consolidated) {
  return {
    summary: 'Consolidation/verification did not run — showing raw, UNVERIFIED reviewer findings.',
    recommendation: 'Re-run /fugu-review; if it persists, triage the raw findings below manually.',
    findings: [], agreements: '', disputes: '',
    blind_spots: 'The verifier pass was skipped or failed; nothing below is cross-checked.',
    raw_reviews: labeledReviews,
  }
}
if (consolidatorProducedNothing && reviewersHadFindings) {
  return {
    summary: (consolidated.summary && consolidated.summary.length > 12)
      ? consolidated.summary
      : 'The verifier returned no findings. This is either a genuine "all reviewer findings refuted" result OR a failed/truncated synthesis (the consolidator payload can hit a size cliff). Reviewers DID surface findings — triage the raw reviews attached below.',
    recommendation: consolidated.recommendation || 'Inspect raw_reviews and re-run if this looks like a truncation, not a real all-clear.',
    findings: [],
    agreements: consolidated.agreements || '',
    disputes: consolidated.disputes || '',
    blind_spots: consolidated.blind_spots || '',
    raw_reviews: labeledReviews,
  }
}

// Normal path — return the consolidated report, still carrying the raw reviews.
return { ...consolidated, raw_reviews: labeledReviews }
