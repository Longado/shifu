//! Projects and tasks (APP-SPEC-B1). A task is a named set of agents started in one project folder; a project is
//! just a folder that has tasks. Stored in `<home>/tasks.json`, written only by the app (hooks never touch it).

use crate::config::{Config, write_atomic};
use crate::{layout, session};
use serde::{Deserialize, Serialize};
use shifu::agent::{self, Res};
use std::path::Path;
use std::sync::Mutex;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Task {
    pub id: String,
    /// Absolute project folder; every agent of the task starts there.
    pub project: String,
    pub name: String,
    /// The name was made by the app (empty name at creation, or a folder name from the migration), so the
    /// frontend shows the first agent's topic instead. Renaming it by hand turns this off.
    /// Old files have no such field; serde's default for a bool would be false, so it says `true` itself.
    #[serde(default = "yes")]
    pub auto_name: bool,
    pub created_at: u64,
    #[serde(default)]
    pub agents: Vec<String>,
}

fn yes() -> bool {
    true
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct TaskFile {
    #[serde(default)]
    pub current: Option<String>,
    #[serde(default)]
    pub tasks: Vec<Task>,
}

/// Model and effort picked for a new agent; `None` leaves each to the CLI's default.
#[derive(Clone, Copy, Default)]
pub struct Pick<'a> {
    pub model: Option<&'a str>,
    pub effort: Option<&'a str>,
}

/// What the frontend gets after a change, plus the name of the agent the change started (if any).
#[derive(Serialize)]
pub struct Changed {
    pub file: TaskFile,
    pub agent: Option<String>,
}

/// An agent as far as grouping is concerned.
pub struct AgentRef {
    pub name: String,
    pub cwd: String,
    pub created_at: u64,
}

fn dir_name(path: &str) -> String {
    Path::new(path).file_name().and_then(|n| n.to_str()).unwrap_or(path).to_string()
}

fn newest<'a>(tasks: impl Iterator<Item = &'a Task>) -> Option<&'a Task> {
    tasks.max_by(|a, b| (a.created_at, &a.id).cmp(&(b.created_at, &b.id)))
}

/// `t-<ms>`, bumped until it is not taken.
fn unique_id(tasks: &[Task], mut now_ms: u64) -> String {
    loop {
        let id = format!("t-{now_ms}");
        if !tasks.iter().any(|t| t.id == id) {
            return id;
        }
        now_ms += 1;
    }
}

/// Puts every agent that is in no task into one: the project's newest task, or a new task named after the folder.
/// With an empty `file` this is the A1 migration (one task per folder). Also repairs a `current` that points nowhere.
pub fn adopt_orphans(mut file: TaskFile, agents: &[AgentRef], now_ms: u64) -> TaskFile {
    let mut orphans: Vec<&AgentRef> =
        agents.iter().filter(|a| !file.tasks.iter().any(|t| t.agents.contains(&a.name))).collect();
    // Oldest first, so a folder's task is born at its oldest agent and later ones join it.
    orphans.sort_by(|a, b| (a.created_at, &a.name).cmp(&(b.created_at, &b.name)));
    for a in orphans {
        let home = newest(file.tasks.iter().filter(|t| t.project == a.cwd)).map(|t| t.id.clone());
        match home.and_then(|id| file.tasks.iter_mut().find(|t| t.id == id)) {
            Some(task) => task.agents.push(a.name.clone()),
            None => {
                let id = unique_id(&file.tasks, now_ms);
                file.tasks.push(Task {
                    id,
                    project: a.cwd.clone(),
                    name: dir_name(&a.cwd),
                    auto_name: true,
                    created_at: a.created_at,
                    agents: vec![a.name.clone()],
                });
            }
        }
    }
    if file.current.as_ref().is_none_or(|c| !file.tasks.iter().any(|t| &t.id == c)) {
        file.current = newest(file.tasks.iter()).map(|t| t.id.clone());
    }
    file
}

/// "任务 N", N = the project's task count plus one.
pub fn default_name(file: &TaskFile, project: &str) -> String {
    format!("任务 {}", file.tasks.iter().filter(|t| t.project == project).count() + 1)
}

/// Drops the task. When it was the current one, the newest task of the same project takes over, else the newest
/// task anywhere, else nothing.
pub fn remove_task(mut file: TaskFile, id: &str) -> TaskFile {
    let Some(gone) = file.tasks.iter().find(|t| t.id == id).cloned() else { return file };
    file.tasks.retain(|t| t.id != id);
    if file.current.as_deref() == Some(id) {
        let next = newest(file.tasks.iter().filter(|t| t.project == gone.project)).or_else(|| newest(file.tasks.iter()));
        file.current = next.map(|t| t.id.clone());
    }
    file
}

pub fn forget_agent(mut file: TaskFile, name: &str) -> TaskFile {
    for t in &mut file.tasks {
        t.agents.retain(|a| a != name);
    }
    file
}

/// Moves an agent (by its session name) into `to_id`: it belongs to exactly one task afterwards, and an agent
/// that belonged to no task is adopted too. `current` stays put — which task's layout is on screen is a view,
/// not ownership (APP-SPEC-B1). Dragging onto its own task is a no-op.
pub fn move_agent_file(mut file: TaskFile, name: &str, to_id: &str) -> TaskFile {
    if name.is_empty() || !file.tasks.iter().any(|t| t.id == to_id) {
        return file;
    }
    let owner = file.tasks.iter().find(|t| t.agents.iter().any(|a| a == name)).map(|t| t.id.clone());
    if owner.as_deref() == Some(to_id) {
        return file;
    }
    for t in &mut file.tasks {
        t.agents.retain(|a| a != name);
    }
    file.tasks.iter_mut().find(|t| t.id == to_id).expect("target was checked").agents.push(name.to_string());
    file
}

fn file_path(cfg: &Config) -> std::path::PathBuf {
    cfg.home.join("tasks.json")
}

pub fn read(cfg: &Config) -> Res<Option<TaskFile>> {
    let path = file_path(cfg);
    match std::fs::read_to_string(&path) {
        Ok(text) => serde_json::from_str(&text).map(Some).map_err(|e| format!("{} is not valid: {e}", path.display())),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(format!("read {} failed: {e}", path.display())),
    }
}

pub fn write(cfg: &Config, file: &TaskFile) -> Res<()> {
    let text = serde_json::to_string_pretty(file).map_err(|e| format!("encode tasks failed: {e}"))?;
    write_atomic(&file_path(cfg), &text)
}

fn now_ms() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0)
}

fn canonical_dir(path: &str) -> Res<String> {
    let dir = Path::new(path).canonicalize().map_err(|e| format!("directory {path} unusable: {e}"))?;
    if !dir.is_dir() {
        return Err(format!("{} is not a directory", dir.display()));
    }
    Ok(dir.to_string_lossy().to_string())
}

/// Every change reads, edits and writes `tasks.json` while holding the lock; commands may arrive in parallel.
#[derive(Default)]
pub struct Tasks {
    lock: Mutex<()>,
}

impl Tasks {
    fn guard(&self) -> std::sync::MutexGuard<'_, ()> {
        // Nothing inside the lock leaves the file half-written, so a poisoned lock is safe to reuse.
        self.lock.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn current_file(cfg: &Config) -> Res<TaskFile> {
        Ok(read(cfg)?.unwrap_or_default())
    }

    /// Startup: read the file (or migrate from the existing agents) and adopt agents that belong to no task.
    pub fn load(&self, cfg: &Config) -> Res<TaskFile> {
        let _held = self.guard();
        let file = Self::current_file(cfg)?;
        let (agents, _) = agent::list();
        let refs: Vec<AgentRef> =
            agents.into_iter().map(|a| AgentRef { name: a.name, cwd: a.cwd, created_at: a.created_at }).collect();
        let adopted = adopt_orphans(file.clone(), &refs, now_ms());
        if adopted != file {
            write(cfg, &adopted)?;
        }
        Ok(adopted)
    }

    /// New task in `project` with its first agent; becomes the current task.
    pub fn create(&self, cfg: &Config, project: &str, name: &str, kind: &str, pick: Pick) -> Res<Changed> {
        let _held = self.guard();
        let project = canonical_dir(project)?;
        let mut file = Self::current_file(cfg)?;
        let agent = session::create_agent(cfg, kind, &project, pick.model, pick.effort)?;
        let (name, auto_name) = match name.trim() {
            "" => (default_name(&file, &project), true),
            n => (n.to_string(), false),
        };
        let id = unique_id(&file.tasks, now_ms());
        file.tasks.push(Task { id: id.clone(), project, name, auto_name, created_at: agent::now_secs(), agents: vec![agent.clone()] });
        file.current = Some(id);
        write(cfg, &file)?;
        Ok(Changed { file, agent: Some(agent) })
    }

    pub fn add_agent(&self, cfg: &Config, task_id: &str, kind: &str, pick: Pick) -> Res<Changed> {
        let _held = self.guard();
        let mut file = Self::current_file(cfg)?;
        let task = file.tasks.iter_mut().find(|t| t.id == task_id).ok_or_else(|| format!("no task {task_id}"))?;
        let agent = session::create_agent(cfg, kind, &task.project, pick.model, pick.effort)?;
        task.agents.push(agent.clone());
        write(cfg, &file)?;
        Ok(Changed { file, agent: Some(agent) })
    }

    pub fn rename(&self, cfg: &Config, task_id: &str, name: &str) -> Res<TaskFile> {
        let _held = self.guard();
        let mut file = Self::current_file(cfg)?;
        let name = name.trim();
        if name.is_empty() {
            return Err("task name is empty".to_string());
        }
        let task = file.tasks.iter_mut().find(|t| t.id == task_id).ok_or_else(|| format!("no task {task_id}"))?;
        task.name = name.to_string();
        task.auto_name = false;
        write(cfg, &file)?;
        Ok(file)
    }

    pub fn set_current(&self, cfg: &Config, task_id: &str) -> Res<TaskFile> {
        let _held = self.guard();
        let mut file = Self::current_file(cfg)?;
        if !file.tasks.iter().any(|t| t.id == task_id) {
            return Err(format!("no task {task_id}"));
        }
        file.current = Some(task_id.to_string());
        write(cfg, &file)?;
        Ok(file)
    }

    /// Ends the task's agents, deletes its layout and the task. If an agent cannot be ended the task stays (the
    /// agents already ended are gone from it on the next try, since a missing record counts as ended).
    pub fn delete(&self, cfg: &Config, task_id: &str) -> Res<TaskFile> {
        let _held = self.guard();
        let file = Self::current_file(cfg)?;
        let task = file.tasks.iter().find(|t| t.id == task_id).ok_or_else(|| format!("no task {task_id}"))?;
        for name in &task.agents {
            if agent::load(name)?.is_some() {
                session::kill_agent(name)?;
            }
        }
        layout::remove(cfg, task_id)?;
        let file = remove_task(file, task_id);
        write(cfg, &file)?;
        Ok(file)
    }

    /// Ends one agent and takes its name off its task.
    pub fn kill_agent(&self, cfg: &Config, name: &str) -> Res<TaskFile> {
        let _held = self.guard();
        session::kill_agent(name)?;
        let file = forget_agent(Self::current_file(cfg)?, name);
        write(cfg, &file)?;
        Ok(file)
    }

    /// Dragging an agent's tab onto another task changes only its ownership: the agent keeps running and no
    /// layout file is touched. `current` stays put, so the dock keeps showing whatever it was showing.
    pub fn move_agent(&self, cfg: &Config, name: &str, to_id: &str) -> Res<TaskFile> {
        let _held = self.guard();
        let file = Self::current_file(cfg)?;
        if !file.tasks.iter().any(|t| t.id == to_id) {
            return Err(format!("no task {to_id}"));
        }
        let file = move_agent_file(file, name, to_id);
        write(cfg, &file)?;
        Ok(file)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn aref(name: &str, cwd: &str, created_at: u64) -> AgentRef {
        AgentRef { name: name.into(), cwd: cwd.into(), created_at }
    }

    fn task(id: &str, project: &str, created_at: u64, agents: &[&str]) -> Task {
        Task {
            id: id.into(),
            project: project.into(),
            name: id.into(),
            auto_name: true,
            created_at,
            agents: agents.iter().map(|s| s.to_string()).collect(),
        }
    }

    #[test]
    fn migration_makes_one_task_per_folder_named_after_it() {
        let agents = [
            aref("claude-a", "/w/alpha", 100),
            aref("codex-a", "/w/alpha", 150),
            aref("claude-b", "/w/中文 beta", 200),
        ];
        let got = adopt_orphans(TaskFile::default(), &agents, 5_000);
        assert_eq!(got.tasks.len(), 2);
        let alpha = got.tasks.iter().find(|t| t.project == "/w/alpha").unwrap();
        assert_eq!((alpha.name.as_str(), alpha.created_at), ("alpha", 100));
        assert!(alpha.auto_name);
        assert_eq!(alpha.agents, ["claude-a", "codex-a"]);
        let beta = got.tasks.iter().find(|t| t.project == "/w/中文 beta").unwrap();
        assert_eq!(beta.name, "中文 beta");
        assert_ne!(alpha.id, beta.id);
        assert_eq!(got.current.as_deref(), Some(beta.id.as_str()));
    }

    #[test]
    fn orphan_joins_the_newest_task_of_its_folder() {
        let file = TaskFile {
            current: Some("t-1".into()),
            tasks: vec![task("t-1", "/w/a", 10, &["x"]), task("t-2", "/w/a", 20, &["y"]), task("t-3", "/w/b", 30, &["z"])],
        };
        let agents = [aref("x", "/w/a", 1), aref("y", "/w/a", 2), aref("z", "/w/b", 3), aref("lost", "/w/a", 4)];
        let got = adopt_orphans(file, &agents, 9);
        assert_eq!(got.tasks[0].agents, ["x"]);
        assert_eq!(got.tasks[1].agents, ["y", "lost"]);
        assert_eq!(got.tasks[2].agents, ["z"]);
        assert_eq!(got.current.as_deref(), Some("t-1"));
    }

    #[test]
    fn orphan_in_a_folder_without_tasks_gets_a_new_task_with_a_fresh_id() {
        let file = TaskFile { current: Some("t-9".into()), tasks: vec![task("t-9", "/w/a", 10, &["x"])] };
        let got = adopt_orphans(file, &[aref("x", "/w/a", 1), aref("n", "/w/new", 2)], 9);
        assert_eq!(got.tasks.len(), 2);
        assert_eq!(got.tasks[1].name, "new");
        // "t-9" is taken, so the id is bumped.
        assert_eq!(got.tasks[1].id, "t-10");
    }

    #[test]
    fn nothing_changes_when_every_agent_already_has_a_task() {
        let file = TaskFile { current: Some("t-1".into()), tasks: vec![task("t-1", "/w/a", 10, &["x"])] };
        assert_eq!(adopt_orphans(file.clone(), &[aref("x", "/w/a", 1)], 9), file);
    }

    #[test]
    fn a_current_that_points_nowhere_is_moved_to_the_newest_task() {
        let file = TaskFile { current: Some("t-gone".into()), tasks: vec![task("t-1", "/w/a", 10, &[]), task("t-2", "/w/b", 20, &[])] };
        assert_eq!(adopt_orphans(file, &[], 9).current.as_deref(), Some("t-2"));
    }

    #[test]
    fn default_name_counts_only_the_projects_own_tasks() {
        let file = TaskFile { current: None, tasks: vec![task("t-1", "/w/a", 1, &[]), task("t-2", "/w/b", 2, &[]), task("t-3", "/w/a", 3, &[])] };
        assert_eq!(default_name(&file, "/w/a"), "任务 3");
        assert_eq!(default_name(&file, "/w/c"), "任务 1");
    }

    fn three() -> TaskFile {
        TaskFile {
            current: Some("t-2".into()),
            tasks: vec![task("t-1", "/w/a", 10, &["p"]), task("t-2", "/w/a", 20, &["q"]), task("t-3", "/w/b", 30, &["r"])],
        }
    }

    #[test]
    fn deleting_the_current_task_moves_to_the_newest_in_its_project_then_to_any() {
        let got = remove_task(three(), "t-2");
        assert_eq!(got.current.as_deref(), Some("t-1"));
        assert_eq!(got.tasks.len(), 2);
        let got = remove_task(remove_task(three(), "t-1"), "t-2");
        assert_eq!(got.current.as_deref(), Some("t-3"));
        let got = remove_task(got, "t-3");
        assert_eq!((got.current, got.tasks.len()), (None, 0));
    }

    #[test]
    fn deleting_another_task_keeps_the_current_one() {
        let got = remove_task(three(), "t-3");
        assert_eq!(got.current.as_deref(), Some("t-2"));
        assert_eq!(remove_task(three(), "t-nope"), three());
    }

    #[test]
    fn forgetting_an_agent_removes_its_name_everywhere() {
        let got = forget_agent(three(), "q");
        assert!(got.tasks[1].agents.is_empty());
        assert_eq!(got.tasks[0].agents, ["p"]);
    }

    #[test]
    fn file_round_trips_and_matches_the_spec_shape() {
        let dir = std::env::temp_dir().join(format!("shifu-tasks-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let cfg = Config { home: dir.clone(), socket: "s".into(), shifu_bin: "shifu".into(), user_statusline: None };
        assert_eq!(read(&cfg).unwrap(), None);
        let file = TaskFile { current: Some("t-1".into()), tasks: vec![task("t-1", "/w/中文", 7, &["claude-a"])] };
        write(&cfg, &file).unwrap();
        assert_eq!(read(&cfg).unwrap(), Some(file));
        let spec = r#"{ "current": "t-1762430000000",
            "tasks": [{ "id": "t-1762430000000", "project": "/abs/p", "name": "修登录", "created_at": 1, "agents": ["claude-xxx"] }] }"#;
        std::fs::write(dir.join("tasks.json"), spec).unwrap();
        let got = read(&cfg).unwrap().unwrap();
        assert_eq!(got.tasks[0].name, "修登录");
        // A B1 file has no `auto_name`; those tasks count as auto-named.
        assert!(got.tasks[0].auto_name);
        std::fs::write(dir.join("tasks.json"), spec.replace("\"agents\"", "\"auto_name\": false, \"agents\"")).unwrap();
        assert!(!read(&cfg).unwrap().unwrap().tasks[0].auto_name);
        std::fs::write(dir.join("tasks.json"), "{oops").unwrap();
        assert!(read(&cfg).unwrap_err().contains("tasks.json"));
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn moving_an_agent_changes_only_its_task() {
        let got = move_agent_file(three(), "q", "t-1");
        assert_eq!(got.tasks[0].agents, ["p", "q"]);
        assert!(got.tasks[1].agents.is_empty());
        assert_eq!(got.tasks[2].agents, ["r"]);
        assert_eq!(got.current.as_deref(), Some("t-2"));
    }

    #[test]
    fn dropping_an_agent_on_its_own_task_changes_nothing() {
        assert_eq!(move_agent_file(three(), "q", "t-2"), three());
    }

    #[test]
    fn moving_to_a_missing_task_or_without_a_name_changes_nothing() {
        assert_eq!(move_agent_file(three(), "q", "t-nope"), three());
        assert_eq!(move_agent_file(three(), "", "t-1"), three());
    }

    #[test]
    fn a_stray_agent_is_adopted_and_old_duplicates_are_repaired() {
        let mut file = three();
        file.tasks[2].agents.push("q".into()); // "q" was mistakenly in two tasks
        let got = move_agent_file(file, "q", "t-1");
        assert_eq!(got.tasks[0].agents, ["p", "q"]);
        assert!(got.tasks[1].agents.is_empty());
        assert_eq!(got.tasks[2].agents, ["r"]);
        // A second, unrelated drag on the repaired file still works.
        let got = move_agent_file(got, "stray", "t-3");
        assert_eq!(got.tasks[2].agents, ["r", "stray"]);
    }
}
