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
pub fn save(cfg: &Config, file: &TaskFile, task_id: &str, text: &str) -> Res<TaskGoal> {
    let task = file.tasks.iter().find(|t| t.id == task_id).ok_or("task not found")?;
    ensure_dir(&cfg.home)?;
    if !written(&read_text(&cfg.home, task_id)) {
        add_known(&cfg.home, task_id, &task.agents)?;
    }
    let target = path(&cfg.home, task_id);
    write_atomic(&target, text)?;
    Ok(TaskGoal { path: target.display().to_string(), text: text.to_string() })
}

pub fn draft_note(path: &str) -> String {
    format!(
        "请根据这个任务到目前为止的对话，起草它的终点、重点和清单，写进 {path}。文件里已有四个标题：「# 终点」写做完是什么样子，一两句；「# 重点」写现在最该推的一件事；「# 清单」写通往终点的几步，每步一行 `- [ ] `，已经做完的写 `- [x] `；「# 记录」写这个任务里别的对话需要知道的发现和决定，每条一行 `- 日期 内容`，没有就留空。只改这个文件，写完告诉我，我来确认。"
    )
}

pub fn share_note(path: &str) -> String {
    format!(
        "这个任务的终点、重点和清单在 {path}。动手前先读它；做完清单里的一步，就把那一行的 `- [ ]` 改成 `- [x]`；觉得终点或重点要调整，先跟我说再改。文件末尾的「# 记录」是这个任务里几个对话共用的：别的对话需要知道的发现、决定和踩过的坑，在那一节下面加一行 `- 日期 内容`（没有这个标题就加上），只追加，不改别人写的。"
    )
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
    let text = if draft { draft_note(&shown) } else { share_note(&shown) };
    for agent in &agents {
        queue(cfg, agent, task_id, &text)?;
    }
    // From here on, agents that join this task later are told about the file on their own.
    add_known(&cfg.home, task_id, &task.agents)?;
    Ok(agents.len())
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
                queue(cfg, agent, &task.id, &share_note(&path(&cfg.home, &task.id).display().to_string()))?;
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
        assert!(share_note("/h/goals/t-1.md").contains("/h/goals/t-1.md"));
        // Both tell the conversation about the shared notes, and the template has the heading they point at.
        assert!(draft_note("/h/goals/t-1.md").contains("# 记录") && share_note("/h/goals/t-1.md").contains("# 记录"));
        assert!(TEMPLATE.contains("# 记录") && !written(TEMPLATE));
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
