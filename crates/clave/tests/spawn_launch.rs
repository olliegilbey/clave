#![cfg(unix)]

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;

fn write_capture_executable(path: &Path, launcher: &str) {
    let script = format!(
        "#!/bin/sh\n\
         {{\n\
           printf 'launcher=%s\\n' '{launcher}'\n\
           printf 'cwd=%s\\n' \"$PWD\"\n\
           printf 'child_claude=%s\\n' \"${{CLAVE_CLAUDE_BIN-}}\"\n\
           for arg in \"$@\"; do printf 'arg=%s\\n' \"$arg\"; done\n\
         }} > \"$CLAVE_CAPTURE\"\n"
    );
    fs::write(path, script).unwrap();
    let mut permissions = fs::metadata(path).unwrap().permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(path, permissions).unwrap();
}

fn transcript_path(config: &Path, cwd: &Path, uuid: &str) -> PathBuf {
    config
        .join("projects")
        .join(clave::munge::munge_cwd(cwd.to_str().unwrap()))
        .join(format!("{uuid}.jsonl"))
}

#[test]
fn spawn_executes_selected_launcher_with_exact_argv_cwd_and_environment() {
    struct Case {
        codex: bool,
        resume: bool,
        launcher: &'static str,
        args: &'static [&'static str],
    }

    let cases = [
        Case {
            codex: false,
            resume: false,
            launcher: "claude",
            args: &["--session-id", "session-u", "--name", "name ; $(false)"],
        },
        Case {
            codex: false,
            resume: true,
            launcher: "claude",
            args: &["--resume", "session-u"],
        },
        Case {
            codex: true,
            resume: false,
            launcher: "claude-codex",
            args: &["--session-id", "session-u", "--name", "name ; $(false)"],
        },
        Case {
            codex: true,
            resume: true,
            launcher: "claude-codex",
            args: &["--resume", "session-u"],
        },
    ];

    for (index, case) in cases.iter().enumerate() {
        let temp = tempfile::tempdir().unwrap();
        let path_bin = temp.path().join("path-bin");
        let selected_bin = temp.path().join("selected-bin");
        let cwd = temp.path().join("repo with spaces");
        let config = temp.path().join("claude-config");
        let home = temp.path().join("home");
        let xdg_config = temp.path().join("xdg-config");
        let capture = temp.path().join(format!("capture-{index}"));
        for dir in [&path_bin, &selected_bin, &cwd, &config, &home, &xdg_config] {
            fs::create_dir_all(dir).unwrap();
        }
        let physical_cwd = fs::canonicalize(&cwd).unwrap();

        // The selected executables are intentionally outside PATH: a regression
        // to bare-name execution must hit these unmistakable decoys instead.
        write_capture_executable(&path_bin.join("claude"), "PATH-DECOY-claude");
        write_capture_executable(&path_bin.join("claude-codex"), "PATH-DECOY-claude-codex");
        let claude = selected_bin.join("claude");
        let wrapper = selected_bin.join("claude-codex");
        write_capture_executable(&claude, "claude");
        write_capture_executable(&wrapper, "claude-codex");

        if case.resume {
            let transcript = transcript_path(&config, &physical_cwd, "session-u");
            fs::create_dir_all(transcript.parent().unwrap()).unwrap();
            fs::write(transcript, "{}\n").unwrap();
        }

        let mut command = Command::new(env!("CARGO_BIN_EXE_clave"));
        command
            .args([
                "spawn",
                "session-u",
                "--name",
                "name ; $(false)",
                "--cwd",
                cwd.to_str().unwrap(),
            ])
            .env("PATH", &path_bin)
            .env("HOME", &home)
            .env("XDG_CONFIG_HOME", &xdg_config)
            .env("CLAUDE_CONFIG_DIR", &config)
            .env("CLAVE_STATE_DIR", temp.path().join("state"))
            .env("CLAVE_CAPTURE", &capture)
            .env("CLAVE_CLAUDE_BIN", &claude)
            .env("CLAVE_CLAUDE_CODEX_BIN", &wrapper)
            .env_remove("ZELLIJ_PANE_ID");
        if case.codex {
            command.arg("--claude-codex");
        }

        let status = command.status().unwrap();
        assert!(status.success(), "case {index} failed");

        let output = fs::read_to_string(&capture).unwrap();
        let lines: Vec<_> = output.lines().collect();
        assert_eq!(lines[0], format!("launcher={}", case.launcher));
        assert_eq!(lines[1], format!("cwd={}", physical_cwd.display()));
        let expected_child_claude = if case.codex {
            claude.display().to_string()
        } else {
            String::new()
        };
        assert_eq!(lines[2], format!("child_claude={expected_child_claude}"));
        let captured_args: Vec<_> = lines[3..]
            .iter()
            .map(|line| line.strip_prefix("arg=").unwrap())
            .collect();
        assert_eq!(captured_args, case.args.to_vec());
        assert!(!temp.path().join("false").exists());
    }
}
