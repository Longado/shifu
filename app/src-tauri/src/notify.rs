//! Background notifications (APP-SPEC-B4). Tauri's notification plugin has no click callback on the desktop, so the
//! notification goes out through `mac-notification-sys` (the crate under notify-rust on macOS), which can wait for the
//! click. Each notification waits in its own thread; only a real click brings the window forward and names the agent.

use serde::Serialize;
use tauri::AppHandle;

#[derive(Clone, Serialize)]
pub struct Jump {
    pub agent: String,
}

#[cfg(target_os = "macos")]
mod mac {
    use super::Jump;
    use mac_notification_sys::{Notification, NotificationResponse, set_application};
    use std::sync::OnceLock;
    use tauri::{AppHandle, Emitter, Manager};

    /// The id of the bundle we are running from. A renamed copy has a different id from the compiled-in one, and
    /// the notification belongs to (and its click activates) whatever id is set, so the Info.plist is asked.
    fn bundle_id(app: &AppHandle) -> String {
        crate::config::running_bundle_id().unwrap_or_else(|| app.config().identifier.clone())
    }

    /// `set_application` works once per process, and a notification sent before it would come from Finder.
    fn ensure_application(app: &AppHandle) -> Result<(), String> {
        static SET: OnceLock<Result<(), String>> = OnceLock::new();
        SET.get_or_init(|| {
            let id = bundle_id(app);
            set_application(&id).map_err(|e| format!("notifications: cannot use bundle id {id}: {e}"))
        })
        .clone()
    }

    /// The one thing a click does: window to the front, then tell the page where to jump.
    fn on_click(app: &AppHandle, agent: &str) {
        if let Some(window) = app.get_webview_window("main") {
            for result in [window.unminimize(), window.show(), window.set_focus()] {
                if let Err(e) = result {
                    eprintln!("shifu-app: bring window forward failed: {e}");
                }
            }
        }
        if let Err(e) = app.emit("notify-jump", Jump { agent: agent.to_string() }) {
            eprintln!("shifu-app: emit notify-jump failed: {e}");
        }
    }

    pub fn send(app: AppHandle, agent: String, title: String, body: String) -> Result<(), String> {
        ensure_application(&app)?;
        std::thread::spawn(move || {
            let sent = Notification::new().title(&title).message(&body).default_sound().wait_for_click(true).send();
            match sent {
                // Closed, expired or slid into the notification centre all come back as something else: not a click.
                Ok(NotificationResponse::Click) => on_click(&app, &agent),
                Ok(_) => {}
                Err(e) => {
                    let message = format!("notification for {agent} failed: {e}");
                    eprintln!("shifu-app: {message}");
                    if let Err(e) = app.emit("notify-error", message) {
                        eprintln!("shifu-app: emit notify-error failed: {e}");
                    }
                }
            }
        });
        Ok(())
    }
}

/// Shows a notification; a click on it brings the window forward and emits `notify-jump` with the agent's name.
#[cfg(target_os = "macos")]
pub fn send(app: AppHandle, agent: String, title: String, body: String) -> Result<(), String> {
    mac::send(app, agent, title, body)
}

#[cfg(not(target_os = "macos"))]
pub fn send(_app: AppHandle, _agent: String, _title: String, _body: String) -> Result<(), String> {
    Err("notifications are only supported on macOS".to_string())
}
