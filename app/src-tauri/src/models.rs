//! Which models each agent CLI offers on this machine, for the new-agent picker. Nothing here sends a prompt:
//! Codex keeps its own list in `<CODEX_HOME>/models_cache.json`; Claude and CodeBuddy answer the SDK
//! `initialize` handshake (`-p --input-format stream-json`) with their model list, and the process is stopped
//! right after. `--setting-sources local` in an empty folder keeps the user's hooks (SessionStart etc.) from running.

use serde::Serialize;
use serde_json::Value;
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::time::Duration;

const PROBE_TIMEOUT: Duration = Duration::from_secs(45);
/// CodeBuddy's handshake lists models without efforts; these are the levels its `--help` gives.
const CODEBUDDY_EFFORTS: [&str; 6] = ["minimal", "low", "medium", "high", "xhigh", "max"];

#[derive(Serialize, Debug, PartialEq)]
pub struct ModelInfo {
    pub id: String,
    pub label: String,
    /// Reasoning levels this model takes; empty when it has none.
    pub efforts: Vec<String>,
}

#[derive(Serialize, Debug)]
pub struct AgentModels {
    pub kind: String,
    pub models: Vec<ModelInfo>,
    pub error: Option<String>,
}

fn strs(v: &Value) -> Vec<String> {
    v.as_array().map(|a| a.iter().filter_map(|x| x.as_str().map(str::to_string)).collect()).unwrap_or_default()
}

/// `models_cache.json`: the entries Codex shows (`visibility: "list"`), with their reasoning levels.
pub fn parse_codex(text: &str) -> Result<Vec<ModelInfo>, String> {
    let v: Value = serde_json::from_str(text).map_err(|e| format!("Codex 模型列表读不懂: {e}"))?;
    let models = v["models"].as_array().ok_or("Codex 模型列表里没有 models")?;
    Ok(models
        .iter()
        .filter(|m| m["visibility"].as_str() == Some("list"))
        .filter_map(|m| {
            Some(ModelInfo {
                id: m["slug"].as_str()?.to_string(),
                label: m["display_name"].as_str().unwrap_or(m["slug"].as_str()?).to_string(),
                efforts: m["supported_reasoning_levels"]
                    .as_array()
                    .map(|a| a.iter().filter_map(|e| e["effort"].as_str().map(str::to_string)).collect())
                    .unwrap_or_default(),
            })
        })
        .collect())
}

/// Claude's handshake: `value` is what `--model` takes; "default" is left out (the picker has its own 默认).
pub fn parse_claude(models: &[Value]) -> Vec<ModelInfo> {
    models
        .iter()
        .filter_map(|m| {
            let id = m["value"].as_str()?;
            (id != "default").then(|| ModelInfo {
                id: id.to_string(),
                label: m["displayName"].as_str().unwrap_or(id).to_string(),
                efforts: strs(&m["supportedEffortLevels"]),
            })
        })
        .collect()
}

/// CodeBuddy's handshake: `{id, name}`, where several ids share a name ("Hy4"), so those show their id.
pub fn parse_codebuddy(models: &[Value]) -> Vec<ModelInfo> {
    let name = |m: &Value| m["name"].as_str().map(str::to_string);
    let names: Vec<Option<String>> = models.iter().map(name).collect();
    models
        .iter()
        .filter_map(|m| {
            let id = m["id"].as_str()?.to_string();
            let label = match name(m) {
                Some(n) if names.iter().filter(|x| x.as_deref() == Some(n.as_str())).count() == 1 => n,
                _ => id.clone(),
            };
            Some(ModelInfo { id, label, efforts: CODEBUDDY_EFFORTS.iter().map(|e| e.to_string()).collect() })
        })
        .collect()
}

/// Kimi Code keeps the models of the account it logged in with in its config.toml (`[models."<id>"]`, refreshed
/// by Kimi itself). No levels: Kimi Code takes none on the command line (`support_efforts` is for its own /model).
/// The name falls back to `model`, as in Kimi.
pub fn parse_kimi(text: &str) -> Result<Vec<ModelInfo>, String> {
    let table: toml::Table = text.parse().map_err(|e| format!("Kimi 的 config.toml 读不懂: {e}"))?;
    let Some(models) = table.get("models").and_then(toml::Value::as_table) else {
        return Ok(Vec::new());
    };
    Ok(models
        .iter()
        .map(|(id, m)| {
            let name = |k: &str| m.get(k).and_then(toml::Value::as_str);
            ModelInfo { id: id.clone(), label: name("display_name").or(name("model")).unwrap_or(id).to_string(), efforts: Vec::new() }
        })
        .collect())
}

fn kimi_config_path() -> PathBuf {
    match std::env::var("KIMI_CODE_HOME").ok().filter(|d| !d.is_empty()) {
        Some(d) => PathBuf::from(d),
        None => PathBuf::from(std::env::var("HOME").unwrap_or_default()).join(".kimi-code"),
    }
    .join("config.toml")
}

/// The `models` array from the first `control_response` line.
pub fn handshake_models(line: &str) -> Option<Result<Vec<Value>, String>> {
    let v: Value = serde_json::from_str(line.trim()).ok()?;
    if v["type"] != "control_response" {
        return None;
    }
    let resp = &v["response"];
    Some(match resp["subtype"].as_str() {
        Some("success") => Ok(resp["response"]["models"].as_array().cloned().unwrap_or_default()),
        _ => Err(format!("握手失败: {}", resp["error"].as_str().unwrap_or("没有说明"))),
    })
}

/// Runs `cli` just long enough to answer `initialize`, in an empty folder so no project settings apply.
fn probe(cli: &str, dir: &Path) -> Result<Vec<Value>, String> {
    std::fs::create_dir_all(dir).map_err(|e| format!("create {} failed: {e}", dir.display()))?;
    let mut child = Command::new(cli)
        .args(["-p", "--input-format", "stream-json", "--output-format", "stream-json", "--verbose", "--setting-sources", "local"])
        .current_dir(dir)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| format!("{cli} 没能启动: {e}"))?;
    let request = r#"{"type":"control_request","request_id":"shifu-models","request":{"subtype":"initialize"}}"#;
    let wrote = child.stdin.as_mut().map(|s| writeln!(s, "{request}"));
    let stdout = child.stdout.take();
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let Some(out) = stdout else { return };
        for line in BufReader::new(out).lines().map_while(Result::ok) {
            if let Some(r) = handshake_models(&line) {
                let _ = tx.send(r);
                return;
            }
        }
    });
    let result = match wrote {
        Some(Ok(())) => rx.recv_timeout(PROBE_TIMEOUT).unwrap_or_else(|_| Err(format!("{cli} 没在 {} 秒内回答", PROBE_TIMEOUT.as_secs()))),
        _ => Err(format!("{cli} 的输入写不进去")),
    };
    // It would wait for a prompt; nothing more is wanted from it.
    let _ = child.kill();
    let _ = child.wait();
    result
}

fn codex_home() -> PathBuf {
    match std::env::var("CODEX_HOME").ok().filter(|h| !h.is_empty()) {
        Some(h) => PathBuf::from(h),
        None => PathBuf::from(std::env::var("HOME").unwrap_or_default()).join(".codex"),
    }
}

fn entry(kind: &str, r: Result<Vec<ModelInfo>, String>) -> AgentModels {
    match r {
        Ok(models) => AgentModels { kind: kind.to_string(), models, error: None },
        Err(e) => AgentModels { kind: kind.to_string(), models: Vec::new(), error: Some(e) },
    }
}

/// All three, probed in parallel (each probe takes a few seconds).
pub fn collect(scratch: &Path) -> Vec<AgentModels> {
    let claude_dir = scratch.join("claude");
    let cb_dir = scratch.join("codebuddy");
    let claude = std::thread::spawn(move || probe("claude", &claude_dir).map(|m| parse_claude(&m)));
    let codebuddy = std::thread::spawn(move || probe("codebuddy", &cb_dir).map(|m| parse_codebuddy(&m)));
    let cache = codex_home().join("models_cache.json");
    let codex = std::fs::read_to_string(&cache).map_err(|e| format!("读 {} 失败: {e}", cache.display())).and_then(|t| parse_codex(&t));
    let joined = |h: std::thread::JoinHandle<Result<Vec<ModelInfo>, String>>| h.join().unwrap_or_else(|_| Err("探测线程崩了".to_string()));
    let kimi_file = kimi_config_path();
    let kimi = std::fs::read_to_string(&kimi_file).map_err(|e| format!("读 {} 失败: {e}", kimi_file.display())).and_then(|t| parse_kimi(&t));
    vec![entry("claude", joined(claude)), entry("codex", codex), entry("codebuddy", joined(codebuddy)), entry("kimi", kimi)]
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn codex_keeps_listed_models_with_their_levels() {
        let text = r#"{"models":[
            {"slug":"gpt-6-astra","display_name":"GPT-6-Astra","visibility":"list","supported_reasoning_levels":[{"effort":"low"},{"effort":"ultra"}]},
            {"slug":"gpt-reserve","display_name":"GPT-Reserve","visibility":"hide","supported_reasoning_levels":[]}]}"#;
        assert_eq!(
            parse_codex(text).unwrap(),
            vec![ModelInfo { id: "gpt-6-astra".into(), label: "GPT-6-Astra".into(), efforts: vec!["low".into(), "ultra".into()] }]
        );
        assert!(parse_codex("{}").is_err());
    }

    #[test]
    fn claude_drops_default_and_keeps_aliases_and_levels() {
        let m = parse_claude(&[
            json!({"value":"default","displayName":"Default (recommended)"}),
            json!({"value":"opus","displayName":"Opus 5.5","supportedEffortLevels":["low","max"]}),
            json!({"value":"claude-haiku-4-5-20251001","displayName":"Haiku 4.5"}),
        ]);
        assert_eq!(m.iter().map(|x| (x.id.as_str(), x.label.as_str(), x.efforts.len())).collect::<Vec<_>>(), [
            ("opus", "Opus 5.5", 2),
            ("claude-haiku-4-5-20251001", "Haiku 4.5", 0)
        ]);
    }

    #[test]
    fn kimi_models_come_from_its_config_without_levels() {
        let text = r#"default_model = "kimi-code/k3"
[models."kimi-code/k3"]
model = "k3"
capabilities = ["thinking", "image_in"]
display_name = "K3"
support_efforts = ["low", "high", "max"]
[models."kimi-code/fast"]
model = "fast"
"#;
        assert_eq!(
            parse_kimi(text).unwrap(),
            vec![
                ModelInfo { id: "kimi-code/fast".into(), label: "fast".into(), efforts: vec![] },
                ModelInfo { id: "kimi-code/k3".into(), label: "K3".into(), efforts: vec![] },
            ]
        );
        assert!(parse_kimi("").unwrap().is_empty());
    }

    #[test]
    fn codebuddy_shared_names_fall_back_to_ids() {
        let m = parse_codebuddy(&[json!({"id":"hy4free","name":"Hy4"}), json!({"id":"hy4std","name":"Hy4"}), json!({"id":"glm-5.3","name":"GLM 5.3"})]);
        assert_eq!(m.iter().map(|x| x.label.as_str()).collect::<Vec<_>>(), ["hy4free", "hy4std", "GLM 5.3"]);
        assert_eq!(m[0].efforts.len(), 6);
    }

    /// Starts the real CLIs on this machine: `cargo test models::tests::real -- --ignored --nocapture`.
    #[test]
    #[ignore]
    fn real_machine() {
        let dir = std::env::temp_dir().join(format!("shifu-model-probe-{}", std::process::id()));
        for a in collect(&dir) {
            println!("{} {} {:?} {:?}", a.kind, a.models.len(), a.models.first().map(|m| (&m.label, &m.efforts)), a.error);
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn handshake_line_is_found_and_errors_kept() {
        assert!(handshake_models(r#"{"type":"system","subtype":"hook_started"}"#).is_none());
        let ok = handshake_models(r#"{"type":"control_response","response":{"subtype":"success","response":{"models":[{"value":"opus"}]}}}"#);
        assert_eq!(ok.unwrap().unwrap().len(), 1);
        let err = handshake_models(r#"{"type":"control_response","response":{"subtype":"error","error":"not logged in"}}"#);
        assert!(err.unwrap().unwrap_err().contains("not logged in"));
    }
}
