use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use serde::{Deserialize, Serialize};
use std::{
    fs,
    path::PathBuf,
    time::{SystemTime, UNIX_EPOCH},
};

pub fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}
pub fn id(value: &str) -> bool {
    value.len() == 32 && value.bytes().all(|b| b.is_ascii_hexdigit())
}
pub fn key(value: &str) -> super::Result<[u8; 32]> {
    if value.len() != 64 || !value.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err("设备密钥无效".into());
    }
    let mut out = [0; 32];
    for (n, b) in out.iter_mut().enumerate() {
        *b = u8::from_str_radix(&value[n * 2..n * 2 + 2], 16).map_err(|_| "设备密钥无效")?;
    }
    Ok(out)
}
pub fn new_key() -> super::Result<String> {
    Ok(format!("{}{}", super::random_key()?, super::random_key()?))
}

#[derive(Clone, Serialize, Deserialize, PartialEq)]
pub struct Network {
    pub relay: String,
    pub host: String,
    pub token: String,
}
#[derive(Clone, Serialize, Deserialize)]
pub struct Pair {
    pub id: String,
    pub key: String,
    pub expires: u64,
}
#[derive(Clone, Serialize, Deserialize)]
pub struct Device {
    pub id: String,
    pub key: String,
    pub name: String,
    pub created: u64,
    pub revoked: bool,
}
#[derive(Clone, Serialize, Deserialize)]
pub struct Pending {
    pub id: String,
    pub pairing: String,
    pub device: Device,
    pub expires: u64,
}

pub struct Store {
    pub path: PathBuf,
}
impl Store {
    pub fn new(path: PathBuf) -> Self {
        Self { path }
    }
    fn read<T: serde::de::DeserializeOwned>(&self, path: &str) -> super::Result<T> {
        serde_json::from_slice(&fs::read(self.path.join(path)).map_err(|e| e.to_string())?)
            .map_err(|e| e.to_string())
    }
    fn write<T: Serialize>(&self, path: &str, value: &T) -> super::Result<()> {
        let dest = self.path.join(path);
        super::private_dir(dest.parent().unwrap())?;
        let tmp = dest.with_extension(format!("{}.tmp", super::random_key()?));
        super::private_write(&tmp, &serde_json::to_vec(value).map_err(|e| e.to_string())?)?;
        fs::rename(tmp, dest).map_err(|e| e.to_string())
    }
    pub fn network(&self) -> super::Result<Network> {
        self.read("network.json")
    }
    pub fn configure(&self, relay: &str, token: &str) -> super::Result<Network> {
        let uri: axum::http::Uri = relay.parse().map_err(|_| "中继地址无效")?;
        let local = matches!(uri.host(), Some("127.0.0.1" | "localhost" | "[::1]"));
        if uri.scheme_str() != Some("https") && !(local && uri.scheme_str() == Some("http")) {
            return Err("公网中继必须使用HTTPS".into());
        }
        if uri.authority().is_none()
            || uri.query().is_some()
            || !matches!(uri.path(), "" | "/")
            || token.len() < 32
            || token.bytes().any(|b| b.is_ascii_whitespace())
        {
            return Err("中继地址或接入凭证无效".into());
        }
        let cfg = Network {
            relay: relay.trim_end_matches('/').to_string(),
            host: self
                .network()
                .ok()
                .map(|n| n.host)
                .unwrap_or(super::random_key()?),
            token: token.to_string(),
        };
        self.write("network.json", &cfg)?;
        Ok(cfg)
    }
    pub fn pair(&self) -> super::Result<(Pair, String)> {
        let cfg = self.network()?;
        let pair = Pair {
            id: super::random_key()?,
            key: new_key()?,
            expires: now() + 600,
        };
        self.write("pairing.json", &pair)?;
        let fragment =
            URL_SAFE_NO_PAD.encode(serde_json::to_vec(&pair).map_err(|e| e.to_string())?);
        Ok((
            pair,
            format!("{}/h/{}#pair={fragment}", cfg.relay, cfg.host),
        ))
    }
    pub fn credential(&self, credential: &str) -> super::Result<(String, bool)> {
        if !id(credential) {
            return Err("设备标识无效".into());
        }
        if let Ok(device) = self.device(credential)
            && !device.revoked
        {
            return Ok((device.key, false));
        }
        let pair: Pair = self.read("pairing.json")?;
        if pair.id == credential
            && pair.expires > now()
            && !self.path.join(format!("used-{}", pair.id)).exists()
        {
            Ok((pair.key, true))
        } else {
            Err("配对码已过期或已使用".into())
        }
    }
    pub fn device(&self, device: &str) -> super::Result<Device> {
        if !id(device) {
            return Err("设备标识无效".into());
        }
        self.read(&format!("devices/{device}.json"))
    }
    pub fn request(&self, pairing: &str, device: Device) -> super::Result<Pending> {
        let (_, temporary) = self.credential(pairing)?;
        if !temporary
            || !id(&device.id)
            || device.name.is_empty()
            || device.name.chars().count() > 80
        {
            return Err("配对申请无效".into());
        }
        key(&device.key)?;
        if self.device(&device.id).is_ok() {
            return Err("设备标识已存在，请重新生成配对申请".into());
        }
        let pair: Pair = self.read("pairing.json")?;
        let request = Pending {
            id: super::random_key()?,
            pairing: pairing.into(),
            device: Device {
                created: now(),
                revoked: false,
                ..device
            },
            expires: pair.expires,
        };
        self.write(&format!("pending/{}.json", request.id), &request)?;
        Ok(request)
    }
    pub fn approve(&self, request: &str) -> super::Result<()> {
        if !id(request) {
            return Err("申请标识无效".into());
        }
        let pending: Pending = self.read(&format!("pending/{request}.json"))?;
        self.credential(&pending.pairing)?;
        if pending.expires <= now() {
            return Err("申请已过期".into());
        }
        use std::os::unix::fs::OpenOptionsExt;
        fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(self.path.join(format!("used-{}", pending.pairing)))
            .map_err(|_| "配对码已经使用")?;
        self.write(
            &format!("devices/{}.json", pending.device.id),
            &pending.device,
        )
    }
    pub fn revoke(&self, device: &str) -> super::Result<()> {
        let mut d = self.device(device)?;
        d.revoked = true;
        self.write(&format!("devices/{device}.json"), &d)
    }
    pub fn devices(&self) -> Vec<Device> {
        self.entries("devices")
    }
    pub fn pending(&self) -> Vec<Pending> {
        self.entries::<Pending>("pending")
            .into_iter()
            .filter(|p| p.expires > now() && self.credential(&p.pairing).is_ok())
            .collect()
    }
    fn entries<T: serde::de::DeserializeOwned>(&self, folder: &str) -> Vec<T> {
        fs::read_dir(self.path.join(folder))
            .into_iter()
            .flatten()
            .filter_map(|e| fs::read(e.ok()?.path()).ok())
            .filter_map(|b| serde_json::from_slice(&b).ok())
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn device_is_inactive_until_approved_and_can_be_revoked() {
        let path = std::env::temp_dir().join(format!(
            "shifu-pair-{}",
            super::super::random_key().unwrap()
        ));
        let store = Store::new(path.clone());
        store
            .configure("http://127.0.0.1:3843", &"a".repeat(64))
            .unwrap();
        let (pair, url) = store.pair().unwrap();
        assert!(url.contains("#pair="));
        assert!(!url.split('#').next().unwrap().contains(&pair.key));
        let d = Device {
            id: "b".repeat(32),
            key: "c".repeat(64),
            name: "test browser".into(),
            created: 0,
            revoked: false,
        };
        let pending = store.request(&pair.id, d.clone()).unwrap();
        assert!(store.device(&d.id).is_err());
        store.approve(&pending.id).unwrap();
        assert!(store.credential(&pair.id).is_err());
        assert!(store.credential(&d.id).is_ok());
        assert!(store.approve(&pending.id).is_err());
        store.revoke(&d.id).unwrap();
        assert!(store.credential(&d.id).is_err());
        fs::remove_dir_all(path).unwrap();
    }
    #[test]
    fn refuses_plaintext_public_relay() {
        let s = Store::new(std::path::Path::new("/unused").into());
        assert!(
            s.configure("http://public.example", &"x".repeat(64))
                .is_err()
        );
    }
    #[test]
    fn duplicate_device_id_does_not_replace_an_authorized_device() {
        let path = std::env::temp_dir().join(format!(
            "shifu-pair-{}",
            super::super::random_key().unwrap()
        ));
        let s = Store::new(path.clone());
        s.configure("http://127.0.0.1:3843", &"a".repeat(64))
            .unwrap();
        let (p, _) = s.pair().unwrap();
        let d = Device {
            id: "b".repeat(32),
            key: "c".repeat(64),
            name: "original".into(),
            created: 0,
            revoked: false,
        };
        let r = s.request(&p.id, d.clone()).unwrap();
        s.approve(&r.id).unwrap();
        let (p, _) = s.pair().unwrap();
        let attempt = Device {
            key: "d".repeat(64),
            ..d.clone()
        };
        assert!(s.request(&p.id, attempt).is_err());
        assert_eq!(s.device(&d.id).unwrap().key, d.key);
        fs::remove_dir_all(path).unwrap();
    }
}
