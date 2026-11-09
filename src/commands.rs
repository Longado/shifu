//! 命令实现：new、ls、jump、打开工作台。

use crate::actions::{self, Action};
use crate::agent::{self, Agent, Kind, Res, make_name, now_secs, shifu_home};
use crate::launch::{agent_command, claude_hooks_json, codebuddy_hooks_json, kimi_config};
use crate::menu::record_recent;
use crate::status::{Status, format_age};
use crate::tabs::tally_text;
use crate::tmux::{SESSION, SIDEBAR_WIDTH, Tmux, sidebar_command};
use std::path::PathBuf;

#[derive(Debug, PartialEq)]
pub struct NewArgs {
    pub agent: String,
    pub dir: Option<String>,
    pub focus: bool,
    pub extra: Vec<String>,
}

pub fn parse_new(args: &[String]) -> Res<NewArgs> {
    let mut positional: Vec<String> = Vec::new();
    let mut focus = false;
    let mut extra = Vec::new();
    let mut iter = args.iter();
    while let Some(arg) = iter.next() {
        match arg.as_str() {
            "--" => {
                extra.extend(iter.by_ref().cloned());
            }
            "--focus" => focus = true,
            _ => positional.push(arg.clone()),
        }
    }
    let mut positional = positional.into_iter();
    let agent = positional.next().filter(|a| !a.is_empty()).ok_or("用法: shifu new <agent> [目录] [-- 额外参数...]")?;
    let dir = positional.next().filter(|d| !d.is_empty());
    if let Some(extra_arg) = positional.next() {
        return Err(format!("多余的参数 {extra_arg:?}；额外参数请放在 -- 后面"));
    }
    Ok(NewArgs { agent, dir, focus, extra })
}

/// 相对路径按当前目录解析，`~/` 展开；不存在就报错。
fn resolve_dir(dir: Option<&str>) -> Res<PathBuf> {
    let cwd = std::env::current_dir().map_err(|e| format!("取不到当前目录: {e}"))?;
    let raw = match dir {
        None => return Ok(cwd),
        Some(d) => d,
    };
    let expanded = match raw.strip_prefix("~/") {
        Some(rest) => PathBuf::from(std::env::var_os("HOME").unwrap_or_default()).join(rest),
        None => PathBuf::from(raw),
    };
    let full = cwd.join(expanded);
    let canonical = full.canonicalize().map_err(|e| format!("目录 {} 不可用: {e}", full.display()))?;
    if canonical.is_dir() { Ok(canonical) } else { Err(format!("{} 不是目录", canonical.display())) }
}

pub fn shifu_exe() -> Res<PathBuf> {
    std::env::current_exe().and_then(|p| p.canonicalize()).map_err(|e| format!("找不到 shifu 自己的路径: {e}"))
}

pub fn cmd_new(args: &[String]) -> Res<()> {
    let parsed = parse_new(args)?;
    let dir = resolve_dir(parsed.dir.as_deref())?;
    let shifu = shifu_exe()?;
    let shifu_str = shifu.to_string_lossy().to_string();
    let tmux = Tmux::from_env();
    let caller_cwd = std::env::current_dir().unwrap_or_else(|_| dir.clone());
    tmux.ensure_session(&caller_cwd, &shifu)?;

    let kind = Kind::of_command(&parsed.agent);
    let (existing, _) = agent::list();
    let taken: Vec<String> = existing.into_iter().map(|a| a.name).collect();
    let name = make_name(&parsed.agent, &dir, &taken);

    let settings_path = write_claude_hooks(kind, &shifu_str)?;
    let launch = agent_command(kind, &parsed.agent, &shifu_str, &settings_path, &parsed.extra);

    let now = now_secs();
    let record = Agent {
        name: name.clone(),
        kind,
        command: parsed.agent.clone(),
        cwd: dir.to_string_lossy().to_string(),
        created_at: now,
        status: Status::Starting,
        status_since: now,
        window_id: None,
        pane_id: None,
        session_id: None,
        last_message: None,
        model: None,
        effort: None,
        label: None,
    };
    agent::save(&record)?;
    let window_id = match open_agent_window(&tmux, &shifu_str, &record, &launch) {
        Ok((window_id, _)) => window_id,
        Err(e) => {
            agent::remove(&name)?;
            return Err(e);
        }
    };
    if let Err(e) = record_recent(&record.cwd) {
        eprintln!("shifu: 最近目录没记上: {e}");
    }

    if parsed.focus {
        tmux.run(["select-window", "-t", &window_id])?;
    } else {
        println!("已开 {name}。运行 shifu 打开工作台。");
    }
    Ok(())
}

/// Claude 的 hooks 文件（每次启动前重写，内容固定）；返回它的路径。其他 kind 不用，路径照给。
/// Claude 和 CodeBuddy 各写一份自己的 hooks 文件（Claude 那份应用还会加 statusLine，不能给 CodeBuddy 用）。
/// Kimi 的 hooks 写进用户自己的 `~/.kimi/config.toml`（见 `launch::kimi_config`），返回那个路径。
pub fn write_claude_hooks(kind: Kind, shifu: &str) -> Res<String> {
    let (file, text) = match kind {
        Kind::CodeBuddy => ("codebuddy-hooks.json", Some(pretty(&codebuddy_hooks_json(shifu))?)),
        Kind::Claude => ("claude-hooks.json", Some(pretty(&claude_hooks_json(shifu))?)),
        Kind::Kimi => return install_kimi_hooks(shifu),
        _ => ("claude-hooks.json", None),
    };
    let path = shifu_home().join(file);
    if let Some(text) = text {
        std::fs::write(&path, text).map_err(|e| format!("写 {} 失败: {e}", path.display()))?;
    }
    Ok(path.to_string_lossy().to_string())
}

fn pretty(value: &serde_json::Value) -> Res<String> {
    serde_json::to_string_pretty(value).map_err(|e| e.to_string())
}

/// `<KIMI_SHARE_DIR or ~/.kimi>/config.toml` gets Shifu's hooks, written only when they are missing or point at
/// another shifu. The first time, the file as it was is kept beside it as `config.toml.before-shifu`.
fn install_kimi_hooks(shifu: &str) -> Res<String> {
    let dir = match std::env::var("KIMI_SHARE_DIR").ok().filter(|d| !d.is_empty()) {
        Some(d) => PathBuf::from(d),
        None => PathBuf::from(std::env::var("HOME").unwrap_or_default()).join(".kimi"),
    };
    let path = dir.join("config.toml");
    let shown = path.display();
    // Empty when Kimi has never been set up; it then says no model is set.
    let user = match std::fs::read_to_string(&path) {
        Ok(text) => text,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(e) => return Err(format!("读 {shown} 失败: {e}")),
    };
    if let Some(text) = kimi_config(&user, shifu)? {
        let backup = dir.join("config.toml.before-shifu");
        if !user.is_empty() && !backup.exists() {
            std::fs::write(&backup, &user).map_err(|e| format!("备份 {shown} 失败: {e}"))?;
        }
        std::fs::create_dir_all(&dir).map_err(|e| format!("建 {} 失败: {e}", dir.display()))?;
        std::fs::write(&path, text).map_err(|e| format!("写 {shown} 失败: {e}"))?;
    }
    Ok(path.to_string_lossy().to_string())
}

/// 给 agent 开一个新窗口（§4.2 第 2–5 步）：起命令、设窗格选项、左边开侧栏、把窗口号和窗格号写回文件。
/// 失败时关掉半成品窗口再返回错误；agent 文件留不留由调用方决定。返回（window_id, pane_id）。
pub fn open_agent_window(tmux: &Tmux, shifu: &str, record: &Agent, launch: &str) -> Res<(String, String)> {
    let name = &record.name;
    let created = tmux.run([
        "new-window",
        "-d",
        "-P",
        "-F",
        "#{window_id} #{pane_id}",
        "-t",
        &format!("={SESSION}:"),
        "-n",
        name,
        "-c",
        &record.cwd,
        "-e",
        &format!("SHIFU_ID={name}"),
        launch,
    ])?;
    let (window_id, pane_id) = match created.split_once(' ') {
        Some((w, p)) => (w.to_string(), p.to_string()),
        None => return Err(format!("tmux new-window 输出看不懂: {created:?}")),
    };

    let finish = || -> Res<()> {
        // 重新读一遍再写：别盖掉 hook 刚写的状态
        let current = agent::load(name)?.unwrap_or_else(|| record.clone());
        agent::save(&Agent { window_id: Some(window_id.clone()), pane_id: Some(pane_id.clone()), ..current })?;
        tmux.run(["set-option", "-p", "-t", &pane_id, "@shifu_status", agent::initial_tab_status(record.kind).name()])?;
        tmux.run(["set-option", "-p", "-t", &pane_id, "remain-on-exit", "on"])?;
        tmux.run(["set-option", "-w", "-t", &window_id, "allow-rename", "off"])?;
        tmux.run(["set-option", "-w", "-t", &window_id, "automatic-rename", "off"])?;
        tmux.run(["split-window", "-h", "-b", "-l", SIDEBAR_WIDTH, "-t", &pane_id, &sidebar_command(shifu)])?;
        tmux.run(["select-pane", "-t", &pane_id])?;
        Ok(())
    };
    if let Err(e) = finish() {
        // 半成品窗口留着只会让侧栏里多一个不能用的 agent
        let _ = tmux.run(["kill-window", "-t", &window_id]);
        return Err(format!("agent 窗口没建好（命令可能一启动就退出了）: {e}"));
    }
    Ok((window_id, pane_id))
}

pub fn cmd_ls() -> Res<()> {
    let (agents, problems) = agent::list();
    for p in problems {
        eprintln!("shifu: {p}");
    }
    if agents.is_empty() {
        println!("还没有 agent。用 shifu new claude 开一个。");
        return Ok(());
    }
    let panes = Tmux::from_env().panes();
    let now = now_secs();
    for entry in agent::sorted_entries(agents, &panes) {
        let a = &entry.agent;
        let label = entry.status.label();
        // 状态文字是中文，每字占两格，补到 8 格宽对齐
        let pad = " ".repeat(8usize.saturating_sub(label.chars().count() * 2));
        println!(
            "{:<28} {:<6} {}{} {:<6} {}",
            a.name,
            a.kind.as_str(),
            label,
            pad,
            format_age(now.saturating_sub(a.status_since)),
            a.cwd
        );
    }
    Ok(())
}

pub fn cmd_jump() -> Res<()> {
    let tmux = Tmux::from_env();
    let (agents, _) = agent::list();
    let entries = agent::sorted_entries(agents, &tmux.panes());
    match entries.iter().find(|e| e.status.is_waiting()) {
        Some(entry) => actions::jump(&tmux, entry.window.as_deref(), &entry.agent),
        None => {
            let message = "没有在等你的 agent";
            if tmux.run(["display-message", message]).is_err() {
                println!("{message}");
            }
            Ok(())
        }
    }
}

/// 标签栏右边的计数：多少个在等你批准、多少个在等你回复。要快，不查 git。
pub fn cmd_tally() -> Res<()> {
    let (agents, _) = agent::list();
    let entries = agent::sorted_entries(agents, &Tmux::from_env().panes());
    let count = |status: Status| entries.iter().filter(|e| e.status == status).count();
    print!("{}", tally_text(count(Status::NeedsApproval), count(Status::NeedsReply)));
    Ok(())
}

/// `shifu act <动作> <名字>`：菜单和按键共用的动作入口。
pub fn cmd_act(args: &[String]) -> Res<()> {
    let (Some(action), Some(name), None) = (args.first(), args.get(1), args.get(3)) else {
        return Err("用法: shifu act jump|right|below|own|even|finder|resume|close <名字> [页]".to_string());
    };
    let action = Action::parse(action).ok_or_else(|| format!("不认识的动作 {action:?}"))?;
    let tmux = Tmux::from_env();
    let shifu = shifu_exe()?.to_string_lossy().to_string();
    // 菜单会把打开它的那一页带进来；命令行直接调用时才去问 tmux 当前页
    let page = match args.get(2) {
        Some(page) => page.clone(),
        None => actions::current_window(&tmux)?,
    };
    actions::run(&tmux, &shifu, action, name, &page)
}

pub fn cmd_open() -> Res<()> {
    let tmux = Tmux::from_env();
    let cwd = std::env::current_dir().map_err(|e| format!("取不到当前目录: {e}"))?;
    tmux.ensure_session(&cwd, &shifu_exe()?)?;
    if tmux.inside() {
        println!("已经在 Shifu 里了。prefix j 跳到等你的 agent，prefix n 新建。");
        return Ok(());
    }
    Err(tmux.exec_attach())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(items: &[&str]) -> Vec<String> {
        items.iter().map(|x| x.to_string()).collect()
    }

    #[test]
    fn parse_new_basic() {
        let p = parse_new(&s(&["claude"])).unwrap();
        assert_eq!(p, NewArgs { agent: "claude".into(), dir: None, focus: false, extra: vec![] });
    }

    #[test]
    fn parse_new_dir_focus_extra() {
        let p = parse_new(&s(&["sh", "/tmp/x/中文 目录", "--focus", "--", "-c", "sleep 600"])).unwrap();
        assert_eq!(p.agent, "sh");
        assert_eq!(p.dir.as_deref(), Some("/tmp/x/中文 目录"));
        assert!(p.focus);
        assert_eq!(p.extra, s(&["-c", "sleep 600"]));
    }

    #[test]
    fn parse_new_tmux_binding_form_with_empty_dir() {
        let p = parse_new(&s(&["claude", "", "--focus"])).unwrap();
        assert_eq!(p.dir, None);
        assert!(p.focus);
    }

    #[test]
    fn parse_new_errors() {
        assert!(parse_new(&s(&[])).is_err());
        assert!(parse_new(&s(&["claude", "/a", "/b"])).is_err());
    }

    #[test]
    fn focus_after_double_dash_belongs_to_agent() {
        let p = parse_new(&s(&["codex", "--", "--focus"])).unwrap();
        assert!(!p.focus);
        assert_eq!(p.extra, s(&["--focus"]));
    }
}
