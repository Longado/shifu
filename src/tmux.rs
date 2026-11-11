//! 和 tmux 打交道：专用 socket、专用配置、起服务、窗格查询。

use crate::agent::{PaneInfo, Res, shifu_home};
use crate::launch::{sh_quote, tmux_dq};
use crate::tabs::{window_status_current_format, window_status_format};
use std::collections::HashMap;
use std::ffi::OsStr;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

pub const SESSION: &str = "shifu";
pub const SIDEBAR_WIDTH: &str = "32";

#[derive(Clone, Debug)]
pub struct Tmux {
    bin: PathBuf,
    pub socket: String,
    conf: PathBuf,
}

pub fn find_tmux_bin() -> PathBuf {
    if let Some(p) = std::env::var_os("SHIFU_TMUX").filter(|p| !p.is_empty()) {
        return PathBuf::from(p);
    }
    for candidate in ["/opt/homebrew/bin/tmux", "/usr/local/bin/tmux"] {
        if Path::new(candidate).exists() {
            return PathBuf::from(candidate);
        }
    }
    PathBuf::from("tmux")
}

impl Tmux {
    pub fn from_env() -> Tmux {
        let socket = std::env::var("SHIFU_SOCKET").ok().filter(|s| !s.is_empty()).unwrap_or_else(|| "shifu".to_string());
        Tmux { bin: find_tmux_bin(), socket, conf: shifu_home().join("tmux.conf") }
    }

    fn command(&self) -> Command {
        let mut cmd = Command::new(&self.bin);
        cmd.arg("-L").arg(&self.socket).arg("-f").arg(&self.conf);
        cmd
    }

    /// 可能起新服务的那条命令：服务里所有窗口都继承这里的环境，所以在这里清掉父会话的标记（见 `server_env_changes`）。
    pub fn server_command(&self) -> Command {
        let mut cmd = self.command();
        let (removed, path) = server_env_changes(&std::env::vars().collect::<Vec<_>>());
        for key in removed {
            cmd.env_remove(key);
        }
        if let Some(path) = path {
            cmd.env("PATH", path);
        }
        cmd
    }

    /// 跑一条 tmux 命令，成功返回去掉末尾换行的 stdout。
    pub fn run<I, S>(&self, args: I) -> Res<String>
    where
        I: IntoIterator<Item = S>,
        S: AsRef<OsStr>,
    {
        let mut cmd = self.command();
        cmd.args(args);
        run_command(cmd)
    }

    /// 当前进程就在这个 tmux 服务里（`TMUX` 的 socket 路径以 `/<socket>` 结尾）。
    pub fn inside(&self) -> bool {
        let var = std::env::var("TMUX").unwrap_or_default();
        var.split(',').next().is_some_and(|path| path.ends_with(&format!("/{}", self.socket)))
    }

    /// 用 attach 替换当前进程；只在失败时返回错误。清掉 `TMUX` 以允许从别的 tmux 里嵌套打开。
    pub fn exec_attach(&self) -> String {
        use std::os::unix::process::CommandExt;
        let err = self.command().args(["attach", "-t", &format!("={SESSION}")]).env_remove("TMUX").exec();
        format!("无法 attach tmux: {err}")
    }

    /// 全部窗格；服务没起时返回空表（所有窗格当作不存在）。
    pub fn panes(&self) -> HashMap<String, PaneInfo> {
        let out = self.run(["list-panes", "-a", "-F", "#{pane_id} #{pane_dead} #{window_id} #{pane_index}"]).unwrap_or_default();
        parse_panes(&out)
    }

    /// 确保 `shifu` 会话在；没起就写配置、清环境、起服务，并建 home 窗口。
    pub fn ensure_session(&self, cwd: &Path, shifu: &Path) -> Res<()> {
        let shifu_str = shifu.to_string_lossy();
        let conf = conf_text(&shifu_str, &self.bin.to_string_lossy());
        if self.run(["has-session", "-t", &format!("={SESSION}")]).is_ok() {
            // 服务已在跑：升级后配置变了就写回并重新载入，不用重启 tmux（agent 照跑）
            if fs::read_to_string(&self.conf).ok().as_deref() != Some(conf.as_str()) {
                fs::write(&self.conf, &conf).map_err(|e| format!("写 {} 失败: {e}", self.conf.display()))?;
                self.run(["source-file".to_string(), self.conf.to_string_lossy().to_string()])?;
            }
            return Ok(());
        }
        let home = shifu_home();
        fs::create_dir_all(&home).map_err(|e| format!("创建 {} 失败: {e}", home.display()))?;
        fs::write(&self.conf, &conf).map_err(|e| format!("写 {} 失败: {e}", self.conf.display()))?;

        let mut cmd = self.server_command();
        cmd.args(["new-session", "-d", "-P", "-F", "#{pane_id}", "-s", SESSION, "-n", "home", "-c"]).arg(cwd);
        // 用当前终端大小建，之后 attach 时侧栏宽度不会被按比例拉伸
        if let Ok((w, h)) = crossterm_size() {
            cmd.args(["-x", &w.to_string(), "-y", &h.to_string()]);
        }
        let shell_pane = run_command(cmd)?;
        self.run(["split-window", "-h", "-b", "-l", SIDEBAR_WIDTH, "-t", &shell_pane, &sidebar_command(&shifu_str)])?;
        self.run(["select-pane", "-t", &shell_pane])?;
        Ok(())
    }
}

/// 起 tmux 服务前要清掉的环境：所有窗口都继承服务的环境，所以只在这里清一次。
/// 返回（要删的变量名，改过的 PATH；PATH 不用改时为 None）。
///
/// - 从 Claude Code 会话里起服务时会带上 CLAUDECODE、CLAUDE_CODE_CHILD_SESSION、CLAUDE_PID、
///   CLAUDE_EFFORT 等；新开的 Claude 会因此停存会话记录、沿用父会话的设置（11-06 实测）。
///   CLAUDE_CONFIG_DIR、CODEX_HOME 是用户自己的配置位置，保留。
/// - 从 Orca 终端里起服务时会带上 ORCA_AGENT_HOOK_* 等，Shifu 里的 agent 会把状态报给 Orca 那个窗格；
///   Orca 的 tmux 替身目录在 PATH 最前面，去掉 ORCA_ 变量后它会直接报错，所以一并从 PATH 拿掉。
fn server_env_changes(vars: &[(String, String)]) -> (Vec<String>, Option<String>) {
    let drop_var = |key: &str| {
        let claude = key == "CLAUDECODE" || (key.starts_with("CLAUDE_") && key != "CLAUDE_CONFIG_DIR");
        let codex = key.starts_with("CODEX_") && key != "CODEX_HOME";
        claude || codex || key.starts_with("ORCA_")
    };
    let removed = vars.iter().map(|(k, _)| k).filter(|k| drop_var(k)).cloned().collect();
    let path = vars.iter().find(|(k, _)| k == "PATH").and_then(|(_, p)| {
        let kept: Vec<&str> = p.split(':').filter(|d| !d.trim_end_matches('/').ends_with("/.orca/claude-agent-teams-bin")).collect();
        let joined = kept.join(":");
        (joined != *p).then_some(joined)
    });
    (removed, path)
}

fn crossterm_size() -> std::io::Result<(u16, u16)> {
    let (w, h) = ratatui::crossterm::terminal::size()?;
    if w == 0 || h == 0 { Err(std::io::Error::other("终端大小为 0")) } else { Ok((w, h)) }
}

pub fn run_command(mut cmd: Command) -> Res<String> {
    let out = cmd.output().map_err(|e| format!("无法运行 {:?}: {e}", cmd.get_program()))?;
    if out.status.success() {
        Ok(String::from_utf8_lossy(&out.stdout).trim_end_matches('\n').to_string())
    } else {
        let err = String::from_utf8_lossy(&out.stderr);
        Err(format!("{:?} 失败: {}", cmd.get_program(), err.trim()))
    }
}

pub fn sidebar_command(shifu: &str) -> String {
    format!("{} sidebar", sh_quote(shifu))
}

/// `command-prompt` 用的模板：把两个回答交给 `shifu new`。
/// 回答里含 `'` `"` `$` `;` 时会坏掉，这条路径只用于键盘输入的常规目录。
pub fn new_prompt_template(shifu: &str) -> String {
    format!("run-shell {}", tmux_dq(&format!("{} new '%1' '%2' --focus", sh_quote(shifu))))
}

pub fn conf_text(shifu: &str, tmux_bin: &str) -> String {
    // 拖标签：tmux 自带的 `MouseDrag1Status swap-window -d -t=` 在 3.6a 里不动（鼠标事件里 -s 和 -t 都是鼠标下那一页）。
    // 改成按下时记住这一页，松开时用 run-shell 拿鼠标下那一页的 id 对调。11-06 用模拟鼠标实测可用。
    let t = sh_quote(tmux_bin);
    // 像浏览器标签一样「插到那个位置」：往左拖插在目标前面，往右拖插在后面，其他页顺次挪开。
    // 不用 shell 变量：tmux 会在双引号里展开 `$`。
    let tm = format!("{t} -S '#{{socket_path}}'");
    let src = "'#{@shifu_drag_src}'";
    let drag_end = format!(
        "[ {src} = '#{{window_id}}' ] || {{ if [ '#{{@shifu_drag_idx}}' -gt '#{{window_index}}' ]; then {tm} move-window -b -s {src} -t '#{{window_id}}'; else {tm} move-window -a -s {src} -t '#{{window_id}}'; fi && {tm} select-window -t {src}; }}"
    );
    let drag_end = tmux_dq(&drag_end);
    let jump = tmux_dq(&format!("{} jump", sh_quote(shifu)));
    let new_binding = tmux_dq(&new_prompt_template(shifu));
    let tally = tmux_dq(&format!("#({} tally) ", sh_quote(shifu)));
    let tab = tmux_dq(&window_status_format());
    let tab_current = tmux_dq(&window_status_current_format());
    format!(
        r##"set -g mouse on
set -s escape-time 10
set -g history-limit 50000
set -g default-terminal "tmux-256color"
set -as terminal-features ",*:RGB"
set -s extended-keys on
set -as terminal-features ",*:extkeys"
set -g allow-passthrough on
set -g focus-events on
set -g set-clipboard on
set -g renumber-windows on
set -g base-index 1
set -g status-style "bg=default,fg=colour244"
set -g status-left " shifu "
set -g status-interval 2
set -g status-right {tally}
set -g window-status-format {tab}
set -g window-status-current-format {tab_current}
set -g pane-border-style "fg=colour238"
set -g pane-active-border-style "fg=colour244"
bind j run-shell {jump}
bind s select-pane -t :.0
bind -n MouseDown1Status select-window -t= \; set -gF @shifu_drag_src "#{{window_id}}" \; set -gF @shifu_drag_idx "#{{window_index}}"
bind -n MouseDragEnd1Status run-shell {drag_end}
bind n command-prompt -p "agent:,目录:" -I "claude,#{{pane_current_path}}" {new_binding}
set-hook -g client-attached "resize-pane -t :.0 -x {SIDEBAR_WIDTH}"
set-hook -g window-resized "resize-pane -t :.0 -x {SIDEBAR_WIDTH}"
set-hook -g after-select-window "resize-pane -t :.0 -x {SIDEBAR_WIDTH}"
"##
    )
}

fn parse_panes(out: &str) -> HashMap<String, PaneInfo> {
    out.lines()
        .filter_map(|line| {
            let mut parts = line.split(' ');
            let pane = parts.next()?;
            let dead = parts.next()? == "1";
            let window_id = parts.next()?;
            let index = parts.next()?.parse().ok()?;
            Some((pane.to_string(), PaneInfo { dead, window_id: window_id.to_string(), index }))
        })
        .collect()
}

/// 一页（一个 tmux 窗口）里的窗格，按 pane_index 从小到大。第 0 个永远是侧栏。
pub fn page_panes(panes: &HashMap<String, PaneInfo>, window: &str) -> Vec<(u32, String)> {
    let mut found: Vec<(u32, String)> =
        panes.iter().filter(|(_, info)| info.window_id == window).map(|(id, info)| (info.index, id.clone())).collect();
    found.sort();
    found
}

#[cfg(test)]
mod tests {
    use super::*;

    fn vars(pairs: &[(&str, &str)]) -> Vec<(String, String)> {
        pairs.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect()
    }

    #[test]
    fn server_env_drops_parent_agent_and_orca_markers() {
        let env = vars(&[
            ("CLAUDECODE", "1"),
            ("CLAUDE_CODE_CHILD_SESSION", "1"),
            ("CLAUDE_EFFORT", "xhigh"),
            ("CLAUDE_PID", "8791"),
            ("CLAUDE_CONFIG_DIR", "/u/.claude-alt"),
            ("CODEX_THREAD_ID", "t"),
            ("CODEX_HOME", "/u/.codex"),
            ("ORCA_AGENT_HOOK_ENDPOINT", "http://x"),
            ("ORCA_TAB_ID", "7"),
            ("HOME", "/u"),
            ("PATH", "/u/.orca/claude-agent-teams-bin:/Applications/Orca.app/Contents/Resources/bin:/opt/homebrew/bin:/usr/bin"),
        ]);
        let (mut removed, path) = server_env_changes(&env);
        removed.sort();
        assert_eq!(
            removed,
            ["CLAUDECODE", "CLAUDE_CODE_CHILD_SESSION", "CLAUDE_EFFORT", "CLAUDE_PID", "CODEX_THREAD_ID", "ORCA_AGENT_HOOK_ENDPOINT", "ORCA_TAB_ID"]
        );
        assert_eq!(path.as_deref(), Some("/Applications/Orca.app/Contents/Resources/bin:/opt/homebrew/bin:/usr/bin"));
    }

    #[test]
    fn server_env_leaves_clean_shell_alone() {
        let env = vars(&[("HOME", "/u"), ("PATH", "/opt/homebrew/bin:/usr/bin")]);
        let (removed, path) = server_env_changes(&env);
        assert!(removed.is_empty());
        assert_eq!(path, None);
    }

    #[test]
    fn parse_panes_reads_dead_flag_window_and_index() {
        let m = parse_panes("%1 0 @1 0\n%5 1 @3 2\n\nbad line\n%6 0 @3");
        assert_eq!(m.len(), 2);
        assert!(!m["%1"].dead);
        assert!(m["%5"].dead);
        assert_eq!(m["%5"].window_id, "@3");
        assert_eq!(m["%5"].index, 2);
    }

    #[test]
    fn page_panes_sorted_by_index_for_one_window() {
        let m = parse_panes("%9 0 @2 2\n%3 0 @2 0\n%4 0 @5 0\n%7 0 @2 1");
        assert_eq!(page_panes(&m, "@2"), [(0, "%3".to_string()), (1, "%7".to_string()), (2, "%9".to_string())]);
        assert!(page_panes(&m, "@8").is_empty());
    }

    #[test]
    fn prompt_template_shape() {
        assert_eq!(new_prompt_template("/p/shifu"), "run-shell \"'/p/shifu' new '%1' '%2' --focus\"");
    }

    #[test]
    fn conf_has_required_bindings() {
        let c = conf_text("/p/shifu", "/opt/tmux");
        // 拖标签：按下时记住按的是哪一页，松开时和鼠标下面那一页对调，焦点跟着被拖的页
        assert!(c.contains(
            "bind -n MouseDown1Status select-window -t= \\; set -gF @shifu_drag_src \"#{window_id}\" \\; set -gF @shifu_drag_idx \"#{window_index}\"\n"
        ));
        let end = c.lines().find(|l| l.starts_with("bind -n MouseDragEnd1Status run-shell ")).expect("drag end binding");
        // 拖到左边插在前面（-b），拖到右边插在后面（-a），拖回自己不动，焦点跟着被拖的页
        for part in [
            "[ '#{@shifu_drag_src}' = '#{window_id}' ] ||",
            "if [ '#{@shifu_drag_idx}' -gt '#{window_index}' ]",
            "'/opt/tmux' -S '#{socket_path}' move-window -b -s '#{@shifu_drag_src}' -t '#{window_id}'",
            "'/opt/tmux' -S '#{socket_path}' move-window -a -s '#{@shifu_drag_src}' -t '#{window_id}'",
            "'/opt/tmux' -S '#{socket_path}' select-window -t '#{@shifu_drag_src}'",
        ] {
            assert!(end.contains(part), "{part}");
        }
        assert!(!end.contains("\\$") && !end.contains("swap-window"));
        assert!(!c.contains("MouseDrag1Status"));
        assert!(c.contains("bind j run-shell \"'/p/shifu' jump\"\n"));
        assert!(c.contains("bind s select-pane -t :.0\n"));
        assert!(c.contains(
            "bind n command-prompt -p \"agent:,目录:\" -I \"claude,#{pane_current_path}\" \"run-shell \\\"'/p/shifu' new '%1' '%2' --focus\\\"\"\n"
        ));
        // 窗口大小变了（attach、拖动窗口、切到别的 agent 窗口）都把侧栏拉回固定宽度，不按比例拉伸
        for hook in ["client-attached", "window-resized", "after-select-window"] {
            assert!(c.contains(&format!("set-hook -g {hook} \"resize-pane -t :.0 -x 32\"\n")), "{hook}");
        }
        assert!(!c.contains("pane-base-index"));
        // 标签栏：每页的圆点和页名，右边是「在等你」的数量，每 2 秒刷新
        assert!(c.contains("set -g status-interval 2\n"));
        assert!(c.contains("set -g status-right \"#('/p/shifu' tally) \"\n"));
        let tab_line = c.lines().find(|l| l.starts_with("set -g window-status-format ")).unwrap();
        assert!(tab_line.contains("#{P:") && tab_line.contains("#{@shifu_status}") && tab_line.ends_with("#W\""), "{tab_line}");
        let current_line = c.lines().find(|l| l.starts_with("set -g window-status-current-format ")).unwrap();
        assert!(current_line.contains("#{P:") && current_line.ends_with("#[fg=colour255,bold] #W\""), "{current_line}");
        assert!(c.contains("set -g base-index 1\n"));
    }
}
