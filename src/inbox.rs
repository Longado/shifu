//! Durable input for an existing desktop conversation. Native input stays on the same pane.
use crate::agent::{Agent, Kind, Res, now_secs};
use crate::app_session::write_atomic;
use crate::status::Status;
use crate::tmux::Tmux;
use serde::{Deserialize, Serialize};
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Phase {
    Queued,
    Delivering,
    Written,
    Received,
    Unknown,
    Cancelled,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Input {
    pub id: String,
    pub agent: String,
    pub session_id: String,
    pub pane: String,
    pub cwd: String,
    pub agent_created_at: u64,
    pub created_at: u64,
    #[serde(default)]
    pub queued_at_ns: u64,
    pub text: String,
    pub phase: Phase,
    pub reason: Option<String>,
    #[serde(default)]
    pub expires_at: Option<u64>,
    #[serde(default)]
    pub stop_file: Option<PathBuf>,
    #[serde(default)]
    pub transcript: Option<PathBuf>,
    #[serde(default)]
    pub transcript_offset: u64,
    #[serde(default)]
    pub transcript_scanned: u64,
}

fn directory(home: &Path, agent: &str) -> Res<PathBuf> {
    if !crate::agent::is_safe_name(agent) {
        return Err("invalid conversation name".into());
    }
    Ok(home.join("inbox").join(agent))
}
fn path(home: &Path, agent: &str, id: &str) -> Res<PathBuf> {
    if id.is_empty()
        || id.len() > 80
        || !id
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || b"_-".contains(&c))
    {
        return Err("message id must contain 1-80 letters, digits, underscores or hyphens".into());
    }
    Ok(directory(home, agent)?.join(format!("{id}.json")))
}
fn lock(dir: &Path) -> Res<File> {
    fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(dir, fs::Permissions::from_mode(0o700)).map_err(|e| e.to_string())?;
    let f = OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(dir.join(".lock"))
        .map_err(|e| e.to_string())?;
    f.lock().map_err(|e| e.to_string())?;
    Ok(f)
}
fn read(path: &Path) -> Res<Input> {
    let data = fs::read(path).map_err(|e| e.to_string())?;
    if data.len() > 64 * 1024 {
        return Err("input record too large".into());
    }
    serde_json::from_slice(&data).map_err(|e| e.to_string())
}
fn save(path: &Path, input: &Input) -> Res<()> {
    write_atomic(
        path,
        &serde_json::to_string(input).map_err(|e| e.to_string())?,
    )
}
fn matches(input: &Input, agent: &Agent) -> bool {
    input.agent == agent.name
        && Some(input.session_id.as_str()) == agent.session_id.as_deref()
        && Some(input.pane.as_str()) == agent.pane_id.as_deref()
        && input.cwd == agent.cwd
        && input.agent_created_at == agent.created_at
}

pub fn enqueue(
    home: &Path,
    agent: &Agent,
    id: &str,
    text: &str,
    expires_at: Option<u64>,
    stop_file: Option<&Path>,
) -> Res<Input> {
    if !matches!(agent.kind, Kind::Codex | Kind::Claude) {
        return Err("queued input currently supports Codex and Claude conversations".into());
    }
    if !matches!(
        agent.status,
        Status::Running | Status::Idle | Status::NeedsReply
    ) {
        return Err("conversation cannot receive queued input in its current state".into());
    }
    if text.trim().is_empty()
        || text.len() > 16 * 1024
        || text
            .chars()
            .any(|c| c.is_control() && c != '\n' && c != '\t')
    {
        return Err(
            "message must be nonempty text, at most 16 KiB, without terminal control characters"
                .into(),
        );
    }
    let session_id = agent
        .session_id
        .clone()
        .filter(|s| !s.is_empty())
        .ok_or("conversation has no native session yet")?;
    let pane = agent.pane_id.clone().ok_or("conversation has no pane")?;
    if stop_file.is_some_and(|p| !p.is_absolute()) {
        return Err("stop file must be absolute".into());
    }
    let dest = path(home, &agent.name, id)?;
    let _lock = lock(dest.parent().unwrap())?;
    if dest.exists() {
        let old = read(&dest)?;
        return if matches(&old, agent)
            && old.text == text
            && old.expires_at == expires_at
            && old.stop_file.as_deref() == stop_file
        {
            Ok(old)
        } else {
            Err("message id already belongs to different input".into())
        };
    }
    let transcript = transcript_path(agent.kind, &session_id);
    let transcript_offset = transcript
        .as_ref()
        .and_then(|p| p.metadata().ok())
        .map_or(0, |m| m.len());
    let input = Input {
        id: id.into(),
        agent: agent.name.clone(),
        session_id,
        pane,
        cwd: agent.cwd.clone(),
        agent_created_at: agent.created_at,
        created_at: now_secs(),
        queued_at_ns: std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos() as u64,
        text: text.into(),
        phase: Phase::Queued,
        reason: None,
        expires_at,
        stop_file: stop_file.map(Path::to_path_buf),
        transcript,
        transcript_offset,
        transcript_scanned: transcript_offset,
    };
    save(&dest, &input)?;
    Ok(input)
}

pub fn receipt(home: &Path, agent: &str, id: &str) -> Res<Input> {
    read(&path(home, agent, id)?)
}

pub fn cancel(home: &Path, agent: &str, id: &str) -> Res<Input> {
    let dest = path(home, agent, id)?;
    let _lock = lock(dest.parent().unwrap())?;
    let mut input = read(&dest)?;
    if input.phase != Phase::Queued {
        return Err("only an unsent queued message can be cancelled".into());
    }
    input.phase = Phase::Cancelled;
    save(&dest, &input)?;
    Ok(input)
}

/// A conservative adapter: unfamiliar screens and nonempty/multiline drafts remain queued.
/// It never clears the composer or types an approval response.
pub fn composer_empty(kind: Kind, screen: &str) -> bool {
    let lines: Vec<&str> = screen.lines().collect();
    let marker = match kind {
        Kind::Codex => '›',
        Kind::Claude => '❯',
        _ => return false,
    };
    let Some(at) = lines
        .iter()
        .rposition(|line| line.trim_start().starts_with(marker))
    else {
        return false;
    };
    let prompt = lines[at].trim().strip_prefix(marker).unwrap().trim();
    let blank = prompt.is_empty() || (kind == Kind::Codex && prompt == "Ask Codex to do anything");
    blank
        && lines[at + 1..].iter().all(|line| {
            let s = line.trim();
            s.is_empty()
                || s.starts_with("GPT-")
                || s.starts_with("gpt-")
                || s.starts_with("? for shortcuts")
                || s.starts_with("enter/esc")
                || s.starts_with("⏵⏵")
        })
}

fn composer_contains(kind: Kind, screen: &str, expected: &str) -> bool {
    let marker = if kind == Kind::Codex { '›' } else { '❯' };
    let lines: Vec<_> = screen.lines().collect();
    let Some(at) = lines
        .iter()
        .rposition(|line| line.trim_start().starts_with(marker))
    else {
        return false;
    };
    let body = lines[at..]
        .iter()
        .take_while(|line| {
            let s = line.trim();
            !s.starts_with("GPT-")
                && !s.starts_with("gpt-")
                && !s.starts_with("? for shortcuts")
                && !s.starts_with("⏵⏵")
        })
        .enumerate()
        .map(|(n, line)| {
            if n == 0 {
                line.trim_start().trim_start_matches(marker)
            } else {
                line
            }
        })
        .collect::<Vec<_>>()
        .join("\n");
    let compact = |s: &str| s.chars().filter(|c| !c.is_whitespace()).collect::<String>();
    compact(&body) == compact(expected)
}
fn current_target(input: &Input, tmux: &Tmux) -> Res<bool> {
    let Some(agent) = crate::agent::load(&input.agent)? else {
        return Ok(false);
    };
    Ok(matches(input, &agent)
        && matches!(
            agent.effective_status(&tmux.panes()),
            Status::Running | Status::Idle | Status::NeedsReply
        ))
}

fn inputs(home: &Path, name: &str) -> Res<Vec<(PathBuf, Input)>> {
    let dir = directory(home, name)?;
    if !dir.exists() {
        return Ok(Vec::new());
    }
    let mut found = Vec::new();
    for entry in fs::read_dir(dir).map_err(|e| e.to_string())? {
        let p = entry.map_err(|e| e.to_string())?.path();
        if p.extension().is_some_and(|e| e == "json") {
            found.push((p.clone(), read(&p)?));
        }
    }
    found.sort_by(|a, b| (a.1.queued_at_ns, &a.1.id).cmp(&(b.1.queued_at_ns, &b.1.id)));
    Ok(found)
}

/// Called with desktop keyboard writes serialized by the PTY owner. At most one message per tick.
pub fn pump_one(home: &Path, agents: &[Agent], tmux: &Tmux) -> Res<()> {
    for agent in agents {
        let dir = directory(home, &agent.name)?;
        if !dir.exists() {
            continue;
        }
        let _lock = lock(&dir)?;
        for (dest, mut input) in inputs(home, &agent.name)? {
            if input.phase == Phase::Written && matches(&input, agent) {
                let previous = input.transcript_scanned;
                match native_received(&mut input) {
                    Ok(true) => {
                        input.phase = Phase::Received;
                        save(&dest, &input)?;
                    }
                    Ok(false) if previous != input.transcript_scanned => save(&dest, &input)?,
                    Ok(false) => (),
                    Err(error) => {
                        input.phase = Phase::Unknown;
                        input.reason = Some(error);
                        save(&dest, &input)?;
                    }
                }
            }
            if input.phase == Phase::Delivering {
                input.phase = Phase::Unknown;
                input.reason = Some(
                    "delivery interrupted; inspect the original conversation before retrying"
                        .into(),
                );
                save(&dest, &input)?;
            }
            if input.phase != Phase::Queued {
                continue;
            }
            if input.expires_at.is_some_and(|end| now_secs() >= end)
                || input.stop_file.as_ref().is_some_and(|p| p.exists())
            {
                input.phase = Phase::Cancelled;
                input.reason = Some("message stopped or deadline reached".into());
                save(&dest, &input)?;
                continue;
            }
            if !matches(&input, agent) {
                input.phase = Phase::Cancelled;
                input.reason = Some("conversation identity changed".into());
                save(&dest, &input)?;
                continue;
            }
            if !matches!(
                agent.status,
                Status::Running | Status::Idle | Status::NeedsReply
            ) {
                break;
            }
            let screen = tmux.run(["capture-pane", "-p", "-t", &input.pane])?;
            if !composer_empty(agent.kind, &screen) {
                break;
            }
            if !current_target(&input, tmux)? {
                break;
            }
            // Persist before any bytes: a crash/ambiguous send can never cause an automatic retry.
            if input.expires_at.is_some_and(|end| now_secs() >= end)
                || input.stop_file.as_ref().is_some_and(|p| p.exists())
            {
                continue;
            }
            input.phase = Phase::Delivering;
            save(&dest, &input)?;
            let buffer = format!("shifu-input-{}", input.id);
            let result = (|| {
                tmux.run(["set-buffer", "-b", &buffer, "--", &input.text])?;
                tmux.run(["paste-buffer", "-p", "-d", "-b", &buffer, "-t", &input.pane])?;
                std::thread::sleep(std::time::Duration::from_millis(300));
                let screen = tmux.run(["capture-pane", "-p", "-t", &input.pane])?;
                if input.expires_at.is_some_and(|end| now_secs() >= end)
                    || input.stop_file.as_ref().is_some_and(|p| p.exists())
                    || !current_target(&input, tmux)?
                    || !composer_contains(agent.kind, &screen, &input.text)
                {
                    return Err(
                        "composer changed after paste; input not submitted, inspect original pane"
                            .into(),
                    );
                }
                tmux.run(["send-keys", "-t", &input.pane, "Enter"])?;
                Ok::<_, String>(())
            })();
            input.phase = if result.is_ok() {
                Phase::Written
            } else {
                Phase::Unknown
            };
            input.reason = result.err();
            save(&dest, &input)?;
            return Ok(());
        }
    }
    Ok(())
}

fn find_transcript(dir: &Path, suffix: &str, depth: usize) -> Option<PathBuf> {
    if depth == 0 {
        return None;
    }
    for entry in fs::read_dir(dir).ok()?.flatten() {
        if entry.file_type().ok()?.is_dir() {
            if let Some(found) = find_transcript(&entry.path(), suffix, depth - 1) {
                return Some(found);
            }
        } else if entry.file_name().to_string_lossy().ends_with(suffix) {
            return Some(entry.path());
        }
    }
    None
}
fn transcript_path(kind: Kind, session: &str) -> Option<PathBuf> {
    let home = PathBuf::from(std::env::var_os("HOME")?);
    match kind {
        Kind::Codex => find_transcript(
            &std::env::var_os("CODEX_HOME")
                .map(PathBuf::from)
                .unwrap_or_else(|| home.join(".codex"))
                .join("sessions"),
            &format!("-{session}.jsonl"),
            6,
        ),
        Kind::Claude => find_transcript(
            &home.join(".claude/projects"),
            &format!("{session}.jsonl"),
            3,
        ),
        _ => None,
    }
}

/// Confirm only this message in this native log, after its enqueue offset. No whole-history scan.
fn native_received(input: &mut Input) -> Res<bool> {
    let Some(path) = &input.transcript else {
        return Ok(false);
    };
    let mut file = File::open(path).map_err(|e| e.to_string())?;
    let len = file.metadata().map_err(|e| e.to_string())?.len();
    if len == input.transcript_scanned {
        return Ok(false);
    }
    let start = input
        .transcript_offset
        .max(len.saturating_sub(2 * 1024 * 1024));
    file.seek(SeekFrom::Start(start))
        .map_err(|e| e.to_string())?;
    let mut bytes = Vec::new();
    file.take(2 * 1024 * 1024)
        .read_to_end(&mut bytes)
        .map_err(|e| e.to_string())?;
    input.transcript_scanned = len;
    let text = String::from_utf8_lossy(&bytes);
    for (n, line) in text.lines().enumerate() {
        if n == 0 && start > input.transcript_offset {
            continue;
        }
        let Ok(value) = serde_json::from_str::<serde_json::Value>(line) else {
            continue;
        };
        let message = if value["type"] == "response_item" && value["payload"]["type"] == "message" {
            &value["payload"]
        } else if value["type"] == "user" {
            &value["message"]
        } else {
            continue;
        };
        if message["role"] != "user" {
            continue;
        }
        let content = &message["content"];
        let plain = content.as_str().map(str::to_string).unwrap_or_else(|| {
            content
                .as_array()
                .into_iter()
                .flatten()
                .filter(|p| p["type"] == "input_text" || p["type"] == "text")
                .filter_map(|p| p["text"].as_str())
                .collect::<Vec<_>>()
                .join("\n")
        });
        if plain.trim() == input.text.trim() {
            return Ok(true);
        }
    }
    Ok(false)
}

/// A native UserPromptSubmit hook confirms receipt; writing bytes alone does not.
pub fn acknowledge(home: &Path, agent: &Agent, prompt: &str) -> Res<()> {
    let dir = directory(home, &agent.name)?;
    if !dir.exists() {
        return Ok(());
    }
    let _lock = lock(&dir)?;
    for (dest, mut input) in inputs(home, &agent.name)? {
        if input.phase == Phase::Written
            && matches(&input, agent)
            && input.text.trim() == prompt.trim()
        {
            input.phase = Phase::Received;
            input.reason = None;
            save(&dest, &input)?;
            break;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn only_recognized_empty_composers_are_writable() {
        assert!(composer_empty(
            Kind::Codex,
            "Working\n› Ask Codex to do anything\n\nGPT-6\n? for shortcuts"
        ));
        assert!(composer_empty(
            Kind::Claude,
            "Working\n❯\n⏵⏵ bypass permissions on"
        ));
        for screen in [
            "› my unfinished draft",
            "›\n  second line draft\nGPT-6",
            "permission required\n1. approve\n2. deny",
            "› Ask Codex to do anything\n  pasted second line",
        ] {
            assert!(!composer_empty(Kind::Codex, screen), "{screen}");
        }
    }
}
