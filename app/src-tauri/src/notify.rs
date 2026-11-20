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
    use std::sync::{Mutex, OnceLock};
    use tauri::{AppHandle, Emitter, Manager};

    /// The id of the bundle we are running from. A renamed copy has a different id from the compiled-in one, and
    /// the notification belongs to (and its click activates) whatever id is set, so the Info.plist is asked.
    fn bundle_id(app: &AppHandle) -> String {
        crate::config::running_bundle_id().unwrap_or_else(|| app.config().identifier.clone())
    }

    static SET: OnceLock<Result<(), String>> = OnceLock::new();

    /// `set_application` works once per process, and a notification sent before it would come from Finder.
    fn ensure_application(app: &AppHandle) -> Result<(), String> {
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

    /// What each sent notification showed, by agent. `mac-notification-sys` gives no handle on a notification, so
    /// the one to take out is found in Notification Center by its title and text.
    struct Sent {
        agent: String,
        title: String,
        body: String,
    }

    static SENT: Mutex<Vec<Sent>> = Mutex::new(Vec::new());

    fn sent() -> std::sync::MutexGuard<'static, Vec<Sent>> {
        SENT.lock().unwrap_or_else(|e| e.into_inner())
    }

    pub fn send(app: AppHandle, agent: String, title: String, body: String) -> Result<(), String> {
        ensure_application(&app)?;
        // One notification per agent: a newer state replaces the older one.
        remove(std::slice::from_ref(&agent));
        sent().push(Sent { agent: agent.clone(), title: title.clone(), body: body.clone() });
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

    /// Takes these agents' notifications out of Notification Center and leaves every other agent's. Each one sent
    /// with `wait_for_click` keeps a 0.5 s timer on the main run loop that asks the notification service,
    /// synchronously, for every delivered notification, until that one is gone. Left there they pile up: 11-16,
    /// after two hours, 36 timers held the main thread 95% of the time and typed keys waited behind them. So the
    /// page removes an agent's notification once it no longer needs the user or is in front of them, and a new one
    /// replaces the agent's older one. Removing ends its waiter as a dismissal, not a click.
    #[allow(deprecated)] // NSUserNotification is what mac-notification-sys delivers through.
    pub fn remove(agents: &[String]) {
        // Nothing was sent yet (and the bundle id is not set up): nothing to remove.
        if !matches!(SET.get(), Some(Ok(()))) || agents.is_empty() {
            return;
        }
        let mut sent = sent();
        let (gone, kept): (Vec<Sent>, Vec<Sent>) = sent.drain(..).partition(|s| agents.contains(&s.agent));
        *sent = kept;
        if gone.is_empty() {
            return;
        }
        let shows = |list: &[Sent], title: &str, body: &str| list.iter().any(|s| s.title == title && s.body == body);
        let center = objc2_foundation::NSUserNotificationCenter::defaultUserNotificationCenter();
        for n in center.deliveredNotifications().iter() {
            let title = n.title().map(|t| t.to_string()).unwrap_or_default();
            let body = n.informativeText().map(|t| t.to_string()).unwrap_or_default();
            // Same words sent for an agent that stays: leave it, a notification too many beats one lost.
            if shows(&gone, &title, &body) && !shows(&sent, &title, &body) {
                center.removeDeliveredNotification(&n);
            }
        }
    }
}

/// Shows a notification; a click on it brings the window forward and emits `notify-jump` with the agent's name.
#[cfg(target_os = "macos")]
pub fn send(app: AppHandle, agent: String, title: String, body: String) -> Result<(), String> {
    mac::send(app, agent, title, body)
}

/// Removes these agents' notifications and keeps the others. See `mac::remove`.
#[cfg(target_os = "macos")]
pub fn remove(agents: &[String]) {
    mac::remove(agents)
}

#[cfg(not(target_os = "macos"))]
pub fn remove(_agents: &[String]) {}

#[cfg(not(target_os = "macos"))]
pub fn send(_app: AppHandle, _agent: String, _title: String, _body: String) -> Result<(), String> {
    Err("notifications are only supported on macOS".to_string())
}
