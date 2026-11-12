mod agents;
mod waimai_window;
mod antigravity;
mod config;
mod files;
mod goals;
mod models;
mod workbuddy;
mod layout;
mod limits;
mod jev;
mod notify;
mod pm;
mod power;
mod lid;
mod pty;
mod resources;
mod search;
mod seen;
mod session;
mod statusline;
mod tasks;

use agents::{AgentView, Branches};
use config::Config;
use portable_pty::PtySize;
use pty::Ptys;
use tasks::{Changed, Pick, TaskFile, Tasks};
use shifu::menu::load_recent;
use shifu::tmux::find_tmux_bin;
use std::io::Write;
use std::path::Path;
use std::process::{Command, Stdio};
use tauri::menu::{Menu, MenuBuilder, MenuItemBuilder, SubmenuBuilder};
use tauri::{AppHandle, Emitter, Manager, State};

fn size(cols: u16, rows: u16) -> PtySize {
    PtySize { rows, cols, pixel_width: 0, pixel_height: 0 }
}

#[tauri::command]
async fn waimai_open(app: AppHandle, cfg: State<'_, Config>) -> Result<(), String> {
    shifu::waimai::load(&cfg.home)?;
    let available = tauri::async_runtime::spawn_blocking(|| {
        std::net::TcpStream::connect_timeout(&"127.0.0.1:3816".parse().unwrap(), std::time::Duration::from_secs(2)).is_ok()
    }).await.map_err(|_| "无法检查点餐后台".to_string())?;
    if !available { return Err("点餐后台尚未启动，请先启动已连接的点餐服务".into()); }
    if let Some(window) = app.get_webview_window("waimai") {
        waimai_window::place(&app, &window)?;
        window.show().map_err(|e| e.to_string())?;
        return window.set_focus().map_err(|e| e.to_string());
    }
    let window = tauri::WebviewWindowBuilder::new(&app, "waimai", tauri::WebviewUrl::External(shifu::waimai::URL.parse().unwrap()))
        .title("Shifu · 点餐").visible(false).inner_size(480.0, 720.0).min_inner_size(420.0, 480.0)
        .background_color(tauri::window::Color(255, 255, 255, 255))
        .on_navigation(|url| url.scheme() == "http" && url.host_str() == Some("127.0.0.1") && url.port() == Some(3816))
        .on_new_window(|url, _| {
            if waimai_window::allowed(&url) && files::open(url.as_str()).is_err() {
                eprintln!("无法打开官方收银台，请在官方应用订单页继续付款");
            }
            tauri::webview::NewWindowResponse::Deny
        })
        .build().map_err(|e| format!("无法打开点餐窗口：{e}"))?;
    waimai_window::place(&app, &window)?;
    window.show().map_err(|e| e.to_string())?;
    window.set_focus().map_err(|e| e.to_string())
}

#[tauri::command]
fn pty_write(ptys: State<Ptys>, id: u32, data: String) -> Result<(), String> {
    ptys.write(id, &data)
}

#[tauri::command]
fn pty_resize(ptys: State<Ptys>, id: u32, cols: u16, rows: u16) -> Result<(), String> {
    ptys.resize(id, size(cols, rows))
}

#[tauri::command]
fn pty_kill(ptys: State<Ptys>, id: u32) -> Result<(), String> {
    ptys.kill(id)
}

#[tauri::command]
fn agents_list(branches: State<Branches>) -> Vec<AgentView> {
    agents::snapshot(&branches)
}

/// Progress page (GOALS.md files and where each agent works). Reads transcripts, so it runs off the main thread.
#[tauri::command]
async fn goals_get() -> goals::Goals {
    let (agents, _) = shifu::agent::list();
    let places: Vec<goals::AgentWhere> = agents
        .iter()
        .map(|a| goals::AgentWhere { name: &a.name, kind: a.kind.as_str(), cwd: &a.cwd, session_id: a.session_id.as_deref() })
        .collect();
    goals::collect(&places)
}

/// The live agents whose conversations mention `query`. Reads transcripts, so it runs off the main thread.
#[tauri::command]
async fn search_conversations(cache: State<'_, search::SearchCache>, query: String) -> Result<Vec<search::Hit>, String> {
    let agents = shifu::agent::list().0;
    Ok(cache.search(&search_sessions(&agents), &query))
}

fn search_sessions(agents: &[shifu::agent::Agent]) -> Vec<search::Session<'_>> {
    agents
        .iter()
        .filter_map(|a| Some(search::Session { agent: &a.name, kind: a.kind.as_str(), session_id: a.session_id.as_deref()? }))
        .collect()
}

/// Memory per live agent and for the machine. Spawns `ps` and tmux, so off the main thread.
#[tauri::command]
async fn resources_get() -> resources::Resources {
    resources::collect()
}

#[tauri::command]
async fn power_get(power: State<'_, power::Power>) -> Result<power::Status, String> {
    power.get()
}

#[tauri::command]
async fn power_set(power: State<'_, power::Power>, mode: power::Mode) -> Result<power::Status, String> {
    power.set(mode)
}

#[tauri::command]
async fn power_timer(power: State<'_, power::Power>, minutes: Option<u32>) -> Result<power::Status, String> {
    power.timer(minutes)
}

#[tauri::command]
async fn power_lid(power: State<'_, power::Power>, enabled: bool) -> Result<power::Status, String> {
    power.set_lid(enabled)
}

#[tauri::command]
async fn power_lid_setup(power: State<'_, power::Power>) -> Result<power::Status, String> {
    power.authorize_lid()
}

/// Models each agent CLI offers here (Claude, Codex, CodeBuddy). Starts two CLIs briefly, so off the main thread.
#[tauri::command]
async fn models_list(cfg: State<'_, Config>) -> Result<Vec<models::AgentModels>, String> {
    Ok(models::collect(&cfg.home.join("model-probe")))
}

#[tauri::command]
fn agent_new(cfg: State<Config>, kind: String, cwd: String) -> Result<String, String> {
    session::create_agent(&cfg, &kind, &cwd, None, None)
}

/// Ends the agent and takes it off its task.
#[tauri::command]
fn agent_kill(cfg: State<Config>, tasks: State<Tasks>, name: String) -> Result<TaskFile, String> {
    tasks.kill_agent(&cfg, &name)
}

/// `null` before the first start of this version.
#[tauri::command]
fn seen_load(cfg: State<Config>) -> Result<Option<seen::Seen>, String> {
    seen::load(&cfg)
}

#[tauri::command]
fn seen_save(cfg: State<Config>, seen: seen::Seen) -> Result<(), String> {
    seen::save(&cfg, &seen)
}

/// Window title and Dock badge: how many agents need the user.
#[tauri::command]
fn set_attention(app: AppHandle, count: usize) {
    agents::show_waiting(&app, count);
}

/// A macOS notification for an agent that needs the user; a click on it comes back as the `notify-jump` event.
#[tauri::command]
fn notify_send(app: AppHandle, agent: String, title: String, body: String) -> Result<(), String> {
    notify::send(app, agent, title, body)
}

/// The end of Jev's `waiting-log.jsonl` (APP-SPEC-C1); `None` when there is no such file.
#[tauri::command]
fn jev_log_tail() -> Option<String> {
    jev::log_tail()
}

/// Plan limits for the sidebar footer; later changes arrive as the `limits` event.
#[tauri::command]
fn limits_get(cfg: State<Config>, slow: State<limits::SlowLimits>) -> limits::Limits {
    limits::collect(&cfg, &mut limits::CodexCache::default(), &slow)
}

#[tauri::command]
fn tasks_load(cfg: State<Config>, tasks: State<Tasks>) -> Result<TaskFile, String> {
    tasks.load(&cfg)
}

/// The project manager's folder (its task is the one whose project is this folder).
#[tauri::command]
fn pm_dir(cfg: State<Config>) -> Result<String, String> {
    pm::ensure_dir(&cfg)
}

/// The project manager's task with an agent in it (`kind`, as picked the first time): started the first time and
/// again after it was ended. It works in its own folder, which holds its instructions.
#[tauri::command]
fn pm_open(
    cfg: State<Config>,
    tasks: State<Tasks>,
    kind: String,
    model: Option<String>,
    effort: Option<String>,
) -> Result<Changed, String> {
    let dir = pm::ensure_dir(&cfg)?;
    let file = tasks.load(&cfg)?;
    let pick = Pick { model: model.as_deref(), effort: effort.as_deref() };
    match file.tasks.iter().find(|t| t.project == dir) {
        // Ending it (the tab's 叉) leaves the task empty; a click on the row starts it again.
        Some(t) if t.agents.is_empty() => {
            let id = t.id.clone();
            tasks.add_agent(&cfg, &id, &kind, pick)
        }
        Some(_) => Ok(Changed { file, agent: None }),
        None => tasks.create(&cfg, &dir, pm::TASK_NAME, &kind, pick),
    }
}

/// `name` may be empty (the task is then called "任务 N").
#[tauri::command]
fn task_new(
    cfg: State<Config>,
    tasks: State<Tasks>,
    project: String,
    name: String,
    kind: String,
    model: Option<String>,
    effort: Option<String>,
) -> Result<Changed, String> {
    tasks.create(&cfg, &project, &name, &kind, Pick { model: model.as_deref(), effort: effort.as_deref() })
}

#[tauri::command]
fn task_add_agent(
    cfg: State<Config>,
    tasks: State<Tasks>,
    id: String,
    kind: String,
    model: Option<String>,
    effort: Option<String>,
) -> Result<Changed, String> {
    tasks.add_agent(&cfg, &id, &kind, Pick { model: model.as_deref(), effort: effort.as_deref() })
}

/// The user's own name for an agent (its tab); blank goes back to the conversation topic.
#[tauri::command]
fn agent_rename(name: String, label: String) -> Result<(), String> {
    let mut record = shifu::agent::load(&name)?.ok_or_else(|| format!("no agent named {name}"))?;
    let label = label.trim();
    record.label = (!label.is_empty()).then(|| label.to_string());
    shifu::agent::save(&record)
}

#[tauri::command]
fn task_rename(cfg: State<Config>, tasks: State<Tasks>, id: String, name: String) -> Result<TaskFile, String> {
    tasks.rename(&cfg, &id, &name)
}

#[tauri::command]
fn task_set_current(cfg: State<Config>, tasks: State<Tasks>, id: String) -> Result<TaskFile, String> {
    tasks.set_current(&cfg, &id)
}

#[tauri::command]
fn task_move_agent(
    cfg: State<Config>,
    tasks: State<Tasks>,
    name: String,
    to_id: String,
) -> Result<TaskFile, String> {
    tasks.move_agent(&cfg, &name, &to_id)
}

#[tauri::command]
fn task_delete(cfg: State<Config>, tasks: State<Tasks>, id: String) -> Result<TaskFile, String> {
    tasks.delete(&cfg, &id)
}

#[tauri::command]
fn path_reveal(path: String) -> Result<(), String> {
    session::reveal(&path)
}

/// Which of the paths written in a terminal exist; relative ones are taken from the agent's directory.
#[tauri::command]
fn paths_resolve(paths: Vec<String>, cwd: String) -> Vec<Option<files::Resolved>> {
    let home = std::env::var("HOME").unwrap_or_default();
    paths.iter().take(64).map(|p| files::resolve(p, &cwd, &home)).collect()
}

#[tauri::command]
fn path_open(target: String) -> Result<(), String> {
    files::open(&target)
}

/// Raw bytes, so a screenshot is not inflated into a JSON number array.
#[tauri::command]
fn file_read(path: String) -> Result<tauri::ipc::Response, String> {
    files::read(&path).map(tauri::ipc::Response::new)
}

#[tauri::command]
fn file_save(path: String, expected: String, text: String) -> Result<(), String> {
    files::save_markdown(&path, &expected, &text)
}

/// Body: the dropped file's bytes; header `x-name`: its file name (percent-encoded, headers are ASCII).
#[tauri::command]
fn drop_save(cfg: State<Config>, request: tauri::ipc::Request) -> Result<String, String> {
    let tauri::ipc::InvokeBody::Raw(bytes) = request.body() else { return Err("没有收到文件内容".into()) };
    let name = request.headers().get("x-name").and_then(|v| v.to_str().ok()).unwrap_or("");
    let name = percent_decode(name);
    let stamp = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
    files::save_drop(&cfg.home.join("drops"), &name, bytes, stamp)
}

fn percent_decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        let hex = (b[i] == b'%' && i + 2 < b.len())
            .then(|| std::str::from_utf8(&b[i + 1..i + 3]).ok().and_then(|h| u8::from_str_radix(h, 16).ok()))
            .flatten();
        if let Some(v) = hex {
            out.push(v);
            i += 3;
            continue;
        }
        out.push(b[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

#[tauri::command]
fn app_name() -> &'static str {
    config::app_name()
}

#[tauri::command]
fn agent_resume(cfg: State<Config>, name: String) -> Result<(), String> {
    session::resume_agent(&cfg, &name)
}

#[tauri::command]
fn agent_reveal(name: String) -> Result<(), String> {
    let record = shifu::agent::load(&name)?.ok_or_else(|| format!("no agent named {name}"))?;
    session::reveal(&record.cwd)
}

/// Recently used directories that still exist.
#[tauri::command]
fn recent_dirs() -> Result<Vec<String>, String> {
    Ok(load_recent()?.into_iter().filter(|d| Path::new(d).is_dir()).collect())
}

/// Opens a PTY running `tmux attach` for the agent's session.
#[tauri::command]
fn term_attach(
    app: AppHandle,
    ptys: State<Ptys>,
    cfg: State<Config>,
    name: String,
    cols: u16,
    rows: u16,
) -> Result<u32, String> {
    let tmux = find_tmux_bin().to_string_lossy().to_string();
    let args = ["-L", &cfg.socket, "attach", "-t", &session::exact_target(&name)].map(String::from).to_vec();
    ptys.spawn(app, &tmux, &args, size(cols, rows), None, &[])
}

#[tauri::command]
fn term_shift_enter(name: String) -> Result<(), String> {
    session::send_shift_enter(&name)
}

#[tauri::command]
fn term_leave_scroll(name: String) -> Result<(), String> {
    session::leave_scroll(&name)
}

/// Text a mouse selection in tmux handed over (OSC 52). `pbcopy` reads UTF-8 because `Config::init` set LANG.
#[tauri::command]
fn clipboard_write(text: String) -> Result<(), String> {
    let mut child = Command::new("pbcopy").stdin(Stdio::piped()).spawn().map_err(|e| format!("pbcopy failed to start: {e}"))?;
    let mut stdin = child.stdin.take().ok_or("pbcopy has no stdin")?;
    stdin.write_all(text.as_bytes()).map_err(|e| format!("pbcopy write failed: {e}"))?;
    drop(stdin);
    let status = child.wait().map_err(|e| format!("pbcopy failed: {e}"))?;
    if status.success() { Ok(()) } else { Err(format!("pbcopy exited with {status}")) }
}

#[tauri::command]
fn layout_save(cfg: State<Config>, task_id: String, json: String) -> Result<(), String> {
    layout::save(&cfg, &task_id, &json)
}

#[tauri::command]
fn layout_load(cfg: State<Config>, task_id: String) -> Result<Option<String>, String> {
    layout::load(&cfg, &task_id)
}

/// Cmd+N / Cmd+T / Cmd+W / Cmd+J (and Cmd+K, Cmd+B, Cmd+D, Shift+Cmd+D) live in the native menu: macOS handles menu shortcuts before the page sees the key,
/// and the stock menu would make Cmd+W close the whole window.
fn build_menu(app: &AppHandle) -> tauri::Result<Menu<tauri::Wry>> {
    let item = |id: &str, text: &str, accel: &str| MenuItemBuilder::with_id(id, text).accelerator(accel).build(app);
    let app_menu = SubmenuBuilder::new(app, "Shifu")
        .about(None)
        .separator()
        .item(&item("appearance", "外观…", "CmdOrCtrl+,")?)
        .separator()
        .hide()
        .hide_others()
        .show_all()
        .separator()
        .quit()
        .build()?;
    let file_menu = SubmenuBuilder::new(app, "文件")
        .item(&item("new-task", "新建任务", "CmdOrCtrl+N")?)
        .item(&item("new", "在当前任务里加 agent", "CmdOrCtrl+T")?)
        .item(&item("close", "关闭面板", "CmdOrCtrl+W")?)
        .separator()
        .item(&item("jump", "跳到等你最久的 agent", "CmdOrCtrl+J")?)
        .build()?;
    let edit_menu = SubmenuBuilder::new(app, "编辑")
        .undo()
        .redo()
        .separator()
        .cut()
        .copy()
        .paste()
        .select_all()
        .build()?;
    let view_menu = SubmenuBuilder::new(app, "显示")
        .item(&item("search", "搜索对话", "CmdOrCtrl+K")?)
        .item(&item("sidebar", "显示或隐藏侧栏", "CmdOrCtrl+B")?)
        .separator()
        .item(&item("split-down", "上下分屏", "CmdOrCtrl+Shift+D")?)
        .item(&item("split-right", "左右分屏", "CmdOrCtrl+D")?)
        .build()?;
    let window_menu = SubmenuBuilder::new(app, "窗口").minimize().maximize().build()?;
    MenuBuilder::new(app).items(&[&app_menu, &file_menu, &edit_menu, &view_menu, &window_menu]).build()
}

fn main() {
    let args: Vec<_> = std::env::args_os().collect();
    if args.get(1).is_some_and(|a| a == "--power-watchdog") {
        if let Some(file) = args.get(2) {
            if let Err(e) = lid::watchdog(Path::new(file)) { eprintln!("Shifu 合盖恢复：{e}"); std::process::exit(1); }
        } else { std::process::exit(2); }
        return;
    }
    let cfg = Config::init();
    std::fs::create_dir_all(&cfg.home).unwrap_or_else(|e| panic!("cannot create {}: {e}", cfg.home.display()));
    let power = power::Power::new(&cfg.home);
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .manage(cfg)
        .manage(Ptys::default())
        .manage(Branches::default())
        .manage(Tasks::default())
        .manage(limits::SlowLimits::default())
        .manage(search::SearchCache::default())
        .manage(power)
        .manage(waimai_window::MainRestore::default())
        .menu(build_menu)
        .on_menu_event(|app, event| {
            if let Err(e) = app.emit("menu", event.id().as_ref()) {
                eprintln!("shifu-app: emit menu failed: {e}");
            }
        })
        .on_window_event(|window, event| {
            // Start restoring with the close request, before the meal window disappears.
            if window.label() == "waimai" && matches!(event, tauri::WindowEvent::CloseRequested { .. } | tauri::WindowEvent::Destroyed) {
                if let Err(error) = waimai_window::restore(window.app_handle()) {
                    eprintln!("无法恢复 Shifu 窗口：{error}");
                }
            }
        })
        .setup(|app| {
            power::spawn_watcher(app.handle().clone());
            agents::spawn_watcher(app.handle().clone());
            limits::spawn_watcher(app.handle().clone());
            // Read the open conversations once now, so the first search does not wait (about a second for 150 MB).
            let handle = app.handle().clone();
            std::thread::spawn(move || {
                let agents = shifu::agent::list().0;
                handle.state::<search::SearchCache>().warm(&search_sessions(&agents));
            });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            power_get,
            power_set,
            power_timer,
            power_lid,
            power_lid_setup,
            pty_write,
            pty_resize,
            pty_kill,
            agents_list,
            goals_get,
            search_conversations,
            pm_dir,
            resources_get,
            pm_open,
            waimai_open,
            models_list,
            agent_new,
            agent_kill,
            seen_load,
            seen_save,
            set_attention,
            notify_send,
            jev_log_tail,
            limits_get,
            tasks_load,
            task_new,
            task_add_agent,
            task_rename,
            agent_rename,
            task_set_current,
            task_move_agent,
            task_delete,
            path_reveal,
            paths_resolve,
            path_open,
            file_read,
            file_save,
            drop_save,
            app_name,
            agent_resume,
            agent_reveal,
            recent_dirs,
            term_attach,
            layout_save,
            layout_load,
            clipboard_write,
            term_shift_enter,
            term_leave_scroll
        ])
        .build(tauri::generate_context!())
        .expect("error while building Shifu")
        .run(|app, event| {
            if matches!(event, tauri::RunEvent::Exit)
                && let Err(e) = app.state::<power::Power>().set(power::Mode::Off) {
                eprintln!("Shifu 合盖恢复：{e}");
            }
        });
}
