//! Library root for the `clave` binary. Reusable, testable logic lives here as
//! modules; `main.rs` stays a thin clap entry point that calls into this crate.
//! (A bin crate can't be reached by integration tests or examples, so we split
//! out a lib — this is what lets the S0b spike and later tasks call `munge_cwd`.)

// Modules are added per task. Task 3 adds `pub mod munge;`.
