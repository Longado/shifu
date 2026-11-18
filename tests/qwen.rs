use shifu::agent::Kind;
use shifu::launch::{agent_command, model_flags, resume_command};
use serde_json::{Value, json};
use std::{fs, io::Write, process::{Command, Stdio}};

#[test]
fn launch_and_resume_pin_the_qwen_conversation_and_chosen_model() {
    assert_eq!(Kind::of_command("qwen-code"), Kind::Qwen);
    assert_eq!(Kind::of_command("qwen"), Kind::Qwen);
    assert_eq!(serde_json::to_value(Kind::Qwen).unwrap(), "qwen");
    let pick = model_flags(Kind::Qwen, Some("qwen3.8-flash"), Some("high"));
    assert_eq!(pick, ["--model", "qwen3.8-flash"]);
    let launch = agent_command(Kind::Qwen, "qwen", "/a path/shifu", "", &pick);
    assert_eq!(launch, "'/a path/shifu' qwen '--model' 'qwen3.8-flash'");
    let resumed = resume_command(Kind::Qwen, "qwen", "/shifu", "", Some("session-a"), &pick);
    assert!(resumed.contains("'--resume' 'session-a'"));
    assert!(!resumed.contains("--continue"));
    assert_eq!(resume_command(Kind::Qwen, "qwen", "/a path/shifu", "", None, &pick), launch);
}

#[test]
fn launcher_supplies_only_environment_credentials_and_its_isolated_settings() {
    use std::os::unix::fs::PermissionsExt;
    let root = std::env::temp_dir().join(format!("shifu-qwen-launch-{}", std::process::id()));
    fs::create_dir_all(&root).unwrap();
    let fake = root.join("fake-qwen");
    fs::write(&fake, "#!/usr/bin/env python3\nimport json,os,sys\nprint(json.dumps({'args':sys.argv[1:],'home':os.environ['QWEN_HOME'],'runtime':os.environ['QWEN_RUNTIME_DIR'],'settings':os.environ['QWEN_CODE_SYSTEM_SETTINGS_PATH'],'base':os.environ['OPENAI_BASE_URL'],'has_key':bool(os.environ.get('DASHSCOPE_API_KEY'))}))\n").unwrap();
    fs::set_permissions(&fake, fs::Permissions::from_mode(0o700)).unwrap();
    let run = |args: &[&str]| Command::new(env!("CARGO_BIN_EXE_shifu")).arg("qwen").args(args)
        .env("SHIFU_HOME", &root).env("SHIFU_QWEN_BIN", &fake)
        .env("DASHSCOPE_API_KEY", "synthetic-credential-do-not-persist").output().unwrap();
    let output = run(&["--model", "qwen3.8-flash", "--resume", "exact-session"]);
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    let output: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(output["home"], root.join("qwen").to_string_lossy().as_ref());
    assert_eq!(output["runtime"], output["home"]);
    assert_eq!(output["base"], shifu::qwen::BASE_URL);
    assert_eq!(output["has_key"], true);
    let args = output["args"].as_array().unwrap();
    assert_eq!(args, &json!(["--approval-mode","yolo","--model","qwen3.8-flash","--resume","exact-session"]).as_array().unwrap().clone());
    let config = fs::read_to_string(root.join("qwen/shifu-settings.json")).unwrap();
    assert!(!config.contains("synthetic-credential"));
    let config: Value = serde_json::from_str(&config).unwrap();
    assert_eq!(config["security"]["auth"]["selectedType"], "openai");
    for provider in config["modelProviders"]["openai"].as_array().unwrap() {
        assert_eq!(provider["envKey"], "DASHSCOPE_API_KEY");
        assert_eq!(provider["baseUrl"], shifu::qwen::BASE_URL);
    }
    let default = run(&[]);
    let default: Value = serde_json::from_slice(&default.stdout).unwrap();
    assert!(default["args"].as_array().unwrap().windows(2).any(|w| w == [json!("--model"), json!("auto")]));
    for forbidden in ["--openai-base-url=https://example.invalid", "--openai-api-key=unused", "--auth-type=openai"] {
        let output = run(&[forbidden]);
        assert!(!output.status.success());
        assert!(output.stdout.is_empty());
    }
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn generated_hooks_track_lifecycle_without_modifying_another_agent_kind() {
    let root = std::env::temp_dir().join(format!("shifu-qwen-hooks-{}", std::process::id()));
    fs::create_dir_all(root.join("agents")).unwrap();
    let path = root.join("agents/qwen-test.json");
    let record = json!({"name":"qwen-test","kind":"qwen","command":"qwen","cwd":"/tmp","created_at":0,
        "status":"starting","status_since":0,"window_id":null,"pane_id":null,"session_id":null,"last_message":null});
    let config = shifu::qwen::settings(env!("CARGO_BIN_EXE_shifu"));
    let invoke = |event: &str, input: Value| {
        let command = config["hooks"][event][0]["hooks"][0]["command"].as_str().unwrap();
        let mut child = Command::new("/bin/sh").args(["-c", command]).env("SHIFU_HOME", &root)
            .env("SHIFU_ID", "qwen-test").env_remove("TMUX").env_remove("TMUX_PANE")
            .stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn().unwrap();
        child.stdin.take().unwrap().write_all(input.to_string().as_bytes()).unwrap();
        let out = child.wait_with_output().unwrap();
        assert!(out.status.success());
        assert!(out.stdout.is_empty());
        assert!(out.stderr.is_empty());
        serde_json::from_str::<Value>(&fs::read_to_string(&path).unwrap()).unwrap()
    };
    fs::write(&path, record.to_string()).unwrap();
    let input = json!({"session_id":"session-a"});
    assert_eq!(invoke("SessionStart", input.clone())["status"], "idle");
    assert_eq!(invoke("UserPromptSubmit", input.clone())["status"], "running");
    let question = json!({"session_id":"session-a","tool_name":"ask_user_question"});
    assert_eq!(invoke("PreToolUse", question.clone())["status"], "needs_reply");
    assert_eq!(invoke("PostToolUse", question)["status"], "running");
    let stopped = invoke("Stop", json!({"session_id":"session-a","last_assistant_message":"finished"}));
    assert_eq!(stopped["status"], "needs_reply");
    assert_eq!(stopped["session_id"], "session-a");
    assert_eq!(stopped["last_message"], "finished");
    assert_eq!(invoke("SessionStart", json!({"session_id":"child-session","agent_id":"child"})), stopped);
    assert_eq!(invoke("SessionEnd", input.clone())["status"], "exited");
    let mut parent = record;
    parent["kind"] = json!("codex");
    fs::write(&path, parent.to_string()).unwrap();
    assert_eq!(invoke("SessionStart", input), parent);
    fs::remove_dir_all(root).unwrap();
}

fn fake_qwen(root: &std::path::Path) -> std::path::PathBuf {
    use std::os::unix::fs::PermissionsExt;
    let fake = root.join("fake-qwen");
    fs::write(&fake, "#!/usr/bin/env python3\nimport json,os,sys\nprint(json.dumps({'args':sys.argv[1:],'has_key':bool(os.environ.get('DASHSCOPE_API_KEY'))}))\n").unwrap();
    fs::set_permissions(&fake, fs::Permissions::from_mode(0o700)).unwrap();
    fake
}

#[test]
fn env_file_supplies_the_key_when_the_environment_has_none() {
    let root = std::env::temp_dir().join(format!("shifu-qwen-envfile-{}", std::process::id()));
    fs::create_dir_all(root.join(".config/secrets")).unwrap();
    fs::write(root.join(".config/secrets/dashscope.env"), "export DASHSCOPE_API_KEY=\"synthetic-credential-do-not-persist\"\n").unwrap();
    let fake = fake_qwen(&root);
    let output = Command::new(env!("CARGO_BIN_EXE_shifu")).arg("qwen")
        .env("SHIFU_HOME", &root).env("SHIFU_QWEN_BIN", &fake).env("HOME", &root)
        .env_remove("DASHSCOPE_API_KEY").output().unwrap();
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    let output: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(output["has_key"], true);
    let config = fs::read_to_string(root.join("qwen/shifu-settings.json")).unwrap();
    assert!(!config.contains("synthetic-credential"));
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn without_env_or_file_the_launcher_falls_back_to_hk() {
    use std::os::unix::fs::PermissionsExt;
    let root = std::env::temp_dir().join(format!("shifu-qwen-hk-{}", std::process::id()));
    fs::create_dir_all(root.join(".local/bin")).unwrap();
    let vault = root.join(".local/bin/hk");
    fs::write(&vault, "#!/bin/sh\necho hk-fallback \"$@\"\n").unwrap();
    fs::set_permissions(&vault, fs::Permissions::from_mode(0o700)).unwrap();
    let fake = fake_qwen(&root);
    let output = Command::new(env!("CARGO_BIN_EXE_shifu")).arg("qwen")
        .env("SHIFU_HOME", &root).env("SHIFU_QWEN_BIN", &fake).env("HOME", &root)
        .env_remove("DASHSCOPE_API_KEY").output().unwrap();
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.starts_with("hk-fallback run Qwen · Shifu -- "), "{stdout}");
    fs::remove_dir_all(root).unwrap();
}
