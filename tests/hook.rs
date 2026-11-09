//! `shifu hook` 作为子进程跑：喂 stdin，检查 agent 文件和 stdout。

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

fn temp_home(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("shifu-test-{}-{}", std::process::id(), tag));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(dir.join("agents")).unwrap();
    dir
}

fn write_agent(home: &Path, name: &str, status: &str) {
    let json = format!(
        r#"{{"name":"{name}","kind":"claude","command":"claude","cwd":"/tmp","created_at":100,
        "status":"{status}","status_since":100,"window_id":"@1","pane_id":"%1","session_id":null,"last_message":null}}"#
    );
    fs::write(home.join("agents").join(format!("{name}.json")), json).unwrap();
}

fn read_agent(home: &Path, name: &str) -> serde_json::Value {
    let text = fs::read_to_string(home.join("agents").join(format!("{name}.json"))).unwrap();
    serde_json::from_str(&text).unwrap()
}

/// 返回 (退出码, stdout, stderr)。不带 TMUX 环境，所以不会碰任何 tmux。
fn run_hook(home: &Path, shifu_id: Option<&str>, event: &str, stdin: &str) -> (i32, Vec<u8>, Vec<u8>) {
    run_hook_env(home, shifu_id, event, stdin, &[])
}

fn run_hook_env(home: &Path, shifu_id: Option<&str>, event: &str, stdin: &str, envs: &[(&str, &Path)]) -> (i32, Vec<u8>, Vec<u8>) {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_shifu"));
    cmd.args(["hook", "claude", event])
        .env("SHIFU_HOME", home)
        .env_remove("SHIFU_ID")
        .env_remove("TMUX")
        .env_remove("TMUX_PANE")
        .envs(envs.iter().copied())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    if let Some(id) = shifu_id {
        cmd.env("SHIFU_ID", id);
    }
    let mut child = cmd.spawn().unwrap();
    child.stdin.take().unwrap().write_all(stdin.as_bytes()).unwrap();
    let out = child.wait_with_output().unwrap();
    (out.status.code().unwrap(), out.stdout, out.stderr)
}

#[test]
fn permission_request_sets_needs_approval_silently() {
    let home = temp_home("perm");
    write_agent(&home, "claude-p", "running");
    let (code, stdout, stderr) = run_hook(&home, Some("claude-p"), "PermissionRequest", r#"{"tool_name":"Bash","session_id":"s-1"}"#);
    assert_eq!(code, 0);
    assert!(stdout.is_empty(), "stdout 必须为空");
    assert!(stderr.is_empty(), "stderr 必须为空");
    let a = read_agent(&home, "claude-p");
    assert_eq!(a["status"], "needs_approval");
    assert_eq!(a["session_id"], "s-1");
    assert!(a["status_since"].as_u64().unwrap() > 100);
}

#[test]
fn stop_records_last_message() {
    let home = temp_home("stop");
    write_agent(&home, "claude-p", "running");
    let (code, stdout, _) = run_hook(&home, Some("claude-p"), "Stop", r#"{"last_assistant_message":"README 改好了"}"#);
    assert_eq!(code, 0);
    assert!(stdout.is_empty());
    let a = read_agent(&home, "claude-p");
    assert_eq!(a["status"], "needs_reply");
    assert_eq!(a["last_message"], "README 改好了");
}

#[test]
fn full_event_sequence_through_processes() {
    let home = temp_home("seq");
    write_agent(&home, "claude-p", "starting");
    let steps = [
        ("SessionStart", "{}", "idle"),
        ("UserPromptSubmit", "{}", "running"),
        ("PreToolUse", r#"{"tool_name":"Bash"}"#, "running"),
        ("PermissionRequest", r#"{"tool_name":"Bash"}"#, "needs_approval"),
        ("PostToolUse", r#"{"tool_name":"Bash"}"#, "running"),
        ("Stop", "{}", "needs_reply"),
        // A sub-agent still finishing up does not wake it; its own tool starting does.
        ("PreToolUse", r#"{"tool_name":"Bash","agent_id":"a1"}"#, "needs_reply"),
        ("PreToolUse", r#"{"tool_name":"Bash"}"#, "running"),
        ("Stop", "{}", "needs_reply"),
        ("SessionEnd", "{}", "exited"),
        ("UserPromptSubmit", "{}", "exited"),
    ];
    for (event, stdin, want) in steps {
        let (code, stdout, _) = run_hook(&home, Some("claude-p"), event, stdin);
        assert_eq!(code, 0);
        assert!(stdout.is_empty());
        assert_eq!(read_agent(&home, "claude-p")["status"], want, "{event} 之后");
    }
}

#[test]
fn without_shifu_id_nothing_changes() {
    let home = temp_home("noid");
    write_agent(&home, "claude-p", "running");
    let before = fs::read(home.join("agents/claude-p.json")).unwrap();
    let (code, stdout, stderr) = run_hook(&home, None, "Stop", "{}");
    assert_eq!(code, 0);
    assert!(stdout.is_empty());
    assert!(stderr.is_empty());
    assert_eq!(fs::read(home.join("agents/claude-p.json")).unwrap(), before);
}

#[test]
fn missing_agent_file_or_garbage_stdin_still_exits_zero() {
    let home = temp_home("missing");
    let (code, stdout, stderr) = run_hook(&home, Some("nobody"), "Stop", "not json at all");
    assert_eq!((code, stdout.is_empty(), stderr.is_empty()), (0, true, true));
    assert!(!home.join("agents/nobody.json").exists());

    write_agent(&home, "claude-p", "running");
    let (code, stdout, _) = run_hook(&home, Some("claude-p"), "Stop", "not json at all");
    assert_eq!(code, 0);
    assert!(stdout.is_empty());
    assert_eq!(read_agent(&home, "claude-p")["status"], "needs_reply");
}

#[test]
fn corrupt_agent_file_logs_and_exits_zero() {
    let home = temp_home("corrupt");
    fs::write(home.join("agents/claude-p.json"), "{ broken").unwrap();
    let (code, stdout, stderr) = run_hook(&home, Some("claude-p"), "Stop", "{}");
    assert_eq!(code, 0);
    assert!(stdout.is_empty());
    assert!(stderr.is_empty());
    let log = fs::read_to_string(home.join("hook-errors.log")).unwrap();
    assert!(log.contains("Stop"), "{log}");
}

#[test]
fn unsafe_shifu_id_is_refused() {
    let home = temp_home("unsafe");
    let (code, stdout, _) = run_hook(&home, Some("../evil"), "Stop", "{}");
    assert_eq!(code, 0);
    assert!(stdout.is_empty());
    assert!(!home.join("evil.json").exists());
}

/// 假 tmux：把收到的参数一行一行记到 `calls`，`exit_code` 不是 0 时往 stderr 写字并失败。
fn fake_tmux(home: &Path, exit_code: i32) -> (PathBuf, PathBuf) {
    let calls = home.join("tmux-calls.txt");
    let script = home.join("fake-tmux");
    fs::write(
        &script,
        format!("#!/bin/sh\necho \"$*\" >> '{}'\n[ {exit_code} -ne 0 ] && echo 'no server' >&2\nexit {exit_code}\n", calls.display()),
    )
    .unwrap();
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(&script, fs::Permissions::from_mode(0o755)).unwrap();
    (script, calls)
}

fn with_tmux<'a>(script: &'a Path, tmux_var: &'a Path, pane: &'a Path) -> [(&'static str, &'a Path); 3] {
    [("SHIFU_TMUX", script), ("TMUX", tmux_var), ("TMUX_PANE", pane)]
}

#[test]
fn status_change_sets_the_pane_option_through_the_servers_socket() {
    let home = temp_home("pane-opt");
    write_agent(&home, "claude-p", "running");
    let (script, calls) = fake_tmux(&home, 0);
    let envs = with_tmux(&script, Path::new("/tmp/fake-sock,123,0"), Path::new("%7"));
    let (code, stdout, stderr) = run_hook_env(&home, Some("claude-p"), "Stop", "{}", &envs);
    assert_eq!((code, stdout.is_empty(), stderr.is_empty()), (0, true, true));
    assert_eq!(fs::read_to_string(&calls).unwrap(), "-S /tmp/fake-sock set-option -p -t %7 @shifu_status needs_reply\n");

    // 状态没变（Stop 之后再来一个 Stop）就不再调 tmux
    let (code, _, _) = run_hook_env(&home, Some("claude-p"), "Stop", "{}", &envs);
    assert_eq!(code, 0);
    assert_eq!(fs::read_to_string(&calls).unwrap().lines().count(), 1);
}

#[test]
fn no_tmux_call_without_tmux_or_pane_variables() {
    let home = temp_home("no-tmux-env");
    write_agent(&home, "claude-p", "running");
    let (script, calls) = fake_tmux(&home, 0);
    let (code, _, _) = run_hook_env(&home, Some("claude-p"), "Stop", "{}", &[("SHIFU_TMUX", script.as_path())]);
    assert_eq!(code, 0);
    let only_tmux = [("SHIFU_TMUX", script.as_path()), ("TMUX", Path::new("/tmp/s,1,0"))];
    write_agent(&home, "claude-p", "running");
    let (code, _, _) = run_hook_env(&home, Some("claude-p"), "Stop", "{}", &only_tmux);
    assert_eq!(code, 0);
    assert!(!calls.exists(), "没有 TMUX 和 TMUX_PANE 两个变量时不该调 tmux");
    assert_eq!(read_agent(&home, "claude-p")["status"], "needs_reply");
}

#[test]
fn failing_tmux_is_logged_and_hook_still_succeeds_silently() {
    let home = temp_home("tmux-fails");
    write_agent(&home, "claude-p", "running");
    let (script, _) = fake_tmux(&home, 1);
    let envs = with_tmux(&script, Path::new("/tmp/fake-sock,123,0"), Path::new("%7"));
    let (code, stdout, stderr) = run_hook_env(&home, Some("claude-p"), "Stop", "{}", &envs);
    assert_eq!((code, stdout.is_empty(), stderr.is_empty()), (0, true, true));
    assert_eq!(read_agent(&home, "claude-p")["status"], "needs_reply", "状态文件照样写");
    let log = fs::read_to_string(home.join("hook-errors.log")).unwrap();
    assert!(log.contains("no server"), "{log}");
}
