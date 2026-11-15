#![cfg(unix)]
use serde_json::Value;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::process::{Command, Output};
use std::time::{SystemTime, UNIX_EPOCH};

fn directory(name: &str) -> std::path::PathBuf {
    let root = std::env::temp_dir().join(format!(
        "shifu-browser-{name}-{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir(&root).unwrap();
    root
}

fn run(root: &std::path::Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_shifu"))
        .arg("browser")
        .args(args)
        .env("SHIFU_BROWSER_HOME", root)
        .env("SHIFU_BROWSER_RUNTIME", root.join("runtime"))
        .env("SHIFU_BROWSER_NODE", root.join("runtime/bin/node"))
        .env("SHIFU_ID", "test-agent")
        .output()
        .unwrap()
}

fn fake_runtime(root: &std::path::Path, body: &str) {
    let runtime = root.join("runtime");
    fs::create_dir_all(runtime.join("bin")).unwrap();
    fs::create_dir_all(runtime.join("node_modules/@playwright/cli")).unwrap();
    fs::write(
        runtime.join("node_modules/@playwright/cli/playwright-cli.js"),
        "// fake provider",
    )
    .unwrap();
    let node = runtime.join("bin/node");
    fs::write(&node, format!("#!/bin/sh\n{body}\n")).unwrap();
    fs::set_permissions(node, fs::Permissions::from_mode(0o755)).unwrap();
}

#[test]
fn guide_is_usable_before_install_and_missing_runtime_is_structured() {
    let root = directory("missing");
    let guide = run(&root, &["guide", "--json"]);
    assert!(guide.status.success());
    let value: Value = serde_json::from_slice(&guide.stdout).unwrap();
    for word in [
        "snapshot",
        "--session",
        "automaticRetry",
        "screenshot",
        "detach",
    ] {
        assert!(value["guide"].as_str().unwrap().contains(word));
    }
    let missing = run(&root, &["snapshot"]);
    assert!(!missing.status.success());
    let value: Value = serde_json::from_slice(&missing.stdout).unwrap();
    assert_eq!(value["error"]["code"], "runtime_missing");
    assert!(!root.join("sessions").exists());
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn provider_errors_are_not_success_even_if_the_process_exits_zero() {
    let root = directory("failure");
    fake_runtime(
        &root,
        "printf '%s\\n' '{\"isError\":true,\"error\":\"stale ref\"}'",
    );
    let output = run(&root, &["click", "e42"]);
    assert!(!output.status.success());
    let value: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(value["ok"], false);
    assert_eq!(value["error"]["message"], "stale ref");
    assert_eq!(value["recovery"]["automaticRetry"], false);
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn a_busy_session_is_rejected_before_the_provider_runs() {
    let root = directory("busy");
    fake_runtime(&root, "exit 99");
    let workspace = root.join("sessions/test-agent");
    fs::create_dir_all(&workspace).unwrap();
    let lock = fs::File::create(workspace.join("command.lock")).unwrap();
    lock.lock().unwrap();
    let output = run(&root, &["snapshot"]);
    let value: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(value["error"]["code"], "session_busy");
    assert!(!output.status.success());
    drop(lock);
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn timeout_is_unknown_and_does_not_replay_the_command() {
    let root = directory("timeout");
    fake_runtime(
        &root,
        "printf 'one\\n' >> invocations\nsleep 2\nprintf '%s\\n' '{\"result\":\"done\"}'",
    );
    let output = run(&root, &["--timeout-ms", "1000", "click", "e1"]);
    let value: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(value["error"]["code"], "result_unknown");
    assert_eq!(value["result"]["recovery"]["automaticRetry"], false);
    assert_eq!(
        fs::read_to_string(root.join("sessions/test-agent/invocations")).unwrap(),
        "one\n"
    );
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn literal_option_like_text_is_not_an_option_or_a_path() {
    let root = directory("literal");
    fake_runtime(
        &root,
        "printf '%s\\n' \"$@\" > arguments\nprintf '%s\\n' '{\"result\":\"recorded\"}'",
    );
    let output = run(&root, &["fill", "e1", "--config=literal text"]);
    assert!(output.status.success());
    let arguments = fs::read_to_string(root.join("sessions/test-agent/arguments")).unwrap();
    assert!(arguments.contains("--json\nfill\ne1\n--\n--config=literal text\n"));
    assert_eq!(
        arguments
            .lines()
            .filter(|line| line.starts_with("-s="))
            .count(),
        1
    );
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn non_object_json_is_an_unknown_response_not_a_panic_or_success() {
    let root = directory("invalid-json");
    fake_runtime(&root, "printf 'null\\n'");
    let output = run(&root, &["snapshot"]);
    let value: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert!(!output.status.success());
    assert_eq!(value["error"]["code"], "invalid_response");
    assert_eq!(value["result"]["recovery"]["automaticRetry"], false);
    fs::remove_dir_all(root).unwrap();
}
