//! Exercise the actual status/tell commands; only the external tmux boundary is replaced.

use serde_json::{Value, json};
use std::{fs, os::unix::fs::PermissionsExt, process::Command};

const WELCOME: &str = "  >_ OpenAI Codex (v0.169.0)\n  To get started, describe a task or try one of these commands:\n\n› Ask Codex to do anything\n  GPT-6.1-Sol max\n  ← for agents · ? for shortcuts\n";

#[test]
fn only_a_fresh_codex_at_its_empty_welcome_prompt_accepts_the_first_message() {
    let root = std::env::temp_dir().join(format!("shifu-bridge-startup-{}", std::process::id()));
    fs::create_dir_all(&root).unwrap();
    let tmux = root.join("tmux");
    fs::write(&tmux, r#"#!/bin/sh
case " $* " in
  *" list-panes "*)
    case " $* " in
      *pane_dead*) printf '%%1 %s @1 0\n' "$SHIFU_TEST_DEAD" ;;
      *) printf '%%1 Codex\n' ;;
    esac ;;
  *" capture-pane "*)
    [ "$SHIFU_TEST_CAPTURE_FAIL" = 1 ] && exit 1
    cat "$SHIFU_HOME/screen.txt" ;;
  *" set-buffer "*|*" paste-buffer "*|*" send-keys "*)
    printf '%s\n' "$*" >> "$SHIFU_HOME/sent.txt" ;;
esac
"#).unwrap();
    fs::set_permissions(&tmux, fs::Permissions::from_mode(0o755)).unwrap();
    for (case, kind, status, screen, dead, capture_fail, expected) in [
        ("ready", "codex", "starting", WELCOME.to_string(), false, false, "idle"),
        ("draft", "codex", "starting", WELCOME.replace("Ask Codex to do anything", "unfinished user draft"), false, false, "starting"),
        ("trust", "codex", "starting", format!("{WELCOME}\nDo you trust the files in this folder?\nEnter to confirm"), false, false, "starting"),
        ("approval", "codex", "starting", format!("{WELCOME}\nWould you like to run this command?\n› 1. Yes"), false, false, "starting"),
        ("busy", "codex", "starting", format!("{WELCOME}\nesc to interrupt"), false, false, "starting"),
        ("running", "codex", "running", WELCOME.to_string(), false, false, "running"),
        ("other-kind", "codebuddy", "starting", WELCOME.to_string(), false, false, "starting"),
        ("dead", "codex", "starting", WELCOME.to_string(), true, false, "exited"),
        ("unknown", "codex", "starting", "Loading…".into(), false, false, "starting"),
        ("unreadable", "codex", "starting", WELCOME.to_string(), false, true, "starting"),
    ] {
        let home = root.join(case);
        fs::create_dir_all(home.join("agents")).unwrap();
        let record = json!({"name":"codex-daily", "kind":kind, "command":kind, "cwd":"/test/daily",
            "created_at":100, "status":status, "status_since":100, "window_id":"@1", "pane_id":"%1",
            "session_id":null, "last_message":null});
        let record_text = serde_json::to_string(&record).unwrap();
        fs::write(home.join("agents/codex-daily.json"), &record_text).unwrap();
        fs::write(home.join("screen.txt"), screen).unwrap();
        let cli = || {
            let mut cmd = Command::new(env!("CARGO_BIN_EXE_shifu"));
            cmd.env("SHIFU_HOME", &home).env("SHIFU_SOCKET", "isolated-test").env("SHIFU_TMUX", &tmux)
                .env("SHIFU_TEST_DEAD", if dead { "1" } else { "0" })
                .env("SHIFU_TEST_CAPTURE_FAIL", if capture_fail { "1" } else { "0" });
            cmd
        };
        let out = cli().args(["status", "--json"]).output().unwrap();
        assert!(out.status.success(), "{case}: {:?}", out);
        let rows: Vec<Value> = serde_json::from_slice(&out.stdout).unwrap();
        assert_eq!(rows[0]["status"], expected, "{case}");
        let sent = cli().args(["tell", "codex-daily", "--from", "test", "start analysis"]).output().unwrap();
        assert_eq!(sent.status.success(), expected == "idle", "{case}: {:?}", sent);
        assert_eq!(home.join("sent.txt").exists(), expected == "idle", "{case}");
        assert_eq!(fs::read_to_string(home.join("agents/codex-daily.json")).unwrap(), record_text);
    }
    fs::remove_dir_all(root).unwrap();
}
#[test]
fn listing_and_status_finish_quietly_when_the_pipe_reader_leaves() {
    use std::os::fd::OwnedFd;
    use std::os::unix::net::UnixStream;

    let root = std::env::temp_dir().join(format!("shifu-closed-report-{}", std::process::id()));
    std::fs::create_dir(&root).unwrap();
    for args in [&["ls"][..], &["status"], &["status", "--json"]] {
        let (reader, writer) = UnixStream::pair().unwrap();
        drop(reader);
        let result = std::process::Command::new(env!("CARGO_BIN_EXE_shifu")).args(args)
            .env("SHIFU_HOME", &root).env("SHIFU_TMUX", "/usr/bin/false")
            .stdout(OwnedFd::from(writer)).output().unwrap();
        assert!(result.status.success(), "{args:?}: {}", String::from_utf8_lossy(&result.stderr));
        assert!(result.stderr.is_empty());
    }
    std::fs::remove_dir(root).unwrap();
}
