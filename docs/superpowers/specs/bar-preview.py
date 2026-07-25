#!/usr/bin/env python3
"""clave sidebar — the LOCKED visual design, rendered.

    python3 docs/superpowers/specs/bar-preview.py

An ILLUSTRATION of the sidebar design ratified 2026-07-25 — not its authority.
`2026-07-25-sidebar-visual-design-lock.md` in this directory is authoritative
for every ruling, number and rationale; where this script and that document
disagree, the document wins and this script is the bug.

It is deliberately a standalone renderer with no clave dependency, so it runs in
any checkout and any terminal without building the wasm plugin, and it asserts
its own core invariant: every row is exactly COLS display cells, measured in
cells rather than code points.

FOLLOW-UP (tracked): this preview duplicates geometry that `compose_row` will
own once S5/S6/S8 land. At that point it should become a Rust example driven by
the real constants, so a code change that moves a column breaks the preview
instead of silently diverging from it. Until then, treat the prose spec as
authoritative and this file as its illustration.

GLYPH RULE — load-bearing. Every glyph below is spelled as a \\U000xxxxx escape,
never as a literal character. During the design rounds, literal glyphs were
silently lost in transit twice; the first time we misdiagnosed it as missing
font coverage and nearly constrained the whole design to one Unicode plane on
the strength of it. Escapes survive every tool in the chain. The same rule
applies to the Rust source: write '\\u{f062c}', not the glyph.
"""

import re
import unicodedata

# \u2500\u2500 ANSI \u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500
R, BOLD = "\x1b[0m", "\x1b[1m"


def rgb(h):
    h = h.lstrip("#")
    return int(h[0:2], 16), int(h[2:4], 16), int(h[4:6], 16)


def fg(h):
    r, g, b = rgb(h)
    return f"\x1b[38;2;{r};{g};{b}m"


def bg(h):
    r, g, b = rgb(h)
    return f"\x1b[48;2;{r};{g};{b}m"


def mix(h, toward, t):
    """Blend `h` toward `toward` by t. Unselected rows are rendered at
    FADE toward the bar background — selection by recession (§4)."""
    a, b_, c = rgb(h)
    d, e, f_ = rgb(toward)
    return "#%02X%02X%02X" % (round(a + (d - a) * t), round(b_ + (e - b_) * t),
                              round(c + (f_ - c) * t))


# \u2500\u2500 locked constants \u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500
COLS = 44                  # expanded bar width
FADE = 0.25                # unselected rows, blended toward BASE
BASE = "#1F1F28"           # kanagawa sumiInk1 — the bar background
RULE_INK = "#DCD7BA"       # kanagawa fujiWhite — the status rule
SEL_BG = "#2D4F67"         # kanagawa waveBlue2 — the selected row
CHIP_INK = "#16161D"       # kanagawa sumiInk0 — text ON a title chip
LCAP, RCAP = "\ue0b6", "\ue0b4"   # powerline half-circle thick (U+E0B6/E0B4)
ELLIPSIS = "\u2026"
RULE = "\u2502"                    # box drawings light vertical

TITLE_W, REPO_W = 7, 7

# The repo palette: 8 kanagawa hues, allocated round-robin and keyed by repo
# root, so one repo is one colour everywhere, forever. Title chips draw from
# the SAME palette but are keyed per-title WITHIN a repo, so two tabs of the
# same repo never share a chip. See the prose spec §5.
PALETTE = [("#7E9CD8", "crystalBlue"), ("#98BB6C", "springGreen"),
           ("#E6C384", "carpYellow"), ("#E46876", "waveRed"),
           ("#957FB8", "oniViolet"), ("#7AA89F", "waveAqua2"),
           ("#FFA066", "surimiOrange"), ("#D27E99", "sakuraPink")]

# Cell 1 of the gutter. The COLOUR is the status; the shape never varies
# except for the two terminal states.
STATUS = {"working": ("\u25cf", "#FF9E3B"), "done": ("\u25cf", "#98BB6C"),
          "unread": ("\u25cf", "#E46876"), "idle": ("\u25cf", "#54546D"),
          "dormant": ("\u25cc", "#54546D"), "stale": ("\u2717", "#E82424")}

# Cell 3 of the gutter — the context battery (S7). Colour is the magnitude
# ramp, green through red. A plain terminal tab shows the console mark here
# instead, because a terminal has no context window.
BATTERY = [("\U000f0079", "#98BB6C"), ("\U000f007e", "#98BB6C"),
           ("\U000f007c", "#E6C384"), ("\U000f007b", "#FFA066"),
           ("\U000f007a", "#E46876")]
CONSOLE = "\U000f018d"                 # nf-md-console

# Cell 4 — provenance. Tinted with the REPO ink, so it matches the repo name.
# A main checkout deliberately shows NOTHING: no terminal tool in the survey
# marks the default branch with a glyph, and blanking it makes the two marked
# states actually mean something.
PROVENANCE = {"worktree": "\U000168c2",   # bamum tree
              "branch": "\U000f062c",     # nf-md-source_branch (lazygit's)
              "main": " "}


def cells(s):
    """Terminal CELLS occupied by `s`, not code points.

    `len()` counts scalars: a wide (East-Asian W/F) glyph occupies two columns
    and a combining mark none, so code-point arithmetic silently misaligns
    every row to its right. This is the same hazard the dossier records against
    the renderer's `str::chars()` clamp. Rust must use `unicode-width` — the
    exact version zellij lays its grid with — not `chars().count()`.
    """
    n = 0
    for ch in s:
        if unicodedata.combining(ch):
            continue
        n += 2 if unicodedata.east_asian_width(ch) in ("W", "F") else 1
    return n


def clamp(s, w):
    """A fixed-width column, measured in CELLS: truncate when long, pad when
    short. Padding is what makes fields line up vertically down the bar."""
    if w <= 0:
        return ""
    if cells(s) <= w:
        return s + " " * (w - cells(s))
    out = ""
    for ch in s:
        if cells(out) + cells(ch) > w - 1:
            break
        out += ch
    return out + ELLIPSIS + " " * (w - cells(out) - 1)


def render(row, selected=False):
    """One row, exactly COLS columns wide.

    The gutter is POSITION-LOCKED: every cell is one column and renders a
    space when its glyph is absent, so a missing glyph can never shift the
    text. The cap columns are reserved on EVERY row for the same reason —
    the selected row must not sit one column right of its neighbours.
    """
    fade = 0.0 if selected else FADE
    o = bg(SEL_BG) if selected else ""

    def ink(h):
        return fg(mix(h, BASE, fade))

    is_term = row.get("terminal")
    repo_ink = row.get("repo_ink", "#54546D")

    out = (fg(SEL_BG) + LCAP + o) if selected else " "          # col 1
    if is_term:
        out += o + " "                                          # col 2
        out += o + " " + ink(RULE_INK) + RULE + o + " "         # cols 3-5
        out += o + ink("#54546D") + CONSOLE + o                 # col 6
        out += o + "  "                                         # cols 7-8
    else:
        sg, si = STATUS[row["status"]]
        bg_, bi = BATTERY[row["battery"]]
        out += o + ink(si) + sg + o                             # col 2
        out += o + " " + ink(RULE_INK) + RULE + o + " "         # cols 3-5
        out += o + ink(bi) + bg_ + o                            # col 6
        out += o + " "                                          # col 7
        pv = PROVENANCE[row["provenance"]]
        out += o + (ink(repo_ink) + pv + o if pv.strip() else " ")   # col 8
    out += o + " "                                              # col 9

    body = COLS - 9 - 1 - 1          # minus gutter, right margin, right cap
    summary_w = body - TITLE_W - REPO_W - 2
    if is_term:
        out += o + ink("#717C7C") + clamp(row["name"], body) + o
    else:
        title = row.get("title")
        if title:
            out += bg(mix(row["title_ink"], BASE, fade)) + fg(CHIP_INK) \
                + clamp(title, TITLE_W) + R + o
        else:
            out += o + " " * TITLE_W
        out += o + " "
        out += ink(repo_ink) + clamp(row["repo"], REPO_W) + o
        out += o + " "
        out += (ink("#DCD7BA") if fade else "") \
            + clamp(row["summary"], summary_w) + o
    out += o + " " + R                                          # right margin
    out += (fg(SEL_BG) + RCAP + R) if selected else " "         # col 44
    return out


# \u2500\u2500 a sample fleet \u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500
INK = {"clave": PALETTE[0][0], "dotfiles": PALETTE[1][0],
       "api-svc": PALETTE[2][0], "infra": PALETTE[4][0], "webapp": PALETTE[5][0]}

FLEET = [
    dict(status="unread", battery=1, provenance="main", repo="dotfiles",
         repo_ink=INK["dotfiles"], title=None,
         summary="I just passed the spec over"),
    dict(status="working", battery=3, provenance="worktree", repo="clave",
         repo_ink=INK["clave"], title="S6-GUT", title_ink=PALETTE[5][0],
         summary="picking the gutter set", selected=True),
    dict(status="working", battery=2, provenance="branch", repo="api-svc",
         repo_ink=INK["api-svc"], title="API-GW", title_ink=PALETTE[3][0],
         summary="retry budget audit"),
    dict(status="done", battery=0, provenance="main", repo="clave",
         repo_ink=INK["clave"], title="UX-DOC", title_ink=PALETTE[6][0],
         summary="there was a stale anchor"),
    dict(terminal=True, name="Tab #16"),
    dict(status="stale", battery=4, provenance="worktree", repo="clave",
         repo_ink=INK["clave"], title="KDL-GRD", title_ink=PALETTE[7][0],
         summary="the real parser test"),
    dict(status="dormant", battery=0, provenance="worktree", repo="clave",
         repo_ink=INK["clave"], title=None, summary="worktree doctor work"),
    dict(status="dormant", battery=1, provenance="branch", repo="infra",
         repo_ink=INK["infra"], title="CFG", title_ink=PALETTE[1][0],
         summary="config plumbing"),
    dict(status="idle", battery=2, provenance="branch", repo="webapp",
         repo_ink=INK["webapp"], title="API-V2", title_ink=PALETTE[2][0],
         summary="cutover plan review"),
]

DIMTXT = fg("#717C7C")


def bar(rows, label):
    ruler = "".join(str((i + 1) % 10) for i in range(COLS))
    print(f"\n  {BOLD}{label}{R}")
    print(f"  {DIMTXT}{ruler}\n  \u250c{"\u2500" * COLS}\u2510{R}")
    for r in rows:
        line = render(r, r.get("selected", False))
        # The lock doc CLAIMS every row is exactly COLS cells. Prove it rather
        # than asserting it in prose: strip the SGR sequences and measure the
        # remainder in display cells. A miscounted glyph fails the preview
        # loudly instead of shipping a ragged bar.
        width = cells(re.sub(r"\x1b\[[0-9;]*m", "", line))
        assert width == COLS, f"row is {width} cells, expected {COLS}: {r!r}"
        print(f"  {DIMTXT}\u2502{R}{line}{DIMTXT}\u2502{R}")
    print(f"  {DIMTXT}\u2514{"\u2500" * COLS}\u2518{R}")


if __name__ == "__main__":
    print(f"\n{BOLD}{"\u2550" * 78}\nclave sidebar — locked visual design "
          f"(2026-07-25)\n{"\u2550" * 78}{R}")
    bar(FLEET, f"expanded — {COLS} columns")

    print(f"""
  {DIMTXT}COLUMN MAP
     1      left cap   — powerline half-circle, selected row only
     2      status     — colour IS the state
     3      space
     4      rule       — U+2502 in fujiWhite, separates status from battery
     5      space
     6      battery    — context level (S7); console mark on a terminal tab
     7      space
     8      provenance — tinted with the repo ink; BLANK for a main checkout
     9      space
    10-16   title      — filled chip, dark text; blank when never renamed
    17      space
    18-24   repo       — tinted text, one colour per repo forever
    25      space
    26-42   summary
    43      right margin
    44      right cap  — selected row only

  Cap columns are reserved on EVERY row so the selected row does not shift
  one column right of its neighbours. Verified: title starts at column 10
  whether or not the row is selected.{R}""")

    print(f"\n  {BOLD}palette — 8 kanagawa hues, round-robin{R}\n")
    for i, (h, n) in enumerate(PALETTE):
        print(f"   {DIMTXT}{i}{R} {fg(h)}\u2588\u2588\u2588\u2588{R}  "
              f"{fg(h)}repo-name{R}   {bg(h)}{fg(CHIP_INK)} TITLE {R}   "
              f"{DIMTXT}{h}  {n}{R}")
    print()
