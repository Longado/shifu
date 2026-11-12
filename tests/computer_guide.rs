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
    for command in ["capabilities", "list-windows", "get-app-state", "--window-id", "--session", "--wait-for", "--expect-path", "--restore-window", "observation_failed", "screenshot", "treeDiff", "baselineSnapshotId", "--full-tree"] {
        assert!(guide.contains(command), "guide must explain {command}");
    }
    assert!(!run(&["--app", "unused"]).status.success());
}
