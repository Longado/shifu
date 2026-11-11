use serde_json::{Value, json};
use std::fs::{self, File, OpenOptions};
use std::io::{BufRead, BufReader, Read, Write};
use std::os::unix::fs::{DirBuilderExt, FileTypeExt, OpenOptionsExt, PermissionsExt};
use std::os::unix::net::{UnixListener, UnixStream};
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

const TIMEOUT: Duration = Duration::from_secs(45);
const MAX_RESPONSE: u64 = 64 * 1024 * 1024;
// A supported 1 MiB stdin payload may expand sixfold as JSON \uXXXX escapes.
const MAX_REQUEST: u64 = 6 * 1024 * 1024 + 64 * 1024;

fn home() -> Result<PathBuf, String> {
    if let Some(path) = std::env::var_os("SHIFU_COMPUTER_HOME") { return Ok(path.into()); }
    let user = std::env::var_os("HOME").ok_or("找不到用户目录")?;
    Ok(PathBuf::from(user).join(".shifu/computer"))
}

fn helper() -> Result<PathBuf, String> {
    let path = std::env::var_os("SHIFU_COMPUTER_HELPER").map(PathBuf::from).unwrap_or_else(|| {
        PathBuf::from(std::env::var_os("HOME").unwrap_or_default())
            .join("Applications/Shifu Computer Use.app/Contents/MacOS/shifu-computer-use-macos")
    });
    if !path.is_file() { return Err("还没安装 Shifu Computer Use 辅助程序；见项目 docs/COMPUTER-USE.md".into()); }
    Ok(path)
}

fn private_dir(path: &Path) -> Result<(), String> {
    fs::DirBuilder::new().recursive(true).mode(0o700).create(path).map_err(|e| e.to_string())?;
    fs::set_permissions(path, fs::Permissions::from_mode(0o700)).map_err(|e| e.to_string())
}

fn private_file(path: &Path) -> std::io::Result<File> {
    OpenOptions::new().write(true).create_new(true).mode(0o600).open(path)
}

pub fn permissions() -> Result<(), String> {
    let binary = helper()?;
    let app = binary.parent().and_then(Path::parent).and_then(Path::parent).ok_or("辅助程序不在 .app 中")?;
    let status = Command::new("/usr/bin/open").arg("-n").arg(app).args(["--args", "--permissions"])
        .status().map_err(|e| e.to_string())?;
    if !status.success() { return Err("打不开 Shifu Computer Use 权限窗口".into()); }
    println!("已打开 Shifu Computer Use 权限窗口；请在系统设置中亲自授权辅助功能和屏幕录制。");
    Ok(())
}

pub fn call(request: Value) -> Result<Value, String> {
    if request.to_string().len() as u64 + 1 > MAX_REQUEST {
        return Err("请求超过传输上限，未执行任何操作".into());
    }
    let home = home()?;
    private_dir(&home)?;
    let socket = home.join("bridge.sock");
    let mut stream = match UnixStream::connect(&socket) {
        Ok(s) => s,
        Err(e) if matches!(e.kind(), std::io::ErrorKind::NotFound | std::io::ErrorKind::ConnectionRefused) => {
            if request["method"] == "stop" { return Ok(json!({"ok":true,"result":{"stopped":true}})); }
            helper()?;
            let log = OpenOptions::new().create(true).append(true).mode(0o600).open(home.join("runtime.log")).map_err(|e| e.to_string())?;
            Command::new(std::env::current_exe().map_err(|e| e.to_string())?)
                .args(["computer", "_serve"]).stdin(Stdio::null())
                .stdout(log.try_clone().map_err(|e| e.to_string())?).stderr(log)
                // Agent command runners terminate the completed command's process group.
                // The broker must survive that command so the next action keeps its snapshot.
                .process_group(0)
                .spawn().map_err(|e| e.to_string())?;
            let until = Instant::now() + Duration::from_secs(12);
            loop {
                match UnixStream::connect(&socket) {
                    Ok(s) => break s,
                    Err(_) if Instant::now() < until => std::thread::sleep(Duration::from_millis(50)),
                    Err(e) => return Err(format!("辅助程序没能启动：{e}；见 {}", home.join("runtime.log").display())),
                }
            }
        }
        Err(e) => return Err(e.to_string()),
    };
    let mut response = exchange(&mut stream, &request)?;
    if let Err(e) = save_screenshot(&mut response, &home) {
        response["result"]["screenshot"] = Value::Null;
        response["result"]["screenshotStatus"] = json!({"state":"failed","reason":e});
    }
    Ok(response)
}

/// A small local broker keeps the native connection alive between CLI calls. Only this
/// parent process may connect to the signed helper; agents use a user-private socket.
pub fn serve() -> Result<(), String> {
    let home = home()?;
    private_dir(&home)?;
    let lock = OpenOptions::new().read(true).write(true).create(true).truncate(false).mode(0o600)
        .open(home.join("runtime.lock")).map_err(|e| e.to_string())?;
    match lock.try_lock() {
        Ok(()) => {},
        Err(std::fs::TryLockError::WouldBlock) => return Ok(()),
        Err(e) => return Err(e.to_string()),
    }
    let socket = home.join("bridge.sock");
    if let Ok(meta) = fs::symlink_metadata(&socket) {
        if !meta.file_type().is_socket() { return Err("bridge.sock 不是 socket，未覆盖该文件".into()); }
        fs::remove_file(&socket).map_err(|e| e.to_string())?;
    }
    let listener = UnixListener::bind(&socket).map_err(|e| e.to_string())?;
    fs::set_permissions(&socket, fs::Permissions::from_mode(0o600)).map_err(|e| e.to_string())?;
    listener.set_nonblocking(true).map_err(|e| e.to_string())?;
    let mut native = Native::start(&home)?;
    let mut last = Instant::now();
    let result = loop {
        let mut client = match listener.accept() {
            Ok((s, _)) => s,
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                if last.elapsed() > Duration::from_secs(300) { break Ok(()); }
                if let Some(status) = native.child.try_wait().map_err(|e| e.to_string())? {
                    break Err(format!("原生辅助程序已退出：{status}"));
                }
                std::thread::sleep(Duration::from_millis(50));
                continue;
            }
            Err(e) => break Err(e.to_string()),
        };
        last = Instant::now();
        // macOS inherits the listener's nonblocking flag on accepted sockets. A screenshot
        // exceeds the socket buffer, so blocking writes are needed to send the complete JSON.
        client.set_nonblocking(false).map_err(|e| e.to_string())?;
        client.set_read_timeout(Some(Duration::from_secs(3))).map_err(|e| e.to_string())?;
        client.set_write_timeout(Some(TIMEOUT)).map_err(|e| e.to_string())?;
        let mut request = match read_request(&client) {
            Ok(r) => r,
            Err(e) => {
                let _ = writeln!(client, "{}", json!({"ok":false,"error":{"code":"invalid_request","message":e}}));
                continue;
            }
        };
        if request["method"] == "stop" {
            let _ = writeln!(client, "{}", json!({"ok":true,"result":{"stopped":true}}));
            break Ok(());
        }
        request["token"] = json!(native.token);
        match exchange(&mut native.stream, &request) {
            Ok(response) => { let _ = writeln!(client, "{response}"); }
            Err(e) => {
                // A timeout may follow an action: report it and stop, never replay it.
                let _ = writeln!(client, "{}", json!({"ok":false,"error":{"code":"transport_error","message":e}}));
                break Err(e);
            }
        }
    };
    drop(native);
    let _ = fs::remove_file(&socket);
    result
}

struct Native {
    stream: UnixStream,
    child: Child,
    token: String,
    directory: PathBuf,
}

fn read_request(reader: impl Read) -> Result<Value, String> {
    let mut line = String::new();
    BufReader::new(reader).take(MAX_REQUEST + 1).read_line(&mut line).map_err(|e| e.to_string())?;
    if line.len() as u64 > MAX_REQUEST { return Err("请求超过传输上限，未执行任何操作".into()); }
    let request: Value = serde_json::from_str(&line).map_err(|e| format!("请求 JSON 无效，未执行任何操作：{e}"))?;
    if !request.is_object() || !request["method"].is_string() {
        return Err("请求缺少 method，未执行任何操作".into());
    }
    Ok(request)
}

impl Native {
    fn start(home: &Path) -> Result<Self, String> {
        let directory = home.join(format!("run-{}", std::process::id()));
        private_dir(&directory)?;
        let mut random = [0u8; 32];
        File::open("/dev/urandom").and_then(|mut f| f.read_exact(&mut random)).map_err(|e| e.to_string())?;
        let token: String = random.iter().map(|b| format!("{b:02x}")).collect();
        let token_path = directory.join("token");
        private_file(&token_path).and_then(|mut f| f.write_all(token.as_bytes())).map_err(|e| e.to_string())?;
        let socket = directory.join("provider.sock");
        let mut child = Command::new(helper()?).arg("--agent").arg(&socket).arg("--token-file").arg(&token_path)
            .stdin(Stdio::null()).stdout(Stdio::null()).spawn().map_err(|e| e.to_string())?;
        let until = Instant::now() + Duration::from_secs(10);
        let connected = loop {
            match UnixStream::connect(&socket) {
                Ok(s) => break Ok(s),
                Err(_) if Instant::now() < until && child.try_wait().map_err(|e| e.to_string())?.is_none() => {
                    std::thread::sleep(Duration::from_millis(50));
                }
                Err(e) => break Err(format!("连接原生辅助程序失败：{e}")),
            }
        };
        let _ = fs::remove_file(&token_path);
        match connected {
            Ok(stream) => Ok(Self { stream, child, token, directory }),
            Err(e) => {
                let _ = child.kill();
                let _ = child.wait();
                let _ = fs::remove_dir_all(&directory);
                Err(e)
            }
        }
    }
}

impl Drop for Native {
    fn drop(&mut self) {
        // Closing the owner's connection makes the helper exit and cancels pending input.
        let _ = self.stream.shutdown(std::net::Shutdown::Both);
        for _ in 0..10 {
            if matches!(self.child.try_wait(), Ok(Some(_))) { break; }
            std::thread::sleep(Duration::from_millis(50));
        }
        if matches!(self.child.try_wait(), Ok(None)) { let _ = self.child.kill(); }
        let _ = self.child.wait();
        let _ = fs::remove_dir_all(&self.directory);
    }
}

fn exchange(stream: &mut UnixStream, request: &Value) -> Result<Value, String> {
    stream.set_read_timeout(Some(TIMEOUT)).map_err(|e| e.to_string())?;
    stream.set_write_timeout(Some(TIMEOUT)).map_err(|e| e.to_string())?;
    writeln!(stream, "{request}").map_err(|e| e.to_string())?;
    let mut line = String::new();
    BufReader::new(stream).take(MAX_RESPONSE + 1).read_line(&mut line).map_err(|e| format!("读取操作结果失败，操作可能已发生，请先重新观察：{e}"))?;
    if line.len() as u64 > MAX_RESPONSE { return Err("操作结果超过 64 MB，请使用 --no-screenshot".into()); }
    if line.is_empty() { return Err("辅助程序断开，操作结果未知；请先重新观察".into()); }
    let response: Value = serde_json::from_str(&line).map_err(|e| format!("操作结果不完整或格式错误，请先重新观察窗口：{e}"))?;
    if response["ok"].as_bool().is_none() { return Err("辅助程序返回的结果缺少状态".into()); }
    if !response["id"].is_null() && response["id"] != request["id"] { return Err("操作结果编号不匹配".into()); }
    Ok(response)
}

fn save_screenshot(response: &mut Value, home: &Path) -> Result<(), String> {
    let Some(encoded) = response["result"]["screenshot"]["data"].as_str() else { return Ok(()); };
    let directory = home.join("screenshots");
    private_dir(&directory)?;
    let stamp = SystemTime::now().duration_since(UNIX_EPOCH).map_err(|e| e.to_string())?.as_nanos();
    let path = directory.join(format!("{stamp}.png"));
    let file = private_file(&path).map_err(|e| e.to_string())?;
    let mut decode = Command::new("/usr/bin/base64").arg("-D").stdin(Stdio::piped()).stdout(file).stderr(Stdio::piped())
        .spawn().map_err(|e| e.to_string())?;
    decode.stdin.take().unwrap().write_all(encoded.as_bytes()).map_err(|e| e.to_string())?;
    let result = decode.wait_with_output().map_err(|e| e.to_string())?;
    if !result.status.success() { return Err("截图解码失败".into()); }
    let shot = response["result"]["screenshot"].as_object_mut().unwrap();
    shot.remove("data");
    shot.insert("path".into(), json!(path));
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn accepted_stdin_survives_json_escaping() {
        let text = "\u{0001}".repeat(1024 * 1024);
        let encoded = format!("{}\n", json!({"id":1,"method":"setValue","params":{"value":text}}));
        let request = read_request(encoded.as_bytes()).unwrap();
        assert_eq!(request["params"]["value"].as_str().unwrap().len(), 1024 * 1024);
    }

    #[test]
    fn oversized_request_is_explicitly_rejected_before_dispatch() {
        let bytes = vec![b' '; MAX_REQUEST as usize + 1];
        assert_eq!(read_request(bytes.as_slice()).unwrap_err(), "请求超过传输上限，未执行任何操作");
    }

    #[test]
    fn round_trip_keeps_native_action_verification() {
        let (mut client, mut helper) = UnixStream::pair().unwrap();
        let peer = std::thread::spawn(move || {
            let mut line = String::new();
            BufReader::new(helper.try_clone().unwrap()).read_line(&mut line).unwrap();
            let request: Value = serde_json::from_str(&line).unwrap();
            assert_eq!(request["params"]["text"], "两行\n文字");
            helper.write_all(b"{\"id\":7,\"ok\":true,\"result\":{\"action\":{\"verification\":{\"state\":\"unverified\"}}}}\n").unwrap();
        });
        let result = exchange(&mut client, &json!({"id":7,"method":"typeText","params":{"text":"两行\n文字"}}));
        drop(client);
        assert!(result.is_ok(), "{result:?}");
        assert_eq!(result.unwrap()["result"]["action"]["verification"]["state"], "unverified");
        peer.join().unwrap();
    }

    #[test]
    fn closed_transport_is_an_error_not_an_empty_success() {
        let (mut client, helper) = UnixStream::pair().unwrap();
        drop(helper);
        assert!(exchange(&mut client, &json!({"id":1})).is_err());
    }
}
