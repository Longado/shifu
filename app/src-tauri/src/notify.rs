//! Background notifications (APP-SPEC-B4). Tauri's notification plugin has no click callback on the desktop, so the
//! notification goes out through `mac-notification-sys` (the crate under notify-rust on macOS), which can wait for the
//! click. Each notification waits in its own thread; only a real click brings the window forward and names the agent.

use serde::Serialize;
use std::time::{Duration, Instant};
use tauri::AppHandle;

#[derive(Clone, Serialize)]
pub struct Jump {
    pub agent: String,
}

/// A notification Shifu sent. `mac-notification-sys` gives no handle on it, so it is found in Notification Center
/// by its title and text.
struct Entry {
    id: u64,
    agent: String,
    title: String,
    body: String,
}

/// How long a withdrawn notification is looked for when it never shows up as delivered (its words changed on the
/// way, say), so the sweeping stops.
const WITHDRAWN_FOR: Duration = Duration::from_secs(30);

/// What Shifu sent and what of it is to come out. A notification is `live` until it is withdrawn (its agent was
/// handled) or its waiter ends (clicked, dismissed, removed). A withdrawn one stays listed until its waiter ends,
/// because macOS can list it as delivered only after the withdrawal: each sweep takes out whatever has shown up.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub(crate) struct Registry {
    next: u64,
    live: Vec<Entry>,
    withdrawn: Vec<(Entry, Instant)>,
    /// A thread is sweeping every 250 ms (see `mac::sweep_until_done`).
    pub(crate) sweeping: bool,
}

#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
impl Registry {
    pub(crate) const fn new() -> Self {
        Registry { next: 0, live: Vec::new(), withdrawn: Vec::new(), sweeping: false }
    }

    /// Records a notification about to go out; the agent's older ones are withdrawn.
    pub(crate) fn add(&mut self, agent: &str, title: &str, body: &str, now: Instant) -> u64 {
        self.withdraw(&[agent.to_string()], now);
        self.next += 1;
        self.live.push(Entry { id: self.next, agent: agent.into(), title: title.into(), body: body.into() });
        self.next
    }

    pub(crate) fn withdraw(&mut self, agents: &[String], now: Instant) {
        let (gone, kept): (Vec<Entry>, Vec<Entry>) = self.live.drain(..).partition(|e| agents.contains(&e.agent));
        self.live = kept;
        self.withdrawn.extend(gone.into_iter().map(|e| (e, now)));
    }

    pub(crate) fn is_live(&self, id: u64) -> bool {
        self.live.iter().any(|e| e.id == id)
    }

    /// Its waiter ended: the notification is no longer in Notification Center.
    pub(crate) fn finished(&mut self, id: u64) {
        self.live.retain(|e| e.id != id);
        self.withdrawn.retain(|(e, _)| e.id != id);
    }

    /// Which of the delivered notifications (title, text) to take out: those showing a withdrawn one's words. Words
    /// also sent for an agent that stays are left: a notification too many beats one lost.
    pub(crate) fn take_out(&mut self, delivered: &[(String, String)], now: Instant) -> Vec<usize> {
        self.withdrawn.retain(|(_, at)| now.duration_since(*at) < WITHDRAWN_FOR);
        let shows = |e: &Entry, (title, body): &(String, String)| &e.title == title && &e.body == body;
        delivered
            .iter()
            .enumerate()
            .filter(|(_, d)| self.withdrawn.iter().any(|(e, _)| shows(e, d)) && !self.live.iter().any(|e| shows(e, d)))
            .map(|(i, _)| i)
            .collect()
    }

    /// A withdrawn notification may still show up, so sweeping goes on.
    pub(crate) fn pending(&self) -> bool {
        !self.withdrawn.is_empty()
    }
}

#[cfg(target_os = "macos")]
mod mac {
    use super::Jump;
    use mac_notification_sys::{Notification, NotificationResponse, set_application};
    use super::Registry;
    use std::sync::{Mutex, MutexGuard, OnceLock};
    use std::time::{Duration, Instant};
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

    fn registry() -> MutexGuard<'static, Registry> {
        static REGISTRY: Mutex<Registry> = Mutex::new(Registry::new());
        REGISTRY.lock().unwrap_or_else(|e| e.into_inner())
    }

    pub fn send(app: AppHandle, agent: String, title: String, body: String) -> Result<(), String> {
        ensure_application(&app)?;
        // One notification per agent: a newer state replaces the older one.
        let id = registry().add(&agent, &title, &body, Instant::now());
        sweep_until_done();
        std::thread::spawn(move || {
            // Already handled before it went out: send nothing.
            if !registry().is_live(id) {
                registry().finished(id);
                return;
            }
            let sent = Notification::new().title(&title).message(&body).default_sound().wait_for_click(true).send();
            // Clicked, dismissed or removed: it is out of Notification Center, nothing is left to take out.
            registry().finished(id);
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
    pub fn remove(agents: &[String]) {
        // Nothing was sent yet (and the bundle id is not set up): nothing to remove.
        if !matches!(SET.get(), Some(Ok(()))) || agents.is_empty() {
            return;
        }
        registry().withdraw(agents, Instant::now());
        sweep_until_done();
    }

    /// One pass over Notification Center; true while a withdrawn notification may still show up there.
    #[allow(deprecated)] // NSUserNotification is what mac-notification-sys delivers through.
    fn sweep(registry: &mut Registry) -> bool {
        let center = objc2_foundation::NSUserNotificationCenter::defaultUserNotificationCenter();
        let delivered: Vec<_> = center.deliveredNotifications().iter().collect();
        let words: Vec<(String, String)> = delivered
            .iter()
            .map(|n| {
                let title = n.title().map(|t| t.to_string()).unwrap_or_default();
                let body = n.informativeText().map(|t| t.to_string()).unwrap_or_default();
                (title, body)
            })
            .collect();
        for i in registry.take_out(&words, Instant::now()) {
            center.removeDeliveredNotification(&delivered[i]);
        }
        registry.pending()
    }

    /// Sweeps now, and again every 250 ms while a withdrawn notification has not gone yet: one that was withdrawn
    /// before macOS listed it as delivered is taken out when it shows up.
    fn sweep_until_done() {
        {
            let mut registry = registry();
            if !sweep(&mut registry) || registry.sweeping {
                return;
            }
            registry.sweeping = true;
        }
        std::thread::spawn(|| loop {
            std::thread::sleep(Duration::from_millis(250));
            let mut registry = registry();
            if !sweep(&mut registry) {
                registry.sweeping = false;
                return;
            }
        });
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

#[cfg(test)]
mod tests {
    use super::*;

    /// A stand-in for Notification Center: what `take_out` picks is taken out of `delivered`.
    fn sweep(registry: &mut Registry, delivered: &mut Vec<(String, String)>, now: Instant) {
        let gone = registry.take_out(delivered, now);
        let mut i = 0;
        delivered.retain(|_| {
            i += 1;
            !gone.contains(&(i - 1))
        });
    }

    fn words(title: &str, body: &str) -> (String, String) {
        (title.to_string(), body.to_string())
    }

    #[test]
    fn a_notification_delivered_after_its_session_was_handled_is_taken_out() {
        let now = Instant::now();
        let mut registry = Registry::new();
        let mut delivered = Vec::new();
        let a = registry.add("a", "A", "等你批准", now);
        registry.add("b", "B", "答完了", now);
        delivered.push(words("B", "答完了"));

        // A is handled before macOS lists its notification as delivered.
        registry.withdraw(&["a".to_string()], now);
        sweep(&mut registry, &mut delivered, now);
        assert!(registry.pending());

        // It shows up late: the next sweep takes it out, and B, still waiting, keeps its own.
        delivered.push(words("A", "等你批准"));
        sweep(&mut registry, &mut delivered, now);
        assert_eq!(delivered, vec![words("B", "答完了")]);

        // Its waiter ends with the removal: nothing is left to sweep for.
        registry.finished(a);
        assert!(!registry.pending());
    }

    #[test]
    fn a_newer_notification_replaces_the_agents_older_one() {
        let now = Instant::now();
        let mut registry = Registry::new();
        let old = registry.add("a", "A", "第一轮", now);
        let mut delivered = vec![words("A", "第一轮")];
        let new = registry.add("a", "A", "第二轮", now);
        assert!(!registry.is_live(old) && registry.is_live(new));
        delivered.push(words("A", "第二轮"));
        sweep(&mut registry, &mut delivered, now);
        assert_eq!(delivered, vec![words("A", "第二轮")]);
    }

    #[test]
    fn the_same_words_sent_for_an_agent_that_stays_are_kept() {
        let now = Instant::now();
        let mut registry = Registry::new();
        registry.add("a", "同名", "答完了", now);
        registry.add("b", "同名", "答完了", now);
        let mut delivered = vec![words("同名", "答完了"), words("同名", "答完了")];
        registry.withdraw(&["a".to_string()], now);
        sweep(&mut registry, &mut delivered, now);
        assert_eq!(delivered.len(), 2);
    }

    #[test]
    fn one_that_never_shows_up_stops_being_looked_for() {
        let now = Instant::now();
        let mut registry = Registry::new();
        registry.add("a", "A", "等你批准", now);
        registry.withdraw(&["a".to_string()], now);
        sweep(&mut registry, &mut Vec::new(), now + WITHDRAWN_FOR);
        assert!(!registry.pending());
    }
}
