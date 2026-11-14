mod network;
mod management;
mod lifecycle;
mod pairing;
mod relay;
mod server;
mod terminal;
mod session_creation;

use serde::{Deserialize, Serialize};
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::net::{Ipv4Addr, SocketAddrV4, TcpStream};
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

type Result<T> = std::result::Result<T, String>;
const SERVICE_API: u8 = 2;
const HELP: &str = "Shifu 远程入口\n\nshifu remote start [--port 3842] [--web-root 目录]  后台启动\nshifu remote serve [--port 3842] [--web-root 目录]  前台运行\nshifu remote status                              查看本机入口\nshifu remote code                                在本机显示管理访问码\nshifu remote stop                                停止入口，保留原会话\nshifu remote link https://中继 --key-file 文件      配置内置加密连接\nshifu remote pair                                显示一次性配对地址\nshifu remote devices                             查看待批准与已配对设备\nshifu remote approve 申请ID                       批准配对\nshifu remote revoke 设备ID                        撤销设备\nshifu remote relay help                          中继服务器用法\n\n本机管理页只监听127.0.0.1；配置中继后，其他设备无需安装Tailscale。";

#[derive(Serialize, Deserialize)]
struct Service {
    pid: u32,
    port: u16,
    #[serde(default)]
    api: u8,
}

fn home() -> PathBuf {
    std::env::var_os("SHIFU_REMOTE_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            PathBuf::from(std::env::var_os("HOME").unwrap_or_default()).join(".shifu/remote")
        })
}

fn private_dir(path: &Path) -> Result<()> {
    fs::create_dir_all(path).map_err(|e| e.to_string())?;
    fs::set_permissions(path, fs::Permissions::from_mode(0o700)).map_err(|e| e.to_string())
}

fn private_write(path: &Path, content: &[u8]) -> Result<()> {
    let mut file = OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(0o600)
        .open(path)
        .map_err(|e| e.to_string())?;
    file.set_permissions(fs::Permissions::from_mode(0o600))
        .map_err(|e| e.to_string())?;
    file.write_all(content).map_err(|e| e.to_string())
}

fn random_key() -> Result<String> {
    let mut bytes = [0; 16];
    File::open("/dev/urandom")
        .and_then(|mut f| f.read_exact(&mut bytes))
        .map_err(|e| e.to_string())?;
    Ok(bytes.iter().map(|b| format!("{b:02x}")).collect())
}

fn access_code(directory: &Path) -> Result<String> {
    let path = directory.join("access-code");
    match fs::read_to_string(&path) {
        Ok(code) if code.len() == 32 && code.bytes().all(|b| b.is_ascii_hexdigit()) => Ok(code),
        Ok(_) => Err("访问码文件无效；请先停止入口，再移走该文件并重新启动。".into()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            let key = random_key()?;
            private_write(&path, key.as_bytes())?;
            Ok(key)
        }
        Err(e) => Err(e.to_string()),
    }
}

fn service() -> Result<Service> {
    let bytes =
        fs::read(home().join("service.json")).map_err(|_| "远程入口未运行。".to_string())?;
    serde_json::from_slice(&bytes).map_err(|e| format!("远程入口状态无效: {e}"))
}

fn request(service: &Service, method: &str, path: &str, code: Option<&str>) -> Result<String> {
    let address = SocketAddrV4::new(Ipv4Addr::LOCALHOST, service.port);
    let mut stream = TcpStream::connect_timeout(&address.into(), Duration::from_secs(1))
        .map_err(|_| "远程入口未运行。".to_string())?;
    stream
        .set_read_timeout(Some(Duration::from_secs(2)))
        .map_err(|e| e.to_string())?;
    let auth = code
        .map(|c| format!("Authorization: Bearer {c}\r\n"))
        .unwrap_or_default();
    write!(stream, "{method} {path} HTTP/1.1\r\nHost: 127.0.0.1:{}\r\n{auth}Content-Length: 0\r\nConnection: close\r\n\r\n", service.port).map_err(|e| e.to_string())?;
    let mut response = String::new();
    stream
        .read_to_string(&mut response)
        .map_err(|e| e.to_string())?;
    if !response.starts_with("HTTP/1.1 200") {
        return Err("远程入口没有接受此请求。".into());
    }
    Ok(response
        .split_once("\r\n\r\n")
        .map(|(_, body)| body)
        .unwrap_or("")
        .to_string())
}

fn running() -> Result<Service> {
    let saved = service()?;
    let live: Service = serde_json::from_str(&request(&saved, "GET", "/health", None)?)
        .map_err(|_| "该端口不是 Shifu 远程入口。".to_string())?;
    if saved.pid != live.pid {
        return Err("远程入口状态已过期。".into());
    }
    Ok(live)
}

fn stop_service() -> Result<()> {
    let s = running()?;
    let code = fs::read_to_string(home().join("access-code")).map_err(|e| e.to_string())?;
    request(&s, "POST", "/internal/stop", Some(&code))?;
    let end = Instant::now() + Duration::from_secs(5);
    while home().join("service.json").exists() && Instant::now() < end {
        std::thread::sleep(Duration::from_millis(50));
    }
    if home().join("service.json").exists() { return Err("停止尚未完成，请稍后查看状态".into()); }
    Ok(())
}

fn options(args: &[String]) -> Result<(u16, PathBuf)> {
    let mut port = 3842;
    let mut web = home().join("web");
    let mut iter = args.iter();
    while let Some(flag) = iter.next() {
        match flag.as_str() {
            "--port" => {
                port = iter
                    .next()
                    .ok_or("--port 缺少端口")?
                    .parse()
                    .map_err(|_| "端口应为 0–65535")?
            }
            "--web-root" => web = PathBuf::from(iter.next().ok_or("--web-root 缺少目录")?),
            _ => return Err(format!("不认识 {flag:?}")),
        }
    }
    let web = web
        .canonicalize()
        .map_err(|_| format!("网页尚未安装: {}", web.display()))?;
    if !web.join("index.html").is_file() {
        return Err("网页目录缺少 index.html。".into());
    }
    Ok((port, web))
}

fn run() -> Result<()> {
    shifu::bridge::default_to_app();
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str).unwrap_or("help") {
        "manage" => {
            let value = serde_json::from_reader(std::io::stdin()).map_err(|_| "远程管理请求无效")?;
            println!("{}", management::control(value)?);
        }
        "relay" => {
            if args
                .get(1)
                .is_some_and(|a| matches!(a.as_str(), "help" | "--help" | "-h"))
            {
                println!(
                    "shifu remote relay --port 3843 --key-file 接入凭证文件 --web-root 网页目录 [--bind 127.0.0.1]\n公网部署使用HTTPS反向代理。接入凭证不存在时自动生成，不打印到日志。"
                );
                return Ok(());
            }
            let mut rest = Vec::new();
            let mut key_file = None;
            let mut bind = "127.0.0.1".to_string();
            let mut iter = args[1..].iter();
            while let Some(flag) = iter.next() {
                match flag.as_str() {
                    "--key-file" => {
                        key_file = Some(PathBuf::from(iter.next().ok_or("缺少密钥文件")?))
                    }
                    "--bind" => bind = iter.next().ok_or("缺少监听地址")?.clone(),
                    _ => rest.push(flag.clone()),
                }
            }
            let (port, web) = options(&rest)?;
            let key_file=key_file.ok_or("用法: shifu remote relay --port 3843 --key-file 文件 [--web-root 网页目录] [--bind 监听地址]")?;
            let token = match fs::read_to_string(&key_file) {
                Ok(key) => key.trim().to_string(),
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                    let key = pairing::new_key()?;
                    if let Some(parent) = key_file.parent() {
                        fs::create_dir_all(parent).map_err(|e| e.to_string())?;
                    }
                    private_write(&key_file, key.as_bytes())?;
                    key
                }
                Err(e) => return Err(e.to_string()),
            };
            if token.len() < 32 || token.bytes().any(|b| b.is_ascii_whitespace()) {
                return Err("中继接入凭证无效".into());
            }
            tokio::runtime::Runtime::new()
                .map_err(|e| e.to_string())?
                .block_on(relay::serve(port, web, token, bind))?;
        }
        "link" => {
            let (Some(url), Some(flag), Some(file)) = (args.get(1), args.get(2), args.get(3))
            else {
                return Err(
                    "用法: shifu remote link https://中继地址 --key-file 接入凭证文件".into(),
                );
            };
            if flag != "--key-file" {
                return Err("接入凭证通过 --key-file 读取。".into());
            }
            let token = fs::read_to_string(file).map_err(|e| e.to_string())?;
            let cfg = pairing::Store::new(home()).configure(url, token.trim())?;
            println!(
                "中继已配置：{}\n本机管理入口：http://127.0.0.1:3842\n生成配对码：shifu remote pair",
                cfg.relay
            );
        }
        "pair" => {
            let (_, url) = pairing::Store::new(home()).pair()?;
            println!(
                "一次性配对地址（10分钟有效，请仅在本机显示）：\n{url}\n手机申请后，在本机网页批准或运行 shifu remote approve <申请ID>。"
            );
        }
        "devices" => {
            let store = pairing::Store::new(home());
            let devices: Vec<_> = store
                .devices()
                .into_iter()
                .map(|d| serde_json::json!({"id":d.id,"name":d.name,"revoked":d.revoked}))
                .collect();
            let pending: Vec<_> = store
                .pending()
                .into_iter()
                .map(|p| serde_json::json!({"id":p.id,"name":p.device.name,"expires":p.expires}))
                .collect();
            println!(
                "{}",
                serde_json::json!({"devices":devices,"pending":pending})
            );
        }
        "approve" => {
            pairing::Store::new(home()).approve(args.get(1).ok_or("缺少申请ID")?)?;
            println!("配对已批准。");
        }
        "revoke" => {
            pairing::Store::new(home()).revoke(args.get(1).ok_or("缺少设备ID")?)?;
            println!("设备已撤销。");
        }
        "help" | "--help" | "-h" => println!("{HELP}"),
        "code" => println!(
            "{}",
            fs::read_to_string(home().join("access-code")).map_err(|_| "尚未启动远程入口。")?
        ),
        "status" => {
            let s = running()?;
            println!("本机管理入口：http://127.0.0.1:{}", s.port);
            if let Ok(cfg) = pairing::Store::new(home()).network() {
                println!(
                    "远程入口：{}/h/{}\n连接状态在本机管理页查看。",
                    cfg.relay, cfg.host
                );
            } else {
                println!("内置中继尚未配置，请在本机管理页的“远程连接”中设置。");
            }
        }
        "stop" => {
            lifecycle::stop(true)?;
            println!("远程入口已停止，原会话继续运行。");
        }
        "start" => {
            if let Ok(s) = running() {
                println!("远程入口已在运行：http://127.0.0.1:{}", s.port);
                return Ok(());
            }
            let (port, web) = options(&args[1..])?;
            private_dir(&home())?;
            let log = OpenOptions::new()
                .append(true)
                .create(true)
                .mode(0o600)
                .open(home().join("server.log"))
                .map_err(|e| e.to_string())?;
            let mut command = Command::new(std::env::current_exe().map_err(|e| e.to_string())?);
            command
                .args(["serve", "--port", &port.to_string(), "--web-root"])
                .arg(web)
                .stdin(Stdio::null())
                .stdout(log.try_clone().map_err(|e| e.to_string())?)
                .stderr(log);
            use std::os::unix::process::CommandExt;
            let mut child = command
                .process_group(0)
                .spawn()
                .map_err(|e| e.to_string())?;
            let end = Instant::now() + Duration::from_secs(10);
            loop {
                if let Ok(s) = running() {
                    println!(
                        "远程入口已启动：http://127.0.0.1:{}\n在本机运行 shifu remote code 查看访问码。",
                        s.port
                    );
                    break;
                }
                if child.try_wait().map_err(|e| e.to_string())?.is_some() {
                    return Err(format!(
                        "启动失败，查看 {}",
                        home().join("server.log").display()
                    ));
                }
                if Instant::now() >= end {
                    return Err(format!(
                        "启动尚未完成，查看 {}",
                        home().join("server.log").display()
                    ));
                }
                std::thread::sleep(Duration::from_millis(100));
            }
        }
        "serve" => {
            if running().is_ok() {
                return Err("远程入口已运行。".into());
            }
            let (port, web) = options(&args[1..])?;
            private_dir(&home())?;
            let code = access_code(&home())?;
            tokio::runtime::Runtime::new()
                .map_err(|e| e.to_string())?
                .block_on(server::serve(port, web, code))?;
        }
        other => return Err(format!("不认识 {other:?}。shifu remote help 看用法。")),
    }
    Ok(())
}

fn main() -> std::process::ExitCode {
    // WebSocket TLS needs an explicit provider, even without other TLS clients.
    let _ = rustls::crypto::ring::default_provider().install_default();
    match run() {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("shifu remote: {error}");
            std::process::ExitCode::FAILURE
        }
    }
}
