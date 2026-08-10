//! The sandbox INSTANCE: which `clave-test…` session and which
//! `~/.local/state/clave-dev…` root this working tree stages into.
//!
//! Until now both were machine-wide singletons, so two agent sessions in two
//! worktrees each ran `just sandbox` and the second one's artifacts won —
//! wasm, generated `config.kdl`/`layout.kdl` and the PATH shim all live under
//! one root. The first agent then launched a "sandbox" running the other's
//! build and measured it for a full round (FOOTGUNS, "PATH and version
//! coherence" — the concurrent-sandbox entry).
//!
//! The key is the WORKTREE DIRECTORY NAME. It is unique per agent by
//! construction (git refuses two worktrees at one path) and it is legible in
//! `zellij list-sessions` — `clave-test-s112-segrega` beside
//! `clave-test-prune-wt` says whose is whose, where a random id would not.
//! One key drives the session name, the state dir, the data dir and the shim
//! dir together, so one agent's sandbox cannot read or write another's.
//!
//! **The MAIN checkout keeps `clave-test` and `~/.local/state/clave-dev`.**
//! That is the name the maintainer runs and the one every doc, footgun and
//! runbook names; it must not change under him because someone else added
//! worktrees. It is a deliberate branch of the derivation (`key_for`), not a
//! fallback — `key_for` returns `None` only for the main tree, and errors
//! rather than silently falling back for a linked worktree it cannot name.

use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context, Result};

/// The sandbox session name for the main checkout, and the prefix every
/// per-worktree name extends.
pub const SESSION_PREFIX: &str = "clave-test";

/// The sandbox root directory name under `~/.local/state`, likewise.
pub const ROOT_PREFIX: &str = "clave-dev";

/// The marker file at a sandbox root naming the worktree it was staged from.
/// The reaper's whole input: no marker, no reap.
pub const ORIGIN_FILE: &str = "origin";

/// Longest session name zellij can actually run, in BYTES.
///
/// Zellij binds a unix domain socket at `<socket dir>/<session name>` and
/// refuses a path of `ZELLIJ_SOCK_MAX_LENGTH` bytes or more — 104 on macOS,
/// 108 elsewhere (`zellij-utils-0.44.3/src/consts.rs:307-309`), matching
/// `sun_path`. The socket dir is `$ZELLIJ_SOCKET_DIR`, else the XDG runtime
/// dir, else `$TMPDIR/zellij-<uid>`, always plus `/contract_version_1`
/// (`consts.rs:316-327`); the socket file is `dir.join(name)`
/// (`sessions.rs:148`).
///
/// macOS has no XDG runtime dir, so it takes the `$TMPDIR` branch — and
/// macOS's per-user `$TMPDIR` is the long `/var/folders/<2>/<24>/T/` form.
/// Measured on the maintainer's machine 2026-08-07: the socket dir is 78
/// bytes, so `78 + 1 + len(name) < 104` gives **24**. A Linux XDG runtime dir
/// (`/run/user/1000/contract_version_1`, 33 bytes) allows ~74, so 24 is the
/// tightest of the platforms clave targets and is used everywhere — a
/// per-machine budget would key the same worktree to two different sandboxes
/// depending on where it ran, for no gain.
///
/// This is NOT a limit clave can discover by trying: only `zellij --session`
/// runs the length check (`cli.rs:13-35`, `:53`), and clave attaches with
/// `zellij attach --create`, which does not. An over-long name there reaches
/// `bind()` and fails at the OS.
pub const SESSION_MAX_BYTES: usize = 24;

/// Bytes left for the key once `clave-test-` is spent: 13.
pub const KEY_MAX_BYTES: usize = SESSION_MAX_BYTES - SESSION_PREFIX.len() - 1;

/// How much of an over-long worktree name survives verbatim in front of the
/// disambiguating digest.
const HEAD_BYTES: usize = 8;

/// FNV-1a, 32-bit. Hand-rolled deliberately: `DefaultHasher` is not stable
/// across toolchains (FOOTGUNS, "Rust and codebase specifics"), and this
/// value names a directory that must survive a `rustc` upgrade.
fn fnv1a32(s: &str) -> u32 {
    let mut h: u32 = 0x811c_9dc5;
    for b in s.as_bytes() {
        h ^= u32::from(*b);
        h = h.wrapping_mul(0x0100_0193);
    }
    h
}

/// A worktree directory name reduced to something safe as a zellij session
/// name, a directory name and a socket file name at once.
///
/// Zellij itself validates almost nothing — `validate_session_name` rejects
/// only empty/blank, `.`, `..` and any `/`
/// (`zellij-utils-0.44.3/src/sessions.rs:519-532`), and even that runs only on
/// the new-session path — so spaces, backslashes and control characters are
/// accepted verbatim and then used as a socket filename and a cache directory
/// name. clave does not rely on that latitude: everything outside
/// `[a-z0-9]` becomes `-`, runs collapse, and the ends are trimmed. That also
/// makes the key safe in the shell (`scripts/sandbox-setup.sh` interpolates
/// it) and in KDL.
///
/// Over `KEY_MAX_BYTES`, the name is cut to `HEAD_BYTES` and given a 4-hex
/// digest of the ORIGINAL directory name, so two worktrees sharing a long
/// prefix stay separate instead of silently sharing a sandbox — which is the
/// bug this module exists to remove.
///
/// `None` means nothing usable survived (a name with no ASCII letter or
/// digit). Callers must fail rather than fall back to the shared instance.
pub fn sanitize_key(dir_name: &str) -> Option<String> {
    let mut out = String::new();
    for c in dir_name.chars() {
        if c.is_ascii_alphanumeric() {
            out.push(c.to_ascii_lowercase());
        } else if !out.ends_with('-') {
            out.push('-');
        }
    }
    let trimmed = out.trim_matches('-');
    if trimmed.is_empty() {
        return None;
    }
    if trimmed.len() <= KEY_MAX_BYTES {
        return Some(trimmed.to_string());
    }
    // ASCII by construction above, so byte slicing cannot split a char.
    let head = trimmed[..HEAD_BYTES].trim_end_matches('-');
    Some(format!("{head}-{:04x}", fnv1a32(dir_name) & 0xffff))
}

/// The sandbox key for a working tree: `None` for the repo's MAIN checkout,
/// `Some(key)` for any linked worktree.
///
/// Both paths must already be canonicalized by the caller — a symlinked
/// `/tmp` vs `/private/tmp` mismatch would read a main checkout as a linked
/// worktree and quietly mint a second sandbox for it.
///
/// Errors rather than returning `None` when a linked worktree's name yields
/// no key: `None` means "use the shared main-checkout instance", which is
/// precisely the clobbering this module removes.
pub fn key_for(toplevel: &Path, main_worktree: &Path) -> Result<Option<String>> {
    if toplevel == main_worktree {
        return Ok(None);
    }
    let name = toplevel
        .file_name()
        .and_then(|n| n.to_str())
        .with_context(|| format!("worktree path {} has no directory name", toplevel.display()))?;
    sanitize_key(name).map(Some).with_context(|| {
        format!(
            "worktree directory {name:?} has no ASCII letter or digit, so it cannot key a sandbox \
             — rename the worktree directory"
        )
    })
}

/// The session name a key maps to. The ONE formatter: the reaper joins a
/// root directory back to its session through this, so a second copy of the
/// format string would let the two drift and make every live sandbox read as
/// dead — and a dead one is a reapable one.
pub fn session_name_for(key: &str) -> String {
    format!("{SESSION_PREFIX}-{key}")
}

/// One agent's sandbox: session name and root, both derived from one key.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Sandbox {
    /// `None` is the main checkout's shared instance.
    pub key: Option<String>,
    pub session: String,
    pub root: PathBuf,
    /// The worktree this instance was derived from — `Some` exactly when
    /// `key` is. Written to the root as `origin` so the reaper can ask
    /// whether the worktree still exists.
    pub origin: Option<PathBuf>,
}

impl Sandbox {
    pub fn new(home: &Path, key: Option<String>, origin: Option<PathBuf>) -> Self {
        let (session, dir) = match &key {
            None => (SESSION_PREFIX.to_string(), ROOT_PREFIX.to_string()),
            Some(k) => (session_name_for(k), format!("{ROOT_PREFIX}-{k}")),
        };
        Sandbox {
            key,
            session,
            root: home.join(".local/state").join(dir),
            origin,
        }
    }

    /// This process's instance: the working directory's worktree against its
    /// repo's main tree. Not a git repo at all (the maintainer launching from
    /// his home directory) is the main instance — the historical behaviour.
    pub fn resolve() -> Result<Sandbox> {
        let home = dirs::home_dir().context("no home dir")?;
        let cwd = std::env::current_dir().context("cwd")?;
        let (key, origin) = match worktree_identity(&cwd) {
            Some((toplevel, main)) => {
                let key = key_for(&toplevel, &main)?;
                let origin = key.is_some().then_some(toplevel);
                (key, origin)
            }
            None => (None, None),
        };
        Ok(Sandbox::new(&home, key, origin))
    }

    pub fn state_dir(&self) -> PathBuf {
        self.root.join("state")
    }
    pub fn data_dir(&self) -> PathBuf {
        self.root.join("data")
    }
    pub fn shim_dir(&self) -> PathBuf {
        self.root.join("shim")
    }

    /// Create the root and stamp the origin marker. Idempotent, and a no-op
    /// on the marker for the main instance: `~/.local/state/clave-dev` is the
    /// maintainer's own and is never a reap candidate, so giving it a marker
    /// would only invite one.
    pub fn ensure(&self) -> Result<()> {
        std::fs::create_dir_all(&self.root)
            .with_context(|| format!("creating sandbox root {}", self.root.display()))?;
        let Some(origin) = &self.origin else {
            return Ok(());
        };
        let marker = self.root.join(ORIGIN_FILE);
        let want = format!("{}\n", origin.display());
        if std::fs::read_to_string(&marker).ok().as_deref() != Some(want.as_str()) {
            std::fs::write(&marker, &want)
                .with_context(|| format!("writing {}", marker.display()))?;
        }
        Ok(())
    }
}

/// `(this worktree's toplevel, the repo's main worktree)`, both canonicalized,
/// or `None` when `dir` is not inside a git repo.
///
/// The main tree is the FIRST `git worktree list --porcelain` record, never
/// `rev-parse --show-toplevel` — inside a linked worktree that returns the
/// worktree's own root (FOOTGUNS, "Rust and codebase specifics"), which would
/// make every worktree look like a main checkout and reinstate the singleton.
fn worktree_identity(dir: &Path) -> Option<(PathBuf, PathBuf)> {
    let git = crate::discover::tool_path(crate::discover::ToolId::Git);
    let top = git_stdout(&git, dir, &["rev-parse", "--show-toplevel"])?;
    let porcelain = git_stdout(&git, dir, &["worktree", "list", "--porcelain"])?;
    let worktrees = crate::add::parse_worktrees(&porcelain);
    let main = crate::add::main_worktree_path(&worktrees)?;
    Some((
        std::fs::canonicalize(top.trim()).ok()?,
        std::fs::canonicalize(main).ok()?,
    ))
}

fn git_stdout(git: &Path, dir: &Path, args: &[&str]) -> Option<String> {
    let out = Command::new(git)
        .current_dir(dir)
        .args(args)
        .output()
        .ok()?;
    out.status
        .success()
        .then(|| String::from_utf8_lossy(&out.stdout).into_owned())
}

/// A per-agent sandbox root found on disk.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Found {
    pub root: PathBuf,
    pub key: String,
    /// The worktree named by the root's `origin` marker; `None` when the
    /// marker is absent or unreadable.
    pub origin: Option<PathBuf>,
}

/// What the reaper should do with one root.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verdict {
    /// The worktree is gone and no session is running: delete the root.
    Reap,
    /// The worktree is still there — this is a live agent's sandbox.
    Keep,
    /// The worktree is gone but the session is up. Session lifecycle is the
    /// maintainer's: print the kill command, delete nothing.
    KeepLive,
    /// No `origin` marker, so nothing proves this root is abandoned. Fail
    /// closed: an unprovable answer must never read as a licence to delete.
    KeepUnmarked,
}

/// The key encoded in a sandbox ROOT directory name, or `None` if this
/// directory is not a per-agent root.
///
/// Rejects the bare `clave-dev`, which is the maintainer's own main-checkout
/// sandbox. That exclusion is structural rather than a special case in the
/// reaper's loop: the main root can never even become a candidate.
pub fn key_from_root_name(name: &str) -> Option<&str> {
    name.strip_prefix(ROOT_PREFIX)?
        .strip_prefix('-')
        .filter(|k| !k.is_empty())
}

/// Every per-agent sandbox root under `state_parent` (`~/.local/state`),
/// sorted by key so output and tests are deterministic.
pub fn scan(state_parent: &Path) -> Vec<Found> {
    let mut out = Vec::new();
    let Ok(rd) = std::fs::read_dir(state_parent) else {
        return out;
    };
    for e in rd.flatten() {
        if !e.path().is_dir() {
            continue;
        }
        let name = e.file_name().to_string_lossy().into_owned();
        let Some(key) = key_from_root_name(&name) else {
            continue;
        };
        let origin = std::fs::read_to_string(e.path().join(ORIGIN_FILE))
            .ok()
            .map(|s| PathBuf::from(s.trim()))
            .filter(|p| !p.as_os_str().is_empty());
        out.push(Found {
            root: e.path(),
            key: key.to_string(),
            origin,
        });
    }
    out.sort_by(|a, b| a.key.cmp(&b.key));
    out
}

/// The whole reaping rule, pure over three facts.
pub fn verdict(origin: Option<&Path>, origin_exists: bool, session_live: bool) -> Verdict {
    match origin {
        None => Verdict::KeepUnmarked,
        Some(_) if origin_exists => Verdict::Keep,
        Some(_) if session_live => Verdict::KeepLive,
        Some(_) => Verdict::Reap,
    }
}

/// Classify every found root against one `zellij list-sessions -n` capture.
/// Touches the filesystem only to ask whether an origin still exists.
pub fn plan(found: Vec<Found>, list_output: &str) -> Vec<(Found, Verdict)> {
    found
        .into_iter()
        .map(|f| {
            let live = crate::setup::session_is_live(list_output, &session_name_for(&f.key));
            let exists = f.origin.as_deref().is_some_and(|p| p.is_dir());
            let v = verdict(f.origin.as_deref(), exists, live);
            (f, v)
        })
        .collect()
}

/// Does this classification authorise `remove_dir_all`?
///
/// A one-line predicate with a function of its own because it is the only
/// place in clave that deletes a directory it did not just create, and
/// because inline it was invisible to the tests: a mutation run over the
/// inlined form (2026-08-07, 55 mutants) showed `&&` -> `||`, `==` -> `!=`
/// and a deleted `!` ALL surviving the whole suite — that last pair means a
/// `--dry-run` that deletes, and a `Keep` that deletes, with every test
/// green. Extracted and exhaustively pinned instead.
pub fn should_delete(v: Verdict, dry_run: bool) -> bool {
    v == Verdict::Reap && !dry_run
}

/// One human-readable line per classified root.
pub fn report_line(f: &Found, v: Verdict, dry_run: bool) -> String {
    let origin = f
        .origin
        .as_ref()
        .map(|p| p.display().to_string())
        .unwrap_or_else(|| "<no origin marker>".to_string());
    match v {
        Verdict::Reap if dry_run => format!("  would reap {}  (worktree gone: {origin})", f.key),
        Verdict::Reap => format!("  reaped     {}  (worktree gone: {origin})", f.key),
        Verdict::Keep => format!("  keep       {}  ({origin})", f.key),
        Verdict::KeepLive => format!(
            "  keep       {}  (worktree gone, but {} is LIVE — yours to kill:\n             \
             zellij kill-session {1} && zellij delete-session --force {1})",
            f.key,
            session_name_for(&f.key)
        ),
        Verdict::KeepUnmarked => format!(
            "  keep       {}  (no {ORIGIN_FILE} marker — cannot prove it is abandoned)",
            f.key
        ),
    }
}

/// `clave dev reap`: delete the sandbox roots whose worktree is gone.
///
/// This is a READ of the session list and a delete of directories under
/// `~/.local/state/clave-dev-*`. It never launches or kills a zellij session;
/// a live one is printed for the maintainer to kill and otherwise left alone.
///
/// An UNRUNNABLE zellij is fatal here, where everywhere else in this codebase
/// it degrades to "no live session": this call decides deletions, so a
/// missing binary must not read as "nothing is live". A zellij that runs and
/// exits non-zero is NOT fatal, because that is exactly what it does when
/// there are no sessions at all (`setup::session_is_live`'s empty-string
/// case) — the residual risk is a zellij that fails for some third reason
/// while sessions are up, and the rule's other two conditions bound it: the
/// root still has to be marked, and its worktree still has to be gone.
pub fn run_reap(dry_run: bool) -> Result<()> {
    let home = dirs::home_dir().context("no home dir")?;
    let parent = home.join(".local/state");
    let zellij = crate::discover::tool_path(crate::discover::ToolId::Zellij);
    let out = Command::new(&zellij)
        .args(["list-sessions", "-n"])
        .output()
        .with_context(|| {
            format!(
                "running `{} list-sessions -n` — reaping deletes directories, so it refuses to \
                 run without proof of which sandbox sessions are live",
                zellij.display()
            )
        })?;
    let list = String::from_utf8_lossy(&out.stdout).into_owned();

    let plan = plan(scan(&parent), &list);
    if plan.is_empty() {
        println!("No per-agent sandboxes under {}", parent.display());
        return Ok(());
    }
    println!("Per-agent sandboxes under {}:", parent.display());
    let mut reaped = 0u32;
    for (f, v) in &plan {
        if should_delete(*v, dry_run) {
            std::fs::remove_dir_all(&f.root)
                .with_context(|| format!("removing {}", f.root.display()))?;
            reaped += 1;
        }
        println!("{}", report_line(f, *v, dry_run));
    }
    // Into THIS agent's own sandbox log, never the ambient one: `log_event`
    // re-resolves the state dir from the environment, and `dev reap` runs
    // from a shell where CLAVE_STATE_DIR is unset — which is the maintainer's
    // live `~/.local/state/clave/clave.log` (FOOTGUNS, "Build, test, CI").
    if let Ok(mine) = Sandbox::resolve() {
        crate::evlog::log_event_in(
            &mine.state_dir(),
            "dev reap",
            &format!(
                "{} candidates, {reaped} reaped, dry_run={dry_run}",
                plan.len()
            ),
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    // ---- the key ----

    #[test]
    fn a_short_worktree_name_is_its_own_key() {
        assert_eq!(sanitize_key("prune-wt").as_deref(), Some("prune-wt"));
        assert_eq!(sanitize_key("s112").as_deref(), Some("s112"));
    }

    /// Every character class zellij accepts verbatim and clave refuses to
    /// pass on: spaces, dots, slashes are impossible in a `file_name()` but
    /// the rest are not. The witness is what the sanitizer produces, not
    /// merely that it is non-empty — a pass-through implementation returns
    /// the input unchanged and would fail every line here.
    #[test]
    fn anything_outside_lowercase_alphanumerics_becomes_a_single_hyphen() {
        assert_eq!(sanitize_key("Fix Auth").as_deref(), Some("fix-auth"));
        assert_eq!(sanitize_key("a..b").as_deref(), Some("a-b"));
        assert_eq!(sanitize_key("a\\b").as_deref(), Some("a-b"));
        assert_eq!(sanitize_key("__lead__").as_deref(), Some("lead"));
        assert_eq!(sanitize_key("a  \t b").as_deref(), Some("a-b"));
        assert_eq!(sanitize_key("caf\u{e9}").as_deref(), Some("caf"));
    }

    #[test]
    fn a_name_with_no_letters_or_digits_yields_no_key() {
        assert_eq!(sanitize_key("---"), None);
        assert_eq!(sanitize_key(""), None);
        assert_eq!(sanitize_key("\u{540d}\u{524d}"), None);
    }

    /// The budget, re-derived from zellij's own numbers rather than restated:
    /// socket dir + `/` + name must stay under `ZELLIJ_SOCK_MAX_LENGTH`.
    /// A key at the cap must still produce a runnable session name.
    #[test]
    fn the_longest_key_still_fits_zellijs_socket_path() {
        const MACOS_SOCK_MAX: usize = 104; // consts.rs:306
        const MEASURED_SOCK_DIR: usize = 78; // $TMPDIR/zellij-<uid>/contract_version_1
        let longest = session_name_for(&"k".repeat(KEY_MAX_BYTES));
        assert_eq!(longest.len(), SESSION_MAX_BYTES);
        assert!(
            MEASURED_SOCK_DIR + 1 + longest.len() < MACOS_SOCK_MAX,
            "{longest} would overflow zellij's socket path"
        );
        // And one more byte would not: the cap is tight, not arbitrary.
        assert!(MEASURED_SOCK_DIR + 1 + longest.len() + 1 >= MACOS_SOCK_MAX);
    }

    /// The truncation path. Both witnesses are longer than the cap, and the
    /// rival implementation this distinguishes is a plain truncation: these
    /// two names share their first 13 characters, so a plain
    /// `trimmed[..KEY_MAX_BYTES]` gives them the SAME key — two agents back
    /// on one sandbox, the bug this module removes.
    #[test]
    fn two_long_names_sharing_a_prefix_get_different_keys() {
        let a = sanitize_key("fleet-legibility-rows").unwrap();
        let b = sanitize_key("fleet-legibility-nav").unwrap();
        assert!(a.len() <= KEY_MAX_BYTES, "{a}");
        assert!(b.len() <= KEY_MAX_BYTES, "{b}");
        assert_eq!(&a[..HEAD_BYTES], "fleet-le");
        assert_ne!(a, b, "a prefix truncation would collide here");
    }

    /// The digest is a stable constant of the input, not of the toolchain.
    /// If this ever changes, every existing sandbox root orphans.
    #[test]
    fn the_digest_is_pinned_to_a_literal() {
        assert_eq!(fnv1a32(""), 0x811c_9dc5);
        assert_eq!(fnv1a32("a"), 0xe40c_292c);
        assert_eq!(fnv1a32("foobar"), 0xbf9c_f968);
        assert_eq!(
            sanitize_key("fleet-legibility-rows").as_deref(),
            Some("fleet-le-ec52")
        );
    }

    #[test]
    fn a_truncated_key_never_ends_in_the_hyphen_it_cut_on() {
        // "sandbox" is 7 chars, so the 8-char head ends on the separator.
        let k = sanitize_key("sandbox-segregation").unwrap();
        assert!(k.starts_with("sandbox-"), "{k}");
        assert!(!k.starts_with("sandbox--"), "{k}");
    }

    // ---- main checkout vs linked worktree ----

    #[test]
    fn the_main_checkout_keeps_the_shared_name_and_root() {
        let main = Path::new("/Users/o/code/clave");
        assert_eq!(key_for(main, main).unwrap(), None);
        let sb = Sandbox::new(Path::new("/Users/o"), None, None);
        assert_eq!(sb.session, "clave-test");
        assert_eq!(sb.root, Path::new("/Users/o/.local/state/clave-dev"));
        assert_eq!(
            sb.state_dir(),
            Path::new("/Users/o/.local/state/clave-dev/state")
        );
        assert_eq!(
            sb.data_dir(),
            Path::new("/Users/o/.local/state/clave-dev/data")
        );
        assert_eq!(
            sb.shim_dir(),
            Path::new("/Users/o/.local/state/clave-dev/shim")
        );
    }

    #[test]
    fn a_linked_worktree_gets_its_own_session_and_root() {
        let main = Path::new("/Users/o/code/clave");
        let wt = Path::new("/Users/o/code/clave/.claude/worktrees/prune-wt");
        let key = key_for(wt, main).unwrap();
        assert_eq!(key.as_deref(), Some("prune-wt"));
        let sb = Sandbox::new(Path::new("/Users/o"), key, Some(wt.to_path_buf()));
        assert_eq!(sb.session, "clave-test-prune-wt");
        assert_eq!(
            sb.root,
            Path::new("/Users/o/.local/state/clave-dev-prune-wt")
        );
        // All four surfaces move together — the singleton was the shared
        // ROOT, not just the shared session name.
        assert_eq!(
            sb.state_dir(),
            Path::new("/Users/o/.local/state/clave-dev-prune-wt/state")
        );
        assert_eq!(
            sb.data_dir(),
            Path::new("/Users/o/.local/state/clave-dev-prune-wt/data")
        );
        assert_eq!(
            sb.shim_dir(),
            Path::new("/Users/o/.local/state/clave-dev-prune-wt/shim")
        );
    }

    #[test]
    fn two_worktrees_never_share_a_surface() {
        let main = Path::new("/r");
        let home = Path::new("/h");
        let a = Sandbox::new(home, key_for(Path::new("/r/wt/alpha"), main).unwrap(), None);
        let b = Sandbox::new(home, key_for(Path::new("/r/wt/beta"), main).unwrap(), None);
        assert_ne!(a.session, b.session);
        assert_ne!(a.root, b.root);
        assert_ne!(a.state_dir(), b.state_dir());
        assert_ne!(a.data_dir(), b.data_dir());
        assert_ne!(a.shim_dir(), b.shim_dir());
    }

    #[test]
    fn an_unnameable_worktree_errors_instead_of_sharing_the_main_sandbox() {
        let e = key_for(Path::new("/r/wt/\u{540d}\u{524d}"), Path::new("/r")).unwrap_err();
        assert!(
            e.to_string().contains("rename the worktree directory"),
            "{e}"
        );
    }

    #[test]
    fn ensure_stamps_the_origin_marker_and_is_idempotent() {
        let home = tempfile::tempdir().unwrap();
        let wt = home.path().join("code/clave/.claude/worktrees/wt-a");
        let sb = Sandbox::new(home.path(), Some("wt-a".into()), Some(wt.clone()));
        sb.ensure().unwrap();
        let marker = sb.root.join(ORIGIN_FILE);
        assert_eq!(
            std::fs::read_to_string(&marker).unwrap().trim(),
            wt.to_str().unwrap()
        );
        sb.ensure().unwrap();
        assert_eq!(
            std::fs::read_to_string(&marker).unwrap().trim(),
            wt.to_str().unwrap()
        );
    }

    #[test]
    fn the_main_instance_is_never_given_an_origin_marker() {
        let home = tempfile::tempdir().unwrap();
        let sb = Sandbox::new(home.path(), None, None);
        sb.ensure().unwrap();
        assert!(sb.root.is_dir());
        assert!(!sb.root.join(ORIGIN_FILE).exists());
    }

    // ---- the reaper ----

    #[test]
    fn the_main_sandbox_root_is_not_a_reap_candidate() {
        assert_eq!(key_from_root_name("clave-dev"), None);
        assert_eq!(key_from_root_name("clave-dev-"), None);
        assert_eq!(key_from_root_name("clave"), None);
        assert_eq!(key_from_root_name("clave-devil"), None);
        assert_eq!(key_from_root_name("clave-dev-wt-a"), Some("wt-a"));
    }

    #[test]
    fn the_rule_keeps_unless_the_worktree_is_gone_and_the_session_is_down() {
        let o = Path::new("/wt");
        assert_eq!(verdict(Some(o), true, false), Verdict::Keep);
        assert_eq!(verdict(Some(o), true, true), Verdict::Keep);
        assert_eq!(verdict(Some(o), false, true), Verdict::KeepLive);
        assert_eq!(verdict(Some(o), false, false), Verdict::Reap);
        assert_eq!(verdict(None, false, false), Verdict::KeepUnmarked);
    }

    /// The join the reaper depends on: root directory name -> key -> session
    /// name -> the `zellij list-sessions -n` line. Get any hop wrong and a
    /// LIVE sandbox reads as dead, which is a delete.
    #[test]
    fn plan_joins_a_root_to_its_live_session_by_name() {
        let home = tempfile::tempdir().unwrap();
        let state = home.path().join(".local/state");
        let gone = state.join("clave-dev-gone");
        let alive = state.join("clave-dev-alive");
        let kept = state.join("clave-dev-kept");
        let unmarked = state.join("clave-dev-nomark");
        let real_wt = home.path().join("wt-kept");
        std::fs::create_dir_all(&real_wt).unwrap();
        for (d, o) in [
            (&gone, Some("/vanished/wt-gone")),
            (&alive, Some("/vanished/wt-alive")),
            (&kept, real_wt.to_str()),
            (&unmarked, None),
        ] {
            std::fs::create_dir_all(d).unwrap();
            if let Some(o) = o {
                std::fs::write(d.join(ORIGIN_FILE), format!("{o}\n")).unwrap();
            }
        }
        // A file, not a directory, and the maintainer's own root: neither is
        // a candidate.
        std::fs::create_dir_all(state.join("clave-dev")).unwrap();
        std::fs::write(state.join("clave-dev-afile"), "x").unwrap();

        let list = "clave [Created 2h ago]\nclave-test-alive [Created 1m ago]\n";
        let got: Vec<(String, Verdict)> = plan(scan(&state), list)
            .into_iter()
            .map(|(f, v)| (f.key, v))
            .collect();
        assert_eq!(
            got,
            vec![
                ("alive".to_string(), Verdict::KeepLive),
                ("gone".to_string(), Verdict::Reap),
                ("kept".to_string(), Verdict::Keep),
                ("nomark".to_string(), Verdict::KeepUnmarked),
            ]
        );
    }

    #[test]
    fn an_exited_session_does_not_protect_a_dead_worktrees_sandbox() {
        let home = tempfile::tempdir().unwrap();
        let state = home.path().join(".local/state");
        let d = state.join("clave-dev-gone");
        std::fs::create_dir_all(&d).unwrap();
        std::fs::write(d.join(ORIGIN_FILE), "/vanished/wt\n").unwrap();
        let exited = "clave-test-gone [Created 3h ago] (EXITED - attach to resurrect)\n";
        assert_eq!(plan(scan(&state), exited)[0].1, Verdict::Reap);
    }

    #[test]
    fn a_live_sandbox_is_reported_with_the_kill_command_and_never_deleted() {
        let f = Found {
            root: PathBuf::from("/h/.local/state/clave-dev-wt-a"),
            key: "wt-a".into(),
            origin: Some(PathBuf::from("/gone")),
        };
        let line = report_line(&f, Verdict::KeepLive, false);
        assert!(
            line.contains("zellij kill-session clave-test-wt-a"),
            "{line}"
        );
        assert!(
            line.contains("zellij delete-session --force clave-test-wt-a"),
            "{line}"
        );
    }

    /// Exhaustive over every (verdict, dry-run) pair, because this is the
    /// only predicate in clave that authorises deleting a directory. The
    /// three rivals it distinguishes are the three mutations that used to
    /// survive when it was inlined: `||` (a dry run deletes), `!=` (every
    /// KEEP deletes) and a dropped `!` (only a dry run deletes).
    #[test]
    fn only_a_wet_reap_ever_deletes_anything() {
        use Verdict::*;
        for v in [Reap, Keep, KeepLive, KeepUnmarked] {
            assert!(!should_delete(v, true), "{v:?} deleted during a dry run");
        }
        assert!(should_delete(Reap, false));
        for v in [Keep, KeepLive, KeepUnmarked] {
            assert!(!should_delete(v, false), "{v:?} authorised a delete");
        }
    }

    #[test]
    fn a_dry_run_line_says_would_and_a_real_one_does_not() {
        let f = Found {
            root: PathBuf::from("/h/.local/state/clave-dev-wt-a"),
            key: "wt-a".into(),
            origin: Some(PathBuf::from("/gone")),
        };
        assert!(report_line(&f, Verdict::Reap, true).contains("would reap"));
        assert!(!report_line(&f, Verdict::Reap, false).contains("would reap"));
    }
}
