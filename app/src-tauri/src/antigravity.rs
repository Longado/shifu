//! Quota from agy's read-only `/usage` report; no direct credential access.
use serde::Serialize;
use serde_json::Value;
use std::fs;
use std::io::Read;
use std::os::fd::AsRawFd;
use std::os::unix::fs::{DirBuilderExt, PermissionsExt};
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

#[derive(Clone, Debug, Default, PartialEq, Serialize)]
pub struct Usage {
    pub groups: Vec<Group>,
    pub updated_at: u64,
    pub error: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Group {
    pub name: String,
    pub buckets: Vec<Bucket>,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Bucket {
    pub name: String,
    pub window: String,
    pub used_percent: Option<f64>,
    pub reset_time: Option<String>,
}

pub fn parse_report(text: &str, now: u64) -> Result<Usage, String> {
    let invalid = || "Antigravity 返回的额度报告无效".to_string();
    let report: Value = serde_json::from_str(text).map_err(|_| invalid())?;
    if report["status"] != "SUCCESS" || report["command"]["name"] != "usage" {
        return Err("未取得额度，请在 agy 中运行 /usage 检查登录状态".into());
    }
    let mut groups = Vec::new();
    for group in report.pointer("/command/data/groups").and_then(Value::as_array).ok_or_else(invalid)? {
        let name = group["name"].as_str().filter(|s| !s.is_empty()).ok_or_else(invalid)?;
        let mut buckets = Vec::new();
        for bucket in group["buckets"].as_array().ok_or_else(invalid)? {
            if bucket["enabled"] == false { continue; }
            let remaining = match bucket.get("remaining_fraction").filter(|v| !v.is_null()) {
                None => None,
                Some(v) => Some(v.as_f64().filter(|v| (0.0..=1.0).contains(v)).ok_or_else(invalid)?),
            };
            buckets.push(Bucket {
                name: bucket["name"].as_str().ok_or_else(invalid)?.into(),
                window: bucket["window"].as_str().unwrap_or("").into(),
                used_percent: remaining.map(|v| (1.0-v)*100.0),
                reset_time: bucket["reset_time"].as_str().map(str::to_string),
            });
        }
        if !buckets.is_empty() { groups.push(Group { name:name.into(), buckets }); }
    }
    if groups.is_empty() { return Err("Antigravity 暂无可读取的额度".into()); }
    Ok(Usage { groups, updated_at:now, error:None })
}

fn supports_report(version: &str) -> bool {
    let parts: Vec<_> = version.trim().trim_start_matches('v').split('.').map(str::parse::<u32>).collect();
    matches!(parts.as_slice(), [Ok(major), Ok(minor), Ok(patch)] if (*major,*minor,*patch) >= (1,1,11))
}

// The report must neither hang the watcher nor leave its child processes running.
fn run_command(mut command: Command, timeout: Duration) -> Result<Vec<u8>, String> {
    let mut child = command.stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::null())
        .process_group(0).spawn().map_err(|e| if e.kind() == std::io::ErrorKind::NotFound {
            "未找到 agy，请先安装 Antigravity CLI".to_string()
        } else { "无法启动 Antigravity 读取".to_string() })?;
    let mut pipe = child.stdout.take().unwrap();
    let result = (|| {
        let fd = pipe.as_raw_fd();
        // SAFETY: this descriptor belongs to the live stdout pipe above.
        let flags = unsafe { libc::fcntl(fd, libc::F_GETFL) };
        if flags < 0 || unsafe { libc::fcntl(fd, libc::F_SETFL, flags | libc::O_NONBLOCK) } < 0 {
            return Err("无法读取 Antigravity 输出".into());
        }
        let start = Instant::now();
        let mut bytes = Vec::new();
        let mut status = None;
        loop {
            let mut buffer = [0u8;8192];
            loop {
                match pipe.read(&mut buffer) {
                    Ok(0) => break,
                    Ok(n) => {
                        bytes.extend_from_slice(&buffer[..n]);
                        if bytes.len() > 1024*1024 { return Err("Antigravity 输出过大".into()); }
                    }
                    Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => break,
                    Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
                    Err(_) => return Err("无法读取 Antigravity 输出".into()),
                }
            }
            if let Some(status) = status {
                return if std::process::ExitStatus::success(&status) { Ok(bytes) }
                    else { Err("读取失败，请在 agy 中检查登录与网络".into()) };
            }
            status = child.try_wait().map_err(|_| "无法读取 Antigravity 进程状态")?;
            if start.elapsed() >= timeout { return Err("Antigravity 读取超时".into()); }
            if status.is_none() { std::thread::sleep(Duration::from_millis(50)); }
        }
    })();
    // SAFETY: process_group(0) made this child the leader of a dedicated process group.
    // It contains only this quota probe and its children, not user-launched sessions.
    unsafe { libc::kill(-(child.id() as i32), libc::SIGKILL); }
    let _ = child.wait();
    result
}

struct ProbeDirectory(PathBuf);
impl Drop for ProbeDirectory {
    fn drop(&mut self) { let _ = fs::remove_dir_all(&self.0); }
}

fn probe_directory() -> Result<ProbeDirectory, String> {
    let stamp = SystemTime::now().duration_since(UNIX_EPOCH).map_err(|_| "系统时间无效")?.as_nanos();
    let directory = std::env::temp_dir().join(format!("shifu-agy-{}-{stamp}",std::process::id()));
    fs::DirBuilder::new().mode(0o700).create(&directory).map_err(|_| "无法创建额度读取临时目录")?;
    let directory = ProbeDirectory(directory);
    // agy can open OAuth in a browser when its login expires. Passive polling must
    // report that failure instead of repeatedly opening login windows.
    for name in ["open","xdg-open"] {
        let path = directory.0.join(name);
        fs::write(&path,"#!/bin/sh\nexit 1\n").and_then(|_| fs::set_permissions(path,fs::Permissions::from_mode(0o700)))
            .map_err(|_| "无法准备额度读取")?;
    }
    Ok(directory)
}

pub fn read_usage() -> Result<Usage, String> {
    let directory = probe_directory()?;
    read_with_cli(Path::new("agy"), &directory.0)
}

pub fn read_models() -> Result<String, String> {
    let directory = probe_directory()?;
    let mut command = passive_command(Path::new("agy"), &directory.0)?;
    command.arg("models");
    let bytes = run_command(command, Duration::from_secs(40))?;
    String::from_utf8(bytes).map_err(|_| "Antigravity 模型列表编码无效".into())
}

fn passive_command(cli: &Path, directory: &Path) -> Result<Command, String> {
    let mut paths = vec![directory.to_path_buf()];
    paths.extend(std::env::split_paths(&std::env::var_os("PATH").unwrap_or_default()));
    let path = std::env::join_paths(paths).map_err(|_| "无法设置 Antigravity 读取路径")?;
    let mut command = Command::new(cli);
    command.current_dir(directory).env("AGY_CLI_DISABLE_AUTO_UPDATE", "true")
        .env("BROWSER", directory.join("open")).env("PATH", path);
    Ok(command)
}

fn read_with_cli(cli: &Path, directory: &Path) -> Result<Usage, String> {
    let mut version = passive_command(cli, directory)?; version.arg("--version");
    let bytes = run_command(version,Duration::from_secs(3))?;
    if !supports_report(&String::from_utf8_lossy(&bytes)) {
        return Err("额度监测需要 agy 1.1.11 或更新版本".into());
    }
    let mut report = passive_command(cli, directory)?;
    report.args(["-p","/usage","--output-format","json","--print-timeout","30s"]);
    let bytes = run_command(report,Duration::from_secs(40))?;
    let now = SystemTime::now().duration_since(UNIX_EPOCH).map_err(|_| "系统时间无效")?.as_secs();
    parse_report(std::str::from_utf8(&bytes).map_err(|_| "额度报告编码无效")?,now)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn report() -> Value {
        json!({"status":"SUCCESS","num_turns":0,"command":{"name":"usage","data":{"groups":[
            {"name":"Gemini Models","buckets":[
                {"name":"Weekly Limit Remaining","window":"weekly","remaining_fraction":0.75,"reset_time":"2026-11-14T03:00:00Z"},
                {"name":"Five Hour Limit Remaining","window":"5h","remaining_fraction":0.5,"reset_time":"2026-11-11T10:00:00Z"}
            ]},
            {"name":"Claude and GPT models","buckets":[{"name":"Weekly Limit Remaining","window":"weekly","remaining_fraction":1}]}
        ]}}})
    }

    #[test]
    fn reports_each_quota_pool_without_using_token_counts() {
        let mut r = report(); r["usage"] = json!({"input_tokens":999999});
        let got = parse_report(&r.to_string(), 42).unwrap();
        assert_eq!(got.updated_at,42);
        assert_eq!(got.groups.len(),2);
        assert_eq!(got.groups[0].buckets[0].used_percent,Some(25.0));
        assert_eq!(got.groups[0].buckets[1].used_percent,Some(50.0));
        assert_eq!(got.groups[1].buckets[0].used_percent,Some(0.0));
        assert_eq!(got.groups[0].buckets[0].reset_time.as_deref(),Some("2026-11-14T03:00:00Z"));
    }

    #[test]
    fn unknown_quota_stays_unknown_and_disabled_buckets_are_not_allowances() {
        let mut r = report();
        r["command"]["data"]["groups"][0]["buckets"][0].as_object_mut().unwrap().remove("remaining_fraction");
        r["command"]["data"]["groups"][0]["buckets"][1]["enabled"] = json!(false);
        let got = parse_report(&r.to_string(),42).unwrap();
        assert_eq!(got.groups[0].buckets.len(),1);
        assert_eq!(got.groups[0].buckets[0].used_percent,None);
    }

    #[test]
    fn rejects_failed_or_unrelated_reports_and_invalid_fractions() {
        for (pointer,value) in [("/status",json!("ERROR")),("/command/name",json!("model")),
            ("/command/data/groups",json!([])),
            ("/command/data/groups/0/buckets/0/remaining_fraction",json!(1.1)),
            ("/command/data/groups/0/buckets/0/remaining_fraction",json!(-0.1)),
            ("/command/data/groups/0/buckets/0/remaining_fraction",json!("0.5"))] {
            let mut r=report(); *r.pointer_mut(pointer).unwrap()=value;
            assert!(parse_report(&r.to_string(),42).is_err(),"{pointer}");
        }
    }

    #[test]
    fn refuses_versions_that_might_send_usage_as_a_model_prompt() {
        assert!(!supports_report("1.1.10"));
        assert!(!supports_report("unknown"));
        assert!(supports_report("1.1.11\n"));
        assert!(supports_report("1.5.2\n"));
    }

    #[test]
    fn command_timeout_and_output_limit_are_bounded() {
        let mut slow = Command::new("/bin/sh"); slow.args(["-c","sleep 5"]);
        let start = Instant::now();
        assert_eq!(run_command(slow,Duration::from_millis(100)).unwrap_err(),"Antigravity 读取超时");
        assert!(start.elapsed() < Duration::from_secs(2));
        let mut huge = Command::new("/usr/bin/head"); huge.args(["-c","1048577","/dev/zero"]);
        assert_eq!(run_command(huge,Duration::from_secs(3)).unwrap_err(),"Antigravity 输出过大");
    }

    #[test]
    fn invokes_only_the_usage_report_and_preserves_failed_exit() {
        let stamp = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos();
        let dir = ProbeDirectory(std::env::temp_dir().join(format!("shifu-agy-test-{stamp}")));
        fs::create_dir(&dir.0).unwrap();
        let cli = dir.0.join("agy");
        let script = format!("#!/bin/sh\n[ \"$AGY_CLI_DISABLE_AUTO_UPDATE\" = true ] || exit 2\n[ \"$1\" = --version ] && {{ echo 1.5.2; exit 0; }}\n[ \"$*\" = '-p /usage --output-format json --print-timeout 30s' ] || exit 3\ncat <<'REPORT'\n{}\nREPORT\n",report());
        fs::write(&cli,script).unwrap(); fs::set_permissions(&cli,fs::Permissions::from_mode(0o700)).unwrap();
        assert_eq!(read_with_cli(&cli,&dir.0).unwrap().groups.len(),2);
        fs::write(&cli,"#!/bin/sh\necho '{\"status\":\"SUCCESS\"}'\nexit 1\n").unwrap();
        assert!(read_with_cli(&cli,&dir.0).is_err());
    }

    #[test]
    #[ignore = "reads the signed-in local agy account; run explicitly"]
    fn real_cli_usage() {
        let got = read_usage().unwrap();
        assert!(!got.groups.is_empty());
        println!("{}",serde_json::to_string(&got).unwrap());
    }
}
