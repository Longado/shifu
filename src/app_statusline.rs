//! Claude only reports its 5-hour and 7-day usage inside the JSON it feeds to a `statusLine` command (APP-SPEC-B3 §2).
//! The app points Claude's `--settings` at a small script that keeps that JSON for the sidebar and then passes the
//! same input to the user's own statusLine command, so what Claude shows in its status bar does not change.

use crate::app_session::{Config, write_atomic};
use serde_json::{Value, json};
use std::path::{Path, PathBuf};

pub fn script_path(cfg: &Config) -> PathBuf {
    cfg.home.join("statusline.sh")
}

/// Where the script keeps the last input that carried `rate_limits`.
pub fn capture_path(home: &Path) -> PathBuf {
    home.join("claude-statusline.json")
}

/// Wrap in single quotes for `sh`.
pub fn shell_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', "'\\''"))
}

/// The `statusLine.command` out of a Claude settings file's text.
pub fn user_command(settings_json: &str) -> Result<Option<String>, String> {
    let v: Value = serde_json::from_str(settings_json).map_err(|e| e.to_string())?;
    Ok(v.pointer("/statusLine/command").and_then(Value::as_str).filter(|c| !c.trim().is_empty()).map(str::to_string))
}

/// Read once at start. A missing file or no `statusLine` is normal; a file that cannot be read says so on stderr.
pub fn user_command_from_file(path: &Path) -> Option<String> {
    let text = match std::fs::read_to_string(path) {
        Ok(t) => t,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return None,
        Err(e) => {
            eprintln!("shifu-app: cannot read {}: {e}", path.display());
            return None;
        }
    };
    user_command(&text).unwrap_or_else(|e| {
        eprintln!("shifu-app: {} is not valid JSON ({e}); its statusLine is not passed on", path.display());
        None
    })
}

/// The script text. `user` is run with the same stdin and its output (and exit code) is the script's own.
pub fn script_text(home: &Path, user: Option<&str>) -> String {
    let dir = shell_quote(&home.to_string_lossy());
    let user = shell_quote(user.unwrap_or(""));
    format!(
        "#!/bin/sh
# Written by Shifu.app before every Claude start; edits are overwritten.
dir={dir}
user={user}
tmp=\"$dir/.statusline.$$\"
cat > \"$tmp\" || exit 0
if grep -q '\"rate_limits\"' \"$tmp\"; then
  cp \"$tmp\" \"$dir/claude-statusline.json.$$\" && mv -f \"$dir/claude-statusline.json.$$\" \"$dir/claude-statusline.json\"
fi
rc=0
if [ -n \"$user\" ]; then
  sh -c \"$user\" < \"$tmp\"
  rc=$?
fi
rm -f \"$tmp\"
exit $rc
"
    )
}

/// Adds `statusLine` to the settings file `write_claude_hooks` just wrote and writes the script it points at.
pub fn install(cfg: &Config, settings_path: &str) -> Result<(), String> {
    let script = script_path(cfg);
    write_atomic(&script, &script_text(&cfg.home, cfg.user_statusline.as_deref()))?;
    let text = std::fs::read_to_string(settings_path).map_err(|e| format!("read {settings_path} failed: {e}"))?;
    let mut settings: Value = serde_json::from_str(&text).map_err(|e| format!("{settings_path} is not valid JSON: {e}"))?;
    let object = settings.as_object_mut().ok_or_else(|| format!("{settings_path} is not a JSON object"))?;
    object.insert(
        "statusLine".to_string(),
        json!({ "type": "command", "command": format!("sh {}", shell_quote(&script.to_string_lossy())) }),
    );
    let out = serde_json::to_string_pretty(&settings).map_err(|e| format!("encode settings failed: {e}"))?;
    write_atomic(Path::new(settings_path), &out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use std::process::{Command, Stdio};

    fn temp_dir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("shifu-statusline-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// Runs the script the way Claude does: JSON on stdin, output captured.
    fn run(home: &Path, user: Option<&str>, input: &str) -> (String, i32) {
        let script = home.join("statusline.sh");
        std::fs::write(&script, script_text(home, user)).unwrap();
        let mut child = Command::new("sh").arg(&script).stdin(Stdio::piped()).stdout(Stdio::piped()).spawn().unwrap();
        child.stdin.take().unwrap().write_all(input.as_bytes()).unwrap();
        let out = child.wait_with_output().unwrap();
        (String::from_utf8(out.stdout).unwrap(), out.status.code().unwrap())
    }

    #[test]
    fn user_command_reads_status_line_command_only() {
        assert_eq!(user_command(r#"{"statusLine":{"type":"command","command":"echo hi"}}"#).unwrap().as_deref(), Some("echo hi"));
        assert_eq!(user_command(r#"{"model":"x"}"#).unwrap(), None);
        assert_eq!(user_command(r#"{"statusLine":{"type":"command","command":"  "}}"#).unwrap(), None);
        assert!(user_command("{oops").is_err());
    }

    #[test]
    fn quotes_survive_single_quotes_and_spaces() {
        assert_eq!(shell_quote("a b"), "'a b'");
        assert_eq!(shell_quote("it's"), "'it'\\''s'");
    }

    #[test]
    fn input_goes_to_the_user_command_unchanged_and_its_output_is_printed_unchanged() {
        let home = temp_dir("pass");
        let input = "{\"model\":\"中文\",\"rate_limits\":{\"seven_day\":{\"used_percentage\":71,\"resets_at\":1794164400}}}\n";
        // The user command echoes its stdin back with markers, so a changed or missing input shows up in the output.
        let (out, rc) = run(&home, Some("printf '<'; cat; printf '> it'\\''s'"), input);
        assert_eq!(out, format!("<{input}> it's"));
        assert_eq!(rc, 0);
        assert_eq!(std::fs::read_to_string(capture_path(&home)).unwrap(), input);
        let leftovers: Vec<_> = std::fs::read_dir(&home).unwrap().map(|e| e.unwrap().file_name().to_string_lossy().to_string()).collect();
        assert!(leftovers.iter().all(|n| n == "statusline.sh" || n == "claude-statusline.json"), "{leftovers:?}");
        std::fs::remove_dir_all(&home).unwrap();
    }

    #[test]
    fn input_without_rate_limits_leaves_the_capture_alone() {
        let home = temp_dir("keep");
        std::fs::write(capture_path(&home), "OLD").unwrap();
        let (out, _) = run(&home, Some("cat"), "{\"model\":\"x\"}");
        assert_eq!(out, "{\"model\":\"x\"}");
        assert_eq!(std::fs::read_to_string(capture_path(&home)).unwrap(), "OLD");
        std::fs::remove_dir_all(&home).unwrap();
    }

    #[test]
    fn without_a_user_command_nothing_is_printed_but_the_capture_is_kept() {
        let home = temp_dir("none");
        let input = "{\"rate_limits\":{}}";
        let (out, rc) = run(&home, None, input);
        assert_eq!((out.as_str(), rc), ("", 0));
        assert_eq!(std::fs::read_to_string(capture_path(&home)).unwrap(), input);
        std::fs::remove_dir_all(&home).unwrap();
    }

    #[test]
    fn exit_code_of_the_user_command_is_passed_on() {
        let home = temp_dir("rc");
        let (_, rc) = run(&home, Some("cat >/dev/null; exit 3"), "{}");
        assert_eq!(rc, 3);
        std::fs::remove_dir_all(&home).unwrap();
    }

    #[test]
    fn install_adds_status_line_and_keeps_the_hooks() {
        let home = temp_dir("install");
        let cfg = Config { home: home.clone(), socket: "s".into(), shifu_bin: "shifu".into(), user_statusline: Some("echo x".into()) };
        let settings = home.join("claude-hooks.json");
        std::fs::write(&settings, r#"{"hooks":{"Stop":[]}}"#).unwrap();
        install(&cfg, settings.to_str().unwrap()).unwrap();
        let v: Value = serde_json::from_str(&std::fs::read_to_string(&settings).unwrap()).unwrap();
        assert_eq!(v["hooks"], json!({"Stop": []}));
        assert_eq!(v["statusLine"]["type"], "command");
        let command = v["statusLine"]["command"].as_str().unwrap();
        assert_eq!(command, format!("sh '{}'", home.join("statusline.sh").display()));
        assert!(std::fs::read_to_string(home.join("statusline.sh")).unwrap().contains("user='echo x'"));
        std::fs::remove_dir_all(&home).unwrap();
    }
}
