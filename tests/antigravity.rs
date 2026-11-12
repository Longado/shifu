use shifu::agent::{Agent, Kind};
use shifu::hook::{apply_event, parse_input};
use shifu::launch::{agent_command, model_flags, resume_command};
use shifu::status::Status;

#[test]
fn antigravity_launch_and_resume_keep_the_exact_conversation_and_model() {
    let kind = Kind::of_command("antigravity");
    assert_eq!(serde_json::to_value(kind).unwrap(), "antigravity");
    assert_eq!(kind, Kind::of_command("agy"));
    let pick = model_flags(kind, Some("gemini-3.8-flash-low"), Some("low"));
    let launch = agent_command(kind, "antigravity", "/opt/shifu", "", &pick);
    assert_eq!(launch, "'agy' '--model' 'gemini-3.8-flash-low' '--effort' 'low'");
    let resumed = resume_command(kind, "antigravity", "/opt/shifu", "", Some("conversation-a"), &pick);
    assert!(resumed.contains("'--conversation' 'conversation-a'"));
    assert!(resumed.contains("'--model' 'gemini-3.8-flash-low'"));
    // A never-started conversation has no ID. Do not resume some other task's latest conversation.
    let fresh = resume_command(kind, "agy", "/opt/shifu", "", None, &pick);
    assert_eq!(fresh, launch);
}

#[test]
fn antigravity_hooks_capture_conversation_and_follow_the_execution_loop() {
    let kind = Kind::of_command("agy");
    let mut agent = Agent { name: "agy-test".into(), kind, command: "agy".into(), cwd: "/tmp".into(),
        created_at: 0, status: Status::Starting, status_since: 0, window_id: None, pane_id: None,
        session_id: None, last_message: None, model: None, effort: None, label: None };
    let input = parse_input(r#"{"conversationId":"conversation-a","toolCall":{"name":"ask_question"}}"#);
    assert_eq!(input.session_id.as_deref(), Some("conversation-a"));
    assert_eq!(input.tool_name.as_deref(), Some("ask_question"));
    agent = apply_event(&agent, "SessionStart", &input, 1);
    agent = apply_event(&agent, "PreInvocation", &input, 2);
    assert_eq!(agent.status, Status::Running);
    agent = apply_event(&agent, "PreToolUse", &input, 3);
    assert_eq!(agent.status, Status::NeedsReply);
    agent = apply_event(&agent, "PostToolUse", &input, 4);
    assert_eq!(agent.status, Status::Running);
    agent = apply_event(&agent, "PostInvocation", &input, 5);
    assert_eq!(agent.status, Status::Running);
    agent = apply_event(&agent, "Stop", &input, 6);
    assert_eq!(agent.status, Status::NeedsReply);
    assert_eq!(agent.session_id.as_deref(), Some("conversation-a"));
}

#[test]
fn antigravity_neutral_hooks_update_only_their_own_shifu_agent() {
    use std::{fs, io::Write, process::{Command, Stdio}};
    let root = std::env::temp_dir().join(format!("shifu-agy-hook-{}", std::process::id()));
    fs::create_dir_all(root.join("agents")).unwrap();
    let path = root.join("agents/agy-test.json");
    let config = shifu::launch::antigravity_config("{}", env!("CARGO_BIN_EXE_shifu")).unwrap().unwrap();
    let config: serde_json::Value = serde_json::from_str(&config).unwrap();
    let command = config["shifu-status"]["PreToolUse"][0]["hooks"][0]["command"].as_str().unwrap();
    let run = |id: Option<&str>| {
        let mut cmd = Command::new("/bin/sh");
        cmd.args(["-c", command]).env("SHIFU_HOME", &root).env_remove("TMUX").env_remove("TMUX_PANE");
        if let Some(id) = id { cmd.env("SHIFU_ID", id); } else { cmd.env_remove("SHIFU_ID"); }
        let mut child = cmd.stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn().unwrap();
        child.stdin.take().unwrap().write_all(br#"{"conversationId":"conversation-a","toolCall":{"name":"ask_question"}}"#).unwrap();
        let output = child.wait_with_output().unwrap();
        assert!(output.status.success());
        assert!(output.stderr.is_empty());
        assert_eq!(serde_json::from_slice::<serde_json::Value>(&output.stdout).unwrap(), serde_json::json!({"decision":"allow"}));
    };
    let record = serde_json::json!({"name":"agy-test","kind":"antigravity","command":"agy","cwd":"/tmp","created_at":0,
        "status":"running","status_since":0,"window_id":null,"pane_id":null,"session_id":null,"last_message":null});
    fs::write(&path, record.to_string()).unwrap();
    run(None);
    assert_eq!(serde_json::from_str::<serde_json::Value>(&fs::read_to_string(&path).unwrap()).unwrap(), record);
    run(Some("agy-test"));
    let updated: serde_json::Value = serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
    assert_eq!(updated["status"], "needs_reply");
    assert_eq!(updated["session_id"], "conversation-a");
    let mut parent = record;
    parent["kind"] = serde_json::json!("claude");
    fs::write(&path, parent.to_string()).unwrap();
    run(Some("agy-test"));
    assert_eq!(serde_json::from_str::<serde_json::Value>(&fs::read_to_string(&path).unwrap()).unwrap(), parent);
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn antigravity_background_work_is_not_reported_as_finished() {
    let kind = Kind::of_command("agy");
    let agent = Agent { name: "agy-test".into(), kind, command: "agy".into(), cwd: "/tmp".into(),
        created_at: 0, status: Status::Running, status_since: 0, window_id: None, pane_id: None,
        session_id: None, last_message: None, model: None, effort: None, label: None };
    let input = parse_input(r#"{"conversationId":"conversation-a","fullyIdle":false}"#);
    assert_eq!(apply_event(&agent, "Stop", &input, 1).status, Status::Running);
}

#[test]
fn antigravity_waiting_for_its_first_prompt_is_idle_and_a_dead_pane_is_exited() {
    use shifu::agent::{PaneInfo, initial_tab_status};
    use std::collections::HashMap;
    let kind = Kind::of_command("agy");
    let agent = Agent { name: "agy-test".into(), kind, command: "agy".into(), cwd: "/tmp".into(),
        created_at: 0, status: Status::Starting, status_since: 0, window_id: None, pane_id: Some("%1".into()),
        session_id: None, last_message: None, model: None, effort: None, label: None };
    let panes = HashMap::from([("%1".into(), PaneInfo { dead: false, window_id: "@1".into(), index: 0 })]);
    assert_eq!(agent.effective_status(&panes), Status::Idle);
    assert_eq!(initial_tab_status(kind), Status::Idle);
    assert_eq!(agent.effective_status(&HashMap::new()), Status::Exited);
}
