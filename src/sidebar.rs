//! `shifu sidebar`：每个 tmux 窗口左边的 agent 列表。

use crate::actions::{self, Action};
use crate::agent::{self, Entry, Res, now_secs};
use crate::menu::{agent_menu, load_recent, menu_args, new_menu};
use crate::status::{Status, format_age};
use crate::tmux::Tmux;
use ratatui::Frame;
use ratatui::crossterm::event::{
    self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers, MouseButton,
    MouseEvent, MouseEventKind,
};
use ratatui::crossterm::execute;
use ratatui::layout::{Constraint, Layout};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use std::collections::HashMap;
use std::process::Command;
use std::time::{Duration, Instant};

const REFRESH: Duration = Duration::from_millis(500);
/// 不在屏幕上的侧栏（别的页、或没人 attach）只隔这么久看一眼自己是否露出来，不读状态、不调 git。
/// 实测 18 个侧栏都按 500 毫秒刷新时合计占 CPU 约 7%，每 5 秒还要各自对每个目录跑一次 git。
const HIDDEN_CHECK: Duration = Duration::from_secs(1);
const BRANCH_TTL: Duration = Duration::from_secs(5);
const KILL_CONFIRM: Duration = Duration::from_secs(2);
const NOTICE_SHOW: Duration = Duration::from_secs(4);
const NARROW: usize = 20;
const CURSOR_BG: Color = Color::Indexed(236);

/// 侧栏里的一个 agent：校正后的状态 + git 分支。
#[derive(Clone, Debug)]
pub struct Item {
    pub entry: Entry,
    pub branch: Option<String>,
}

/// 渲染好的一行，记着它属于第几个 agent（点击用）。
pub struct BodyLine {
    pub line: Line<'static>,
    pub item: usize,
}

fn status_color(s: Status) -> Color {
    match s {
        Status::NeedsApproval => Color::Yellow,
        Status::NeedsReply => Color::LightBlue,
        Status::Running => Color::Green,
        Status::Idle | Status::Starting => Color::Gray,
        Status::Exited => Color::DarkGray,
    }
}

fn text_width(s: &str) -> usize {
    Span::raw(s).width()
}

/// 按显示宽度截断，超出时末尾放 `…`。
pub fn fit(s: &str, max: usize) -> String {
    if text_width(s) <= max {
        return s.to_string();
    }
    if max == 0 {
        return String::new();
    }
    let mut out = String::new();
    let mut used = 0;
    for c in s.chars() {
        let w = text_width(c.encode_utf8(&mut [0; 4]));
        if used + w > max - 1 {
            break;
        }
        out.push(c);
        used += w;
    }
    out.push('…');
    out
}

fn one_line(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn dir_name(cwd: &str) -> &str {
    cwd.rsplit('/').find(|p| !p.is_empty()).unwrap_or(cwd)
}

/// 每个 agent 2–3 行（太窄时 1 行）。
pub fn build_lines(items: &[Item], cursor: usize, current_window: &str, width: usize, now: u64) -> Vec<BodyLine> {
    let mut lines = Vec::new();
    for (idx, item) in items.iter().enumerate() {
        let a = &item.entry.agent;
        let status = item.entry.status;
        let is_current = !current_window.is_empty() && item.entry.window.as_deref() == Some(current_window);
        let row_style = if idx == cursor { Style::default().bg(CURSOR_BG) } else { Style::default() };
        let marker = if is_current {
            Span::styled("▎", Style::default().fg(Color::White))
        } else {
            Span::raw(" ")
        };
        let mut name_style = Style::default();
        if is_current {
            name_style = name_style.add_modifier(Modifier::BOLD);
        }
        if status == Status::Exited {
            name_style = name_style.fg(Color::DarkGray);
        }
        let mut push = |line: Line<'static>| lines.push(BodyLine { line: line.style(row_style), item: idx });

        // 标记 1 列 + 圆点 1 列 + 空格 1 列
        let name = fit(&a.name, width.saturating_sub(3));
        push(Line::from(vec![
            marker.clone(),
            Span::styled("●", Style::default().fg(status_color(status))),
            Span::raw(" "),
            Span::styled(name, name_style),
        ]));
        if width < NARROW {
            continue;
        }

        let dim = Style::default().fg(Color::DarkGray);
        let avail = width.saturating_sub(3);
        let tail = format!("{} {}", status.label(), format_age(now.saturating_sub(a.status_since)));
        let head = match &item.branch {
            Some(b) => format!("{} · {}", dir_name(&a.cwd), b),
            None => dir_name(&a.cwd).to_string(),
        };
        let head_room = avail.saturating_sub(text_width(&tail) + 3);
        let detail = if head_room >= 4 {
            vec![
                marker.clone(),
                Span::raw("  "),
                Span::styled(format!("{} · ", fit(&head, head_room)), dim),
                Span::styled(tail, Style::default().fg(status_color(status))),
            ]
        } else {
            vec![marker.clone(), Span::raw("  "), Span::styled(fit(&tail, avail), Style::default().fg(status_color(status)))]
        };
        push(Line::from(detail));

        if let Some(msg) = a.last_message.as_deref().map(one_line).filter(|m| !m.is_empty()) {
            push(Line::from(vec![marker.clone(), Span::raw("  "), Span::styled(fit(&msg, avail), dim)]));
        }
    }
    lines
}

/// 让光标所在的 agent 整块可见的滚动偏移。
pub fn scroll_offset(lines: &[BodyLine], cursor: usize, height: usize) -> usize {
    let Some(first) = lines.iter().position(|l| l.item == cursor) else {
        return 0;
    };
    let last = lines.iter().rposition(|l| l.item == cursor).unwrap_or(first);
    let offset = (last + 1).saturating_sub(height);
    offset.min(first)
}

struct App {
    tmux: Tmux,
    shifu: String,
    /// 侧栏自己的窗格，菜单靠它定位
    own_pane: String,
    items: Vec<Item>,
    cursor: usize,
    pending_kill: Option<(String, Instant)>,
    notice: Option<(String, Instant)>,
    current_window: String,
    branches: HashMap<String, (Instant, Option<String>)>,
}

/// 一帧要画的东西。
struct View {
    lines: Vec<BodyLine>,
    offset: usize,
    waiting: usize,
    footer: Line<'static>,
}

impl App {
    fn new(tmux: Tmux, shifu: String) -> App {
        let own_pane = std::env::var("TMUX_PANE").unwrap_or_default();
        let current_window = tmux.run(["display-message", "-p", "-t", &own_pane, "#{window_id}"]).unwrap_or_default();
        App {
            tmux,
            shifu,
            own_pane,
            items: Vec::new(),
            cursor: 0,
            pending_kill: None,
            notice: None,
            current_window,
            branches: HashMap::new(),
        }
    }

    fn selected_name(&self) -> Option<&str> {
        self.items.get(self.cursor).map(|i| i.entry.agent.name.as_str())
    }

    fn branch_of(&mut self, cwd: &str) -> Option<String> {
        if let Some((at, branch)) = self.branches.get(cwd)
            && at.elapsed() < BRANCH_TTL
        {
            return branch.clone();
        }
        let out = Command::new("git").args(["-C", cwd, "branch", "--show-current"]).output().ok();
        let branch = out
            .filter(|o| o.status.success())
            .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
            .filter(|b| !b.is_empty());
        self.branches.insert(cwd.to_string(), (Instant::now(), branch.clone()));
        branch
    }

    fn refresh(&mut self) {
        let keep = self.selected_name().map(str::to_string);
        let (agents, _) = agent::list();
        let entries = agent::sorted_entries(agents, &self.tmux.panes());
        let items: Vec<Item> = entries
            .into_iter()
            .map(|entry| {
                let branch = if entry.status == Status::Exited { None } else { self.branch_of(&entry.agent.cwd) };
                Item { entry, branch }
            })
            .collect();
        // 光标跟着名字走；名字没了就停在原来的行号附近
        self.cursor = keep
            .and_then(|n| items.iter().position(|i| i.entry.agent.name == n))
            .unwrap_or_else(|| self.cursor.min(items.len().saturating_sub(1)));
        self.items = items;
    }

    /// 光标移到这一页自己的 agent 上，并取消没确认完的关闭。
    fn focus_own_page(&mut self) {
        if let Some(idx) = cursor_on_page(&self.items, &self.current_window) {
            self.cursor = idx;
        }
        self.pending_kill = None;
    }

    fn expire(&mut self) {
        if self.pending_kill.as_ref().is_some_and(|(_, at)| at.elapsed() > KILL_CONFIRM) {
            self.pending_kill = None;
        }
        if self.notice.as_ref().is_some_and(|(_, at)| at.elapsed() > NOTICE_SHOW) {
            self.notice = None;
        }
    }

    fn view(&self, width: u16, height: u16) -> View {
        let now = now_secs();
        let lines = if self.items.is_empty() {
            empty_hint(width as usize)
        } else {
            build_lines(&self.items, self.cursor, &self.current_window, width as usize, now)
        };
        let offset = scroll_offset(&lines, self.cursor, (height as usize).saturating_sub(2));
        let waiting = self.items.iter().filter(|i| i.entry.status.is_waiting()).count();
        let footer = match (&self.pending_kill, &self.notice) {
            (Some((name, _)), _) => Line::styled(fit(&format!("再按 X 关闭 {name}"), width as usize), Style::default().fg(Color::Yellow)),
            (None, Some((msg, _))) => Line::styled(fit(msg, width as usize), Style::default().fg(Color::Red)),
            _ => Line::styled(fit("⏎ 跳  v 右  b 下  u 拆  n 新建  右键 更多", width as usize), Style::default().fg(Color::DarkGray)),
        };
        View { lines, offset, waiting, footer }
    }

    /// 侧栏所在的页正显示在某个已 attach 的终端上。查不到就当看得见，宁可多刷。
    fn is_visible(&self) -> bool {
        let out = self.tmux.run(["display-message", "-p", "-t", &self.own_pane, "#{window_active}#{session_attached}"]);
        // 输出是 window_active（0/1）紧跟 session_attached（attach 的终端数）
        out.map(|s| s.starts_with('1') && s.get(1..).is_some_and(|n| !n.is_empty() && n != "0")).unwrap_or(true)
    }

    fn report<T>(&mut self, result: Res<T>) {
        if let Err(e) = result {
            self.notice = Some((e, Instant::now()));
        }
    }

    fn move_cursor(&mut self, delta: isize) {
        let max = self.items.len().saturating_sub(1) as isize;
        self.cursor = (self.cursor as isize + delta).clamp(0, max) as usize;
    }

    /// 对光标所在的 agent 做一个动作；出错在底部提示，成功后立刻刷新。
    fn act(&mut self, action: Action) {
        let Some(name) = self.selected_name().map(str::to_string) else { return };
        let result = actions::run(&self.tmux, &self.shifu, action, &name, &self.current_window);
        self.report(result);
        self.refresh();
    }

    fn jump_to(&mut self, idx: usize) {
        self.cursor = idx.min(self.items.len().saturating_sub(1));
        self.act(Action::Jump);
    }

    /// 侧栏窗格左上角在终端里的位置，菜单坐标要加上它。
    fn origin(&self) -> (u16, u16) {
        let out = self.tmux.run(["display-message", "-p", "-t", &self.own_pane, "#{pane_left} #{pane_top}"]).unwrap_or_default();
        let mut parts = out.split(' ').filter_map(|n| n.parse().ok());
        (parts.next().unwrap_or(0), parts.next().unwrap_or(0))
    }

    /// 「新建 agent」菜单。`at` 是侧栏里的（列，行），None 表示键盘调出，放在侧栏底部。
    fn open_new_menu(&mut self, at: Option<(u16, u16)>) {
        let base = match self.items.get(self.cursor) {
            Some(i) => i.entry.agent.cwd.clone(),
            None => std::env::current_dir().map(|p| p.to_string_lossy().to_string()).unwrap_or_default(),
        };
        let recent = load_recent().unwrap_or_else(|e| {
            self.notice = Some((e, Instant::now()));
            Vec::new()
        });
        let items = new_menu(&self.shifu, &base, &recent);
        self.show_menu("新建 agent", at, &items);
    }

    fn open_agent_menu(&mut self, at: (u16, u16)) {
        let Some(item) = self.items.get(self.cursor) else { return };
        let items = agent_menu(&self.shifu, &item.entry.agent.name, item.entry.status == Status::Exited, &self.current_window);
        let title = item.entry.agent.name.clone();
        self.show_menu(&title, Some(at), &items);
    }

    fn show_menu(&mut self, title: &str, at: Option<(u16, u16)>, items: &[crate::menu::MenuItem]) {
        let (x, y) = match at {
            // display-menu 的 y 是菜单底边的下一行（实测 tmux 3.6a），所以加上菜单高度，菜单就从点击的那一行往下长；
            // 放不下时 tmux 自己会往上挪
            Some((col, row)) => {
                let (left, top) = self.origin();
                let height = items.len() as u16 + 2;
                ((left + col).to_string(), (top + row + height).to_string())
            }
            None => ("P".to_string(), "P".to_string()),
        };
        let result = self.tmux.run(menu_args(title, &x, &y, &self.own_pane, items));
        self.report(result);
    }

    /// 返回 true 表示退出。
    fn on_key(&mut self, key: KeyEvent) -> bool {
        if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('c') {
            return true;
        }
        let confirming = self.pending_kill.take();
        match key.code {
            KeyCode::Up | KeyCode::Char('k') => self.move_cursor(-1),
            KeyCode::Down | KeyCode::Char('j') => self.move_cursor(1),
            KeyCode::Enter => self.act(Action::Jump),
            KeyCode::Char('v') => self.act(Action::Right),
            KeyCode::Char('b') => self.act(Action::Below),
            KeyCode::Char('u') => self.act(Action::Own),
            KeyCode::Char('=') => self.act(Action::Even),
            KeyCode::Char('o') => self.act(Action::Finder),
            KeyCode::Char('R') => self.act(Action::Resume),
            KeyCode::Char('n') => self.open_new_menu(None),
            KeyCode::Char('X') => {
                let name = self.selected_name().map(str::to_string);
                match (confirming, name) {
                    (Some((pending, at)), Some(name)) if pending == name && at.elapsed() <= KILL_CONFIRM => self.act(Action::Close),
                    (_, Some(name)) => self.pending_kill = Some((name, Instant::now())),
                    _ => {}
                }
            }
            _ => {}
        }
        false
    }

    fn on_mouse(&mut self, m: MouseEvent, view: &View, height: u16) {
        // 点在哪个 agent 的行上；标题、底部提示和空白处都是 None
        let body_rows = (height as usize).saturating_sub(2);
        let row = m.row as usize;
        let hit = if (1..=body_rows).contains(&row) { view.lines.get(view.offset + row - 1).map(|l| l.item) } else { None };
        let hit = hit.filter(|idx| *idx < self.items.len());
        match (m.kind, hit) {
            (MouseEventKind::ScrollUp, _) => self.move_cursor(-1),
            (MouseEventKind::ScrollDown, _) => self.move_cursor(1),
            (MouseEventKind::Down(MouseButton::Left), Some(idx)) => self.jump_to(idx),
            (MouseEventKind::Down(MouseButton::Right), Some(idx)) => {
                self.cursor = idx;
                self.open_agent_menu((m.column, m.row));
            }
            (MouseEventKind::Down(MouseButton::Right), None) => self.open_new_menu(Some((m.column, m.row))),
            _ => {}
        }
    }
}

fn draw(frame: &mut Frame, view: &View) {
    let [header, body, footer] = Layout::vertical([Constraint::Length(1), Constraint::Min(0), Constraint::Length(1)]).areas(frame.area());
    let mut head = vec![Span::styled("shifu", Style::default().add_modifier(Modifier::BOLD))];
    if view.waiting > 0 {
        head.push(Span::styled(format!("  {} 个在等你", view.waiting), Style::default().fg(Color::Yellow)));
    }
    frame.render_widget(Paragraph::new(Line::from(head)), header);
    let visible: Vec<Line> = view.lines.iter().skip(view.offset).take(body.height as usize).map(|l| l.line.clone()).collect();
    frame.render_widget(Paragraph::new(visible), body);
    frame.render_widget(Paragraph::new(view.footer.clone()), footer);
}

/// 侧栏循环为什么结束。
enum LoopEnd {
    Quit,
    /// shifu 程序文件被换过（重新安装）：用新程序重开侧栏，用户不用重启 tmux。
    Upgraded,
}

pub fn run() -> Res<()> {
    let shifu = crate::commands::shifu_exe()?.to_string_lossy().to_string();
    let mut app = App::new(Tmux::from_env(), shifu.clone());
    let mut terminal = ratatui::init();
    execute!(std::io::stdout(), EnableMouseCapture).map_err(|e| format!("开启鼠标失败: {e}"))?;
    let result = event_loop(&mut app, &mut terminal);
    let _ = execute!(std::io::stdout(), DisableMouseCapture);
    ratatui::restore();
    match result? {
        LoopEnd::Quit => Ok(()),
        LoopEnd::Upgraded => {
            use std::os::unix::process::CommandExt;
            let err = Command::new(&shifu).arg("sidebar").exec();
            Err(format!("换新版侧栏失败: {err}"))
        }
    }
}

fn exe_mtime(path: &str) -> Option<std::time::SystemTime> {
    std::fs::metadata(path).and_then(|m| m.modified()).ok()
}

/// 这一页自己的第一个 agent 在列表里的位置。切到某一页时，那一页侧栏的光标落在这里，
/// 免得光标还停在别的 agent 上、按 X 关错（用模拟终端测出来的）。
pub fn cursor_on_page(items: &[Item], current_window: &str) -> Option<usize> {
    items.iter().position(|i| !current_window.is_empty() && i.entry.window.as_deref() == Some(current_window))
}

/// 还没有 agent 时侧栏里的提示。行不属于任何 agent（item 越界），点了不会跳。
fn empty_hint(width: usize) -> Vec<BodyLine> {
    ["", "还没有 agent", "按 n 或右键新建"]
        .iter()
        .map(|t| BodyLine { line: Line::styled(fit(t, width), Style::default().fg(Color::DarkGray)), item: usize::MAX })
        .collect()
}

fn event_loop(app: &mut App, terminal: &mut ratatui::DefaultTerminal) -> Res<LoopEnd> {
    let io = |e: std::io::Error| format!("终端出错: {e}");
    let started_mtime = exe_mtime(&app.shifu);
    let mut visible = true;
    let mut first_round = true;
    let mut last_refresh = Instant::now() - REFRESH;
    loop {
        let interval = if visible { REFRESH } else { HIDDEN_CHECK };
        if last_refresh.elapsed() >= interval {
            let was_visible = visible;
            visible = app.is_visible();
            // 第一轮总是读一次：页刚建好就被切过去时，不至于先空着一秒
            if visible || first_round {
                app.refresh();
                if !was_visible || first_round {
                    app.focus_own_page();
                }
            }
            first_round = false;
            if exe_mtime(&app.shifu) != started_mtime {
                return Ok(LoopEnd::Upgraded);
            }
            last_refresh = Instant::now();
        }
        app.expire();
        let size = terminal.size().map_err(io)?;
        let view = app.view(size.width, size.height);
        terminal.draw(|f| draw(f, &view)).map_err(io)?;
        if !event::poll(Duration::from_millis(100)).map_err(io)? {
            continue;
        }
        match event::read().map_err(io)? {
            Event::Key(key) if key.kind == KeyEventKind::Press => {
                if app.on_key(key) {
                    return Ok(LoopEnd::Quit);
                }
            }
            Event::Mouse(m) => app.on_mouse(m, &view, size.height),
            _ => {}
        }
    }
}

#[cfg(test)]
mod cursor_tests {
    use super::*;
    use crate::agent::{Agent, Entry, Kind};

    fn at(name: &str, window: Option<&str>) -> Item {
        let agent = Agent {
            name: name.to_string(),
            kind: Kind::Other,
            command: "sh".to_string(),
            cwd: "/w".to_string(),
            created_at: 0,
            status: Status::Running,
            status_since: 0,
            window_id: None,
            pane_id: None,
            session_id: None,
            last_message: None,
            model: None,
            effort: None,
            label: None,
        };
        Item { entry: Entry { agent, status: Status::Running, window: window.map(str::to_string) }, branch: None }
    }

    #[test]
    fn cursor_lands_on_this_pages_agent() {
        let items = [at("a", Some("@1")), at("b", Some("@4")), at("c", Some("@4"))];
        assert_eq!(cursor_on_page(&items, "@4"), Some(1));
        assert_eq!(cursor_on_page(&items, "@9"), None);
        assert_eq!(cursor_on_page(&items, ""), None);
        assert_eq!(cursor_on_page(&[at("x", None)], "@1"), None);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::{Agent, Kind};

    fn item(name: &str, status: Status, cwd: &str, branch: Option<&str>, msg: Option<&str>, window: &str) -> Item {
        Item {
            entry: Entry {
                agent: Agent {
                    name: name.to_string(),
                    kind: Kind::Claude,
                    command: "claude".to_string(),
                    cwd: cwd.to_string(),
                    created_at: 0,
                    status,
                    status_since: 1000,
                    window_id: Some(window.to_string()),
                    pane_id: Some("%1".to_string()),
                    session_id: None,
                    last_message: msg.map(str::to_string),
                    model: None,
                    effort: None,
                    label: None,
                },
                status,
                window: Some(window.to_string()),
            },
            branch: branch.map(str::to_string),
        }
    }

    fn plain(l: &BodyLine) -> String {
        l.line.spans.iter().map(|s| s.content.as_ref()).collect()
    }

    #[test]
    fn fit_respects_display_width() {
        assert_eq!(fit("abc", 5), "abc");
        assert_eq!(fit("abcdef", 4), "abc…");
        assert_eq!(fit("中文名字很长", 7), "中文名…");
        assert_eq!(fit("中文名字很长", 6), "中文…");
        assert_eq!(fit("abc", 0), "");
    }

    #[test]
    fn block_has_three_lines_with_message() {
        let items = vec![item("claude-myproj", Status::NeedsReply, "/w/myproj", Some("main"), Some("已经把 README\n改好了"), "@1")];
        let lines = build_lines(&items, 0, "@9", 32, 1000 + 180);
        let text: Vec<String> = lines.iter().map(plain).collect();
        assert_eq!(text, [" ● claude-myproj", "   myproj · main · 等你回复 3分", "   已经把 README 改好了"]);
        assert!(lines.iter().all(|l| l.item == 0));
    }

    #[test]
    fn no_message_means_two_lines_and_no_branch_ok() {
        let items = vec![item("a", Status::Running, "/w/p", None, None, "@1")];
        let text: Vec<String> = build_lines(&items, 0, "", 32, 1010).iter().map(plain).collect();
        assert_eq!(text, [" ● a", "   p · 运行中 刚刚"]);
    }

    #[test]
    fn current_window_gets_marker_and_bold() {
        let items = vec![item("a", Status::Idle, "/w/p", None, None, "@3"), item("b", Status::Idle, "/w/q", None, None, "@4")];
        let lines = build_lines(&items, 1, "@3", 32, 1000);
        assert!(plain(&lines[0]).starts_with('▎'));
        assert!(lines[0].line.spans[3].style.add_modifier.contains(Modifier::BOLD));
        assert!(plain(&lines[2]).starts_with(' '));
        assert!(!lines[2].line.spans[3].style.add_modifier.contains(Modifier::BOLD));
        assert_eq!(lines[2].line.style.bg, Some(CURSOR_BG));
        assert_eq!(lines[0].line.style.bg, None);
    }

    #[test]
    fn narrow_width_shows_name_only() {
        let items = vec![item("claude-myproj", Status::Running, "/w/p", Some("m"), Some("x"), "@1")];
        let text: Vec<String> = build_lines(&items, 0, "", 12, 1000).iter().map(plain).collect();
        assert_eq!(text, [" ● claude-m…"]);
    }

    #[test]
    fn status_dot_colors() {
        let items = vec![
            item("a", Status::NeedsApproval, "/x", None, None, "@1"),
            item("b", Status::NeedsReply, "/x", None, None, "@2"),
            item("c", Status::Running, "/x", None, None, "@3"),
        ];
        let lines = build_lines(&items, 0, "", 32, 1000);
        let dot = |i: usize| lines[i].line.spans[1].style.fg;
        assert_eq!(dot(0), Some(Color::Yellow));
        assert_eq!(dot(2), Some(Color::LightBlue));
        assert_eq!(dot(4), Some(Color::Green));
    }

    #[test]
    fn scroll_keeps_cursor_block_visible() {
        let items: Vec<Item> = (0..5).map(|i| item(&format!("a{i}"), Status::Idle, "/x", None, None, "@1")).collect();
        let lines = build_lines(&items, 0, "", 32, 1000); // 每个 2 行，共 10 行
        assert_eq!(scroll_offset(&lines, 0, 4), 0);
        assert_eq!(scroll_offset(&lines, 4, 4), 6); // 最后一块占 8..10，窗口 4 行 → 从 6 开始
        assert_eq!(scroll_offset(&lines, 2, 4), 2); // 块 4..6 → 偏移 2 即可看到
    }
}
