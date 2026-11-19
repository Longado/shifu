//! In macOS full screen the window's content runs to the top of the screen, and the menu bar plus the title bar
//! strip (it holds the traffic lights) are drawn over it. With the overlay title bar that strip covers the sidebar
//! head and the tab bars. AppKit hosts the strip in a separate window and slides it in and out with the menu bar,
//! so its frame says how much of the top is covered right now; the page pads its top by that much.
//!
//! tao re-applies `trafficLightPosition` after every title change by moving the title bar view to
//! `window height - inset`. In full screen that view sits in the 32pt strip, so the move pushes it (and the close,
//! minimize and full-screen buttons) out of sight; the watcher puts it back while the window is full screen.

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
            if fullscreen {
                restore_strip_buttons(&w);
            }
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
fn ns_window(window: &tauri::WebviewWindow) -> Option<&objc2_app_kit::NSWindow> {
    let ptr = window.ns_window().ok()?;
    // SAFETY: Tauri hands out the live NSWindow of this webview window, and callers run on the main thread.
    Some(unsafe { &*(ptr as *const objc2_app_kit::NSWindow) })
}

/// The view holding the traffic lights, and the strip window it lives in when that is not the main window.
#[cfg(target_os = "macos")]
fn strip_of(ns: &objc2_app_kit::NSWindow) -> Option<(objc2::rc::Retained<objc2_app_kit::NSView>, objc2::rc::Retained<objc2_app_kit::NSWindow>)> {
    use objc2_app_kit::NSWindowButton;
    let close = ns.standardWindowButton(NSWindowButton::CloseButton)?;
    // SAFETY: plain view hierarchy reads on the main thread.
    let container = unsafe { close.superview()?.superview()? };
    let strip = container.window().filter(|strip| !std::ptr::eq(&**strip, ns))?;
    Some((container, strip))
}

#[cfg(target_os = "macos")]
fn covered_top(window: &tauri::WebviewWindow) -> f64 {
    let Some(ns) = ns_window(window) else { return 0.0 };
    let Some(screen) = ns.screen() else { return 0.0 };
    let frame = screen.frame();
    let screen_top = frame.origin.y + frame.size.height;
    // The menu bar alone, when it shows without the strip.
    let visible = screen.visibleFrame();
    let menu_bar = screen_top - (visible.origin.y + visible.size.height);
    let strip = strip_of(ns).map_or(0.0, |(_, strip)| screen_top - strip.frame().origin.y);
    // A hidden strip parks above the screen; never pad more than a generous menu bar plus title bar.
    strip.max(menu_bar).clamp(0.0, 120.0).round()
}

#[cfg(target_os = "macos")]
fn restore_strip_buttons(window: &tauri::WebviewWindow) {
    let Some(ns) = ns_window(window) else { return };
    let Some((container, _)) = strip_of(ns) else { return };
    // SAFETY: plain view hierarchy access on the main thread.
    let Some(parent) = (unsafe { container.superview() }) else { return };
    let bounds = parent.bounds();
    if container.frame() != bounds {
        container.setFrame(bounds);
    }
}

#[cfg(not(target_os = "macos"))]
fn covered_top(_window: &tauri::WebviewWindow) -> f64 {
    0.0
}

#[cfg(not(target_os = "macos"))]
fn restore_strip_buttons(_window: &tauri::WebviewWindow) {}
