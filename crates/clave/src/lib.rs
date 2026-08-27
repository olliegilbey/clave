//! Library root for the `clave` binary. Reusable, testable logic lives here as
//! modules; `main.rs` stays a thin clap entry point that calls into this crate.
//! (A bin crate can't be reached by integration tests or examples, so we split
//! out a lib — this is what lets the S0b spike and later tasks call `munge_cwd`.)

// Modules are added per task. Task 2 adds `pub mod store;`.
pub mod add;
pub mod backfill;
pub mod dev;
pub mod discover;
pub mod doctor;
pub mod env;
pub mod evlog;
pub mod hook;
pub mod lsview;
pub mod munge;
pub mod open;
pub mod pr;
pub mod release;
pub mod sandbox;
pub mod setup;
pub mod spawn;
pub mod store;
