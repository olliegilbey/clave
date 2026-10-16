//! `scripts/ct.sh` must refuse when the only live server is ANOTHER session's.
//!
//! Before it lets a `zellij action` through, ct.sh proves that a live server
//! holds this sandbox's socket (its step 2). That match was a bare substring.
//! So a stale `clave-test` socket passed while only a `clave-test-<key>`
//! server lived, and the action went to whatever zellij then picked (PR #282
//! review). These tests run ct.sh against a fake socket folder, a fake server
//! process and a fake `zellij` first on PATH. No real zellij runs, and no
//! session is touched.

use std::os::unix::fs::PermissionsExt;
use std::os::unix::net::UnixListener;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Output};

struct Fixture {
    root: PathBuf,
    _socket: UnixListener,
    server: Child,
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = self.server.kill();
        let _ = self.server.wait();
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

/// Write an executable script, and never hold its write fd in this process.
///
/// The tests here run as threads of one process. A spawn copies every open fd
/// into the child, and the child keeps the copies until its own exec (std
/// spawns with `clone3(CLONE_VM|CLONE_VFORK)` and no `CLONE_FILES`; clone(2)).
/// While a child holds a write fd on a script, each exec of the script fails
/// with "Text file busy" (ETXTBSY, execve(2)): here, and in ct.sh too. With
/// `std::fs::write` here, the fake server's spawn failed 8 runs in 1600
/// (FOOTGUNS, "Text file busy"). A rename does not help, because the fd holds
/// the inode and not the name. So `sh` writes the file. It holds the only
/// write fd, and it closes that fd before it exits.
fn write_exe(path: &Path, body: &str) {
    let written = Command::new("sh")
        .args(["-c", r#"printf '%s' "$2" > "$1""#, "sh"])
        .arg(path)
        .arg(body)
        .status()
        .unwrap();
    assert!(written.success(), "sh could not write {}", path.display());
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755)).unwrap();
}

/// A socket for `session`, and one live fake server for `server_session`.
fn fixture(tag: &str, session: &str, server_session: &str) -> Fixture {
    // `/tmp`, not `temp_dir()`: macOS `$TMPDIR` is long, and a unix socket
    // path must fit in 104 bytes there.
    let root = PathBuf::from(format!("/tmp/ct-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let bin = root.join("bin");
    let sockets = root.join("sock/contract_version_1");
    std::fs::create_dir_all(&bin).unwrap();
    std::fs::create_dir_all(&sockets).unwrap();
    // As a server the fake idles; as a client it prints its argv and exits.
    write_exe(
        &bin.join("zellij"),
        "#!/bin/sh\nif [ \"$1\" = --server ]; then while :; do sleep 1; done; fi\necho \"FAKE-ZELLIJ $*\"\n",
    );
    // ct.sh asks this binary which session the checkout owns.
    write_exe(&bin.join("clave"), &format!("#!/bin/sh\necho {session}\n"));
    let socket = UnixListener::bind(sockets.join(session)).unwrap();
    // `spawn` returns after the exec, so the argv is in the process table
    // before ct.sh looks.
    let server = Command::new(bin.join("zellij"))
        .arg("--server")
        .arg(sockets.join(server_session))
        .spawn()
        .unwrap();
    Fixture {
        root,
        _socket: socket,
        server,
    }
}

fn run_ct(f: &Fixture) -> Output {
    let ct = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../scripts/ct.sh");
    Command::new("bash")
        .arg(ct)
        .arg("dump-layout")
        .env_clear()
        // The fake zellij comes first, so a ct.sh that lets the call through
        // reaches the fake and nothing else.
        .env(
            "PATH",
            format!("{}:/usr/bin:/bin", f.root.join("bin").display()),
        )
        .env("CLAVE_BIN", f.root.join("bin/clave"))
        .env("ZELLIJ_SOCKET_DIR", f.root.join("sock"))
        .output()
        .unwrap()
}

#[test]
fn a_stale_socket_beside_a_longer_named_server_is_refused() {
    let f = fixture("refuse", "qa-sess", "qa-sess-longer");
    let out = run_ct(&f);
    let stdout = String::from_utf8_lossy(&out.stdout);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        !out.status.success() && !stdout.contains("FAKE-ZELLIJ"),
        "ct.sh let an action through with no server for its own session:\n{stdout}{stderr}"
    );
    assert!(
        stderr.contains("no 'zellij --server"),
        "refused for another reason, so the test proves nothing:\n{stderr}"
    );
}

#[test]
fn the_sessions_own_server_lets_the_action_through() {
    // The control: without it, a ct.sh that refused everything would pass
    // the test above.
    let f = fixture("accept", "qa-sess", "qa-sess");
    let out = run_ct(&f);
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        out.status.success() && stdout.contains("FAKE-ZELLIJ --session qa-sess action dump-layout"),
        "ct.sh refused a live sandbox:\n{stdout}{}",
        String::from_utf8_lossy(&out.stderr)
    );
}
