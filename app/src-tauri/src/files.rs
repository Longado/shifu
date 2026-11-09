//! Paths clicked in a terminal: which exist, open them in their default app, read one for the in-app viewer.

use serde::Serialize;
use shifu::tmux::run_command;
use std::path::{Path, PathBuf};
use std::process::Command;

/// Big enough for screenshots and PDFs, small enough not to stall the window.
const MAX_READ: u64 = 30 * 1024 * 1024;

#[derive(Serialize, Debug, PartialEq)]
pub struct Resolved {
    pub path: String,
    pub is_dir: bool,
}

/// `~/x` against `home`, `x` and `./x` against `cwd`; `None` when nothing is there.
pub fn resolve(written: &str, cwd: &str, home: &str) -> Option<Resolved> {
    let full: PathBuf = if written == "~" {
        PathBuf::from(home)
    } else if let Some(rest) = written.strip_prefix("~/") {
        Path::new(home).join(rest)
    } else if written.starts_with('/') {
        PathBuf::from(written)
    } else {
        Path::new(cwd).join(written)
    };
    let real = full.canonicalize().ok()?;
    let is_dir = real.is_dir();
    Some(Resolved { path: real.to_string_lossy().into_owned(), is_dir })
}

/// A web link, or an existing absolute path, in its default app (`open` decides which).
pub fn open(target: &str) -> Result<(), String> {
    let web = target.starts_with("https://") || target.starts_with("http://");
    if !web && !(target.starts_with('/') && Path::new(target).exists()) {
        return Err(format!("打不开：{target}"));
    }
    let mut cmd = Command::new("open");
    cmd.arg(target);
    run_command(cmd).map(|_| ())
}

pub fn read(path: &str) -> Result<Vec<u8>, String> {
    let meta = std::fs::metadata(path).map_err(|e| format!("读不了 {path}：{e}"))?;
    if !meta.is_file() {
        return Err(format!("{path} 不是文件"));
    }
    if meta.len() > MAX_READ {
        return Err(format!("文件有 {} MB，太大，请用默认程序打开", meta.len() / 1024 / 1024));
    }
    std::fs::read(path).map_err(|e| format!("读不了 {path}：{e}"))
}

/// A file dropped onto a terminal: the webview gets its bytes but not its path, so keep a copy in `dir` and hand
/// back that path. The name keeps its stem and extension; spaces and shell characters become `-`, so the path can be
/// typed into a prompt as is.
pub fn save_drop(dir: &Path, name: &str, bytes: &[u8], stamp: u64) -> Result<String, String> {
    let base = Path::new(name).file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let clean: String = base
        .chars()
        .map(|c| if c.is_alphanumeric() || matches!(c, '.' | '_' | '-') { c } else { '-' })
        .collect();
    let clean = if clean.trim_matches(|c| c == '-' || c == '.').is_empty() { "dropped".to_string() } else { clean };
    std::fs::create_dir_all(dir).map_err(|e| format!("建不了 {}：{e}", dir.display()))?;
    let path = dir.join(format!("{stamp}-{clean}"));
    std::fs::write(&path, bytes).map_err(|e| format!("存不了 {}：{e}", path.display()))?;
    Ok(path.to_string_lossy().into_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dropped_file_keeps_a_typeable_name() {
        let dir = std::env::temp_dir().join(format!("shifu-drops-{}", std::process::id()));
        let path = save_drop(&dir, "截屏2026-11-07 23.10.11 (2).png", b"png", 7).unwrap();
        assert_eq!(path, dir.join("7-截屏2026-11-07-23.10.11--2-.png").to_string_lossy());
        assert_eq!(std::fs::read(&path).unwrap(), b"png");
        assert!(save_drop(&dir, "../../etc/x y.txt", b"", 8).unwrap().ends_with("/8-x-y.txt"));
        assert!(save_drop(&dir, "", b"", 9).unwrap().ends_with("/9-dropped"));
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn resolves_home_relative_and_absolute_and_drops_missing() {
        let dir = std::env::temp_dir().join(format!("shifu-files-{}", std::process::id()));
        std::fs::create_dir_all(dir.join("sub")).unwrap();
        std::fs::write(dir.join("sub/a.md"), "x").unwrap();
        let real = dir.canonicalize().unwrap();
        let base = real.to_string_lossy().to_string();
        let file = format!("{base}/sub/a.md");

        assert_eq!(resolve("sub/a.md", &base, "/nowhere"), Some(Resolved { path: file.clone(), is_dir: false }));
        assert_eq!(resolve("./sub/../sub/a.md", &base, "/nowhere"), Some(Resolved { path: file.clone(), is_dir: false }));
        assert_eq!(resolve("~/sub/a.md", "/nowhere", &base), Some(Resolved { path: file.clone(), is_dir: false }));
        assert_eq!(resolve(&file, "/nowhere", "/nowhere"), Some(Resolved { path: file, is_dir: false }));
        assert_eq!(resolve("sub", &base, "/nowhere").map(|r| r.is_dir), Some(true));
        assert_eq!(resolve("sub/missing.md", &base, "/nowhere"), None);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn open_refuses_relative_missing_and_option_like_targets() {
        assert!(open("relative/a.md").is_err());
        assert!(open("/definitely/not/here.md").is_err());
        assert!(open("-a Calculator").is_err());
    }
}
