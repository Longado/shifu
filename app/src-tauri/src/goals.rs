//! Progress page: each project's `GOALS.md` (checkbox list, parsed by the page) and which project each agent is
//! working in. Read only.
//!
//! An agent belongs to the project whose folder holds its working directory. Most agents run in the Workspace
//! root, so for Claude the last file it read or edited (from its transcript) decides instead.

use crate::workbuddy::{self, WbSession};
use serde::Serialize;
use std::collections::HashMap;
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
    pub agents: HashMap<String, String>,
    /// WorkBuddy's conversations, newest first, each with the task list it broke its work into.
    pub workbuddy: Vec<WbSession>,
    pub workbuddy_error: Option<String>,
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
    let mut placed = HashMap::new();
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
    Goals { files, agents: placed, workbuddy: wb, workbuddy_error: wb_error }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dirs() -> Vec<String> {
        vec!["/w".into(), "/w/01_项目/shifu".into(), "/w/01_项目/jade".into()]
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
        let t = r#"{"input":{"file_path":"/w/01_项目/jade/a.ts"}}
{"input":{"file_path":"/w/01_项目/shifu/b.rs"}}
{"input":{"file_path":"/w/README.md"}}"#;
        assert_eq!(last_project_in(t, &d, "/w").unwrap(), "/w/01_项目/shifu");
        assert_eq!(last_project_in(r#""file_path":"/w/x.md""#, &d, "/w"), None);
    }

    #[test]
    fn finds_goal_files_two_levels_down_and_skips_archive() {
        let root = std::env::temp_dir().join(format!("shifu-goals-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        for d in ["01/a", "01/b/deep", "archive/c"] {
            std::fs::create_dir_all(root.join(d)).unwrap();
        }
        for f in ["01/a/GOALS.md", "01/b/deep/GOALS.md", "archive/c/GOALS.md"] {
            std::fs::write(root.join(f), "- [ ] x").unwrap();
        }
        let found: Vec<String> = find_files(&root).into_iter().map(|f| f.dir).collect();
        assert_eq!(found, vec![root.join("01/a").to_string_lossy().into_owned()]);
        std::fs::remove_dir_all(&root).unwrap();
    }
}
