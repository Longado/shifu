//! Each task's 终点 (what done looks like), 重点 (the one thing to push now), checklist and 记录, in one Markdown
//! file per task under `<home>/goals/`. The user edits it on the progress page; the task's agents read and tick it.
//! 记录 is the task's shared space: what one conversation found or decided and the others need to know.
//!
//! Agents learn where the file is through the queued-input inbox, so a message waits until the conversation is
//! free instead of cutting into its work. `known.json` lists, per task, the agents already told (or deliberately
//! left alone when the file was first written), so only agents added later are told on their own.

use crate::config::{Config, write_atomic};
use crate::tasks::TaskFile;
use serde::Serialize;
use shifu::agent::{Agent, Kind};
use shifu::app_session::read_optional_text;
use shifu::status::Status;
use shifu::tmux::Tmux;
use std::collections::{BTreeMap, HashMap};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

// File read/modify/write sections never hold a pane input gate or wait for a paste.
static GOAL_FILES: Mutex<()> = Mutex::new(());
static KNOWN_FILE: Mutex<()> = Mutex::new(());

type Res<T> = Result<T, String>;

pub const TEMPLATE: &str = "# 终点\n\n\n# 重点\n\n\n# 清单\n\n\n# 记录\n\n";

#[derive(Serialize, Clone, Debug, PartialEq)]
pub struct TaskGoal {
    pub path: String,
    /// Empty when the task has no file yet.
    pub text: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub read_error: Option<String>,
}

fn dir(home: &Path) -> PathBuf {
    home.join("goals")
}

pub fn path(home: &Path, task_id: &str) -> PathBuf {
    dir(home).join(format!("{task_id}.md"))
}

fn read_text(home: &Path, task_id: &str) -> Res<String> {
    let file = path(home, task_id);
    read_optional_text(&file).map(Option::unwrap_or_default)
        .map_err(|e| format!("无法读取任务文件 {}（{e}）", file.display()))
}

/// A file counts once it says more than the empty template.
fn written(text: &str) -> bool {
    text.lines().any(|l| {
        let l = l.trim();
        !l.is_empty() && !l.starts_with('#')
    })
}

pub fn read_all(home: &Path, file: &TaskFile) -> BTreeMap<String, TaskGoal> {
    file.tasks
        .iter()
        .map(|t| {
            let (text, read_error) = match read_text(home, &t.id) {
                Ok(text) => (text, None),
                Err(error) => (String::new(), Some(error)),
            };
            let goal = TaskGoal { path: path(home, &t.id).display().to_string(), text, read_error };
            (t.id.clone(), goal)
        })
        .collect()
}

fn known_path(home: &Path) -> PathBuf {
    dir(home).join("known.json")
}

fn read_known(home: &Path) -> Res<BTreeMap<String, Vec<String>>> {
    let file = known_path(home);
    let Some(text) = read_optional_text(&file).map_err(|e| format!("read {} failed: {e}", file.display()))? else {
        return Ok(BTreeMap::new());
    };
    serde_json::from_str(&text).map_err(|e| format!("decode {} failed: {e}", file.display()))
}

fn add_known(home: &Path, task_id: &str, names: &[String]) -> Res<()> {
    let _file = KNOWN_FILE.lock().unwrap();
    add_known_locked(home, task_id, names)
}

fn add_known_locked(home: &Path, task_id: &str, names: &[String]) -> Res<()> {
    let mut known = read_known(home)?;
    let list = known.entry(task_id.to_string()).or_default();
    for name in names {
        if !list.contains(name) {
            list.push(name.clone());
        }
    }
    let text = serde_json::to_string_pretty(&known).map_err(|e| format!("encode goals/known.json failed: {e}"))?;
    write_atomic(&known_path(home), &text)
}

fn ensure_dir(home: &Path) -> Res<()> {
    std::fs::create_dir_all(dir(home)).map_err(|e| format!("create {} failed: {e}", dir(home).display()))
}

/// Saving the first real content leaves the task's current agents alone (they are told only when the user asks);
/// agents added after that are told automatically.
#[derive(Serialize, Debug, PartialEq)]
pub struct Saved {
    /// False when the file no longer says what the editor started from: nothing was written.
    pub saved: bool,
    /// The file as it is now: the saved text, or what a conversation made of it meanwhile.
    pub goal: TaskGoal,
}

/// Saves the user's edit made on top of `expected`. The task's agents tick and annotate the same file, so an
/// edit begun before their change would silently undo it; such a save is refused and the current file handed back.
pub fn save(cfg: &Config, file: &TaskFile, task_id: &str, expected: &str, text: &str) -> Res<Saved> {
    let _files = GOAL_FILES.lock().unwrap();
    let task = file.tasks.iter().find(|t| t.id == task_id).ok_or("task not found")?;
    ensure_dir(&cfg.home)?;
    let target = path(&cfg.home, task_id);
    let shown = target.display().to_string();
    let current = read_text(&cfg.home, task_id)?;
    if current != expected {
        return Ok(Saved { saved: false, goal: TaskGoal { path: shown, text: current, read_error: None } });
    }
    if !written(&current) {
        add_known(&cfg.home, task_id, &task.agents)?;
    }
    // The agents append lines to this file; a last line without its newline would get the next note glued to it.
    let text = if text.is_empty() || text.ends_with('\n') { text.to_string() } else { format!("{text}\n") };
    write_atomic(&target, &text)?;
    Ok(Saved { saved: true, goal: TaskGoal { path: shown, text, read_error: None } })
}

pub fn draft_note(path: &str) -> String {
    format!(
        "请先读 {path}。如果已有任务内容，保留原文并告诉我已有人写好。文件仍为空或只有标题时，根据这个任务到目前为止的对话起草：「# 终点」写做完是什么样子，一两句；「# 重点」写现在最该推的一件事；「# 清单」每步一行 `- [ ] `，已经做完的写 `- [x] `；「# 记录」每条一行 `- 日期 内容`，没有就留空。只改这份任务文件，保存前再读一次，写完告诉我，我来确认。"
    )
}

/// `name` is the agent being told: checklist lines ending in ` @<name>` are the ones given to it.
pub fn share_note(path: &str, name: &str) -> String {
    format!(
        "这个任务的终点、重点和清单在 {path}。动手前先读它；做完清单里的一步，就把那一行的 `- [ ]` 改成 `- [x]`；觉得终点或重点要调整，先跟我说再改。清单里行末标了 `@名字` 的条目已经分给那个对话，你在 Shifu 里的名字是 `{name}`：标着别的名字的不要做；没标名字的条目不要因为它空着就自己接，等我交给你，或者你手头正在做的事正好就是它时再做，开始前先在那一行末尾加上 ` @{name}`。文件末尾的「# 记录」是这个任务里几个对话共用的：别的对话需要知道的发现、决定和踩过的坑，在那一节下面加一行 `- 日期 内容`（没有这个标题就加上），只追加，不改别人写的。"
    )
}

pub fn assign_note(path: &str, item: &str, name: &str) -> String {
    format!(
        "这个任务清单里的这一条交给你做：「{item}」。任务文件是 {path}，动手前先读它：终点、重点，还有别的对话写在「# 记录」里的发现。这一行末尾已经标了 `@{name}`（你在 Shifu 里的名字），表示归你；标着别的名字的条目归别的对话，不要做。做完把这一行的 `- [ ]` 改成 `- [x]`，别的对话需要知道的发现在「# 记录」下面加一行 `- 日期 内容`。"
    )
}

pub fn unassign_note(path: &str, item: &str, to: &str) -> String {
    format!(
        "清单里「{item}」这一条改交给 `@{to}` 了，你不用做了。已经动了手的话停在这里，把做到哪、留下了什么，在任务文件 {path} 的「# 记录」下面加一行说明。"
    )
}

/// A checklist line (`- [ ] …`, `* [x] …`) split into whether it is ticked and the byte offset where its text
/// starts. Matches the page's own rule in `app/src/progress.ts`, so the nth item here is the nth row there.
fn item(line: &str) -> Option<(bool, usize)> {
    let rest = line.trim_start();
    let rest = rest.strip_prefix(['-', '*', '+'])?.strip_prefix(' ')?;
    let (done, rest) = match rest.strip_prefix("[ ]") {
        Some(rest) => (false, rest),
        None => (true, rest.strip_prefix("[x]").or_else(|| rest.strip_prefix("[X]"))?),
    };
    (rest.starts_with(char::is_whitespace) && rest.chars().count() > 1).then(|| (done, line.len() - rest.len()))
}

/// Where the ` @对话名` at the end of an item's text starts, if it has one. Markdown marks around it do not count.
fn owner_start(text: &str) -> Option<usize> {
    let body = text.trim_end();
    let at = body.rfind(char::is_whitespace)?;
    let token = body[at..].trim_start().replace("**", "").replace('`', "");
    (token.len() > 1 && token.starts_with('@') && !body[..at].trim().is_empty()).then_some(at)
}

/// An item's text as the page shows it: no Markdown marks, no owner.
fn as_shown(text: &str) -> String {
    let text = &text[..owner_start(text).unwrap_or(text.len())];
    text.replace("**", "").replace('`', "").trim().to_string()
}

/// `text` with its `index`th checklist line given to `name`, and who had that line before. `expect` is the line as
/// the page showed it when the user clicked: the file is also edited by the agents, so a line that moved or
/// changed since is refused.
fn with_owner(text: &str, index: usize, expect: &str, name: &str) -> Res<(String, Option<String>)> {
    let mut lines: Vec<String> = text.split('\n').map(str::to_string).collect();
    let stale = || "清单刚被改过，再点一次".to_string();
    let (at, (done, start)) =
        lines.iter().enumerate().filter_map(|(n, l)| Some((n, item(l)?))).nth(index).ok_or_else(stale)?;
    let body = &lines[at][start..];
    if as_shown(body) != expect {
        return Err(stale());
    }
    if done {
        return Err("这一条已经做完了".into());
    }
    let owner = owner_start(body);
    let before = owner.map(|o| body[o..].trim().replace("**", "").replace('`', "").trim_start_matches('@').to_string());
    let kept = body[..owner.unwrap_or(body.len())].trim_end();
    lines[at] = format!("{}{kept} @{name}", &lines[at][..start]);
    Ok((lines.join("\n"), before))
}

fn can_queue(agent: &Agent) -> bool {
    matches!(agent.kind, Kind::Codex | Kind::Claude) && matches!(agent.status, Status::Running | Status::Idle | Status::NeedsReply)
}

fn queue(cfg: &Config, agent: &Agent, task_id: &str, text: &str) -> Res<()> {
    let id = format!("goal-{task_id}-{}", std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_nanos());
    shifu::inbox::enqueue(&cfg.home, agent, &id, text, None, None).map(|_| ())
}

/// How a message gets to a conversation right now.
enum Way {
    /// Through the queued-input inbox: delivered once the conversation is free.
    Queue,
    /// Straight into this pane. Only for a Codex conversation nobody has typed in yet: Codex starts its session on
    /// the first prompt, so the inbox has no session to follow, but its empty welcome composer is on screen.
    Paste(String),
}

fn way(agent: &Agent, tmux: &Tmux) -> Option<Way> {
    if can_queue(agent) {
        return Some(Way::Queue);
    }
    let fresh = agent.kind == Kind::Codex && agent.status == Status::Starting && agent.session_id.is_none();
    let pane = agent.pane_id.as_deref().filter(|_| fresh)?;
    shifu::bridge::codex_welcome(tmux, pane).then(|| Way::Paste(pane.to_string()))
}

#[derive(Serialize, Clone, Copy, Debug, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum DeliveryState { Queued, Written, Unknown, Unavailable }

#[derive(Serialize, Debug, PartialEq)]
pub struct Notification {
    pub agent: String,
    pub state: DeliveryState,
    pub previous: bool,
}

#[derive(Serialize, Debug, PartialEq)]
pub struct Told { pub notifications: Vec<Notification> }

#[derive(Serialize, Debug, PartialEq)]
pub struct Assigned { pub goal: TaskGoal, pub notifications: Vec<Notification> }

fn deliver(cfg: &Config, agent: &Agent, task_id: &str, text: &str, way: &Way, tmux: &Tmux, ptys: &crate::pty::Ptys) -> Res<DeliveryState> {
    match way {
        Way::Queue => queue(cfg, agent, task_id, text).map(|_| DeliveryState::Queued),
        Way::Paste(pane) => ptys.serialize_input(pane, || {
            let current = shifu::agent::load(&agent.name)?.ok_or("conversation disappeared")?;
            if current.created_at != agent.created_at || current.pane_id != agent.pane_id
                || current.session_id != agent.session_id || current.cwd != agent.cwd || current.kind != agent.kind
                || !shifu::bridge::codex_welcome(tmux, pane)
            {
                return Err("conversation changed or has a draft; message not sent".into());
            }
            let screen = tmux.run(["capture-pane", "-e", "-p", "-t", pane])?;
            if !shifu::inbox::composer_empty(agent.kind, &screen) {
                return Err("conversation has a draft; message not sent".into());
            }
            // Codex may fold a long first prompt into a paste summary. Its visible text is not
            // a receipt of the complete prompt; retain the established welcome-page first send.
            Ok(if shifu::bridge::send(tmux, pane, text).is_ok() { DeliveryState::Written } else { DeliveryState::Unknown })
        }),
    }
}

/// The task's agents as live records with their current status; ones whose record is gone are left out.
fn live(task_agents: &[String], records: &[Agent]) -> Vec<Agent> {
    task_agents.iter().filter_map(|n| records.iter().find(|r| &r.name == n).cloned()).collect()
}

/// `draft`: one agent (an idle one first) writes the first version. Otherwise every agent is told where it is.
/// Returns the actual submission result for each selected conversation.
pub fn tell(cfg: &Config, file: &TaskFile, task_id: &str, draft: bool, records: &[Agent], tmux: &Tmux, ptys: &crate::pty::Ptys) -> Res<Told> {
    let task = file.tasks.iter().find(|t| t.id == task_id).ok_or("task not found")?;
    ensure_dir(&cfg.home)?;
    let target = path(&cfg.home, task_id);
    {
        let _files = GOAL_FILES.lock().unwrap();
        match std::fs::File::create_new(&target) {
            Ok(mut file) => file.write_all(TEMPLATE.as_bytes()).map_err(|e| format!("write {} failed: {e}", target.display()))?,
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => { read_text(&cfg.home, task_id)?; }
            Err(e) => return Err(format!("create {} failed: {e}", target.display())),
        }
    }
    let shown = target.display().to_string();
    let mut agents: Vec<(Agent, Way)> =
        live(&task.agents, records).into_iter().filter_map(|a| way(&a, tmux).map(|w| (a, w))).collect();
    if draft && agents.is_empty() {
        return Err("这个任务里没有能收消息的 Claude 或 Codex 会话".into());
    }
    if draft {
        // A conversation nobody has talked to has nothing to draft from; it writes only when it is the only one.
        agents.sort_by_key(|(a, w)| (matches!(w, Way::Paste(_)), a.status != Status::Idle));
        agents.truncate(1);
    }
    // Persist the deliberate foreground attempt before sending. A later metadata failure must
    // not make the newcomer poll automatically repeat a message already submitted here.
    add_known(&cfg.home, task_id, &task.agents)?;
    let mut notifications = Vec::new();
    for (agent, way) in &agents {
        let text = if draft { draft_note(&shown) } else { share_note(&shown, &agent.name) };
        let state = deliver(cfg, agent, task_id, &text, way, tmux, ptys).unwrap_or(DeliveryState::Unavailable);
        notifications.push(Notification { agent: agent.name.clone(), state, previous: false });
    }
    if !draft {
        for name in &task.agents {
            if !notifications.iter().any(|n| &n.agent == name) {
                notifications.push(Notification { agent: name.clone(), state: DeliveryState::Unavailable, previous: false });
            }
        }
    }
    Ok(Told { notifications })
}

/// Gives one checklist line to one of the task's conversations: the line gets ` @<name>` at its end (replacing an
/// earlier owner) and that conversation is told. A conversation that had the line before is told it no longer
/// does; the rest learn it from the file.
#[allow(clippy::too_many_arguments)]
pub fn assign(cfg: &Config, file: &TaskFile, task_id: &str, index: usize, expect: &str, name: &str, records: &[Agent], tmux: &Tmux, ptys: &crate::pty::Ptys) -> Res<Assigned> {
    let task = file.tasks.iter().find(|t| t.id == task_id).ok_or("task not found")?;
    let members = live(&task.agents, records);
    let agent = members.iter().find(|a| a.name == name).ok_or("这个对话不在这个任务里")?;
    let way = way(agent, tmux).ok_or("这个对话现在收不了消息")?;
    let target = path(&cfg.home, task_id);
    let (text, before) = {
        let _files = GOAL_FILES.lock().unwrap();
        let (text, before) = with_owner(&read_text(&cfg.home, task_id)?, index, expect, name)?;
        write_atomic(&target, &text)?;
        (text, before)
    };
    let shown = target.display().to_string();
    // The ownership edit is already saved. Notification failures must not hide that result.
    let state = deliver(cfg, agent, task_id, &assign_note(&shown, expect, name), &way, tmux, ptys)
        .unwrap_or(DeliveryState::Unavailable);
    let mut notifications = vec![Notification { agent: name.to_string(), state, previous: false }];
    if let Some(old_name) = before.filter(|old| old != name) {
        let state = members.iter().find(|a| a.name == old_name && can_queue(a))
            .map(|old| queue(cfg, old, task_id, &unassign_note(&shown, expect, name))
                .map(|_| DeliveryState::Queued).unwrap_or(DeliveryState::Unavailable))
            .unwrap_or(DeliveryState::Unavailable);
        notifications.push(Notification { agent: old_name, state, previous: true });
    }
    Ok(Assigned { goal: TaskGoal { path: shown, text, read_error: None }, notifications })
}

/// Called on every agent poll: tells agents that joined a task after its file was written. Agents still starting
/// are tried again on a later poll; kinds the inbox cannot reach are marked so they are not retried forever.
pub fn tell_newcomers(cfg: &Config, records: &[Agent]) -> Res<()> {
    let Some(file) = crate::tasks::read(cfg)? else { return Ok(()) };
    let known = read_known(&cfg.home)?;
    let mut read_error = None;
    let by_name: HashMap<&str, &Agent> = records.iter().map(|a| (a.name.as_str(), a)).collect();
    for task in &file.tasks {
        let Some(seen) = known.get(&task.id) else { continue };
        for name in task.agents.iter().filter(|name| !seen.contains(name)) {
            // Serialize only this decision/enqueue/update, not the full background poll.
            let _known_file = KNOWN_FILE.lock().unwrap();
            let known = read_known(&cfg.home)?;
            let Some(seen) = known.get(&task.id) else { continue };
            if seen.contains(name) { continue; }
            let Some(agent) = by_name.get(name.as_str()) else { continue };
            if !matches!(agent.kind, Kind::Codex | Kind::Claude) {
                add_known_locked(&cfg.home, &task.id, std::slice::from_ref(name))?;
                continue;
            }
            if !can_queue(agent) {
                continue;
            }
            let text = match read_text(&cfg.home, &task.id) {
                Ok(text) => text,
                Err(error) => {
                    read_error.get_or_insert(error);
                    continue;
                }
            };
            if written(&text) {
                queue(cfg, agent, &task.id, &share_note(&path(&cfg.home, &task.id).display().to_string(), name))?;
            }
            add_known_locked(&cfg.home, &task.id, std::slice::from_ref(name))?;
        }
    }
    read_error.map_or(Ok(()), Err)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_text_beyond_the_headings_counts_as_written() {
        assert!(!written(TEMPLATE));
        assert!(!written(""));
        assert!(written("# 终点\n拿到 offer\n"));
        assert!(written("# 清单\n- [ ] 投十家\n"));
    }

    #[test]
    fn notes_name_the_file() {
        assert!(draft_note("/h/goals/t-1.md").contains("/h/goals/t-1.md"));
        assert!(share_note("/h/goals/t-1.md", "codex-a").contains("/h/goals/t-1.md"));
        // Both tell the conversation about the shared notes, and the template has the heading they point at.
        assert!(draft_note("/h/goals/t-1.md").contains("# 记录") && share_note("/h/goals/t-1.md", "codex-a").contains("# 记录"));
        // A conversation is told its own name, so it can tell its lines from the others'.
        assert!(share_note("/h/goals/t-1.md", "codex-a").contains("@codex-a"));
        // Being told about the file is not being handed its open lines (a real Claude took one on this note alone).
        assert!(share_note("/h/goals/t-1.md", "codex-a").contains("不要因为它空着就自己接"));
        assert!(assign_note("/h/goals/t-1.md", "投十家", "codex-a").contains("「投十家」"));
        assert!(TEMPLATE.contains("# 记录") && !written(TEMPLATE));
    }

    const LIST: &str = "# 终点\n拿到 offer\n\n# 清单\n- [x] 改简历\n- [ ] **投十家** @codex-a\n* [ ] 约 `内推`\n\n# 记录\n- 11-19 A 公司不招了\n";

    #[test]
    fn giving_a_line_marks_only_that_line() {
        let (out, before) = with_owner(LIST, 2, "约 内推", "claude-b").unwrap();
        assert_eq!(out, LIST.replace("* [ ] 约 `内推`", "* [ ] 约 `内推` @claude-b"));
        assert_eq!(before, None);
    }

    #[test]
    fn giving_a_line_again_replaces_its_owner() {
        // Who had it comes back, so that conversation can be told to stop.
        let (out, before) = with_owner(LIST, 1, "投十家", "claude-b").unwrap();
        assert_eq!(out, LIST.replace("**投十家** @codex-a", "**投十家** @claude-b"));
        assert_eq!(before.as_deref(), Some("codex-a"));
        assert!(unassign_note("/h/goals/t-1.md", "投十家", "claude-b").contains("@claude-b"));
    }

    #[test]
    fn a_line_that_changed_or_is_done_is_refused() {
        // The agents edit the same file: the row the user clicked may have moved or been ticked meanwhile.
        assert!(with_owner(LIST, 1, "约 内推", "claude-b").is_err());
        assert!(with_owner(LIST, 7, "投十家", "claude-b").is_err());
        assert_eq!(with_owner(LIST, 0, "改简历", "claude-b").unwrap_err(), "这一条已经做完了");
    }

    #[test]
    fn only_a_trailing_at_word_after_some_text_is_an_owner() {
        assert_eq!(as_shown("投十家 @codex-a"), "投十家");
        assert_eq!(as_shown("@codex-a"), "@codex-a");
        assert_eq!(as_shown("发给 a@b.com 的信"), "发给 a@b.com 的信");
    }

    #[test]
    fn a_save_begun_before_a_conversation_changed_the_file_is_refused() {
        let home = std::env::temp_dir().join(format!("shifu-taskgoals-save-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&home);
        let cfg = Config { home: home.clone(), socket: "s".into(), shifu_bin: "shifu".into(), user_statusline: None };
        let task = crate::tasks::Task { id: "t-1".into(), project: "/w".into(), name: "求职".into(), auto_name: false, created_at: 0, agents: vec![] };
        let file = TaskFile { tasks: vec![task], ..TaskFile::default() };
        let opened = "# 清单\n- [ ] 投十家\n";
        // The first save starts from no file at all.
        assert!(save(&cfg, &file, "t-1", "", opened).unwrap().saved);

        // A conversation ticks the line while the user is still typing on top of what they opened.
        let ticked = "# 清单\n- [x] 投十家\n";
        std::fs::write(path(&home, "t-1"), ticked).unwrap();
        let mine = "# 终点\n拿到 offer\n\n# 清单\n- [ ] 投十家\n";
        let refused = save(&cfg, &file, "t-1", opened, mine).unwrap();
        assert_eq!((refused.saved, refused.goal.text.as_str()), (false, ticked));
        assert_eq!(read_text(&home, "t-1").unwrap(), ticked);

        // Saved again on top of the file as it is now, it goes through.
        let merged = "# 终点\n拿到 offer\n\n# 清单\n- [x] 投十家\n";
        assert!(save(&cfg, &file, "t-1", ticked, merged).unwrap().saved);
        assert_eq!(read_text(&home, "t-1").unwrap(), merged);
        std::fs::remove_dir_all(&home).unwrap();
    }

    #[test]
    fn known_agents_accumulate_without_duplicates() {
        let home = std::env::temp_dir().join(format!("shifu-taskgoals-{}", std::process::id()));
        std::fs::create_dir_all(dir(&home)).unwrap();
        add_known(&home, "t-1", &["a".into(), "b".into()]).unwrap();
        add_known(&home, "t-1", &["b".into(), "c".into()]).unwrap();
        assert_eq!(read_known(&home).unwrap()["t-1"], ["a", "b", "c"]);
        std::fs::remove_dir_all(&home).unwrap();
    }

    #[test]
    fn a_goal_that_cannot_be_decoded_survives_a_save_from_an_empty_editor() {
        let (cfg, file) = fixture("unreadable");
        let target = path(&cfg.home, "t-1");
        let original = [0xff, 0xfe, b'A'];
        std::fs::write(&target, original).unwrap();
        let shown = &read_all(&cfg.home, &file)["t-1"];
        assert!(shown.read_error.as_deref().unwrap().contains("t-1.md"));
        assert!(save(&cfg, &file, "t-1", "", "replacement").is_err());
        assert_eq!(std::fs::read(&target).unwrap(), original);
        assert!(tell(&cfg, &file, "t-1", true, &[], &Tmux::from_env(), &crate::pty::Ptys::default()).is_err());
        assert!(!known_path(&cfg.home).exists());
        std::fs::remove_dir_all(cfg.home).unwrap();
    }

    #[test]
    fn malformed_known_members_survive_an_attempt_to_add_a_new_member() {
        let (cfg, _) = fixture("malformed-known");
        let original = "{truncated";
        std::fs::write(known_path(&cfg.home), original).unwrap();
        assert!(add_known(&cfg.home, "t-1", &["agent-b".into()]).is_err());
        assert_eq!(std::fs::read_to_string(known_path(&cfg.home)).unwrap(), original);
        std::fs::remove_dir_all(cfg.home).unwrap();
    }
    fn fixture(label: &str) -> (Config, TaskFile) {
        let home = std::env::temp_dir().join(format!("shifu-goals-{label}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&home);
        ensure_dir(&home).unwrap();
        let cfg = Config { home, socket: "goals-test".into(), shifu_bin: "shifu".into(), user_statusline: None };
        let task = crate::tasks::Task { id: "t-1".into(), project: "/w".into(), name: "task".into(), auto_name: false, created_at: 0, agents: vec!["codex-test".into()] };
        (cfg, TaskFile { tasks: vec![task], ..TaskFile::default() })
    }

    fn test_agent() -> Agent {
        serde_json::from_value(serde_json::json!({
            "name": "codex-test", "kind": "codex", "command": "codex", "cwd": "/w",
            "created_at": 1, "status": "starting", "status_since": 1,
            "window_id": "@1", "pane_id": "%1", "session_id": null,
            "last_message": null, "waiting_for": null, "model": null, "effort": null, "label": null
        })).unwrap()
    }

    #[test]
    fn batch_results_preserve_successes_and_assignment_keeps_saved_file_on_notify_failure() {
        let (cfg, mut file) = fixture("outcomes");
        let mut records = Vec::new();
        for name in ["first", "blocked", "last"] {
            let mut a = test_agent();
            a.name = name.into(); a.status = Status::Idle; a.session_id = Some(format!("native-{name}"));
            records.push(a);
        }
        file.tasks[0].agents = records.iter().map(|a| a.name.clone()).collect();
        std::fs::create_dir_all(cfg.home.join("inbox")).unwrap();
        std::fs::write(cfg.home.join("inbox/blocked"), "not a directory").unwrap();
        let ptys = crate::pty::Ptys::default();
        let tmux = Tmux::from_env();
        let told = tell(&cfg, &file, "t-1", false, &records, &tmux, &ptys).unwrap();
        assert_eq!(told.notifications.iter().map(|n| n.state).collect::<Vec<_>>(),
            [DeliveryState::Queued, DeliveryState::Unavailable, DeliveryState::Queued]);
        assert_eq!(read_known(&cfg.home).unwrap()["t-1"], file.tasks[0].agents);
        for name in ["first", "last"] {
            assert_eq!(std::fs::read_dir(cfg.home.join("inbox").join(name)).unwrap()
                .filter_map(Result::ok).filter(|e| e.path().extension().is_some_and(|e| e == "json")).count(), 1);
        }
        std::fs::write(path(&cfg.home, "t-1"), "# 清单\n- [ ] work @first\n").unwrap();
        let assigned = assign(&cfg, &file, "t-1", 0, "work", "blocked", &records, &tmux, &ptys).unwrap();
        assert!(assigned.goal.text.contains("work @blocked"));
        assert_eq!(read_text(&cfg.home, "t-1").unwrap(), assigned.goal.text);
        assert_eq!(assigned.notifications[0].state, DeliveryState::Unavailable);
        assert!(assigned.notifications[1].previous);
        assert_eq!(assigned.notifications[1].state, DeliveryState::Queued);
        let drafted = tell(&cfg, &file, "t-1", true, &records, &tmux, &ptys).unwrap();
        assert_eq!(drafted.notifications.len(), 1);
        std::fs::remove_dir_all(cfg.home).unwrap();
    }

    #[test]
    fn welcome_first_prompt_submits_even_when_codex_folds_the_paste() {
        const CHILD: &str = "SHIFU_GOAL_WELCOME_TEST";
        if std::env::var_os(CHILD).is_none() {
            // Isolate SHIFU_HOME/TMUX from the rest of this multithreaded test binary.
            let (cfg, _) = fixture("welcome");
            let fake = cfg.home.join("tmux");
            std::fs::write(&fake, r##"#!/usr/bin/env python3
import os, pathlib, sys
p=pathlib.Path(os.environ['SHIFU_HOME']); a=sys.argv[5:]
with (p/'commands').open('a') as f: f.write(a[0]+'\n')
if a[0]=='capture-pane': print((p/'screen').read_text())
elif a[0]=='set-buffer': (p/'buffer').write_text(a[a.index('--')+1])
elif a[0]=='paste-buffer': (p/'screen').write_text('› [Pasted Content 600 lines]\n? for shortcuts')
elif a[0]=='send-keys':
 if (p/'fail-send').exists(): sys.exit(1)
 (p/'submitted').write_text((p/'buffer').read_text())
"##).unwrap();
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&fake, std::fs::Permissions::from_mode(0o755)).unwrap();
            let result = std::process::Command::new(std::env::current_exe().unwrap())
                .args(["--exact", "taskgoals::tests::welcome_first_prompt_submits_even_when_codex_folds_the_paste", "--nocapture"])
                .env(CHILD, "1").env("SHIFU_HOME", &cfg.home).env("SHIFU_TMUX", &fake)
                .env("SHIFU_SOCKET", "goals-fixture").output().unwrap();
            assert!(result.status.success(), "{}\n{}", String::from_utf8_lossy(&result.stdout), String::from_utf8_lossy(&result.stderr));
            std::fs::remove_dir_all(cfg.home).unwrap();
            return;
        }
        let cfg = Config { home: shifu::agent::shifu_home(), socket: "goals-fixture".into(), shifu_bin: "shifu".into(), user_statusline: None };
        let agent = test_agent();
        shifu::agent::save(&agent).unwrap();
        let welcome = ">_ OpenAI Codex (v1)\nTo get started, describe a task\n› Ask Codex to do anything\n? for shortcuts";
        std::fs::write(cfg.home.join("screen"), welcome).unwrap();
        let ptys = crate::pty::Ptys::default();
        let tmux = Tmux::from_env();
        let text = share_note("/very/long/task/path.md", &agent.name).repeat(20);
        assert_eq!(deliver(&cfg, &agent, "t-1", &text, &Way::Paste("%1".into()), &tmux, &ptys).unwrap(), DeliveryState::Written);
        assert_eq!(std::fs::read_to_string(cfg.home.join("submitted")).unwrap(), text);
        std::fs::remove_file(cfg.home.join("submitted")).unwrap();
        std::fs::write(cfg.home.join("screen"), format!("{welcome}\n› my draft")).unwrap();
        assert!(deliver(&cfg, &agent, "t-1", "new", &Way::Paste("%1".into()), &tmux, &ptys).is_err());
        std::fs::write(cfg.home.join("screen"), welcome).unwrap();
        let mut changed = agent.clone(); changed.created_at += 1;
        shifu::agent::save(&changed).unwrap();
        assert!(deliver(&cfg, &agent, "t-1", "new", &Way::Paste("%1".into()), &tmux, &ptys).is_err());
        assert!(!cfg.home.join("submitted").exists());
        shifu::agent::save(&agent).unwrap();
        std::fs::write(cfg.home.join("fail-send"), "").unwrap();
        assert_eq!(deliver(&cfg, &agent, "t-1", "uncertain", &Way::Paste("%1".into()), &tmux, &ptys).unwrap(), DeliveryState::Unknown);

    }

    #[test]
    fn concurrent_goal_changes_keep_rows_and_known_members_without_blocking_input() {
        const CHILD: &str = "SHIFU_GOAL_CONCURRENCY_TEST";
        if std::env::var_os(CHILD).is_none() {
            let (cfg, _) = fixture("concurrency-env");
            let result = std::process::Command::new(std::env::current_exe().unwrap())
                .args(["--exact", "taskgoals::tests::concurrent_goal_changes_keep_rows_and_known_members_without_blocking_input", "--nocapture"])
                .env(CHILD, "1").env("SHIFU_HOME", &cfg.home).env("CODEX_HOME", cfg.home.join("codex"))
                .output().unwrap();
            assert!(result.status.success(), "{}\n{}", String::from_utf8_lossy(&result.stdout), String::from_utf8_lossy(&result.stderr));
            std::fs::remove_dir_all(cfg.home).unwrap();
            return;
        }
        use std::sync::{Barrier, mpsc};
        use std::time::Duration;
        let (cfg, file) = fixture("parallel");
        let ptys = crate::pty::Ptys::default();
        let (done_tx, done_rx) = mpsc::channel();
        std::thread::scope(|threads| {
            let held = GOAL_FILES.lock().unwrap();
            threads.spawn(|| { done_tx.send(save(&cfg, &file, "t-1", "", "# 清单\n- [ ] one\n- [ ] two\n")).unwrap(); });
            // A file-lock waiter must not hold the pane gate (or any PTY registry lock).
            ptys.serialize_input("%other", || ());
            assert!(done_rx.recv_timeout(Duration::from_millis(30)).is_err());
            drop(held);
            assert!(done_rx.recv_timeout(Duration::from_secs(2)).unwrap().unwrap().saved);
        });
        let mut agent = test_agent(); agent.status = Status::Idle; agent.session_id = Some("native-test".into());
        let records = [agent];
        let barrier = Barrier::new(3);
        std::thread::scope(|threads| {
            for (index, item) in [(0, "one"), (1, "two")] {
                let barrier = &barrier; let cfg = &cfg; let file = &file; let records = &records; let ptys = &ptys;
                threads.spawn(move || {
                    barrier.wait();
                    assign(cfg, file, "t-1", index, item, "codex-test", records, &Tmux::from_env(), ptys).unwrap();
                });
            }
            barrier.wait();
        });
        let current = read_text(&cfg.home, "t-1").unwrap();
        assert!(current.contains("one @codex-test") && current.contains("two @codex-test"));
        let barrier = Barrier::new(9);
        std::thread::scope(|threads| {
            let mut saves = Vec::new();
            for n in 0..8 {
                let barrier = &barrier; let cfg = &cfg; let file = &file; let expected = &current;
                saves.push(threads.spawn(move || {
                    barrier.wait();
                    save(cfg, file, "t-1", expected, &format!("{expected}note {n}\n")).unwrap().saved
                }));
            }
            barrier.wait();
            assert_eq!(saves.into_iter().map(|s| s.join().unwrap()).filter(|saved| *saved).count(), 1);
        });
        // Background newcomer updates and foreground updates merge the same known.json.
        std::fs::write(cfg.home.join("tasks.json"), serde_json::to_vec(&file).unwrap()).unwrap();
        write_atomic(&known_path(&cfg.home), r#"{"t-1":[]}"#).unwrap();
        let barrier = Barrier::new(9);
        std::thread::scope(|threads| {
            for n in 0..8 {
                let barrier = &barrier; let cfg = &cfg; let records = &records;
                threads.spawn(move || {
                    barrier.wait();
                    if n % 2 == 0 { tell_newcomers(cfg, records).unwrap(); }
                    add_known(&cfg.home, &format!("task-{n}"), &[format!("agent-{n}")]).unwrap();
                });
            }
            barrier.wait();
        });
        let known = read_known(&cfg.home).unwrap();
        assert_eq!(known.len(), 9);
        assert_eq!(known["t-1"], ["codex-test"]);
        assert_eq!(std::fs::read_dir(cfg.home.join("inbox/codex-test")).unwrap()
            .filter_map(Result::ok).filter(|entry| entry.path().extension().is_some_and(|ext| ext == "json")).count(), 3);
        for n in 0..8 { assert_eq!(known[&format!("task-{n}")], [format!("agent-{n}")]); }
        std::fs::remove_dir_all(cfg.home).unwrap();
    }

}
