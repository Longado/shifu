//! Read-only projection of the existing controller's receipt-based progress.
use crate::agent::{Agent, Res};
use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ReviewUsage {
    pub recorded_judgments: u64,
    pub reported_judgments: u64,
    pub input_tokens: Option<u64>,
    pub cached_input_tokens: Option<u64>,
    pub output_tokens: Option<u64>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct OutcomeReceipt {
    pub id: String,
    pub evidence: String,
    pub sha256: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Progress {
    pub run_id: String,
    pub agent: String,
    pub session_id: String,
    pub cwd: String,
    pub deadline: String,
    pub batch: Option<String>,
    pub completed: Option<u64>,
    pub target: u64,
    pub last_progress_at: u64,
    pub phase: String,
    pub error: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub review: Option<ReviewUsage>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub receipt: Option<OutcomeReceipt>,
}

pub fn read(home: &Path, agent: &Agent) -> Res<Option<Progress>> {
    let path = home
        .join("supervision")
        .join(format!("{}.json", agent.name));
    let bytes = match std::fs::read(&path) {
        Ok(bytes) => bytes,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(format!("cannot read progress: {e}")),
    };
    if bytes.len() > 16 * 1024 {
        return Err("progress projection too large".into());
    }
    let mut view: Progress =
        serde_json::from_slice(&bytes).map_err(|e| format!("invalid progress: {e}"))?;
    if view.agent != agent.name
        || view.cwd != agent.cwd
        || Some(view.session_id.as_str()) != agent.session_id.as_deref()
    {
        return Ok(None); // A previous native session's result must not be attached to a replacement.
    }
    if view.target == 0
        || !["observing", "met", "stalled", "invalid", "stopped"].contains(&view.phase.as_str())
    {
        return Err("invalid progress target or phase".into());
    }
    if let Some(receipt) = &view.receipt
        && (view.completed.unwrap_or(0) == 0 || view.error.is_some() || view.phase == "invalid"
            || receipt.id.trim().is_empty() || !Path::new(&receipt.evidence).is_absolute()
            || receipt.sha256.len() != 64 || !receipt.sha256.bytes().all(|b| b.is_ascii_hexdigit()))
    {
        view.receipt = None;
    }
    Ok(Some(view))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn stale_session_is_hidden_and_malformed_progress_is_reported() {
        let home = std::env::temp_dir().join(format!("shifu-supervision-{}", std::process::id()));
        std::fs::create_dir_all(home.join("supervision")).unwrap();
        let agent: Agent = serde_json::from_value(serde_json::json!({
            "name":"codex-demo", "kind":"codex", "command":"codex", "cwd":"/work/demo",
            "created_at":1, "status":"running", "status_since":1, "session_id":"current"
        }))
        .unwrap();
        let path = home.join("supervision/codex-demo.json");
        let mut progress = serde_json::json!({
            "run_id":"run", "agent":"codex-demo", "session_id":"previous", "cwd":"/work/demo",
            "deadline":"2026-12-01T15:00:00+08:00", "batch":"batch-one", "completed":1,
            "target":3, "last_progress_at":1, "phase":"stalled", "error":null
        });
        std::fs::write(&path, progress.to_string()).unwrap();
        assert!(read(&home, &agent).unwrap().is_none());
        progress["session_id"] = "current".into();
        std::fs::write(&path, progress.to_string()).unwrap();
        let legacy = read(&home, &agent).unwrap().unwrap();
        assert_eq!(legacy.completed, Some(1));
        assert!(legacy.review.is_none());
        assert!(legacy.receipt.is_none());
        let hash = "ab".repeat(32);
        progress["receipt"] = serde_json::json!({"id":"verified-one","evidence":"/work/demo/proof file.json","sha256":hash});
        std::fs::write(&path, progress.to_string()).unwrap();
        assert_eq!(read(&home, &agent).unwrap().unwrap().receipt.unwrap().sha256, hash);
        progress["receipt"]["evidence"] = "relative".into();
        std::fs::write(&path, progress.to_string()).unwrap();
        let without_link = read(&home, &agent).unwrap().unwrap();
        assert!(without_link.receipt.is_none());
        assert_eq!(without_link.completed, Some(1));
        progress["review"] = serde_json::json!({"recorded_judgments": 2, "reported_judgments": 1,
            "input_tokens": 12, "cached_input_tokens": null, "output_tokens": 0});
        std::fs::write(&path, progress.to_string()).unwrap();
        let usage = read(&home, &agent).unwrap().unwrap().review.unwrap();
        assert_eq!((usage.recorded_judgments, usage.reported_judgments), (2, 1));
        assert_eq!((usage.input_tokens, usage.cached_input_tokens, usage.output_tokens), (Some(12), None, Some(0)));
        std::fs::write(&path, "invalid").unwrap();
        assert!(read(&home, &agent).is_err());
        std::fs::remove_dir_all(home).unwrap();
    }
}
