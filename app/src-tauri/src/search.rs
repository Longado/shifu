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
        self.each(sessions, |s, msgs| {
            if let Some((count, snippet, user)) = find(msgs, query) {
                hits.push(Hit { agent: s.agent.to_string(), count, snippet, user });
            }
        });
        hits
    }

    /// Each session's messages, caught up with its transcript.
    fn each(&self, sessions: &[Session], mut f: impl FnMut(&Session, &[Msg])) {
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
            f(s, &idx.msgs);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
