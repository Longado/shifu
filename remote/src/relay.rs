use axum::{
    Router,
    extract::{
        Path, State, WebSocketUpgrade,
        ws::{Message, WebSocket},
    },
    http::{HeaderMap, StatusCode, header},
    middleware,
    response::{IntoResponse, Response},
    routing::get,
};
use serde::Deserialize;
use std::{
    collections::HashMap,
    path::PathBuf,
    sync::{
        Arc, Mutex,
        atomic::{AtomicU64, Ordering},
    },
    time::Duration,
};
use tokio::sync::mpsc;

type Sender = mpsc::Sender<Message>;
struct Relay {
    key: String,
    web: PathBuf,
    hosts: Mutex<HashMap<String, Sender>>,
    views: Mutex<HashMap<(String, u64), Sender>>,
    next: AtomicU64,
}
async fn index() -> Response {
    axum::response::Html("<!doctype html><meta charset=utf-8><meta name=viewport content='width=device-width'><title>Shifu</title><body style='font:16px -apple-system,BlinkMacSystemFont,sans-serif;padding:40px'><h1 style='font-size:24px'>Shifu</h1><p>请在远端 Mac 生成配对二维码，用配对地址连接。</p>").into_response()
}
async fn hosted(State(r): State<Arc<Relay>>, Path(host): Path<String>) -> Response {
    if !super::pairing::id(&host) {
        return StatusCode::NOT_FOUND.into_response();
    }
    super::server::asset(&r.web, "index.html".into()).await
}
async fn asset(State(r): State<Arc<Relay>>, Path(path): Path<String>) -> Response {
    super::server::asset(&r.web, path).await
}
async fn host(
    State(r): State<Arc<Relay>>,
    Path(id): Path<String>,
    headers: HeaderMap,
    upgrade: WebSocketUpgrade,
) -> Response {
    let token = headers
        .get(header::AUTHORIZATION)
        .and_then(|s| s.to_str().ok())
        .and_then(|s| s.strip_prefix("Bearer "));
    if !token.is_some_and(|t| super::server::equal_key(t, &r.key)) {
        return StatusCode::UNAUTHORIZED.into_response();
    }
    if !super::pairing::id(&id) {
        return StatusCode::BAD_REQUEST.into_response();
    }
    upgrade
        .max_message_size(65543)
        .on_upgrade(move |socket| host_stream(socket, r, id))
}
async fn viewer(
    State(r): State<Arc<Relay>>,
    Path(id): Path<String>,
    headers: HeaderMap,
    upgrade: WebSocketUpgrade,
) -> Response {
    if !viewer_origin(&headers) {
        return StatusCode::FORBIDDEN.into_response();
    }
    if !super::pairing::id(&id) {
        return StatusCode::BAD_REQUEST.into_response();
    }
    upgrade
        .max_message_size(65535)
        .on_upgrade(move |socket| view_stream(socket, r, id))
}

fn viewer_origin(headers: &HeaderMap) -> bool {
    // Desktop bundles use Tauri's local origin. This only permits the encrypted
    // viewer handshake; the remote Mac still requires its pairing credential.
    super::server::same_origin(headers)
        || headers.get(header::ORIGIN).and_then(|v| v.to_str().ok()).is_some_and(|origin| {
            matches!(origin, "tauri://localhost" | "http://tauri.localhost" | "https://tauri.localhost")
        })
}
#[derive(Deserialize)]
struct Intro {
    credential: String,
}

#[cfg(test)]
mod origin_tests {
    use super::*;

    #[test]
    fn accepts_local_desktop_origins_without_relaxing_the_local_management_origin() {
        for origin in ["tauri://localhost", "http://tauri.localhost", "https://tauri.localhost"] {
            let mut headers = HeaderMap::new();
            headers.insert(header::HOST, "relay.example".parse().unwrap());
            headers.insert(header::ORIGIN, origin.parse().unwrap());
            assert!(viewer_origin(&headers));
            assert!(!super::super::server::same_origin(&headers));
        }
        for origin in ["https://evil.example", "tauri://localhost.evil.example", "null"] {
            let mut headers = HeaderMap::new();
            headers.insert(header::HOST, "relay.example".parse().unwrap());
            headers.insert(header::ORIGIN, origin.parse().unwrap());
            assert!(!viewer_origin(&headers));
        }
    }
}

async fn view_stream(mut socket: WebSocket, r: Arc<Relay>, host: String) {
    let intro = tokio::time::timeout(Duration::from_secs(10), socket.recv()).await;
    let Ok(Some(Ok(Message::Text(text)))) = intro else {
        return;
    };
    let Ok(intro) = serde_json::from_str::<Intro>(&text) else {
        return;
    };
    if !super::pairing::id(&intro.credential) {
        return;
    }
    let Some(to_host) = r.hosts.lock().unwrap().get(&host).cloned() else {
        let _ = socket
            .send(Message::Text("{\"type\":\"unavailable\"}".into()))
            .await;
        return;
    };
    let id = r.next.fetch_add(1, Ordering::Relaxed);
    let (tx, mut rx) = mpsc::channel(32);
    {
        let mut views = r.views.lock().unwrap();
        if views.keys().filter(|(h, _)| h == &host).count() >= 32 {
            return;
        }
        views.insert((host.clone(), id), tx);
    }
    if to_host
        .send(Message::Text(
            serde_json::json!({"op":"join","id":id,"credential":intro.credential})
                .to_string()
                .into(),
        ))
        .await
        .is_ok()
    {
        loop {
            tokio::select! {
                message=socket.recv()=>match message {
                    Some(Ok(Message::Binary(bytes)))=>{let mut packet=id.to_be_bytes().to_vec();packet.extend_from_slice(&bytes);if to_host.send(Message::Binary(packet.into())).await.is_err(){break}},
                    Some(Ok(Message::Ping(bytes)))=>{if socket.send(Message::Pong(bytes)).await.is_err(){break}},
                    Some(Ok(Message::Pong(_)))=>{},_=>break,
                },
                message=rx.recv()=>{let Some(message)=message else{break};let closed=matches!(message,Message::Close(_));if socket.send(message).await.is_err()||closed{break}},
            }
        }
    }
    r.views.lock().unwrap().remove(&(host, id));
    let _ = to_host
        .send(Message::Text(
            serde_json::json!({"op":"close","id":id}).to_string().into(),
        ))
        .await;
}

async fn host_stream(mut socket: WebSocket, r: Arc<Relay>, id: String) {
    let (tx, mut rx) = mpsc::channel(64);
    {
        let mut hosts = r.hosts.lock().unwrap();
        if hosts.contains_key(&id) {
            return;
        }
        hosts.insert(id.clone(), tx);
    }
    let mut ping = tokio::time::interval(Duration::from_secs(20));
    loop {
        tokio::select! {
            _=ping.tick()=>{if socket.send(Message::Ping(Vec::new().into())).await.is_err(){break}},
            message=rx.recv()=>{let Some(message)=message else{break};if socket.send(message).await.is_err(){break}},
            message=socket.recv()=>match message{
                Some(Ok(Message::Binary(packet))) if packet.len()>8=>{
                    let connection=u64::from_be_bytes(packet[..8].try_into().unwrap());
                    let to_view=r.views.lock().unwrap().get(&(id.clone(),connection)).cloned();
                    if let Some(to_view)=to_view {let _=to_view.send(Message::Binary(packet.slice(8..))).await;}
                },
                Some(Ok(Message::Text(text)))=>{
                    if let Ok(value)=serde_json::from_str::<serde_json::Value>(&text)
                        && value["op"]=="close"
                        && let Some(connection)=value["id"].as_u64() {
                        let to_view=r.views.lock().unwrap().remove(&(id.clone(),connection));
                        if let Some(to_view)=to_view {let _=to_view.send(Message::Close(None)).await;}
                    }
                },
                Some(Ok(Message::Ping(bytes)))=>{if socket.send(Message::Pong(bytes)).await.is_err(){break}},
                Some(Ok(Message::Pong(_)))=>{},_=>break,
            }
        }
    }
    r.hosts.lock().unwrap().remove(&id);
    let affected: Vec<_> = {
        let mut views = r.views.lock().unwrap();
        let keys: Vec<_> = views.keys().filter(|(h, _)| h == &id).cloned().collect();
        keys.into_iter()
            .filter_map(|key| views.remove(&key))
            .collect()
    };
    for view in affected {
        let _ = view.send(Message::Close(None)).await;
    }
}

pub async fn serve(port: u16, web: PathBuf, key: String, bind: String) -> super::Result<()> {
    let listener = tokio::net::TcpListener::bind((bind.as_str(), port))
        .await
        .map_err(|e| e.to_string())?;
    let port = listener.local_addr().map_err(|e| e.to_string())?.port();
    let state = Arc::new(Relay {
        key,
        web,
        hosts: Mutex::default(),
        views: Mutex::default(),
        next: AtomicU64::new(1),
    });
    let app = Router::new()
        .route("/", get(index))
        .route("/h/{host}", get(hosted))
        .route("/relay/host/{host}", get(host))
        .route("/relay/view/{host}", get(viewer))
        .route("/{*path}", get(asset))
        .layer(middleware::from_fn(super::server::secure_headers))
        .with_state(state);
    super::private_dir(&super::home())?;
    super::private_write(
        &super::home().join("relay-service.json"),
        &serde_json::to_vec(&super::Service {
            api: super::SERVICE_API,
            pid: std::process::id(),
            port,
        })
        .map_err(|e| e.to_string())?,
    )?;
    axum::serve(listener, app)
        .with_graceful_shutdown(async {
            let _ = tokio::signal::ctrl_c().await;
        })
        .await
        .map_err(|e| e.to_string())
}
