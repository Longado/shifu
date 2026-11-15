use super::Request;
use serde_json::{Value, json};
use std::fs::{self, OpenOptions};
use std::io::Read;
use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt, PermissionsExt};
use std::os::unix::process::CommandExt;
use std::path::{Component, Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

const CLI: &str = "node_modules/@playwright/cli/playwright-cli.js";
const MAX_OUTPUT: u64 = 16 * 1024 * 1024;

fn home() -> Result<PathBuf, String> {
    let path = if let Some(path) = std::env::var_os("SHIFU_BROWSER_HOME") {
        PathBuf::from(path)
    } else {
        PathBuf::from(std::env::var_os("HOME").ok_or("找不到用户目录")?).join(".shifu/browser")
    };
    if !path.is_absolute() {
        return Err("SHIFU_BROWSER_HOME 必须是绝对路径".into());
    }
    Ok(path)
}

fn runtime(home: &Path) -> PathBuf {
    std::env::var_os("SHIFU_BROWSER_RUNTIME")
        .map(PathBuf::from)
        .unwrap_or_else(|| home.join("runtime"))
}

fn node(runtime: &Path) -> PathBuf {
    std::env::var_os("SHIFU_BROWSER_NODE")
        .map(PathBuf::from)
        .unwrap_or_else(|| runtime.join("bin/node"))
}

fn private_dir(path: &Path) -> Result<(), String> {
    fs::DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(path)
        .map_err(|e| e.to_string())?;
    fs::set_permissions(path, fs::Permissions::from_mode(0o700)).map_err(|e| e.to_string())
}

pub(super) fn doctor() -> Result<Value, String> {
    let home = home()?;
    let runtime = runtime(&home);
    let node = node(&runtime);
    if !runtime.is_absolute() || !node.is_absolute() {
        return Err("浏览器运行时和 Node 必须是绝对路径".into());
    }
    let cli = runtime.join(CLI);
    let version = if node.is_file() {
        Command::new(&node)
            .arg("--version")
            .output()
            .ok()
            .filter(|out| out.status.success())
            .map(|out| String::from_utf8_lossy(&out.stdout).trim().to_string())
    } else {
        None
    };
    let compatible = version
        .as_deref()
        .and_then(|s| {
            s.trim_start_matches('v')
                .split('.')
                .next()?
                .parse::<u32>()
                .ok()
        })
        .is_some_and(|v| v >= 20);
    let ready = compatible && cli.is_file();
    Ok(
        json!({"ok":ready,"result":{"runtime":runtime,"node":node,"nodeVersion":version,"installed":node.is_file() && cli.is_file(),
        "install":"在 Shifu 仓库运行 bash scripts/install-browser.sh",
        "browser":"默认使用本机 Chrome；本检查不启动浏览器、不读取登录信息"}}),
    )
}

pub(super) fn call(request: &Request) -> Result<Value, String> {
    let home = home()?;
    let runtime = runtime(&home);
    let node = node(&runtime);
    let cli = runtime.join(CLI);
    if !node.is_file() || !cli.is_file() {
        return Ok(
            json!({"ok":false,"error":{"code":"runtime_missing","message":"尚未安装浏览器运行时；在 Shifu 仓库运行 bash scripts/install-browser.sh"}}),
        );
    }
    if !runtime.is_absolute() || !node.is_absolute() {
        return Err("浏览器运行时和 Node 必须是绝对路径".into());
    }
    private_dir(&home)?;
    let workspace = home.join("sessions").join(&request.session);
    private_dir(&workspace)?;
    private_dir(&workspace.join(".playwright"))?;
    private_dir(&workspace.join(".playwright-cli"))?;
    let lock = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .mode(0o600)
        .open(workspace.join("command.lock"))
        .map_err(|e| e.to_string())?;
    match lock.try_lock() {
        Ok(()) => {}
        Err(std::fs::TryLockError::WouldBlock) => {
            return Ok(
                json!({"ok":false,"error":{"code":"session_busy","message":"该浏览器会话正在执行另一个命令"},"result":{"recovery":{"automaticRetry":false}}}),
            );
        }
        Err(e) => return Err(e.to_string()),
    }
    let caller = std::env::current_dir().map_err(|e| e.to_string())?;
    let args = prepare_args(&request.args, &caller)?;
    let mut child = Command::new(&node)
        .arg(&cli)
        .arg(format!("-s={}", request.session))
        .arg("--json")
        .args(&args)
        .current_dir(&workspace)
        .env("NO_UPDATE_NOTIFIER", "1")
        .env_remove("PLAYWRIGHT_CLI_SESSION")
        .env_remove("PLAYWRIGHT_CLI_SESSION_NAME")
        .env_remove("PLAYWRIGHT_CLI_SESSION_DIR")
        // Inherit Node on PATH for any provider-owned child process; no shell is used.
        .env(
            "PATH",
            format!(
                "{}:{}",
                node.parent().unwrap_or(Path::new("/")).display(),
                std::env::var("PATH").unwrap_or_default()
            ),
        )
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .process_group(0)
        .spawn()
        .map_err(|e| format!("无法启动浏览器运行时：{e}"))?;
    let stdout = child.stdout.take().unwrap();
    let stderr = child.stderr.take().unwrap();
    let out = std::thread::spawn(move || capture(stdout));
    let err = std::thread::spawn(move || capture(stderr));
    let deadline = Instant::now() + Duration::from_millis(request.timeout_ms);
    let status = loop {
        match child.try_wait().map_err(|e| e.to_string())? {
            Some(status) => break Some(status),
            None if Instant::now() < deadline => std::thread::sleep(Duration::from_millis(25)),
            None => {
                let _ = child.kill();
                let _ = child.wait();
                break None;
            }
        }
    };
    let stdout = out.join().map_err(|_| "读取浏览器结果失败")??;
    let stderr = err.join().map_err(|_| "读取浏览器错误失败")??;
    let Some(status) = status else {
        return Ok(
            json!({"ok":false,"session":request.session,"error":{"code":"result_unknown","message":"等待超时；页面动作可能已经发生，请先 snapshot/tab-list 观察，不能盲目重试"},"result":{"recovery":{"automaticRetry":false}}}),
        );
    };
    let mut raw: Value = match serde_json::from_slice::<Value>(&stdout) {
        Ok(value) if value.is_object() => value,
        _ => {
            return Ok(
                json!({"ok":false,"error":{"code":"invalid_response","message":"浏览器返回的结果无法解析；动作结果未知，请先重新观察"},"result":{"stderr":String::from_utf8_lossy(&stderr),"recovery":{"automaticRetry":false}}}),
            );
        }
    };
    let ok = status.success() && raw["isError"] != true && raw["result"]["isError"] != true;
    normalize_files(&mut raw, &workspace);
    if request.args[0] == "screenshot"
        && let Some(path) = screenshot_path(&raw, &workspace)
    {
        raw["screenshot"] = json!({"path":path});
    }
    let mut response =
        json!({"ok":ok,"session":request.session,"backend":"playwright-cli","result":raw});
    if !ok {
        response["error"] = json!({"code":"browser_error","message":raw["error"].as_str().unwrap_or("浏览器命令未完成；查看 result，先重新观察")});
        response["recovery"] = json!({"automaticRetry":false,"nextStep":"observe"});
    }
    Ok(response)
}

fn capture(reader: impl Read) -> Result<Vec<u8>, String> {
    let mut bytes = Vec::new();
    reader
        .take(MAX_OUTPUT + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| e.to_string())?;
    if bytes.len() as u64 > MAX_OUTPUT {
        return Err("浏览器结果超过 16 MiB；不要重试动作，请缩小 snapshot 范围后重新观察".into());
    }
    Ok(bytes)
}

fn prepare_args(args: &[String], caller: &Path) -> Result<Vec<String>, String> {
    let mut args = args.to_vec();
    if let Some(index) = args.iter().position(|arg| arg == "--text-stdin") {
        if !matches!(args[0].as_str(), "fill" | "type" | "eval" | "run-code")
            || index != args.len() - 1
        {
            return Err(
                "--text-stdin 用于 fill <引用> / type / eval / run-code，且放在最后".into(),
            );
        }
        let expected = if args[0] == "fill" { 3 } else { 2 };
        if args.len() != expected {
            return Err("--text-stdin 不能与已有文字同时使用".into());
        }
        let mut text = String::new();
        std::io::stdin()
            .take(1024 * 1024 + 1)
            .read_to_string(&mut text)
            .map_err(|e| e.to_string())?;
        if text.len() > 1024 * 1024 {
            return Err("标准输入文字超过 1 MiB，未执行任何操作".into());
        }
        args[index] = text;
    }
    // The provider runs in a private workspace; caller-supplied paths still refer to the caller's directory.
    let command = args[0].clone();
    // Positional text is data, even when it starts with a flag such as --session=other.
    // Protect it before resolving any provider option paths.
    if matches!(command.as_str(), "fill" | "type" | "eval" | "run-code") {
        let literal = if command == "fill" { 2 } else { 1 };
        if args.len() == literal + 1 {
            args.insert(literal, "--".into());
        }
    }
    let mut path_value = false;
    for arg in args.iter_mut().skip(1) {
        if arg == "--" {
            break;
        }
        if path_value {
            *arg = absolute(caller, arg).to_string_lossy().into_owned();
            path_value = false;
        } else if matches!(arg.as_str(), "--filename" | "--config" | "--profile") {
            path_value = true;
        } else if ["--filename=", "--config=", "--profile="]
            .iter()
            .any(|flag| arg.starts_with(flag))
        {
            let (flag, value) = arg.split_once('=').unwrap();
            *arg = format!("{flag}={}", absolute(caller, value).display());
        } else if matches!(command.as_str(), "upload" | "state-load" | "state-save")
            && !arg.starts_with('-')
        {
            *arg = absolute(caller, arg).to_string_lossy().into_owned();
        }
    }
    if command == "open"
        && !args
            .iter()
            .any(|s| s == "--browser" || s.starts_with("--browser="))
    {
        args.push("--browser=chrome".into());
    }
    Ok(args)
}

fn absolute(base: &Path, value: &str) -> PathBuf {
    let path = Path::new(value);
    if path.is_absolute() {
        path.into()
    } else {
        base.join(path)
    }
}

fn normalize_files(value: &mut Value, base: &Path) {
    match value {
        Value::Object(map) => {
            if let Some(file) = map.get("file").and_then(Value::as_str)
                && file.starts_with(".playwright-cli/")
                && !Path::new(file)
                    .components()
                    .any(|c| c == Component::ParentDir)
            {
                map.insert("path".into(), json!(base.join(file)));
            }
            for child in map.values_mut() {
                normalize_files(child, base);
            }
        }
        Value::Array(list) => {
            for child in list {
                normalize_files(child, base);
            }
        }
        _ => {}
    }
}

fn screenshot_path(raw: &Value, base: &Path) -> Option<PathBuf> {
    let result = raw["result"].as_str()?;
    let (_, link) = result.rsplit_once("](")?;
    let (file, _) = link.rsplit_once(')')?;
    let path = absolute(base, file);
    path.is_file().then_some(path)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn artifact_paths_are_absolute_without_interpreting_page_text_as_files() {
        let mut value =
            json!({"snapshot":{"file":".playwright-cli/page.yml"},"page":{"file":"/etc/passwd"}});
        normalize_files(&mut value, Path::new("/private/session"));
        assert_eq!(
            value["snapshot"]["path"],
            "/private/session/.playwright-cli/page.yml"
        );
        assert!(value["page"]["path"].is_null());
        let mut traversal = json!({"file":".playwright-cli/../../private"});
        normalize_files(&mut traversal, Path::new("/private/session"));
        assert!(traversal["path"].is_null());
    }

    #[test]
    fn private_workspace_does_not_change_upload_or_output_path_meaning() {
        let args = ["upload", "my=report.pdf", "image.png"].map(String::from);
        assert_eq!(
            prepare_args(&args, Path::new("/work")).unwrap(),
            ["upload", "/work/my=report.pdf", "/work/image.png"]
        );
        let args = ["screenshot", "--filename=shot.png"].map(String::from);
        assert_eq!(
            prepare_args(&args, Path::new("/work")).unwrap()[1],
            "--filename=/work/shot.png"
        );
        let args = ["fill", "e1", "--config=literal-text"].map(String::from);
        assert_eq!(
            prepare_args(&args, Path::new("/work")).unwrap(),
            ["fill", "e1", "--", "--config=literal-text"]
        );
    }
}
