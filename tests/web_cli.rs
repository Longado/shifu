use std::process::{Command, Stdio};
use std::io::Write;

#[test]
fn web_help_is_reachable_without_starting_any_browser() {
    let result = Command::new(env!("CARGO_BIN_EXE_shifu")).args(["web", "help"]).output().unwrap();
    assert!(result.status.success());
    let text = String::from_utf8(result.stdout).unwrap();
    assert!(text.contains("无界面网页任务池"));
    assert!(text.contains("request --control"));
    let root = Command::new(env!("CARGO_BIN_EXE_shifu")).arg("help").output().unwrap();
    let text = String::from_utf8(root.stdout).unwrap();
    assert!(text.contains("shifu workspace help"));
    assert!(text.contains("shifu computer help"));
    assert!(text.contains("shifu browser help"));
    assert!(text.contains("shifu web help"));
}

#[test]
fn malformed_request_does_not_connect_or_start_a_browser() {
    let mut child = Command::new(env!("CARGO_BIN_EXE_shifu")).args(["web", "request"])
        .stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn().unwrap();
    child.stdin.take().unwrap().write_all(b"null\n").unwrap();
    let result = child.wait_with_output().unwrap();
    assert!(!result.status.success());
    let reply: serde_json::Value = serde_json::from_slice(&result.stdout).unwrap();
    assert_eq!(reply["ok"], false);
    assert_eq!(reply["error"]["phase"], "not_dispatched");
}

#[test]
fn unavailable_runtime_returns_a_structured_not_dispatched_error() {
    let result = Command::new(env!("CARGO_BIN_EXE_shifu")).args(["web", "request"])
        .env("SHIFU_WEB_RUNTIME", "/missing-shifu-web-runtime/cli.mjs").output().unwrap();
    assert!(!result.status.success());
    let reply: serde_json::Value = serde_json::from_slice(&result.stdout).unwrap();
    assert_eq!(reply["error"]["code"], "RUNTIME_UNAVAILABLE");
    assert_eq!(reply["error"]["phase"], "not_dispatched");
}
