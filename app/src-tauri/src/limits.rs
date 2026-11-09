//! Plan limits for the sidebar footer (APP-SPEC-B3): how much of each usage window is spent and when it resets.
//! Codex: the last `rate_limits` in the newest session file. Claude: the statusLine input the app's script kept.
//! Both are read-only; nothing here writes under `~/.codex` or `~/.claude`.
//! Kimi: asked from its server every few minutes through Kimi's own Python (kimi_usage.py), which may refresh its
//! login under its own lock, as Kimi does; Shifu never sees the token.
//! CodeBuddy: the per-turn usage it writes into its own session files (read-only too).

use crate::config::Config;
use crate::statusline::capture_path;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use tauri::{AppHandle, Emitter, Manager};

const POLL: Duration = Duration::from_secs(3);
/// A rollout file can be tens of MB; the last `token_count` is almost always in the tail. Grow the read only when
/// it is not found.
const TAIL_SIZES: [u64; 3] = [256 * 1024, 4 * 1024 * 1024, 32 * 1024 * 1024];
/// A file that Codex has just opened has no `token_count` yet; look at the next newest ones before giving up.
const FILES_TRIED: usize = 5;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Window {
    pub used_percent: f64,
    pub window_minutes: u64,
    /// Unix seconds.
    pub resets_at: u64,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct VendorLimits {
    pub windows: Vec<Window>,
    /// The balance as Codex writes it, only when the account has credits.
    pub credits: Option<String>,
    /// Unix seconds: when the data was recorded (the file's modification time).
    pub updated_at: u64,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize)]
pub struct Limits {
    pub claude: Option<VendorLimits>,
    pub codex: Option<VendorLimits>,
    pub kimi: Option<VendorLimits>,
    pub codebuddy: Option<CodeBuddyUse>,
}

/// CodeBuddy has no windows on the paid models, only credits per turn; the free model also reports how much of its
/// quota is gone.
#[derive(Clone, Debug, Default, PartialEq, Serialize)]
pub struct CodeBuddyUse {
    /// Every charged turn of the last two days as [Unix ms, credits]; the page adds up what falls on its today.
    pub spends: Vec<(u64, f64)>,
    /// The newest free-model quota seen, while its reset is still ahead.
    pub free: Option<FreeQuota>,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct FreeQuota {
    pub used_percent: f64,
    /// Unix seconds.
    pub resets_at: u64,
}

/// Windows and the credits balance (when the account has credits) of one `token_count` record.
pub type CodexRecord = (Vec<Window>, Option<String>);

fn window_of(v: &Value, percent_key: &str, minutes: Option<u64>) -> Option<Window> {
    Some(Window {
        used_percent: v.get(percent_key)?.as_f64()?,
        window_minutes: minutes.or_else(|| v.get("window_minutes").and_then(Value::as_u64))?,
        resets_at: v.get("resets_at")?.as_u64()?,
    })
}

/// The last record in `text` (JSON lines) that is a `token_count` event with at least one window.
pub fn parse_codex(text: &str) -> Option<CodexRecord> {
    text.lines().rev().filter(|l| l.contains("\"token_count\"") && l.contains("\"rate_limits\"")).find_map(|line| {
        let v: Value = serde_json::from_str(line).ok()?;
        if v.get("type")?.as_str()? != "event_msg" || v.pointer("/payload/type")?.as_str()? != "token_count" {
            return None;
        }
        let limits = v.pointer("/payload/rate_limits")?;
        let windows: Vec<Window> =
            ["primary", "secondary"].iter().filter_map(|k| window_of(limits.get(*k)?, "used_percent", None)).collect();
        if windows.is_empty() {
            return None;
        }
        let credits = limits
            .get("credits")
            .filter(|c| c.get("has_credits").and_then(Value::as_bool) == Some(true))
            .and_then(|c| c.get("balance")?.as_str())
            .map(str::to_string);
        Some((windows, credits))
    })
}

/// The 5-hour and 7-day windows out of the statusLine input; a window missing a field is left out.
pub fn parse_claude(json: &str) -> Option<Vec<Window>> {
    let v: Value = serde_json::from_str(json).ok()?;
    let limits = v.get("rate_limits")?;
    let windows: Vec<Window> = [("five_hour", 300), ("seven_day", 10080)]
        .iter()
        .filter_map(|(key, minutes)| window_of(limits.get(*key)?, "used_percentage", Some(*minutes)))
        .collect();
    (!windows.is_empty()).then_some(windows)
}

fn secs(t: SystemTime) -> u64 {
    t.duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

/// The last `size` bytes as text, without the line that the cut may have split.
fn read_tail(path: &Path, size: u64) -> Result<(String, bool), String> {
    let mut file = std::fs::File::open(path).map_err(|e| format!("open {} failed: {e}", path.display()))?;
    let len = file.metadata().map_err(|e| format!("stat {} failed: {e}", path.display()))?.len();
    let start = len.saturating_sub(size);
    file.seek(SeekFrom::Start(start)).map_err(|e| format!("seek {} failed: {e}", path.display()))?;
    let mut bytes = Vec::new();
    file.read_to_end(&mut bytes).map_err(|e| format!("read {} failed: {e}", path.display()))?;
    let text = String::from_utf8_lossy(&bytes);
    let text = if start > 0 { text.split_once('\n').map(|(_, rest)| rest).unwrap_or("") } else { &text };
    Ok((text.to_string(), start == 0))
}

/// Rollout files under `sessions` (any depth), newest modification time first. Long conversations keep writing to
/// a file in an old date folder, so the folder names say nothing.
fn rollouts_newest_first(sessions: &Path) -> Vec<(PathBuf, SystemTime, u64)> {
    let mut found = Vec::new();
    let mut dirs = vec![sessions.to_path_buf()];
    while let Some(dir) = dirs.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else { continue };
        for entry in entries.flatten() {
            let path = entry.path();
            let Ok(meta) = entry.metadata() else { continue };
            if meta.is_dir() {
                dirs.push(path);
            } else if path.file_name().and_then(|n| n.to_str()).is_some_and(|n| n.starts_with("rollout-") && n.ends_with(".jsonl"))
                && let Ok(modified) = meta.modified()
            {
                found.push((path, modified, meta.len()));
            }
        }
    }
    found.sort_by_key(|f| std::cmp::Reverse(f.1));
    found
}

/// Tail of one file, reading more only if the record is not in what was read.
fn codex_from_file(path: &Path) -> Result<Option<CodexRecord>, String> {
    for size in TAIL_SIZES {
        let (text, whole) = read_tail(path, size)?;
        if let Some(found) = parse_codex(&text) {
            return Ok(Some(found));
        }
        if whole {
            break;
        }
    }
    Ok(None)
}

/// What the last read of the newest rollout file produced; the key says whether it is still current.
#[derive(Default)]
pub struct CodexCache {
    key: Option<(PathBuf, SystemTime, u64)>,
    value: Option<VendorLimits>,
}

pub fn read_codex(sessions: &Path, cache: &mut CodexCache) -> Option<VendorLimits> {
    let files = rollouts_newest_first(sessions);
    let newest = files.first()?;
    if cache.key.as_ref() == Some(newest) {
        return cache.value.clone();
    }
    let value = files.iter().take(FILES_TRIED).find_map(|(path, modified, _)| match codex_from_file(path) {
        Ok(found) => found.map(|(windows, credits)| VendorLimits { windows, credits, updated_at: secs(*modified) }),
        Err(e) => {
            eprintln!("shifu-app: limits: {e}");
            None
        }
    });
    cache.key = Some(newest.clone());
    cache.value = value.clone();
    value
}

pub fn read_claude(home: &Path) -> Option<VendorLimits> {
    let path = capture_path(home);
    let text = match std::fs::read_to_string(&path) {
        Ok(t) => t,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return None,
        Err(e) => {
            eprintln!("shifu-app: limits: read {} failed: {e}", path.display());
            return None;
        }
    };
    let windows = parse_claude(&text)?;
    let updated_at = std::fs::metadata(&path).and_then(|m| m.modified()).map(secs).unwrap_or(0);
    Some(VendorLimits { windows, credits: None, updated_at })
}

/// `$CODEX_HOME/sessions`, else `~/.codex/sessions` (Codex itself honours `CODEX_HOME`).
pub fn codex_sessions_dir() -> PathBuf {
    let base = match std::env::var("CODEX_HOME").ok().filter(|h| !h.is_empty()) {
        Some(h) => PathBuf::from(h),
        None => PathBuf::from(std::env::var("HOME").unwrap_or_default()).join(".codex"),
    };
    base.join("sessions")
}

/// What the slower readers found last, kept between their rounds; each is filled by its own thread (`spawn_watcher`).
#[derive(Default)]
pub struct Slow {
    pub kimi: Mutex<Option<VendorLimits>>,
    pub codebuddy: Mutex<Option<CodeBuddyUse>>,
}
pub type SlowLimits = Arc<Slow>;

pub fn collect(cfg: &Config, cache: &mut CodexCache, slow: &SlowLimits) -> Limits {
    Limits {
        claude: read_claude(&cfg.home),
        codex: read_codex(&codex_sessions_dir(), cache),
        kimi: slow.kimi.lock().map(|k| k.clone()).unwrap_or(None),
        codebuddy: slow.codebuddy.lock().map(|c| c.clone()).unwrap_or(None),
    }
}

/// One CodeBuddy session file: its charged turns since `since_ms`, and its newest free-model quota with the time it
/// was written. Lines it cannot read are skipped (a file being written ends mid-line).
pub fn parse_codebuddy(text: &str, since_ms: u64) -> (Vec<(u64, f64)>, Option<(u64, FreeQuota)>) {
    let mut spends = Vec::new();
    let mut quota: Option<(u64, FreeQuota)> = None;
    for line in text.lines() {
        let Ok(v) = serde_json::from_str::<Value>(line) else { continue };
        let at = v["timestamp"].as_u64().unwrap_or(0);
        let usage = &v["providerData"]["rawUsage"];
        if let Some(credit) = usage["credit"].as_f64().filter(|c| *c > 0.0) {
            if at >= since_ms {
                spends.push((at, credit));
            }
        }
        let q = &usage["quota"];
        let reset = q["reset_at"].as_u64().or_else(|| q["reset_at"].as_str().and_then(|s| s.parse().ok()));
        if let (Some(used_percent), Some(resets_at)) = (q["percent"].as_f64(), reset) {
            if quota.as_ref().is_none_or(|(t, _)| at >= *t) {
                quota = Some((at, FreeQuota { used_percent, resets_at }));
            }
        }
    }
    (spends, quota)
}

const CODEBUDDY_EVERY: Duration = Duration::from_secs(30);
const TWO_DAYS: Duration = Duration::from_secs(2 * 86400);

/// Every session file (sub-agents' too: they spend as well) written in the last two days.
fn read_codebuddy(projects: &Path) -> CodeBuddyUse {
    let now = SystemTime::now();
    let since = now - TWO_DAYS;
    let since_ms = since.duration_since(UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0);
    let mut use_ = CodeBuddyUse::default();
    let mut newest: Option<(u64, FreeQuota)> = None;
    let mut dirs = vec![projects.to_path_buf()];
    while let Some(dir) = dirs.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else { continue };
        for e in entries.flatten() {
            let path = e.path();
            let Ok(meta) = e.metadata() else { continue };
            if meta.is_dir() {
                dirs.push(path);
            } else if path.extension().is_some_and(|x| x == "jsonl") && meta.modified().is_ok_and(|m| m >= since) {
                let Ok(text) = std::fs::read_to_string(&path) else { continue };
                let (spends, quota) = parse_codebuddy(&text, since_ms);
                use_.spends.extend(spends);
                if let Some(q) = quota.filter(|(t, _)| newest.as_ref().is_none_or(|(n, _)| t > n)) {
                    newest = Some(q);
                }
            }
        }
    }
    use_.spends.sort_by_key(|(t, _)| *t);
    use_.free = newest.map(|(_, q)| q).filter(|q| q.resets_at > secs(now));
    use_
}

const KIMI_SCRIPT: &str = include_str!("kimi_usage.py");
/// Each ask starts Python and may refresh Kimi's login, so not often; the reset countdown moves on in between.
const KIMI_EVERY: Duration = Duration::from_secs(300);
const KIMI_TIMEOUT: Duration = Duration::from_secs(30);

/// The Python of Kimi's install: `kimi-cli` on PATH is a link into its uv tool environment, next to that `python`.
fn kimi_python() -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    let program = std::env::split_paths(&path).map(|d| d.join("kimi-cli")).find(|p| p.is_file())?;
    Some(std::fs::canonicalize(program).ok()?.with_file_name("python")).filter(|p| p.is_file())
}

/// The script's answer: windows, or why there are none.
pub fn parse_kimi(text: &str) -> Result<Vec<Window>, String> {
    #[derive(Deserialize)]
    struct Answer {
        windows: Option<Vec<Window>>,
        error: Option<String>,
    }
    let a: Answer = serde_json::from_str(text.trim()).map_err(|e| format!("读不懂: {e}"))?;
    match (a.windows, a.error) {
        (_, Some(e)) => Err(e),
        (Some(w), None) if !w.is_empty() => Ok(w),
        _ => Err("没有用量窗口".into()),
    }
}

/// Errors are logged by the caller, which keeps the last good answer.
fn ask_kimi(python: &Path) -> Result<VendorLimits, String> {
    let mut child = Command::new(python)
        .args(["-I", "-c", KIMI_SCRIPT])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| format!("启动 {} 失败: {e}", python.display()))?;
    let deadline = Instant::now() + KIMI_TIMEOUT;
    while child.try_wait().map_err(|e| e.to_string())?.is_none() {
        if Instant::now() > deadline {
            let _ = child.kill();
            let _ = child.wait();
            return Err("超时".into());
        }
        std::thread::sleep(Duration::from_millis(200));
    }
    let mut out = String::new();
    child.stdout.take().ok_or("没有输出")?.read_to_string(&mut out).map_err(|e| e.to_string())?;
    let windows = parse_kimi(&out)?;
    Ok(VendorLimits { windows, credits: None, updated_at: secs(SystemTime::now()) })
}

/// Reads every `POLL`, emits `limits` only when the numbers changed. Kimi and CodeBuddy have their own threads.
pub fn spawn_watcher(app: AppHandle) {
    let slow: SlowLimits = app.state::<SlowLimits>().inner().clone();
    {
        let slow = slow.clone();
        // Kimi is looked for every round: it may be installed, or reinstalled, while Shifu runs.
        std::thread::spawn(move || loop {
            match kimi_python().map(|python| ask_kimi(&python)) {
                Some(Ok(v)) => {
                    if let Ok(mut k) = slow.kimi.lock() {
                        *k = Some(v);
                    }
                }
                Some(Err(e)) => eprintln!("shifu-app: limits: Kimi: {e}"),
                None => {}
            }
            std::thread::sleep(KIMI_EVERY);
        });
    }
    let projects = PathBuf::from(std::env::var("HOME").unwrap_or_default()).join(".codebuddy/projects");
    if projects.is_dir() {
        let slow = slow.clone();
        std::thread::spawn(move || loop {
            let found = read_codebuddy(&projects);
            if let Ok(mut c) = slow.codebuddy.lock() {
                *c = Some(found);
            }
            std::thread::sleep(CODEBUDDY_EVERY);
        });
    }
    std::thread::spawn(move || {
        let cfg = app.state::<Config>();
        let mut cache = CodexCache::default();
        let mut last: Option<Limits> = None;
        loop {
            let limits = collect(&cfg, &mut cache, &slow);
            if last.as_ref() != Some(&limits) {
                if let Err(e) = app.emit("limits", &limits) {
                    eprintln!("shifu-app: emit limits failed: {e}");
                }
                last = Some(limits);
            }
            std::thread::sleep(POLL);
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The real record shape: one weekly window, no secondary, credits on.
    const REAL: &str = r#"{"timestamp":"2026-11-06T15:35:09.953Z","type":"event_msg","payload":{"type":"token_count","info":null,"rate_limits":{"limit_id":"codex","limit_name":null,"primary":{"used_percent":100.0,"window_minutes":10080,"resets_at":1794201111},"secondary":null,"credits":{"has_credits":true,"unlimited":false,"balance":"29855.1005840000"},"plan_type":"pro"}}}"#;

    fn w(percent: f64, minutes: u64, resets: u64) -> Window {
        Window { used_percent: percent, window_minutes: minutes, resets_at: resets }
    }

    #[test]
    fn codebuddy_turns_give_credits_and_the_newest_free_quota() {
        // Shaped like a real session: free-model turns carry a quota and cost 0, paid ones cost credits.
        let line = |at: u64, credit: f64, quota: &str| {
            format!(r#"{{"timestamp":{at},"providerData":{{"model":"hy4","rawUsage":{{"total_tokens":9,"credit":{credit},"quota":{quota}}}}}}}"#)
        };
        let text = [
            line(1000, 0.0, r#"{"model":"hy4","percent":9,"reset_at":"1794290954","shared_models":[]}"#),
            line(2000, 0.0, r#"{"model":"hy4","percent":12,"reset_at":"1794290954","shared_models":[]}"#),
            line(3000, 15.06, "null"),
            "{\"type\":\"user\"}".to_string(),
            line(4000, 1.75, "null"),
            "{\"timestamp\":5000,\"provid".to_string(),
        ]
        .join("\n");
        let (spends, quota) = parse_codebuddy(&text, 3500);
        assert_eq!(spends, vec![(4000, 1.75)]);
        assert_eq!(quota, Some((2000, FreeQuota { used_percent: 12.0, resets_at: 1794290954 })));
        assert_eq!(parse_codebuddy(&text, 0).0.len(), 2);
    }

    #[test]
    fn kimi_answer_gives_its_windows_or_says_why_not() {
        // What kimi_usage.py printed for a real account.
        let real = r#"{"windows": [{"used_percent": 2.0, "window_minutes": 300, "resets_at": 1794235666}, {"used_percent": 0.0, "window_minutes": 10080, "resets_at": 1794361666}]}"#;
        assert_eq!(parse_kimi(real).unwrap(), vec![w(2.0, 300, 1794235666), w(0.0, 10080, 1794361666)]);
        assert_eq!(parse_kimi(r#"{"error": "查用量返回 401"}"#), Err("查用量返回 401".to_string()));
        assert!(parse_kimi(r#"{"windows": []}"#).is_err());
        assert!(parse_kimi("Traceback (most recent call last):").is_err());
    }

    #[test]
    fn real_codex_record_gives_the_weekly_window_and_credits() {
        let (windows, credits) = parse_codex(REAL).unwrap();
        assert_eq!(windows, vec![w(100.0, 10080, 1794201111)]);
        assert_eq!(credits.as_deref(), Some("29855.1005840000"));
    }

    #[test]
    fn last_usable_record_wins_and_unusable_ones_are_skipped() {
        let older = REAL.replace("100.0", "40.0");
        let no_limits = r#"{"type":"event_msg","payload":{"type":"token_count","info":null,"rate_limits":null}}"#;
        let empty = r#"{"type":"event_msg","payload":{"type":"token_count","rate_limits":{"primary":null,"secondary":null}}}"#;
        let other_event = REAL.replace("\"event_msg\"", "\"response_item\"");
        let text = format!("{older}\n{REAL}\n{no_limits}\n{empty}\n{other_event}\nnot json token_count rate_limits\n");
        assert_eq!(parse_codex(&text).unwrap().0[0].used_percent, 100.0);
        assert_eq!(parse_codex(&format!("{older}\n{no_limits}\n")).unwrap().0[0].used_percent, 40.0);
        assert_eq!(parse_codex(&format!("{no_limits}\n{empty}\n")), None);
    }

    #[test]
    fn both_windows_and_no_credits() {
        let line = r#"{"type":"event_msg","payload":{"type":"token_count","rate_limits":{"primary":{"used_percent":12.5,"window_minutes":300,"resets_at":10},"secondary":{"used_percent":3.0,"window_minutes":10080,"resets_at":20},"credits":{"has_credits":false,"balance":"0"}}}}"#;
        let (windows, credits) = parse_codex(line).unwrap();
        assert_eq!(windows, vec![w(12.5, 300, 10), w(3.0, 10080, 20)]);
        assert_eq!(credits, None);
    }

    #[test]
    fn claude_input_with_only_the_seven_day_window() {
        let json = r#"{"model":{"id":"x"},"rate_limits":{"seven_day":{"used_percentage":71,"resets_at":1794164400}}}"#;
        assert_eq!(parse_claude(json).unwrap(), vec![w(71.0, 10080, 1794164400)]);
    }

    #[test]
    fn claude_input_with_both_windows_and_decimals() {
        let json = r#"{"rate_limits":{"five_hour":{"used_percentage":42.5,"resets_at":100},"seven_day":{"used_percentage":63,"resets_at":200}}}"#;
        assert_eq!(parse_claude(json).unwrap(), vec![w(42.5, 300, 100), w(63.0, 10080, 200)]);
    }

    #[test]
    fn claude_input_with_missing_fields_is_left_out() {
        assert_eq!(parse_claude(r#"{"model":{}}"#), None);
        assert_eq!(parse_claude(r#"{"rate_limits":{}}"#), None);
        assert_eq!(parse_claude(r#"{"rate_limits":{"five_hour":{"used_percentage":5}}}"#), None);
        let json = r#"{"rate_limits":{"five_hour":{"resets_at":5},"seven_day":{"used_percentage":9,"resets_at":7}}}"#;
        assert_eq!(parse_claude(json).unwrap(), vec![w(9.0, 10080, 7)]);
        assert_eq!(parse_claude("{oops"), None);
    }

    fn temp_dir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("shifu-limits-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn newest_file_by_modification_time_wins_even_in_an_old_date_folder_and_only_the_tail_is_read() {
        let root = temp_dir("tail");
        let old_day = root.join("2026/11/03");
        let new_day = root.join("2026/11/06");
        std::fs::create_dir_all(&old_day).unwrap();
        std::fs::create_dir_all(&new_day).unwrap();
        // Written first, in the newer folder, with different numbers.
        std::fs::write(new_day.join("rollout-b.jsonl"), REAL.replace("100.0", "1.0")).unwrap();
        std::thread::sleep(Duration::from_millis(1100));
        // A long conversation: a big line of padding, then the record, then a trailing line; the cut lands inside padding.
        let padding = "x".repeat(600 * 1024);
        let content = format!("{{\"pad\":\"{padding}\"}}\n{REAL}\n{{\"type\":\"event_msg\",\"payload\":{{\"type\":\"agent_message\"}}}}\n");
        std::fs::write(old_day.join("rollout-a.jsonl"), content).unwrap();
        std::fs::write(old_day.join("notes.txt"), "ignored").unwrap();
        let got = read_codex(&root, &mut CodexCache::default()).unwrap();
        assert_eq!(got.windows[0].used_percent, 100.0);
        assert_eq!(got.credits.as_deref(), Some("29855.1005840000"));
        assert!(got.updated_at > 1_700_000_000);
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn a_just_opened_file_without_a_record_falls_back_to_the_next_newest() {
        let root = temp_dir("fallback");
        std::fs::write(root.join("rollout-old.jsonl"), format!("{REAL}\n")).unwrap();
        std::thread::sleep(Duration::from_millis(1100));
        std::fs::write(root.join("rollout-new.jsonl"), "{\"type\":\"session_meta\"}\n").unwrap();
        let mut cache = CodexCache::default();
        assert_eq!(read_codex(&root, &mut cache).unwrap().windows[0].resets_at, 1794201111);
        // Unchanged files are not read again: the cached value comes back even if the old file vanishes.
        std::fs::remove_file(root.join("rollout-old.jsonl")).unwrap();
        assert!(read_codex(&root, &mut cache).is_some());
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn no_sessions_folder_or_no_files_means_no_codex_data() {
        let root = temp_dir("none");
        assert_eq!(read_codex(&root.join("missing"), &mut CodexCache::default()), None);
        assert_eq!(read_codex(&root, &mut CodexCache::default()), None);
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn claude_capture_file_is_read_with_its_modification_time() {
        let home = temp_dir("claude");
        assert_eq!(read_claude(&home), None);
        std::fs::write(capture_path(&home), r#"{"rate_limits":{"seven_day":{"used_percentage":71,"resets_at":5}}}"#).unwrap();
        let got = read_claude(&home).unwrap();
        assert_eq!(got.windows, vec![w(71.0, 10080, 5)]);
        assert!(got.updated_at > 1_700_000_000 && got.credits.is_none());
        std::fs::remove_dir_all(&home).unwrap();
    }
}
