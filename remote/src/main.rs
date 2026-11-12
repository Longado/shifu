mod server;
mod terminal;

use serde::{Deserialize, Serialize};
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::net::{Ipv4Addr, SocketAddrV4, TcpStream};
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

type Result<T> = std::result::Result<T, String>;
const HELP: &str = "Shifu 手机远程入口\n\nshifu remote start [--port 3842] [--web-root 目录]  后台启动\nshifu remote serve [--port 3842] [--web-root 目录]  前台运行\nshifu remote status                              查看本机入口\nshifu remote code                                在本机显示访问码\nshifu remote stop                                停止入口，保留原会话\n\n服务只监听 127.0.0.1。手机访问需 Tailscale Serve 或同等私有连接。";

#[derive(Serialize, Deserialize)]
struct Service {
    pid: u32,
    port: u16,
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
        "help" | "--help" | "-h" => println!("{HELP}"),
        "code" => println!(
            "{}",
            fs::read_to_string(home().join("access-code")).map_err(|_| "尚未启动远程入口。")?
        ),
        "status" => {
            let s = running()?;
            println!(
                "远程入口运行中：http://127.0.0.1:{}\n手机请使用已配置的 Tailscale Serve 地址访问。",
                s.port
            );
        }
        "stop" => {
            let s = running()?;
            let code = fs::read_to_string(home().join("access-code")).map_err(|e| e.to_string())?;
            request(&s, "POST", "/internal/stop", Some(&code))?;
            let end = Instant::now() + Duration::from_secs(5);
            while home().join("service.json").exists() && Instant::now() < end {
                std::thread::sleep(Duration::from_millis(50));
            }
            if home().join("service.json").exists() {
                return Err("停止尚未完成，请稍后查看 status。".into());
            }
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
    match run() {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("shifu remote: {error}");
            std::process::ExitCode::FAILURE
        }
    }
}
