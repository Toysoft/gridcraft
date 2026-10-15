//! Scripts with logic (Automate tab).
//!
//! Scripts are written in [Rhai](https://rhai.rs), a small embeddable language implemented in
//! pure Rust (so it builds for wasm32 too). A script sees an object model shaped like Excel's
//! Office Scripts API, so what people know from there carries over:
//!
//! ```text
//! fn main(workbook) {
//!     let sheet = workbook.getActiveWorksheet();
//!     sheet.getRange("A1:B2").setValues([[1, 2], [3, 4]]);
//!     sheet.getRange("A1:B1").getFormat().getFont().setBold(true);
//!     run("home.fillColor", #{range: "A1:B1", color: "#FFFF00"});   // any GridCraft command
//!     console.log("done");
//! }
//! ```
//!
//! How a run works:
//! - Every change goes through engine commands ([`Session::execute`]), so sheet protection,
//!   recalculation and the journal see a script's edits like the user's own. The commands of one
//!   run collapse into a single undo step. Reads come straight from the workbook model.
//! - The session is lent to the script for the duration of the run: it is moved into shared
//!   ownership that the object model's methods reach, and moved back afterwards whatever happens
//!   (errors, limits, a panic caught on the way).
//! - Sandbox: the language has no file, network, clock or module access (`no_module`, `no_time`;
//!   `eval` is disabled). Limits stop runaway operation counts, deep recursion and oversized
//!   strings, arrays and maps with an error. Commands that read or write files, and the
//!   `automate.*` commands (so scripts can't start scripts), are refused unless the session allows
//!   file access, which only the CLI's `--allow-files` does.
//! - Natively a run happens on a helper thread with a large stack, so the recursion the limits
//!   still allow (nested calls, deeply nested arrays) never reaches the end of a smaller stack.

mod convert;
mod model;
pub mod record;
#[cfg(test)]
mod tests;

use std::cell::RefCell;
use std::rc::Rc;

use rhai::{Dynamic, Engine, EvalAltResult, Position};
use serde::Serialize;
use serde_json::Value as Json;

use crate::{DocState, Session};

/// The resource limits of one run ([`Session::script_limits`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Limits {
    /// Rhai operations (roughly: expressions evaluated) before the run is stopped.
    pub operations: u64,
    /// Seconds a run may take (natively; the browser has no clock to measure it). Some operations
    /// cost more as data grows (a growing array is checked against its limit on every change), so
    /// the operation count alone doesn't bound the time.
    pub seconds: u64,
    /// Nested script function calls.
    pub call_levels: usize,
    /// Expression nesting in the source, at top level and inside functions.
    pub expr_depth: usize,
    pub fn_expr_depth: usize,
    /// Bytes in one string.
    pub string_len: usize,
    /// Elements in one array, counting nested arrays' elements.
    pub array_size: usize,
    /// Properties in one object map, counting nested maps.
    pub map_size: usize,
    /// Variables in scope and script functions defined.
    pub variables: usize,
    pub functions: usize,
}

/// Limits on native targets, where the run has a large stack of its own.
#[cfg(not(target_arch = "wasm32"))]
pub const LIMITS: Limits = Limits {
    operations: 50_000_000,
    seconds: 30,
    call_levels: 128,
    expr_depth: 64,
    fn_expr_depth: 64,
    string_len: 16 * 1024 * 1024,
    array_size: 1_000_000,
    map_size: 100_000,
    variables: 10_000,
    functions: 10_000,
};

/// Limits in the browser: no clock to stop a slow run and the page's (small) stack, so smaller
/// arrays and maps (growing one costs more the larger it is, and nesting is recursion).
#[cfg(target_arch = "wasm32")]
pub const LIMITS: Limits = Limits {
    operations: 20_000_000,
    seconds: 0,
    call_levels: 32,
    expr_depth: 32,
    fn_expr_depth: 32,
    string_len: 4 * 1024 * 1024,
    array_size: 10_000,
    map_size: 5_000,
    variables: 10_000,
    functions: 10_000,
};

impl Default for Limits {
    fn default() -> Self {
        LIMITS
    }
}

/// Stack of the helper thread a run uses natively (reserved address space, committed as used).
#[cfg(not(target_arch = "wasm32"))]
const STACK: usize = 256 * 1024 * 1024;

/// Longest script source accepted.
pub const MAX_CODE: usize = 1024 * 1024;
/// Output kept from one run.
const MAX_OUTPUT_LINES: usize = 10_000;
const MAX_OUTPUT_BYTES: usize = 1024 * 1024;

/// What a run printed and how it ended (Code Editor › Output).
#[derive(Clone, Debug, Default, PartialEq, Serialize)]
pub struct ScriptRun {
    /// The script's name (or what the caller called inline code).
    pub name: String,
    /// `print`, `debug` and `console.log` lines, in order.
    pub output: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<ScriptError>,
}

/// Why a run stopped early, with where in the source when known.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct ScriptError {
    pub message: String,
    /// 1-based line and column in the script.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub line: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub column: Option<usize>,
}

impl ScriptError {
    fn plain(message: impl Into<String>) -> ScriptError {
        ScriptError { message: message.into(), line: None, column: None }
    }

    /// `Line 3, column 5: message`.
    pub fn describe(&self) -> String {
        match (self.line, self.column) {
            (Some(l), Some(c)) => format!("Line {l}, column {c}: {}", self.message),
            (Some(l), None) => format!("Line {l}: {}", self.message),
            _ => self.message.clone(),
        }
    }

    fn at(message: String, pos: Position) -> ScriptError {
        ScriptError { message, line: pos.line(), column: pos.position() }
    }

    fn from_eval(err: &EvalAltResult, l: &Limits) -> ScriptError {
        // The innermost error says what went wrong and where; wrappers only name the functions
        // it happened in.
        let inner = err.unwrap_inner();
        let pos = inner.position();
        let pos = if pos.is_none() { err.position() } else { pos };
        let message = match inner {
            EvalAltResult::ErrorRuntime(v, _) => convert::text(v),
            EvalAltResult::ErrorTooManyOperations(_) => {
                format!("The script ran too long and was stopped (more than {} operations). Check for a loop that never ends.", l.operations)
            }
            EvalAltResult::ErrorTerminated(..) => {
                format!("The script ran too long and was stopped (more than {} seconds). Check for a loop that never ends.", l.seconds)
            }
            EvalAltResult::ErrorStackOverflow(_) => {
                format!("Function calls are nested too deeply (more than {} levels). Check for recursion that never ends.", l.call_levels)
            }
            EvalAltResult::ErrorDataTooLarge(what, _) => format!("{what} is larger than scripts allow."),
            EvalAltResult::ErrorTooManyVariables(_) => format!("The script uses too many variables (more than {}).", l.variables),
            // Rhai's message, without the position it appends (reported separately).
            other => {
                let t = other.to_string();
                match t.rfind(" (line ") {
                    Some(i) if t.ends_with(')') => t.get(..i).unwrap_or(&t).to_string(),
                    _ => t,
                }
            }
        };
        ScriptError::at(message, pos)
    }
}

/// A runtime error raised by the object model (the call's position is filled in by Rhai).
pub(crate) fn fail(message: impl Into<String>) -> Box<EvalAltResult> {
    EvalAltResult::ErrorRuntime(Dynamic::from(message.into()), Position::NONE).into()
}

pub(crate) type Res<T> = Result<T, Box<EvalAltResult>>;

/// The message for running a workbook's own script before its content was enabled.
pub const BLOCKED: &str =
    "Scripts in this workbook are turned off because it was opened from a file. Enable Content (automate.enableScripts) to run them in this session.";

// ---------------------------------------------------------------- sandbox

/// `file.*` commands scripts may run: they don't touch the file system or other documents.
const FILE_SAFE: &[&str] = &["file.properties", "file.options", "file.saveBytes", "file.print"];

/// Why a script may not run `id` with `params`, or `None` when it may. `files` is the session's
/// file access ([`Session::script_file_access`]).
pub fn refusal(id: &str, params: &Json, files: bool) -> Option<String> {
    if id.starts_with("automate.") {
        return Some(format!("Scripts can't run `{id}` (scripts don't start, record or change scripts)."));
    }
    if files {
        return None;
    }
    let names_file = ["path", "url"].iter().any(|k| params.get(*k).is_some_and(|v| v.as_str().is_some_and(|t| !t.is_empty()) || v.is_object()));
    let file_command = id.starts_with("file.") && !FILE_SAFE.contains(&id);
    (names_file || file_command).then(|| {
        format!("`{id}` opens, reads or writes files, which scripts may not do (gridcraft-cli run --allow-files permits it for a script you trust).")
    })
}

// ---------------------------------------------------------------- host

/// What the object model reaches during a run: the lent session, the workbook the script runs
/// in, and the output so far.
#[derive(Clone)]
pub(crate) struct Host(Rc<HostState>);

pub(crate) struct HostState {
    session: RefCell<Session>,
    /// Uid of the document the script runs in.
    doc: u64,
    files: bool,
    output: RefCell<Output>,
}

#[derive(Default)]
struct Output {
    lines: Vec<String>,
    bytes: usize,
    truncated: bool,
}

impl Host {
    /// Records a line of output, within the output limits.
    pub(crate) fn print(&self, text: &str) {
        let Ok(mut out) = self.0.output.try_borrow_mut() else { return };
        if out.truncated {
            return;
        }
        if out.lines.len() >= MAX_OUTPUT_LINES || out.bytes.saturating_add(text.len()) > MAX_OUTPUT_BYTES {
            out.truncated = true;
            out.lines.push("… (more output was cut off)".into());
            return;
        }
        out.bytes = out.bytes.saturating_add(text.len());
        out.lines.push(text.to_string());
    }

    /// Runs a command for the script, on the script's workbook.
    pub(crate) fn exec(&self, id: &str, params: Json) -> Res<Json> {
        if let Some(why) = refusal(id, &params, self.0.files) {
            return Err(fail(why));
        }
        let mut s = self.0.session.try_borrow_mut().map_err(|_| fail("The workbook is busy."))?;
        focus(&mut s, self.0.doc).map_err(fail)?;
        s.execute(id, params).map_err(|e| fail(e.to_string()))
    }

    /// Runs a command with sheet `sheet` active (commands work on the active sheet), then makes
    /// the sheet that was active before active again.
    pub(crate) fn exec_on(&self, sheet: &str, id: &str, params: Json) -> Res<Json> {
        let before = {
            let mut s = self.0.session.try_borrow_mut().map_err(|_| fail("The workbook is busy."))?;
            focus(&mut s, self.0.doc).map_err(fail)?;
            let d = s.doc_mut().map_err(|e| fail(e.to_string()))?;
            let target = d.wb.sheet_index(sheet).ok_or_else(|| fail(no_sheet(sheet)))?;
            let before = d.wb.active().map(|sh| sh.name.clone());
            d.wb_switch_sheet(target);
            before
        };
        let r = self.exec(id, params);
        if let Some(name) = before
            && let Ok(mut s) = self.0.session.try_borrow_mut()
            && focus(&mut s, self.0.doc).is_ok()
            && let Ok(d) = s.doc_mut()
            && let Some(i) = d.wb.sheet_index(&name)
        {
            d.wb_switch_sheet(i);
        }
        r
    }

    /// Reads the script's workbook.
    pub(crate) fn read<R>(&self, f: impl FnOnce(&DocState) -> Res<R>) -> Res<R> {
        let s = self.0.session.try_borrow().map_err(|_| fail("The workbook is busy."))?;
        let d = s.documents().iter().find(|d| d.uid == self.0.doc).ok_or_else(|| fail(CLOSED))?;
        f(d)
    }
}

const CLOSED: &str = "The workbook the script runs in was closed.";

pub(crate) fn no_sheet(name: &str) -> String {
    format!("There is no worksheet named \"{name}\" (was it renamed or deleted?).")
}

/// Makes the script's document the active one (a command may have switched windows).
fn focus(s: &mut Session, doc: u64) -> Result<(), String> {
    let i = s.documents().iter().position(|d| d.uid == doc).ok_or(CLOSED)?;
    s.set_active(i);
    Ok(())
}

// ---------------------------------------------------------------- running

/// Runs `code` on the active workbook. Everything the run changed stays in the workbook as one
/// undo step (taken by the calling command); the output and any error come back in the
/// [`ScriptRun`], the value the script returned as JSON.
pub fn run(s: &mut Session, name: &str, code: &str) -> (ScriptRun, Option<Json>) {
    let mut run = ScriptRun { name: name.to_string(), ..ScriptRun::default() };
    if code.len() > MAX_CODE {
        run.error = Some(ScriptError::plain(format!("The script is longer than {} KB.", MAX_CODE / 1024)));
        return (run, None);
    }
    let Some(uid) = s.active().map(|d| d.uid) else {
        run.error = Some(ScriptError::plain("No workbook is open."));
        return (run, None);
    };
    // The run's commands keep their own undo steps in a scratch history (so `edit.undo` inside a
    // script undoes the script's last step); afterwards the workbook's history is put back and
    // the calling command records the run as one step.
    let (undo, redo) = match s.active_mut() {
        Some(d) => (std::mem::take(&mut d.undo), std::mem::take(&mut d.redo)),
        None => (vec![], vec![]),
    };
    let requests = s.ui_requests.len();
    let files = s.script_file_access;
    let limits = s.script_limits;
    let (lines, error, value) = on_script_stack(s, |s| execute(s, name, code, files, &limits));
    if let Some(i) = s.documents().iter().position(|d| d.uid == uid) {
        s.set_active(i);
        if let Some(d) = s.active_mut() {
            d.undo = undo;
            d.redo = redo;
        }
    }
    // Commands run programmatically don't open dialogs; their messages become output.
    let queued: Vec<crate::UiRequest> = s.ui_requests.drain(requests.min(s.ui_requests.len())..).collect();
    run.output = lines;
    for q in queued {
        if let crate::UiRequest::Message(m) = q
            && run.output.len() < MAX_OUTPUT_LINES
        {
            run.output.push(m);
        }
    }
    run.error = error;
    (run, value)
}

/// What a run produced: output lines, the error that stopped it, the value it returned.
type Outcome = (Vec<String>, Option<ScriptError>, Option<Json>);

/// Runs `f` on a helper thread with a large stack (natively), or right here (wasm, or when no
/// thread can be started).
fn on_script_stack(s: &mut Session, f: impl FnOnce(&mut Session) -> Outcome + Send) -> Outcome {
    #[cfg(not(target_arch = "wasm32"))]
    {
        let mut job = Some(f);
        let mut result: Option<Outcome> = None;
        std::thread::scope(|scope| {
            let (job, result, s) = (&mut job, &mut result, &mut *s);
            let spawned = std::thread::Builder::new().name("gridcraft-script".into()).stack_size(STACK).spawn_scoped(scope, move || {
                if let Some(f) = job.take() {
                    *result = Some(f(s));
                }
            });
            // `execute` catches panics itself; joining only waits for the thread.
            if let Ok(handle) = spawned {
                let _ = handle.join();
            }
        });
        match (result, job) {
            (Some(r), _) => r,
            // No thread could be started: run here, with the stack there is.
            (None, Some(f)) => f(s),
            (None, None) => (vec![], Some(ScriptError::plain("The script stopped unexpectedly.")), None),
        }
    }
    #[cfg(target_arch = "wasm32")]
    {
        f(s)
    }
}

/// Lends the session to the script, runs it, and takes the session back.
fn execute(s: &mut Session, name: &str, code: &str, files: bool, limits: &Limits) -> Outcome {
    let Some(doc) = s.active().map(|d| d.uid) else { return (vec![], Some(ScriptError::plain("No workbook is open.")), None) };
    let host = Host(Rc::new(HostState { session: RefCell::new(std::mem::take(s)), doc, files, output: RefCell::default() }));
    let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| evaluate(&host, name, code, limits)));
    let lines = host.0.output.try_borrow_mut().map(|mut o| std::mem::take(&mut o.lines)).unwrap_or_default();
    // Everything that referred to the host was dropped with the engine, so the session comes
    // back whole; the fallback only matters if something still holds a reference.
    match Rc::try_unwrap(host.0) {
        Ok(state) => *s = state.session.into_inner(),
        Err(shared) => {
            if let Ok(mut lent) = shared.session.try_borrow_mut() {
                *s = std::mem::take(&mut *lent);
            }
        }
    }
    match outcome {
        Ok(Ok(value)) => (lines, None, Some(value)),
        Ok(Err(e)) => (lines, Some(e), None),
        Err(_) => (lines, Some(ScriptError::plain("The script stopped because of an internal error; the changes it made so far were kept.")), None),
    }
}

/// Compiles and runs the script: the top-level statements, then `main(workbook)` if it has one.
fn evaluate(host: &Host, name: &str, code: &str, limits: &Limits) -> Result<Json, ScriptError> {
    let engine = build_engine(host, limits);
    let mut ast = engine.compile(code).map_err(|e| {
        let pos = e.position();
        ScriptError::at(e.err_type().to_string(), pos)
    })?;
    ast.set_source(name);
    let mut scope = rhai::Scope::new();
    scope.push_constant("workbook", model::Workbook::new(host.clone()));
    let top: Dynamic = engine.eval_ast_with_scope(&mut scope, &ast).map_err(|e| ScriptError::from_eval(&e, limits))?;
    let main = ast.iter_functions().find(|f| f.name == "main").map(|f| f.params.len());
    let options = rhai::CallFnOptions::new().eval_ast(false);
    let result: Dynamic = match main {
        None => top,
        Some(0) => engine.call_fn_with_options(options, &mut scope, &ast, "main", ()).map_err(|e| ScriptError::from_eval(&e, limits))?,
        Some(1) => engine
            .call_fn_with_options(options, &mut scope, &ast, "main", (model::Workbook::new(host.clone()),))
            .map_err(|e| ScriptError::from_eval(&e, limits))?,
        Some(n) => return Err(ScriptError::plain(format!("`main` takes the workbook as its one parameter; this one has {n}."))),
    };
    Ok(convert::to_json(&result))
}

/// The syntax error in `code`, if it doesn't parse (the Code Editor reports it on Save).
pub fn check(code: &str) -> Option<ScriptError> {
    let mut e = Engine::new_raw();
    let l = LIMITS;
    e.set_max_expr_depths(l.expr_depth, l.fn_expr_depth).set_max_functions(l.functions).set_max_string_size(l.string_len);
    e.disable_symbol("eval");
    match e.compile(code) {
        Ok(_) => None,
        Err(err) => Some(ScriptError::at(err.err_type().to_string(), err.position())),
    }
}

/// A sandboxed Rhai engine with the object model registered.
fn build_engine(host: &Host, l: &Limits) -> Engine {
    let mut e = Engine::new();
    e.set_max_operations(l.operations)
        .set_max_call_levels(l.call_levels)
        .set_max_expr_depths(l.expr_depth, l.fn_expr_depth)
        .set_max_string_size(l.string_len)
        .set_max_array_size(l.array_size)
        .set_max_map_size(l.map_size)
        .set_max_variables(l.variables)
        .set_max_functions(l.functions);
    // A time budget on top of the operation count (natively; wasm has no clock).
    #[cfg(not(target_arch = "wasm32"))]
    if l.seconds > 0 {
        #[allow(clippy::disallowed_methods)] // not on wasm
        let start = std::time::Instant::now();
        let budget = std::time::Duration::from_secs(l.seconds);
        e.on_progress(move |ops| ((ops & 0xFF) == 0 && start.elapsed() > budget).then(|| Dynamic::from("time")));
    }
    // Code is what the user wrote: no evaluating strings as more code.
    e.disable_symbol("eval");
    let h = host.clone();
    e.on_print(move |t| h.print(t));
    let h = host.clone();
    e.on_debug(move |t, _, pos| match pos.line() {
        Some(l) => h.print(&format!("[line {l}] {t}")),
        None => h.print(t),
    });
    // `console` is a value every function sees (Rhai functions don't see outer variables).
    // `on_var` is stable API that Rhai marks as volatile.
    #[allow(deprecated)]
    e.on_var(|name, _, _| Ok((name == "console").then(|| Dynamic::from(model::Console))));
    model::register(&mut e, host);
    e
}
