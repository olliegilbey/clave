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
| **tab** | A zellij tab. Identified by `tab_id` (stable, but zellij **recycles** ids) and by `position` (renumbers on close). One row of the sidebar. | a browser tab, a Claude session |
| **pane** | A zellij pane inside a tab. | a tab |
| **plugin pane** | The pane running `clave-bar`, one per tab. Every tab has its own bar instance. | the sidebar as a concept |
| **agent session** | One Claude Code conversation, identified by its minted **uuid**. Say "agent session" or "conversation" — never bare "session". | a zellij session |
| **minted uuid** | The id `clave add` generates and passes as `--session-id`. The store's join key, **stable for the life of the row** — binds, the tab timeline and every wire field depend on it never moving. | the session id in a hook payload |
| **live session id** | The id Claude is using *right now*, in the hook payload and in the transcript filename. Starts equal to the minted uuid and **changes** when the pane gets a fresh conversation (a `/clear`, and probably a resume). Never the STORE's key — the minted uuid is that, always. It is a translation input: `AgentRecord::live_session` remembers it (`None` while the two are not known to disagree) because resurrection has to name a conversation before any Claude exists to report one, and `resume_candidates`, `live_uuid_union` and `open_is_live` map it back to the row it belongs to (#99). | the minted uuid |

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
| **fleet** | All agent sessions clave knows about, live and dormant. The thing the sidebar shows. |
| **store** | The on-disk JSON that hooks write and the CLI reads. The single writer of truth; the plugin never reads it directly. |
| **snapshot** | The full-replace payload the CLI pipes to the plugin. Carries **seq**, a monotonic counter — a consumer applies only strictly-newer seq and discards the rest. |
| **bind** | Associating an agent session's uuid with a `tab_id`. Done once, by the agent tab's own bar instance. |
| **hook** | A Claude Code lifecycle callback that writes into the store. The source of status and recency. |
| **agent** | One record in the store / one entry in a snapshot. The data; a **row** is its rendering. |

---

## 3. The sidebar

> The terms in this section describe a design that is **locked and rendered**:
> [`docs/superpowers/specs/2026-07-25-sidebar-visual-design-lock.md`](docs/superpowers/specs/2026-07-25-sidebar-visual-design-lock.md)
> is the ruling and the reasoning; `cargo run -p clave-bar --example bar-preview`
> draws it. If a word here is unclear, run the preview — it is one screen.

The vertical bar itself is the **sidebar** (or **the bar**). Its two width
states are **expanded** and **collapsed** — never "mini", "wide" or "narrow".
The process that drives the pane toward a target width is the **width seek**.

### 3.1 A row

**Row** — one rendered line. Three kinds:

- **live row** — an agent session with a zellij tab open.
- **dormant row** — an agent session with no tab open.
- **terminal tab** — a zellij tab with no agent session bound to it.

**Live and dormant rows both take their text from the STORE record** — title,
repo and summary as separate fields. A live row does *not* render the zellij tab
name; a tab name has no field structure and cannot fill fixed-width columns.
Only a terminal tab, which has no store record, renders its tab name. Ruled in
the design lock §7.1.

### 3.2 A row's parts

```text
 ●  │  󰁻  𖣂   S6-GUT   clave    picking the gutter set
└┬┘ └┬┘ └┬┘ └┬┘  └──┬─┘  └──┬─┘  └────────┬─────────┘
status rule battery │     title    repo         summary
                provenance
└──────── gutter ────────┘└─────── text area ───────────┘
```

| Term | Means |
|---|---|
| **gutter** | The fixed-width glyph region at the left of every row. **Position-locked**: each cell is exactly one column and renders a space when its glyph is absent, so nothing ever reflows. |
| **cell** | One column of the gutter, referred to by its role — the *status cell*, the *battery cell*, the *provenance cell*. Not by number. |
| **rule** | The vertical line separating the status cell from the rest, so the status hue is not read against the battery hue. |
| **cap** | The powerline half-circle at each end of the **selected row**. Its column is reserved on every row so nothing shifts. |
| **text area** | Everything right of the gutter: title, repo, summary. |
| **status glyph** | The dot. Its **colour** is the state — the shape barely varies. |
| **battery** | The context-window level of that agent session. A terminal tab shows the console mark in this cell instead, because a terminal has no context window. |
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
| **palette** | The fixed, ordered set of hues. Allocated **round-robin** — never hashed. |
| **chip** | Text on a filled background. Only the title is a chip. |
| **tint** | Foreground colour on text or a glyph, no background. |
| **fade** | Rendering a row blended toward the bar background. Unselected rows are faded; **selection is by recession**, not by ornament. |

### 3.5 Row states

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
| **sandbox** | The isolated dev environment and its `clave-test` zellij session. The only place an agent may hot-reload. |
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
