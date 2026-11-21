//! Progress page: each project's `GOALS.md` (checkbox list, parsed by the page) and which project each agent is
//! working in. Read only.
//!
//! An agent belongs to the project whose folder holds its working directory. Most agents run in the Workspace
//! root, so for Claude the last file it read or edited (from its transcript) decides instead.

use crate::workbuddy::{self, WbSession};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

const FILE: &str = "GOALS.md";
/// Folders under the root searched for GOALS.md: the root itself, its children and grandchildren.
const DEPTH: usize = 2;
const SKIP: [&str; 4] = ["node_modules", "archive", "target", "dist"];
/// How much of a transcript's end is searched for file paths.
const TAIL: u64 = 512 * 1024;

#[derive(Serialize)]
pub struct GoalFile {
    pub dir: String,
    pub text: String,
}

#[derive(Serialize)]
pub struct Goals {
    pub files: Vec<GoalFile>,
    /// Agent name -> the `dir` of the project it is working in.
    pub agents: BTreeMap<String, String>,
    /// WorkBuddy's conversations, newest first, each with the task list it broke its work into.
    pub workbuddy: Vec<WbSession>,
    pub workbuddy_error: Option<String>,
    /// Every task (cancelled ones left out) of the projects in `.data/tasks.db` that still have open work.
    pub todos: Vec<TodoRow>,
    pub todos_error: Option<String>,
}

#[derive(Serialize, Deserialize)]
pub struct TodoRow {
    pub project: String,
    pub title: String,
    pub status: String,
    pub due: Option<String>,
}

const TODO_SQL: &str = "SELECT p.name AS project, t.title, t.status, t.due_date AS due \
FROM tasks t JOIN projects p ON p.id = t.project_id \
WHERE t.status != 'cancelled' \
AND t.project_id IN (SELECT project_id FROM tasks WHERE status IN ('todo','doing','blocked')) \
ORDER BY p.name, t.id";

/// The Workspace task list (`.data/tasks.db`), read with the system sqlite3 so the app needs no SQLite library.
pub fn read_todos(root: &Path) -> Result<Vec<TodoRow>, String> {
    let db = root.join(".data/tasks.db");
    if !db.is_file() {
        return Ok(Vec::new());
    }
    let out = std::process::Command::new("/usr/bin/sqlite3")
        .args(["-readonly", "-json"])
        .arg(&db)
        .arg(TODO_SQL)
        .output()
        .map_err(|e| format!("读不了待办表：{e}"))?;
    if !out.status.success() {
        return Err(format!("读不了待办表：{}", String::from_utf8_lossy(&out.stderr).trim()));
    }
    let text = String::from_utf8_lossy(&out.stdout);
    // sqlite3 -json prints nothing at all when no row matches.
    if text.trim().is_empty() {
        return Ok(Vec::new());
    }
    serde_json::from_str(&text).map_err(|e| format!("待办表格式不对：{e}"))
}

pub fn root() -> PathBuf {
    match std::env::var_os("SHIFU_GOALS_ROOT") {
        Some(r) => PathBuf::from(r),
        None => PathBuf::from(std::env::var_os("HOME").unwrap_or_default()).join("Desktop/Workspace"),
    }
}

pub fn find_files(root: &Path) -> Vec<GoalFile> {
    let mut found = Vec::new();
    let mut level = vec![root.to_path_buf()];
    for depth in 0..=DEPTH {
        let mut next = Vec::new();
        for dir in level {
            if let Ok(text) = std::fs::read_to_string(dir.join(FILE)) {
                found.push(GoalFile { dir: dir.to_string_lossy().into_owned(), text });
            }
            if depth == DEPTH {
                continue;
            }
            let Ok(entries) = std::fs::read_dir(&dir) else { continue };
            for e in entries.flatten() {
                let name = e.file_name().to_string_lossy().into_owned();
                if name.starts_with('.') || SKIP.contains(&name.as_str()) || !e.file_type().is_ok_and(|t| t.is_dir()) {
                    continue;
                }
                // `.git` as a file marks a linked worktree: the same project checked out again, not another project.
                if e.path().join(".git").is_file() {
                    continue;
                }
                next.push(e.path());
            }
        }
        level = next;
    }
    found.sort_by(|a, b| a.dir.cmp(&b.dir));
    found
}

/// The deepest project folder containing `path` (the root's own GOALS.md only when nothing deeper matches).
pub fn project_of<'a>(path: &str, dirs: &'a [String]) -> Option<&'a String> {
    dirs.iter().filter(|d| path == d.as_str() || path.starts_with(&format!("{d}/"))).max_by_key(|d| d.len())
}

/// The last `"file_path":"…"` in `text` that lies in a project other than the root's.
pub fn last_project_in<'a>(text: &str, dirs: &'a [String], root: &str) -> Option<&'a String> {
    let key = "\"file_path\":\"";
    text.rmatch_indices(key).find_map(|(i, _)| {
        let rest = &text[i + key.len()..];
        let path = &rest[..rest.find('"')?];
        project_of(path, dirs).filter(|d| d.as_str() != root)
    })
}

fn read_tail(path: &Path) -> Option<String> {
    let mut f = std::fs::File::open(path).ok()?;
    let len = f.metadata().ok()?.len();
    f.seek(SeekFrom::Start(len.saturating_sub(TAIL))).ok()?;
    let mut bytes = Vec::new();
    f.read_to_end(&mut bytes).ok()?;
    Some(String::from_utf8_lossy(&bytes).into_owned())
}

fn claude_transcript(session_id: &str) -> Option<PathBuf> {
    let projects = PathBuf::from(std::env::var_os("HOME")?).join(".claude/projects");
    let file = format!("{session_id}.jsonl");
    std::fs::read_dir(projects).ok()?.flatten().map(|e| e.path().join(&file)).find(|p| p.is_file())
}

pub struct AgentWhere<'a> {
    pub name: &'a str,
    pub kind: &'a str,
    pub cwd: &'a str,
    pub session_id: Option<&'a str>,
}

pub fn collect(agents: &[AgentWhere]) -> Goals {
    let root = root();
    let root_s = root.to_string_lossy().into_owned();
    let files = find_files(&root);
    let dirs: Vec<String> = files.iter().map(|f| f.dir.clone()).collect();
    let mut placed = BTreeMap::new();
    for a in agents {
        let by_cwd = project_of(a.cwd, &dirs).filter(|d| d.as_str() != root_s);
        let by_files = || {
            let sid = a.session_id.filter(|_| a.kind == "claude")?;
            last_project_in(&read_tail(&claude_transcript(sid)?)?, &dirs, &root_s)
        };
        let dir = by_cwd.or_else(by_files).or_else(|| project_of(a.cwd, &dirs));
        if let Some(d) = dir {
            placed.insert(a.name.to_string(), d.clone());
        }
    }
    let (wb, wb_error) = match workbuddy::read(&workbuddy::home()) {
        Ok(s) => (s, None),
        Err(e) => (Vec::new(), Some(e)),
    };
    let (todos, todos_error) = match read_todos(&root) {
        Ok(t) => (t, None),
        Err(e) => (Vec::new(), Some(e)),
    };
    Goals { files, agents: placed, workbuddy: wb, workbuddy_error: wb_error, todos, todos_error }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn todo_fixture(name: &str, sql: &str) -> PathBuf {
        let root = std::env::temp_dir().join(format!("shifu-todos-{name}-{}", std::process::id()));
        std::fs::create_dir_all(root.join(".data")).unwrap();
        let out = std::process::Command::new("/usr/bin/sqlite3")
            .arg(root.join(".data/tasks.db"))
            .arg(sql)
            .output()
            .unwrap();
        assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
        root
    }

    #[test]
    fn optional_todo_database_can_be_absent() {
        let root = std::env::temp_dir().join(format!("shifu-todos-absent-{}", std::process::id()));
        assert!(read_todos(&root).unwrap().is_empty());
        assert!(!root.exists());
    }

    #[test]
    fn todos_include_completed_work_only_in_projects_with_open_tasks() {
        let root = todo_fixture("open-projects", "\
            CREATE TABLE projects (id INTEGER PRIMARY KEY, name TEXT);\
            CREATE TABLE tasks (id INTEGER PRIMARY KEY, project_id INTEGER, title TEXT, status TEXT, due_date TEXT);\
            INSERT INTO projects VALUES (1, 'Alpha'), (2, 'Completed'), (3, 'Blocked');\
            INSERT INTO tasks VALUES\
              (1, 1, 'Finished', 'done', NULL),\
              (2, 1, 'Next', 'todo', '2026-12-01'),\
              (3, 1, 'Discarded', 'cancelled', NULL),\
              (4, 2, 'All done', 'done', NULL),\
              (5, 3, 'Need input', 'blocked', NULL);");
        let rows = read_todos(&root).unwrap();
        assert_eq!(serde_json::to_value(rows).unwrap(), serde_json::json!([
            {"project": "Alpha", "title": "Finished", "status": "done", "due": null},
            {"project": "Alpha", "title": "Next", "status": "todo", "due": "2026-12-01"},
            {"project": "Blocked", "title": "Need input", "status": "blocked", "due": null}
        ]));
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn empty_todo_query_returns_an_empty_list() {
        let root = todo_fixture("empty", "\
            CREATE TABLE projects (id INTEGER PRIMARY KEY, name TEXT);\
            CREATE TABLE tasks (id INTEGER PRIMARY KEY, project_id INTEGER, title TEXT, status TEXT, due_date TEXT);");
        assert!(read_todos(&root).unwrap().is_empty());
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn unreadable_todo_schema_is_reported_instead_of_becoming_empty_work() {
        let root = todo_fixture("bad-schema", "CREATE TABLE unrelated (id INTEGER);");
        assert!(read_todos(&root).err().unwrap().contains("no such table"));
        std::fs::remove_dir_all(root).unwrap();
    }

    fn dirs() -> Vec<String> {
        vec!["/w".into(), "/w/01_项目/shifu".into(), "/w/01_项目/notes".into()]
    }

    #[test]
    fn deepest_folder_wins_and_prefixes_must_end_at_a_slash() {
        let d = dirs();
        assert_eq!(project_of("/w/01_项目/shifu/app/src", &d).unwrap(), "/w/01_项目/shifu");
        assert_eq!(project_of("/w/01_项目/shifu", &d).unwrap(), "/w/01_项目/shifu");
        assert_eq!(project_of("/w/01_项目/shifu-old/x", &d).unwrap(), "/w");
        assert_eq!(project_of("/elsewhere", &d), None);
    }

    #[test]
    fn last_file_in_a_project_decides() {
        let d = dirs();
        let t = r#"{"input":{"file_path":"/w/01_项目/notes/a.ts"}}
{"input":{"file_path":"/w/01_项目/shifu/b.rs"}}
{"input":{"file_path":"/w/README.md"}}"#;
        assert_eq!(last_project_in(t, &d, "/w").unwrap(), "/w/01_项目/shifu");
        assert_eq!(last_project_in(r#""file_path":"/w/x.md""#, &d, "/w"), None);
    }

    #[test]
    fn finds_goal_files_two_levels_down_and_skips_archive() {
        let root = std::env::temp_dir().join(format!("shifu-goals-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        for d in ["01/a", "01/a-wt", "01/b/deep", "archive/c"] {
            std::fs::create_dir_all(root.join(d)).unwrap();
        }
        for f in ["01/a/GOALS.md", "01/a-wt/GOALS.md", "01/b/deep/GOALS.md", "archive/c/GOALS.md"] {
            std::fs::write(root.join(f), "- [ ] x").unwrap();
        }
        // A linked worktree is the same project checked out again: its list would be a second card of the same name.
        std::fs::write(root.join("01/a-wt/.git"), "gitdir: /elsewhere/.git/worktrees/a-wt").unwrap();
        let found: Vec<String> = find_files(&root).into_iter().map(|f| f.dir).collect();
        assert_eq!(found, vec![root.join("01/a").to_string_lossy().into_owned()]);
        std::fs::remove_dir_all(&root).unwrap();
    }
}
