//! Embeds the bar wasm into release binaries (spec §Distribution): cargo-dist
//! ships ONE file, so the wasm must ride inside the CLI. Gated on the
//! CLAVE_BAR_WASM env var (the CLAVE_BUILD_TAG pattern): release CI builds
//! the wasm first and points this var at it; dev builds embed an empty
//! marker and the sandbox flow (just dev-install) is untouched.
fn main() {
    println!("cargo:rerun-if-env-changed=CLAVE_BAR_WASM");
    let out = std::path::PathBuf::from(std::env::var("OUT_DIR").unwrap()).join("clave-bar.embedded");
    match std::env::var("CLAVE_BAR_WASM") {
        Ok(src) => {
            std::fs::copy(&src, &out).expect("CLAVE_BAR_WASM is set but unreadable");
            // Fix 6 (review 2026-07-22): fail LOUDLY at build time on a
            // mis-embedded wasm. A zero-byte or non-wasm file would ship a
            // release whose embedded_wasm() returns None (empty marker) or
            // junk, silently mis-guiding end users — a release build must
            // never ship a broken embed. Validate the copied bytes: non-empty
            // AND the wasm magic (\0asm) leading the module header.
            let bytes = std::fs::read(&out).expect("reading copied embed for validation");
            assert!(
                bytes.len() >= 4 && &bytes[..4] == b"\0asm",
                "CLAVE_BAR_WASM at {src} is not a wasm module \
                 (empty or missing the \\0asm magic) — refusing to embed"
            );
            println!("cargo:rerun-if-changed={src}");
        }
        Err(_) => std::fs::write(&out, []).expect("writing empty embed marker"),
    }
}
