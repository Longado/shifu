//! Optional Node/Playwright runtime; the ordinary CLI has no browser dependency.
use crate::agent::Res;
use std::path::PathBuf;
use std::process::Command;

fn runtime() -> Res<PathBuf> {
    if let Some(path) = std::env::var_os("SHIFU_WEB_RUNTIME") {
        let path = PathBuf::from(path);
        return path.is_file().then_some(path).ok_or_else(|| "SHIFU_WEB_RUNTIME 没有指向网页任务池入口。".into());
    }
    if let Ok(executable) = std::env::current_exe() {
        let bundled = executable.parent().unwrap_or(&executable).join("web/cli.mjs");
        if bundled.is_file() { return Ok(bundled); }
    }
    let source = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("web/cli.mjs");
    source.is_file().then_some(source).ok_or_else(|| "未找到网页运行入口；请使用包含web目录的安装包。".into())
}

pub fn run(args: &[String]) -> Res<()> {
    let path = runtime().map_err(|error| unavailable(args, error))?;
    let status = Command::new("node").arg(path).args(args).status()
        .map_err(|error| unavailable(args, format!("无法启动网页入口，请确认Node.js已安装：{error}")))?;
    if status.success() { Ok(()) } else { Err("网页请求未成功；上方JSON说明是否已执行，不要自动重发修改动作。".into()) }
}

fn unavailable(args: &[String], message: String) -> String {
    if args.first().is_some_and(|command| command == "request") {
        println!("{}", serde_json::json!({"ok": false, "error": {"code": "RUNTIME_UNAVAILABLE", "message": message, "phase": "not_dispatched"}}));
    }
    message
}
