//! The Excel object model macros see: `Application`, `Workbook(s)`, `Worksheet(s)`/`Sheets`,
//! `Range` (with `Cells`, `Offset`, `End`, `Find`, `Copy`/`PasteSpecial`, formatting…),
//! `WorksheetFunction`, `ActiveWindow`, and the workbook's UserForms. Everything goes through the
//! session's commands and edits, so macros recalculate, respect sheet protection and can be
//! undone like any other change.

use std::sync::Arc;

use gridcraft_core::{CellError, CellRef, MAX_COLS, MAX_ROWS, RangeRef, Value as CValue, col_to_letters};
use gridcraft_model::{Cell, Workbook};
use gridcraft_vba::{Array, HostArg, HostObj, Object, Value, VbaError};
use serde_json::{Value as Json, json};

use crate::Session;
use crate::vba::forms::{ControlState, FormState, control_call, control_get};

type R<T> = Result<T, VbaError>;

/// An object of the model, held by the interpreter in a [`HostObj`].
#[derive(Clone, Debug, PartialEq)]
pub enum XlObj {
    Application,
    Workbooks,
    Workbook(u64),
    /// `Sheets` / `Worksheets` of a workbook, or a window's selected sheets.
    Sheets {
        doc: u64,
        selected: bool,
    },
    Sheet {
        doc: u64,
        name: String,
    },
    Range {
        doc: u64,
        sheet: String,
        areas: Vec<RangeRef>,
    },
    Font(Box<XlObj>),
    Interior(Box<XlObj>),
    Borders(Box<XlObj>),
    Border(Box<XlObj>, i64),
    WorksheetFunction,
    Window(u64),
    /// A UserForm by name (`UserForm1`).
    Form(String),
    Control {
        form: String,
        name: String,
    },
    /// A control on a worksheet (ActiveX/forms control by name, e.g. `Sheet1.ComboBox1`).
    SheetControl {
        doc: u64,
        sheet: String,
        name: String,
    },
    Names(u64),
}

impl XlObj {
    fn class(&self) -> &'static str {
        match self {
            XlObj::Application => "Application",
            XlObj::Workbooks => "Workbooks",
            XlObj::Workbook(_) => "Workbook",
            XlObj::Sheets { .. } => "Sheets",
            XlObj::Sheet { .. } => "Worksheet",
            XlObj::Range { .. } => "Range",
            XlObj::Font(_) => "Font",
            XlObj::Interior(_) => "Interior",
            XlObj::Borders(_) => "Borders",
            XlObj::Border(..) => "Border",
            XlObj::WorksheetFunction => "WorksheetFunction",
            XlObj::Window(_) => "Window",
            XlObj::Form(_) => "UserForm",
            XlObj::Control { .. } => "Control",
            XlObj::SheetControl { .. } => "OLEObject",
            XlObj::Names(_) => "Names",
        }
    }
}

pub fn obj(o: XlObj) -> Value {
    let class = o.class();
    Value::Object(Object::Host(HostObj::new(class, o)))
}

fn xl(h: &HostObj) -> R<&XlObj> {
    h.get::<XlObj>().ok_or_else(|| VbaError::runtime(438, format!("Object doesn't support this property or method ({})", h.class)))
}

fn e1004(m: impl Into<String>) -> VbaError {
    VbaError::runtime(1004, m)
}

fn eng(e: crate::EngineError) -> VbaError {
    e1004(e.to_string())
}

fn no_member(o: &XlObj, m: &str) -> VbaError {
    VbaError::runtime(438, format!("Object doesn't support this property or method: {}.{m}", o.class()))
}

/// The argument at position `i` or named `name`.
fn arg(args: &[HostArg], i: usize, name: &str) -> Option<Value> {
    args.iter()
        .find(|a| a.name.as_deref().is_some_and(|n| n.eq_ignore_ascii_case(name)))
        .or_else(|| args.get(i).filter(|a| a.name.is_none()))
        .map(|a| a.value.clone())
        .filter(|v| !matches!(v, Value::Missing))
}

// ---------------------------------------------------------------- documents and sheets

fn doc_pos(s: &Session, uid: u64) -> R<usize> {
    s.documents().iter().position(|d| d.uid == uid).ok_or_else(|| e1004("The workbook is no longer open"))
}

fn wb_of(s: &Session, uid: u64) -> R<&Workbook> {
    Ok(&s.documents().get(doc_pos(s, uid)?).ok_or_else(|| e1004("The workbook is no longer open"))?.wb)
}

fn sheet_pos(s: &Session, uid: u64, name: &str) -> R<usize> {
    wb_of(s, uid)?.sheet_index(name).ok_or_else(|| VbaError::runtime(9, format!("Subscript out of range: no sheet named {name}")))
}

/// Runs `f` with `uid` as the active document (restored after).
fn with_doc<T>(s: &mut Session, uid: u64, f: impl FnOnce(&mut Session) -> R<T>) -> R<T> {
    let i = doc_pos(s, uid)?;
    let prev = s.active_index();
    s.set_active(i);
    let r = f(s);
    if s.active_index() == i {
        s.set_active(prev);
    }
    r
}

/// Runs a command on sheet `si` of `uid` (activating it for the command, then going back).
fn on_sheet(s: &mut Session, uid: u64, si: usize, id: &str, params: Json) -> R<Json> {
    with_doc(s, uid, |s| {
        let prev = s.doc().map_err(eng)?.wb.active_sheet;
        if prev != si {
            s.execute("sheet.activate", json!({ "sheet": si })).map_err(eng)?;
        }
        let r = s.execute(id, params).map_err(eng);
        if prev != si {
            let _ = s.execute("sheet.activate", json!({ "sheet": prev }));
        }
        r
    })
}

fn quote_sheet(name: &str) -> String {
    if name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') && !name.chars().next().is_some_and(|c| c.is_ascii_digit()) {
        name.to_string()
    } else {
        format!("'{}'", name.replace('\'', "''"))
    }
}

/// Parses a reference text (`A1`, `A1:B2,D4`, `Sheet2!B:B`, a defined name) relative to
/// `sheet`: the sheet it is on and its areas.
pub(crate) fn parse_ref(wb: &Workbook, sheet: usize, text: &str, depth: u32) -> R<(usize, Vec<RangeRef>)> {
    let t = text.trim().trim_start_matches('=');
    if depth > 8 {
        return Err(e1004(format!("Method 'Range' of object '_Global' failed: {text}")));
    }
    // A defined name (sheet scope first).
    if let Some(n) = wb.name(t, sheet) {
        let f = n.formula.clone();
        return parse_ref(wb, sheet, &f, depth + 1);
    }
    let mut out_sheet = sheet;
    let mut areas = Vec::new();
    for part in t.split(',') {
        let part = part.trim();
        let (sh, body) = crate::cmd::split_sheet(part);
        if let Some(sh) = sh {
            out_sheet = wb.sheet_index(&sh).ok_or_else(|| e1004(format!("Method 'Range' of object '_Global' failed: no sheet {sh}")))?;
        }
        let r = RangeRef::parse(&body.replace('$', "")).ok_or_else(|| e1004(format!("Method 'Range' of object '_Global' failed: `{part}`")))?;
        areas.push(r);
    }
    if areas.is_empty() {
        return Err(e1004("Method 'Range' of object '_Global' failed"));
    }
    Ok((out_sheet, areas))
}

fn range_obj(s: &Session, uid: u64, si: usize, areas: Vec<RangeRef>) -> R<Value> {
    let name = wb_of(s, uid)?.sheet(si).map(|sh| sh.name.clone()).ok_or_else(|| e1004("No such sheet"))?;
    Ok(obj(XlObj::Range { doc: uid, sheet: name, areas }))
}

fn active_uid(s: &Session) -> R<u64> {
    s.active().map(|d| d.uid).ok_or_else(|| e1004("No workbook is open"))
}

fn active_sheet_name(s: &Session) -> R<String> {
    let d = s.doc().map_err(eng)?;
    d.wb.active().map(|sh| sh.name.clone()).ok_or_else(|| e1004("No sheet"))
}

/// `Range(…)` on a sheet: a text reference or two corner ranges.
fn range_call(s: &Session, uid: u64, si: usize, args: &[HostArg]) -> R<Value> {
    let first = arg(args, 0, "Cell1").ok_or_else(|| VbaError::runtime(450, "Range needs an address"))?;
    let second = arg(args, 1, "Cell2");
    let wb = wb_of(s, uid)?;
    let resolve = |v: &Value| -> R<(usize, Vec<RangeRef>)> {
        match v {
            Value::Object(Object::Host(h)) => match xl(h)? {
                XlObj::Range { sheet, areas, doc } => Ok((sheet_pos(s, *doc, sheet)?, areas.clone())),
                o => Err(no_member(o, "Range")),
            },
            v => parse_ref(wb, si, &v.to_text()?, 0),
        }
    };
    let (sa, a) = resolve(&first)?;
    match second {
        None => range_obj(s, uid, sa, a),
        Some(b) => {
            let (_, b) = resolve(&b)?;
            let bounds = a.iter().chain(b.iter()).copied().reduce(|x, y| x.union(&y)).unwrap_or_default();
            range_obj(s, uid, sa, vec![bounds])
        }
    }
}

/// `Cells(r, c)` relative to `base` (1-based); `Cells(n)` counts across then down.
fn cells_call(s: &Session, uid: u64, si: usize, base: RangeRef, args: &[HostArg]) -> R<Value> {
    if args.is_empty() {
        return range_obj(s, uid, si, vec![base]);
    }
    let r = arg(args, 0, "RowIndex").map(|v| v.to_int()).transpose()?.unwrap_or(1);
    let c = match arg(args, 1, "ColumnIndex") {
        Some(Value::Str(t)) => {
            i64::from(gridcraft_core::letters_to_col(&t.to_ascii_uppercase()).ok_or_else(|| e1004(format!("bad column {t}")))?) + 1
        }
        Some(v) => v.to_int()?,
        None => {
            // A single index runs along the rows of the range.
            let w = i64::from(base.width());
            let k = r - 1;
            let (rr, cc) = (k.div_euclid(w) + 1, k.rem_euclid(w) + 1);
            return offset_cell(s, uid, si, base, rr - 1, cc - 1);
        }
    };
    offset_cell(s, uid, si, base, r - 1, c - 1)
}

fn offset_cell(s: &Session, uid: u64, si: usize, base: RangeRef, dr: i64, dc: i64) -> R<Value> {
    let c = base.start.offset(dr, dc).ok_or_else(|| e1004("Application-defined or object-defined error"))?;
    range_obj(s, uid, si, vec![RangeRef::cell(c)])
}

// ---------------------------------------------------------------- values

/// VBA's `CVErr` numbers for cell errors.
fn error_number(e: CellError) -> i64 {
    match e {
        CellError::Null => 2000,
        CellError::Div0 => 2007,
        CellError::Value => 2015,
        CellError::Ref => 2023,
        CellError::Name => 2029,
        CellError::Num => 2036,
        CellError::NA => 2042,
        _ => 2015,
    }
}

fn error_of(n: i64) -> CellError {
    match n {
        2000 => CellError::Null,
        2007 => CellError::Div0,
        2023 => CellError::Ref,
        2029 => CellError::Name,
        2036 => CellError::Num,
        2042 => CellError::NA,
        _ => CellError::Value,
    }
}

/// A cell's value as VBA reads it: numbers in date formats are Dates.
fn cell_value(wb: &Workbook, si: usize, c: CellRef) -> Value {
    let Some(sh) = wb.sheet(si) else { return Value::Empty };
    match sh.value(c) {
        CValue::Empty => Value::Empty,
        CValue::Number(n) => {
            let fmt = wb.styles.get(sh.style_id(c)).num_fmt.clone();
            if gridcraft_numfmt::NumberFormat::parse(fmt.as_str()).is_date() { Value::Date(n) } else { Value::Double(n) }
        }
        CValue::Text(t) => Value::Str(t.to_string().into()),
        CValue::Bool(b) => Value::Bool(b),
        CValue::Error(e) => Value::Error(error_number(e)),
        CValue::Array(a) => match a.data.first() {
            Some(CValue::Number(n)) => Value::Double(*n),
            Some(CValue::Text(t)) => Value::Str(t.to_string().into()),
            _ => Value::Empty,
        },
    }
}

/// The value of a range: one cell's value, or a 1-based 2-D array.
fn range_value(wb: &Workbook, si: usize, r: RangeRef) -> R<Value> {
    if r.is_single() {
        return Ok(cell_value(wb, si, r.start));
    }
    // Whole columns/rows read up to the used range.
    let r = trim_used(wb, si, r);
    if r.count() > 10_000_000 {
        return Err(VbaError::runtime(7, "Out of memory"));
    }
    let (h, w) = (r.height() as usize, r.width() as usize);
    let mut a = Array::new(vec![(1, h), (1, w)])?;
    for (i, c) in r.iter().enumerate() {
        let (ri, ci) = (i / w, i % w);
        if let Some(off) = a.offset(&[ri as i64 + 1, ci as i64 + 1])
            && let Some(slot) = a.data.get_mut(off)
        {
            *slot = cell_value(wb, si, c);
        }
    }
    Ok(Value::Array(Arc::new(a)))
}

fn trim_used(wb: &Workbook, si: usize, r: RangeRef) -> RangeRef {
    if !(r.is_full_cols() || r.is_full_rows()) {
        return r;
    }
    match wb.sheet(si).and_then(|sh| sh.used_range()) {
        Some(u) => r.intersection(&u.union(&RangeRef::cell(r.start))).unwrap_or(RangeRef::cell(r.start)),
        None => RangeRef::cell(r.start),
    }
}

/// The cell to store for a VBA value at `at` (keeping the cell's style).
fn to_cell(v: &Value, si: usize, at: CellRef, wb: &mut Workbook) -> R<Option<Cell>> {
    let style = wb.sheet(si).map(|sh| sh.style_id(at)).unwrap_or_default();
    let plain = |value: CValue| Some(Cell { value, formula: None, style });
    Ok(match v {
        Value::Empty | Value::Null | Value::Missing => None,
        Value::Bool(b) => plain(CValue::Bool(*b)),
        Value::Int(n) => plain(CValue::number(*n as f64)),
        Value::Double(n) => plain(CValue::number(*n)),
        Value::Date(d) => {
            let mut style = style;
            if wb.styles.get(style).num_fmt.as_str() == "General" {
                let code = if d.fract() == 0.0 { "m/d/yyyy" } else { "m/d/yyyy h:mm" };
                style = wb.styles.derive(style, |s| s.num_fmt = gridcraft_model::NumFmt::new(code));
            }
            Some(Cell { value: CValue::number(*d), formula: None, style })
        }
        Value::Error(n) => plain(CValue::Error(error_of(*n))),
        Value::Str(t) => crate::cmd::edit::input_to_cell(t, si, at, wb).map_err(eng)?,
        Value::Array(_) | Value::Object(_) | Value::Nothing => return Err(VbaError::runtime(13, "Type mismatch")),
    })
}

/// Writes `cells` on sheet `si` of `uid` in one edit (one recalculation).
fn put(s: &mut Session, uid: u64, si: usize, cells: Vec<(CellRef, Value)>) -> R<()> {
    with_doc(s, uid, |s| {
        crate::cmd::edit(s, |cx| {
            for (at, v) in &cells {
                let cell = to_cell(v, si, *at, &mut cx.wb).map_err(|e| crate::EngineError::Other(e.to_string()))?;
                let sh = cx.sheet_mut(si)?;
                match cell {
                    Some(c) => sh.set_cell(*at, c),
                    None => {
                        // Clearing keeps the cell's formatting.
                        let style = sh.style_id(*at);
                        sh.remove_cell(*at);
                        if style != Default::default() {
                            sh.set_cell(*at, Cell { style, ..Cell::default() });
                        }
                    }
                }
                cx.touch(si, *at);
            }
            Ok(())
        })
        .map_err(eng)
    })
}

/// Writes one cell (a sheet control's linked cell).
pub(crate) fn put_value(s: &mut Session, uid: u64, si: usize, at: CellRef, v: Value) -> R<()> {
    put(s, uid, si, vec![(at, v)])
}

/// `Range.Value = v`: a scalar fills every cell; an array fills from the top-left.
fn set_range_value(s: &mut Session, uid: u64, si: usize, areas: &[RangeRef], v: &Value) -> R<()> {
    let mut cells = Vec::new();
    for r in areas {
        if r.count() > 5_000_000 {
            return Err(VbaError::runtime(7, "Out of memory"));
        }
        match v {
            Value::Array(a) => {
                let two = a.dims.len() == 2;
                for (i, c) in r.iter().enumerate() {
                    let (ri, ci) = ((i / r.width() as usize) as i64, (i % r.width() as usize) as i64);
                    let item = if two {
                        let (lo0, lo1) = (a.dims[0].0, a.dims[1].0);
                        a.offset(&[lo0 + ri, lo1 + ci]).and_then(|o| a.data.get(o)).cloned()
                    } else {
                        // A 1-D array fills a row (and repeats down).
                        a.offset(&[a.dims.first().map_or(0, |d| d.0) + ci]).and_then(|o| a.data.get(o)).cloned()
                    };
                    cells.push((c, item.unwrap_or(Value::Error(2042))));
                }
            }
            v => cells.extend(r.iter().map(|c| (c, v.clone()))),
        }
    }
    put(s, uid, si, cells)
}

fn a1(r: &RangeRef, row_abs: bool, col_abs: bool) -> String {
    let cell = |c: CellRef| format!("{}{}{}{}", if col_abs { "$" } else { "" }, col_to_letters(c.col), if row_abs { "$" } else { "" }, c.row + 1);
    if r.is_full_cols() && !r.is_full_rows() {
        let d = if col_abs { "$" } else { "" };
        return format!("{d}{}:{d}{}", col_to_letters(r.start.col), col_to_letters(r.end.col));
    }
    if r.is_full_rows() && !r.is_full_cols() {
        let d = if row_abs { "$" } else { "" };
        return format!("{d}{}:{d}{}", r.start.row + 1, r.end.row + 1);
    }
    if r.is_single() { cell(r.start) } else { format!("{}:{}", cell(r.start), cell(r.end)) }
}

// ---------------------------------------------------------------- globals

/// Names the object model knows without an object: `Range`, `Cells`, `ActiveSheet`, `Sheets`,
/// a UserForm, a sheet's code name… `this` is the workbook the macro belongs to.
pub fn global(s: &mut Session, this: u64, name: &str, args: &[HostArg]) -> R<Option<Value>> {
    let l = name.to_ascii_lowercase();
    // The workbook's UserForms and sheet code names.
    if let Some(v) = crate::vba::form_object(s, this, name)? {
        return Ok(Some(match args.is_empty() {
            true => v,
            false => return Err(VbaError::runtime(13, "Type mismatch")),
        }));
    }
    if let Some(sheet) = crate::vba::sheet_by_code_name(s, this, name) {
        return Ok(Some(obj(XlObj::Sheet { doc: this, name: sheet })));
    }
    let app = obj(XlObj::Application);
    let Value::Object(Object::Host(h)) = &app else { return Ok(None) };
    match l.as_str() {
        "application" => Ok(Some(app)),
        "range" | "cells" | "activesheet" | "activeworkbook" | "thisworkbook" | "activecell" | "selection" | "workbooks" | "worksheets"
        | "sheets" | "rows" | "columns" | "worksheetfunction" | "activewindow" | "names" | "calculate" | "evaluate" | "intersect" | "union" => {
            get_this(s, this, h, name, args).map(Some)
        }
        _ => Ok(None),
    }
}

fn get_this(s: &mut Session, this: u64, h: &HostObj, member: &str, args: &[HostArg]) -> R<Value> {
    application(s, this, xl(h)?, member, args)
}

fn application(s: &mut Session, this: u64, o: &XlObj, member: &str, args: &[HostArg]) -> R<Value> {
    let m = member.to_ascii_lowercase();
    let uid = active_uid(s).unwrap_or(this);
    match m.as_str() {
        "application" | "parent" => Ok(obj(XlObj::Application)),
        "thisworkbook" => Ok(obj(XlObj::Workbook(this))),
        "activeworkbook" => Ok(obj(XlObj::Workbook(uid))),
        "workbooks" => match arg(args, 0, "Index") {
            Some(i) => workbooks_item(s, &i),
            None => Ok(obj(XlObj::Workbooks)),
        },
        "worksheets" | "sheets" => sheets_member(s, uid, false, "_Default", args, true),
        "activesheet" => Ok(obj(XlObj::Sheet { doc: uid, name: active_sheet_name(s)? })),
        "activecell" => {
            let d = s.doc().map_err(eng)?;
            range_obj(s, uid, d.wb.active_sheet, vec![RangeRef::cell(d.selection.active)])
        }
        "selection" => {
            let d = s.doc().map_err(eng)?;
            range_obj(s, uid, d.wb.active_sheet, d.selection.ranges.clone())
        }
        "range" => {
            let si = s.doc().map_err(eng)?.wb.active_sheet;
            range_call(s, uid, si, args)
        }
        "cells" => {
            let si = s.doc().map_err(eng)?.wb.active_sheet;
            cells_call(s, uid, si, RangeRef::all(), args)
        }
        "rows" | "columns" => {
            let si = s.doc().map_err(eng)?.wb.active_sheet;
            let all = range_obj(s, uid, si, vec![RangeRef::all()])?;
            let Value::Object(Object::Host(h)) = &all else { return Ok(all) };
            range_member(s, xl(h)?, member, args)
        }
        "worksheetfunction" => Ok(obj(XlObj::WorksheetFunction)),
        "activewindow" => Ok(obj(XlObj::Window(uid))),
        "names" => Ok(obj(XlObj::Names(uid))),
        "screenupdating" | "displayalerts" | "enableevents" | "interactive" | "visible" | "displaystatusbar" => Ok(Value::Bool(true)),
        "statusbar" => Ok(Value::Bool(false)),
        "calculation" => Ok(Value::Int(-4105)),
        "cutcopymode" => Ok(Value::Bool(s.clipboard.is_some())),
        "version" => Ok(Value::from("16.0")),
        "name" => Ok(Value::from("Microsoft Excel")),
        "operatingsystem" => Ok(Value::from(std::env::consts::OS)),
        "username" => Ok(Value::from(s.prefs.user_name.clone())),
        "pathseparator" => Ok(Value::from("/")),
        "international" => Ok(Value::Empty),
        "calculate" | "calculatefull" => {
            let _ = s.execute("formulas.calculateNow", json!({}));
            Ok(Value::Empty)
        }
        "wait" | "volatile" | "goto" | "ontime" => Ok(Value::Empty),
        "onkey" => {
            let key = arg(args, 0, "Key").map(|v| v.to_text()).transpose()?.unwrap_or_default();
            let procedure = arg(args, 1, "Procedure").map(|v| v.to_text()).transpose()?;
            crate::vba::set_on_key(s, this, &key, procedure);
            Ok(Value::Empty)
        }
        "evaluate" => {
            let f = arg(args, 0, "Name").map(|v| v.to_text()).transpose()?.unwrap_or_default();
            evaluate(s, uid, &f)
        }
        "intersect" | "union" => {
            let mut sheet = None;
            let mut acc: Option<Vec<RangeRef>> = None;
            for a in args {
                let Value::Object(Object::Host(h)) = &a.value else { continue };
                let XlObj::Range { sheet: sh, areas, .. } = xl(h)? else { continue };
                sheet = Some(sh.clone());
                acc = Some(match (acc, m.as_str()) {
                    (None, _) => areas.clone(),
                    (Some(prev), "union") => prev.into_iter().chain(areas.iter().copied()).collect(),
                    (Some(prev), _) => prev.iter().flat_map(|p| areas.iter().filter_map(move |q| p.intersection(q))).collect(),
                });
            }
            match (sheet, acc) {
                (Some(sheet), Some(areas)) if !areas.is_empty() => Ok(obj(XlObj::Range { doc: uid, sheet, areas })),
                _ => Ok(Value::Nothing),
            }
        }
        "quit" => Err(VbaError::Stopped),
        _ => Err(no_member(o, member)),
    }
}

fn evaluate(s: &Session, uid: u64, f: &str) -> R<Value> {
    let wb = wb_of(s, uid)?;
    let si = wb.active_sheet;
    // A plain reference evaluates to the range.
    if let Ok((sheet, areas)) = parse_ref(wb, si, f, 0) {
        return range_obj(s, uid, sheet, areas);
    }
    let v = gridcraft_calc::evaluate(wb, si, CellRef::new(0, 0), &format!("={}", f.trim_start_matches('=')));
    Ok(core_to_vba(&v))
}

fn core_to_vba(v: &CValue) -> Value {
    match v {
        CValue::Empty => Value::Empty,
        CValue::Number(n) => Value::Double(*n),
        CValue::Text(t) => Value::Str(t.to_string().into()),
        CValue::Bool(b) => Value::Bool(*b),
        CValue::Error(e) => Value::Error(error_number(*e)),
        CValue::Array(a) => {
            let (h, w) = (a.rows, a.cols);
            match Array::new(vec![(1, h), (1, w)]) {
                Ok(mut arr) => {
                    let w = w.max(1);
                    for (i, x) in a.data.iter().enumerate() {
                        if let Some(o) = arr.offset(&[(i / w) as i64 + 1, (i % w) as i64 + 1])
                            && let Some(slot) = arr.data.get_mut(o)
                        {
                            *slot = core_to_vba(x);
                        }
                    }
                    Value::Array(Arc::new(arr))
                }
                Err(_) => Value::Error(2015),
            }
        }
    }
}

fn workbooks_item(s: &Session, i: &Value) -> R<Value> {
    let docs = s.documents();
    let d = match i {
        Value::Str(name) => docs.iter().find(|d| d.title.eq_ignore_ascii_case(name) || d.path.as_deref().is_some_and(|p| p.ends_with(&**name))),
        v => usize::try_from(v.to_int()? - 1).ok().and_then(|k| docs.get(k)),
    };
    d.map(|d| obj(XlObj::Workbook(d.uid))).ok_or_else(|| VbaError::runtime(9, "Subscript out of range"))
}

// ---------------------------------------------------------------- dispatch

/// A property read or method call on a model object.
pub fn get(s: &mut Session, this: u64, h: &HostObj, member: &str, args: &[HostArg]) -> R<Value> {
    let o = xl(h)?.clone();
    match &o {
        XlObj::Application => application(s, this, &o, member, args),
        XlObj::Workbooks => match member.to_ascii_lowercase().as_str() {
            "count" => Ok(Value::Int(s.documents().len() as i64)),
            "item" | "_default" => workbooks_item(s, &arg(args, 0, "Index").unwrap_or_default()),
            "add" => {
                s.new_workbook();
                Ok(obj(XlObj::Workbook(active_uid(s)?)))
            }
            _ => Err(no_member(&o, member)),
        },
        XlObj::Workbook(uid) => workbook_member(s, *uid, &o, member, args),
        XlObj::Sheets { doc, selected } => sheets_member(s, *doc, *selected, member, args, false),
        XlObj::Sheet { doc, name } => sheet_member(s, *doc, name, &o, member, args),
        XlObj::Range { .. } => range_member(s, &o, member, args),
        XlObj::Font(_) | XlObj::Interior(_) | XlObj::Borders(_) | XlObj::Border(..) => format_get(s, &o, member, args),
        XlObj::WorksheetFunction => worksheet_function(s, active_uid(s).unwrap_or(this), member, args),
        XlObj::Window(uid) => match member.to_ascii_lowercase().as_str() {
            "selectedsheets" => Ok(obj(XlObj::Sheets { doc: *uid, selected: true })),
            "activesheet" => Ok(obj(XlObj::Sheet { doc: *uid, name: active_sheet_name(s)? })),
            "activecell" | "selection" => application(s, this, &XlObj::Application, member, args),
            "zoom" => Ok(Value::Int(100)),
            "smallscroll" | "largescroll" | "scrollworkbooktabs" | "activatenext" | "activateprevious" => Ok(Value::Empty),
            "scrollrow" | "scrollcolumn" => Ok(Value::Int(1)),
            "freezepanes" | "displaygridlines" | "displayheadings" => Ok(Value::Bool(false)),
            _ => Err(no_member(&o, member)),
        },
        XlObj::Form(name) => crate::vba::form_get(s, this, name, member, args),
        XlObj::Control { form, name } => crate::vba::control_member(s, this, form, name, member, args, None),
        XlObj::SheetControl { doc, sheet, name } => crate::vba::sheet_control_get(s, *doc, sheet, name, member, args),
        XlObj::Names(uid) => match member.to_ascii_lowercase().as_str() {
            "count" => Ok(Value::Int(wb_of(s, *uid)?.names.len() as i64)),
            "add" => {
                let name = arg(args, 0, "Name").map(|v| v.to_text()).transpose()?.unwrap_or_default();
                let refers = arg(args, 1, "RefersTo").map(|v| v.to_text()).transpose()?.unwrap_or_default();
                with_doc(s, *uid, |s| {
                    s.execute("formulas.defineName", json!({"name": name, "refersTo": refers.trim_start_matches('=')})).map_err(eng)?;
                    Ok(Value::Empty)
                })
            }
            _ => Err(no_member(&o, member)),
        },
    }
}

/// A property assignment.
pub fn set(s: &mut Session, this: u64, h: &HostObj, member: &str, args: &[HostArg], value: Value) -> R<()> {
    let o = xl(h)?.clone();
    let m = member.to_ascii_lowercase();
    match &o {
        XlObj::Application => match m.as_str() {
            "cutcopymode" => {
                if !value.to_bool().unwrap_or(false) {
                    s.clipboard = None;
                }
                Ok(())
            }
            // Display and calculation switches have no effect here.
            "screenupdating" | "displayalerts" | "enableevents" | "calculation" | "statusbar" | "interactive" | "cursor" | "displaystatusbar"
            | "visible" | "displayformulabar" | "displayfullscreen" | "caption" | "windowstate" | "askToUpdateLinks" | "iteration" => Ok(()),
            _ => Err(no_member(&o, member)),
        },
        XlObj::Range { doc, sheet, areas } => {
            let si = sheet_pos(s, *doc, sheet)?;
            match m.as_str() {
                "value" | "value2" | "_default" | "formula" | "formular1c1" | "formulalocal" => {
                    // `Range("A1:B2")(1, 2) = x`: arguments pick a cell first.
                    if !args.is_empty() {
                        let target = range_member(s, &o, "Item", args)?;
                        let Value::Object(Object::Host(t)) = target else { return Err(e1004("bad cell")) };
                        return set(s, this, &t, "Value", &[], value);
                    }
                    set_range_value(s, *doc, si, areas, &value)
                }
                "numberformat" | "numberformatlocal" => {
                    let code = value.to_text()?;
                    run_on_areas(s, *doc, si, areas, "home.numberFormat", |r| json!({"range": r, "code": code}))
                }
                "horizontalalignment" => {
                    let id = match value.to_int()? {
                        -4108 => "home.alignCenter",
                        -4152 => "home.alignRight",
                        -4131 => "home.alignLeft",
                        _ => return Ok(()),
                    };
                    run_on_areas(s, *doc, si, areas, id, |r| json!({ "range": r }))
                }
                "wraptext" => {
                    let on = value.to_bool()?;
                    run_on_areas(s, *doc, si, areas, "home.wrapText", |r| json!({"range": r, "on": on}))
                }
                "columnwidth" => {
                    let w = value.to_num()?;
                    let cols: Vec<String> = areas.iter().map(|r| format!("{}:{}", col_to_letters(r.start.col), col_to_letters(r.end.col))).collect();
                    on_sheet(s, *doc, si, "home.columnWidth", json!({"cols": cols.join(","), "chars": w})).map(|_| ())
                }
                "rowheight" => {
                    let h = value.to_num()?;
                    let rows: Vec<String> = areas.iter().map(|r| format!("{}:{}", r.start.row + 1, r.end.row + 1)).collect();
                    on_sheet(s, *doc, si, "home.rowHeight", json!({"rows": rows.join(","), "height": h})).map(|_| ())
                }
                "name" => {
                    let n = value.to_text()?;
                    let r = format!("{}!{}", quote_sheet(sheet), areas.first().map(|r| a1(r, true, true)).unwrap_or_default());
                    with_doc(s, *doc, |s| s.execute("formulas.defineName", json!({"name": n, "refersTo": r})).map(|_| ()).map_err(eng))
                }
                "hidden" | "entirerow.hidden" => Ok(()),
                "locked" | "mergecells" | "shrinktofit" | "indentlevel" | "verticalalignment" | "orientation" => Ok(()),
                _ => Err(no_member(&o, member)),
            }
        }
        XlObj::Font(_) | XlObj::Interior(_) | XlObj::Borders(_) | XlObj::Border(..) => format_set(s, &o, member, value),
        XlObj::Sheet { doc, name } => match m.as_str() {
            "name" => {
                let new = value.to_text()?;
                let si = sheet_pos(s, *doc, name)?;
                with_doc(s, *doc, |s| s.execute("sheet.rename", json!({"sheet": si, "name": new})).map(|_| ()).map_err(eng))
            }
            "visible" => {
                let si = sheet_pos(s, *doc, name)?;
                let hide = matches!(value, Value::Bool(false)) || matches!(value.to_int(), Ok(0) | Ok(2));
                with_doc(s, *doc, |s| s.execute(if hide { "sheet.hide" } else { "sheet.unhide" }, json!({"sheet": si})).map(|_| ()).map_err(eng))
            }
            _ => Err(no_member(&o, member)),
        },
        XlObj::Form(name) => crate::vba::form_set(s, this, name, member, value),
        XlObj::Control { form, name } => crate::vba::control_member(s, this, form, name, member, args, Some(value)).map(|_| ()),
        XlObj::SheetControl { doc, sheet, name } => crate::vba::sheet_control_set(s, *doc, sheet, name, member, value),
        XlObj::Window(_) => Ok(()),
        _ => Err(no_member(&o, member)),
    }
}

/// Asks the UI to print the active sheet (or show its preview). A headless run ignores it.
fn request_print(s: &mut Session, uid: u64, preview: bool) {
    request_print_with(s, uid, preview, json!({}));
}

fn request_print_with(s: &mut Session, _uid: u64, preview: bool, params: Json) {
    s.ui_requests.push(crate::UiRequest::Dialog(if preview { "printPreview".into() } else { "macroPrint".into() }, params));
}

/// The objects `For Each` walks.
pub fn items(s: &mut Session, h: &HostObj) -> R<Vec<Value>> {
    let o = xl(h)?.clone();
    match &o {
        XlObj::Range { doc, sheet, areas } => {
            let si = sheet_pos(s, *doc, sheet)?;
            let wb = wb_of(s, *doc)?;
            let mut out = Vec::new();
            for r in areas {
                let r = trim_used(wb, si, *r);
                if r.count() > 2_000_000 {
                    return Err(VbaError::runtime(7, "Out of memory"));
                }
                out.extend(r.iter().map(|c| obj(XlObj::Range { doc: *doc, sheet: sheet.clone(), areas: vec![RangeRef::cell(c)] })));
            }
            Ok(out)
        }
        XlObj::Sheets { doc, .. } => Ok(wb_of(s, *doc)?.sheets.iter().map(|sh| obj(XlObj::Sheet { doc: *doc, name: sh.name.clone() })).collect()),
        XlObj::Workbooks => Ok(s.documents().iter().map(|d| obj(XlObj::Workbook(d.uid))).collect()),
        XlObj::Form(name) => crate::vba::form_controls(s, name),
        _ => Err(VbaError::runtime(438, format!("For Each is not supported on {}", o.class()))),
    }
}

fn run_on_areas(s: &mut Session, uid: u64, si: usize, areas: &[RangeRef], id: &str, params: impl Fn(String) -> Json) -> R<()> {
    for r in areas {
        on_sheet(s, uid, si, id, params(r.a1()))?;
    }
    Ok(())
}

// ---------------------------------------------------------------- workbook / sheets

fn workbook_member(s: &mut Session, uid: u64, o: &XlObj, member: &str, args: &[HostArg]) -> R<Value> {
    let m = member.to_ascii_lowercase();
    let d = s.documents().get(doc_pos(s, uid)?).ok_or_else(|| e1004("no workbook"))?;
    match m.as_str() {
        "name" => Ok(Value::from(d.title.clone())),
        "fullname" => Ok(Value::from(d.path.clone().unwrap_or_else(|| d.title.clone()))),
        "path" => {
            Ok(Value::from(d.path.as_deref().and_then(|p| std::path::Path::new(p).parent()).map(|p| p.display().to_string()).unwrap_or_default()))
        }
        "saved" => Ok(Value::Bool(!d.is_dirty())),
        "worksheets" | "sheets" => sheets_member(s, uid, false, "_Default", args, true),
        "activesheet" => {
            let name = d.wb.active().map(|sh| sh.name.clone()).unwrap_or_default();
            Ok(obj(XlObj::Sheet { doc: uid, name }))
        }
        "names" => Ok(obj(XlObj::Names(uid))),
        "application" | "parent" => Ok(obj(XlObj::Application)),
        "activate" => {
            let i = doc_pos(s, uid)?;
            s.set_active(i);
            Ok(Value::Empty)
        }
        "save" => with_doc(s, uid, |s| {
            s.execute("file.save", json!({})).map_err(eng)?;
            Ok(Value::Empty)
        }),
        "saveas" => {
            let path = arg(args, 0, "Filename").map(|v| v.to_text()).transpose()?.ok_or_else(|| e1004("SaveAs needs a file name"))?;
            with_doc(s, uid, |s| {
                s.execute("file.saveAs", json!({"path": path})).map_err(eng)?;
                Ok(Value::Empty)
            })
        }
        "close" => Ok(Value::Empty),
        "refreshall" | "protect" | "unprotect" => Ok(Value::Empty),
        _ => Err(no_member(o, member)),
    }
}

/// `Sheets(…)`/`Worksheets(…)`: with an index or name the sheet, else the collection's member.
fn sheets_member(s: &mut Session, uid: u64, selected: bool, member: &str, args: &[HostArg], from_call: bool) -> R<Value> {
    let m = member.to_ascii_lowercase();
    let wb = wb_of(s, uid)?;
    let names: Vec<String> =
        if selected { wb.active().map(|sh| vec![sh.name.clone()]).unwrap_or_default() } else { wb.sheets.iter().map(|sh| sh.name.clone()).collect() };
    match m.as_str() {
        "_default" | "item" => {
            let Some(i) = arg(args, 0, "Index") else {
                return if from_call {
                    Ok(obj(XlObj::Sheets { doc: uid, selected }))
                } else {
                    Err(VbaError::runtime(450, "Wrong number of arguments"))
                };
            };
            let name = match &i {
                Value::Str(n) => names.iter().find(|x| x.eq_ignore_ascii_case(n)).cloned(),
                v => usize::try_from(v.to_int()? - 1).ok().and_then(|k| names.get(k)).cloned(),
            };
            name.map(|name| obj(XlObj::Sheet { doc: uid, name })).ok_or_else(|| VbaError::runtime(9, "Subscript out of range"))
        }
        "count" => Ok(Value::Int(names.len() as i64)),
        "add" => {
            with_doc(s, uid, |s| s.execute("home.insertSheet", json!({})).map_err(eng))?;
            Ok(obj(XlObj::Sheet { doc: uid, name: active_sheet_name(s)? }))
        }
        "printout" | "printpreview" => {
            request_print(s, uid, m == "printpreview");
            Ok(Value::Empty)
        }
        "select" => Ok(Value::Empty),
        _ => Err(VbaError::runtime(438, format!("Object doesn't support this property or method: Sheets.{member}"))),
    }
}

fn sheet_member(s: &mut Session, uid: u64, name: &str, o: &XlObj, member: &str, args: &[HostArg]) -> R<Value> {
    let m = member.to_ascii_lowercase();
    let si = sheet_pos(s, uid, name)?;
    match m.as_str() {
        "name" | "_default" if args.is_empty() => Ok(Value::from(name.to_string())),
        "codename" => Ok(Value::from(crate::vba::code_name_of(s, uid, name).unwrap_or_else(|| name.to_string()))),
        "index" => Ok(Value::Int(si as i64 + 1)),
        "range" => range_call(s, uid, si, args),
        "cells" => cells_call(s, uid, si, RangeRef::all(), args),
        "rows" | "columns" => {
            let all = range_obj(s, uid, si, vec![RangeRef::all()])?;
            let Value::Object(Object::Host(h)) = &all else { return Ok(all) };
            range_member(s, xl(h)?, member, args)
        }
        "usedrange" => {
            let r = wb_of(s, uid)?.sheet(si).and_then(|sh| sh.used_range()).unwrap_or(RangeRef::cell(CellRef::new(0, 0)));
            range_obj(s, uid, si, vec![r])
        }
        "select" | "activate" => {
            with_doc(s, uid, |s| s.execute("sheet.activate", json!({"sheet": si})).map_err(eng))?;
            let i = doc_pos(s, uid)?;
            s.set_active(i);
            Ok(Value::Empty)
        }
        "visible" => Ok(Value::Int(match wb_of(s, uid)?.sheet(si).map(|sh| sh.visibility) {
            Some(gridcraft_model::Visibility::Hidden) => 0,
            Some(gridcraft_model::Visibility::VeryHidden) => 2,
            _ => -1,
        })),
        "parent" => Ok(obj(XlObj::Workbook(uid))),
        "application" => Ok(obj(XlObj::Application)),
        "paste" => {
            let at = arg(args, 0, "Destination");
            let at = match at {
                Some(Value::Object(Object::Host(h))) => match xl(&h)? {
                    XlObj::Range { areas, .. } => areas.first().map(|r| r.start.a1()),
                    _ => None,
                },
                _ => None,
            };
            let params = match at {
                Some(a) => json!({"at": a}),
                None => json!({}),
            };
            on_sheet(s, uid, si, "edit.paste", params)?;
            Ok(Value::Empty)
        }
        "delete" => {
            with_doc(s, uid, |s| s.execute("home.deleteSheet", json!({"sheet": si})).map_err(eng))?;
            Ok(Value::Empty)
        }
        "calculate" => {
            let _ = s.execute("formulas.calculateSheet", json!({}));
            Ok(Value::Empty)
        }
        "printout" | "printpreview" => {
            with_doc(s, uid, |s| s.execute("sheet.activate", json!({"sheet": si})).map_err(eng))?;
            request_print(s, uid, m == "printpreview");
            Ok(Value::Empty)
        }
        "protect" | "unprotect" | "showalldata" => {
            let id = match m.as_str() {
                "protect" => "review.protectSheet",
                "unprotect" => "review.unprotectSheet",
                _ => "data.clearFilter",
            };
            let pw = arg(args, 0, "Password").map(|v| v.to_text()).transpose()?;
            let params = match pw {
                Some(p) => json!({"password": p}),
                None => json!({}),
            };
            let _ = on_sheet(s, uid, si, id, params);
            Ok(Value::Empty)
        }
        "autofiltermode" => Ok(Value::Bool(wb_of(s, uid)?.sheet(si).is_some_and(|sh| sh.autofilter.is_some()))),
        "next" | "previous" => {
            let wb = wb_of(s, uid)?;
            let k = if m == "next" { si + 1 } else { si.wrapping_sub(1) };
            match wb.sheet(k) {
                Some(sh) => Ok(obj(XlObj::Sheet { doc: uid, name: sh.name.clone() })),
                None => Ok(Value::Nothing),
            }
        }
        _ => {
            // A control on the sheet (`Sheet1.ComboBox1`), else a member we don't model.
            if crate::vba::sheet_has_control(s, uid, si, member) {
                return Ok(obj(XlObj::SheetControl { doc: uid, sheet: name.to_string(), name: member.to_string() }));
            }
            Err(no_member(o, member))
        }
    }
}

// ---------------------------------------------------------------- ranges

fn range_member(s: &mut Session, o: &XlObj, member: &str, args: &[HostArg]) -> R<Value> {
    let XlObj::Range { doc, sheet, areas } = o else { return Err(no_member(o, member)) };
    let (uid, sheet) = (*doc, sheet.clone());
    let si = sheet_pos(s, uid, &sheet)?;
    let first = areas.first().copied().unwrap_or_default();
    let m = member.to_ascii_lowercase();
    let same = |areas: Vec<RangeRef>| obj(XlObj::Range { doc: uid, sheet: sheet.clone(), areas });
    match m.as_str() {
        "value" | "value2" | "_default" if args.is_empty() => range_value(wb_of(s, uid)?, si, first),
        "_default" | "item" => cells_call(s, uid, si, first, args),
        "text" => {
            let wb = wb_of(s, uid)?;
            Ok(Value::from(wb.sheet(si).map(|sh| crate::display::cell_text(wb, sh, first.start)).unwrap_or_default()))
        }
        "formula" | "formulalocal" | "formular1c1" => {
            let wb = wb_of(s, uid)?;
            let c = wb.sheet(si).and_then(|sh| sh.cell(first.start));
            Ok(Value::from(match c {
                Some(cell) => match &cell.formula {
                    Some(f) => format!("={}", f.text),
                    None => crate::display::value_text(wb, wb.sheet(si).ok_or_else(|| e1004("no sheet"))?, first.start),
                },
                None => String::new(),
            }))
        }
        "hasformula" => Ok(Value::Bool(wb_of(s, uid)?.sheet(si).and_then(|sh| sh.cell(first.start)).is_some_and(|c| c.formula.is_some()))),
        "numberformat" => {
            let wb = wb_of(s, uid)?;
            Ok(Value::from(wb.sheet(si).map(|sh| wb.styles.get(sh.style_id(first.start)).num_fmt.as_str().to_string()).unwrap_or_default()))
        }
        "address" => {
            let row_abs = arg(args, 0, "RowAbsolute").map(|v| v.to_bool()).transpose()?.unwrap_or(true);
            let col_abs = arg(args, 1, "ColumnAbsolute").map(|v| v.to_bool()).transpose()?.unwrap_or(true);
            let external = arg(args, 3, "External").map(|v| v.to_bool()).transpose()?.unwrap_or(false);
            let text = areas.iter().map(|r| a1(r, row_abs, col_abs)).collect::<Vec<_>>().join(",");
            Ok(Value::from(if external { format!("{}!{text}", quote_sheet(&sheet)) } else { text }))
        }
        "row" => Ok(Value::Int(i64::from(first.start.row) + 1)),
        "column" => Ok(Value::Int(i64::from(first.start.col) + 1)),
        "count" | "countlarge" => Ok(Value::Int(areas.iter().map(|r| r.count() as i64).sum())),
        "rows" | "columns" => {
            let rows = m == "rows";
            match arg(args, 0, "Index") {
                // `Columns("I:I")`, `Columns("B")`, `Rows("3:5")`: lines of the sheet by name.
                Some(Value::Str(t)) => {
                    let t = t.replace('$', "").to_ascii_uppercase();
                    let spec = if t.contains(':') { t.clone() } else { format!("{t}:{t}") };
                    let r = RangeRef::parse(&spec).ok_or_else(|| e1004(format!("Application-defined or object-defined error: `{t}`")))?;
                    Ok(same(vec![r]))
                }
                Some(i) => {
                    let k = i.to_int()? - 1;
                    let r = if rows {
                        let row = first.start.row as i64 + k;
                        RangeRef::new(
                            CellRef::new(row.clamp(0, MAX_ROWS as i64 - 1) as u32, first.start.col),
                            CellRef::new(row.clamp(0, MAX_ROWS as i64 - 1) as u32, first.end.col),
                        )
                    } else {
                        let col = (first.start.col as i64 + k).clamp(0, MAX_COLS as i64 - 1) as u32;
                        RangeRef::new(CellRef::new(first.start.row, col), CellRef::new(first.end.row, col))
                    };
                    Ok(same(vec![r]))
                }
                None => Ok(obj(XlObj::Range { doc: uid, sheet: sheet.clone(), areas: vec![first] })).map(|v| tag_lines(v, rows)),
            }
        }
        "entirerow" => Ok(same(areas.iter().map(|r| RangeRef::rows(r.start.row, r.end.row)).collect())),
        "entirecolumn" => Ok(same(areas.iter().map(|r| RangeRef::cols(r.start.col, r.end.col)).collect())),
        "offset" => {
            let dr = arg(args, 0, "RowOffset").map(|v| v.to_int()).transpose()?.unwrap_or(0);
            let dc = arg(args, 1, "ColumnOffset").map(|v| v.to_int()).transpose()?.unwrap_or(0);
            let shifted: Option<Vec<RangeRef>> = areas.iter().map(|r| Some(RangeRef::new(r.start.offset(dr, dc)?, r.end.offset(dr, dc)?))).collect();
            shifted.map(same).ok_or_else(|| e1004("Application-defined or object-defined error"))
        }
        "resize" => {
            let h = arg(args, 0, "RowSize").map(|v| v.to_int()).transpose()?.unwrap_or(i64::from(first.height()));
            let w = arg(args, 1, "ColumnSize").map(|v| v.to_int()).transpose()?.unwrap_or(i64::from(first.width()));
            if h < 1 || w < 1 {
                return Err(e1004("Application-defined or object-defined error"));
            }
            let end = first.start.offset(h - 1, w - 1).ok_or_else(|| e1004("Application-defined or object-defined error"))?;
            Ok(same(vec![RangeRef::new(first.start, end)]))
        }
        "cells" => cells_call(s, uid, si, first, args),
        "range" => {
            // `rng.Range("A1")` is relative to rng's top-left.
            let inner = range_call(s, uid, si, args)?;
            let Value::Object(Object::Host(h)) = &inner else { return Ok(inner) };
            match xl(h)? {
                XlObj::Range { areas: ia, .. } => {
                    let moved: Option<Vec<RangeRef>> = ia
                        .iter()
                        .map(|r| {
                            Some(RangeRef::new(
                                r.start.offset(i64::from(first.start.row), i64::from(first.start.col))?,
                                r.end.offset(i64::from(first.start.row), i64::from(first.start.col))?,
                            ))
                        })
                        .collect();
                    moved.map(same).ok_or_else(|| e1004("bad range"))
                }
                _ => Ok(inner),
            }
        }
        "end" => {
            let dir = arg(args, 0, "Direction").map(|v| v.to_int()).transpose()?.unwrap_or(-4121);
            let (dr, dc) = match dir {
                -4162 => (-1, 0),
                -4121 => (1, 0),
                -4159 => (0, -1),
                _ => (0, 1),
            };
            let wb = wb_of(s, uid)?;
            let sh = wb.sheet(si).ok_or_else(|| e1004("no sheet"))?;
            let c = crate::selection::jump(sh, first.start, dr, dc);
            range_obj(s, uid, si, vec![RangeRef::cell(c)])
        }
        "currentregion" => {
            let wb = wb_of(s, uid)?;
            let sh = wb.sheet(si).ok_or_else(|| e1004("no sheet"))?;
            Ok(same(vec![crate::selection::current_region(sh, first.start)]))
        }
        "areas" => match arg(args, 0, "Index") {
            Some(i) => {
                let k = usize::try_from(i.to_int()? - 1).map_err(|_| VbaError::runtime(9, "Subscript out of range"))?;
                areas.get(k).map(|r| same(vec![*r])).ok_or_else(|| VbaError::runtime(9, "Subscript out of range"))
            }
            None => Ok(same(areas.clone())),
        },
        "worksheet" | "parent" => Ok(obj(XlObj::Sheet { doc: uid, name: sheet.clone() })),
        "application" => Ok(obj(XlObj::Application)),
        "font" => Ok(obj(XlObj::Font(Box::new(o.clone())))),
        "interior" => Ok(obj(XlObj::Interior(Box::new(o.clone())))),
        "borders" => match arg(args, 0, "Index") {
            Some(i) => Ok(obj(XlObj::Border(Box::new(o.clone()), i.to_int()?))),
            None => Ok(obj(XlObj::Borders(Box::new(o.clone())))),
        },
        "select" | "activate" => {
            let i = doc_pos(s, uid)?;
            s.set_active(i);
            let prev = s.doc().map_err(eng)?.wb.active_sheet;
            if prev != si {
                s.execute("sheet.activate", json!({"sheet": si})).map_err(eng)?;
            }
            let text = areas.iter().map(|r| r.a1()).collect::<Vec<_>>().join(",");
            let mut params = json!({"range": text});
            if m == "activate" && areas.len() == 1 && first.is_single() {
                params = json!({"range": first.a1()});
            }
            s.execute("selection.set", params).map_err(eng)?;
            Ok(Value::Empty)
        }
        "copy" | "cut" => {
            let text = format!("{}!{}", quote_sheet(&sheet), areas.iter().map(|r| r.a1()).collect::<Vec<_>>().join(","));
            with_doc(s, uid, |s| s.execute(if m == "copy" { "edit.copy" } else { "edit.cut" }, json!({"range": text})).map_err(eng))?;
            if let Some(Value::Object(Object::Host(h))) = arg(args, 0, "Destination") {
                let XlObj::Range { doc: duid, sheet: dsheet, areas: dareas } = xl(&h)?.clone() else {
                    return Err(e1004("Destination must be a range"));
                };
                let dsi = sheet_pos(s, duid, &dsheet)?;
                let at = dareas.first().map(|r| r.start.a1()).unwrap_or_default();
                on_sheet(s, duid, dsi, "edit.paste", json!({"at": at}))?;
                s.clipboard = None;
            }
            Ok(Value::Empty)
        }
        "pastespecial" => {
            let what = match arg(args, 0, "Paste").map(|v| v.to_int()).transpose()?.unwrap_or(-4104) {
                -4163 => "values",
                -4123 => "formulas",
                -4122 => "formats",
                -4144 => "comments",
                12 => "valuesAndNumberFormats",
                11 => "formulasAndNumberFormats",
                7 => "allExceptBorders",
                8 => "columnWidths",
                _ => "all",
            };
            let operation = match arg(args, 1, "Operation").map(|v| v.to_int()).transpose()?.unwrap_or(-4142) {
                2 => "add",
                3 => "subtract",
                4 => "multiply",
                5 => "divide",
                _ => "none",
            };
            let skip = arg(args, 2, "SkipBlanks").map(|v| v.to_bool()).transpose()?.unwrap_or(false);
            let transpose = arg(args, 3, "Transpose").map(|v| v.to_bool()).transpose()?.unwrap_or(false);
            // Paste Special pastes at the range: select it on its sheet first.
            let i = doc_pos(s, uid)?;
            s.set_active(i);
            if s.doc().map_err(eng)?.wb.active_sheet != si {
                s.execute("sheet.activate", json!({"sheet": si})).map_err(eng)?;
            }
            s.execute("selection.set", json!({"range": first.a1()})).map_err(eng)?;
            s.execute("edit.pasteSpecial", json!({"what": what, "operation": operation, "skipBlanks": skip, "transpose": transpose})).map_err(eng)?;
            Ok(Value::Empty)
        }
        "clearcontents" | "clear" | "clearformats" | "clearcomments" => {
            let id = match m.as_str() {
                "clearcontents" => "edit.clearContents",
                "clear" => "edit.clearAll",
                "clearformats" => "edit.clearFormats",
                _ => "edit.clearComments",
            };
            for r in areas {
                on_sheet(s, uid, si, id, json!({"range": r.a1()}))?;
            }
            Ok(Value::Empty)
        }
        "delete" | "insert" => {
            let shift = arg(args, 0, "Shift").map(|v| v.to_int()).transpose()?;
            for r in areas.iter().rev() {
                let (id, params) = if r.is_full_rows() {
                    (
                        if m == "delete" { "home.deleteRows" } else { "home.insertRows" },
                        json!({"rows": format!("{}:{}", r.start.row + 1, r.end.row + 1)}),
                    )
                } else if r.is_full_cols() {
                    (
                        if m == "delete" { "home.deleteColumns" } else { "home.insertColumns" },
                        json!({"cols": format!("{}:{}", col_to_letters(r.start.col), col_to_letters(r.end.col))}),
                    )
                } else {
                    let shift = match (m.as_str(), shift) {
                        ("delete", Some(-4159)) => "left",
                        ("delete", _) => "up",
                        (_, Some(-4161)) => "right",
                        _ => "down",
                    };
                    (if m == "delete" { "home.deleteCells" } else { "home.insertCells" }, json!({"range": r.a1(), "shift": shift}))
                };
                on_sheet(s, uid, si, id, params)?;
            }
            Ok(Value::Empty)
        }
        "find" | "findnext" => find(s, uid, si, &sheet, first, &m, args),
        "removeduplicates" => {
            let cols: Vec<i64> = match arg(args, 0, "Columns") {
                Some(Value::Array(a)) => a.data.iter().map(|v| v.to_int()).collect::<R<_>>()?,
                Some(v) => vec![v.to_int()?],
                None => vec![1],
            };
            let header = arg(args, 1, "Header").map(|v| v.to_int()).transpose()?.unwrap_or(2) == 1;
            let wb = wb_of(s, uid)?;
            let r = trim_used(wb, si, first);
            let columns: Vec<String> = cols.iter().filter_map(|c| u32::try_from(r.start.col as i64 + c - 1).ok()).map(col_to_letters).collect();
            on_sheet(s, uid, si, "data.removeDuplicates", json!({"range": r.a1(), "columns": columns, "header": header}))?;
            Ok(Value::Empty)
        }
        "sort" => {
            let key = arg(args, 0, "Key1");
            let order = arg(args, 1, "Order1").map(|v| v.to_int()).transpose()?.unwrap_or(1);
            let column = match key {
                Some(Value::Object(Object::Host(h))) => match xl(&h)? {
                    XlObj::Range { areas, .. } => areas.first().map(|r| col_to_letters(r.start.col)),
                    _ => None,
                },
                _ => None,
            };
            let mut params = json!({"range": trim_used(wb_of(s, uid)?, si, first).a1()});
            if let Some(c) = column {
                params["column"] = Json::String(c);
            }
            on_sheet(s, uid, si, if order == 2 { "data.sortDescending" } else { "data.sortAscending" }, params)?;
            Ok(Value::Empty)
        }
        "autofit" => Ok(Value::Empty),
        "merge" | "unmerge" => {
            on_sheet(s, uid, si, if m == "merge" { "home.mergeCells" } else { "home.unmergeCells" }, json!({"range": first.a1()}))?;
            Ok(Value::Empty)
        }
        "filldown" | "fillright" => {
            on_sheet(s, uid, si, if m == "filldown" { "edit.fillDown" } else { "edit.fillRight" }, json!({"range": first.a1()}))?;
            Ok(Value::Empty)
        }
        "calculate" | "show" | "autofilter" => Ok(Value::Empty),
        "printout" | "printpreview" => {
            let mut params = json!({});
            if !(first.is_full_cols() && first.is_full_rows()) {
                params["range"] = Json::String(first.a1());
            }
            request_print_with(s, uid, m == "printpreview", params);
            Ok(Value::Empty)
        }
        "specialcells" => {
            let kind = arg(args, 0, "Type").map(|v| v.to_int()).transpose()?.unwrap_or(11);
            let wb = wb_of(s, uid)?;
            let used = wb.sheet(si).and_then(|sh| sh.used_range()).unwrap_or(RangeRef::cell(CellRef::new(0, 0)));
            match kind {
                11 => range_obj(s, uid, si, vec![RangeRef::cell(used.end)]),
                _ => Ok(same(vec![used])),
            }
        }
        "mergecells" => Ok(Value::Bool(wb_of(s, uid)?.sheet(si).is_some_and(|sh| sh.merges.iter().any(|mm| mm.contains(first.start))))),
        "columnwidth" => Ok(Value::Double(wb_of(s, uid)?.sheet(si).map_or(8.43, |sh| f64::from(sh.col_width(first.start.col))))),
        "rowheight" => Ok(Value::Double(wb_of(s, uid)?.sheet(si).map_or(15.0, |sh| f64::from(sh.row_height(first.start.row))))),
        "horizontalalignment" => Ok(Value::Int(1)),
        "hidden" => Ok(Value::Bool(false)),
        "name" => Ok(Value::Empty),
        _ => Err(no_member(o, member)),
    }
}

/// `Rows`/`Columns` without an index is the range itself (for `.Count`, the collection's count):
/// marks which lines it counts by remembering the request on the object's class.
fn tag_lines(v: Value, rows: bool) -> Value {
    match v {
        Value::Object(Object::Host(h)) => match h.get::<XlObj>() {
            Some(XlObj::Range { doc, sheet, areas }) => {
                let lines = LinesObj { doc: *doc, sheet: sheet.clone(), area: areas.first().copied().unwrap_or_default(), rows };
                Value::Object(Object::Host(HostObj::new(if rows { "Rows" } else { "Columns" }, lines)))
            }
            _ => Value::Object(Object::Host(h)),
        },
        v => v,
    }
}

/// `Range.Rows` / `Range.Columns` (and the sheet's): a collection of lines.
#[derive(Clone, Debug)]
pub struct LinesObj {
    doc: u64,
    sheet: String,
    area: RangeRef,
    rows: bool,
}

/// Members of a lines collection: `Count`, an item, or any range member on the whole block.
pub fn lines_get(s: &mut Session, this: u64, l: &LinesObj, member: &str, args: &[HostArg]) -> R<Value> {
    let m = member.to_ascii_lowercase();
    match m.as_str() {
        "count" => Ok(Value::Int(i64::from(if l.rows { l.area.height() } else { l.area.width() }))),
        "_default" | "item" if !args.is_empty() => {
            let range = XlObj::Range { doc: l.doc, sheet: l.sheet.clone(), areas: vec![l.area] };
            range_member(s, &range, if l.rows { "Rows" } else { "Columns" }, args)
        }
        _ => {
            let range = obj(XlObj::Range { doc: l.doc, sheet: l.sheet.clone(), areas: vec![l.area] });
            let Value::Object(Object::Host(h)) = range else { return Err(e1004("bad range")) };
            get(s, this, &h, member, args)
        }
    }
}

/// `Range.Find(What, …, LookIn, LookAt, …, MatchCase)`: the first matching cell after the
/// range's first cell, row by row (`Nothing` when there is none).
fn find(s: &mut Session, uid: u64, si: usize, sheet: &str, scope: RangeRef, m: &str, args: &[HostArg]) -> R<Value> {
    // Find(What, After, LookIn, LookAt, SearchOrder, SearchDirection, MatchCase, …);
    // FindNext(After) repeats the last search from After.
    let next = m == "findnext";
    let (what, after) = if next {
        (s.macros.last_find.as_ref().and_then(|j| j.get("vbaWhat")).and_then(Json::as_str).unwrap_or("").to_string(), arg(args, 0, "After"))
    } else {
        (arg(args, 0, "What").map(|v| v.to_text()).transpose()?.unwrap_or_default(), arg(args, 1, "After"))
    };
    let opts = s.macros.last_find.clone().unwrap_or(Json::Null);
    let int_opt = |i: usize, n: &str, key: &str, d: i64| -> R<i64> {
        match (next, arg(args, i, n)) {
            (false, Some(v)) => v.to_int(),
            _ => Ok(opts.get(key).and_then(Json::as_i64).unwrap_or(d)),
        }
    };
    let look_at = int_opt(3, "LookAt", "vbaLookAt", 2)?;
    let look_in = int_opt(2, "LookIn", "vbaLookIn", -4163)?;
    let match_case = if next {
        opts.get("vbaMatchCase").and_then(Json::as_bool).unwrap_or(false)
    } else {
        arg(args, 6, "MatchCase").map(|v| v.to_bool()).transpose()?.unwrap_or(false)
    };
    s.macros.last_find = Some(json!({"vbaWhat": what, "vbaLookAt": look_at, "vbaLookIn": look_in, "vbaMatchCase": match_case}));
    let whole = look_at == 1;
    let formulas = look_in == -4123;
    let wb = wb_of(s, uid)?;
    let sh = wb.sheet(si).ok_or_else(|| e1004("no sheet"))?;
    let scope = trim_used(wb, si, scope);
    let fold = |t: &str| if match_case { t.to_string() } else { t.to_lowercase() };
    let needle = fold(&what);
    let hit = |c: CellRef| -> bool {
        let Some(cell) = sh.cell(c) else { return false };
        let text = match (&cell.formula, formulas) {
            (Some(f), true) => format!("={}", f.text),
            _ => crate::display::cell_text(wb, sh, c),
        };
        if text.is_empty() {
            return false;
        }
        let t = fold(&text);
        if whole { t == needle } else { t.contains(&needle) }
    };
    // Search the stored cells in the range, starting after its first cell, wrapping around.
    let cells: Vec<CellRef> = sh.cells.iter_range(scope).map(|(c, _)| c).collect();
    // The search starts after `After` (the range's first cell by default).
    let start = match &after {
        Some(Value::Object(Object::Host(h))) => match h.get::<XlObj>() {
            Some(XlObj::Range { areas, .. }) => areas.first().map_or(scope.start, |r| r.start),
            _ => scope.start,
        },
        _ => scope.start,
    };
    let after = cells.iter().position(|c| (c.row, c.col) > (start.row, start.col)).unwrap_or(cells.len());
    let found = cells.iter().skip(after).chain(cells.iter().take(after)).find(|c| hit(**c)).copied();
    match found {
        Some(c) => Ok(obj(XlObj::Range { doc: uid, sheet: sheet.to_string(), areas: vec![RangeRef::cell(c)] })),
        None => Ok(Value::Nothing),
    }
}

// ---------------------------------------------------------------- formatting

fn format_target(o: &XlObj) -> Option<(&u64, &String, &Vec<RangeRef>)> {
    let inner = match o {
        XlObj::Font(r) | XlObj::Interior(r) | XlObj::Borders(r) | XlObj::Border(r, _) => r.as_ref(),
        _ => return None,
    };
    match inner {
        XlObj::Range { doc, sheet, areas } => Some((doc, sheet, areas)),
        _ => None,
    }
}

fn format_get(s: &mut Session, o: &XlObj, member: &str, args: &[HostArg]) -> R<Value> {
    let (uid, sheet, areas) = format_target(o).ok_or_else(|| no_member(o, member))?;
    let si = sheet_pos(s, *uid, sheet)?;
    let wb = wb_of(s, *uid)?;
    let c = areas.first().map(|r| r.start).unwrap_or_default();
    let style = wb.sheet(si).map(|sh| wb.styles.get(sh.style_id(c)).clone()).unwrap_or_default();
    let m = member.to_ascii_lowercase();
    let color = |c: &gridcraft_model::Color| {
        let [r, g, b] = c.resolve(&wb.theme).unwrap_or([0, 0, 0]);
        Value::Int(i64::from(r) | (i64::from(g) << 8) | (i64::from(b) << 16))
    };
    Ok(match (o, m.as_str()) {
        (XlObj::Font(_), "bold") => Value::Bool(style.font.bold),
        (XlObj::Font(_), "italic") => Value::Bool(style.font.italic),
        (XlObj::Font(_), "size") => Value::Double(f64::from(style.font.size)),
        (XlObj::Font(_), "name") => Value::from(style.font.name.clone()),
        (XlObj::Font(_), "color") => color(&style.font.color),
        (XlObj::Interior(_), "color") => match style.fill.pattern {
            gridcraft_model::PatternType::None => Value::Int(0xFF_FFFF),
            _ => color(&style.fill.fg),
        },
        (XlObj::Interior(_), "colorindex") => Value::Int(if style.fill.pattern == gridcraft_model::PatternType::None { -4142 } else { 1 }),
        (XlObj::Borders(_), "item" | "_default") => match arg(args, 0, "Index") {
            Some(i) => {
                let Some(XlObj::Borders(inner)) = Some(o) else { return Err(no_member(o, member)) };
                obj(XlObj::Border(inner.clone(), i.to_int()?))
            }
            None => Value::Empty,
        },
        (XlObj::Border(..) | XlObj::Borders(_), "linestyle") => Value::Int(-4142),
        (XlObj::Border(..) | XlObj::Borders(_), "weight") => Value::Int(2),
        (_, "color") => Value::Int(0),
        _ => return Err(no_member(o, member)),
    })
}

fn format_set(s: &mut Session, o: &XlObj, member: &str, value: Value) -> R<()> {
    let (uid, sheet, areas) = format_target(o).ok_or_else(|| no_member(o, member))?;
    let (uid, sheet, areas) = (*uid, sheet.clone(), areas.clone());
    let si = sheet_pos(s, uid, &sheet)?;
    let m = member.to_ascii_lowercase();
    let hex = |v: &Value| -> R<String> {
        let n = v.to_int()?;
        Ok(format!("#{:02X}{:02X}{:02X}", n & 0xFF, (n >> 8) & 0xFF, (n >> 16) & 0xFF))
    };
    match (o, m.as_str()) {
        (XlObj::Font(_), "bold") => run_on_areas(s, uid, si, &areas, "home.bold", |r| json!({"range": r, "on": value.to_bool().unwrap_or(true)})),
        (XlObj::Font(_), "italic") => run_on_areas(s, uid, si, &areas, "home.italic", |r| json!({"range": r, "on": value.to_bool().unwrap_or(true)})),
        (XlObj::Font(_), "underline") => {
            run_on_areas(s, uid, si, &areas, "home.underline", |r| json!({"range": r, "on": !matches!(value.to_int(), Ok(-4142) | Ok(0))}))
        }
        (XlObj::Font(_), "size") => {
            let size = value.to_num()?;
            run_on_areas(s, uid, si, &areas, "home.fontSize", |r| json!({"range": r, "size": size}))
        }
        (XlObj::Font(_), "name") => {
            let name = value.to_text()?;
            run_on_areas(s, uid, si, &areas, "home.fontName", |r| json!({"range": r, "name": name}))
        }
        (XlObj::Font(_), "color") => {
            let c = hex(&value)?;
            run_on_areas(s, uid, si, &areas, "home.fontColor", |r| json!({"range": r, "color": c}))
        }
        (XlObj::Interior(_), "color") => {
            let c = hex(&value)?;
            run_on_areas(s, uid, si, &areas, "home.fillColor", |r| json!({"range": r, "color": c}))
        }
        (XlObj::Interior(_), "colorindex" | "pattern") => {
            if matches!(value.to_int(), Ok(-4142) | Ok(0)) {
                run_on_areas(s, uid, si, &areas, "home.fillColor", |r| json!({"range": r, "color": "none"}))
            } else {
                Ok(())
            }
        }
        (XlObj::Border(..) | XlObj::Borders(_), "linestyle" | "weight") => {
            let which = if let XlObj::Border(_, w) = o { Some(*w) } else { None };
            let none = m == "linestyle" && matches!(value.to_int(), Ok(-4142));
            let style = match (m.as_str(), value.to_int().unwrap_or(1)) {
                ("weight", -4138) => "medium",
                ("weight", 4) => "thick",
                ("weight", 1) => "hair",
                ("linestyle", -4115) => "dashed",
                ("linestyle", -4118) => "dotted",
                ("linestyle", -4119) => "double",
                _ => "thin",
            };
            let preset = match (which, none) {
                (_, true) if which.is_none() => "none",
                (Some(8), _) => "top",
                (Some(9), _) => "bottom",
                (Some(7), _) => "left",
                (Some(10), _) => "right",
                (Some(11), _) => "insideVertical",
                (Some(12), _) => "insideHorizontal",
                (Some(5), _) => "diagonalDown",
                (Some(6), _) => "diagonalUp",
                _ => "all",
            };
            if none {
                // Removing one edge: GridCraft clears the range's borders on that side.
                return run_on_areas(s, uid, si, &areas, "home.borders", |r| json!({"range": r, "preset": preset, "style": "none"}));
            }
            run_on_areas(s, uid, si, &areas, "home.borders", |r| json!({"range": r, "preset": preset, "style": style}))
        }
        (XlObj::Border(..) | XlObj::Borders(_), "color" | "colorindex" | "themecolor" | "tintandshade") => Ok(()),
        (_, "themecolor" | "tintandshade" | "patterncolorindex" | "fontstyle" | "strikethrough" | "superscript" | "subscript") => Ok(()),
        _ => Err(no_member(o, member)),
    }
}

// ---------------------------------------------------------------- worksheet functions

/// `WorksheetFunction.F(args)`: evaluated as the formula `=F(args)`, with ranges passed as
/// references.
fn worksheet_function(s: &mut Session, uid: u64, name: &str, args: &[HostArg]) -> R<Value> {
    let wb = wb_of(s, uid)?;
    let mut parts = Vec::with_capacity(args.len());
    for a in args {
        parts.push(match &a.value {
            Value::Object(Object::Host(h)) => match xl(h)? {
                XlObj::Range { sheet, areas, .. } => {
                    let r = areas.iter().map(|r| format!("{}!{}", quote_sheet(sheet), a1(r, true, true))).collect::<Vec<_>>();
                    if r.len() == 1 { r.into_iter().next().unwrap_or_default() } else { format!("({})", r.join(",")) }
                }
                o => return Err(no_member(o, name)),
            },
            Value::Str(t) => format!("\"{}\"", t.replace('"', "\"\"")),
            Value::Bool(b) => if *b { "TRUE" } else { "FALSE" }.to_string(),
            Value::Empty | Value::Missing => String::new(),
            Value::Int(n) => n.to_string(),
            Value::Double(n) | Value::Date(n) => format!("{n:?}"),
            Value::Array(arr) => {
                let items: R<Vec<String>> = arr
                    .data
                    .iter()
                    .map(|v| match v {
                        Value::Str(t) => Ok(format!("\"{}\"", t.replace('"', "\"\""))),
                        v => Ok(v.to_num()?.to_string()),
                    })
                    .collect();
                format!("{{{}}}", items?.join(","))
            }
            _ => return Err(VbaError::runtime(13, "Type mismatch")),
        });
    }
    let si = wb.active_sheet;
    let v = gridcraft_calc::evaluate(wb, si, CellRef::new(0, 0), &format!("={}({})", name, parts.join(",")));
    match v {
        CValue::Error(e) => Err(e1004(format!("Unable to get the {name} property of the WorksheetFunction class ({})", e.as_str()))),
        v => Ok(core_to_vba(&v)),
    }
}

// ---------------------------------------------------------------- forms bridge

/// A form's control as a model object.
pub fn control_obj(form: &str, name: &str) -> Value {
    obj(XlObj::Control { form: form.to_string(), name: name.to_string() })
}

pub fn form_obj(name: &str) -> Value {
    obj(XlObj::Form(name.to_string()))
}

/// Reads or writes a control member, returning the events the change fires.
pub fn control_access(
    form: &mut FormState,
    name: &str,
    member: &str,
    args: &[HostArg],
    value: Option<Value>,
) -> R<(Value, Vec<gridcraft_vba::Event>)> {
    let fname = form.name().to_string();
    let i = form.control(name).ok_or_else(|| VbaError::runtime(438, format!("Could not find the specified object: {name}")))?;
    let c: &mut ControlState = form.controls.get_mut(i).ok_or_else(|| VbaError::runtime(438, "no control"))?;
    match (member.to_ascii_lowercase().as_str(), &value) {
        ("setfocus", None) => {
            form.focus = Some(i);
            Ok((Value::Empty, vec![]))
        }
        (_, None) if !matches!(member.to_ascii_lowercase().as_str(), "additem" | "removeitem" | "clear") => {
            control_get(c, member, args).map(|v| (v, vec![]))
        }
        _ => control_call(&fname, c, member, args, value),
    }
}
