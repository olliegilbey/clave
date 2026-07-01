//! Debug helper for the S0b spike (Task 4): prints `munge_cwd(argv[1])` so a
//! shell harness can compare our computed dir name against what Claude wrote to
//! `~/.claude/projects/`. Not part of the shipped binary.
use clave::munge::munge_cwd;

fn main() {
    let arg = std::env::args()
        .nth(1)
        .expect("usage: cargo run -p clave --example munge -- <path>");
    println!("{}", munge_cwd(&arg));
}
