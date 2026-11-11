//! 给 Shifu 外面的人用的两条命令：`shifu status` 看每个对话在干什么，`shifu tell` 往一个对话里发一句话。
//! 项目经理（Shifu 里常驻的那个 Claude）和 Grok Bot 这类能在本机跑命令的助手都靠它们跟 Shifu 协作。
//! 没设 SHIFU_HOME 时看的是桌面应用（`~/.shifu/app`，tmux 服务 `shifu-app`），不是旧的终端版。

use crate::agent::{self, Res, now_secs, shifu_home};
use crate::status::{Status, format_age};
use crate::tmux::Tmux;
use crate::topic::{clean_title, hostname, pane_titles};
use serde::Serialize;
use serde_json::Value;
use std::path::Path;

#[derive(Debug, Clone, Serialize)]
pub struct Row {
    /// Shifu 内部名（`claude-Workspace-3`），`tell` 用它最稳。
    pub name: String,
    pub kind: String,
    /// 显示名：用户起的名字，否则对话话题，否则内部名。
    pub title: String,
    pub task: Option<String>,
    pub status: Status,
    /// 进入当前状态的时刻（Unix 秒）。
    pub since: u64,
    pub last_message: Option<String>,
    pub cwd: String,
    #[serde(skip)]
    pub pane: Option<String>,
}

/// 没设 SHIFU_HOME 就指向桌面应用。只在单线程的命令行开头调用。
pub fn default_to_app() {
    if std::env::var_os("SHIFU_HOME").is_none_or(|v| v.is_empty()) {
        let home = std::path::PathBuf::from(std::env::var_os("HOME").unwrap_or_default()).join(".shifu/app");
        // SAFETY: 命令行在派发命令之前调用，那时只有主线程。
        unsafe {
            std::env::set_var("SHIFU_HOME", home);
            std::env::set_var("SHIFU_SOCKET", "shifu-app");
        }
    }
}

/// agent 名 -> 任务名，读 `<SHIFU_HOME>/tasks.json`。自动起名的任务没有自己的名字（显示的是话题），不算。
fn task_names(home: &Path) -> Vec<(String, String)> {
    let Ok(text) = std::fs::read_to_string(home.join("tasks.json")) else { return Vec::new() };
    let Ok(v) = serde_json::from_str::<Value>(&text) else { return Vec::new() };
    let mut out = Vec::new();
    for t in v["tasks"].as_array().into_iter().flatten() {
        if t["auto_name"].as_bool() == Some(true) {
            continue;
        }
        let Some(name) = t["name"].as_str() else { continue };
        for a in t["agents"].as_array().into_iter().flatten().filter_map(Value::as_str) {
            out.push((a.to_string(), name.to_string()));
        }
    }
    out
}

pub fn rows() -> Vec<Row> {
    let tmux = Tmux::from_env();
    let panes = tmux.panes();
    let titles = pane_titles(&tmux);
    let tasks = task_names(&shifu_home());
    let (agents, _) = agent::list();
    agents
        .into_iter()
        .map(|a| {
            let dir = Path::new(&a.cwd).file_name().and_then(|n| n.to_str()).unwrap_or(&a.cwd).to_string();
            let topic = a.pane_id.as_ref().and_then(|p| titles.get(p)).and_then(|t| clean_title(t, &dir, hostname()));
            Row {
                title: a.label.clone().or(topic).unwrap_or_else(|| a.name.clone()),
                task: tasks.iter().find(|(n, _)| *n == a.name).map(|(_, t)| t.clone()),
                status: a.effective_status(&panes),
                since: a.status_since,
                kind: a.kind.as_str().to_string(),
                last_message: a.last_message.clone(),
                cwd: a.cwd.clone(),
                pane: a.pane_id.clone(),
                name: a.name,
            }
        })
        .collect()
}

const GROUPS: [(Status, &str); 6] = [
    (Status::NeedsApproval, "等你批准"),
    (Status::NeedsReply, "等你回复"),
    (Status::Running, "在干活"),
    (Status::Starting, "启动中"),
    (Status::Idle, "空闲"),
    (Status::Exited, "已结束"),
];

fn one_line(text: &str, max: usize) -> String {
    let flat = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if flat.chars().count() <= max { flat } else { format!("{}…", flat.chars().take(max).collect::<String>()) }
}

/// 按急迫程度分组，组里等得最久的在前。
pub fn status_text(rows: &[Row], now: u64) -> String {
    if rows.is_empty() {
        return "Shifu 里没有对话。".to_string();
    }
    let mut out = String::new();
    for (status, heading) in GROUPS {
        let mut group: Vec<&Row> = rows.iter().filter(|r| r.status == status).collect();
        if group.is_empty() {
            continue;
        }
        group.sort_by_key(|r| r.since);
        out.push_str(&format!("{heading}（{}）\n", group.len()));
        for r in group {
            let task = r.task.as_deref().map(|t| format!("  任务「{t}」")).unwrap_or_default();
            out.push_str(&format!("  {}{}  {}  {}  [{}]\n", r.title, task, r.kind, format_age(now.saturating_sub(r.since)), r.name));
            if let Some(last) = r.last_message.as_deref().filter(|l| !l.trim().is_empty()) {
                out.push_str(&format!("    最后一句：{}\n", one_line(last, 140)));
            }
        }
    }
    out.trim_end().to_string()
}

/// 内部名完全一样的优先；否则显示名或任务名里含这几个字（不分大小写），只能有一个。
pub fn resolve<'a>(rows: &'a [Row], query: &str) -> Res<&'a Row> {
    if let Some(r) = rows.iter().find(|r| r.name == query) {
        return Ok(r);
    }
    let q = query.to_lowercase();
    let hits: Vec<&Row> = rows
        .iter()
        .filter(|r| r.title.to_lowercase().contains(&q) || r.task.as_deref().is_some_and(|t| t.to_lowercase().contains(&q)))
        .collect();
    match hits.as_slice() {
        [one] => Ok(one),
        [] => Err(format!("没有叫「{query}」的对话。shifu status 看全部。")),
        many => Err(format!(
            "「{query}」对上了 {} 个对话，用方括号里的名字指定：{}",
            many.len(),
            many.iter().map(|r| format!("{}[{}]", r.title, r.name)).collect::<Vec<_>>().join("、")
        )),
    }
}

/// 只往停着的对话里发：在干活的会被插话，等批准的会把这句话当成对批准框的回答。
pub fn can_tell(r: &Row) -> Res<()> {
    match r.status {
        Status::Idle | Status::NeedsReply => Ok(()),
        Status::Running => Err(format!("「{}」还在干活，等它停下再发，不然会插进它手上的事。", r.title)),
        Status::NeedsApproval => Err(format!("「{}」在等 用户 批准，先处理批准再发。", r.title)),
        Status::Starting => Err(format!("「{}」还在启动，稍后再发。", r.title)),
        Status::Exited => Err(format!("「{}」已经结束了。", r.title)),
    }
}

pub fn message(from: Option<&str>, text: &str) -> String {
    match from {
        Some(f) => format!("（{f}转来）{text}"),
        None => text.to_string(),
    }
}

/// 用括号粘贴把整段话放进输入框（多行也不会提前提交），再按回车。
fn send(tmux: &Tmux, pane: &str, text: &str) -> Res<()> {
    tmux.run(["set-buffer", "-b", "shifu-tell", "--", text])?;
    tmux.run(["paste-buffer", "-p", "-d", "-b", "shifu-tell", "-t", pane])?;
    // 粘贴和回车贴得太近时，有的输入框还没收完粘贴就收到回车。
    std::thread::sleep(std::time::Duration::from_millis(300));
    tmux.run(["send-keys", "-t", pane, "Enter"]).map(|_| ())
}

pub fn cmd_status(args: &[String]) -> Res<()> {
    let rows = rows();
    match args.first().map(String::as_str) {
        Some("--json") => {
            println!("{}", serde_json::to_string_pretty(&rows).map_err(|e| e.to_string())?);
        }
        None => println!("{}", status_text(&rows, now_secs())),
        Some(other) => return Err(format!("shifu status 不认识 {other:?}；可以加 --json")),
    }
    Ok(())
}

pub fn cmd_tell(args: &[String]) -> Res<()> {
    let mut from = None;
    let mut rest = Vec::new();
    let mut iter = args.iter();
    while let Some(a) = iter.next() {
        if a == "--from" {
            from = Some(iter.next().ok_or("--from 后面要写是谁")?.clone());
        } else {
            rest.push(a.clone());
        }
    }
    let (Some(target), true) = (rest.first(), rest.len() > 1) else {
        return Err("用法: shifu tell <对话名或话题里的几个字> [--from 谁] <要说的话>".to_string());
    };
    let text = rest[1..].join(" ");
    let rows = rows();
    let row = resolve(&rows, target)?;
    can_tell(row)?;
    let pane = row.pane.as_deref().ok_or_else(|| format!("「{}」没有终端窗格", row.title))?;
    send(&Tmux::from_env(), pane, &message(from.as_deref(), &text))?;
    println!("已发给「{}」[{}]", row.title, row.name);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(name: &str, title: &str, task: Option<&str>, status: Status, since: u64, last: Option<&str>) -> Row {
        Row {
            name: name.into(),
            kind: "claude".into(),
            title: title.into(),
            task: task.map(Into::into),
            status,
            since,
            last_message: last.map(Into::into),
            cwd: "/w".into(),
            pane: Some("%1".into()),
        }
    }

    #[test]
    fn status_groups_by_urgency_oldest_first() {
        let rows = [
            row("c-1", "赌石云同步", Some("游戏"), Status::Running, 900, None),
            row("c-2", "股市", None, Status::NeedsReply, 500, Some("看完了，\n结论是  跌")),
            row("c-3", "Kimi 接入", None, Status::NeedsApproval, 990, None),
            row("c-4", "旧的", None, Status::NeedsReply, 100, None),
        ];
        let text = status_text(&rows, 1000);
        let order: Vec<&str> = ["等你批准（1）", "Kimi 接入", "等你回复（2）", "旧的", "股市", "最后一句：看完了， 结论是 跌", "在干活（1）", "赌石云同步  任务「游戏」"]
            .into_iter()
            .collect();
        let mut at = 0;
        for want in order {
            let found = text[at..].find(want).unwrap_or_else(|| panic!("{want:?} not after {at} in\n{text}"));
            at += found;
        }
    }

    #[test]
    fn resolve_by_exact_name_then_unique_words() {
        let rows = [
            row("claude-W", "赌石模拟器云端同步", None, Status::Idle, 0, None),
            row("claude-W-2", "股市暴跌", Some("运营"), Status::Idle, 0, None),
            row("codex-W", "股市日报", None, Status::Idle, 0, None),
        ];
        assert_eq!(resolve(&rows, "claude-W").unwrap().name, "claude-W");
        assert_eq!(resolve(&rows, "赌石").unwrap().name, "claude-W");
        assert_eq!(resolve(&rows, "运营").unwrap().name, "claude-W-2");
        let many = resolve(&rows, "股市").unwrap_err();
        assert!(many.contains("2 个") && many.contains("[claude-W-2]") && many.contains("[codex-W]"), "{many}");
        assert!(resolve(&rows, "没有").is_err());
    }

    #[test]
    fn only_stopped_conversations_take_a_message() {
        for s in [Status::Idle, Status::NeedsReply] {
            assert!(can_tell(&row("a", "a", None, s, 0, None)).is_ok());
        }
        for s in [Status::Running, Status::NeedsApproval, Status::Starting, Status::Exited] {
            assert!(can_tell(&row("a", "a", None, s, 0, None)).is_err(), "{s:?}");
        }
    }

    #[test]
    fn a_relayed_message_says_who_sent_it() {
        assert_eq!(message(Some("项目经理"), "接着做云同步"), "（项目经理转来）接着做云同步");
        assert_eq!(message(None, "接着做"), "接着做");
    }
}
