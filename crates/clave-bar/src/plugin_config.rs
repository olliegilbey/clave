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
        c.insert(clave_types::CLAVE_BINARY_KEY.to_string(), "clave".to_string());
        assert_eq!(resolve_binary(&c).as_deref(), Some("clave"));
    }
}
