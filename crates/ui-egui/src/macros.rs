//! Running a workbook's VBA macros in the app.
//!
//! Each workbook with macros gets a worker thread that owns its interpreter (so module-level
//! variables live between runs, as in Excel). The worker's host forwards every object-model call
//! to this (UI) thread, which serves them against the session each frame within a time budget,
//! so the window stays responsive. A `MsgBox`/`InputBox` waits for the dialog the UI shows, and a
//! modal UserForm's `.Show` waits for the user's actions on the form, delivered as VBA events.
//!
//! Macros are off until the user clicks Enable Content on the workbook's security bar.

use std::collections::{HashMap, VecDeque};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, Sender, channel};

use egui::{Align2, Color32, Key, Modifiers, Sense, vec2};
use gridcraft_engine::vba::{self as evba};
use gridcraft_engine::vbalang::{Event, HostArg, HostObj, Value, VbaError};

use crate::{Notice, NoticeKind, SheetApp, theme};

/// A call the macro thread makes into the object model.
enum Call {
    Global(String, Vec<HostArg>),
    Get(HostObj, String, Vec<HostArg>),
    Set(HostObj, String, Vec<HostArg>, Value),
    Items(HostObj),
    TakeEvent,
    EventDone(Event, Vec<Value>),
}

enum Reply {
    Value(Result<Option<Value>, VbaError>),
    Unit(Result<(), VbaError>),
    Values(Result<Vec<Value>, VbaError>),
    Event(Option<Event>),
}

/// What the macro thread asks of the UI thread.
enum Request {
    Call(Call, Sender<Reply>),
    MsgBox {
        prompt: String,
        buttons: i64,
        title: String,
        reply: Sender<i64>,
    },
    InputBox {
        prompt: String,
        title: String,
        default: String,
        reply: Sender<Option<String>>,
    },
    /// A modal form is open: the next event for it, `None` once it closed.
    WaitEvent(Sender<Option<Event>>),
    Print(String),
    Done(Option<VbaError>),
}

enum Job {
    Run(String),
    Event(Event),
}

/// The host the interpreter sees on the macro thread: each call is a round trip to the UI.
struct Proxy {
    tx: Sender<Request>,
    stop: Arc<AtomicBool>,
}

impl Proxy {
    fn call(&self, c: Call) -> Result<Reply, VbaError> {
        let (rtx, rrx) = channel();
        self.tx.send(Request::Call(c, rtx)).map_err(|_| VbaError::Stopped)?;
        rrx.recv().map_err(|_| VbaError::Stopped)
    }
}

fn local_now() -> f64 {
    gridcraft_engine::core::date::unix_now_ms().map(gridcraft_engine::core::date::serial_from_unix_ms).unwrap_or(0.0)
}

impl gridcraft_engine::vbalang::Host for Proxy {
    fn global(&mut self, name: &str, args: &[HostArg]) -> Result<Option<Value>, VbaError> {
        match self.call(Call::Global(name.to_string(), args.to_vec()))? {
            Reply::Value(v) => v,
            _ => Ok(None),
        }
    }
    fn get(&mut self, obj: &HostObj, member: &str, args: &[HostArg]) -> Result<Value, VbaError> {
        match self.call(Call::Get(obj.clone(), member.to_string(), args.to_vec()))? {
            Reply::Value(v) => v.map(Option::unwrap_or_default),
            _ => Ok(Value::Empty),
        }
    }
    fn set(&mut self, obj: &HostObj, member: &str, args: &[HostArg], value: Value) -> Result<(), VbaError> {
        match self.call(Call::Set(obj.clone(), member.to_string(), args.to_vec(), value))? {
            Reply::Unit(r) => r,
            _ => Ok(()),
        }
    }
    fn items(&mut self, obj: &HostObj) -> Result<Vec<Value>, VbaError> {
        match self.call(Call::Items(obj.clone()))? {
            Reply::Values(v) => v,
            _ => Ok(vec![]),
        }
    }
    fn msgbox(&mut self, prompt: &str, buttons: i64, title: &str) -> Result<i64, VbaError> {
        let (rtx, rrx) = channel();
        self.tx.send(Request::MsgBox { prompt: prompt.into(), buttons, title: title.into(), reply: rtx }).map_err(|_| VbaError::Stopped)?;
        rrx.recv().map_err(|_| VbaError::Stopped)
    }
    fn inputbox(&mut self, prompt: &str, title: &str, default: &str) -> Result<Option<String>, VbaError> {
        let (rtx, rrx) = channel();
        self.tx
            .send(Request::InputBox { prompt: prompt.into(), title: title.into(), default: default.into(), reply: rtx })
            .map_err(|_| VbaError::Stopped)?;
        rrx.recv().map_err(|_| VbaError::Stopped)
    }
    fn now(&mut self) -> f64 {
        local_now()
    }
    fn debug_print(&mut self, text: &str) {
        let _ = self.tx.send(Request::Print(text.to_string()));
    }
    fn take_event(&mut self) -> Option<Event> {
        match self.call(Call::TakeEvent) {
            Ok(Reply::Event(e)) => e,
            _ => None,
        }
    }
    fn wait_event(&mut self) -> Result<Option<Event>, VbaError> {
        let (rtx, rrx) = channel();
        self.tx.send(Request::WaitEvent(rtx)).map_err(|_| VbaError::Stopped)?;
        rrx.recv().map_err(|_| VbaError::Stopped)
    }
    fn event_done(&mut self, event: &Event, args: &[Value]) {
        let _ = self.call(Call::EventDone(event.clone(), args.to_vec()));
    }
    fn should_stop(&mut self) -> bool {
        self.stop.load(Ordering::Relaxed)
    }
}

/// A pending dialog the macro is waiting on.
enum Pending {
    MsgBox { prompt: String, buttons: i64, title: String, reply: Sender<i64> },
    InputBox { prompt: String, title: String, text: String, reply: Sender<Option<String>> },
    Wait(Sender<Option<Event>>),
}

/// One workbook's macro thread.
struct Worker {
    jobs: Sender<Job>,
    requests: Receiver<Request>,
    stop: Arc<AtomicBool>,
    busy: bool,
    pending: Option<Pending>,
    /// What's running (for the status bar).
    running: String,
    /// Profiling (`GRIDCRAFT_PROFILE`): calls served, ms spent serving them, start time.
    calls: u64,
    serve_ms: f64,
    started: f64,
}

/// The app's macro state.
#[derive(Default)]
pub struct Macros {
    workers: HashMap<u64, Worker>,
    /// User actions on forms, waiting for the macro (per workbook).
    user_events: HashMap<u64, VecDeque<Event>>,
    /// `Debug.Print` output (the Immediate window).
    pub immediate: Vec<String>,
}

impl Macros {
    /// Whether a macro is running in the workbook (forms then take input, the sheet waits).
    pub fn running(&self, uid: u64) -> Option<&str> {
        self.workers.get(&uid).filter(|w| w.busy).map(|w| w.running.as_str())
    }
    /// Calls served and milliseconds spent serving them in the current run (profiling).
    pub fn stats(&self, uid: u64) -> (u64, f64) {
        self.workers.get(&uid).map_or((0, 0.0), |w| (w.calls, w.serve_ms))
    }
    /// Queues a user action on a form (a click, typing, a key) for the macro.
    pub fn user_event(&mut self, uid: u64, ev: Event) {
        self.user_events.entry(uid).or_default().push_back(ev);
    }
    /// Asks the running macro to stop (Esc / Ctrl+Break).
    pub fn stop(&mut self, uid: u64) {
        if let Some(w) = self.workers.get(&uid) {
            w.stop.store(true, Ordering::Relaxed);
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn spawn(project: gridcraft_engine::vbalang::ovba::Project) -> Option<Worker> {
    let (jtx, jrx) = channel::<Job>();
    let (rtx, rrx) = channel::<Request>();
    let stop = Arc::new(AtomicBool::new(false));
    let stop2 = stop.clone();
    std::thread::Builder::new()
        .name("vba".into())
        .spawn(move || {
            let mut it = gridcraft_engine::vbalang::Interp::new(&project);
            let mut host = Proxy { tx: rtx, stop: stop2 };
            for job in jrx {
                host.stop.store(false, Ordering::Relaxed);
                let r = match job {
                    Job::Run(name) => it.run(&mut host, &name, vec![]).map(|_| ()),
                    Job::Event(ev) => it.run_event(&mut host, &ev.module, &ev.proc, ev.args.clone()).map(|after| {
                        if let Some(after) = after {
                            gridcraft_engine::vbalang::Host::event_done(&mut host, &ev, &after);
                        }
                    }),
                };
                let err = r.err().filter(|e| !matches!(e, VbaError::Stopped)).map(|e| evba::with_location(e, &it));
                if host.tx.send(Request::Done(err)).is_err() {
                    break;
                }
            }
        })
        .ok()?;
    Some(Worker { jobs: jtx, requests: rrx, stop, busy: false, pending: None, running: String::new(), calls: 0, serve_ms: 0.0, started: 0.0 })
}

#[cfg(target_arch = "wasm32")]
fn spawn(_project: gridcraft_engine::vbalang::ovba::Project) -> Option<Worker> {
    None
}

fn worker(app: &mut SheetApp, uid: u64) -> Result<(), String> {
    if app.macros.workers.contains_key(&uid) {
        return Ok(());
    }
    let c = evba::contents(&mut app.session, uid).ok_or("This workbook has no macros GridCraft can read.")?;
    let w = spawn(c.project.clone()).ok_or("Macros can't run here (no worker thread).")?;
    app.macros.workers.insert(uid, w);
    Ok(())
}

/// Runs a macro of the active workbook (asking for Enable Content first if needed).
pub fn run(app: &mut SheetApp, name: &str) {
    let Some(uid) = evba::active_uid(&app.session) else { return };
    if !evba::enabled(&app.session, uid) {
        app.message = Some((
            "Macros are disabled".into(),
            format!("This workbook's macros are disabled, so '{name}' can't run. Click Enable Content on the security bar to allow them."),
        ));
        return;
    }
    if let Err(e) = worker(app, uid) {
        app.message = Some(("Macros".into(), e));
        return;
    }
    let Some(w) = app.macros.workers.get_mut(&uid) else { return };
    if w.busy {
        app.toast = Some((format!("'{}' is still running.", w.running), crate::now_ms()));
        return;
    }
    if w.jobs.send(Job::Run(name.to_string())).is_ok() {
        w.busy = true;
        w.running = name.to_string();
        w.calls = 0;
        w.serve_ms = 0.0;
        w.started = crate::now_ms();
    }
}

/// Runs an event handler (a modeless form's control, `Workbook_Open`) when no macro is running;
/// while one waits on a modal form the event goes to it instead.
pub fn event(app: &mut SheetApp, uid: u64, ev: Event) {
    let waiting = app.macros.workers.get(&uid).is_some_and(|w| w.busy);
    if waiting {
        app.macros.user_event(uid, ev);
        return;
    }
    if !evba::enabled(&app.session, uid) || worker(app, uid).is_err() {
        return;
    }
    if let Some(w) = app.macros.workers.get_mut(&uid)
        && w.jobs.send(Job::Event(ev.clone())).is_ok()
    {
        w.busy = true;
        w.running = format!("{}.{}", ev.module, ev.proc);
        w.calls = 0;
        w.serve_ms = 0.0;
        w.started = crate::now_ms();
    }
}

/// Hands queued events (a sheet control's `Change`, actions on a modeless form) to the macro
/// thread: one starts now when it is idle, the rest wait for it. Without macros enabled they
/// are dropped.
pub fn flush_events(app: &mut SheetApp, uid: u64) {
    if !evba::enabled(&app.session, uid) {
        while evba::take_event(&mut app.session, uid).is_some() {}
        return;
    }
    while let Some(ev) = evba::take_event(&mut app.session, uid) {
        event(app, uid, ev);
    }
}

/// Enable Content: macros may run; `Workbook_Open` (and `Auto_Open`) run now.
pub fn enable_content(app: &mut SheetApp) {
    let Some(uid) = evba::active_uid(&app.session) else { return };
    evba::enable(&mut app.session, uid, true);
    app.notice = None;
    let Some(c) = evba::contents(&mut app.session, uid) else { return };
    let this = app.session.active().and_then(|d| d.wb.code_name.clone()).unwrap_or_else(|| "ThisWorkbook".into());
    if c.project.module(&this).is_some_and(|m| m.code.to_ascii_lowercase().contains("sub workbook_open")) {
        event(app, uid, Event { module: this, proc: "Workbook_Open".into(), args: vec![] });
    } else if c.project.modules.iter().any(|m| m.code.to_ascii_lowercase().contains("sub auto_open")) {
        run(app, "Auto_Open");
    }
}

/// The security bar a workbook with macros opens with.
pub fn security_notice(app: &mut SheetApp) {
    app.notice = Some(Notice {
        text: "SECURITY WARNING  Macros have been disabled.".into(),
        kind: NoticeKind::Warning,
        action: Some(("Enable Content".into(), "__enableMacros".into())),
    });
}

fn serve(app: &mut SheetApp, uid: u64, c: Call) -> Reply {
    let s = &mut app.session;
    match c {
        Call::Global(name, args) => Reply::Value(evba::model::global(s, uid, &name, &args)),
        Call::Get(obj, member, args) => Reply::Value(evba::dispatch_get(s, uid, &obj, &member, &args).map(Some)),
        Call::Set(obj, member, args, v) => Reply::Unit(evba::model::set(s, uid, &obj, &member, &args, v)),
        Call::Items(obj) => Reply::Values(evba::model::items(s, &obj)),
        Call::TakeEvent => Reply::Event(evba::take_event(s, uid)),
        Call::EventDone(_ev, _args) => Reply::Unit(Ok(())),
    }
}

/// Serves the macro threads' requests (call once per frame). Keeps working for a few
/// milliseconds so a busy macro runs fast, then lets the frame finish.
pub fn pump(app: &mut SheetApp, ctx: &egui::Context) {
    let uids: Vec<u64> = app.macros.workers.keys().copied().collect();
    let deadline = crate::now_ms() + 30.0;
    for uid in uids {
        loop {
            // A modal form waiting for the user: deliver an action, or close.
            if matches!(app.macros.workers.get(&uid).map(|w| &w.pending), Some(Some(Pending::Wait(_)))) {
                let ev = app.macros.user_events.get_mut(&uid).and_then(VecDeque::pop_front).or_else(|| evba::take_event(&mut app.session, uid));
                let open = app.session.macros.doc(uid).is_some_and(|d| d.forms.iter().any(|f| f.visible && f.modal));
                if ev.is_none() && open {
                    break;
                }
                if let Some(w) = app.macros.workers.get_mut(&uid)
                    && let Some(Pending::Wait(reply)) = w.pending.take()
                {
                    let _ = reply.send(ev);
                }
            }
            let req = {
                let Some(w) = app.macros.workers.get_mut(&uid) else { break };
                if w.pending.is_some() || !w.busy {
                    break;
                }
                let left = deadline - crate::now_ms();
                if left <= 0.0 {
                    ctx.request_repaint();
                    break;
                }
                match w.requests.recv_timeout(std::time::Duration::from_micros((left * 1000.0) as u64)) {
                    Ok(r) => r,
                    Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
                        ctx.request_repaint();
                        break;
                    }
                    Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
                        app.macros.workers.remove(&uid);
                        break;
                    }
                }
            };
            let pending = match req {
                Request::Call(c, reply) => {
                    let t = crate::now_ms();
                    let r = serve(app, uid, c);
                    let _ = reply.send(r);
                    app.after_engine();
                    if let Some(w) = app.macros.workers.get_mut(&uid) {
                        w.calls += 1;
                        w.serve_ms += crate::now_ms() - t;
                    }
                    None
                }
                Request::MsgBox { prompt, buttons, title, reply } => Some(Pending::MsgBox { prompt, buttons, title, reply }),
                Request::InputBox { prompt, title, default, reply } => Some(Pending::InputBox { prompt, title, text: default, reply }),
                Request::WaitEvent(reply) => Some(Pending::Wait(reply)),
                Request::Print(t) => {
                    app.macros.immediate.push(t);
                    if app.macros.immediate.len() > 1000 {
                        app.macros.immediate.drain(..500);
                    }
                    None
                }
                Request::Done(err) => {
                    if let Some(w) = app.macros.workers.get_mut(&uid) {
                        w.busy = false;
                        w.pending = None;
                        if std::env::var_os("GRIDCRAFT_PROFILE").is_some() {
                            log::info!(
                                "macro {}: {} calls, {:.0} ms serving, {:.0} ms wall",
                                w.running,
                                w.calls,
                                w.serve_ms,
                                crate::now_ms() - w.started
                            );
                        }
                    }
                    if let Some(e) = err {
                        app.macros.user_events.remove(&uid);
                        app.message = Some(("Macro error".into(), run_time_error(&e)));
                    } else if let Some(next) = app.macros.user_events.get_mut(&uid).and_then(VecDeque::pop_front) {
                        // Actions on a modeless form that arrived while a handler ran.
                        event(app, uid, next);
                    }
                    break;
                }
            };
            if let Some(p) = pending
                && let Some(w) = app.macros.workers.get_mut(&uid)
            {
                w.pending = Some(p);
            }
        }
    }
    // A macro is running or waiting on a dialog or a form: keep frames coming.
    if app.macros.workers.values().any(|w| w.busy) {
        ctx.request_repaint_after(std::time::Duration::from_millis(30));
    }
}

/// Excel's wording for run-time errors.
fn run_time_error(e: &VbaError) -> String {
    match e {
        VbaError::Runtime { number, description } => format!("Run-time error '{number}':\n\n{description}"),
        VbaError::Syntax { line, msg } => format!("Compile error on line {line}:\n\n{msg}"),
        e => e.to_string(),
    }
}

/// `MsgBox` and `InputBox` dialogs of running macros.
pub fn dialogs(app: &mut SheetApp, ctx: &egui::Context) {
    let uids: Vec<u64> = app.macros.workers.keys().copied().collect();
    for uid in uids {
        let Some(w) = app.macros.workers.get_mut(&uid) else { continue };
        let mut answer: Option<i64> = None;
        let mut input: Option<Option<String>> = None;
        match &mut w.pending {
            Some(Pending::MsgBox { prompt, buttons, title, .. }) => {
                let (prompt, title, buttons) = (prompt.clone(), title.clone(), *buttons);
                egui::Modal::new(egui::Id::new(("vba_msgbox", uid))).show(ctx, |ui| {
                    ui.set_min_width(300.0);
                    ui.set_max_width(520.0);
                    ui.label(egui::RichText::new(&title).strong());
                    ui.add_space(6.0);
                    ui.horizontal(|ui| {
                        // vbCritical 16, vbQuestion 32, vbExclamation 48, vbInformation 64.
                        let icon = match buttons & 0x70 {
                            16 => Some(("×", Color32::from_rgb(0xC4, 0x2B, 0x1C))),
                            32 => Some(("?", Color32::from_rgb(0x1F, 0x5F, 0xC9))),
                            48 => Some(("!", Color32::from_rgb(0xF2, 0xC3, 0x2E))),
                            64 => Some(("i", Color32::from_rgb(0x1F, 0x5F, 0xC9))),
                            _ => None,
                        };
                        if let Some((glyph, color)) = icon {
                            let (r, _) = ui.allocate_exact_size(vec2(32.0, 32.0), Sense::hover());
                            ui.painter().circle_filled(r.center(), 14.0, color);
                            ui.painter().text(r.center(), Align2::CENTER_CENTER, glyph, theme::ui_bold(18.0), Color32::WHITE);
                            ui.add_space(6.0);
                        }
                        ui.add(egui::Label::new(egui::RichText::new(&prompt).font(theme::ui_font(14.0))).wrap());
                    });
                    ui.add_space(10.0);
                    let set: &[(&str, i64)] = match buttons & 7 {
                        1 => &[("OK", 1), ("Cancel", 2)],
                        2 => &[("Abort", 3), ("Retry", 4), ("Ignore", 5)],
                        3 => &[("Yes", 6), ("No", 7), ("Cancel", 2)],
                        4 => &[("Yes", 6), ("No", 7)],
                        5 => &[("Retry", 4), ("Cancel", 2)],
                        _ => &[("OK", 1)],
                    };
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        for (label, v) in set.iter().rev() {
                            if ui.button(format!("  {label}  ")).clicked() {
                                answer = Some(*v);
                            }
                        }
                    });
                    if ui.input(|i| i.key_pressed(Key::Enter)) {
                        answer = set.first().map(|(_, v)| *v);
                    }
                    if ui.input(|i| i.key_pressed(Key::Escape)) && set.iter().any(|(_, v)| *v == 2) {
                        answer = Some(2);
                    }
                });
            }
            Some(Pending::InputBox { prompt, title, text, .. }) => {
                egui::Modal::new(egui::Id::new(("vba_inputbox", uid))).show(ctx, |ui| {
                    ui.set_width(380.0);
                    ui.label(egui::RichText::new(if title.is_empty() { "GridCraft" } else { title.as_str() }).strong());
                    ui.add_space(6.0);
                    ui.add(egui::Label::new(prompt.as_str()).wrap());
                    let r = ui.add(egui::TextEdit::singleline(text).desired_width(f32::INFINITY));
                    r.request_focus();
                    ui.add_space(8.0);
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.button("Cancel").clicked() || ui.input(|i| i.key_pressed(Key::Escape)) {
                            input = Some(None);
                        }
                        if ui.button("  OK  ").clicked() || ui.input(|i| i.key_pressed(Key::Enter)) {
                            input = Some(Some(text.clone()));
                        }
                    });
                });
            }
            _ => {}
        }
        if let Some(v) = answer
            && let Some(Pending::MsgBox { reply, .. }) = w.pending.take()
        {
            let _ = reply.send(v);
        }
        if let Some(v) = input
            && let Some(Pending::InputBox { reply, .. }) = w.pending.take()
        {
            let _ = reply.send(v);
        }
    }
}

// ---------------------------------------------------------------- keys

/// An `OnKey` key code (`^{RETURN}`, `^j`, `+{F2}`) as modifiers and a key.
fn parse_key(code: &str) -> Option<(bool, bool, bool, Key)> {
    let (mut ctrl, mut shift, mut alt) = (false, false, false);
    let mut rest = code;
    loop {
        match rest.chars().next() {
            Some('^') => ctrl = true,
            Some('+') => shift = true,
            Some('%') => alt = true,
            _ => break,
        }
        rest = &rest[1..];
    }
    let key = match rest.trim_matches(['{', '}']).to_ascii_uppercase().as_str() {
        "RETURN" | "ENTER" | "~" => Key::Enter,
        "TAB" => Key::Tab,
        "ESC" | "ESCAPE" => Key::Escape,
        "DELETE" | "DEL" => Key::Delete,
        "BACKSPACE" | "BS" => Key::Backspace,
        "UP" => Key::ArrowUp,
        "DOWN" => Key::ArrowDown,
        "LEFT" => Key::ArrowLeft,
        "RIGHT" => Key::ArrowRight,
        "HOME" => Key::Home,
        "END" => Key::End,
        "F1" => Key::F1,
        "F2" => Key::F2,
        "F3" => Key::F3,
        "F4" => Key::F4,
        "F5" => Key::F5,
        "F6" => Key::F6,
        "F7" => Key::F7,
        "F8" => Key::F8,
        "F9" => Key::F9,
        "F10" => Key::F10,
        "F11" => Key::F11,
        "F12" => Key::F12,
        one if one.chars().count() == 1 => {
            let c = one.chars().next()?;
            // An upper-case letter in a recorded shortcut means Shift.
            if rest.chars().next().is_some_and(char::is_uppercase) {
                shift = true;
            }
            Key::from_name(&c.to_string())?
        }
        _ => return None,
    };
    Some((ctrl, shift, alt, key))
}

/// A key bound to a macro (`OnKey`, or a recorded macro's Ctrl+letter): runs it. Returns
/// whether the key was taken.
pub fn key(app: &mut SheetApp, key: Key, m: Modifiers) -> bool {
    let Some(uid) = evba::active_uid(&app.session) else { return false };
    if !evba::enabled(&app.session, uid) {
        return false;
    }
    let mut bindings: Vec<(String, String)> = app.session.macros.doc(uid).map(|d| d.on_key.clone()).unwrap_or_default();
    bindings.extend(evba::shortcuts(&mut app.session, uid));
    let ctrl = m.ctrl || m.command;
    for (code, proc) in bindings {
        if let Some((c, s, a, k)) = parse_key(&code)
            && k == key
            && c == ctrl
            && s == m.shift
            && a == m.alt
        {
            run(app, &proc);
            return true;
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn on_key_codes() {
        assert_eq!(parse_key("^{RETURN}"), Some((true, false, false, Key::Enter)));
        assert_eq!(parse_key("^j"), Some((true, false, false, Key::J)));
        assert_eq!(parse_key("^J"), Some((true, true, false, Key::J)));
        assert_eq!(parse_key("+{F2}"), Some((false, true, false, Key::F2)));
        assert_eq!(parse_key("{NOPE}"), None);
    }
}
