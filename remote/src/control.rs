//! Controller side from the command line: reach a paired remote Mac's sessions with the
//! credential the desktop app already saved, so agents can list, read and reply without
//! touching the Shifu window on screen. Same relay route and Noise channel as the app.

use base64::{Engine, engine::general_purpose::STANDARD};
use futures_util::{SinkExt, StreamExt};
use serde::Deserialize;
use serde_json::{Value, json};
use std::path::PathBuf;
use std::time::{Duration, Instant};
use tokio_tungstenite::{
    MaybeTlsStream, WebSocketStream, connect_async,
    tungstenite::{Message, client::IntoClientRequest},
};

type Result<T> = super::Result<T>;
type Socket = WebSocketStream<MaybeTlsStream<tokio::net::TcpStream>>;

pub const HELP: &str = "控制已配对的远端 Mac（不改动本机 Shifu 窗口）\n\nshifu remote macs                                   列出桌面 App 已保存的远端 Mac\nshifu remote agents [--all] [--json]                列出远端会话及状态（--all 含已退出的）\nshifu remote new 项目 [claude|codex]                在远端已有项目里新开一个会话\nshifu remote tell 会话 文字… [--done [秒]]          发文字并回车；--done 等它停下再返回屏幕（- 表示从标准输入读）\nshifu remote keys 会话 键… [--done [秒]]            按键：enter esc tab up down left right space backspace ctrl-c，或单个字符\nshifu remote wait 会话… [--all] [--timeout 秒]      等会话停下（等你回复、待批准或退出）再返回屏幕；已停着的立即返回；默认任一个停下就返回\nshifu remote read 会话 [--wait 秒]                  读取远端会话当前屏幕\nshifu remote close 会话                             让该会话里的 Agent 退出\nshifu remote mcp                                    以 MCP 服务（stdio）提供以上能力\n\n通用：--mac 名称（只保存一台时可省略）。会话可写名字（claude-Workspace-7）或标题。";

#[derive(Deserialize)]
struct Credential {
    id: String,
    key: String,
}

#[derive(Deserialize)]
struct Mac {
    name: String,
    relay: String,
    host: String,
    credential: Credential,
}

fn store_path() -> PathBuf {
    std::env::var_os("SHIFU_HOME")
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(std::env::var_os("HOME").unwrap_or_default()).join(".shifu/app"))
        .join("remote-macs.json")
}

fn hex(value: &str, length: usize) -> bool {
    value.len() == length && value.bytes().all(|b| b.is_ascii_hexdigit())
}

fn load() -> Result<Vec<Mac>> {
    let path = store_path();
    let bytes = match std::fs::read(&path) {
        Ok(bytes) => bytes,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => return Err(format!("无法读取已保存的远端 Mac：{e}")),
    };
    let macs: Vec<Mac> = serde_json::from_slice(&bytes).map_err(|_| "已保存的远端 Mac 资料损坏")?;
    if macs.iter().any(|m| !hex(&m.host, 32) || !hex(&m.credential.id, 32) || !hex(&m.credential.key, 64)
        || !(m.relay.starts_with("https://") || m.relay.starts_with("http://127.0.0.1") || m.relay.starts_with("http://localhost")))
    {
        return Err("已保存的远端 Mac 资料无效，请在桌面 App 里重新配对".into());
    }
    Ok(macs)
}

fn pick(name: Option<&str>) -> Result<Mac> {
    let mut macs = load()?;
    let names = || macs.iter().map(|m| m.name.as_str()).collect::<Vec<_>>().join("、");
    let index = match name {
        Some(name) => macs.iter().position(|m| m.name == name)
            .ok_or_else(|| format!("没有叫 {name:?} 的远端 Mac。已保存：{}", names()))?,
        None if macs.len() == 1 => 0,
        None if macs.is_empty() => return Err("还没有保存远端 Mac，请先在桌面 Shifu 的「远程连接」里配对".into()),
        None => return Err(format!("保存了多台远端 Mac，请用 --mac 指定：{}", names())),
    };
    Ok(macs.swap_remove(index))
}

fn key(hex: &str) -> Vec<u8> {
    (0..hex.len()).step_by(2).map(|i| u8::from_str_radix(&hex[i..i + 2], 16).unwrap_or(0)).collect()
}

struct Link {
    socket: Socket,
    cipher: shifu_channel::Channel,
}

impl Link {
    async fn open(mac: &Mac) -> Result<Self> {
        let url = format!("{}/relay/view/{}", mac.relay.replacen("https://", "wss://", 1).replacen("http://", "ws://", 1), mac.host);
        let mut request = url.into_client_request().map_err(|e| e.to_string())?;
        // The relay admits viewers from the desktop app's origin; the remote Mac still checks the pairing credential.
        request.headers_mut().insert("origin", "tauri://localhost".parse().unwrap());
        let (mut socket, _) = tokio::time::timeout(Duration::from_secs(15), connect_async(request)).await
            .map_err(|_| "连接中继超时")?.map_err(|e| format!("无法连接中继：{e}"))?;
        let mut cipher = shifu_channel::Channel::new(true, &key(&mac.credential.key), &format!("shifu-relay-v1/{}/{}", mac.host, mac.credential.id))?;
        socket.send(Message::Text(json!({"credential": mac.credential.id}).to_string().into())).await.map_err(|e| e.to_string())?;
        socket.send(Message::Binary(cipher.write(&[])?.into())).await.map_err(|e| e.to_string())?;
        let mut link = Self { socket, cipher };
        let reply = link.frame(Duration::from_secs(15)).await?;
        link.cipher.read(&reply)?;
        match link.next(Duration::from_secs(15)).await? {
            v if v["type"] == "authenticated" => Ok(link),
            v if v["type"] == "revoked" => Err("此设备已被远端撤销，请在桌面 App 里重新配对".into()),
            v => Err(format!("远端没有确认身份：{}", v["type"])),
        }
    }

    async fn frame(&mut self, limit: Duration) -> Result<Vec<u8>> {
        let deadline = Instant::now() + limit;
        loop {
            let left = deadline.saturating_duration_since(Instant::now());
            match tokio::time::timeout(left, self.socket.next()).await {
                Err(_) => return Err("远端响应超时".into()),
                Ok(Some(Ok(Message::Binary(bytes)))) => return Ok(bytes.to_vec()),
                // The relay speaks plain text only when the remote Mac is not connected.
                Ok(Some(Ok(Message::Text(_)))) => return Err("远端 Mac 当前不在线：请确认它已联网并开着 Shifu".into()),
                Ok(Some(Ok(Message::Ping(_) | Message::Pong(_) | Message::Frame(_)))) => continue,
                Ok(Some(Ok(Message::Close(_))) | None) => return Err("连接已被关闭".into()),
                Ok(Some(Err(e))) => return Err(format!("连接中断：{e}")),
            }
        }
    }

    async fn next(&mut self, limit: Duration) -> Result<Value> {
        let bytes = self.frame(limit).await?;
        serde_json::from_slice(&self.cipher.read(&bytes)?).map_err(|_| "远端返回了无法识别的消息".into())
    }

    async fn send(&mut self, value: Value) -> Result<()> {
        let bytes = self.cipher.write(value.to_string().as_bytes())?;
        self.socket.send(Message::Binary(bytes.into())).await.map_err(|e| format!("发送失败：{e}"))
    }

    async fn request(&mut self, op: Value, reply: &str) -> Result<Value> {
        self.send(op).await?;
        loop {
            let v = self.next(Duration::from_secs(30)).await?;
            if v["id"] == 2 && v["type"] == reply { return Ok(v); }
            if v["id"] == 2 && v["type"] == "request_error" { return Err(v["message"].as_str().unwrap_or("远端请求失败").to_string()); }
        }
    }

    async fn agents(&mut self) -> Result<Vec<Value>> {
        self.send(json!({"op": "list", "id": 1})).await?;
        loop {
            let v = self.next(Duration::from_secs(15)).await?;
            if v["type"] == "agents" && v["id"] == 1 {
                return v["agents"].as_array().cloned().ok_or_else(|| "远端返回了无效的会话列表".into());
            }
        }
    }

    async fn session(&mut self, wanted: &str) -> Result<String> {
        resolve(&self.agents().await?, wanted)
    }

    /// Opens the terminal and returns its size once the remote side reports it ready.
    async fn attach(&mut self, name: &str, screen: &mut Option<vt100::Parser>) -> Result<()> {
        self.send(json!({"op": "open", "name": name})).await?;
        loop {
            if let Event::Ready = self.terminal(screen, Duration::from_secs(15)).await? {
                return Ok(());
            }
        }
    }

    async fn terminal(&mut self, screen: &mut Option<vt100::Parser>, limit: Duration) -> Result<Event> {
        let v = self.next(limit).await?;
        match v["type"].as_str() {
            Some("terminal") => {
                if let Some(data) = v["binary"].as_str() {
                    let bytes = STANDARD.decode(data).map_err(|_| "远端终端数据无效")?;
                    if let Some(screen) = screen { screen.process(&bytes); }
                    return Ok(Event::Output);
                }
                let inner: Value = serde_json::from_str(v["text"].as_str().unwrap_or("")).unwrap_or(Value::Null);
                match inner["type"].as_str() {
                    Some("ready") => {
                        let rows = inner["rows"].as_u64().unwrap_or(40).clamp(1, 200) as u16;
                        let cols = inner["cols"].as_u64().unwrap_or(120).clamp(1, 512) as u16;
                        *screen = Some(vt100::Parser::new(rows, cols, 0));
                        Ok(Event::Ready)
                    }
                    Some("sent") => Ok(Event::Sent(inner["id"].as_u64().unwrap_or(0))),
                    Some("error" | "ended") => Err(inner["message"].as_str().unwrap_or("远端会话已结束").to_string()),
                    _ => Ok(Event::Other),
                }
            }
            Some("terminal_close") => Err("远端会话连接已关闭".into()),
            Some("revoked") => Err("此设备已被远端撤销，请在桌面 App 里重新配对".into()),
            _ => Ok(Event::Other),
        }
    }

    /// Feeds terminal output into the screen until nothing new arrives for `quiet`, or `limit` passes.
    async fn settle(&mut self, screen: &mut Option<vt100::Parser>, quiet: Duration, limit: Duration) -> Result<()> {
        // The first redraw after attaching can take a moment; quiet only counts once it has arrived.
        let deadline = Instant::now() + limit.max(Duration::from_secs(5));
        let mut drawn = false;
        while Instant::now() < deadline {
            let left = deadline.saturating_duration_since(Instant::now());
            match self.terminal(screen, if drawn { left.min(quiet) } else { left }).await {
                Ok(Event::Output) => drawn = true,
                Ok(_) => continue,
                Err(e) if e == "远端响应超时" => return Ok(()),
                Err(e) => return Err(e),
            }
        }
        Ok(())
    }

    async fn close(mut self) {
        let _ = self.send(json!({"op": "close"})).await;
        let _ = self.socket.close(None).await;
    }
}

/// Resolves a session by its stable name first, then by an exact title.
fn resolve(agents: &[Value], wanted: &str) -> Result<String> {
    if agents.iter().any(|a| a["name"] == wanted) {
        return Ok(wanted.to_string());
    }
    let titled: Vec<_> = agents.iter().filter(|a| a["title"] == wanted).collect();
    match titled.as_slice() {
        [one] => Ok(one["name"].as_str().unwrap_or_default().to_string()),
        [] => Err(format!("远端没有会话 {wanted:?}。运行 shifu remote agents 查看")),
        _ => Err(format!("有多个会话标题叫 {wanted:?}，请改用会话名")),
    }
}

enum Event {
    Ready,
    Output,
    Other,
    Sent(u64),
}

fn text(screen: &Option<vt100::Parser>) -> String {
    let Some(screen) = screen else { return String::new() };
    let rows: Vec<String> = screen.screen().rows(0, screen.screen().size().1).map(|r| r.trim_end().to_string()).collect();
    let end = rows.iter().rposition(|r| !r.is_empty()).map_or(0, |i| i + 1);
    rows[..end].join("\n")
}

#[derive(Default)]
struct Options {
    mac: Option<String>,
    json: bool,
    wait: f64,
    done: Option<f64>,
    all: bool,
    rest: Vec<String>,
}

const DONE_LIMIT: f64 = 3600.0;
const STOPPED: [&str; 4] = ["idle", "needs_reply", "needs_approval", "exited"];

fn options(args: &[String]) -> Result<Options> {
    let mut o = Options { wait: 2.0, ..Default::default() };
    let mut iter = args.iter().peekable();
    let seconds = |v: Option<&String>, max: f64, flag: &str| v.and_then(|v| v.parse().ok()).filter(|v: &f64| (0.0..=max).contains(v)).ok_or(format!("{flag} 需要 0 到 {max} 之间的秒数"));
    while let Some(arg) = iter.next() {
        match arg.as_str() {
            "--mac" => o.mac = Some(iter.next().ok_or("--mac 缺少名称")?.clone()),
            "--json" => o.json = true,
            "--all" => o.all = true,
            "--wait" => o.wait = seconds(iter.next(), 60.0, "--wait")?,
            "--done" | "--timeout" => {
                let given = iter.peek().is_some_and(|v| v.parse::<f64>().is_ok());
                o.done = Some(if given { seconds(iter.next(), DONE_LIMIT, arg)? } else { 600.0 });
            }
            "--" => { o.rest.extend(iter.by_ref().cloned()); break; }
            _ => o.rest.push(arg.clone()),
        }
    }
    Ok(o)
}

fn key_bytes(name: &str) -> Result<String> {
    Ok(match name.to_ascii_lowercase().as_str() {
        "enter" | "return" => "\r".into(),
        "esc" | "escape" => "\x1b".into(),
        "tab" => "\t".into(),
        "up" => "\x1b[A".into(),
        "down" => "\x1b[B".into(),
        "right" => "\x1b[C".into(),
        "left" => "\x1b[D".into(),
        "space" => " ".into(),
        "backspace" => "\x7f".into(),
        "ctrl-c" => "\x03".into(),
        _ if name.chars().count() == 1 => name.into(),
        _ => return Err(format!("不认识的键 {name:?}：可用 enter esc tab up down left right space backspace ctrl-c 或单个字符")),
    })
}

pub fn run(command: &str, args: &[String]) -> Result<()> {
    if command == "mcp" { return mcp(); }
    let mut o = options(args)?;
    if command == "tell" && o.rest.len() == 2 && o.rest[1] == "-" {
        let mut s = String::new();
        std::io::Read::read_to_string(&mut std::io::stdin(), &mut s).map_err(|e| e.to_string())?;
        o.rest[1] = s;
    }
    println!("{}", execute(command, &o)?);
    Ok(())
}

fn execute(command: &str, o: &Options) -> Result<String> {
    if command == "macs" {
        return Ok(load()?.iter().map(|m| format!("{}\t{}", m.name, m.relay)).collect::<Vec<_>>().join("\n"));
    }
    let mac = pick(o.mac.as_deref())?;
    tokio::runtime::Runtime::new().map_err(|e| e.to_string())?.block_on(async move {
        let mut link = Link::open(&mac).await?;
        let result = act(&mut link, command, o).await;
        link.close().await;
        result
    })
}

/// Status and the time it last changed, keyed by session name.
async fn states(link: &mut Link) -> Result<std::collections::HashMap<String, (String, i64)>> {
    Ok(snapshot(&link.agents().await?))
}

fn snapshot(agents: &[Value]) -> std::collections::HashMap<String, (String, i64)> {
    agents.iter().map(|a| (
        a["name"].as_str().unwrap_or_default().to_string(),
        (a["status"].as_str().unwrap_or_default().to_string(), a["since"].as_i64().unwrap_or(0)),
    )).collect()
}

/// Waits until the named sessions have stopped after `before` (their status-change time when the
/// request began). A session still showing its old stopped state is given a short grace period to
/// start working, then counted as already stopped. Status comes from the remote Mac's own hooks,
/// so this checks a small list every two seconds instead of streaming screens.
async fn wait_for(link: &mut Link, names: &[String], before: &std::collections::HashMap<String, (String, i64)>, all: bool, limit: f64, grace: Duration) -> Result<Vec<(String, String)>> {
    let started = Instant::now();
    loop {
        let now = states(link).await?;
        let mut done = Vec::new();
        for name in names {
            let Some((status, since)) = now.get(name) else { done.push((name.clone(), "exited".to_string())); continue };
            let changed = before.get(name).is_none_or(|(_, old)| old != since);
            if STOPPED.contains(&status.as_str()) && (changed || started.elapsed() >= grace) {
                done.push((name.clone(), status.clone()));
            }
        }
        if (all && done.len() == names.len()) || (!all && !done.is_empty()) { return Ok(done); }
        if started.elapsed().as_secs_f64() >= limit {
            return Err(format!("等了 {limit:.0} 秒仍在运行：{}", names.iter().filter(|n| !done.iter().any(|(d, _)| d == *n)).cloned().collect::<Vec<_>>().join("、")));
        }
        tokio::time::sleep(Duration::from_secs(2)).await;
    }
}

async fn screen_of(link: &mut Link, name: &str, settle: f64) -> Result<String> {
    let mut screen = None;
    link.attach(name, &mut screen).await?;
    link.settle(&mut screen, Duration::from_millis(800), Duration::from_secs_f64(settle.max(0.5))).await?;
    Ok(text(&screen))
}

fn label(status: &str) -> &str {
    match status { "needs_reply" => "等你回复", "needs_approval" => "待批准", "idle" => "空闲", "exited" => "已退出", "running" => "运行中", other => other }
}

async fn send_and_maybe_wait(link: &mut Link, name: &str, before: &std::collections::HashMap<String, (String, i64)>, data: Value, ack: bool, o: &Options) -> Result<String> {
    let mut screen = None;
    link.attach(name, &mut screen).await?;
    link.send(json!({"op": "input", "data": data.to_string()})).await?;
    if ack {
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            let left = deadline.saturating_duration_since(Instant::now());
            let unconfirmed = || "发送结果未确认，请先用 shifu remote read 查看再决定是否重发".to_string();
            if left.is_zero() { return Err(unconfirmed()); }
            match link.terminal(&mut screen, left).await {
                Ok(Event::Sent(1)) => break,
                Ok(_) => continue,
                Err(e) if e == "远端响应超时" => return Err(unconfirmed()),
                Err(e) => return Err(e),
            }
        }
    }
    let mut out = format!("已发送到 {name}");
    if let Some(limit) = o.done {
        let finished = wait_for(link, &[name.to_string()], before, true, limit, Duration::from_secs(20)).await?;
        out += &format!("\n状态：{}\n\n{}", label(&finished[0].1), screen_of(link, name, 1.0).await?);
    } else if o.wait > 0.0 {
        link.settle(&mut screen, Duration::from_millis(800), Duration::from_secs_f64(o.wait)).await?;
        out += &format!("\n\n{}", text(&screen));
    }
    Ok(out)
}

async fn act(link: &mut Link, command: &str, o: &Options) -> Result<String> {
    match command {
        "agents" => {
            let agents: Vec<Value> = link.agents().await?.into_iter().filter(|a| o.all || a["status"] != "exited").collect();
            if o.json { return Ok(Value::Array(agents).to_string()); }
            Ok(agents.iter().map(|a| {
                let s = |k: &str| a[k].as_str().unwrap_or("").to_string();
                format!("{}\t{}\t{}\t{}", s("name"), label(&s("status")), s("task_title"), s("title"))
            }).collect::<Vec<_>>().join("\n"))
        }
        "read" => {
            let wanted = o.rest.first().ok_or("用法：shifu remote read 会话 [--wait 秒]")?;
            let name = link.session(wanted).await?;
            screen_of(link, &name, o.wait).await
        }
        "tell" => {
            let (wanted, words) = o.rest.split_first().ok_or("用法：shifu remote tell 会话 文字…")?;
            let message = words.join(" ").trim_end().to_string();
            if message.is_empty() { return Err("没有要发送的文字".into()); }
            if message.len() > 16 * 1024 { return Err("每次最多发送 16 KB".into()); }
            let agents = link.agents().await?;
            let name = resolve(&agents, wanted)?;
            // Same paste-then-Enter path the desktop reply box uses, so multi-line text arrives as one message.
            send_and_maybe_wait(link, &name, &snapshot(&agents), json!({"type": "paste", "id": 1, "text": message}), true, o).await
        }
        "keys" => {
            let (wanted, keys) = o.rest.split_first().ok_or("用法：shifu remote keys 会话 键…")?;
            if keys.is_empty() { return Err("没有要按的键".into()); }
            let data = keys.iter().map(|k| key_bytes(k)).collect::<Result<String>>()?;
            let agents = link.agents().await?;
            let name = resolve(&agents, wanted)?;
            send_and_maybe_wait(link, &name, &snapshot(&agents), json!({"type": "input", "data": data}), false, o).await
        }
        "wait" => {
            if o.rest.is_empty() { return Err("用法：shifu remote wait 会话… [--all] [--timeout 秒]".into()); }
            let agents = link.agents().await?;
            let names = o.rest.iter().map(|w| resolve(&agents, w)).collect::<Result<Vec<_>>>()?;
            let before = snapshot(&agents);
            // Nothing was just sent here, so a session that is already stopped is already done.
            let stopped: Vec<(String, String)> = names.iter().filter_map(|n| match before.get(n) {
                Some((status, _)) if STOPPED.contains(&status.as_str()) => Some((n.clone(), status.clone())),
                None => Some((n.clone(), "exited".to_string())),
                _ => None,
            }).collect();
            let finished = if (o.all && stopped.len() == names.len()) || (!o.all && !stopped.is_empty()) { stopped }
                else { wait_for(link, &names, &before, o.all, o.done.unwrap_or(600.0), Duration::ZERO).await? };
            let mut out = Vec::new();
            for (name, status) in finished {
                let screen = if status == "exited" { String::new() } else { screen_of(link, &name, 1.0).await? };
                out.push(format!("== {name}：{}\n{screen}", label(&status)));
            }
            Ok(out.join("\n\n"))
        }
        "close" => {
            let wanted = o.rest.first().ok_or("用法：shifu remote close 会话")?;
            let agents = link.agents().await?;
            let name = resolve(&agents, wanted)?;
            let kind = agents.iter().find(|a| a["name"] == name.as_str()).and_then(|a| a["kind"].as_str().map(str::to_string)).unwrap_or_default();
            let command = match kind.as_str() { "claude" => "/exit", "codex" => "/quit", other => return Err(format!("不知道怎么让 {other:?} 类会话退出")) };
            let before = snapshot(&agents);
            send_and_maybe_wait(link, &name, &before, json!({"type": "paste", "id": 1, "text": command}), true, &Options::default()).await?;
            let finished = wait_for(link, &[name.clone()], &before, true, 30.0, Duration::from_secs(20)).await?;
            Ok(format!("{name}：{}", label(&finished[0].1)))
        }
        "new" => {
            let (project, rest) = o.rest.split_first().ok_or("用法：shifu remote new 项目 [claude|codex]")?;
            let kind = rest.first().map(String::as_str).unwrap_or("claude");
            let catalog = link.request(json!({"op": "catalog", "id": 2}), "catalog").await?;
            let projects = catalog["catalog"]["projects"].as_array().cloned().unwrap_or_default();
            let path = projects.iter().find(|p| p["name"] == project.as_str() || p["path"] == project.as_str())
                .and_then(|p| p["path"].as_str().map(str::to_string))
                .ok_or_else(|| format!("远端没有项目 {project:?}。可用：{}", projects.iter().filter_map(|p| p["name"].as_str()).collect::<Vec<_>>().join("、")))?;
            // A create whose reply is lost may still have happened, so never retry it automatically.
            let created = link.request(json!({"op": "create", "id": 2, "project": path, "kind": kind}), "created").await
                .map_err(|e| format!("{e}（是否已创建未确认，请先运行 shifu remote agents 查看）"))?;
            Ok(created["name"].as_str().unwrap_or_default().to_string())
        }
        other => Err(format!("不认识 {other:?}。\n\n{HELP}")),
    }
}

// ---------- MCP (stdio JSON-RPC) ----------

fn tools() -> Value {
    let session = json!({"type": "string", "description": "会话名（如 claude-Workspace-7）或完整标题"});
    let mac = json!({"type": "string", "description": "远端 Mac 名称；只保存一台时可省略"});
    let done = json!({"type": "number", "description": "等会话停下（等回复、待批准或退出）的最长秒数；给了就等它做完再返回屏幕"});
    json!([
        {"name": "remote_agents", "description": "列出远端 Mac 上的会话：名字、状态、任务、标题。默认不含已退出的。", "inputSchema": {"type": "object", "properties": {"include_exited": {"type": "boolean"}, "mac": mac}}},
        {"name": "remote_read", "description": "读取远端会话当前一屏的文字。", "inputSchema": {"type": "object", "properties": {"session": session, "mac": mac}, "required": ["session"]}},
        {"name": "remote_tell", "description": "给远端会话发一段文字并回车（回答它的追问也用这个）。给 done_seconds 时，等它做完停下再返回结果屏幕，无需轮询。", "inputSchema": {"type": "object", "properties": {"session": session, "text": {"type": "string"}, "done_seconds": done, "mac": mac}, "required": ["session", "text"]}},
        {"name": "remote_keys", "description": "在远端会话里按键，用于确认框或选项：enter esc tab up down left right space backspace ctrl-c，或单个字符如 1、y。", "inputSchema": {"type": "object", "properties": {"session": session, "keys": {"type": "array", "items": {"type": "string"}}, "done_seconds": done, "mac": mac}, "required": ["session", "keys"]}},
        {"name": "remote_wait", "description": "等一个或多个远端会话停下再返回它们的屏幕；已停着的立即返回。刚发完消息请改用 remote_tell 的 done_seconds。默认任一个停下就返回，all=true 时等全部。", "inputSchema": {"type": "object", "properties": {"sessions": {"type": "array", "items": {"type": "string"}}, "all": {"type": "boolean"}, "timeout_seconds": {"type": "number"}, "mac": mac}, "required": ["sessions"]}},
        {"name": "remote_new", "description": "在远端已登记的项目里新开一个 Claude 或 Codex 会话，返回会话名。", "inputSchema": {"type": "object", "properties": {"project": {"type": "string"}, "kind": {"type": "string", "enum": ["claude", "codex"]}, "mac": mac}, "required": ["project"]}},
        {"name": "remote_close", "description": "让远端会话里的 Agent 退出。", "inputSchema": {"type": "object", "properties": {"session": session, "mac": mac}, "required": ["session"]}}
    ])
}

fn call(name: &str, args: &Value) -> Result<String> {
    let s = |k: &str| args[k].as_str().map(str::to_string);
    let list = |k: &str| args[k].as_array().map(|a| a.iter().filter_map(|v| v.as_str().map(str::to_string)).collect::<Vec<_>>()).unwrap_or_default();
    let seconds = |k: &str| args[k].as_f64().map(|v| v.clamp(1.0, DONE_LIMIT));
    let mut o = Options { mac: s("mac"), wait: 2.0, ..Default::default() };
    let need = |k: &str| s(k).ok_or(format!("缺少参数 {k}"));
    let command = match name {
        "remote_agents" => { o.all = args["include_exited"].as_bool().unwrap_or(false); "agents" }
        "remote_read" => { o.rest = vec![need("session")?]; "read" }
        "remote_tell" => { o.rest = vec![need("session")?, need("text")?]; o.done = seconds("done_seconds"); o.wait = 0.0; "tell" }
        "remote_keys" => { o.rest = std::iter::once(need("session")?).chain(list("keys")).collect(); o.done = seconds("done_seconds"); o.wait = 1.0; "keys" }
        "remote_wait" => { o.rest = list("sessions"); o.all = args["all"].as_bool().unwrap_or(false); o.done = seconds("timeout_seconds"); "wait" }
        "remote_new" => { o.rest = vec![need("project")?, s("kind").unwrap_or_else(|| "claude".into())]; "new" }
        "remote_close" => { o.rest = vec![need("session")?]; "close" }
        other => return Err(format!("没有工具 {other}")),
    };
    execute(command, &o)
}

fn mcp() -> Result<()> {
    use std::io::{BufRead, Write};
    let stdin = std::io::stdin();
    let mut stdout = std::io::stdout();
    for line in stdin.lock().lines() {
        let line = line.map_err(|e| e.to_string())?;
        let Ok(request) = serde_json::from_str::<Value>(&line) else { continue };
        let Some(id) = request.get("id").cloned() else { continue }; // notifications need no reply
        let params = &request["params"];
        let reply = match request["method"].as_str().unwrap_or("") {
            "initialize" => json!({"protocolVersion": params["protocolVersion"].as_str().unwrap_or("2025-06-18"), "capabilities": {"tools": {}}, "serverInfo": {"name": "shifu-remote", "version": env!("CARGO_PKG_VERSION")}}),
            "tools/list" => json!({"tools": tools()}),
            "tools/call" => match call(params["name"].as_str().unwrap_or(""), &params["arguments"]) {
                Ok(text) => json!({"content": [{"type": "text", "text": text}]}),
                Err(e) => json!({"content": [{"type": "text", "text": e}], "isError": true}),
            },
            "ping" => json!({}),
            other => {
                writeln!(stdout, "{}", json!({"jsonrpc": "2.0", "id": id, "error": {"code": -32601, "message": format!("不支持 {other}")}})).map_err(|e| e.to_string())?;
                stdout.flush().map_err(|e| e.to_string())?;
                continue;
            }
        };
        writeln!(stdout, "{}", json!({"jsonrpc": "2.0", "id": id, "result": reply})).map_err(|e| e.to_string())?;
        stdout.flush().map_err(|e| e.to_string())?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn options_keep_text_and_flags_apart() {
        let o = options(&["claude-1".into(), "--mac".into(), "美国 Mac".into(), "你好".into(), "--".into(), "--json".into()]).unwrap();
        assert_eq!(o.mac.as_deref(), Some("美国 Mac"));
        assert!(!o.json);
        assert_eq!(o.rest, ["claude-1", "你好", "--json"]);
        assert!(options(&["--wait".into(), "99".into()]).is_err());
    }

    #[test]
    fn done_flag_has_a_default_and_keys_map_to_terminal_bytes() {
        assert_eq!(options(&["s".into(), "--done".into()]).unwrap().done, Some(600.0));
        let o = options(&["s".into(), "--done".into(), "90".into(), "继续".into()]).unwrap();
        assert_eq!((o.done, o.rest.clone()), (Some(90.0), vec!["s".to_string(), "继续".to_string()]));
        assert_eq!(key_bytes("Down").unwrap(), "\x1b[B");
        assert_eq!(key_bytes("ctrl-c").unwrap(), "\x03");
        assert_eq!(key_bytes("1").unwrap(), "1");
        assert!(key_bytes("f13").is_err());
    }

    #[test]
    fn mcp_tools_match_the_commands_they_call() {
        let names: Vec<_> = tools().as_array().unwrap().iter().map(|t| t["name"].as_str().unwrap().to_string()).collect();
        assert_eq!(names, ["remote_agents", "remote_read", "remote_tell", "remote_keys", "remote_wait", "remote_new", "remote_close"]);
        assert!(call("remote_tell", &json!({"text": "hi"})).unwrap_err().contains("session"));
        assert!(call("nope", &json!({})).is_err());
    }

    #[test]
    fn screen_text_drops_trailing_blank_rows() {
        let mut parser = vt100::Parser::new(5, 20, 0);
        parser.process("第一行\r\n  second  \r\n".as_bytes());
        assert_eq!(text(&Some(parser)), "第一行\n  second");
        assert_eq!(text(&None), "");
    }

    #[test]
    fn talks_to_a_host_over_the_same_noise_binding() {
        let binding = format!("shifu-relay-v1/{}/{}", "a".repeat(32), "b".repeat(32));
        let secret = key(&"c".repeat(64));
        let mut controller = shifu_channel::Channel::new(true, &secret, &binding).unwrap();
        let mut host = shifu_channel::Channel::new(false, &secret, &binding).unwrap();
        host.read(&controller.write(&[]).unwrap()).unwrap();
        controller.read(&host.write(&[]).unwrap()).unwrap();
        let wire = controller.write(br#"{"op":"list","id":1}"#).unwrap();
        assert_eq!(host.read(&wire).unwrap(), br#"{"op":"list","id":1}"#);
    }
}
