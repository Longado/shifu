use super::pairing::{Device, Network, Store};
use axum::http::{StatusCode, header};
use base64::{Engine, engine::general_purpose::STANDARD};
use futures_util::{SinkExt, StreamExt};
use serde::Deserialize;
use serde_json::{Value, json};
use std::{collections::HashMap, sync::Arc, time::Duration};
use tokio::sync::{mpsc, watch};
use tokio_tungstenite::{
    connect_async,
    tungstenite::{Error, Message, client::IntoClientRequest},
};

enum RelayFailure {
    CredentialsRejected,
    Unavailable,
}

fn store() -> Store {
    Store::new(super::home())
}
fn status(state: &str, error: Option<&str>) {
    let value = json!({"state":state,"error":error});
    if let Err(e) = super::private_write(
        &super::home().join("network-status.json"),
        value.to_string().as_bytes(),
    ) {
        eprintln!("远程连接状态无法保存: {e}");
    }
}

pub async fn run(g: Arc<super::server::Gateway>, mut shutdown: watch::Receiver<bool>) {
    loop {
        if *shutdown.borrow() {
            return;
        }
        if let Ok(cfg) = store().network() {
            status("connecting", None);
            let connection_shutdown = shutdown.clone();
            let attempt = tokio::select! {_=shutdown.changed()=>return,result=connected(g.clone(),cfg,connection_shutdown)=>result};
            if let Err(error) = attempt {
                let message = match error {
                    RelayFailure::CredentialsRejected => "接入凭证被中继拒绝，请检查后重新保存。",
                    RelayFailure::Unavailable => "无法连接中继，正在重试。",
                };
                status("offline", Some(message));
            }
        }
        tokio::select! {_=shutdown.changed()=>return,_=tokio::time::sleep(Duration::from_secs(2))=>{}}
    }
}

async fn connected(
    g: Arc<super::server::Gateway>,
    cfg: Network,
    mut shutdown: watch::Receiver<bool>,
) -> Result<(), RelayFailure> {
    let url = format!(
        "{}/relay/host/{}",
        cfg.relay
            .replace("https://", "wss://")
            .replace("http://", "ws://"),
        cfg.host
    );
    let mut request = url
        .into_client_request()
        .map_err(|_| RelayFailure::Unavailable)?;
    request.headers_mut().insert(
        header::AUTHORIZATION,
        format!("Bearer {}", cfg.token)
            .parse()
            .map_err(|_| RelayFailure::Unavailable)?,
    );
    let (mut socket, _) = tokio::time::timeout(Duration::from_secs(15), connect_async(request))
        .await
        .map_err(|_| RelayFailure::Unavailable)?
        .map_err(|error| match error {
            Error::Http(response) if response.status() == StatusCode::UNAUTHORIZED => {
                RelayFailure::CredentialsRejected
            }
            _ => RelayFailure::Unavailable,
        })?;
    status("online", None);
    let (tx, mut rx) = mpsc::channel::<Message>(64);
    let mut clients = HashMap::<u64, mpsc::Sender<Vec<u8>>>::new();
    let mut jobs = tokio::task::JoinSet::new();
    let mut heartbeat = tokio::time::interval(Duration::from_secs(2));
    loop {
        tokio::select! {
            _=shutdown.changed()=>break,
            _=heartbeat.tick()=>{if store().network().ok().as_ref()!=Some(&cfg){break}},
            Some(_)=jobs.join_next(),if !jobs.is_empty()=>{},
            message=rx.recv()=>{if let Some(message)=message{socket.send(message).await.map_err(|_|RelayFailure::Unavailable)?;}},
            incoming=socket.next()=>match incoming {
                Some(Ok(Message::Binary(packet))) if packet.len()>8=>{
                    let id=u64::from_be_bytes(packet[..8].try_into().unwrap());
                    if let Some(client)=clients.get(&id)
                        && client.send(packet[8..].to_vec()).await.is_err() {
                        clients.remove(&id);
                    }
                },
                Some(Ok(Message::Text(text)))=>{
                    let value:Value=serde_json::from_str(&text).map_err(|_|RelayFailure::Unavailable)?;
                    if let Some(id)=value["id"].as_u64(){
                        if value["op"]=="close" {clients.remove(&id);}
                        else if value["op"]=="join"&&clients.len()<32 {
                            let credential=value["credential"].as_str().unwrap_or("").to_string();
                            let (to_client,input)=mpsc::channel(32);clients.insert(id,to_client);
                            let output=tx.clone();let gateway=g.clone();let network=cfg.clone();
                            jobs.spawn(async move{
                                let _=client(gateway,&network,id,&credential,input,output.clone()).await;
                                let _=output.send(Message::Text(json!({"op":"close","id":id}).to_string().into())).await;
                            });
                        }
                    }
                },
                Some(Ok(Message::Ping(bytes)))=>{socket.send(Message::Pong(bytes)).await.map_err(|_|RelayFailure::Unavailable)?;},
                Some(Ok(Message::Pong(_)))=>{},_=>break,
            }
        }
    }
    clients.clear();
    jobs.shutdown().await;
    let _ = socket.close(None).await;
    Ok(())
}

async fn receive(input: &mut mpsc::Receiver<Vec<u8>>) -> super::Result<Vec<u8>> {
    tokio::time::timeout(Duration::from_secs(20), input.recv())
        .await
        .map_err(|_| "连接验证超时")?
        .ok_or_else(|| "连接已关闭".into())
}
async fn packet(output: &mpsc::Sender<Message>, id: u64, bytes: Vec<u8>) -> super::Result<()> {
    let mut data = id.to_be_bytes().to_vec();
    data.extend(bytes);
    output
        .send(Message::Binary(data.into()))
        .await
        .map_err(|_| "中继已断开".into())
}
async fn encrypted(
    output: &mpsc::Sender<Message>,
    id: u64,
    cipher: &mut shifu_channel::Channel,
    value: Value,
) -> super::Result<()> {
    packet(
        output,
        id,
        cipher.write(&serde_json::to_vec(&value).map_err(|e| e.to_string())?)?,
    )
    .await
}

#[derive(Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
enum Operation {
    List {
        id: u32,
    },
    Catalog {
        id: u32,
    },
    Create {
        id: u32,
        project: String,
        kind: String,
    },
    Open {
        name: String,
    },
    Input {
        data: String,
    },
    Close,
    Pair {
        device: String,
        key: String,
        name: String,
    },
}

async fn client(
    g: Arc<super::server::Gateway>,
    cfg: &Network,
    id: u64,
    credential: &str,
    mut input: mpsc::Receiver<Vec<u8>>,
    output: mpsc::Sender<Message>,
) -> super::Result<()> {
    let (key, temporary) = store().credential(credential)?;
    let mut cipher = shifu_channel::Channel::new(
        false,
        &super::pairing::key(&key)?,
        &format!("shifu-relay-v1/{}/{}", cfg.host, credential),
    )?;
    if !cipher.read(&receive(&mut input).await?)?.is_empty() {
        return Err("握手负载无效".into());
    }
    packet(&output, id, cipher.write(&[])?).await?;
    if temporary {
        let value = cipher.read(&receive(&mut input).await?)?;
        let Operation::Pair { device, key, name } =
            serde_json::from_slice(&value).map_err(|e| e.to_string())?
        else {
            return Err("需要申请配对".into());
        };
        let request = store().request(
            credential,
            Device {
                id: device,
                key,
                name,
                created: 0,
                revoked: false,
            },
        )?;
        encrypted(&output, id, &mut cipher, json!({"type":"pair_pending"})).await?;
        loop {
            if let Ok(d) = store().device(&request.device.id)
                && !d.revoked
                && d.key == request.device.key
            {
                encrypted(&output, id, &mut cipher, json!({"type":"paired"})).await?;
                return Ok(());
            }
            if request.expires <= super::pairing::now() {
                return Err("配对申请已过期".into());
            }
            tokio::select! {message=input.recv()=>{if message.is_none(){return Ok(())}},_=tokio::time::sleep(Duration::from_millis(300))=>{}}
        }
    }
    encrypted(&output, id, &mut cipher, json!({"type":"authenticated","capabilities":["session_create"]})).await?;
    let mut local: Option<
        tokio_tungstenite::WebSocketStream<
            tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>,
        >,
    > = None;
    let mut cookie: Option<String> = None;
    let mut check = tokio::time::interval(Duration::from_secs(2));
    let result:super::Result<()>=async {loop {tokio::select!{
        _=check.tick()=>{if store().credential(credential).is_err(){encrypted(&output,id,&mut cipher,json!({"type":"revoked"})).await?;return Ok(())}},
        wire=input.recv()=>{
            let Some(wire)=wire else{return Ok(())};
            // Active requests can arrive before the periodic check; notify before closing.
            if store().credential(credential).is_err(){encrypted(&output,id,&mut cipher,json!({"type":"revoked"})).await?;return Ok(())}
            let clear=cipher.read(&wire)?;
            let command:Operation=serde_json::from_slice(&clear).map_err(|e|e.to_string())?;
            match command {
                Operation::List{id:request}=>{
                    let rows=tokio::task::spawn_blocking(||shifu::bridge::rows().into_iter().map(|mut row|{row.last_message=None;row}).collect::<Vec<_>>()).await.map_err(|e|e.to_string())?;
                    encrypted(&output,id,&mut cipher,json!({"type":"agents","id":request,"agents":rows})).await?;
                },
                Operation::Catalog{id:request}=>{
                    let result=tokio::task::spawn_blocking(super::session_creation::catalog).await.map_err(|e|e.to_string())?;
                    let reply=match result {
                        Ok(catalog)=>json!({"type":"catalog","id":request,"catalog":catalog}),
                        Err(message)=>json!({"type":"request_error","id":request,"message":message}),
                    };
                    encrypted(&output,id,&mut cipher,reply).await?;
                },
                Operation::Create{id:request,project,kind}=>{
                    let result=tokio::task::spawn_blocking(move||super::session_creation::create(&project,&kind)).await.map_err(|e|e.to_string())?;
                    let reply=match result {
                        Ok(name)=>json!({"type":"created","id":request,"name":name}),
                        Err(message)=>json!({"type":"request_error","id":request,"message":message}),
                    };
                    encrypted(&output,id,&mut cipher,reply).await?;
                },
                Operation::Open{name}=>{
                    if let Some(mut old)=local.take(){let _=old.close(None).await;}
                    if let Some(old)=cookie.take(){g.revoke_session(&old);}
                    if !shifu::bridge::rows().iter().any(|r|r.name==name){return Err("会话不存在".into())}
                    let token=g.grant()?;
                    let encoded:String=name.bytes().map(|b|if b.is_ascii_alphanumeric()||b==b'-'||b==b'_' {(b as char).to_string()}else{format!("%{b:02X}")}).collect();
                    let mut request=format!("ws://127.0.0.1:{}/api/terminal/{encoded}",g.port).into_client_request().map_err(|e|e.to_string())?;
                    request.headers_mut().insert(header::ORIGIN,format!("http://127.0.0.1:{}",g.port).parse().unwrap());
                    request.headers_mut().insert(header::COOKIE,format!("shifu_remote={token}").parse().unwrap());cookie=Some(token);
                    local=Some(connect_async(request).await.map_err(|e|e.to_string())?.0);
                },
                Operation::Input{data} if data.len()<=128*1024=>{if let Some(local)=&mut local{local.send(Message::Text(data.into())).await.map_err(|e|e.to_string())?;}},
                Operation::Close=>{
                    if let Some(mut old)=local.take(){let _=old.close(None).await;}
                    if let Some(old)=cookie.take(){g.revoke_session(&old);}
                },
                _=>return Err("请求不支持".into()),
            }
        },
        event=async{if let Some(local)=&mut local{local.next().await}else{std::future::pending().await}}=>{
            match event {
                Some(Ok(Message::Binary(bytes)))=>encrypted(&output,id,&mut cipher,json!({"type":"terminal","binary":STANDARD.encode(bytes)})).await?,
                Some(Ok(Message::Text(text)))=>encrypted(&output,id,&mut cipher,json!({"type":"terminal","text":text.as_str()})).await?,
                Some(Ok(Message::Ping(bytes)))=>{if let Some(local)=&mut local{local.send(Message::Pong(bytes)).await.map_err(|e|e.to_string())?;}},
                Some(Ok(Message::Pong(_)))=>{},
                _=>{local=None;encrypted(&output,id,&mut cipher,json!({"type":"terminal_close"})).await?;},
            }
        }
    }}}.await;
    if let Some(mut local) = local {
        let _ = local.close(None).await;
    }
    if let Some(token) = cookie {
        g.revoke_session(&token);
    }
    result
}
