use serde::Deserialize;
use serde_json::{Value, json};

#[derive(Deserialize)]
#[serde(tag = "action", rename_all = "snake_case", deny_unknown_fields)]
pub(crate) enum NetworkAction {
    Configure { relay: String, token: String },
    Pair,
    Approve { id: String },
    Revoke { id: String },
}

pub(crate) fn network_status() -> super::Result<Value> {
    let store = super::pairing::Store::new(super::home());
    let cfg = if store.path.join("network.json").exists() {
        Some(store.network()?)
    } else {
        None
    };
    let devices: Vec<_> = store
        .devices()
        .into_iter()
        .map(|d| json!({"id":d.id,"name":d.name,"revoked":d.revoked}))
        .collect();
    let pending: Vec<_> = store
        .pending()
        .into_iter()
        .map(|p| json!({"id":p.id,"name":p.device.name}))
        .collect();
    let status = std::fs::read(super::home().join("network-status.json"))
        .ok()
        .and_then(|b| serde_json::from_slice::<Value>(&b).ok());
    Ok(
        json!({"relay":cfg.as_ref().map(|c|&c.relay),"host":cfg.as_ref().map(|c|&c.host),
        "status":status,"devices":devices,"pending":pending}),
    )
}

pub(crate) fn apply(action: NetworkAction) -> super::Result<Value> {
    let store = super::pairing::Store::new(super::home());
    match action {
        NetworkAction::Configure { relay, token } => {
            let token = if token.is_empty() {
                store.network().map_err(|_| "首次设置需要接入凭证")?.token
            } else {
                token
            };
            store.configure(&relay, &token)?;
            Ok(json!({"ok":true}))
        }
        NetworkAction::Pair => {
            let (pair, url) = store.pair()?;
            let svg = qrcode::QrCode::new(url.as_bytes())
                .map_err(|e| e.to_string())?
                .render::<qrcode::render::svg::Color>()
                .min_dimensions(240, 240)
                .build();
            Ok(json!({"url":url,"svg":svg,"expires":pair.expires}))
        }
        NetworkAction::Approve { id } => {
            store.approve(&id)?;
            Ok(json!({"ok":true}))
        }
        NetworkAction::Revoke { id } => {
            store.revoke(&id)?;
            Ok(json!({"ok":true}))
        }
    }
}

pub(crate) fn snapshot() -> super::Result<Value> {
    let mut value = network_status()?;
    let live = super::running().ok();
    value["available"] = json!(true);
    value["running"] = json!(live.is_some());
    value["compatible"] = json!(live.as_ref().is_none_or(|s| s.api == super::SERVICE_API));
    value["port"] = json!(live.as_ref().map(|s| s.port));
    value["autostart"] = json!(super::lifecycle::autostart());
    value["managed"] = json!(super::lifecycle::loaded()?);
    if live.is_none() {
        value["status"] = Value::Null;
    }
    Ok(value)
}

pub(crate) fn control(value: Value) -> super::Result<Value> {
    match value.get("action").and_then(Value::as_str) {
        Some("status") => snapshot(),
        Some("start") => {
            super::lifecycle::start()?;
            snapshot()
        }
        Some("stop") => {
            super::lifecycle::stop(true)?;
            snapshot()
        }
        Some("autostart") => {
            let enabled = value
                .get("enabled")
                .and_then(Value::as_bool)
                .ok_or("需要指定登录启动状态")?;
            super::lifecycle::set_autostart(enabled)?;
            snapshot()
        }
        _ => apply(serde_json::from_value(value).map_err(|_| "不支持的远程管理操作")?),
    }
}
