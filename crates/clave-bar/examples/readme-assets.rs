//! README asset generator — the committed SVGs under `docs/assets/`.
//!
//!     cargo run -p clave-bar --example readme-assets
//!
//! Two outputs, both traced from real font outlines so GitHub needs no fonts
//! and no terminal:
//!
//! - `docs/assets/glyphs/*.svg` — the vocabulary icons: every status mark,
//!   provenance mark, terminal-status mark, battery level and repo swatch,
//!   each a uniform square on the bar's own background.
//! - `docs/assets/sidebar.svg` — the hero frame: the shared showcase fixture
//!   rendered through `render_rows` (the plugin's own renderer), its ANSI
//!   parsed into positioned, coloured glyph outlines.
//!
//! Colours come from `clave_bar::render` (`RowStatus::mark`, `BATTERY`,
//! `PALETTE`, `BASE`, …), never copied, so the assets cannot drift from the
//! shipped palette. Fonts are the ones the design was ratified against:
//! JetBrainsMono Nerd Font Mono, plus Noto Sans Bamum for the worktree mark
//! (macOS ships it in the system Supplemental fonts). Override discovery with
//! `CLAVE_ASSET_FONT` / `CLAVE_ASSET_FONT_BAMUM` / `CLAVE_ASSET_FONT_EXTRA`
//! (extra = fallback for symbols the mono font lacks).

use std::fmt::Write as _;
use std::path::{Path, PathBuf};

use clave_bar::render::{
    BASE, BATTERY, CONSOLE, DEFAULT_INK, DESIGN_COLS, PALETTE, Provenance, Rgb, RowStatus,
    TermStatus, Widths, display_cells, render_rows, strip_sgr,
};

#[path = "shared/showcase_fixture.rs"]
#[allow(dead_code)]
mod showcase_fixture;
use showcase_fixture::showcase;

// ── fonts ───────────────────────────────────────────────────────────────────

struct Font {
    data: Vec<u8>,
    path: PathBuf,
}

impl Font {
    fn face(&self) -> ttf_parser::Face<'_> {
        ttf_parser::Face::parse(&self.data, 0)
            .unwrap_or_else(|e| panic!("unparseable font {}: {e}", self.path.display()))
    }
}

/// First existing path wins; the env var beats them all.
fn find_font(env: &str, candidates: &[&str]) -> Option<Font> {
    let home = std::env::var("HOME").unwrap_or_default();
    std::env::var(env)
        .ok()
        .map(PathBuf::from)
        .into_iter()
        .chain(
            candidates
                .iter()
                .map(|c| PathBuf::from(c.replace('~', &home))),
        )
        .find(|p| p.exists())
        .map(|path| Font {
            data: std::fs::read(&path)
                .unwrap_or_else(|e| panic!("reading {}: {e}", path.display())),
            path,
        })
}

fn load_fonts() -> Vec<Font> {
    let mono = find_font(
        "CLAVE_ASSET_FONT",
        &[
            "~/Library/Fonts/JetBrainsMonoNerdFontMono-Regular.ttf",
            "/Library/Fonts/JetBrainsMonoNerdFontMono-Regular.ttf",
            "~/.local/share/fonts/JetBrainsMonoNerdFontMono-Regular.ttf",
            "/usr/share/fonts/truetype/jetbrains-mono/JetBrainsMonoNerdFontMono-Regular.ttf",
        ],
    )
    .expect("JetBrainsMono Nerd Font Mono not found; set CLAVE_ASSET_FONT");
    let bamum = find_font(
        "CLAVE_ASSET_FONT_BAMUM",
        &[
            "/System/Library/Fonts/Supplemental/NotoSansBamum-Regular.ttf",
            "/usr/share/fonts/truetype/noto/NotoSansBamum-Regular.ttf",
        ],
    )
    .expect("Noto Sans Bamum not found; set CLAVE_ASSET_FONT_BAMUM");
    let mut fonts = vec![mono, bamum];
    // Symbols the mono font does not carry (✖ and ↻ live outside the Nerd
    // Font patch ranges); the terminal solves this with system fallback, we
    // solve it with every extra face that exists. `CLAVE_ASSET_FONT_EXTRA`
    // prepends one more.
    if let Some(extra) = find_font("CLAVE_ASSET_FONT_EXTRA", &[]) {
        fonts.push(extra);
    }
    for candidate in [
        "/System/Library/Fonts/Supplemental/STIXGeneral.otf",
        "/System/Library/Fonts/ZapfDingbats.ttf",
        "/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf",
    ] {
        let p = PathBuf::from(candidate);
        if p.exists() {
            fonts.push(Font {
                data: std::fs::read(&p).unwrap_or_else(|e| panic!("reading {candidate}: {e}")),
                path: p,
            });
        }
    }
    fonts
}

// ── outlines ────────────────────────────────────────────────────────────────

/// Collects a glyph outline as an SVG path `d` string, in font units (y-up;
/// the flip happens in the def's transform).
struct PathSink(String);

impl ttf_parser::OutlineBuilder for PathSink {
    fn move_to(&mut self, x: f32, y: f32) {
        let _ = write!(self.0, "M{x:.1} {y:.1}");
    }
    fn line_to(&mut self, x: f32, y: f32) {
        let _ = write!(self.0, "L{x:.1} {y:.1}");
    }
    fn quad_to(&mut self, x1: f32, y1: f32, x: f32, y: f32) {
        let _ = write!(self.0, "Q{x1:.1} {y1:.1} {x:.1} {y:.1}");
    }
    fn curve_to(&mut self, x1: f32, y1: f32, x2: f32, y2: f32, x: f32, y: f32) {
        let _ = write!(self.0, "C{x1:.1} {y1:.1} {x2:.1} {y2:.1} {x:.1} {y:.1}");
    }
    fn close(&mut self) {
        self.0.push('Z');
    }
}

/// A traced glyph: which font, its path, its bbox, all in that font's units.
struct Traced {
    font: usize,
    d: String,
    bbox: ttf_parser::Rect,
    units_per_em: f32,
}

/// Trace `ch` from the first font that has a non-empty outline for it.
fn trace(fonts: &[Font], ch: char) -> Option<Traced> {
    for (i, font) in fonts.iter().enumerate() {
        let face = font.face();
        let Some(gid) = face.glyph_index(ch) else {
            continue;
        };
        let mut sink = PathSink(String::new());
        let Some(bbox) = face.outline_glyph(gid, &mut sink) else {
            continue;
        };
        return Some(Traced {
            font: i,
            d: sink.0,
            bbox,
            units_per_em: face.units_per_em() as f32,
        });
    }
    None
}

fn hex(c: Rgb) -> String {
    format!("#{:02X}{:02X}{:02X}", c.0, c.1, c.2)
}

// ── icons ───────────────────────────────────────────────────────────────────

/// One square icon: the bar's background as a rounded tile, the glyph's bbox
/// centered on it. Uniform `SIDE` regardless of glyph, so the README's table
/// cells all line up.
const SIDE: f32 = 64.0;

fn icon_svg(fonts: &[Font], ch: char, ink: Rgb) -> String {
    let t = trace(fonts, ch).unwrap_or_else(|| panic!("no font here carries {ch:?}"));
    // The glyph renders at a fixed em size; centering is by outline bbox so
    // dots, crosses and marks all sit optically centred on the tile.
    let scale = SIDE * 0.72 / t.units_per_em;
    let (w, h) = (
        (t.bbox.x_max - t.bbox.x_min) as f32 * scale,
        (t.bbox.y_max - t.bbox.y_min) as f32 * scale,
    );
    let x = (SIDE - w) / 2.0 - t.bbox.x_min as f32 * scale;
    let y = (SIDE + h) / 2.0 + t.bbox.y_min as f32 * scale;
    format!(
        "<svg xmlns=\"http://www.w3.org/2000/svg\" viewBox=\"0 0 {SIDE} {SIDE}\" \
         width=\"20\" height=\"20\">\n\
         <rect width=\"{SIDE}\" height=\"{SIDE}\" rx=\"10\" fill=\"{bg}\"/>\n\
         <path d=\"{d}\" fill=\"{ink}\" \
         transform=\"translate({x:.1} {y:.1}) scale({scale:.4} -{scale:.4})\"/>\n\
         </svg>\n",
        bg = hex(BASE),
        d = t.d,
        ink = hex(ink),
    )
}

/// A plain colour tile — the repo-ink swatches.
fn swatch_svg(ink: Rgb) -> String {
    format!(
        "<svg xmlns=\"http://www.w3.org/2000/svg\" viewBox=\"0 0 {SIDE} {SIDE}\" \
         width=\"20\" height=\"20\">\n\
         <rect width=\"{SIDE}\" height=\"{SIDE}\" rx=\"10\" fill=\"{bg}\"/>\n\
         <rect x=\"14\" y=\"14\" width=\"36\" height=\"36\" rx=\"8\" fill=\"{ink}\"/>\n\
         </svg>\n",
        bg = hex(BASE),
        ink = hex(ink),
    )
}

fn write_icons(fonts: &[Font], dir: &Path) {
    let status = [
        ("status-needs-you", RowStatus::NeedsYou),
        ("status-working", RowStatus::Working),
        ("status-done", RowStatus::Done),
        ("status-idle", RowStatus::Idle),
        ("status-failed", RowStatus::Failed),
        ("status-stale", RowStatus::Stale),
        ("status-dormant", RowStatus::Dormant),
        ("status-dormant-selected", RowStatus::DormantSelected),
        ("status-opening", RowStatus::Opening),
    ];
    for (name, s) in status {
        let (ch, ink) = s.mark();
        write_file(&dir.join(format!("{name}.svg")), &icon_svg(fonts, ch, ink));
    }
    for (name, t) in [
        ("term-idle", TermStatus::Idle),
        ("term-running", TermStatus::Running),
        ("term-done", TermStatus::Done),
        ("term-failed", TermStatus::Failed),
    ] {
        write_file(
            &dir.join(format!("{name}.svg")),
            &icon_svg(fonts, CONSOLE, t.ink()),
        );
    }
    // Provenance marks ride the default ink, like the bar renders them.
    let ink = DEFAULT_INK;
    for (name, p) in [
        ("mark-branch", Provenance::Branch),
        ("mark-worktree", Provenance::Worktree),
    ] {
        let ch = p.mark().expect("marked provenance has a glyph");
        write_file(&dir.join(format!("{name}.svg")), &icon_svg(fonts, ch, ink));
    }
    for (i, (ch, ink)) in BATTERY.iter().enumerate() {
        write_file(
            &dir.join(format!("battery-{i:02}.svg")),
            &icon_svg(fonts, *ch, *ink),
        );
    }
    for (i, (ink, _)) in PALETTE.iter().enumerate() {
        write_file(&dir.join(format!("repo-{i}.svg")), &swatch_svg(*ink));
    }
}

// ── the hero frame ──────────────────────────────────────────────────────────

/// Minimal truecolor SGR state: exactly what `render_rows` emits.
#[derive(Clone, Copy)]
struct Pen {
    fg: Option<Rgb>,
    bg: Option<Rgb>,
}

/// One positioned glyph (cell coordinates) and the bg runs, parsed from one
/// rendered row.
struct Span {
    cell: usize,
    width: usize,
    ch: char,
    fg: Option<Rgb>,
}

fn parse_row(line: &str) -> (Vec<Span>, Vec<(usize, usize, Rgb)>) {
    let mut pen = Pen { fg: None, bg: None };
    let mut spans = Vec::new();
    let mut bgs: Vec<(usize, usize, Rgb)> = Vec::new();
    let mut cell = 0usize;
    let mut chars = line.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\u{1b}' {
            // CSI … 'm' only — that is all the renderer emits.
            let mut body = String::new();
            for e in chars.by_ref() {
                if e == '[' {
                    continue;
                }
                if e == 'm' {
                    break;
                }
                body.push(e);
            }
            let mut codes = body.split(';').map(|s| s.parse::<u8>().unwrap_or(0));
            while let Some(code) = codes.next() {
                match code {
                    0 => pen = Pen { fg: None, bg: None },
                    38 | 48 => {
                        // 38;2;r;g;b / 48;2;r;g;b
                        let (two, r, g, b) = (
                            codes.next().unwrap_or(0),
                            codes.next().unwrap_or(0),
                            codes.next().unwrap_or(0),
                            codes.next().unwrap_or(0),
                        );
                        assert_eq!(two, 2, "renderer emits truecolor only");
                        let rgb = Rgb(r, g, b);
                        if code == 38 {
                            pen.fg = Some(rgb);
                        } else {
                            pen.bg = Some(rgb);
                        }
                    }
                    1 => {} // bold: the outline font is the weight we trace
                    other => panic!("unexpected SGR code {other} in rendered row"),
                }
            }
            continue;
        }
        let w = display_cells(&c.to_string()).max(1);
        if let Some(bg) = pen.bg {
            match bgs.last_mut() {
                Some((start, len, run)) if *start + *len == cell && *run == bg => *len += w,
                _ => bgs.push((cell, w, bg)),
            }
        }
        if c != ' ' {
            spans.push(Span {
                cell,
                width: w,
                ch: c,
                fg: pen.fg,
            });
        }
        cell += w;
    }
    (spans, bgs)
}

fn hero_svg(fonts: &[Font]) -> String {
    let rows = showcase();
    let lines = render_rows(&rows, DESIGN_COLS, rows.len(), Widths::EXPANDED);
    for (line, row) in lines.iter().zip(&rows) {
        let width = display_cells(&strip_sgr(line));
        assert_eq!(width, DESIGN_COLS, "row is {width} cells: {row:?}");
    }

    // Cell geometry from the mono face itself, at a fixed pixel size.
    let face = fonts[0].face();
    let upem = face.units_per_em() as f32;
    let font_px = 16.0;
    let advance = face
        .glyph_index('M')
        .and_then(|g| face.glyph_hor_advance(g))
        .expect("mono face has an advance") as f32
        / upem
        * font_px;
    let ascent = face.ascender() as f32 / upem * font_px;
    let descent = -face.descender() as f32 / upem * font_px;
    let cell_h = (ascent + descent) * 1.06;
    let pad = 12.0;
    let width = DESIGN_COLS as f32 * advance + pad * 2.0;
    let height = lines.len() as f32 * cell_h + pad * 2.0;

    // Dedupe outlines: one def per (font, char), `<use>` per placement.
    let mut defs = String::new();
    let mut seen: std::collections::BTreeMap<char, (usize, f32)> =
        std::collections::BTreeMap::new();
    let mut body = String::new();
    for (r, line) in lines.iter().enumerate() {
        let (spans, bgs) = parse_row(line);
        let top = pad + r as f32 * cell_h;
        for (start, len, bg) in bgs {
            let _ = writeln!(
                body,
                "<rect x=\"{:.1}\" y=\"{top:.1}\" width=\"{:.1}\" height=\"{cell_h:.1}\" fill=\"{}\"/>",
                pad + start as f32 * advance,
                len as f32 * advance,
                hex(bg),
            );
        }
        for s in spans {
            if let std::collections::btree_map::Entry::Vacant(e) = seen.entry(s.ch) {
                let t =
                    trace(fonts, s.ch).unwrap_or_else(|| panic!("no font here carries {:?}", s.ch));
                let scale = font_px / t.units_per_em;
                let _ = writeln!(defs, "<path id=\"g{:x}\" d=\"{}\"/>", s.ch as u32, t.d);
                e.insert((t.font, scale));
            }
            let (_, scale) = seen[&s.ch];
            // Center wide glyphs across their display cells; baseline sits
            // `ascent` below the row top.
            let x = pad + s.cell as f32 * advance;
            let y = top + ascent;
            let ink = s.fg.map_or_else(|| hex(DEFAULT_INK), hex);
            let _ = writeln!(
                body,
                "<use href=\"#g{:x}\" fill=\"{ink}\" \
                 transform=\"translate({x:.1} {y:.1}) scale({scale:.4} -{scale:.4})\"/>",
                s.ch as u32,
            );
            let _ = s.width;
        }
    }
    format!(
        "<svg xmlns=\"http://www.w3.org/2000/svg\" viewBox=\"0 0 {width:.0} {height:.0}\" \
         width=\"{w2:.0}\">\n<defs>\n{defs}</defs>\n\
         <rect width=\"{width:.0}\" height=\"{height:.0}\" rx=\"8\" fill=\"{base}\"/>\n\
         {body}</svg>\n",
        base = hex(BASE),
        w2 = width,
    )
}

// ── io ──────────────────────────────────────────────────────────────────────

fn write_file(path: &Path, content: &str) {
    std::fs::write(path, content).unwrap_or_else(|e| panic!("writing {}: {e}", path.display()));
    println!("wrote {}", path.display());
}

fn main() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let glyphs = root.join("docs/assets/glyphs");
    std::fs::create_dir_all(&glyphs).expect("creating docs/assets/glyphs");
    let fonts = load_fonts();
    write_icons(&fonts, &glyphs);
    write_file(&root.join("docs/assets/sidebar.svg"), &hero_svg(&fonts));
}
