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

fn recovery_fixture(scenario: &str) {
    let home = agent::shifu_home();
    fs::create_dir_all(home.join("agents")).unwrap();
    let record = Agent {
        name: NAME.into(), kind: Kind::Codex, command: "codex".into(), cwd: home.to_string_lossy().into(),
        created_at: 100, status: Status::Exited, status_since: 100, pane_id: None, window_id: None,
        session_id: Some("saved-conversation".into()), last_message: None, waiting_for: None,
        model: None, effort: None, label: None,
    };
    agent::save(&record).unwrap();
    let cfg = Config { home: home.clone(), socket: "isolated-test".into(), shifu_bin: env!("CARGO_BIN_EXE_shifu").into(), user_statusline: None };
    let names = if scenario == "recovery-delayed" {
        agent::save(&Agent { name: "codex-second".into(), ..record.clone() }).unwrap();
        vec![NAME.into(), "codex-second".into()]
    } else { vec![NAME.into()] };
    let mut settled = false;
    app_session::recover_sessions(&cfg, &names, |duration| {
        if settled && scenario == "recovery-delayed" && duration.as_secs() == 8 {
            agent::remove("codex-second").unwrap();
        }
        if duration.as_secs() == 30 { settled = true; }
        if duration.as_secs() == 30 {
            match scenario {
                "recovery-delete" => agent::remove(NAME).unwrap(),
                "recovery-replace" => agent::save(&Agent { session_id: Some("replacement".into()), ..agent::load(NAME).unwrap().unwrap() }).unwrap(),
                "recovery-exit" => fs::write(home.join("exit-code"), "0").unwrap(),
                "recovery-manual" => fs::write(home.join("pid"), "222").unwrap(),
                _ => (),
            }
        }
    });
    let count = fs::read_to_string(home.join("attempts")).unwrap().lines().count();
    assert_eq!(count, if scenario == "recovery-delayed" { 3 } else if matches!(scenario, "recovery-success" | "recovery-fail" | "recovery-error") { 2 } else { 1 }, "{scenario}");
    if scenario == "recovery-success" { assert_eq!(agent::load(NAME).unwrap().unwrap().pane_id.as_deref(), Some("%new")); }
}

/// The tmux server is gone (a restart of the Mac): every record still names a pane of the old server.
fn lost_server_fixture(scenario: &str) {
    let home = agent::shifu_home();
    fs::create_dir_all(home.join("agents")).unwrap();
    let record = |name: &str, kind: Kind, pane: &str, status: Status, since: u64, session: Option<&str>| Agent {
        name: name.into(), kind, command: kind.as_str().into(), cwd: home.to_string_lossy().into(), created_at: 100,
        status, status_since: since, pane_id: Some(pane.into()), window_id: Some("@old".into()),
        session_id: session.map(Into::into), last_message: Some("Previous work finished".into()), waiting_for: None,
        model: None, effort: None, label: None,
    };
    for agent in [
        record("claude-recent", Kind::Claude, "%1", Status::NeedsReply, 300, Some("claude-conversation")),
        record("codex-older", Kind::Codex, "%2", Status::Idle, 200, Some("codex-conversation")),
        record("codebuddy-unsaved", Kind::CodeBuddy, "%5", Status::Starting, 100, None),
    ] { agent::save(&agent).unwrap(); }
    let config = Config {
        home: home.clone(), socket: "isolated-test".into(),
        shifu_bin: env!("CARGO_BIN_EXE_shifu").into(), user_statusline: None,
    };
    let load = |name: &str| agent::load(name).unwrap().unwrap();
    if scenario == "server-quit-with-error" {
        // %1 still runs, %2 gave up with an error, %5 was quit by the user: only the error is tried again.
        let reopened = ["claude-recent", "codex-older", "codebuddy-unsaved"].map(String::from);
        assert_eq!(app_session::quit_with_error(&reopened), ["codex-older"]);
        return;
    }
    if scenario == "server-alive" {
        // Another agent's pane is there, so the server never went away: nothing is reopened behind the user's back.
        assert!(app_session::lost_sessions().is_empty());
        return;
    }
    // Only conversations that can be reopened, the one used last first.
    assert_eq!(app_session::lost_sessions(), ["claude-recent", "codex-older"]);
    let outcome = app_session::resume_agent(&config, "claude-recent");
    // The new server numbers its panes from %0 again: an old id may now be another agent's pane.
    let reused = std::collections::HashMap::from([
        ("%1".to_string(), agent::PaneInfo { dead: false, window_id: "@1".into(), index: 0 }),
        ("%2".to_string(), agent::PaneInfo { dead: false, window_id: "@2".into(), index: 0 }),
        ("%5".to_string(), agent::PaneInfo { dead: false, window_id: "@5".into(), index: 0 }),
    ]);
    for name in ["codex-older", "codebuddy-unsaved"] {
        let lost = load(name);
        assert_eq!((lost.pane_id.as_deref(), lost.effective_status(&reused)), (None, Status::Exited), "{name}");
    }
    assert_eq!(load("codex-older").session_id.as_deref(), Some("codex-conversation"));
    if scenario == "server-gone-launch-fails" {
        assert!(outcome.is_err());
        let failed = load("claude-recent");
        assert_eq!((failed.pane_id.as_deref(), failed.effective_status(&reused)), (None, Status::Exited));
        assert_eq!(failed.session_id.as_deref(), Some("claude-conversation"));
        return;
    }
    outcome.unwrap();
    assert_eq!(load("claude-recent").pane_id.as_deref(), Some("%new"));
    assert!(fs::read_to_string(home.join("launch.txt")).unwrap().contains("claude-conversation"));
    app_session::resume_agent(&config, "codex-older").unwrap();
    assert_eq!((load("codex-older").pane_id.as_deref(), load("codex-older").status), (Some("%new"), Status::Idle));
    assert!(fs::read_to_string(home.join("launch.txt")).unwrap().contains("codex-conversation"));
}

#[test]
fn restored_sessions_and_activity_without_startup_callbacks() {
    // Re-exec with isolated environment instead of changing the test runner's environment.
    if let Ok(scenario) = std::env::var("SHIFU_TEST_RESTORE") {
        if scenario.starts_with("recovery-") { recovery_fixture(&scenario) } else if scenario.starts_with("server-") { lost_server_fixture(&scenario) } else { fixture(&scenario) }
        return;
    }
    let root = std::env::temp_dir().join(format!("shifu-restore-status-{}", std::process::id()));
    fs::create_dir_all(&root).unwrap();
    let tmux = root.join("tmux");
    fs::write(&tmux, r#"#!/bin/sh
for last do :; done
case " $* " in
  *" display-message "*) cat "$SHIFU_HOME/pid" 2>/dev/null || printf '111';;
  *" list-panes "*)
    case "$SHIFU_TEST_RESTORE" in
      recovery-*)
        if test -f "$SHIFU_HOME/started"; then
          case "$*" in
            *pane_dead_status*) printf '%%new '; cat "$SHIFU_HOME/exit-code" 2>/dev/null || printf '1'; printf '\n';;
            *) printf '%%new 1 @new 0\n';;
          esac
        fi;;
      *dead) printf '%%old 1 @old 0\n';;
      active) printf '%%old 0 @old 0\n';;
      server-alive) printf '%%9 0 @9 0\n';;
      server-quit-with-error) printf '%%1 \n%%2 1\n%%5 0\n';;
    esac
    ;;
  *" new-session "*|*" respawn-pane "*)
    printf '%s' "$last" > "$SHIFU_HOME/launch.txt"
    printf 'launch\n' >> "$SHIFU_HOME/attempts"
    case "$SHIFU_TEST_RESTORE" in
      recovery-success|recovery-delete|recovery-replace) test -f "$SHIFU_HOME/failed" || { touch "$SHIFU_HOME/failed"; exit 1; };;
      recovery-delayed) test "$(wc -l < "$SHIFU_HOME/attempts")" -gt 2 || exit 1;;
      recovery-fail) exit 1;;
    esac
    touch "$SHIFU_HOME/started"
    case "$SHIFU_TEST_RESTORE" in
      prompt-*) printf '{"session_id":"saved-conversation"}' | "$SHIFU_TEST_CLI" hook codex UserPromptSubmit ;;
      fail-*|server-gone-launch-fails) exit 1 ;;
    esac
    case " $* " in *" new-session "*) printf '%%new @new\n';; esac
    ;;
esac
"#).unwrap();
    fs::set_permissions(&tmux, fs::Permissions::from_mode(0o755)).unwrap();
    for scenario in ["recovery-delayed", "recovery-success", "recovery-fail", "recovery-delete", "recovery-replace", "recovery-error", "recovery-exit", "recovery-manual", "new", "dead", "prompt-new", "prompt-dead", "fresh", "claude", "fail-new", "fail-dead", "active", "server-gone", "server-gone-launch-fails", "server-alive", "server-quit-with-error"] {
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
