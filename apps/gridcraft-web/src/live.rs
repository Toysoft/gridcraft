//! The control channel in the browser (`?control=ws://127.0.0.1:7980`, or just `?control=7980`).
//!
//! A page can't listen on a port, so it dials out over a WebSocket to `gridcraft-cli relay`,
//! which serves the ordinary JSON-lines control protocol to scripts, `gridcraft-cli send` and
//! MCP. Each message from the relay is one request `{id, method, params}`; it runs through the
//! same handler as the desktop app's `--control` port and the reply goes back as
//! `{id, ok, result | error}`. The page reconnects by itself when the relay restarts.

use std::cell::RefCell;
use std::collections::VecDeque;
use std::rc::Rc;
use std::sync::mpsc::{Receiver, Sender, channel};

use gridcraft_ui_egui::{ControlRequest, ControlResponse};
use serde_json::{Value as Json, json};
use wasm_bindgen::JsCast as _;
use wasm_bindgen::closure::Closure;

const RETRY_MS: i32 = 2000;
/// How long a request may wait for the app. Only a screenshot waits at all, for a painted frame,
/// and a hidden tab or covered window paints none.
const REPLY_MS: f64 = 10_000.0;

/// The relay's address from `?control=`: a port on this machine, or a `ws://`/`wss://` URL.
pub fn url_from(param: &str) -> Option<String> {
    let p = param.trim();
    if let Ok(port) = p.parse::<u16>() {
        return Some(format!("ws://127.0.0.1:{port}"));
    }
    (p.starts_with("ws://") || p.starts_with("wss://")).then(|| p.to_string())
}

struct State {
    url: String,
    ctx: egui::Context,
    socket: RefCell<Option<web_sys::WebSocket>>,
    /// Requests that arrived and haven't been handed to the app yet.
    arrived: RefCell<VecDeque<String>>,
    handlers: RefCell<Vec<Closure<dyn FnMut(web_sys::Event)>>>,
    connected: RefCell<bool>,
}

pub struct Live {
    state: Rc<State>,
    tx: Sender<ControlRequest>,
    waiting: Vec<(Json, Receiver<ControlResponse>, f64)>,
}

impl Live {
    /// Connects to the relay; the app reads requests from the returned receiver
    /// (`SheetApp::control_rx`).
    pub fn start(url: String, ctx: egui::Context) -> (Self, Receiver<ControlRequest>) {
        let (tx, rx) = channel();
        let state = Rc::new(State {
            url,
            ctx,
            socket: RefCell::new(None),
            arrived: RefCell::new(VecDeque::new()),
            handlers: RefCell::new(Vec::new()),
            connected: RefCell::new(false),
        });
        connect(&state);
        (Self { state, tx, waiting: Vec::new() }, rx)
    }

    /// Hands the requests that arrived to the app. Call before `SheetApp::logic`.
    pub fn pump(&mut self) {
        let arrived: Vec<String> = self.state.arrived.borrow_mut().drain(..).collect();
        for text in arrived {
            let msg: Json = match serde_json::from_str(&text) {
                Ok(v) => v,
                Err(e) => {
                    self.reply(Json::Null, json!({"ok": false, "error": format!("invalid JSON: {e}")}));
                    continue;
                }
            };
            let id = msg.get("id").cloned().unwrap_or(Json::Null);
            let method = msg.get("method").and_then(Json::as_str).unwrap_or("").to_string();
            let params = msg.get("params").cloned().unwrap_or(json!({}));
            let (req, reply) = ControlRequest::new(method, params);
            if self.tx.send(req).is_ok() {
                self.waiting.push((id, reply, js_sys::Date::now() + REPLY_MS));
            }
        }
    }

    /// Sends back the replies the app has made. Call after `SheetApp::logic`; a screenshot
    /// replies a few frames later.
    pub fn flush(&mut self, ctx: &egui::Context) {
        let mut done = Vec::new();
        let now = js_sys::Date::now();
        self.waiting.retain(|(id, rx, deadline)| match rx.try_recv() {
            Ok(v) => {
                done.push((id.clone(), v));
                false
            }
            Err(std::sync::mpsc::TryRecvError::Empty) if now > *deadline => {
                let error = "the page drew no frame in time: bring its tab to the front (a hidden tab or covered window doesn't paint), then retry";
                done.push((id.clone(), json!({"ok": false, "error": error})));
                false
            }
            Err(std::sync::mpsc::TryRecvError::Empty) => true,
            Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                done.push((id.clone(), json!({"ok": false, "error": "the request was dropped"})));
                false
            }
        });
        for (id, v) in done {
            self.reply(id, v);
        }
        if !self.waiting.is_empty() {
            ctx.request_repaint_after(std::time::Duration::from_millis(30));
        }
    }

    fn reply(&self, id: Json, mut v: Json) {
        if let Some(o) = v.as_object_mut() {
            o.insert("id".into(), id);
        }
        if let Some(socket) = self.state.socket.borrow().as_ref() {
            if let Err(e) = socket.send_with_str(&v.to_string()) {
                log::warn!("live control: couldn't send a reply: {e:?}");
            }
        }
    }
}

fn connect(state: &Rc<State>) {
    let socket = match web_sys::WebSocket::new(&state.url) {
        Ok(s) => s,
        Err(e) => {
            log::error!("live control: can't connect to {}: {e:?}", state.url);
            return;
        }
    };
    let on_open = {
        let state = state.clone();
        Closure::<dyn FnMut(web_sys::Event)>::new(move |_| {
            *state.connected.borrow_mut() = true;
            log::info!("live control: connected to {}", state.url);
        })
    };
    let on_message = {
        let state = state.clone();
        Closure::<dyn FnMut(web_sys::Event)>::new(move |e: web_sys::Event| {
            let Some(text) = e.dyn_ref::<web_sys::MessageEvent>().and_then(|m| m.data().as_string()) else { return };
            state.arrived.borrow_mut().push_back(text);
            state.ctx.request_repaint();
        })
    };
    let on_close = {
        let state = state.clone();
        Closure::<dyn FnMut(web_sys::Event)>::new(move |_| {
            if std::mem::replace(&mut *state.connected.borrow_mut(), false) {
                log::warn!("live control: disconnected from {}; retrying", state.url);
            }
            retry(&state);
        })
    };
    socket.set_onopen(Some(on_open.as_ref().unchecked_ref()));
    socket.set_onmessage(Some(on_message.as_ref().unchecked_ref()));
    socket.set_onclose(Some(on_close.as_ref().unchecked_ref()));
    if let Some(old) = state.socket.borrow_mut().replace(socket) {
        old.set_onopen(None);
        old.set_onmessage(None);
        old.set_onclose(None);
    }
    // The previous socket's handlers are detached above, so they can go.
    *state.handlers.borrow_mut() = vec![on_open, on_message, on_close];
}

fn retry(state: &Rc<State>) {
    let Some(window) = web_sys::window() else { return };
    let state = state.clone();
    let again = Closure::once_into_js(move || connect(&state));
    if window.set_timeout_with_callback_and_timeout_and_arguments_0(again.unchecked_ref(), RETRY_MS).is_err() {
        log::error!("live control: couldn't schedule a reconnect");
    }
}
