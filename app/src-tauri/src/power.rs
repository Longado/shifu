//! Session-only idle-sleep control, using the same IOKit assertions as Steamcup.
//! No lid-sleep override, helper process, or persistent system setting.

use serde::{Deserialize, Serialize};
use std::sync::Mutex;

#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum Mode {
    #[default]
    Off,
    System,
    Display,
}

#[derive(Default)]
pub struct Power(Mutex<Option<(Mode, Assertion)>>);

impl Power {
    pub fn get(&self) -> Result<Mode, String> {
        self.0.lock().map(|s| s.as_ref().map_or(Mode::Off, |(m, _)| *m)).map_err(|e| e.to_string())
    }

    pub fn set(&self, mode: Mode) -> Result<Mode, String> {
        let mut state = self.0.lock().map_err(|e| e.to_string())?;
        if state.as_ref().map_or(Mode::Off, |(m, _)| *m) == mode {
            return Ok(mode);
        }
        // Acquire first: a failed change must leave the existing hold intact.
        let next = match mode {
            Mode::Off => None,
            _ => Some((mode, Assertion::create(mode)?)),
        };
        if let Some((_, old)) = state.as_ref() {
            old.release()?;
        }
        if let Some((_, old)) = state.as_mut() {
            old.0 = None;
        }
        *state = next;
        Ok(mode)
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

    fn assertions() -> String {
        let out = std::process::Command::new("/usr/bin/pmset").args(["-g", "assertions"]).output().unwrap();
        assert!(out.status.success());
        String::from_utf8(out.stdout).unwrap()
    }

    #[test]
    fn real_assertion_follows_mode_and_is_released_on_drop() {
        let name = format!("Shifu keep awake {}", std::process::id());
        let power = Power::default();
        assert_eq!(power.get().unwrap(), Mode::Off);
        assert_eq!(power.set(Mode::System).unwrap(), Mode::System);
        assert!(assertions().lines().any(|l| l.contains(&name) && l.contains("PreventUserIdleSystemSleep")));
        assert_eq!(power.set(Mode::System).unwrap(), Mode::System);
        assert_eq!(assertions().lines().filter(|l| l.contains(&name)).count(), 1);
        assert_eq!(power.set(Mode::Display).unwrap(), Mode::Display);
        let a = assertions();
        assert!(a.lines().any(|l| l.contains(&name) && l.contains("PreventUserIdleDisplaySleep")));
        assert_eq!(a.lines().filter(|l| l.contains(&name)).count(), 1);
        assert_eq!(power.set(Mode::Off).unwrap(), Mode::Off);
        assert!(!assertions().contains(&name));
        power.set(Mode::System).unwrap();
        drop(power);
        assert!(!assertions().contains(&name));
    }

    #[test]
    fn invalid_mode_is_rejected_at_command_boundary() {
        assert!(serde_json::from_str::<Mode>("\"lid\"").is_err());
    }
}
