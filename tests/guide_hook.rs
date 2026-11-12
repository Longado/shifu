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
