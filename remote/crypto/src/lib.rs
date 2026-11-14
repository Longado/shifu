#[cfg(target_arch = "wasm32")]
use wasm_bindgen::prelude::*;

#[cfg_attr(target_arch = "wasm32", wasm_bindgen)]
pub struct Channel {
    handshake: Option<snow::HandshakeState>,
    transport: Option<snow::TransportState>,
}

#[cfg_attr(target_arch = "wasm32", wasm_bindgen)]
impl Channel {
    #[cfg_attr(target_arch = "wasm32", wasm_bindgen(constructor))]
    pub fn new(initiator: bool, key: &[u8], binding: &str) -> Result<Self, String> {
        let key: &[u8; 32] = key.try_into().map_err(|_| "设备密钥长度无效")?;
        let builder = snow::Builder::new(
            "Noise_NNpsk0_25519_ChaChaPoly_SHA256"
                .parse()
                .map_err(|e| format!("{e}"))?,
        )
        .prologue(binding.as_bytes())
        .map_err(|e| e.to_string())?
        .psk(0, key)
        .map_err(|e| e.to_string())?;
        let handshake = if initiator {
            builder.build_initiator()
        } else {
            builder.build_responder()
        }
        .map_err(|e| e.to_string())?;
        Ok(Self {
            handshake: Some(handshake),
            transport: None,
        })
    }
    pub fn ready(&self) -> bool {
        self.transport.is_some()
    }
    pub fn write(&mut self, bytes: &[u8]) -> Result<Vec<u8>, String> {
        if bytes.len() > 65519 {
            return Err("消息过大".into());
        }
        let mut out = vec![0; 65535];
        let n = match &mut self.transport {
            Some(state) => state.write_message(bytes, &mut out),
            None => self
                .handshake
                .as_mut()
                .ok_or("通道未初始化")?
                .write_message(bytes, &mut out),
        }
        .map_err(|e| e.to_string())?;
        out.truncate(n);
        self.finish()?;
        Ok(out)
    }
    pub fn read(&mut self, bytes: &[u8]) -> Result<Vec<u8>, String> {
        if bytes.len() > 65535 {
            return Err("消息过大".into());
        }
        let mut out = vec![0; 65535];
        let n = match &mut self.transport {
            Some(state) => state.read_message(bytes, &mut out),
            None => self
                .handshake
                .as_mut()
                .ok_or("通道未初始化")?
                .read_message(bytes, &mut out),
        }
        .map_err(|e| e.to_string())?;
        out.truncate(n);
        self.finish()?;
        Ok(out)
    }
}

impl Channel {
    fn finish(&mut self) -> Result<(), String> {
        if self
            .handshake
            .as_ref()
            .is_some_and(|h| h.is_handshake_finished())
        {
            self.transport = Some(
                self.handshake
                    .take()
                    .unwrap()
                    .into_transport_mode()
                    .map_err(|e| e.to_string())?,
            );
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn paired() -> (Channel, Channel) {
        let mut a = Channel::new(true, &[7; 32], "test-host/client").unwrap();
        let mut b = Channel::new(false, &[7; 32], "test-host/client").unwrap();
        assert!(b.read(&a.write(&[]).unwrap()).unwrap().is_empty());
        assert!(a.read(&b.write(&[]).unwrap()).unwrap().is_empty());
        assert!(a.ready() && b.ready());
        (a, b)
    }
    #[test]
    fn peers_exchange_binary_and_chinese() {
        let (mut a, mut b) = paired();
        let text = "跨国 Mac 中文回复".as_bytes();
        let encrypted = a.write(text).unwrap();
        assert!(!encrypted.windows(text.len()).any(|w| w == text));
        assert_eq!(b.read(&encrypted).unwrap(), text);
        assert_eq!(
            a.read(&b.write(&[0, 255, 3]).unwrap()).unwrap(),
            [0, 255, 3]
        );
    }
    #[test]
    fn wrong_key_cannot_handshake() {
        let mut a = Channel::new(true, &[1; 32], "host/pair").unwrap();
        let mut b = Channel::new(false, &[2; 32], "host/pair").unwrap();
        assert!(b.read(&a.write(&[]).unwrap()).is_err());
    }
    #[test]
    fn different_device_binding_cannot_handshake() {
        let mut a = Channel::new(true, &[1; 32], "host/client-a").unwrap();
        let mut b = Channel::new(false, &[1; 32], "host/client-b").unwrap();
        assert!(b.read(&a.write(&[]).unwrap()).is_err());
    }
    #[test]
    fn modified_or_replayed_messages_are_rejected() {
        let (mut a, mut b) = paired();
        let encrypted = a.write(b"one reply").unwrap();
        let mut modified = encrypted.clone();
        modified[0] ^= 1;
        assert!(b.read(&modified).is_err());
        assert_eq!(b.read(&encrypted).unwrap(), b"one reply");
        assert!(b.read(&encrypted).is_err());
    }
}
