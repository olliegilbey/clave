//! One home for every visual variable (#145) — Ollie's framing: "almost like
//! tailwind for CSS". Every colour, glyph and visual constant the bar renders
//! is DEFINED in this file; `render.rs` holds the arithmetic that arranges
//! them, never a value of its own.
//!
//! Two kinds of colour live here, and the split is ratified (#145, grill
//! 2026-08-24):
//!
//! - **Fixed semantic hues** — the status marks and the battery risk bands. A
//!   red status means failed under EVERY theme, so these are consts and never
//!   follow the user's theme.
//! - **[`Theme`]** — what DOES follow the user's zellij theme: the backgrounds,
//!   the default ink (rule, summary, dormant glyph), the chip text, the
//!   untinted grey and the eight repo inks. `Theme::default()` is the curated
//!   kanagawa design the lock ratified, byte-identical to the pre-#145
//!   constants, so every golden and the README assets pin the default;
//!   [`Theme::from_styling`] maps a live zellij theme onto the same fields.
//!
//! zellij-tile DATA types only (`Styling` et al. are re-exported
//! `zellij-utils::data` — pure serde structs). The lib's no-zellij-tile rule
//! exists because the SHIMS have no host symbols (see `lib.rs`); data types
//! link and test on the host, which is exactly why the mapping lives here and
//! not in the untestable bin.
//!
//! GLYPH RULE, load-bearing (lock §5.4): every non-ASCII glyph below is a
//! `\u{...}` escape, never a literal. Literal glyphs were silently lost in
//! transit twice and the loss misdiagnosed as missing font coverage.

use zellij_tile::prelude::{DEFAULT_STYLES, PaletteColor, Styling};

// ── the colour type ─────────────────────────────────────────────────────────

/// 24-bit truecolor, not ANSI-16 (LEDGER D8): the kanagawa palette has no
/// ANSI-16 equivalent, and lock §4.1 grants the provenance cell an arbitrary
/// RGB on purpose. `Status::glyph()` in clave-types keeps its `u8` ANSI
/// contract for the host CLI — the bar owns its own palette (D10). Indexed
/// theme colours are CONVERTED to RGB at the [`Theme::from_styling`] boundary,
/// so everything past it (the `mix` fades especially) stays truecolor.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Rgb(pub u8, pub u8, pub u8);

impl Rgb {
    pub fn fg(self) -> String {
        format!("\u{1b}[38;2;{};{};{}m", self.0, self.1, self.2)
    }

    pub fn bg(self) -> String {
        format!("\u{1b}[48;2;{};{};{}m", self.0, self.1, self.2)
    }

    pub fn hex(self) -> String {
        format!("#{:02X}{:02X}{:02X}", self.0, self.1, self.2)
    }

    /// Blend toward `other` by `t`. Ties round to EVEN — not away from zero —
    /// because the ratified preview's captured output was produced by Python's
    /// `round()`, which is round-half-to-even. One channel landing on `.5` is
    /// enough to make the port off-by-one against the design that was signed
    /// off, and `#DCD7BA` faded onto `#1F1F28` puts blue on exactly `149.5`.
    pub fn mix(self, other: Rgb, t: f64) -> Rgb {
        let ch = |a: u8, b: u8| {
            (f64::from(a) + (f64::from(b) - f64::from(a)) * t).round_ties_even() as u8
        };
        Rgb(
            ch(self.0, other.0),
            ch(self.1, other.1),
            ch(self.2, other.2),
        )
    }
}

pub const RESET: &str = "\u{1b}[0m";

// ── the curated kanagawa defaults (the lock's design) ───────────────────────

/// Pub for the `readme-assets` example: the README's generated frame paints
/// this canvas rather than copying the hex.
pub const BASE: Rgb = Rgb(0x1F, 0x1F, 0x28); // sumiInk3 — the bar background
pub const SEL_BG: Rgb = Rgb(0x2D, 0x4F, 0x67); // waveBlue2 — the selected row
/// fujiWhite — kanagawa's default foreground. Inks the rule, the summary, and
/// the dormant glyph, which was sumiInk4 and all but invisible against `BASE`.
/// Pub for the `readme-assets` example, like `BASE` above.
pub const DEFAULT_INK: Rgb = Rgb(0xDC, 0xD7, 0xBA);
/// sumiInk0 — text ON a title chip. Public because the preview draws the same
/// chip in its palette swatches (lock §4).
pub const CHIP_INK: Rgb = Rgb(0x16, 0x16, 0x1D);

/// The ink a row falls back to when it has no palette entry yet. Reachable:
/// allocation is store-backed iterate-and-wrap (lock §4) and a row can render
/// before its colour exists.
pub const UNTINTED: Rgb = Rgb(0x54, 0x54, 0x6D); // sumiInk4

/// Eight kanagawa hues, allocated round-robin and keyed by repo root, so one
/// repo is one colour everywhere forever (lock §4). Twelve was rendered first
/// and rejected: they start colliding after the fifth. Hashing is overruled
/// twice over — `DefaultHasher` is not stable across toolchains, and the
/// maintainer rejected collisions outright.
///
/// The NAMES belong to the curated default only — a themed palette
/// ([`Theme::palette`]) has hues with no names, which is why `Theme` carries
/// `[Rgb; PALETTE_LEN]` and this table keeps the labels for the preview.
pub const PALETTE: [(Rgb, &str); PALETTE_LEN] = [
    (Rgb(0x7E, 0x9C, 0xD8), "crystalBlue"),
    (Rgb(0x98, 0xBB, 0x6C), "springGreen"),
    (Rgb(0xE6, 0xC3, 0x84), "carpYellow"),
    (Rgb(0xE4, 0x68, 0x76), "waveRed"),
    (Rgb(0x95, 0x7F, 0xB8), "oniViolet"),
    (Rgb(0x7A, 0xA8, 0x9F), "waveAqua2"),
    (Rgb(0xFF, 0xA0, 0x66), "surimiOrange"),
    (Rgb(0xD2, 0x7E, 0x99), "sakuraPink"),
];

/// The repo-ink table's FIXED length, under any theme. The persisted ink
/// indices (`model.rs` allocation, store-backed) and every `i % len` wrap are
/// written against it; a theme that yields fewer distinct hues pads from the
/// curated palette rather than shrinking the table.
pub const PALETTE_LEN: usize = 8;

// ── fixed semantic hues (never themed) ──────────────────────────────────────

/// The status-mark inks (LEDGER D10's table). Fixed under every theme (#145):
/// the COLOUR is the state, and a red that meant "failed" only under kanagawa
/// would be a legend the reader has to relearn per theme.
pub const NEEDS_YOU_INK: Rgb = Rgb(0xE4, 0x68, 0x76); // waveRed
pub const WORKING_INK: Rgb = Rgb(0xFF, 0x9E, 0x3B); // roninYellow
pub const DONE_INK: Rgb = Rgb(0x98, 0xBB, 0x6C); // springGreen
pub const FAILED_INK: Rgb = Rgb(0xE8, 0x24, 0x24); // samuraiRed — also Stale
pub const OPENING_INK: Rgb = Rgb(0xE6, 0xC3, 0x84); // carpYellow — also DormantSelected

// These four are byte-identical to `PALETTE` entries 1, 2, 3 and 6
// (springGreen, carpYellow, waveRed, surimiOrange), and the duplication is
// deliberate rather than an oversight (#145 resolution): `PALETTE` is the
// REPO ink table — themed, allocated round-robin — while these are RISK bands,
// fixed under every theme. Sharing a constant would re-colour the battery
// whenever the theme changes the repo palette, which is two unrelated meanings
// on one value.
pub const GREEN: Rgb = Rgb(0x98, 0xBB, 0x6C);
pub const YELLOW: Rgb = Rgb(0xE6, 0xC3, 0x84);
pub const ORANGE: Rgb = Rgb(0xFF, 0xA0, 0x66);
pub const RED: Rgb = Rgb(0xE4, 0x68, 0x76);

// ── the card's fixed hues (#232) ────────────────────────────────────────────
//
// The two-line card's own inks, fixed under every theme for the same reason
// the status marks are: each one is a MEANING (a brand, a pull request, the
// card's linework) rather than a role the user's theme has an opinion about.
// Ratified with the card itself — `examples/double-preview.rs` is the picture
// these values produce.

/// The bracket's alternating pair — two quiet kanagawa inks, cool and warm,
/// close enough in weight that neither reads as a state. The zebra lives in
/// this linework: glass forbids painting every second background, so the
/// alternation is carried by the `\u{256d}`/`\u{2570}` bracket instead.
// `BRACKET_A` and `META_INK` below share this exact value (both fujiGray,
// 0x727169) — a coincidence, not a shared identity. They play unrelated
// roles (bracket linework vs. metadata text) and are declared as separate
// constants on purpose; changing one to retune its role must not silently
// repaint the other's.
pub const BRACKET_A: Rgb = Rgb(0x72, 0x71, 0x69); // fujiGray
pub const BRACKET_B: Rgb = Rgb(0x9C, 0xAB, 0xCA); // springViolet2

/// The card's quiet metadata ink: branch, model, elapsed, and the `TERM` tag.
/// Same RGB as [`BRACKET_A`] (both fujiGray) — coincidence, not aliasing; see
/// the note there.
pub const META_INK: Rgb = Rgb(0x72, 0x71, 0x69); // fujiGray
/// The pull-request number.
pub const PR_INK: Rgb = Rgb(0x98, 0xBB, 0x6C); // springGreen

/// The provider brand cells. BRAND colours, so they are not themed at all —
/// nf-cod-claude in Anthropic coral, nf-cod-openai in OpenAI green. A provider
/// clave does not know renders nothing (`card::provider_mark`).
pub const CLAUDE_GLYPH: char = '\u{ec82}';
pub const CLAUDE_INK: Rgb = Rgb(0xD9, 0x77, 0x57);
pub const OPENAI_GLYPH: char = '\u{ec81}';
pub const OPENAI_INK: Rgb = Rgb(0x10, 0xA3, 0x7F);

/// The card's rounded bracket, top and bottom — the glyph that binds a row's
/// two lines into one card (round 7: the light arc, not the heavy one).
pub const CARD_TOP: char = '\u{256d}';
pub const CARD_BOT: char = '\u{2570}';

// ── fades ───────────────────────────────────────────────────────────────────

/// Unselected rows recede 25% toward the bar background (lock §6). Selection by
/// recession costs zero columns and gets MORE effective as the fleet grows,
/// which is the opposite of a background tint — a tint competes with the title
/// chips and repo inks for the same channel, which is why it read as
/// insufficient. Fades at 8/12/15/20/30/40% were rendered and rejected.
pub const FADE: f64 = 0.25;

/// The dormant fade (#206). Deeper than [`FADE`] and UNCONDITIONAL — a dormant
/// row dims whether or not anything is selected, because it marks what the row
/// IS (no session behind it), not where focus sits. Before this the only
/// dormant tell was the hollow gutter glyph, and the block read as live fleet
/// at a glance. The glyph stays fujiWhite-based (#123's near-invisible
/// sumiInk4 lesson), so even this deep it outreads the old ink. 0.5 was
/// rendered first; Ollie ratified 0.6 against the ux-gate1 fleet (#210).
/// Pub for the README asset generator: the dormant icon must carry the same
/// fade `render_row` applies, not the raw mark ink.
pub const DORMANT_FADE: f64 = 0.6;

// ── glyphs (lock §5) ────────────────────────────────────────────────────────

pub const LCAP: char = '\u{e0b6}'; // powerline half-circle thick, left
pub const RCAP: char = '\u{e0b4}'; // powerline half-circle thick, right
pub const RULE: char = '\u{2502}'; // box drawings light vertical
pub const ELLIPSIS: char = '\u{2026}';
/// Pub for the `readme-assets` example (as are `BATTERY` and the `mark`/`ink`
/// tables in render.rs): the README's icons are generated from these values,
/// never copied.
pub const CONSOLE: char = '\u{f018d}'; // nf-md-console — the STATUS cell's mark (#206)
/// The battery cell's terminal-class marker (#206): the word where an agent
/// row shows its count, a glyph where it shows its ramp. `TERM` is four cells
/// exactly — the full expanded battery cell, right-aligned like the digits.
pub const TERM_MARK: &str = "TERM";
pub const TERM_GLYPH: char = '\u{f120}'; // nf-fa-terminal — the rightward prompt, collapsed

/// The S7 ramp (#62). Index is the context level: `0` is full, the last entry
/// is empty and past the user's smart zone.
///
/// TWO AXES AT DIFFERENT RESOLUTIONS, deliberately. The GLYPH carries magnitude
/// finely — one step per tenth of the zone, so the cell reads as a gauge you can
/// watch descend. The INK carries risk coarsely — four bands, for the glance
/// that never resolves a glyph at all. They cannot contradict each other because
/// both are functions of the same index.
///
/// The ink bands are green below six tenths, yellow to eight, orange to the
/// zone, and red AT it. The zone is where the battery turns red rather than
/// where the ramp ends, so the last entry is also the clamp: four times over
/// reads the same as one token over, and #105's token text carries the
/// magnitude the glyph has stopped resolving.
///
/// Glyphs are the Material Design battery family, verified against the installed
/// patched font's glyph-name table rather than assumed: `md-battery` (U+F0079),
/// `md-battery_90`…`md-battery_10` (U+F0082 down to U+F007A — note they run
/// BACKWARDS through the codepoints), `md-battery_outline` (U+F008E). Written as
/// escapes, never literals: design-lock §5.4, load-bearing.
pub const BATTERY: [(char, Rgb); clave_types::BATTERY_LEVELS as usize] = [
    ('\u{f0079}', GREEN),  // full        · below a tenth spent
    ('\u{f0082}', GREEN),  // nine tenths
    ('\u{f0081}', GREEN),  // eight
    ('\u{f0080}', GREEN),  // seven
    ('\u{f007f}', GREEN),  // six
    ('\u{f007e}', GREEN),  // five        · half the zone gone
    ('\u{f007d}', YELLOW), // four
    ('\u{f007c}', YELLOW), // three
    ('\u{f007b}', ORANGE), // two
    ('\u{f007a}', ORANGE), // one tenth
    ('\u{f008e}', RED),    // empty       · at or past the zone
];

// ── the theme ───────────────────────────────────────────────────────────────

/// The theme-following half of the bar's colour language (#145 half two):
/// everything a user's zellij theme is allowed to repaint. Status marks and
/// battery bands are NOT here — they are the fixed consts above.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Theme {
    /// The bar background — also the target every fade `mix`es toward.
    pub base: Rgb,
    /// The selected row's background and its two powerline caps.
    pub sel_bg: Rgb,
    /// The rule, the summary, the dormant mark, `TERM`, and every "no better
    /// ink" fallback.
    pub default_ink: Rgb,
    /// Dark text ON a title chip; the background OF a terminal row's name chip.
    pub chip_ink: Rgb,
    /// The Idle mark and the absent/out-of-range ink fallback.
    pub untinted: Rgb,
    /// The repo inks (lock §4). Always [`PALETTE_LEN`] long — see the const.
    pub palette: [Rgb; PALETTE_LEN],
}

impl Default for Theme {
    /// The curated kanagawa design, byte-identical to the constants above —
    /// what the goldens pin, what the README assets are generated from, and
    /// what an unthemed zellij falls back to.
    fn default() -> Theme {
        Theme {
            base: BASE,
            sel_bg: SEL_BG,
            default_ink: DEFAULT_INK,
            chip_ink: CHIP_INK,
            untinted: UNTINTED,
            palette: core::array::from_fn(|i| PALETTE[i].0),
        }
    }
}

/// A themed repo-ink candidate must clear this luminance distance from the bar
/// background or it is skipped: zellij themes routinely leave slots at `0`
/// (kanagawa's own `multiplayer_user_colors` has five), and a black repo name
/// on a near-black bar is not an identity, it is a hole. 48/255 is calibrated
/// against kanagawa: it rejects black and would reject sumiInk4 — the hue the
/// design already classes as "untinted, barely visible" — while keeping every
/// hue the curated palette considers legible.
const MIN_INK_LUMA_DIST: f64 = 48.0;

/// Squared RGB distance — the harvest's "how different" measure. Integer and
/// exact, so the farthest-first ordering is bit-identical on every host and
/// in the wasm.
fn dist2(a: Rgb, b: Rgb) -> u32 {
    let d = |x: u8, y: u8| {
        let d = i32::from(x) - i32::from(y);
        (d * d) as u32
    };
    d(a.0, b.0) + d(a.1, b.1) + d(a.2, b.2)
}

/// The closest pair in the curated palette (sakuraPink beside waveRed, 45
/// apart) — the design's own answer to "how close can two repo inks sit and
/// still read as two". A harvested hue nearer than this to any pick is a twin,
/// not an identity. Self-calibrating, like the selection cap in `clamp_sel`.
fn curated_spread() -> u32 {
    PALETTE
        .iter()
        .enumerate()
        .flat_map(|(i, a)| PALETTE[i + 1..].iter().map(move |b| dist2(a.0, b.0)))
        .min()
        .unwrap_or(0)
}

/// Rec. 709 relative luminance, in channel units (0–255).
fn luma(c: Rgb) -> f64 {
    0.2126 * f64::from(c.0) + 0.7152 * f64::from(c.1) + 0.0722 * f64::from(c.2)
}

impl Theme {
    /// Map a live zellij theme (`ModeInfo.style.colors`, via `ModeUpdate`)
    /// onto the bar's colour roles. The mapping is semantic — zellij's theme
    /// format has no palette, only per-component style declarations — so each
    /// role reads the slot whose zellij meaning matches its own:
    ///
    /// - `base`/`default_ink` ← `text_unselected` background/base — the theme's
    ///   ordinary text on its ordinary ground.
    /// - `sel_bg` ← `list_selected.background` when it is a quiet surface
    ///   tint; when it is loud, a tint of `text_unselected.emphasis_1` at the
    ///   curated selection weight instead — [`clamp_sel`] carries the full
    ///   reasoning (drive finding, 2026-08-27: kanagawa's selection green).
    /// - `chip_ink` ← `base` darkened 30% toward black: "darker than the bar",
    ///   which is the relationship sumiInk0 has to sumiInk3 in the curated
    ///   design, preserved as a relationship rather than a colour.
    /// - `untinted` ← `default_ink` mixed 65% toward `base`: legible-but-quiet,
    ///   the relationship sumiInk4 has to the kanagawa pair.
    /// - `palette` ← [`harvest`].
    ///
    /// Stock `DEFAULT_STYLES` means "the user never set a theme" — every such
    /// user has been looking at curated kanagawa since v0.1.0, so honesty about
    /// zellij's stock colours would be a regression nobody asked for; they keep
    /// the curated default. (A deliberate `theme "default"` lands in the same
    /// arm, accepted at the grill.)
    pub fn from_styling(s: &Styling) -> Theme {
        if *s == DEFAULT_STYLES {
            return Theme::default();
        }
        let base = rgb(s.text_unselected.background);
        let default_ink = rgb(s.text_unselected.base);
        Theme {
            base,
            sel_bg: clamp_sel(
                base,
                rgb(s.list_selected.background),
                rgb(s.text_unselected.emphasis_1),
            ),
            default_ink,
            chip_ink: base.mix(Rgb(0, 0, 0), 0.3),
            untinted: default_ink.mix(base, 0.65),
            palette: harvest(s, base, default_ink),
        }
    }
}

/// The themed selection may not shout louder than the curated one. The cap is
/// the curated pair's own luminance distance (waveBlue2 over sumiInk3, ~42) —
/// self-calibrating, not a magic number. A quiet `list_selected.background`
/// (catppuccin, tokyo-night, gruvbox, nord, dracula) passes through untouched.
///
/// A LOUD one is a colour zellij built for dark text (kanagawa's bright green,
/// everforest's) — the theme gave the bar no usable selection surface, and
/// quieting that green would still leave the selected row wearing a status hue
/// (green means Done in the bar's fixed language). So the loud case tints from
/// the theme's `text_unselected.emphasis_1` instead — blue or cyan in five of
/// the seven popular shipped themes, and the theme's own signature hue in the
/// green pair, where a green selection is honest — mixed toward `base` to land
/// exactly on the cap. A degenerate emphasis (luma too close to the bar to ink
/// anything, [`MIN_INK_LUMA_DIST`]) falls back to the loud colour, clamped.
/// Distances are `abs`, so a light theme clamps the same way.
fn clamp_sel(base: Rgb, sel: Rgb, emphasis: Rgb) -> Rgb {
    let cap = luma(SEL_BG) - luma(BASE);
    let dist = (luma(sel) - luma(base)).abs();
    if dist <= cap {
        return sel;
    }
    let e_dist = (luma(emphasis) - luma(base)).abs();
    if e_dist >= MIN_INK_LUMA_DIST {
        base.mix(emphasis, cap / e_dist)
    } else {
        base.mix(sel, cap / dist)
    }
}

/// Eight repo inks from a theme that never heard of repo inks.
///
/// The pool is every candidate slot below, in a fixed order so the same theme
/// always yields the same palette (a repo keeps its colour across restarts,
/// lock §4): `multiplayer_user_colors` leads because it is the one place
/// zellij's format means what the repo palette means — "N mutually
/// distinguishable identity hues" — then the emphasis slots. A candidate is
/// legible when it is not the default ink (a repo painted exactly like the
/// summary text reads untinted) and clears [`MIN_INK_LUMA_DIST`] from the bar
/// background.
///
/// Picks are farthest-first ([`spread_into`]): the first legible hue seeds,
/// then each slot takes the pool hue most distant from every pick so far, so
/// the low slots — the ones a small fleet actually uses — are the starkest
/// pairs the theme has. A hue nearer than [`curated_spread`] to a pick is a
/// twin, not an identity, and never enters (kanagawa's two blues, its two
/// oranges). When the theme runs dry the curated palette pads the rest under
/// the same rule, so a short theme no longer paints one hue twice. An empty
/// harvest (every slot degenerate) is the curated palette verbatim; a harvest
/// still short after the pad (a theme whose hues sit between curated pairs)
/// cycles its tail, the honest last resort.
fn harvest(s: &Styling, base: Rgb, default_ink: Rgb) -> [Rgb; PALETTE_LEN] {
    let m = &s.multiplayer_user_colors;
    let candidates = [
        m.player_1,
        m.player_2,
        m.player_3,
        m.player_4,
        m.player_5,
        m.player_6,
        m.player_7,
        m.player_8,
        m.player_9,
        m.player_10,
        s.text_unselected.emphasis_0,
        s.text_unselected.emphasis_1,
        s.text_unselected.emphasis_2,
        s.text_unselected.emphasis_3,
        s.ribbon_unselected.emphasis_0,
        s.ribbon_unselected.emphasis_1,
        s.ribbon_unselected.emphasis_2,
        s.ribbon_unselected.emphasis_3,
        s.ribbon_selected.emphasis_0,
        s.ribbon_selected.emphasis_1,
        s.ribbon_selected.emphasis_2,
        s.ribbon_selected.emphasis_3,
        s.frame_highlight.base,
        s.exit_code_success.base,
        s.exit_code_error.base,
    ];
    let legible: Vec<Rgb> = candidates
        .into_iter()
        .map(rgb)
        .filter(|&c| c != default_ink && (luma(c) - luma(base)).abs() >= MIN_INK_LUMA_DIST)
        .collect();
    if legible.is_empty() {
        return Theme::default().palette;
    }
    let spread = curated_spread();
    let mut picked: Vec<Rgb> = Vec::with_capacity(PALETTE_LEN);
    spread_into(&mut picked, &legible, spread);
    let curated: Vec<Rgb> = PALETTE.iter().map(|p| p.0).collect();
    spread_into(&mut picked, &curated, spread);
    core::array::from_fn(|i| picked[i % picked.len()])
}

/// Farthest-first selection: append pool hues to `picked`, each time the one
/// whose nearest pick is farthest away, until the palette is full or the best
/// hue left is nearer than `spread` to a pick (everything after it is nearer
/// still). An empty `picked` takes the pool's first hue. Ties keep pool order,
/// so the pick is deterministic on every host and in the wasm.
fn spread_into(picked: &mut Vec<Rgb>, pool: &[Rgb], spread: u32) {
    while picked.len() < PALETTE_LEN {
        let mut best: Option<(u32, Rgb)> = None;
        for &c in pool {
            let nearest = picked
                .iter()
                .map(|&p| dist2(c, p))
                .min()
                .unwrap_or(u32::MAX);
            if best.is_none_or(|(d, _)| nearest > d) {
                best = Some((nearest, c));
            }
        }
        match best {
            Some((d, c)) if d >= spread => picked.push(c),
            _ => return,
        }
    }
}

/// `PaletteColor` → truecolor. Indexed values go through the xterm 256 table
/// so everything downstream (the fades especially — you cannot `mix` an index)
/// stays 24-bit, upholding LEDGER D8 for any theme. zellij's own conversion
/// (`zellij-utils::shared::eightbit_to_rgb`, via the ansi_colours crate) uses
/// the same cube and grayscale arithmetic; the 0–15 rows are xterm's defaults,
/// which is as canonical as those sixteen get — the terminal remaps them
/// anyway, and a theme that styles UI components with raw 0–15 indices has
/// already accepted approximation.
fn rgb(p: PaletteColor) -> Rgb {
    match p {
        PaletteColor::Rgb((r, g, b)) => Rgb(r, g, b),
        PaletteColor::EightBit(i) => eightbit(i),
    }
}

fn eightbit(i: u8) -> Rgb {
    const XTERM_16: [Rgb; 16] = [
        Rgb(0, 0, 0),
        Rgb(205, 0, 0),
        Rgb(0, 205, 0),
        Rgb(205, 205, 0),
        Rgb(0, 0, 238),
        Rgb(205, 0, 205),
        Rgb(0, 205, 205),
        Rgb(229, 229, 229),
        Rgb(127, 127, 127),
        Rgb(255, 0, 0),
        Rgb(0, 255, 0),
        Rgb(255, 255, 0),
        Rgb(92, 92, 255),
        Rgb(255, 0, 255),
        Rgb(0, 255, 255),
        Rgb(255, 255, 255),
    ];
    match i {
        0..=15 => XTERM_16[usize::from(i)],
        16..=231 => {
            let i = i - 16;
            let step = |n: u8| if n == 0 { 0 } else { 55 + 40 * n };
            Rgb(step(i / 36), step((i / 6) % 6), step(i % 6))
        }
        232..=255 => {
            let v = 8 + 10 * (i - 232);
            Rgb(v, v, v)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use zellij_tile::prelude::{MultiplayerColors, StyleDeclaration};

    /// zellij's OWN kanagawa theme (`zellij-utils/assets/themes/kanagawa.kdl`,
    /// transcribed verbatim) — the fixture every mapping expectation below is
    /// read against. Note these are NOT the curated hues: zellij's kanagawa
    /// picked autumnGreen where the lock picked springGreen, and the grill
    /// (2026-08-24) accepted that shift as the price of passthrough.
    fn zellij_kanagawa() -> Styling {
        let d = |base: (u8, u8, u8),
                 background: PaletteColor,
                 e0: (u8, u8, u8),
                 e1: (u8, u8, u8),
                 e2: (u8, u8, u8),
                 e3: (u8, u8, u8)| StyleDeclaration {
            base: PaletteColor::Rgb(base),
            background,
            emphasis_0: PaletteColor::Rgb(e0),
            emphasis_1: PaletteColor::Rgb(e1),
            emphasis_2: PaletteColor::Rgb(e2),
            emphasis_3: PaletteColor::Rgb(e3),
        };
        let text = (220, 215, 186);
        let ink0 = (22, 22, 29);
        let green = (118, 148, 106);
        let zero = PaletteColor::EightBit(0);
        Styling {
            text_unselected: d(
                text,
                PaletteColor::Rgb(ink0),
                (255, 160, 102),
                (127, 180, 202),
                green,
                (149, 127, 184),
            ),
            text_selected: d(
                ink0,
                PaletteColor::Rgb(green),
                (255, 160, 102),
                (127, 180, 202),
                green,
                (149, 127, 184),
            ),
            ribbon_selected: d(
                ink0,
                PaletteColor::Rgb(green),
                (195, 64, 67),
                (255, 160, 102),
                (149, 127, 184),
                (126, 156, 216),
            ),
            ribbon_unselected: d(
                ink0,
                PaletteColor::Rgb(text),
                (195, 64, 67),
                text,
                (126, 156, 216),
                (149, 127, 184),
            ),
            table_title: d(
                green,
                zero,
                (255, 160, 102),
                (127, 180, 202),
                green,
                (149, 127, 184),
            ),
            table_cell_selected: d(
                ink0,
                PaletteColor::Rgb(green),
                (255, 160, 102),
                (127, 180, 202),
                green,
                (149, 127, 184),
            ),
            table_cell_unselected: d(
                text,
                PaletteColor::Rgb(ink0),
                (255, 160, 102),
                (127, 180, 202),
                green,
                (149, 127, 184),
            ),
            list_selected: d(
                ink0,
                PaletteColor::Rgb(green),
                (255, 160, 102),
                (127, 180, 202),
                green,
                (149, 127, 184),
            ),
            list_unselected: d(
                text,
                PaletteColor::Rgb(ink0),
                (255, 160, 102),
                (127, 180, 202),
                green,
                (149, 127, 184),
            ),
            frame_unselected: None,
            frame_selected: d(
                green,
                zero,
                (255, 160, 102),
                (127, 180, 202),
                (149, 127, 184),
                (0, 0, 0),
            ),
            frame_highlight: d(
                (255, 160, 102),
                zero,
                (149, 127, 184),
                (255, 160, 102),
                (255, 160, 102),
                (255, 160, 102),
            ),
            exit_code_success: d(
                green,
                zero,
                (127, 180, 202),
                ink0,
                (149, 127, 184),
                (126, 156, 216),
            ),
            exit_code_error: d(
                (195, 64, 67),
                zero,
                (255, 158, 59),
                (0, 0, 0),
                (0, 0, 0),
                (0, 0, 0),
            ),
            multiplayer_user_colors: MultiplayerColors {
                player_1: PaletteColor::Rgb((149, 127, 184)),
                player_2: PaletteColor::Rgb((126, 156, 216)),
                player_3: zero,
                player_4: PaletteColor::Rgb((255, 158, 59)),
                player_5: PaletteColor::Rgb((127, 180, 202)),
                player_6: zero,
                player_7: PaletteColor::Rgb((195, 64, 67)),
                player_8: zero,
                player_9: zero,
                player_10: zero,
            },
        }
    }

    #[test]
    fn stock_default_styles_keep_the_curated_kanagawa() {
        assert_eq!(Theme::from_styling(&DEFAULT_STYLES), Theme::default());
    }

    #[test]
    fn kanagawa_roles_map_to_their_semantic_slots() {
        let t = Theme::from_styling(&zellij_kanagawa());
        assert_eq!(t.base, Rgb(22, 22, 29), "bar bg = text_unselected bg");
        assert_eq!(t.default_ink, Rgb(220, 215, 186), "ink = text base");
        // zellij's kanagawa paints selections a BRIGHT green (118,148,106)
        // meant for dark text, so `clamp_sel` tints from emphasis_1 —
        // springBlue — at the curated selection weight instead.
        assert_eq!(t.sel_bg, Rgb(52, 67, 78), "selection = springBlue tint");
        assert_eq!(t.chip_ink, Rgb(15, 15, 20), "chip text = base toward black");
    }

    /// The harvest, pinned over the real kanagawa slots. The legible theme
    /// hues (multiplayer's five live hues, then the emphasis top-up; black
    /// slots and the fujiWhite that equals the default ink are out) are taken
    /// farthest-first: oniViolet leads as the theme's first identity hue, then
    /// whichever remaining hue is most distant from everything picked. Two
    /// theme hues never make it — crystalBlue sits 28 from springBlue and
    /// surimiOrange 43 from roninYellow, both inside the curated spread — so
    /// the curated palette pads the last three slots under the same rule.
    /// Eight distinct hues, nothing wraps.
    #[test]
    fn kanagawa_harvest_takes_the_farthest_hues_first_and_pads_from_the_curated_palette() {
        let t = Theme::from_styling(&zellij_kanagawa());
        let expect = [
            Rgb(149, 127, 184),    // player_1 — oniViolet, the seed
            Rgb(255, 158, 59),     // player_4 — roninYellow, 167 from violet
            Rgb(195, 64, 67),      // player_7 — autumnRed, 112 from the nearest pick
            Rgb(118, 148, 106),    // text emphasis_2 — autumnGreen, 87
            Rgb(127, 180, 202),    // player_5 — springBlue, 60
            Rgb(0xE6, 0xC3, 0x84), // pad: carpYellow, 86
            Rgb(0xE4, 0x68, 0x76), // pad: waveRed, 73
            Rgb(0x98, 0xBB, 0x6C), // pad: springGreen, 52
        ];
        assert_eq!(t.palette, expect);
    }

    /// Every pair in a harvested palette clears the curated palette's own
    /// closest pair (sakuraPink beside waveRed) — the bar's definition of
    /// "distinct enough". This is the property the exact pin above serves.
    #[test]
    fn harvested_hues_are_pairwise_at_least_the_curated_spread_apart() {
        let p = Theme::from_styling(&zellij_kanagawa()).palette;
        let spread = curated_spread();
        for (i, a) in p.iter().enumerate() {
            for b in &p[i + 1..] {
                assert!(
                    dist2(*a, *b) >= spread,
                    "{} and {} sit {} apart, under the curated spread {}",
                    a.hex(),
                    b.hex(),
                    dist2(*a, *b),
                    spread
                );
            }
        }
    }

    /// A theme whose one legible hue sits between sakuraPink and waveRed
    /// kills both pads (each is nearer than the spread), so seven hues survive
    /// and the eighth slot cycles back to the first — the honest last resort,
    /// and the only way a harvested palette still repeats a hue.
    #[test]
    fn a_harvest_short_even_after_the_pad_cycles_its_tail() {
        let black = PaletteColor::EightBit(0);
        let one = StyleDeclaration {
            base: black,
            background: black,
            emphasis_0: PaletteColor::Rgb((219, 115, 135)),
            emphasis_1: black,
            emphasis_2: black,
            emphasis_3: black,
        };
        let s = Styling {
            text_unselected: one,
            ..all_black()
        };
        let p = Theme::from_styling(&s).palette;
        assert_eq!(p[0], Rgb(219, 115, 135));
        assert_eq!(p[7], p[0], "the tail cycles");
        let distinct: std::collections::BTreeSet<(u8, u8, u8)> =
            p.iter().map(|c| (c.0, c.1, c.2)).collect();
        assert_eq!(distinct.len(), 7);
        assert!(
            !p.contains(&PALETTE[3].0),
            "waveRed is a twin of the theme hue"
        );
        assert!(
            !p.contains(&PALETTE[7].0),
            "sakuraPink is a twin of the theme hue"
        );
    }

    /// The luma gate is a DISTANCE from the bar background, not a brightness
    /// floor: on a mid-grey bar a slightly lighter grey is a hole, however
    /// bright it is in absolute terms. Nothing legible ⇒ the curated fallback.
    #[test]
    fn the_luma_gate_measures_distance_from_the_bar_not_brightness() {
        let grey = PaletteColor::Rgb((80, 80, 80));
        let near = StyleDeclaration {
            base: grey,
            background: grey,
            emphasis_0: PaletteColor::Rgb((100, 100, 100)),
            emphasis_1: grey,
            emphasis_2: grey,
            emphasis_3: grey,
        };
        let s = Styling {
            text_unselected: near,
            ..all_of(grey)
        };
        assert_eq!(Theme::from_styling(&s).palette, Theme::default().palette);
    }

    /// The selection stops at a full palette even when the pool has more
    /// well-separated hues to give, seeds on the pool's first hue, and takes
    /// the farthest hue next.
    #[test]
    fn spread_into_seeds_first_takes_farthest_next_and_stops_when_full() {
        let pool = [
            Rgb(0, 0, 0),
            Rgb(255, 0, 0),
            Rgb(0, 255, 0),
            Rgb(0, 0, 255),
            Rgb(255, 255, 0),
            Rgb(255, 0, 255),
            Rgb(0, 255, 255),
            Rgb(255, 255, 255),
            Rgb(128, 128, 128),
        ];
        let mut picked = Vec::new();
        spread_into(&mut picked, &pool, curated_spread());
        assert_eq!(picked.len(), PALETTE_LEN);
        assert_eq!(picked[0], Rgb(0, 0, 0));
        assert_eq!(picked[1], Rgb(255, 255, 255));
    }

    /// A theme whose every candidate is degenerate (all slots black on a black
    /// background) must not paint eight holes — the curated palette is the
    /// fallback of last resort.
    #[test]
    fn an_all_degenerate_harvest_falls_back_to_the_curated_palette() {
        assert_eq!(
            Theme::from_styling(&all_black()).palette,
            Theme::default().palette
        );
    }

    /// The all-degenerate theme: black on a black bar in every slot, so no
    /// candidate clears [`MIN_INK_LUMA_DIST`] and the harvest reaches its
    /// fallback of last resort (LEDGER.md, repo inks HARVEST: an empty harvest
    /// is the curated palette, never eight holes).
    fn all_black() -> Styling {
        all_of(PaletteColor::EightBit(0))
    }

    /// One flat colour in every slot, multiplayer included, so every candidate
    /// equals the bar base and fails the luma gate by construction. A test that
    /// then overrides ONE slot has isolated that slot's fate: it is the only
    /// candidate that can enter the harvest at all.
    fn all_of(c: PaletteColor) -> Styling {
        let flat = StyleDeclaration {
            base: c,
            background: c,
            emphasis_0: c,
            emphasis_1: c,
            emphasis_2: c,
            emphasis_3: c,
        };
        Styling {
            text_unselected: flat,
            text_selected: flat,
            ribbon_selected: flat,
            ribbon_unselected: flat,
            table_title: flat,
            table_cell_selected: flat,
            table_cell_unselected: flat,
            list_selected: flat,
            list_unselected: flat,
            frame_unselected: None,
            frame_selected: flat,
            frame_highlight: flat,
            exit_code_success: flat,
            exit_code_error: flat,
            multiplayer_user_colors: MultiplayerColors {
                player_1: c,
                player_2: c,
                player_3: c,
                player_4: c,
                player_5: c,
                player_6: c,
                player_7: c,
                player_8: c,
                player_9: c,
                player_10: c,
            },
        }
    }

    /// The selection clamp's three regimes, pinned. A quiet selection
    /// background (tokyo-night's (56,62,90), the well-behaved majority) passes
    /// through byte-identical. A loud one (kanagawa's bright green) tints from
    /// the emphasis hue — springBlue, giving the muted blue — and a loud one
    /// whose emphasis is degenerate (black) clamps the loud hue itself. Both
    /// loud results sit within one luma unit of the curated pair's own
    /// distance, which is the invariant the clamp exists to hold.
    #[test]
    fn selection_clamp_passes_quiet_themes_and_tames_loud_ones() {
        let blue = Rgb(127, 180, 202); // kanagawa emphasis_1 — springBlue
        let base = Rgb(26, 27, 38);
        let quiet = Rgb(56, 62, 90);
        assert_eq!(clamp_sel(base, quiet, blue), quiet);

        let k_base = Rgb(22, 22, 29);
        let loud = Rgb(118, 148, 106);
        let cap = luma(SEL_BG) - luma(BASE);

        let tinted = clamp_sel(k_base, loud, blue);
        assert_eq!(tinted, Rgb(52, 67, 78));
        assert!((luma(tinted) - luma(k_base) - cap).abs() < 1.0);

        let clamped = clamp_sel(k_base, loud, Rgb(0, 0, 0));
        assert_eq!(clamped, Rgb(57, 67, 57));
        assert!((luma(clamped) - luma(k_base) - cap).abs() < 1.0);
    }

    /// The three regimes of the xterm 256 table, one probe each, plus both
    /// cube endpoints — the conversion zellij's own `eightbit_to_rgb` performs
    /// (ansi_colours), re-derived here so an indexed theme slot can join the
    /// truecolor fades.
    #[test]
    fn eightbit_conversion_matches_the_xterm_table() {
        assert_eq!(eightbit(0), Rgb(0, 0, 0));
        assert_eq!(eightbit(15), Rgb(255, 255, 255));
        assert_eq!(eightbit(16), Rgb(0, 0, 0)); // cube floor
        assert_eq!(eightbit(196), Rgb(255, 0, 0)); // cube: 16 + 36*5
        assert_eq!(eightbit(231), Rgb(255, 255, 255)); // cube ceiling
        assert_eq!(eightbit(232), Rgb(8, 8, 8)); // grayscale floor
        assert_eq!(eightbit(255), Rgb(238, 238, 238)); // grayscale ceiling
    }

    /// The default theme IS the curated constants — the bridge every golden in
    /// render.rs crosses: they pin `Theme::default()` and these equalities are
    /// what make that the same picture the lock ratified.
    #[test]
    fn the_default_theme_is_the_curated_design() {
        let t = Theme::default();
        assert_eq!(t.base, BASE);
        assert_eq!(t.sel_bg, SEL_BG);
        assert_eq!(t.default_ink, DEFAULT_INK);
        assert_eq!(t.chip_ink, CHIP_INK);
        assert_eq!(t.untinted, UNTINTED);
        for (i, (hue, _)) in PALETTE.iter().enumerate() {
            assert_eq!(t.palette[i], *hue);
        }
    }
}
