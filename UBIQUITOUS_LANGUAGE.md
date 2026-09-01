# UBIQUITOUS_LANGUAGE.md — clave

The shared vocabulary. One word, one meaning, everywhere: code, specs, issues,
PRs, commit messages, and conversation.

This exists because clave straddles three namespaces that all reuse the same
English words. **Zellij** has sessions, tabs and panes. **Claude Code** has
sessions and conversations. **clave** has its own rows and labels layered over
both. "Session" alone is ambiguous three ways, and an ambiguous word in a spec
becomes a defect in code.

**Rule: if a term is on this page, use it exactly. If you need a new one, add it
here in the same change.**

---

## 1. The two host namespaces

| Term | Means | Never means |
|---|---|---|
| **zellij session** | The multiplexer session — the thing `zellij attach` connects to. Its lifecycle belongs to the human, always. | a Claude conversation |
| **tab** | A zellij tab. Identified by `tab_id` (stable, but zellij **recycles** ids) and by `position` (renumbers on close). One row of the sidebar. Two kinds, below. | a browser tab, a Claude session |
| **terminal tab** | A tab with no agent session bound — a plain shell. Opened with `Alt+t`. | an agent tab whose bind failed (that is a defect, not a kind) |
| **agent tab** | A tab hosting an agent session. Opened with `Alt+a` → pick repo → "new" or "resume". | any tab that merely has Claude running in it by hand |
| **pane** | A zellij pane inside a tab. | a tab |
| **plugin pane** | The pane running `clave-bar`, one per tab. Every tab has its own bar instance. | the sidebar as a concept |
| **workspace** | The non-sidebar pane of a tab — where the terminal or the Claude runs. Both are "the terminal" underneath; the workspace is the UX word for that side of the split, in either tab kind. | a pane the sidebar owns |
| **agent session** | One Claude Code conversation, identified by its minted **uuid**. Say "agent session" or "conversation" — never bare "session". | a zellij session |
| **minted uuid** | The id `clave add` generates and passes as `--session-id`. The store's join key, **stable for the life of the row** — binds, the tab timeline and every wire field depend on it never moving. | the session id in a hook payload |
| **live session id** | The id Claude is using *right now*, in the hook payload and in the transcript filename. Starts equal to the minted uuid and **changes** when the pane gets a fresh conversation — a `/clear` does it; a `--resume` does NOT (both measured, 2026-07-31, CLI v2.1.220: a resume continues the resumed transcript under its own id). Never the STORE's key — the minted uuid is that, always. It is a translation input: `AgentRecord::live_session` remembers it (`None` while the two are not known to disagree) because resurrection has to name a conversation before any Claude exists to report one, and `resume_candidates`, `live_uuid_union` and `open_is_live` map it back to the row it belongs to (#99). | the minted uuid |

> The single most expensive ambiguity in this project. When you write "session",
> qualify it.
>
> The minted/live split is the same trap one level down, and it cost #97: the
> two are equal right up until the first `/clear`, so code that conflates them
> works perfectly in every test and freezes the row in the field.

---

## 2. clave's own nouns

| Term | Means |
|---|---|
| **fleet** | All agent sessions clave knows about, live and dormant. The thing the sidebar shows. A fleet is **launched, killed, seeded — never collapsed or expanded**: width states belong to the sidebar. |
| **store** | The on-disk JSON that hooks write and the CLI reads. The single writer of truth *about panes*; the plugin never reads it directly. See the canon rule below. |
| **transcript** | A conversation's jsonl under Claude's own `projects/` tree. **The canon of conversations** — see the canon rule below. |
| **snapshot** | The full-replace payload the CLI pipes to the plugin. Carries **seq**, a monotonic counter — a consumer applies only strictly-newer seq and discards the rest. |
| **bind** | Associating an agent session's uuid with a `tab_id`. Done once, by the agent tab's own bar instance. |
| **hook** | A Claude Code lifecycle callback that writes into the store. The source of status and recency. |
| **agent** | One record in the store / one entry in a snapshot. The data; a **row** is its rendering. |
| **frecency** | The default row-order score: Σ commitment count × `0.5^(age_days × 24 / half_life_hours)` over a row's day buckets. Ranks invested threads above whatever was merely touched last. `OrderMode::Recency` is the pure-ordinal alternative, still selectable (`clave order recency`). |
| **day bucket** | `unix_day → commitment count`, where frecency's weight is banked. Written twice per commitment, same doubling as `tab_order`/`commit_ord`: on the `AgentRecord` (uuid-keyed, survives dormancy) and on `Store.tab_buckets` (tab-keyed, covers terminal tabs). Pruned to the trailing 7 days on every write — and, because that prune is lazy (it runs only on a row's next bump), a bucket outside the window scores ZERO in the bar at every dial. |
| **opener** | The tab a newborn row's buckets are copied from at creation: the tab holding the max `tab_order` ordinal, a store-native proxy for "the one you were just working in". Identical buckets plus the existing position tiebreak land the newborn directly below its opener. |

> **The canon rule** (maintainer ruling, 2026-08-17): **Claude's jsonl is the
> canon of conversations; clave's store is a disposable snapshot of panes.** A
> store field earns its place only by recording what the jsonl cannot cheaply
> answer — and the load-bearing case is "which conversation does *this pane*
> currently hold" (`live_session`), because a project dir holds a transcript
> for every claude ever run there (#99's banned heuristic). Anything read back
> out of the store must defer to the jsonl at the point of use — existence-check
> before trust (`resume_target`), discover from the project dir first
> (`resume_candidates`). No lineage lists, no backfill guessing: a stale
> snapshot degrades to "resume opens the older conversation", never to loss.

---

## 3. The sidebar

> The terms in this section describe a design that is **locked and rendered**:
> [`docs/superpowers/specs/2026-07-25-sidebar-visual-design-lock.md`](docs/superpowers/specs/2026-07-25-sidebar-visual-design-lock.md)
> is the ruling and the reasoning; `cargo run -p clave-bar --example bar-preview`
> draws it. If a word here is unclear, run the preview — it is one screen.

The vertical bar itself is the **sidebar** (or **the bar**) — and in UX
language it is **singular**: one continuous sidebar for the whole zellij
session. That each tab carries its own plugin pane is implementation detail;
the UX contract is that the instances are indistinguishable, and a user ever
noticing a "per-tab bar" (mismatched widths, a tab out of step) is that
abstraction leaking — a defect, not a nuance to document around.

Its two width
states are **expanded** and **collapsed** — never "mini", "wide" or "narrow".
Each is a **geometry**: a named layout in the generated KDL that zellij switches
the tab between, and reports back by name. The **birth position** is the third,
unnamed one zellij hides at the head of that cycle — the layout the tab was
created from. (The **width seek**, the loop that used to step the pane toward a
column target, was deleted at #181; the term survives only in the ledger.)

### 3.1 A row

**Row** — one entry of the fleet as the bar draws it. Three kinds:

- **live row** — an agent session with a zellij tab open.
- **dormant row** — an agent session with no tab open.
- **terminal tab** — a zellij tab with no agent session bound to it.

**Row is the data-side word — a row is what gets rendered, never the shape it is
rendered in.** The shape is the **row height**, and there are two:

| Term | Means |
|---|---|
| **card** | A row's **two-line** rendering, the default since #232. Line 1 is status, chip and token count; line 2 is identity — provenance, repo, branch, PR, provider, model, effort and elapsed. The two lines are one unit: one click target, one viewport slot, one zebra parity. Locked in `docs/superpowers/specs/2026-08-26-double-height-card-lock.md`. |
| **single-line row** | The original one-line rendering, retained behind `clave rows single` and locked in `docs/superpowers/specs/2026-07-25-sidebar-visual-design-lock.md`. Its geometry is §3.2 below. |

Both come in the same two **width states**, expanded and collapsed; height and
width are independent choices.

**Live and dormant rows both take their text from the STORE record** — title,
repo and summary as separate fields. A live row does *not* render the zellij tab
name; a tab name has no field structure and cannot fill fixed-width columns.
Only a terminal tab, which has no store record, renders its tab name. Ruled in
the design lock §7.1.

**speaker** — the pane that speaks for a terminal tab's row: the tab's focused
tiled terminal, falling back to its first. Plugin panes never speak (the bar
lives in one) and floating panes don't either — the row describes the tab's
resident content. Status, the cwd-derived repo cell, borrowed provenance and
the command summary are all read off the speaker (#206).

### 3.2 A row's parts

```text
 ●  │  105k 𖣂   S6-GUT   clave    picking the gutter
└┬┘ └┬┘ └─┬┘ └┬┘  └──┬─┘  └──┬─┘  └───────┬────────┘
status rule battery │     title    repo        summary
                provenance
└───────── gutter ─────────┘└────── text area ────────┘
```

| Term | Means |
|---|---|
| **gutter** | The fixed-width region at the left of every row. **Position-locked**: each cell holds its column and renders a space when its glyph is absent, so nothing ever reflows. Every cell is one column except the **battery** in the expanded view, which is four — so the gutter is 12 columns expanded, 9 collapsed. |
| **cell** | One fixed slot of the gutter, referred to by its role — the *status cell*, the *battery cell*, the *provenance cell*. Not by number, and not necessarily one column: the battery cell is four in the expanded view. |
| **rule** | The vertical line separating the status cell from the rest, so the status hue is not read against the battery hue. |
| **cap** | The powerline half-circle at each end of the **selected row**. Its column is reserved on every row so nothing shifts. |
| **text area** | Everything right of the gutter: title, repo, summary. |
| **status glyph** | The dot. Its **colour** is the state — the shape barely varies. On a terminal tab the glyph is the console mark instead, coloured the same way: Running / Done / Failed for a command pane, Idle / Running for a shell — an interactive shell never exits while its tab lives, so it has no Done or Failed. A shell binary absent from `SHELLS` degrades to always-Running, its argv reading as a command that never finishes (#206). |
| **battery** | How much of its **smart zone** that agent session has spent. Two readings of one number: the expanded view prints the **count** — thousands of tokens, right-aligned, inked with the ramp's band (`105k`, `1.1m`) — and the collapsed view shows the **ramp glyph**, which empties a tenth at a time. A terminal tab has no context window; its battery cell shows `TERM` expanded and the prompt glyph collapsed, and the console mark lives in the status cell (#206 — this moved; it used to sit here). |
| **smart zone** | How many tokens of context *this user* trusts a model to stay sharp within — set once, globally, in `CLAVE_AGENT_SMART_ZONE_TOKENS` (default 150,000). Explicitly **not** the model's context window: the window is where Claude auto-compacts, which is not a thing anyone steers by, and the same smart zone holds across a 200k model, a 1M model, or a future non-Claude agent. It is where the battery turns **red** — not where the ramp ends. |
| **provenance** | Whether the row's checkout is a **worktree**, on a **branch**, or a **main checkout**. Rendered as a glyph tinted with the repo ink; a main checkout shows nothing. |

### 3.3 The three text fields

These three are constantly confused. They are not interchangeable.

| Term | Is | Example |
|---|---|---|
| **title** | The name **you** gave this tab — the rename. Optional; absent until you name it. Rendered as a filled **chip**. | `S6-GUT` |
| **repo** | The repo name, derived from the git toplevel of the agent's cwd. Rendered as tinted text. | `clave` |
| **summary** | What the agent is currently doing, from its hooks. | `picking the gutter set` |
| **label** | The *composed* string clave writes onto the tab: title, repo and summary joined. A **field of the store**, not a thing you see as a unit. | `S6-GUT · clave · picking…` |

> **title vs label** is the trap. The *title* is the user's rename; the *label*
> is the whole composed line. `compose_label` builds a label out of a title.

### 3.4 Colour

| Term | Means |
|---|---|
| **ink** | A colour drawn from the palette and assigned to something. **repo ink** is keyed by repo root — one repo, one colour, forever. **title ink** is keyed per title *within* its repo — two tabs of the same repo never share one. |
| **palette** | The fixed, ordered set of hues. Allocated **round-robin** — never hashed. Always eight entries, under any theme. |
| **theme** | The user's zellij theme, mapped onto the bar's colour roles (`Theme`, LEDGER D42). **Theme-following** colours (backgrounds, inks, palette) repaint with it; **fixed semantic** colours (status marks, battery bands) never do — red means failed everywhere. The default theme is the curated kanagawa design. |
| **chip** | Text on a filled background. Two chips exist: an agent row's title (title-ink background) and a terminal tab's name — fujiWhite on black, black meaning *unclaimed by agent ink*, and it stays black on the selected row (#206). |
| **tint** | Foreground colour on text or a glyph, no background. |
| **fade** | Rendering a row blended toward the bar background. Unselected rows are faded; **selection is by recession**, not by ornament. |

### 3.5 The viewport

| Term | Means |
|---|---|
| **viewport** | The slice of the row list the pane actually shows. It rests at the top of the list and scrolls when the selection and its lookahead would exceed the bottom edge — overflow accumulates at the bottom, and while the selection sits in the live block the live rows stay in view (chat-app scrolling). A dormant selection held deep keeps the viewport scrolled — earlier rows, the live block included, legitimately slide out of view and return when a pick lands back in the live block; that is the scroll, not a defect. **Derived, never remembered**: recomputed each draw from list, selection and pane height. No overflow markers: a list reaching the pane's bottom edge is itself the signal that more rows exist. |
| **lookahead** | The couple of rows kept visible *below* the selection while the viewport scrolls, so you can see what you are walking into. |

### 3.6 Row states

| Term | Means |
|---|---|
| **selected** | The row for the currently focused tab. Exactly one. |
| **live** / **dormant** | Has a tab open / does not. See §3.1. |
| **unread** | Finished while you were not looking — `done && !visited`. |
| **stale** | `clave open` found the row's cwd missing. A row flag, **not** a status. |

`Status` — the enum — has exactly five variants and they are spelled this way:
**Idle, Working, NeedsYou, Done, Failed**.

---

## 4. Environments and artifacts

| Term | Means |
|---|---|
| **stable** | The released install under `~/.local/share/clave/`. Only `just release` writes it. |
| **launcher** | The **unversioned** entry point a cut installs at `~/.local/share/clave/bin/clave` — the one name an operator *types*, refreshed on every release (#43a). Never a *baked* reference: generated artifacts always name the **versioned copy**, because an unversioned plugin location is a different plugin identity to zellij. |
| **versioned copy** | `<data>/bin/clave-vX.Y.Z` — the immutable per-cut CLI that keybinds, layouts and hooks bake. Typed by nobody. |
| **dev binary** | `~/.cargo/bin/clave-dev` — the working-tree build from `just dev-install` (#43b). It shares a name with neither of the above; that is the point. |
| **sandbox** | The isolated dev environment and its zellij session. The only place an agent may hot-reload. |
| **sandbox instance** | One working tree's sandbox — its session name, state dir, data dir and shim dir, all derived from one **sandbox key**. The main checkout's is `clave-test` at `~/.local/state/clave-dev`; every linked worktree gets its own. Two agents in two worktrees can stage and drive at the same time. |
| **sandbox key** | The worktree directory name, lowercased, non-alphanumerics collapsed to `-`, and truncated with a short digest if it would overflow zellij's socket-path budget. `None` for the main checkout — that is what keeps its familiar names. |
| **reap** | `clave dev reap`: deleting the sandbox roots whose originating worktree no longer exists. Deletes directories only; a live session is printed for the human to kill. |
| **the one leak** | The PATH hazard that broke v0.1.1: the bar shelled out to bare `clave` and `dev-install` owned that name, so a working-tree build silently took over the running fleet. Closed by #44 (no PATH resolution), #43a (the launcher) and #43b (the dev binary). See CONTRIBUTING. |
| **handoff** | The session status document under `docs/status/`. Tracked; the newest is current state. |

---

## 5. House rules for writing

- **Glyphs are written as `\u{...}` escapes in source, never as literal
  characters.** Literals get silently eaten in transit; the failure mode is tofu
  in production from a diff that looked clean. This has bitten twice.
- **Cite, don't restate.** A comment says *why*, with a `file:line`, a spec §, or
  an issue number. The code already says what.
- **A lane that did not run is not a lane that passed.** Name the review lanes
  that actually executed.
