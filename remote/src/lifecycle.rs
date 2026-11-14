//! One foreground gateway belongs to one user launchd job. Login startup is optional.
use std::path::PathBuf;

fn label() -> String {
    std::env::var("SHIFU_REMOTE_LAUNCH_LABEL").unwrap_or_else(|_| {
        default_label(
            &super::home(),
            std::path::Path::new(&std::env::var_os("HOME").unwrap_or_default()),
        )
    })
}
fn default_label(directory: &std::path::Path, user_home: &std::path::Path) -> String {
    if directory == user_home.join(".shifu/remote") {
        return "dev.longado.shifu.remote".into();
    }
    // A test/custom data directory must never unload the normal user's job.
    let suffix = directory
        .as_os_str()
        .as_encoded_bytes()
        .iter()
        .fold(0xcbf29ce484222325_u64, |hash, byte| {
            (hash ^ u64::from(*byte)).wrapping_mul(0x100000001b3)
        });
    format!("dev.longado.shifu.remote.{suffix:016x}")
}
fn login_file() -> PathBuf {
    let directory = std::env::var_os("SHIFU_REMOTE_LAUNCH_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            PathBuf::from(std::env::var_os("HOME").unwrap_or_default()).join("Library/LaunchAgents")
        });
    directory.join(format!("{}.plist", label()))
}
pub(crate) fn autostart() -> bool {
    login_file().is_file()
}

#[cfg(target_os = "macos")]
fn domain() -> super::Result<String> {
    let out = std::process::Command::new("/usr/bin/id")
        .arg("-u")
        .output()
        .map_err(|e| e.to_string())?;
    if !out.status.success() {
        return Err("无法读取当前系统用户".into());
    }
    Ok(format!(
        "gui/{}",
        String::from_utf8_lossy(&out.stdout).trim()
    ))
}

pub(crate) fn loaded() -> super::Result<bool> {
    #[cfg(target_os = "macos")]
    {
        let out = std::process::Command::new("/bin/launchctl")
            .args(["print", &format!("{}/{}", domain()?, label())])
            .output()
            .map_err(|e| e.to_string())?;
        if out.status.success() {
            return Ok(true);
        }
        let error = String::from_utf8_lossy(&out.stderr);
        if error.contains("Could not find service") {
            return Ok(false);
        }
        Err(format!("无法读取远程后台任务：{}", error.trim()))
    }
    #[cfg(not(target_os = "macos"))]
    {
        Ok(false)
    }
}

#[cfg(target_os = "macos")]
fn write_job(path: &std::path::Path) -> super::Result<()> {
    use serde_json::json;
    use std::{
        collections::BTreeMap,
        io::Write,
        process::{Command, Stdio},
    };
    let mut environment: BTreeMap<String, String> = [
        "HOME",
        "PATH",
        "LANG",
        "SHIFU_HOME",
        "SHIFU_SOCKET",
        "SHIFU_APP_SHIFU",
        "SHIFU_TMUX_BIN",
    ]
    .into_iter()
    .filter_map(|k| std::env::var(k).ok().map(|v| (k.to_string(), v)))
    .collect();
    environment.insert(
        "SHIFU_REMOTE_HOME".into(),
        super::home().to_string_lossy().into_owned(),
    );
    let (port, web) = managed_options()?;
    let job = json!({"Label":label(),"ProgramArguments":[std::env::current_exe().map_err(|e| e.to_string())?,
        "serve","--port",port.to_string(),"--web-root",web],
        "EnvironmentVariables":environment,"RunAtLoad":true,"KeepAlive":{"SuccessfulExit":false},
        "ThrottleInterval":5,"StandardOutPath":super::home().join("server.log"),
        "StandardErrorPath":super::home().join("server.log")});
    let mut child = Command::new("/usr/bin/plutil")
        .args(["-convert", "xml1", "-o", "-", "-"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| e.to_string())?;
    child
        .stdin
        .take()
        .unwrap()
        .write_all(&serde_json::to_vec(&job).map_err(|e| e.to_string())?)
        .map_err(|e| e.to_string())?;
    let out = child.wait_with_output().map_err(|e| e.to_string())?;
    if !out.status.success() {
        return Err("无法生成远程后台任务".into());
    }
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    super::private_write(path, &out.stdout)
}

#[cfg(target_os = "macos")]
fn managed_options() -> super::Result<(u16, PathBuf)> {
    let port = std::env::var("SHIFU_REMOTE_PORT").unwrap_or_else(|_| "3842".into());
    let web = std::env::var_os("SHIFU_REMOTE_WEB")
        .map(PathBuf::from)
        .unwrap_or_else(|| super::home().join("web"));
    super::options(&[
        "--port".into(),
        port,
        "--web-root".into(),
        web.to_string_lossy().into_owned(),
    ])
}

pub(crate) fn start() -> super::Result<()> {
    #[cfg(target_os = "macos")]
    {
        use std::{
            fs::OpenOptions,
            os::unix::fs::OpenOptionsExt,
            process::Command,
            time::{Duration, Instant},
        };
        // Validate the release before stopping an older gateway.
        managed_options()?;
        stop(false)?;
        super::private_dir(&super::home())?;
        OpenOptions::new()
            .create(true)
            .append(true)
            .mode(0o600)
            .open(super::home().join("server.log"))
            .map_err(|e| e.to_string())?;
        let path = if autostart() {
            login_file()
        } else {
            super::home().join("launch.plist")
        };
        write_job(&path)?;
        let out = Command::new("/bin/launchctl")
            .args(["bootstrap", &domain()?])
            .arg(path)
            .output()
            .map_err(|e| e.to_string())?;
        if !out.status.success() {
            return Err(format!(
                "无法启动远程后台任务：{}",
                String::from_utf8_lossy(&out.stderr).trim()
            ));
        }
        let end = Instant::now() + Duration::from_secs(10);
        while Instant::now() < end {
            if super::running().is_ok() {
                return Ok(());
            }
            std::thread::sleep(Duration::from_millis(100));
        }
        Err("远程服务尚未启动，请查看远程运行日志后重试".into())
    }
    #[cfg(not(target_os = "macos"))]
    {
        Err("桌面远程运行端目前只支持macOS".into())
    }
}

pub(crate) fn set_autostart(enabled: bool) -> super::Result<()> {
    if enabled {
        if super::running().is_err() || !loaded()? {
            return Err("请先在Shifu中启用远程服务".into());
        }
        #[cfg(target_os = "macos")]
        write_job(&login_file())?;
    } else {
        remove_login()?;
    }
    Ok(())
}
fn remove_login() -> super::Result<()> {
    match std::fs::remove_file(login_file()) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(format!("无法取消远程登录启动：{e}")),
    }
}

pub(crate) fn stop(remove_autostart: bool) -> super::Result<()> {
    if remove_autostart {
        remove_login()?;
    }
    #[cfg(target_os = "macos")]
    if loaded()? {
        let out = std::process::Command::new("/bin/launchctl")
            .args(["bootout", &format!("{}/{}", domain()?, label())])
            .output()
            .map_err(|e| e.to_string())?;
        if !out.status.success() {
            return Err(format!(
                "无法停用远程后台任务：{}",
                String::from_utf8_lossy(&out.stderr).trim()
            ));
        }
    }
    if super::running().is_ok() {
        super::stop_service()?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn custom_remote_home_cannot_target_the_normal_login_job() {
        let user_home = std::path::Path::new("/Users/test");
        let normal = default_label(&user_home.join(".shifu/remote"), user_home);
        assert_eq!(normal, "dev.longado.shifu.remote");
        assert_ne!(
            default_label(std::path::Path::new("/tmp/remote-fixture"), user_home),
            normal
        );
        assert_ne!(
            default_label(std::path::Path::new("/tmp/remote-a"), user_home),
            default_label(std::path::Path::new("/tmp/remote-b"), user_home)
        );
    }
}
