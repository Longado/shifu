//! Steamcup's lid safety and crash recovery, owned by Shifu's session.
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Clone, Copy, Debug, Serialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum Stop { Hot, LowBattery }

#[derive(Clone, Copy, Serialize)]
pub struct Health { pub thermal: u8, pub on_battery: bool, pub percent: u8 }

impl Health {
    fn stop(self) -> Option<Stop> {
        if self.thermal >= 2 { Some(Stop::Hot) }
        else if self.on_battery && self.percent <= 10 { Some(Stop::LowBattery) }
        else { None }
    }
    fn may_resume(self, stop: Stop) -> bool {
        self.stop().is_none() && match stop {
            Stop::Hot => self.thermal == 0,
            Stop::LowBattery => !self.on_battery,
        }
    }
}

#[derive(Serialize)]
pub struct Observation { pub disabled: bool, pub health: Health }

// Only native effects are replaced in tests; ownership and recovery use real files.
pub trait Platform: Send + Sync {
    fn observe(&self) -> Result<Observation, String>;
    fn switch(&self, disabled: bool) -> Result<(), String>;
    fn recovery(&self, enabled: bool, record: &Path) -> Result<(), String>;
    fn ready(&self) -> bool;
    fn authorize(&self) -> Result<(), String>;
}

#[derive(Clone, Debug, Default, Serialize, PartialEq)]
pub struct Status {
    pub enabled: bool,
    pub active: bool,
    pub ready: bool,
    pub paused: Option<Stop>,
    pub error: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Record { pub pid: u32, pub until: Option<u64>, pub active: bool }

pub struct Lid { pub status: Status, file: PathBuf, pub platform: std::sync::Arc<dyn Platform> }

impl Lid {
    pub fn new(file: PathBuf, platform: std::sync::Arc<dyn Platform>) -> Self {
        let ready = platform.ready();
        Self { status: Status { ready, ..Status::default() }, file, platform }
    }
    pub fn set_enabled(&mut self, enabled: bool, until: Option<u64>) -> Result<(), String> {
        if enabled {
            self.begin(until)?;
        } else {
            self.stop()?;
            self.status.enabled = false;
            self.platform.recovery(false, &self.file)?;
        }
        self.status.enabled = enabled;
        self.status.error = None;
        Ok(())
    }

    pub fn begin(&mut self, until: Option<u64>) -> Result<(), String> {
        self.status.ready = self.platform.ready();
        if !self.status.ready { return Err("合盖运行需要首次授权，请在杯子菜单中授权后开启".into()); }
        let observation = self.platform.observe()?;
        if observation.disabled && !self.status.active {
            return Err("合盖保持运行已由其他程序开启，请先关闭 Steamcup 的合盖模式或原有设置，再重试".into());
        }
        if let Some(stop) = observation.health.stop() {
            self.stop()?;
            self.status.paused = Some(stop);
            return Ok(());
        }
        self.platform.recovery(true, &self.file)?;
        self.write_record(true, until)?;
        // Record ownership before pmset: a crash or an uncertain command result must still recover.
        self.status.active = true;
        self.status.enabled = true;
        self.platform.switch(true)?;
        self.status.paused = None;
        self.status.error = None;
        Ok(())
    }

    pub fn stop(&mut self) -> Result<(), String> {
        if self.status.active {
            self.platform.switch(false).map_err(|e| format!("合盖睡眠尚未恢复：{e}。可在终端运行 sudo pmset -a disablesleep 0"))?;
            self.write_record(false, None)?;
            self.status.active = false;
        }
        self.status.paused = None;
        self.status.error = None;
        Ok(())
    }

    pub fn tick(&mut self, until: Option<u64>) -> Result<(), String> {
        if !self.status.enabled { return Ok(()); }
        let observation = match observe_with_retry(&self.file, &*self.platform) {
            Ok(o) => o,
            Err(e) => {
                diagnostic(&self.file, &serde_json::json!({"at": crate::power::now_millis(), "event": "session_read_failed", "phase": "requested", "owner_pid": std::process::id(), "error": e}));
                self.stop()?;
                return Err(e);
            }
        };
        if self.status.active {
            if let Some(stop) = observation.health.stop() {
                diagnostic(&self.file, &serde_json::json!({"at": crate::power::now_millis(), "event": "session_safety", "reason": stop, "owner_pid": std::process::id(), "observation": observation}));
                self.stop()?;
                self.status.paused = Some(stop);
            } else if !observation.disabled {
                if until.is_some_and(|end| crate::power::now_millis() >= end) { return self.stop(); }
                // UU Remote periodically clears this global setting. Keep our still-active
                // session's choice; stopped/expired sessions and safety pauses must not repair it.
                let mut note = serde_json::json!({"at": crate::power::now_millis(), "event": "external_reset", "phase": "requested", "owner_pid": std::process::id(), "until": until, "observation": observation});
                diagnostic(&self.file, &note);
                if let Err(error) = self.platform.switch(true) {
                    note["phase"] = "failed".into();
                    note["error"] = error.clone().into();
                    diagnostic(&self.file, &note);
                    return Err(format!("合盖设置被其他程序覆盖，未能恢复：{error}"));
                }
                note["phase"] = "applied".into();
                diagnostic(&self.file, &note);
                self.status.error = None;
            }
        } else if self.status.paused.is_some_and(|stop| observation.health.may_resume(stop)) {
            self.begin(until)?;
        }
        Ok(())
    }

    pub fn deadline(&mut self, until: Option<u64>) -> Result<(), String> {
        if self.status.active { self.write_record(true, until)?; }
        Ok(())
    }

    fn write_record(&self, active: bool, until: Option<u64>) -> Result<(), String> {
        let record = Record { pid: std::process::id(), active, until };
        crate::config::write_atomic(&self.file, &serde_json::to_string(&record).map_err(|e| e.to_string())?)
    }

}

fn needs_recovery(record: &Record, alive: bool, health: Health, now: u64) -> bool {
    record.active && (!alive || health.stop().is_some() || record.until.is_some_and(|end| now >= end))
}

pub struct System { home: PathBuf, service: String }

impl System {
    pub fn new(home: &Path) -> Self {
        #[cfg(target_os = "macos")]
        let bundle = objc2_foundation::NSBundle::mainBundle().bundleIdentifier().map(|s| s.to_string());
        #[cfg(not(target_os = "macos"))]
        let bundle: Option<String> = None;
        Self { home: home.to_path_buf(), service: format!("{}.power.recovery", bundle.as_deref().unwrap_or("dev.longado.shifu")) }
    }

    fn disabled() -> Result<bool, String> {
        let out = run("/usr/bin/pmset", &["-g"], true)?;
        Ok(out.lines().any(|line| {
            let mut p = line.split_whitespace();
            p.next() == Some("SleepDisabled") && p.next() == Some("1")
        }))
    }

    fn service_path(&self) -> PathBuf {
        PathBuf::from(std::env::var_os("HOME").unwrap_or_default()).join("Library/LaunchAgents").join(format!("{}.plist", self.service))
    }

    fn service_target(&self) -> String { format!("gui/{}/{}", unsafe { libc::getuid() }, self.service) }
}

impl Platform for System {
    fn observe(&self) -> Result<Observation, String> {
        #[cfg(target_os = "macos")]
        let thermal = objc2_foundation::NSProcessInfo::processInfo().thermalState().0 as u8;
        #[cfg(not(target_os = "macos"))]
        let thermal = 0;
        let battery = run("/usr/bin/pmset", &["-g", "batt"], true)?;
        let on_battery = battery.contains("'Battery Power'");
        let percent = battery.split_whitespace().find_map(|s| s.strip_suffix("%;").and_then(|s| s.parse::<u8>().ok()));
        if on_battery && percent.is_none() { return Err("无法读取电池电量，合盖运行已暂停".into()); }
        Ok(Observation { disabled: Self::disabled()?, health: Health { thermal, on_battery, percent: percent.unwrap_or(100) } })
    }

    fn switch(&self, disabled: bool) -> Result<(), String> {
        run("/usr/bin/sudo", &["-n", "/usr/bin/pmset", "-a", "disablesleep", if disabled { "1" } else { "0" }], true)?;
        if Self::disabled()? != disabled { return Err("系统未应用合盖睡眠设置，请重新读取状态".into()); }
        if !disabled {
            let lid = run("/usr/sbin/ioreg", &["-r", "-n", "IOPMrootDomain", "-d", "1"], true)?;
            let yes = |key: &str| lid.lines().any(|l| l.trim() == format!("\"{key}\" = Yes"));
            // Restoring the global switch alone does not always sleep an already closed laptop.
            if yes("AppleClamshellState") && yes("AppleClamshellCausesSleep") {
                run("/usr/bin/pmset", &["sleepnow"], true)?;
            }
        }
        Ok(())
    }

    fn recovery(&self, enabled: bool, record: &Path) -> Result<(), String> {
        let target = self.service_target();
        let loaded = run("/bin/launchctl", &["print", &target], true).is_ok();
        let path = self.service_path();
        if !enabled {
            if loaded { run("/bin/launchctl", &["bootout", &target], true)?; }
            if path.exists() { std::fs::remove_file(&path).map_err(|e| format!("无法移除合盖恢复任务：{e}"))?; }
            return Ok(());
        }
        let exe = std::env::current_exe().map_err(|e| e.to_string())?;
        let plist = format!("<?xml version=\"1.0\" encoding=\"UTF-8\"?><!DOCTYPE plist PUBLIC \"-//Apple//DTD PLIST 1.0//EN\" \"http://www.apple.com/DTDs/PropertyList-1.0.dtd\"><plist version=\"1.0\"><dict><key>Label</key><string>{}</string><key>ProgramArguments</key><array><string>{}</string><string>--power-watchdog</string><string>{}</string></array><key>StartInterval</key><integer>60</integer><key>RunAtLoad</key><true/><key>ProcessType</key><string>Background</string></dict></plist>", xml(&self.service), xml(&exe.to_string_lossy()), xml(&record.to_string_lossy()));
        if loaded && std::fs::read_to_string(&path).ok().as_deref() == Some(&plist) { return Ok(()); }
        std::fs::create_dir_all(path.parent().unwrap()).map_err(|e| e.to_string())?;
        crate::config::write_atomic(&path, &plist)?;
        if loaded { run("/bin/launchctl", &["bootout", &target], true)?; }
        let domain = format!("gui/{}", unsafe { libc::getuid() });
        run("/bin/launchctl", &["bootstrap", &domain, &path.to_string_lossy()], true)?;
        run("/bin/launchctl", &["print", &target], true).map_err(|_| "合盖恢复任务没有启动，请允许 Shifu 的后台项目后重试".to_string())?;
        Ok(())
    }

    fn ready(&self) -> bool {
        ["1", "0"].iter().all(|value| run("/usr/bin/sudo", &["-n", "-l", "/usr/bin/pmset", "-a", "disablesleep", value], true).is_ok())
    }

    fn authorize(&self) -> Result<(), String> {
        let script = self.home.join("setup-lid-mode.sh");
        std::fs::write(&script, include_str!("setup-lid-mode.sh")).map_err(|e| format!("无法准备合盖授权：{e}"))?;
        let user = run("/usr/bin/id", &["-un"], true)?;
        let cmd = format!("/bin/zsh {} {}", shifu::launch::sh_quote(&script.to_string_lossy()), shifu::launch::sh_quote(user.trim()));
        let literal = cmd.replace('\\', "\\\\").replace('"', "\\\"");
        run("/usr/bin/osascript", &["-e", &format!("do shell script \"{literal}\" with administrator privileges")], false)?;
        Ok(())
    }
}

fn xml(s: &str) -> String { s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;").replace('\'', "&apos;") }

fn run(program: &str, args: &[&str], bounded: bool) -> Result<String, String> {
    use std::io::Read;
    use std::process::{Command, Stdio};
    let mut command = Command::new(program);
    command.args(args).env("LANG", "C").stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped());
    #[cfg(unix)]
    { use std::os::unix::process::CommandExt; command.process_group(0); }
    let mut child = command.spawn().map_err(|e| format!("无法运行 {program}：{e}"))?;
    let mut stdout = child.stdout.take().unwrap();
    let mut stderr = child.stderr.take().unwrap();
    let output = std::thread::spawn(move || { let mut b = Vec::new(); stdout.read_to_end(&mut b).map(|_| b) });
    let errors = std::thread::spawn(move || { let mut b = Vec::new(); stderr.read_to_end(&mut b).map(|_| b) });
    let started = std::time::Instant::now();
    let result = loop {
        match child.try_wait() {
            Ok(Some(status)) => break Ok(status),
            Ok(None) if !bounded || started.elapsed() < std::time::Duration::from_secs(5) => std::thread::sleep(std::time::Duration::from_millis(25)),
            Ok(None) => { unsafe { libc::kill(-(child.id() as i32), libc::SIGKILL); } let _ = child.wait(); break Err(format!("{program} {} 执行超时，请重新读取合盖状态", args.join(" "))); }
            Err(e) => { unsafe { libc::kill(-(child.id() as i32), libc::SIGKILL); } let _ = child.wait(); break Err(e.to_string()); }
        }
    };
    let out = output.join().map_err(|_| "读取系统输出失败")?.map_err(|e| e.to_string())?;
    let err = errors.join().map_err(|_| "读取系统错误失败")?.map_err(|e| e.to_string())?;
    let status = result?;
    if !status.success() {
        let detail = String::from_utf8_lossy(if err.is_empty() { &out } else { &err });
        return Err(format!("{program} 未完成：{}", detail.trim()));
    }
    Ok(String::from_utf8_lossy(&out).into_owned())
}

pub fn watchdog(file: &Path) -> Result<(), String> {
    let system = System::new(file.parent().ok_or("合盖恢复路径不完整")?);
    recover(file, &system)
}

fn diagnostic(file: &Path, value: &serde_json::Value) {
    if let Err(error) = crate::config::write_atomic(&file.with_extension("diagnostic.json"), &value.to_string()) {
        eprintln!("合盖恢复原因未能记录：{error}");
    }
}

fn observe_with_retry(file: &Path, platform: &dyn Platform) -> Result<Observation, String> {
    match platform.observe() {
        Ok(observation) => Ok(observation),
        Err(first) => {
            // An unreadable sample is not evidence of unsafe heat or an expired hold.
            // Recheck once; persistent failures still take the existing safety path.
            match platform.observe() {
                Ok(observation) => {
                    diagnostic(file, &serde_json::json!({"at": crate::power::now_millis(), "event": "read_recovered", "requester_pid": std::process::id(), "first_error": first}));
                    Ok(observation)
                }
                Err(second) => Err(format!("{first}；复核仍失败：{second}")),
            }
        }
    }
}

pub(crate) fn recover(file: &Path, platform: &dyn Platform) -> Result<(), String> {
    let text = match std::fs::read_to_string(file) {
        Ok(s) => s,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(e) => return Err(e.to_string()),
    };
    let mut record: Record = serde_json::from_str(&text).map_err(|e| e.to_string())?;
    if !record.active { return Ok(()); }
    let alive = record.pid > 1 && (unsafe { libc::kill(record.pid as i32, 0) } == 0 || std::io::Error::last_os_error().raw_os_error() == Some(libc::EPERM));
    let observed = observe_with_retry(file, platform);
    let restore = match &observed {
        Ok(o) => needs_recovery(&record, alive, o.health, crate::power::now_millis()),
        Err(_) => true, // A failed safety read must not leave our global hold enabled.
    };
    if restore {
        // Preserve the actual decision before a closed lid can put the machine to sleep.
        let mut note = serde_json::json!({"at": crate::power::now_millis(), "event": "recovery", "phase": "requested", "owner_pid": record.pid, "requester_pid": std::process::id(), "owner_alive": alive, "until": record.until, "observation": observed});
        diagnostic(file, &note);
        if let Err(error) = platform.switch(false) {
            note["phase"] = "failed".into();
            note["error"] = error.clone().into();
            diagnostic(file, &note);
            return Err(error);
        }
        note["phase"] = "applied".into();
        diagnostic(file, &note);
        // Don't rewrite a running app's record while it may be making another switch.
        // A new app may have taken over (and written its own record) while this ran, e.g. right after a reinstall;
        // only retire the record when it still names the dead owner.
        let current = std::fs::read_to_string(file).ok().and_then(|t| serde_json::from_str::<Record>(&t).ok());
        if !alive && current.is_some_and(|c| c.pid == record.pid) {
            record.active = false;
            crate::config::write_atomic(file, &serde_json::to_string(&record).map_err(|e| e.to_string())?)?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};
    const SAFE: Health = Health { thermal: 0, on_battery: false, percent: 80 };

    struct Fake { state: Arc<Mutex<FakeState>> }
    struct FakeState { disabled: bool, health: Health, switches: Vec<bool>, fail_restore: bool, read_failures: u8 }
    impl Platform for Fake {
        fn observe(&self) -> Result<Observation, String> {
            let mut s = self.state.lock().unwrap();
            if s.read_failures > 0 {
                s.read_failures -= 1;
                return Err("电源状态读取暂时失败".into());
            }
            Ok(Observation { disabled: s.disabled, health: s.health })
        }
        fn switch(&self, disabled: bool) -> Result<(), String> {
            let mut s = self.state.lock().unwrap();
            s.switches.push(disabled);
            if !disabled && s.fail_restore { return Err("系统拒绝恢复睡眠".into()); }
            s.disabled = disabled;
            Ok(())
        }
        fn recovery(&self, _enabled: bool, _record: &Path) -> Result<(), String> { Ok(()) }
        fn ready(&self) -> bool { true }
        fn authorize(&self) -> Result<(), String> { panic!("tests must never request admin permission") }
    }
    fn fixture(name: &str) -> (Lid, Arc<Mutex<FakeState>>) {
        let home = std::env::temp_dir().join(format!("shifu-lid-{}-{name}", std::process::id()));
        std::fs::create_dir_all(&home).unwrap();
        let state = Arc::new(Mutex::new(FakeState { disabled: false, health: SAFE, switches: vec![], fail_restore: false, read_failures: 0 }));
        (Lid::new(home.join("lid.json"), Arc::new(Fake { state: state.clone() })), state)
    }

    #[test]
    fn owns_only_its_switch_and_preserves_failed_recovery() {
        let (mut lid, state) = fixture("ownership");
        state.lock().unwrap().disabled = true;
        assert!(lid.set_enabled(true, None).is_err());
        assert!(state.lock().unwrap().switches.is_empty());
        state.lock().unwrap().disabled = false;
        lid.set_enabled(true, Some(500)).unwrap();
        assert!(lid.status.active);
        let record: Record = serde_json::from_str(&std::fs::read_to_string(&lid.file).unwrap()).unwrap();
        assert!(record.active);
        assert_eq!(record.until, Some(500));
        lid.deadline(Some(800)).unwrap();
        let record: Record = serde_json::from_str(&std::fs::read_to_string(&lid.file).unwrap()).unwrap();
        assert_eq!(record.until, Some(800));
        state.lock().unwrap().fail_restore = true;
        assert!(lid.set_enabled(false, None).is_err());
        assert!(lid.status.active);
        state.lock().unwrap().fail_restore = false;
        lid.set_enabled(false, None).unwrap();
        assert!(!lid.status.active);
        assert!(!lid.status.enabled);
        assert!(!state.lock().unwrap().disabled);
    }

    #[test]
    fn pauses_for_heat_and_low_battery_then_resumes_after_recovery() {
        let (mut lid, state) = fixture("safety");
        lid.set_enabled(true, None).unwrap();
        state.lock().unwrap().health = Health { on_battery: true, percent: 10, ..SAFE };
        lid.tick(None).unwrap();
        assert_eq!(lid.status.paused, Some(Stop::LowBattery));
        assert!(!state.lock().unwrap().disabled);
        state.lock().unwrap().health.percent = 11;
        lid.tick(None).unwrap();
        assert!(!lid.status.active);
        state.lock().unwrap().health.on_battery = false;
        lid.tick(None).unwrap();
        assert!(lid.status.active);
        state.lock().unwrap().health.thermal = 2;
        lid.tick(None).unwrap();
        assert_eq!(lid.status.paused, Some(Stop::Hot));
        state.lock().unwrap().health.thermal = 1;
        lid.tick(None).unwrap();
        assert!(!lid.status.active);
        state.lock().unwrap().health.thermal = 0;
        lid.tick(None).unwrap();
        assert!(lid.status.active);
        lid.set_enabled(false, None).unwrap();
    }

    #[test]
    fn a_single_watchdog_read_failure_does_not_restore_a_live_hold() {
        let (mut lid, state) = fixture("watchdog-transient-read");
        lid.set_enabled(true, None).unwrap();
        state.lock().unwrap().read_failures = 1;
        recover(&lid.file, &*lid.platform).unwrap();
        assert!(state.lock().unwrap().disabled);
        assert_eq!(state.lock().unwrap().switches, vec![true]);
        lid.set_enabled(false, None).unwrap();
    }

    #[test]
    fn a_single_session_read_failure_does_not_stop_a_live_hold() {
        let (mut lid, state) = fixture("session-transient-read");
        lid.set_enabled(true, None).unwrap();
        state.lock().unwrap().read_failures = 1;
        lid.tick(None).unwrap();
        assert!(lid.status.active);
        assert!(state.lock().unwrap().disabled);
        assert_eq!(state.lock().unwrap().switches, vec![true]);
        lid.set_enabled(false, None).unwrap();
    }

    #[test]
    fn persistent_read_failure_still_restores_sleep_and_records_the_actual_reason() {
        let (mut lid, state) = fixture("watchdog-persistent-read");
        lid.set_enabled(true, None).unwrap();
        state.lock().unwrap().read_failures = 2;
        recover(&lid.file, &*lid.platform).unwrap();
        assert!(!state.lock().unwrap().disabled);
        assert_eq!(state.lock().unwrap().switches, vec![true, false]);
        let note: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(lid.file.with_extension("diagnostic.json")).unwrap()).unwrap();
        assert_eq!(note["phase"], "applied");
        assert_eq!(note["owner_alive"], true);
        assert_eq!(note["owner_pid"], std::process::id());
        assert!(note["observation"]["Err"].as_str().unwrap().contains("复核仍失败"));
        lid.set_enabled(false, None).unwrap();
    }

    #[test]
    fn a_recovered_read_still_repairs_an_owned_hold_reset_elsewhere() {
        let (mut lid, state) = fixture("external-stop-after-read-failure");
        lid.set_enabled(true, None).unwrap();
        {
            let mut s = state.lock().unwrap();
            s.disabled = false;
            s.read_failures = 1;
        }
        lid.tick(None).unwrap();
        assert!(lid.status.active);
        assert!(lid.status.error.is_none());
        assert_eq!(state.lock().unwrap().switches, vec![true, true]);
        lid.set_enabled(false, None).unwrap();
    }

    #[test]
    fn an_owned_active_hold_survives_repeated_external_power_resets() {
        let (mut lid, state) = fixture("external-power-resets");
        let deadline = crate::power::now_millis() + 60_000;
        lid.set_enabled(true, Some(deadline)).unwrap();
        for _ in 0..3 {
            state.lock().unwrap().disabled = false;
            lid.tick(Some(deadline)).unwrap();
            assert!(lid.status.active);
            assert!(state.lock().unwrap().disabled);
            let record: Record = serde_json::from_str(&std::fs::read_to_string(&lid.file).unwrap()).unwrap();
            assert!(record.active);
            assert_eq!(record.until, Some(deadline));
        }
        let note: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(lid.file.with_extension("diagnostic.json")).unwrap()).unwrap();
        assert_eq!(note["event"], "external_reset");
        assert_eq!(note["phase"], "applied");
        lid.set_enabled(false, None).unwrap();
    }

    #[test]
    fn repair_does_not_restart_a_stopped_hold_or_override_heat_and_low_battery() {
        let (mut lid, state) = fixture("repair-boundaries");
        lid.set_enabled(true, None).unwrap();
        lid.stop().unwrap();
        lid.tick(None).unwrap();
        assert!(!state.lock().unwrap().disabled);
        assert_eq!(state.lock().unwrap().switches, vec![true, false]);
        for health in [Health { thermal: 2, ..SAFE }, Health { on_battery: true, percent: 10, ..SAFE }] {
            state.lock().unwrap().health = SAFE;
            lid.set_enabled(true, None).unwrap();
            {
                let mut s = state.lock().unwrap();
                s.disabled = false;
                s.health = health;
            }
            lid.tick(None).unwrap();
            assert!(!lid.status.active);
            assert!(!state.lock().unwrap().disabled);
            assert!(lid.status.paused.is_some());
        }
        lid.set_enabled(false, None).unwrap();
        state.lock().unwrap().health = SAFE;
        lid.tick(None).unwrap();
        assert!(!state.lock().unwrap().disabled);
    }

    /// A new app takes over while the watchdog is restoring the dead owner's hold.
    struct TakeoverDuringRestore { file: PathBuf }
    impl Platform for TakeoverDuringRestore {
        fn observe(&self) -> Result<Observation, String> { Ok(Observation { disabled: true, health: SAFE }) }
        fn switch(&self, _disabled: bool) -> Result<(), String> {
            std::fs::write(&self.file, r#"{"pid":4242,"until":null,"active":true}"#).map_err(|e| e.to_string())
        }
        fn recovery(&self, _enabled: bool, _record: &Path) -> Result<(), String> { Ok(()) }
        fn ready(&self) -> bool { true }
        fn authorize(&self) -> Result<(), String> { panic!("tests must never request admin permission") }
    }

    #[test]
    fn recovery_keeps_the_record_of_an_app_that_took_over_meanwhile() {
        let home = std::env::temp_dir().join(format!("shifu-lid-{}-takeover", std::process::id()));
        std::fs::create_dir_all(&home).unwrap();
        let file = home.join("lid.json");
        // pid 0x7ffffffe is not a running process.
        std::fs::write(&file, r#"{"pid":2147483646,"until":null,"active":true}"#).unwrap();
        recover(&file, &TakeoverDuringRestore { file: file.clone() }).unwrap();
        let record: Record = serde_json::from_str(&std::fs::read_to_string(&file).unwrap()).unwrap();
        assert_eq!((record.pid, record.active), (4242, true));
        std::fs::remove_dir_all(&home).unwrap();
    }

    #[test]
    fn recovery_handles_crash_freeze_and_timer_but_leaves_other_holds_alone() {
        let mut r = Record { pid: 999, active: true, until: Some(500) };
        assert!(!needs_recovery(&r, true, SAFE, 499));
        assert!(needs_recovery(&r, true, SAFE, 500));
        assert!(needs_recovery(&r, false, SAFE, 100));
        assert!(needs_recovery(&r, true, Health { thermal: 2, ..SAFE }, 100));
        assert!(needs_recovery(&r, true, Health { on_battery: true, percent: 10, ..SAFE }, 100));
        r.active = false;
        assert!(!needs_recovery(&r, false, Health { thermal: 3, ..SAFE }, 999));
    }

    #[test]
    fn watchdog_restores_an_expired_live_session_and_retains_failed_recovery() {
        let (mut lid, state) = fixture("watchdog");
        lid.set_enabled(true, Some(1)).unwrap();
        state.lock().unwrap().fail_restore = true;
        assert!(recover(&lid.file, &*lid.platform).is_err());
        let record: Record = serde_json::from_str(&std::fs::read_to_string(&lid.file).unwrap()).unwrap();
        assert!(record.active);
        state.lock().unwrap().fail_restore = false;
        recover(&lid.file, &*lid.platform).unwrap();
        assert!(!state.lock().unwrap().disabled);
        lid.tick(Some(1)).unwrap();
        assert!(!lid.status.active);
        let switches = state.lock().unwrap().switches.len();
        // A restored/inactive record cannot switch someone else's new hold back off.
        state.lock().unwrap().disabled = true;
        recover(&lid.file, &*lid.platform).unwrap();
        assert!(state.lock().unwrap().disabled);
        assert_eq!(state.lock().unwrap().switches.len(), switches);
        std::fs::write(&lid.file, "invalid json").unwrap();
        assert!(recover(&lid.file, &*lid.platform).is_err());
        assert_eq!(state.lock().unwrap().switches.len(), switches);
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn native_status_reads_real_thermal_battery_and_sleep_without_changing_them() {
        let system = System::new(&std::env::temp_dir());
        let before = System::disabled().unwrap();
        let o = system.observe().unwrap();
        eprintln!("native power observation: {}", serde_json::json!({"disabled": o.disabled, "thermal": o.health.thermal, "on_battery": o.health.on_battery, "percent": o.health.percent}));
        assert!(o.health.thermal <= 3);
        assert!(o.health.percent <= 100);
        assert_eq!(o.disabled, before);
        assert_eq!(System::disabled().unwrap(), before);
    }

}
