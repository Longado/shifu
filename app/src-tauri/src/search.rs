//! Search what was said in each live agent's conversation (the user's messages and the agent's replies; tool calls
//! and their output are left out). Transcripts can be 100 MB (Codex), so each file's messages are kept in memory
//! and only the bytes written since the last search are read.

use serde::Serialize;
use serde_json::Value;
use std::collections::HashMap;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

pub struct Msg {
    pub user: bool,
    pub text: String,
}

#[derive(Default)]
struct FileIdx {
    offset: u64,
    msgs: Vec<Msg>,
    questions: PendingQuestions,
}

/// A question keeps its identity when other questions in the same call are answered.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PendingQuestion {
    pub id: String,
    pub title: String,
}

#[derive(Default)]
struct PendingQuestions {
    calls: Vec<QuestionCall>,
}

struct QuestionCall {
    id: String,
    tool: String,
    questions: Vec<Option<String>>,
    accepted: bool,
}

impl PendingQuestions {
    fn observe(&mut self, line: &[u8]) {
        let mentions = |needle: &[u8]| memchr::memmem::find(line, needle).is_some();
        // Outputs dominate the transcript. Only a question call, its response, or the explicit user answer
        // envelope can change this projection. In particular, ordinary work does not answer an async question.
        if !mentions(b"request_user_input") && !mentions(b"send_user_message_question_reply")
            && !(mentions(b"function_call_output") && self.calls.iter().any(|c| mentions(c.id.as_bytes())))
        {
            return;
        }
        let Ok(record) = serde_json::from_slice::<Value>(line) else { return };
        if record["type"] != "response_item" { return }
        let event = &record["payload"];
        match event["type"].as_str() {
            Some("function_call") => {
                let tool = event["name"].as_str().unwrap_or_default().rsplit('.').next().unwrap_or_default();
                if !matches!(tool, "request_user_input_async" | "request_user_input") || event["status"] == "failed" { return }
                let Some(id) = event["call_id"].as_str().filter(|id| !id.is_empty()) else { return };
                let Some(arguments) = event["arguments"].as_str().and_then(|s| serde_json::from_str::<Value>(s).ok()) else { return };
                let Some(items) = arguments["questions"].as_array() else { return };
                let questions: Vec<_> = items.iter().map(|q| {
                    q["title"].as_str().or_else(|| q["question"].as_str()).map(str::trim)
                        .filter(|s| !s.is_empty()).map(str::to_string)
                }).collect();
                if questions.iter().all(Option::is_none) || self.calls.iter().any(|c| c.id == id) { return }
                self.calls.push(QuestionCall { id: id.to_string(), tool: tool.to_string(), questions, accepted: tool == "request_user_input" });
            }
            Some("function_call_output") => {
                let Some(id) = event["call_id"].as_str() else { return };
                let output = match &event["output"] {
                    Value::String(raw) => serde_json::from_str::<Value>(raw).unwrap_or(Value::Null),
                    value => value.clone(),
                };
                // A synchronous result completes the request. Async acceptance only says the question was shown.
                self.calls.retain(|c| c.id != id || (c.tool == "request_user_input_async" && output["accepted"] == true));
                if let Some(call) = self.calls.iter_mut().find(|c| c.id == id) {
                    call.accepted = true;
                }
            }
            Some("message") if event["role"] == "user" => {
                let Some(content) = event["content"].as_array() else { return };
                for part in content {
                    let Some(text) = part["text"].as_str() else { continue };
                    let Some(body) = text.trim().strip_prefix("<send_user_message_question_reply>")
                        .and_then(|s| s.trim().strip_suffix("</send_user_message_question_reply>")) else { continue };
                    let Some(replies) = serde_json::from_str::<Value>(body.trim()).ok().and_then(|v| v.as_array().cloned()) else { continue };
                    for reply in replies {
                        let Some(key) = reply["questionItemId"].as_str().and_then(|s| serde_json::from_str::<Value>(s).ok()) else { continue };
                        let (Some(tool), Some(id), Some(index)) = (key[0].as_str(), key[1].as_str(), key[2].as_u64()) else { continue };
                        if let Some(question) = self.calls.iter_mut().find(|c| c.id == id && c.tool == tool.rsplit('.').next().unwrap_or(tool))
                            .and_then(|c| c.questions.get_mut(index as usize))
                        {
                            *question = None;
                        }
                    }
                }
                self.calls.retain(|c| c.questions.iter().any(Option::is_some));
            }
            _ => {}
        }
    }

    fn items(&self) -> Vec<PendingQuestion> {
        self.calls.iter().filter(|c| c.accepted).flat_map(|call| {
            call.questions.iter().enumerate().filter_map(|(index, title)| {
                title.as_ref().map(|title| PendingQuestion {
                    id: serde_json::json!([call.tool, call.id, index]).to_string(),
                    title: title.clone(),
                })
            })
        }).collect()
    }
}

/// Messages per transcript, plus where each session's transcript was found.
#[derive(Default)]
pub struct SearchCache {
    files: Mutex<HashMap<PathBuf, FileIdx>>,
    paths: Mutex<HashMap<String, PathBuf>>,
}

#[derive(Serialize, Debug, PartialEq)]
pub struct Hit {
    pub agent: String,
    /// Messages that contain the query.
    pub count: usize,
    /// Around the newest match, on one line.
    pub snippet: String,
    /// The newest match was the user's own message.
    pub user: bool,
}

pub struct Session<'a> {
    pub agent: &'a str,
    pub kind: &'a str,
    pub session_id: &'a str,
}

/// The said text in one transcript line, whichever CLI wrote it: Claude (`message` of a user/assistant line),
/// Codex (`payload` of a `response_item` message), CodeBuddy (a `message` line itself), Kimi Code
/// (`message` of `context.append_message`).
pub fn said(line: &str) -> Option<Msg> {
    let d: Value = serde_json::from_str(line).ok()?;
    if d.get("isMeta").and_then(Value::as_bool) == Some(true) || d.get("isSidechain").and_then(Value::as_bool) == Some(true) {
        return None;
    }
    let ty = d.get("type").and_then(Value::as_str).unwrap_or("");
    let m = match ty {
        "response_item" => d.get("payload").filter(|p| p.get("type").and_then(Value::as_str) == Some("message"))?,
        "message" => &d,
        _ => d.get("message").filter(|m| m.is_object())?,
    };
    let user = match m.get("role").and_then(Value::as_str)? {
        "user" => true,
        "assistant" => false,
        _ => return None,
    };
    let parts: Vec<&str> = match m.get("content")? {
        Value::String(s) => vec![s.as_str()],
        Value::Array(items) => items
            .iter()
            .filter(|i| matches!(i.get("type").and_then(Value::as_str), Some("text" | "input_text" | "output_text")))
            .filter_map(|i| i.get("text").and_then(Value::as_str))
            .collect(),
        _ => return None,
    };
    // Injected context (system reminders, environment blocks, AGENTS.md) is not something anyone said.
    let text = parts
        .into_iter()
        .map(str::trim)
        .filter(|t| !t.is_empty() && !t.starts_with('<') && !t.starts_with("# AGENTS.md"))
        .collect::<Vec<_>>()
        .join("\n");
    (!text.is_empty()).then_some(Msg { user, text })
}

/// Read what was appended since `idx.offset`, up to the last complete line.
fn catch_up(path: &Path, idx: &mut FileIdx) {
    let Ok(mut f) = std::fs::File::open(path) else { return };
    let len = f.metadata().map(|m| m.len()).unwrap_or(0);
    if len < idx.offset {
        *idx = FileIdx::default();
    }
    if len == idx.offset || f.seek(SeekFrom::Start(idx.offset)).is_err() {
        return;
    }
    let mut bytes = Vec::new();
    if f.read_to_end(&mut bytes).is_err() {
        return;
    }
    let Some(end) = memchr::memrchr(b'\n', &bytes) else { return };
    // Most of a transcript is tool calls and their output; every message carries a role, so only lines with one
    // are parsed (a few hundred of Codex's 6000).
    let role = memchr::memmem::Finder::new(b"\"role\"");
    let mut start = 0;
    for nl in memchr::memchr_iter(b'\n', &bytes[..=end]) {
        let line = &bytes[start..nl];
        start = nl + 1;
        idx.questions.observe(line);
        if role.find(line).is_some()
            && let Some(m) = std::str::from_utf8(line).ok().and_then(said)
        {
            idx.msgs.push(m);
        }
    }
    idx.offset += end as u64 + 1;
}

/// One line of text around byte range `at..at + len` of `text`.
pub fn snippet(text: &str, at: usize, len: usize) -> String {
    const BEFORE: usize = 16;
    const AFTER: usize = 60;
    let head: String = text[..at].chars().rev().take(BEFORE).collect::<Vec<_>>().into_iter().rev().collect();
    let tail: String = text[at + len..].chars().take(AFTER).collect();
    let mut out = String::new();
    if head.chars().count() == BEFORE && at > head.len() {
        out.push('…');
    }
    out.push_str(&head);
    out.push_str(&text[at..at + len]);
    out.push_str(&tail);
    if text.len() > at + len + tail.len() {
        out.push('…');
    }
    out.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// ASCII letters match either case; byte offsets stay those of the original text.
pub fn find(msgs: &[Msg], query: &str) -> Option<(usize, String, bool)> {
    let q = query.to_ascii_lowercase();
    let mut count = 0;
    let mut newest = None;
    for m in msgs.iter().rev() {
        if let Some(at) = m.text.to_ascii_lowercase().find(&q) {
            count += 1;
            if newest.is_none() {
                newest = Some((snippet(&m.text, at, q.len()), m.user));
            }
        }
    }
    newest.map(|(s, user)| (count, s, user))
}

fn home() -> PathBuf {
    PathBuf::from(std::env::var_os("HOME").unwrap_or_default())
}

/// `<root>/<any folder>/<file>`: Claude and CodeBuddy keep one folder per working directory.
fn in_subdir(root: &Path, file: &str) -> Option<PathBuf> {
    std::fs::read_dir(root).ok()?.flatten().map(|e| e.path().join(file)).find(|p| p.is_file())
}

fn find_codex(dir: &Path, suffix: &str) -> Option<PathBuf> {
    for e in std::fs::read_dir(dir).ok()?.flatten() {
        let p = e.path();
        if p.is_dir() {
            if let Some(found) = find_codex(&p, suffix) {
                return Some(found);
            }
        } else if p.file_name().and_then(|n| n.to_str()).is_some_and(|n| n.ends_with(suffix)) {
            return Some(p);
        }
    }
    None
}

fn transcript(kind: &str, sid: &str) -> Option<PathBuf> {
    match kind {
        "claude" => in_subdir(&home().join(".claude/projects"), &format!("{sid}.jsonl")),
        "codebuddy" => in_subdir(&home().join(".codebuddy/projects"), &format!("{sid}.jsonl")),
        "codex" => find_codex(&crate::limits::codex_sessions_dir(), &format!("-{sid}.jsonl")),
        "kimi" => {
            let root = std::env::var_os("KIMI_CODE_HOME").map(PathBuf::from).unwrap_or_else(|| home().join(".kimi-code"));
            in_subdir(&root.join("sessions"), sid).map(|d| d.join("agents/main/wire.jsonl")).filter(|p| p.is_file())
        }
        _ => None,
    }
}

/// Markdown destinations stay on one line. The frontend still validates the adjacent rendered file path.
fn file_link_labels(text: &str, labels: &mut Vec<String>) {
    if labels.len() >= 128 { return }
    for line in text.lines() {
        for (at, _) in line.match_indices("](") {
            let Some(open) = line[..at].rfind('[') else { continue };
            if open > 0 && line.as_bytes()[open - 1] == b'!' {
                continue;
            }
            let label = &line[open + 1..at];
            if label.trim().is_empty() || label.chars().count() > 256 || label.contains(']') {
                continue;
            }
            let destination = &line[at + 2..];
            let target = if let Some(angle) = destination.strip_prefix('<') {
                let Some(end) = angle.find('>') else { continue };
                if !angle[end + 1..].starts_with(')') { continue }
                &angle[..end]
            } else {
                let mut depth = 0;
                let Some(end) = destination.char_indices().find_map(|(i, c)| {
                    if c == '(' { depth += 1; }
                    if c == ')' {
                        if depth == 0 { return Some(i) }
                        depth -= 1;
                    }
                    None
                }) else { continue };
                &destination[..end]
            };
            // Web links have their own terminal handling and must not lend their labels to local files.
            let local = target.strip_prefix("file://").unwrap_or(target);
            let scheme = local.find(':').is_some_and(|at| {
                local[..at].chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '-' | '.'))
            });
            if target.is_empty() || target.starts_with('#') || scheme || labels.iter().any(|l| l == label) {
                continue;
            }
            labels.push(label.to_string());
            if labels.len() == 128 { return }
        }
    }
}

impl SearchCache {
    /// Read each transcript up to now, so the first search does not wait for 100 MB (run at start-up).
    pub fn warm(&self, sessions: &[Session]) {
        self.each(sessions, |_, _| {});
    }

    pub fn search(&self, sessions: &[Session], query: &str) -> Vec<Hit> {
        let query = query.trim();
        let mut hits = Vec::new();
        if query.is_empty() {
            return hits;
        }
        self.each(sessions, |s, idx| {
            if let Some((count, snippet, user)) = find(&idx.msgs, query) {
                hits.push(Hit { agent: s.agent.to_string(), count, snippet, user });
            }
        });
        hits
    }

    /// File labels from this Codex session's recent complete replies, including labels beyond the sidebar excerpt.
    pub fn codex_file_link_labels(&self, session: &Session<'_>) -> Vec<String> {
        if session.kind != "codex" {
            return Vec::new();
        }
        let mut labels = Vec::new();
        self.each(std::slice::from_ref(session), |_, idx| {
            for message in idx.msgs.iter().rev().filter(|m| !m.user).take(32) {
                file_link_labels(&message.text, &mut labels);
                if labels.len() == 128 {
                    break;
                }
            }
        });
        labels
    }

    /// Questions in these Codex sessions still waiting for an explicit answer, even while work continues.
    pub fn pending_questions(&self, sessions: &[Session<'_>]) -> HashMap<String, Vec<PendingQuestion>> {
        let mut pending = HashMap::new();
        self.each(sessions, |session, idx| {
            if session.kind == "codex" {
                let questions = idx.questions.items();
                if !questions.is_empty() { pending.insert(session.session_id.to_string(), questions); }
            }
        });
        pending
    }

    /// Each session's messages, caught up with its transcript.
    fn each(&self, sessions: &[Session], mut f: impl FnMut(&Session, &FileIdx)) {
        for s in sessions {
            let path = {
                let mut paths = self.paths.lock().unwrap_or_else(|e| e.into_inner());
                match paths.get(s.session_id) {
                    Some(p) => p.clone(),
                    None => match transcript(s.kind, s.session_id) {
                        Some(p) => {
                            paths.insert(s.session_id.to_string(), p.clone());
                            p
                        }
                        None => continue,
                    },
                }
            };
            let mut files = self.files.lock().unwrap_or_else(|e| e.into_inner());
            let idx = files.entry(path.clone()).or_default();
            catch_up(&path, idx);
            f(s, idx);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn question_call(tool: &str, id: &str, questions: Value) -> String {
        serde_json::json!({"type":"response_item","payload":{"type":"function_call","name":tool,"call_id":id,"arguments":serde_json::json!({"questions":questions}).to_string()}}).to_string()
    }

    fn question_output(id: &str, output: Value) -> String {
        serde_json::json!({"type":"response_item","payload":{"type":"function_call_output","call_id":id,"output":output.to_string()}}).to_string()
    }

    fn question_answer(tool: &str, id: &str, indices: &[usize]) -> String {
        let replies: Vec<_> = indices.iter().map(|index| serde_json::json!({"answer":"chosen", "questionItemId":serde_json::json!([tool,id,index]).to_string()})).collect();
        serde_json::json!({"type":"response_item","payload":{"type":"message","role":"user","content":[{"type":"input_text","text":format!("<send_user_message_question_reply>\n{}\n</send_user_message_question_reply>",serde_json::to_string(&replies).unwrap())}]}}).to_string()
    }

    fn question_titles(questions: &[PendingQuestion]) -> Vec<&str> {
        questions.iter().map(|question| question.title.as_str()).collect()
    }

    #[test]
    fn question_ids_follow_call_and_original_index_even_for_identical_titles() {
        let mut pending = PendingQuestions::default();
        pending.observe(question_call("request_user_input_async", "call-a", serde_json::json!([{"title":"Continue?"},{"title":"Continue?"}])).as_bytes());
        pending.observe(question_output("call-a", serde_json::json!({"accepted":true})).as_bytes());
        let original = pending.items();
        assert_ne!(original[0].id, original[1].id);
        pending.observe(question_answer("request_user_input_async", "call-a", &[0]).as_bytes());
        assert_eq!(pending.items(), [original[1].clone()]);
        pending.observe(question_call("request_user_input_async", "call-b", serde_json::json!([{"title":"Continue?"}])).as_bytes());
        pending.observe(question_output("call-b", serde_json::json!({"accepted":true})).as_bytes());
        let current = pending.items();
        assert_eq!(current[0], original[1]);
        assert_eq!(current[1].title, original[0].title);
        assert_ne!(current[1].id, original[0].id);
        assert_ne!(current[1].id, original[1].id);
    }

    #[test]
    fn async_questions_survive_acceptance_and_work_until_each_exact_answer() {
        let mut pending = PendingQuestions::default();
        pending.observe(question_call("request_user_input_async", "call-a", serde_json::json!([{"title":"Choose a format"},{"title":"Choose a language"}])).as_bytes());
        assert!(pending.items().is_empty());
        pending.observe(question_output("work", serde_json::json!({"output":"done"})).as_bytes());
        assert!(pending.items().is_empty());
        pending.observe(question_output("call-a", serde_json::json!({"accepted":true})).as_bytes());
        pending.observe(br#"{"type":"response_item","payload":{"type":"function_call","name":"exec_command","call_id":"work","arguments":"{}"}}"#);
        pending.observe(question_output("work", serde_json::json!({"output":"done"})).as_bytes());
        assert_eq!(question_titles(&pending.items()), ["Choose a format", "Choose a language"]);
        pending.observe(question_answer("request_user_input_async", "other-call", &[0]).as_bytes());
        pending.observe(question_answer("request_user_input", "call-a", &[0]).as_bytes());
        assert_eq!(pending.items().len(), 2);
        pending.observe(question_answer("request_user_input_async", "call-a", &[1]).as_bytes());
        assert_eq!(question_titles(&pending.items()), ["Choose a format"]);
        pending.observe(question_answer("request_user_input_async", "call-a", &[0, 1]).as_bytes());
        assert!(pending.items().is_empty());
    }

    #[test]
    fn failed_async_requests_and_completed_sync_requests_do_not_stay_pending() {
        let mut pending = PendingQuestions::default();
        for output in [serde_json::json!({"accepted":false}),serde_json::json!({"error":"unavailable"})] {
            pending.observe(question_call("functions.request_user_input_async", "failure", serde_json::json!([{"title":"Pick one"}])).as_bytes());
            pending.observe(question_output("failure", output).as_bytes());
            assert!(pending.items().is_empty());
        }
        pending.observe(question_call("request_user_input", "sync", serde_json::json!([{"id":"format","question":"Which format?"}])).as_bytes());
        assert_eq!(question_titles(&pending.items()), ["Which format?"]);
        pending.observe(question_output("sync", serde_json::json!({"answers":{"format":{"answers":["PDF"]}}})).as_bytes());
        assert!(pending.items().is_empty());
    }

    #[test]
    fn question_projection_is_incremental_session_scoped_and_rebuilds_after_truncation() {
        let dir = std::env::temp_dir().join(format!("shifu-pending-questions-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let a = dir.join("a.jsonl");
        let b = dir.join("b.jsonl");
        let call = question_call("request_user_input_async", "call-a", serde_json::json!([{"title":"Confirm export"}]));
        let mut bytes = format!("{call}\n{}\n", question_output("call-a", serde_json::json!({"accepted":true})));
        let answer = question_answer("request_user_input_async", "call-a", &[0]);
        let split = answer.len() / 2;
        bytes.push_str(&answer[..split]);
        std::fs::write(&a, &bytes).unwrap();
        std::fs::write(&b, format!("{}\n{}\n",
            question_call("request_user_input_async", "call-b", serde_json::json!([{"title":"Different session"}])),
            question_output("call-b", serde_json::json!({"accepted":true}))
        )).unwrap();
        let cache = SearchCache::default();
        cache.paths.lock().unwrap().extend([("a".into(),a.clone()),("b".into(),b.clone())]);
        let sessions = [Session { agent:"one",kind:"codex",session_id:"a" },Session { agent:"two",kind:"codex",session_id:"b" }];
        let pending = cache.pending_questions(&sessions[..1]);
        let original_id = pending["a"][0].id.clone();
        assert_eq!(pending.len(),1);
        assert_eq!(question_titles(&pending["a"]),["Confirm export"]);
        assert_eq!(cache.pending_questions(&sessions[..1]),pending);
        assert_eq!(question_titles(&cache.pending_questions(&sessions)["b"]),["Different session"]);
        bytes.push_str(&answer[split..]);
        bytes.push('\n');
        std::fs::write(&a,&bytes).unwrap();
        assert!(!cache.pending_questions(&sessions).contains_key("a"));
        assert!(cache.pending_questions(&sessions[..1]).is_empty());
        // A shorter replacement resets both the message index and pending questions.
        std::fs::write(&a,format!("{call}\n{}\n",question_output("call-a",serde_json::json!({"accepted":true})))).unwrap();
        assert_eq!(question_titles(&cache.pending_questions(&sessions[..1])["a"]),["Confirm export"]);
        let restarted = SearchCache::default();
        restarted.paths.lock().unwrap().insert("a".into(),a.clone());
        let rebuilt = restarted.pending_questions(&sessions[..1]);
        assert_eq!(question_titles(&rebuilt["a"]),["Confirm export"]);
        assert_eq!(rebuilt["a"][0].id, original_id);
        assert!(cache.pending_questions(&[Session { agent:"one",kind:"claude",session_id:"a" }]).is_empty());
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn reads_each_clis_messages_and_skips_tools_and_context() {
        let claude_user = r#"{"type":"user","message":{"role":"user","content":"上下分屏怎么做"}}"#;
        let claude_reply = r#"{"type":"assistant","message":{"role":"assistant","content":[{"type":"thinking","thinking":"x"},{"type":"text","text":"用 dockview"},{"type":"tool_use","name":"Bash"}]}}"#;
        let claude_tool = r#"{"type":"user","message":{"role":"user","content":[{"type":"tool_result","content":"secret output"}]}}"#;
        let claude_meta = r#"{"type":"user","isMeta":true,"message":{"role":"user","content":"meta"}}"#;
        let codex = r#"{"type":"response_item","payload":{"type":"message","role":"assistant","content":[{"type":"output_text","text":"done"}]}}"#;
        let codex_ctx = r#"{"type":"response_item","payload":{"type":"message","role":"user","content":[{"type":"input_text","text":"<environment_context>x</environment_context>"}]}}"#;
        let codebuddy = r#"{"type":"message","role":"user","content":[{"type":"input_text","text":"混元"}]}"#;
        let kimi = r#"{"type":"context.append_message","message":{"role":"user","content":[{"type":"text","text":"Kimi 新版"}],"toolCalls":[]}}"#;
        let got = |l: &str| said(l).map(|m| (m.user, m.text));
        assert_eq!(got(claude_user), Some((true, "上下分屏怎么做".into())));
        assert_eq!(got(claude_reply), Some((false, "用 dockview".into())));
        assert_eq!(got(claude_tool), None);
        assert_eq!(got(claude_meta), None);
        assert_eq!(got(codex), Some((false, "done".into())));
        assert_eq!(got(codex_ctx), None);
        assert_eq!(got(codebuddy), Some((true, "混元".into())));
        assert_eq!(got(kimi), Some((true, "Kimi 新版".into())));
        assert_eq!(got("not json"), None);
    }

    #[test]
    fn file_labels_include_late_reply_links_and_exclude_web_images_and_duplicates() {
        let reply = format!("{} [研究结果](</reports/a file.docx>) [研究结果](/reports/again.docx) [网站](https://example.com/a) ![图片](/reports/a.png) [邮件](mailto:a@example.com)", "长".repeat(400));
        let mut labels = Vec::new();
        file_link_labels(&reply, &mut labels);
        assert_eq!(labels, vec!["研究结果"]);
        file_link_labels("[普通标题](reports/a(1).md) [另一个](plain.docx)", &mut labels);
        assert_eq!(labels, vec!["研究结果", "普通标题", "另一个"]);
    }

    #[test]
    fn session_file_labels_use_only_recent_assistant_replies_and_incremental_updates() {
        let dir = std::env::temp_dir().join(format!("shifu-file-labels-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let a = dir.join("a.jsonl");
        let b = dir.join("b.jsonl");
        let message = |role: &str, text: &str| format!("{}\n", serde_json::json!({"type":"response_item","payload":{"type":"message","role":role,"content":[{"type":"output_text","text":text}]}}));
        let mut text = message("assistant", "[太旧](old.md)");
        for _ in 0..32 { text.push_str(&message("assistant", "done")); }
        text.push_str(&message("user", "[用户输入](user.md)"));
        text.push_str(&message("assistant", &format!("{} [本会话成果](reports/a.docx)", "长".repeat(400))));
        std::fs::write(&a, &text).unwrap();
        std::fs::write(&b, message("assistant", "[别的会话](reports/b.docx)")).unwrap();
        let cache = SearchCache::default();
        cache.paths.lock().unwrap().extend([("a".into(), a.clone()), ("b".into(), b.clone())]);
        let session = Session { agent: "one", kind: "codex", session_id: "a" };
        assert_eq!(cache.codex_file_link_labels(&session), vec!["本会话成果"]);
        text.push_str(&message("assistant", "[新成果](reports/new.docx)"));
        std::fs::write(&a, text).unwrap();
        assert_eq!(cache.codex_file_link_labels(&session), vec!["新成果", "本会话成果"]);
        assert!(cache.codex_file_link_labels(&Session { kind: "claude", ..session }).is_empty());
        assert_eq!(cache.codex_file_link_labels(&Session { agent: "two", kind: "codex", session_id: "b" }), vec!["别的会话"]);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn file_labels_are_bounded_and_ignore_oversized_labels() {
        let mut labels = Vec::new();
        file_link_labels(&format!("[{}](a.md)", "长".repeat(257)), &mut labels);
        assert!(labels.is_empty());
        for i in 0..140 { file_link_labels(&format!("[file{i}](a.md)"), &mut labels); }
        assert_eq!(labels.len(), 128);
    }

    #[test]
    fn counts_matches_and_shows_the_newest_ignoring_ascii_case() {
        let msgs = vec![
            Msg { user: true, text: "first Shifu note".into() },
            Msg { user: false, text: "nothing here".into() },
            Msg { user: false, text: "later the SHIFU sidebar".into() },
        ];
        assert_eq!(find(&msgs, "shifu"), Some((2, "later the SHIFU sidebar".into(), false)));
        assert_eq!(find(&msgs, "absent"), None);
    }

    #[test]
    fn snippet_is_one_line_around_the_match() {
        let text = "开头说了很多很多很多很多很多很多很多的话\n然后讲到分屏\n之后";
        let at = text.find("分屏").unwrap();
        let s = snippet(text, at, "分屏".len());
        assert!(s.starts_with('…') && s.contains("然后讲到分屏 之后") && !s.contains('\n'), "{s}");
    }

    #[test]
    fn reads_only_what_was_appended() {
        let dir = std::env::temp_dir().join(format!("shifu-search-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let f = dir.join("t.jsonl");
        std::fs::write(&f, "{\"type\":\"message\",\"role\":\"user\",\"content\":\"one\"}\n{\"type\":\"message\",\"role\"").unwrap();
        let mut idx = FileIdx::default();
        catch_up(&f, &mut idx);
        assert_eq!(idx.msgs.len(), 1);
        // The half-written line is finished later and read then, once.
        let mut all = std::fs::read_to_string(&f).unwrap();
        all.push_str(":\"assistant\",\"content\":\"two\"}\n");
        std::fs::write(&f, all).unwrap();
        catch_up(&f, &mut idx);
        catch_up(&f, &mut idx);
        assert_eq!(idx.msgs.iter().map(|m| m.text.as_str()).collect::<Vec<_>>(), ["one", "two"]);
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
