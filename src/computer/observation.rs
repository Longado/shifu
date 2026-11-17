//! Bounded observation and explicit postconditions. Actions are never replayed.
use serde_json::{Value, json};
use std::path::Path;
use std::time::{Duration, Instant};

const OPTIONS: [&str; 11] = ["match", "limit", "role", "within", "ancestors", "waitFor", "waitForGone", "timeoutMs", "expectPath", "expectMissingPath", "foregroundFallback"];

pub fn validate(method: &str, params: &Value) -> Result<(), String> {
    if params["backgroundOnly"] == true {
        if params["restoreWindow"] == true || params["foregroundFallback"] == true || params["briefForeground"] == true {
            return Err("--background-only 不能使用前台恢复或前台读取兜底".into());
        }
        if !matches!(method, "handshake" | "listApps" | "listWindows" | "getAppState" | "setValue") {
            return Err("--background-only 仅支持读取和文本框 set-value；当前动作可能改变焦点、打开窗口或使用合成输入／剪贴板，未执行".into());
        }
    }
    let waiting = ["waitFor", "waitForGone", "expectPath", "expectMissingPath"].iter().any(|key| !params[key].is_null());
    if !params["waitFor"].is_null() && params["waitFor"] == params["waitForGone"] {
        return Err("等待出现和消失的文字不能相同".into());
    }
    if matches!(method, "handshake" | "listApps" | "listWindows" | "stop") && (OPTIONS.iter().any(|key| !params[key].is_null()) || params["fullTree"] == true) {
        return Err("筛选和等待参数只用于窗口读取或窗口操作".into());
    }
    for key in ["match", "role", "waitFor", "waitForGone", "expectPath", "expectMissingPath"] {
        if params[key].as_str().is_some_and(|s| s.trim().is_empty()) { return Err(format!("{key} 不能为空")); }
    }
    if params["limit"].as_u64().is_some_and(|n| n == 0 || n > 1200) { return Err("--limit 范围是 1 到 1200".into()); }
    if params["ancestors"] == true && params["match"].is_null() && params["role"].is_null() { return Err("--ancestors 需要和 --match 或 --role 一起用".into()); }
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

const FILTERS: [&str; 4] = ["match", "limit", "role", "within"];

fn index_of(line: &str) -> Option<u64> { line.trim_start().split_once(' ')?.0.parse().ok() }
fn depth(line: &str) -> usize { line.bytes().take_while(|b| *b == b'\t').count() }

/// Words macOS puts in front of a type, so `--role button` also finds "radio button" and "pop up button".
const QUALIFIERS: [&str; 21] = ["menu", "radio", "pop", "up", "toggle", "disclosure", "default", "search", "secure", "tab", "check", "combo",
    "close", "minimize", "zoom", "full", "screen", "increment", "decrement", "arrow", "page"];

/// The type is the leading words after the index ("button", "radio button", "pop up button"); a
/// label that merely contains the word does not count. At most two known qualifiers may precede it.
fn has_role(line: &str, roles: &[String]) -> bool {
    let text = line.trim_start().split_once(' ').map_or("", |(_, rest)| rest).to_lowercase();
    let words: Vec<&str> = text.split(|c: char| !c.is_alphanumeric()).filter(|w| !w.is_empty()).collect();
    roles.iter().any(|role| {
        let want: Vec<&str> = role.split_whitespace().collect();
        (0..=2).any(|skip| words.len() >= skip + want.len() && words[skip..skip + want.len()] == want[..]
            && words[..skip].iter().all(|w| QUALIFIERS.contains(w)))
    })
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
    if let Some(text) = options["waitForGone"].as_str() {
        let snap = &response["result"]["snapshot"];
        let absent = snap["treeText"].as_str().is_some_and(|tree| control_lines(tree).next().is_some() && !control_lines(tree).any(|l| l.contains(text)));
        let complete = snap["truncation"]["truncated"] == false && snap["truncation"]["maxDepthReached"] == false;
        checks.push(json!({"kind":"text_absent", "expected":text, "met":absent && complete}));
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
    if request["params"]["backgroundOnly"] == true && request["method"] != "handshake" {
        // Older helpers ignore unknown parameters. Verify support before sending the target request.
        let capabilities = call(json!({"id":1,"method":"handshake","params":{}}))?;
        if capabilities["ok"] == false { return Ok(capabilities); }
        if capabilities["ok"] != true || capabilities["result"]["supports"]["input"]["backgroundOnly"] != true {
            return Ok(json!({"ok":false,"error":{
                "code":"background_only_unsupported",
                "message":"当前辅助程序不支持严格后台模式，目标请求未发送；请使用匹配的新辅助程序"
            },"result":{"recovery":{"automaticRetry":false,"actionReported":false,"nextStep":"stop"}}}));
        }
    }
    let mut options = json!({});
    for key in OPTIONS {
        if let Some(value) = request["params"].as_object_mut().unwrap().remove(key) { options[key] = value; }
    }
    let waiting = ["waitFor", "waitForGone", "expectPath", "expectMissingPath"].iter().any(|key| !options[key].is_null());
    let filtered = FILTERS.iter().any(|key| !options[key].is_null());
    if waiting || filtered { request["params"]["fullTree"] = json!(true); }
    // A client that saw only matching rows cannot reconstruct the next delta.
    if filtered { request["params"]["treeViewFiltered"] = json!(true); }
    let timeout = Duration::from_millis(options["timeoutMs"].as_u64().unwrap_or(5000));
    let start = Instant::now();
    let is_read = request["method"] == "getAppState";
    let explicit_window = !request["params"]["windowId"].is_null() || !request["params"]["windowIndex"].is_null();
    let mut response = call(request.clone())?;
    let action = response["result"]["action"].clone();
    let mut used_foreground = false;
    loop {
        let retryable = is_read && matches!(response["error"]["code"].as_str(), Some("window_not_found" | "app_not_found"));
        if response["ok"] != true && !retryable { break; }
        if !waiting { break; }
        if explicit_window && (!options["waitFor"].is_null() || !options["waitForGone"].is_null()) {
            let target_window = action["targetWindowId"].as_u64();
            let observed_window = response["result"]["snapshot"]["window"]["id"].as_u64();
            if let (Some(target), Some(observed)) = (target_window, observed_window)
                && target != observed
            {
                response["ok"] = json!(false);
                response["error"] = json!({
                    "code":"observation_failed",
                    "message":"观察窗口与动作目标窗口不一致"
                });
                response["result"]["postcondition"] = json!({"state":"observation_failed"});
                break;
            }
        }
        let checks = match conditions(&options, &response) {
            Ok(checks) => checks,
            Err(message) if !action.is_null() => {
                response["ok"] = json!(false);
                response["error"] = json!({"code":"observation_failed","message":message});
                response["result"]["postcondition"] = json!({"state":"observation_failed"});
                break;
            }
            Err(message) => return Err(message),
        };
        let met = response["ok"] == true && checks.iter().all(|c| c["met"] == true);
        if met || start.elapsed() >= timeout {
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
        request["params"].as_object_mut().unwrap().retain(|k, _| matches!(k.as_str(), "app" | "session" | "windowId" | "windowIndex" | "noScreenshot" | "fullTree" | "treeViewFiltered" | "backgroundOnly"));
        let interval = if options["foregroundFallback"] == true { (timeout / 2).min(Duration::from_millis(200)) } else { Duration::from_millis(200) };
        std::thread::sleep(interval.min(timeout.saturating_sub(start.elapsed())));
        if !used_foreground && options["foregroundFallback"] == true && start.elapsed() >= timeout / 2 {
            request["params"]["briefForeground"] = json!(true);
            used_foreground = true;
        }
        response = match call(request.clone()) {
            Ok(response) => response,
            Err(message) if !action.is_null() => {
                json!({"ok":false,"error":{"code":"observation_failed","message":message},"result":{"postcondition":{"state":"observation_failed"}}})
            }
            Err(message) => return Err(message),
        };
    }
    if !action.is_null() {
        if !response["result"].is_object() { response["result"] = json!({}); }
        response["result"]["action"] = action;
        if response["result"]["action"]["verification"].is_object() {
            response["result"]["action"]["verification"]["scope"] = json!("element");
        }
        response["result"]["action"]["taskCompletion"] = json!("not_checked");
    }
    if response["ok"] == false {
        if !response["result"].is_object() { response["result"] = json!({}); }
        let (next_step, message) = match response["error"]["code"].as_str() {
            Some("permission_denied") => ("check_permissions", "请检查辅助功能与屏幕录制权限，完成授权后再决定下一步；不会自动重试"),
            Some("app_blocked" | "invalid_argument" | "background_only_unsupported") => ("stop", "请停止当前请求，检查应用限制、后台模式或修正参数后再决定下一步"),
            _ => ("observe", "请先观察当前状态，确认动作是否已执行，再决定下一步；不会自动重试"),
        };
        let action_reported = !response["result"]["action"].is_null();
        response["result"]["recovery"] = json!({
            "automaticRetry": false,
            "actionReported": action_reported,
            "nextStep": next_step,
            "message": message,
        });
    }
    if FILTERS.iter().any(|key| !options[key].is_null()) {
        let snap = &mut response["result"]["snapshot"];
        if let Some(tree) = snap["treeText"].as_str() {
            let all: Vec<&str> = control_lines(tree).collect();
            // --within narrows to one control and everything nested under it.
            let scope = match options["within"].as_u64() {
                None => Some(0..all.len()),
                Some(n) => all.iter().position(|l| index_of(l) == Some(n)).map(|p| {
                    let end = all[p + 1..].iter().position(|l| depth(l) <= depth(all[p])).map_or(all.len(), |e| p + 1 + e);
                    p..end
                }),
            };
            let needle = options["match"].as_str().unwrap_or("").to_lowercase();
            let roles: Vec<String> = options["role"].as_str().unwrap_or("").split(',').map(|r| r.trim().to_lowercase()).filter(|r| !r.is_empty()).collect();
            let hits: Vec<usize> = scope.clone().unwrap_or(0..0)
                .filter(|&i| all[i].to_lowercase().contains(&needle) && (roles.is_empty() || has_role(all[i], &roles))).collect();
            let matched = hits.len();
            let limit = options["limit"].as_u64().unwrap_or(1200) as usize;
            let mut keep: std::collections::BTreeSet<usize> = hits.iter().take(limit).copied().collect();
            let shown = keep.len();
            if options["ancestors"] == true {
                for &hit in hits.iter().take(limit) {
                    let mut level = depth(all[hit]);
                    for i in (0..hit).rev() {
                        if level == 0 { break; }
                        if depth(all[i]) < level { level = depth(all[i]); keep.insert(i); }
                    }
                }
            }
            let text = keep.iter().map(|&i| all[i]).collect::<Vec<_>>().join("\n");
            let mut view = json!({"filtered":true,"matched":matched,"shown":shown,"omitted":matched - shown,"indexesPreserved":true});
            if options["ancestors"] == true { view["ancestorsShown"] = json!(keep.len() - shown); }
            if !options["within"].is_null() { view["withinFound"] = json!(scope.is_some()); }
            snap["treeText"] = json!(text);
            snap["view"] = view;
        }
    }
    Ok(response)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn strict_mode_does_not_send_target_request_to_an_old_helper() {
        let mut methods = Vec::new();
        let response = execute(request("setValue", json!({"backgroundOnly":true})), |r| {
            methods.push(r["method"].as_str().unwrap().to_owned());
            Ok(json!({"ok":true,"result":{"supports":{"input":{"background":true}}}}))
        }).unwrap();
        assert_eq!(methods, ["handshake"]);
        assert_eq!(response["ok"], false);
        assert_eq!(response["error"]["code"], "background_only_unsupported");
        assert_eq!(response["result"]["recovery"]["actionReported"], false);
    }

    #[test]
    fn strict_mode_preserves_helper_connection_errors() {
        let error = execute(request("getAppState", json!({"backgroundOnly":true})), |_| Err("host unavailable".into())).unwrap_err();
        assert_eq!(error, "host unavailable");
        let response = execute(request("getAppState", json!({"backgroundOnly":true})), |_| {
            Ok(json!({"ok":false,"error":{"code":"permission_denied","message":"existing diagnostic"}}))
        }).unwrap();
        assert_eq!(response["error"]["code"], "permission_denied");
    }

    #[test]
    fn strict_policy_is_retained_when_polling_after_a_single_assignment() {
        let mut methods = Vec::new();
        let response = execute(request("setValue", json!({"backgroundOnly":true,"value":"hello","waitFor":"Ready","timeoutMs":1000})), |r| {
            let method = r["method"].as_str().unwrap();
            methods.push(method.to_owned());
            match method {
                "handshake" => Ok(json!({"ok":true,"result":{"supports":{"input":{"backgroundOnly":true}}}})),
                "setValue" => Ok(json!({"ok":true,"result":{"action":{"path":"accessibility","targetWindowId":42},"snapshot":{"window":{"id":42},"treeText":"0 text Saving"}}})),
                "getAppState" => {
                    assert_eq!(r["params"]["backgroundOnly"], true);
                    assert!(r["params"]["value"].is_null());
                    Ok(json!({"ok":true,"result":{"snapshot":{"window":{"id":42},"treeText":"0 text Ready"}}}))
                },
                _ => panic!("unexpected call: {method}"),
            }
        }).unwrap();
        assert_eq!(methods, ["handshake", "setValue", "getAppState"]);
        assert_eq!(response["ok"], true);
        assert_eq!(response["result"]["action"]["taskCompletion"], "not_checked");
    }

    #[test]
    fn ordinary_assignment_keeps_the_existing_helper_protocol() {
        let mut methods = Vec::new();
        let response = execute(request("setValue", json!({})), |r| {
            methods.push(r["method"].as_str().unwrap().to_owned());
            Ok(json!({"ok":true,"result":{"action":{"path":"accessibility"}}}))
        }).unwrap();
        assert_eq!(methods, ["setValue"]);
        assert_eq!(response["ok"], true);
    }

    fn request(method: &str, options: Value) -> Value {
        let mut params = json!({"app":"test", "noScreenshot":true});
        params.as_object_mut().unwrap().extend(options.as_object().unwrap().clone());
        json!({"id":1,"method":method,"params":params})
    }
    fn snapshot(tree: &str) -> Value {
        json!({"ok":true,"result":{"snapshot":{"treeText":tree,"elementCount":4,"window":{"id":23}}}})
    }
    #[test]
    fn waiting_and_filtering_require_full_provider_trees() {
        for options in [json!({"waitFor":"Ready","timeoutMs":0}), json!({"waitForGone":"Busy","timeoutMs":0}), json!({"expectPath":"/","timeoutMs":0}), json!({"match":"Ready"}), json!({"limit":2})] {
            let r = execute(request("click", options), |q| {
                assert_eq!(q["params"]["fullTree"], true, "postconditions and filters need unchanged elements too");
                let mut s = snapshot("0 window\n8 text Ready");
                s["result"]["snapshot"]["truncation"] = json!({"truncated":false,"maxDepthReached":false});
                Ok(s)
            }).unwrap();
            assert_eq!(r["ok"], true);
        }
        execute(request("click", json!({})), |q| {
            assert!(q["params"]["fullTree"].is_null(), "ordinary actions may use incremental output");
            Ok(snapshot("0 window"))
        }).unwrap();
    }
    #[test]
    fn filtered_responses_cannot_seed_a_complete_delta_baseline() {
        execute(request("getAppState", json!({"match":"Ready"})), |q| {
            assert_eq!(q["params"]["treeViewFiltered"], true);
            Ok(snapshot("0 window\n8 text Ready"))
        }).unwrap();
        execute(request("getAppState", json!({"waitFor":"Ready","timeoutMs":0})), |q| {
            assert!(q["params"]["treeViewFiltered"].is_null());
            Ok(snapshot("0 window\n8 text Ready"))
        }).unwrap();
    }
    #[test]
    fn filtered_output_preserves_indexes_and_reports_omissions() {
        let r = execute(request("getAppState", json!({"match":"保存","limit":1})), |_| Ok(snapshot("App=x\nWindow: 保存\n\n0 window\n\t17 button 保存\n\t28 button 另存保存\nThe focused UI element is 17 button 保存."))).unwrap();
        assert_eq!(r["result"]["snapshot"]["treeText"], "\t17 button 保存");
        assert_eq!(r["result"]["snapshot"]["view"]["matched"], 2);
        assert_eq!(r["result"]["snapshot"]["view"]["shown"], 1);
    }
    const FORM: &str = "App=x\nWindow: 表单\n\n0 standard window 表单\n\t1 group 账号\n\t\t2 text field 邮箱\n\t\t3 radio button 个人\n\t\t4 button 保存\n\t5 group 页脚\n\t\t6 button 保存草稿\n\t\t7 link 帮助\nThe focused UI element is 2 text field 邮箱.";
    fn view(params: Value) -> Value { execute(request("getAppState", params), |_| Ok(snapshot(FORM))).unwrap()["result"]["snapshot"].clone() }
    #[test]
    fn role_filter_keeps_only_controls_of_the_named_types() {
        let s = view(json!({"role":"button"}));
        assert_eq!(s["treeText"], "\t\t3 radio button 个人\n\t\t4 button 保存\n\t\t6 button 保存草稿");
        let r = execute(request("getAppState", json!({"role":"button"})), |_| Ok(snapshot("0 window\n\t1 close button\n\t2 full screen button\n\t3 decrement arrow button\n\t4 text button"))).unwrap();
        assert_eq!(r["result"]["snapshot"]["view"]["matched"], 3);
        let s = view(json!({"role":"text field, link"}));
        assert_eq!(s["treeText"], "\t\t2 text field 邮箱\n\t\t7 link 帮助");
        assert_eq!(s["view"]["matched"], 2);
        // A label that merely contains the word is not that type.
        let r = execute(request("getAppState", json!({"role":"link"})), |_| Ok(snapshot("0 window\n\t1 button copy link\n\t2 link Docs"))).unwrap();
        assert_eq!(r["result"]["snapshot"]["treeText"], "\t2 link Docs");
    }
    #[test]
    fn within_reads_only_one_controls_subtree() {
        let s = view(json!({"within":5}));
        assert_eq!(s["treeText"], "\t5 group 页脚\n\t\t6 button 保存草稿\n\t\t7 link 帮助");
        let s = view(json!({"within":1,"match":"保存"}));
        assert_eq!(s["treeText"], "\t\t4 button 保存");
        let s = view(json!({"within":99}));
        assert_eq!(s["treeText"], "");
        assert_eq!(s["view"]["withinFound"], false);
    }
    #[test]
    fn ancestors_show_where_each_match_sits_without_counting_as_matches() {
        let s = view(json!({"match":"保存","ancestors":true}));
        assert_eq!(s["treeText"], "0 standard window 表单\n\t1 group 账号\n\t\t4 button 保存\n\t5 group 页脚\n\t\t6 button 保存草稿");
        assert_eq!(s["view"]["matched"], 2);
        assert_eq!(s["view"]["ancestorsShown"], 3);
        let s = view(json!({"match":"保存","ancestors":true,"limit":1}));
        assert_eq!(s["treeText"], "0 standard window 表单\n\t1 group 账号\n\t\t4 button 保存");
        assert_eq!(s["view"]["omitted"], 1);
    }
    #[test]
    fn new_filters_force_a_full_tree_and_reject_empty_or_pointless_use() {
        for options in [json!({"role":"button"}), json!({"within":3})] {
            execute(request("getAppState", options), |q| { assert_eq!(q["params"]["fullTree"], true); assert_eq!(q["params"]["treeViewFiltered"], true); Ok(snapshot("0 window")) }).unwrap();
        }
        assert!(validate("getAppState", &json!({"role":" "})).is_err());
        assert!(validate("getAppState", &json!({"ancestors":true})).is_err());
        assert!(validate("listApps", &json!({"role":"button"})).is_err());
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
    fn fallback_keeps_waiting_after_single_foreground_read() {
        let mut calls = 0;
        let mut foreground = 0;
        let r = execute(request("getAppState", json!({"waitFor":"Ready","timeoutMs":80,"foregroundFallback":true})), |q| {
            calls += 1;
            if q["params"]["briefForeground"] == true { foreground += 1; }
            Ok(snapshot(if calls >= 3 {"3 text Ready"} else {"0 window"}))
        }).unwrap();
        assert_eq!(r["ok"], true); assert_eq!(foreground, 1);
    }
    #[test]
    fn transport_failure_keeps_action_receipt() {
        let mut calls = 0;
        let r = execute(request("click", json!({"waitFor":"Ready","timeoutMs":300})), |_| {
            calls += 1;
            if calls > 1 { return Err("connection lost".into()); }
            let mut s = snapshot("0 window");
            s["result"]["action"] = json!({"targetWindowId":23}); Ok(s)
        }).unwrap();
        assert_eq!(r["ok"], false);
        assert_eq!(r["error"]["code"], "observation_failed");
        assert_eq!(r["result"]["action"]["targetWindowId"], 23);
        assert_eq!(calls, 2);
    }
    #[test]
    fn gone_needs_complete_tree() {
        for truncated in [true, false] {
            let r = execute(request("getAppState", json!({"waitForGone":"Loading","timeoutMs":0})), |_| {
                let mut s = snapshot("0 window"); s["result"]["snapshot"]["truncation"] = json!({"truncated":truncated,"maxDepthReached":false}); Ok(s)
            }).unwrap();
            assert_eq!(r["ok"], !truncated);
            assert_eq!(r["result"]["postcondition"]["state"], if truncated {"timeout"} else {"met"});
        }
    }
    #[test]
    fn gone_rejects_present_empty_or_incomplete_observation() {
        for (tree, truncation) in [
            ("0 window\n2 text Loading", json!({"truncated":false,"maxDepthReached":false})),
            ("", json!({"truncated":false,"maxDepthReached":false})),
            ("0 window", json!(null)),
            ("0 window", json!({"truncated":false,"maxDepthReached":true})),
            ("0 window", json!({"truncated":true,"maxDepthReached":false})),
        ] {
            let r = execute(request("getAppState", json!({"waitForGone":"Loading","timeoutMs":0})), |_| {
                let mut s = snapshot(tree); s["result"]["snapshot"]["truncation"] = truncation.clone(); Ok(s)
            }).unwrap();
            assert_eq!(r["ok"], false, "{tree}: {truncation}");
        }
    }
    #[test]
    fn path_check_failure_preserves_action() {
        let parent = std::env::temp_dir().join(format!("shifu-observation-file-{}", std::process::id()));
        std::fs::write(&parent, "fixture").unwrap();
        let path = parent.join("child");
        assert_eq!(std::fs::symlink_metadata(&path).unwrap_err().kind(), std::io::ErrorKind::NotADirectory);
        let r = execute(request("click", json!({"expectPath":path,"timeoutMs":0})), |_| {
            let mut s = snapshot("0 window"); s["result"]["action"] = json!({"targetWindowId":23}); Ok(s)
        }).unwrap();
        std::fs::remove_file(parent).unwrap();
        assert_eq!(r["error"]["code"], "observation_failed");
        assert_eq!(r["result"]["postcondition"]["state"], "observation_failed");
        assert_eq!(r["result"]["action"]["targetWindowId"], 23);
        assert_eq!(r["result"]["action"]["taskCompletion"], "not_checked");
    }
    #[test]
    fn text_postcondition_cannot_use_another_explicit_window() {
        for selector in [json!({"windowId":23}), json!({"windowIndex":0})] {
            for condition in [json!({"waitFor":"Ready"}), json!({"waitForGone":"Loading"})] {
                let mut options = json!({"timeoutMs":0});
                options.as_object_mut().unwrap().extend(selector.as_object().unwrap().clone());
                options.as_object_mut().unwrap().extend(condition.as_object().unwrap().clone());
                let mut calls = 0;
                let r = execute(request("click", options), |_| {
                    calls += 1;
                    let mut s = snapshot("0 window\n8 text Ready");
                    s["result"]["snapshot"]["window"]["id"] = json!(24);
                    s["result"]["snapshot"]["truncation"] = json!({"truncated":false,"maxDepthReached":false});
                    s["result"]["action"] = json!({"targetWindowId":23});
                    Ok(s)
                }).unwrap();
                assert_eq!(r["ok"], false, "{selector}: {condition}");
                assert_eq!(r["error"]["code"], "observation_failed");
                assert_eq!(r["result"]["postcondition"]["state"], "observation_failed");
                assert_eq!(r["result"]["action"]["targetWindowId"], 23);
                assert_eq!(r["result"]["snapshot"]["window"]["id"], 24);
                assert_eq!(calls, 1);
            }
        }
    }
    #[test]
    fn window_change_preserves_file_conditions_and_app_scoped_text_wait() {
        let missing = std::env::temp_dir().join(format!("shifu-not-created-window-condition-{}", std::process::id()));
        for options in [json!({"windowId":23,"expectMissingPath":missing,"timeoutMs":0}), json!({"waitFor":"Ready","timeoutMs":0})] {
            let r = execute(request("click", options), |_| {
                let mut s = snapshot("0 window\n8 text Ready");
                s["result"]["snapshot"]["window"]["id"] = json!(24);
                s["result"]["action"] = json!({"targetWindowId":23});
                Ok(s)
            }).unwrap();
            assert_eq!(r["ok"], true);
            assert_eq!(r["result"]["postcondition"]["state"], "met");
        }
    }
    #[test]
    fn rejects_conflicting_text_conditions() {
        assert!(validate("getAppState", &json!({"waitFor":"Ready","waitForGone":"Ready"})).is_err());
    }
    #[test]
    fn recovery_advice_preserves_error_and_never_replays_input() {
        for (code, next) in [("permission_denied", "check_permissions"), ("window_not_focused", "observe"), ("transport_error", "observe"), ("app_blocked", "stop"), ("invalid_argument", "stop")] {
            let error = json!({"code":code,"message":"original cause"});
            let mut calls = 0;
            let r = execute(request("click", json!({})), |_| {
                calls += 1;
                Ok(json!({"ok":false,"error":error.clone()}))
            }).unwrap();
            assert_eq!(r["error"], error);
            assert_eq!(r["result"]["recovery"]["automaticRetry"], false);
            assert_eq!(r["result"]["recovery"]["actionReported"], false);
            assert_eq!(r["result"]["recovery"]["nextStep"], next);
            assert_eq!(calls, 1);
        }
        let r = execute(request("click", json!({"waitFor":"Ready","timeoutMs":0})), |_| {
            let mut s = snapshot("0 window");
            s["result"]["action"] = json!({"targetWindowId":23});
            Ok(s)
        }).unwrap();
        assert_eq!(r["error"]["code"], "postcondition_timeout");
        assert_eq!(r["result"]["recovery"]["actionReported"], true);
        assert_eq!(r["result"]["recovery"]["nextStep"], "observe");
        assert_eq!(r["result"]["action"]["targetWindowId"], 23);
        let success = execute(request("getAppState", json!({})), |_| Ok(snapshot("0 window"))).unwrap();
        assert!(success["result"].get("recovery").is_none());
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
