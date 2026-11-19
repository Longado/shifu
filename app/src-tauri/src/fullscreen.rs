//! In macOS full screen the window's content runs to the top of the screen, and the menu bar plus the title bar
//! strip (it holds the traffic lights) are drawn over it. With the overlay title bar that strip covers the sidebar
//! head and the tab bars. AppKit hosts the strip in a separate window and slides it in and out with the menu bar,
//! so its frame says how much of the top is covered right now; the page pads its top by that much.

use serde::Serialize;
use std::sync::Mutex;
use tauri::{AppHandle, Emitter, Manager};

#[derive(Clone, Copy, Default, PartialEq, Serialize)]
pub struct Inset {
    pub fullscreen: bool,
    /// Points covered at the top of the window; 0 when the strip is hidden or the window is not full screen.
    pub top: f64,
}

static LAST: Mutex<Inset> = Mutex::new(Inset { fullscreen: false, top: 0.0 });

#[tauri::command]
pub fn fullscreen_inset() -> Inset {
    *LAST.lock().unwrap()
}

/// Polls the main window a few times a second (the strip has no notification of its own) and emits
/// `fullscreen-inset` when the covered height changes.
pub fn spawn_watcher(app: AppHandle) {
    std::thread::spawn(move || loop {
        std::thread::sleep(std::time::Duration::from_millis(150));
        let Some(window) = app.get_webview_window("main") else { continue };
        let handle = app.clone();
        let w = window.clone();
        let _ = window.run_on_main_thread(move || {
            let fullscreen = w.is_fullscreen().unwrap_or(false);
            let inset = Inset { fullscreen, top: if fullscreen { covered_top(&w) } else { 0.0 } };
            let mut last = LAST.lock().unwrap();
            if *last != inset {
                *last = inset;
                if let Err(e) = handle.emit("fullscreen-inset", inset) {
                    eprintln!("shifu-app: emit fullscreen-inset failed: {e}");
                }
            }
        });
    });
}

#[cfg(target_os = "macos")]
fn covered_top(window: &tauri::WebviewWindow) -> f64 {
    use objc2_app_kit::{NSWindow, NSWindowButton};
    let Ok(ptr) = window.ns_window() else { return 0.0 };
    // SAFETY: Tauri hands out the live NSWindow of this webview window, and this runs on the main thread.
    let ns: &NSWindow = unsafe { &*(ptr as *const NSWindow) };
    let Some(screen) = ns.screen() else { return 0.0 };
    // The close button sits in the title bar view; in full screen that view lives in AppKit's own strip window.
    let Some(strip) = ns
        .standardWindowButton(NSWindowButton::CloseButton)
        .and_then(|button| button.window())
        .filter(|strip| !std::ptr::eq(&**strip, ns))
    else { return 0.0 };
    let screen_top = screen.frame().origin.y + screen.frame().size.height;
    let covered = screen_top - strip.frame().origin.y;
    // A hidden strip parks above the screen; never pad more than a generous menu bar plus title bar.
    covered.clamp(0.0, 120.0).round()
}

#[cfg(not(target_os = "macos"))]
fn covered_top(_window: &tauri::WebviewWindow) -> f64 {
    0.0
}
