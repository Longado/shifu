//! Qwen Code usage for the sidebar footer: the tokens its own session records spent, and the Bailian Token Plan
//! quota through the installed `bl` client. Read-only: Shifu never refreshes the Bailian login and never reads the
//! key; a quota it cannot get keeps the last numbers (as Kimi's does).

use crate::limits::{secs, utc_seconds, VendorLimits, Window};
use serde::Serialize;
use serde_json::Value;
use std::io::Read;
use std::os::fd::AsRawFd;
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant, SystemTime};

#[derive(Clone, Debug, Default, PartialEq, Serialize)]
pub struct Tokens {
    pub calls: u64,
    pub input: u64,
    pub output: u64,
    pub cached: u64,
    pub total: u64,
}

impl Tokens {
    fn add(&mut self, other: &Tokens) {
        self.calls += other.calls;
        self.input += other.input;
        self.output += other.output;
        self.cached += other.cached;
        self.total += other.total;
    }
}

/// What Qwen spent in the sessions Shifu started: today since local midnight, and over the last seven days.
#[derive(Clone, Debug, Default, PartialEq, Serialize)]
pub struct QwenUse {
    pub today: Tokens,
    pub week: Tokens,
    /// Unix seconds of the newest record read; 0 when nothing was found.
    pub updated_at: u64,
}

/// Shifu's Qwen home, the same directory `shifu qwen` sets as `QWEN_HOME` (src/qwen.rs). The user's own `~/.qwen`
/// holds other sessions, which this footer is not about.
pub fn home() -> PathBuf {
    shifu::agent::shifu_home().join("qwen")
}

/// Unix seconds of the local midnight that starts the day `now` falls in; `offset` is the local UTC offset.
pub fn midnight(now: u64, offset: i64) -> u64 {
    let local = i64::try_from(now).unwrap_or(i64::MAX).saturating_add(offset);
    (local.div_euclid(86400) * 86400 - offset).max(0) as u64
}

/// The local UTC offset now, in seconds: the day should end when the user's calendar day does, not at UTC midnight.
fn local_offset() -> i64 {
    let now = secs(SystemTime::now()) as libc::time_t;
    // SAFETY: localtime_r fills the caller's own tm and takes the time by reference.
    unsafe {
        let mut broken: libc::tm = std::mem::zeroed();
        if libc::localtime_r(&now, &mut broken).is_null() { 0 } else { broken.tm_gmtoff }
    }
}

/// One assistant record of a Qwen transcript as (Unix seconds, tokens); `None` when the line is not one.
fn record(line: &Value) -> Option<(u64, Tokens)> {
    let usage = line.get("usageMetadata")?.as_object()?;
    let number = |key: &str| usage.get(key).and_then(Value::as_u64);
    let total = number("totalTokenCount")?;
    let at = utc_seconds(line.get("timestamp")?.as_str()?)?;
    Some((at, Tokens {
        calls: 1,
        input: number("promptTokenCount").unwrap_or(0),
        output: number("candidatesTokenCount").unwrap_or(0) + number("thoughtsTokenCount").unwrap_or(0),
        cached: number("cachedContentTokenCount").unwrap_or(0),
        total,
    }))
}

/// One session transcript (`projects/<project>/chats/<id>.jsonl`): every call it recorded and when. Lines it cannot
/// read are skipped (a running session ends mid-line).
pub fn parse_session(text: &str) -> Vec<(u64, Tokens)> {
    text.lines()
        .filter(|line| line.contains("usageMetadata"))
        .filter_map(|line| serde_json::from_str::<Value>(line).ok().and_then(|v| record(&v)))
        .collect()
}

/// Sum the transcripts under `<home>/projects`, reading only the files written since `week_from`.
pub fn read(home: &Path, today_from: u64, week_from: u64) -> QwenUse {
    let mut found = QwenUse::default();
    let mut dirs = vec![home.join("projects")];
    while let Some(dir) = dirs.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else { continue };
        for entry in entries.flatten() {
            let path = entry.path();
            let Ok(meta) = entry.metadata() else { continue };
            if meta.is_dir() {
                dirs.push(path);
                continue;
            }
            if path.extension().and_then(|x| x.to_str()) != Some("jsonl") {
                continue;
            }
            if !meta.modified().is_ok_and(|m| secs(m) >= week_from) {
                continue;
            }
            let Ok(text) = std::fs::read_to_string(&path) else { continue };
            for (at, tokens) in parse_session(&text) {
                if at >= week_from {
                    found.week.add(&tokens);
                }
                if at >= today_from {
                    found.today.add(&tokens);
                }
                found.updated_at = found.updated_at.max(at);
            }
        }
    }
    found
}

/// A transcript line is appended with every answer, so nothing is missed by waiting this long for the next read.
pub const EVERY: Duration = Duration::from_secs(30);

const WEEK: Duration = Duration::from_secs(7 * 86400);

pub fn read_usage() -> QwenUse {
    let now = secs(SystemTime::now());
    read(&home(), midnight(now, local_offset()), now.saturating_sub(WEEK.as_secs()))
}

/// `bl usage token-plan --output json` answers with `per<N><Unit>Percentage` (a 0..1 fraction) and
/// `per<N><Unit>ResetTime` (Unix milliseconds) per window — the keys bailian-cli 2.7.2 writes from the console's
/// `/tokenplan/personal/api/v2/usage`. The units are the CLI's own set; `Month` counts 30 days as it does.
const UNITS: [(&str, u64); 4] = [("Hour", 60), ("Day", 1440), ("Week", 10080), ("Month", 43200)];

/// The window `per5HourPercentage` speaks for (minutes) and whether the key is its use or its reset time.
fn part(key: &str) -> Option<(u64, bool)> {
    let rest = key.strip_prefix("per")?;
    for (unit, minutes) in UNITS {
        for (suffix, is_use) in [("Percentage", true), ("ResetTime", false)] {
            let Some(head) = rest.strip_suffix(suffix) else { continue };
            // What is left of the head is the count: "per5Hour…" is five of the unit, not one.
            let Some(count) = head.strip_suffix(unit) else { continue };
            return Some((count.parse::<u64>().ok()? * minutes, is_use));
        }
    }
    None
}

pub fn parse_quota(text: &str, now: u64) -> Result<VendorLimits, String> {
    let invalid = || "Token Plan 暂无可读取的额度".to_string();
    let report: Value =
        serde_json::from_str(text.trim()).map_err(|_| "读不懂 bl 的额度回答，可能要更新百炼 CLI".to_string())?;
    let Some(entries) = report.as_object() else { return Err(invalid()) };
    // A window's two keys can arrive in either order, so each is collected per window first.
    let mut uses: Vec<(u64, f64)> = Vec::new();
    let mut resets: Vec<(u64, u64)> = Vec::new();
    for (key, value) in entries {
        let Some((minutes, is_use)) = part(key) else { continue };
        if is_use {
            // A window the plan does not cap comes back with no percentage at all; anything outside 0..1 is not the
            // fraction this contract promises, so it is left out rather than guessed at.
            if let Some(fraction) = value.as_f64().filter(|f| (0.0..=1.0).contains(f)) {
                uses.push((minutes, fraction * 100.0));
            }
        } else if let Some(ms) = value.as_u64() {
            resets.push((minutes, ms / 1000));
        }
    }
    if uses.is_empty() {
        return Err(invalid());
    }
    // A reset time for a window that has no percentage says nothing about use, so only the percentage windows count.
    let mut windows: Vec<Window> = uses
        .into_iter()
        .map(|(minutes, used_percent)| Window {
            used_percent,
            window_minutes: minutes,
            resets_at: resets.iter().find(|(m, _)| *m == minutes).map_or(0, |(_, at)| *at),
        })
        .collect();
    windows.sort_by_key(|w| w.window_minutes);
    Ok(VendorLimits { windows, credits: None, updated_at: now, error: None })
}

/// The Bailian CLI to ask: `bl` on PATH, else the per-user install. The app is not always started from a shell that
/// has it.
fn locate() -> Option<PathBuf> {
    let here = |dir: PathBuf| {
        let candidate = dir.join("bl");
        candidate.is_file().then_some(candidate)
    };
    std::env::var_os("PATH")
        .into_iter()
        .flat_map(|path| std::env::split_paths(&path).collect::<Vec<_>>())
        .find_map(here)
        .or_else(|| std::env::var_os("HOME").map(PathBuf::from).and_then(|h| here(h.join(".local/bin"))))
}

const NOT_FOUND: &str = "找不到 bl 命令，装一下百炼 CLI 才能看 Token Plan 额度";
const TIMEOUT: Duration = Duration::from_secs(30);

pub fn read_quota() -> Result<VendorLimits, String> {
    let now = secs(SystemTime::now());
    let program = locate().ok_or(NOT_FOUND)?;
    let out = ask(&program).map_err(|e| if e == NOT_FOUND { e.to_string() } else { e })?;
    if !out.status.success() {
        let text = String::from_utf8_lossy(&out.stderr)
            .lines()
            .find(|l| !l.trim().is_empty())
            .unwrap_or("")
            .trim()
            .to_string();
        // The CLI says this when only the API key is configured: the quota lives behind a console login.
        return Err(if text.contains("console access token") {
            "百炼控制台未登录，终端里跑一次 bl auth login --console".into()
        } else {
            format!("Token Plan 额度查询失败：{text}")
        });
    }
    parse_quota(&String::from_utf8_lossy(&out.stdout), now)
}

/// Run the report call without letting a stalled client hold the reader thread.
fn ask(program: &Path) -> Result<std::process::Output, String> {
    let mut child = Command::new(program)
        .args(["usage", "token-plan", "--output", "json"])
        .env_remove("SHIFU_ID")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .process_group(0)
        .spawn()
        .map_err(|_| NOT_FOUND.to_string())?;
    let mut stdout = child.stdout.take().ok_or(NOT_FOUND.to_string())?;
    let mut stderr = child.stderr.take().ok_or(NOT_FOUND.to_string())?;
    // Nonblocking reads bound the whole call, including a client that never prints anything.
    for fd in [stdout.as_raw_fd(), stderr.as_raw_fd()] {
        // SAFETY: fcntl on the pipes this child was spawned with; O_NONBLOCK only changes how reads answer.
        unsafe {
            let flags = libc::fcntl(fd, libc::F_GETFL);
            if flags < 0 || libc::fcntl(fd, libc::F_SETFL, flags | libc::O_NONBLOCK) < 0 {
                return Err("无法准备 Token Plan 额度查询".into());
            }
        }
    }
    let start = Instant::now();
    let (mut out, mut err) = (Vec::new(), Vec::new());
    let status = loop {
        for (buffer, stream) in [(&mut out, &mut stdout as &mut dyn Read), (&mut err, &mut stderr as &mut dyn Read)] {
            let mut bytes = [0u8; 8192];
            match stream.read(&mut bytes) {
                Ok(n) if n > 0 => buffer.extend_from_slice(&bytes[..n]),
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {}
                Err(_) => return Err("Token Plan 额度读取失败".into()),
                _ => {}
            }
        }
        if let Some(status) = child.try_wait().map_err(|_| "无法读取 bl 进程状态".to_string())? {
            break status;
        }
        if start.elapsed() >= TIMEOUT {
            // SAFETY: process_group(0) made this child the leader of a group holding only this call.
            unsafe { libc::kill(-(child.id() as i32), libc::SIGKILL) };
            return Err("Token Plan 额度查询超时".into());
        }
        std::thread::sleep(Duration::from_millis(50));
    };
    let _ = child.wait();
    Ok(std::process::Output { status, stdout: out, stderr: err })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::fs::{self, File, FileTimes};
    use std::os::unix::fs::PermissionsExt;
    use std::time::UNIX_EPOCH;

    fn line(at: &str, prompt: u64, candidates: u64, thoughts: u64, cached: u64, total: u64) -> String {
        json!({"uuid":"u","timestamp":at,"type":"assistant","model":"qwen4-max-preview",
            "usageMetadata":{"promptTokenCount":prompt,"candidatesTokenCount":candidates,
                "thoughtsTokenCount":thoughts,"cachedContentTokenCount":cached,"totalTokenCount":total}})
        .to_string()
    }

    #[test]
    fn a_transcript_line_gives_its_call_and_tokens() {
        // Shaped like a real session file in ~/.shifu/app/qwen/projects.
        let text = [
            line("2026-11-17T01:00:00.000Z", 26933, 221, 127, 0, 27154),
            line("2026-11-17T01:00:06.000Z", 27210, 114, 18, 26927, 27324),
            r#"{"timestamp":"2026-11-17T01:00:07.000Z","type":"user"}"#.to_string(),
            r#"{"truncated":"half a line"#.to_string(),
        ]
        .join("\n");
        let found = parse_session(&text);
        assert_eq!(found.len(), 2);
        assert_eq!(found[0].0, utc_seconds("2026-11-17T01:00:00Z").unwrap());
        assert_eq!(found[0].1, Tokens { calls: 1, input: 26933, output: 348, cached: 0, total: 27154 });
        assert_eq!(found[1].1.cached, 26927);
    }

    /// The mtime cut is what keeps the read cheap, so the fixture sets file times as well as record times.
    fn touch(path: &Path, at: u64) {
        let file = File::options().write(true).open(path).unwrap();
        file.set_times(FileTimes::new().set_modified(UNIX_EPOCH + Duration::from_secs(at))).unwrap();
    }

    #[test]
    fn today_and_the_week_are_summed_from_the_transcripts() {
        // The cuts have to be on the same clock as the records, or every 2026 record lands in "today".
        let week_from = utc_seconds("2026-11-10T00:00:00Z").unwrap();
        let today_from = utc_seconds("2026-11-17T00:00:00Z").unwrap();
        let root = std::env::temp_dir().join(format!("shifu-qwen-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        let chats = root.join("projects").join("-w").join("chats");
        fs::create_dir_all(&chats).unwrap();
        fs::write(
            chats.join("a.jsonl"),
            [line("2026-11-10T00:00:00Z", 10, 1, 0, 0, 11), line("2026-11-17T00:00:00Z", 1000, 20, 5, 900, 1025)]
                .join("\n"),
        )
        .unwrap();
        fs::write(chats.join("b.jsonl"), line("2026-11-17T01:00:00Z", 500, 10, 0, 400, 510)).unwrap();
        // Written before the week started: not opened at all, though its record time would be inside it.
        fs::write(chats.join("old.jsonl"), line("2026-11-20T00:00:00Z", 9, 9, 9, 9, 27)).unwrap();
        touch(&chats.join("a.jsonl"), today_from + 20);
        touch(&chats.join("b.jsonl"), today_from + 30);
        touch(&chats.join("old.jsonl"), week_from - 1);

        let found = read(&root, today_from, week_from);
        // Today is the two 11-17 records; the week adds a.jsonl's 11-10 one. old.jsonl is never opened.
        assert_eq!(found.today, Tokens { calls: 2, input: 1500, output: 35, cached: 1300, total: 1535 });
        assert_eq!(found.week, Tokens { calls: 3, input: 1510, output: 36, cached: 1300, total: 1546 },
            "the earlier call of the week counts in the week only");
        assert_eq!(found.updated_at, utc_seconds("2026-11-17T01:00:00Z").unwrap());
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn local_midnight_follows_the_offset() {
        let noon = utc_seconds("2026-11-17T04:00:00Z").unwrap();
        assert_eq!(midnight(noon, 8 * 3600), utc_seconds("2026-11-16T16:00:00Z").unwrap());
        assert_eq!(midnight(noon, 0), utc_seconds("2026-11-17T00:00:00Z").unwrap());
        // At 04:00Z it is still 23:00 of the 16th five hours west, so that local day began at 05:00Z of the 16th.
        assert_eq!(midnight(noon, -5 * 3600), utc_seconds("2026-11-16T05:00:00Z").unwrap());
    }

    #[test]
    fn token_plan_windows_come_from_the_percentage_and_reset_time() {
        // The keys bailian-cli 2.7.2 writes for `usage token-plan --output json`, in its own order.
        let text = json!({
            "per1HourPercentage": 0.04, "per1HourResetTime": 1_794_900_000_000u64,
            "per5HourPercentage": 0.375, "per5HourResetTime": 1_794_910_000_000u64,
            "per1WeekPercentage": 0.12, "per1WeekResetTime": 1_795_000_000_000u64,
            "hasQuotaData": true
        })
        .to_string();
        let data = parse_quota(&text, 1_794_880_000).unwrap();
        assert_eq!(data.windows.len(), 3);
        assert_eq!(data.windows[0], Window { used_percent: 4.0, window_minutes: 60, resets_at: 1_794_900_000 });
        assert_eq!(data.windows[1].used_percent, 37.5);
        assert_eq!(data.windows[2].window_minutes, 10080);
        assert_eq!(data.updated_at, 1_794_880_000);
    }

    #[test]
    fn a_percentage_without_a_reset_time_keeps_its_window() {
        let data = parse_quota(&json!({"per1MonthPercentage": 0.5}).to_string(), 10).unwrap();
        assert_eq!(data.windows, vec![Window { used_percent: 50.0, window_minutes: 43200, resets_at: 0 }]);
    }

    #[test]
    fn no_quota_data_says_so_and_a_foreign_answer_is_not_guessed() {
        assert!(parse_quota(&json!({"hasQuotaData": false}).to_string(), 10).is_err());
        assert!(parse_quota(&json!({"per1HourPercentage": 37.5}).to_string(), 10).is_err(), "not a fraction");
        assert!(parse_quota("not json".into(), 10).is_err());
        assert!(parse_quota(&json!({"perFooPercentage": 0.5}).to_string(), 10).is_err());
    }

    #[test]
    fn asks_the_installed_client_for_the_report_only() {
        let root = std::env::temp_dir().join(format!("shifu-qwen-bl-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        let script = root.join("bl");
        let reply = json!({"per5HourPercentage": 0.2, "per5HourResetTime": 1_794_910_000_000u64}).to_string();
        fs::write(
            &script,
            format!("#!/bin/sh\n[ \"$*\" = 'usage token-plan --output json' ] || exit 7\ncat <<'REPORT'\n{reply}\nREPORT\n"),
        )
        .unwrap();
        fs::set_permissions(&script, fs::Permissions::from_mode(0o755)).unwrap();
        let out = ask(&script).unwrap();
        assert!(out.status.success(), "wrong arguments would leave exit 7");
        let data = parse_quota(&String::from_utf8_lossy(&out.stdout), 1_794_880_000).unwrap();
        assert_eq!(data.windows, vec![Window { used_percent: 20.0, window_minutes: 300, resets_at: 1_794_910_000 }]);
        let _ = fs::remove_dir_all(&root);
    }

    /// A client that answers with an error rather than a report must not pass as a plan without limits.
    #[test]
    fn a_failed_call_is_reported_as_the_reason_it_failed() {
        let root = std::env::temp_dir().join(format!("shifu-qwen-bl-fail-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        let script = root.join("bl");
        fs::write(&script, "#!/bin/sh\necho '' >&2\necho 'Error:       No console access token found.' >&2\nexit 3\n")
            .unwrap();
        fs::set_permissions(&script, fs::Permissions::from_mode(0o755)).unwrap();
        // Same wording as the real client, checked against the same parser path.
        let out = ask(&script).unwrap();
        assert!(!out.status.success());
        let text = String::from_utf8_lossy(&out.stderr).to_string();
        assert!(text.contains("console access token"), "{text}");
        let _ = fs::remove_dir_all(&root);
    }
}
