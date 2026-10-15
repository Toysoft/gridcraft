//! PivotTables: reading the source list, grouping and aggregating, laying out the report and
//! writing it onto the sheet as static, styled cells.
//!
//! Behaviour follows Excel's documented PivotTable model (rows/columns/values/filters areas,
//! Compact/Outline/Tabular layouts, subtotals, grand totals, show-values-as); the code and the
//! look (our own palette) are original.

use std::cmp::Ordering;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use gridcraft_core::date::datetime_from_serial;
use gridcraft_core::{CellError, CellRef, MAX_COLS, MAX_ROWS, RangeRef, Value};
use gridcraft_formula::FormulaLocale;
use gridcraft_model::*;

use crate::pivot_calc::CalcSet;

/// Largest source block read (cells).
pub const MAX_SOURCE_CELLS: u64 = 10_000_000;
/// Largest report written (cells).
pub const MAX_OUTPUT_CELLS: u64 = 2_000_000;
/// Most fields in the Rows plus Columns areas.
pub const MAX_AXIS_FIELDS: usize = 32;
/// Most fields in the Values area.
pub const MAX_VALUE_FIELDS: usize = 256;

const MONTHS: [&str; 12] = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];

// ---------------------------------------------------------------- source

/// The source list: header names, per-column number formats and the records.
#[derive(Clone, Debug)]
pub struct Source {
    pub sheet: usize,
    pub range: RangeRef,
    pub headers: Vec<String>,
    pub formats: Vec<String>,
    /// Every non-blank value in the column is a number.
    pub numeric: Vec<bool>,
    /// Numeric with a date number format.
    pub dates: Vec<bool>,
    pub rows: Vec<Vec<Value>>,
}

impl Source {
    pub fn col(&self, name: &str) -> Option<usize> {
        self.headers.iter().position(|h| h.eq_ignore_ascii_case(name))
    }
}

/// `'My Sheet'!A1` → (`Some("My Sheet")`, `A1`).
fn split_sheet(r: &str) -> (Option<String>, &str) {
    match r.rsplit_once('!') {
        Some((sh, body)) => (Some(sh.trim_matches('\'').replace("''", "'")), body),
        None => (None, r),
    }
}

/// Quotes a sheet name for a reference when needed.
pub fn quote_sheet(name: &str) -> String {
    if !name.is_empty()
        && name.chars().all(|c| c.is_alphanumeric() || c == '_' || c == '.')
        && !name.chars().next().is_some_and(|c| c.is_ascii_digit())
    {
        name.to_string()
    } else {
        format!("'{}'", name.replace('\'', "''"))
    }
}

/// Absolute reference text for a range on a sheet: `Sheet1!$A$1:$E$20`.
pub fn range_text(sheet_name: &str, r: RangeRef) -> String {
    let abs = |c: CellRef| format!("${}${}", gridcraft_core::col_to_letters(c.col), c.row as u64 + 1);
    format!("{}!{}:{}", quote_sheet(sheet_name), abs(r.start), abs(r.end))
}

/// Resolves source text (range, table name or defined name) to (sheet, range including header).
pub fn resolve_source(wb: &Workbook, text: &str, default_sheet: usize) -> Result<(usize, RangeRef), String> {
    resolve_inner(wb, text, default_sheet, 0)
}

fn resolve_inner(wb: &Workbook, text: &str, default_sheet: usize, depth: u32) -> Result<(usize, RangeRef), String> {
    const INVALID: &str = "The reference isn't valid.";
    let t = text.trim().trim_start_matches('=').trim();
    if t.is_empty() || depth > 4 {
        return Err(INVALID.into());
    }
    if let Some((si, ti)) = wb.table(t)
        && let Some(tb) = wb.sheet(si).and_then(|s| s.tables.get(ti))
    {
        if !tb.header_row {
            return Err("The table needs a header row to be used as a PivotTable source.".into());
        }
        let end_row = tb.range.end.row.saturating_sub(tb.totals_row as u32).max(tb.range.start.row);
        return Ok((si, RangeRef::new(tb.range.start, CellRef::new(end_row, tb.range.end.col))));
    }
    let (sheet, body) = split_sheet(t);
    if let Some(r) = RangeRef::parse(body) {
        let si = match sheet {
            Some(n) => wb.sheet_index(&n).ok_or_else(|| INVALID.to_string())?,
            None => default_sheet,
        };
        let sh = wb.sheet(si).ok_or_else(|| INVALID.to_string())?;
        let r = if r.is_full_cols() || r.is_full_rows() {
            let used = sh.used_range().ok_or_else(|| INVALID.to_string())?;
            r.intersection(&used).ok_or_else(|| INVALID.to_string())?
        } else {
            r
        };
        return Ok((si, r));
    }
    if sheet.is_none()
        && let Some(n) = wb.name(t, default_sheet)
    {
        let f = n.formula.clone();
        return resolve_inner(wb, &f, default_sheet, depth + 1);
    }
    Err(INVALID.into())
}

const FIELD_NAME_ERR: &str =
    "The PivotTable field name is not valid. To create a PivotTable, you must use data that is organized as a list with labeled columns.";

/// Reads the source list.
pub fn read_source(wb: &Workbook, text: &str, default_sheet: usize) -> Result<Source, String> {
    let (si, r) = resolve_source(wb, text, default_sheet)?;
    let sh = wb.sheet(si).ok_or("The reference isn't valid.")?;
    if r.count() > MAX_SOURCE_CELLS {
        return Err("The source range is too large for a PivotTable.".into());
    }
    let mut headers: Vec<String> = Vec::new();
    // Names taken so far (lower-case), and the next suffix to try for each repeated header, so
    // that thousands of equal headers don't take cubic time.
    let mut taken: HashSet<String> = HashSet::new();
    let mut next_suffix: HashMap<String, usize> = HashMap::new();
    let mut formats = Vec::new();
    for c in r.start.col..=r.end.col {
        let h = crate::display::cell_text(wb, sh, CellRef::new(r.start.row, c)).trim().to_string();
        if h.is_empty() {
            return Err(FIELD_NAME_ERR.into());
        }
        let key = h.to_ascii_lowercase();
        let mut n = h.clone();
        let mut k = next_suffix.get(&key).copied().unwrap_or(2);
        while taken.contains(&n.to_ascii_lowercase()) {
            n = format!("{h}{k}");
            k += 1;
        }
        next_suffix.insert(key, k);
        taken.insert(n.to_ascii_lowercase());
        headers.push(n);
        let fmt = if r.end.row > r.start.row {
            wb.styles.get(sh.style_id(CellRef::new(r.start.row + 1, c))).num_fmt.as_str().to_string()
        } else {
            "General".into()
        };
        formats.push(fmt);
    }
    let w = headers.len();
    let mut rows = Vec::new();
    let mut numeric = vec![true; w];
    let mut any = vec![false; w];
    if r.end.row > r.start.row {
        for row in r.start.row + 1..=r.end.row {
            let rec: Vec<Value> = (r.start.col..=r.end.col).map(|c| sh.value(CellRef::new(row, c)).scalar()).collect();
            if rec.iter().all(Value::is_empty) {
                continue;
            }
            for (i, v) in rec.iter().enumerate() {
                if !v.is_empty() {
                    if let Some(a) = any.get_mut(i) {
                        *a = true;
                    }
                    if !v.is_number()
                        && let Some(n) = numeric.get_mut(i)
                    {
                        *n = false;
                    }
                }
            }
            rows.push(rec);
        }
    }
    for (n, a) in numeric.iter_mut().zip(any.iter()) {
        *n = *n && *a;
    }
    let dates = numeric.iter().zip(formats.iter()).map(|(n, f)| *n && crate::display::number_format(f).is_date()).collect();
    Ok(Source { sheet: si, range: r, headers, formats, numeric, dates, rows })
}

// ---------------------------------------------------------------- items

#[derive(Clone, Debug)]
struct Item {
    value: Value,
    label: String,
    class: u8,
    num: f64,
    text: String,
    first: usize,
    /// The value is a number shown with the source format.
    formatted: bool,
}

#[derive(Default)]
struct FieldItems {
    items: Vec<Item>,
    index: HashMap<String, u32>,
}

fn cmp_items(a: &Item, b: &Item) -> Ordering {
    a.class.cmp(&b.class).then(a.num.total_cmp(&b.num)).then_with(|| a.text.cmp(&b.text))
}

/// How a field's values become items: as they are, by a date or time part (with the Grouping
/// dialog's bounds and day span), by number range, or into manual groups (Group Selection).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Grouping<'a> {
    pub date: PivotDateGroup,
    pub number: Option<PivotNumberGroup>,
    /// Bounds and day span of a date grouping.
    pub range: Option<PivotDateRange>,
    /// First day of the first item when grouping by several days from automatic bounds (the
    /// first date in the source).
    pub first_day: f64,
    /// Last day the last N-day item reaches (Ending at, or the automatic end after the last date).
    pub last_day: f64,
    /// The Group Selection groups the field's items fall into.
    pub manual: Option<&'a PivotGroupField>,
}

impl<'a> Grouping<'a> {
    /// The date or number grouping of an axis field entry (no manual groups, automatic first day 0).
    pub fn of(f: &PivotField) -> Grouping<'static> {
        Grouping { date: f.date_group, number: f.number_group, range: f.date_range, first_day: 0.0, last_day: f64::MAX, manual: None }
    }

    /// Fills in what depends on the source: the first day of N-day items with automatic bounds.
    fn on(mut self, src: &Source, col: usize) -> Self {
        let Some(range) = self.range.filter(|r| self.date == PivotDateGroup::Days && r.days > 1) else { return self };
        let (lo, hi) = src
            .rows
            .iter()
            .filter_map(|r| match r.get(col) {
                Some(Value::Number(n)) => Some(*n),
                _ => None,
            })
            .fold((f64::INFINITY, f64::NEG_INFINITY), |(lo, hi), n| (lo.min(n), hi.max(n)));
        self.first_day = range.start.unwrap_or(lo).floor();
        // Excel's automatic end is the day after a last whole date (the last span is cut there).
        let auto_end = if hi.fract() == 0.0 { hi + 1.0 } else { hi };
        self.last_day = range.end.unwrap_or(auto_end).floor();
        if !self.first_day.is_finite() || !self.last_day.is_finite() {
            self.first_day = 0.0;
            self.last_day = f64::MAX;
        }
        self
    }
}

/// The source column and grouping of an axis field: a source field, or a Group Selection field
/// (`Region2`) over one.
pub fn resolve_field<'a>(src: &Source, pt: &'a PivotTable, f: &PivotField) -> Option<(usize, Grouping<'a>)> {
    if let Some(c) = src.col(&f.source_col) {
        return Some((c, Grouping::of(f).on(src, c)));
    }
    let gf = pt.group_field(&f.source_col)?;
    let c = src.col(&gf.base)?;
    Some((c, Grouping { manual: Some(gf), ..Grouping::default() }))
}

/// The source column and grouping of a field named `name` wherever it is: its first Rows or
/// Columns entry, a Group Selection field, or a plain source field (report filters).
pub fn resolve_name<'a>(src: &Source, pt: &'a PivotTable, name: &str) -> Option<(usize, Grouping<'a>)> {
    match pt.rows.iter().chain(pt.columns.iter()).find(|f| f.source_col.eq_ignore_ascii_case(name.trim())) {
        Some(f) => resolve_field(src, pt, f),
        None => resolve_field(src, pt, &PivotField { source_col: name.trim().to_string(), ..PivotField::default() }),
    }
}

/// `3/5/2026` for a date serial (the captions of day ranges and of the `<` and `>` items).
fn mdy(wb: &Workbook, serial: f64) -> String {
    match datetime_from_serial(wb.date_system, serial) {
        Some(d) => format!("{}/{}/{}", d.month, d.day, d.year),
        None => gridcraft_core::number_to_text(serial),
    }
}

/// An hour as Excel's Hours grouping shows it: `12 AM`, `1 AM` … `11 PM`.
pub fn hour_caption(h: u32) -> String {
    let h = h % 24;
    let twelve = if h.is_multiple_of(12) { 12 } else { h % 12 };
    format!("{twelve} {}", if h < 12 { "AM" } else { "PM" })
}

/// (identity key, item) for a source value under a field's grouping.
fn make_item(wb: &Workbook, v: &Value, group: Grouping, fmt: &str, first: usize) -> (String, Item) {
    let it = |value: Value, label: String, class: u8, num: f64, formatted: bool| Item {
        text: label.to_lowercase(),
        value,
        label,
        class,
        num,
        first,
        formatted,
    };
    if let Some(gf) = group.manual {
        // Group Selection: base items in a group become that group's item.
        let base = make_item(wb, v, Grouping::default(), fmt, first);
        return match gf.group_of(&base.1.label) {
            Some(g) => (format!("G{}", g.name.to_lowercase()), it(Value::text(g.name.as_str()), g.name.clone(), 1, 0.0, false)),
            None => base,
        };
    }
    match v {
        Value::Empty => ("b".into(), it(Value::text("(blank)"), "(blank)".into(), 4, 0.0, false)),
        Value::Number(n) => {
            if let Some(ng) = &group.number {
                let text = |label: String, num: f64, key: String| (key, it(Value::text(label.as_str()), label, 0, num, false));
                if *n < ng.start {
                    return text(ng.below_caption(), f64::MIN, "lt".into());
                }
                if *n > ng.end {
                    return text(ng.above_caption(), f64::MAX, "gt".into());
                }
                let k = ng.range_index(*n);
                return text(ng.caption(k), ng.range_start(k), format!("r{k}"));
            }
            if group.date != PivotDateGroup::None
                && let Some(dt) = datetime_from_serial(wb.date_system, *n)
            {
                let text = |label: String, num: f64, key: String| (key, it(Value::text(label.as_str()), label, 0, num, false));
                let range = group.range.unwrap_or_default();
                // Before Starting at / after Ending at: one item each.
                if let Some(s) = range.start.filter(|s| n.floor() < s.floor()) {
                    return text(format!("<{}", mdy(wb, s.floor())), f64::MIN, "lt".into());
                }
                if let Some(e) = range.end.filter(|e| n.floor() > e.floor()) {
                    return text(format!(">{}", mdy(wb, e.floor())), f64::MAX, "gt".into());
                }
                let mon = MONTHS.get(dt.month.saturating_sub(1) as usize).copied().unwrap_or("Jan");
                let (label, key) = match group.date {
                    PivotDateGroup::Years => (dt.year.to_string(), dt.year as f64),
                    PivotDateGroup::Quarters => {
                        let q = (dt.month.saturating_sub(1)) / 3 + 1;
                        (format!("Qtr{q}"), q as f64)
                    }
                    PivotDateGroup::Months => (mon.to_string(), dt.month as f64),
                    PivotDateGroup::Days if range.days > 1 => {
                        // Spans of `days` days from Starting at (or the first date).
                        let start = range.start.unwrap_or(group.first_day).floor();
                        let days = f64::from(range.days.min(PivotDateRange::MAX_DAYS));
                        let k = ((n.floor() - start) / days).floor();
                        let lo = start + k * days;
                        let hi = (lo + days - 1.0).min(group.last_day).max(lo);
                        (format!("{} - {}", mdy(wb, lo), mdy(wb, hi)), lo)
                    }
                    PivotDateGroup::Hours => (hour_caption(dt.hour), dt.hour as f64),
                    PivotDateGroup::Minutes => (format!(":{:02}", dt.minute), dt.minute as f64),
                    PivotDateGroup::Seconds => (format!(":{:02}", dt.second), dt.second as f64),
                    _ => (format!("{}-{mon}", dt.day), (dt.month * 100 + dt.day) as f64),
                };
                return text(label, key, format!("g{key}"));
            }
            let label = crate::display::format(v, fmt, wb).text;
            (format!("n{}", n.to_bits()), it(v.clone(), label, 0, *n, true))
        }
        Value::Text(t) => (format!("t{}", t.to_lowercase()), it(v.clone(), t.to_string(), 1, 0.0, false)),
        Value::Bool(b) => {
            let l = if *b { "TRUE" } else { "FALSE" };
            (format!("l{l}"), it(Value::Bool(*b), l.into(), 2, *b as u8 as f64, false))
        }
        Value::Error(e) => (format!("e{}", e.as_str()), it(v.clone(), e.as_str().into(), 3, 0.0, false)),
        Value::Array(_) => make_item(wb, &v.scalar(), group, fmt, first),
    }
}

impl FieldItems {
    fn intern(&mut self, wb: &Workbook, v: &Value, group: Grouping, fmt: &str, first: usize) -> u32 {
        let (key, item) = make_item(wb, v, group, fmt, first);
        if let Some(i) = self.index.get(&key) {
            return *i;
        }
        let i = self.items.len() as u32;
        self.items.push(item);
        self.index.insert(key, i);
        i
    }
    /// The item of calculated item `name` (a text item after the data items in source order).
    fn intern_calc(&mut self, name: &str) -> u32 {
        let key = format!("c{}", name.to_lowercase());
        if let Some(i) = self.index.get(&key) {
            return *i;
        }
        let i = self.items.len() as u32;
        self.items.push(Item {
            value: Value::text(name),
            label: name.to_string(),
            class: 1,
            num: 0.0,
            text: name.to_lowercase(),
            first: usize::MAX,
            formatted: false,
        });
        self.index.insert(key, i);
        i
    }
    /// The item whose label is `label` (case-insensitive).
    fn find(&self, label: &str) -> Option<u32> {
        self.items.iter().position(|it| it.label.eq_ignore_ascii_case(label.trim())).map(|i| i as u32)
    }
    fn get(&self, i: u32) -> Option<&Item> {
        self.items.get(i as usize)
    }
    fn label(&self, i: u32) -> String {
        self.get(i).map(|x| x.label.clone()).unwrap_or_default()
    }
}

/// The label of a source value under a grouping (for filters and field lists).
pub fn item_label(wb: &Workbook, v: &Value, group: Grouping, fmt: &str) -> String {
    make_item(wb, v, group, fmt, 0).1.label
}

/// Distinct item labels of a source column, sorted like the field list shows them.
pub fn distinct_labels(wb: &Workbook, src: &Source, col: usize, group: Grouping, limit: usize) -> Vec<String> {
    let mut fi = FieldItems::default();
    let fmt = src.formats.get(col).map(String::as_str).unwrap_or("General");
    for (i, r) in src.rows.iter().enumerate() {
        if let Some(v) = r.get(col) {
            fi.intern(wb, v, group, fmt, i);
        }
        if fi.items.len() > 100_000 {
            break;
        }
    }
    let mut items = fi.items;
    items.sort_by(cmp_items);
    items.into_iter().take(limit).map(|i| i.label).collect()
}

// ---------------------------------------------------------------- aggregation

#[derive(Clone, Debug, Default)]
struct Acc {
    count: u64,
    nums: u64,
    sum: f64,
    prod: f64,
    min: f64,
    max: f64,
    mean: f64,
    m2: f64,
    err: Option<CellError>,
    /// Calculated item results added here: how many, and their total (a calculated item's result
    /// is already a summary, so under Count it adds its value, not one).
    calc_n: u64,
    calc_count: f64,
}

impl Acc {
    /// Folds a number into the sum, product, minimum and maximum.
    fn fold(&mut self, n: f64) {
        let first = self.nums + self.calc_n == 0;
        self.sum += n;
        if first {
            self.prod = n;
            self.min = n;
            self.max = n;
        } else {
            self.prod *= n;
            self.min = self.min.min(n);
            self.max = self.max.max(n);
        }
    }
    fn add(&mut self, v: &Value) {
        match v {
            Value::Empty => {}
            Value::Number(n) => {
                let n = *n;
                self.fold(n);
                self.count += 1;
                self.nums += 1;
                let d = n - self.mean;
                self.mean += d / self.nums as f64;
                self.m2 += d * (n - self.mean);
            }
            Value::Error(e) => {
                self.count += 1;
                if self.err.is_none() {
                    self.err = Some(*e);
                }
            }
            _ => self.count += 1,
        }
    }
    /// Adds a calculated item's result for one combination of the other fields' items.
    fn add_calc(&mut self, v: &Value) {
        match v {
            Value::Number(n) => {
                self.fold(*n);
                self.calc_n += 1;
                self.calc_count += *n;
            }
            Value::Error(e) if self.err.is_none() => self.err = Some(*e),
            _ => {}
        }
    }
    fn result(&self, f: PivotFunc) -> Value {
        let n = self.nums as f64;
        match f {
            PivotFunc::Count => return Value::number(self.count as f64 + self.calc_count),
            PivotFunc::CountNumbers => return Value::number(n + self.calc_count),
            _ => {}
        }
        if let Some(e) = self.err {
            return Value::Error(e);
        }
        let div0 = Value::Error(CellError::Div0);
        match f {
            PivotFunc::Sum => Value::number(self.sum),
            PivotFunc::Average => {
                if self.nums == 0 {
                    div0
                } else {
                    Value::number(self.sum / n)
                }
            }
            PivotFunc::Max => Value::number(if self.nums + self.calc_n == 0 { 0.0 } else { self.max }),
            PivotFunc::Min => Value::number(if self.nums + self.calc_n == 0 { 0.0 } else { self.min }),
            PivotFunc::Product => Value::number(if self.nums + self.calc_n == 0 { 0.0 } else { self.prod }),
            PivotFunc::StdDev | PivotFunc::Var => {
                if self.nums < 2 {
                    div0
                } else {
                    let var = (self.m2 / (n - 1.0)).max(0.0);
                    Value::number(if f == PivotFunc::Var { var } else { var.sqrt() })
                }
            }
            PivotFunc::StdDevP | PivotFunc::VarP => {
                if self.nums < 1 {
                    div0
                } else {
                    let var = (self.m2 / n).max(0.0);
                    Value::number(if f == PivotFunc::VarP { var } else { var.sqrt() })
                }
            }
            PivotFunc::Count | PivotFunc::CountNumbers => Value::Empty,
        }
    }
}

/// Excel's caption verb for a function ("Sum of Sales").
pub fn func_caption(f: PivotFunc) -> &'static str {
    match f {
        PivotFunc::Sum => "Sum",
        PivotFunc::Count => "Count",
        PivotFunc::Average => "Average",
        PivotFunc::Max => "Max",
        PivotFunc::Min => "Min",
        PivotFunc::Product => "Product",
        PivotFunc::CountNumbers => "Count Numbers",
        PivotFunc::StdDev => "StdDev",
        PivotFunc::StdDevP => "StdDevp",
        PivotFunc::Var => "Var",
        PivotFunc::VarP => "Varp",
    }
}

// ---------------------------------------------------------------- trees

#[derive(Clone, Debug)]
struct Node {
    item: u32,
    children: Vec<Node>,
}

fn build_tree(paths: &[&[u32]], depth: usize, fields: &[&PivotField], items: &[FieldItems]) -> Vec<Node> {
    let (Some(field), Some(fi)) = (fields.get(depth), items.get(depth)) else { return vec![] };
    let mut order: Vec<u32> = Vec::new();
    let mut groups: HashMap<u32, Vec<&[u32]>> = HashMap::new();
    for p in paths {
        if let Some(&it) = p.get(depth) {
            let g = groups.entry(it).or_default();
            if g.is_empty() {
                order.push(it);
            }
            g.push(p);
        }
    }
    let mut nodes: Vec<Node> = order
        .into_iter()
        .map(|it| {
            let ps = groups.remove(&it).unwrap_or_default();
            Node { item: it, children: build_tree(&ps, depth + 1, fields, items) }
        })
        .collect();
    let by = |a: &Node, b: &Node| match (fi.get(a.item), fi.get(b.item)) {
        (Some(x), Some(y)) => match field.sort {
            PivotSort::Asc => cmp_items(x, y),
            PivotSort::Desc => cmp_items(y, x),
            PivotSort::None => x.first.cmp(&y.first),
        },
        _ => Ordering::Equal,
    };
    nodes.sort_by(by);
    nodes
}

fn is_collapsed(field: &PivotField, label: &str) -> bool {
    field.collapsed_items.iter().any(|c| c.eq_ignore_ascii_case(label))
}

fn collect_siblings(nodes: &[Node], prefix: &mut Vec<u32>, out: &mut HashMap<Vec<u32>, Vec<u32>>) {
    out.insert(prefix.clone(), nodes.iter().map(|n| n.item).collect());
    for n in nodes {
        prefix.push(n.item);
        collect_siblings(&n.children, prefix, out);
        prefix.pop();
    }
}

// ---------------------------------------------------------------- output

/// What a report cell is.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize)]
pub enum CellKind {
    Empty,
    Header,
    RowLabel,
    ColLabel,
    Data,
    Subtotal,
    GrandTotal,
    Filter,
    Placeholder,
}

/// What a report row is (for row-wide styling).
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize)]
pub enum RowKind {
    Filter,
    Blank,
    Header,
    /// Item row; `true` for a parent item (bold).
    Item(bool),
    Subtotal,
    Grand,
    Placeholder,
}

#[derive(Clone, Debug, PartialEq)]
pub struct OutCell {
    pub value: Value,
    pub kind: CellKind,
    pub indent: u8,
    pub num_fmt: Option<String>,
}

impl OutCell {
    fn empty() -> OutCell {
        OutCell { value: Value::Empty, kind: CellKind::Empty, indent: 0, num_fmt: None }
    }
}

/// A computed report: a dense grid relative to its top-left, with row kinds.
#[derive(Clone, Debug)]
pub struct Output {
    pub grid: Vec<Vec<OutCell>>,
    pub row_kinds: Vec<RowKind>,
    /// Rows above the body (report filters plus a blank row).
    pub body_offset: u32,
    pub width: u32,
    /// Grid rows of the innermost (or collapsed) items: a PivotChart's categories.
    pub chart_rows: Vec<u32>,
    /// Grid columns of item (not total) values with their series names: a PivotChart's series.
    pub chart_cols: Vec<(u32, String)>,
    /// Grid column holding the innermost row labels (`None` without row fields).
    pub label_col: Option<u32>,
    /// What each line and data column stands for (GETPIVOTDATA reads it).
    pub report: PivotReport,
    /// The item label cells: which field and item each shows.
    pub items: Vec<ItemCell>,
}

/// A report cell that shows an item of a row or column field.
#[derive(Clone, Debug, PartialEq)]
pub struct ItemCell {
    /// Grid row and column (relative to the report's top-left).
    pub row: u32,
    pub col: u32,
    /// The field (as named in Rows or Columns) and the item's caption.
    pub field: String,
    pub label: String,
    pub on_rows: bool,
}

impl Output {
    pub fn height(&self) -> u32 {
        self.grid.len() as u32
    }
    /// Value at (row, col) relative to the top-left.
    pub fn value(&self, r: usize, c: usize) -> Value {
        self.grid.get(r).and_then(|row| row.get(c)).map(|x| x.value.clone()).unwrap_or_default()
    }
}

struct Grid {
    rows: Vec<Vec<OutCell>>,
    kinds: Vec<RowKind>,
    width: usize,
}

impl Grid {
    fn push_row(&mut self, kind: RowKind) -> Result<usize, String> {
        if (self.rows.len() as u64 + 1) * self.width.max(1) as u64 > MAX_OUTPUT_CELLS || self.rows.len() as u64 >= MAX_ROWS as u64 {
            return Err("The PivotTable report is too large.".into());
        }
        self.rows.push(vec![OutCell::empty(); self.width]);
        self.kinds.push(kind);
        Ok(self.rows.len() - 1)
    }
    fn set(&mut self, r: usize, c: usize, value: Value, kind: CellKind, indent: u8, fmt: Option<String>) {
        if let Some(cell) = self.rows.get_mut(r).and_then(|row| row.get_mut(c)) {
            *cell = OutCell { value, kind, indent, num_fmt: fmt };
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum LineKind {
    Item(bool),
    Subtotal,
    Grand,
}

struct Line {
    kind: LineKind,
    /// A category of a PivotChart: an innermost (or collapsed) item.
    leaf: bool,
    path: Vec<u32>,
    values: bool,
    /// (column, value, indent, number format)
    labels: Vec<(usize, Value, u8, Option<String>)>,
    /// The value field of the line when Σ Values sits in Rows (`None`: one per data column).
    vi: Option<usize>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum ColKind {
    Leaf,
    Subtotal,
    Grand,
}

struct ColSpec {
    kind: ColKind,
    path: Vec<u32>,
}

struct Ctx<'a> {
    pt: &'a PivotTable,
    rows: Vec<&'a PivotField>,
    row_items: Vec<FieldItems>,
    row_fmts: Vec<String>,
    /// Depth of Σ Values among the row fields when it sits in Rows (two or more value fields).
    vpos: Option<usize>,
    /// Value field captions.
    val_names: Vec<String>,
}

impl Ctx<'_> {
    fn label_cell(&self, depth: usize, item: u32) -> (Value, Option<String>, String) {
        let Some(it) = self.row_items.get(depth).and_then(|fi| fi.get(item)) else { return (Value::Empty, None, String::new()) };
        let fmt = if it.formatted { self.row_fmts.get(depth).cloned() } else { None };
        (it.value.clone(), fmt, it.label.clone())
    }

    /// (column, indent) of a row level (Σ Values counts as a level) in Compact/Outline form.
    fn level_cell(&self, level: usize) -> (usize, u8) {
        if self.pt.layout == PivotLayout::Outline { (level, 0) } else { (0, level.min(250) as u8) }
    }

    /// Σ Values comes right here, at row field depth `depth` (`vi`: the value field already chosen above).
    fn values_here(&self, depth: usize, vi: Option<usize>) -> bool {
        vi.is_none() && self.vpos == Some(depth)
    }

    /// Per-value subtotal lines of an item with Σ Values below it ("East Sum of Sales").
    fn value_subtotals(&self, col: usize, indent: u8, label: &str, prefix: &[u32], out: &mut Vec<Line>) {
        for (vi, name) in self.val_names.iter().enumerate() {
            out.push(Line {
                kind: LineKind::Subtotal,
                leaf: false,
                path: prefix.to_vec(),
                values: true,
                labels: vec![(col, Value::text(format!("{label} {name}")), indent, None)],
                vi: Some(vi),
            });
        }
    }

    /// The Σ Values level at row field depth `depth`: one item per value field, each followed by
    /// the row fields below it (`nodes`) or, innermost, holding that value.
    fn flatten_values(&self, nodes: &[Node], depth: usize, prefix: &mut Vec<u32>, out: &mut Vec<Line>) {
        let (col, indent) = self.level_cell(depth);
        for (vi, name) in self.val_names.iter().enumerate() {
            let labels = vec![(col, Value::text(name.as_str()), indent, None)];
            let leaf = nodes.is_empty();
            out.push(Line { kind: LineKind::Item(!leaf), leaf, path: prefix.clone(), values: leaf, labels, vi: Some(vi) });
            if !leaf {
                self.flatten(nodes, depth, Some(vi), prefix, out);
            }
        }
    }

    fn flatten(&self, nodes: &[Node], depth: usize, vi: Option<usize>, prefix: &mut Vec<u32>, out: &mut Vec<Line>) {
        if self.values_here(depth, vi) {
            self.flatten_values(nodes, depth, prefix, out);
            return;
        }
        let n = self.rows.len();
        // Σ Values still to come below this level: totals need one line per value field.
        let values_below = vi.is_none() && self.vpos.is_some();
        let (col, indent) = self.level_cell(depth + usize::from(vi.is_some() && self.vpos.is_some()));
        for node in nodes {
            prefix.push(node.item);
            let (v, fmt, label) = self.label_cell(depth, node.item);
            let collapsed = depth + 1 < n && self.rows.get(depth).is_some_and(|f| is_collapsed(f, &label));
            let leaf = depth + 1 >= n || collapsed || node.children.is_empty();
            if leaf && !values_below {
                out.push(Line {
                    kind: LineKind::Item(depth + 1 < n),
                    leaf: true,
                    path: prefix.clone(),
                    values: true,
                    labels: vec![(col, v, indent, fmt)],
                    vi,
                });
            } else if leaf {
                out.push(Line {
                    kind: LineKind::Item(true),
                    leaf: false,
                    path: prefix.clone(),
                    values: false,
                    labels: vec![(col, v, indent, fmt)],
                    vi,
                });
                // Outline gives each level its own column, so the captions go in the Values
                // column even when the levels between are collapsed away.
                let level = if self.pt.layout == PivotLayout::Outline { self.vpos.unwrap_or(depth + 1) } else { depth + 1 };
                self.flatten_values(&[], level, prefix, out);
            } else if values_below {
                // Subtotals per value field, below the group (one line can't hold them all).
                out.push(Line {
                    kind: LineKind::Item(true),
                    leaf: false,
                    path: prefix.clone(),
                    values: false,
                    labels: vec![(col, v, indent, fmt)],
                    vi,
                });
                self.flatten(&node.children, depth + 1, None, prefix, out);
                if self.pt.subtotals {
                    self.value_subtotals(col, indent, &label, prefix, out);
                }
            } else {
                let top = self.pt.subtotals && self.pt.subtotals_top;
                out.push(Line {
                    kind: LineKind::Item(true),
                    leaf: false,
                    path: prefix.clone(),
                    values: top,
                    labels: vec![(col, v, indent, fmt)],
                    vi,
                });
                self.flatten(&node.children, depth + 1, vi, prefix, out);
                if self.pt.subtotals && !self.pt.subtotals_top {
                    out.push(Line {
                        kind: LineKind::Subtotal,
                        leaf: false,
                        path: prefix.clone(),
                        values: true,
                        labels: vec![(col, Value::text(format!("{label} Total")), indent, None)],
                        vi,
                    });
                }
            }
            prefix.pop();
        }
    }

    /// Tabular form of [`Self::flatten_values`]: the first line also carries the parents' labels.
    fn flatten_tabular_values(
        &self,
        nodes: &[Node],
        depth: usize,
        prefix: &mut Vec<u32>,
        pending: &[(usize, Value, u8, Option<String>)],
        out: &mut Vec<Line>,
    ) {
        for (vi, name) in self.val_names.iter().enumerate() {
            let mut labels: Vec<(usize, Value, u8, Option<String>)> = if vi == 0 { pending.to_vec() } else { vec![] };
            labels.push((depth, Value::text(name.as_str()), 0, None));
            if nodes.is_empty() {
                out.push(Line { kind: LineKind::Item(false), leaf: true, path: prefix.clone(), values: true, labels, vi: Some(vi) });
            } else {
                self.flatten_tabular(nodes, depth, Some(vi), prefix, &labels, out);
            }
        }
    }

    fn flatten_tabular(
        &self,
        nodes: &[Node],
        depth: usize,
        vi: Option<usize>,
        prefix: &mut Vec<u32>,
        pending: &[(usize, Value, u8, Option<String>)],
        out: &mut Vec<Line>,
    ) {
        if self.values_here(depth, vi) {
            self.flatten_tabular_values(nodes, depth, prefix, pending, out);
            return;
        }
        let n = self.rows.len();
        let values_below = vi.is_none() && self.vpos.is_some();
        let col = depth + usize::from(vi.is_some() && self.vpos.is_some());
        for (i, node) in nodes.iter().enumerate() {
            prefix.push(node.item);
            let (v, fmt, label) = self.label_cell(depth, node.item);
            let mut labels: Vec<(usize, Value, u8, Option<String>)> = if i == 0 { pending.to_vec() } else { vec![] };
            labels.push((col, v, 0, fmt));
            let collapsed = depth + 1 < n && self.rows.get(depth).is_some_and(|f| is_collapsed(f, &label));
            if depth + 1 >= n || collapsed || node.children.is_empty() {
                if values_below {
                    self.flatten_tabular_values(&[], self.vpos.unwrap_or(depth + 1), prefix, &labels, out);
                } else {
                    out.push(Line { kind: LineKind::Item(false), leaf: true, path: prefix.clone(), values: true, labels, vi });
                }
            } else {
                self.flatten_tabular(&node.children, depth + 1, vi, prefix, &labels, out);
                if self.pt.subtotals && values_below {
                    self.value_subtotals(col, 0, &label, prefix, out);
                } else if self.pt.subtotals {
                    out.push(Line {
                        kind: LineKind::Subtotal,
                        leaf: false,
                        path: prefix.clone(),
                        values: true,
                        labels: vec![(col, Value::text(format!("{label} Total")), 0, None)],
                        vi,
                    });
                }
            }
            prefix.pop();
        }
    }
}

fn flatten_cols(
    nodes: &[Node],
    depth: usize,
    fields: &[&PivotField],
    items: &[FieldItems],
    subtotals: bool,
    prefix: &mut Vec<u32>,
    out: &mut Vec<ColSpec>,
) {
    let n = fields.len();
    for node in nodes {
        prefix.push(node.item);
        let label = items.get(depth).map(|fi| fi.label(node.item)).unwrap_or_default();
        let collapsed = depth + 1 < n && fields.get(depth).is_some_and(|f| is_collapsed(f, &label));
        if depth + 1 >= n || collapsed || node.children.is_empty() {
            out.push(ColSpec { kind: ColKind::Leaf, path: prefix.clone() });
        } else {
            flatten_cols(&node.children, depth + 1, fields, items, subtotals, prefix, out);
            if subtotals {
                out.push(ColSpec { kind: ColKind::Subtotal, path: prefix.clone() });
            }
        }
        prefix.pop();
    }
}

type AggMap = HashMap<(Vec<u32>, Vec<u32>), Vec<Acc>>;

/// Where a value field's numbers come from.
#[derive(Clone, Copy)]
enum ValSrc {
    /// A source column.
    Col(usize),
    /// A calculated field (index into the PivotTable's calculated fields).
    Calc(usize),
}

// ---------------------------------------------------------------- label, value and Top 10 filters

/// Orders an item caption against a filter value: as numbers when both are numbers, otherwise
/// as text without regard to case.
fn caption_cmp(a: &str, b: &str) -> Ordering {
    match (a.trim().parse::<f64>(), b.trim().parse::<f64>()) {
        (Ok(x), Ok(y)) => x.total_cmp(&y),
        _ => a.to_lowercase().cmp(&b.to_lowercase()),
    }
}

/// Whether an item caption passes a label filter. The text comparisons take `*` and `?`
/// wildcards (and `~` to escape them); none depends on case.
fn label_matches(op: PivotCompare, label: &str, a: &str, b: &str) -> bool {
    let wild = |pat: String| crate::cmd::edit::wildcard(&label.to_lowercase(), &pat.to_lowercase());
    let (lo, hi) = if caption_cmp(a, b) == Ordering::Greater { (b, a) } else { (a, b) };
    let between = || caption_cmp(label, lo) != Ordering::Less && caption_cmp(label, hi) != Ordering::Greater;
    match op {
        PivotCompare::Equals => wild(a.to_string()),
        PivotCompare::NotEquals => !wild(a.to_string()),
        PivotCompare::BeginsWith => wild(format!("{a}*")),
        PivotCompare::NotBeginsWith => !wild(format!("{a}*")),
        PivotCompare::EndsWith => wild(format!("*{a}")),
        PivotCompare::NotEndsWith => !wild(format!("*{a}")),
        PivotCompare::Contains => wild(format!("*{a}*")),
        PivotCompare::NotContains => !wild(format!("*{a}*")),
        PivotCompare::Greater => caption_cmp(label, a) == Ordering::Greater,
        PivotCompare::GreaterOrEqual => caption_cmp(label, a) != Ordering::Less,
        PivotCompare::Less => caption_cmp(label, a) == Ordering::Less,
        PivotCompare::LessOrEqual => caption_cmp(label, a) != Ordering::Greater,
        PivotCompare::Between => between(),
        PivotCompare::NotBetween => !between(),
    }
}

/// Whether an item's total passes a value filter.
fn value_matches(op: PivotCompare, v: f64, a: f64, b: f64) -> bool {
    let (lo, hi) = if a <= b { (a, b) } else { (b, a) };
    match op {
        PivotCompare::Equals => v == a,
        PivotCompare::NotEquals => v != a,
        PivotCompare::Greater => v > a,
        PivotCompare::GreaterOrEqual => v >= a,
        PivotCompare::Less => v < a,
        PivotCompare::LessOrEqual => v <= a,
        PivotCompare::Between => v >= lo && v <= hi,
        PivotCompare::NotBetween => v < lo || v > hi,
        // Text comparisons are refused for value filters; one read from a file keeps every item.
        _ => true,
    }
}

/// The items a value or Top 10 filter drops among siblings, given each item's total (`None` when
/// it is not a number).
fn rule_drops(rule: &PivotRule, scored: &[(u32, Option<f64>)]) -> Vec<u32> {
    match rule {
        PivotRule::Label { .. } => vec![],
        PivotRule::Value { op, value, value2, .. } => {
            scored.iter().filter(|(_, v)| !v.is_some_and(|x| value_matches(*op, x, *value, *value2))).map(|(i, _)| *i).collect()
        }
        PivotRule::Top { top, count, by, .. } => {
            // Items with a number, best first (ties keep their order), then those without.
            let mut order: Vec<(u32, f64)> = scored.iter().filter_map(|(i, v)| v.map(|x| (*i, x))).collect();
            order.sort_by(|a, b| if *top { b.1.total_cmp(&a.1) } else { a.1.total_cmp(&b.1) });
            let keep = match by {
                PivotTopBy::Items => {
                    let n = count.max(0.0).floor();
                    let n = if n >= order.len() as f64 { order.len() } else { n as usize };
                    // As in Excel, items tied with the last one kept are kept too (a Top 15 with
                    // items 14 to 16 all equal shows 16 items).
                    match n.checked_sub(1).and_then(|i| order.get(i)) {
                        Some((_, cut)) => n + order.iter().skip(n).take_while(|(_, v)| v == cut).count(),
                        None => n,
                    }
                }
                PivotTopBy::Percent | PivotTopBy::Sum => {
                    // Items are kept until their running sum reaches the goal.
                    let total: f64 = order.iter().map(|x| x.1.max(0.0)).sum();
                    let goal = if *by == PivotTopBy::Percent { total * count / 100.0 } else { *count };
                    let mut acc = 0.0;
                    let mut n = 0;
                    for (_, v) in &order {
                        if acc >= goal {
                            break;
                        }
                        acc += v.max(0.0);
                        n += 1;
                    }
                    n
                }
            };
            let kept: HashSet<u32> = order.iter().take(keep).map(|x| x.0).collect();
            scored.iter().map(|x| x.0).filter(|i| !kept.contains(i)).collect()
        }
    }
}

/// True when `col_groups` distinct column leaves (plus the grand-total column) times `values` value fields
/// (at least one data column per group) already exceed the sheet width. This is a lower bound on the final column count.
fn data_cols_exceed_sheet(col_groups: usize, grand_total: bool, values: usize) -> bool {
    (col_groups + usize::from(grand_total)).saturating_mul(values.max(1)) > MAX_COLS as usize
}

/// Computes a PivotTable report from the workbook.
pub fn compute(wb: &Workbook, pt: &PivotTable, pivot_sheet: usize) -> Result<Output, String> {
    let src = read_source(wb, &pt.source, pivot_sheet)?;
    let fmt_of = |c: usize| src.formats.get(c).cloned().unwrap_or_else(|| "General".into());
    // Axis fields found in the source (source fields, or Group Selection fields over one), with
    // the grouping that makes their items.
    let row_axis: Vec<(usize, &PivotField, Grouping)> = pt.rows.iter().filter_map(|f| resolve_field(&src, pt, f).map(|(c, g)| (c, f, g))).collect();
    let col_axis: Vec<(usize, &PivotField, Grouping)> =
        pt.columns.iter().filter_map(|f| resolve_field(&src, pt, f).map(|(c, g)| (c, f, g))).collect();
    let rows: Vec<(usize, &PivotField)> = row_axis.iter().map(|(c, f, _)| (*c, *f)).collect();
    let cols: Vec<(usize, &PivotField)> = col_axis.iter().map(|(c, f, _)| (*c, *f)).collect();
    let row_groups: Vec<Grouping> = row_axis.iter().map(|x| x.2).collect();
    let col_groups: Vec<Grouping> = col_axis.iter().map(|x| x.2).collect();
    // Value fields: a source column, or a calculated field evaluated on sums (see `pivot_calc`).
    let calc = CalcSet::new(pt, &src.headers);
    let vals: Vec<(ValSrc, &PivotValue)> = pt
        .values
        .iter()
        .filter_map(|v| match src.col(&v.source_col) {
            Some(c) => Some((ValSrc::Col(c), v)),
            None => calc.index(&v.source_col).map(|i| (ValSrc::Calc(i), v)),
        })
        .collect();
    // Source columns summed for the calculated fields, accumulated after the value fields.
    let mut extra: Vec<usize> = Vec::new();
    for (vs, _) in &vals {
        if let ValSrc::Calc(i) = vs {
            for name in calc.source_refs(*i) {
                if let Some(c) = src.col(&name)
                    && !extra.contains(&c)
                {
                    extra.push(c);
                }
            }
        }
    }
    if rows.len() + cols.len() > MAX_AXIS_FIELDS {
        return Err(format!("A PivotTable can have at most {MAX_AXIS_FIELDS} row and column fields."));
    }
    if vals.len() > MAX_VALUE_FIELDS {
        return Err(format!("A PivotTable can have at most {MAX_VALUE_FIELDS} value fields."));
    }
    // (column, filter, grouping used for labels, shown as a report filter)
    let filters: Vec<(usize, &PivotFilter, Grouping, bool)> = pt
        .filters
        .iter()
        .filter_map(|f| {
            let (c, g) = resolve_name(&src, pt, &f.source_col)?;
            let on_axis = pt.rows.iter().chain(pt.columns.iter()).any(|a| a.source_col.eq_ignore_ascii_case(&f.source_col));
            if !on_axis && f.rule.is_some() {
                return None;
            }
            Some((c, f, g, !on_axis))
        })
        .collect();
    // Timelines connected to this PivotTable: (date column, first day, last day) of each selection.
    let periods: Vec<(usize, f64, f64)> = wb
        .sheets
        .iter()
        .flat_map(|s| s.timelines.iter())
        .filter(|t| t.pivots.contains(&pt.id))
        .filter_map(|t| {
            let (a, b) = t.selection?;
            Some((src.col(&t.field)?, a.floor(), b.floor()))
        })
        .collect();
    let shown_filters: Vec<&(usize, &PivotFilter, Grouping, bool)> = filters.iter().filter(|f| f.3).collect();

    let k = vals.len();
    let empty = rows.is_empty() && cols.is_empty() && vals.is_empty();
    // Σ Values in Rows (it exists with two or more value fields): its depth among the row fields
    // found in the source.
    let vpos: Option<usize> = (pt.values_on_rows && k > 1).then(|| {
        let p = pt.values_position.unwrap_or(pt.rows.len());
        pt.rows.iter().take(p).filter(|f| resolve_field(&src, pt, f).is_some()).count()
    });
    let on_rows = vpos.is_some();
    let row_levels = rows.len() + usize::from(on_rows);

    // ---- aggregate
    let mut row_items: Vec<FieldItems> = rows.iter().map(|_| FieldItems::default()).collect();
    let mut col_items: Vec<FieldItems> = cols.iter().map(|_| FieldItems::default()).collect();
    let mut aggs: AggMap = HashMap::new();
    let mut row_set: HashSet<Vec<u32>> = HashSet::new();
    let mut col_set: HashSet<Vec<u32>> = HashSet::new();
    let mut col_leaves: HashSet<Vec<u32>> = HashSet::new();
    let filter_fmts: Vec<String> = filters.iter().map(|f| fmt_of(f.0)).collect();
    let row_fmts: Vec<String> = rows.iter().map(|(c, _)| fmt_of(*c)).collect();
    let col_fmts: Vec<String> = cols.iter().map(|(c, _)| fmt_of(*c)).collect();
    // Value and Top 10 filters, by axis level; they need every record's paths kept for a second pass.
    let value_rule = |f: &PivotField| {
        pt.filters
            .iter()
            .find(|x| x.source_col.eq_ignore_ascii_case(&f.source_col))
            .and_then(|x| x.rule.as_ref())
            .filter(|r| r.value_field().is_some())
    };
    let row_rules: Vec<Option<&PivotRule>> = rows.iter().map(|(_, f)| value_rule(f)).collect();
    let col_rules: Vec<Option<&PivotRule>> = cols.iter().map(|(_, f)| value_rule(f)).collect();
    let by_value = k > 0 && row_rules.iter().chain(col_rules.iter()).any(Option::is_some);
    let mut recs: Vec<(usize, Vec<u32>, Vec<u32>)> = Vec::new();
    let add = |aggs: &mut AggMap, rec: &[Value], rp: &[u32], cp: &[u32]| -> Result<(), String> {
        for a in 0..=rp.len() {
            for b in 0..=cp.len() {
                let e = aggs.entry((rp[..a].to_vec(), cp[..b].to_vec())).or_insert_with(|| vec![Acc::default(); k + extra.len()]);
                for (acc, (vs, _)) in e.iter_mut().zip(vals.iter()) {
                    if let ValSrc::Col(c) = vs {
                        acc.add(rec.get(*c).unwrap_or(&Value::Empty));
                    }
                }
                for (acc, c) in e.iter_mut().skip(k).zip(extra.iter()) {
                    acc.add(rec.get(*c).unwrap_or(&Value::Empty));
                }
            }
        }
        if aggs.len() > 5_000_000 {
            return Err("The PivotTable report is too large.".into());
        }
        Ok(())
    };
    'rec: for (ri, rec) in src.rows.iter().enumerate() {
        for ((c, f, g, report), fmt) in filters.iter().zip(filter_fmts.iter()) {
            let label_rule = f.rule.as_ref().filter(|r| !*report && r.value_field().is_none());
            if f.selected.is_none() && label_rule.is_none() {
                continue;
            }
            let label = item_label(wb, rec.get(*c).unwrap_or(&Value::Empty), *g, fmt);
            if let Some(sel) = &f.selected
                && !sel.iter().any(|s| s.eq_ignore_ascii_case(&label))
            {
                continue 'rec;
            }
            if let Some(PivotRule::Label { op, value, value2 }) = label_rule
                && !label_matches(*op, &label, value, value2)
            {
                continue 'rec;
            }
        }
        // A timeline keeps the records whose date falls in its selection.
        for (c, a, b) in &periods {
            match rec.get(*c) {
                Some(Value::Number(n)) if n.floor() >= *a && n.floor() <= *b => {}
                _ => continue 'rec,
            }
        }
        if empty {
            continue;
        }
        let mut rp = Vec::with_capacity(rows.len());
        for ((((c, _), fi), fmt), g) in rows.iter().zip(row_items.iter_mut()).zip(row_fmts.iter()).zip(row_groups.iter()) {
            rp.push(fi.intern(wb, rec.get(*c).unwrap_or(&Value::Empty), *g, fmt, ri));
        }
        let mut cp = Vec::with_capacity(cols.len());
        for ((((c, _), fi), fmt), g) in cols.iter().zip(col_items.iter_mut()).zip(col_fmts.iter()).zip(col_groups.iter()) {
            cp.push(fi.intern(wb, rec.get(*c).unwrap_or(&Value::Empty), *g, fmt, ri));
        }
        add(&mut aggs, rec, &rp, &cp)?;
        // The column leaf this record lands in: its path cut after the first collapsed item (as in `flatten_cols`).
        let shown = cols
            .iter()
            .zip(col_items.iter())
            .zip(cp.iter())
            .position(|(((_, f), fi), &it)| !f.collapsed_items.is_empty() && is_collapsed(f, &fi.label(it)))
            .map_or(cp.len(), |d| d + 1);
        if !col_leaves.contains(&cp[..shown]) {
            col_leaves.insert(cp[..shown].to_vec());
        }
        // Refuse a report that cannot fit before the accumulators for more column groups are allocated.
        if !cols.is_empty() && data_cols_exceed_sheet(col_leaves.len(), pt.grand_totals_rows, k) {
            return Err("The PivotTable report will not fit on the sheet.".into());
        }
        if by_value {
            recs.push((ri, rp.clone(), cp.clone()));
        }
        row_set.insert(rp);
        col_set.insert(cp);
    }

    // A value field's result for one report cell: a source column's summary, or a calculated
    // field evaluated on the sums of the fields it names.
    let value_at = |aggs: &AggMap, rp: &[u32], cp: &[u32], vi: usize| -> Option<Value> {
        let (vs, v) = vals.get(vi)?;
        let accs = aggs.get(&(rp.to_vec(), cp.to_vec()))?;
        match vs {
            ValSrc::Col(_) => accs.get(vi).map(|a| a.result(v.func)),
            ValSrc::Calc(i) => {
                let sum = |name: &str| -> Value {
                    src.col(name)
                        .and_then(|c| extra.iter().position(|x| *x == c))
                        .and_then(|j| accs.get(k + j))
                        .map(|a| a.result(PivotFunc::Sum))
                        .unwrap_or(Value::Number(0.0))
                };
                Some(calc.value(wb, *i, &sum))
            }
        }
    };

    // ---- calculated items: for each combination of the other fields' items, a formula runs on
    // the summaries of the items it names; the results then count in subtotals and grand totals
    // like any item's, as in Excel.
    let slot_funcs: Vec<Option<PivotFunc>> = vals
        .iter()
        .map(|(vs, v)| match vs {
            ValSrc::Col(_) => Some(v.func),
            ValSrc::Calc(_) => None,
        })
        .chain(extra.iter().map(|_| Some(PivotFunc::Sum)))
        .collect();
    let calc_items = prepare_calc_items(pt, &rows, &cols, &mut row_items, &mut col_items);
    let levels = (rows.len(), cols.len());
    apply_calc_items(wb, &calc_items, &slot_funcs, &mut aggs, (&mut row_set, &mut col_set), levels, &|_, _| false)?;

    // ---- value and Top 10 filters: each drops items among their siblings (by each item's total),
    // then the report is aggregated again without them, so subtotals and grand totals count only
    // the items shown, as in Excel.
    if by_value {
        let value_index = |name: &str| vals.iter().position(|(_, v)| v.name.eq_ignore_ascii_case(name));
        let mut excl_rows: HashSet<Vec<u32>> = HashSet::new();
        let mut excl_cols: HashSet<Vec<u32>> = HashSet::new();
        for (on_rows, rules, set, excl) in [(true, &row_rules, &row_set, &mut excl_rows), (false, &col_rules, &col_set, &mut excl_cols)] {
            let mut siblings: HashMap<Vec<u32>, Vec<u32>> = HashMap::new();
            for path in set {
                for (d, item) in path.iter().enumerate() {
                    if rules.get(d).is_some_and(Option::is_some) {
                        let e = siblings.entry(path[..d].to_vec()).or_default();
                        if !e.contains(item) {
                            e.push(*item);
                        }
                    }
                }
            }
            for (parent, items) in &siblings {
                let Some(Some(rule)) = rules.get(parent.len()) else { continue };
                let Some(vi) = rule.value_field().and_then(value_index) else { continue };
                let scored: Vec<(u32, Option<f64>)> = items
                    .iter()
                    .map(|it| {
                        let mut path = parent.clone();
                        path.push(*it);
                        let v = if on_rows { value_at(&aggs, &path, &[], vi) } else { value_at(&aggs, &[], &path, vi) };
                        (*it, v.as_ref().and_then(Value::as_f64))
                    })
                    .collect();
                for it in rule_drops(rule, &scored) {
                    let mut path = parent.clone();
                    path.push(it);
                    excl.insert(path);
                }
            }
        }
        if !excl_rows.is_empty() || !excl_cols.is_empty() {
            let hit = |excl: &HashSet<Vec<u32>>, p: &[u32]| (1..=p.len()).any(|n| excl.contains(&p[..n]));
            aggs.clear();
            row_set.clear();
            col_set.clear();
            for (ri, rp, cp) in recs {
                if hit(&excl_rows, &rp) || hit(&excl_cols, &cp) {
                    continue;
                }
                let Some(rec) = src.rows.get(ri) else { continue };
                add(&mut aggs, rec, &rp, &cp)?;
                row_set.insert(rp);
                col_set.insert(cp);
            }
            let dropped = |rp: &[u32], cp: &[u32]| hit(&excl_rows, rp) || hit(&excl_cols, cp);
            apply_calc_items(wb, &calc_items, &slot_funcs, &mut aggs, (&mut row_set, &mut col_set), levels, &dropped)?;
        }
    }

    // ---- axes
    let row_fields: Vec<&PivotField> = rows.iter().map(|(_, f)| *f).collect();
    let col_fields: Vec<&PivotField> = cols.iter().map(|(_, f)| *f).collect();
    let row_paths: Vec<&[u32]> = row_set.iter().map(Vec::as_slice).collect();
    let col_paths: Vec<&[u32]> = col_set.iter().map(Vec::as_slice).collect();
    let row_tree = build_tree(&row_paths, 0, &row_fields, &row_items);
    let col_tree = build_tree(&col_paths, 0, &col_fields, &col_items);
    let mut siblings: HashMap<Vec<u32>, Vec<u32>> = HashMap::new();
    collect_siblings(&row_tree, &mut Vec::new(), &mut siblings);

    let val_names: Vec<String> = vals.iter().map(|(_, v)| v.name.clone()).collect();
    let ctx = Ctx { pt, rows: row_fields.clone(), row_items, row_fmts: row_fmts.clone(), vpos, val_names: val_names.clone() };
    let mut lines: Vec<Line> = Vec::new();
    if row_levels == 0 {
        lines.push(Line { kind: LineKind::Item(false), leaf: true, path: vec![], values: true, labels: vec![], vi: None });
    } else {
        if pt.layout == PivotLayout::Tabular {
            ctx.flatten_tabular(&row_tree, 0, None, &mut Vec::new(), &[], &mut lines);
        } else {
            ctx.flatten(&row_tree, 0, None, &mut Vec::new(), &mut lines);
        }
        // With only Σ Values in Rows its items are the totals already.
        if pt.grand_totals_cols && !rows.is_empty() {
            let grand = |text: String, vi: Option<usize>| Line {
                kind: LineKind::Grand,
                leaf: false,
                path: vec![],
                values: true,
                labels: vec![(0, Value::text(text), 0, None)],
                vi,
            };
            if on_rows {
                lines.extend(val_names.iter().enumerate().map(|(vi, name)| grand(format!("Total {name}"), Some(vi))));
            } else {
                lines.push(grand("Grand Total".into(), None));
            }
        }
    }
    let mut specs: Vec<ColSpec> = Vec::new();
    if cols.is_empty() {
        specs.push(ColSpec { kind: ColKind::Leaf, path: vec![] });
    } else {
        flatten_cols(&col_tree, 0, &col_fields, &col_items, pt.subtotals, &mut Vec::new(), &mut specs);
        if pt.grand_totals_rows {
            specs.push(ColSpec { kind: ColKind::Grand, path: vec![] });
        }
    }
    let mut data_cols: Vec<(usize, Option<usize>)> = Vec::new();
    for (si, _) in specs.iter().enumerate() {
        if on_rows {
            // Each line names its value field; one data column per column item.
            data_cols.push((si, None));
        } else if k == 0 {
            if !cols.is_empty() {
                data_cols.push((si, None));
            }
        } else {
            for vi in 0..k {
                data_cols.push((si, Some(vi)));
            }
        }
        if data_cols.len() > MAX_COLS as usize {
            return Err("The PivotTable report will not fit on the sheet.".into());
        }
    }

    let compact = pt.layout == PivotLayout::Compact;
    let l = if row_levels == 0 && cols.is_empty() {
        0
    } else if compact {
        1
    } else {
        row_levels.max(1)
    };
    let mut width = l + data_cols.len();
    if !shown_filters.is_empty() {
        width = width.max(2);
    }
    if empty {
        width = width.max(1);
    }
    let width = width.max(1);
    let mut g = Grid { rows: vec![], kinds: vec![], width };

    // ---- report filters
    for (c, f, _, _) in &shown_filters {
        let r = g.push_row(RowKind::Filter)?;
        let name = src.headers.get(*c).cloned().unwrap_or_default();
        let shown = match &f.selected {
            None => "(All)".to_string(),
            Some(v) if v.len() == 1 => v.first().cloned().unwrap_or_default(),
            Some(_) => "(Multiple Items)".to_string(),
        };
        g.set(r, 0, Value::text(name), CellKind::Filter, 0, None);
        g.set(r, 1, Value::text(shown), CellKind::Filter, 0, None);
    }
    if !shown_filters.is_empty() {
        g.push_row(RowKind::Blank)?;
    }
    let body_offset = g.rows.len() as u32;

    if empty {
        let r = g.push_row(RowKind::Placeholder)?;
        g.set(r, 0, Value::text(format!("{} — choose fields", pt.name)), CellKind::Placeholder, 0, None);
        return Ok(Output {
            width: g.width as u32,
            grid: g.rows,
            row_kinds: g.kinds,
            body_offset,
            chart_rows: vec![],
            chart_cols: vec![],
            label_col: None,
            items: vec![],
            report: PivotReport::default(),
        });
    }

    let val_name = |vi: usize| vals.get(vi).map(|(_, v)| v.name.clone()).unwrap_or_default();
    let headers_on = pt.show_headers;
    let row_header = |g: &mut Grid, r: usize| {
        if row_levels == 0 || !headers_on {
            return;
        }
        if compact {
            g.set(r, 0, Value::text("Row Labels"), CellKind::Header, 0, None);
        } else {
            // One column per row field, Σ Values ("Values") at its place.
            let mut names: Vec<&str> = row_fields.iter().map(|f| f.source_col.as_str()).collect();
            if let Some(p) = vpos {
                names.insert(p.min(names.len()), "Values");
            }
            for (i, name) in names.into_iter().enumerate() {
                g.set(r, i, Value::text(name), CellKind::Header, 0, None);
            }
        }
    };

    let mut item_cells: Vec<ItemCell> = Vec::new();
    // ---- header rows
    // The report row of each column field's item labels.
    let mut col_label_rows: Vec<u32> = Vec::new();
    if cols.is_empty() {
        let r = g.push_row(RowKind::Header)?;
        row_header(&mut g, r);
        for (j, (_, vi)) in data_cols.iter().enumerate() {
            match vi {
                Some(vi) => g.set(r, l + j, Value::text(val_name(*vi)), CellKind::Header, 0, None),
                // Values on rows, no column field: the one data column is the grand total.
                None if on_rows => g.set(r, l + j, Value::text("Total"), CellKind::Header, 0, None),
                None => {}
            }
        }
    } else {
        let cap = g.push_row(RowKind::Header)?;
        if l > 0 && k == 1 {
            g.set(cap, 0, Value::text(val_name(0)), CellKind::Header, 0, None);
        }
        if headers_on {
            let caption = if compact { "Column Labels".to_string() } else { col_fields.first().map(|f| f.source_col.clone()).unwrap_or_default() };
            g.set(cap, l, Value::text(caption), CellKind::Header, 0, None);
        }
        let levels = cols.len() + usize::from(k > 1 && !on_rows);
        let first_level = g.rows.len();
        col_label_rows = (0..cols.len()).map(|lv| (first_level + lv) as u32).collect();
        for _ in 0..levels {
            g.push_row(RowKind::Header)?;
        }
        let mut prev: Option<&ColSpec> = None;
        for (j, (si, vi)) in data_cols.iter().enumerate() {
            let Some(spec) = specs.get(*si) else { continue };
            let c = l + j;
            match spec.kind {
                ColKind::Leaf => {
                    for (lv, &it) in spec.path.iter().enumerate() {
                        let same = prev.is_some_and(|p| p.kind != ColKind::Grand && p.path.get(..=lv) == spec.path.get(..=lv));
                        if !same {
                            let Some(item) = col_items.get(lv).and_then(|fi| fi.get(it)) else { continue };
                            let fmt = if item.formatted { cols.get(lv).map(|(cc, _)| fmt_of(*cc)) } else { None };
                            g.set(first_level + lv, c, item.value.clone(), CellKind::ColLabel, 0, fmt);
                            if let Some((_, f)) = cols.get(lv) {
                                item_cells.push(ItemCell {
                                    row: (first_level + lv) as u32,
                                    col: c as u32,
                                    field: f.source_col.clone(),
                                    label: item.label.clone(),
                                    on_rows: false,
                                });
                            }
                        }
                    }
                    if k > 1
                        && let Some(vi) = vi
                    {
                        g.set(first_level + cols.len(), c, Value::text(val_name(*vi)), CellKind::ColLabel, 0, None);
                    }
                }
                ColKind::Subtotal => {
                    let lv = spec.path.len().saturating_sub(1);
                    let label = spec.path.last().and_then(|it| col_items.get(lv).map(|fi| fi.label(*it))).unwrap_or_default();
                    let text = match vi {
                        Some(vi) if k > 1 => format!("{label} {}", val_name(*vi)),
                        _ => format!("{label} Total"),
                    };
                    g.set(first_level + lv, c, Value::text(text), CellKind::ColLabel, 0, None);
                }
                ColKind::Grand => {
                    let text = match vi {
                        Some(vi) if k > 1 => format!("Total {}", val_name(*vi)),
                        _ => "Grand Total".to_string(),
                    };
                    g.set(first_level, c, Value::text(text), CellKind::ColLabel, 0, None);
                }
            }
            prev = Some(spec);
        }
        let last = g.rows.len() - 1;
        row_header(&mut g, last);
    }

    // ---- body
    let get = |rp: &[u32], cp: &[u32], vi: usize| value_at(&aggs, rp, cp, vi);
    let num = |v: &Option<Value>| v.as_ref().and_then(Value::as_f64);
    let ratio = |a: Value, b: Option<Value>| -> Value {
        match (a, b) {
            (Value::Error(e), _) => Value::Error(e),
            (_, Some(Value::Error(e))) => Value::Error(e),
            (Value::Number(x), Some(Value::Number(y))) => {
                if y == 0.0 {
                    Value::Error(CellError::Div0)
                } else {
                    Value::number(x / y)
                }
            }
            (v, _) => v,
        }
    };
    let data_fmt: Vec<Option<String>> = vals
        .iter()
        .map(|(vs, v)| match v.show_as {
            sa if sa.is_percent() => Some(v.number_format.clone().unwrap_or_else(|| "0.00%".into())),
            PivotShowAs::Rank | PivotShowAs::RankAscending | PivotShowAs::Index => v.number_format.clone(),
            _ => v.number_format.clone().or_else(|| match v.func {
                PivotFunc::Count | PivotFunc::CountNumbers => None,
                _ => {
                    let f = match vs {
                        ValSrc::Col(c) => fmt_of(*c),
                        ValSrc::Calc(_) => "General".to_string(),
                    };
                    (f != "General").then_some(f)
                }
            }),
        })
        .collect();
    let mut chart_rows: Vec<u32> = Vec::new();
    let chart_cols: Vec<(u32, String)> = data_cols
        .iter()
        .enumerate()
        .filter_map(|(j, (si, vi))| {
            let spec = specs.get(*si)?;
            if spec.kind != ColKind::Leaf {
                return None;
            }
            let mut parts: Vec<String> =
                spec.path.iter().enumerate().map(|(lv, it)| col_items.get(lv).map(|fi| fi.label(*it)).unwrap_or_default()).collect();
            if let Some(vi) = vi
                && (k > 1 || parts.is_empty())
            {
                parts.push(val_name(*vi));
            } else if parts.is_empty() {
                parts.push("Total".into());
            }
            Some(((l + j) as u32, parts.join(" - ")))
        })
        .filter(|_| k > 0)
        .collect();
    let label_col = if row_levels == 0 {
        None
    } else if compact {
        Some(0)
    } else {
        Some(row_levels.saturating_sub(1) as u32)
    };
    let mut col_siblings: HashMap<Vec<u32>, Vec<u32>> = HashMap::new();
    collect_siblings(&col_tree, &mut Vec::new(), &mut col_siblings);
    // Per value field: the base field (axis, depth) and base item.
    let bases: Vec<Option<(bool, usize, BaseItem)>> = vals
        .iter()
        .map(|(_, v)| {
            if !v.show_as.needs_base_field() {
                return None;
            }
            let (is_row, depth) = match v.base_field.as_deref() {
                Some(b) => match row_fields.iter().position(|f| f.source_col.eq_ignore_ascii_case(b)) {
                    Some(d) => (true, d),
                    None => (false, col_fields.iter().position(|f| f.source_col.eq_ignore_ascii_case(b))?),
                },
                None if !row_fields.is_empty() => (true, 0),
                None if !col_fields.is_empty() => (false, 0),
                None => return None,
            };
            let items = if is_row { ctx.row_items.get(depth) } else { col_items.get(depth) };
            let item = match v.base_item.as_deref() {
                Some(BASE_PREVIOUS) => BaseItem::Previous,
                Some(BASE_NEXT) => BaseItem::Next,
                Some(label) => items
                    .and_then(|fi| fi.items.iter().position(|it| it.label.eq_ignore_ascii_case(label)))
                    .map_or(BaseItem::Missing, |i| BaseItem::At(i as u32)),
                None => BaseItem::Missing,
            };
            Some((is_row, depth, item))
        })
        .collect();
    let show = |raw: Value, rp: &[u32], cp: &[u32], vi: usize| -> Value {
        let Some((_, vdef)) = vals.get(vi) else { return raw };
        match vdef.show_as {
            PivotShowAs::Normal => raw,
            PivotShowAs::PercentOfGrandTotal => ratio(raw, get(&[], &[], vi)),
            PivotShowAs::PercentOfColumnTotal => ratio(raw, get(&[], cp, vi)),
            PivotShowAs::PercentOfRowTotal => ratio(raw, get(rp, &[], vi)),
            PivotShowAs::PercentOfParentRowTotal => match rp.split_last() {
                Some((_, parent)) => ratio(raw, get(parent, cp, vi)),
                None => ratio(raw.clone(), Some(raw)),
            },
            PivotShowAs::PercentOfParentColumnTotal => match cp.split_last() {
                Some((_, parent)) => ratio(raw, get(rp, parent, vi)),
                None => ratio(raw.clone(), Some(raw)),
            },
            PivotShowAs::Index => match (raw.as_f64(), num(&get(&[], &[], vi)), num(&get(rp, &[], vi)), num(&get(&[], cp, vi))) {
                (Some(v), Some(g), Some(r), Some(c)) => {
                    if r == 0.0 || c == 0.0 {
                        Value::Error(CellError::Div0)
                    } else {
                        Value::number(v * g / (r * c))
                    }
                }
                _ => raw,
            },
            _ => {
                let Some(Some((is_row, d, item))) = bases.get(vi) else { return raw };
                let (is_row, d) = (*is_row, *d);
                let path = if is_row { rp } else { cp };
                // Rows or columns above the base field's level (subtotals, grand totals).
                let (Some(&mine), Some(parent)) = (path.get(d), path.get(..d)) else {
                    return match vdef.show_as {
                        PivotShowAs::RunningTotal => raw,
                        _ => Value::Empty,
                    };
                };
                let sibs: Vec<u32> = (if is_row { &siblings } else { &col_siblings }).get(parent).cloned().unwrap_or_default();
                let at = |it: u32| -> Option<Value> {
                    let mut p = path.to_vec();
                    if let Some(x) = p.get_mut(d) {
                        *x = it;
                    }
                    if is_row { get(&p, cp, vi) } else { get(rp, &p, vi) }
                };
                let pos = sibs.iter().position(|x| *x == mine);
                let base = match item {
                    BaseItem::At(i) => Some(*i),
                    BaseItem::Previous => pos.and_then(|p| p.checked_sub(1)).and_then(|p| sibs.get(p)).copied(),
                    BaseItem::Next => pos.and_then(|p| sibs.get(p + 1)).copied(),
                    BaseItem::Missing => None,
                };
                let running = || -> f64 {
                    let upto = pos.map_or(0, |p| p + 1);
                    sibs.iter().take(upto).map(|s| num(&at(*s)).unwrap_or(0.0)).sum()
                };
                match vdef.show_as {
                    PivotShowAs::PercentOf => match base {
                        Some(b) if b == mine => ratio(raw.clone(), Some(raw)),
                        Some(b) => match at(b) {
                            Some(bv) => ratio(raw, Some(bv)),
                            None => Value::Error(CellError::NA),
                        },
                        None => Value::Error(CellError::NA),
                    },
                    PivotShowAs::DifferenceFrom | PivotShowAs::PercentDifferenceFrom => match base {
                        Some(b) if b == mine => Value::Empty,
                        Some(b) => match (raw.as_f64(), at(b)) {
                            (Some(v), Some(bv)) => match (bv.as_f64(), vdef.show_as) {
                                (Some(x), PivotShowAs::DifferenceFrom) => Value::number(v - x),
                                (Some(0.0), _) => Value::Error(CellError::Div0),
                                (Some(x), _) => Value::number((v - x) / x),
                                (None, _) => bv,
                            },
                            (_, None) => Value::Error(CellError::NA),
                            (None, _) => raw,
                        },
                        None => Value::Empty,
                    },
                    PivotShowAs::RunningTotal => match raw {
                        Value::Number(_) => Value::number(running()),
                        other => other,
                    },
                    PivotShowAs::PercentRunningTotal => match raw {
                        Value::Number(_) => {
                            let total: f64 = sibs.iter().map(|s| num(&at(*s)).unwrap_or(0.0)).sum();
                            if total == 0.0 { Value::Error(CellError::Div0) } else { Value::number(running() / total) }
                        }
                        other => other,
                    },
                    PivotShowAs::Rank | PivotShowAs::RankAscending => match raw.as_f64() {
                        Some(me) => {
                            let desc = vdef.show_as == PivotShowAs::Rank;
                            let ahead = sibs.iter().filter(|s| num(&at(**s)).is_some_and(|x| if desc { x > me } else { x < me })).count();
                            Value::number((ahead + 1) as f64)
                        }
                        None => raw,
                    },
                    PivotShowAs::PercentOfParentTotal => {
                        let top = path.get(..=d).unwrap_or(path);
                        let denom = if is_row { get(top, cp, vi) } else { get(rp, top, vi) };
                        ratio(raw, denom)
                    }
                    _ => raw,
                }
            }
        }
    };
    let mut report_rows: Vec<ReportKey> = Vec::new();
    for line in &lines {
        let rk = match line.kind {
            LineKind::Item(b) => RowKind::Item(b),
            LineKind::Subtotal => RowKind::Subtotal,
            LineKind::Grand => RowKind::Grand,
        };
        let ck = match line.kind {
            LineKind::Item(_) => (CellKind::RowLabel, CellKind::Data),
            LineKind::Subtotal => (CellKind::Subtotal, CellKind::Subtotal),
            LineKind::Grand => (CellKind::GrandTotal, CellKind::GrandTotal),
        };
        let r = g.push_row(rk)?;
        if line.leaf && k > 0 {
            chart_rows.push(r as u32);
        }
        for (c, v, indent, fmt) in &line.labels {
            g.set(r, *c, v.clone(), ck.0, *indent, fmt.clone());
            // The level a label is at: its indent in Compact form, else its column; Σ Values
            // (in Rows) counts as a level of its own.
            let level = if compact { *indent as usize } else { *c };
            let depth = match vpos {
                Some(p) if level == p => continue,
                Some(p) if level > p => level - 1,
                _ => level,
            };
            if let (LineKind::Item(_), Some(&it), Some(f)) = (line.kind, line.path.get(depth), row_fields.get(depth)) {
                let label = ctx.row_items.get(depth).map(|fi| fi.label(it)).unwrap_or_default();
                item_cells.push(ItemCell { row: r as u32, col: *c as u32, field: f.source_col.clone(), label, on_rows: true });
            }
        }
        if !line.values {
            continue;
        }
        report_rows.push(ReportKey { offset: r as u32, data: line.vi.map(|v| v as u32), items: key_items(&line.path, rows.len()) });
        let rp = line.path.as_slice();
        for (j, (si, col_vi)) in data_cols.iter().enumerate() {
            let line_vi = col_vi.or(line.vi);
            let (Some(spec), Some(vi)) = (specs.get(*si), &line_vi) else { continue };
            let cp = spec.path.as_slice();
            let Some(raw) = get(rp, cp, *vi) else { continue };
            let shown = show(raw, rp, cp, *vi);
            g.set(r, l + j, shown, ck.1, 0, data_fmt.get(*vi).cloned().flatten());
        }
    }
    let report_cols: Vec<ReportKey> = data_cols
        .iter()
        .enumerate()
        .filter_map(|(j, (si, vi))| {
            let spec = specs.get(*si)?;
            Some(ReportKey { offset: (l + j) as u32, data: vi.map(|v| v as u32), items: key_items(&spec.path, cols.len()) })
        })
        .collect();
    let field = |(c, f): &(usize, &PivotField), fi: &FieldItems| ReportField {
        // A Group Selection field (`Region2`) goes by its own name, not its base field's.
        name: src.headers.get(*c).filter(|_| src.col(&f.source_col).is_some()).cloned().unwrap_or_else(|| f.source_col.clone()),
        dates: f.date_group == PivotDateGroup::None && src.dates.get(*c).copied().unwrap_or(false),
        items: fi.items.iter().map(|it| ReportItem { value: it.value.clone(), label: it.label.clone() }).collect(),
    };
    let report = PivotReport {
        data_fields: vals.iter().map(|(_, v)| ReportDataField { name: v.name.clone(), source: v.source_col.clone() }).collect(),
        row_fields: rows.iter().zip(ctx.row_items.iter()).map(|(f, fi)| field(f, fi)).collect(),
        col_fields: cols.iter().zip(col_items.iter()).map(|(f, fi)| field(f, fi)).collect(),
        page_fields: shown_filters
            .iter()
            .map(|&&(c, f, _, _)| {
                let one = match &f.selected {
                    Some(v) if v.len() == 1 => v.first().cloned(),
                    _ => None,
                };
                (src.headers.get(c).cloned().unwrap_or_default(), one)
            })
            .collect(),
        rows: report_rows,
        cols: report_cols,
        col_label_rows,
        label_cols: l as u32,
        compact,
    };
    Ok(Output { width: g.width as u32, grid: g.rows, row_kinds: g.kinds, body_offset, chart_rows, chart_cols, label_col, items: item_cells, report })
}

/// A calculated item placed on an axis of the report.
struct CalcAt {
    on_rows: bool,
    /// Depth of its field on that axis.
    depth: usize,
    /// Its item id in the field's items.
    item: u32,
    /// Not hidden by its field's own filter.
    shown: bool,
    compiled: Result<crate::pivot_calc::Compiled, String>,
    /// Item ids of the formula's references, in `compiled.refs` order (`None`: not an item now).
    refs: Vec<Option<u32>>,
}

/// Most calculated item results one report computes (combinations × value fields).
const MAX_CALC_RESULTS: usize = 2_000_000;

/// The calculated items whose field is in Rows or Columns, as items of that field.
fn prepare_calc_items(
    pt: &PivotTable,
    rows: &[(usize, &PivotField)],
    cols: &[(usize, &PivotField)],
    row_items: &mut [FieldItems],
    col_items: &mut [FieldItems],
) -> Vec<CalcAt> {
    let place = |field: &str| {
        let pos = |list: &[(usize, &PivotField)]| list.iter().position(|(_, f)| f.source_col.eq_ignore_ascii_case(field));
        match (pos(rows), pos(cols)) {
            (Some(d), _) => Some((true, d)),
            (None, Some(d)) => Some((false, d)),
            _ => None,
        }
    };
    // Every calculated item is an item before any formula is read, so formulas may name them.
    let mut placed: Vec<(&PivotCalcItem, bool, usize, u32)> = Vec::new();
    for ci in pt.calculated_items.iter().take(crate::pivot_items::MAX_CALC_ITEMS) {
        let Some((on_rows, depth)) = place(&ci.field) else { continue };
        let items = if on_rows { row_items.get_mut(depth) } else { col_items.get_mut(depth) };
        let Some(items) = items else { continue };
        placed.push((ci, on_rows, depth, items.intern_calc(&ci.name)));
    }
    placed
        .into_iter()
        .map(|(ci, on_rows, depth, item)| {
            let items: Option<&FieldItems> = if on_rows { row_items.get(depth) } else { col_items.get(depth) };
            let lookup = |n: &str| items.and_then(|fi| fi.find(n).map(|i| fi.label(i)));
            let compiled = crate::pivot_items::compile(&ci.formula, &ci.field, &lookup);
            let refs = match &compiled {
                Ok(c) => c.refs.iter().map(|r| items.and_then(|fi| fi.find(r))).collect(),
                Err(_) => vec![],
            };
            let filter = pt.filters.iter().find(|f| f.source_col.eq_ignore_ascii_case(&ci.field));
            let shown = filter.is_none_or(|f| {
                f.selected.as_ref().is_none_or(|sel| sel.iter().any(|x| x.eq_ignore_ascii_case(&ci.name)))
                    && match &f.rule {
                        Some(PivotRule::Label { op, value, value2 }) => label_matches(*op, &ci.name, value, value2),
                        _ => true,
                    }
            });
            CalcAt { on_rows, depth, item, shown, compiled, refs }
        })
        .collect()
}

/// Adds each calculated item's results to the aggregates, in solve order: for every combination
/// of the other fields' items (`levels` = row and column depths of a full path) its formula runs
/// on the summaries of the items it names, and the result counts at every level above, so
/// subtotals and grand totals include it. `skip` leaves out combinations a value filter dropped.
fn apply_calc_items(
    wb: &Workbook,
    calc: &[CalcAt],
    funcs: &[Option<PivotFunc>],
    aggs: &mut AggMap,
    sets: (&mut HashSet<Vec<u32>>, &mut HashSet<Vec<u32>>),
    levels: (usize, usize),
    skip: &dyn Fn(&[u32], &[u32]) -> bool,
) -> Result<(), String> {
    let (row_set, col_set) = sets;
    for ci in calc.iter().filter(|c| c.shown) {
        let place = |rp: &[u32], cp: &[u32], item: u32| -> Option<(Vec<u32>, Vec<u32>)> {
            let (mut r, mut c) = (rp.to_vec(), cp.to_vec());
            *(if ci.on_rows { r.get_mut(ci.depth) } else { c.get_mut(ci.depth) })? = item;
            Some((r, c))
        };
        let contexts: HashSet<(Vec<u32>, Vec<u32>)> = aggs
            .keys()
            .filter(|(rp, cp)| rp.len() == levels.0 && cp.len() == levels.1)
            .filter_map(|(rp, cp)| place(rp, cp, ci.item))
            .filter(|(rp, cp)| !skip(rp, cp))
            .collect();
        if contexts.len().saturating_mul(funcs.len().max(1)) > MAX_CALC_RESULTS {
            return Err("The PivotTable has too many calculated item values.".into());
        }
        let mut results: Vec<(Vec<u32>, Vec<u32>, Vec<Option<Value>>)> = Vec::with_capacity(contexts.len());
        for (rp, cp) in contexts {
            let leaf = |item: Option<u32>| item.and_then(|it| place(&rp, &cp, it)).and_then(|key| aggs.get(&key));
            let vals: Vec<Option<Value>> = funcs
                .iter()
                .enumerate()
                .map(|(j, f)| {
                    let f = (*f)?;
                    Some(match &ci.compiled {
                        Ok(c) => {
                            let args: Vec<Value> =
                                ci.refs.iter().map(|r| leaf(*r).and_then(|a| a.get(j)).map_or(Value::Number(0.0), |a| a.result(f))).collect();
                            crate::pivot_calc::eval(wb, c, &args)
                        }
                        Err(_) => Value::Error(CellError::Name),
                    })
                })
                .collect();
            results.push((rp, cp, vals));
        }
        for (rp, cp, vals) in results {
            for a in 0..=rp.len() {
                for b in 0..=cp.len() {
                    let e = aggs.entry((rp[..a].to_vec(), cp[..b].to_vec())).or_insert_with(|| vec![Acc::default(); funcs.len()]);
                    for (acc, v) in e.iter_mut().zip(vals.iter()) {
                        if let Some(v) = v {
                            acc.add_calc(v);
                        }
                    }
                }
            }
            row_set.insert(rp);
            col_set.insert(cp);
        }
        if aggs.len() > 5_000_000 {
            return Err("The PivotTable report is too large.".into());
        }
    }
    Ok(())
}

/// A Show Values As base item, resolved against the base field's items.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum BaseItem {
    At(u32),
    Previous,
    Next,
    /// None given, or not among the field's items.
    Missing,
}

/// A line's or column's item path as one entry per field of its axis (`None`: a total).
fn key_items(path: &[u32], fields: usize) -> Vec<Option<u32>> {
    (0..fields).map(|i| path.get(i).copied()).collect()
}

// ---------------------------------------------------------------- styling and writing

/// Colours for a PivotTable style name (our own palette, keyed by the familiar
/// `PivotStyleLight1…28 / Medium1…28 / Dark1…28` names). The PivotTable Styles gallery draws
/// its swatches from it.
pub struct Look {
    pub header_fill: Color,
    pub header_font: Color,
    pub line: Color,
    pub filter_fill: Color,
}

pub fn look(style: &str) -> Look {
    let rest = style.strip_prefix("PivotStyle").unwrap_or(style);
    let (fam, n) = ["Light", "Medium", "Dark"]
        .iter()
        .find_map(|f| rest.strip_prefix(f).map(|num| (*f, num.parse::<u32>().unwrap_or(16))))
        .unwrap_or(("Light", 16));
    let k = n.saturating_sub(1) % 7;
    let acc = if k == 0 { None } else { Some(3 + k as u8) };
    let tone = |tint: i16| match acc {
        Some(a) => Color::Theme(a, tint),
        None => Color::Theme(1, tint.clamp(-1000, 1000)),
    };
    match fam {
        "Dark" => Look { header_fill: Color::Theme(1, 0), header_font: Color::Theme(0, 0), line: tone(0), filter_fill: tone(600) },
        "Medium" => Look { header_fill: tone(0), header_font: Color::Theme(0, 0), line: tone(0), filter_fill: tone(800) },
        _ => Look { header_fill: tone(800), header_font: Color::Auto, line: tone(400), filter_fill: tone(800) },
    }
}

fn cell_style(lk: &Look, rk: RowKind, cell: &OutCell, last_header: bool) -> Style {
    let mut s = Style::default();
    match rk {
        RowKind::Header => {
            s.font.bold = true;
            s.fill = Fill::solid(lk.header_fill);
            s.font.color = lk.header_font;
            if last_header {
                s.borders.bottom = BorderLine { style: BorderStyle::Thin, color: lk.line };
            }
        }
        RowKind::Filter => {
            s.fill = Fill::solid(lk.filter_fill);
            s.font.bold = last_header;
        }
        RowKind::Item(parent) => s.font.bold = parent,
        RowKind::Subtotal => s.font.bold = true,
        RowKind::Grand => {
            s.font.bold = true;
            s.borders.top = BorderLine { style: BorderStyle::Thin, color: lk.line };
        }
        RowKind::Placeholder => {
            s.font.italic = true;
            s.font.color = Color::Theme(1, 500);
        }
        RowKind::Blank => {}
    }
    if matches!(cell.kind, CellKind::RowLabel | CellKind::ColLabel | CellKind::Subtotal | CellKind::GrandTotal) && !cell.value.is_number() {
        s.align.h = HAlign::Left;
    }
    if matches!(cell.kind, CellKind::RowLabel | CellKind::ColLabel) && cell.value.is_number() {
        s.align.h = HAlign::Left;
    }
    s.align.indent = cell.indent.min(15);
    if let Some(f) = &cell.num_fmt {
        s.num_fmt = NumFmt::new(f);
    }
    s
}

/// Recomputes a pivot and writes its report. With `replace` false, refuses when the target area
/// holds other data. Returns the written area.
pub fn refresh(wb: &mut Workbook, sheet: usize, idx: usize, replace: bool) -> Result<RangeRef, String> {
    let pt = wb.sheet(sheet).and_then(|s| s.pivots.get(idx)).cloned().ok_or("There's no such PivotTable.")?;
    let out = compute(wb, &pt, sheet)?;
    let h = out.height().max(1);
    let w = out.width.max(1);
    let k = out.body_offset;
    let origin_row = if pt.anchor.row >= k { pt.anchor.row - k } else { pt.anchor.row };
    let origin = CellRef::new(origin_row, pt.anchor.col);
    let end_row = origin.row as u64 + h as u64 - 1;
    let end_col = origin.col as u64 + w as u64 - 1;
    if end_row >= MAX_ROWS as u64 || end_col >= MAX_COLS as u64 {
        return Err("The PivotTable report will not fit on the sheet. Choose a different location.".into());
    }
    let area = RangeRef::new(origin, CellRef::new(end_row as u32, end_col as u32));
    let sh = wb.sheet(sheet).ok_or("There's no such sheet.")?;
    for (j, other) in sh.pivots.iter().enumerate() {
        if j != idx && other.last_range.is_some_and(|r| r.intersects(&area)) {
            return Err("A PivotTable report cannot overlap another PivotTable report.".into());
        }
    }
    if sh.is_protected() {
        return Err("The cell or chart you're trying to change is on a protected sheet.".into());
    }
    if let Ok((ssi, sr)) = resolve_source(wb, &pt.source, sheet)
        && ssi == sheet
        && sr.intersects(&area)
    {
        return Err("The PivotTable report can't overlap its source data. Choose a different location.".into());
    }
    if sh.tables.iter().any(|t| t.range.intersects(&area)) {
        return Err("A PivotTable report cannot overlap a table.".into());
    }
    if !replace {
        for (c, cell) in sh.cells.iter_range(area) {
            if (!cell.value.is_empty() || cell.formula.is_some()) && !pt.last_range.is_some_and(|r| r.contains(c)) {
                return Err(format!(
                    "There's already data in {}!{}. Do you want to replace it? (Run again with replace: true to overwrite.)",
                    quote_sheet(&sh.name),
                    c.a1()
                ));
            }
        }
        if let Some(c) = sh.spill.keys().find(|c| area.contains(**c) && !pt.last_range.is_some_and(|r| r.contains(**c))) {
            return Err(format!(
                "There's already data in {}!{}. Do you want to replace it? (Run again with replace: true to overwrite.)",
                quote_sheet(&sh.name),
                c.a1()
            ));
        }
    }
    // Styles first (the style table is workbook-wide).
    let lk = look(&pt.style);
    let header_last = out.row_kinds.iter().rposition(|k| *k == RowKind::Header);
    let mut cache: HashMap<Style, StyleId> = HashMap::new();
    let mut cells: Vec<(CellRef, Cell)> = Vec::new();
    for (ri, row) in out.grid.iter().enumerate() {
        let rk = out.row_kinds.get(ri).copied().unwrap_or(RowKind::Blank);
        if rk == RowKind::Blank {
            continue;
        }
        for (ci, oc) in row.iter().enumerate() {
            if rk == RowKind::Filter && oc.value.is_empty() {
                continue;
            }
            if rk == RowKind::Placeholder && oc.value.is_empty() {
                continue;
            }
            let flag = match rk {
                RowKind::Header => Some(ri) == header_last,
                RowKind::Filter => ci == 0,
                _ => false,
            };
            let st = cell_style(&lk, rk, oc, flag);
            let id = match cache.get(&st) {
                Some(id) => *id,
                None => {
                    let id = wb.styles.intern(st.clone());
                    cache.insert(st, id);
                    id
                }
            };
            if oc.value.is_empty() && id == StyleId::default() {
                continue;
            }
            cells.push((CellRef::new(origin.row + ri as u32, origin.col + ci as u32), Cell { value: oc.value.clone(), formula: None, style: id }));
        }
    }
    // Column widths that fit the labels.
    let mut widths: HashMap<u32, f32> = HashMap::new();
    for (c, cell) in &cells {
        if cell.value.is_empty() {
            continue;
        }
        let st = wb.styles.get(cell.style);
        let text = crate::display::format(&cell.value, st.num_fmt.as_str(), wb).text;
        let wpt = crate::cmd::format::approx_text_width(&text, st.font.size, st.font.bold) + st.align.indent as f32 * 9.0 + 14.0;
        let e = widths.entry(c.col).or_insert(0.0);
        *e = e.max(wpt.min(320.0));
    }
    let shm = wb.sheet_mut(sheet).ok_or("There's no such sheet.")?;
    if let Some(old) = pt.last_range {
        shm.take_cells(old);
    }
    shm.take_cells(area);
    for (c, cell) in cells {
        shm.set_cell(c, cell);
    }
    for (col, wpt) in widths {
        if wpt > shm.col_width(col) && !shm.is_col_hidden(col) {
            shm.cols.entry(col).or_default().size = Some(wpt);
        }
    }
    if let Some(p) = shm.pivots.get_mut(idx) {
        p.last_range = Some(area);
        p.report = PivotReportIndex(Some(Arc::new(out.report.clone())));
    }
    let sheet_name = shm.name.clone();
    rebind_charts(wb, &sheet_name, &pt.name, origin, &out);
    Ok(area)
}

// ---------------------------------------------------------------- PivotCharts

/// The link a PivotChart stores to its PivotTable: `Sheet2!PivotTable1`.
pub fn chart_link(sheet: &str, pivot: &str) -> String {
    format!("{}!{pivot}", quote_sheet(sheet))
}

/// (sheet, PivotTable name) of a PivotChart link; a leading `[Book.xlsx]` is ignored.
pub fn parse_chart_link(link: &str) -> Option<(String, String)> {
    let t = link.trim();
    let t = match t.strip_prefix('[') {
        Some(rest) => rest.split_once(']').map(|(_, r)| r).unwrap_or(rest),
        None => t,
    };
    let (sheet, name) = t.rsplit_once('!')?;
    let sheet = sheet.trim();
    let sheet = sheet.strip_prefix('\'').and_then(|x| x.strip_suffix('\'')).map(|x| x.replace("''", "'")).unwrap_or_else(|| sheet.to_string());
    let name = name.trim();
    if sheet.is_empty() || name.is_empty() {
        return None;
    }
    Some((sheet, name.to_string()))
}

/// Whether a PivotChart link points at `pivot` on `sheet`.
pub fn links_to(link: &str, sheet: &str, pivot: &str) -> bool {
    parse_chart_link(link).is_some_and(|(s, n)| s.eq_ignore_ascii_case(sheet) && n.eq_ignore_ascii_case(pivot))
}

/// An absolute reference to `col` over grid `rows` (contiguous runs joined into a union).
fn rows_ref(sheet: &str, origin: CellRef, col: u32, rows: &[u32]) -> Option<String> {
    let q = quote_sheet(sheet);
    let c = origin.col.checked_add(col)?;
    let letters = gridcraft_core::col_to_letters(c);
    let mut parts: Vec<String> = Vec::new();
    let mut i = 0;
    while let Some(&start) = rows.get(i) {
        let mut end = start;
        while rows.get(i + 1).is_some_and(|n| Some(*n) == end.checked_add(1)) {
            end += 1;
            i += 1;
        }
        i += 1;
        let (a, b) = (origin.row.checked_add(start)? + 1, origin.row.checked_add(end)? + 1);
        parts.push(if a == b { format!("{q}!${letters}${a}") } else { format!("{q}!${letters}${a}:${letters}${b}") });
    }
    match parts.len() {
        0 => None,
        1 => parts.pop(),
        _ => Some(format!("({})", parts.join(","))),
    }
}

/// Most category rows a PivotChart plots.
const MAX_CHART_ROWS: usize = 10_000;

/// A PivotChart's series for a report written at `origin`: categories are the innermost row
/// items, one series per item column (column items × value fields); totals are left out.
pub fn chart_series(sheet: &str, origin: CellRef, out: &Output) -> Vec<Series> {
    let rows: Vec<u32> = out.chart_rows.iter().copied().take(MAX_CHART_ROWS).collect();
    let categories = out.label_col.and_then(|c| rows_ref(sheet, origin, c, &rows));
    out.chart_cols
        .iter()
        .filter_map(|(c, name)| {
            Some(Series {
                name: Some(format!("\"{}\"", name.replace('"', "\"\""))),
                categories: categories.clone(),
                values: rows_ref(sheet, origin, *c, &rows)?,
                bubble_sizes: None,
                color: None,
                secondary: false,
                kind: None,
                smooth: false,
                trendlines: Vec::new(),
            })
        })
        .collect()
}

/// Rebuilds the series of every PivotChart of a PivotTable after its report was written.
fn rebind_charts(wb: &mut Workbook, sheet: &str, pivot: &str, origin: CellRef, out: &Output) {
    let linked = |ch: &Chart| ch.pivot.as_deref().is_some_and(|l| links_to(l, sheet, pivot));
    let hits: Vec<usize> = (0..wb.sheets.len()).filter(|i| wb.sheet(*i).is_some_and(|sh| sh.charts.iter().any(linked))).collect();
    if hits.is_empty() {
        return;
    }
    let series = chart_series(sheet, origin, out);
    for i in hits {
        let Some(sh) = wb.sheet_mut(i) else { continue };
        for ch in sh.charts.iter_mut().filter(|c| linked(c)) {
            ch.series = series.clone();
        }
    }
}

/// The item label cells of a PivotTable's report, at their sheet cells (an empty list when the
/// report can't be computed).
pub fn report_items(wb: &Workbook, sheet: usize, idx: usize) -> Vec<(CellRef, ItemCell)> {
    let Some(pt) = wb.sheet(sheet).and_then(|s| s.pivots.get(idx)) else { return vec![] };
    let Ok(out) = compute(wb, pt, sheet) else { return vec![] };
    let k = out.body_offset;
    let origin_row = if pt.anchor.row >= k { pt.anchor.row - k } else { pt.anchor.row };
    out.items.into_iter().filter_map(|it| Some((CellRef::new(origin_row.checked_add(it.row)?, pt.anchor.col.checked_add(it.col)?), it))).collect()
}

/// Clears a pivot's last output area.
pub fn clear_output(wb: &mut Workbook, sheet: usize, idx: usize) {
    let Some(shm) = wb.sheet_mut(sheet) else { return };
    let Some(r) = shm.pivots.get(idx).and_then(|p| p.last_range) else { return };
    shm.take_cells(r);
    if let Some(p) = shm.pivots.get_mut(idx) {
        p.last_range = None;
        p.report = PivotReportIndex::default();
    }
}

/// Indexes the reports of PivotTables read from a file: their cells are on the sheet, but no
/// report was written in this session. A PivotTable is indexed only when its report, computed
/// again, has its values exactly where the sheet has them; until a refresh, GETPIVOTDATA gives
/// `#REF!` for one whose report doesn't match (a stale or differently laid out file).
pub fn index_reports(wb: &mut Workbook) {
    let mut found: Vec<(usize, usize, PivotReport)> = Vec::new();
    for (si, sh) in wb.sheets.iter().enumerate() {
        for (pi, pt) in sh.pivots.iter().enumerate() {
            let Some(area) = pt.last_range else { continue };
            if pt.report.get().is_some() {
                continue;
            }
            let Ok(out) = compute(wb, pt, si) else { continue };
            if out.height().max(1) != area.height() || out.width.max(1) != area.width() || !report_matches(sh, area.start, &out) {
                continue;
            }
            found.push((si, pi, out.report));
        }
    }
    for (si, pi, report) in found {
        if let Some(p) = wb.sheet_mut(si).and_then(|sh| sh.pivots.get_mut(pi)) {
            p.report = PivotReportIndex(Some(Arc::new(report)));
        }
    }
}

/// Whether every value cell of a computed report holds, on the sheet, the value it computes.
fn report_matches(sh: &Sheet, origin: CellRef, out: &Output) -> bool {
    let same = |a: &Value, b: &Value| match (a, b) {
        (Value::Number(x), Value::Number(y)) => (x - y).abs() <= 1e-9 * x.abs().max(y.abs()).max(1.0),
        _ => a == b,
    };
    out.report.rows.iter().all(|r| {
        out.report.cols.iter().all(|c| {
            let (Some(row), Some(col)) = (origin.row.checked_add(r.offset), origin.col.checked_add(c.offset)) else { return false };
            same(&out.value(r.offset as usize, c.offset as usize), &sh.value(CellRef::new(row, col)))
        })
    })
}

// ---------------------------------------------------------------- GETPIVOTDATA

/// The formula (without `=`) Excel's Generate GetPivotData writes for a value cell of a
/// PivotTable: `GETPIVOTDATA("Sum of Sales",$A$3,"Region","East")`, naming the value field by its
/// caption, the PivotTable by its top-left cell and each row and column item of the cell. Dates
/// are written with DATE. `None` when `cell` on `sheet` isn't a value cell of a report. The
/// formula is for a cell on `from_sheet`, in `locale`'s syntax.
pub fn getpivotdata_formula(wb: &Workbook, from_sheet: usize, sheet: usize, cell: CellRef, locale: FormulaLocale) -> Option<String> {
    let sh = wb.sheet(sheet)?;
    let pt = sh.pivots.iter().find(|p| p.last_range.is_some_and(|r| r.contains(cell)))?;
    let origin = pt.last_range?.start;
    let report = pt.report.get()?;
    let (data, pairs) = report.value_cell(cell.row.checked_sub(origin.row)?, cell.col.checked_sub(origin.col)?)?;
    let sep = locale.list_separator();
    let text = |t: &str| format!("\"{}\"", t.replace('"', "\"\""));
    let number = |n: f64| {
        let t = gridcraft_core::number_to_text(n);
        if locale.decimal_separator() == '.' { t } else { t.replace('.', &locale.decimal_separator().to_string()) }
    };
    let anchor = format!("${}${}", gridcraft_core::col_to_letters(pt.anchor.col), u64::from(pt.anchor.row) + 1);
    let target = if sheet == from_sheet { anchor } else { format!("{}!{anchor}", quote_sheet(&sh.name)) };
    let mut out = format!("{}({}{sep}{target}", locale.function_name("GETPIVOTDATA"), text(&report.data_fields.get(data as usize)?.name));
    for (field, item) in pairs {
        let literal = match &item.value {
            Value::Number(n) if field.dates && n.fract() == 0.0 => match datetime_from_serial(wb.date_system, *n) {
                Some(d) => format!("{}({}{sep}{}{sep}{})", locale.function_name("DATE"), d.year, d.month, d.day),
                None => number(*n),
            },
            Value::Number(n) => number(*n),
            Value::Bool(b) => locale.function_name(if *b { "TRUE" } else { "FALSE" }).to_string(),
            _ => text(&item.label),
        };
        out.push_str(&format!("{sep}{}{sep}{literal}", text(&field.name)));
    }
    out.push(')');
    Some(out)
}

#[cfg(test)]
mod fit_tests {
    use super::data_cols_exceed_sheet;

    #[test]
    fn column_budget_counts_value_fields_and_grand_total() {
        assert!(!data_cols_exceed_sheet(64, true, 128));
        assert!(data_cols_exceed_sheet(64, true, 256));
        assert!(!data_cols_exceed_sheet(64, false, 256));
        assert!(!data_cols_exceed_sheet(16_384, false, 0));
        assert!(data_cols_exceed_sheet(16_385, false, 0));
    }
}
