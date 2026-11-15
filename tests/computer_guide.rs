use std::process::Command;

#[test]
fn common_guide_is_available_without_a_native_helper() {
    let run = |extra: &[&str]| {
        Command::new(env!("CARGO_BIN_EXE_shifu"))
            .args(["computer", "guide"])
            .args(extra)
            .env("SHIFU_COMPUTER_HELPER", "/nonexistent/shifu-guide-helper")
            .output().unwrap()
    };
    let plain = run(&[]);
    assert!(plain.status.success(), "{}", String::from_utf8_lossy(&plain.stderr));
    let json = run(&["--json"]);
    assert!(json.status.success(), "{}", String::from_utf8_lossy(&json.stderr));
    let value: serde_json::Value = serde_json::from_slice(&json.stdout).unwrap();
    assert_eq!(value["version"], 1);
    let guide = value["guide"].as_str().unwrap();
    assert_eq!(String::from_utf8(plain.stdout).unwrap().trim(), guide.trim());
    for command in ["capabilities", "list-windows", "get-app-state", "--window-id", "--session", "--wait-for", "--expect-path", "--restore-window", "observation_failed", "screenshot", "treeDiff", "baselineSnapshotId", "--full-tree", "--background-only"] {
        assert!(guide.contains(command), "guide must explain {command}");
    }
    assert!(!run(&["--app", "unused"]).status.success());
}

#[test]
#[cfg(target_os = "macos")]
fn background_only_rejects_foreground_routes_without_starting_a_helper() {
    for args in [
        vec!["permissions", "--background-only"],
        vec!["click", "--app", "test", "--element-index", "1", "--background-only"],
        vec!["paste-text", "--app", "test", "--text", "hello", "--background-only"],
        vec!["get-app-state", "--app", "test", "--restore-window", "--background-only"],
    ] {
        let output = Command::new(env!("CARGO_BIN_EXE_shifu"))
            .arg("computer").args(&args)
            .env("SHIFU_COMPUTER_HELPER", "/nonexistent/strict-background-helper")
            .output().unwrap();
        assert!(!output.status.success(), "{args:?}");
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(stderr.contains("--background-only"), "{args:?}: {stderr}");
        assert!(!stderr.contains("还没安装"), "request reached helper startup: {stderr}");
    }
}

#[test]
#[cfg(target_os = "macos")]
fn background_only_stops_after_the_real_cli_sees_an_old_helper() {
    use std::io::{BufRead, BufReader, Write};
    use std::os::unix::net::UnixListener;
    use std::time::{Duration, SystemTime, UNIX_EPOCH};
    let nonce = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos();
    let home = std::path::PathBuf::from(format!("/tmp/shifu-bg-{}-{nonce}", std::process::id()));
    std::fs::create_dir(&home).unwrap();
    let socket = home.join("bridge.sock");
    let listener = UnixListener::bind(&socket).unwrap();
    listener.set_nonblocking(true).unwrap();
    let server = std::thread::spawn(move || {
        let start = std::time::Instant::now();
        let mut stream = loop {
            match listener.accept() {
                Ok((stream, _)) => break stream,
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock && start.elapsed() < Duration::from_secs(5) => std::thread::sleep(Duration::from_millis(10)),
                Err(e) => panic!("CLI did not connect: {e}"),
            }
        };
        stream.set_read_timeout(Some(Duration::from_secs(2))).unwrap();
        let mut line = String::new();
        BufReader::new(stream.try_clone().unwrap()).read_line(&mut line).unwrap();
        let request: serde_json::Value = serde_json::from_str(&line).unwrap();
        assert_eq!(request["method"], "handshake", "an action was sent before capability negotiation");
        writeln!(stream, "{}", serde_json::json!({"ok":true,"result":{"supports":{"input":{"background":true}}}})).unwrap();
    });
    let output = Command::new(env!("CARGO_BIN_EXE_shifu"))
        .args(["computer", "set-value", "--app", "test", "--element-index", "1", "--value", "hello", "--background-only"])
        .env("SHIFU_COMPUTER_HOME", &home)
        .env("SHIFU_COMPUTER_HELPER", "/nonexistent/strict-background-helper")
        .output().unwrap();
    server.join().unwrap();
    std::fs::remove_file(&socket).unwrap();
    std::fs::remove_dir(&home).unwrap();
    assert!(!output.status.success());
    let response: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(response["error"]["code"], "background_only_unsupported");
    assert_eq!(response["result"]["recovery"]["actionReported"], false);
}

#[cfg(target_os = "macos")]
#[test]
fn doctor_reports_the_missing_helper_without_opening_permissions() {
    let output = Command::new(env!("CARGO_BIN_EXE_shifu"))
        .args(["computer", "doctor", "--json"])
        .env("SHIFU_COMPUTER_HELPER", "/nonexistent/shifu-doctor-helper")
        .output().unwrap();
    let value: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert!(!output.status.success());
    assert_eq!(value["result"]["installed"], false);
    assert!(value["result"]["nextStep"].as_str().unwrap().contains("install-computer.sh"));
}
