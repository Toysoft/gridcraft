//! Protocol 1.2: document events. A plugin that declares `"events": ["documentSaved", …]` in its
//! manifest gets `events/documentSaved` (and the others) after the event happened.
//!
//! Events are notifications, sent from a worker thread in the order they happened: the save, the
//! open or the close they report has already finished, and a slow or failing plugin never holds
//! it up. What the plugins answer (a message, an error) is queued for the app to show
//! ([`take_outcomes`]). During an event a plugin may log and show messages, but `host/sync`
//! (and the workbook's VBA project) are refused with `AccessDenied`.

use std::time::Duration;

use serde_json::{Value as Json, json};

use crate::HostError;

pub const DOCUMENT_OPENED: &str = "documentOpened";
pub const DOCUMENT_SAVED: &str = "documentSaved";
pub const DOCUMENT_CLOSED: &str = "documentClosed";
/// The events a manifest may declare.
pub const EVENTS: &[&str] = &[DOCUMENT_OPENED, DOCUMENT_SAVED, DOCUMENT_CLOSED];
/// Outcomes kept for the app to pick up (a headless host never takes them).
const MAX_OUTCOMES: usize = 100;

/// How long a plugin may take over an event: 30 s, 5 s for `documentClosed` (the app may be
/// quitting).
pub fn timeout(event: &str) -> Duration {
    if event == DOCUMENT_CLOSED { Duration::from_secs(5) } else { Duration::from_secs(30) }
}

/// What came of sending an event to one plugin.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum OutcomeKind {
    /// The plugin answered `{"message": …}`: shown in the status bar.
    Message,
    /// The plugin answered with an error (or timed out, or exited): shown as an error notice.
    Error,
    /// The plugin sent `host/message` while it handled the event.
    HostMessage,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Outcome {
    pub plugin: String,
    pub event: String,
    pub kind: OutcomeKind,
    pub text: String,
}

/// The answer to a `host/…` request a plugin sends while it handles an event.
pub(crate) fn host_during_event(plugin: &str, event: &str, method: &str, params: &Json) -> Result<Json, HostError> {
    match method {
        "host/log" => {
            let text: String = params.get("text").and_then(Json::as_str).unwrap_or("").chars().take(4000).collect();
            log::info!("plugin {plugin}: {text}");
            Ok(json!({}))
        }
        "host/message" => {
            let text: String = params.get("text").and_then(Json::as_str).unwrap_or("").chars().take(4000).collect();
            push_outcome(Outcome { plugin: plugin.to_string(), event: event.to_string(), kind: OutcomeKind::HostMessage, text });
            Ok(json!({"result": 1}))
        }
        "host/sync" | "host/vbaProject/get" | "host/vbaProject/set" => {
            let message = format!("{method} isn't available during {event}: events are notifications, not edits");
            Err(HostError { code: -32000, message: message.clone(), data: json!({"code": "AccessDenied", "message": message, "action": Json::Null}) })
        }
        _ => Err(HostError::unknown()),
    }
}

/// Sends `event` to one plugin and records what came of it.
fn deliver(plugin: &crate::Plugin, event: &str, params: &Json) {
    let id = plugin.manifest.id.clone();
    let method = format!("events/{event}");
    let r = plugin.request_with(&method, params.clone(), timeout(event), &mut |m, p| host_during_event(&id, event, m, p));
    match r {
        Ok(answer) => {
            if let Some(text) = answer.get("message").and_then(Json::as_str).map(str::trim).filter(|t| !t.is_empty()) {
                push_outcome(Outcome { plugin: id, event: event.to_string(), kind: OutcomeKind::Message, text: text.chars().take(4000).collect() });
            }
        }
        Err(e) => {
            log::warn!("plugin {id}: {method}: {e}");
            push_outcome(Outcome { plugin: id, event: event.to_string(), kind: OutcomeKind::Error, text: e.chars().take(4000).collect() });
        }
    }
}

/// The loaded, enabled plugins that declared `event`.
pub fn subscribers(event: &str) -> Vec<std::sync::Arc<crate::Plugin>> {
    crate::list().into_iter().filter(|p| p.enabled() && p.manifest.office.is_none() && p.manifest.events.iter().any(|e| e == event)).collect()
}

fn outcomes() -> &'static std::sync::Mutex<std::collections::VecDeque<Outcome>> {
    static OUTCOMES: std::sync::OnceLock<std::sync::Mutex<std::collections::VecDeque<Outcome>>> = std::sync::OnceLock::new();
    OUTCOMES.get_or_init(Default::default)
}

fn push_outcome(o: Outcome) {
    let mut q = crate::lock(outcomes());
    if q.len() >= MAX_OUTCOMES {
        q.pop_front();
    }
    q.push_back(o);
}

/// What the plugins answered to the events since the last call, oldest first.
pub fn take_outcomes() -> Vec<Outcome> {
    crate::lock(outcomes()).drain(..).collect()
}

#[cfg(not(target_arch = "wasm32"))]
pub use native::{dispatch, pending, wait_idle};

#[cfg(target_arch = "wasm32")]
pub use web::{dispatch, pending, wait_idle};

#[cfg(target_arch = "wasm32")]
mod web {
    use super::*;

    /// The web app has no plugins: nothing to send.
    pub fn dispatch(_event: &str, _params: Json) {}
    pub fn pending() -> usize {
        0
    }
    pub fn wait_idle(_timeout: Duration) -> bool {
        true
    }
}

#[cfg(not(target_arch = "wasm32"))]
#[allow(clippy::disallowed_methods)] // off wasm only: the clock works here
mod native {
    use std::collections::VecDeque;
    use std::sync::{Condvar, Mutex, OnceLock};
    use std::time::Instant;

    use super::*;

    struct Queue {
        jobs: VecDeque<(String, Json)>,
        /// Events queued or being delivered.
        pending: usize,
        worker: bool,
    }

    fn queue() -> &'static (Mutex<Queue>, Condvar) {
        static QUEUE: OnceLock<(Mutex<Queue>, Condvar)> = OnceLock::new();
        QUEUE.get_or_init(|| (Mutex::new(Queue { jobs: VecDeque::new(), pending: 0, worker: false }), Condvar::new()))
    }

    /// Sends `event` with `params` to every plugin that declared it, on the events thread: this
    /// returns at once. Nothing happens when no plugin wants it.
    pub fn dispatch(event: &str, params: Json) {
        if subscribers(event).is_empty() {
            return;
        }
        let (lock, cv) = queue();
        let mut q = crate::lock(lock);
        if !q.worker {
            let spawned = std::thread::Builder::new().name("plugin-events".into()).spawn(work);
            if let Err(e) = spawned {
                log::warn!("plugin events: can't start the events thread: {e}");
                return;
            }
            q.worker = true;
        }
        q.jobs.push_back((event.to_string(), params));
        q.pending += 1;
        cv.notify_all();
    }

    fn work() {
        let (lock, cv) = queue();
        loop {
            let job = {
                let mut q = crate::lock(lock);
                loop {
                    if let Some(job) = q.jobs.pop_front() {
                        break job;
                    }
                    q = match cv.wait(q) {
                        Ok(g) => g,
                        Err(p) => p.into_inner(),
                    };
                }
            };
            let (event, params) = job;
            for plugin in subscribers(&event) {
                // A plugin's misbehaviour must not end the thread (the next events still go out).
                let ok = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| deliver(&plugin, &event, &params)));
                if ok.is_err() {
                    log::warn!("plugin {}: events/{event} failed unexpectedly", plugin.manifest.id);
                }
            }
            let mut q = crate::lock(lock);
            q.pending = q.pending.saturating_sub(1);
            cv.notify_all();
        }
    }

    /// Events queued or being delivered.
    pub fn pending() -> usize {
        crate::lock(&queue().0).pending
    }

    /// Waits until every event sent so far has been delivered, at most `timeout`. True when they
    /// all were.
    pub fn wait_idle(timeout: Duration) -> bool {
        let deadline = Instant::now() + timeout;
        let (lock, cv) = queue();
        let mut q = crate::lock(lock);
        while q.pending > 0 {
            let left = deadline.saturating_duration_since(Instant::now());
            if left.is_zero() {
                return false;
            }
            q = match cv.wait_timeout(q, left) {
                Ok((g, _)) => g,
                Err(p) => p.into_inner().0,
            };
        }
        true
    }
}
