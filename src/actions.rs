//! 侧栏按键和右键菜单共用的动作：跳转、并排、拆出、平均排列、Finder、恢复会话、关闭。
//!
//! 一页 = 一个 tmux 窗口，第 0 个窗格是侧栏。agent 的身份是 pane_id，
//! 它在哪一页每次都从 tmux 现查（`Tmux::panes`），不看 agent 文件里存的 window_id。

use crate::agent::{self, Agent, PaneInfo, Res, now_secs};
use crate::commands::{open_agent_window, write_claude_hooks};
use crate::launch::{model_flags, resume_command};
use crate::status::Status;
use crate::tabs::page_name;
use crate::tmux::{SIDEBAR_WIDTH, Tmux, page_panes, run_command};
use std::collections::HashMap;
use std::process::Command;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    Jump,
    Right,
    Below,
    Own,
    Even,
    Finder,
    Resume,
    Close,
}

impl Action {
    pub fn parse(name: &str) -> Option<Action> {
        Some(match name {
            "jump" => Action::Jump,
            "right" => Action::Right,
            "below" => Action::Below,
            "own" => Action::Own,
            "even" => Action::Even,
            "finder" => Action::Finder,
            "resume" => Action::Resume,
            "close" => Action::Close,
            _ => return None,
        })
    }
}

/// 「当前页」：从 `shifu act` 跑起来的这一刻，用户正在看的那个窗口。
pub fn current_window(tmux: &Tmux) -> Res<String> {
    let mut args = vec!["display-message".to_string(), "-p".to_string()];
    if let Some(pane) = std::env::var("TMUX_PANE").ok().filter(|p| !p.is_empty()) {
        args.extend(["-t".to_string(), pane]);
    }
    args.push("#{window_id}".to_string());
    tmux.run(args)
}

/// 对名叫 `name` 的 agent 做 `action`；`page` 是当前页（窗口 id）。
pub fn run(tmux: &Tmux, shifu: &str, action: Action, name: &str, page: &str) -> Res<()> {
    let agent = agent::load(name)?.ok_or_else(|| format!("没有叫 {name} 的 agent"))?;
    let panes = tmux.panes();
    match action {
        Action::Jump => jump(tmux, window_of(&agent, &panes), &agent),
        Action::Right => join(tmux, &agent, &panes, page, "-h"),
        Action::Below => join(tmux, &agent, &panes, page, "-v"),
        Action::Own => own(tmux, shifu, &agent, &panes),
        Action::Even => even(tmux, page),
        Action::Finder => open_in_finder(&agent),
        Action::Resume => resume(tmux, shifu, &agent, &panes),
        Action::Close => close(tmux, &agent, &panes),
    }
}

fn window_of<'a>(agent: &Agent, panes: &'a HashMap<String, PaneInfo>) -> Option<&'a str> {
    agent.pane_id.as_ref().and_then(|p| panes.get(p)).map(|info| info.window_id.as_str())
}

/// agent 的窗格此刻的位置；窗格已经没了就报错。
fn live<'a>(agent: &'a Agent, panes: &'a HashMap<String, PaneInfo>) -> Res<(&'a str, &'a PaneInfo)> {
    agent
        .pane_id
        .as_deref()
        .and_then(|p| panes.get(p).map(|info| (p, info)))
        .ok_or_else(|| format!("{} 的窗格已经没了", agent.name))
}

/// 跳到某个 agent：先选窗口再选窗格。`window` 是从 tmux 查到的，不是文件里存的。
pub fn jump(tmux: &Tmux, window: Option<&str>, agent: &Agent) -> Res<()> {
    let (Some(window), Some(pane)) = (window, agent.pane_id.as_deref()) else {
        return Err(format!("{} 的窗格已经没了", agent.name));
    };
    tmux.run(["select-window", "-t", window])?;
    tmux.run(["select-pane", "-t", pane])?;
    Ok(())
}

fn restore_sidebar(tmux: &Tmux, sidebar: &str) -> Res<()> {
    tmux.run(["resize-pane", "-t", sidebar, "-x", SIDEBAR_WIDTH]).map(|_| ())
}

/// 把 agent 并排到当前页：`direction` 是 `-h`（右边）或 `-v`（下面）。
/// 目标是当前页里除侧栏外 pane_index 最大的窗格；页里只有侧栏就并到侧栏右边。
fn join(tmux: &Tmux, agent: &Agent, panes: &HashMap<String, PaneInfo>, page: &str, direction: &str) -> Res<()> {
    let (pane, info) = live(agent, panes)?;
    if info.window_id == page {
        return Err("已经在这一页了".to_string());
    }
    let here = page_panes(panes, page);
    let Some((_, sidebar)) = here.first() else {
        return Err(format!("{page} 这一页没有侧栏，不能并排"));
    };
    let target = if here.len() > 1 { &here[here.len() - 1].1 } else { sidebar };
    tmux.run(["join-pane", direction, "-d", "-s", pane, "-t", target])?;
    restore_sidebar(tmux, sidebar)?;
    settle_pages(tmux, &[&info.window_id, page])?;
    tmux.run(["select-pane", "-t", pane])?;
    Ok(())
}

/// 单独成页：拆成新窗口，左边开侧栏，切过去。
fn own(tmux: &Tmux, shifu: &str, agent: &Agent, panes: &HashMap<String, PaneInfo>) -> Res<()> {
    let (pane, info) = live(agent, panes)?;
    let neighbours = page_panes(panes, &info.window_id).iter().filter(|(index, id)| *index != 0 && id != pane).count();
    if neighbours == 0 {
        return Err("已经是单独一页".to_string());
    }
    let new_window = tmux.run(["break-pane", "-d", "-P", "-F", "#{window_id}", "-s", pane])?;
    tmux.run(["set-option", "-w", "-t", &new_window, "allow-rename", "off"])?;
    tmux.run(["set-option", "-w", "-t", &new_window, "automatic-rename", "off"])?;
    tmux.run(["split-window", "-h", "-b", "-l", SIDEBAR_WIDTH, "-t", pane, &crate::tmux::sidebar_command(shifu)])?;
    settle_pages(tmux, &[&info.window_id, &new_window])?;
    tmux.run(["select-window", "-t", &new_window])?;
    tmux.run(["select-pane", "-t", pane])?;
    Ok(())
}

/// 一个窗格的位置和大小（终端格数）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Geometry {
    pub pane: String,
    pub left: u32,
    pub top: u32,
    pub width: u32,
    pub height: u32,
}

/// 平均排列的做法。
#[derive(Debug, PartialEq, Eq)]
pub enum EvenPlan {
    /// 并排成一行：除最后一个外，每个窗格的目标宽度（最后一个自动拿剩下的）。
    Widths(Vec<(String, u32)>),
    /// 叠成一列：除最后一个外，每个窗格的目标高度。
    Heights(Vec<(String, u32)>),
    /// 既有并排又有叠放，交给 tmux 自己的 `select-layout -E`。
    Mixed,
}

/// 把 `total` 格分给 `count` 个窗格，窗格之间各占 1 格分隔线；余数给靠前的。
pub fn even_sizes(total: u32, count: u32) -> Vec<u32> {
    let usable = total.saturating_sub(count.saturating_sub(1));
    (0..count).map(|i| usable / count + u32::from(i < usable % count)).collect()
}

/// 页里除侧栏外的窗格怎么平均：全在同一行就均分宽度，全在同一列就均分高度。
/// 侧栏占左边 `sidebar_width` 格加一条分隔线。
pub fn plan_even(agents: &[Geometry], window_width: u32, window_height: u32, sidebar_width: u32) -> EvenPlan {
    let same = |f: fn(&Geometry) -> u32| agents.iter().all(|g| f(g) == f(&agents[0]));
    let take_all_but_last = |ordered: Vec<&Geometry>, sizes: Vec<u32>| -> Vec<(String, u32)> {
        ordered.iter().zip(sizes).take(agents.len() - 1).map(|(g, size)| (g.pane.clone(), size)).collect()
    };
    let count = agents.len() as u32;
    if agents.len() < 2 {
        EvenPlan::Widths(Vec::new())
    } else if same(|g| g.top) && same(|g| g.height) {
        let mut ordered: Vec<&Geometry> = agents.iter().collect();
        ordered.sort_by_key(|g| g.left);
        EvenPlan::Widths(take_all_but_last(ordered, even_sizes(window_width.saturating_sub(sidebar_width + 1), count)))
    } else if same(|g| g.left) && same(|g| g.width) {
        let mut ordered: Vec<&Geometry> = agents.iter().collect();
        ordered.sort_by_key(|g| g.top);
        EvenPlan::Heights(take_all_but_last(ordered, even_sizes(window_height, count)))
    } else {
        EvenPlan::Mixed
    }
}

/// `list-panes` 每行 `pane_id left top width height`。
fn parse_geometry(listing: &str) -> Vec<Geometry> {
    listing
        .lines()
        .filter_map(|line| {
            let mut parts = line.split(' ');
            let pane = parts.next()?.to_string();
            let mut number = || parts.next()?.parse().ok();
            Some(Geometry { pane, left: number()?, top: number()?, width: number()?, height: number()? })
        })
        .collect()
}

/// 平均排列当前页的 agent 窗格，侧栏保持 32 列。
fn even(tmux: &Tmux, page: &str) -> Res<()> {
    let panes = tmux.panes();
    let here = page_panes(&panes, page);
    let Some((_, sidebar)) = here.first() else {
        return Err(format!("{page} 这一页没有侧栏"));
    };
    let listing = tmux.run(["list-panes", "-t", page, "-F", "#{pane_id} #{pane_left} #{pane_top} #{pane_width} #{pane_height}"])?;
    let agents: Vec<Geometry> = parse_geometry(&listing).into_iter().filter(|g| g.pane != *sidebar).collect();
    let size = tmux.run(["display-message", "-p", "-t", page, "#{window_width} #{window_height}"])?;
    let (width, height) = size
        .split_once(' ')
        .and_then(|(w, h)| Some((w.parse().ok()?, h.parse().ok()?)))
        .ok_or_else(|| format!("看不懂窗口大小: {size:?}"))?;
    let sidebar_width = SIDEBAR_WIDTH.parse().expect("侧栏宽度是常量");
    match plan_even(&agents, width, height, sidebar_width) {
        EvenPlan::Widths(sizes) => resize_in_order(tmux, "-x", &sizes)?,
        EvenPlan::Heights(sizes) => resize_in_order(tmux, "-y", &sizes)?,
        EvenPlan::Mixed => {
            tmux.run(["select-layout", "-E", "-t", &agents[0].pane])?;
        }
    }
    restore_sidebar(tmux, sidebar)
}

/// 从前往后逐个设大小：每次调整都是从后一个窗格那里匀空间。
fn resize_in_order(tmux: &Tmux, axis: &str, sizes: &[(String, u32)]) -> Res<()> {
    for (pane, size) in sizes {
        tmux.run(["resize-pane", "-t", pane, axis, &size.to_string()])?;
    }
    Ok(())
}

fn open_in_finder(agent: &Agent) -> Res<()> {
    let mut cmd = Command::new("open");
    cmd.arg(&agent.cwd);
    run_command(cmd).map(|_| ())
}

/// 恢复已退出的会话。窗格还在（remain-on-exit 留下的死窗格）就原地重开，pane_id 不变；窗格没了就新开一页。
fn resume(tmux: &Tmux, shifu: &str, agent: &Agent, panes: &HashMap<String, PaneInfo>) -> Res<()> {
    if agent.effective_status(panes) != Status::Exited {
        return Err(format!("{} 还在运行", agent.name));
    }
    let settings = write_claude_hooks(agent.kind, shifu)?;
    let pick = model_flags(agent.kind, agent.model.as_deref(), agent.effort.as_deref());
    let launch = resume_command(agent.kind, &agent.command, shifu, &settings, agent.session_id.as_deref(), &pick);
    // 先写文件再起进程：新进程的第一个 hook 不能被这次写入盖掉
    let restarted = Agent { status: Status::Starting, status_since: now_secs(), ..agent.clone() };
    agent::save(&restarted)?;
    match agent.pane_id.as_ref().filter(|p| panes.contains_key(*p)) {
        Some(pane) => {
            tmux.run(["set-option", "-p", "-t", pane, "@shifu_status", agent::initial_tab_status(agent.kind).name()])?;
            let id = format!("SHIFU_ID={}", agent.name);
            tmux.run(["respawn-pane", "-k", "-t", pane, "-c", &agent.cwd, "-e", &id, &launch])?;
            Ok(())
        }
        None => open_agent_window(tmux, shifu, &restarted, &launch).map(|_| ()),
    }
}

/// 关闭：先删文件（关最后一个窗格会连侧栏进程一起带走），再关窗格，页里只剩侧栏就关页。
fn close(tmux: &Tmux, agent: &Agent, panes: &HashMap<String, PaneInfo>) -> Res<()> {
    agent::remove(&agent.name)?;
    if let Ok((pane, info)) = live(agent, panes) {
        tmux.run(["kill-pane", "-t", pane])?;
        settle_pages(tmux, &[&info.window_id])?;
    }
    Ok(())
}

/// 动过窗格的页收尾：只剩侧栏的页关掉，其余的按里面的 agent 重算页名。
fn settle_pages(tmux: &Tmux, windows: &[&str]) -> Res<()> {
    let panes = tmux.panes();
    let (agents, _) = agent::list();
    let mut seen: Vec<&str> = Vec::new();
    for window in windows {
        if seen.contains(window) {
            continue;
        }
        seen.push(window);
        let here = page_panes(&panes, window);
        match here.len() {
            0 => {}
            1 => {
                tmux.run(["kill-window", "-t", window])?;
            }
            _ => {
                let names: Vec<String> = here
                    .iter()
                    .filter_map(|(_, pane)| agents.iter().find(|a| a.pane_id.as_ref() == Some(pane)))
                    .map(|a| a.name.clone())
                    .collect();
                tmux.run(["rename-window", "-t", window, &page_name(&names)])?;
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn g(pane: &str, left: u32, top: u32, width: u32, height: u32) -> Geometry {
        Geometry { pane: pane.to_string(), left, top, width, height }
    }

    fn sized(items: &[(&str, u32)]) -> Vec<(String, u32)> {
        items.iter().map(|(p, n)| (p.to_string(), *n)).collect()
    }

    #[test]
    fn action_names() {
        for (name, want) in [
            ("jump", Action::Jump),
            ("right", Action::Right),
            ("below", Action::Below),
            ("own", Action::Own),
            ("even", Action::Even),
            ("finder", Action::Finder),
            ("resume", Action::Resume),
            ("close", Action::Close),
        ] {
            assert_eq!(Action::parse(name), Some(want));
        }
        assert_eq!(Action::parse("explode"), None);
    }

    #[test]
    fn even_sizes_split_with_borders_and_spread_remainder() {
        assert_eq!(even_sizes(87, 2), [43, 43]);
        assert_eq!(even_sizes(88, 2), [44, 43]);
        assert_eq!(even_sizes(167, 3), [55, 55, 55]);
        assert_eq!(even_sizes(30, 2), [15, 14]);
        assert_eq!(even_sizes(50, 1), [50]);
    }

    #[test]
    fn two_side_by_side_get_equal_widths() {
        // 窗口 120 列：侧栏 32 + 分隔线 1 + 左 60 + 分隔线 1 + 右 26
        let agents = [g("%2", 33, 0, 60, 29), g("%3", 94, 0, 26, 29)];
        assert_eq!(plan_even(&agents, 120, 30, 32), EvenPlan::Widths(sized(&[("%2", 43)])));
    }

    #[test]
    fn three_in_a_row_ordered_by_position_not_by_input() {
        let agents = [g("%9", 130, 0, 30, 40), g("%2", 33, 0, 60, 40), g("%5", 94, 0, 35, 40)];
        assert_eq!(plan_even(&agents, 160, 41, 32), EvenPlan::Widths(sized(&[("%2", 42), ("%5", 42)])));
    }

    #[test]
    fn stacked_panes_get_equal_heights() {
        let agents = [g("%3", 33, 16, 87, 13), g("%2", 33, 0, 87, 15)];
        assert_eq!(plan_even(&agents, 120, 30, 32), EvenPlan::Heights(sized(&[("%2", 15)])));
    }

    #[test]
    fn l_shaped_page_falls_back_to_tmux_spread() {
        let agents = [g("%2", 33, 0, 40, 29), g("%3", 74, 0, 46, 14), g("%4", 74, 15, 46, 14)];
        assert_eq!(plan_even(&agents, 120, 30, 32), EvenPlan::Mixed);
    }

    #[test]
    fn single_agent_needs_no_resizing() {
        assert_eq!(plan_even(&[g("%2", 33, 0, 87, 29)], 120, 30, 32), EvenPlan::Widths(Vec::new()));
    }

    #[test]
    fn geometry_lines_parse() {
        let got = parse_geometry("%1 0 0 32 29\n%2 33 0 87 29\nbroken\n");
        assert_eq!(got, [g("%1", 0, 0, 32, 29), g("%2", 33, 0, 87, 29)]);
    }
}
