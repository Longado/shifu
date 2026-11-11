//! CLI boundary for Shifu's independent macOS computer-use helper.
use serde_json::{Value, json};
use std::collections::HashSet;
use std::io::Read;

#[cfg(target_os = "macos")]
mod runtime;
mod observation;

pub const HELP: &str = "shifu computer — 操作本机窗口，不依赖 Orca

  capabilities / list-apps / permissions / stop
  list-windows --app <bundle-id|pid:PID|应用名>
  get-app-state --app <应用> [--window-id ID] [--no-screenshot]
    [--match 文字] [--limit 数量] [--wait-for 控件文字] [--timeout-ms 5000]
    [--foreground-fallback]
  click --app <应用> --element-index N
  click --app <应用> --x X --y Y
  set-value --app <应用> --element-index N --value <文字>
  type-text / paste-text --app <应用> --text <文字>
  press-key / hotkey --app <应用> --key <Return|CmdOrCtrl+A>
  scroll --app <应用> --element-index N --direction <up|down|left|right>
  drag --app <应用> --from-element-index N --to-element-index N

所有操作返回 JSON；截图保存为本地文件。先读窗口再使用返回的控件索引。
--text-stdin / --value-stdin 从标准输入读文字。--session 指定独立操作上下文。
--match / --limit 缩小控件输出，保留原索引。--wait-for 等待控件文字出现。
--expect-path / --expect-missing-path 核对绝对路径；超时报告失败，不重做动作。
--foreground-fallback 只在等待读取超时前短暂前台读取一次，再还原原应用。
默认后台操作；只有明确需要抢前台时才加 --restore-window。
首次使用运行 permissions，在系统设置给 Shifu Computer Use 授权。";

pub fn run(args: &[String]) -> Result<(), String> {
    if args.is_empty() || matches!(args[0].as_str(), "help" | "--help" | "-h") {
        println!("{HELP}");
        return Ok(());
    }
    #[cfg(target_os = "macos")]
    {
        if args[0] == "_serve" { return runtime::serve(); }
        if args[0] == "permissions" { return runtime::permissions(); }
        let namespace = std::env::var("SHIFU_ID").unwrap_or_else(|_| "default".into());
        let r = request(args, &namespace)?;
        let response = observation::execute(r, runtime::call)?;
        println!("{}", serde_json::to_string_pretty(&response).map_err(|e| e.to_string())?);
        if response["ok"] == false { return Err("操作未完成，原因见上方 error".into()); }
        Ok(())
    }
    #[cfg(not(target_os = "macos"))]
    Err("computer use 目前只支持 macOS".into())
}

pub fn request(args: &[String], namespace: &str) -> Result<Value, String> {
    let command = args.first().ok_or("缺少 computer 子命令")?.as_str();
    let method = match command {
        "capabilities" => "handshake", "list-apps" => "listApps", "list-windows" => "listWindows",
        "get-app-state" => "getAppState", "click" => "click", "set-value" => "setValue",
        "type-text" => "typeText", "paste-text" => "pasteText", "press-key" => "pressKey",
        "hotkey" => "hotkey", "scroll" => "scroll", "drag" => "drag",
        "perform-secondary-action" => "performSecondaryAction", "stop" => "stop",
        _ => return Err(format!("不认识的 computer 子命令 {command}")),
    };
    let mut params = json!({"session": namespace});
    let mut seen = HashSet::new();
    let mut words = args[1..].iter();
    while let Some(flag) = words.next() {
        let (key, kind) = match flag.as_str() {
            "--json" => continue,
            "--app" => ("app", "text"), "--session" => ("session", "text"),
            "--window-id" => ("windowId", "integer"), "--window-index" => ("windowIndex", "integer"),
            "--element-index" => ("elementIndex", "integer"),
            "--from-element-index" => ("fromElementIndex", "integer"), "--to-element-index" => ("toElementIndex", "integer"),
            "--x" => ("x", "number"), "--y" => ("y", "number"),
            "--from-x" => ("fromX", "number"), "--from-y" => ("fromY", "number"),
            "--to-x" => ("toX", "number"), "--to-y" => ("toY", "number"),
            "--direction" => ("direction", "text"), "--pages" => ("pages", "number"),
            "--text" => ("text", "text"), "--value" => ("value", "text"), "--key" => ("key", "text"),
            "--action" => ("action", "text"), "--mouse-button" => ("mouseButton", "text"),
            "--modifiers" => ("modifiers", "text"), "--click-count" => ("clickCount", "integer"),
            "--no-screenshot" => ("noScreenshot", "bool"), "--restore-window" => ("restoreWindow", "bool"),
            "--match" => ("match", "text"), "--limit" => ("limit", "integer"),
            "--wait-for" => ("waitFor", "text"), "--timeout-ms" => ("timeoutMs", "integer"),
            "--expect-path" => ("expectPath", "text"), "--expect-missing-path" => ("expectMissingPath", "text"),
            "--foreground-fallback" => ("foregroundFallback", "bool"),
            "--text-stdin" => ("text", "stdin"), "--value-stdin" => ("value", "stdin"),
            _ => return Err(format!("不认识的选项 {flag}")),
        };
        if !seen.insert(key) { return Err(format!("选项重复：{flag}")); }
        let value = match kind {
            "bool" => json!(true),
            "stdin" => {
                let mut text = String::new();
                std::io::stdin().take(1024 * 1024 + 1).read_to_string(&mut text).map_err(|e| e.to_string())?;
                if text.len() > 1024 * 1024 { return Err("输入文字超过 1 MB".into()); }
                json!(text)
            }
            _ => {
                let text = words.next().ok_or_else(|| format!("{flag} 缺少值"))?;
                match kind {
                    "integer" => json!(text.parse::<u32>().map_err(|_| format!("{flag} 需要非负整数"))?),
                    "number" => {
                        let n = text.parse::<f64>().map_err(|_| format!("{flag} 需要数字"))?;
                        if !n.is_finite() || n < 0.0 { return Err(format!("{flag} 需要非负有限数字")); }
                        json!(n)
                    }
                    _ => json!(text),
                }
            }
        };
        params[key] = value;
    }
    if !matches!(command, "capabilities" | "list-apps" | "stop") && params["app"].as_str().is_none_or(|s| s.trim().is_empty()) {
        return Err("请用 --app 明确要操作的应用".into());
    }
    if seen.contains("windowId") && seen.contains("windowIndex") {
        return Err("--window-id 和 --window-index 只能选一个".into());
    }
    observation::validate(method, &params)?;
    Ok(json!({"id": 1, "method": method, "params": params}))
}

#[cfg(test)]
mod tests {
    use super::*;
    fn args(words: &[&str]) -> Vec<String> { words.iter().map(|s| s.to_string()).collect() }

    #[test]
    fn window_observation_preserves_target_and_agent_namespace() {
        let r = request(&args(&["get-app-state", "--app", "pid:123", "--window-id", "42", "--no-screenshot"]), "agent-a").unwrap();
        assert_eq!(r["method"], "getAppState");
        assert_eq!(r["params"], json!({"app":"pid:123", "windowId":42, "noScreenshot":true, "session":"agent-a"}));
    }

    #[test]
    fn semantic_action_preserves_literal_text() {
        let r = request(&args(&["set-value", "--app", "TextEdit", "--element-index", "7", "--value", "中文\n$(not-a-shell)" ]), "a").unwrap();
        assert_eq!(r["params"]["value"], "中文\n$(not-a-shell)");
        assert_eq!(r["params"]["elementIndex"], 7);
        assert_eq!(r["method"], "setValue");
    }

    #[test]
    fn rejects_ambiguous_or_invalid_targets_before_starting_helper() {
        for words in [
            vec!["click", "--element-index", "3"],
            vec!["click", "--app", "x", "--element-index", "-1"],
            vec!["click", "--app", "x", "--window-id", "3", "--window-index", "0"],
            vec!["click", "--app", "x", "--x", "nan", "--y", "2"],
            vec!["click", "--app", "x", "--typo"],
            vec!["click", "--app", "x", "--app", "y"],
        ] { assert!(request(&args(&words), "a").is_err(), "{words:?}"); }
    }

    #[test]
    fn global_reads_do_not_need_a_target() {
        assert_eq!(request(&args(&["capabilities", "--json"]), "a").unwrap()["method"], "handshake");
        assert_eq!(request(&args(&["list-apps"]), "a").unwrap()["method"], "listApps");
    }
}
