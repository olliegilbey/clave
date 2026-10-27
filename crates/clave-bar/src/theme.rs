//! One home for every visual variable (#145) — Ollie's framing: "almost like
//! tailwind for CSS". Every colour, glyph and visual constant the bar renders
//! is DEFINED in this file; `render.rs` holds the arithmetic that arranges
//! them, never a value of its own.
//!
//! Two kinds of colour live here, and the split is ratified (#145, grill
//! 2026-08-24):
//!
//! - **Fixed semantic hues** — the status marks and the battery risk bands. A
//!   red status means failed under EVERY theme, so a theme never picks these.
//!   Only the ground picks their shade ([`Inks`]): the consts below are the
//!   dark ground's, and [`Inks::LIGHT`] holds the light ground's.
//! - **[`Theme`]** — what DOES follow the user's zellij theme: the backgrounds,
//!   the default ink (rule, summary, dormant glyph), the chip text, the
//!   terminal green, the untinted grey and the eight repo inks. It also
//!   carries the fixed set for its ground.
//!   `Theme::default()` is the curated kanagawa design the lock ratified,
//!   byte-identical to the pre-#145 constants, so every golden and the README
//!   assets pin the default; [`Theme::from_styling`] maps a live zellij theme
//!   onto the same fields.
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

use zellij_tile::prelude::{
    DEFAULT_STYLES, Palette, PaletteColor, PaletteSource, Styling, ThemeHue,
};

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

/// springGreen — the CURATED default for the tab name on a terminal chip. The
/// black block says "no agent ink claimed this row", which tells the reader
/// only what the row is NOT; green on black says terminal.
///
/// This is a default, not a meaning: the role is themed (see
/// [`Theme::term_ink`]) and takes the user's own success hue, which is green
/// in 31 of the 41 vendored themes and olive, blue or teal in the other ten.
/// The maintainer chose that over a fixed green (2026-09-18, LEDGER D42): a
/// terminal is identified by its chip and its console mark as well as by the
/// hue, so the odd theme where the name is not green loses an accent rather
/// than the meaning — which is NOT true of a red status mark.
const TERM_INK: Rgb = Rgb(0x98, 0xBB, 0x6C);

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

// ── fixed semantic hues, the dark ground's shades ──────────────────────────

/// Provenance's own inks on the four-line card, FIXED and semantic rather than
/// borrowed from the repo: the rail carries the repo's identity there, so the
/// glyph is free to say what KIND of checkout this is, and to say it the same
/// way on every card. Main draws nothing, as ever. Fixed under every theme for
/// the same reason the status inks are — a green that meant "worktree" only
/// under kanagawa would be a legend the reader has to relearn per theme.
///
/// The glyph-and-ink table is docs/superpowers/specs/2026-09-08-triple-height-card-lock.md
/// §4.2, over §4.1's rail.
pub const WORKTREE_INK: Rgb = Rgb(0x98, 0xBB, 0x6C); // springGreen
pub const BRANCH_INK: Rgb = Rgb(0x95, 0x7F, 0xB8); // oniViolet

/// The clock's ink while a turn is RUNNING (kanagawa crystalBlue). It is the
/// card's only live number, and the blue is the signal.
///
/// Once the turn is over the cell DIMS to `META_INK` rather than blanking —
/// the design that shipped, and the reverse of what this comment used to
/// promise. Blanking was right while the clock was a cell of its own; the
/// lock's §4.4 amendment made it one cell at two resolutions, and the same
/// number then means staleness, which is worth keeping on screen. So the
/// contrast that reads as "thinking" is lit-vs-dim, not filled-vs-blank.
pub const TURN_INK: Rgb = Rgb(0x7E, 0x9C, 0xD8);

/// The subagent mark and its ink — a quiet blue-grey, because it says "this
/// row has depth", not "this row is hot". A BOOLEAN on the card, not a count:
/// "this row has fanned out" is the whole signal and a digit beside it was
/// noise.
///
/// `md-robot_happy_outline`, verified present in the installed Nerd Font by
/// reading its character map rather than a cheat sheet. The codicon set has no
/// `cod-robot` at all, and `fa-robot` is not at the codepoint published for it;
/// both candidates would have shipped as tofu.
///
/// The boolean is docs/superpowers/specs/2026-09-08-triple-height-card-lock.md
/// §4.6; the two tofu candidates are its §6, and FOOTGUNS.md § Text, glyphs,
/// rendering.
pub const SUBS_INK: Rgb = Rgb(0x9C, 0xAB, 0xCA);
pub const SUBS_MARK: char = '\u{f171a}';

/// The status-mark inks (LEDGER D10's table), on the dark ground. The MEANING
/// is fixed under every theme (#145): the COLOUR is the state, and a red that
/// meant "failed" only under kanagawa would be a legend the reader has to
/// relearn per theme. A light ground takes a darker shade of each
/// ([`Inks::LIGHT`]).
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
/// clave does not know renders nothing (`card::provider_mark`). Not shaded per
/// ground either: a brand's colour is the brand's. On the light reference
/// ground (kanagawa lotus) they measure contrast 2.6 (coral) and 2.7
/// (green), under the 3.0 a mark wants; the glyph's shape names the provider
/// as well (light-mode renders, 2026-10-20).
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

/// The two fades on a light ground, which only a light zellij theme gives. A
/// mix toward a light base costs more contrast than the same mix toward a
/// dark one: on the light reference ground (kanagawa lotus), text faded 25%
/// read 3.56 where wave text reads 6.92, and dormant text 1.84 against 2.97.
/// So the light ground fades less, until its text reads as the dark ground's
/// does at each depth (`light_rows_recede_as_far_as_the_dark`). The
/// maintainer found the rendered variants hard to tell apart and kept these
/// (2026-10-27, LEDGER D42).
const LIGHT_FADE: f64 = 0.17;
const LIGHT_DORMANT_FADE: f64 = 0.47;

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
pub const BATTERY: [(char, Band); clave_types::BATTERY_LEVELS as usize] = [
    ('\u{f0079}', Band::Green),  // full        · below a tenth spent
    ('\u{f0082}', Band::Green),  // nine tenths
    ('\u{f0081}', Band::Green),  // eight
    ('\u{f0080}', Band::Green),  // seven
    ('\u{f007f}', Band::Green),  // six
    ('\u{f007e}', Band::Green),  // five        · half the zone gone
    ('\u{f007d}', Band::Yellow), // four
    ('\u{f007c}', Band::Yellow), // three
    ('\u{f007b}', Band::Orange), // two
    ('\u{f007a}', Band::Orange), // one tenth
    ('\u{f008e}', Band::Red),    // empty       · at or past the zone
];

/// The battery's four risk bands (#62). The table above names a band per
/// level; the ground picks the ink for it ([`Inks::band`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Band {
    Green,
    Yellow,
    Orange,
    Red,
}

/// Line 4's hairline ink on a dark ground — a rule so dark it reads as a
/// shadow under the card rather than as a border between two (card.rs
/// `SHADOW`).
pub const SHADOW_INK: Rgb = Rgb(0x2A, 0x2A, 0x37); // sumiInk4

/// The fixed semantic inks for one ground. The MEANINGS are fixed under every
/// theme (#145): red means failed, green means done, whatever the user's
/// theme says. The SHADE is not: the dark set is the curated kanagawa design
/// and is unreadable on a light ground, where yellow and orange marks vanish
/// and the shadow rule reads as a black border (light-mode renders,
/// 2026-10-20). So each meaning has one ink per ground, and [`Theme`] carries
/// the set for the ground it paints on.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Inks {
    pub needs_you: Rgb,
    pub working: Rgb,
    pub done: Rgb,
    /// Also `Stale`.
    pub failed: Rgb,
    /// Also `DormantSelected`.
    pub opening: Rgb,
    pub green: Rgb,
    pub yellow: Rgb,
    pub orange: Rgb,
    pub red: Rgb,
    pub worktree: Rgb,
    pub branch: Rgb,
    pub turn: Rgb,
    pub subs: Rgb,
    pub bracket_a: Rgb,
    pub bracket_b: Rgb,
    pub meta: Rgb,
    pub pr: Rgb,
    pub shadow: Rgb,
}

impl Inks {
    /// The curated kanagawa (wave) inks — the consts above, byte for byte.
    pub const DARK: Inks = Inks {
        needs_you: NEEDS_YOU_INK,
        working: WORKING_INK,
        done: DONE_INK,
        failed: FAILED_INK,
        opening: OPENING_INK,
        green: GREEN,
        yellow: YELLOW,
        orange: ORANGE,
        red: RED,
        worktree: WORKTREE_INK,
        branch: BRANCH_INK,
        turn: TURN_INK,
        subs: SUBS_INK,
        bracket_a: BRACKET_A,
        bracket_b: BRACKET_B,
        meta: META_INK,
        pr: PR_INK,
        shadow: SHADOW_INK,
    };

    /// The same meanings on a light ground. Each ink starts from kanagawa's
    /// OWN lotus counterpart for the role its wave ink plays (`kanagawa.nvim`
    /// `lua/kanagawa/themes.lua`, wave vs lotus): waveRed is `syn.preproc`,
    /// whose lotus value is lotusRed; roninYellow is `diag.warning`,
    /// lotusOrange2; and so on down the table. Taking the pairing from the
    /// design's own author keeps one legend across the two grounds.
    ///
    /// Raw lotus is too pale for a bar read at a glance: lotusOrange2 is
    /// contrast 2.2 on the lotus ground, where roninYellow is 7.9 on wave. So
    /// each ink is then darkened (every channel scaled toward black, which
    /// keeps the hue) until it reads on the lotus ground (`Theme::lotus`, the
    /// tests' light reference) as well as its wave twin reads on the curated
    /// design. Three readings count, and
    /// `light_inks_read_as_well_as_the_dark` pins each one:
    ///
    /// - on the ground, to WCAG AA's 4.5 at most;
    /// - on the selected row, to 3.0 at most;
    /// - FADED, as most rows are (each ground's own [`Theme::row_fade`]): to
    ///   4.0 at most. A cap at AA would turn amber and green to brown. The
    ///   cap was the faded lotus text (3.56) until the light ground got its
    ///   own fade; that text now reads 7.0, and every mark reads 4.0 or more.
    ///
    /// The faded reading is the one that sets the shade. Tuned to the first
    /// two alone, every faded mark read near 2.9, where its wave twin reads
    /// 4.8 to 6 (review finding, PR #291).
    ///
    /// Two picks are not lotus's own (light-mode renders, 2026-10-20): the
    /// yellow band and `opening` take lotusYellow3, because lotus's carpYellow
    /// counterpart is an olive that sits beside the green band; and the
    /// orange band leans toward red, because darkening alone put it nearer
    /// the yellow band than the curated palette's closest pair
    /// (`the_battery_bands_stay_apart_on_both_grounds`).
    pub const LIGHT: Inks = Inks {
        needs_you: Rgb(0xA6, 0x36, 0x45), // lotusRed      ← waveRed
        working: Rgb(0x7F, 0x4B, 0x00),   // lotusOrange2  ← roninYellow
        done: Rgb(0x4A, 0x5B, 0x34),      // lotusGreen    ← springGreen
        failed: Rgb(0xE8, 0x24, 0x24),    // lotusRed3     ← samuraiRed, as is
        opening: Rgb(0x74, 0x4F, 0x00),   // lotusYellow3  ← carpYellow
        green: Rgb(0x4A, 0x5B, 0x34),     // lotusGreen
        yellow: Rgb(0x74, 0x4F, 0x00),    // lotusYellow3
        orange: Rgb(0x9D, 0x3B, 0x00),    // lotusOrange, toward red
        red: Rgb(0xA6, 0x36, 0x45),       // lotusRed
        worktree: Rgb(0x4A, 0x5B, 0x34),  // lotusGreen    ← springGreen
        branch: Rgb(0x62, 0x47, 0x85),    // lotusViolet4  ← oniViolet
        turn: Rgb(0x32, 0x58, 0x85),      // lotusBlue4    ← crystalBlue
        subs: Rgb(0x33, 0x5B, 0x6A),      // lotusTeal1    ← springViolet2
        bracket_a: Rgb(0x7C, 0x7B, 0x73), // lotusGray3    ← fujiGray
        bracket_b: Rgb(0x33, 0x5B, 0x6A), // lotusTeal1    ← springViolet2
        meta: Rgb(0x7C, 0x7B, 0x73),      // lotusGray3    ← fujiGray
        pr: Rgb(0x4A, 0x5B, 0x34),        // lotusGreen    ← springGreen
        shadow: Rgb(0xE7, 0xDB, 0xA0),    // lotusWhite4   ← sumiInk4, as is
    };

    pub fn band(&self, band: Band) -> Rgb {
        match band {
            Band::Green => self.green,
            Band::Yellow => self.yellow,
            Band::Orange => self.orange,
            Band::Red => self.red,
        }
    }
}

// ── the theme ───────────────────────────────────────────────────────────────

/// The theme-following half of the bar's colour language (#145 half two):
/// everything a user's zellij theme is allowed to repaint. Status marks and
/// battery bands are not the theme's to pick: `inks` carries the fixed set
/// for the theme's ground.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Theme {
    /// The bar background — also the target every fade `mix`es toward.
    pub base: Rgb,
    /// The selected row's background and its two powerline caps.
    pub sel_bg: Rgb,
    /// The rule, the summary, the dormant mark, `TERM`, and every "no better
    /// ink" fallback.
    pub default_ink: Rgb,
    /// The text ON a title chip; the background OF a terminal row's name
    /// chip ([`chip_ink`]).
    pub chip_ink: Rgb,
    /// The tab name ON a terminal row's name chip — the theme's own green.
    pub term_ink: Rgb,
    /// The Idle mark and the absent/out-of-range ink fallback.
    pub untinted: Rgb,
    /// The repo inks (lock §4). Always [`PALETTE_LEN`] long — see the const.
    pub palette: [Rgb; PALETTE_LEN],
    /// The fixed semantic inks for this theme's ground ([`Inks`]).
    pub inks: Inks,
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
            term_ink: TERM_INK,
            untinted: UNTINTED,
            palette: core::array::from_fn(|i| PALETTE[i].0),
            inks: Inks::DARK,
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
    /// - `chip_ink` ← [`chip_ink`]: `base` pushed away from the hue chips.
    /// - `term_ink` ← `exit_code_success.base`, through [`legible_on`].
    ///   "Success" is the slot a theme is most likely to paint green, and no
    ///   other slot beats it. Measured over the 41 themes vendored with zellij
    ///   0.44.3 (`zellij-utils-0.44.3/assets/themes/*.kdl`), counting hue
    ///   70–175° at saturation 0.12 or more as green: 31 of 41 are green. The
    ///   other ten are six olive-yellows (gruvbox ×2, solarized ×2, atelier,
    ///   everforest-light), three blues (ao, iceberg ×2) and one teal
    ///   (terafox). Those ten still wear the colour that MEANS success in
    ///   their own theme, which is the honest reading of a themed role — and
    ///   it is the trade the maintainer chose over a fixed green (2026-09-18).
    /// - `untinted` ← `default_ink` mixed 65% toward `base`: legible-but-quiet,
    ///   the relationship sumiInk4 has to the kanagawa pair.
    /// - `palette` ← [`harvest`].
    ///
    /// - `inks` ← [`Inks::LIGHT`] when `base` is a light ground, else
    ///   [`Inks::DARK`]: the meanings stay fixed, the shade follows the ground.
    ///
    /// Stock colours ([`names_no_theme`]) mean "the user never set a theme",
    /// and such a user gets the curated design, not zellij's 256-colour stock
    /// set. A deliberate `theme "default"` lands in the same arm, accepted at
    /// the grill: no built-in theme has that name. The curated design is dark
    /// only. The bar does not read the terminal's light or dark report: Ghostty
    /// reports light for a config with no light and dark theme pair
    /// (ghostty-org/ghostty#13604), and a plugin cannot tell a wrong report
    /// from a right one (ruled 2026-10-27, LEDGER D42). A light bar comes from
    /// a light zellij theme. With both `theme_dark` and `theme_light` set,
    /// zellij swaps the theme on the terminal's report and sends the new one
    /// here (zellij-server-0.45.1 `src/screen.rs:6972-7027`).
    pub fn from_styling(s: &Styling) -> Theme {
        if names_no_theme(s) {
            return Theme::default();
        }
        let base = rgb(s.text_unselected.background);
        let default_ink = rgb(s.text_unselected.base);
        let chip_ink = chip_ink(base);
        Theme {
            base,
            sel_bg: clamp_sel(
                base,
                rgb(s.list_selected.background),
                rgb(s.text_unselected.emphasis_1),
            ),
            default_ink,
            chip_ink,
            term_ink: legible_on(rgb(s.exit_code_success.base), chip_ink).unwrap_or(default_ink),
            untinted: default_ink.mix(base, 0.65),
            palette: harvest(s, base, default_ink),
            inks: if light_ground(base) {
                Inks::LIGHT
            } else {
                Inks::DARK
            },
        }
    }

    /// How far a row's inks recede toward `base`: none when selected or when
    /// nothing is, [`FADE`] for the rest, and [`DORMANT_FADE`] for a dormant
    /// row whatever is selected. A light ground takes its own two depths.
    pub fn row_fade(&self, selected: bool, dormant: bool, any_selected: bool) -> f64 {
        let light = self.inks == Inks::LIGHT;
        match (selected, dormant, any_selected) {
            (true, _, _) => 0.0,
            (false, true, _) if light => LIGHT_DORMANT_FADE,
            (false, true, _) => DORMANT_FADE,
            (false, false, false) => 0.0,
            (false, false, true) if light => LIGHT_FADE,
            (false, false, true) => FADE,
        }
    }
}

#[cfg(test)]
impl Theme {
    /// The tests' light reference ground: kanagawa's own light variant,
    /// lotus, mapped role for role from the wave design the default carries
    /// (`kanagawa.nvim` `lua/kanagawa/themes.lua`). The bar background is
    /// `ui.bg` (lotusWhite3), the text `ui.fg`, and the selection
    /// `ui.bg_search`. [`Inks::LIGHT`], [`LOTUS_PALETTE`] and the light fades
    /// were tuned on it. No bar paints it: with no zellij theme the bar is
    /// dark (LEDGER D42, 2026-10-27), and a light zellij theme brings its own
    /// colours.
    ///
    /// The selection is `ui.bg_search`, not `ui.pmenu.bg_sel`: lotusBlue3 put
    /// the battery's yellow at contrast 1.2 (light-mode renders, 2026-10-20).
    /// The chip text follows [`chip_ink`], as a light theme's does.
    pub(crate) fn lotus() -> Theme {
        let base = Rgb(0xF2, 0xEC, 0xBC); // lotusWhite3 ← sumiInk3
        Theme {
            base,
            sel_bg: Rgb(0xB5, 0xCB, 0xD2), // lotusBlue2   ← waveBlue2
            // lotusInk1 (#545464) read 6.2 to fujiWhite's 11.3, so no fade
            // could match the dark rows; darkened by the `Inks::LIGHT` rule.
            default_ink: Rgb(0x2D, 0x2D, 0x36), // lotusInk1, darkened ← fujiWhite
            chip_ink: chip_ink(base),
            term_ink: Rgb(0x4A, 0x5B, 0x34), // lotusGreen, darkened
            untinted: Rgb(0xA0, 0x9C, 0xAC), // lotusViolet1 ← sumiInk6
            palette: LOTUS_PALETTE,
            inks: Inks::LIGHT,
        }
    }
}

/// True when zellij names no theme. The bar can hold one of two colour sets
/// then, and neither is a theme the user chose:
///
/// - `DEFAULT_STYLES`, a plugin's own start value before any `ModeUpdate`
///   (`Styling::default()`, zellij-utils-0.45.1 `data.rs:1614`). zellij never
///   sends it;
/// - [`no_theme_styles`], which the server puts in every `ModeUpdate` when the
///   config names no theme (zellij-server-0.45.1 `lib.rs:1024-1026` at the
///   first client, `lib.rs:453-454` on a config reload).
///
/// The two differ: the text is colour 255 on colour 16 in the second, and 245
/// on 238 in the first. The bar checked for `DEFAULT_STYLES` only, so a live
/// bar with no theme took zellij's stock set for a theme. It drew zellij's
/// 256-colour greens and oranges (drive 47, 2026-10-27).
fn names_no_theme(s: &Styling) -> bool {
    *s == DEFAULT_STYLES || *s == no_theme_styles()
}

/// zellij's colours for a config with no theme: `default_palette()`
/// (zellij-utils-0.45.1 `shared.rs:114-136`, colour numbers at
/// `shared.rs:85-103`) through `Styling::from`. A plugin cannot call it,
/// because zellij-tile exports `data` and not `shared`. The guardrail
/// `a_config_with_no_theme_gets_the_curated_design` (crates/clave) feeds the
/// real one through zellij's own config parser, and fails if this copy drifts.
fn no_theme_styles() -> Styling {
    use PaletteColor::EightBit as C;
    Styling::from(Palette {
        source: PaletteSource::Default,
        theme_hue: ThemeHue::Dark,
        fg: C(245),
        bg: C(238),
        black: C(16),
        red: C(124),
        green: C(154),
        yellow: C(226),
        blue: C(45),
        magenta: C(201),
        cyan: C(51),
        white: C(255),
        orange: C(166),
        gray: C(238),
        purple: C(99),
        gold: C(136),
        silver: C(245),
        pink: C(207),
        brown: C(215),
    })
}

/// Whose colours the bar paints for `s`: `curated` when zellij names no
/// theme ([`names_no_theme`]), `zellij` for a theme. The bar logs it on each
/// change, and the drive checks it against the sandbox config (qa/lib.sh
/// `theme_source_checks`): that seam broke with every unit test green
/// (drive 47).
pub fn source(s: &Styling) -> &'static str {
    if names_no_theme(s) {
        "curated"
    } else {
        "zellij"
    }
}

/// The curated repo inks for a light ground: the pad pool for a light themed
/// bar ([`harvest`]). They are the same eight `syn` roles the curated palette
/// takes from wave (`kanagawa.nvim` `lua/kanagawa/themes.lua`), in their lotus
/// shades, darkened by the rule on [`Inks::LIGHT`]. The yellow, red and orange
/// slots take the band shades, for the reason given there. They are the same
/// VALUES, not the same constants, for the reason the dark `GREEN` gives: a
/// repo ink and a risk band are two meanings.
///
/// Darkening pulls hues together, and two repos must not share a colour.
/// Darkened alone, four pairs sat nearer than [`curated_spread`], the rule
/// [`harvest`] holds every themed palette to (blue beside violet was 1286).
/// So the five other slots were searched near their darkened shade (hue
/// within 20 degrees, every rule on [`Inks::LIGHT`] kept) for the set whose
/// closest pair clears it (review finding, PR #291). The green moved
/// furthest, 12 degrees toward yellow, away from the aqua.
/// `the_light_repo_inks_stay_apart` pins the spread.
const LOTUS_PALETTE: [Rgb; PALETTE_LEN] = [
    Rgb(0x32, 0x58, 0x85), // lotusBlue4   ← crystalBlue, 6 degrees toward cyan
    Rgb(0x50, 0x5A, 0x30), // lotusGreen   ← springGreen, 12 degrees toward yellow
    Rgb(0x74, 0x4F, 0x00), // lotusYellow3 ← carpYellow
    Rgb(0xA6, 0x36, 0x45), // lotusRed     ← waveRed
    Rgb(0x62, 0x47, 0x85), // lotusViolet4 ← oniViolet, 2 degrees toward magenta
    Rgb(0x32, 0x5D, 0x55), // lotusAqua    ← waveAqua2
    Rgb(0x9D, 0x3B, 0x00), // lotusOrange  ← surimiOrange
    Rgb(0x7E, 0x44, 0x5A), // lotusPink    ← sakuraPink, 2 degrees toward violet
];

/// The text ON a title chip, and the background OF a terminal chip: `base`
/// pushed AWAY from the hue chips the text sits on. On a dark ground the hues
/// are lighter than the bar, so the text goes 30% toward black — the
/// relationship sumiInk0 has to sumiInk3 in the curated design. On a light
/// ground the hues are darker than the bar, so the text goes 60% toward
/// white. The old rule darkened on every ground and gave mid-grey text on
/// light chips: catppuccin-latte's was `#9A9DA2`, under contrast 2.0 on most
/// of its own chips (light-mode renders, 2026-10-20).
fn chip_ink(base: Rgb) -> Rgb {
    if light_ground(base) {
        base.mix(Rgb(0xFF, 0xFF, 0xFF), 0.6)
    } else {
        base.mix(Rgb(0, 0, 0), 0.3)
    }
}

/// Whether `base` is a light ground, by zellij's own rule: HSP perceived
/// brightness above 127.5 (`detect_theme_hue`, zellij-utils-0.45.1
/// `src/shared.rs:139-154`). zellij reads the same colour,
/// `text_unselected.background`, when it tells a pane whether the theme is
/// light (zellij-server-0.45.1 `src/screen.rs:2938-2945`). So the bar and the
/// panes beside it agree on every theme. Rec. 709 luma, the first rule here,
/// disagreed with zellij on 1.8M of the 16.7M colours (counted 2026-10-21,
/// review finding, PR #291).
///
/// In integers, scaled by 1000 and squared: no colour sits on the midpoint,
/// and over every colour this gives zellij's float answer exactly (both
/// counted by brute force, 2026-10-21).
fn light_ground(base: Rgb) -> bool {
    let sq = |c: u8| u32::from(c) * u32::from(c);
    299 * sq(base.0) + 587 * sq(base.1) + 114 * sq(base.2) > 16_256_250
}

/// WCAG 2.1 contrast ratio, 1.0 (same colour) to 21.0 (black on white). The
/// ORDER of the two colours does not matter, which is the point: a text-on-
/// ground question has no direction, and a signed difference invites a caller
/// to forget the sign. `luma` above is Rec. 709 in channel units and answers a
/// different question — how far apart are two hues on the bar — so the two
/// measures are separate on purpose.
fn contrast(a: Rgb, b: Rgb) -> f64 {
    let lum = |c: Rgb| {
        let ch = |v: u8| {
            let v = f64::from(v) / 255.0;
            if v <= 0.03928 {
                v / 12.92
            } else {
                ((v + 0.055) / 1.055).powf(2.4)
            }
        };
        0.2126 * ch(c.0) + 0.7152 * ch(c.1) + 0.0722 * ch(c.2)
    };
    let (a, b) = (lum(a), lum(b));
    (a.max(b) + 0.05) / (a.min(b) + 0.05)
}

/// WCAG AA for normal-size text. The chip's name IS normal-size text, so the
/// standard's own floor is the floor rather than a number tuned here. Measured
/// over the 41 vendored themes it costs exactly one green: none of the 31
/// green successes lands between 3.0 and 4.5, and the only slot in that band
/// is ao's BLUE success at 4.08, which the bar loses nothing by dropping.
const MIN_CHIP_CONTRAST: f64 = 4.5;

/// `ink` if it can be read off `ground`, else `None`. A theme is free to paint
/// "success" nearly the colour of the chip it would sit on: dayfox's
/// `#396847` on its own chip is contrast 1.94, and the previous gate — a luma
/// distance calibrated against the BAR, not against a chip — passed it by 0.7
/// units and shipped a chip worse than no green at all (review finding,
/// 2026-09-18). On a light ground [`chip_ink`] is lighter than the bar, so
/// dayfox's green now clears the gate (contrast 5.4 on its chip). A caller
/// that falls back to the default ink is still not guaranteed a legible chip:
/// ayu-light's own text is `#FCFCFC` on a near-white bar, which no chip rule
/// can fix (light-mode renders, 2026-10-20).
fn legible_on(ink: Rgb, ground: Rgb) -> Option<Rgb> {
    (contrast(ink, ground) >= MIN_CHIP_CONTRAST).then_some(ink)
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
/// oranges). When the theme runs dry the curated palette for the theme's
/// ground pads the rest under the same rule and the same legibility test, so
/// a short theme no longer paints one hue twice. An empty harvest (every slot
/// degenerate) is that curated palette verbatim; a harvest still short after
/// the pad (a theme whose hues sit between curated pairs) cycles its tail,
/// the honest last resort.
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
    let readable = |c: Rgb| c != default_ink && (luma(c) - luma(base)).abs() >= MIN_INK_LUMA_DIST;
    // The curated set for THIS ground. The dark one on a light theme padded
    // inks at contrast 1.3 to 2.4 (iceberg-light, pencil-light, ayu-light;
    // review finding, PR #291).
    let curated: [Rgb; PALETTE_LEN] = if light_ground(base) {
        LOTUS_PALETTE
    } else {
        core::array::from_fn(|i| PALETTE[i].0)
    };
    let legible: Vec<Rgb> = candidates
        .into_iter()
        .map(rgb)
        .filter(|&c| readable(c))
        .collect();
    if legible.is_empty() {
        return curated;
    }
    let spread = curated_spread();
    let mut picked: Vec<Rgb> = Vec::with_capacity(PALETTE_LEN);
    spread_into(&mut picked, &legible, spread);
    let pad: Vec<Rgb> = curated.into_iter().filter(|&c| readable(c)).collect();
    spread_into(&mut picked, &pad, spread);
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

    /// The bar logs this, and the drive checks it against the sandbox config.
    #[test]
    fn the_source_names_whose_colours_the_bar_paints() {
        for (styling, want) in [
            (no_theme_styles(), "curated"),
            (zellij_kanagawa(), "zellij"),
            (DEFAULT_STYLES, "curated"),
        ] {
            assert_eq!(source(&styling), want);
        }
    }

    /// With no zellij theme the bar paints the curated dark design, on a light
    /// terminal too: the terminal's report is not an input (LEDGER D42,
    /// 2026-10-27). Both sets zellij sends with no theme ([`names_no_theme`])
    /// count. Before drive 47 only the first was known, and it is the one a
    /// live bar never gets after its first `ModeUpdate`.
    #[test]
    fn with_no_theme_the_bar_paints_the_curated_dark_design() {
        for stock in [DEFAULT_STYLES, no_theme_styles()] {
            let t = Theme::from_styling(&stock);
            assert_eq!(t, Theme::default());
            assert!(!light_ground(t.base), "the curated design is dark");
        }
    }

    /// A themed bar takes its ground from its own background: a light zellij
    /// theme is the one way to a light bar.
    #[test]
    fn a_themed_bar_takes_its_ground_from_its_background() {
        let mut s = zellij_kanagawa();
        assert_eq!(Theme::from_styling(&s).inks, Inks::DARK);
        // catppuccin-latte's bar and text (`catppuccin-latte.kdl`).
        s.text_unselected.background = PaletteColor::Rgb((220, 224, 232));
        s.text_unselected.base = PaletteColor::Rgb((76, 79, 105));
        let t = Theme::from_styling(&s);
        assert_eq!(t.inks, Inks::LIGHT);
        assert!(
            luma(t.chip_ink) > luma(t.base),
            "chip text lighter than the bar"
        );
    }

    /// The cut is zellij's (`detect_theme_hue`). The two colours nearest it,
    /// one on each side, found by brute force over every triple (2026-10-21):
    /// a scaled sum one above the cut is light, and two below is dark.
    /// `#0DA371` is where the old Rec. 709 rule and zellij disagreed: luma
    /// put it exactly on the midpoint, which read dark; HSP is 130.8, light.
    #[test]
    fn the_ground_is_light_where_zellij_says_light() {
        assert!(light_ground(Rgb(200, 85, 22)), "one above the cut");
        assert!(!light_ground(Rgb(104, 148, 38)), "two below the cut");
        assert!(light_ground(Rgb(13, 163, 113)), "zellij calls it light");
        assert!(!light_ground(Rgb(127, 127, 127)));
        assert!(light_ground(Rgb(128, 128, 128)));
    }

    /// A light ground fades less than a dark one: a mix toward a light base
    /// costs more contrast, and at the dark depths the light rows washed out
    /// (the maintainer's light-mode check, 2026-10-27). At each depth the
    /// light text reads at least as the dark text does, and not much more,
    /// so the rows still recede. Red if the light ground takes the dark
    /// depths, and red if it stops fading. Measured on the lotus reference
    /// ground, where the depths were tuned.
    #[test]
    fn light_rows_recede_as_far_as_the_dark() {
        let (wave, lotus) = (Theme::default(), Theme::lotus());
        let reads = |t: &Theme, selected: bool, dormant: bool| {
            let ink = t
                .default_ink
                .mix(t.base, t.row_fade(selected, dormant, true));
            contrast(ink, t.base)
        };
        let mut last = f64::MAX;
        for (depth, selected, dormant) in [
            ("selected", true, false),
            ("faded", false, false),
            ("dormant", false, true),
        ] {
            let (dark, light) = (
                reads(&wave, selected, dormant),
                reads(&lotus, selected, dormant),
            );
            assert!(light >= dark, "{depth}: light {light:.2} < dark {dark:.2}");
            assert!(
                light < dark * 1.1,
                "{depth}: light {light:.2} barely recedes"
            );
            assert!(light < last, "{depth} must recede past the depth above");
            last = light;
        }
    }

    /// The light ground must read as well as the dark one does: each LIGHT
    /// ink, on the lotus reference ground, on its selected row, and faded as
    /// an unselected row fades, reaches the contrast its DARK twin has on the
    /// curated design. Each floor stops somewhere, so a dark ink that is far
    /// brighter than it needs to be does not force a light ink into black:
    /// WCAG AA on the bar (4.5), 3.0 on the selection, and 4.0 when faded.
    /// The shadow rule is exempt: it is meant to be barely there. The repo
    /// inks are the light pad pool ([`LOTUS_PALETTE`]), each under the chip
    /// text the light ground's [`chip_ink`] rule gives.
    #[test]
    fn light_inks_read_as_well_as_the_dark() {
        let roles = |i: Inks| {
            [
                ("needs_you", i.needs_you),
                ("working", i.working),
                ("done", i.done),
                ("failed", i.failed),
                ("opening", i.opening),
                ("green", i.green),
                ("yellow", i.yellow),
                ("orange", i.orange),
                ("red", i.red),
                ("worktree", i.worktree),
                ("branch", i.branch),
                ("turn", i.turn),
                ("subs", i.subs),
                ("bracket_a", i.bracket_a),
                ("bracket_b", i.bracket_b),
                ("meta", i.meta),
                ("pr", i.pr),
            ]
        };
        let (wave, lotus) = (Theme::default(), Theme::lotus());
        let faded =
            |c: Rgb, t: &Theme| contrast(c.mix(t.base, t.row_fade(false, false, true)), t.base);
        let reads = |name: &str, dark: Rgb, light: Rgb| {
            let on_bar = contrast(dark, wave.base).min(4.5);
            let on_sel = contrast(dark, wave.sel_bg).min(3.0);
            let as_faded = faded(dark, &wave).min(4.0);
            assert!(contrast(light, lotus.base) >= on_bar, "{name} on the bar");
            assert!(
                contrast(light, lotus.sel_bg) >= on_sel,
                "{name} on the selection"
            );
            assert!(faded(light, &lotus) >= as_faded, "{name} faded");
        };
        for ((name, dark), (_, light)) in roles(Inks::DARK).into_iter().zip(roles(Inks::LIGHT)) {
            reads(name, dark, light);
        }
        for (i, (dark, light)) in wave.palette.iter().zip(lotus.palette).enumerate() {
            reads(&format!("palette {i}"), *dark, light);
            assert!(
                contrast(lotus.chip_ink, light) >= MIN_CHIP_CONTRAST,
                "chip {i}"
            );
        }
    }

    /// Two repos must not share a colour on the light ground either: the
    /// lotus palette holds the spread every harvested palette is held to.
    /// Darkened alone, blue and violet sat 1286 apart (review finding,
    /// PR #291).
    #[test]
    fn the_light_repo_inks_stay_apart() {
        let spread = curated_spread();
        for (i, a) in LOTUS_PALETTE.iter().enumerate() {
            for b in &LOTUS_PALETTE[i + 1..] {
                assert!(
                    dist2(*a, *b) >= spread,
                    "{} and {} sit {} apart",
                    a.hex(),
                    b.hex(),
                    dist2(*a, *b)
                );
            }
        }
    }

    /// A light theme pads from the light set (review finding, PR #291). The
    /// dark curated palette padded it before, at contrast 1.3 on
    /// iceberg-light. Here a pencil-light bar has one legible hue: every pad
    /// is lotus's, and none is a dark ink. A light bar with no legible hue
    /// takes the lotus palette whole.
    #[test]
    fn a_light_theme_pads_from_the_light_set() {
        let bar = PaletteColor::Rgb((241, 241, 241));
        let one = StyleDeclaration {
            base: PaletteColor::Rgb((66, 66, 66)),
            background: bar,
            emphasis_0: PaletteColor::Rgb((32, 113, 185)),
            emphasis_1: bar,
            emphasis_2: bar,
            emphasis_3: bar,
        };
        let s = Styling {
            text_unselected: one,
            ..all_of(bar)
        };
        let p = Theme::from_styling(&s).palette;
        assert_eq!(p[0], Rgb(32, 113, 185), "the theme's own hue seeds");
        for c in &p[1..] {
            assert!(LOTUS_PALETTE.contains(c), "pad {} is lotus's", c.hex());
        }
        assert_eq!(Theme::from_styling(&all_of(bar)).palette, LOTUS_PALETTE);
    }

    /// The four bands are one gauge read by colour, so neighbours must not
    /// blur into one. The bar is the curated palette's closest pair (sakuraPink
    /// beside waveRed): the design's own answer to "how close can two inks sit
    /// and still read as two". Darkening lotusOrange alone put the orange band
    /// at 482 from the yellow one.
    #[test]
    fn the_battery_bands_stay_apart_on_both_grounds() {
        for inks in [Inks::DARK, Inks::LIGHT] {
            let ramp = [Band::Green, Band::Yellow, Band::Orange, Band::Red].map(|b| inks.band(b));
            for pair in ramp.windows(2) {
                assert!(dist2(pair[0], pair[1]) >= curated_spread(), "{pair:?}");
            }
        }
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
        // autumnGreen — zellij kanagawa's own `exit_code_success`, and NOT the
        // curated springGreen: the role follows the theme, so the terminal
        // chip wears the green this theme calls success.
        assert_eq!(t.term_ink, Rgb(118, 148, 106), "terminal name = success");
    }

    /// The terminal green is the one themed ink that sits on the chip rather
    /// than on the bar, so it is gated against the chip. A theme whose success
    /// colour cannot be read there keeps a readable name instead of a green
    /// one: an unreadable chip is worse than a chip with no green in it.
    ///
    /// The two real themes below are the cases a luma-distance gate got wrong
    /// (review finding, 2026-09-18) — dayfox passed that gate by 0.7 units at
    /// contrast 1.94, and every LIGHT theme puts its success BELOW the chip in
    /// luminance, the direction an unsigned distance cannot see.
    #[test]
    fn a_success_colour_the_chip_cannot_carry_falls_back_to_the_default_ink() {
        let mut s = zellij_kanagawa();
        let chip = Theme::from_styling(&s).chip_ink;
        s.exit_code_success.base = PaletteColor::Rgb((chip.0, chip.1, chip.2));
        let t = Theme::from_styling(&s);
        assert_eq!(t.term_ink, t.default_ink, "swallowed green reads as text");
        // The floor is strict, and that is the intent: molokai's `#008C00`
        // reads at 4.75 on molokai's own chip but only 4.32 on kanagawa's
        // near-black one, so the same green is kept there and dropped here.
        // The gate answers "on THIS chip", never "is this colour bright".
        s.exit_code_success.base = PaletteColor::Rgb((0, 140, 0));
        assert_eq!(Theme::from_styling(&s).term_ink, t.default_ink);
        // dracula's success, 15.3 on the same chip — kept, obviously.
        s.exit_code_success.base = PaletteColor::Rgb((80, 250, 123));
        assert_eq!(Theme::from_styling(&s).term_ink, Rgb(80, 250, 123));
        // dayfox, whole: a LIGHT bar. Its forest green read at 1.94 on the
        // old chip, a grey darker than the bar. The chip is lighter than the
        // bar now, so the same green is kept, and the gate still decides it.
        s.text_unselected.background = PaletteColor::Rgb((211, 199, 187));
        s.text_unselected.base = PaletteColor::Rgb((100, 63, 97));
        s.exit_code_success.base = PaletteColor::Rgb((57, 104, 71));
        let t = Theme::from_styling(&s);
        assert!(contrast(Rgb(57, 104, 71), t.chip_ink) >= MIN_CHIP_CONTRAST);
        assert_eq!(t.term_ink, Rgb(57, 104, 71), "light chip carries the green");
    }

    /// Contrast has no direction — the same pair either reads or does not,
    /// whichever way round it is asked. A signed or unsigned DIFFERENCE would
    /// have to remember which colour is the ground; this cannot.
    #[test]
    fn contrast_is_symmetric_and_spans_the_standard_range() {
        let (black, white) = (Rgb(0, 0, 0), Rgb(255, 255, 255));
        assert!((contrast(black, white) - 21.0).abs() < 1e-9);
        assert!((contrast(white, black) - 21.0).abs() < 1e-9);
        assert!((contrast(white, white) - 1.0).abs() < 1e-9);
        let (a, b) = (Rgb(57, 104, 71), Rgb(148, 139, 131));
        assert!((contrast(a, b) - contrast(b, a)).abs() < 1e-9);
        assert_eq!(legible_on(a, b), None, "dayfox: 1.94 is not readable");
        assert_eq!(legible_on(black, white), Some(black));
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
