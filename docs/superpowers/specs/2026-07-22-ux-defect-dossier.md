# UX defect dossier — sidebar ordering, tab close, labels, colour

_2026-07-22 · research synthesis, main `50fa26a` (v0.1.1 + PR #29) · read-only investigation_

This is the **shared source of truth** for eight workstreams — S0–S6 and S8
(S7, the context battery, is deferred and unspecced; S6 reserves its gutter
cell). Each has its
own spec beside this file. Read this first, then your workstream's spec. Every
claim below carries a `file:line` — verify before you build on it, but do not
re-derive from scratch.

Four symptoms were reported from daily driving. They decompose into **seven root
causes**, and two of the symptoms share one.

| Symptom (maintainer's words) | Root causes |
| --- | --- |
| "some tabs don't shift to the top when interacted with" | RC-A, RC-B, RC-C |
| "user→terminal interaction should also bump the terminal tab" | RC-D |
| "closing a tab goes all kinds of wrong — sometimes idle, sometimes another tab moves to top" | RC-A, RC-C, RC-E |
| "sidebar shows the old worktree name; Claude's rename isn't picked up" | RC-F |
| (new) per-repo colour coding | RC-G |

---

## The ordering machine, as built

Established mechanics. Take as given; verify cheaply if you touch them.

**Live rows sort on the store's `tab_timeline` and nothing else** — `crates/clave-bar/src/model.rs:391-393`:

```rust
/// §6.6 sort key: the STORE's tab timeline, nothing else.
fn sort_key(&self, t: &TabMeta) -> u64 {
    self.timeline.get(&t.tab_id).copied().unwrap_or(0)
}
```

The comparator is `model.rs:791` — `entries.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(&b.1)))`,
i.e. key **descending**, tiebreak **ascending**. Entries are built in two passes
in `rows()` (`model.rs:728-793`):

- **Live tab rows** (`model.rs:755-765`): key = `sort_key(t)`, tiebreak = `t.position`.
- **Dormant rows** (`model.rs:769-789`): key = `a.last_interacted`, tiebreak = `usize::MAX - i`.

Consequences that matter downstream:

1. A live tab with **no timeline entry sorts at key `0`** — below every dormant
   row that has ever been prompted.
2. At an **equal key, every live row outranks every dormant row** (small
   `position` beats `usize::MAX - i`). Converting live→dormant therefore changes
   both the key *and* the tiebreak class.
3. Timeline values are **whole unix seconds** (`crates/clave/src/store.rs:352-357`,
   `as_secs()`), so ties are common, and ties resolve on tab position — the
   lower-positioned tab wins regardless of who was actually touched last.

**`tab_timeline` has exactly two writers:**

| Writer | Trigger | Gate |
| --- | --- | --- |
| `clave touch <tab_id>` → `store::apply_touch` (`store.rs:213-220`) | the bar's one-shot **birth touch**, `crates/clave-bar/src/main.rs:434-445` | `is_active_instance() && needs_birth_touch(id)`; `needs_birth_touch` (`model.rs:383-385`) is **once ever per (instance, tab)** and never re-arms |
| `UserPromptSubmit` hook → `hook::apply_hook_event` (`crates/clave/src/hook.rs:241-253`) | user submits a prompt | **only `if let Some(tab_id) = rec.tab_id`** (`hook.rs:246,250-253`) |

`clave focus` (`store.rs:197-207`) writes `last_visited` and flips Done→Idle; it
**never** touches `tab_timeline` or `last_interacted`. Focus, clicks and Alt+j/k
deliberately never reorder — `model.rs:327-341` (beacon), `model.rs:795-823`
(click), rationale restated at `crates/clave/src/setup.rs:96-99`.

`EventType::InputReceived` is deliberately **not** subscribed
(`crates/clave-bar/src/main.rs:369-374`): it fires for every keystroke including
the nav binds themselves, and the resulting touch-spawn storm exhausted the
zellij server's file descriptors (`docs/superpowers/spikes/SUBSYSTEM-VALIDATION.md:232-243`).

---

## RC-A — frame-coherence: the executor election joins stale panes to fresh tabs

**This is the highest-severity defect in the dossier. It causes symptom 1 and
symptom 3 simultaneously, and it is sticky — it does not self-heal.**

`is_active_instance()` (`crates/clave-bar/src/main.rs:43-54`) and `own_tab_id()`
(`main.rs:60-71`) both answer "am I the bar in the active tab?" by joining
**`plugin_panes`** (last `PaneUpdate`) against **`last_tabs`** (last `TabUpdate`)
**by tab position**. Those two events arrive independently, so one frame is
routinely stale relative to the other. A tab close renumbers every position after
the closed index — which is precisely when the join is wrong.

`bind_effects` (`model.rs:414-439`) compounds it: `own_position` is read from
`self.tabs` (fresh) while `tab_position_of_pane` reads `self.panes` (stale) —
mismatched frames on **both sides** of the join.

Reproduction:

```text
tabs 10@pos0, 11@pos1, 12@pos2; focus on 12.  Alt+w closes tab 10.
post-close: 11@pos0 (active), 12@pos1.
the bar in tab 11 gets the close TabUpdate (fresh last_tabs)
  but its plugin_panes still places its own pane at position 1.
  model_tab_active_at(1) → last_tabs[pos1] = tab 12 → "active" → TRUE.
  is_active_instance() wrongly TRUE; own_tab_id() returns 12.
  fire_binds → `clave bind <u-11> 12`.
```

Both observed symptoms fall out:

- `apply_bind` **evicts** the rightful tenant (`store.rs:239-245`): `u-12.tab_id
  = None` → `is_dormant` (`model.rs:479-488`) is true for a **live** agent → tab
  12's row loses its status dot and a dim `◌` duplicate appears. *That is the
  "goes idle".*
- `u-11`'s next prompt stamps `tab_timeline[12]` (`hook.rs:246,250-253`) → **tab
  12 jumps to row 0** and `u-11` never moves. *That is "another tab moved to the
  top", and it persists.*
- It is **sticky**: `sent_binds` (`model.rs:197,305,429,431`) has no `remove`,
  no `clear`, and no reset in `apply_snapshot` — the corrective re-bind is
  blocked for the life of the plugin instance.

The ledger already records the gate as structurally weak —
`SUBSYSTEM-VALIDATION.md:656-659`: *"`is_active_instance` write-gating is
degenerate (every instance's stale tabs claim it's active)"*.

**Coverage gap:** `is_active_instance` / `own_tab_id` live entirely in
`clave-bar/src/main.rs`, the deliberately-untested adapter half. There is no test
anywhere for either under a stale `plugin_panes` frame.

---

## RC-B — the eager cold-start tab is never bound

`clave bind` is the **only** producer of a `tab_id` (`crates/clave/src/main.rs:309-315`
→ `store.rs:227-257`), and its only emitter is `Effect::Bind` (`model.rs:414-439`).
Nothing in the launch path writes one.

At cold start `launch_session` calls `clear_tab_timeline` **before** session
create (`setup.rs:647-651` → `store.rs:340-349`), which wipes `tab_timeline` and
sets `tab_id = None` on **every** agent. So the eager tab must win a race with no
retry.

`fire_binds()` is called from exactly four sites — `main.rs:447` (after
`TabUpdate`), `main.rs:467` (after `PaneUpdate`), `main.rs:268` (after a
`clave-status` pipe), `main.rs:279` (after a `clave-register` pipe).

**It is *not* called after the hydrate snapshot.** `main.rs:393-412`
(`RunCommandResult`) does `apply_snapshot` → `run_effects(fx)` with no
`fire_binds()`, while the byte-adjacent `clave-status` arm at `main.rs:264-270`
does. The hydrate (`main.rs:385-391`) is the **only** thing that populates
`self.agents` at session birth, and it is the one snapshot path that does not
kick the binder. `bind_effects` loops over `self.agents` (`model.rs:421`) — empty
agents means zero iterations, silently.

Losing interleaving, entirely ordinary:

| # | Event | Bind outcome |
| --- | --- | --- |
| 1 | `load()` → subscribe, `own_plugin_id` (`main.rs:342-381`) | — |
| 2 | `PermissionRequestResult` → `run_command(["clave","snapshot"])` (`main.rs:385-391`) | in flight |
| 3 | `TabUpdate` + `PaneUpdate` → `fire_binds` ×2 | `agents` empty → no-op |
| 4 | `clave-register` pipe → `register()` → `fire_binds` | `agents` still empty → no-op |
| 5 | `RunCommandResult` snapshot → `agents` populated | **`fire_binds` not called** |
| 6 | quiescence | **never bound** |

The first thing that reliably heals it is the user's own first
`UserPromptSubmit` — and by then `apply_hook_event` has already run with
`rec.tab_id == None`, so the stamp is skipped. **The first prompt to the eager
agent never moves its tab, by construction.**

A permanent variant exists: `register_pane` (`crates/clave/src/spawn.rs:55-93`)
is a double-forked fire-and-forget `zellij pipe --name clave-register`, emitted
once at pane start. If it lands before the wasm is loaded it is dropped, and
nothing re-sends it. Only the bar **in the same tab** can bind that agent
(`model.rs:426`), so no other instance can heal it.

**Bind guarantee, weakest to strongest:**

| | eager launch tab | `clave open` | `clave add` (Alt+a) |
| --- | --- | --- | --- |
| plugin state | **cold — first load, server also booting** | warm | warm |
| store bind to fall back on | **none — just wiped** | possibly stale | `None` (fresh) |
| push after tab creation | **none** | only if `stale` flipped (`store.rs:320-334`) | **guaranteed** (`add.rs:757-760`) |

**Discriminating observable:** if the register pipe was *lost*, that tab's own
bar shows a duplicate ghost `◌` row for the agent whose tab it is sitting in,
while bars in other tabs do not. Ghost visible only from inside the misbehaving
tab ⇒ register lost (permanent). No ghost but the tab still won't rise ⇒ the bind
kick was missed (heals on the second prompt).

---

## RC-C — timeline key semantics: ties, and the live→dormant discontinuity

Two independent problems, both pure sort maths, both reproducible without any race.

**Whole-second ties.** Both stampers use `now_unix()` at second resolution
(`store.rs:352-357`). Two interactions in the same second tie, and the tiebreak
is `tab.position` ascending (`model.rs:756,791`) — the lower-positioned tab wins
regardless of who was touched last.

**Live→dormant demotion reorders neighbours on every close.** Closing a tab
changes that row's key from `timeline[tab_id]` to `last_interacted` *and* its
tiebreak from `position` to `usize::MAX - i`. Worked example:

```text
store: tab_timeline = {10: 1000, 11: 1000};  agent u-A: tab_id=10, last_interacted=1000
tabs:  10 @ pos 0 (u-A),  11 @ pos 1 (plain terminal tab)

BEFORE  rows = 0: Tab(10)  ← u-A, amber ●
               1: Tab(11)

close tab 10 → prune removes timeline[10], clears u-A.tab_id → u-A dormant

AFTER   rows = 0: Tab(11)         ← "an unrelated tab jumped to the top"
               1: Dormant u-A     ← dim ◌ "went idle"
```

Both reported close symptoms, one close, **no race required**. This is the
by-design baseline underneath the RC-A defect.

Aggravator: `apply_touch` bumps *only* the timeline, leaving `last_interacted` at
its old value (often `0` for a never-prompted agent), so a touch-only tab plunges
from the top to the very bottom on close.

**Zellij position renumbering is NOT a cause of reordering** — it is
order-preserving, so an ascending-position tiebreak yields the same relative
order. Refuted; do not chase it.

---

## RC-D — there is no signal for "the user gave an instruction to a terminal"

Verified against the vendored crates at
`~/.cargo/registry/src/index.crates.io-*/zellij-utils-0.44.3/`. Note
`zellij-server` is **not** vendored, so *emit conditions* for server-side events
cannot be read from source — that is what the S2 spike exists to settle.

**`InputReceived` carries nothing.** `zellij-utils-0.44.3/src/data.rs:959-960`
declares it as a unit variant; the wire form confirms `payload: None`
(`src/plugin_api/event.rs:131-132`, `:697-698`). No key, no pane, no tab, no
client. It cannot distinguish a nav keybind from a keystroke in a terminal pane —
the documented cause of the fd storm.

**Three candidates that do carry a pane identity** (all new information; the
2026-06-30 design predates them):

- **`CommandChanged(PaneId, Vec<String>, bool is_foreground, Vec<ClientId>)`** —
  `data.rs:1016`, proto `event.proto:138-143`. Fires when the command running in
  a pane changes, and **already distinguishes foreground from background**. This
  is the closest thing to "a command started in this pane" and needs no shell
  config and no extra permission. **Emit conditions unverified — S2 spike.**
- `CwdChanged(PaneId, PathBuf, Vec<ClientId>)` — `data.rs`, event 39. Fires on `cd`.
- `UserAction(Action, ClientId, Option<u32> terminal_id, Option<ClientId>)` —
  `data.rs:1010-1011`. Identifies the pane, but requires
  `PermissionType::InterceptInput`, which clave does not hold
  (`clave-bar/src/main.rs:356-361`) and whose name implies the plugin becomes an
  input sink.

**Fields that do NOT move when a command runs in a plain shell pane** (checked
against `PaneInfo`, `data.rs:2296-2347`): `exited`/`exit_status` (`:2312-2316`
— "most panes close themselves before setting this flag, so this is only relevant
to command panes"), `is_held` (`:2317-2319`), `terminal_command` (`:2331-2333`,
static — verified live: an agent pane reports the baked `clave spawn …` string
while actually running something else). `title` may move but the update logic is
server-side and unverifiable. `cursor_coordinates_in_pane` moves on every
keystroke *and* on output — indiscriminate.

**Bells are output-side** (`TabInfo.has_bell_notification`, `data.rs:2271-2274`)
and would fire on background completion — the exact thing to exclude.

**Shell-hook viability.** `ZELLIJ_TAB_ID` is **absent** — grep across both
vendored crates returns nothing, and a live pane's full zellij env is
`ZELLIJ=0`, `ZELLIJ_PANE_ID=0`, `ZELLIJ_SESSION_NAME=clave`. A shell hook knows
its **pane**, never its tab. Resolution paths, measured live:

| command | wall | resolves pane→tab? |
| --- | --- | --- |
| `zellij action list-panes -t` | ~0.04s | **yes** — TAB_ID/TAB_POS/TAB_NAME/PANE_ID columns |
| `zellij action list-panes -t -j` | ~0.19s | yes (JSON, `PaneListEntry`, `data.rs:2350-2362`) |
| `zellij action dump-layout` | ~0.07s | **no** — no ids at all in the output |
| `zellij action current-tab-info` | ~0.04s | **no** — returns the session's *active* tab, not the caller's |

Cheaper in-process alternative: the bar already holds the join —
`PaneMeta { tab_position, pane_id, .. }` (`model.rs:26-31`, built at
`main.rs:452-466`) and `fn tab_position_of_pane(&self, pane_id: u32)`
(`model.rs:396`). This is exactly how `clave-register` already works
(`spawn.rs:55-80`).

**Shell integration today: absent.** No rc-file write anywhere in `crates/`;
policy stated at `crates/clave/src/discover.rs:7` — *"the user's shell config is
their business; clave just works."* The preexec design was explicitly parked:
`docs/superpowers/specs/2026-06-30-clave-orchestrator-design.md:539-540`,
`SUBSYSTEM-VALIDATION.md:260-262`.

**SSH.** The plugin and every `run_command` child run wherever the zellij
*server* runs, so a plugin-side signal holds under SSH by construction. A shell
hook holds for "user SSHes in and attaches", but breaks for `ssh other-host`
*inside a pane*: `ZELLIJ_*` are ordinary env vars that ssh does not forward, and
the remote host has no socket, no store and probably no `clave`.

`clave touch` today takes a tab id only (`crates/clave/src/main.rs:93-101`,
`store.rs:213-220`), and `apply_touch` pushes a snapshot **unconditionally** —
unlike the change-gated `apply_bind`/`apply_prune_tabs` (`store.rs:227-253`).

---

## RC-E — tab close: the residual races beyond RC-A

**The close path.** `Alt+w` → `CloseTab` (`crates/clave/src/setup.rs:95`) is the
only sanctioned close; `Ctrl+q`/`Ctrl+t` were unbound by #28 (`setup.rs:139`).
Zellij closes, renumbers positions, focuses a survivor, emits `TabUpdate` →
`apply_tabs` (`model.rs:593-715`). Stale detection, `model.rs:694-712`:

```rust
if !live.is_empty() {
    let mut stale: BTreeSet<usize> = self.agents.iter()
        .filter_map(|a| a.tab_id).filter(|id| !live.contains(id)).collect();
    stale.extend(self.timeline.keys().copied().filter(|id| !live.contains(id)));
    if !stale.is_empty() { effects.push(Effect::PruneTabs { stale_ids: … }); }
}
```

Emission is **detection-driven, not set-change-gated** (`model.rs:673-693`) — it
re-derives every `TabUpdate` until the store's prune echo clears the mirror.

**Effect ordering.** `ReanchorVisit` (`model.rs:636`) → `MarkRead` (`:653`) →
`PruneTabs` (`:708`), executed in order (`main.rs:89`), **all gated on the same
`active` bool** computed once at `main.rs:88`. Asymmetric failure: `PruneTabs`
retries; `ReanchorVisit` does **not**, because `self.current_tab` was already
mutated at `model.rs:635`, so `stranded` is false forever after on that instance.
Documented trade at `model.rs:621-624`.

**Tab id reuse.** Recorded as independently verified (`docs/status/2026-07-22-1606-clave-orchestrator.md:54-58`):
`get_new_tab_id` = `self.tabs.keys().last() + 1` over a `BTreeMap`, so **closing
the highest-id tab recycles that id**. Cited in-code at `model.rs:67`,
`model.rs:660-663`, `store.rs:232-238`, `store.rs:262-263`. The race, accepted as
residual at `store.rs:274-276`:

```text
t0  close highest-id tab 11 → bar emits PruneTabs{[11]}
t2  `clave prune-tabs 11` SPAWNED (fire-and-forget)
t3  before it takes the flock: a new tab is created → gets recycled id 11
    its bar birth-touches → tab_timeline[11] = now;  binds → u-new.tab_id = 11
t4  the t2 subprocess takes the flock:
      store.rs:284      tab_timeline.retain(…)  → deletes the FRESH entry
      store.rs:286-290                          → u-new.tab_id = None
t5  new tab sorts at key 0 (bottom), loses its glyph, u-new duplicates as ◌
```

Unrecoverable until the next prompt: the birth touch is spent
(`model.rs:383-385`) and `sent_binds` blocks the re-bind (`model.rs:429`).

**Every path to `Idle`** (relevant because "it goes idle" has four causes):

| Path | file:line | Close-triggered? |
| --- | --- | --- |
| `SessionEnd` hook → `Status::Idle` | `hook.rs:45`, registered `setup.rs:255` | **yes, directly** — pane dies, claude exits |
| `apply_focus`: Done→Idle | `store.rs:201-203` | **yes, on a *different* agent** — close forces focus onto a survivor → `MarkRead` (`model.rs:648-654`) |
| dormant glyph `('◌', 90)` — **same dim 90 as `Status::Idle`'s `●`** (`clave-types/src/lib.rs:29`) | `model.rs:775` | **yes, on every close** — dormant rows ignore status entirely |
| RC-A wrong-bind eviction | `store.rs:239-245` | yes — and here it is genuinely **wrong** |
| `apply_prune_tabs` | `store.rs:278-298` | **no — it never writes `status`** |
| `merge_resume_record` → Idle | `add.rs:343-354` | no — `clave add` resume only |

**Concurrency.** Two concurrent prunes are safe — `with_store_mut`
(`store.rs:135-164`) holds an exclusive flock across read→mutate→write, removals
commute, the loser sees `changed == false` and pushes nothing. Prune vs
touch/bind is **not** safe: they serialize but their order is arbitrary — the
recycled-id race above. `apply_touch`'s max-merge gives no protection against a
later deletion.

**Also in the blast radius:** `Effect::SwitchTab { position }`
(`model.rs:891-900`, `:806-818`) resolves position from `self.tabs`; a nav or
click processed between the close and the renumbering TabUpdate lands on the
wrong tab (`main.rs:96-101`).

---

## RC-F — the label is frozen at `clave add`, and Claude's rename is never read

**What a row displays.** No precedence chain — two disjoint branches:

- **Live tab row** (`model.rs:746-765`): `name: t.name.clone()` — the **zellij
  tab name**, from `TabInfo.name`. The joined agent is consulted *only* for the
  glyph (`model.rs:747`, `:406-408`); `Agent.label` is never read here.
- **Dormant row** (`model.rs:767-791`): `name: a.label.clone()` — the store label.

They converge because clave writes the label onto the tab: born as
`tab name="{label}"` (`add.rs:106,127`; `setup.rs:181,207`), then renamed via
`Effect::RenameTab` → `rename_tab_with_id` (`main.rs:137-139`), emitted **only on
label change** (`model.rs:569-582`) and executor-gated to the active instance.

**The prefix is captured once and never re-read** — `add.rs:699-711`:

```rust
let label = match &existing {
    Some(row) => sanitize_label(&row.label),
    None => {
        let dir_name = agent_cwd.rsplit('/').next().unwrap_or(&agent_cwd);
        sanitize_label(&format!("{dir_name} · {agent_branch}"))
    }
};
```

`cwd` is written only at record creation (`add.rs:745`). `merge_resume_record`
preserves cwd/branch/label/label_source verbatim and deliberately
(`add.rs:331-355`), pinned by `merge_resume_preserves_existing_row_and_resets_status`
(`add.rs:890-921`). `refresh_label` rebuilds the prefix every call
(`hook.rs:158-170`) but always from the frozen `rec.cwd`. **That is the stale
`issue-10-kdl-guardrail`.**

**Nothing re-reads cwd mid-session — confirmed absent.** `current_dir()` appears
once, at `add.rs:481` (the picker). No assignment to `rec.cwd` outside record
construction. Hooks only *read* it (`hook.rs:160,283`). `HookPayload`
(`hook.rs:22-34`) **does not even deserialize Claude's `cwd` field, which every
hook event carries** — that is the cheap, exact seam.

**`F-CLA` is Claude's own session title, not clave's.** clave seeds it once via
`--name` at create (`crates/clave/src/main.rs:251-255`; `Resume` passes only
`--resume`). Thereafter Claude owns it, re-appending
`{"type":"custom-title","customTitle":…}` to the jsonl, latest-wins
(mechanism recorded at `docs/status/2026-07-21-1658-clave-orchestrator.md:53-57`).
Grep for `customTitle|custom-title|custom_title` across `crates/` returns
**exactly one hit — a comment** at `hook.rs:79-82` naming it as future work. No
code reads it. Claude's rename updates the *pane* title (`PaneInfo.title`,
`data.rs:2310-2311`), which the bar discards at `main.rs:458-463`; it never calls
`rename_tab_with_id`.

**The read is already there.** `refresh_label` tails the same jsonl for summaries
— `summary_from_tail` (`hook.rs:120-135`) over `read_tail`'s last 64 KiB
(`hook.rs:139-147`), invoked when `label_source == FirstPrompt` and the event is
`Stop`/`UserPromptSubmit` (`hook.rs:277-285`). A `custom-title` tier is an
extension of a read already happening.

**One structural obstacle:** `refresh_label` hard-stops on
`label_source == LabelSource::Summary` (`hook.rs:155-157`) — *"once a summary
named the session, the label is frozen forever."* Titles change repeatedly, so a
title tier must sit above that stop and stay live.

**Injected-prompt guard** (keep it): `HARNESS_INJECTED_PREFIXES`
(`hook.rs:83-88`) = `<task-notification`, `<system-reminder`,
`<local-command-caveat`, `<command-name`, matched on `trim_start().starts_with`
(`hook.rs:93-96`). It skips the upgrade wholesale rather than stripping, so
`label_source` stays `FirstPrompt` and the next real prompt still earns.

---

## RC-G — no per-row colour, and `Row.name` is a single opaque string

**Raw ANSI already works in the pane** — `crates/clave-bar/src/main.rs:539-559`:

```rust
let gutter = match row.glyph {
    Some((glyph, colour)) => format!("\u{1b}[{colour}m{glyph}\u{1b}[0m "),
    None => "  ".to_string(),
};
let budget = cols.saturating_sub(3);          // gutter + margin
let name: String = if row.name.chars().count() > budget {
    let mut n: String = row.name.chars().take(budget.saturating_sub(1)).collect();
    n.push('…'); n
} else { row.name.clone() };
if row.active { println!("{gutter}\u{1b}[7m{name}\u{1b}[0m"); } else { println!("{gutter}{name}"); }
```

Only two ANSI sites exist: the glyph colour (`main.rs:541`) and the active-row
reverse-video (`main.rs:555`). Row **text** carries no colour today. Colours come
from `Status::glyph()` (`crates/clave-types/src/lib.rs:24-32`, SGR 31/33/32/90)
and the dormant glyphs at `model.rs:770-777`.

**The width clamp is not escape-aware.** It counts Unicode scalars via
`.chars().count()`/`.take()` directly on `row.name`. Embedding ANSI *into*
`row.name` would count escape bytes as visible characters — truncating text early
or, worse, mid-escape, leaking raw escape text or leaving the line coloured with
no reset. Colour must be applied **after** clamping, on segments.

**`Row` is `{ key, name: String, active, glyph }`** (`model.rs:163-169`). To
colour only the repo segment it needs a structured field. Useful fact: **`Agent`
already carries `repo_root`** (`clave-types/src/lib.rs:44-45`, doc: *"git toplevel
of cwd; the grouping key in the bar"*), populated everywhere
(`store.rs:45,178`, `add.rs`, `dev.rs:232`) and delivered to the plugin in every
snapshot — but **`rows()` never reads it** (`model.rs:747`, `:783-791`). A plain
terminal tab has no repo, so any new field must be `Option`.

**Consumers of `Row.name`:** the clamp and `println!` (`main.rs:547-557`), and
tests at `model.rs:1229-1234`, `:1237`, `:1246`, `:1249`, `:1305-1306`, `:2106`.
Nothing outside `crates/clave-bar` reads it. `main.rs` is `test = false` (wasm
bin), so no test asserts on rendered output — the render half is unguarded.

**zellij-tile styling.** A `Text` builder exists
(`zellij-tile-0.44.3/src/ui_components/text.rs`) with `print_text`,
`serialize_text`, `color_range`, and semantic index-levels (`DIM_LEVEL`,
`ERROR_COLOR_LEVEL`, …) resolved host-side from the user's theme. It offers **no
arbitrary-palette API** — you cannot say "colour #4 of my 10". Raw ANSI is not so
limited: `\x1b[38;2;R;G;Bm` gives truecolor. Theme access exists if wanted —
`Event::ModeUpdate(ModeInfo)` carries `Style { colors: Styling }`
(`zellij-utils-0.44.3/src/data.rs:1375-1379`, `:1724-1728`), and `PaletteColor`
(`data.rs:1211-1213`) is `Rgb((u8,u8,u8)) | EightBit(u8)`. clave does **not**
subscribe to `ModeUpdate` today (`main.rs:363-369`).

**Stable hashing: nothing exists, and the obvious choice is wrong.** No
`DefaultHasher`/`std::hash`/`SipHash` usage anywhere in `crates/` — grep returns
zero. `DefaultHasher` is **not** stable across toolchain versions; using it would
silently reshuffle every repo's colour on a `rustc` upgrade, violating the
stability requirement. A fixed algorithm (e.g. FNV-1a) must be written locally;
`clave-bar` has no hash crate dependency (`Cargo.toml`: `zellij-tile`,
`clave-types`, `serde_json`, dev-dep `proptest`).

---

## Read-only live diagnosis

Non-mutating. `read_store` is lock-free-safe — writers use temp+atomic-rename
(`store.rs:120-129`), so reading cannot tear or block the fleet.

```bash
# 1. Full store truth: binds + timeline + statuses + seq.
clave ls --json | jq '{seq, tab_timeline,
   agents: [.agents[] | {uuid, label, status, tab_id, stale, last_interacted}]}'

# 2. Raw store (adds worktree + label_source, which the snapshot drops).
jq . "$HOME/.local/state/clave/agents.json"

# 3. Ground truth for tab ids and positions, from the server.
zellij action list-panes -t

# 4. Bar versions + plugin stderr (macOS).
grep -n 'clave-bar' "$TMPDIR/zellij-$(id -u)/zellij-log/zellij.log" | tail -50

# 5. Which `clave` the plugin's shellouts actually resolve to (issue #44).
command -v clave && clave --version
```

| Observation | Conclusion |
| --- | --- |
| an agent's `tab_id` names a **live** tab hosting a *different* agent's pane, and that tab's `tab_timeline` entry is the newest | **RC-A confirmed** — the signature to look for |
| an agent has `tab_id: null` but its pane is visibly open | **RC-A** (the `apply_bind` eviction victim) |
| the eager agent (`grep '"cmd":"launch"' "$HOME/.local/state/clave/clave.log" \| tail -3`) has `tab_id: null` while others have integers | **RC-B confirmed** |
| that agent is also **missing from `tab_timeline`** | the birth touch was missed too → key 0 → below dormant rows |
| a brand-new tab is missing from `tab_timeline` and its agent has `tab_id: null`, and the closed tab was the **highest id** | **RC-E recycled-id race** |
| two rows have **equal** `tab_timeline` values and the one that jumped has the higher `position` | **RC-C** — by-design demotion, not a defect |
| a different, untouched agent flipped `done → idle` with `last_visited` == close time | collateral `MarkRead` — correct behaviour |
| `clave --version` ≠ the version in `clave-bar: loaded vX.Y.Z` log lines | **issue #44/#43** — treat every other reading as suspect until fixed |

**Caveats on the diagnostics themselves.** `clave.log` is blind to this bug
class: `touch`, `bind`, `prune-tabs`, `focus` and `collapse` call no `log_event`
(only `add.rs:761`, `open.rs:49/74/81/91/95/128`, `main.rs:226`, `dev.rs:254`,
`setup.rs:663/669/699`). `clave ls --json` emits an `AgentSnapshot`, which omits
`worktree` and `label_source` (`store.rs:167-189`). `clave ls`'s human output
sorts by `last_interacted` (`lsview.rs:13-15`), **not** by the bar's rule — the
divergence between `clave ls` order and sidebar order is itself the RC-A/RC-B
signature. `clave dev status` is the **wrong tool**: `run_status` calls
`enter_sandbox` first (`dev.rs:262-265`), so it reads the sandbox store.

---

## Workstream split and sequencing

Split so that two agents in two worktrees do not edit the same seam.

| # | Workstream | Root causes | Primary files | Depends on |
| --- | --- | --- | --- | --- |
| **S0** | Frame coherence & executor election | RC-A, RC-B | `clave-bar/src/main.rs:43-71,87-231,434-467`; `model.rs:414-439` | — |
| **S1** | Prompt→top ordering semantics | RC-C | `model.rs:391-393,728-793`; `store.rs`; `hook.rs` | S0 |
| **S2** | Terminal-tab interaction signal | RC-D | spike first; then `main.rs` subscribe + `store.rs` | spike |
| **S3** | Tab-close correctness | RC-E | `model.rs:593-715`; `store.rs:278-298` | S0, S1 |
| **S4** | Label: Claude rename + live cwd | RC-F | `hook.rs`; `add.rs`; `clave-types` | — |
| **S5** | Per-repo + per-title colour | RC-G | `main.rs:525-560`; `model.rs` `Row`/`rows()`; `store.rs` (ink allocation) | — (see note) |
| **S6** | Three-cell gutter (status · battery slot · worktree) | — (row identity) | `model.rs` `compose_row`/`gutter_segments`; `store.rs` snapshot | — |
| **S7** | Context battery (deferred, unspecced) | #24 item 4 | fills S6's reserved cell | S6 |
| **S8** | Sidebar width 30→38 | — (#24 item 6) | `model.rs` seek consts; `clave-types`; `setup.rs` KDL | — |

**S0 first, alone.** It fixes the sticky mis-bind behind both the worst
ordering symptom and the worst close symptom, and S1/S3 build on its seam.

**S2, S4, S5, S6, S8 are parallelisable with each other and with S0** — different
files. S5 keys colour on `Agent.repo_root` (already in the snapshot, already
unread), so it does not depend on S4's label composition; both touch row
identity, so whichever lands second rebases. S6 (gutter) and S8 (width) both
parameterise the text budget rather than hardcoding it, so they compose with S4/S5
regardless of landing order.

**S1 and S3 are not parallel with each other** — both edit `model.rs`
ordering/`apply_tabs`. Run them in sequence after S0: S0 → S1 → S3.

## Rules for every workstream

- `AGENTS.md` is the operating agreement. Read it first.
- `docs/dev/TESTING.md` — pick evidence by the risk taxonomy before choosing what
  to test. Tier 2 does **not** exist (#47), so anything crossing the process or
  environment seam needs a written argument in the PR dossier plus an adversarial
  reviewer.
- Never launch or kill a zellij session; never `just release`, `cargo install`,
  or `just dev-install` while the maintainer may be daily-driving; never write
  under `~/.claude/`; never commit without explicit approval.
- Every live-validation step is the **maintainer's** to execute. Print the
  command; do not run it.
- Issue #44 (the bar shells out to bare `clave`) is **unfixed**. It can corrupt
  any live reading. Confirm `clave --version` matches the loaded bar version
  before trusting a live observation.
