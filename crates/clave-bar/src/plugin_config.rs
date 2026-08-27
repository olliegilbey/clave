//! Plugin-configuration resolution (#44). Split out of `main.rs` — like
//! `model.rs` — so it host-tests without linking the wasm-only bin (see
//! `lib.rs`: the bin's `report_panic` shim references `host_run_plugin_command`
//! unconditionally, so ANY test harness built for the bin fails to link on
//! the host target regardless of what the test itself touches).

use std::collections::BTreeMap;

/// The `clave` binary this bar must invoke, from its zellij plugin
/// configuration (#44).
///
/// `Option`, not `(String, bool)`: the caller needs to distinguish "key
/// absent" (warn — a pre-#44 layout, and we are about to resolve through
/// PATH, which is exactly what broke v0.1.1) from "key present with the
/// value `clave`" (the legitimate dev/sandbox baking, no warning owed). An
/// empty value counts as absent — running `""` is worse than the fallback.
///
/// Pure so it unit-tests on the host: no wasm target, no zellij, no TTY.
pub fn resolve_binary(config: &BTreeMap<String, String>) -> Option<String> {
    config
        .get(clave_types::CLAVE_BINARY_KEY)
        .filter(|v| !v.is_empty())
        .cloned()
}

/// Which row geometry this bar renders, from its zellij plugin configuration
/// (#232). Every layout since Task 5 bakes `row_height` alongside
/// `clave_binary`, so the key is present in steady state; a pre-#232 layout
/// or a hand-edited config lacks it and `RowHeight::from_config_value` fails
/// CLOSED to `Double`, never a surprise legacy `Single` render.
///
/// Pure so it unit-tests on the host, same discipline as `resolve_binary`.
pub fn resolve_row_height(config: &BTreeMap<String, String>) -> clave_types::RowHeight {
    clave_types::RowHeight::from_config_value(
        config.get(clave_types::ROW_HEIGHT_KEY).map(String::as_str),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolve_binary_takes_the_configured_path() {
        let mut c = BTreeMap::new();
        c.insert(
            clave_types::CLAVE_BINARY_KEY.to_string(),
            "/data/clave/bin/clave-v0.1.1".to_string(),
        );
        assert_eq!(
            resolve_binary(&c).as_deref(),
            Some("/data/clave/bin/clave-v0.1.1")
        );
    }

    #[test]
    fn resolve_binary_is_none_when_absent() {
        // A pre-#44 layout or a hand-edited config. The caller falls back to
        // PATH `clave` AND announces it — silence is what hid the v0.1.1 field
        // incident for hours (#44).
        assert_eq!(resolve_binary(&BTreeMap::new()), None);
    }

    #[test]
    fn resolve_binary_treats_empty_as_absent() {
        // run_command(&["", "open", …]) is a worse failure than the fallback.
        let mut c = BTreeMap::new();
        c.insert(clave_types::CLAVE_BINARY_KEY.to_string(), String::new());
        assert_eq!(resolve_binary(&c), None);
    }

    #[test]
    fn resolve_binary_accepts_bare_clave() {
        // The dev/sandbox value is literally `clave` — present and legitimate,
        // so it must be Some (no warning), NOT conflated with the absent case.
        let mut c = BTreeMap::new();
        c.insert(
            clave_types::CLAVE_BINARY_KEY.to_string(),
            "clave".to_string(),
        );
        assert_eq!(resolve_binary(&c).as_deref(), Some("clave"));
    }

    #[test]
    fn resolve_row_height_reads_the_key_and_defaults_double() {
        let mut c = BTreeMap::new();
        assert_eq!(resolve_row_height(&c), clave_types::RowHeight::Double);
        c.insert(clave_types::ROW_HEIGHT_KEY.into(), "single".into());
        assert_eq!(resolve_row_height(&c), clave_types::RowHeight::Single);
        c.insert(clave_types::ROW_HEIGHT_KEY.into(), "garbage".into());
        assert_eq!(resolve_row_height(&c), clave_types::RowHeight::Double);
    }

    #[test]
    fn main_never_shells_out_to_a_bare_clave() {
        // The seven bar→CLI shellouts are the whole point of #44, but they live
        // in main.rs, which `test = false` (Cargo.toml) makes unreachable by any
        // runtime test — the bin can't host-link (see lib.rs). Until #47's
        // tier-2 real-zellij harness exists, this source-text guard is the only
        // thing standing between a reverted shellout and a green suite.
        //
        // COUNT, not substring-absence. The original form asserted the absence
        // of `"clave",` and a 2026-07-25 review proved it blind to three live
        // mutations: a byte-exact revert of the prune-tabs site (whose pre-#44
        // text was `vec!["clave".into(), …]` — no comma after the quote), any
        // variable indirection (`let cli = "clave";`), and disabling the feature
        // wholesale in load(). Counting the bare `"clave"` literal catches all
        // three: main.rs must contain EXACTLY ONE, the PATH fallback in load().
        // Pipe names (`"clave-nav"`, `"clave-visited"`, …) never match — the
        // char after `clave` is `-`, not the closing quote.
        let src = include_str!("../src/main.rs");
        assert_eq!(
            src.matches(r#""clave""#).count(),
            1,
            "#44: main.rs must contain exactly ONE bare \"clave\" literal — the \
             PATH fallback in load(). Any other is a shellout resolving through \
             PATH again; bake self.clave_binary instead (see resolve_binary)"
        );
        // The count above cannot see load() being cut off from its configuration
        // (mutation J: `resolve_binary(&BTreeMap::new())` leaves the literal
        // count at 1 while disabling the feature entirely).
        assert!(
            src.contains("resolve_binary(&config)"),
            "#44: load() no longer feeds its zellij plugin configuration to \
             resolve_binary — the bar is back on PATH for every shellout"
        );
    }
}
