//! Plan limits for the sidebar footer (APP-SPEC-B3): how much of each usage window is spent and when it resets.
//! Codex: the last `rate_limits` in the newest session file. Claude: the statusLine input the app's script kept.
//! Both are read-only; nothing here writes under `~/.codex` or `~/.claude`.
//! Kimi: asked from its server every few minutes with the login Kimi Code keeps fresh while it runs. Shifu only
//! reads that login and never refreshes it (the refresh token changes on every refresh: a second refresher would
//! log Kimi out); with no fresh login the last numbers stay.
//! CodeBuddy: the per-turn usage it writes into its own session files (read-only too).

use crate::config::Config;
use crate::statusline::capture_path;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
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

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
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
    pub antigravity: crate::antigravity::Usage,
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
    pub antigravity: Mutex<crate::antigravity::Usage>,
}
pub type SlowLimits = Arc<Slow>;

pub fn collect(cfg: &Config, cache: &mut CodexCache, slow: &SlowLimits) -> Limits {
    Limits {
        claude: read_claude(&cfg.home),
        codex: read_codex(&codex_sessions_dir(), cache),
        kimi: slow.kimi.lock().map(|k| k.clone()).unwrap_or(None),
        codebuddy: slow.codebuddy.lock().map(|c| c.clone()).unwrap_or(None),
        antigravity: slow.antigravity.lock().map(|v| v.clone()).unwrap_or_default(),
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

/// Kimi Code refreshes its login about every 15 minutes while it runs; between asks the countdown moves on.
const KIMI_EVERY: Duration = Duration::from_secs(300);

fn kimi_home() -> PathBuf {
    match std::env::var("KIMI_CODE_HOME").ok().filter(|d| !d.is_empty()) {
        Some(d) => PathBuf::from(d),
        None => PathBuf::from(std::env::var("HOME").unwrap_or_default()).join(".kimi-code"),
    }
}

/// The access token of Kimi Code's login while it is still good for a minute; `None` when Kimi Code is not logged
/// in or has not run lately.
pub fn fresh_kimi_token(credentials: &str, now: u64) -> Option<String> {
    let v: Value = serde_json::from_str(credentials).ok()?;
    let expires = v["expires_at"].as_u64()?;
    let token = v["access_token"].as_str().filter(|t| !t.is_empty())?;
    (expires > now + 60).then(|| token.to_string())
}

/// "2026-11-09T19:47:46Z" (fractions ignored) as Unix seconds.
fn utc_seconds(text: &str) -> Option<u64> {
    let t = text.strip_suffix('Z')?;
    let (date, time) = t.split_once('T')?;
    let num = |s: &str| s.parse::<i64>().ok();
    let mut d = date.split('-').map(num);
    let (y, m, day) = (d.next()??, d.next()??, d.next()??);
    let mut c = time.split('.').next()?.split(':').map(num);
    let (h, min, sec) = (c.next()??, c.next()??, c.next()??);
    // Days from civil (Howard Hinnant).
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let doy = (153 * (m + if m > 2 { -3 } else { 9 }) + 2) / 5 + day - 1;
    let days = era * 146097 + yoe * 365 + yoe / 4 - yoe / 100 + doy - 719468;
    u64::try_from(days * 86400 + h * 3600 + min * 60 + sec).ok()
}

/// Kimi's `/usages` answer: the short windows from "limits", then the weekly one from "usage", as Kimi's /usage
/// lists them. Counts come as strings.
pub fn parse_kimi(text: &str) -> Result<Vec<Window>, String> {
    let v: Value = serde_json::from_str(text.trim()).map_err(|e| format!("读不懂: {e}"))?;
    let count = |x: &Value| x.as_str().and_then(|s| s.parse::<f64>().ok()).or(x.as_f64());
    let window = |d: &Value, minutes: u64| -> Option<Window> {
        let limit = count(&d["limit"]).filter(|l| *l > 0.0)?;
        let used = count(&d["used"]).or_else(|| count(&d["remaining"]).map(|r| limit - r))?;
        let resets_at = utc_seconds(d["resetTime"].as_str()?)?;
        Some(Window { used_percent: (used * 10000.0 / limit).round() / 100.0, window_minutes: minutes, resets_at })
    };
    let unit = |u: &str| match u {
        "TIME_UNIT_MINUTE" => 1,
        "TIME_UNIT_HOUR" => 60,
        "TIME_UNIT_DAY" => 1440,
        _ => 0,
    };
    let mut found: Vec<Window> = v["limits"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|item| {
            let minutes = item["window"]["duration"].as_u64()? * unit(item["window"]["timeUnit"].as_str()?);
            window(if item["detail"].is_object() { &item["detail"] } else { item }, minutes).filter(|_| minutes > 0)
        })
        .collect();
    found.extend(window(&v["usage"], 7 * 1440));
    if found.is_empty() { Err("没有用量窗口".into()) } else { Ok(found) }
}

/// The token goes to curl on stdin, not in its arguments, where any process could read it.
fn ask_kimi(token: &str) -> Result<VendorLimits, String> {
    let base = std::env::var("KIMI_CODE_BASE_URL").unwrap_or_else(|_| "https://api.kimi.com/coding/v1".to_string());
    let mut child = Command::new("curl")
        .args(["-sS", "--fail", "--max-time", "20", "-K", "-"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("启动 curl 失败: {e}"))?;
    let config = format!("url = \"{}/usages\"\nheader = \"Authorization: Bearer {token}\"\n", base.trim_end_matches('/'));
    child.stdin.take().ok_or("curl 没有输入")?.write_all(config.as_bytes()).map_err(|e| e.to_string())?;
    let out = child.wait_with_output().map_err(|e| e.to_string())?;
    if !out.status.success() {
        return Err(format!("查用量失败: {}", String::from_utf8_lossy(&out.stderr).trim()));
    }
    let windows = parse_kimi(&String::from_utf8_lossy(&out.stdout))?;
    Ok(VendorLimits { windows, credits: None, updated_at: secs(SystemTime::now()) })
}

/// Kimi's last answer, kept across restarts: Shifu can ask only while Kimi Code's own login is fresh (about a quarter of
/// an hour after Kimi last ran), and nothing is spent on the plan while Kimi is closed, so the last answer stays right
/// until Kimi runs again .
fn kimi_saved() -> PathBuf {
    shifu::agent::shifu_home().join("kimi-limits.json")
}

pub fn load_kimi(path: &Path) -> Option<VendorLimits> {
    serde_json::from_str(&std::fs::read_to_string(path).ok()?).ok()
}

/// Written whole and renamed into place, so a reader never sees half a file.
pub fn save_kimi(path: &Path, limits: &VendorLimits) -> std::io::Result<()> {
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, serde_json::to_vec(limits)?)?;
    std::fs::rename(&tmp, path)
}

/// Reads every `POLL`, emits `limits` only when the numbers changed. Kimi and CodeBuddy have their own threads.
pub fn spawn_watcher(app: AppHandle) {
    let slow: SlowLimits = app.state::<SlowLimits>().inner().clone();
    {
        let slow = slow.clone();
        std::thread::spawn(move || loop {
            let result = crate::antigravity::read_usage();
            if let Ok(mut usage) = slow.antigravity.lock() {
                match result {
                    Ok(fresh) => *usage = fresh,
                    Err(error) => usage.error = Some(error),
                }
            }
            std::thread::sleep(Duration::from_secs(300));
        });
    }
    {
        let slow = slow.clone();
        if let (Some(saved), Ok(mut k)) = (load_kimi(&kimi_saved()), slow.kimi.lock()) {
            *k = Some(saved);
        }
        // Looked for every round: Kimi Code may log in, or start and refresh its login, while Shifu runs.
        std::thread::spawn(move || loop {
            let credentials = std::fs::read_to_string(kimi_home().join("credentials/kimi-code.json")).unwrap_or_default();
            match fresh_kimi_token(&credentials, secs(SystemTime::now())).map(|token| ask_kimi(&token)) {
                Some(Ok(v)) => {
                    if let Err(e) = save_kimi(&kimi_saved(), &v) {
                        eprintln!("shifu-app: limits: Kimi not saved: {e}");
                    }
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

    /// The real record shape (11-06): one weekly window, no secondary, credits on.
    const REAL: &str = r#"{"timestamp":"2026-11-06T15:35:09.953Z","type":"event_msg","payload":{"type":"token_count","info":null,"rate_limits":{"limit_id":"codex","limit_name":null,"primary":{"used_percent":100.0,"window_minutes":10080,"resets_at":1794201111},"secondary":null,"credits":{"has_credits":true,"unlimited":false,"balance":"29855.1005840000"},"plan_type":"pro"}}}"#;

    fn w(percent: f64, minutes: u64, resets: u64) -> Window {
        Window { used_percent: percent, window_minutes: minutes, resets_at: resets }
    }

    #[test]
    fn codebuddy_turns_give_credits_and_the_newest_free_quota() {
        // Shaped like the user's session of 11-09: free-model turns carry a quota and cost 0, paid ones cost credits.
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
        // What /usages answered for the user's account on 11-09 (its "usages" ratios left out).
        let real = r#"{"usage":{"limit":"100","used":"6","remaining":"94","resetTime":"2026-11-11T01:47:46Z"},
            "limits":[{"window":{"duration":300,"timeUnit":"TIME_UNIT_MINUTE"},
            "detail":{"limit":"100","used":"13","remaining":"87","resetTime":"2026-11-09T19:47:46.123Z"}}]}"#;
        assert_eq!(parse_kimi(real).unwrap(), vec![w(13.0, 300, 1794253666), w(6.0, 10080, 1794361666)]);
        let only_remaining = r#"{"usage":{"limit":"200","remaining":"150","resetTime":"2026-11-11T01:47:46Z"}}"#;
        assert_eq!(parse_kimi(only_remaining).unwrap(), vec![w(25.0, 10080, 1794361666)]);
        assert!(parse_kimi(r#"{"error":{"code":"unauthorized"}}"#).is_err());
        assert!(parse_kimi("<html>").is_err());
    }

    #[test]
    #[ignore = "asks the user's Kimi account; run with --ignored while Kimi Code has run in the last 15 minutes"]
    fn kimi_real_account() {
        let credentials = std::fs::read_to_string(kimi_home().join("credentials/kimi-code.json")).unwrap();
        let token = fresh_kimi_token(&credentials, secs(SystemTime::now())).expect("no fresh Kimi Code login");
        let got = ask_kimi(&token).unwrap();
        eprintln!("{:?}", got.windows);
        assert!(!got.windows.is_empty());
    }

    #[test]
    fn kimi_answer_is_kept_across_restarts() {
        let dir = temp_dir("kimi-saved");
        let path = dir.join("kimi-limits.json");
        assert_eq!(load_kimi(&path), None);
        let v = VendorLimits { windows: vec![w(13.0, 300, 1794253666)], credits: None, updated_at: 1794240000 };
        save_kimi(&path, &v).unwrap();
        assert_eq!(load_kimi(&path), Some(v));
        std::fs::write(&path, "half").unwrap();
        assert_eq!(load_kimi(&path), None);
    }

    #[test]
    fn kimi_login_is_used_only_while_fresh() {
        let c = r#"{"access_token":"tok","refresh_token":"r","expires_at":1000,"expires_in":900}"#;
        assert_eq!(fresh_kimi_token(c, 500).as_deref(), Some("tok"));
        assert_eq!(fresh_kimi_token(c, 950), None);
        assert_eq!(fresh_kimi_token("", 500), None);
        assert_eq!(fresh_kimi_token(r#"{"access_token":"","expires_at":1000}"#, 500), None);
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
