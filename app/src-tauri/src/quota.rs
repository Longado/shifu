//! Read-only quota requests through the installed clients. No prompts, conversations, or credential-file access.
use crate::limits::{VendorLimits, Window, utc_seconds};
use serde_json::{Value, json};
use std::io::{Read, Write};
use std::os::fd::AsRawFd;
use std::os::unix::process::CommandExt;
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

#[derive(Clone, Copy)]
pub enum Client { Codex, Claude }

impl Client {
    fn name(self) -> &'static str { match self { Self::Codex => "Codex", Self::Claude => "Claude" } }

    fn initialize(self) -> Value {
        match self {
            Self::Codex => json!({"id":"shifu-init","method":"initialize","params":{
                "clientInfo":{"name":"shifu_usage","version":"0.1.0"},
                "capabilities":{"explicitGatewayOauth":true}
            }}),
            Self::Claude => json!({"type":"control_request","request_id":"shifu-init","request":{"subtype":"initialize"}}),
        }
    }

    fn request(self) -> Value {
        match self {
            Self::Codex => json!({"id":"shifu-usage","method":"account/rateLimits/read","params":{"excludeResetCreditDetails":true}}),
            Self::Claude => json!({"type":"control_request","request_id":"shifu-usage","request":{"subtype":"get_usage","skip_behaviors":true}}),
        }
    }
}

pub fn read(client: Client, directory: &Path) -> Result<VendorLimits, String> {
    let program = match client { Client::Codex => "codex", Client::Claude => "claude" };
    probe(client, Path::new(program), directory, Duration::from_secs(40))
}

fn probe(client: Client, program: &Path, directory: &Path, timeout: Duration) -> Result<VendorLimits, String> {
    let name = client.name();
    let failure = || format!("{name} 查询失败，请在客户端检查登录与网络");
    std::fs::create_dir_all(directory).map_err(|_| format!("无法准备 {name} 用量查询"))?;
    let mut command = Command::new(program);
    match client {
        Client::Codex => { command.args(["app-server", "--stdio"]); }
        Client::Claude => {
            // safe-mode disables hooks/plugins; the empty MCP/tool set and lack of a user message
            // keep this a control request. The official client alone owns authentication.
            command.args(["--print", "--safe-mode", "--setting-sources", "local", "--no-session-persistence",
                "--strict-mcp-config", "--mcp-config", "{\"mcpServers\":{}}", "--tools", "",
                "--input-format", "stream-json", "--output-format", "stream-json", "--verbose"]);
        }
    }
    let mut child = command.current_dir(directory)
        .env("DISABLE_AUTOUPDATER", "1").env("BROWSER", "/usr/bin/false")
        .env_remove("CLAUDECODE").env_remove("SHIFU_ID")
        .stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::null())
        .process_group(0).spawn().map_err(|_| format!("无法启动 {name} 客户端查询用量"))?;
    let result = (|| {
        let mut input = child.stdin.take().ok_or_else(failure)?;
        let mut output = child.stdout.take().ok_or_else(failure)?;
        let fd = output.as_raw_fd();
        // Nonblocking reads bound the whole exchange, including a client that never prints a newline.
        unsafe {
            let flags = libc::fcntl(fd, libc::F_GETFL);
            if flags < 0 || libc::fcntl(fd, libc::F_SETFL, flags | libc::O_NONBLOCK) < 0 { return Err(failure()); }
        }
        writeln!(input, "{}", client.initialize()).map_err(|_| failure())?;
        let start = Instant::now();
        let mut pending = Vec::new();
        let mut total = 0usize;
        let mut requested = false;
        loop {
            if start.elapsed() >= timeout { return Err(format!("{name} 查询超时")); }
            let mut bytes = [0u8; 8192];
            match output.read(&mut bytes) {
                Ok(0) => return Err(failure()),
                Ok(n) => {
                    total += n;
                    if total > 1024 * 1024 { return Err(format!("{name} 用量响应过大")); }
                    pending.extend_from_slice(&bytes[..n]);
                }
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                    std::thread::sleep(Duration::from_millis(20));
                    continue;
                }
                Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
                Err(_) => return Err(failure()),
            }
            while let Some(end) = pending.iter().position(|b| *b == b'\n') {
                let line: Vec<u8> = pending.drain(..=end).collect();
                let Ok(value) = serde_json::from_slice::<Value>(&line) else { continue };
                let response = match client {
                    Client::Codex => &value,
                    Client::Claude if value["type"] == "control_response" => &value["response"],
                    Client::Claude => continue,
                };
                let id = match client { Client::Codex => &response["id"], Client::Claude => &response["request_id"] };
                if id != "shifu-init" && id != "shifu-usage" { continue; }
                // Never surface raw client output: it may include account or authentication details.
                if !response["error"].is_null() || response["subtype"] == "error" { return Err(failure()); }
                if id == "shifu-init" && !requested {
                    if matches!(client, Client::Codex) {
                        writeln!(input, "{}", json!({"method":"initialized"})).map_err(|_| failure())?;
                    }
                    writeln!(input, "{}", client.request()).map_err(|_| failure())?;
                    requested = true;
                } else if id == "shifu-usage" && requested {
                    let payload = match client { Client::Codex => &response["result"], Client::Claude => &response["response"] };
                    let now = SystemTime::now().duration_since(UNIX_EPOCH).map_err(|_| "系统时间无效")?.as_secs();
                    return parse(client, payload, now);
                }
            }
        }
    })();
    // Only this probe and its descendants belong to the dedicated process group.
    unsafe { libc::kill(-(child.id() as i32), libc::SIGKILL); }
    let _ = child.wait();
    result
}

fn parse(client: Client, value: &Value, now: u64) -> Result<VendorLimits, String> {
    let mut windows = Vec::new();
    let mut credits = None;
    match client {
        Client::Codex => {
            let limits = value.pointer("/rateLimitsByLimitId/codex").unwrap_or(&value["rateLimits"]);
            for key in ["primary", "secondary"] {
                let w = &limits[key];
                if w.is_null() { continue; }
                windows.push((|| Some(Window {
                    used_percent: w["usedPercent"].as_f64()?,
                    window_minutes: w["windowDurationMins"].as_u64().filter(|v| *v > 0)?,
                    resets_at: w["resetsAt"].as_u64()?,
                }))().ok_or("Codex 用量格式已变化")?);
            }
            if limits["credits"]["hasCredits"] == true {
                credits = limits["credits"]["balance"].as_str().map(str::to_string);
            }
        }
        Client::Claude => {
            if value["rate_limits_available"] == false { return Err("当前 Claude 登录方式不提供订阅额度".into()); }
            for (key, minutes) in [("five_hour", 300), ("seven_day", 10080)] {
                let w = &value["rate_limits"][key];
                if w.is_null() { continue; }
                windows.push((|| Some(Window {
                    used_percent: w["utilization"].as_f64()?, window_minutes: minutes,
                    resets_at: utc_seconds(w["resets_at"].as_str()?)?,
                }))().ok_or("Claude 用量格式已变化")?);
            }
        }
    }
    if windows.is_empty() { return Err(format!("{} 暂无可读取的订阅额度", client.name())); }
    Ok(VendorLimits { windows, credits, updated_at: now, error: None })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    #[test]
    fn both_protocols_use_account_usage_without_starting_a_turn() {
        let root = std::env::temp_dir().join(format!("shifu-quota-protocol-{}", std::process::id()));
        std::fs::create_dir_all(&root).unwrap();
        let fixture = root.join("client");
        std::fs::write(&fixture, r#"#!/usr/bin/env python3
import json,sys
from pathlib import Path
codex = sys.argv[1] == 'app-server'
requests=[]
for line in sys.stdin:
 v=json.loads(line);requests.append(v)
 Path('requests.json').write_text(json.dumps(requests))
 if codex:
  if v.get('method')=='initialize': r={'id':v['id'],'result':{}}
  elif v.get('method')=='account/rateLimits/read': r={'id':v['id'],'result':{'rateLimits':{'primary':{'usedPercent':23,'windowDurationMins':300,'resetsAt':2000000000}}}}
  else: continue
 else:
  p={} if v['request']['subtype']=='initialize' else {'rate_limits':{'seven_day':{'utilization':61,'resets_at':'2030-01-01T00:00:00+00:00'}}}
  r={'type':'control_response','response':{'subtype':'success','request_id':v['request_id'],'response':p}}
 print(json.dumps(r),flush=True)
"#).unwrap();
        std::fs::set_permissions(&fixture, std::fs::Permissions::from_mode(0o700)).unwrap();
        for client in [Client::Codex, Client::Claude] {
            let directory = root.join(client.name());
            let data = probe(client, &fixture, &directory, Duration::from_secs(3)).unwrap();
            assert!(!data.windows.is_empty());
            let requests: Vec<Value> = serde_json::from_str(&std::fs::read_to_string(directory.join("requests.json")).unwrap()).unwrap();
            assert_eq!(requests.first(), Some(&client.initialize()));
            assert_eq!(requests.last(), Some(&client.request()));
            assert_eq!(requests.len(), if matches!(client, Client::Codex) { 3 } else { 2 });
        }
        std::fs::write(&fixture, "#!/bin/sh\nsleep 20\n").unwrap();
        let started = Instant::now();
        assert!(probe(Client::Claude, &fixture, &root, Duration::from_millis(100)).unwrap_err().contains("超时"));
        assert!(started.elapsed() < Duration::from_secs(2));
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn parses_current_windows_and_rejects_broken_or_unavailable_readings() {
        let result = json!({"rateLimitsByLimitId":{"codex":{"primary":{"usedPercent":36,"windowDurationMins":10080,"resetsAt":2000000000},"credits":{"hasCredits":true,"balance":"123.4"}}},
            "rateLimits":{"primary":{"usedPercent":99,"windowDurationMins":300,"resetsAt":2000000000}}});
        let data = parse(Client::Codex, &result, 123).unwrap();
        assert_eq!(data.windows[0].used_percent, 36.0);
        assert_eq!(data.windows[0].window_minutes, 10080);
        assert_eq!(data.credits.as_deref(), Some("123.4"));
        assert_eq!(data.updated_at, 123);
        assert!(parse(Client::Codex, &json!({"rateLimits":{"primary":{"usedPercent":12}}}), 123).is_err());
        assert!(parse(Client::Claude, &json!({"rate_limits_available":false}), 123).unwrap_err().contains("登录方式"));
        assert!(parse(Client::Claude, &json!({"rate_limits":{}}), 123).is_err());
    }

    #[test]
    #[ignore = "read-only quota requests through the user's installed clients; never sends a prompt"]
    fn live_installed_clients() {
        let root = std::env::temp_dir().join(format!("shifu-quota-live-{}", std::process::id()));
        for client in [Client::Codex, Client::Claude] {
            let started = Instant::now();
            let data = read(client, &root.join(client.name())).unwrap();
            eprintln!("{}: {} windows, fresh={}, {} ms", client.name(), data.windows.len(), data.error.is_none(), started.elapsed().as_millis());
            assert!(data.updated_at > 0 && !data.windows.is_empty());
        }
        std::fs::remove_dir_all(root).unwrap();
    }
}
