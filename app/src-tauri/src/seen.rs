//! Which replies the user has looked at (APP-SPEC-B2). `<home>/seen.json` maps an agent name to the `status_since`
//! it had when the user saw it. Only the app writes it; the frontend decides what "seen" means.

use crate::config::{Config, write_atomic};
use std::collections::HashMap;

pub type Seen = HashMap<String, u64>;

fn path(cfg: &Config) -> std::path::PathBuf {
    cfg.home.join("seen.json")
}

/// `None` when there is no file yet (first start after the upgrade).
pub fn load(cfg: &Config) -> Result<Option<Seen>, String> {
    let path = path(cfg);
    match std::fs::read_to_string(&path) {
        Ok(text) => serde_json::from_str(&text).map(Some).map_err(|e| format!("{} is not valid: {e}", path.display())),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(format!("read {} failed: {e}", path.display())),
    }
}

pub fn save(cfg: &Config, seen: &Seen) -> Result<(), String> {
    let text = serde_json::to_string_pretty(seen).map_err(|e| format!("encode seen failed: {e}"))?;
    write_atomic(&path(cfg), &text)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_file_is_none_and_saved_map_round_trips() {
        let dir = std::env::temp_dir().join(format!("shifu-seen-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let cfg = Config { home: dir.clone(), socket: "s".into(), shifu_bin: "shifu".into(), user_statusline: None };
        assert_eq!(load(&cfg).unwrap(), None);
        let seen: Seen = [("claude-中文-1".to_string(), 1762430000)].into();
        save(&cfg, &seen).unwrap();
        assert_eq!(load(&cfg).unwrap(), Some(seen));
        std::fs::write(dir.join("seen.json"), "{oops").unwrap();
        assert!(load(&cfg).unwrap_err().contains("seen.json"));
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
