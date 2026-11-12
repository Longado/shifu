use axum::{
    Json, Router,
    extract::{
        Path, Request, State, WebSocketUpgrade,
        ws::{Message, WebSocket},
    },
    http::{HeaderMap, StatusCode, Uri, header},
    middleware::{self, Next},
    response::{IntoResponse, Response},
    routing::{get, post},
};
use serde::Deserialize;
use serde_json::json;
use shifu::{bridge, tmux::Tmux};
use std::{
    collections::HashMap,
    path::PathBuf,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};
use tokio::sync::watch;

struct Gateway {
    code: String,
    web: PathBuf,
    port: u16,
    sessions: Mutex<HashMap<String, Instant>>,
    shutdown: watch::Sender<bool>,
}

impl Gateway {
    fn session(&self, headers: &HeaderMap) -> Option<String> {
        let token = headers
            .get(header::COOKIE)?
            .to_str()
            .ok()?
            .split(';')
            .find_map(|p| p.trim().strip_prefix("shifu_remote="))?;
        let mut sessions = self.sessions.lock().unwrap();
        sessions.retain(|_, expires| *expires > Instant::now());
        sessions.contains_key(token).then(|| token.to_string())
    }
}

fn same_origin(headers: &HeaderMap) -> bool {
    let Some(origin) = headers
        .get(header::ORIGIN)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.parse::<Uri>().ok())
    else {
        return false;
    };
    let host = headers.get(header::HOST).and_then(|v| v.to_str().ok());
    matches!(origin.scheme_str(), Some("http" | "https"))
        && origin.authority().is_some_and(|a| Some(a.as_str()) == host)
        && matches!(origin.path(), "" | "/")
        && origin.query().is_none()
}

fn equal_key(left: &str, right: &str) -> bool {
    left.len() == right.len()
        && left
            .bytes()
            .zip(right.bytes())
            .fold(0, |diff, (a, b)| diff | (a ^ b))
            == 0
}

async fn secure_headers(request: Request, next: Next) -> Response {
    let mut response = next.run(request).await;
    let h = response.headers_mut();
    h.insert(header::CACHE_CONTROL, "no-store".parse().unwrap());
    h.insert("x-content-type-options", "nosniff".parse().unwrap());
    h.insert("referrer-policy", "no-referrer".parse().unwrap());
    h.insert("content-security-policy", "default-src 'none'; script-src 'self'; style-src 'self' 'unsafe-inline'; connect-src 'self'; img-src 'self'; font-src 'self'; base-uri 'none'; frame-ancestors 'none'; form-action 'self'".parse().unwrap());
    response
}

async fn health(State(g): State<Arc<Gateway>>) -> Json<serde_json::Value> {
    Json(json!({"pid":std::process::id(),"port":g.port}))
}

#[derive(Deserialize)]
struct Login {
    code: String,
}

async fn login(
    State(g): State<Arc<Gateway>>,
    headers: HeaderMap,
    Json(login): Json<Login>,
) -> Response {
    if !same_origin(&headers) {
        return StatusCode::FORBIDDEN.into_response();
    }
    if !equal_key(&g.code, &login.code) {
        return (StatusCode::UNAUTHORIZED, "访问码不正确").into_response();
    }
    let token = match super::random_key() {
        Ok(token) => token,
        Err(_) => return StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    };
    g.sessions.lock().unwrap().insert(
        token.clone(),
        Instant::now() + Duration::from_secs(12 * 60 * 60),
    );
    let secure = if headers
        .get(header::ORIGIN)
        .is_some_and(|v| v.as_bytes().starts_with(b"https:"))
    {
        "; Secure"
    } else {
        ""
    };
    (
        StatusCode::NO_CONTENT,
        [(
            header::SET_COOKIE,
            format!(
                "shifu_remote={token}; HttpOnly; SameSite=Strict; Path=/; Max-Age=43200{secure}"
            ),
        )],
    )
        .into_response()
}

async fn logout(State(g): State<Arc<Gateway>>, headers: HeaderMap) -> Response {
    if !same_origin(&headers) {
        return StatusCode::FORBIDDEN.into_response();
    }
    if let Some(token) = g.session(&headers) {
        g.sessions.lock().unwrap().remove(&token);
    }
    (
        StatusCode::NO_CONTENT,
        [(
            header::SET_COOKIE,
            "shifu_remote=; HttpOnly; SameSite=Strict; Path=/; Max-Age=0",
        )],
    )
        .into_response()
}

async fn agents(State(g): State<Arc<Gateway>>, headers: HeaderMap) -> Response {
    if g.session(&headers).is_none() {
        return StatusCode::UNAUTHORIZED.into_response();
    }
    match tokio::task::spawn_blocking(bridge::rows).await {
        Ok(rows) => Json(rows).into_response(),
        Err(_) => StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    }
}

async fn stop(State(g): State<Arc<Gateway>>, headers: HeaderMap) -> StatusCode {
    let bearer = headers
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "));
    if !bearer.is_some_and(|b| equal_key(b, &g.code)) {
        return StatusCode::UNAUTHORIZED;
    }
    let _ = g.shutdown.send(true);
    StatusCode::OK
}

async fn index(State(g): State<Arc<Gateway>>) -> Response {
    asset(g, "index.html".into()).await
}
async fn static_asset(State(g): State<Arc<Gateway>>, Path(path): Path<String>) -> Response {
    asset(g, path).await
}

async fn asset(g: Arc<Gateway>, path: String) -> Response {
    if path.split('/').any(|s| s.is_empty() || s.starts_with('.')) {
        return StatusCode::NOT_FOUND.into_response();
    }
    let Ok(file) = g.web.join(path).canonicalize() else {
        return StatusCode::NOT_FOUND.into_response();
    };
    if !file.starts_with(&g.web) {
        return StatusCode::NOT_FOUND.into_response();
    }
    let mime = match file.extension().and_then(|s| s.to_str()) {
        Some("html") => "text/html; charset=utf-8",
        Some("js") => "text/javascript; charset=utf-8",
        Some("css") => "text/css; charset=utf-8",
        Some("svg") => "image/svg+xml",
        Some("webmanifest") => "application/manifest+json",
        _ => return StatusCode::NOT_FOUND.into_response(),
    };
    match tokio::fs::read(file).await {
        Ok(bytes) => ([(header::CONTENT_TYPE, mime)], bytes).into_response(),
        Err(_) => StatusCode::NOT_FOUND.into_response(),
    }
}

async fn terminal(
    State(g): State<Arc<Gateway>>,
    Path(name): Path<String>,
    headers: HeaderMap,
    upgrade: WebSocketUpgrade,
) -> Response {
    if !same_origin(&headers) {
        return StatusCode::FORBIDDEN.into_response();
    }
    let Some(token) = g.session(&headers) else {
        return StatusCode::UNAUTHORIZED.into_response();
    };
    let target = tokio::task::spawn_blocking(move || {
        let row = bridge::rows()
            .into_iter()
            .find(|r| r.name == name)
            .ok_or(StatusCode::NOT_FOUND)?;
        if row.status == shifu::status::Status::Exited {
            return Err(StatusCode::GONE);
        }
        let pane = row.pane.ok_or(StatusCode::GONE)?;
        let text = Tmux::from_env()
            .run([
                "display-message",
                "-p",
                "-t",
                &pane,
                "#{session_id} #{pane_width} #{pane_height}",
            ])
            .map_err(|_| StatusCode::GONE)?;
        let mut parts = text.split_whitespace();
        let session = parts.next().ok_or(StatusCode::GONE)?.to_string();
        let cols = parts
            .next()
            .and_then(|v| v.parse::<u16>().ok())
            .ok_or(StatusCode::GONE)?;
        let rows = parts
            .next()
            .and_then(|v| v.parse::<u16>().ok())
            .ok_or(StatusCode::GONE)?;
        Ok(crate::terminal::Target {
            session,
            cols,
            rows,
        })
    })
    .await;
    match target {
        Ok(Ok(target)) => upgrade
            .max_message_size(128 * 1024)
            .on_upgrade(move |socket| stream(socket, g, token, target))
            .into_response(),
        Ok(Err(status)) => status.into_response(),
        Err(_) => StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    }
}

#[derive(Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum ClientMessage {
    Input { data: String },
    Paste { text: String, id: u32 },
    Resize { cols: u16, rows: u16 },
}

async fn stream(
    mut socket: WebSocket,
    g: Arc<Gateway>,
    token: String,
    target: crate::terminal::Target,
) {
    let (cols, rows) = (target.cols, target.rows);
    let opened = tokio::task::spawn_blocking(move || crate::terminal::open(target)).await;
    let (mut terminal, mut output) = match opened {
        Ok(Ok(opened)) => opened,
        _ => {
            let _ = socket
                .send(Message::Text(
                    json!({"type":"error","message":"无法连接原会话，请刷新对话列表。"})
                        .to_string()
                        .into(),
                ))
                .await;
            return;
        }
    };
    if socket
        .send(Message::Text(
            json!({"type":"ready","cols":cols,"rows":rows})
                .to_string()
                .into(),
        ))
        .await
        .is_err()
    {
        return;
    }
    let mut heartbeat = tokio::time::interval(Duration::from_secs(2));
    let mut shutdown = g.shutdown.subscribe();
    loop {
        tokio::select! {
            _ = shutdown.changed() => break,
            _ = heartbeat.tick() => {
                if !g.sessions.lock().unwrap().get(&token).is_some_and(|end| *end > Instant::now()) {break}
                if socket.send(Message::Ping(Vec::new().into())).await.is_err() {break}
            }
            data = output.recv() => {
                match data {
                    Some(crate::terminal::Output::Data(bytes)) => {if socket.send(Message::Binary(bytes.into())).await.is_err() {break}}
                    Some(crate::terminal::Output::Exit) | None => {let _=socket.send(Message::Text(json!({"type":"ended"}).to_string().into())).await;break}
                }
            }
            message = socket.recv() => {
                if !g.sessions.lock().unwrap().get(&token).is_some_and(|end| *end > Instant::now()) {break}
                let Some(Ok(message)) = message else {break};
                let Message::Text(text) = message else {if matches!(message,Message::Close(_)){break}continue};
                let parsed = serde_json::from_str::<ClientMessage>(&text);
                let result = match parsed {
                    Ok(ClientMessage::Input {data}) if data.len() <= 16*1024 => terminal.write(data.as_bytes()),
                    Ok(ClientMessage::Resize {cols,rows}) if (1..=512).contains(&cols) && (1..=200).contains(&rows) => terminal.resize(cols,rows),
                    Ok(ClientMessage::Paste {text,id}) if !text.is_empty() && text.len() <= 16*1024 => {
                        let pasted = terminal.write(format!("\x1b[200~{text}\x1b[201~").as_bytes());
                        if pasted.is_err() {pasted} else {
                            tokio::time::sleep(Duration::from_millis(300)).await;
                            if *shutdown.borrow() || !g.sessions.lock().unwrap().get(&token).is_some_and(|end| *end > Instant::now()) {break}
                            match terminal.write(b"\r") {
                                Ok(()) => {if socket.send(Message::Text(json!({"type":"sent","id":id}).to_string().into())).await.is_err(){break}Ok(())}
                                error => error,
                            }
                        }
                    }
                    _ => Err("输入格式或长度不支持。".into()),
                };
                if let Err(error) = result {let _=socket.send(Message::Text(json!({"type":"error","message":error}).to_string().into())).await;break}
            }
        }
    }
    let _ = socket.send(Message::Close(None)).await;
    // Drop terminates only the tmux attach client. The original agent belongs to tmux.
}

pub async fn serve(port: u16, web: PathBuf, code: String) -> super::Result<()> {
    let listener = tokio::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, port))
        .await
        .map_err(|e| e.to_string())?;
    let port = listener.local_addr().map_err(|e| e.to_string())?.port();
    let (shutdown, mut receiver) = watch::channel(false);
    let g = Arc::new(Gateway {
        code,
        web,
        port,
        sessions: Mutex::default(),
        shutdown: shutdown.clone(),
    });
    let app = Router::new()
        .route("/", get(index))
        .route("/health", get(health))
        .route("/api/login", post(login))
        .route("/api/logout", post(logout))
        .route("/api/agents", get(agents))
        .route("/api/terminal/{name}", get(terminal))
        .route("/internal/stop", post(stop))
        .route("/{*path}", get(static_asset))
        .layer(middleware::from_fn(secure_headers))
        .with_state(g);
    let service = super::Service {
        pid: std::process::id(),
        port,
    };
    super::private_write(
        &super::home().join("service.json"),
        &serde_json::to_vec(&service).map_err(|e| e.to_string())?,
    )?;
    let result = axum::serve(listener,app).with_graceful_shutdown(async move {
        tokio::select! {_=tokio::signal::ctrl_c()=>{let _=shutdown.send(true);},_=receiver.changed()=>{}}
    }).await;
    let _ = std::fs::remove_file(super::home().join("service.json"));
    result.map_err(|e| e.to_string())
}
