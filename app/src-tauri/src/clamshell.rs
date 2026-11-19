//! With 合盖继续运行 on, closing the lid on battery keeps the Mac awake, but macOS still turns every screen off:
//! closed-display mode only exists on AC power. When an external screen is attached, this wakes it again by
//! declaring user activity, so the user can keep working on that screen with the lid closed.
//!
//! Only the first `WINDOW` after the lid closes is watched, so a screen that later sleeps because nobody is using
//! it is left asleep.

const WINDOW_MS: u64 = 30_000;
const RETRY_MS: u64 = 3_000;

#[derive(Default)]
pub struct Waker {
    closed_at: Option<u64>,
    last_nudge: Option<u64>,
}

/// What the screens look like right now.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Screens {
    pub lid_closed: bool,
    pub external: bool,
    pub external_asleep: bool,
}

impl Waker {
    /// Returns true when the external screen should be woken now.
    pub fn step(&mut self, now: u64, screens: Screens) -> bool {
        if !screens.lid_closed {
            self.closed_at = None;
            return false;
        }
        let closed_at = *self.closed_at.get_or_insert(now);
        let due = screens.external
            && screens.external_asleep
            && now.saturating_sub(closed_at) <= WINDOW_MS
            && self.last_nudge.is_none_or(|last| now.saturating_sub(last) >= RETRY_MS);
        if due {
            self.last_nudge = Some(now);
        }
        due
    }

    /// Called once a second while 合盖继续运行 is active.
    pub fn tick(&mut self, now: u64) {
        let Some(screens) = read() else { return };
        if !self.step(now, screens) { return; }
        let result = wake();
        // One line per attempt, so a lid test can be checked afterwards.
        let line = format!("{now} wake external screen: {}\n", result.as_ref().map_or_else(|e| format!("failed: {e}"), |_| "requested".into()));
        let path = shifu::agent::shifu_home().join("clamshell.log");
        if let Err(e) = std::fs::OpenOptions::new().create(true).append(true).open(&path).and_then(|mut f| std::io::Write::write_all(&mut f, line.as_bytes())) {
            eprintln!("Steamcup: write {} failed: {e}", path.display());
        }
    }

    pub fn reset(&mut self) {
        self.closed_at = None;
    }
}

#[cfg(target_os = "macos")]
#[link(name = "CoreGraphics", kind = "framework")]
unsafe extern "C" {
    fn CGGetOnlineDisplayList(max: u32, displays: *mut u32, count: *mut u32) -> i32;
    fn CGDisplayIsBuiltin(display: u32) -> u32;
    fn CGDisplayIsAsleep(display: u32) -> u32;
}

#[cfg(target_os = "macos")]
#[link(name = "IOKit", kind = "framework")]
unsafe extern "C" {
    fn IOPMAssertionDeclareUserActivity(name: *const std::ffi::c_void, kind: u32, id: *mut u32) -> i32;
    fn IOPMAssertionRelease(id: u32) -> i32;
}

#[cfg(target_os = "macos")]
#[link(name = "CoreFoundation", kind = "framework")]
unsafe extern "C" {
    fn CFStringCreateWithCString(allocator: *const std::ffi::c_void, text: *const std::ffi::c_char, encoding: u32) -> *const std::ffi::c_void;
    fn CFRelease(value: *const std::ffi::c_void);
}

#[cfg(target_os = "macos")]
fn lid_closed() -> Option<bool> {
    let out = std::process::Command::new("/usr/sbin/ioreg").args(["-r", "-n", "IOPMrootDomain", "-d", "1"]).output().ok()?;
    let text = String::from_utf8_lossy(&out.stdout);
    Some(text.lines().any(|l| l.trim() == "\"AppleClamshellState\" = Yes"))
}

#[cfg(target_os = "macos")]
fn read() -> Option<Screens> {
    let lid_closed = lid_closed()?;
    if !lid_closed {
        return Some(Screens { lid_closed, external: false, external_asleep: false });
    }
    let mut ids = [0u32; 16];
    let mut count = 0u32;
    if unsafe { CGGetOnlineDisplayList(ids.len() as u32, ids.as_mut_ptr(), &mut count) } != 0 {
        return None;
    }
    let external: Vec<u32> = ids[..count as usize].iter().copied().filter(|&d| unsafe { CGDisplayIsBuiltin(d) } == 0).collect();
    Some(Screens {
        lid_closed,
        external: !external.is_empty(),
        external_asleep: external.iter().any(|&d| unsafe { CGDisplayIsAsleep(d) } != 0),
    })
}

#[cfg(not(target_os = "macos"))]
fn read() -> Option<Screens> {
    None
}

/// The same thing a key press does to a sleeping screen.
#[cfg(target_os = "macos")]
fn wake() -> Result<(), String> {
    unsafe {
        let name = CFStringCreateWithCString(std::ptr::null(), c"Shifu wakes the external screen with the lid closed".as_ptr(), 0x08000100);
        if name.is_null() {
            return Err("无法创建唤醒请求".into());
        }
        let mut id = 0;
        let result = IOPMAssertionDeclareUserActivity(name, 0, &mut id);
        CFRelease(name);
        if result != 0 {
            return Err(format!("系统错误 {result}"));
        }
        IOPMAssertionRelease(id);
    }
    Ok(())
}

#[cfg(not(target_os = "macos"))]
fn wake() -> Result<(), String> {
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    const CLOSED_DARK: Screens = Screens { lid_closed: true, external: true, external_asleep: true };

    #[test]
    fn wakes_a_dark_external_screen_soon_after_the_lid_closes_and_retries_sparingly() {
        let mut w = Waker::default();
        assert!(w.step(1_000, CLOSED_DARK));
        assert!(!w.step(2_000, CLOSED_DARK));
        assert!(w.step(4_000, CLOSED_DARK));
        assert!(!w.step(1_000 + WINDOW_MS + 4_000, CLOSED_DARK));
    }

    #[test]
    fn leaves_lit_screens_laptops_without_one_and_open_lids_alone() {
        let mut w = Waker::default();
        assert!(!w.step(0, Screens { external_asleep: false, ..CLOSED_DARK }));
        assert!(!w.step(5_000, Screens { external: false, ..CLOSED_DARK }));
        assert!(!w.step(10_000, Screens { lid_closed: false, ..CLOSED_DARK }));
        // Closing again starts a new window.
        assert!(w.step(100_000, CLOSED_DARK));
    }
}
