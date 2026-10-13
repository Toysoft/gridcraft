//! Macros: a workbook's VBA project ([`gridcraft_vba`]) run against GridCraft's Excel object
//! model ([`model`]) and UserForms ([`forms`]).
//!
//! Macros are off until the user enables them for the workbook (Excel's "Enable Content"):
//! [`enable`]. A run happens through a [`gridcraft_vba::Host`]: [`DirectHost`] runs on the
//! calling thread (CLI, MCP, tests: message boxes are logged and answered with OK, a modal form
//! closes when there is nothing left to do), and the desktop app runs macros on a worker thread
//! whose host forwards each object-model call to the UI thread's session.

pub mod forms;
pub mod model;

use std::collections::{HashMap, VecDeque};
use std::sync::Arc;

use gridcraft_vba::{Event, HostArg, HostObj, Interp, MODAL, Object, Value, VbaError};
use gridcraft_xlsx::VbaContents;
use serde_json::{Value as Json, json};

use crate::Session;
use forms::FormState;

type R<T> = Result<T, VbaError>;

/// A workbook's macro state for the session.
#[derive(Debug, Default)]
pub struct MacroDoc {
    pub contents: Option<Arc<VbaContents>>,
    /// Why the project couldn't be read.
    pub load_error: Option<String>,
    /// The user enabled macros for this workbook.
    pub enabled: bool,
    /// Loaded UserForms.
    pub forms: Vec<FormState>,
    /// Events waiting for the program (form loads, controls changed by code, user actions).
    pub events: VecDeque<Event>,
    /// `Application.OnKey` bindings: key code → macro (`None` restores the key).
    pub on_key: Vec<(String, String)>,
    /// Message boxes shown by headless runs.
    pub messages: Vec<String>,
    shown: u64,
    /// The unloaded form whose `Terminate` event was handed out last (its `Me` stays valid).
    terminating: Option<String>,
    /// Sheet controls' current values: (sheet, control) in lower case → value.
    pub sheet_values: HashMap<(String, String), String>,
}

/// Every open workbook's macro state, by document uid.
#[derive(Debug, Default)]
pub struct MacroState {
    docs: HashMap<u64, MacroDoc>,
    /// The last `Range.Find` (what `FindNext` repeats).
    pub last_find: Option<Json>,
}

impl MacroState {
    pub fn doc(&self, uid: u64) -> Option<&MacroDoc> {
        self.docs.get(&uid)
    }
    pub fn doc_mut(&mut self, uid: u64) -> &mut MacroDoc {
        self.docs.entry(uid).or_default()
    }
}

/// The active workbook's uid.
pub fn active_uid(s: &Session) -> Option<u64> {
    s.active().map(|d| d.uid)
}

/// The workbook's VBA project, read on first use (`None`: it has no macros or they can't be
/// read; the reason is in `load_error`).
pub fn contents(s: &mut Session, uid: u64) -> Option<Arc<VbaContents>> {
    if let Some(c) = s.macros.doc(uid).and_then(|d| d.contents.clone()) {
        return Some(c);
    }
    let bytes = s.documents().iter().find(|d| d.uid == uid)?.wb.vba_project.clone()?;
    match gridcraft_xlsx::read_vba(&bytes) {
        Ok(c) => {
            let c = Arc::new(c);
            s.macros.doc_mut(uid).contents = Some(c.clone());
            Some(c)
        }
        Err(e) => {
            s.macros.doc_mut(uid).load_error = Some(e);
            None
        }
    }
}

/// Whether macros may run in the workbook.
pub fn enabled(s: &Session, uid: u64) -> bool {
    s.macros.doc(uid).is_some_and(|d| d.enabled)
}

pub fn enable(s: &mut Session, uid: u64, on: bool) {
    s.macros.doc_mut(uid).enabled = on;
}

/// A fresh interpreter for the workbook's project.
pub fn interpreter(s: &mut Session, uid: u64) -> R<Interp> {
    let c = contents(s, uid).ok_or_else(|| {
        let why = s.macros.doc(uid).and_then(|d| d.load_error.clone()).unwrap_or_else(|| "This workbook has no macros.".into());
        VbaError::runtime(1004, why)
    })?;
    Ok(Interp::new(&c.project))
}

// ---------------------------------------------------------------- object-model glue

/// The sheet whose VBA code name is `name`.
pub fn sheet_by_code_name(s: &Session, uid: u64, name: &str) -> Option<String> {
    let wb = &s.documents().iter().find(|d| d.uid == uid)?.wb;
    wb.sheets.iter().find(|sh| sh.code_name.as_deref().is_some_and(|c| c.eq_ignore_ascii_case(name))).map(|sh| sh.name.clone())
}

pub fn code_name_of(s: &Session, uid: u64, sheet: &str) -> Option<String> {
    let wb = &s.documents().iter().find(|d| d.uid == uid)?.wb;
    wb.sheets.iter().find(|sh| sh.name.eq_ignore_ascii_case(sheet)).and_then(|sh| sh.code_name.clone())
}

/// Recorded macros' keyboard shortcuts (`Attribute Macro1.VB_ProcData.VB_Invoke_Func =
/// "j\n14"` is Ctrl+J), as `OnKey` codes (`^j`, `^J` for Ctrl+Shift+J) with the macro.
pub fn shortcuts(s: &mut Session, uid: u64) -> Vec<(String, String)> {
    let Some(c) = contents(s, uid) else { return vec![] };
    let mut out = Vec::new();
    for m in &c.project.modules {
        for line in m.code.lines() {
            let Some(rest) = line.trim().strip_prefix("Attribute ") else { continue };
            let Some((target, value)) = rest.split_once('=') else { continue };
            let Some(proc) = target.trim().strip_suffix(".VB_ProcData.VB_Invoke_Func") else { continue };
            let key = value.trim().trim_matches('"').chars().next().filter(|c| c.is_ascii_alphabetic());
            if let Some(k) = key {
                out.push((format!("^{k}"), proc.to_string()));
            }
        }
    }
    out
}

pub fn set_on_key(s: &mut Session, uid: u64, key: &str, procedure: Option<String>) {
    let d = s.macros.doc_mut(uid);
    d.on_key.retain(|(k, _)| !k.eq_ignore_ascii_case(key));
    if let Some(p) = procedure.filter(|p| !p.is_empty()) {
        d.on_key.push((key.to_string(), p));
    }
}

/// The form called `name`, loading it (and queueing `UserForm_Initialize`) on first use.
pub fn form_object(s: &mut Session, uid: u64, name: &str) -> R<Option<Value>> {
    if load_form(s, uid, name)?.is_none() {
        return Ok(None);
    }
    Ok(Some(model::form_obj(name)))
}

/// Loads a form if the project has one by that name: its index in the loaded forms.
fn load_form(s: &mut Session, uid: u64, name: &str) -> R<Option<usize>> {
    if let Some(i) = s.macros.doc(uid).and_then(|d| d.forms.iter().position(|f| f.name().eq_ignore_ascii_case(name))) {
        let d = s.macros.doc_mut(uid);
        let zombie = d.forms.get(i).is_some_and(|f| f.unloaded);
        if !zombie || d.terminating.as_deref().is_some_and(|t| t.eq_ignore_ascii_case(name)) {
            return Ok(Some(i));
        }
        // Referenced again after it was unloaded: a fresh instance.
        d.forms.remove(i);
    }
    let Some(c) = contents(s, uid) else { return Ok(None) };
    let Some(design) = c.forms.iter().find(|f| f.name.eq_ignore_ascii_case(name)) else { return Ok(None) };
    let mut form = FormState::new(design);
    // List boxes bound to a sheet range fill from it.
    for ctl in form.controls.iter_mut() {
        if !ctl.design.row_source.is_empty() {
            ctl.list = row_source(s, uid, &ctl.design.row_source.clone());
        }
    }
    let form_name = design.name.clone();
    let d = s.macros.doc_mut(uid);
    d.forms.push(form);
    d.events.push_back(Event { module: form_name, proc: "UserForm_Initialize".into(), args: vec![] });
    Ok(Some(d.forms.len() - 1))
}

/// A list's rows from a `RowSource` range (`Sheet1!A1:B10`, a name).
fn row_source(s: &Session, uid: u64, src: &str) -> Vec<Vec<String>> {
    let Some(d) = s.documents().iter().find(|d| d.uid == uid) else { return vec![] };
    let wb = &d.wb;
    let Ok((si, areas)) = model::parse_ref(wb, wb.active_sheet, src, 0) else { return vec![] };
    let Some(sh) = wb.sheet(si) else { return vec![] };
    let Some(r) = areas.first() else { return vec![] };
    let r = match sh.used_range() {
        Some(u) if r.is_full_cols() => r.intersection(&u).unwrap_or(*r),
        _ => *r,
    };
    if r.count() > 2_000_000 {
        return vec![];
    }
    (r.start.row..=r.end.row)
        .map(|row| (r.start.col..=r.end.col).map(|col| crate::display::cell_text(wb, sh, gridcraft_core::CellRef::new(row, col))).collect())
        .collect()
}

fn form_mut<'a>(s: &'a mut Session, uid: u64, name: &str) -> R<&'a mut FormState> {
    s.macros
        .doc_mut(uid)
        .forms
        .iter_mut()
        .find(|f| f.name().eq_ignore_ascii_case(name))
        .ok_or_else(|| VbaError::runtime(424, format!("Object required: {name}")))
}

/// Members of a UserForm.
pub fn form_get(s: &mut Session, uid: u64, name: &str, member: &str, args: &[HostArg]) -> R<Value> {
    load_form(s, uid, name)?;
    let m = member.to_ascii_lowercase();
    match m.as_str() {
        "show" => {
            let modeless = args.first().is_some_and(|a| matches!(a.value.to_int(), Ok(0)));
            let d = s.macros.doc_mut(uid);
            d.shown += 1;
            let order = d.shown;
            let f = form_mut(s, uid, name)?;
            let was = f.visible;
            f.visible = true;
            f.modal = !modeless;
            f.shown_order = order;
            if !was {
                let fname = f.name().to_string();
                s.macros.doc_mut(uid).events.push_back(Event { module: fname, proc: "UserForm_Activate".into(), args: vec![] });
            }
            if modeless {
                return Ok(Value::Empty);
            }
            Ok(Value::Object(Object::Host(HostObj::new(MODAL, name.to_string()))))
        }
        "hide" => {
            let f = form_mut(s, uid, name)?;
            f.visible = false;
            f.modal = false;
            Ok(Value::Empty)
        }
        "_unload" => {
            unload(s, uid, name);
            Ok(Value::Empty)
        }
        "_load" | "repaint" | "setfocus" | "printform" => Ok(Value::Empty),
        "caption" => Ok(Value::from(form_mut(s, uid, name)?.caption.clone())),
        "width" | "insidewidth" => Ok(Value::Double(f64::from(form_mut(s, uid, name)?.width))),
        "height" | "insideheight" => Ok(Value::Double(f64::from(form_mut(s, uid, name)?.height))),
        "visible" => Ok(Value::Bool(form_mut(s, uid, name)?.visible)),
        "name" => Ok(Value::from(form_mut(s, uid, name)?.name().to_string())),
        "activecontrol" => {
            let f = form_mut(s, uid, name)?;
            match f.focus.and_then(|i| f.controls.get(i)) {
                Some(c) => Ok(model::control_obj(name, c.name())),
                None => Ok(Value::Nothing),
            }
        }
        "controls" => match args.first() {
            Some(a) => {
                let f = form_mut(s, uid, name)?;
                let ctl = match &a.value {
                    Value::Str(n) => f.controls.iter().find(|c| c.name().eq_ignore_ascii_case(n)),
                    v => usize::try_from(v.to_int()?).ok().and_then(|i| f.controls.get(i)),
                };
                ctl.map(|c| model::control_obj(name, c.name())).ok_or_else(|| VbaError::runtime(-2147024809, "Could not find the specified object."))
            }
            None => Ok(model::form_obj(name)),
        },
        "count" => Ok(Value::Int(form_mut(s, uid, name)?.controls.len() as i64)),
        _ => {
            // A control by name (`Me.TextBox1`, `UserForm1.ListBox1`).
            let f = form_mut(s, uid, name)?;
            if f.control(member).is_some() {
                return Ok(model::control_obj(name, member));
            }
            Err(VbaError::runtime(438, format!("Object doesn't support this property or method: {name}.{member}")))
        }
    }
}

pub fn form_set(s: &mut Session, uid: u64, name: &str, member: &str, v: Value) -> R<()> {
    load_form(s, uid, name)?;
    let f = form_mut(s, uid, name)?;
    match member.to_ascii_lowercase().as_str() {
        "caption" => f.caption = v.to_text()?,
        "width" => f.width = v.to_num()? as f32,
        "height" => f.height = v.to_num()? as f32,
        "backcolor" => {
            let n = v.to_int()? as u32;
            f.back = Some(((n & 0xFF) << 16) | (n & 0xFF00) | ((n >> 16) & 0xFF));
        }
        "visible" => f.visible = v.to_bool()?,
        "left" | "top" | "startupposition" | "scrollbars" | "scrolltop" | "scrollheight" => {}
        _ => {
            // `Me.TextBox1 = x` with the control's default member.
            if f.control(member).is_some() {
                return control_member(s, uid, name, member, "Value", &[], Some(v)).map(|_| ());
            }
            return Err(VbaError::runtime(438, format!("Object doesn't support this property or method: {name}.{member}")));
        }
    }
    Ok(())
}

/// Closes a form. Its `UserForm_Terminate` runs from the event queue, with `Me` still the
/// (now hidden) form.
pub fn unload(s: &mut Session, uid: u64, name: &str) {
    let d = s.macros.doc_mut(uid);
    if let Some(f) = d.forms.iter_mut().find(|f| f.name().eq_ignore_ascii_case(name) && !f.unloaded) {
        f.unloaded = true;
        f.visible = false;
        f.modal = false;
        let module = f.name().to_string();
        d.events.push_back(Event { module, proc: "UserForm_Terminate".into(), args: vec![] });
    }
}

/// A control member read, write or method call.
pub fn control_member(s: &mut Session, uid: u64, form: &str, name: &str, member: &str, args: &[HostArg], value: Option<Value>) -> R<Value> {
    load_form(s, uid, form)?;
    // Setting `RowSource` fills the list from the sheet.
    if member.eq_ignore_ascii_case("rowsource")
        && let Some(v) = &value
    {
        let src = v.to_text()?;
        let rows = row_source(s, uid, &src);
        let f = form_mut(s, uid, form)?;
        if let Some(c) = f.control(name).and_then(|i| f.controls.get_mut(i)) {
            c.design.row_source = src;
            c.list = rows;
            c.list_index = -1;
        }
        return Ok(Value::Empty);
    }
    let f = form_mut(s, uid, form)?;
    let (v, events) = model::control_access(f, name, member, args, value)?;
    s.macros.doc_mut(uid).events.extend(events);
    Ok(v)
}

pub fn form_controls(s: &mut Session, name: &str) -> R<Vec<Value>> {
    let uid = active_uid(s).unwrap_or(0);
    let f = form_mut(s, uid, name)?;
    Ok(f.controls.iter().map(|c| model::control_obj(name, c.name())).collect())
}

fn sheet_control<'a>(s: &'a Session, uid: u64, sheet: &str, name: &str) -> Option<&'a gridcraft_model::SheetControl> {
    let wb = &s.documents().iter().find(|d| d.uid == uid)?.wb;
    let si = wb.sheet_index(sheet)?;
    wb.sheet(si)?.controls.iter().find(|c| c.name.eq_ignore_ascii_case(name))
}

pub fn sheet_has_control(s: &Session, uid: u64, si: usize, name: &str) -> bool {
    s.documents()
        .iter()
        .find(|d| d.uid == uid)
        .and_then(|d| d.wb.sheet(si))
        .is_some_and(|sh| sh.controls.iter().any(|c| c.name.eq_ignore_ascii_case(name)))
}

/// A sheet control's current value (what the user or a macro picked, else as saved).
pub fn sheet_control_value(s: &Session, uid: u64, sheet: &str, name: &str) -> String {
    let key = (sheet.to_ascii_lowercase(), name.to_ascii_lowercase());
    if let Some(v) = s.macros.doc(uid).and_then(|d| d.sheet_values.get(&key)) {
        return v.clone();
    }
    sheet_control(s, uid, sheet, name).map(|c| c.value.clone()).unwrap_or_default()
}

/// The items of a sheet control's list (`ListFillRange`).
pub fn sheet_control_list(s: &Session, uid: u64, sheet: &str, name: &str) -> Vec<String> {
    let Some(c) = sheet_control(s, uid, sheet, name) else { return vec![] };
    let Some(src) = c.list_fill_range.clone() else { return vec![] };
    row_source(s, uid, &src).into_iter().filter_map(|r| r.into_iter().next()).filter(|t| !t.is_empty()).collect()
}

/// The user (or a macro) set a sheet control's value: it goes to the linked cell, and the
/// control's `Change` event is queued on the sheet's module.
pub fn sheet_control_set_value(s: &mut Session, uid: u64, sheet: &str, name: &str, value: String) -> R<()> {
    let Some(c) = sheet_control(s, uid, sheet, name).cloned() else {
        return Err(VbaError::runtime(438, format!("Object doesn't support this property or method: {name}")));
    };
    let key = (sheet.to_ascii_lowercase(), name.to_ascii_lowercase());
    let old = sheet_control_value(s, uid, sheet, name);
    s.macros.doc_mut(uid).sheet_values.insert(key, value.clone());
    if let Some(link) = &c.linked_cell
        && let Some(d) = s.documents().iter().find(|d| d.uid == uid)
        && let Some(si) = d.wb.sheet_index(sheet)
        && let Ok((lsi, areas)) = model::parse_ref(&d.wb, si, link, 0)
        && let Some(r) = areas.first()
    {
        let at = r.start;
        model::put_value(s, uid, lsi, at, Value::from(value.clone()))?;
    }
    if old != value {
        let module = code_name_of(s, uid, sheet).unwrap_or_else(|| sheet.to_string());
        s.macros.doc_mut(uid).events.push_back(Event { module, proc: format!("{}_Change", c.name), args: vec![] });
    }
    Ok(())
}

pub fn sheet_control_get(s: &mut Session, uid: u64, sheet: &str, name: &str, member: &str, args: &[HostArg]) -> R<Value> {
    let c = sheet_control(s, uid, sheet, name).cloned().ok_or_else(|| VbaError::runtime(424, format!("Object required: {name}")))?;
    let value = sheet_control_value(s, uid, sheet, name);
    match member.to_ascii_lowercase().as_str() {
        "value" | "text" | "_default" => Ok(Value::from(value)),
        "name" => Ok(Value::from(c.name)),
        "caption" => Ok(Value::from(c.caption)),
        "linkedcell" => Ok(Value::from(c.linked_cell.unwrap_or_default())),
        "listfillrange" => Ok(Value::from(c.list_fill_range.unwrap_or_default())),
        "listcount" => Ok(Value::Int(sheet_control_list(s, uid, sheet, name).len() as i64)),
        "listindex" => Ok(Value::Int(sheet_control_list(s, uid, sheet, name).iter().position(|x| *x == value).map_or(-1, |i| i as i64))),
        "list" => {
            let list = sheet_control_list(s, uid, sheet, name);
            match args.first() {
                Some(a) => Ok(usize::try_from(a.value.to_int()?).ok().and_then(|i| list.get(i)).map_or(Value::Null, |t| Value::from(t.clone()))),
                None => Ok(Value::Array(std::sync::Arc::new(gridcraft_vba::Array::from_vec(0, list.into_iter().map(Value::from).collect())))),
            }
        }
        "enabled" | "visible" => Ok(Value::Bool(true)),
        "object" => Ok(model::obj(model::XlObj::SheetControl { doc: uid, sheet: sheet.to_string(), name: name.to_string() })),
        "select" | "activate" | "setfocus" => Ok(Value::Empty),
        _ => Err(VbaError::runtime(438, format!("Object doesn't support this property or method: {name}.{member}"))),
    }
}

pub fn sheet_control_set(s: &mut Session, uid: u64, sheet: &str, name: &str, member: &str, v: Value) -> R<()> {
    match member.to_ascii_lowercase().as_str() {
        "value" | "text" | "_default" => {
            let t = if matches!(v, Value::Null | Value::Empty) { String::new() } else { v.to_text()? };
            sheet_control_set_value(s, uid, sheet, name, t)
        }
        "listindex" => {
            let list = sheet_control_list(s, uid, sheet, name);
            let i = v.to_int()?;
            let t = usize::try_from(i).ok().and_then(|i| list.get(i)).cloned().unwrap_or_default();
            sheet_control_set_value(s, uid, sheet, name, t)
        }
        "enabled" | "visible" | "listfillrange" | "linkedcell" => Ok(()),
        _ => Err(VbaError::runtime(438, format!("Object doesn't support this property or method: {name}.{member}"))),
    }
}

/// The next queued event of the workbook. Unloaded forms whose `Terminate` has been handed out
/// are dropped once the next event is taken.
pub fn take_event(s: &mut Session, uid: u64) -> Option<Event> {
    let d = s.macros.doc_mut(uid);
    if let Some(t) = d.terminating.take() {
        d.forms.retain(|f| !(f.unloaded && f.name().eq_ignore_ascii_case(&t)));
    }
    let ev = d.events.pop_front()?;
    if ev.proc.eq_ignore_ascii_case("UserForm_Terminate") {
        d.terminating = Some(ev.module.clone());
    }
    Some(ev)
}

/// Whether the modal form `name` is still showing.
pub fn modal_open(s: &Session, uid: u64, name: &str) -> bool {
    s.macros.doc(uid).is_some_and(|d| d.forms.iter().any(|f| f.name().eq_ignore_ascii_case(name) && f.visible && f.modal && !f.unloaded))
}

// ---------------------------------------------------------------- running on this thread

/// What a headless run does about dialogs.
pub trait MacroUi {
    fn msgbox(&mut self, prompt: &str, buttons: i64, title: &str) -> i64;
    fn inputbox(&mut self, prompt: &str, title: &str, default: &str) -> Option<String>;
    /// While a modal form is open: the next user event, or `None` to close it.
    fn wait_event(&mut self, s: &mut Session, uid: u64) -> Option<Event>;
    fn should_stop(&mut self) -> bool {
        false
    }
}

/// No user: message boxes answer OK (and are logged), input boxes give their default, modal
/// forms close straight away unless events were scripted.
#[derive(Debug, Default)]
pub struct HeadlessUi {
    pub messages: Vec<String>,
    /// Events to deliver while modal forms are open, in order.
    pub script: VecDeque<Event>,
}

impl MacroUi for HeadlessUi {
    fn msgbox(&mut self, prompt: &str, buttons: i64, _title: &str) -> i64 {
        self.messages.push(prompt.to_string());
        // vbOK, vbYes for Yes/No boxes.
        if buttons & 7 == 4 || buttons & 7 == 3 { 6 } else { 1 }
    }
    fn inputbox(&mut self, _p: &str, _t: &str, default: &str) -> Option<String> {
        Some(default.to_string())
    }
    fn wait_event(&mut self, _s: &mut Session, _uid: u64) -> Option<Event> {
        self.script.pop_front()
    }
}

/// Runs the object model on the calling thread.
pub struct DirectHost<'a> {
    pub s: &'a mut Session,
    /// The workbook whose macros run (`ThisWorkbook`).
    pub uid: u64,
    pub ui: &'a mut dyn MacroUi,
}

fn local_now() -> f64 {
    gridcraft_core::date::unix_now_ms().map(gridcraft_core::date::serial_from_unix_ms).unwrap_or(0.0)
}

impl gridcraft_vba::Host for DirectHost<'_> {
    fn global(&mut self, name: &str, args: &[HostArg]) -> R<Option<Value>> {
        model::global(self.s, self.uid, name, args)
    }
    fn get(&mut self, obj: &HostObj, member: &str, args: &[HostArg]) -> R<Value> {
        dispatch_get(self.s, self.uid, obj, member, args)
    }
    fn set(&mut self, obj: &HostObj, member: &str, args: &[HostArg], value: Value) -> R<()> {
        model::set(self.s, self.uid, obj, member, args, value)
    }
    fn items(&mut self, obj: &HostObj) -> R<Vec<Value>> {
        model::items(self.s, obj)
    }
    fn msgbox(&mut self, prompt: &str, buttons: i64, title: &str) -> R<i64> {
        self.s.macros.doc_mut(self.uid).messages.push(prompt.to_string());
        Ok(self.ui.msgbox(prompt, buttons, title))
    }
    fn inputbox(&mut self, prompt: &str, title: &str, default: &str) -> R<Option<String>> {
        Ok(self.ui.inputbox(prompt, title, default))
    }
    fn now(&mut self) -> f64 {
        local_now()
    }
    fn take_event(&mut self) -> Option<Event> {
        take_event(self.s, self.uid)
    }
    fn wait_event(&mut self) -> R<Option<Event>> {
        // The innermost modal form: wait for its events while it shows.
        let open = self
            .s
            .macros
            .doc(self.uid)
            .and_then(|d| d.forms.iter().filter(|f| f.visible && f.modal).max_by_key(|f| f.shown_order).map(|f| f.name().to_string()));
        let Some(form) = open else { return Ok(None) };
        match self.ui.wait_event(self.s, self.uid) {
            Some(ev) => Ok(Some(ev)),
            None => {
                // Nobody to interact with: the form closes.
                if let Ok(f) = form_mut(self.s, self.uid, &form) {
                    f.visible = false;
                    f.modal = false;
                }
                Ok(None)
            }
        }
    }
    fn should_stop(&mut self) -> bool {
        self.ui.should_stop()
    }
}

/// `get` with the lines collections (`Rows`, `Columns`) the model hands out besides `XlObj`s.
pub fn dispatch_get(s: &mut Session, uid: u64, obj: &HostObj, member: &str, args: &[HostArg]) -> R<Value> {
    if let Some(l) = obj.get::<model::LinesObj>() {
        return model::lines_get(s, uid, l, member, args);
    }
    model::get(s, uid, obj, member, args)
}

/// Runs a macro on this thread. Errors come back as text with the VBA error number.
pub fn run_headless(s: &mut Session, uid: u64, name: &str, ui: &mut dyn MacroUi) -> R<Value> {
    let mut it = interpreter(s, uid)?;
    let mut host = DirectHost { s, uid, ui };
    let r = it.run(&mut host, name, vec![]);
    r.map_err(|e| with_location(e, &it))
}

/// The error with where it happened (`… (Module1.Macro2, line 12)`).
pub fn with_location(e: VbaError, it: &Interp) -> VbaError {
    match (e, &it.error_at) {
        (VbaError::Runtime { number, description }, Some((m, p, line))) => {
            VbaError::Runtime { number, description: format!("{description} ({m}.{p}, line {line})") }
        }
        (e, _) => e,
    }
}

// ---------------------------------------------------------------- commands

pub(crate) fn macro_names(s: &mut Session, uid: u64) -> Vec<String> {
    match interpreter(s, uid) {
        Ok(it) => it.macros(),
        Err(_) => vec![],
    }
}

/// `developer.runMacro`: runs a macro headless (the desktop app runs it on its macro thread).
pub(crate) fn run_command(s: &mut Session, p: &Json) -> crate::Result<Json> {
    let name = p.get("name").and_then(Json::as_str).ok_or_else(|| crate::cmd::bad("developer.runMacro", "missing `name`"))?.to_string();
    let uid = active_uid(s).ok_or(crate::EngineError::NoDocument)?;
    if !enabled(s, uid) && p.get("enable").and_then(Json::as_bool) != Some(true) {
        return Err(crate::EngineError::Other("Macros in this workbook are disabled. Enable content (developer.enableMacros) to run them.".into()));
    }
    enable(s, uid, true);
    let mut ui = HeadlessUi::default();
    let r = run_headless(s, uid, &name, &mut ui);
    let messages = ui.messages;
    match r {
        Ok(_) => Ok(json!({"ran": name, "messages": messages})),
        Err(e) => Err(crate::EngineError::Other(format!("{e} (in {name})"))),
    }
}

pub(crate) fn source_command(s: &mut Session, p: &Json) -> crate::Result<Json> {
    let uid = active_uid(s).ok_or(crate::EngineError::NoDocument)?;
    let c = contents(s, uid).ok_or_else(|| crate::EngineError::Other("This workbook has no macros.".into()))?;
    let want = p.get("module").and_then(Json::as_str);
    let modules: Vec<Json> = c
        .project
        .modules
        .iter()
        .filter(|m| want.is_none_or(|w| m.name.eq_ignore_ascii_case(w)))
        .map(|m| {
            let kind = match m.kind {
                gridcraft_vba::ovba::ModuleKind::Standard => "module",
                gridcraft_vba::ovba::ModuleKind::Class => "class",
                gridcraft_vba::ovba::ModuleKind::Document => "document",
                gridcraft_vba::ovba::ModuleKind::Form => "form",
            };
            // Attribute lines are VBA's own bookkeeping; the editor doesn't show them.
            let code: String = m.code.lines().filter(|l| !l.starts_with("Attribute VB_")).collect::<Vec<_>>().join("\n");
            json!({"name": m.name, "kind": kind, "code": code})
        })
        .collect();
    Ok(json!({"modules": modules, "forms": c.forms.iter().map(|f| f.name.clone()).collect::<Vec<_>>()}))
}

#[cfg(test)]
mod tests;
