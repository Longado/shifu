//! One agent = one tmux session on the app's own socket. Creating, resuming and ending them.

use crate::app_statusline as statusline;
use crate::agent::{self, Agent, Kind, Res, make_name, now_secs};
use crate::commands::write_claude_hooks;
use crate::launch::{agent_command, model_flags, resume_command};
use crate::menu::record_recent;
use crate::status::Status;
use crate::tmux::{Tmux, run_command};
use std::path::{Path, PathBuf};
use std::process::Command;

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

/// Write to a temp file and rename, so a crash never leaves half a file.
pub fn write_atomic(dest: &Path, text: &str) -> Result<(), String> {
    let tmp = dest.with_extension(format!("json.{}.tmp", std::process::id()));
    std::fs::write(&tmp, text).map_err(|e| format!("write {} failed: {e}", tmp.display()))?;
    std::fs::rename(&tmp, dest).map_err(|e| format!("rename to {} failed: {e}", dest.display()))
}

/// Size of a freshly created session; the first attached terminal resizes it.
const NEW_COLS: &str = "120";
const NEW_ROWS: &str = "40";

/// tmux turns `.` and `:` in a session name into `_`, so `attach -t =<agent name>` would miss it.
pub fn session_name(agent_name: &str) -> String {
    agent_name.replace(['.', ':'], "_")
}

/// `=` makes tmux match the name exactly instead of by prefix.
pub fn exact_target(agent_name: &str) -> String {
    format!("={}", session_name(agent_name))
}

/// Config the app writes before it starts a tmux server (APP-SPEC-A1 §1).
pub fn conf_text() -> String {
    "\
set -g mouse on
set -g status off
set -s escape-time 10
set -g history-limit 50000
set -g default-terminal \"tmux-256color\"
set -as terminal-features \",*:RGB\"
set -as terminal-features \",*:extkeys\"
set -s extended-keys on
set -g allow-passthrough on
set -g set-clipboard on
set -g focus-events on
set -g window-size latest
setw -g aggressive-resize on
# A dead pane must stay so the app can show \"exited\" and resume it; set globally so there is no window
# between creating the pane and flagging it.
setw -g remain-on-exit on
"
    .to_string()
}

/// `new-session` arguments; prints `<pane_id> <window_id>`.
pub fn new_session_args(agent_name: &str, cwd: &str, launch: &str) -> Vec<String> {
    [
        "new-session", "-d", "-P", "-F", "#{pane_id} #{window_id}", "-s", &session_name(agent_name), "-c", cwd, "-e",
        &format!("SHIFU_ID={agent_name}"), "-x", NEW_COLS, "-y", NEW_ROWS, launch,
    ]
    .iter()
    .map(|s| s.to_string())
    .collect()
}

fn write_conf(cfg: &Config) -> Res<()> {
    std::fs::create_dir_all(&cfg.home).map_err(|e| format!("create {} failed: {e}", cfg.home.display()))?;
    let path = cfg.home.join("tmux.conf");
    std::fs::write(&path, conf_text()).map_err(|e| format!("write {} failed: {e}", path.display()))
}

/// Starts the session (and the tmux server if it is not running) and records pane/window ids on the agent file.
/// The agent file must exist before the process starts: its first hook has to find it.
fn start_session(cfg: &Config, record: &Agent, launch: &str) -> Res<()> {
    write_conf(cfg)?;
    let tmux = Tmux::from_env();
    let mut cmd = tmux.server_command();
    cmd.args(new_session_args(&record.name, &record.cwd, launch));
    let created = run_command(cmd)?;
    let (pane, window) = created.split_once(' ').ok_or_else(|| format!("unexpected new-session output: {created:?}"))?;
    // Re-read so a hook that already fired is not overwritten.
    let current = agent::load(&record.name)?.unwrap_or_else(|| record.clone());
    agent::save(&Agent { pane_id: Some(pane.to_string()), window_id: Some(window.to_string()), ..current })
}

/// The shared library writes Claude's hooks file; the app adds its `statusLine` to it (APP-SPEC-B3).
fn write_settings(cfg: &Config, kind: Kind) -> Res<String> {
    let path = write_claude_hooks(kind, &cfg.shifu_bin)?;
    if kind == Kind::Claude {
        statusline::install(cfg, &path)?;
    }
    Ok(path)
}

/// `kind` is `claude`, `codex`, or any other program name. Returns the agent's name.
/// A model or effort picked in the new-agent popover goes into a shell command (quoted), but only ids like
/// `claude-opus-5-5`, `gpt-6.1-sol` or `high` are expected.
fn check_pick(what: &str, value: Option<&str>) -> Res<()> {
    match value {
        // '/' for Kimi's ids (`kimi-code/k3`).
        Some(v) if v.is_empty() || !v.chars().all(|c| c.is_ascii_alphanumeric() || "._-/".contains(c)) => {
            Err(format!("{what} {v:?} is not a model id"))
        }
        _ => Ok(()),
    }
}

pub fn create_agent(cfg: &Config, command: &str, cwd: &str, model: Option<&str>, effort: Option<&str>) -> Res<String> {
    check_pick("model", model)?;
    check_pick("effort", effort)?;
    let dir = Path::new(cwd).canonicalize().map_err(|e| format!("directory {cwd} unusable: {e}"))?;
    if !dir.is_dir() {
        return Err(format!("{} is not a directory", dir.display()));
    }
    let kind = Kind::of_command(command);
    let (existing, _) = agent::list();
    let taken: Vec<String> = existing.into_iter().map(|a| a.name).collect();
    let name = make_name(command, &dir, &taken);
    let settings = write_settings(cfg, kind)?;
    let launch = agent_command(kind, command, &cfg.shifu_bin, &settings, &model_flags(kind, model, effort));
    let now = now_secs();
    let record = Agent {
        name: name.clone(),
        kind,
        command: command.to_string(),
        cwd: dir.to_string_lossy().to_string(),
        created_at: now,
        status: Status::Starting,
        status_since: now,
        window_id: None,
        pane_id: None,
        session_id: None,
        last_message: None,
        model: model.map(str::to_string),
        effort: effort.map(str::to_string),
        label: None,
    };
    agent::save(&record)?;
    if let Err(e) = start_session(cfg, &record, &launch) {
        agent::remove(&name)?;
        return Err(e);
    }
    if let Err(e) = record_recent(&record.cwd) {
        eprintln!("shifu-app: recent directory not recorded: {e}");
    }
    Ok(name)
}

/// Restarts an exited agent: in place when its dead pane is still there, otherwise in a new session.
pub fn resume_agent(cfg: &Config, name: &str) -> Res<()> {
    let record = agent::load(name)?.ok_or_else(|| format!("no agent named {name}"))?;
    let tmux = Tmux::from_env();
    let panes = tmux.panes();
    if record.effective_status(&panes) != Status::Exited {
        return Err(format!("{name} is still running"));
    }
    let settings = write_settings(cfg, record.kind)?;
    let pick = model_flags(record.kind, record.model.as_deref(), record.effort.as_deref());
    let launch = resume_command(record.kind, &record.command, &cfg.shifu_bin, &settings, record.session_id.as_deref(), &pick);
    // Write before the process starts so its first hook is not overwritten by this write.
    let restarted = Agent { status: Status::Starting, status_since: now_secs(), ..record.clone() };
    agent::save(&restarted)?;
    let live_pane = record.pane_id.as_ref().filter(|p| panes.contains_key(*p));
    match live_pane {
        Some(pane) => {
            let id = format!("SHIFU_ID={name}");
            tmux.run(["respawn-pane", "-k", "-t", pane, "-c", &record.cwd, "-e", &id, &launch]).map(|_| ())
        }
        None => start_session(cfg, &restarted, &launch),
    }
}

/// Deletes the record first (the hooks of a dying process must not recreate state), then the session.
pub fn kill_agent(name: &str) -> Res<()> {
    let record = agent::load(name)?.ok_or_else(|| format!("no agent named {name}"))?;
    agent::remove(name)?;
    let tmux = Tmux::from_env();
    let session_alive = record.pane_id.as_ref().is_some_and(|p| tmux.panes().contains_key(p));
    if session_alive {
        tmux.run(["kill-session", "-t", &exact_target(name)])?;
    }
    Ok(())
}

/// Shift+Enter in the CSI u form, which Claude Code and Codex both read as "new line".
const SHIFT_ENTER: &str = "\x1b[13;2u";

/// Written straight into the agent's pane. Sent through the attached terminal instead, tmux parses it as a key and,
/// for programs that did not ask for extended keys (Codex), turns it into a plain Enter, which submits.
pub fn send_shift_enter(name: &str) -> Res<()> {
    let target = format!("{}:", exact_target(name));
    Tmux::from_env().run(["send-keys", "-t", &target, "-l", SHIFT_ENTER]).map(|_| ())
}

/// Leaves tmux's scroll mode, if the pane is in it. A wheel turn over a program that does not take the mouse
/// (CodeBuddy) starts that mode, and then Enter and the arrows move tmux's cursor instead of reaching the program.
/// The inner command names the pane too: left bare it goes to whichever session tmux thinks is current, which with
/// several agents open is another one, and that one answers "not in a mode".
pub fn leave_scroll(name: &str) -> Res<()> {
    let target = format!("{}:", exact_target(name));
    let cancel = format!("send-keys -t '{target}' -X cancel");
    Tmux::from_env().run(["if-shell", "-F", "-t", &target, "#{pane_in_mode}", &cancel]).map(|_| ())
}

pub fn reveal(cwd: &str) -> Res<()> {
    let mut cmd = Command::new("open");
    cmd.args(["-R", cwd]);
    run_command(cmd).map(|_| ())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn session_name_follows_tmux_renaming_of_dots_and_colons() {
        assert_eq!(session_name("claude-proj"), "claude-proj");
        assert_eq!(session_name("claude-my.proj"), "claude-my_proj");
        assert_eq!(exact_target("codex-a.b"), "=codex-a_b");
    }

    #[test]
    fn new_session_args_pass_cwd_id_and_launch_verbatim() {
        let got = new_session_args("codex-a.b", "/tmp/中文 目录", "'codex' -c 'x'");
        let want = [
            "new-session", "-d", "-P", "-F", "#{pane_id} #{window_id}", "-s", "codex-a_b", "-c", "/tmp/中文 目录", "-e",
            "SHIFU_ID=codex-a.b", "-x", "120", "-y", "40", "'codex' -c 'x'",
        ];
        assert_eq!(got, want);
    }

    #[test]
    fn conf_has_the_required_options_and_no_status_bar_or_sidebar() {
        let conf = conf_text();
        for line in [
            "set -g mouse on",
            "set -g status off",
            "set -s escape-time 10",
            "set -g history-limit 50000",
            "set -g default-terminal \"tmux-256color\"",
            "set -as terminal-features \",*:RGB\"",
            "set -as terminal-features \",*:extkeys\"",
            "set -s extended-keys on",
            "set -g allow-passthrough on",
            "set -g set-clipboard on",
            "set -g focus-events on",
            "set -g window-size latest",
            "setw -g aggressive-resize on",
            "setw -g remain-on-exit on",
        ] {
            assert!(conf.lines().any(|l| l == line), "missing: {line}");
        }
        assert!(!conf.contains("sidebar") && !conf.contains("status-right"));
    }
}
