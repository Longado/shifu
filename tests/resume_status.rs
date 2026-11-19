//! Restore the real agent record through the shared lifecycle, with only tmux replaced.

use shifu::{agent::{self, Agent, Kind}, app_session::{self, Config}, status::Status};
use std::{fs, io::Write, os::unix::fs::PermissionsExt, path::Path, process::{Command, Stdio}};

const NAME: &str = "codex-restore";

fn hook(home: &Path, event: &str) {
    let mut child = Command::new(env!("CARGO_BIN_EXE_shifu"))
        .args(["hook", "codex", event])
        .env("SHIFU_HOME", home).env("SHIFU_ID", NAME)
        .env_remove("TMUX").env_remove("TMUX_PANE")
        .stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped())
        .spawn().unwrap();
    child.stdin.take().unwrap().write_all(br#"{"session_id":"saved-conversation","tool_name":"Bash","last_assistant_message":"Finished"}"#).unwrap();
    let out = child.wait_with_output().unwrap();
    assert!(out.status.success());
    assert!(out.stdout.is_empty());
    assert!(out.stderr.is_empty());
}

fn fixture(scenario: &str) {
    let home = agent::shifu_home();
    fs::create_dir_all(home.join("agents")).unwrap();
    let record = Agent {
        name: NAME.into(), kind: if scenario == "claude" { Kind::Claude } else { Kind::Codex },
        command: if scenario == "claude" { "claude" } else { "codex" }.into(),
        cwd: home.to_string_lossy().into(), created_at: 100,
        status: Status::Exited, status_since: 100,
        pane_id: Some("%old".into()), window_id: Some("@old".into()),
        session_id: if scenario == "fresh" { None } else { Some("saved-conversation".into()) },
        last_message: Some("Previous work finished".into()),
        waiting_for: None,
        model: Some("test-model".into()), effort: Some("high".into()), label: Some("Keep my label".into()),
    };
    agent::save(&record).unwrap();
    let config = Config {
        home: home.clone(), socket: "isolated-test".into(),
        shifu_bin: env!("CARGO_BIN_EXE_shifu").into(), user_statusline: None,
    };
    let outcome = app_session::resume_agent(&config, NAME);
    if scenario == "active" {
        assert!(outcome.unwrap_err().contains("still running"));
        assert_eq!(agent::load(NAME).unwrap().unwrap(), record);
        assert!(!home.join("launch.txt").exists());
        return;
    }
    if scenario.starts_with("fail-") {
        assert!(outcome.is_err());
        let failed = agent::load(NAME).unwrap().unwrap();
        assert_eq!(failed.effective_status(&Default::default()), Status::Exited);
        assert_eq!(failed.session_id, record.session_id);
        assert_eq!(failed.last_message, record.last_message);
        return;
    }
    outcome.unwrap();
    let restored = agent::load(NAME).unwrap().unwrap();
    let expected = match scenario {
        "prompt-new" | "prompt-dead" => Status::Running,
        "fresh" | "claude" => Status::Starting,
        _ => Status::Idle,
    };
    assert_eq!(restored.status, expected, "restore without SessionStart: {scenario}");
    assert!(restored.status_since > record.status_since);
    assert_eq!(restored.session_id, record.session_id);
    assert_eq!(restored.last_message, record.last_message);
    assert_eq!(restored.created_at, record.created_at);
    assert_eq!(restored.label, record.label);
    assert_eq!(restored.model, record.model);
    assert_eq!(restored.effort, record.effort);
    assert_eq!(restored.pane_id.as_deref(), Some(if scenario.ends_with("dead") { "%old" } else { "%new" }));
    let launch = fs::read_to_string(home.join("launch.txt")).unwrap();
    if scenario != "fresh" { assert!(launch.contains("saved-conversation")); }
    assert!(launch.contains("test-model"));

    if scenario == "claude" { return; }
    // A missed startup/prompt callback must not mask actual tool activity either.
    hook(&home, "PostToolUse");
    assert_eq!(agent::load(NAME).unwrap().unwrap().status, Status::Running);
    hook(&home, "Stop");
    let completed = agent::load(NAME).unwrap().unwrap();
    assert_eq!(completed.status, Status::NeedsReply);
    assert_eq!(completed.last_message.as_deref(), Some("Finished"));
    hook(&home, "SessionStart");
    assert_eq!(agent::load(NAME).unwrap().unwrap().status, Status::NeedsReply);
    hook(&home, "UserPromptSubmit");
    assert_eq!(agent::load(NAME).unwrap().unwrap().status, Status::Running);
    hook(&home, "SessionStart");
    assert_eq!(agent::load(NAME).unwrap().unwrap().status, Status::Running);
}

#[test]
fn restored_sessions_and_activity_without_startup_callbacks() {
    // Re-exec with isolated environment instead of changing the test runner's environment.
    if let Ok(scenario) = std::env::var("SHIFU_TEST_RESTORE") {
        fixture(&scenario);
        return;
    }
    let root = std::env::temp_dir().join(format!("shifu-restore-status-{}", std::process::id()));
    fs::create_dir_all(&root).unwrap();
    let tmux = root.join("tmux");
    fs::write(&tmux, r#"#!/bin/sh
for last do :; done
case " $* " in
  *" list-panes "*)
    case "$SHIFU_TEST_RESTORE" in
      *dead) printf '%%old 1 @old 0\n';;
      active) printf '%%old 0 @old 0\n';;
    esac
    ;;
  *" new-session "*|*" respawn-pane "*)
    printf '%s' "$last" > "$SHIFU_HOME/launch.txt"
    case "$SHIFU_TEST_RESTORE" in
      prompt-*) printf '{"session_id":"saved-conversation"}' | "$SHIFU_TEST_CLI" hook codex UserPromptSubmit ;;
      fail-*) exit 1 ;;
    esac
    case " $* " in *" new-session "*) printf '%%new @new\n';; esac
    ;;
esac
"#).unwrap();
    fs::set_permissions(&tmux, fs::Permissions::from_mode(0o755)).unwrap();
    for scenario in ["new", "dead", "prompt-new", "prompt-dead", "fresh", "claude", "fail-new", "fail-dead", "active"] {
        let out = Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "restored_sessions_and_activity_without_startup_callbacks", "--nocapture"])
            .env("SHIFU_TEST_RESTORE", scenario).env("SHIFU_TEST_CLI", env!("CARGO_BIN_EXE_shifu"))
            .env("SHIFU_HOME", root.join(scenario)).env("SHIFU_SOCKET", "isolated-test")
            .env("SHIFU_TMUX", &tmux).env("SHIFU_ID", NAME)
            .env_remove("TMUX").env_remove("TMUX_PANE")
            .output().unwrap();
        assert!(out.status.success(), "{scenario}: {}\n{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));
    }
    fs::remove_dir_all(root).unwrap();
}
