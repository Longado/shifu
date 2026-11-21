//! Guide discovery is delivered through real hook subprocesses, without starting an agent CLI.
use serde_json::{Value, json};
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

fn home(tag: &str, kind: &str, session: Option<&str>) -> PathBuf {
    let path = std::env::temp_dir().join(format!("shifu-guide-hook-{}-{tag}", std::process::id()));
    let _ = fs::remove_dir_all(&path);
    fs::create_dir_all(path.join("agents")).unwrap();
    fs::write(path.join("agents/agent.json"), json!({
        "name":"agent", "kind":kind, "command":kind, "cwd":"/tmp", "created_at":100,
        "status":"starting", "status_since":100, "window_id":"@1", "pane_id":"%1",
        "session_id":session, "last_message":null
    }).to_string()).unwrap();
    path
}

fn hook(home: &Path, kind: &str, event: &str, input: Value) -> String {
    let mut command = Command::new(env!("CARGO_BIN_EXE_shifu"));
    command.args(["hook", kind, event]).env("SHIFU_HOME", home).env("SHIFU_ID", "agent")
        .env_remove("TMUX").env_remove("TMUX_PANE")
        .stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped());
    let mut child = command.spawn().unwrap();
    child.stdin.take().unwrap().write_all(input.to_string().as_bytes()).unwrap();
    let result = child.wait_with_output().unwrap();
    assert!(result.status.success());
    assert!(result.stderr.is_empty(), "{}", String::from_utf8_lossy(&result.stderr));
    String::from_utf8(result.stdout).unwrap()
}

#[test]
fn new_claude_and_codex_receive_a_short_session_context() {
    for kind in ["claude", "codex"] {
        let path = home(kind, kind, None);
        let output = hook(&path, kind, "SessionStart", json!({"session_id":"s-1"}));
        let value: Value = serde_json::from_str(&output).expect("SessionStart must return structured context");
        assert_eq!(value["hookSpecificOutput"]["hookEventName"], "SessionStart");
        let context = value["hookSpecificOutput"]["additionalContext"].as_str().unwrap();
        let expected = format!("{} computer guide", shifu::launch::sh_quote(env!("CARGO_BIN_EXE_shifu")));
        assert!(context.contains(&expected), "{context}");
        assert!(context.contains("browser guide"), "{context}");
        assert!(context.contains("API/CLI") && context.contains("截图"));
        assert!(context.chars().count() < 500);
        let record: Value = serde_json::from_str(&fs::read_to_string(path.join("agents/agent.json")).unwrap()).unwrap();
        assert_eq!(record["session_id"], "s-1");
        assert_eq!(record["status"], "idle");
    }
}

#[test]
fn existing_sessions_and_unrelated_hooks_remain_silent() {
    let cases = [
        ("existing", "claude", "claude", "SessionStart", json!({"session_id":"s-1"}), Some("s-1")),
        ("kind", "claude", "codex", "SessionStart", json!({"session_id":"s-1"}), None),
        ("sub", "claude", "claude", "SessionStart", json!({"session_id":"s-1","agent_id":"child"}), None),
        ("missing", "claude", "claude", "SessionStart", json!({}), None),
        ("empty", "claude", "claude", "SessionStart", json!({"session_id":""}), None),
        ("other-session", "claude", "claude", "SessionStart", json!({"session_id":"child-session"}), Some("parent-session")),
        ("tool", "claude", "claude", "PreToolUse", json!({"session_id":"s-1","tool_name":"Bash"}), None),
        ("prompt", "claude", "claude", "UserPromptSubmit", json!({"session_id":"s-1"}), None),
        ("kimi-existing-start", "kimi", "kimi", "SessionStart", json!({"session_id":"s-1"}), Some("s-1")),
        ("kimi-no-pending", "kimi", "kimi", "UserPromptSubmit", json!({"session_id":"s-1"}), Some("s-1")),
    ];
    for (tag, registered, kind, event, input, session) in cases {
        let path = home(tag, registered, session);
        assert!(hook(&path, kind, event, input).is_empty(), "{tag}");
    }
}

#[test]
fn kimi_delivers_once_on_the_matching_first_prompt() {
    let path = home("kimi", "kimi", None);
    assert!(hook(&path, "kimi", "SessionStart", json!({"session_id":"kimi-1","agent_id":"main"})).is_empty());
    let pending = path.join("computer-guide-pending/agent");
    assert_eq!(fs::read_to_string(&pending).unwrap(), "kimi-1");
    assert!(hook(&path, "kimi", "UserPromptSubmit", json!({"session_id":"kimi-1","agent_id":"child"})).is_empty());
    assert!(pending.exists());
    let output = hook(&path, "kimi", "UserPromptSubmit", json!({"session_id":"kimi-1","agent_id":"main"}));
    assert!(output.contains("computer guide"));
    assert!(!output.starts_with('{'), "Kimi needs plain context rather than Claude's JSON");
    assert!(!pending.exists());
    assert!(hook(&path, "kimi", "UserPromptSubmit", json!({"session_id":"kimi-1"})).is_empty());
}

#[test]
fn a_mismatched_kimi_session_does_not_consume_pending_context() {
    let path = home("kimi-mismatch", "kimi", None);
    hook(&path, "kimi", "SessionStart", json!({"session_id":"kimi-1"}));
    assert!(hook(&path, "kimi", "UserPromptSubmit", json!({"session_id":"kimi-2"})).is_empty());
    assert_eq!(fs::read_to_string(path.join("computer-guide-pending/agent")).unwrap(), "kimi-1");
}

fn put_in_task(home: &Path, goal: Option<&str>) -> PathBuf {
    fs::write(home.join("tasks.json"), json!({"current":"t-1","tasks":[
        {"id":"t-0","project":"/tmp","name":"别的事","auto_name":false,"created_at":1,"agents":["other"]},
        {"id":"t-1","project":"/tmp","name":"求职","auto_name":false,"created_at":1,"agents":["agent"]}
    ]}).to_string()).unwrap();
    let file = home.join("goals/t-1.md");
    if let Some(text) = goal {
        fs::create_dir_all(home.join("goals")).unwrap();
        fs::write(&file, text).unwrap();
    }
    file
}

fn context(output: &str) -> String {
    let value: Value = serde_json::from_str(output).unwrap_or_else(|_| panic!("not JSON: {output:?}"));
    value["hookSpecificOutput"]["additionalContext"].as_str().unwrap().to_string()
}

#[test]
fn a_conversation_in_a_task_without_a_goal_is_asked_to_draft_it() {
    let path = home("task-empty", "claude", None);
    let file = put_in_task(&path, Some("# 终点\n\n\n# 重点\n\n\n# 清单\n\n\n# 记录\n\n"));
    let text = context(&hook(&path, "claude", "SessionStart", json!({"session_id":"s-1","source":"startup"})));
    assert!(text.contains("computer guide"), "{text}");
    assert!(text.contains("「求职」") && text.contains(&file.display().to_string()), "{text}");
    assert!(text.contains("起草"), "{text}");
}

#[test]
fn a_conversation_in_a_task_with_a_goal_is_told_to_read_and_tick_it() {
    let path = home("task-written", "codex", None);
    let file = put_in_task(&path, Some("# 终点\n拿到 offer\n\n# 清单\n- [ ] 投十家\n"));
    let text = context(&hook(&path, "codex", "SessionStart", json!({"session_id":"s-1"})));
    assert!(text.contains(&file.display().to_string()) && text.contains("- [x]"), "{text}");
    assert!(text.contains("`agent`"), "the conversation learns its own name for @ marks: {text}");
    assert!(!text.contains("起草"), "{text}");
}

#[test]
fn an_unreadable_task_file_keeps_its_identity_in_the_start_hook() {
    let path = home("task-unreadable", "codex", None);
    let file = put_in_task(&path, Some(""));
    fs::write(&file, [0xff, 0xfe, b'A']).unwrap();
    let text = context(&hook(&path, "codex", "SessionStart", json!({"session_id":"s-1"})));
    assert!(text.contains("无法读取") && text.contains(&file.display().to_string()), "{text}");
    assert!(!text.contains("起草"), "{text}");
}

#[test]
fn clear_compact_and_resume_repeat_the_task_but_not_the_computer_guide() {
    for source in ["clear", "compact", "resume"] {
        let path = home(&format!("task-{source}"), "claude", Some("old"));
        let file = put_in_task(&path, None);
        let text = context(&hook(&path, "claude", "SessionStart", json!({"session_id":"new","source":source})));
        assert!(text.contains(&file.display().to_string()), "{source}: {text}");
        assert!(!text.contains("computer guide"), "{source}: {text}");
    }
    let path = home("task-other-start", "claude", Some("parent"));
    put_in_task(&path, None);
    assert!(hook(&path, "claude", "SessionStart", json!({"session_id":"child","source":"startup"})).is_empty(),
        "a CLI started inside the conversation is not the conversation");
    let path = home("no-task-clear", "claude", Some("old"));
    assert!(hook(&path, "claude", "SessionStart", json!({"session_id":"new","source":"clear"})).is_empty());
}

#[test]
fn kimi_gets_its_task_with_the_first_prompt() {
    let path = home("kimi-task", "kimi", None);
    let file = put_in_task(&path, None);
    hook(&path, "kimi", "SessionStart", json!({"session_id":"kimi-1"}));
    let output = hook(&path, "kimi", "UserPromptSubmit", json!({"session_id":"kimi-1"}));
    assert!(output.contains("computer guide") && output.contains(&file.display().to_string()), "{output}");
}

#[test]
fn the_pinned_roles_are_not_asked_to_draft_a_goal() {
    for role in ["pm", "daily"] {
        let path = home(&format!("role-{role}"), "codex", None);
        let dir = path.join(role);
        fs::write(path.join("tasks.json"), json!({"current":"t-1","tasks":[
            {"id":"t-1","project":dir,"name":"每日分析","auto_name":false,"created_at":1,"agents":["agent"]}
        ]}).to_string()).unwrap();
        let text = context(&hook(&path, "codex", "SessionStart", json!({"session_id":"s-1","source":"startup"})));
        assert!(text.contains("computer guide"), "{role}: {text}");
        assert!(!text.contains("起草") && !text.contains("goals/"), "{role}: {text}");
        let restarted = home(&format!("role-{role}-clear"), "codex", Some("old"));
        fs::write(restarted.join("tasks.json"), json!({"current":"t-1","tasks":[
            {"id":"t-1","project":restarted.join(role),"name":"每日分析","auto_name":false,"created_at":1,"agents":["agent"]}
        ]}).to_string()).unwrap();
        assert!(hook(&restarted, "codex", "SessionStart", json!({"session_id":"new","source":"clear"})).is_empty(), "{role}");
    }
}
