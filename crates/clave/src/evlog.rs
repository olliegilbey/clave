//! §6.9 observability: every clave CLI invocation appends ONE JSON line —
//! timestamp, command, decision — to `<state>/clave.log`. This is the log
//! Claude reads after each user-driven validation step. Best-effort by
//! design: a logging failure must never break a spawn/open/hook (same
//! zero-risk stance as `clave hook`, §6.5).

use std::io::Write;

/// Append one event line to the AMBIENT state dir (`store_paths()`). For the
/// CLI entry points, which resolve their store the same way. Swallows all
/// errors (stderr note only).
pub fn log_event(cmd: &str, detail: &str) {
    match crate::store::store_paths() {
        Ok(paths) => log_event_in(&paths.dir, cmd, detail),
        Err(e) => eprintln!("clave evlog: {e:#}"),
    }
}

/// Append one event line beside a SPECIFIC store. Callers already holding a
/// `StorePaths` must use this: re-resolving the ambient dir would send the
/// sandbox's events to the stable log (and, in unit tests, would append to the
/// maintainer's real `~/.local/state/clave/clave.log` — which is exactly what
/// happened when `apply_bind` first grew its eviction line).
pub fn log_event_in(dir: &std::path::Path, cmd: &str, detail: &str) {
    if let Err(e) = try_log(dir, cmd, detail) {
        eprintln!("clave evlog: {e:#}");
    }
}

fn try_log(dir: &std::path::Path, cmd: &str, detail: &str) -> anyhow::Result<()> {
    std::fs::create_dir_all(dir)?;
    let line = serde_json::json!({
        "ts": crate::store::now_unix(),
        "cmd": cmd,
        "detail": detail,
    });
    // ONE buffer, ONE write. O_APPEND makes a single `write` atomic against
    // other appenders; it does nothing for a SEQUENCE of them, and
    // `writeln!(f, "{line}")` on an unbuffered File emits a syscall per token
    // the Display impl produces. Two clave processes logging in the same
    // instant — `clave open` and the spawn it runs — then shredded each other
    // into `{{""tsts""::…`, and the QA drive's open-counter read zero opens
    // that had plainly run (drive run, 2026-09-07, phase 3 churn C).
    let mut line = line.to_string();
    line.push('\n');
    let mut f = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(dir.join("clave.log"))?;
    f.write_all(line.as_bytes())?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn log_lines_are_json_with_ts_cmd_detail() {
        // The real WRITE path, now that it takes its directory as an argument
        // rather than re-resolving the ambient one. Before that it could only
        // be shape-tested through the serializer.
        let d = tempfile::tempdir().unwrap();
        log_event_in(d.path(), "open", "d");
        let body = std::fs::read_to_string(d.path().join("clave.log")).unwrap();
        let v: serde_json::Value = serde_json::from_str(body.trim()).unwrap();
        assert_eq!(v["cmd"], "open");
        assert_eq!(v["detail"], "d");
        assert!(v["ts"].is_u64());
    }

    #[test]
    fn concurrent_writers_never_interleave_a_line() {
        // The drive's phase-3 witness went red on a mangled line: `clave open`
        // and the spawn it runs both append at the same instant, and the log
        // came back as `{{""tsts""::…` — two records shredded into each other,
        // so `grep '"cmd":"open"'` counted zero opens that had plainly run.
        // O_APPEND makes ONE write atomic; it does not make a sequence of them
        // atomic, and a `writeln!` of a Display type emits a syscall per token.
        // Every line must parse, and every line must be one of ours.
        let d = tempfile::tempdir().unwrap();
        let dir = d.path().to_path_buf();
        let writers: Vec<_> = (0..8)
            .map(|w| {
                let dir = dir.clone();
                std::thread::spawn(move || {
                    for i in 0..40 {
                        log_event_in(&dir, "open", &format!("writer {w} line {i}"));
                    }
                })
            })
            .collect();
        for w in writers {
            w.join().unwrap();
        }
        let body = std::fs::read_to_string(dir.join("clave.log")).unwrap();
        assert_eq!(body.lines().count(), 8 * 40, "no line was lost");
        for (n, line) in body.lines().enumerate() {
            let v: serde_json::Value = serde_json::from_str(line)
                .unwrap_or_else(|e| panic!("line {n} is not JSON ({e}): {line}"));
            assert_eq!(v["cmd"], "open", "line {n} carries a shredded cmd");
        }
    }

    #[test]
    fn lines_append_and_the_directory_is_created_on_demand() {
        let d = tempfile::tempdir().unwrap();
        let nested = d.path().join("state");
        log_event_in(&nested, "a", "1");
        log_event_in(&nested, "b", "2");
        let body = std::fs::read_to_string(nested.join("clave.log")).unwrap();
        assert_eq!(body.lines().count(), 2);
    }
}
