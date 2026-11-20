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
use shifu::status::Status;
use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};

type Res<T> = Result<T, String>;

pub const TEMPLATE: &str = "# 终点\n\n\n# 重点\n\n\n# 清单\n\n\n# 记录\n\n";

#[derive(Serialize, Clone, Debug, PartialEq)]
pub struct TaskGoal {
    pub path: String,
    /// Empty when the task has no file yet.
    pub text: String,
}

fn dir(home: &Path) -> PathBuf {
    home.join("goals")
}

pub fn path(home: &Path, task_id: &str) -> PathBuf {
    dir(home).join(format!("{task_id}.md"))
}

fn read_text(home: &Path, task_id: &str) -> String {
    std::fs::read_to_string(path(home, task_id)).unwrap_or_default()
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
            let goal = TaskGoal { path: path(home, &t.id).display().to_string(), text: read_text(home, &t.id) };
            (t.id.clone(), goal)
        })
        .collect()
}

fn known_path(home: &Path) -> PathBuf {
    dir(home).join("known.json")
}

fn read_known(home: &Path) -> BTreeMap<String, Vec<String>> {
    std::fs::read_to_string(known_path(home)).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default()
}

fn add_known(home: &Path, task_id: &str, names: &[String]) -> Res<()> {
    let mut known = read_known(home);
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
    let task = file.tasks.iter().find(|t| t.id == task_id).ok_or("task not found")?;
    ensure_dir(&cfg.home)?;
    let target = path(&cfg.home, task_id);
    let shown = target.display().to_string();
    let current = read_text(&cfg.home, task_id);
    if current != expected {
        return Ok(Saved { saved: false, goal: TaskGoal { path: shown, text: current } });
    }
    if !written(&current) {
        add_known(&cfg.home, task_id, &task.agents)?;
    }
    write_atomic(&target, text)?;
    Ok(Saved { saved: true, goal: TaskGoal { path: shown, text: text.to_string() } })
}

pub fn draft_note(path: &str) -> String {
    format!(
        "请根据这个任务到目前为止的对话，起草它的终点、重点和清单，写进 {path}。文件里已有四个标题：「# 终点」写做完是什么样子，一两句；「# 重点」写现在最该推的一件事；「# 清单」写通往终点的几步，每步一行 `- [ ] `，已经做完的写 `- [x] `；「# 记录」写这个任务里别的对话需要知道的发现和决定，每条一行 `- 日期 内容`，没有就留空。只改这个文件，写完告诉我，我来确认。"
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

/// `text` with its `index`th checklist line given to `name`. `expect` is that line as the page showed it when the
/// user clicked: the file is also edited by the agents, so a line that moved or changed since is refused.
fn with_owner(text: &str, index: usize, expect: &str, name: &str) -> Res<String> {
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
    let kept = body[..owner_start(body).unwrap_or(body.len())].trim_end();
    lines[at] = format!("{}{kept} @{name}", &lines[at][..start]);
    Ok(lines.join("\n"))
}

fn can_queue(agent: &Agent) -> bool {
    matches!(agent.kind, Kind::Codex | Kind::Claude) && matches!(agent.status, Status::Running | Status::Idle | Status::NeedsReply)
}

fn queue(cfg: &Config, agent: &Agent, task_id: &str, text: &str) -> Res<()> {
    let id = format!("goal-{task_id}-{}", std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_nanos());
    shifu::inbox::enqueue(&cfg.home, agent, &id, text, None, None).map(|_| ())
}

/// The task's agents as live records with their current status; ones whose record is gone are left out.
fn live(task_agents: &[String], records: &[Agent]) -> Vec<Agent> {
    task_agents.iter().filter_map(|n| records.iter().find(|r| &r.name == n).cloned()).collect()
}

/// `draft`: one agent (an idle one first) writes the first version. Otherwise every agent is told where it is.
/// Returns how many conversations got the message.
pub fn tell(cfg: &Config, file: &TaskFile, task_id: &str, draft: bool, records: &[Agent]) -> Res<usize> {
    let task = file.tasks.iter().find(|t| t.id == task_id).ok_or("task not found")?;
    ensure_dir(&cfg.home)?;
    let target = path(&cfg.home, task_id);
    if !target.exists() {
        write_atomic(&target, TEMPLATE)?;
    }
    let shown = target.display().to_string();
    let mut agents: Vec<Agent> = live(&task.agents, records).into_iter().filter(can_queue).collect();
    if agents.is_empty() {
        return Err("这个任务里没有能收消息的 Claude 或 Codex 会话".into());
    }
    if draft {
        agents.sort_by_key(|a| a.status != Status::Idle);
        agents.truncate(1);
    }
    for agent in &agents {
        let text = if draft { draft_note(&shown) } else { share_note(&shown, &agent.name) };
        queue(cfg, agent, task_id, &text)?;
    }
    // From here on, agents that join this task later are told about the file on their own.
    add_known(&cfg.home, task_id, &task.agents)?;
    Ok(agents.len())
}

/// Gives one checklist line to one of the task's conversations: the line gets ` @<name>` at its end (replacing an
/// earlier owner) and that conversation is told. The others learn it from the file.
pub fn assign(cfg: &Config, file: &TaskFile, task_id: &str, index: usize, expect: &str, name: &str, records: &[Agent]) -> Res<TaskGoal> {
    let task = file.tasks.iter().find(|t| t.id == task_id).ok_or("task not found")?;
    let agent = live(&task.agents, records).into_iter().find(|a| a.name == name).ok_or("这个对话不在这个任务里")?;
    if !can_queue(&agent) {
        return Err("这个对话现在收不了消息".into());
    }
    let target = path(&cfg.home, task_id);
    let text = with_owner(&read_text(&cfg.home, task_id), index, expect, name)?;
    write_atomic(&target, &text)?;
    let shown = target.display().to_string();
    queue(cfg, &agent, task_id, &assign_note(&shown, expect, name))?;
    Ok(TaskGoal { path: shown, text })
}

/// Called on every agent poll: tells agents that joined a task after its file was written. Agents still starting
/// are tried again on a later poll; kinds the inbox cannot reach are marked so they are not retried forever.
pub fn tell_newcomers(cfg: &Config, records: &[Agent]) -> Res<()> {
    let Some(file) = crate::tasks::read(cfg)? else { return Ok(()) };
    let known = read_known(&cfg.home);
    let by_name: HashMap<&str, &Agent> = records.iter().map(|a| (a.name.as_str(), a)).collect();
    for task in &file.tasks {
        let Some(seen) = known.get(&task.id) else { continue };
        for name in task.agents.iter().filter(|n| !seen.contains(n)) {
            let Some(agent) = by_name.get(name.as_str()) else { continue };
            if !matches!(agent.kind, Kind::Codex | Kind::Claude) {
                add_known(&cfg.home, &task.id, std::slice::from_ref(name))?;
                continue;
            }
            if !can_queue(agent) {
                continue;
            }
            let text = read_text(&cfg.home, &task.id);
            if written(&text) {
                queue(cfg, agent, &task.id, &share_note(&path(&cfg.home, &task.id).display().to_string(), name))?;
            }
            add_known(&cfg.home, &task.id, std::slice::from_ref(name))?;
        }
    }
    Ok(())
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
        let out = with_owner(LIST, 2, "约 内推", "claude-b").unwrap();
        assert_eq!(out, LIST.replace("* [ ] 约 `内推`", "* [ ] 约 `内推` @claude-b"));
    }

    #[test]
    fn giving_a_line_again_replaces_its_owner() {
        let out = with_owner(LIST, 1, "投十家", "claude-b").unwrap();
        assert_eq!(out, LIST.replace("**投十家** @codex-a", "**投十家** @claude-b"));
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
        assert_eq!(read_text(&home, "t-1"), ticked);

        // Saved again on top of the file as it is now, it goes through.
        let merged = "# 终点\n拿到 offer\n\n# 清单\n- [x] 投十家\n";
        assert!(save(&cfg, &file, "t-1", ticked, merged).unwrap().saved);
        assert_eq!(read_text(&home, "t-1"), merged);
        std::fs::remove_dir_all(&home).unwrap();
    }

    #[test]
    fn known_agents_accumulate_without_duplicates() {
        let home = std::env::temp_dir().join(format!("shifu-taskgoals-{}", std::process::id()));
        std::fs::create_dir_all(dir(&home)).unwrap();
        add_known(&home, "t-1", &["a".into(), "b".into()]).unwrap();
        add_known(&home, "t-1", &["b".into(), "c".into()]).unwrap();
        assert_eq!(read_known(&home)["t-1"], ["a", "b", "c"]);
        std::fs::remove_dir_all(&home).unwrap();
    }
}
