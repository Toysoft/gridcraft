//! How cells look as text: number formats applied, for the grid, clipboard, CSV and agents.
//!
//! The plain functions give the canonical (en-US, English) text agents, the clipboard and files
//! get. The `_in` variants draw for people in a [`DisplayLocale`]: the regional number format the
//! interface uses and the formula language's words for booleans, errors and formulas.

use std::collections::HashMap;
use std::sync::Mutex;

use gridcraft_core::{CellRef, NumberLocale, RangeRef, Value};
use gridcraft_formula::FormulaLocale;
use gridcraft_model::{Sheet, Workbook};
use gridcraft_numfmt::{Formatted, NumberFormat, format_value, format_value_in};

/// How values are shown to people: the regional number format (separators, dates) and the
/// formula language (booleans, error values, function names). The default is canonical: what
/// files, commands and agents use.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct DisplayLocale {
    pub number: NumberLocale,
    pub formula: FormulaLocale,
}

impl DisplayLocale {
    pub const CANONICAL: DisplayLocale = DisplayLocale { number: NumberLocale::EN_US, formula: FormulaLocale::En };

    pub fn is_canonical(&self) -> bool {
        *self == DisplayLocale::CANONICAL
    }

    /// A boolean or error value in the formula language's words (`WAHR`, `#NV`); `None` for other
    /// values.
    pub fn word(&self, v: &Value) -> Option<&'static str> {
        match v {
            Value::Bool(b) => Some(self.formula.boolean_name(*b)),
            Value::Error(e) => Some(self.formula.error_name(*e)),
            Value::Array(a) => a.data.first().and_then(|first| self.word(first)),
            _ => None,
        }
    }
}

/// Parsed number formats are cached by code.
fn parsed(code: &str) -> NumberFormat {
    static CACHE: std::sync::LazyLock<Mutex<HashMap<String, NumberFormat>>> = std::sync::LazyLock::new(|| Mutex::new(HashMap::new()));
    let lock = || CACHE.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    if let Some(f) = lock().get(code) {
        return f.clone();
    }
    // Parse outside the lock, so a slow code never blocks the other threads' lookups.
    let f = NumberFormat::parse(code);
    let mut m = lock();
    if m.len() > 4096 {
        m.clear();
    }
    m.insert(code.to_string(), f.clone());
    f
}

pub fn number_format(code: &str) -> NumberFormat {
    parsed(code)
}

/// Formats a value with a format code.
pub fn format(v: &Value, code: &str, wb: &Workbook) -> Formatted {
    format_value(v, &parsed(code), wb.date_system)
}

/// Formats a value with a format code as people see it in `loc`.
pub fn format_in(v: &Value, code: &str, wb: &Workbook, loc: &DisplayLocale) -> Formatted {
    if let Some(word) = loc.word(v) {
        return Formatted { text: word.to_string(), ..Formatted::default() };
    }
    format_value_in(v, &parsed(code), wb.date_system, &loc.number)
}

/// A formula as people see it in `loc` (`=SUMME(A1;2,5)` in German). Workbook and table names
/// that look like local function names keep their spelling (see [`gridcraft_formula::print_input`]).
pub fn formula_text_in(text: &str, wb: &Workbook, sheet: usize, loc: &DisplayLocale) -> String {
    let Some(body) = text.strip_prefix('=').filter(|_| loc.formula != FormulaLocale::En) else { return text.to_string() };
    gridcraft_formula::parse(body)
        .and_then(|expr| gridcraft_formula::print_input(&expr, loc.formula, |name| wb.name(name, sheet).is_some() || wb.table(name).is_some()))
        .map_or_else(|_| text.to_string(), |body| format!("={body}"))
}

/// What the cell editor and the formula bar start from. Same as `Cell::input_text`, except that a
/// constant number in a date or time format is shown as a date (as the grid shows it), not as its
/// serial. When the formatted text would not read back as the same date (for example
/// `dddd, mmmm d`), an ISO `yyyy-mm-dd` (with `hh:mm:ss` for a time of day; just `hh:mm:ss`
/// for a time alone) is used instead, and if even that would not read back, the serial.
pub fn edit_text(wb: &Workbook, sheet: &Sheet, c: CellRef) -> String {
    let raw = sheet.input_text(c);
    let Some(cell) = sheet.cell(c) else { return raw };
    let Value::Number(n) = &cell.value else { return raw };
    let code = wb.styles.get(cell.style).num_fmt.as_str();
    if cell.formula.is_some() || !parsed(code).is_date() {
        return raw;
    }
    let reads_back = |s: &str| matches!(gridcraft_core::parse::parse_input(s, wb.date_system).value, Value::Number(m) if (m - n).abs() < 1e-9);
    let shown = format(&cell.value, code, wb).text;
    if reads_back(&shown) {
        return shown;
    }
    let Some(dt) = gridcraft_core::date::datetime_from_serial(wb.date_system, *n) else { return raw };
    let time = format!("{:02}:{:02}:{:02}", dt.hour, dt.minute, dt.second);
    let date = format!("{:04}-{:02}-{:02}", dt.year, dt.month, dt.day);
    let iso = if *n < 1.0 {
        time
    } else if n.fract() == 0.0 {
        date
    } else {
        format!("{date} {time}")
    };
    if reads_back(&iso) { iso } else { raw }
}

/// What the cell editor starts from in `loc`: [`edit_text`] with the regional format's numbers
/// and dates (`1234,5`, `13.10.2026`, `13.10.2026 14:30:00`) and the formula language's
/// booleans and errors. Formulas are the caller's (they need the editor's name lookups). The text
/// reads back as the same value through [`gridcraft_core::parse::parse_input_in`].
pub fn edit_text_in(wb: &Workbook, sheet: &Sheet, c: CellRef, loc: &DisplayLocale) -> String {
    if loc.is_canonical() {
        return edit_text(wb, sheet, c);
    }
    let Some(cell) = sheet.cell(c).filter(|cell| cell.formula.is_none()) else { return edit_text(wb, sheet, c) };
    if let Some(word) = loc.word(&cell.value) {
        return word.to_string();
    }
    // Only the words differ when numbers are canonical (Spanish, Portuguese, Russian for now).
    let Value::Number(n) = &cell.value else { return edit_text(wb, sheet, c) };
    if loc.number.is_canonical() {
        return edit_text(wb, sheet, c);
    }
    let n = *n;
    let raw = || {
        let text = Value::Number(n).display();
        if loc.number.decimal == '.' { text } else { text.replace('.', loc.number.decimal.encode_utf8(&mut [0; 4])) }
    };
    let code = wb.styles.get(cell.style).num_fmt.as_str();
    if !parsed(code).is_date() {
        return raw();
    }
    let reads_back =
        |s: &str| matches!(gridcraft_core::parse::parse_input_in(s, wb.date_system, &loc.number).value, Value::Number(m) if (m - n).abs() < 1e-9);
    let shown = format_in(&cell.value, code, wb, loc).text;
    if reads_back(&shown) {
        return shown;
    }
    // Excel's editor shows a date as the short date, with the time of day when there is one.
    let short = if n < 1.0 {
        format_in(&cell.value, "hh:mm:ss", wb, loc).text
    } else if n.fract() == 0.0 {
        format_in(&cell.value, "m/d/yyyy", wb, loc).text
    } else {
        format!("{} {}", format_in(&cell.value, "m/d/yyyy", wb, loc).text, format_in(&cell.value, "hh:mm:ss", wb, loc).text)
    };
    if reads_back(&short) { short } else { raw() }
}

/// Whether `c` has a formula that must not be shown: its cell is formatted Hidden on a
/// protected sheet. A spilled cell follows its anchor.
pub fn formula_hidden(wb: &Workbook, sheet: &Sheet, c: CellRef) -> bool {
    if !sheet.is_protected() {
        return false;
    }
    let at = match sheet.cell(c) {
        Some(x) if x.formula.is_some() => c,
        _ => match sheet.spill_ranges.iter().find(|(_, r)| r.contains(c)) {
            Some((anchor, _)) => *anchor,
            None => return false,
        },
    };
    wb.styles.get(sheet.style_id(at)).protection.hidden
}

/// The text a cell shows (full precision General; the grid narrows General to fit the column).
pub fn cell_text(wb: &Workbook, sheet: &Sheet, c: CellRef) -> String {
    if sheet.show_formulas
        && let Some(f) = sheet.cell(c).and_then(|x| x.formula.as_ref())
        && !formula_hidden(wb, sheet, c)
    {
        return format!("={}", f.text);
    }
    value_text(wb, sheet, c)
}

/// [`cell_text`] as people see it in `loc`: formulas (Show Formulas) in the formula language,
/// values in the regional format.
pub fn cell_text_in(wb: &Workbook, si: usize, c: CellRef, loc: &DisplayLocale) -> String {
    let Some(sheet) = wb.sheet(si) else { return String::new() };
    if sheet.show_formulas
        && let Some(f) = sheet.cell(c).and_then(|x| x.formula.as_ref())
        && !formula_hidden(wb, sheet, c)
    {
        return formula_text_in(&format!("={}", f.text), wb, si, loc);
    }
    value_text_in(wb, sheet, c, loc)
}

/// A cell's value with its number format applied, independent of column width: no `#####`
/// and no General narrowing (Save As CSV/Text writes this). Formulas give their result.
pub fn value_text(wb: &Workbook, sheet: &Sheet, c: CellRef) -> String {
    value_text_in(wb, sheet, c, &DisplayLocale::CANONICAL)
}

/// [`value_text`] as people see it in `loc`.
pub fn value_text_in(wb: &Workbook, sheet: &Sheet, c: CellRef, loc: &DisplayLocale) -> String {
    let style = wb.styles.get(sheet.style_id(c));
    let f = format_in(&sheet.value(c), style.num_fmt.as_str(), wb, loc);
    match f.fill {
        Some((_, pos)) => {
            let mut t = f.text;
            if pos <= t.len() && t.is_char_boundary(pos) {
                t.insert(pos, ' ');
            }
            t
        }
        None => f.text,
    }
}

/// Tab-separated text of a range, as the system clipboard gets it.
pub fn range_text(wb: &Workbook, sheet: usize, r: RangeRef) -> String {
    let Some(sh) = wb.sheet(sheet) else { return String::new() };
    let mut out = String::new();
    let r = clipboard_range(r);
    for row in r.start.row..=r.end.row {
        if sh.is_row_hidden(row) {
            continue;
        }
        let mut first = true;
        for col in r.start.col..=r.end.col {
            if !first {
                out.push('\t');
            }
            first = false;
            let t = cell_text(wb, sh, CellRef::new(row, col));
            if t.contains(['\t', '\n', '"']) {
                out.push('"');
                out.push_str(&t.replace('"', "\"\""));
                out.push('"');
            } else {
                out.push_str(&t);
            }
        }
        out.push('\n');
    }
    out
}

/// The existing plain-text clipboard limit, shared with its HTML representation.
pub(crate) fn clipboard_range(r: RangeRef) -> RangeRef {
    if r.count() > 2_000_000 { RangeRef::new(r.start, r.start.offset_clamped(1999, 999)) } else { r }
}

/// Status-bar statistics for a selection: average, count (non-empty), numerical count, min,
/// max, sum.
#[derive(Clone, Debug, Default, PartialEq, serde::Serialize)]
pub struct Stats {
    pub count: u64,
    pub numeric: u64,
    pub sum: f64,
    pub min: f64,
    pub max: f64,
    pub average: Option<f64>,
}

pub fn stats(sheet: &Sheet, ranges: &[RangeRef]) -> Stats {
    let mut s = Stats { min: f64::INFINITY, max: f64::NEG_INFINITY, ..Default::default() };
    let mut seen = std::collections::HashSet::new();
    for r in ranges {
        let r = match sheet.used_range().and_then(|u| r.intersection(&u)) {
            Some(x) => x,
            None => continue,
        };
        let mut visit = |c: CellRef, v: &Value| {
            if !seen.insert(c) || sheet.is_row_hidden(c.row) {
                return;
            }
            if v.is_empty() {
                return;
            }
            s.count += 1;
            if let Value::Number(n) = v {
                s.numeric += 1;
                s.sum += n;
                s.min = s.min.min(*n);
                s.max = s.max.max(*n);
            }
        };
        for (c, cell) in sheet.cells.iter_range(r) {
            visit(c, &cell.value);
        }
        for (c, v) in sheet.spill.range(r.start..=r.end) {
            if r.contains(*c) {
                visit(*c, v);
            }
        }
    }
    if s.numeric > 0 {
        s.average = Some(s.sum / s.numeric as f64);
    } else {
        s.min = 0.0;
        s.max = 0.0;
    }
    s
}
