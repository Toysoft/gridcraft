//! The VBA interpreter: runs procedures of a parsed project against a [`Host`].

use std::collections::HashMap;
use std::rc::Rc;
use std::sync::{Arc, Mutex};

use crate::VbaError;
use crate::ast::*;
use crate::ovba::{ModuleKind, Project};
use crate::value::{Array, Collection, HostObj, Instance, Object, Value, parse_date};

/// An argument passed to the host: its name when given as `name:=value`.
#[derive(Clone, Debug)]
pub struct HostArg {
    pub name: Option<String>,
    pub value: Value,
}

/// An event the host asks the program to handle: `module`'s `proc` (e.g. `UserForm1`,
/// `CommandButton1_Click`) with arguments. ByRef arguments (`Cancel`, `KeyCode`) come back
/// through [`Host::event_done`].
#[derive(Clone, Debug)]
pub struct Event {
    pub module: String,
    pub proc: String,
    pub args: Vec<Value>,
}

/// The world a macro runs in: GridCraft's Excel object model, UserForms and dialogs.
pub trait Host {
    /// A name the program doesn't define (`Range`, `ActiveSheet`, `UserForm1`, `xlUp`).
    fn global(&mut self, name: &str, args: &[HostArg]) -> Result<Option<Value>, VbaError>;
    /// A property read or method call on a host object.
    fn get(&mut self, obj: &HostObj, member: &str, args: &[HostArg]) -> Result<Value, VbaError>;
    /// A property assignment.
    fn set(&mut self, obj: &HostObj, member: &str, args: &[HostArg], value: Value) -> Result<(), VbaError>;
    /// An object used as a value or called with arguments (`Range("A1")` → its value,
    /// `ws.Cells(1, 2)`).
    fn default_get(&mut self, obj: &HostObj, args: &[HostArg]) -> Result<Value, VbaError> {
        self.get(obj, "_Default", args)
    }
    fn default_set(&mut self, obj: &HostObj, args: &[HostArg], value: Value) -> Result<(), VbaError> {
        self.set(obj, "_Default", args, value)
    }
    /// The members of a collection object, for `For Each`.
    fn items(&mut self, obj: &HostObj) -> Result<Vec<Value>, VbaError>;
    /// `MsgBox`: the button pressed (`vbOK` = 1 …).
    fn msgbox(&mut self, prompt: &str, buttons: i64, title: &str) -> Result<i64, VbaError>;
    /// `InputBox`: the text entered, `None` when cancelled.
    fn inputbox(&mut self, prompt: &str, title: &str, default: &str) -> Result<Option<String>, VbaError>;
    /// The local date-time as a serial.
    fn now(&mut self) -> f64;
    /// `Debug.Print`.
    fn debug_print(&mut self, _text: &str) {}
    /// An event queued while the program ran (`UserForm_Initialize` when a form loads, a
    /// control's `Change` when code sets it). Run before the program continues.
    fn take_event(&mut self) -> Option<Event> {
        None
    }
    /// While a modal UserForm is open: blocks until the next event, `None` once it closed.
    fn wait_event(&mut self) -> Result<Option<Event>, VbaError> {
        Ok(None)
    }
    /// The values of an event's arguments after its handler ran (ByRef `Cancel`, `KeyCode`).
    fn event_done(&mut self, _event: &Event, _args: &[Value]) {}
    /// Checked between statements: `true` stops the run (the user pressed Esc / Ctrl+Break).
    fn should_stop(&mut self) -> bool {
        false
    }
}

/// Class name the host gives a value returned from a call that opened a modal form: the
/// interpreter then serves events until it closes.
pub const MODAL: &str = "__Modal";
/// The interpreter's own `Err` and `Debug` objects.
const ERR_CLASS: &str = "__Err";
const DEBUG_CLASS: &str = "__Debug";

/// Deepest procedure nesting (VBA's "Out of stack space").
const MAX_CALL_DEPTH: usize = 400;

struct ModuleRt {
    name: String,
    kind: ModuleKind,
    ast: ModuleAst,
}

/// A variable and the type it was declared with (assignments convert to it).
#[derive(Clone, Debug)]
struct Var {
    value: Value,
    ty: TypeName,
}

impl Var {
    fn new(ty: TypeName) -> Var {
        Var { value: default_of(&ty), ty }
    }
}

fn default_of(ty: &TypeName) -> Value {
    match ty {
        TypeName::Variant => Value::Empty,
        TypeName::Integer | TypeName::Long | TypeName::LongLong | TypeName::Byte => Value::Int(0),
        TypeName::Single | TypeName::Double | TypeName::Currency | TypeName::Decimal => Value::Double(0.0),
        TypeName::Date => Value::Date(0.0),
        TypeName::String => Value::str(""),
        TypeName::Boolean => Value::Bool(false),
        TypeName::Object | TypeName::Named(_) => Value::Nothing,
    }
}

#[derive(Clone, Debug, PartialEq)]
enum ErrMode {
    Off,
    ResumeNext,
    GoTo(String),
}

struct Frame {
    module: usize,
    proc_name: String,
    /// Source line being run.
    line: u32,
    locals: HashMap<String, Var>,
    with: Vec<Value>,
    me: Option<Value>,
    err_mode: ErrMode,
    /// In an error handler (an error here goes to the caller).
    handling: bool,
    /// Top-level statement index to resume after (`Resume Next`) / at (`Resume`).
    resume_at: Option<usize>,
}

enum Flow {
    Normal,
    Exit(ExitKind),
    GoTo(String),
    Resume(Option<String>),
}

/// The `Err` object.
#[derive(Clone, Debug, Default)]
pub struct ErrObj {
    pub number: i64,
    pub description: String,
    pub source: String,
}

pub struct Interp {
    modules: Vec<ModuleRt>,
    /// Module-level variables: (module index, lower-case name).
    module_vars: HashMap<(usize, String), Var>,
    /// `Static` locals: (module, procedure, name).
    statics: HashMap<(usize, String, String), Var>,
    frames: Vec<Frame>,
    pub err: ErrObj,
    /// Modules that failed to parse, with the error.
    pub load_errors: Vec<(String, VbaError)>,
    /// Statements run (for the stop check cadence).
    ticks: u64,
    /// The host stopped the run or `End` ran.
    stopped: bool,
    /// Where the last unhandled run-time error happened: module, procedure, line.
    pub error_at: Option<(String, String, u32)>,
}

fn lower(s: &str) -> String {
    s.to_ascii_lowercase()
}

impl Interp {
    /// Parses every module of `project`. Modules with syntax errors are left out (and listed in
    /// `load_errors`); the rest still run.
    pub fn new(project: &Project) -> Interp {
        let mut modules = Vec::new();
        let mut load_errors = Vec::new();
        for m in &project.modules {
            match crate::parser::parse_module(&m.code) {
                Ok(ast) => modules.push(ModuleRt { name: m.name.clone(), kind: m.kind, ast }),
                Err(e) => {
                    load_errors.push((m.name.clone(), e));
                    modules.push(ModuleRt { name: m.name.clone(), kind: m.kind, ast: ModuleAst::default() });
                }
            }
        }
        let mut it = Interp {
            modules,
            module_vars: HashMap::new(),
            statics: HashMap::new(),
            frames: Vec::new(),
            err: ErrObj::default(),
            load_errors,
            ticks: 0,
            stopped: false,
            error_at: None,
        };
        it.init_module_vars();
        it
    }

    fn init_module_vars(&mut self) {
        for (mi, m) in self.modules.iter().enumerate() {
            // A class module's variables belong to each instance.
            if m.kind == ModuleKind::Class {
                continue;
            }
            for (d, _) in &m.ast.vars {
                let v = match &d.bounds {
                    Some(b) if !b.is_empty() => {
                        // Constant bounds only at module level.
                        let dims: Vec<(i64, usize)> = b
                            .iter()
                            .map(|(lo, hi)| {
                                let lo = lo.as_ref().map_or(m.ast.option_base, const_int);
                                let hi = const_int(hi);
                                (lo, usize::try_from(hi - lo + 1).unwrap_or(0))
                            })
                            .collect();
                        Var { value: Array::new(dims).map(|a| Value::Array(Arc::new(a))).unwrap_or(Value::Empty), ty: TypeName::Variant }
                    }
                    Some(_) => Var { value: Value::Array(Arc::new(Array::from_vec(0, vec![]))), ty: TypeName::Variant },
                    None => Var::new(d.ty.clone()),
                };
                self.module_vars.insert((mi, lower(&d.name)), v);
            }
        }
    }

    /// Public procedures that can run as macros: parameterless Subs, `Module.Name` for those in
    /// document modules.
    pub fn macros(&self) -> Vec<String> {
        let mut out = Vec::new();
        for m in &self.modules {
            for p in &m.ast.procs {
                if p.kind == ProcKind::Sub && !p.private && p.params.iter().all(|x| x.optional) {
                    match m.kind {
                        ModuleKind::Standard => out.push(p.name.clone()),
                        ModuleKind::Document => out.push(format!("{}.{}", m.name, p.name)),
                        _ => {}
                    }
                }
            }
        }
        out
    }

    /// Whether `module` has a procedure `proc` (an event handler, say).
    pub fn has_proc(&self, module: &str, proc: &str) -> bool {
        self.modules.iter().any(|m| m.name.eq_ignore_ascii_case(module) && m.ast.procs.iter().any(|p| p.name.eq_ignore_ascii_case(proc)))
    }

    fn find_proc(&self, module: Option<usize>, name: &str) -> Option<(usize, Rc<Proc>)> {
        let want = |p: &&Proc| p.name.eq_ignore_ascii_case(name) && p.kind != ProcKind::PropertyLet && p.kind != ProcKind::PropertySet;
        // The current module first, then public procedures of standard modules.
        if let Some(mi) = module
            && let Some(p) = self.modules.get(mi).and_then(|m| m.ast.procs.iter().find(want))
        {
            return Some((mi, Rc::new(p.clone())));
        }
        for (mi, m) in self.modules.iter().enumerate() {
            if m.kind == ModuleKind::Standard
                && let Some(p) = m.ast.procs.iter().find(|p| want(p) && !p.private)
            {
                return Some((mi, Rc::new(p.clone())));
            }
        }
        None
    }

    fn module_index(&self, name: &str) -> Option<usize> {
        self.modules.iter().position(|m| m.name.eq_ignore_ascii_case(name))
    }

    /// Runs a macro by name (`Macro1`, `Module1.Macro1`, `Sheet1.Proc`). `End` and a stop are a
    /// normal finish.
    pub fn run(&mut self, host: &mut dyn Host, name: &str, args: Vec<Value>) -> Result<Value, VbaError> {
        self.stopped = false;
        self.frames.clear();
        self.error_at = None;
        let found = match name.split_once('.') {
            Some((m, p)) => self
                .module_index(m)
                .and_then(|mi| self.modules[mi].ast.procs.iter().find(|x| x.name.eq_ignore_ascii_case(p)).map(|x| (mi, Rc::new(x.clone())))),
            None => self.find_proc(None, name),
        };
        let Some((mi, proc)) = found else {
            return Err(VbaError::runtime(1004, format!("Cannot run the macro '{name}'. The macro may not be available in this workbook.")));
        };
        let me = self.module_me(host, mi)?;
        let args: Vec<ArgVal> = args.into_iter().map(|v| ArgVal { value: v, name: None, back: None }).collect();
        match self.call_proc(host, mi, &proc, args, me) {
            Err(VbaError::Stopped) => Ok(Value::Empty),
            r => r.map(|(v, _)| v),
        }
    }

    /// Runs an event handler if the module has it; the values of its arguments afterwards (for
    /// ByRef `Cancel`), or `None` when there is no handler.
    pub fn run_event(&mut self, host: &mut dyn Host, module: &str, proc: &str, args: Vec<Value>) -> Result<Option<Vec<Value>>, VbaError> {
        let Some(mi) = self.module_index(module) else { return Ok(None) };
        let Some(p) = self.modules[mi].ast.procs.iter().find(|p| p.name.eq_ignore_ascii_case(proc)).map(|p| Rc::new(p.clone())) else {
            return Ok(None);
        };
        let top = self.frames.is_empty();
        if top {
            self.stopped = false;
            self.error_at = None;
        }
        let me = self.module_me(host, mi)?;
        let n = args.len();
        let args: Vec<ArgVal> = args.into_iter().map(|v| ArgVal { value: v, name: None, back: None }).collect();
        match self.call_proc(host, mi, &p, args, me) {
            Ok((_, after)) => Ok(Some(after.into_iter().take(n).collect())),
            Err(VbaError::Stopped) if top => Ok(Some(vec![])),
            Err(e) => Err(e),
        }
    }

    /// `Me` for a document or form module: the host object of that name.
    fn module_me(&mut self, host: &mut dyn Host, mi: usize) -> Result<Option<Value>, VbaError> {
        let m = &self.modules[mi];
        if matches!(m.kind, ModuleKind::Document | ModuleKind::Form) {
            let name = m.name.clone();
            return host.global(&name, &[]);
        }
        Ok(None)
    }

    // ------------------------------------------------------------------ calls

    fn call_proc(
        &mut self,
        host: &mut dyn Host,
        mi: usize,
        proc: &Rc<Proc>,
        args: Vec<ArgVal>,
        me: Option<Value>,
    ) -> Result<(Value, Vec<Value>), VbaError> {
        if self.frames.len() >= MAX_CALL_DEPTH {
            return Err(VbaError::runtime(28, "Out of stack space"));
        }
        let mut locals: HashMap<String, Var> = HashMap::new();
        // Bind arguments: positional, then named.
        let mut bound: Vec<Option<Value>> = vec![None; proc.params.len()];
        let mut extra: Vec<Value> = Vec::new();
        let mut pos = 0usize;
        for a in &args {
            match &a.name {
                Some(n) => {
                    let Some(i) = proc.params.iter().position(|p| p.name.eq_ignore_ascii_case(n)) else {
                        return Err(VbaError::runtime(448, format!("Named argument not found: {n}")));
                    };
                    bound[i] = Some(a.value.clone());
                }
                None => {
                    if let Some(p) = proc.params.get(pos) {
                        if p.param_array {
                            extra.push(a.value.clone());
                        } else {
                            bound[pos] = Some(a.value.clone());
                            pos += 1;
                        }
                    } else if proc.params.last().is_some_and(|p| p.param_array) {
                        extra.push(a.value.clone());
                    } else {
                        return Err(VbaError::runtime(450, "Wrong number of arguments or invalid property assignment"));
                    }
                }
            }
        }
        for (i, p) in proc.params.iter().enumerate() {
            let v = if p.param_array {
                Value::Array(Arc::new(Array::from_vec(0, std::mem::take(&mut extra))))
            } else {
                match bound[i].take() {
                    Some(Value::Missing) | None if p.optional => match &p.default {
                        Some(d) => self.eval_const(d)?,
                        None => {
                            if p.ty == TypeName::Variant {
                                Value::Missing
                            } else {
                                default_of(&p.ty)
                            }
                        }
                    },
                    Some(v) => coerce(v, &p.ty)?,
                    None => return Err(VbaError::runtime(449, format!("Argument not optional: {}", p.name))),
                }
            };
            locals.insert(lower(&p.name), Var { value: v, ty: p.ty.clone() });
        }
        if matches!(proc.kind, ProcKind::Function | ProcKind::PropertyGet) {
            locals.insert(lower(&proc.name), Var::new(proc.ret.clone()));
        }
        self.frames.push(Frame {
            module: mi,
            proc_name: proc.name.clone(),
            line: proc.line,
            locals,
            with: Vec::new(),
            me,
            err_mode: ErrMode::Off,
            handling: false,
            resume_at: None,
        });
        let r = self.exec_body(host, &proc.body);
        let frame = self.frames.pop();
        r?;
        let mut frame = frame.ok_or(VbaError::runtime(51, "Internal error"))?;
        let ret = if matches!(proc.kind, ProcKind::Function | ProcKind::PropertyGet) {
            frame.locals.remove(&lower(&proc.name)).map(|v| v.value).unwrap_or_default()
        } else {
            Value::Empty
        };
        let after: Vec<Value> = proc.params.iter().map(|p| frame.locals.get(&lower(&p.name)).map(|v| v.value.clone()).unwrap_or_default()).collect();
        // Copy ByRef arguments back to the caller's variables.
        for (i, a) in args.iter().enumerate() {
            let param = match &a.name {
                Some(n) => proc.params.iter().position(|p| p.name.eq_ignore_ascii_case(n)),
                None => Some(i),
            };
            if let (Some(back), Some(pi)) = (&a.back, param)
                && let Some(p) = proc.params.get(pi)
                && !p.by_val
                && !p.param_array
                && let Some(v) = after.get(pi)
            {
                self.assign(host, back, v.clone(), false)?;
            }
        }
        Ok((ret, after))
    }

    fn frame(&self) -> Result<&Frame, VbaError> {
        self.frames.last().ok_or(VbaError::runtime(51, "Internal error: no frame"))
    }
    fn frame_mut(&mut self) -> Result<&mut Frame, VbaError> {
        self.frames.last_mut().ok_or(VbaError::runtime(51, "Internal error: no frame"))
    }

    // ------------------------------------------------------------------ statements

    /// A procedure body: top-level statements with labels, `GoTo` and `On Error GoTo`.
    fn exec_body(&mut self, host: &mut dyn Host, body: &Rc<Vec<Stmt>>) -> Result<(), VbaError> {
        let mut i = 0usize;
        while i < body.len() {
            let stmt = &body[i];
            let r = self.exec(host, stmt);
            let flow = match r {
                Ok(f) => f,
                Err(VbaError::Runtime { number, description }) => {
                    let mode = self.frame()?.err_mode.clone();
                    let handling = self.frame()?.handling;
                    match mode {
                        ErrMode::GoTo(label) if !handling => {
                            self.set_err(number, &description);
                            let f = self.frame_mut()?;
                            f.handling = true;
                            f.resume_at = Some(i);
                            Flow::GoTo(label)
                        }
                        ErrMode::ResumeNext => {
                            self.set_err(number, &description);
                            Flow::Normal
                        }
                        _ => {
                            self.note_error_at();
                            return Err(VbaError::Runtime { number, description });
                        }
                    }
                }
                Err(e) => return Err(e),
            };
            match flow {
                Flow::Normal => i += 1,
                Flow::Exit(ExitKind::Sub | ExitKind::Function | ExitKind::Property) => return Ok(()),
                Flow::Exit(_) => i += 1,
                Flow::GoTo(label) => {
                    i = find_label(body, &label).ok_or_else(|| VbaError::runtime(20, format!("Label not defined: {label}")))?;
                }
                Flow::Resume(target) => {
                    let at = self.frame()?.resume_at.ok_or(VbaError::runtime(20, "Resume without error"))?;
                    {
                        let f = self.frame_mut()?;
                        f.handling = false;
                        f.resume_at = None;
                    }
                    self.err = ErrObj::default();
                    i = match target.as_deref() {
                        None => at,
                        Some("next") => at + 1,
                        Some(l) => find_label(body, l).ok_or_else(|| VbaError::runtime(20, format!("Label not defined: {l}")))?,
                    };
                }
            }
        }
        Ok(())
    }

    /// Records where an error leaves a procedure (the innermost place wins).
    fn note_error_at(&mut self) {
        if self.error_at.is_some() {
            return;
        }
        if let Some(f) = self.frames.last() {
            let module = self.modules.get(f.module).map(|m| m.name.clone()).unwrap_or_default();
            self.error_at = Some((module, f.proc_name.clone(), f.line));
        }
    }

    fn set_err(&mut self, number: i64, description: &str) {
        // A handled error is no longer where the run failed.
        self.error_at = None;
        let source = self.frames.last().map(|f| self.modules.get(f.module).map(|m| m.name.clone()).unwrap_or_default()).unwrap_or_default();
        self.err = ErrObj { number, description: description.to_string(), source };
    }

    /// A nested block. `On Error Resume Next` continues inside it; other flow goes up.
    fn exec_block(&mut self, host: &mut dyn Host, body: &[Stmt]) -> Result<Flow, VbaError> {
        for stmt in body {
            match self.exec(host, stmt) {
                Ok(Flow::Normal) => {}
                Ok(f) => return Ok(f),
                Err(VbaError::Runtime { number, description }) if self.frame()?.err_mode == ErrMode::ResumeNext => {
                    self.set_err(number, &description);
                }
                Err(e) => return Err(e),
            }
        }
        Ok(Flow::Normal)
    }

    fn tick(&mut self, host: &mut dyn Host) -> Result<(), VbaError> {
        self.ticks = self.ticks.wrapping_add(1);
        if self.stopped || (self.ticks.is_multiple_of(256) && host.should_stop()) {
            self.stopped = true;
            return Err(VbaError::Stopped);
        }
        Ok(())
    }

    fn exec(&mut self, host: &mut dyn Host, stmt: &Stmt) -> Result<Flow, VbaError> {
        self.tick(host)?;
        match stmt {
            Stmt::Nop | Stmt::Label(_) => Ok(Flow::Normal),
            Stmt::Line(n) => {
                if let Some(f) = self.frames.last_mut() {
                    f.line = *n;
                }
                Ok(Flow::Normal)
            }
            Stmt::Dim(decls) | Stmt::Static(decls) => {
                let is_static = matches!(stmt, Stmt::Static(_));
                for d in decls {
                    let v = self.new_var(host, d)?;
                    let f = self.frame()?;
                    let key = lower(&d.name);
                    if is_static {
                        let sk = (f.module, lower(&f.proc_name), key.clone());
                        let var = self.statics.entry(sk).or_insert(v).clone();
                        self.frame_mut()?.locals.insert(key, var);
                    } else {
                        self.frame_mut()?.locals.insert(key, v);
                    }
                }
                Ok(Flow::Normal)
            }
            Stmt::Const(list) => {
                for (n, e) in list {
                    let v = self.eval_value(host, e)?;
                    self.frame_mut()?.locals.insert(lower(n), Var { value: v, ty: TypeName::Variant });
                }
                Ok(Flow::Normal)
            }
            Stmt::ReDim { preserve, decls } => {
                for d in decls {
                    let Some(bounds) = &d.bounds else { continue };
                    let dims = self.dims(host, bounds)?;
                    let mut arr = Array::new(dims)?;
                    if *preserve && let Some(Value::Array(old)) = self.lookup_var(&d.name).map(|v| v.value.clone()) {
                        // Preserve keeps elements whose index is still in range.
                        copy_preserved(&old, &mut arr);
                    }
                    let v = Value::Array(Arc::new(arr));
                    self.assign(host, &Expr::Name(d.name.clone()), v, true)?;
                }
                Ok(Flow::Normal)
            }
            Stmt::Erase(list) => {
                for e in list {
                    self.assign(host, e, Value::Array(Arc::new(Array::from_vec(0, vec![]))), true)?;
                }
                Ok(Flow::Normal)
            }
            Stmt::Let(lhs, rhs) => {
                let v = self.eval_value(host, rhs)?;
                self.assign(host, lhs, v, false)?;
                Ok(Flow::Normal)
            }
            Stmt::Set(lhs, rhs) => {
                let v = self.eval(host, rhs)?;
                if !v.is_object() && !matches!(v, Value::Empty) {
                    return Err(VbaError::runtime(424, "Object required"));
                }
                self.assign(host, lhs, v, true)?;
                Ok(Flow::Normal)
            }
            Stmt::Call(target, args) => {
                self.call_statement(host, target, args)?;
                Ok(Flow::Normal)
            }
            Stmt::If { arms, otherwise } => {
                for (c, body) in arms {
                    if self.eval_bool(host, c)? {
                        return self.exec_block(host, body);
                    }
                }
                self.exec_block(host, otherwise)
            }
            Stmt::For { var, from, to, step, body } => {
                let start = self.eval_value(host, from)?;
                let end = self.eval_value(host, to)?.to_num()?;
                let step = match step {
                    Some(s) => self.eval_value(host, s)?.to_num()?,
                    None => 1.0,
                };
                self.assign(host, var, start, false)?;
                loop {
                    self.tick(host)?;
                    let cur = self.eval_value(host, var)?.to_num()?;
                    if (step >= 0.0 && cur > end) || (step < 0.0 && cur < end) {
                        break;
                    }
                    match self.exec_block(host, body)? {
                        Flow::Normal => {}
                        Flow::Exit(ExitKind::For) => break,
                        f => return Ok(f),
                    }
                    let cur = self.eval_value(host, var)?;
                    let next = match cur {
                        Value::Int(n) if step.fract() == 0.0 => Value::Int(n.saturating_add(step as i64)),
                        v => Value::Double(v.to_num()? + step),
                    };
                    self.assign(host, var, next, false)?;
                }
                Ok(Flow::Normal)
            }
            Stmt::ForEach { var, items, body } => {
                let coll = self.eval(host, items)?;
                let list: Vec<Value> = match &coll {
                    Value::Array(a) => a.data.clone(),
                    Value::Object(Object::Collection(c)) => c.lock().map(|c| c.items.iter().map(|(_, v)| v.clone()).collect()).unwrap_or_default(),
                    Value::Object(Object::Host(h)) => {
                        let v = host.items(h)?;
                        self.drain_events(host)?;
                        v
                    }
                    _ => return Err(VbaError::runtime(13, "Type mismatch: For Each needs a collection or an array")),
                };
                for item in list {
                    let is_obj = item.is_object();
                    self.assign(host, var, item, is_obj)?;
                    match self.exec_block(host, body)? {
                        Flow::Normal => {}
                        Flow::Exit(ExitKind::For) => break,
                        f => return Ok(f),
                    }
                }
                Ok(Flow::Normal)
            }
            Stmt::Do { cond, body } => {
                loop {
                    self.tick(host)?;
                    if let Some((c, until, true)) = cond
                        && self.eval_bool(host, c)? == *until
                    {
                        break;
                    }
                    match self.exec_block(host, body)? {
                        Flow::Normal => {}
                        Flow::Exit(ExitKind::Do) => break,
                        f => return Ok(f),
                    }
                    if let Some((c, until, false)) = cond
                        && self.eval_bool(host, c)? == *until
                    {
                        break;
                    }
                }
                Ok(Flow::Normal)
            }
            Stmt::While(c, body) => {
                while self.eval_bool(host, c)? {
                    self.tick(host)?;
                    match self.exec_block(host, body)? {
                        Flow::Normal => {}
                        f => return Ok(f),
                    }
                }
                Ok(Flow::Normal)
            }
            Stmt::Select { subject, cases, otherwise } => {
                let v = self.eval_value(host, subject)?;
                for (tests, body) in cases {
                    for t in tests {
                        let hit = match t {
                            CaseTest::Value(e) => {
                                let x = self.eval_value(host, e)?;
                                self.compare(&v, &x)? == Some(std::cmp::Ordering::Equal)
                            }
                            CaseTest::Range(a, b) => {
                                let (a, b) = (self.eval_value(host, a)?, self.eval_value(host, b)?);
                                matches!(self.compare(&v, &a)?, Some(o) if o != std::cmp::Ordering::Less)
                                    && matches!(self.compare(&v, &b)?, Some(o) if o != std::cmp::Ordering::Greater)
                            }
                            CaseTest::Is(op, e) => {
                                let x = self.eval_value(host, e)?;
                                self.binary(host, *op, v.clone(), x)?.to_bool()?
                            }
                        };
                        if hit {
                            return self.exec_block(host, body);
                        }
                    }
                }
                match otherwise {
                    Some(b) => self.exec_block(host, b),
                    None => Ok(Flow::Normal),
                }
            }
            Stmt::With(subject, body) => {
                let v = self.eval(host, subject)?;
                self.frame_mut()?.with.push(v);
                let r = self.exec_block(host, body);
                self.frame_mut()?.with.pop();
                r
            }
            Stmt::Exit(k) => Ok(Flow::Exit(*k)),
            Stmt::GoTo(l) => Ok(Flow::GoTo(l.clone())),
            Stmt::OnError(m) => {
                let f = self.frame_mut()?;
                match m {
                    OnError::ResumeNext => f.err_mode = ErrMode::ResumeNext,
                    OnError::GoTo(l) => f.err_mode = ErrMode::GoTo(l.clone()),
                    OnError::GoToZero => {
                        f.err_mode = ErrMode::Off;
                        f.handling = false;
                    }
                }
                self.err = ErrObj::default();
                Ok(Flow::Normal)
            }
            Stmt::Resume(t) => Ok(Flow::Resume(t.clone())),
            Stmt::End => {
                self.stopped = true;
                Err(VbaError::Stopped)
            }
        }
    }

    fn dims(&mut self, host: &mut dyn Host, bounds: &[(Option<Expr>, Expr)]) -> Result<Vec<(i64, usize)>, VbaError> {
        let base = self.frames.last().and_then(|f| self.modules.get(f.module)).map_or(0, |m| m.ast.option_base);
        let mut dims = Vec::new();
        for (lo, hi) in bounds {
            let lo = match lo {
                Some(e) => self.eval_value(host, e)?.to_int()?,
                None => base,
            };
            let hi = self.eval_value(host, hi)?.to_int()?;
            let len = hi.checked_sub(lo).and_then(|d| d.checked_add(1)).filter(|n| *n >= 0).ok_or(VbaError::runtime(9, "Subscript out of range"))?;
            dims.push((lo, usize::try_from(len).map_err(|_| VbaError::runtime(9, "Subscript out of range"))?));
        }
        Ok(dims)
    }

    fn new_var(&mut self, host: &mut dyn Host, d: &VarDecl) -> Result<Var, VbaError> {
        if let Some(b) = &d.bounds {
            let value = if b.is_empty() { Array::from_vec(0, vec![]) } else { Array::new(self.dims(host, b)?)? };
            let fill = default_of(&d.ty);
            let mut value = value;
            if !matches!(fill, Value::Empty) {
                value.data.iter_mut().for_each(|x| *x = fill.clone());
            }
            return Ok(Var { value: Value::Array(Arc::new(value)), ty: TypeName::Variant });
        }
        let mut v = Var::new(d.ty.clone());
        if let TypeName::Named(t) = &d.ty {
            if d.new {
                v.value = self.new_object(host, t)?;
            } else if let Some(rec) = self.new_record(t) {
                v.value = rec;
            }
        }
        Ok(v)
    }

    /// A user-defined `Type` value.
    fn new_record(&self, name: &str) -> Option<Value> {
        for m in &self.modules {
            if let Some((tn, members)) = m.ast.types.iter().find(|(n, _)| n.eq_ignore_ascii_case(name)) {
                let fields = members.iter().map(|d| (d.name.clone(), default_of(&d.ty))).collect();
                return Some(Value::Object(Object::Record(Arc::new(Mutex::new(Instance { class: tn.clone(), fields })))));
            }
        }
        None
    }

    fn new_object(&mut self, host: &mut dyn Host, class: &str) -> Result<Value, VbaError> {
        if class.eq_ignore_ascii_case("Collection") {
            return Ok(Value::Object(Object::Collection(Arc::new(Mutex::new(Collection::default())))));
        }
        if let Some(mi) = self.modules.iter().position(|m| m.kind == ModuleKind::Class && m.name.eq_ignore_ascii_case(class)) {
            let fields = self.modules[mi].ast.vars.iter().map(|(d, _)| (d.name.clone(), default_of(&d.ty))).collect();
            let obj = Value::Object(Object::Instance(Arc::new(Mutex::new(Instance { class: self.modules[mi].name.clone(), fields }))));
            if let Some(p) = self.modules[mi].ast.procs.iter().find(|p| p.name.eq_ignore_ascii_case("Class_Initialize")).map(|p| Rc::new(p.clone())) {
                self.call_proc(host, mi, &p, vec![], Some(obj.clone()))?;
            }
            return Ok(obj);
        }
        // Host classes (`New Collection` aside, e.g. a UserForm by name).
        if let Some(v) = host.global(class, &[])? {
            return Ok(v);
        }
        Err(VbaError::runtime(429, format!("ActiveX component can't create object: {class}")))
    }

    fn call_statement(&mut self, host: &mut dyn Host, target: &Expr, args: &[Arg]) -> Result<(), VbaError> {
        // A bare name that is nothing callable is an error here (as an expression it would be an
        // implicitly declared variable).
        let callee = match target {
            Expr::Call(inner, _) => &**inner,
            t => t,
        };
        if let Expr::Name(n) = callee
            && !self.is_callable(host, n)?
        {
            return Err(VbaError::runtime(35, format!("Sub or Function not defined: {n}")));
        }
        let v = match target {
            // `x.Method a, b` / `Proc a, b` / `obj.Method(a)`: arguments may be empty.
            Expr::Call(inner, inner_args) if args.is_empty() => self.call_expr(host, inner, inner_args)?,
            t => self.call_expr(host, t, args)?,
        };
        self.after_call(host, v)?;
        Ok(())
    }

    /// A value from a host call: serves a modal form's events until it closes.
    fn after_call(&mut self, host: &mut dyn Host, v: Value) -> Result<Value, VbaError> {
        self.drain_events(host)?;
        if let Value::Object(Object::Host(h)) = &v
            && &*h.class == MODAL
        {
            while let Some(ev) = host.wait_event()? {
                self.handle_event(host, ev)?;
                self.drain_events(host)?;
            }
            return Ok(Value::Empty);
        }
        Ok(v)
    }

    fn drain_events(&mut self, host: &mut dyn Host) -> Result<(), VbaError> {
        let mut n = 0;
        while let Some(ev) = host.take_event() {
            n += 1;
            if n > 10_000 {
                return Err(VbaError::runtime(28, "Out of stack space: events keep firing"));
            }
            self.handle_event(host, ev)?;
        }
        Ok(())
    }

    fn handle_event(&mut self, host: &mut dyn Host, ev: Event) -> Result<(), VbaError> {
        // An event handler runs with its own error handling; its errors end the macro.
        let saved_err = std::mem::take(&mut self.err);
        let r = self.run_event(host, &ev.module, &ev.proc, ev.args.clone());
        self.err = saved_err;
        if let Some(after) = r? {
            host.event_done(&ev, &after);
        }
        Ok(())
    }

    // ------------------------------------------------------------------ assignment

    fn lookup_var(&self, name: &str) -> Option<&Var> {
        let key = lower(name);
        let f = self.frames.last()?;
        if let Some(v) = f.locals.get(&key) {
            return Some(v);
        }
        if let Some(v) = self.module_vars.get(&(f.module, key.clone())) {
            return Some(v);
        }
        self.public_var_key(&key).and_then(|k| self.module_vars.get(&k))
    }

    fn public_var_key(&self, key: &str) -> Option<(usize, String)> {
        self.modules.iter().enumerate().find_map(|(mi, m)| {
            (m.kind == ModuleKind::Standard && m.ast.vars.iter().any(|(d, private)| !private && lower(&d.name) == key)).then(|| (mi, key.to_string()))
        })
    }

    fn var_mut(&mut self, name: &str) -> Option<&mut Var> {
        let key = lower(name);
        let f = self.frames.last()?;
        let module = f.module;
        if f.locals.contains_key(&key) {
            return self.frames.last_mut()?.locals.get_mut(&key);
        }
        if self.module_vars.contains_key(&(module, key.clone())) {
            return self.module_vars.get_mut(&(module, key));
        }
        let k = self.public_var_key(&key)?;
        self.module_vars.get_mut(&k)
    }

    fn assign(&mut self, host: &mut dyn Host, lhs: &Expr, v: Value, is_set: bool) -> Result<(), VbaError> {
        match lhs {
            Expr::Name(n) => {
                if let Some(var) = self.var_mut(n) {
                    var.value = if is_set { v } else { coerce(v, &var.ty)? };
                    return Ok(());
                }
                if let Some(Value::Object(Object::Instance(inst))) = self.frames.last().and_then(|f| f.me.clone())
                    && get_field(&inst, n).is_ok()
                {
                    return set_field(&inst, n, v);
                }
                // A control or property of `Me` (`TextBox1 = 5` in a form).
                if let Some(Value::Object(Object::Host(me))) = self.frame()?.me.clone()
                    && self.find_proc(Some(self.frame()?.module), n).is_none()
                {
                    let has = host.get(&me, n, &[]);
                    if let Ok(Value::Object(Object::Host(ctl))) = has {
                        host.default_set(&ctl, &[], v)?;
                        return self.drain_events(host);
                    }
                    if has.is_ok() {
                        host.set(&me, n, &[], v)?;
                        return self.drain_events(host);
                    }
                }
                // An undeclared variable springs into being (no Option Explicit enforcement).
                self.frame_mut()?.locals.insert(lower(n), Var { value: v, ty: TypeName::Variant });
                Ok(())
            }
            Expr::Member(obj, m) => {
                let o = self.eval(host, obj)?;
                self.member_set(host, &o, m, &[], v)
            }
            Expr::WithMember(m) => {
                let o = self.frame()?.with.last().cloned().ok_or(VbaError::runtime(91, "Object variable or With block variable not set"))?;
                self.member_set(host, &o, m, &[], v)
            }
            Expr::Call(target, args) => {
                // An array element.
                if let Expr::Name(n) = &**target
                    && matches!(self.lookup_var(n).map(|x| &x.value), Some(Value::Array(_)))
                {
                    let idx = self.indices(host, args)?;
                    let var = self.var_mut(n).ok_or(VbaError::runtime(9, "Subscript out of range"))?;
                    let Value::Array(arr) = &mut var.value else { return Err(VbaError::runtime(9, "Subscript out of range")) };
                    let a = Arc::make_mut(arr);
                    let off = a.offset(&idx).ok_or(VbaError::runtime(9, "Subscript out of range"))?;
                    if let Some(slot) = a.data.get_mut(off) {
                        *slot = v;
                    }
                    return Ok(());
                }
                // `obj.Prop(args) = v` (a parameterised property), else `f(args) = v` on the
                // object a call returns (`Cells(1, 2) = 5`).
                let hargs = self.host_args(host, args)?;
                if let Expr::Member(obj, m) = &**target {
                    let o = self.eval(host, obj)?;
                    if let Value::Object(Object::Host(h)) = &o {
                        let r = host.get(h, m, &hargs)?;
                        if let Value::Object(Object::Host(inner)) = r {
                            host.default_set(&inner, &[], v)?;
                        } else {
                            host.set(h, m, &hargs, v)?;
                        }
                        return self.drain_events(host);
                    }
                }
                let o = self.eval(host, lhs)?;
                match o {
                    Value::Object(Object::Host(h)) => {
                        host.default_set(&h, &[], v)?;
                        self.drain_events(host)
                    }
                    _ => Err(VbaError::runtime(424, "Object required")),
                }
            }
            _ => Err(VbaError::runtime(424, "Object required")),
        }
    }

    fn member_set(&mut self, host: &mut dyn Host, o: &Value, m: &str, args: &[HostArg], v: Value) -> Result<(), VbaError> {
        match o {
            Value::Object(Object::Host(h)) if &*h.class == ERR_CLASS => {
                match m.to_ascii_lowercase().as_str() {
                    "number" => self.err.number = v.to_int()?,
                    "description" => self.err.description = v.to_text()?,
                    "source" => self.err.source = v.to_text()?,
                    _ => return Err(VbaError::runtime(438, format!("Object doesn't support this property or method: Err.{m}"))),
                }
                Ok(())
            }
            Value::Object(Object::Host(h)) => {
                host.set(h, m, args, v)?;
                self.drain_events(host)
            }
            Value::Object(Object::Instance(inst)) => {
                // A Property Let/Set in the class, else a public field.
                let class = inst.lock().map(|i| i.class.clone()).unwrap_or_default();
                if let Some(mi) = self.module_index(&class)
                    && let Some(p) = self.modules[mi]
                        .ast
                        .procs
                        .iter()
                        .find(|p| p.name.eq_ignore_ascii_case(m) && matches!(p.kind, ProcKind::PropertyLet | ProcKind::PropertySet))
                        .map(|p| Rc::new(p.clone()))
                {
                    let mut a: Vec<ArgVal> = args.iter().map(|x| ArgVal { value: x.value.clone(), name: None, back: None }).collect();
                    a.push(ArgVal { value: v, name: None, back: None });
                    self.call_proc(host, mi, &p, a, Some(o.clone()))?;
                    return Ok(());
                }
                set_field(inst, m, v)
            }
            Value::Object(Object::Record(rec)) => set_field(rec, m, v),
            Value::Nothing => Err(VbaError::runtime(91, "Object variable or With block variable not set")),
            _ => Err(VbaError::runtime(424, "Object required")),
        }
    }

    // ------------------------------------------------------------------ expressions

    /// A value where a scalar is needed: objects give their default member.
    pub fn eval_value(&mut self, host: &mut dyn Host, e: &Expr) -> Result<Value, VbaError> {
        let v = self.eval(host, e)?;
        self.scalar(host, v)
    }

    pub(crate) fn scalar_pub(&mut self, host: &mut dyn Host, v: Value) -> Result<Value, VbaError> {
        self.scalar(host, v)
    }

    fn scalar(&mut self, host: &mut dyn Host, v: Value) -> Result<Value, VbaError> {
        match v {
            Value::Object(Object::Host(h)) => {
                let r = host.default_get(&h, &[])?;
                self.drain_events(host)?;
                match r {
                    Value::Object(_) => Err(VbaError::runtime(438, "Object doesn't support this property or method")),
                    r => Ok(r),
                }
            }
            v => Ok(v),
        }
    }

    fn eval_bool(&mut self, host: &mut dyn Host, e: &Expr) -> Result<bool, VbaError> {
        match self.eval_value(host, e)? {
            Value::Null => Ok(false),
            v => v.to_bool(),
        }
    }

    fn eval_const(&mut self, e: &Expr) -> Result<Value, VbaError> {
        Ok(match e {
            Expr::Int(n) => Value::Int(*n),
            Expr::Num(n) => Value::Double(*n),
            Expr::Str(s) => Value::Str(s.to_string().into()),
            Expr::Bool(b) => Value::Bool(*b),
            Expr::Unary(UnOp::Neg, x) => match self.eval_const(x)? {
                Value::Int(n) => Value::Int(-n),
                v => Value::Double(-v.to_num()?),
            },
            Expr::Name(n) => crate::builtins::constant(n).unwrap_or_default(),
            Expr::Date(d) => Value::Date(parse_date(d).unwrap_or(0.0)),
            _ => Value::Empty,
        })
    }

    /// Evaluates keeping objects as objects.
    pub fn eval(&mut self, host: &mut dyn Host, e: &Expr) -> Result<Value, VbaError> {
        match e {
            Expr::Int(n) => Ok(Value::Int(*n)),
            Expr::Num(n) => Ok(Value::Double(*n)),
            Expr::Str(s) => Ok(Value::Str(s.to_string().into())),
            Expr::Date(d) => parse_date(d).map(Value::Date).ok_or(VbaError::runtime(13, "Type mismatch")),
            Expr::Bool(b) => Ok(Value::Bool(*b)),
            Expr::Nothing => Ok(Value::Nothing),
            Expr::Empty => Ok(Value::Empty),
            Expr::Null => Ok(Value::Null),
            Expr::Missing => Ok(Value::Missing),
            Expr::AddressOf(_) => Ok(Value::Int(0)),
            Expr::New(c) => self.new_object(host, c),
            Expr::TypeOf(x, t) => {
                let v = self.eval(host, x)?;
                Ok(Value::Bool(match &v {
                    Value::Object(o) => o.type_name().eq_ignore_ascii_case(t) || t.eq_ignore_ascii_case("Object"),
                    _ => false,
                }))
            }
            Expr::Unary(UnOp::Neg, x) => match self.eval_value(host, x)? {
                Value::Int(n) => Ok(Value::Int(n.checked_neg().ok_or(VbaError::runtime(6, "Overflow"))?)),
                Value::Null => Ok(Value::Null),
                Value::Date(d) => Ok(Value::Date(-d)),
                v => Ok(Value::Double(-v.to_num()?)),
            },
            Expr::Unary(UnOp::Not, x) => match self.eval_value(host, x)? {
                Value::Bool(b) => Ok(Value::Bool(!b)),
                Value::Null => Ok(Value::Null),
                v => Ok(Value::Int(!v.to_int()?)),
            },
            Expr::Binary(op, a, b) => {
                if *op == BinOp::Is {
                    let (x, y) = (self.eval(host, a)?, self.eval(host, b)?);
                    return Ok(Value::Bool(match (&x, &y) {
                        (Value::Nothing, Value::Nothing) => true,
                        (Value::Object(p), Value::Object(q)) => p.same(q),
                        _ => false,
                    }));
                }
                let x = self.eval_value(host, a)?;
                // Short-circuit nothing (VBA evaluates both sides), but keep Null semantics.
                let y = self.eval_value(host, b)?;
                self.binary(host, *op, x, y)
            }
            Expr::Name(n) => self.name_value(host, n),
            Expr::WithMember(m) => {
                let o = self.frame()?.with.last().cloned().ok_or(VbaError::runtime(91, "Object variable or With block variable not set"))?;
                self.member_get(host, &o, m, &[])
            }
            Expr::Member(obj, m) => {
                // `Module1.Proc`, `UserForm1.Show` (a form's module name is its default object).
                if let Expr::Name(mn) = &**obj
                    && self.lookup_var(mn).is_none()
                    && let Some(mi) = self.module_index(mn)
                    && self.modules[mi].kind == ModuleKind::Standard
                {
                    return self.call_in_module(host, mi, m, &[]);
                }
                let o = self.eval(host, obj)?;
                self.member_get(host, &o, m, &[])
            }
            Expr::Call(target, args) => self.call_expr(host, target, args),
        }
    }

    fn call_in_module(&mut self, host: &mut dyn Host, mi: usize, name: &str, args: &[Arg]) -> Result<Value, VbaError> {
        let p = self.modules[mi].ast.procs.iter().find(|p| p.name.eq_ignore_ascii_case(name)).map(|p| Rc::new(p.clone()));
        if let Some(p) = p {
            let a = self.proc_args(host, args, &p)?;
            let me = self.module_me(host, mi)?;
            return Ok(self.call_proc(host, mi, &p, a, me)?.0);
        }
        if let Some(var) = self.module_vars.get(&(mi, lower(name))) {
            return Ok(var.value.clone());
        }
        Err(VbaError::runtime(438, format!("Object doesn't support this property or method: {name}")))
    }

    /// A bare name's value: a variable, constant, function without arguments, `Me`'s member, a
    /// host global or a built-in.
    fn name_value(&mut self, host: &mut dyn Host, n: &str) -> Result<Value, VbaError> {
        if let Some(v) = self.lookup_var(n) {
            return Ok(v.value.clone());
        }
        // A field of the class instance a method runs on.
        if let Some(Value::Object(Object::Instance(inst))) = self.frames.last().and_then(|f| f.me.clone())
            && let Ok(v) = get_field(&inst, n)
        {
            return Ok(v);
        }
        let l = lower(n);
        if l == "err" || l == "debug" {
            return Ok(Value::Object(Object::Host(HostObj::new(if l == "err" { ERR_CLASS } else { DEBUG_CLASS }, ()))));
        }
        if l == "me" {
            return self.frame()?.me.clone().ok_or(VbaError::runtime(424, "Object required: Me"));
        }
        if let Some(v) = self.module_const(host, n)? {
            return Ok(v);
        }
        if let Some(mi) = self.frames.last().map(|f| f.module)
            && let Some((pmi, p)) = self.find_proc(Some(mi), n)
            && p.params.iter().all(|x| x.optional || x.param_array)
        {
            let me = if pmi == mi { self.frame()?.me.clone() } else { self.module_me(host, pmi)? };
            return Ok(self.call_proc(host, pmi, &p, vec![], me)?.0);
        }
        self.call_named(host, n, vec![], &[])
    }

    fn module_const(&mut self, host: &mut dyn Host, n: &str) -> Result<Option<Value>, VbaError> {
        let Some(mi) = self.frames.last().map(|f| f.module) else { return Ok(None) };
        let found = self.modules[mi]
            .ast
            .consts
            .iter()
            .find(|(c, _, _)| c.eq_ignore_ascii_case(n))
            .map(|(_, e, _)| e.clone())
            .or_else(|| {
                self.modules
                    .iter()
                    .find_map(|m| m.ast.consts.iter().find(|(c, _, private)| !private && c.eq_ignore_ascii_case(n)).map(|(_, e, _)| e.clone()))
            })
            .or_else(|| {
                // Enum members.
                self.modules.iter().find_map(|m| {
                    m.ast.enums.iter().find_map(|(_, members)| {
                        let mut next = 0i64;
                        for (name, val) in members {
                            let v = match val {
                                Some(Expr::Int(k)) => *k,
                                Some(Expr::Unary(UnOp::Neg, b)) => match &**b {
                                    Expr::Int(k) => -k,
                                    _ => next,
                                },
                                _ => next,
                            };
                            if name.eq_ignore_ascii_case(n) {
                                return Some(Expr::Int(v));
                            }
                            next = v + 1;
                        }
                        None
                    })
                })
            });
        match found {
            Some(e) => Ok(Some(self.eval_value(host, &e)?)),
            None => Ok(None),
        }
    }

    /// A name called with arguments (or none): a procedure, `Me`'s member, a host global, a
    /// built-in function.
    fn call_named(&mut self, host: &mut dyn Host, n: &str, args: Vec<ArgVal>, raw: &[Arg]) -> Result<Value, VbaError> {
        // A member of `Me` (a form's control, a sheet's Range).
        if let Some(Value::Object(Object::Host(me))) = self.frames.last().and_then(|f| f.me.clone()) {
            let hargs: Vec<HostArg> = args.iter().map(|a| HostArg { name: a.name.clone(), value: a.value.clone() }).collect();
            match host.get(&me, n, &hargs) {
                Ok(v) => return self.after_call(host, v),
                Err(VbaError::Runtime { number: 438, .. }) => {}
                Err(e) => return Err(e),
            }
        }
        let hargs: Vec<HostArg> = args.iter().map(|a| HostArg { name: a.name.clone(), value: a.value.clone() }).collect();
        if let Some(v) = host.global(n, &hargs)? {
            return self.after_call(host, v);
        }
        if let Some(r) = crate::builtins::call(self, host, n, &args, raw) {
            return r;
        }
        if let Some(c) = crate::builtins::constant(n) {
            return Ok(c);
        }
        // A `Declare`d DLL function: not available, returns 0.
        if self.modules.iter().any(|m| m.ast.declares.iter().any(|d| d.name.eq_ignore_ascii_case(n))) {
            return Ok(Value::Int(0));
        }
        if args.is_empty() {
            // An undeclared variable reads as Empty.
            return Ok(Value::Empty);
        }
        Err(VbaError::runtime(35, format!("Sub or Function not defined: {n}")))
    }

    /// Whether a bare name can be called: a variable (array/object), a procedure, a declared
    /// function, `Me`'s member, a host global or a built-in.
    fn is_callable(&mut self, host: &mut dyn Host, n: &str) -> Result<bool, VbaError> {
        let l = lower(n);
        if self.lookup_var(n).is_some() || l == "err" || l == "debug" || l == "me" {
            return Ok(true);
        }
        let mi = self.frames.last().map(|f| f.module);
        if self.find_proc(mi, n).is_some() || self.modules.iter().any(|m| m.ast.declares.iter().any(|d| d.name.eq_ignore_ascii_case(n))) {
            return Ok(true);
        }
        if crate::builtins::is_builtin(&l) {
            return Ok(true);
        }
        // `Me`'s members are checked when called; otherwise the host must know the name.
        if self.frames.last().and_then(|f| f.me.as_ref()).is_some() {
            return Ok(true);
        }
        Ok(host.global(n, &[]).map(|v| v.is_some()).unwrap_or(true))
    }

    fn indices(&mut self, host: &mut dyn Host, args: &[Arg]) -> Result<Vec<i64>, VbaError> {
        args.iter().map(|a| self.eval_value(host, &a.value).and_then(|v| v.to_int())).collect()
    }

    fn host_args(&mut self, host: &mut dyn Host, args: &[Arg]) -> Result<Vec<HostArg>, VbaError> {
        let mut out = Vec::with_capacity(args.len());
        for a in args {
            out.push(HostArg { name: a.name.clone(), value: self.eval(host, &a.value)? });
        }
        Ok(out)
    }

    /// Arguments for a procedure call: values, with where to copy ByRef results back.
    fn proc_args(&mut self, host: &mut dyn Host, args: &[Arg], p: &Proc) -> Result<Vec<ArgVal>, VbaError> {
        let mut out = Vec::with_capacity(args.len());
        for (i, a) in args.iter().enumerate() {
            let param = match &a.name {
                Some(n) => p.params.iter().find(|x| x.name.eq_ignore_ascii_case(n)),
                None => p.params.get(i).or_else(|| p.params.last().filter(|x| x.param_array)),
            };
            let wants_object = param.is_some_and(|x| matches!(x.ty, TypeName::Object | TypeName::Named(_)) || x.ty == TypeName::Variant);
            let value = if wants_object { self.eval(host, &a.value)? } else { self.eval_value(host, &a.value)? };
            let back = match &a.value {
                Expr::Name(_) | Expr::Call(_, _) | Expr::Member(_, _) if !a.by_val => Some(a.value.clone()),
                _ => None,
            };
            out.push(ArgVal { value, name: a.name.clone(), back });
        }
        Ok(out)
    }

    fn call_expr(&mut self, host: &mut dyn Host, target: &Expr, args: &[Arg]) -> Result<Value, VbaError> {
        match target {
            Expr::Name(n) => {
                // An array element or a callable variable (an object's default member).
                if let Some(var) = self.lookup_var(n).map(|v| v.value.clone()) {
                    return self.index_value(host, var, args);
                }
                if let Some(mi) = self.frames.last().map(|f| f.module)
                    && let Some((pmi, p)) = self.find_proc(Some(mi), n)
                {
                    let a = self.proc_args(host, args, &p)?;
                    let me = if pmi == mi { self.frame()?.me.clone() } else { self.module_me(host, pmi)? };
                    return Ok(self.call_proc(host, pmi, &p, a, me)?.0);
                }
                let mut a = Vec::with_capacity(args.len());
                for x in args {
                    a.push(ArgVal { value: self.eval(host, &x.value)?, name: x.name.clone(), back: None });
                }
                self.call_named(host, n, a, args)
            }
            Expr::Member(obj, m) => {
                if let Expr::Name(mn) = &**obj
                    && self.lookup_var(mn).is_none()
                    && let Some(mi) = self.module_index(mn)
                    && self.modules[mi].kind == ModuleKind::Standard
                {
                    return self.call_in_module(host, mi, m, args);
                }
                let o = self.eval(host, obj)?;
                let hargs = self.host_args(host, args)?;
                self.member_get(host, &o, m, &hargs)
            }
            Expr::WithMember(m) => {
                let o = self.frame()?.with.last().cloned().ok_or(VbaError::runtime(91, "Object variable or With block variable not set"))?;
                let hargs = self.host_args(host, args)?;
                self.member_get(host, &o, m, &hargs)
            }
            other => {
                let v = self.eval(host, other)?;
                self.index_value(host, v, args)
            }
        }
    }

    /// `v(args)`: an array element, an object's default member with arguments, or `v` itself.
    fn index_value(&mut self, host: &mut dyn Host, v: Value, args: &[Arg]) -> Result<Value, VbaError> {
        if args.is_empty() {
            return Ok(v);
        }
        match &v {
            Value::Array(a) => {
                let idx = self.indices(host, args)?;
                let off = a.offset(&idx).ok_or(VbaError::runtime(9, "Subscript out of range"))?;
                Ok(a.data.get(off).cloned().unwrap_or_default())
            }
            Value::Object(Object::Host(h)) => {
                let hargs = self.host_args(host, args)?;
                let r = host.default_get(h, &hargs)?;
                self.after_call(host, r)
            }
            Value::Object(Object::Collection(_)) => {
                let hargs = self.host_args(host, args)?;
                self.member_get(host, &v, "Item", &hargs)
            }
            _ => Err(VbaError::runtime(13, "Type mismatch")),
        }
    }

    fn member_get(&mut self, host: &mut dyn Host, o: &Value, m: &str, args: &[HostArg]) -> Result<Value, VbaError> {
        match o {
            Value::Object(Object::Host(h)) if &*h.class == ERR_CLASS => self.err_member(m, args),
            Value::Object(Object::Host(h)) if &*h.class == DEBUG_CLASS => {
                if m.eq_ignore_ascii_case("print") {
                    let mut parts = Vec::new();
                    for a in args {
                        parts.push(self.scalar(host, a.value.clone())?.to_text()?);
                    }
                    host.debug_print(&parts.join(" "));
                }
                Ok(Value::Empty)
            }
            // `Application.Run "Macro", args…` runs a procedure of this program.
            Value::Object(Object::Host(h)) if &*h.class == "Application" && m.eq_ignore_ascii_case("run") => {
                let name = args.first().map(|a| a.value.to_text()).transpose()?.unwrap_or_default();
                let name = name.rsplit('!').next().unwrap_or(&name).trim_matches('\'').to_string();
                let rest: Vec<ArgVal> = args.iter().skip(1).map(|a| ArgVal { value: a.value.clone(), name: None, back: None }).collect();
                let found = match name.split_once('.') {
                    Some((mn, pn)) => self
                        .module_index(mn)
                        .and_then(|mi| self.modules[mi].ast.procs.iter().find(|x| x.name.eq_ignore_ascii_case(pn)).map(|x| (mi, Rc::new(x.clone())))),
                    None => self.find_proc(None, &name),
                };
                let (mi, p) = found.ok_or_else(|| VbaError::runtime(1004, format!("Cannot run the macro '{name}'")))?;
                let me = self.module_me(host, mi)?;
                Ok(self.call_proc(host, mi, &p, rest, me)?.0)
            }
            // `Application.InputBox(Prompt, Title, Default, …)`: False when cancelled.
            Value::Object(Object::Host(h)) if &*h.class == "Application" && m.eq_ignore_ascii_case("inputbox") => {
                let get = |i: usize, n: &str| {
                    args.iter()
                        .find(|a| a.name.as_deref().is_some_and(|x| x.eq_ignore_ascii_case(n)))
                        .or_else(|| args.get(i).filter(|a| a.name.is_none()))
                        .map(|a| a.value.clone())
                };
                let text =
                    |v: Option<Value>| v.filter(|v| !matches!(v, Value::Missing)).map(|v| v.to_text()).transpose().map(Option::unwrap_or_default);
                let r = host.inputbox(&text(get(0, "Prompt"))?, &text(get(1, "Title"))?, &text(get(2, "Default"))?)?;
                Ok(r.map_or(Value::Bool(false), Value::from))
            }
            Value::Object(Object::Host(h)) => {
                let r = host.get(h, m, args)?;
                self.after_call(host, r)
            }
            Value::Object(Object::Collection(c)) => collection_member(c, m, args),
            Value::Object(Object::Instance(inst)) => {
                let class = inst.lock().map(|i| i.class.clone()).unwrap_or_default();
                if let Some(mi) = self.module_index(&class)
                    && let Some(p) = self.modules[mi]
                        .ast
                        .procs
                        .iter()
                        .find(|p| p.name.eq_ignore_ascii_case(m) && !matches!(p.kind, ProcKind::PropertyLet | ProcKind::PropertySet))
                        .map(|p| Rc::new(p.clone()))
                {
                    let a = args.iter().map(|x| ArgVal { value: x.value.clone(), name: x.name.clone(), back: None }).collect();
                    return Ok(self.call_proc(host, mi, &p, a, Some(o.clone()))?.0);
                }
                get_field(inst, m)
            }
            Value::Object(Object::Record(rec)) => get_field(rec, m),
            Value::Nothing | Value::Empty => Err(VbaError::runtime(91, "Object variable or With block variable not set")),
            _ => Err(VbaError::runtime(424, format!("Object required: .{m}"))),
        }
    }

    /// `Err.Number`, `Err.Description`, `Err.Source`, `Err.Clear`, `Err.Raise`.
    fn err_member(&mut self, m: &str, args: &[HostArg]) -> Result<Value, VbaError> {
        let arg = |i: usize, name: &str| {
            args.iter()
                .find(|a| a.name.as_deref().is_some_and(|n| n.eq_ignore_ascii_case(name)))
                .or_else(|| args.get(i).filter(|a| a.name.is_none()))
                .map(|a| a.value.clone())
        };
        match m.to_ascii_lowercase().as_str() {
            "number" | "_default" => Ok(Value::Int(self.err.number)),
            "description" => Ok(Value::from(self.err.description.clone())),
            "source" => Ok(Value::from(self.err.source.clone())),
            "clear" => {
                self.err = ErrObj::default();
                Ok(Value::Empty)
            }
            "raise" => {
                let number = arg(0, "Number").map(|v| v.to_int()).transpose()?.unwrap_or(5);
                let description = match arg(2, "Description") {
                    Some(Value::Missing) | None => format!("Application-defined or object-defined error {number}"),
                    Some(d) => d.to_text()?,
                };
                Err(VbaError::Runtime { number, description })
            }
            _ => Err(VbaError::runtime(438, format!("Object doesn't support this property or method: Err.{m}"))),
        }
    }

    // ------------------------------------------------------------------ operators

    fn text_compare(&self) -> bool {
        self.frames.last().and_then(|f| self.modules.get(f.module)).is_some_and(|m| m.ast.option_compare_text)
    }

    /// Compares two values: `None` when one is Null.
    pub fn compare(&self, a: &Value, b: &Value) -> Result<Option<std::cmp::Ordering>, VbaError> {
        use std::cmp::Ordering;
        if matches!(a, Value::Null) || matches!(b, Value::Null) {
            return Ok(None);
        }
        let is_num = |v: &Value| matches!(v, Value::Int(_) | Value::Double(_) | Value::Bool(_) | Value::Date(_));
        match (a, b) {
            (Value::Str(x), Value::Str(y)) => Ok(Some(if self.text_compare() { x.to_lowercase().cmp(&y.to_lowercase()) } else { x.cmp(y) })),
            (Value::Empty, Value::Str(y)) => Ok(Some("".cmp(y))),
            (Value::Str(x), Value::Empty) => Ok(Some((**x).cmp(""))),
            (Value::Str(s), n) | (n, Value::Str(s)) if is_num(n) || matches!(n, Value::Empty) => {
                // A numeric string compares as a number; other text sorts after numbers.
                let swapped = matches!(b, Value::Str(_));
                let o = match crate::value::parse_num(s) {
                    Some(x) => n.to_num()?.partial_cmp(&x).unwrap_or(Ordering::Equal),
                    None => Ordering::Less,
                };
                Ok(Some(if swapped { o } else { o.reverse() }))
            }
            _ => Ok(Some(a.to_num()?.partial_cmp(&b.to_num()?).unwrap_or(Ordering::Equal))),
        }
    }

    fn binary(&mut self, _host: &mut dyn Host, op: BinOp, a: Value, b: Value) -> Result<Value, VbaError> {
        use std::cmp::Ordering;
        let cmp = |s: &Self, f: fn(Ordering) -> bool| -> Result<Value, VbaError> {
            Ok(match s.compare(&a, &b)? {
                Some(o) => Value::Bool(f(o)),
                None => Value::Null,
            })
        };
        match op {
            BinOp::Eq => cmp(self, |o| o == Ordering::Equal),
            BinOp::Ne => cmp(self, |o| o != Ordering::Equal),
            BinOp::Lt => cmp(self, |o| o == Ordering::Less),
            BinOp::Gt => cmp(self, |o| o == Ordering::Greater),
            BinOp::Le => cmp(self, |o| o != Ordering::Greater),
            BinOp::Ge => cmp(self, |o| o != Ordering::Less),
            BinOp::Concat => {
                if matches!(a, Value::Null) && matches!(b, Value::Null) {
                    return Ok(Value::Null);
                }
                let t = |v: &Value| if matches!(v, Value::Null) { Ok(String::new()) } else { v.to_text() };
                Ok(Value::from(format!("{}{}", t(&a)?, t(&b)?)))
            }
            BinOp::Like => Ok(Value::Bool(like(&a.to_text()?, &b.to_text()?, self.text_compare()))),
            BinOp::And | BinOp::Or | BinOp::Xor | BinOp::Eqv | BinOp::Imp => {
                if let (Value::Bool(x), Value::Bool(y)) = (&a, &b) {
                    return Ok(Value::Bool(match op {
                        BinOp::And => *x && *y,
                        BinOp::Or => *x || *y,
                        BinOp::Xor => x != y,
                        BinOp::Eqv => x == y,
                        _ => !*x || *y,
                    }));
                }
                if matches!(a, Value::Null) || matches!(b, Value::Null) {
                    return Ok(Value::Null);
                }
                let (x, y) = (a.to_int()?, b.to_int()?);
                Ok(Value::Int(match op {
                    BinOp::And => x & y,
                    BinOp::Or => x | y,
                    BinOp::Xor => x ^ y,
                    BinOp::Eqv => !(x ^ y),
                    _ => !x | y,
                }))
            }
            _ => {
                if matches!(a, Value::Null) || matches!(b, Value::Null) {
                    return Ok(Value::Null);
                }
                // `+` on two strings concatenates.
                if op == BinOp::Add
                    && let (Value::Str(x), Value::Str(y)) = (&a, &b)
                {
                    return Ok(Value::from(format!("{x}{y}")));
                }
                let date = matches!(a, Value::Date(_)) ^ matches!(b, Value::Date(_)) && matches!(op, BinOp::Add | BinOp::Sub);
                if let (Value::Int(x), Value::Int(y)) = (&a, &b) {
                    let r = match op {
                        BinOp::Add => x.checked_add(*y),
                        BinOp::Sub => x.checked_sub(*y),
                        BinOp::Mul => x.checked_mul(*y),
                        BinOp::IntDiv => {
                            if *y == 0 {
                                return Err(VbaError::runtime(11, "Division by zero"));
                            }
                            x.checked_div(*y)
                        }
                        BinOp::Mod => {
                            if *y == 0 {
                                return Err(VbaError::runtime(11, "Division by zero"));
                            }
                            x.checked_rem(*y)
                        }
                        _ => None,
                    };
                    if let Some(r) = r {
                        return Ok(Value::Int(r));
                    }
                }
                let (x, y) = (a.to_num()?, b.to_num()?);
                let r = match op {
                    BinOp::Add => x + y,
                    BinOp::Sub => x - y,
                    BinOp::Mul => x * y,
                    BinOp::Div => {
                        if y == 0.0 {
                            return Err(VbaError::runtime(11, "Division by zero"));
                        }
                        x / y
                    }
                    BinOp::IntDiv | BinOp::Mod => {
                        let (xi, yi) = (crate::value::round_even(x), crate::value::round_even(y));
                        if yi == 0.0 {
                            return Err(VbaError::runtime(11, "Division by zero"));
                        }
                        if op == BinOp::IntDiv { (xi / yi).trunc() } else { xi % yi }
                    }
                    BinOp::Pow => x.powf(y),
                    _ => 0.0,
                };
                if !r.is_finite() {
                    return Err(VbaError::runtime(6, "Overflow"));
                }
                if date {
                    return Ok(Value::Date(r));
                }
                if matches!(op, BinOp::IntDiv | BinOp::Mod) && r.abs() < 9e18 {
                    return Ok(Value::Int(r as i64));
                }
                Ok(Value::Double(r))
            }
        }
    }
}

/// An argument with the expression to copy a ByRef result back to.
pub struct ArgVal {
    pub value: Value,
    pub name: Option<String>,
    pub(crate) back: Option<Expr>,
}

fn const_int(e: &Expr) -> i64 {
    match e {
        Expr::Int(n) => *n,
        Expr::Unary(UnOp::Neg, x) => -const_int(x),
        _ => 0,
    }
}

fn find_label(body: &[Stmt], label: &str) -> Option<usize> {
    body.iter().position(|s| matches!(s, Stmt::Label(l) if l.eq_ignore_ascii_case(label)))
}

fn copy_preserved(old: &Array, new: &mut Array) {
    let n = old.data.len();
    if old.dims.len() == 1 && new.dims.len() == 1 {
        for (i, v) in old.data.iter().enumerate().take(new.data.len()) {
            if let Some(slot) = new.data.get_mut(i) {
                *slot = v.clone();
            }
        }
        return;
    }
    // More dimensions: only the last bound may change; copy by index.
    let mut idx: Vec<i64> = old.dims.iter().map(|(lo, _)| *lo).collect();
    for v in old.data.iter().take(n) {
        if let Some(off) = new.offset(&idx)
            && let Some(slot) = new.data.get_mut(off)
        {
            *slot = v.clone();
        }
        for (k, (lo, len)) in old.dims.iter().enumerate() {
            if let Some(x) = idx.get_mut(k) {
                *x += 1;
                if *x < lo + *len as i64 {
                    break;
                }
                *x = *lo;
            }
        }
    }
}

/// The value converted to a declared type (Let into a typed variable).
pub fn coerce(v: Value, ty: &TypeName) -> Result<Value, VbaError> {
    let range = |n: i64, lo: i64, hi: i64| if (lo..=hi).contains(&n) { Ok(Value::Int(n)) } else { Err(VbaError::runtime(6, "Overflow")) };
    match ty {
        TypeName::Variant | TypeName::Object | TypeName::Named(_) => Ok(v),
        TypeName::Integer => range(v.to_int()?, -32_768, 32_767),
        TypeName::Long => range(v.to_int()?, -2_147_483_648, 2_147_483_647),
        TypeName::LongLong => Ok(Value::Int(v.to_int()?)),
        TypeName::Byte => range(v.to_int()?, 0, 255),
        TypeName::Single | TypeName::Double | TypeName::Currency | TypeName::Decimal => Ok(Value::Double(v.to_num()?)),
        TypeName::Date => Ok(Value::Date(v.to_date()?)),
        TypeName::String => match v {
            Value::Null => Err(VbaError::runtime(94, "Invalid use of Null")),
            v => Ok(Value::from(v.to_text()?)),
        },
        TypeName::Boolean => Ok(Value::Bool(v.to_bool()?)),
    }
}

fn get_field(inst: &Arc<Mutex<Instance>>, m: &str) -> Result<Value, VbaError> {
    let i = inst.lock().map_err(|_| VbaError::runtime(51, "Internal error"))?;
    i.fields
        .iter()
        .find(|(n, _)| n.eq_ignore_ascii_case(m))
        .map(|(_, v)| v.clone())
        .ok_or_else(|| VbaError::runtime(438, format!("Object doesn't support this property or method: {m}")))
}

fn set_field(inst: &Arc<Mutex<Instance>>, m: &str, v: Value) -> Result<(), VbaError> {
    let mut i = inst.lock().map_err(|_| VbaError::runtime(51, "Internal error"))?;
    match i.fields.iter_mut().find(|(n, _)| n.eq_ignore_ascii_case(m)) {
        Some(slot) => {
            slot.1 = v;
            Ok(())
        }
        None => Err(VbaError::runtime(438, format!("Object doesn't support this property or method: {m}"))),
    }
}

fn collection_member(c: &Arc<Mutex<Collection>>, m: &str, args: &[HostArg]) -> Result<Value, VbaError> {
    let mut c = c.lock().map_err(|_| VbaError::runtime(51, "Internal error"))?;
    let arg = |i: usize, name: &str| {
        args.iter()
            .find(|a| a.name.as_deref().is_some_and(|n| n.eq_ignore_ascii_case(name)))
            .or_else(|| args.get(i).filter(|a| a.name.is_none()))
            .map(|a| a.value.clone())
    };
    let find = |c: &Collection, key: &Value| -> Result<usize, VbaError> {
        match key {
            Value::Str(k) => c
                .items
                .iter()
                .position(|(ik, _)| ik.as_deref().is_some_and(|x| x.eq_ignore_ascii_case(k)))
                .ok_or(VbaError::runtime(5, "Invalid procedure call or argument")),
            v => {
                let i = v.to_int()?;
                usize::try_from(i - 1).ok().filter(|i| *i < c.items.len()).ok_or(VbaError::runtime(9, "Subscript out of range"))
            }
        }
    };
    match m.to_ascii_lowercase().as_str() {
        "count" => Ok(Value::Int(c.items.len() as i64)),
        "add" => {
            let item = arg(0, "Item").unwrap_or_default();
            let key = match arg(1, "Key") {
                Some(Value::Missing) | None => None,
                Some(k) => Some(k.to_text()?),
            };
            if let Some(k) = &key
                && c.items.iter().any(|(ik, _)| ik.as_deref().is_some_and(|x| x.eq_ignore_ascii_case(k)))
            {
                return Err(VbaError::runtime(457, "This key is already associated with an element of this collection"));
            }
            c.items.push((key, item));
            Ok(Value::Empty)
        }
        "item" => {
            let i = find(&c, &arg(0, "Index").unwrap_or_default())?;
            Ok(c.items.get(i).map(|(_, v)| v.clone()).unwrap_or_default())
        }
        "remove" => {
            let i = find(&c, &arg(0, "Index").unwrap_or_default())?;
            c.items.remove(i);
            Ok(Value::Empty)
        }
        _ => Err(VbaError::runtime(438, format!("Object doesn't support this property or method: {m}"))),
    }
}

/// VBA `Like`: `?` one character, `*` any run, `#` a digit, `[a-z]` / `[!a-z]` classes.
pub fn like(text: &str, pattern: &str, ignore_case: bool) -> bool {
    let fold = |s: &str| if ignore_case { s.to_lowercase() } else { s.to_string() };
    let t: Vec<char> = fold(text).chars().collect();
    let p: Vec<char> = fold(pattern).chars().collect();
    fn m(t: &[char], p: &[char], depth: usize) -> bool {
        if depth > 10_000 {
            return false;
        }
        let Some((&pc, prest)) = p.split_first() else { return t.is_empty() };
        match pc {
            '*' => (0..=t.len()).any(|k| m(&t[k..], prest, depth + 1)),
            '?' => !t.is_empty() && m(&t[1..], prest, depth + 1),
            '#' => t.first().is_some_and(|c| c.is_ascii_digit()) && m(&t[1..], prest, depth + 1),
            '[' => {
                let Some(close) = prest.iter().position(|c| *c == ']') else { return false };
                let class = &prest[..close];
                let (neg, class) = match class.split_first() {
                    Some(('!', rest)) => (true, rest),
                    _ => (false, class),
                };
                let Some(&c) = t.first() else { return false };
                let mut hit = false;
                let mut i = 0;
                while i < class.len() {
                    if i + 2 < class.len() && class[i + 1] == '-' {
                        hit |= class[i] <= c && c <= class[i + 2];
                        i += 3;
                    } else {
                        hit |= class[i] == c;
                        i += 1;
                    }
                }
                hit != neg && m(&t[1..], &prest[close + 1..], depth + 1)
            }
            c => t.first() == Some(&c) && m(&t[1..], prest, depth + 1),
        }
    }
    m(&t, &p, 0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ovba::{Module, ModuleKind, Project};
    use std::collections::{BTreeMap, VecDeque};

    /// A host with one sheet of cells (`Range("A1").Value`, `Cells(r, c)`), a modal form
    /// `Form1` whose events are scripted, and a log of message boxes.
    #[derive(Default)]
    struct Mock {
        cells: BTreeMap<String, Value>,
        msgs: Vec<String>,
        script: VecDeque<Event>,
        queued: VecDeque<Event>,
        form_open: bool,
        loaded: bool,
        stop_after: Option<u64>,
        polls: u64,
    }

    #[derive(Debug)]
    enum Obj {
        Range(String),
        Form,
        Control(String),
    }

    fn obj(o: Obj) -> Value {
        let class = match &o {
            Obj::Range(_) => "Range",
            Obj::Form => "Form1",
            Obj::Control(_) => "TextBox",
        };
        Value::Object(Object::Host(HostObj::new(class, o)))
    }

    impl Host for Mock {
        fn global(&mut self, name: &str, args: &[HostArg]) -> Result<Option<Value>, VbaError> {
            Ok(match name.to_ascii_lowercase().as_str() {
                "range" => Some(obj(Obj::Range(args.first().map(|a| a.value.to_text()).transpose()?.unwrap_or_default().to_uppercase()))),
                "cells" => {
                    let r = args.first().map(|a| a.value.to_int()).transpose()?.unwrap_or(1);
                    let c = args.get(1).map(|a| a.value.to_int()).transpose()?.unwrap_or(1);
                    Some(obj(Obj::Range(format!("{}{r}", (b'A' + (c - 1) as u8) as char))))
                }
                "form1" => {
                    if !self.loaded {
                        self.loaded = true;
                        self.queued.push_back(Event { module: "Form1".into(), proc: "UserForm_Initialize".into(), args: vec![] });
                    }
                    Some(obj(Obj::Form))
                }
                _ => None,
            })
        }
        fn get(&mut self, o: &HostObj, member: &str, _args: &[HostArg]) -> Result<Value, VbaError> {
            match (o.get::<Obj>(), member.to_ascii_lowercase().as_str()) {
                (Some(Obj::Range(a)), "value" | "_default") => Ok(self.cells.get(a).cloned().unwrap_or_default()),
                (Some(Obj::Range(a)), "address") => Ok(Value::from(format!("${}", a))),
                (Some(Obj::Form), "show") => {
                    self.form_open = true;
                    Ok(Value::Object(Object::Host(HostObj::new(MODAL, ()))))
                }
                (Some(Obj::Form), "_unload" | "hide") => {
                    self.form_open = false;
                    Ok(Value::Empty)
                }
                (Some(Obj::Form), name) if name.starts_with("textbox") => Ok(obj(Obj::Control(name.to_string()))),
                (Some(Obj::Control(n)), "value" | "_default" | "text") => Ok(self.cells.get(&format!("ctl:{n}")).cloned().unwrap_or(Value::from(""))),
                _ => Err(VbaError::runtime(438, format!("no member {member}"))),
            }
        }
        fn set(&mut self, o: &HostObj, member: &str, _args: &[HostArg], value: Value) -> Result<(), VbaError> {
            match (o.get::<Obj>(), member.to_ascii_lowercase().as_str()) {
                (Some(Obj::Range(a)), "value" | "_default") => {
                    self.cells.insert(a.clone(), value);
                    Ok(())
                }
                (Some(Obj::Control(n)), "value" | "_default" | "text") => {
                    self.cells.insert(format!("ctl:{n}"), value);
                    // Setting a control from code fires its Change event.
                    self.queued.push_back(Event { module: "Form1".into(), proc: format!("{n}_Change"), args: vec![] });
                    Ok(())
                }
                _ => Err(VbaError::runtime(438, format!("no member {member}"))),
            }
        }
        fn items(&mut self, _o: &HostObj) -> Result<Vec<Value>, VbaError> {
            Ok(vec![obj(Obj::Range("A1".into())), obj(Obj::Range("A2".into()))])
        }
        fn msgbox(&mut self, prompt: &str, _b: i64, _t: &str) -> Result<i64, VbaError> {
            self.msgs.push(prompt.to_string());
            Ok(1)
        }
        fn inputbox(&mut self, _p: &str, _t: &str, d: &str) -> Result<Option<String>, VbaError> {
            Ok(Some(d.to_string()))
        }
        fn now(&mut self) -> f64 {
            43838.5
        }
        fn take_event(&mut self) -> Option<Event> {
            self.queued.pop_front()
        }
        fn wait_event(&mut self) -> Result<Option<Event>, VbaError> {
            if !self.form_open {
                return Ok(None);
            }
            Ok(self.script.pop_front().or_else(|| {
                self.form_open = false;
                None
            }))
        }
        fn should_stop(&mut self) -> bool {
            self.polls += 1;
            self.stop_after.is_some_and(|n| self.polls > n)
        }
    }

    fn project(modules: &[(&str, ModuleKind, &str)]) -> Project {
        Project {
            name: "P".into(),
            code_page: 1252,
            modules: modules.iter().map(|(n, k, c)| Module { name: n.to_string(), stream: n.to_string(), kind: *k, code: c.to_string() }).collect(),
        }
    }

    fn run(code: &str, mock: &mut Mock) -> Result<Value, VbaError> {
        let p = project(&[("Module1", ModuleKind::Standard, code)]);
        let mut it = Interp::new(&p);
        assert!(it.load_errors.is_empty(), "{:?}", it.load_errors);
        it.run(mock, "Main", vec![])
    }

    fn cell(m: &Mock, a: &str) -> Value {
        m.cells.get(a).cloned().unwrap_or_default()
    }

    #[test]
    fn language_basics() {
        let mut m = Mock::default();
        run(
            r##"
Sub Main()
    Dim i As Long, total As Long, s As String
    For i = 1 To 10
        If i Mod 2 = 0 Then total = total + i
    Next i
    Range("A1").Value = total
    s = "a"
    Do While Len(s) < 4
        s = s & "b"
    Loop
    Range("A2") = s
    Select Case total
        Case 1 To 10: Range("A3") = "low"
        Case Is > 20: Range("A3") = "high"
        Case Else: Range("A3") = "mid"
    End Select
    Dim arr() As Variant
    ReDim arr(1 To 3)
    arr(1) = "x": arr(3) = 7
    ReDim Preserve arr(1 To 4)
    Range("A4") = arr(1) & UBound(arr) & arr(3)
    Range("A5") = Twice(21)
    Dim n As Integer
    n = 5
    Bump n
    Range("A6") = n
    Range("A7") = Left("hello", 2) & Mid("hello", 2, 3) & UCase(Right("hello", 1)) & InStr("hello", "l")
    Range("A8") = Format(1234.5, "#,##0.00")
    Range("A9") = IIf(n > 5, "big", "small")
    Range("A10") = Split("a,b,c", ",")(1)
End Sub
Function Twice(x)
    Twice = x * 2
End Function
Sub Bump(ByRef v As Integer)
    v = v + 1
End Sub
"##,
            &mut m,
        )
        .unwrap();
        assert_eq!(cell(&m, "A1"), Value::Int(30));
        assert_eq!(cell(&m, "A2"), Value::from("abbb"));
        assert_eq!(cell(&m, "A3"), Value::from("high"));
        assert_eq!(cell(&m, "A4"), Value::from("x47"));
        assert_eq!(cell(&m, "A5"), Value::Int(42));
        assert_eq!(cell(&m, "A6"), Value::Int(6), "ByRef copies back");
        assert_eq!(cell(&m, "A7"), Value::from("heellO3"));
        assert_eq!(cell(&m, "A8"), Value::from("1,234.50"));
        assert_eq!(cell(&m, "A9"), Value::from("big"));
        assert_eq!(cell(&m, "A10"), Value::from("b"));
    }

    #[test]
    fn typed_variables_convert_like_vba() {
        let mut m = Mock::default();
        m.cells.insert("B1".into(), Value::from("12"));
        run(
            r##"
Sub Main()
    Dim t As Long, d As Double, s As String
    t = Range("B1").Value
    Range("A1") = t + 1
    s = 5
    Range("A2") = s & "x"
    Dim p1, p2, p3 As Long
    p1 = "3": p2 = "4"
    Range("A3") = p1 + p2
    Range("A4") = p1 - p2
End Sub
"##,
            &mut m,
        )
        .unwrap();
        assert_eq!(cell(&m, "A1"), Value::Int(13));
        assert_eq!(cell(&m, "A2"), Value::from("5x"));
        assert_eq!(cell(&m, "A3"), Value::from("34"), "Variant + Variant strings concatenate");
        assert_eq!(cell(&m, "A4"), Value::Double(-1.0));
    }

    #[test]
    fn error_handling() {
        let mut m = Mock::default();
        run(
            r##"
Sub Main()
    On Error Resume Next
    x = 1 / 0
    Range("A1") = Err.Number
    On Error GoTo Handler
    y = 1 / 0
    Range("A2") = "after"
    Exit Sub
Handler:
    Range("A3") = Err.Description
    Resume Next
End Sub
"##,
            &mut m,
        )
        .unwrap();
        assert_eq!(cell(&m, "A1"), Value::Int(11), "Resume Next records the error");
        assert_eq!(cell(&m, "A3"), Value::from("Division by zero"), "the handler ran");
        assert_eq!(cell(&m, "A2"), Value::from("after"), "Resume Next went on after the failing line");
    }

    #[test]
    fn errors_reach_the_caller_with_their_number() {
        let mut m = Mock::default();
        let e = run("Sub Main()\n  x = 1 / 0\nEnd Sub\n", &mut m).unwrap_err();
        assert!(matches!(e, VbaError::Runtime { number: 11, .. }), "{e:?}");
        let e = run("Sub Main()\n  Main2\nEnd Sub\n", &mut m).unwrap_err();
        assert!(matches!(e, VbaError::Runtime { number: 35, .. }), "{e:?}");
        // Runaway recursion is an error, not a stack overflow.
        let e = run("Sub Main()\n  Main\nEnd Sub\n", &mut m).unwrap_err();
        assert!(matches!(e, VbaError::Runtime { number: 28, .. }), "{e:?}");
    }

    #[test]
    fn host_objects_with_and_for_each() {
        let mut m = Mock::default();
        run(
            r##"
Sub Main()
    Dim r As Range
    Set r = Range("C1")
    With r
        .Value = "w"
    End With
    Dim c
    For Each c In r
        n = n + 1
    Next
    Range("A1") = n & r.Address
    Cells(2, 2).Value = "b2"
    MsgBox "done " & Range("C1")
End Sub
"##,
            &mut m,
        )
        .unwrap();
        assert_eq!(cell(&m, "C1"), Value::from("w"));
        assert_eq!(cell(&m, "A1"), Value::from("2$C1"));
        assert_eq!(cell(&m, "B2"), Value::from("b2"));
        assert_eq!(m.msgs, vec!["done w"]);
    }

    #[test]
    fn modal_forms_run_their_events() {
        let mut m = Mock::default();
        m.script.push_back(Event { module: "Form1".into(), proc: "CommandButton1_Click".into(), args: vec![] });
        let p = project(&[
            ("Module1", ModuleKind::Standard, "Sub Main()\n  Form1.Show\n  Range(\"A9\") = \"closed\"\nEnd Sub\n"),
            (
                "Form1",
                ModuleKind::Form,
                "Private Sub UserForm_Initialize()\n  TextBox1.Value = \"init\"\nEnd Sub\nPrivate Sub TextBox1_Change()\n  Range(\"A2\") = \"changed:\" & TextBox1.Value\nEnd Sub\nPrivate Sub CommandButton1_Click()\n  Range(\"A1\") = TextBox1.Value\n  Unload Me\nEnd Sub\n",
            ),
        ]);
        let mut it = Interp::new(&p);
        assert!(it.load_errors.is_empty(), "{:?}", it.load_errors);
        it.run(&mut m, "Main", vec![]).unwrap();
        assert_eq!(cell(&m, "A2"), Value::from("changed:init"), "Initialize ran and set the control, which fired Change");
        assert_eq!(cell(&m, "A1"), Value::from("init"), "the button's Click ran while the form was open");
        assert_eq!(cell(&m, "A9"), Value::from("closed"), "Show returned when the form closed");
    }

    #[test]
    fn collections_classes_and_types() {
        let mut m = Mock::default();
        let p = project(&[
            (
                "Module1",
                ModuleKind::Standard,
                "Type Pt\n  x As Long\nEnd Type\nSub Main()\n  Dim c As New Collection\n  c.Add 5, \"five\"\n  c.Add 7\n  Range(\"A1\") = c.Count & c(\"five\") & c.Item(2)\n  Dim k As Class1\n  Set k = New Class1\n  k.Name = \"kay\"\n  Range(\"A2\") = k.Greet()\n  Dim p As Pt\n  p.x = 3\n  Range(\"A3\") = p.x\nEnd Sub\n",
            ),
            ("Class1", ModuleKind::Class, "Public Name As String\nPublic Function Greet()\n  Greet = \"hi \" & Name\nEnd Function\n"),
        ]);
        let mut it = Interp::new(&p);
        it.run(&mut m, "Main", vec![]).unwrap();
        assert_eq!(cell(&m, "A1"), Value::from("257"));
        assert_eq!(cell(&m, "A2"), Value::from("hi kay"));
        assert_eq!(cell(&m, "A3"), Value::Int(3));
    }

    #[test]
    fn runaway_loops_can_be_stopped_and_end_stops() {
        let mut m = Mock { stop_after: Some(3), ..Default::default() };
        assert!(run("Sub Main()\n  Do\n  Loop\nEnd Sub\n", &mut m).is_ok(), "a stop ends the run quietly");
        let mut m = Mock::default();
        run("Sub Main()\n  Range(\"A1\") = 1\n  End\n  Range(\"A1\") = 2\nEnd Sub\n", &mut m).unwrap();
        assert_eq!(cell(&m, "A1"), Value::Int(1));
    }

    #[test]
    fn sandbox_refuses_the_outside_world() {
        let mut m = Mock::default();
        for call in ["Shell \"rm -rf /\"", "Kill \"x\"", "Set o = CreateObject(\"WScript.Shell\")"] {
            let e = run(&format!("Sub Main()\n  {call}\nEnd Sub\n"), &mut m).unwrap_err();
            assert!(matches!(e, VbaError::Runtime { number: 70, .. }), "{call}: {e:?}");
        }
        // Declared DLL functions are inert.
        let p = project(&[(
            "Module1",
            ModuleKind::Standard,
            "Private Declare PtrSafe Function GetTickCount Lib \"kernel32\" () As Long\nSub Main()\n  Range(\"A1\") = GetTickCount()\nEnd Sub\n",
        )]);
        Interp::new(&p).run(&mut m, "Main", vec![]).unwrap();
        assert_eq!(cell(&m, "A1"), Value::Int(0));
    }

    #[test]
    fn macros_lists_runnable_subs() {
        let p = project(&[
            ("Module1", ModuleKind::Standard, "Sub A()\nEnd Sub\nPrivate Sub B()\nEnd Sub\nSub C(x)\nEnd Sub\nFunction F()\nEnd Function\n"),
            ("Sheet1", ModuleKind::Document, "Sub D()\nEnd Sub\n"),
        ]);
        assert_eq!(Interp::new(&p).macros(), vec!["A".to_string(), "Sheet1.D".to_string()]);
    }
}
