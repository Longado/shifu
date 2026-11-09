//! Each task's dockview layout JSON, kept as an opaque string in `<home>/layouts/<task id>.json`.

use crate::config::{Config, write_atomic};
use std::path::PathBuf;

/// The id becomes a file name, so only the characters task ids are made of are accepted.
fn path(cfg: &Config, task_id: &str) -> Result<PathBuf, String> {
    let ok = !task_id.is_empty() && task_id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_');
    if !ok {
        return Err(format!("bad task id: {task_id:?}"));
    }
    Ok(cfg.home.join("layouts").join(format!("{task_id}.json")))
}

pub fn save(cfg: &Config, task_id: &str, json: &str) -> Result<(), String> {
    let dest = path(cfg, task_id)?;
    let dir = cfg.home.join("layouts");
    std::fs::create_dir_all(&dir).map_err(|e| format!("create {} failed: {e}", dir.display()))?;
    write_atomic(&dest, json)
}

pub fn load(cfg: &Config, task_id: &str) -> Result<Option<String>, String> {
    let file = path(cfg, task_id)?;
    match std::fs::read_to_string(&file) {
        Ok(text) => Ok(Some(text)),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(format!("read {} failed: {e}", file.display())),
    }
}

pub fn remove(cfg: &Config, task_id: &str) -> Result<(), String> {
    let file = path(cfg, task_id)?;
    match std::fs::remove_file(&file) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(format!("remove {} failed: {e}", file.display())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg_in(dir: &std::path::Path) -> Config {
        Config { home: dir.to_path_buf(), socket: "s".into(), shifu_bin: "shifu".into(), user_statusline: None }
    }

    #[test]
    fn each_task_keeps_its_own_file_and_removal_touches_only_that_one() {
        let dir = std::env::temp_dir().join(format!("shifu-layout-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let cfg = cfg_in(&dir);
        assert_eq!(load(&cfg, "t-1").unwrap(), None);
        save(&cfg, "t-1", r#"{"grid":{"x":"中文"}}"#).unwrap();
        save(&cfg, "t-2", "{}").unwrap();
        assert_eq!(load(&cfg, "t-1").unwrap().as_deref(), Some(r#"{"grid":{"x":"中文"}}"#));
        assert_eq!(load(&cfg, "t-2").unwrap().as_deref(), Some("{}"));
        remove(&cfg, "t-1").unwrap();
        remove(&cfg, "t-1").unwrap();
        assert_eq!(load(&cfg, "t-1").unwrap(), None);
        assert_eq!(load(&cfg, "t-2").unwrap().as_deref(), Some("{}"));
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn ids_that_could_leave_the_layouts_folder_are_refused() {
        let cfg = cfg_in(std::path::Path::new("/nonexistent"));
        for bad in ["", "../x", "a/b", "a.b"] {
            assert!(load(&cfg, bad).is_err(), "{bad:?}");
        }
    }
}
