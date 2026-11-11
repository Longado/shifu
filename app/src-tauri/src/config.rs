//! Where the app keeps its state and which tmux server it talks to.
//!
//! The terminal edition uses socket `shifu` and `~/.shifu/agents`; the app must never touch those,
//! so it has its own defaults, each overridable (tests use a temp home and socket `shifu-app-dev`).

use std::path::{Path, PathBuf};

#[derive(Clone, Debug)]
pub struct Config {
    /// Holds agents/, tasks.json, layouts/, recent.json, tmux.conf, claude-hooks.json, statusline.sh, claude-statusline.json.
    pub home: PathBuf,
    pub socket: String,
    /// The `shifu` binary the agents' hooks call.
    pub shifu_bin: String,
    /// The `statusLine` command in the user's own `~/.claude/settings.json`, read once at start (APP-SPEC-B3).
    pub user_statusline: Option<String>,
}

pub fn resolve_home(app_home: Option<&str>, user_home: &str) -> PathBuf {
    match app_home.filter(|h| !h.is_empty()) {
        Some(h) => PathBuf::from(h),
        None => PathBuf::from(user_home).join(".shifu").join("app"),
    }
}

pub fn resolve_socket(app_socket: Option<&str>) -> String {
    app_socket.filter(|s| !s.is_empty()).unwrap_or("shifu-app").to_string()
}

/// A Finder-launched app gets PATH=/usr/bin:/bin:/usr/sbin:/sbin, so `claude`/`codex` would not be found.
/// Keep the inherited entries first and append the usual install locations that are missing.
pub fn merge_path(current: &str, user_home: &str) -> String {
    // Kimi Code before ~/.local/bin: a reinstalled old kimi-cli (uv) would put a `kimi` there too.
    let extras = [
        format!("{user_home}/.kimi-code/bin"),
        format!("{user_home}/.local/bin"),
        format!("{user_home}/.cargo/bin"),
        "/opt/homebrew/bin".to_string(),
        "/opt/homebrew/sbin".to_string(),
        "/usr/local/bin".to_string(),
    ];
    let mut dirs: Vec<&str> = current.split(':').filter(|d| !d.is_empty()).collect();
    for extra in &extras {
        if !dirs.contains(&extra.as_str()) {
            dirs.push(extra);
        }
    }
    dirs.join(":")
}

/// Write to a temp file and rename, so a crash never leaves half a file.
pub fn write_atomic(dest: &Path, text: &str) -> Result<(), String> {
    let tmp = dest.with_extension(format!("json.{}.tmp", std::process::id()));
    std::fs::write(&tmp, text).map_err(|e| format!("write {} failed: {e}", tmp.display()))?;
    std::fs::rename(&tmp, dest).map_err(|e| format!("rename to {} failed: {e}", dest.display()))
}

impl Config {
    /// Reads the overrides and exports `SHIFU_HOME` / `SHIFU_SOCKET`, which the shared `shifu` library reads for
    /// its agent files and tmux calls. Must run first thing in `main`, before any thread exists.
    pub fn init() -> Config {
        let var = |key: &str| std::env::var(key).ok();
        let user_home = var("HOME").unwrap_or_default();
        let home = resolve_home(var("SHIFU_APP_HOME").as_deref(), &user_home);
        let socket = resolve_socket(var("SHIFU_APP_SOCKET").as_deref());
        let shifu_bin = var("SHIFU_APP_SHIFU")
            .filter(|b| !b.is_empty())
            .unwrap_or_else(|| format!("{user_home}/.cargo/bin/shifu"));
        let user_statusline = crate::statusline::user_command_from_file(&Path::new(&user_home).join(".claude/settings.json"));
        let path = merge_path(&var("PATH").unwrap_or_default(), &user_home);
        // A Finder-launched app has no locale; without UTF-8 the agents mangle Chinese.
        let lang = var("LANG").filter(|l| l.contains("UTF-8")).unwrap_or_else(|| "en_US.UTF-8".to_string());
        // SAFETY: called at the top of main before any other thread is started.
        unsafe {
            std::env::set_var("SHIFU_HOME", &home);
            std::env::set_var("SHIFU_SOCKET", &socket);
            std::env::set_var("PATH", path);
            std::env::set_var("LANG", lang);
        }
        Config { home, socket, shifu_bin, user_statusline }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn home_defaults_under_dot_shifu_app_and_can_be_overridden() {
        assert_eq!(resolve_home(None, "/Users/e"), PathBuf::from("/Users/e/.shifu/app"));
        assert_eq!(resolve_home(Some(""), "/Users/e"), PathBuf::from("/Users/e/.shifu/app"));
        assert_eq!(resolve_home(Some("/tmp/x"), "/Users/e"), PathBuf::from("/tmp/x"));
    }

    #[test]
    fn socket_defaults_to_shifu_app_never_the_terminal_edition() {
        assert_eq!(resolve_socket(None), "shifu-app");
        assert_eq!(resolve_socket(Some("")), "shifu-app");
        assert_eq!(resolve_socket(Some("shifu-app-dev")), "shifu-app-dev");
    }

    #[test]
    fn path_keeps_inherited_order_and_appends_only_missing_dirs() {
        let got = merge_path("/usr/bin:/bin:/opt/homebrew/bin", "/Users/e");
        assert_eq!(got, "/usr/bin:/bin:/opt/homebrew/bin:/Users/e/.kimi-code/bin:/Users/e/.local/bin:/Users/e/.cargo/bin:/opt/homebrew/sbin:/usr/local/bin");
    }

    #[test]
    fn path_from_empty_gets_the_usual_dirs() {
        assert!(merge_path("", "/Users/e").starts_with("/Users/e/.kimi-code/bin:/Users/e/.local/bin:"));
    }
}

/// The id of the bundle this process runs from, read from the Info.plist beside the executable: a renamed copy has
/// a different id from the compiled-in one. `None` when not running from a bundle (e.g. `cargo run`).
pub fn running_bundle_id() -> Option<String> {
    let exe = std::env::current_exe().ok()?;
    let plist = exe.parent()?.parent()?.join("Info.plist");
    if !plist.is_file() {
        return None;
    }
    let out = std::process::Command::new("/usr/bin/plutil")
        .args(["-extract", "CFBundleIdentifier", "raw", "-o", "-"])
        .arg(&plist)
        .output()
        .ok()?;
    out.status.success().then(|| String::from_utf8_lossy(&out.stdout).trim().to_string()).filter(|s| !s.is_empty())
}

/// the user's own app is "Shifu"; any other bundle (the acceptance copy) says "Shifu 测试" in its window title and
/// sidebar, so the two cannot be mistaken for each other (11-08).
pub fn name_for(bundle_id: Option<&str>) -> &'static str {
    match bundle_id {
        None | Some("dev.longado.shifu") => "Shifu",
        Some(_) => "Shifu 测试",
    }
}

pub fn app_name() -> &'static str {
    static NAME: std::sync::OnceLock<&'static str> = std::sync::OnceLock::new();
    NAME.get_or_init(|| name_for(running_bundle_id().as_deref()))
}

#[cfg(test)]
mod name_tests {
    use super::name_for;

    #[test]
    fn only_the_real_bundle_is_plain_shifu() {
        assert_eq!(name_for(Some("dev.longado.shifu")), "Shifu");
        assert_eq!(name_for(None), "Shifu");
        assert_eq!(name_for(Some("dev.longado.shifu.accept")), "Shifu 测试");
    }
}
