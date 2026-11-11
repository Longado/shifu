//! Bounded observation and explicit postconditions. Actions are never replayed.
use serde_json::{Value, json};
use std::path::Path;
use std::time::{Duration, Instant};

const OPTIONS: [&str; 7] = ["match", "limit", "waitFor", "timeoutMs", "expectPath", "expectMissingPath", "foregroundFallback"];

pub fn validate(method: &str, params: &Value) -> Result<(), String> {
    let waiting = ["waitFor", "expectPath", "expectMissingPath"].iter().any(|key| !params[key].is_null());
    if matches!(method, "handshake" | "listApps" | "listWindows" | "stop") && OPTIONS.iter().any(|key| !params[key].is_null()) {
        return Err("筛选和等待参数只用于窗口读取或窗口操作".into());
    }
    for key in ["match", "waitFor", "expectPath", "expectMissingPath"] {
        if params[key].as_str().is_some_and(|s| s.trim().is_empty()) { return Err(format!("{key} 不能为空")); }
    }
    if params["limit"].as_u64().is_some_and(|n| n == 0 || n > 1200) { return Err("--limit 范围是 1 到 1200".into()); }
    if params["timeoutMs"].as_u64().is_some_and(|n| n > 30000) { return Err("--timeout-ms 最多 30000 毫秒".into()); }
    if !params["timeoutMs"].is_null() && !waiting { return Err("--timeout-ms 需要等待文字或文件条件".into()); }
    if params["foregroundFallback"] == true && (method != "getAppState" || !waiting || params["restoreWindow"] == true) {
        return Err("--foreground-fallback 只用于带等待条件的 get-app-state，且不能和 --restore-window 同用".into());
    }
    for key in ["expectPath", "expectMissingPath"] {
        if let Some(path) = params[key].as_str()
            && !Path::new(path).is_absolute() { return Err(format!("{key} 需要绝对路径")); }
    }
    if !params["expectPath"].is_null() && params["expectPath"] == params["expectMissingPath"] {
        return Err("存在和不存在条件不能指向同一路径".into());
    }
    Ok(())
}

fn control_lines(tree: &str) -> impl Iterator<Item = &str> {
    tree.lines().filter(|line| {
        let text = line.trim_start();
        text.split_once(' ').is_some_and(|(index, _)| !index.is_empty() && index.bytes().all(|c| c.is_ascii_digit()))
    })
}

fn conditions(options: &Value, response: &Value) -> Result<Vec<Value>, String> {
    let mut checks = Vec::new();
    if let Some(text) = options["waitFor"].as_str() {
        let found = response["result"]["snapshot"]["treeText"].as_str().is_some_and(|tree| control_lines(tree).any(|l| l.contains(text)));
        checks.push(json!({"kind":"text", "expected":text, "met":found}));
    }
    for (key, expected) in [("expectPath", true), ("expectMissingPath", false)] {
        if let Some(path) = options[key].as_str() {
            // symlink_metadata observes the named entry, including dangling symlinks.
            let exists = match std::fs::symlink_metadata(path) {
                Ok(_) => true,
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => false,
                Err(e) => return Err(format!("无法核对路径 {path}：{e}")),
            };
            checks.push(json!({"kind":"path", "path":path, "exists":exists, "expectedExists":expected, "met":exists == expected}));
        }
    }
    Ok(checks)
}

pub fn execute(mut request: Value, mut call: impl FnMut(Value) -> Result<Value, String>) -> Result<Value, String> {
    let mut options = json!({});
    for key in OPTIONS {
        if let Some(value) = request["params"].as_object_mut().unwrap().remove(key) { options[key] = value; }
    }
    let waiting = ["waitFor", "expectPath", "expectMissingPath"].iter().any(|key| !options[key].is_null());
    let timeout = Duration::from_millis(options["timeoutMs"].as_u64().unwrap_or(5000));
    let start = Instant::now();
    let is_read = request["method"] == "getAppState";
    let mut response = call(request.clone())?;
    let action = response["result"]["action"].clone();
    let mut used_foreground = false;
    loop {
        let retryable = is_read && matches!(response["error"]["code"].as_str(), Some("window_not_found" | "app_not_found"));
        if response["ok"] != true && !retryable { break; }
        if !waiting { break; }
        let checks = conditions(&options, &response)?;
        let met = response["ok"] == true && checks.iter().all(|c| c["met"] == true);
        if met || start.elapsed() >= timeout || used_foreground {
            if !response["result"].is_object() { response["result"] = json!({}); }
            response["result"]["postcondition"] = json!({"state":if met {"met"} else {"timeout"},"checks":checks,"foregroundFallbackUsed":used_foreground,"elapsedMs":start.elapsed().as_millis() as u64});
            if !met {
                response["ok"] = json!(false);
                response["error"] = json!({"code":"postcondition_timeout","message":"未观察到要求的结果；动作不会自动重做，请检查当前状态"});
            }
            break;
        }
        // Keep polling the same window. A mutating command is dispatched exactly once.
        let window = response["result"]["snapshot"]["window"]["id"].clone();
        if !window.is_null() {
            request["params"].as_object_mut().unwrap().remove("windowIndex");
            request["params"]["windowId"] = window;
        }
        request["method"] = json!("getAppState");
        request["params"].as_object_mut().unwrap().retain(|k, _| matches!(k.as_str(), "app" | "session" | "windowId" | "windowIndex" | "noScreenshot"));
        let interval = if options["foregroundFallback"] == true { (timeout / 2).min(Duration::from_millis(200)) } else { Duration::from_millis(200) };
        std::thread::sleep(interval.min(timeout.saturating_sub(start.elapsed())));
        if options["foregroundFallback"] == true && start.elapsed() >= timeout / 2 {
            request["params"]["briefForeground"] = json!(true);
            used_foreground = true;
        }
        response = call(request.clone())?;
    }
    if !action.is_null() {
        if !response["result"].is_object() { response["result"] = json!({}); }
        response["result"]["action"] = action;
        if response["result"]["action"]["verification"].is_object() {
            response["result"]["action"]["verification"]["scope"] = json!("element");
        }
        response["result"]["action"]["taskCompletion"] = json!("not_checked");
    }
    if !options["match"].is_null() || !options["limit"].is_null() {
        let snap = &mut response["result"]["snapshot"];
        if let Some(tree) = snap["treeText"].as_str() {
            let needle = options["match"].as_str().unwrap_or("").to_lowercase();
            let lines: Vec<_> = control_lines(tree).filter(|l| l.to_lowercase().contains(&needle)).collect();
            let matched = lines.len();
            let limit = options["limit"].as_u64().unwrap_or(1200) as usize;
            let text = lines.iter().take(limit).copied().collect::<Vec<_>>().join("\n");
            snap["treeText"] = json!(text);
            snap["view"] = json!({"filtered":true,"matched":matched,"shown":matched.min(limit),"omitted":matched.saturating_sub(limit),"indexesPreserved":true});
        }
    }
    Ok(response)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn request(method: &str, options: Value) -> Value {
        let mut params = json!({"app":"test", "noScreenshot":true});
        params.as_object_mut().unwrap().extend(options.as_object().unwrap().clone());
        json!({"id":1,"method":method,"params":params})
    }
    fn snapshot(tree: &str) -> Value {
        json!({"ok":true,"result":{"snapshot":{"treeText":tree,"elementCount":4,"window":{"id":23}}}})
    }
    #[test]
    fn filtered_output_preserves_indexes_and_reports_omissions() {
        let r = execute(request("getAppState", json!({"match":"保存","limit":1})), |_| Ok(snapshot("App=x\nWindow: 保存\n\n0 window\n\t17 button 保存\n\t28 button 另存保存\nThe focused UI element is 17 button 保存."))).unwrap();
        assert_eq!(r["result"]["snapshot"]["treeText"], "\t17 button 保存");
        assert_eq!(r["result"]["snapshot"]["view"]["matched"], 2);
        assert_eq!(r["result"]["snapshot"]["view"]["shown"], 1);
    }
    #[test]
    fn waits_for_control_not_window_title_and_never_repeats_action() {
        let mut methods = Vec::new();
        let r = execute(request("click", json!({"waitFor":"Ready","timeoutMs":1000,"match":"Done"})), |q| {
            methods.push(q["method"].as_str().unwrap().to_owned());
            assert!(q["params"].get("waitFor").is_none());
            if methods.len() == 1 {
                let mut s = snapshot("Window: Ready\n0 window");
                s["result"]["action"] = json!({"verification":{"state":"verified"}}); Ok(s)
            } else {
                assert_eq!(q["params"]["windowId"], 23);
                Ok(snapshot("0 window\n8 text Ready\n9 text Done"))
            }
        }).unwrap();
        assert_eq!(methods, ["click","getAppState"]);
        assert_eq!(r["result"]["postcondition"]["state"], "met");
        assert_eq!(r["result"]["action"]["verification"]["scope"], "element");
    }
    #[test]
    fn verified_element_does_not_hide_a_failed_file_condition() {
        let missing = std::env::temp_dir().join(format!("shifu-not-created-{}", std::process::id()));
        let r = execute(request("setValue", json!({"expectPath":missing,"timeoutMs":0})), |_| {
            let mut s = snapshot("0 text field new.txt");
            s["result"]["action"] = json!({"verification":{"state":"verified"}}); Ok(s)
        }).unwrap();
        assert_eq!(r["ok"], false);
        assert_eq!(r["error"]["code"], "postcondition_timeout");
        assert_eq!(r["result"]["action"]["verification"]["state"], "verified");
    }
    #[test]
    fn stops_on_permission_error_without_retry() {
        let mut calls = 0;
        let r = execute(request("getAppState", json!({"waitFor":"Ready"})), |_| {
            calls += 1; Ok(json!({"ok":false,"error":{"code":"permission_denied"}}))
        }).unwrap();
        assert_eq!(calls, 1); assert_eq!(r["error"]["code"], "permission_denied");
    }
    #[test]
    fn fallback_is_explicit_once_and_only_reads() {
        let mut calls = 0;
        let r = execute(request("getAppState", json!({"waitFor":"body","timeoutMs":20,"foregroundFallback":true})), |q| {
            calls += 1;
            assert_eq!(q["method"], "getAppState");
            if q["params"]["briefForeground"] == true { Ok(snapshot("2 text body")) }
            else { Ok(snapshot("0 window")) }
        }).unwrap();
        assert_eq!(r["result"]["postcondition"]["state"], "met");
        assert_eq!(r["result"]["postcondition"]["foregroundFallbackUsed"], true);
        assert_eq!(calls, 2);
    }
    #[test]
    fn window_not_ready_can_appear_but_actions_are_not_retried_on_error() {
        let mut calls = 0;
        let r = execute(request("getAppState", json!({"waitFor":"Ready","timeoutMs":1000})), |_| {
            calls += 1;
            if calls == 1 { Ok(json!({"ok":false,"error":{"code":"window_not_found"}})) }
            else { Ok(snapshot("9 text Ready")) }
        }).unwrap();
        assert_eq!(r["ok"], true); assert_eq!(calls, 2);
        let mut calls = 0;
        let r = execute(request("click", json!({"waitFor":"Ready"})), |_| {
            calls += 1; Ok(json!({"ok":false,"error":{"code":"window_not_found"}}))
        }).unwrap();
        assert_eq!(r["ok"], false); assert_eq!(calls, 1);
    }
    #[test]
    fn rejects_invalid_or_ignored_options() {
        for (method, params) in [
            ("getAppState",json!({"limit":0})),
            ("getAppState",json!({"waitFor":""})),
            ("getAppState",json!({"timeoutMs":30001,"waitFor":"x"})),
            ("getAppState",json!({"timeoutMs":100})),
            ("click",json!({"foregroundFallback":true,"waitFor":"x"})),
            ("listApps",json!({"match":"x"})),
            ("getAppState",json!({"expectPath":"relative/path"})),
        ] { assert!(validate(method,&params).is_err(), "{method}: {params}"); }
    }
}
