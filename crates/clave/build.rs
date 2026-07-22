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
            println!("cargo:rerun-if-changed={src}");
        }
        Err(_) => std::fs::write(&out, []).expect("writing empty embed marker"),
    }
}
