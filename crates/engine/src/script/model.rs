//! The object model scripts see, shaped like Excel's Office Scripts API: `Workbook`,
//! `Worksheet`, `Range` with its `RangeFormat`, `RangeFont` and `RangeFill`, `NamedItem`, plus
//! `run(id, params)` for any command and `console.log`.
//!
//! Methods keep Office Scripts' names (`getRange`, `setValues`, `getFormat().getFont().setBold`)
//! and its conventions: row and column indexes are 0-based, addresses are A1-style, an empty cell
//! reads as `""`, and getters of a multi-cell range report its top-left cell. Writes go through
//! engine commands, reads straight to the model.

use gridcraft_core::{CellRef, MAX_COLS, MAX_ROWS, RangeRef};
use gridcraft_model::{HAlign, Underline, VAlign, Visibility};
use rhai::{Array, Dynamic, Engine, INT, Map};
use serde_json::{Value as Json, json};

use super::convert::{self, cell_value, number};
use super::{Host, Res, fail, no_sheet};

/// Most cells a script reads or writes in one call.
const MAX_CELLS: u64 = 1_000_000;

// ---------------------------------------------------------------- types

#[derive(Clone)]
pub(crate) struct Workbook {
    h: Host,
}

#[derive(Clone)]
pub(crate) struct Worksheet {
    h: Host,
    name: String,
}

#[derive(Clone)]
pub(crate) struct Range {
    h: Host,
    sheet: String,
    r: RangeRef,
}

#[derive(Clone)]
pub(crate) struct RangeFormat(Range);

#[derive(Clone)]
pub(crate) struct RangeFont(Range);

#[derive(Clone)]
pub(crate) struct RangeFill(Range);

#[derive(Clone)]
pub(crate) struct NamedItem {
    h: Host,
    name: String,
    /// Sheet index for a sheet-scoped name.
    scope: Option<usize>,
}

/// `console`: `console.log(…)` and friends print to the script's output.
#[derive(Clone)]
pub(crate) struct Console;

impl Workbook {
    pub(crate) fn new(h: Host) -> Workbook {
        Workbook { h }
    }
}

/// An object of the object model as text (`print(range)`, values passed to commands).
pub(crate) fn describe(v: &Dynamic) -> Option<String> {
    if let Some(r) = v.read_lock::<Range>() {
        return Some(r.address());
    }
    if let Some(w) = v.read_lock::<Worksheet>() {
        return Some(w.name.clone());
    }
    if let Some(n) = v.read_lock::<NamedItem>() {
        return Some(n.name.clone());
    }
    if v.is::<Workbook>() {
        return Some("Workbook".into());
    }
    if let Some(f) = v.read_lock::<RangeFormat>() {
        return Some(format!("Format of {}", f.0.address()));
    }
    if let Some(f) = v.read_lock::<RangeFont>() {
        return Some(format!("Font of {}", f.0.address()));
    }
    if let Some(f) = v.read_lock::<RangeFill>() {
        return Some(format!("Fill of {}", f.0.address()));
    }
    v.is::<Console>().then(|| "console".into())
}

// ---------------------------------------------------------------- helpers

/// An integer argument; whole floats count (values read from cells).
fn int(v: &Dynamic, what: &str) -> Res<i64> {
    if let Ok(i) = v.as_int() {
        return Ok(i);
    }
    match v.as_float() {
        Ok(f) if f.fract() == 0.0 && f.abs() < 9e15 => Ok(f as i64),
        _ => Err(fail(format!("{what} must be a whole number, not {}.", convert::text(v)))),
    }
}

/// A number argument.
fn num(v: &Dynamic, what: &str) -> Res<f64> {
    if let Ok(i) = v.as_int() {
        return Ok(i as f64);
    }
    v.as_float().map_err(|_| fail(format!("{what} must be a number, not {}.", convert::text(v))))
}

/// A cell at 0-based (row, column), if it is on the sheet.
fn cell_at(row: i64, col: i64) -> Res<CellRef> {
    if !(0..i64::from(MAX_ROWS)).contains(&row) || !(0..i64::from(MAX_COLS)).contains(&col) {
        return Err(fail(format!("Row {row}, column {col} is outside the worksheet.")));
    }
    Ok(CellRef::new(row as u32, col as u32))
}

/// Parses an A1 address for `sheet`: a `Sheet!` prefix must name that sheet.
fn parse_address(sheet: &str, address: &str) -> Res<RangeRef> {
    let (prefix, body) = crate::cmd::split_sheet(address.trim());
    if let Some(p) = prefix
        && !p.eq_ignore_ascii_case(sheet)
    {
        return Err(fail(format!("\"{address}\" is on another worksheet; use that worksheet's getRange.")));
    }
    RangeRef::parse(body).ok_or_else(|| fail(format!("\"{address}\" is not a cell or range address.")))
}

fn check_size(r: &RangeRef) -> Res<()> {
    if r.count() > MAX_CELLS {
        return Err(fail(format!("{} has more than {MAX_CELLS} cells; work on it in parts.", r.a1())));
    }
    Ok(())
}

fn h_align_name(a: HAlign) -> &'static str {
    match a {
        HAlign::General => "General",
        HAlign::Left => "Left",
        HAlign::Center => "Center",
        HAlign::Right => "Right",
        HAlign::Fill => "Fill",
        HAlign::Justify => "Justify",
        HAlign::CenterAcross => "CenterAcrossSelection",
        HAlign::Distributed => "Distributed",
    }
}

fn v_align_name(a: VAlign) -> &'static str {
    match a {
        VAlign::Top => "Top",
        VAlign::Center => "Center",
        VAlign::Bottom => "Bottom",
        VAlign::Justify => "Justify",
        VAlign::Distributed => "Distributed",
    }
}

fn underline_name(u: Underline) -> &'static str {
    match u {
        Underline::None => "None",
        Underline::Single => "Single",
        Underline::Double => "Double",
        Underline::SingleAccounting => "SingleAccountant",
        Underline::DoubleAccounting => "DoubleAccountant",
    }
}

// ---------------------------------------------------------------- workbook

impl Workbook {
    fn active_sheet(&self) -> Res<Worksheet> {
        let name = self.h.read(|d| d.wb.active().map(|s| s.name.clone()).ok_or_else(|| fail("The workbook has no worksheets.")))?;
        Ok(Worksheet { h: self.h.clone(), name })
    }

    fn sheet(&self, name: &str) -> Res<Dynamic> {
        let found = self.h.read(|d| Ok(d.wb.sheet_index(name).and_then(|i| d.wb.sheet(i)).map(|s| s.name.clone())))?;
        Ok(found.map_or(Dynamic::UNIT, |name| Dynamic::from(Worksheet { h: self.h.clone(), name })))
    }

    fn sheets(&self) -> Res<Array> {
        let names = self.h.read(|d| Ok(d.wb.sheets.iter().map(|s| s.name.clone()).collect::<Vec<_>>()))?;
        Ok(names.into_iter().map(|name| Dynamic::from(Worksheet { h: self.h.clone(), name })).collect())
    }

    fn add_sheet(&self, name: Option<&str>) -> Res<Worksheet> {
        // Office Scripts adds at the end and leaves the active sheet as it was.
        let (count, active) = self.h.read(|d| Ok((d.wb.sheets.len(), d.wb.active().map(|s| s.name.clone()))))?;
        let mut p = json!({"before": count});
        if let Some(n) = name {
            p["name"] = json!(n);
        }
        let r = self.h.exec("home.insertSheet", p)?;
        let name = r.get("name").and_then(Json::as_str).unwrap_or_default().to_string();
        if let Some(a) = active {
            self.h.exec("sheet.activate", json!({"sheet": a}))?;
        }
        Ok(Worksheet { h: self.h.clone(), name })
    }

    fn selected_range(&self) -> Res<Range> {
        let (sheet, r) = self.h.read(|d| Ok((d.wb.active().map(|s| s.name.clone()).unwrap_or_default(), d.selection.current())))?;
        Ok(Range { h: self.h.clone(), sheet, r })
    }

    fn active_cell(&self) -> Res<Range> {
        let (sheet, c) = self.h.read(|d| Ok((d.wb.active().map(|s| s.name.clone()).unwrap_or_default(), d.selection.active)))?;
        Ok(Range { h: self.h.clone(), sheet, r: RangeRef::cell(c) })
    }

    fn named_item(&self, name: &str) -> Res<Dynamic> {
        let found = self.h.read(|d| {
            let si = d.wb.active_sheet;
            Ok(d.wb.name(name, si).filter(|n| !n.hidden).map(|n| (n.name.clone(), n.scope)))
        })?;
        Ok(found.map_or(Dynamic::UNIT, |(name, scope)| Dynamic::from(NamedItem { h: self.h.clone(), name, scope })))
    }

    fn names(&self) -> Res<Array> {
        let list = self.h.read(|d| Ok(d.wb.names.iter().filter(|n| !n.hidden).map(|n| (n.name.clone(), n.scope)).collect::<Vec<_>>()))?;
        Ok(list.into_iter().map(|(name, scope)| Dynamic::from(NamedItem { h: self.h.clone(), name, scope })).collect())
    }
}

// ---------------------------------------------------------------- worksheet

impl Worksheet {
    fn index(&self) -> Res<usize> {
        self.h.read(|d| d.wb.sheet_index(&self.name).ok_or_else(|| fail(no_sheet(&self.name))))
    }

    fn range(&self, r: RangeRef) -> Range {
        Range { h: self.h.clone(), sheet: self.name.clone(), r }
    }

    fn get_range(&self, address: &str) -> Res<Range> {
        self.index()?;
        Ok(self.range(parse_address(&self.name, address)?))
    }

    fn used_range(&self) -> Res<Dynamic> {
        let i = self.index()?;
        let used = self.h.read(|d| Ok(d.wb.sheet(i).and_then(|s| s.used_range())))?;
        Ok(used.map_or(Dynamic::UNIT, |r| Dynamic::from(self.range(r))))
    }

    fn set_name(&mut self, name: &str) -> Res<()> {
        self.h.exec("sheet.rename", json!({"sheet": self.name, "name": name}))?;
        self.name = name.to_string();
        Ok(())
    }

    fn visibility(&self) -> Res<String> {
        let i = self.index()?;
        self.h.read(|d| {
            Ok(match d.wb.sheet(i).map(|s| s.visibility) {
                Some(Visibility::Hidden) => "Hidden",
                Some(Visibility::VeryHidden) => "VeryHidden",
                _ => "Visible",
            }
            .to_string())
        })
    }

    fn set_visibility(&self, v: &str) -> Res<()> {
        match v.to_ascii_lowercase().as_str() {
            "visible" => self.h.exec("sheet.unhide", json!({"sheet": self.name})).map(drop),
            "hidden" | "veryhidden" => self.h.exec("sheet.hide", json!({"sheet": self.name})).map(drop),
            _ => Err(fail(format!("Visibility is \"Visible\" or \"Hidden\", not \"{v}\"."))),
        }
    }

    fn tab_color(&self) -> Res<String> {
        let i = self.index()?;
        self.h.read(|d| Ok(d.wb.sheet(i).and_then(|s| s.tab_color).and_then(|c| c.hex(&d.wb.theme)).unwrap_or_default()))
    }
}

// ---------------------------------------------------------------- range

impl Range {
    fn address(&self) -> String {
        format!("{}!{}", gridcraft_formula::quote_sheet(&self.sheet), self.r.a1())
    }

    fn with(&self, r: RangeRef) -> Range {
        Range { h: self.h.clone(), sheet: self.sheet.clone(), r }
    }

    fn sheet_index(&self) -> Res<usize> {
        self.h.read(|d| d.wb.sheet_index(&self.sheet).ok_or_else(|| fail(no_sheet(&self.sheet))))
    }

    /// Runs a command on this range (its sheet active for the call).
    fn exec(&self, id: &str, mut params: Json) -> Res<Json> {
        if let Some(o) = params.as_object_mut() {
            o.entry("range").or_insert_with(|| json!(self.r.a1()));
        }
        self.h.exec_on(&self.sheet, id, params)
    }

    /// One value per cell, row by row.
    fn grid<T>(&self, f: impl Fn(&gridcraft_model::Workbook, &gridcraft_model::Sheet, CellRef) -> T) -> Res<Vec<Vec<T>>> {
        check_size(&self.r)?;
        let si = self.sheet_index()?;
        self.h.read(|d| {
            let sh = d.wb.sheet(si).ok_or_else(|| fail(no_sheet(&self.sheet)))?;
            Ok((self.r.start.row..=self.r.end.row)
                .map(|row| (self.r.start.col..=self.r.end.col).map(|col| f(&d.wb, sh, CellRef::new(row, col))).collect())
                .collect())
        })
    }

    /// The top-left cell's value of `f`.
    fn first<T>(&self, f: impl Fn(&gridcraft_model::Workbook, &gridcraft_model::Sheet, CellRef) -> T) -> Res<T> {
        let si = self.sheet_index()?;
        self.h.read(|d| {
            let sh = d.wb.sheet(si).ok_or_else(|| fail(no_sheet(&self.sheet)))?;
            Ok(f(&d.wb, sh, self.r.start))
        })
    }

    fn style(&self) -> Res<gridcraft_model::Style> {
        self.first(|wb, sh, c| wb.styles.get(sh.style_id(c)).clone())
    }

    fn values(&self) -> Res<Array> {
        Ok(self.grid(|_, sh, c| cell_value(&sh.value(c)))?.into_iter().map(Dynamic::from_array).collect())
    }

    fn formulas(&self) -> Res<Array> {
        let rows = self.grid(|_, sh, c| match sh.cell(c).and_then(|x| x.formula.as_ref()) {
            Some(f) => Dynamic::from(format!("={}", f.text)),
            None => cell_value(&sh.value(c)),
        })?;
        Ok(rows.into_iter().map(Dynamic::from_array).collect())
    }

    fn texts(&self) -> Res<Array> {
        let rows = self.grid(|wb, sh, c| Dynamic::from(crate::display::cell_text(wb, sh, c)))?;
        Ok(rows.into_iter().map(Dynamic::from_array).collect())
    }

    fn number_formats(&self) -> Res<Array> {
        let rows = self.grid(|wb, sh, c| Dynamic::from(wb.styles.get(sh.style_id(c)).num_fmt.as_str().to_string()))?;
        Ok(rows.into_iter().map(Dynamic::from_array).collect())
    }

    /// Writes a 2-D array that must have this range's shape (as in Office Scripts).
    fn set_grid(&self, rows: Array, what: &str) -> Res<()> {
        check_size(&self.r)?;
        let (h, w) = (self.r.height() as usize, self.r.width() as usize);
        if rows.len() != h {
            return Err(fail(format!("{what} needs {h} row(s) of {w} value(s) for {}, got {} row(s).", self.r.a1(), rows.len())));
        }
        let mut out = Vec::with_capacity(h);
        for (i, row) in rows.iter().enumerate() {
            let Some(cells) = row.read_lock::<Array>() else {
                return Err(fail(format!("{what} takes an array of rows, each an array of values (row {i} isn't).")));
            };
            if cells.len() != w {
                return Err(fail(format!("{what} needs {w} value(s) in each row for {}, row {i} has {}.", self.r.a1(), cells.len())));
            }
            out.push(Json::Array(cells.iter().map(convert::cell_input).collect::<Result<Vec<_>, _>>().map_err(fail)?));
        }
        self.exec("range.setValues", json!({"values": out})).map(drop)
    }

    /// Writes the same value into every cell.
    fn fill_with(&self, v: &Dynamic) -> Res<()> {
        check_size(&self.r)?;
        let one = convert::cell_input(v).map_err(fail)?;
        let row = Json::Array(vec![one; self.r.width() as usize]);
        self.exec("range.setValues", json!({"values": vec![row; self.r.height() as usize]})).map(drop)
    }

    fn offset(&self, dr: i64, dc: i64) -> Res<Range> {
        let s = cell_at(i64::from(self.r.start.row).saturating_add(dr), i64::from(self.r.start.col).saturating_add(dc))?;
        let e = cell_at(i64::from(self.r.end.row).saturating_add(dr), i64::from(self.r.end.col).saturating_add(dc))?;
        Ok(self.with(RangeRef::new(s, e)))
    }

    fn resized(&self, dr: i64, dc: i64) -> Res<Range> {
        let e = cell_at(i64::from(self.r.end.row).saturating_add(dr), i64::from(self.r.end.col).saturating_add(dc))?;
        if e.row < self.r.start.row || e.col < self.r.start.col {
            return Err(fail("A range can't be resized to fewer than one row and one column."));
        }
        Ok(self.with(RangeRef::new(self.r.start, e)))
    }

    fn cell(&self, row: i64, col: i64) -> Res<Range> {
        let c = cell_at(i64::from(self.r.start.row).saturating_add(row), i64::from(self.r.start.col).saturating_add(col))?;
        Ok(self.with(RangeRef::cell(c)))
    }

    fn row(&self, i: i64) -> Res<Range> {
        let s = self.cell(i, 0)?;
        Ok(self.with(RangeRef::new(s.r.start, CellRef::new(s.r.start.row, self.r.end.col))))
    }

    fn column(&self, i: i64) -> Res<Range> {
        let s = self.cell(0, i)?;
        Ok(self.with(RangeRef::new(s.r.start, CellRef::new(self.r.end.row, s.r.start.col))))
    }

    fn clear(&self, what: &str) -> Res<()> {
        let id = match what.to_ascii_lowercase().as_str() {
            "" | "all" => "edit.clearAll",
            "contents" => "edit.clearContents",
            "formats" => "edit.clearFormats",
            _ => return Err(fail(format!("clear takes \"All\", \"Contents\" or \"Formats\", not \"{what}\"."))),
        };
        self.exec(id, json!({})).map(drop)
    }

    fn rows_spec(&self) -> String {
        format!("{}:{}", self.r.start.row + 1, self.r.end.row + 1)
    }

    fn cols_spec(&self) -> String {
        format!("{}:{}", gridcraft_core::col_to_letters(self.r.start.col), gridcraft_core::col_to_letters(self.r.end.col))
    }
}

// ---------------------------------------------------------------- named items

impl NamedItem {
    fn formula(&self) -> Res<String> {
        self.h.read(|d| {
            d.wb.names
                .iter()
                .find(|n| n.scope == self.scope && n.name.eq_ignore_ascii_case(&self.name))
                .map(|n| format!("={}", n.formula))
                .ok_or_else(|| fail(format!("The name \"{}\" no longer exists.", self.name)))
        })
    }

    fn range(&self) -> Res<Range> {
        let f = self.formula()?;
        let body = f.trim_start_matches('=');
        let (sheet, addr) = crate::cmd::split_sheet(body);
        let sheet = match sheet {
            Some(s) => s,
            None => return Err(fail(format!("The name \"{}\" doesn't refer to a range ({f}).", self.name))),
        };
        let r = RangeRef::parse(addr).ok_or_else(|| fail(format!("The name \"{}\" doesn't refer to a single range ({f}).", self.name)))?;
        Ok(Range { h: self.h.clone(), sheet, r })
    }
}

// ---------------------------------------------------------------- registration

/// Registers the object model, `run` and `console` with the engine.
pub(crate) fn register(e: &mut Engine, host: &Host) {
    e.register_type_with_name::<Workbook>("Workbook")
        .register_type_with_name::<Worksheet>("Worksheet")
        .register_type_with_name::<Range>("Range")
        .register_type_with_name::<RangeFormat>("RangeFormat")
        .register_type_with_name::<RangeFont>("RangeFont")
        .register_type_with_name::<RangeFill>("RangeFill")
        .register_type_with_name::<NamedItem>("NamedItem")
        .register_type_with_name::<Console>("Console");
    register_text(e);
    register_globals(e, host);
    register_workbook(e);
    register_worksheet(e);
    register_range(e);
    register_format(e);
}

/// `print(x)` and string interpolation of the object model's types.
fn register_text(e: &mut Engine) {
    macro_rules! text {
        ($($t:ty),*) => {$(
            e.register_fn("to_string", |x: &mut $t| describe(&Dynamic::from(x.clone())).unwrap_or_default());
            e.register_fn("to_debug", |x: &mut $t| describe(&Dynamic::from(x.clone())).unwrap_or_default());
        )*};
    }
    text!(Workbook, Worksheet, Range, RangeFormat, RangeFont, RangeFill, NamedItem, Console);
}

fn register_globals(e: &mut Engine, host: &Host) {
    let h = host.clone();
    e.register_fn("run", move |id: &str| -> Res<Dynamic> { h.exec(id, json!({})).map(|r| convert::from_json(&r)) });
    let h = host.clone();
    e.register_fn("run", move |id: &str, params: Map| -> Res<Dynamic> {
        let p = convert::to_json(&Dynamic::from_map(params));
        h.exec(id, p).map(|r| convert::from_json(&r))
    });
    // console.log(a, b, …): up to six values, separated by spaces.
    for method in ["log", "info", "warn", "error"] {
        let prefix = match method {
            "warn" => "Warning: ",
            "error" => "Error: ",
            _ => "",
        };
        let line = move |parts: &[&Dynamic]| format!("{prefix}{}", parts.iter().map(|d| convert::text(d)).collect::<Vec<_>>().join(" "));
        let h = host.clone();
        e.register_fn(method, move |_: &mut Console| h.print(prefix.trim_end_matches(": ")));
        let h = host.clone();
        e.register_fn(method, move |_: &mut Console, a: Dynamic| h.print(&line(&[&a])));
        let h = host.clone();
        e.register_fn(method, move |_: &mut Console, a: Dynamic, b: Dynamic| h.print(&line(&[&a, &b])));
        let h = host.clone();
        e.register_fn(method, move |_: &mut Console, a: Dynamic, b: Dynamic, c: Dynamic| h.print(&line(&[&a, &b, &c])));
        let h = host.clone();
        e.register_fn(method, move |_: &mut Console, a: Dynamic, b: Dynamic, c: Dynamic, d: Dynamic| h.print(&line(&[&a, &b, &c, &d])));
        let h = host.clone();
        e.register_fn(method, move |_: &mut Console, a: Dynamic, b: Dynamic, c: Dynamic, d: Dynamic, f: Dynamic| {
            h.print(&line(&[&a, &b, &c, &d, &f]))
        });
        let h = host.clone();
        e.register_fn(method, move |_: &mut Console, a: Dynamic, b: Dynamic, c: Dynamic, d: Dynamic, f: Dynamic, g: Dynamic| {
            h.print(&line(&[&a, &b, &c, &d, &f, &g]))
        });
    }
}

fn register_workbook(e: &mut Engine) {
    e.register_fn("getActiveWorksheet", |w: &mut Workbook| w.active_sheet())
        .register_fn("getWorksheet", |w: &mut Workbook, name: &str| w.sheet(name))
        .register_fn("getWorksheets", |w: &mut Workbook| w.sheets())
        .register_fn("getFirstWorksheet", |w: &mut Workbook| -> Res<Worksheet> {
            let name = w.h.read(|d| d.wb.sheets.first().map(|s| s.name.clone()).ok_or_else(|| fail("The workbook has no worksheets.")))?;
            Ok(Worksheet { h: w.h.clone(), name })
        })
        .register_fn("getLastWorksheet", |w: &mut Workbook| -> Res<Worksheet> {
            let name = w.h.read(|d| d.wb.sheets.last().map(|s| s.name.clone()).ok_or_else(|| fail("The workbook has no worksheets.")))?;
            Ok(Worksheet { h: w.h.clone(), name })
        })
        .register_fn("addWorksheet", |w: &mut Workbook| w.add_sheet(None))
        .register_fn("addWorksheet", |w: &mut Workbook, name: &str| w.add_sheet(Some(name)))
        .register_fn("getSelectedRange", |w: &mut Workbook| w.selected_range())
        .register_fn("getActiveCell", |w: &mut Workbook| w.active_cell())
        .register_fn("getNamedItem", |w: &mut Workbook, name: &str| w.named_item(name))
        .register_fn("getNames", |w: &mut Workbook| w.names())
        .register_fn("addNamedItem", |w: &mut Workbook, name: &str, reference: &str| -> Res<NamedItem> {
            let refers = if reference.starts_with('=') { reference.to_string() } else { format!("={reference}") };
            w.h.exec("formulas.defineName", json!({"name": name, "refersTo": refers}))?;
            Ok(NamedItem { h: w.h.clone(), name: name.to_string(), scope: None })
        })
        .register_fn("getName", |w: &mut Workbook| w.h.read(|d| Ok(d.display_title())));
}

fn register_worksheet(e: &mut Engine) {
    e.register_fn("getName", |s: &mut Worksheet| s.name.clone())
        .register_fn("setName", |s: &mut Worksheet, name: &str| s.set_name(name))
        .register_fn("getPosition", |s: &mut Worksheet| s.index().map(|i| i as INT))
        .register_fn("activate", |s: &mut Worksheet| s.h.exec("sheet.activate", json!({"sheet": s.name})).map(drop))
        .register_fn("delete", |s: &mut Worksheet| s.h.exec("home.deleteSheet", json!({"sheet": s.name})).map(drop))
        .register_fn("getRange", |s: &mut Worksheet| -> Res<Range> {
            s.index()?;
            Ok(s.range(RangeRef::all()))
        })
        .register_fn("getRange", |s: &mut Worksheet, address: &str| s.get_range(address))
        .register_fn("getCell", |s: &mut Worksheet, row: Dynamic, col: Dynamic| -> Res<Range> {
            s.index()?;
            Ok(s.range(RangeRef::cell(cell_at(int(&row, "The row")?, int(&col, "The column")?)?)))
        })
        .register_fn("getRangeByIndexes", |s: &mut Worksheet, row: Dynamic, col: Dynamic, rows: Dynamic, cols: Dynamic| -> Res<Range> {
            s.index()?;
            let (r, c) = (int(&row, "The start row")?, int(&col, "The start column")?);
            let (n, m) = (int(&rows, "The row count")?, int(&cols, "The column count")?);
            if n < 1 || m < 1 {
                return Err(fail("A range has at least one row and one column."));
            }
            let start = cell_at(r, c)?;
            let end = cell_at(r.saturating_add(n - 1), c.saturating_add(m - 1))?;
            Ok(s.range(RangeRef::new(start, end)))
        })
        .register_fn("getUsedRange", |s: &mut Worksheet| s.used_range())
        .register_fn("getUsedRange", |s: &mut Worksheet, _values_only: bool| s.used_range())
        .register_fn("getVisibility", |s: &mut Worksheet| s.visibility())
        .register_fn("setVisibility", |s: &mut Worksheet, v: &str| s.set_visibility(v))
        .register_fn("getTabColor", |s: &mut Worksheet| s.tab_color())
        .register_fn("setTabColor", |s: &mut Worksheet, color: &str| {
            let color = if color.is_empty() { "none" } else { color };
            s.h.exec("sheet.tabColor", json!({"sheet": s.name, "color": color})).map(drop)
        });
    e.register_fn("getName", |n: &mut NamedItem| n.name.clone())
        .register_fn("getFormula", |n: &mut NamedItem| n.formula())
        .register_fn("getRange", |n: &mut NamedItem| n.range())
        .register_fn("getScope", |n: &mut NamedItem| if n.scope.is_some() { "Worksheet" } else { "Workbook" }.to_string())
        .register_fn("delete", |n: &mut NamedItem| -> Res<()> {
            let scope = match n.scope {
                Some(i) => json!(n.h.read(|d| Ok(d.wb.sheet(i).map(|s| s.name.clone()).unwrap_or_default()))?),
                None => json!("Workbook"),
            };
            n.h.exec("formulas.deleteName", json!({"name": n.name, "scope": scope})).map(drop)
        });
}

fn register_range(e: &mut Engine) {
    e.register_fn("getAddress", |r: &mut Range| r.address())
        .register_fn("getWorksheet", |r: &mut Range| Worksheet { h: r.h.clone(), name: r.sheet.clone() })
        .register_fn("getRowCount", |r: &mut Range| INT::from(r.r.height()))
        .register_fn("getColumnCount", |r: &mut Range| INT::from(r.r.width()))
        .register_fn("getCellCount", |r: &mut Range| INT::try_from(r.r.count()).unwrap_or(INT::MAX))
        .register_fn("getRowIndex", |r: &mut Range| INT::from(r.r.start.row))
        .register_fn("getColumnIndex", |r: &mut Range| INT::from(r.r.start.col))
        .register_fn("getValues", |r: &mut Range| r.values())
        .register_fn("setValues", |r: &mut Range, rows: Array| r.set_grid(rows, "setValues"))
        .register_fn("getValue", |r: &mut Range| r.first(|_, sh, c| cell_value(&sh.value(c))))
        .register_fn("setValue", |r: &mut Range, v: Dynamic| r.fill_with(&v))
        .register_fn("getFormulas", |r: &mut Range| r.formulas())
        .register_fn("setFormulas", |r: &mut Range, rows: Array| r.set_grid(rows, "setFormulas"))
        .register_fn("getFormula", |r: &mut Range| {
            r.first(|_, sh, c| match sh.cell(c).and_then(|x| x.formula.as_ref()) {
                Some(f) => Dynamic::from(format!("={}", f.text)),
                None => cell_value(&sh.value(c)),
            })
        })
        .register_fn("setFormula", |r: &mut Range, f: Dynamic| r.fill_with(&f))
        .register_fn("getText", |r: &mut Range| r.first(crate::display::cell_text))
        .register_fn("getTexts", |r: &mut Range| r.texts())
        .register_fn("getNumberFormat", |r: &mut Range| r.first(|wb, sh, c| wb.styles.get(sh.style_id(c)).num_fmt.as_str().to_string()))
        .register_fn("getNumberFormats", |r: &mut Range| r.number_formats())
        .register_fn("setNumberFormat", |r: &mut Range, code: &str| r.exec("home.numberFormat", json!({"code": code})).map(drop))
        .register_fn("clear", |r: &mut Range| r.clear("All"))
        .register_fn("clear", |r: &mut Range, what: &str| r.clear(what))
        .register_fn("select", |r: &mut Range| -> Res<()> {
            let sheet = r.sheet.clone();
            r.h.exec("sheet.activate", json!({"sheet": sheet}))?;
            r.h.exec("selection.set", json!({"range": r.r.a1()})).map(drop)
        })
        .register_fn("merge", |r: &mut Range| r.exec("home.mergeCells", json!({})).map(drop))
        .register_fn("merge", |r: &mut Range, across: bool| r.exec(if across { "home.mergeAcross" } else { "home.mergeCells" }, json!({})).map(drop))
        .register_fn("unmerge", |r: &mut Range| r.exec("home.unmergeCells", json!({})).map(drop))
        .register_fn("getFormat", |r: &mut Range| RangeFormat(r.clone()))
        .register_fn("getOffsetRange", |r: &mut Range, dr: Dynamic, dc: Dynamic| {
            r.offset(int(&dr, "The row offset")?, int(&dc, "The column offset")?)
        })
        .register_fn("getResizedRange", |r: &mut Range, dr: Dynamic, dc: Dynamic| {
            r.resized(int(&dr, "The row change")?, int(&dc, "The column change")?)
        })
        .register_fn("getCell", |r: &mut Range, row: Dynamic, col: Dynamic| r.cell(int(&row, "The row")?, int(&col, "The column")?))
        .register_fn("getRow", |r: &mut Range, i: Dynamic| r.row(int(&i, "The row")?))
        .register_fn("getColumn", |r: &mut Range, i: Dynamic| r.column(int(&i, "The column")?))
        .register_fn("getEntireRow", |r: &mut Range| r.with(RangeRef::rows(r.r.start.row, r.r.end.row)))
        .register_fn("getEntireColumn", |r: &mut Range| r.with(RangeRef::cols(r.r.start.col, r.r.end.col)))
        .register_fn("getLastCell", |r: &mut Range| r.with(RangeRef::cell(r.r.end)))
        .register_fn("getLastRow", |r: &mut Range| r.with(RangeRef::new(CellRef::new(r.r.end.row, r.r.start.col), r.r.end)))
        .register_fn("getLastColumn", |r: &mut Range| r.with(RangeRef::new(CellRef::new(r.r.start.row, r.r.end.col), r.r.end)))
        .register_fn("getSurroundingRegion", |r: &mut Range| -> Res<Range> {
            let si = r.sheet_index()?;
            let region = r.h.read(|d| Ok(d.wb.sheet(si).map(|sh| crate::selection::current_region(sh, r.r.start)).unwrap_or(r.r)))?;
            Ok(r.with(region))
        })
        .register_fn("getIntersection", |r: &mut Range, o: Range| -> Dynamic {
            match (o.sheet.eq_ignore_ascii_case(&r.sheet), r.r.intersection(&o.r)) {
                (true, Some(x)) => Dynamic::from(r.with(x)),
                _ => Dynamic::UNIT,
            }
        })
        .register_fn("getBoundingRect", |r: &mut Range, o: Range| -> Res<Range> {
            if !o.sheet.eq_ignore_ascii_case(&r.sheet) {
                return Err(fail("getBoundingRect needs two ranges on the same worksheet."));
            }
            Ok(r.with(r.r.union(&o.r)))
        });
}

fn register_format(e: &mut Engine) {
    e.register_fn("getFont", |f: &mut RangeFormat| RangeFont(f.0.clone()))
        .register_fn("getFill", |f: &mut RangeFormat| RangeFill(f.0.clone()))
        .register_fn("getHorizontalAlignment", |f: &mut RangeFormat| f.0.style().map(|s| h_align_name(s.align.h).to_string()))
        .register_fn("setHorizontalAlignment", |f: &mut RangeFormat, a: &str| -> Res<()> {
            let h = match a.to_ascii_lowercase().as_str() {
                "general" => "General",
                "left" => "Left",
                "center" => "Center",
                "right" => "Right",
                "fill" => "Fill",
                "justify" => "Justify",
                "centeracrossselection" => "CenterAcross",
                "distributed" => "Distributed",
                _ => return Err(fail(format!("\"{a}\" is not a horizontal alignment (Left, Center, Right, General…)."))),
            };
            f.0.exec("home.formatCells", json!({"style": {"align": {"h": h}}})).map(drop)
        })
        .register_fn("getVerticalAlignment", |f: &mut RangeFormat| f.0.style().map(|s| v_align_name(s.align.v).to_string()))
        .register_fn("setVerticalAlignment", |f: &mut RangeFormat, a: &str| -> Res<()> {
            let v = match a.to_ascii_lowercase().as_str() {
                "top" => "Top",
                "center" => "Center",
                "bottom" => "Bottom",
                "justify" => "Justify",
                "distributed" => "Distributed",
                _ => return Err(fail(format!("\"{a}\" is not a vertical alignment (Top, Center, Bottom…)."))),
            };
            f.0.exec("home.formatCells", json!({"style": {"align": {"v": v}}})).map(drop)
        })
        .register_fn("getWrapText", |f: &mut RangeFormat| f.0.style().map(|s| s.align.wrap))
        .register_fn("setWrapText", |f: &mut RangeFormat, on: bool| f.0.exec("home.wrapText", json!({"on": on})).map(drop))
        .register_fn("getColumnWidth", |f: &mut RangeFormat| -> Res<f64> {
            let si = f.0.sheet_index()?;
            f.0.h.read(|d| Ok(d.wb.sheet(si).map(|sh| f64::from(sh.col_width(f.0.r.start.col))).unwrap_or(0.0)))
        })
        .register_fn("setColumnWidth", |f: &mut RangeFormat, w: Dynamic| -> Res<()> {
            let w = num(&w, "The column width")?;
            f.0.exec("home.columnWidth", json!({"cols": f.0.cols_spec(), "width": w})).map(drop)
        })
        .register_fn("getRowHeight", |f: &mut RangeFormat| -> Res<f64> {
            let si = f.0.sheet_index()?;
            f.0.h.read(|d| Ok(d.wb.sheet(si).map(|sh| f64::from(sh.row_height(f.0.r.start.row))).unwrap_or(0.0)))
        })
        .register_fn("setRowHeight", |f: &mut RangeFormat, h: Dynamic| -> Res<()> {
            let h = num(&h, "The row height")?;
            f.0.exec("home.rowHeight", json!({"rows": f.0.rows_spec(), "height": h})).map(drop)
        })
        .register_fn("autofitColumns", |f: &mut RangeFormat| f.0.exec("home.autofitColumnWidth", json!({"cols": f.0.cols_spec()})).map(drop))
        .register_fn("autofitRows", |f: &mut RangeFormat| f.0.exec("home.autofitRowHeight", json!({"rows": f.0.rows_spec()})).map(drop));
    e.register_fn("getBold", |f: &mut RangeFont| f.0.style().map(|s| s.font.bold))
        .register_fn("setBold", |f: &mut RangeFont, on: bool| f.0.exec("home.bold", json!({"on": on})).map(drop))
        .register_fn("getItalic", |f: &mut RangeFont| f.0.style().map(|s| s.font.italic))
        .register_fn("setItalic", |f: &mut RangeFont, on: bool| f.0.exec("home.italic", json!({"on": on})).map(drop))
        .register_fn("getStrikethrough", |f: &mut RangeFont| f.0.style().map(|s| s.font.strike))
        .register_fn("setStrikethrough", |f: &mut RangeFont, on: bool| f.0.exec("home.strikethrough", json!({"on": on})).map(drop))
        .register_fn("getUnderline", |f: &mut RangeFont| f.0.style().map(|s| underline_name(s.font.underline).to_string()))
        .register_fn("setUnderline", |f: &mut RangeFont, u: &str| -> Res<()> {
            let style = match u.to_ascii_lowercase().as_str() {
                "none" => return f.0.exec("home.underline", json!({"on": false})).map(drop),
                "single" => "single",
                "double" => "double",
                "singleaccountant" => "singleAccounting",
                "doubleaccountant" => "doubleAccounting",
                _ => return Err(fail(format!("\"{u}\" is not an underline style (None, Single, Double…)."))),
            };
            f.0.exec("home.underline", json!({"on": true, "style": style})).map(drop)
        })
        .register_fn("getColor", |f: &mut RangeFont| -> Res<String> {
            let si = f.0.sheet_index()?;
            f.0.h.read(|d| {
                let sh = d.wb.sheet(si).ok_or_else(|| fail(no_sheet(&f.0.sheet)))?;
                Ok(d.wb.styles.get(sh.style_id(f.0.r.start)).font.color.hex(&d.wb.theme).unwrap_or_else(|| "#000000".into()))
            })
        })
        .register_fn("setColor", |f: &mut RangeFont, c: &str| f.0.exec("home.fontColor", json!({"color": c})).map(drop))
        .register_fn("getName", |f: &mut RangeFont| f.0.style().map(|s| s.font.name.clone()))
        .register_fn("setName", |f: &mut RangeFont, n: &str| f.0.exec("home.fontName", json!({"name": n})).map(drop))
        .register_fn("getSize", |f: &mut RangeFont| f.0.style().map(|s| number(f64::from(s.font.size))))
        .register_fn("setSize", |f: &mut RangeFont, size: Dynamic| -> Res<()> {
            let size = num(&size, "The font size")?;
            f.0.exec("home.fontSize", json!({"size": size})).map(drop)
        });
    e.register_fn("getColor", |f: &mut RangeFill| -> Res<String> {
        let si = f.0.sheet_index()?;
        f.0.h.read(|d| {
            let sh = d.wb.sheet(si).ok_or_else(|| fail(no_sheet(&f.0.sheet)))?;
            let st = d.wb.styles.get(sh.style_id(f.0.r.start));
            Ok(if st.fill.pattern == gridcraft_model::PatternType::None { String::new() } else { st.fill.fg.hex(&d.wb.theme).unwrap_or_default() })
        })
    })
    .register_fn("setColor", |f: &mut RangeFill, c: &str| f.0.exec("home.fillColor", json!({"color": c})).map(drop))
    .register_fn("clear", |f: &mut RangeFill| f.0.exec("home.fillColor", json!({"color": "none"})).map(drop));
}
