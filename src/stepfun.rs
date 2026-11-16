//! StepFun uses an isolated Pi runtime; credentials enter only through hk's child environment.

use crate::agent::{Res, is_safe_name, shifu_home};
use std::path::PathBuf;
use std::process::Command;

pub const MODELS: &str = include_str!("../integrations/stepfun/models.json");
const EXTENSION: &str = include_str!("../integrations/stepfun/extension.mjs");

pub fn runtime_path() -> PathBuf {
    std::env::var_os("SHIFU_STEPFUN_RUNTIME").map(PathBuf::from).unwrap_or_else(|| {
        PathBuf::from(std::env::var_os("HOME").unwrap_or_default())
            .join(".local/share/shifu/stepfun/node_modules/@mariozechner/pi-coding-agent/dist/cli.js")
    })
}

pub fn run(args: &[String]) -> Res<()> {
    let runtime = runtime_path();
    if !runtime.is_file() {
        return Err("StepFun 会话引擎尚未准备，请按 integrations/stepfun/README.md 配置 Pi 0.73.1".into());
    }
    let executable = std::env::current_exe().map_err(|e| e.to_string())?;
    if std::env::var_os("STEP_API_KEY").is_none_or(|key| key.is_empty()) {
        // Re-enter only after the vault has supplied the environment. No credential appears in argv or files.
        let vault = PathBuf::from(std::env::var_os("HOME").unwrap_or_default()).join(".local/bin/hk");
        let mut cmd = Command::new(vault);
        cmd.args(["run", "StepFun · Shifu", "--"]).arg(&executable).arg("stepfun").args(args);
        return execute(cmd);
    }
    let home = shifu_home().join("stepfun");
    let sessions = home.join("sessions");
    std::fs::create_dir_all(&sessions).map_err(|e| e.to_string())?;
    let extension = home.join("extension.mjs");
    std::fs::write(&extension, EXTENSION).map_err(|e| e.to_string())?;
    std::fs::write(home.join("catalog.json"), MODELS).map_err(|e| e.to_string())?;
    let mut cmd = Command::new("node");
    cmd.arg(runtime).args(["--no-extensions", "--no-skills", "--no-prompt-templates", "--no-themes"])
        .arg("--extension").arg(extension).args(["--provider", "stepfun", "--model", "step-3.7-flash"])
        .arg("--session-dir").arg(&sessions)
        .env("PI_CODING_AGENT_DIR", &home)
        .env("PI_OFFLINE", "1")
        .env("PI_TELEMETRY", "0")
        .env("SHIFU_STEPFUN_BIN", executable);
    if !args.iter().any(|a| a == "--session") {
        if let Ok(id) = std::env::var("SHIFU_ID") {
            if !is_safe_name(&id) { return Err("SHIFU_ID 不是合法名字".into()); }
            cmd.arg("--session").arg(sessions.join(format!("{id}.jsonl")));
        }
    }
    cmd.args(args);
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
        if status.success() { Ok(()) } else { Err(format!("StepFun 退出: {status}")) }
    }
}
