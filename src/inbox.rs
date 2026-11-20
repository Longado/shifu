//! Durable input for an existing desktop conversation. Native input stays on the same pane.
use crate::agent::{Agent, Kind, Res, now_secs};
use crate::app_session::write_atomic;
use crate::status::Status;
use crate::tmux::Tmux;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs::{self, File};
use std::io::{Read, Seek, SeekFrom};
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub native_receipt_end: Option<u64>,
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
    crate::agent::lock_in(dir)
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
        native_receipt_end: None,
    };
    save(&dest, &input)?;
    Ok(input)
}

/// Public list projection: never expose native transcript locations or full message bodies.
#[derive(Clone, Debug, Serialize)]
pub struct ReceiptSummary {
    pub id: String,
    pub summary: String,
    pub created_at: u64,
    pub phase: Phase,
}

impl From<Input> for ReceiptSummary {
    fn from(input: Input) -> Self {
        let compact = input.text.split_whitespace().collect::<Vec<_>>().join(" ");
        let mut summary: String = compact.chars().take(120).collect();
        if compact.chars().count() > 120 { summary.push('…'); }
        Self { id: input.id, summary, created_at: input.created_at, phase: input.phase }
    }
}

fn current_conversation(home: &Path, name: &str) -> Res<Agent> {
    directory(home, name)?;
    let data = fs::read(home.join("agents").join(format!("{name}.json")))
        .map_err(|_| "conversation no longer exists".to_string())?;
    serde_json::from_slice(&data).map_err(|e| e.to_string())
}

// Agent deletion preserves inbox history. Resume preserves the native session but can change pane.
// Include the native id so reusing a name within one timestamp second cannot expose old input.
fn same_conversation(input: &Input, agent: &Agent) -> bool {
    input.agent == agent.name && input.agent_created_at == agent.created_at && input.cwd == agent.cwd
        && Some(input.session_id.as_str()) == agent.session_id.as_deref()
}

/// Recent receipts for the current conversation, newest first; no transcript access.
pub fn recent(home: &Path, name: &str) -> Res<Vec<ReceiptSummary>> {
    let agent = current_conversation(home, name)?;
    let mut rows: Vec<Input> = inputs(home, name)?.into_iter().map(|(_, row)| row)
        .filter(|row| same_conversation(row, &agent)).collect();
    rows.sort_by(|a, b| {
        let time = |row: &Input| if row.queued_at_ns == 0 {
            row.created_at.saturating_mul(1_000_000_000)
        } else { row.queued_at_ns };
        (time(b), &b.id).cmp(&(time(a), &a.id))
    });
    Ok(rows.into_iter().take(30).map(ReceiptSummary::from).collect())
}

/// Desktop cancellation is scoped to the currently visible conversation identity.
pub fn cancel_current(home: &Path, name: &str, id: &str) -> Res<ReceiptSummary> {
    cancel_checked(home, name, id, |input| {
        if same_conversation(input, &current_conversation(home, name)?) {
            Ok(())
        } else {
            Err("message belongs to an earlier conversation".into())
        }
    }).map(ReceiptSummary::from)
}

pub fn receipt(home: &Path, agent: &str, id: &str) -> Res<Input> {
    read(&path(home, agent, id)?)
}

pub fn cancel(home: &Path, agent: &str, id: &str) -> Res<Input> {
    cancel_checked(home, agent, id, |_| Ok(()))
}

fn cancel_checked(home: &Path, agent: &str, id: &str, check: impl FnOnce(&Input) -> Res<()>) -> Res<Input> {
    let dest = path(home, agent, id)?;
    let _lock = lock(dest.parent().unwrap())?;
    let mut input = read(&dest)?;
    check(&input)?;
    if input.phase != Phase::Queued {
        return Err("only an unsent queued message can be cancelled".into());
    }
    input.phase = Phase::Cancelled;
    save(&dest, &input)?;
    Ok(input)
}

/// A conservative adapter: unfamiliar screens and nonempty/multiline drafts remain queued.
/// It never clears the composer or types an approval response.
/// Where the agent's own footer starts under its input box: Codex's model line, the rule Claude draws under the box.
/// From there down the lines are the agent's status. They change between versions and wrap in a narrow pane, so
/// nothing below is read; a draft's further lines sit above it.
fn footer_starts(line: &str) -> bool {
    let s = line.trim();
    s.starts_with("GPT-")
        || s.starts_with("gpt-")
        || s.contains("? for shortcuts")
        || s.starts_with("enter/esc")
        || s.starts_with("⏵⏵")
        || (!s.is_empty() && s.chars().all(|c| c == '─'))
}

/// A screen line as text and, per character, whether it is drawn dim. `capture-pane -e` keeps the colour
/// sequences; a line captured without them comes back unchanged and never dim.
fn styled(line: &str) -> Vec<(char, bool)> {
    let mut out = Vec::new();
    let mut dim = false;
    let mut chars = line.chars();
    while let Some(c) = chars.next() {
        if c != '\x1b' {
            out.push((c, dim));
            continue;
        }
        if chars.next() != Some('[') {
            continue;
        }
        let sequence: String = chars.by_ref().take_while(|c| !('@'..='~').contains(c)).collect();
        // Only `m` sequences reach here from capture-pane. 38/48 take a colour whose numbers are not attributes.
        let mut codes = sequence.split(';');
        while let Some(code) = codes.next() {
            match code {
                "" | "0" | "22" => dim = false,
                "2" => dim = true,
                "38" | "48" => {
                    let skip = if codes.next() == Some("2") { 3 } else { 1 };
                    codes.by_ref().take(skip).for_each(drop);
                }
                _ => (),
            }
        }
    }
    out
}

pub fn composer_empty(kind: Kind, screen: &str) -> bool {
    let styled: Vec<Vec<(char, bool)>> = screen.lines().map(styled).collect();
    let plain: Vec<String> = styled.iter().map(|line| line.iter().map(|(c, _)| c).collect()).collect();
    let lines: Vec<&str> = plain.iter().map(String::as_str).collect();
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
    // Dim text in the box is the agent's own: Codex's placeholder, or the next message Claude suggests.
    let typed = styled[at].iter().skip_while(|(c, _)| *c != marker).skip(1).any(|(c, dim)| !c.is_whitespace() && !dim);
    let blank = !typed || (kind == Kind::Codex && prompt == "Ask Codex to do anything");
    blank
        && lines[at + 1..]
            .iter()
            .take_while(|line| !footer_starts(line))
            .all(|line| line.trim().is_empty())
}

pub fn composer_contains(kind: Kind, screen: &str, expected: &str) -> bool {
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
        .take_while(|line| !footer_starts(line))
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
    pump_one_with_input_lock(home, agents, tmux, |_, send| send())
}

/// Keep history and receipt scanning outside the desktop input lock. The lock covers
/// the final composer check through submission, so a draft cannot race a paste.
pub fn pump_one_with_input_lock(
    home: &Path,
    agents: &[Agent],
    tmux: &Tmux,
    serialize: impl FnMut(&str, &mut dyn FnMut() -> Res<()>) -> Res<()>,
) -> Res<()> {
    InboxPump::default().tick(home, agents, tmux, serialize)
}

// Shifu replaces receipt files atomically, changing the directory. A periodic scan also picks up in-place edits.
const QUIET_RECHECK: Duration = Duration::from_secs(60);

#[derive(Eq, PartialEq)]
struct DirectoryStamp {
    device: u64,
    inode: u64,
    modified: (i64, i64),
    changed: (i64, i64),
}

impl DirectoryStamp {
    fn read(dir: &Path) -> std::io::Result<Self> {
        let m = fs::metadata(dir)?;
        Ok(Self {
            device: m.dev(),
            inode: m.ino(),
            modified: (m.mtime(), m.mtime_nsec()),
            changed: (m.ctime(), m.ctime_nsec()),
        })
    }
}

/// A desktop watcher remembers settled inboxes and rotates among conversations ready for delivery.
#[derive(Default)]
pub struct InboxPump {
    quiet: HashMap<PathBuf, (DirectoryStamp, Instant)>,
    last_served: Option<String>,
}

impl InboxPump {
    pub fn tick(
        &mut self,
        home: &Path,
        agents: &[Agent],
        tmux: &Tmux,
        mut serialize: impl FnMut(&str, &mut dyn FnMut() -> Res<()>) -> Res<()>,
    ) -> Res<()> {
        self.quiet.retain(|_, (_, checked)| checked.elapsed() < QUIET_RECHECK);
        let start = self.last_served.as_ref()
            .and_then(|name| agents.iter().position(|a| &a.name == name))
            .map_or(0, |index| index + 1);
        let mut first_error = None;
        for agent in agents[start..].iter().chain(&agents[..start]) {
            match self.visit(home, agent, tmux, &mut serialize) {
                Ok(true) => {
                    self.last_served = Some(agent.name.clone());
                    break;
                }
                Ok(false) => {}
                Err(error) => {
                    first_error.get_or_insert_with(|| format!("{}: {error}", agent.name));
                }
            }
        }
        first_error.map_or(Ok(()), Err)
    }

    fn visit(
        &mut self,
        home: &Path,
        agent: &Agent,
        tmux: &Tmux,
        serialize: &mut impl FnMut(&str, &mut dyn FnMut() -> Res<()>) -> Res<()>,
    ) -> Res<bool> {
        let dir = directory(home, &agent.name)?;
        let current = match DirectoryStamp::read(&dir) {
            Ok(stamp) => stamp,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                self.quiet.remove(&dir);
                return Ok(false);
            }
            Err(e) => return Err(e.to_string()),
        };
        if self.quiet.get(&dir).is_some_and(|(stamp, _)| *stamp == current) {
            return Ok(false);
        }
        self.quiet.remove(&dir);
        let _lock = lock(&dir)?;
        let before_scan = DirectoryStamp::read(&dir).map_err(|e| e.to_string())?;
        let mut history = inputs(home, &agent.name)?;
        if history.iter().all(|(_, row)| matches!(row.phase, Phase::Received | Phase::Cancelled | Phase::Unknown)) {
            self.quiet.insert(dir, (before_scan, Instant::now()));
            return Ok(false);
        }
        let awaiting: std::collections::HashSet<String> = history.iter()
            .filter(|(_, row)| row.phase == Phase::Written && matches(row, agent))
            .map(|(_, row)| row.text.trim().to_owned()).collect();
        for index in 0..history.len() {
            let row = &history[index].1;
            let reconcile = row.phase == Phase::Received && row.native_receipt_end.is_none()
                && awaiting.contains(row.text.trim()) && matches(row, agent);
            if !matches!(row.phase, Phase::Written | Phase::Queued | Phase::Delivering) && !reconcile {
                continue;
            }
            let (dest, mut input) = history[index].clone();
            if (input.phase == Phase::Written || (input.phase == Phase::Received && input.native_receipt_end.is_none())) && matches(&input, agent) {
                let previous = input.transcript_scanned;
                match native_received(&mut input, &history) {
                    Ok(true) => {
                        input.phase = Phase::Received;
                        save(&dest, &input)?;
                    }
                    Ok(false) if previous != input.transcript_scanned => save(&dest, &input)?,
                    Ok(false) => (),
                    Err(_) if input.phase == Phase::Received => {
                        // A lost log cannot revoke a native hook receipt.
                    }
                    Err(error) => {
                        input.phase = Phase::Unknown;
                        input.reason = Some(error);
                        save(&dest, &input)?;
                    }
                }
            }
            history[index].1 = input.clone();
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
            let pane = input.pane.clone();
            serialize(&pane, &mut || {
                let screen = tmux.run(["capture-pane", "-e", "-p", "-t", &input.pane])?;
                if !composer_empty(agent.kind, &screen) {
                    return Ok(());
                }
                if !current_target(&input, tmux)? {
                    return Ok(());
                }
                // Persist before any bytes: a crash/ambiguous send can never cause an automatic retry.
                if input.expires_at.is_some_and(|end| now_secs() >= end)
                    || input.stop_file.as_ref().is_some_and(|p| p.exists())
                {
                    return Ok(());
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
                    // An identical prompt may have arrived while this one was queued.
                    // Only native events after this submission can acknowledge it.
                    if let Some(path) = &input.transcript {
                        if let Ok(metadata) = path.metadata() {
                            input.transcript_offset = metadata.len();
                            input.transcript_scanned = metadata.len();
                        } else {
                            // Without a trustworthy cursor, wait for the submit hook.
                            input.transcript = None;
                        }
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
                Ok(())
            })?;
            if input.phase == Phase::Queued {
                // This pane has a draft or is no longer ready. Leave its messages in order
                // and let another conversation receive a message on this tick.
                break;
            }
            return Ok(true);
        }
        Ok(false)
    }
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

/// Confirm only this message in this native log, after its submission offset. No whole-history scan.
fn native_received(input: &mut Input, history: &[(PathBuf, Input)]) -> Res<bool> {
    let Some(path) = &input.transcript else {
        return Ok(false);
    };
    let mut file = File::open(path).map_err(|e| e.to_string())?;
    let len = file.metadata().map_err(|e| e.to_string())?.len();
    // Hooks and transcript rows describe the same submissions. Reserve one transcript
    // row for every attempted identical send, including hook-confirmed and ambiguous
    // sends, rather than letting each Written record consume the same native row.
    let mut attempts: Vec<&Input> = history.iter().map(|(_, row)| row)
        .filter(|row| row.agent == input.agent
            && row.session_id == input.session_id
            && row.cwd == input.cwd
            && row.agent_created_at == input.agent_created_at
            && row.text.trim() == input.text.trim()
            && matches!(row.phase, Phase::Delivering | Phase::Written | Phase::Received | Phase::Unknown)
            && !(row.id != input.id && row.phase == Phase::Received
                && row.native_receipt_end.is_some_and(|end| end <= input.transcript_offset)))
        .collect();
    attempts.sort_by(|a, b| (a.transcript_offset, &a.id).cmp(&(b.transcript_offset, &b.id)));
    let earliest = attempts.first().map_or(input.transcript_offset, |row| row.transcript_offset);
    if len < input.transcript_offset { return Ok(false); }
    // Recover a legacy hook receipt only from its own non-overlapping historical
    // window. A USER after the next identical submission cannot identify the old
    // hook, and must not be silently spent on it. The fixed end also makes a
    // failed bounded recovery a one-time read while the live log keeps growing.
    let recovery_end = if input.phase == Phase::Received {
        if attempts.iter().any(|row| row.id != input.id && row.transcript_offset == input.transcript_offset) {
            return Ok(false);
        }
        attempts.iter().filter(|row| row.transcript_offset > input.transcript_offset)
            .map(|row| row.transcript_offset).min()
            .map(|next| next.min(input.transcript_offset.saturating_add(2 * 1024 * 1024)))
    } else { None };
    let end = recovery_end.unwrap_or(len).min(len);
    if end == input.transcript_scanned { return Ok(false); }
    let start = if recovery_end.is_some() { input.transcript_offset }
        else { earliest.max(end.saturating_sub(2 * 1024 * 1024)) };
    file.seek(SeekFrom::Start(start))
        .map_err(|e| e.to_string())?;
    let mut bytes = Vec::new();
    file.take(end.saturating_sub(start))
        .read_to_end(&mut bytes)
        .map_err(|e| e.to_string())?;
    input.transcript_scanned = end;
    let mut position = start;
    let mut events = Vec::new();
    for line in bytes.split_inclusive(|byte| *byte == b'\n') {
        let event_start = position;
        position += line.len() as u64;
        // An incomplete JSONL row may still be extended by the native writer.
        if !line.ends_with(b"\n") || (event_start == start && start > earliest) { continue; }
        let Ok(value) = serde_json::from_slice::<serde_json::Value>(line) else {
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
        if plain.trim() == input.text.trim()
            && attempts.get(events.len()).is_some_and(|row| event_start >= row.transcript_offset)
        {
            events.push(position);
        }
    }
    let rank = attempts.iter().position(|row| row.id == input.id);
    if let Some(end) = rank.and_then(|rank| events.get(rank)).copied() {
        if input.phase == Phase::Received || events.len() == attempts.len() {
            input.native_receipt_end = Some(end);
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
    let mut history = inputs(home, &agent.name)?;
    let mut written = history.iter().enumerate().filter(|(_, (_, row))| {
        matches(row, agent) && row.phase == Phase::Written && row.text.trim() == prompt.trim()
    });
    let Some((index, _)) = written.next() else { return Ok(()) };
    if written.next().is_some() { return Ok(()); }

    // A hook and its transcript row are two observations of the same submission.
    // Resolve earlier hook receipts before giving this text to the pending input.
    for i in 0..history.len() {
        let (dest, old) = &history[i];
        if !same_conversation(old, agent) || old.text.trim() != prompt.trim()
            || old.phase != Phase::Received || old.native_receipt_end.is_some() {
            continue;
        }
        let mut reconciled = old.clone();
        if native_received(&mut reconciled, &history).unwrap_or(false) {
            save(dest, &reconciled)?;
            history[i].1 = reconciled;
        }
    }
    let candidate = &history[index].1;
    if history.iter().any(|(_, row)| row.id != candidate.id
        && same_conversation(row, agent) && row.text.trim() == prompt.trim()
        && matches!(row.phase, Phase::Delivering | Phase::Written | Phase::Received | Phase::Unknown)
        && !(row.phase == Phase::Received && row.native_receipt_end.is_some_and(|end| end <= candidate.transcript_offset))) {
        return Ok(());
    }
    let (dest, input) = &mut history[index];
    input.phase = Phase::Received;
    input.reason = None;
    save(dest, input)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quiet_history_wakes_for_changed_receipts_and_a_bad_inbox_is_isolated() {
        let home = std::env::temp_dir().join(format!("shifu-pump-wakeup-{}", std::process::id()));
        let dir = home.join("inbox/demo");
        fs::create_dir_all(&dir).unwrap();
        let agent: Agent = serde_json::from_value(serde_json::json!({
            "name":"demo", "kind":"codex", "command":"codex", "status":"idle", "status_since":1,
            "cwd":"/work", "created_at":1, "session_id":"s", "pane_id":"%1"
        })).unwrap();
        let mut row: Input = serde_json::from_value(serde_json::json!({
            "id":"one", "agent":"demo", "session_id":"s", "pane":"%1", "cwd":"/work",
            "agent_created_at":1, "created_at":1, "text":"fixture", "phase":"received"
        })).unwrap();
        let dest = dir.join("one.json");
        save(&dest, &row).unwrap();
        let mut pump = InboxPump::default();
        let tmux = Tmux::from_env();
        let tick = |pump: &mut InboxPump, agents: &[Agent]| pump.tick(&home, agents, &tmux, |_, _| panic!("expired input reached a pane"));
        tick(&mut pump, std::slice::from_ref(&agent)).unwrap();
        let checked = pump.quiet[&dir].1;
        tick(&mut pump, std::slice::from_ref(&agent)).unwrap();
        assert_eq!(pump.quiet[&dir].1, checked);

        let broken = Agent { name: "broken".into(), ..agent.clone() };
        let other_dir = home.join("inbox/broken");
        fs::create_dir_all(&other_dir).unwrap();
        fs::write(other_dir.join("bad.json"), "[").unwrap();
        row.id = "two".into();
        row.phase = Phase::Queued;
        row.expires_at = Some(1);
        save(&dir.join("two.json"), &row).unwrap();
        let error = tick(&mut pump, &[broken, agent.clone()]).unwrap_err();
        assert!(error.contains("broken"));
        assert_eq!(receipt(&home, "demo", "two").unwrap().phase, Phase::Cancelled);

        tick(&mut pump, std::slice::from_ref(&agent)).unwrap();
        let mut interrupted = read(&dest).unwrap();
        interrupted.phase = Phase::Delivering;
        fs::write(&dest, serde_json::to_vec(&interrupted).unwrap()).unwrap();
        pump.quiet.get_mut(&dir).unwrap().1 = Instant::now() - QUIET_RECHECK;
        tick(&mut pump, &[agent]).unwrap();
        assert_eq!(read(&dest).unwrap().phase, Phase::Unknown);
        fs::remove_dir_all(home).unwrap();
    }

    #[test]
    fn native_receipts_reserve_old_attempts_and_require_complete_bounded_evidence() {
        let root = std::env::temp_dir().join(format!("shifu-native-receipts-{}", std::process::id()));
        fs::create_dir_all(&root).unwrap();
        let log = root.join("native.jsonl");
        // Legacy records lack queued_at_ns and scan cursor; defaults remain readable.
        let mut old: Input = serde_json::from_value(serde_json::json!({
            "id":"old", "agent":"demo", "session_id":"s", "pane":"%1", "cwd":"/work",
            "agent_created_at":1, "created_at":1, "text":"again", "phase":"received",
            "reason":null, "transcript":log, "transcript_offset":0
        })).unwrap();
        let event = "{\"type\":\"user\",\"message\":{\"role\":\"user\",\"content\":\"again\"}}\n";
        fs::write(&log, event).unwrap();
        let mut next = old.clone();
        next.id = "next".into();
        next.phase = Phase::Written;
        next.transcript_offset = event.len() as u64;
        // A previous hook-confirmed attempt still reserves its delayed native row.
        fs::write(&log, format!("{event}{event}")).unwrap();
        let history = vec![(PathBuf::new(), old.clone()), (PathBuf::new(), next.clone())];
        assert!(native_received(&mut next, &history).unwrap());
        let mut uncertain = old.clone();
        uncertain.phase = Phase::Unknown;
        fs::write(&log, event).unwrap();
        next.transcript_scanned = 0;
        let uncertain_history = vec![(PathBuf::new(), uncertain), (PathBuf::new(), next.clone())];
        assert!(!native_received(&mut next, &uncertain_history).unwrap());
        fs::write(&log, format!("{event}{event}")).unwrap();
        // A foreign session and different text never reserve this conversation's evidence.
        let mut foreign = old.clone();
        foreign.session_id = "other".into();
        let mut different = old.clone();
        different.text = "different".into();
        let history = vec![(PathBuf::new(), next.clone()), (PathBuf::new(), foreign), (PathBuf::new(), different)];
        next.transcript_scanned = 0;
        assert!(native_received(&mut next, &history).unwrap());
        // Partial rows are not receipts; completing the same row is observed later.
        fs::write(&log, event.trim_end()).unwrap();
        old.transcript_scanned = 0;
        let history = vec![(PathBuf::new(), old.clone())];
        assert!(!native_received(&mut old, &history).unwrap());
        fs::write(&log, event).unwrap();
        assert!(native_received(&mut old, &history).unwrap());
        // Rotation/truncation cannot move an old submission's cursor backwards.
        next.transcript_scanned = u64::MAX;
        fs::write(&log, "").unwrap();
        assert!(!native_received(&mut next, &history).unwrap());
        // An empty oversized suffix cannot masquerade as receipt evidence.
        let file = File::create(&log).unwrap();
        file.set_len(2 * 1024 * 1024 + 1).unwrap();
        old.transcript_scanned = 0;
        assert!(!native_received(&mut old, &history).unwrap());
        // A unique submission still finds its row in the bounded tail of a huge log.
        use std::io::Write;
        writeln!(fs::OpenOptions::new().append(true).open(&log).unwrap()).unwrap();
        write!(fs::OpenOptions::new().append(true).open(&log).unwrap(), "{event}").unwrap();
        old.transcript_scanned = 0;
        old.phase = Phase::Written;
        let history = vec![(PathBuf::new(), old.clone())];
        assert!(native_received(&mut old, &history).unwrap());
        assert_eq!(old.native_receipt_end, Some(log.metadata().unwrap().len()));
        // A known consumed event, however old, does not reserve new evidence.
        old.phase = Phase::Received;
        next.transcript_offset = log.metadata().unwrap().len();
        next.transcript_scanned = 0;
        write!(fs::OpenOptions::new().append(true).open(&log).unwrap(), "{event}").unwrap();
        let history = vec![(PathBuf::new(), old), (PathBuf::new(), next.clone())];
        assert!(native_received(&mut next, &history).unwrap());
        // A bounded tail can start inside a Chinese UTF-8 code point. Receipt
        // positions must still use original bytes, not replacement-character text.
        let mut unicode_log = format!("{}\n{event}", serde_json::json!({
            "type":"assistant", "content":"汉".repeat(800_000)
        }));
        let start = unicode_log.len() - 2 * 1024 * 1024;
        if std::str::from_utf8(&unicode_log.as_bytes()[start..]).is_ok() {
            unicode_log.insert(unicode_log.len() - 1, ' ');
        }
        let start = unicode_log.len() - 2 * 1024 * 1024;
        assert!(std::str::from_utf8(&unicode_log.as_bytes()[start..]).is_err());
        fs::write(&log, &unicode_log).unwrap();
        next.transcript_offset = 0;
        next.transcript_scanned = 0;
        next.native_receipt_end = None;
        let history = vec![(PathBuf::new(), next.clone())];
        assert!(native_received(&mut next, &history).unwrap());
        assert_eq!(next.native_receipt_end, Some(unicode_log.len() as u64));
        fs::remove_file(&log).unwrap();
        assert!(native_received(&mut next, &history).is_err());
        fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn legacy_hook_recovery_uses_its_own_bounded_history_before_new_submission() {
        use std::io::Write;
        let root = std::env::temp_dir().join(format!("shifu-legacy-recovery-{}", std::process::id()));
        let agent: Agent = serde_json::from_value(serde_json::json!({
            "name":"demo", "kind":"codex", "command":"codex", "status":"idle", "status_since":1,
            "cwd":"/work", "created_at":1, "session_id":"s", "pane_id":"%1"
        })).unwrap();
        let event = "{\"type\":\"user\",\"message\":{\"role\":\"user\",\"content\":\"again\"}}\n";
        for has_old_user in [false, true] {
            let home = root.join(if has_old_user { "with-old" } else { "without-old" });
            fs::create_dir_all(home.join("inbox/demo")).unwrap();
            let log = home.join("native.jsonl");
            fs::write(&log, if has_old_user { event } else { "" }).unwrap();
            let mut file = fs::OpenOptions::new().append(true).open(&log).unwrap();
            writeln!(file, "{}", serde_json::json!({"type":"assistant","content":"x".repeat(2 * 1024 * 1024 + 100)})).unwrap();
            let new_offset = log.metadata().unwrap().len();
            write!(file, "{event}").unwrap();
            let old: Input = serde_json::from_value(serde_json::json!({
                "id":"old", "agent":"demo", "session_id":"s", "pane":"%1", "cwd":"/work",
                "agent_created_at":1, "created_at":1, "text":"again", "phase":"received",
                "reason":null, "transcript":log, "transcript_offset":0
            })).unwrap();
            let mut new = old.clone();
            new.id = "new".into();
            new.phase = Phase::Written;
            new.queued_at_ns = 2;
            new.transcript_offset = new_offset;
            new.transcript_scanned = new_offset;
            save(&path(&home, "demo", "old").unwrap(), &old).unwrap();
            save(&path(&home, "demo", "new").unwrap(), &new).unwrap();
            pump_one(&home, std::slice::from_ref(&agent), &Tmux::from_env()).unwrap();
            let old = receipt(&home, "demo", "old").unwrap();
            let new = receipt(&home, "demo", "new").unwrap();
            assert_eq!(old.phase, Phase::Received);
            assert_eq!(old.native_receipt_end, has_old_user.then_some(event.len() as u64));
            assert_eq!(new.phase, if has_old_user { Phase::Received } else { Phase::Written });
            let old_bytes = fs::read(path(&home, "demo", "old").unwrap()).unwrap();
            // Growing unrelated live output does not repeat recovery of a fixed old window.
            writeln!(file, "{{\"type\":\"assistant\"}}").unwrap();
            pump_one(&home, std::slice::from_ref(&agent), &Tmux::from_env()).unwrap();
            acknowledge(&home, &agent, "again").unwrap();
            assert_eq!(fs::read(path(&home, "demo", "old").unwrap()).unwrap(), old_bytes);
            assert_eq!(receipt(&home, "demo", "new").unwrap().phase, new.phase);
        }
        fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn completed_legacy_history_is_not_reconciled_without_pending_input() {
        let root = std::env::temp_dir().join(format!("shifu-completed-history-{}", std::process::id()));
        let dir = root.join("inbox/demo");
        fs::create_dir_all(&dir).unwrap();
        let log = root.join("native.jsonl");
        fs::write(&log, "{\"type\":\"user\",\"message\":{\"role\":\"user\",\"content\":\"old prompt\"}}\n").unwrap();
        let agent: Agent = serde_json::from_value(serde_json::json!({
            "name":"demo", "kind":"codex", "command":"codex", "status":"idle", "status_since":1,
            "cwd":"/work", "created_at":1, "session_id":"s", "pane_id":"%1"
        })).unwrap();
        let legacy = serde_json::json!({
            "id":"old", "agent":"demo", "session_id":"s", "pane":"%1", "cwd":"/work",
            "agent_created_at":1, "created_at":1, "text":"old prompt", "phase":"received",
            "reason":null, "transcript":log, "transcript_offset":0, "legacy_extra":"preserve bytes"
        }).to_string();
        let rows: Vec<String> = (0..71).map(|n| {
            let mut row: serde_json::Value = serde_json::from_str(&legacy).unwrap();
            row["id"] = serde_json::json!(format!("old{n}"));
            row.to_string()
        }).collect();
        for (n, row) in rows.iter().enumerate() { fs::write(dir.join(format!("old{n}.json")), row).unwrap(); }
        pump_one(&root, std::slice::from_ref(&agent), &Tmux::from_env()).unwrap();
        acknowledge(&root, &agent, "old prompt").unwrap();
        for (n, row) in rows.iter().enumerate() {
            assert_eq!(&fs::read_to_string(dir.join(format!("old{n}.json"))).unwrap(), row);
        }
        // Corrupt queue data propagates as an error instead of inventing a receipt.
        fs::write(dir.join("broken.json"), "{").unwrap();
        assert!(acknowledge(&root, &agent, "old prompt").is_err());
        assert!(pump_one(&root, std::slice::from_ref(&agent), &Tmux::from_env()).is_err());
        fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn recent_is_bounded_private_and_scoped_to_current_identity() {
        let home = std::env::temp_dir().join(format!("shifu-recent-{}", std::process::id()));
        let _ = fs::remove_dir_all(&home);
        fs::create_dir_all(home.join("agents")).unwrap();
        let agent: Agent = serde_json::from_value(serde_json::json!({
            "name":"demo", "kind":"codex", "command":"codex", "status":"idle", "status_since":10, "cwd":"/work", "created_at":10,
            "session_id":"session", "pane_id":"%1"
        })).unwrap();
        fs::write(home.join("agents/demo.json"), serde_json::to_vec(&agent).unwrap()).unwrap();
        for n in 0..35 {
            let mut row = enqueue(&home, &agent, &format!("m{n:02}"), &"中文 long\n".repeat(100), None, None).unwrap();
            row.created_at = 100 + n;
            row.queued_at_ns = if n % 2 == 0 { 0 } else { (100 + n) * 1_000_000_000 };
            save(&path(&home, "demo", &row.id).unwrap(), &row).unwrap();
        }
        let rows = recent(&home, "demo").unwrap();
        assert_eq!(rows.len(), 30);
        assert_eq!(rows[0].id, "m34");
        assert_eq!(rows[29].id, "m05");
        assert_eq!(rows[0].summary.chars().count(), 121);
        let json = serde_json::to_value(&rows[0]).unwrap();
        assert_eq!(json.as_object().unwrap().len(), 4);
        assert!(json.get("text").is_none() && json.get("transcript").is_none());
        assert_eq!(cancel_current(&home, "demo", "m34").unwrap().phase, Phase::Cancelled);
        assert!(cancel_current(&home, "demo", "m34").is_err());
        let mut resumed = agent.clone();
        resumed.pane_id = Some("%2".into());
        fs::write(home.join("agents/demo.json"), serde_json::to_vec(&resumed).unwrap()).unwrap();
        assert_eq!(recent(&home, "demo").unwrap().len(), 30);
        resumed.session_id = Some("new-native-session".into());
        fs::write(home.join("agents/demo.json"), serde_json::to_vec(&resumed).unwrap()).unwrap();
        assert!(recent(&home, "demo").unwrap().is_empty());
        assert!(cancel_current(&home, "demo", "m33").is_err());
        // The CLI single-message lookup remains available for explicit history inspection.
        assert_eq!(receipt(&home, "demo", "m33").unwrap().text, "中文 long\n".repeat(100));
        fs::remove_dir_all(home).unwrap();
    }
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
        // What the agents draw today (2026-11): Claude puts a rule under its input box, Codex a second footer line,
        // and a long model line wraps in a narrow pane.
        let rule = "─".repeat(40);
        assert!(composer_empty(
            Kind::Claude,
            &format!("⏺ 每分钟最多 60 次。\n{rule}\n❯ \n{rule}\n  ⏵⏵ bypass permissions on (shift+tab to cycle) · ← 1 agent")
        ));
        assert!(composer_empty(
            Kind::Codex,
            "› Ask Codex to do anything\n\n  GPT-6.2-Sol xhigh · ~/work/a-long-folder-name-that-\n  wraps · 标题\n  ← for agents · ? for shortcuts      ⚠ 3 warnings · f2 to view"
        ));
        assert!(!composer_empty(Kind::Claude, &format!("{rule}\n❯ \n  第二行草稿\n{rule}\n  ⏵⏵ bypass permissions on")));
        assert!(!composer_empty(Kind::Claude, &format!("{rule}\n❯ 写到一半\n{rule}\n  ⏵⏵ bypass permissions on")));
        assert!(composer_contains(
            Kind::Claude,
            &format!("{rule}\n❯ 这个任务的终点、重点和清单在 /h/goals/t-1.md。动手前\n  先读它\n{rule}\n  ⏵⏵ bypass permissions on"),
            "这个任务的终点、重点和清单在 /h/goals/t-1.md。动手前先读它"
        ));
        assert!(!composer_contains(Kind::Claude, &format!("{rule}\n❯ 别的话\n{rule}\n  ⏵⏵ bypass"), "要送的话"));
        // Captured with colours: a suggested next message (Claude) and the placeholder (Codex) are drawn dim and
        // are not input; typed text is not dim, whatever colour it has.
        assert!(composer_empty(Kind::Claude, &format!("{rule}\n\x1b[39m❯\u{a0}\x1b[2m确认\x1b[0m\n{rule}\n  ⏵⏵ bypass permissions on")));
        assert!(composer_empty(Kind::Codex, "\x1b[1m›\x1b[0m \x1b[2mAsk Codex to do anything\x1b[0m\n\n  \x1b[2mGPT-6.2-Sol xhigh\x1b[0m"));
        assert!(!composer_empty(Kind::Claude, &format!("{rule}\n\x1b[39m❯\u{a0}确认\x1b[0m\n{rule}\n  ⏵⏵ bypass permissions on")));
        assert!(!composer_empty(Kind::Claude, &format!("{rule}\n❯\u{a0}\x1b[38;2;200;200;200m写到一半\x1b[0m\n{rule}")));
        assert!(!composer_empty(Kind::Claude, &format!("{rule}\n❯\u{a0}\x1b[2m建议\x1b[22m又打了字\x1b[0m\n{rule}")));
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
