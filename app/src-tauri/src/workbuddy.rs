//! WorkBuddy's own breakdown of its work, for the progress page. Read only: its conversations from
//! `~/.workbuddy/workbuddy.db` (through the system `sqlite3`, opened read-only) and the task list it writes per
//! conversation in `~/.workbuddy/tasks/<conversation id>/<n>.json` (subject, status).

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::process::Command;

/// Newest conversations looked at; the page shows fewer.
const LIMIT: usize = 30;

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct WbTask {
    pub subject: String,
    /// "completed", "in_progress" or "pending".
    pub status: String,
}

#[derive(Serialize, Debug, PartialEq)]
pub struct WbSession {
    pub id: String,
    pub title: String,
    /// "working" while it runs, "completed" after.
    pub status: String,
    /// Unix milliseconds.
    pub updated_at: i64,
    pub tasks: Vec<WbTask>,
}

#[derive(Deserialize)]
struct Row {
    id: String,
    title: Option<String>,
    status: Option<String>,
    updated_at: i64,
}

pub fn home() -> PathBuf {
    match std::env::var_os("SHIFU_WORKBUDDY_HOME") {
        Some(h) => PathBuf::from(h),
        None => PathBuf::from(std::env::var_os("HOME").unwrap_or_default()).join(".workbuddy"),
    }
}

/// One conversation's task list in file order (1.json, 2.json, …).
pub fn tasks_in(dir: &Path) -> Vec<WbTask> {
    let Ok(entries) = std::fs::read_dir(dir) else { return Vec::new() };
    let mut files: Vec<(u64, PathBuf)> = entries
        .flatten()
        .filter_map(|e| {
            let p = e.path();
            let n = p.file_stem()?.to_str()?.parse().ok()?;
            (p.extension()? == "json").then_some((n, p))
        })
        .collect();
    files.sort();
    files.iter().filter_map(|(_, p)| serde_json::from_str(&std::fs::read_to_string(p).ok()?).ok()).collect()
}

pub fn parse_rows(json: &str) -> Result<Vec<(String, String, String, i64)>, String> {
    if json.trim().is_empty() {
        return Ok(Vec::new()); // sqlite3 -json prints nothing for no rows
    }
    let rows: Vec<Row> = serde_json::from_str(json).map_err(|e| format!("WorkBuddy 对话列表读不懂: {e}"))?;
    Ok(rows
        .into_iter()
        .map(|r| (r.id, r.title.unwrap_or_default(), r.status.unwrap_or_default(), r.updated_at))
        .collect())
}

/// `Ok(empty)` when WorkBuddy is not installed; `Err` when its database is there but cannot be read.
pub fn read(home: &Path) -> Result<Vec<WbSession>, String> {
    let db = home.join("workbuddy.db");
    if !db.is_file() {
        return Ok(Vec::new());
    }
    let sql = format!(
        "select id, coalesce(nullif(custom_title,''), title) as title, status, updated_at from sessions \
         where deleted_at is null and coalesce(is_background_automation, 0) = 0 order by updated_at desc limit {LIMIT}"
    );
    let out = Command::new("/usr/bin/sqlite3")
        .args(["-readonly", "-json"])
        .arg(&db)
        .arg(sql)
        .output()
        .map_err(|e| format!("sqlite3 没能启动: {e}"))?;
    if !out.status.success() {
        return Err(format!("读 WorkBuddy 数据库失败: {}", String::from_utf8_lossy(&out.stderr).trim()));
    }
    let rows = parse_rows(&String::from_utf8_lossy(&out.stdout))?;
    Ok(rows
        .into_iter()
        .map(|(id, title, status, updated_at)| {
            let tasks = tasks_in(&home.join("tasks").join(&id));
            WbSession { id, title, status, updated_at, tasks }
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn task_files_in_number_order_and_rows_parsed() {
        let dir = std::env::temp_dir().join(format!("shifu-wb-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        for (n, s) in [(10, "completed"), (2, "in_progress"), (1, "completed")] {
            let body = format!(r#"{{"subject":"第{n}步","status":"{s}","id":"{n}","activeForm":"x"}}"#);
            std::fs::write(dir.join(format!("{n}.json")), body).unwrap();
        }
        std::fs::write(dir.join("notes.txt"), "x").unwrap();
        let subjects: Vec<String> = tasks_in(&dir).into_iter().map(|t| t.subject).collect();
        assert_eq!(subjects, ["第1步", "第2步", "第10步"]);
        std::fs::remove_dir_all(&dir).unwrap();

        let rows = parse_rows(r#"[{"id":"a","title":null,"status":"working","updated_at":5}]"#).unwrap();
        assert_eq!(rows, vec![("a".into(), "".into(), "working".into(), 5)]);
        assert_eq!(parse_rows("").unwrap(), vec![]);
    }

    #[test]
    fn no_workbuddy_is_empty_not_an_error() {
        assert_eq!(read(Path::new("/nonexistent-shifu")).unwrap(), vec![]);
    }
}
