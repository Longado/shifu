//! Steamcup controls inside Shifu. Idle assertions belong to this process;
//! lid holds additionally have an independent crash/heat/battery recovery job.
use serde::{Deserialize, Serialize};
use std::sync::Mutex;
use crate::lid::{self, Lid};

#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum Mode { #[default] Off, System, Display }

#[derive(Clone, Debug, Serialize, PartialEq)]
pub struct Status {
    pub mode: Mode,
    pub until: Option<u64>,
    pub minutes: Option<u32>,
    pub lid: lid::Status,
}

struct Session {
    hold: Option<(Mode, Assertion)>,
    until: Option<u64>,
    minutes: Option<u32>,
    lid: Lid,
    last_health_check: u64,
}

impl Session {
    fn status(&self) -> Status {
        Status { mode: self.hold.as_ref().map_or(Mode::Off, |(m, _)| *m), until: self.until, minutes: self.minutes, lid: self.lid.status.clone() }
    }

    fn set(&mut self, mode: Mode) -> Result<(), String> {
        if mode == Mode::Off {
            self.lid.stop()?;
        }
        if self.status().mode == mode {
            if mode == Mode::Off { self.until = None; self.minutes = None; }
            return Ok(());
        }
        // Acquire first: a failed change must leave the existing hold intact.
        let next = if mode == Mode::Off { None } else { Some((mode, Assertion::create(mode)?)) };
        if mode != Mode::Off && self.hold.is_none() && self.lid.status.enabled { self.lid.begin(self.until)?; }
        if let Some((_, old)) = self.hold.as_ref() { old.release()?; }
        if let Some((_, old)) = self.hold.as_mut() { old.0 = None; }
        self.hold = next;
        if mode == Mode::Off { self.until = None; self.minutes = None; }
        Ok(())
    }
}

pub struct Power(Mutex<Session>);

impl Power {
    pub fn new(home: &std::path::Path) -> Self {
        Self(Mutex::new(Session {
            hold: None, until: None, minutes: None, last_health_check: 0,
            lid: Lid::new(home.join("power-lid.json"), std::sync::Arc::new(lid::System::new(home))),
        }))
    }

    pub fn get(&self) -> Result<Status, String> {
        self.0.lock().map(|s| s.status()).map_err(|e| e.to_string())
    }

    pub fn set(&self, mode: Mode) -> Result<Status, String> {
        let mut s = self.0.lock().map_err(|e| e.to_string())?;
        if let Err(e) = s.set(mode) { s.lid.status.error = Some(e.clone()); return Err(e); }
        Ok(s.status())
    }

    pub fn timer(&self, minutes: Option<u32>) -> Result<Status, String> {
        if minutes.is_some_and(|m| !(1..=10_080).contains(&m)) { return Err("定时时长应为 1 到 10080 分钟".into()); }
        let mut s = self.0.lock().map_err(|e| e.to_string())?;
        let until = minutes.map(|m| now_millis() + u64::from(m) * 60_000);
        if s.hold.is_none() { s.set(Mode::System)?; }
        s.lid.deadline(until)?;
        s.until = until;
        s.minutes = minutes;
        Ok(s.status())
    }

    pub fn set_lid(&self, enabled: bool) -> Result<Status, String> {
        let mut s = self.0.lock().map_err(|e| e.to_string())?;
        let until = s.until;
        if let Err(e) = s.lid.set_enabled(enabled, until) { s.lid.status.error = Some(e.clone()); return Err(e); }
        if enabled && s.hold.is_none() {
            // begin() has already acquired the lid hold; don't acquire it twice.
            match Assertion::create(Mode::System) {
                Ok(a) => s.hold = Some((Mode::System, a)),
                Err(e) => { s.lid.set_enabled(false, None)?; return Err(e); }
            }
        }
        Ok(s.status())
    }

    pub fn authorize_lid(&self) -> Result<Status, String> {
        // The administrator prompt can stay open while an existing timer still expires.
        let platform = self.0.lock().map_err(|e| e.to_string())?.lid.platform.clone();
        platform.authorize()?;
        let ready = platform.ready();
        let mut s = self.0.lock().map_err(|e| e.to_string())?;
        s.lid.status.ready = ready;
        if !ready { return Err("合盖运行授权未生效".into()); }
        Ok(s.status())
    }

    pub fn expire(&self, now: u64) -> Result<bool, String> {
        let mut s = self.0.lock().map_err(|e| e.to_string())?;
        if s.until.is_some_and(|end| now >= end) { s.set(Mode::Off)?; return Ok(true); }
        Ok(false)
    }

    fn tick(&self) -> Result<Option<Status>, String> {
        let now = now_millis();
        let expired = self.expire(now)?;
        let mut s = self.0.lock().map_err(|e| e.to_string())?;
        let before = s.status();
        if before.mode != Mode::Off && now.saturating_sub(s.last_health_check) >= 15_000 {
            s.last_health_check = now;
            if let Err(e) = s.lid.tick(before.until) { s.lid.status.error = Some(e); }
        }
        let after = s.status();
        Ok((expired || before != after).then_some(after))
    }
}

pub fn now_millis() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_millis() as u64
}

pub fn spawn_watcher(app: tauri::AppHandle) {
    use tauri::{Emitter, Manager};
    std::thread::spawn(move || loop {
        let delay = match app.state::<Power>().tick() {
            Ok(Some(status)) => { if let Err(e) = app.emit("power-changed", status) { eprintln!("Steamcup: {e}"); } 1 }
            Ok(None) => 1,
            Err(e) => {
                eprintln!("Steamcup: {e}");
                if let Ok(mut status) = app.state::<Power>().get() {
                    status.lid.error = Some(e);
                    if let Err(e) = app.emit("power-changed", status) { eprintln!("Steamcup: {e}"); }
                }
                15
            }
        };
        std::thread::sleep(std::time::Duration::from_secs(delay));
    });
}

impl Drop for Power {
    fn drop(&mut self) {
        if let Ok(s) = self.0.get_mut()
            && let Err(e) = s.lid.stop() {
            eprintln!("Steamcup: {e}; 后台恢复任务将继续恢复合盖睡眠");
        }
    }
}

struct Assertion(Option<u32>);

#[cfg(target_os = "macos")]
#[link(name = "IOKit", kind = "framework")]
unsafe extern "C" {
    fn IOPMAssertionCreateWithName(kind: *const std::ffi::c_void, level: u32, name: *const std::ffi::c_void, id: *mut u32) -> i32;
    fn IOPMAssertionRelease(id: u32) -> i32;
}

#[cfg(target_os = "macos")]
#[link(name = "CoreFoundation", kind = "framework")]
unsafe extern "C" {
    fn CFStringCreateWithCString(allocator: *const std::ffi::c_void, text: *const std::ffi::c_char, encoding: u32) -> *const std::ffi::c_void;
    fn CFRelease(value: *const std::ffi::c_void);
}

impl Assertion {
    #[cfg(target_os = "macos")]
    fn create(mode: Mode) -> Result<Self, String> {
        let kind = match mode {
            Mode::System => c"PreventUserIdleSystemSleep",
            Mode::Display => c"PreventUserIdleDisplaySleep",
            Mode::Off => return Err("正常睡眠不需要保持唤醒".into()),
        };
        let name = std::ffi::CString::new(format!("Shifu keep awake {}", std::process::id())).unwrap();
        // Both CF strings are owned here and released after IOKit copies them.
        unsafe {
            let k = CFStringCreateWithCString(std::ptr::null(), kind.as_ptr(), 0x08000100);
            let n = CFStringCreateWithCString(std::ptr::null(), name.as_ptr(), 0x08000100);
            if k.is_null() || n.is_null() {
                if !k.is_null() { CFRelease(k); }
                if !n.is_null() { CFRelease(n); }
                return Err("无法创建保持唤醒设置".into());
            }
            let mut id = 0;
            let result = IOPMAssertionCreateWithName(k, 255, n, &mut id);
            CFRelease(k);
            CFRelease(n);
            if result != 0 { return Err(format!("无法保持唤醒（系统错误 {result}）")); }
            Ok(Self(Some(id)))
        }
    }

    #[cfg(not(target_os = "macos"))]
    fn create(_mode: Mode) -> Result<Self, String> {
        Err("保持唤醒目前只支持 macOS".into())
    }

    fn release(&self) -> Result<(), String> {
        #[cfg(target_os = "macos")]
        if let Some(id) = self.0 {
            let result = unsafe { IOPMAssertionRelease(id) };
            if result != 0 { return Err(format!("无法恢复正常睡眠（系统错误 {result}）")); }
        }
        Ok(())
    }
}

impl Drop for Assertion {
    fn drop(&mut self) {
        if let Err(e) = self.release() { eprintln!("{e}"); }
    }
}

#[cfg(all(test, target_os = "macos"))]
mod tests {
    use super::*;

    static ASSERTION_LOCK: Mutex<()> = Mutex::new(());

    fn assertions() -> String {
        let out = std::process::Command::new("/usr/bin/pmset").args(["-g", "assertions"]).output().unwrap();
        assert!(out.status.success());
        String::from_utf8(out.stdout).unwrap()
    }

    #[test]
    fn real_assertion_follows_mode_and_is_released_on_drop() {
        let _guard = ASSERTION_LOCK.lock().unwrap();
        let name = format!("Shifu keep awake {}", std::process::id());
        let power = Power::new(&std::env::temp_dir().join("shifu-power-timer-test"));
        assert_eq!(power.get().unwrap().mode, Mode::Off);
        assert_eq!(power.set(Mode::System).unwrap().mode, Mode::System);
        assert!(assertions().lines().any(|l| l.contains(&name) && l.contains("PreventUserIdleSystemSleep")));
        assert_eq!(power.set(Mode::System).unwrap().mode, Mode::System);
        assert_eq!(assertions().lines().filter(|l| l.contains(&name)).count(), 1);
        assert_eq!(power.set(Mode::Display).unwrap().mode, Mode::Display);
        let a = assertions();
        assert!(a.lines().any(|l| l.contains(&name) && l.contains("PreventUserIdleDisplaySleep")));
        assert_eq!(a.lines().filter(|l| l.contains(&name)).count(), 1);
        assert_eq!(power.set(Mode::Off).unwrap().mode, Mode::Off);
        assert!(!assertions().contains(&name));
        power.set(Mode::System).unwrap();
        drop(power);
        assert!(!assertions().contains(&name));
    }

    #[test]
    fn timed_hold_expires_and_mode_changes_keep_its_deadline() {
        let _guard = ASSERTION_LOCK.lock().unwrap();
        let power = Power::new(&std::env::temp_dir().join("shifu-power-timer-test"));
        let timed = power.timer(Some(30)).unwrap();
        assert_eq!(timed.mode, Mode::System);
        assert_eq!(timed.minutes, Some(30));
        let deadline = timed.until.unwrap();
        assert!(deadline > now_millis());
        let display = power.set(Mode::Display).unwrap();
        assert_eq!(display.until, Some(deadline));
        assert!(!power.expire(deadline - 1).unwrap());
        assert_eq!(power.get().unwrap().mode, Mode::Display);
        assert!(power.expire(deadline).unwrap());
        let stopped = power.get().unwrap();
        assert_eq!(stopped.mode, Mode::Off);
        assert_eq!(stopped.until, None);
        assert_eq!(stopped.minutes, None);

        power.timer(Some(1)).unwrap();
        let indefinite = power.timer(None).unwrap();
        assert_eq!(indefinite.mode, Mode::System);
        assert_eq!(indefinite.until, None);
        assert_eq!(indefinite.minutes, None);
        power.timer(Some(1)).unwrap();
        assert_eq!(power.set(Mode::Off).unwrap().until, None);
        assert!(!power.expire(u64::MAX).unwrap());
        assert!(power.timer(Some(0)).is_err());
        assert!(power.timer(Some(10_081)).is_err());
        assert_eq!(power.get().unwrap().mode, Mode::Off);
    }

    #[test]
    fn invalid_mode_is_rejected_at_command_boundary() {
        assert!(serde_json::from_str::<Mode>("\"lid\"").is_err());
    }
}
