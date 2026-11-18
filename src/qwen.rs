//! Qwen Code with an isolated home and the user's Bailian Token Plan subscription.

use crate::agent::{Res, shifu_home};
use crate::launch::hook_command;
use serde_json::{Value, json};
use std::path::{Path, PathBuf};
use std::process::Command;

pub const MODELS: &str = include_str!("../integrations/qwen/models.json");
pub const BASE_URL: &str = "https://token-plan.cn-beijing.maas.aliyuncs.com/compatible-mode/v1";

pub fn settings(shifu: &str) -> Value {
    let mut hooks = serde_json::Map::new();
    for event in ["SessionStart", "UserPromptSubmit", "PreToolUse", "PostToolUse", "Stop", "StopFailure", "SessionEnd"] {
        hooks.insert(event.to_string(), json!([{"hooks":[{
            "type":"command", "name":format!("shifu-{event}"),
            "command":hook_command(shifu, "qwen", event), "timeout":5
        }]}]));
    }
    let models: Vec<Value> = serde_json::from_str(MODELS).expect("embedded Qwen catalog");
    let providers: Vec<Value> = models.iter().map(|model| json!({
        "id":model["id"], "name":model["label"], "baseUrl":BASE_URL, "envKey":"DASHSCOPE_API_KEY"
    })).collect();
    json!({
        "$version":4,
        "general":{"enableAutoUpdate":false},
        "context":{"fileName":["AGENTS.md","CLAUDE.md","QWEN.md"]},
        "security":{"auth":{"selectedType":"openai"}},
        "modelProviders":{"openai":providers},
        "model":{"name":"auto"},
        "hooks":hooks,
        "telemetry":{"enabled":false},
        "privacy":{"usageStatisticsEnabled":false}
    })
}

fn write_settings(path: &Path, text: &str) -> Res<()> {
    // Concurrent sessions share the same settings; an atomic replacement avoids partial JSON reads.
    let temporary = path.with_extension(format!("{}.tmp", std::process::id()));
    std::fs::write(&temporary, text).map_err(|e| e.to_string())?;
    std::fs::rename(&temporary, path).map_err(|e| e.to_string())
}

// tmux starts agents through /bin/sh without loading .zshrc. If no key is inherited,
// use the optional local env file before asking the compatible vault launcher.
fn key_from_env_file() -> Option<String> {
    let path = PathBuf::from(std::env::var_os("HOME")?).join(".config/secrets/dashscope.env");
    let text = std::fs::read_to_string(path).ok()?;
    for line in text.lines() {
        let line = line.trim();
        let rest = line.strip_prefix("export ").unwrap_or(line);
        let Some(value) = rest.strip_prefix("DASHSCOPE_API_KEY=") else { continue };
        let value = value.trim();
        let value = value.strip_prefix('"').and_then(|v| v.strip_suffix('"'))
            .or_else(|| value.strip_prefix('\'').and_then(|v| v.strip_suffix('\'')))
            .unwrap_or(value);
        if !value.is_empty() { return Some(value.to_string()); }
    }
    None
}

pub fn run(args: &[String]) -> Res<()> {
    let program = std::env::var_os("SHIFU_QWEN_BIN").unwrap_or_else(|| "qwen".into());
    if args.len() == 1 && matches!(args[0].as_str(), "--help" | "-h" | "--version" | "-v") {
        let mut cmd = Command::new(program);
        cmd.args(args);
        return execute(cmd);
    }
    // The key is for this subscription endpoint; never send it to a URL supplied through extra arguments.
    if args.iter().any(|a| ["--openai-api-key", "--openai-base-url", "--auth-type"].iter()
        .any(|flag| a == flag || a.starts_with(&format!("{flag}=")))) {
        return Err("Qwen 入口固定使用百炼 Token Plan，不能用额外参数替换鉴权或接口地址".into());
    }
    let executable = std::env::current_exe().map_err(|e| e.to_string())?;
    let env_key_missing = std::env::var_os("DASHSCOPE_API_KEY").is_none_or(|key| key.is_empty());
    let file_key = if env_key_missing { key_from_env_file() } else { None };
    if env_key_missing && file_key.is_none() {
        let vault = PathBuf::from(std::env::var_os("HOME").unwrap_or_default()).join(".local/bin/hk");
        let mut cmd = Command::new(vault);
        cmd.args(["run", "Qwen · Shifu", "--"]).arg(executable).arg("qwen").args(args);
        return execute(cmd);
    }
    let home = shifu_home().join("qwen");
    std::fs::create_dir_all(&home).map_err(|e| e.to_string())?;
    let config = home.join("shifu-settings.json");
    write_settings(&config, &serde_json::to_string_pretty(&settings(&executable.to_string_lossy())).map_err(|e| e.to_string())?)?;
    let mut cmd = Command::new(program);
    if let Some(key) = &file_key {
        cmd.env("DASHSCOPE_API_KEY", key);
    }
    cmd.env("QWEN_HOME", &home)
        .env("QWEN_RUNTIME_DIR", &home)
        .env("QWEN_CODE_SYSTEM_SETTINGS_PATH", &config)
        .env("OPENAI_BASE_URL", BASE_URL)
        .arg("--approval-mode").arg("yolo")
        .args(args);
    if !args.iter().any(|a| a == "--model" || a == "-m" || a.starts_with("--model=")) {
        cmd.args(["--model", "auto"]);
    }
    execute(cmd)
}

fn execute(mut cmd: Command) -> Res<()> {
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        Err(cmd.exec().to_string())
    }
    #[cfg(not(unix))]
    {
        let status = cmd.status().map_err(|e| e.to_string())?;
        if status.success() { Ok(()) } else { Err(format!("Qwen Code 退出: {status}")) }
    }
}
