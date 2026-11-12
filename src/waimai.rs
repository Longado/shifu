//! Bridge to the existing local ordering service; credentials stay with its client.
use std::path::{Path, PathBuf};
use std::process::Command;
use serde::Deserialize;

pub const URL: &str = "http://127.0.0.1:3816/";
#[derive(Deserialize)]
pub struct Connection { pub tool_script: PathBuf }
pub fn load(home: &Path) -> Result<Connection, String> {
    let text = std::fs::read_to_string(home.join("waimai.json"))
        .map_err(|_| "尚未连接点餐系统，请配置本机点餐入口".to_string())?;
    let config: Connection = serde_json::from_str(&text).map_err(|_| "点餐入口配置无法读取".to_string())?;
    if !config.tool_script.is_absolute() || !config.tool_script.is_file() {
        return Err("点餐工具路径不存在或不是绝对路径".to_string());
    }
    Ok(config)
}
pub fn run(args: &[String]) -> Result<(), String> {
    if args.len() > 2 { return Err("用法：shifu waimai [工具名] [JSON参数]".into()); }
    if let Some(input) = args.get(1) { serde_json::from_str::<serde_json::Value>(input).map_err(|_| "点餐参数必须是JSON".to_string())?; }
    let home = std::env::var("SHIFU_APP_HOME").map(PathBuf::from).unwrap_or_else(|_| PathBuf::from(std::env::var("HOME").unwrap_or_default()).join(".shifu/app"));
    let config = load(&home)?;
    let status = Command::new("node").arg(config.tool_script).args(args).status().map_err(|_| "无法启动点餐工具，请检查Node.js".to_string())?;
    if status.success() { Ok(()) } else { Err("点餐工具未完成请求，原因见上方返回；不要重复提交购买".into()) }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test] fn missing_connection_is_explicit() {
        assert!(load(Path::new("/nonexistent-shifu-waimai-test")).is_err());
    }
    #[test] fn malformed_arguments_do_not_start_a_client() {
        assert!(run(&["get_status".into(), "bad-json".into()]).unwrap_err().contains("JSON"));
        assert!(run(&["a".into(), "b".into(), "c".into()]).is_err());
    }
}
