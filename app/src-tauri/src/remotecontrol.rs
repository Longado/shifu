use crate::config::Config;
use serde::{Deserialize, Serialize};
use std::{fs, io::Write, os::unix::fs::OpenOptionsExt, path::Path, sync::Mutex};
use tauri::State;

#[derive(Default)]
pub struct RemoteMacs(Mutex<()>);

#[derive(Clone, Deserialize, Serialize)]
pub struct Credential {
    id: String,
    key: String,
}

#[derive(Clone, Deserialize, Serialize)]
pub struct RemoteMac {
    name: String,
    relay: String,
    host: String,
    credential: Credential,
}

fn hex(value: &str, length: usize) -> bool {
    value.len() == length && value.bytes().all(|b| b.is_ascii_hexdigit())
}

fn validate(mac: &RemoteMac) -> Result<(), String> {
    let url = tauri::Url::parse(&mac.relay).map_err(|_| "远端地址无效")?;
    let local = matches!(url.host_str(), Some("localhost" | "127.0.0.1" | "[::1]"));
    if !(url.scheme() == "https" || (url.scheme() == "http" && local))
        || !url.username().is_empty() || url.password().is_some()
        || url.query().is_some() || url.fragment().is_some()
        || url.path() != "/" || mac.relay != url.origin().ascii_serialization()
        || mac.name.trim().is_empty() || mac.name.chars().count() > 80
        || !hex(&mac.host, 32) || !hex(&mac.credential.id, 32) || !hex(&mac.credential.key, 64)
    {
        return Err("远端连接资料无效，请重新配对".into());
    }
    Ok(())
}

fn load(path: &Path) -> Result<Vec<RemoteMac>, String> {
    let bytes = match fs::read(path) {
        Ok(bytes) => bytes,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => return Err(format!("无法读取远端连接：{e}")),
    };
    let macs: Vec<RemoteMac> = serde_json::from_slice(&bytes).map_err(|_| "远端连接资料损坏，请保留文件并检查")?;
    for mac in &macs { validate(mac)?; }
    Ok(macs)
}

fn write(path: &Path, macs: &[RemoteMac]) -> Result<(), String> {
    use std::os::unix::fs::PermissionsExt;
    let temporary = path.with_extension("json.tmp");
    let bytes = serde_json::to_vec(macs).map_err(|e| e.to_string())?;
    let mut file = fs::OpenOptions::new().create(true).truncate(true).write(true).mode(0o600)
        .open(&temporary).map_err(|e| format!("无法保存远端连接：{e}"))?;
    file.set_permissions(fs::Permissions::from_mode(0o600)).map_err(|e| e.to_string())?;
    file.write_all(&bytes).map_err(|e| format!("无法保存远端连接：{e}"))?;
    fs::rename(temporary, path).map_err(|e| format!("无法保存远端连接：{e}"))
}

fn save(path: &Path, mac: RemoteMac) -> Result<(), String> {
    validate(&mac)?;
    let mut macs = load(path)?;
    if let Some(current) = macs.iter_mut().find(|m| m.relay == mac.relay && m.host == mac.host) { *current = mac; }
    else { macs.push(mac); }
    write(path, &macs)
}

fn forget(path: &Path, relay: &str, host: &str) -> Result<(), String> {
    let mut macs = load(path)?;
    macs.retain(|m| m.relay != relay || m.host != host);
    write(path, &macs)
}

#[tauri::command]
pub fn remote_macs_load(cfg: State<Config>, macs: State<RemoteMacs>) -> Result<Vec<RemoteMac>, String> {
    let _guard = macs.0.lock().map_err(|e| e.to_string())?;
    load(&cfg.home.join("remote-macs.json"))
}

#[tauri::command]
pub fn remote_macs_save(cfg: State<Config>, macs: State<RemoteMacs>, mac: RemoteMac) -> Result<(), String> {
    let _guard = macs.0.lock().map_err(|e| e.to_string())?;
    save(&cfg.home.join("remote-macs.json"), mac)
}

#[tauri::command]
pub fn remote_macs_forget(cfg: State<Config>, macs: State<RemoteMacs>, relay: String, host: String) -> Result<(), String> {
    let _guard = macs.0.lock().map_err(|e| e.to_string())?;
    forget(&cfg.home.join("remote-macs.json"), &relay, &host)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    fn mac() -> RemoteMac {
        RemoteMac { name: "加州 Mac".into(), relay: "https://relay.example".into(), host: "1".repeat(32), credential: Credential { id: "2".repeat(32), key: "3".repeat(64) } }
    }

    #[test]
    fn saves_private_credentials_and_forgets_only_the_selected_host() {
        let folder = std::env::temp_dir().join(format!("shifu-control-store-{}", std::process::id()));
        fs::create_dir_all(&folder).unwrap();
        let path = folder.join("remote-macs.json");
        assert!(load(&path).unwrap().is_empty());
        save(&path, mac()).unwrap();
        let mut second = mac(); second.host = "4".repeat(32); save(&path, second).unwrap();
        let mut renamed = mac(); renamed.name = "美国 Mac".into(); save(&path, renamed).unwrap();
        assert_eq!(load(&path).unwrap().len(), 2);
        assert_eq!(load(&path).unwrap()[0].name, "美国 Mac");
        assert_eq!(fs::metadata(&path).unwrap().permissions().mode() & 0o777, 0o600);
        forget(&path, "https://relay.example", &"1".repeat(32)).unwrap();
        assert_eq!(load(&path).unwrap()[0].host, "4".repeat(32));
        fs::remove_dir_all(folder).unwrap();
    }

    #[test]
    fn rejects_invalid_connections_and_does_not_overwrite_corrupt_storage() {
        let mut bad = mac(); bad.relay = "http://public.example".into(); assert!(validate(&bad).is_err());
        bad = mac(); bad.credential.key = "bad-key".into(); assert!(validate(&bad).is_err());
        bad = mac(); bad.relay = "https://relay.example/path#secret".into(); assert!(validate(&bad).is_err());
        let path = std::env::temp_dir().join(format!("shifu-control-corrupt-{}.json", std::process::id()));
        fs::write(&path, b"broken").unwrap(); assert!(save(&path, mac()).is_err());
        assert_eq!(fs::read(&path).unwrap(), b"broken"); fs::remove_file(path).unwrap();
    }
}
