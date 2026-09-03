# Alt+a dir picker ranking — design (proposal)

> **Landed on top of #247 (hour buckets).** Where this ratified text says
> `today`, `unix_day` or `BUCKET_RETAIN_DAYS`, the code reads `now_hour`,
> `unix_hour` and `BUCKET_RETAIN_HOURS`. Same window (7 days, now 168 hours),
> same dial, same score; only the bucket key's unit changed.

Status: **ratified 2026-09-02** — Ollie took the recommended default on every
decision below, and asked for the frecency score to be reused as-is (DRY). Built in the `dir-picker-ranking` branch.

## The problem

The Alt+a picker's list is `cwd`, then `zoxide query -l` verbatim, then the
worktrees of every store-known repo, piped into fzf bottom-up (best next to
the prompt). Two things are wrong with it and one is merely untidy.

**zoxide is blind to agent work.** zoxide learns from a shell hook on `cd`.
An agent tab never runs that hook, and neither does Claude's own Bash tool,
so a repo driven only through clave earns no zoxide score. Measured on the
maintainer's machine, 2026-09-02:

| Directory      | zoxide rank | zoxide score | store cluster score (this week) | jsonl files touched, 2 days |
| -------------- | ----------- | ------------ | ------------------------------- | --------------------------- |
| dotfiles       | 1           | 82.5         | 20.6                            | 1                           |
| resumate       | 4           | 18.2         | 0                               | 0                           |
| corti/olympus  | 9           | 8.0          | 262.1                           | 14                          |
| a fourth repo  | 11          | 7.2          | 105.3                           | 14                          |
| clave          | 3           | 18.5         | 112.9                           | 5                           |

The store already knows the truth: summed per repo, this week's commitments
rank olympus, clave, that fourth repo, corti, dotfiles — the same order the sidebar's
repo clusters show. The picker ignores that number and asks zoxide instead.

**The list ends where clave's knowledge starts.** Store-known worktrees are
appended unranked at the far end, whatever their weight.

**cwd is listed twice.** It is prepended as line 1 and `Vec::dedup` only
removes ADJACENT duplicates, so zoxide's own copy survives further up.

## UX principles

1. **Bottom-up stays.** The best candidate sits next to the prompt: where
   the cursor starts, where the next screen's `new`/`resume` choice sits,
   and where Claude's own input line lives. The eye never has to travel to
   the top of the pane. `--layout=reverse` is rejected, for every picker.
2. **One score, two surfaces.** The picker ranks a repo by Σ over the
   repo's rows of `frecency_millis(buckets, now_hour, half_life)`, the same
   per-row score the sidebar's repo layer sums for its clusters. No second
   frecency, no new dial. The two orders can still differ: the bar's live
   block sums live rows only, while the picker also weighs dormant rows (a
   repo driven this week and closed still ranks), so a repo with dormant
   weight may sit higher here than in the bar.
3. **Nothing known is hidden, nothing remembered is dropped.** A dir zoxide
   knows and clave has never opened still appears, in zoxide's order, above
   the store-ranked block (further from the prompt). Store knowledge
   outranks zoxide because for THIS picker it is the truer signal; zoxide
   remains the memory for everything clave has not driven this week.
4. **A repo's dirs travel together** (#234 applied to the picker). The repo
   root and its worktrees are adjacent, worktrees still marked `(wt)`, so a
   repo reads as one unit however its worktrees' individual weights fall.
5. **cwd first, once.** "Another agent here" is the default action; the
   duplicate goes.
6. **Beyond the window, defer to zoxide.** A store repo whose buckets have
   all aged past `BUCKET_RETAIN_DAYS` scores zero and takes its zoxide
   position; if zoxide has never seen it either, it trails, exactly as
   unseen worktrees do today.
7. **Fast.** The Alt+a path reads no transcripts. The store read is the
   lock-free one `run_add` already performs for worktree discovery.

## The design

Every candidate dir belongs to exactly one **cluster**. A store repo's
cluster is its root plus its worktrees; rows only weigh, never list. A
zoxide-only dir is a cluster of one. `cwd` is pinned first and removed from
wherever else it occurs.

Per cluster:

- `cluster_millis` = Σ `frecency_millis(row.buckets, today, half_life)` over
  every store row with that `repo_root`, live or dormant. The bar's repo
  layer clusters only live rows and ranks dormant rows flat; the picker has
  no live/dormant distinction, so all rows weigh in. This is the one
  deliberate divergence from the bar's comparator.
- `zoxide_rank` = the smallest zoxide position among the cluster's members,
  or `usize::MAX` when zoxide knows none of them.

Cluster order (best first): `cluster_millis` descending, then `zoxide_rank`
ascending, then root path. That single key yields all three regions with no
special cases: scored store repos (bottom, nearest the prompt), then
everything zoxide remembers in zoxide's order — zero-score store repos fall
into place here by their own zoxide rank — then store dirs neither source
has weighted.

Within a cluster: the root first, then worktrees by their own `dir_millis`
(Σ over rows whose `cwd` is that worktree) descending, then path.

Dedup is by exact path string, as `dir_candidates` already does. zoxide
records `$PWD`, which for a checkout is the canonical path the store holds,
so a store root and its zoxide twin collapse to one line. A symlinked
zoxide entry stays its own line — today's behaviour, unchanged.

`half_life` is the store's dial when `Store.order` is `Frecency`; a fleet in
`Recency` mode uses the default 24h. `now_hour` is `unix_hour(now_unix())`,
the store's own arithmetic.

### Data flow in `run_add`, step 1

```text
cwd
zoxide query -l                       (ordered, as before)
store (lock-free read, as before)
store_dirs(git, store, now_hour)      → StoreDirs { roots,
                                          worktrees: (root, path, linked),
                                          rows: (repo_root, cwd, frecency_millis) }
        │
        ▼
ranked_dir_candidates(cwd, zoxide, &StoreDirs) -> Vec<(String /*path*/, bool /*wt*/)>
        │
        ▼
dir_line / fzf_pick / picked_dir      (untouched)
```

`ranked_dir_candidates` replaces `dir_candidates`. It is pure: every input
is a value, so the whole ordering is unit-testable without git, zoxide or a
TTY, like `dir_candidates` today.

### The shared function

`frecency_millis` moves from `clave-bar/src/model.rs` to `clave-types`,
next to `OrderMode` and `BUCKET_RETAIN_DAYS`, which it already depends on.
The bar imports it; the host imports it. Its two pure-decay tests move with
it; the bar's ordering tests stay and keep guarding the comparator. The
function body does not change.

## Depth of the change

| File                              | Change                                                                                   | Size            |
| --------------------------------- | ---------------------------------------------------------------------------------------- | --------------- |
| `crates/clave-types/src/lib.rs`   | `pub fn frecency_millis` moves in, with its two decay tests                              | ~40 lines       |
| `crates/clave-bar/src/model.rs`   | function removed, one import added                                                       | ~-30 lines      |
| `crates/clave/src/add.rs`         | `dir_candidates` → `ranked_dir_candidates`; `run_add` passes rows, today, dial; cwd dedup | ~100 lines + ~100 test |
| `README.md:51`                    | one sentence: the picker ranks by your fleet's frecency, zoxide fills the rest           | 1 line          |
| `UBIQUITOUS_LANGUAGE.md`          | note under **frecency** that the picker shares the score                                 | 1 line          |

No wire change, no store schema change, no KDL, no hook, no new CLI
surface. The bar's ordering is untouched in behaviour — the function moves,
it does not change — and its existing tests hold that.

**Change class** (TESTING.md § risk taxonomy): pure logic. TDD red-first on
`ranked_dir_candidates`; `cargo test --workspace`; `just mutants` on the new
function and the moved one. One Tier 3 human pass: `just sandbox`, Alt+a,
confirm the bottom block reads like the sidebar's cluster order.

**Estimate:** one PR, half a day including mutants. Risk: low.

## Out of scope, deliberately

- **Cold start.** A fresh install has no store rows, so its picker is
  zoxide-only — the CLAUDE.md ideal ("a rich, full session from the jsonl
  alone") is not met here. The fix belongs in `backfill`, not the picker:
  mint dormant rows for recently active transcripts the store has never
  seen, which warms the sidebar's dormant list and this picker in one move.
  A picker-time transcript scan is ruled out by measurement: 99 project
  dirs, 281 transcripts, 116 modified this week, 712 MB. Separate spec.
- **Terminal tabs.** `tab_buckets` carry no cwd in the store. A repo driven
  from a terminal tab is one you `cd`'d into, which is exactly what zoxide
  already covers.
- **Blending zoxide's score with the store's.** The units are incomparable;
  block ordering with a zoxide tiebreak is the whole design.

## Decisions (ratified 2026-09-02: the recommended default in each)

1. **Worktrees cluster with their repo** (recommended, principle 4) vs rank
   flat by their own score.
2. **Root first within a cluster** (recommended: the root is the repo's name
   in the list, worktrees hang off it) vs root ranked by its own weight
   like any member.
3. **Recency-mode fleets use the 24h default dial** (recommended: one
   comparator) vs rank store repos by max `last_interacted`.
4. **All rows weigh in, live and dormant** (recommended: the picker has no
   live/dormant axis) vs mirror the bar exactly and count live rows only.
