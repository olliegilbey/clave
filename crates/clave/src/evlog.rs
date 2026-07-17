//! §6.9 observability: every clave CLI invocation appends ONE JSON line —
//! timestamp, command, decision — to `<state>/clave.log`. This is the log
//! Claude reads after each user-driven validation step. Best-effort by
//! design: a logging failure must never break a spawn/open/hook (same
//! zero-risk stance as `clave hook`, §6.5).

use std::io::Write;

/// Append one event line. Swallows all errors (stderr note only).
pub fn log_event(cmd: &str, detail: &str) {
    if let Err(e) = try_log(cmd, detail) {
        eprintln!("clave evlog: {e:#}");
    }
}

fn try_log(cmd: &str, detail: &str) -> anyhow::Result<()> {
    let paths = crate::store::store_paths()?;
    std::fs::create_dir_all(&paths.dir)?;
    let line = serde_json::json!({
        "ts": crate::store::now_unix(),
        "cmd": cmd,
        "detail": detail,
    });
    let mut f = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(paths.dir.join("clave.log"))?;
    writeln!(f, "{line}")?;
    Ok(())
}

#[cfg(test)]
mod tests {
    #[test]
    fn log_lines_are_json_with_ts_cmd_detail() {
        // Shape-only test through the serializer (try_log's path comes from
        // store_paths(), which tests can't redirect without process-global
        // env — the WRITE path is exercised live by every C8 scenario).
        let line = serde_json::json!({"ts": 1u64, "cmd": "open", "detail": "d"});
        let v: serde_json::Value = serde_json::from_str(&line.to_string()).unwrap();
        assert_eq!(v["cmd"], "open");
        assert!(v["ts"].is_u64());
    }
}
