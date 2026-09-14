//! What `just release` builds must BE a release, by the test the code applies.
//!
//! `run_setup` (setup.rs) asks one question to tell a release install from a
//! dev build: `release::embedded_wasm()`. Every local cut answered "dev",
//! because the `release:` recipe never set `CLAVE_BAR_WASM` on the CLI build —
//! only `dist-build` and release CI did. The installed launcher therefore took
//! the dev branch of its own setup and hit the guard that refuses a dev
//! binary's setup against a release install. `clave setup` and `clave rows`
//! were unusable on every machine cut with `just release`; on 2026-09-14
//! `clave rows card` wrote the store, failed the regeneration, and left the
//! split that starts a second sidebar (docs/status/2026-09-14-1220).
//!
//! Source text rather than behaviour, like `script_hygiene.rs`: the property
//! is about what the recipe SAYS, and running it costs a full release build
//! and writes the maintainer's install.

const JUSTFILE: &str = include_str!("../../../justfile");

/// The body of a recipe: the indented lines under `<name>:`, up to the next
/// line that starts at column 0.
fn recipe_body(name: &str) -> Vec<&'static str> {
    let mut lines = JUSTFILE
        .lines()
        .skip_while(|l| !(l.starts_with(name) && l[name.len()..].starts_with(':')));
    lines
        .next()
        .unwrap_or_else(|| panic!("recipe {name} is missing from the justfile"));
    lines
        .take_while(|l| l.trim().is_empty() || l.starts_with([' ', '\t']))
        .collect()
}

/// The line that builds the HOST CLI, which is the artifact that gets
/// installed and later asked whether it is a release.
fn cli_build_line(body: &[&str]) -> String {
    let found: Vec<&&str> = body
        .iter()
        .filter(|l| {
            // `-p clave-bar`, not `clave-bar`: the CLI line NAMES the bar wasm
            // in CLAVE_BAR_WASM, which is the whole point of it.
            l.contains("cargo build") && l.contains("-p clave") && !l.contains("-p clave-bar")
        })
        .collect();
    assert_eq!(
        found.len(),
        1,
        "expected exactly one CLI build line, got {found:?}"
    );
    found[0].to_string()
}

#[test]
fn a_local_cut_embeds_the_bar_the_way_release_ci_does() {
    let line = cli_build_line(&recipe_body("release"));
    assert!(
        line.contains("CLAVE_BAR_WASM="),
        "`just release` builds the CLI without CLAVE_BAR_WASM, so the installed \
         binary reports embedded_wasm() == None and its own setup takes the dev \
         branch: {line}"
    );
}

#[test]
fn the_parity_build_stays_in_parity() {
    let line = cli_build_line(&recipe_body("dist-build"));
    assert!(
        line.contains("CLAVE_BAR_WASM="),
        "dist-build is the local stand-in for what cargo-dist ships; without \
         the embed it stands in for nothing: {line}"
    );
}
