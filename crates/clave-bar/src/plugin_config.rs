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
        c.insert(
            clave_types::CLAVE_BINARY_KEY.to_string(),
            "clave".to_string(),
        );
        assert_eq!(resolve_binary(&c).as_deref(), Some("clave"));
    }

    #[test]
    fn main_never_shells_out_to_a_bare_clave() {
        // The seven bar→CLI shellouts are the whole point of #44, but they live
        // in main.rs, which `test = false` (Cargo.toml) makes unreachable by any
        // runtime test — the bin can't host-link (see lib.rs). A whole-branch
        // review proved the gap live: reverting one site to
        // `run_command(&["clave", …])` kept every gate green. This crude
        // source-text guard is the ONLY thing that fails on that reversion until
        // #47's tier-2 real-zellij harness exists.
        //
        // Pattern `"clave",` = a bare "clave" as the FIRST argv element followed
        // by more args — the exact reverted-shellout shape. It cannot match the
        // legitimate `"clave".to_string()` PATH fallback in load() (`.` not `,`
        // after the quote), nor any `"clave-*"` pipe name (suffix before the
        // closing quote). Both properties are asserted below so a future rename
        // that breaks the discriminator fails loudly instead of silently.
        let src = include_str!("../src/main.rs");
        assert!(
            !src.contains(r#""clave","#),
            "#44: a bar shellout resolves bare `clave` through PATH again — \
             bake self.clave_binary instead (see resolve_binary)"
        );
        // Guard-integrity: the two shapes the pattern must tolerate DO occur in
        // main.rs, so if a refactor made either collide with the pattern this
        // test would start lying. Pin them.
        assert!(
            src.contains(r#""clave".to_string()"#),
            "the PATH fallback moved — re-vet that the guard pattern still \
             excludes it before trusting this test"
        );
    }
}
