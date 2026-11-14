use serde_json::Value;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use tauri::Manager;

fn run(binary: &Path, web: &Path, action: &Value) -> Result<Value, String> {
    let mut child = Command::new(binary)
        .arg("manage")
        .env("SHIFU_REMOTE_WEB", web)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("无法启动远程组件：{e}"))?;
    // Credentials must never become process arguments or shell history.
    child
        .stdin
        .take()
        .unwrap()
        .write_all(&serde_json::to_vec(action).map_err(|e| e.to_string())?)
        .map_err(|e| format!("无法提交远程设置：{e}"))?;
    let output = child.wait_with_output().map_err(|e| e.to_string())?;
    if !output.status.success() {
        return Err(format!(
            "远程操作失败：{}",
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    serde_json::from_slice(&output.stdout).map_err(|_| "远程组件返回了无效状态".into())
}

fn runtime(app: &tauri::AppHandle) -> Result<(PathBuf, PathBuf), String> {
    let directory = app
        .path()
        .resource_dir()
        .map_err(|e| e.to_string())?
        .join("remote");
    let binary = std::env::var_os("SHIFU_REMOTE_BIN")
        .map(PathBuf::from)
        .unwrap_or_else(|| directory.join("shifu-remote"));
    let web = std::env::var_os("SHIFU_REMOTE_WEB")
        .map(PathBuf::from)
        .unwrap_or_else(|| directory.join("web"));
    Ok((binary, web))
}

async fn control(app: tauri::AppHandle, action: Value) -> Result<Value, String> {
    let (binary, web) = runtime(&app)?;
    if !binary.is_file() {
        return Err("本版Shifu缺少远程组件，请使用包含远程模块的安装包".into());
    }
    tauri::async_runtime::spawn_blocking(move || run(&binary, &web, &action))
        .await
        .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn remote_status(app: tauri::AppHandle) -> Result<Value, String> {
    if !runtime(&app)?.0.is_file() {
        return Ok(
            serde_json::json!({"available":false,"running":false,"managed":false,"autostart":false,
            "compatible":true,"relay":null,"host":null,"status":null,"devices":[],"pending":[],"port":null}),
        );
    }
    control(app, serde_json::json!({"action":"status"})).await
}
#[tauri::command]
pub async fn remote_start(app: tauri::AppHandle) -> Result<Value, String> {
    control(app, serde_json::json!({"action":"start"})).await
}
#[tauri::command]
pub async fn remote_stop(app: tauri::AppHandle) -> Result<Value, String> {
    control(app, serde_json::json!({"action":"stop"})).await
}
#[tauri::command]
pub async fn remote_configure(
    app: tauri::AppHandle,
    relay: String,
    token: String,
) -> Result<Value, String> {
    control(
        app,
        serde_json::json!({"action":"configure","relay":relay,"token":token}),
    )
    .await
}
#[tauri::command]
pub async fn remote_pair(app: tauri::AppHandle) -> Result<Value, String> {
    control(app, serde_json::json!({"action":"pair"})).await
}
#[tauri::command]
pub async fn remote_approve(app: tauri::AppHandle, id: String) -> Result<Value, String> {
    control(app, serde_json::json!({"action":"approve","id":id})).await
}
#[tauri::command]
pub async fn remote_revoke(app: tauri::AppHandle, id: String) -> Result<Value, String> {
    control(app, serde_json::json!({"action":"revoke","id":id})).await
}
#[tauri::command]
pub async fn remote_autostart(app: tauri::AppHandle, enabled: bool) -> Result<Value, String> {
    control(
        app,
        serde_json::json!({"action":"autostart","enabled":enabled}),
    )
    .await
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    #[test]
    fn control_sends_credentials_on_stdin_and_preserves_process_failure() {
        let directory =
            std::env::temp_dir().join(format!("shifu-remote-ipc-{}", std::process::id()));
        std::fs::create_dir_all(&directory).unwrap();
        let binary = directory.join("sidecar.py");
        let args = directory.join("arguments.json");
        let web = directory.join("网页");
        std::fs::write(&binary, format!("#!/usr/bin/python3\nimport json,sys,os,pathlib\npathlib.Path({}).write_text(json.dumps(sys.argv[1:]))\nx=json.load(sys.stdin)\nif x['action']=='fail':\n sys.stderr.write('fixture process failed');sys.exit(7)\nprint(json.dumps({{'received':x['token']=='fixture-secret','web':os.environ['SHIFU_REMOTE_WEB']}}))\n", serde_json::to_string(&args).unwrap())).unwrap();
        std::fs::set_permissions(&binary, std::fs::Permissions::from_mode(0o700)).unwrap();
        let result = run(&binary, &web, &serde_json::json!({"action":"configure","relay":"https://relay.example","token":"fixture-secret"})).unwrap();
        assert_eq!(result["received"], true);
        assert_eq!(result["web"], web.to_string_lossy().as_ref());
        let arguments: Vec<String> =
            serde_json::from_slice(&std::fs::read(&args).unwrap()).unwrap();
        assert_eq!(arguments, ["manage"]);
        assert!(
            run(&binary, &web, &serde_json::json!({"action":"fail"}))
                .unwrap_err()
                .contains("fixture process failed")
        );
        std::fs::remove_dir_all(directory).unwrap();
    }
}
