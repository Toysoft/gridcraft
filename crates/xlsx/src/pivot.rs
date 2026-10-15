//! PivotTables: pivot cache definitions and records (ECMA-376 Part 1 §18.10.1) and PivotTable
//! definitions (§18.10.1.73), written from the model's [`PivotTable`]s and read back into them.
//!
//! Writing: one cache per PivotTable, with a records snapshot of the source list (omitted, with
//! `saveData="0"`, for very large sources) and `refreshOnLoad="1"` so a reader recomputes the
//! report from the source on open. Fields used on an axis get shared items (or, for date and
//! number range grouping, `fieldGroup` group items) so the table's `<items>` can refer to them. The report
//! cells the engine wrote stay on the sheet as plain cells.
//!
//! Reading: the cache's source and field names plus the table's areas, functions, show-values-as,
//! layout flags, grand totals and style become a model [`PivotTable`]. Anything we don't model
//! (OLAP sources, manual groups, time grouping…) becomes a warning.

use std::collections::HashMap;
use std::fmt::Write as _;

use gridcraft_core::date::datetime_from_serial;
use gridcraft_core::{CellRef, DateSystem, RangeRef, Value, number_to_text};
use gridcraft_formula::quote_sheet;
use gridcraft_model::{
    BASE_NEXT, BASE_PREVIOUS, PivotCalcField, PivotCalcItem, PivotCompare, PivotDateGroup, PivotDateRange, PivotField, PivotFilter, PivotFunc,
    PivotGroupField, PivotItemGroup, PivotLayout, PivotNumberGroup, PivotRule, PivotShowAs, PivotSort, PivotTable, PivotTopBy, PivotValue, Workbook,
};

use crate::IoError;
use crate::read::Ctx;
use crate::write::{NS_MAIN, NS_REL, Out, Rels, XML_DECL, abs_range};
use crate::xml::{El, esc_attr};

pub const CT_PIVOT_TABLE: &str = "application/vnd.openxmlformats-officedocument.spreadsheetml.pivotTable+xml";
pub const CT_PIVOT_CACHE_DEF: &str = "application/vnd.openxmlformats-officedocument.spreadsheetml.pivotCacheDefinition+xml";
pub const CT_PIVOT_CACHE_REC: &str = "application/vnd.openxmlformats-officedocument.spreadsheetml.pivotCacheRecords+xml";

/// Largest source block scanned for a cache (cells); bigger sources are not written.
const MAX_SOURCE_CELLS: u64 = 10_000_000;
/// Largest records snapshot written (cells); bigger caches are written without records.
const MAX_RECORD_CELLS: u64 = 2_000_000;
/// Most shared items kept per field (Excel's limit is 1,048,576 unique items).
const MAX_ITEMS: usize = 1_048_576;
/// Most day-span group items written (a span of a few days over centuries is capped).
const MAX_SPANS: usize = 100_000;
/// Most cache fields read.
const MAX_FIELDS: usize = 16_384;
/// The `x14:dataField` extension (Excel 2010 show-values-as additions such as rank).
const EXT_X14_DATA_FIELD: &str = "{E15A36E0-9728-4e99-A89B-3F7291B0FE68}";
const NS_X14: &str = "http://schemas.microsoft.com/office/spreadsheetml/2009/9/main";
/// The `x14:pivotCacheDefinition` extension (the cache id slicers and timelines refer to).
const EXT_X14_CACHE: &str = "{725AE2AE-9491-48be-B2B4-4EB974FC3084}";

const MONTHS: [&str; 12] = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];

// ================================================================ shared helpers

/// The pivot item label of a value (blank shows as "(blank)").
fn label(v: &Value) -> String {
    match v {
        Value::Empty => "(blank)".into(),
        Value::Number(n) => number_to_text(*n),
        Value::Text(t) => t.to_string(),
        Value::Bool(b) => if *b { "TRUE" } else { "FALSE" }.into(),
        Value::Error(e) => e.as_str().into(),
        Value::Array(_) => label(&v.scalar()),
    }
}

fn same(a: &str, b: &str) -> bool {
    a.to_lowercase() == b.to_lowercase()
}

fn func_name(f: PivotFunc) -> &'static str {
    match f {
        PivotFunc::Sum => "sum",
        PivotFunc::Count => "count",
        PivotFunc::Average => "average",
        PivotFunc::Max => "max",
        PivotFunc::Min => "min",
        PivotFunc::Product => "product",
        PivotFunc::CountNumbers => "countNums",
        PivotFunc::StdDev => "stdDev",
        PivotFunc::StdDevP => "stdDevp",
        PivotFunc::Var => "var",
        PivotFunc::VarP => "varp",
    }
}

fn func_from(s: &str) -> Option<PivotFunc> {
    Some(match s {
        "sum" => PivotFunc::Sum,
        "count" => PivotFunc::Count,
        "average" => PivotFunc::Average,
        "max" => PivotFunc::Max,
        "min" => PivotFunc::Min,
        "product" => PivotFunc::Product,
        "countNums" => PivotFunc::CountNumbers,
        "stdDev" => PivotFunc::StdDev,
        "stdDevp" => PivotFunc::StdDevP,
        "var" => PivotFunc::Var,
        "varp" => PivotFunc::VarP,
        _ => return None,
    })
}

fn func_caption(f: PivotFunc) -> &'static str {
    match f {
        PivotFunc::Sum => "Sum",
        PivotFunc::Count => "Count",
        PivotFunc::Average => "Average",
        PivotFunc::Max => "Max",
        PivotFunc::Min => "Min",
        PivotFunc::Product => "Product",
        PivotFunc::CountNumbers => "Count",
        PivotFunc::StdDev => "StdDev",
        PivotFunc::StdDevP => "StdDevp",
        PivotFunc::Var => "Var",
        PivotFunc::VarP => "Varp",
    }
}

fn group_by_name(g: PivotDateGroup) -> &'static str {
    match g {
        PivotDateGroup::Years => "years",
        PivotDateGroup::Quarters => "quarters",
        PivotDateGroup::Months => "months",
        PivotDateGroup::Days => "days",
        PivotDateGroup::Hours => "hours",
        PivotDateGroup::Minutes => "minutes",
        PivotDateGroup::Seconds => "seconds",
        PivotDateGroup::None => "range",
    }
}

fn group_caption(g: PivotDateGroup) -> &'static str {
    match g {
        PivotDateGroup::Years => "Years",
        PivotDateGroup::Quarters => "Quarters",
        PivotDateGroup::Months => "Months",
        PivotDateGroup::Days => "Days",
        PivotDateGroup::Hours => "Hours",
        PivotDateGroup::Minutes => "Minutes",
        PivotDateGroup::Seconds => "Seconds",
        PivotDateGroup::None => "Group",
    }
}

/// An hour as Excel's Hours grouping shows it: `12 AM`, `1 AM` … `11 PM`.
fn hour_caption(h: u32) -> String {
    let h = h % 24;
    let twelve = if h.is_multiple_of(12) { 12 } else { h % 12 };
    format!("{twelve} {}", if h < 12 { "AM" } else { "PM" })
}

/// `2024-03-05T00:00:00` for a date serial.
fn iso(sys: DateSystem, serial: f64) -> Option<String> {
    let d = datetime_from_serial(sys, serial)?;
    Some(format!("{:04}-{:02}-{:02}T{:02}:{:02}:{:02}", d.year, d.month, d.day, d.hour, d.minute, d.second))
}

/// `3/5/2024` for a date serial (group boundary captions).
fn mdy(sys: DateSystem, serial: f64) -> String {
    match datetime_from_serial(sys, serial) {
        Some(d) => format!("{}/{}/{}", d.month, d.day, d.year),
        None => number_to_text(serial),
    }
}

// ================================================================ writing

/// Identity of a shared item (text items match case-insensitively, as in Excel).
#[derive(Clone, PartialEq, Eq, Hash)]
enum Key {
    Blank,
    Num(u64),
    Text(String),
    Bool(bool),
    Err(&'static str),
}

fn key(v: &Value) -> Key {
    match v {
        Value::Empty => Key::Blank,
        Value::Number(n) => Key::Num(if *n == 0.0 { 0 } else { n.to_bits() }),
        Value::Text(t) => Key::Text(t.to_lowercase()),
        Value::Bool(b) => Key::Bool(*b),
        Value::Error(e) => Key::Err(e.as_str()),
        Value::Array(_) => key(&v.scalar()),
    }
}

#[derive(Default)]
struct Stats {
    blank: bool,
    text: bool,
    number: bool,
    non_int: bool,
    boolean: bool,
    error: bool,
    long: bool,
    min: f64,
    max: f64,
}

impl Stats {
    fn add(&mut self, v: &Value) {
        match v {
            Value::Empty => self.blank = true,
            Value::Number(n) => {
                if !self.number {
                    self.min = *n;
                    self.max = *n;
                }
                self.number = true;
                self.min = self.min.min(*n);
                self.max = self.max.max(*n);
                if n.fract() != 0.0 {
                    self.non_int = true;
                }
            }
            Value::Text(t) => {
                self.text = true;
                if t.chars().count() > 255 {
                    self.long = true;
                }
            }
            Value::Bool(_) => self.boolean = true,
            Value::Error(_) => self.error = true,
            Value::Array(_) => self.add(&v.scalar()),
        }
    }
    fn kinds(&self) -> usize {
        [self.text, self.number, self.boolean, self.error].iter().filter(|b| **b).count()
    }
}

/// Date, number range or manual (Group Selection) grouping of a cache field.
#[derive(Default)]
struct Group {
    /// The date part; `None` for a number range or manual grouping.
    by: PivotDateGroup,
    /// Number range grouping (`groupBy="range"` with `startNum`, `endNum`, `groupInterval`).
    number: Option<PivotNumberGroup>,
    /// The database field the grouping applies to.
    base: usize,
    /// The next (coarser) grouping field of the same base.
    par: Option<usize>,
    start: f64,
    end: f64,
    /// Starting at / Ending at set by hand (`autoStart="0"`, `autoEnd="0"`).
    fixed: (bool, bool),
    /// Days per item when grouping by days (`groupInterval`).
    days: u32,
    /// Group Selection: the group item of each of the base field's shared items (`discretePr`).
    discrete: Option<Vec<u32>>,
    /// Group item captions, including the `<start` and `>end` items.
    items: Vec<String>,
}

#[derive(Default)]
struct CField {
    name: String,
    /// Column offset in the source (database fields only).
    col: Option<u32>,
    stats: Stats,
    /// Shared items (written when `itemized`).
    items: Vec<Value>,
    index: HashMap<Key, u32>,
    itemized: bool,
    group: Option<Group>,
    /// A calculated field's formula (without `=`); such fields aren't database fields.
    formula: Option<String>,
    /// The Group Selection field made from this one (`<fieldGroup par>` without a grouping).
    par: Option<usize>,
    /// Written as dates although not grouped (the date field of a timeline).
    dates: bool,
}

impl CField {
    /// Grouped by a date part, or a timeline's date field (its values are written as dates).
    fn date_grouped(&self) -> bool {
        self.dates || self.group.as_ref().is_some_and(|g| g.number.is_none() && g.discrete.is_none())
    }
    /// Item captions the PivotTable's `<item x>` indices refer to.
    fn item_labels(&self) -> Vec<String> {
        match &self.group {
            Some(g) => g.items.clone(),
            None => self.items.iter().map(label).collect(),
        }
    }
}

/// A cache built from a PivotTable's source.
struct Cache {
    fields: Vec<CField>,
    /// Pivot field → cache field, by the pivot field's position in rows, then columns.
    axis_map: Vec<Option<usize>>,
    source_xml: String,
    records: Option<(u64, String)>,
    /// Calculated items: (cache field, its shared item, formula without `=`).
    calc_items: Vec<(usize, u32, String)>,
    /// The cache's `x14:pivotCacheDefinition pivotCacheId` (written when a timeline refers to it).
    ext_id: Option<u32>,
}

/// PivotTables being written; the tables themselves are finished after the styles part (their
/// value number formats need style number-format ids).
#[derive(Default)]
pub struct PivotWriter {
    /// (cache id, workbook-relative part name).
    caches: Vec<(u32, String)>,
    pending: Vec<Pending>,
    tables: u32,
}

struct Pending {
    part: String,
    sheet: usize,
    idx: usize,
    cache_id: u32,
    cache: Cache,
}

impl PivotWriter {
    /// Writes the caches of a sheet's PivotTables and adds the sheet's relationships to its
    /// PivotTable parts. Pivots whose source can't be resolved are not written.
    pub fn add_sheet(&mut self, wb: &Workbook, si: usize, out: &mut Out, rels: &mut Rels) {
        let Some(sheet) = wb.sheet(si) else { return };
        for (idx, pt) in sheet.pivots.iter().enumerate() {
            let Some(mut cache) = build_cache(wb, pt, si) else { continue };
            let n = self.caches.len() as u32 + 1;
            // A timeline cache refers to the pivot cache of the PivotTable it lists by this id.
            if wb.sheets.iter().flat_map(|s| s.timelines.iter()).any(|t| t.pivots.first() == Some(&pt.id)) {
                cache.ext_id = Some(n);
            }
            let cache_part = format!("xl/pivotCache/pivotCacheDefinition{n}.xml");
            let rec_rid = if let Some((count, xml)) = &cache.records {
                let mut r = Rels::default();
                let rid = r.add("pivotCacheRecords", &format!("pivotCacheRecords{n}.xml"));
                out.part(&format!("xl/pivotCache/_rels/pivotCacheDefinition{n}.xml.rels"), None, r.xml().into_bytes());
                let s = format!("{XML_DECL}<pivotCacheRecords xmlns=\"{NS_MAIN}\" xmlns:r=\"{NS_REL}\" count=\"{count}\">{xml}</pivotCacheRecords>");
                out.part(&format!("xl/pivotCache/pivotCacheRecords{n}.xml"), Some(CT_PIVOT_CACHE_REC), s.into_bytes());
                Some(rid)
            } else {
                None
            };
            out.part(&cache_part, Some(CT_PIVOT_CACHE_DEF), cache_xml(&cache, rec_rid.as_deref(), wb.date_system).into_bytes());
            self.caches.push((n, format!("pivotCache/pivotCacheDefinition{n}.xml")));
            self.tables += 1;
            let t = self.tables;
            rels.add("pivotTable", &format!("../pivotTables/pivotTable{t}.xml"));
            let mut tr = Rels::default();
            tr.add("pivotCacheDefinition", &format!("../pivotCache/pivotCacheDefinition{n}.xml"));
            out.part(&format!("xl/pivotTables/_rels/pivotTable{t}.xml.rels"), None, tr.xml().into_bytes());
            self.pending.push(Pending { part: format!("xl/pivotTables/pivotTable{t}.xml"), sheet: si, idx, cache_id: n, cache });
        }
    }

    /// Adds the workbook relationships to the caches; returns the `<pivotCaches>` element (empty
    /// when there are none).
    pub fn workbook_xml(&self, wb_rels: &mut Rels) -> String {
        if self.caches.is_empty() {
            return String::new();
        }
        let mut s = String::from("<pivotCaches>");
        for (id, target) in &self.caches {
            let rid = wb_rels.add("pivotCacheDefinition", target);
            let _ = write!(s, "<pivotCache cacheId=\"{id}\" r:id=\"{rid}\"/>");
        }
        s.push_str("</pivotCaches>");
        s
    }

    /// Number formats used by value fields (to be registered in the styles part).
    pub fn number_formats(&self, wb: &Workbook) -> Vec<String> {
        let mut v = Vec::new();
        for p in &self.pending {
            if let Some(pt) = wb.sheet(p.sheet).and_then(|s| s.pivots.get(p.idx)) {
                for val in &pt.values {
                    if let Some(f) = &val.number_format
                        && !v.contains(f)
                    {
                        v.push(f.clone());
                    }
                }
            }
        }
        v
    }

    /// The pivot cache id of the PivotTable at (sheet, index), if it was written.
    pub fn cache_id(&self, sheet: usize, idx: usize) -> Option<u32> {
        self.pending.iter().find(|p| p.sheet == sheet && p.idx == idx).map(|p| p.cache_id)
    }

    /// The first and last number of source field `field` of the PivotTable at (sheet, index).
    pub fn bounds(&self, sheet: usize, idx: usize, field: &str) -> Option<(f64, f64)> {
        let p = self.pending.iter().find(|p| p.sheet == sheet && p.idx == idx)?;
        let f = p.cache.fields.iter().find(|f| f.col.is_some() && same(&f.name, field))?;
        f.stats.number.then_some((f.stats.min, f.stats.max))
    }

    /// Writes the PivotTable parts. `fmt_id` maps a number format code to its styles id.
    pub fn finish(self, wb: &Workbook, out: &mut Out, fmt_id: &dyn Fn(&str) -> Option<u32>) {
        for p in self.pending {
            let Some(pt) = wb.sheet(p.sheet).and_then(|s| s.pivots.get(p.idx)) else { continue };
            // The selections of the timelines filtering this PivotTable.
            let timeline_filters: Vec<String> = crate::timeline::filters_of(wb, pt.id)
                .into_iter()
                .zip(crate::timeline::FILTER_ID_BASE..)
                .filter_map(|(t, id)| {
                    let fld = p.cache.fields.iter().position(|f| f.col.is_some() && same(&f.name, &t.field))?;
                    Some(crate::timeline::filter_xml(fld, id, t))
                })
                .collect();
            let xml = table_xml(pt, p.idx, p.cache_id, &p.cache, fmt_id, &timeline_filters);
            out.part(&p.part, Some(CT_PIVOT_TABLE), xml.into_bytes());
        }
    }
}

/// Resolves a pivot source to (sheet, range including the header row, worksheetSource XML).
fn resolve_source(wb: &Workbook, text: &str, default_sheet: usize, depth: u32) -> Option<(usize, RangeRef, String)> {
    let t = text.trim().trim_start_matches('=').trim();
    if t.is_empty() || depth > 4 {
        return None;
    }
    if let Some((si, ti)) = wb.table(t)
        && let Some(sh) = wb.sheet(si)
        && let Some(tb) = sh.tables.get(ti)
    {
        if !tb.header_row {
            return None;
        }
        let end_row = tb.range.end.row.saturating_sub(tb.totals_row as u32).max(tb.range.start.row);
        let r = RangeRef::new(tb.range.start, CellRef::new(end_row, tb.range.end.col));
        // Table names are sanitized when written; refer by range when this one would change.
        let valid = !tb.name.is_empty()
            && tb.name.chars().all(|c| c.is_alphanumeric() || c == '_' || c == '.')
            && tb.name.starts_with(|c: char| c.is_alphabetic() || c == '_')
            && CellRef::parse(&tb.name).is_none();
        let xml = if valid { format!("<worksheetSource name=\"{}\"/>", esc_attr(&tb.name)) } else { range_source_xml(&sh.name, r) };
        return Some((si, r, xml));
    }
    let (sheet, body) = match t.rsplit_once('!') {
        Some((sh, body)) => (Some(sh.trim_matches('\'').replace("''", "'")), body),
        None => (None, t),
    };
    if let Some(r) = RangeRef::parse(body) {
        let si = match &sheet {
            Some(n) => wb.sheet_index(n)?,
            None => default_sheet,
        };
        let sh = wb.sheet(si)?;
        let r = if r.is_full_cols() || r.is_full_rows() { r.intersection(&sh.used_range()?)? } else { r };
        return Some((si, r, range_source_xml(&sh.name, r)));
    }
    if sheet.is_none()
        && let Some(n) = wb.name(t, default_sheet)
    {
        let (si, r, _) = resolve_source(wb, &n.formula.clone(), default_sheet, depth + 1)?;
        let xml = if n.scope.is_none() {
            format!("<worksheetSource name=\"{}\"/>", esc_attr(&n.name))
        } else {
            wb.sheet(si).map(|s| range_source_xml(&s.name, r))?
        };
        return Some((si, r, xml));
    }
    None
}

fn range_source_xml(sheet: &str, r: RangeRef) -> String {
    let name: String = sheet.chars().take(31).collect();
    format!("<worksheetSource ref=\"{}\" sheet=\"{}\"/>", r.a1(), esc_attr(&name))
}

fn header_text(v: &Value) -> String {
    match v {
        Value::Empty => String::new(),
        v => label(v).trim().to_string(),
    }
}

fn build_cache(wb: &Workbook, pt: &PivotTable, pivot_sheet: usize) -> Option<Cache> {
    let (si, r, source_xml) = resolve_source(wb, &pt.source, pivot_sheet, 0)?;
    if r.count() > MAX_SOURCE_CELLS {
        return None;
    }
    let sh = wb.sheet(si)?;
    let mut fields: Vec<CField> = Vec::new();
    // Lower-case names taken, and the next suffix for each header (linear for repeated headers).
    let mut taken = std::collections::HashSet::new();
    let mut next_suffix: HashMap<String, usize> = HashMap::new();
    for (k, c) in (r.start.col..=r.end.col).enumerate() {
        let mut h = header_text(&sh.value(CellRef::new(r.start.row, c)).scalar());
        if h.is_empty() {
            h = format!("Column{}", k + 1);
        }
        let mut n = h.clone();
        let mut i = next_suffix.get(&h.to_lowercase()).copied().unwrap_or(2);
        while taken.contains(&n.to_lowercase()) {
            n = format!("{h}{i}");
            i += 1;
        }
        next_suffix.insert(h.to_lowercase(), i);
        taken.insert(n.to_lowercase());
        fields.push(CField { name: n, col: Some(k as u32), ..CField::default() });
    }
    let find = |fields: &[CField], name: &str| fields.iter().position(|f| f.col.is_some() && same(&f.name, name));

    // Records (rows that aren't entirely blank), read once.
    let mut rows: Vec<u32> = Vec::new();
    if r.end.row > r.start.row {
        for row in r.start.row + 1..=r.end.row {
            let blank = (r.start.col..=r.end.col).all(|c| sh.value(CellRef::new(row, c)).scalar().is_empty());
            if !blank {
                rows.push(row);
            }
        }
    }
    for &row in &rows {
        for f in fields.iter_mut() {
            let c = r.start.col + f.col.unwrap_or(0);
            f.stats.add(&sh.value(CellRef::new(row, c)).scalar());
        }
    }

    // Which fields are on an axis, and how dates are grouped: per base column, the base field
    // takes the finest grouping and coarser ones become extra (non-database) fields chained
    // through `par`, as Excel lays them out.
    let axis: Vec<&PivotField> = pt.rows.iter().chain(pt.columns.iter()).collect();
    let groupable = |f: &CField| {
        let s = &f.stats;
        s.number
            && !s.text
            && !s.boolean
            && !s.error
            && datetime_from_serial(wb.date_system, s.min).is_some()
            && datetime_from_serial(wb.date_system, s.max).is_some()
    };
    // (base column field, groupings fine → coarse, the Grouping dialog's bounds and day span)
    let mut groupings: Vec<(usize, Vec<PivotDateGroup>, Option<PivotDateRange>)> = Vec::new();
    for pf in &axis {
        let Some(b) = find(&fields, &pf.source_col) else { continue };
        if pf.date_group == PivotDateGroup::None || !fields.get(b).is_some_and(groupable) {
            continue;
        }
        match groupings.iter_mut().find(|(x, ..)| *x == b) {
            Some((_, v, range)) => {
                if !v.contains(&pf.date_group) {
                    v.push(pf.date_group);
                }
                if range.is_none() {
                    *range = pf.date_range;
                }
            }
            None => groupings.push((b, vec![pf.date_group], pf.date_range)),
        }
    }
    let fineness = |g: &PivotDateGroup| g.fineness().unwrap_or(PivotDateGroup::LEVELS.len());
    for (b, list, range) in groupings.iter_mut() {
        list.sort_by_key(fineness);
        let (min, max) = fields.get(*b).map(|f| (f.stats.min, f.stats.max)).unwrap_or((0.0, 0.0));
        let range = range.unwrap_or_default();
        // Excel's automatic end is the day after a last whole date.
        let auto_end = if max.fract() == 0.0 { max + 1.0 } else { max }.floor();
        let (start, end) = (range.start.unwrap_or(min).floor(), range.end.map_or(auto_end, f64::floor));
        let fixed = (range.start.is_some(), range.end.is_some());
        let days = range.days.clamp(1, PivotDateRange::MAX_DAYS);
        let make = |by: PivotDateGroup| {
            let days = if by == PivotDateGroup::Days { days } else { 1 };
            Group { by, base: *b, start, end, fixed, days, items: group_items(wb.date_system, by, start, end, days), ..Group::default() }
        };
        let mut prev = *b;
        for (k, by) in list.iter().enumerate() {
            if k == 0 {
                if let Some(f) = fields.get_mut(*b) {
                    f.group = Some(make(*by));
                }
                continue;
            }
            let new = fields.len();
            let caption = group_caption(*by);
            let mut n = caption.to_string();
            let mut i = 2;
            while fields.iter().any(|f| same(&f.name, &n)) {
                n = format!("{caption}{i}");
                i += 1;
            }
            if let Some(g) = fields.get_mut(prev).and_then(|f| f.group.as_mut()) {
                g.par = Some(new);
            }
            fields.push(CField { name: n, group: Some(make(*by)), ..CField::default() });
            prev = new;
        }
    }
    // Number ranges group the base field in place (a field has one grouping; dates come first).
    for pf in &axis {
        let (Some(ng), Some(b)) = (pf.number_group, find(&fields, &pf.source_col)) else { continue };
        if let Some(f) = fields.get_mut(b)
            && f.group.is_none()
            && f.stats.number
            && !f.stats.text
            && !f.stats.boolean
            && !f.stats.error
        {
            f.group = Some(Group { number: Some(ng), base: b, start: ng.start, end: ng.end, days: 1, items: ng.captions(), ..Group::default() });
        }
    }
    // Group Selection fields: extra fields over their base field's shared items (`discretePr`).
    let mut manual: Vec<(usize, &PivotGroupField, usize)> = Vec::new();
    for gf in &pt.group_fields {
        let used = axis.iter().any(|a| same(&a.source_col, &gf.name)) || pt.filters.iter().any(|f| same(&f.source_col, &gf.name));
        let Some(b) = find(&fields, &gf.base) else { continue };
        if !used || fields.get(b).is_some_and(|f| f.group.is_some() || f.par.is_some()) || fields.iter().any(|f| same(&f.name, &gf.name)) {
            continue;
        }
        let new = fields.len();
        if let Some(f) = fields.get_mut(b) {
            f.itemized = true;
            f.par = Some(new);
        }
        fields.push(CField {
            name: gf.name.clone(),
            group: Some(Group { base: b, days: 1, discrete: Some(vec![]), ..Group::default() }),
            ..CField::default()
        });
        manual.push((new, gf, b));
    }
    let find_manual = |fields: &[CField], name: &str| {
        fields.iter().position(|f| f.col.is_none() && f.group.as_ref().is_some_and(|g| g.discrete.is_some()) && same(&f.name, name))
    };
    let mut axis_map: Vec<Option<usize>> = Vec::with_capacity(axis.len());
    for pf in &axis {
        let Some(b) = find(&fields, &pf.source_col) else {
            axis_map.push(find_manual(&fields, &pf.source_col));
            continue;
        };
        let grouped = fields.iter().position(|f| f.group.as_ref().is_some_and(|g| g.base == b && g.by == pf.date_group));
        match grouped {
            Some(i) if pf.date_group != PivotDateGroup::None => axis_map.push(Some(i)),
            _ => {
                if let Some(f) = fields.get_mut(b)
                    && f.group.is_none()
                {
                    f.itemized = true;
                }
                axis_map.push(Some(b));
            }
        }
    }
    // Page fields (filters not on an axis) need items too.
    for f in &pt.filters {
        let on_axis = axis.iter().any(|a| same(&a.source_col, &f.source_col));
        if !on_axis
            && let Some(i) = find(&fields, &f.source_col)
            && let Some(cf) = fields.get_mut(i)
        {
            cf.itemized = true;
        }
    }
    // A timeline's date field is written as dates (Excel's timelines need a date field).
    for tl in wb.sheets.iter().flat_map(|s| s.timelines.iter()).filter(|t| t.pivots.contains(&pt.id)) {
        if let Some(cf) = find(&fields, &tl.field).and_then(|i| fields.get_mut(i))
            && cf.group.is_none()
            && !cf.itemized
            && cf.stats.number
            && !cf.stats.text
            && !cf.stats.boolean
            && !cf.stats.error
            && datetime_from_serial(wb.date_system, cf.stats.min).is_some()
            && datetime_from_serial(wb.date_system, cf.stats.max).is_some()
        {
            cf.dates = true;
            cf.itemized = false;
        }
    }
    for f in fields.iter_mut() {
        if f.group.is_some() {
            f.itemized = false;
        } else if f.col.is_some() && (f.stats.text || f.stats.boolean || f.stats.error || f.par.is_some()) {
            f.itemized = true;
        }
    }
    // Shared items, in order of first appearance.
    for &row in &rows {
        for f in fields.iter_mut().filter(|f| f.itemized) {
            let c = r.start.col + f.col.unwrap_or(0);
            let v = sh.value(CellRef::new(row, c)).scalar();
            let k = key(&v);
            if !f.index.contains_key(&k) && f.items.len() < MAX_ITEMS {
                f.index.insert(k, f.items.len() as u32);
                f.items.push(v);
            }
        }
    }
    // Group Selection: each base item's group item; base items in no group are items of their own.
    for (gi, gf, b) in &manual {
        let base_labels: Vec<String> = fields.get(*b).map(|f| f.items.iter().map(label).collect()).unwrap_or_default();
        let mut items: Vec<String> = Vec::new();
        let mut map: Vec<u32> = Vec::with_capacity(base_labels.len());
        for l in &base_labels {
            let caption = gf.group_of(l).map(|g| g.name.clone()).unwrap_or_else(|| l.clone());
            let k = match items.iter().position(|x| same(x, &caption)) {
                Some(k) => k,
                None => {
                    items.push(caption);
                    items.len() - 1
                }
            };
            map.push(k as u32);
        }
        if let Some(g) = fields.get_mut(*gi).and_then(|f| f.group.as_mut()) {
            g.items = items;
            g.discrete = Some(map);
        }
    }
    // Calculated items are shared items of their field, after the data items; no record uses them.
    let mut calc_items: Vec<(usize, u32, String)> = Vec::new();
    for ci in &pt.calculated_items {
        let Some(i) = find(&fields, &ci.field) else { continue };
        let Some(f) = fields.get_mut(i) else { continue };
        if !f.itemized || f.group.is_some() || f.items.len() >= MAX_ITEMS || f.items.iter().any(|v| same(&label(v), &ci.name)) {
            continue;
        }
        let formula = ci.formula.trim();
        calc_items.push((i, f.items.len() as u32, formula.strip_prefix('=').unwrap_or(formula).trim().to_string()));
        f.items.push(Value::text(ci.name.as_str()));
    }
    // Records snapshot.
    let db_fields = fields.iter().filter(|f| f.col.is_some()).count() as u64;
    let records = if (rows.len() as u64).saturating_mul(db_fields) <= MAX_RECORD_CELLS {
        let mut s = String::new();
        for &row in &rows {
            s.push_str("<r>");
            for f in fields.iter().filter(|f| f.col.is_some()) {
                let c = r.start.col + f.col.unwrap_or(0);
                let v = sh.value(CellRef::new(row, c)).scalar();
                if f.itemized {
                    match f.index.get(&key(&v)) {
                        Some(i) => {
                            let _ = write!(s, "<x v=\"{i}\"/>");
                        }
                        None => s.push_str(&item_xml(&v, wb.date_system, false)),
                    }
                } else {
                    s.push_str(&item_xml(&v, wb.date_system, f.date_grouped()));
                }
            }
            s.push_str("</r>");
        }
        Some((rows.len() as u64, s))
    } else {
        None
    };
    // Calculated fields come after the database and grouping fields.
    for cf in &pt.calculated_fields {
        let f = cf.formula.trim();
        let f = f.strip_prefix('=').unwrap_or(f).trim();
        if cf.name.trim().is_empty() || f.is_empty() || fields.iter().any(|x| same(&x.name, &cf.name)) {
            continue;
        }
        fields.push(CField { name: cf.name.clone(), formula: Some(f.to_string()), ..CField::default() });
    }
    let ext_id = None;
    Some(Cache { fields, axis_map, source_xml, records, calc_items, ext_id })
}

fn group_items(sys: DateSystem, by: PivotDateGroup, start: f64, end: f64, days: u32) -> Vec<String> {
    let mut v = vec![format!("<{}", mdy(sys, start))];
    match by {
        PivotDateGroup::Days if days > 1 => {
            // Spans of `days` days from the start, the last one cut at the end.
            let step = f64::from(days);
            let mut lo = start;
            while lo <= end && v.len() < MAX_SPANS {
                v.push(format!("{} - {}", mdy(sys, lo), mdy(sys, (lo + step - 1.0).min(end))));
                lo += step;
            }
        }
        PivotDateGroup::Years => {
            let y0 = datetime_from_serial(sys, start).map(|d| d.year).unwrap_or(1900);
            let y1 = datetime_from_serial(sys, end - 1.0).map(|d| d.year).unwrap_or(y0).max(y0);
            for y in y0..=y1.min(y0 + 10_000) {
                v.push(y.to_string());
            }
        }
        PivotDateGroup::Quarters => v.extend((1..=4).map(|q| format!("Qtr{q}"))),
        PivotDateGroup::Months => v.extend(MONTHS.iter().map(|m| m.to_string())),
        PivotDateGroup::Hours => v.extend((0..24).map(hour_caption)),
        PivotDateGroup::Minutes | PivotDateGroup::Seconds => v.extend((0..60).map(|m| format!(":{m:02}"))),
        PivotDateGroup::Days | PivotDateGroup::None => {
            const DAYS: [u32; 12] = [31, 29, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31];
            for (m, n) in MONTHS.iter().zip(DAYS) {
                for d in 1..=n {
                    v.push(format!("{d}-{m}"));
                }
            }
        }
    }
    v.push(format!(">{}", mdy(sys, end)));
    v
}

/// A cache value element (`<n>`, `<s>`, `<m>`…); `date` writes numbers as `<d>`.
fn item_xml(v: &Value, sys: DateSystem, date: bool) -> String {
    match v {
        Value::Empty => "<m/>".into(),
        Value::Number(n) => {
            if date && let Some(d) = iso(sys, *n) {
                return format!("<d v=\"{d}\"/>");
            }
            format!("<n v=\"{}\"/>", crate::xml::num(*n))
        }
        Value::Text(t) => format!("<s v=\"{}\"/>", esc_attr(t)),
        Value::Bool(b) => format!("<b v=\"{}\"/>", u8::from(*b)),
        Value::Error(e) => format!("<e v=\"{}\"/>", esc_attr(e.as_str())),
        Value::Array(_) => item_xml(&v.scalar(), sys, date),
    }
}

fn cache_xml(c: &Cache, records_rid: Option<&str>, sys: DateSystem) -> String {
    let mut s = format!("{XML_DECL}<pivotCacheDefinition xmlns=\"{NS_MAIN}\" xmlns:r=\"{NS_REL}\"");
    match (records_rid, &c.records) {
        (Some(rid), Some((count, _))) => {
            let _ = write!(s, " r:id=\"{rid}\" recordCount=\"{count}\"");
        }
        _ => s.push_str(" saveData=\"0\""),
    }
    s.push_str(" refreshOnLoad=\"1\" refreshedBy=\"GridCraft\" createdVersion=\"6\" refreshedVersion=\"6\" minRefreshableVersion=\"3\">");
    let _ = write!(s, "<cacheSource type=\"worksheet\">{}</cacheSource>", c.source_xml);
    let _ = write!(s, "<cacheFields count=\"{}\">", c.fields.len());
    for f in &c.fields {
        let fmt = if f.date_grouped() { 14 } else { 0 };
        let _ = write!(s, "<cacheField name=\"{}\" numFmtId=\"{fmt}\"", esc_attr(&f.name));
        if let Some(formula) = &f.formula {
            let _ = write!(s, " formula=\"{}\"", esc_attr(formula));
        }
        if f.col.is_none() {
            s.push_str(" databaseField=\"0\"");
        }
        s.push('>');
        let st = &f.stats;
        if f.col.is_some() {
            s.push_str("<sharedItems");
            if f.date_grouped() {
                s.push_str(" containsSemiMixedTypes=\"0\" containsNonDate=\"0\" containsDate=\"1\" containsString=\"0\"");
                if st.blank {
                    s.push_str(" containsBlank=\"1\"");
                }
                if let (Some(a), Some(b)) = (iso(sys, st.min), iso(sys, st.max)) {
                    let _ = write!(s, " minDate=\"{a}\" maxDate=\"{b}\"");
                }
            } else {
                let semi = st.text || st.blank || st.boolean || st.error;
                if !semi {
                    s.push_str(" containsSemiMixedTypes=\"0\"");
                }
                if !st.text {
                    s.push_str(" containsString=\"0\"");
                }
                if st.blank {
                    s.push_str(" containsBlank=\"1\"");
                }
                if st.kinds() > 1 {
                    s.push_str(" containsMixedTypes=\"1\"");
                }
                if st.number {
                    s.push_str(" containsNumber=\"1\"");
                    if !st.non_int {
                        s.push_str(" containsInteger=\"1\"");
                    }
                    let _ = write!(s, " minValue=\"{}\" maxValue=\"{}\"", crate::xml::num(st.min), crate::xml::num(st.max));
                }
                if st.long {
                    s.push_str(" longText=\"1\"");
                }
            }
            if f.itemized {
                let _ = write!(s, " count=\"{}\">", f.items.len());
                for v in &f.items {
                    s.push_str(&item_xml(v, sys, false));
                }
                s.push_str("</sharedItems>");
            } else {
                s.push_str("/>");
            }
        }
        if let (None, Some(p)) = (&f.group, f.par) {
            let _ = write!(s, "<fieldGroup par=\"{p}\"/>");
        }
        if let Some(g) = &f.group {
            s.push_str("<fieldGroup");
            if let Some(p) = g.par {
                let _ = write!(s, " par=\"{p}\"");
            }
            let _ = write!(s, " base=\"{}\">", g.base);
            if let Some(map) = &g.discrete {
                let _ = write!(s, "<discretePr count=\"{}\">", map.len());
                for k in map {
                    let _ = write!(s, "<x v=\"{k}\"/>");
                }
                let _ = write!(s, "</discretePr><groupItems count=\"{}\">", g.items.len());
                for it in &g.items {
                    let _ = write!(s, "<s v=\"{}\"/>", esc_attr(it));
                }
                s.push_str("</groupItems></fieldGroup></cacheField>");
                continue;
            }
            match g.number {
                Some(ng) => {
                    let _ = write!(
                        s,
                        "<rangePr autoStart=\"0\" autoEnd=\"0\" startNum=\"{}\" endNum=\"{}\" groupInterval=\"{}\"",
                        crate::xml::num(ng.start),
                        crate::xml::num(ng.end),
                        crate::xml::num(ng.by)
                    );
                }
                None => {
                    s.push_str("<rangePr");
                    if g.fixed.0 {
                        s.push_str(" autoStart=\"0\"");
                    }
                    if g.fixed.1 {
                        s.push_str(" autoEnd=\"0\"");
                    }
                    let _ = write!(s, " groupBy=\"{}\"", group_by_name(g.by));
                    if let (Some(a), Some(b)) = (iso(sys, g.start), iso(sys, g.end)) {
                        let _ = write!(s, " startDate=\"{a}\" endDate=\"{b}\"");
                    }
                    if g.days > 1 {
                        let _ = write!(s, " groupInterval=\"{}\"", g.days);
                    }
                }
            }
            let _ = write!(s, "/><groupItems count=\"{}\">", g.items.len());
            for it in &g.items {
                let _ = write!(s, "<s v=\"{}\"/>", esc_attr(it));
            }
            s.push_str("</groupItems></fieldGroup>");
        }
        s.push_str("</cacheField>");
    }
    s.push_str("</cacheFields>");
    if !c.calc_items.is_empty() {
        let _ = write!(s, "<calculatedItems count=\"{}\">", c.calc_items.len());
        for (fi, x, formula) in &c.calc_items {
            let _ = write!(
                s,
                "<calculatedItem field=\"{fi}\" formula=\"{}\"><pivotArea cacheIndex=\"1\" outline=\"0\" fieldPosition=\"0\"><references count=\"1\"><reference field=\"{fi}\" count=\"1\"><x v=\"{x}\"/></reference></references></pivotArea></calculatedItem>",
                esc_attr(formula)
            );
        }
        s.push_str("</calculatedItems>");
    }
    if let Some(id) = c.ext_id {
        let _ =
            write!(s, "<extLst><ext uri=\"{EXT_X14_CACHE}\" xmlns:x14=\"{NS_X14}\"><x14:pivotCacheDefinition pivotCacheId=\"{id}\"/></ext></extLst>");
    }
    s.push_str("</pivotCacheDefinition>");
    s
}

fn table_xml(pt: &PivotTable, idx: usize, cache_id: u32, c: &Cache, fmt_id: &dyn Fn(&str) -> Option<u32>, extra_filters: &[String]) -> String {
    let nf = c.fields.len();
    let find_db = |name: &str| c.fields.iter().position(|f| f.col.is_some() && same(&f.name, name));
    let find_page = |name: &str| {
        find_db(name).or_else(|| c.fields.iter().position(|f| f.group.as_ref().is_some_and(|g| g.discrete.is_some()) && same(&f.name, name)))
    };
    let row_idx: Vec<usize> = c.axis_map.iter().take(pt.rows.len()).filter_map(|x| *x).collect();
    let col_idx: Vec<usize> = c.axis_map.iter().skip(pt.rows.len()).filter_map(|x| *x).collect();
    // Rules off the axes are dormant, not page fields. Item selections off an axis are pages.
    let mut hidden: HashMap<usize, &PivotFilter> = HashMap::new();
    let mut pages: Vec<(usize, &PivotFilter)> = Vec::new();
    for f in &pt.filters {
        let axis_pos = pt.rows.iter().chain(pt.columns.iter()).position(|a| same(&a.source_col, &f.source_col));
        match axis_pos {
            Some(p) => {
                if let Some(Some(i)) = c.axis_map.get(p) {
                    hidden.insert(*i, f);
                }
            }
            None if f.rule.is_none() => {
                if let Some(i) = find_page(&f.source_col)
                    && !pages.iter().any(|(j, _)| *j == i)
                {
                    pages.push((i, f));
                    hidden.insert(i, f);
                }
            }
            None => {}
        }
    }
    let find_calc = |name: &str| c.fields.iter().position(|f| f.formula.is_some() && same(&f.name, name));
    let vals: Vec<(usize, &PivotValue)> =
        pt.values.iter().filter_map(|v| find_db(&v.source_col).or_else(|| find_calc(&v.source_col)).map(|i| (i, v))).collect();
    let axis_field = |i: usize| -> Option<&PivotField> {
        let p = c.axis_map.iter().position(|x| *x == Some(i))?;
        pt.rows.iter().chain(pt.columns.iter()).nth(p)
    };

    let compact = pt.layout == PivotLayout::Compact;
    let name = if pt.name.trim().is_empty() { format!("PivotTable{}", idx + 1) } else { pt.name.clone() };
    let mut s = format!(
        "{XML_DECL}<pivotTableDefinition xmlns=\"{NS_MAIN}\" name=\"{}\" cacheId=\"{cache_id}\" applyNumberFormats=\"0\" applyBorderFormats=\"0\" applyFontFormats=\"0\" applyPatternFormats=\"0\" applyAlignmentFormats=\"0\" applyWidthHeightFormats=\"1\" dataCaption=\"Values\" updatedVersion=\"6\" minRefreshableVersion=\"3\" useAutoFormatting=\"1\" itemPrintTitles=\"1\" createdVersion=\"6\" indent=\"0\"",
        esc_attr(&name)
    );
    match pt.layout {
        PivotLayout::Compact => s.push_str(" outline=\"1\" outlineData=\"1\""),
        PivotLayout::Outline => s.push_str(" compact=\"0\" compactData=\"0\" outline=\"1\" outlineData=\"1\""),
        PivotLayout::Tabular => s.push_str(" compact=\"0\" compactData=\"0\""),
    }
    if pt.values_on_rows {
        s.push_str(" dataOnRows=\"1\"");
    }
    if !pt.show_headers {
        s.push_str(" showHeaders=\"0\"");
    }
    if !pt.grand_totals_rows {
        s.push_str(" rowGrandTotals=\"0\"");
    }
    if !pt.grand_totals_cols {
        s.push_str(" colGrandTotals=\"0\"");
    }
    s.push_str(" multipleFieldFilters=\"0\">");

    // Location: the body (report filters sit above it).
    let a = pt.anchor;
    let end = pt.last_range.map(|r| r.end).unwrap_or(a);
    let loc = RangeRef::new(a, CellRef::new(end.row.max(a.row), end.col.max(a.col)));
    let k = vals.len();
    // Σ Values (two or more value fields): its index among the written fields of its axis.
    let (row_map, _) = c.axis_map.split_at(pt.rows.len().min(c.axis_map.len()));
    let values_at = |map: &[Option<usize>], n: usize| pt.values_position.map_or(n, |p| map.iter().take(p).filter(|x| x.is_some()).count());
    let row_values = (k > 1 && pt.values_on_rows).then(|| values_at(row_map, row_idx.len()));
    // In Columns Σ Values comes after the last field: the column layout doesn't place it elsewhere.
    let col_values = (k > 1 && !pt.values_on_rows).then_some(col_idx.len());
    let first_data_row = if col_idx.is_empty() { 1 } else { 1 + col_idx.len() + usize::from(col_values.is_some()) };
    let row_levels = row_idx.len() + usize::from(row_values.is_some());
    let first_data_col = if row_levels == 0 && col_idx.is_empty() {
        0
    } else if compact {
        1
    } else {
        row_levels.max(1)
    };
    let _ = write!(s, "<location ref=\"{}\" firstHeaderRow=\"1\" firstDataRow=\"{first_data_row}\" firstDataCol=\"{first_data_col}\"", loc.a1());
    if !pages.is_empty() {
        let _ = write!(s, " rowPageCount=\"{}\" colPageCount=\"1\"", pages.len());
    }
    s.push_str("/>");

    // Fields.
    let _ = write!(s, "<pivotFields count=\"{nf}\">");
    for (i, f) in c.fields.iter().enumerate() {
        s.push_str("<pivotField");
        let axis = if row_idx.contains(&i) {
            Some("axisRow")
        } else if col_idx.contains(&i) {
            Some("axisCol")
        } else if pages.iter().any(|(j, _)| *j == i) {
            Some("axisPage")
        } else {
            None
        };
        if let Some(ax) = axis {
            let _ = write!(s, " axis=\"{ax}\"");
        }
        if vals.iter().any(|(j, _)| *j == i) {
            s.push_str(" dataField=\"1\"");
        }
        if f.formula.is_some() {
            s.push_str(" dragToRow=\"0\" dragToCol=\"0\" dragToPage=\"0\"");
        }
        match pt.layout {
            PivotLayout::Compact => {}
            PivotLayout::Outline => s.push_str(" compact=\"0\""),
            PivotLayout::Tabular => s.push_str(" compact=\"0\" outline=\"0\""),
        }
        if f.date_grouped() {
            s.push_str(" numFmtId=\"14\"");
        }
        let filter = hidden.get(&i).copied();
        let labels = f.item_labels();
        let is_page = axis == Some("axisPage");
        // Hidden items; never hide every item (a field needs one visible item).
        let mut hide: Vec<bool> = match filter.and_then(|f| f.selected.as_ref()) {
            Some(sel) if !(is_page && sel.len() == 1) => labels.iter().map(|l| !sel.iter().any(|x| same(x, l))).collect(),
            _ => vec![false; labels.len()],
        };
        if !hide.is_empty() && hide.iter().all(|h| *h) {
            hide.iter_mut().for_each(|h| *h = false);
        }
        if is_page && hide.iter().any(|h| *h) {
            s.push_str(" multipleItemSelectionAllowed=\"1\"");
        }
        s.push_str(" showAll=\"0\"");
        if let Some(af) = axis_field(i) {
            match af.sort {
                // Excel sorts the captions of date groups as text under `sortType`; their own
                // order is already chronological, so ascending date groups go without it.
                PivotSort::Asc if f.date_grouped() => {}
                PivotSort::Asc => s.push_str(" sortType=\"ascending\""),
                PivotSort::Desc => s.push_str(" sortType=\"descending\""),
                PivotSort::None => {}
            }
        }
        if !pt.subtotals {
            s.push_str(" defaultSubtotal=\"0\"");
        }
        if !pt.subtotals_top {
            s.push_str(" subtotalTop=\"0\"");
        }
        if axis.is_some() {
            let collapsed: &[String] = axis_field(i).map(|f| f.collapsed_items.as_slice()).unwrap_or(&[]);
            let count = labels.len() + usize::from(pt.subtotals);
            let _ = write!(s, "><items count=\"{count}\">");
            for (x, l) in labels.iter().enumerate() {
                let _ = write!(s, "<item x=\"{x}\"");
                if hide.get(x).copied().unwrap_or(false) {
                    s.push_str(" h=\"1\"");
                }
                if collapsed.iter().any(|cl| same(cl, l)) {
                    s.push_str(" sd=\"0\"");
                }
                s.push_str("/>");
            }
            if pt.subtotals {
                s.push_str("<item t=\"default\"/>");
            }
            s.push_str("</items></pivotField>");
        } else {
            s.push_str("/>");
        }
    }
    s.push_str("</pivotFields>");

    axis_fields_xml(&mut s, "rowFields", &row_idx, row_values);
    axis_fields_xml(&mut s, "colFields", &col_idx, col_values);
    if !pages.is_empty() {
        let _ = write!(s, "<pageFields count=\"{}\">", pages.len());
        for (i, f) in &pages {
            let _ = write!(s, "<pageField fld=\"{i}\"");
            if let Some(sel) = &f.selected
                && sel.len() == 1
                && let Some(one) = sel.first()
                && let Some(x) = c.fields.get(*i).and_then(|cf| cf.item_labels().iter().position(|l| same(l, one)))
            {
                let _ = write!(s, " item=\"{x}\"");
            }
            s.push_str(" hier=\"-1\"/>");
        }
        s.push_str("</pageFields>");
    }
    if !vals.is_empty() {
        let _ = write!(s, "<dataFields count=\"{k}\">");
        let mut used: Vec<String> = Vec::new();
        for (i, v) in &vals {
            let base = if v.name.trim().is_empty() {
                format!("{} of {}", func_caption(v.func), c.fields.get(*i).map(|f| f.name.as_str()).unwrap_or(""))
            } else {
                v.name.clone()
            };
            let mut n = base.clone();
            let mut j = 2;
            while used.iter().any(|u| same(u, &n)) {
                n = format!("{base}{j}");
                j += 1;
            }
            used.push(n.clone());
            let _ = write!(s, "<dataField name=\"{}\" fld=\"{i}\"", esc_attr(&n));
            if v.func != PivotFunc::Sum {
                let _ = write!(s, " subtotal=\"{}\"", func_name(v.func));
            }
            // The base field: the named axis field, else the first row (or column) field.
            let named = v.base_field.as_deref().and_then(|b| {
                let p = pt.rows.iter().chain(pt.columns.iter()).position(|a| same(&a.source_col, b))?;
                c.axis_map.get(p).copied().flatten()
            });
            let base_field = named.or(row_idx.first().or(col_idx.first()).copied()).unwrap_or(0);
            let base_item = match v.base_item.as_deref() {
                Some(BASE_PREVIOUS) => 1_048_828,
                Some(BASE_NEXT) => 1_048_829,
                Some(label) => c.fields.get(base_field).and_then(|f| f.item_labels().iter().position(|l| same(l, label))).unwrap_or(0),
                None => 0,
            };
            let (data_as, x14): (Option<&str>, Option<&str>) = match v.show_as {
                PivotShowAs::Normal => (None, None),
                PivotShowAs::PercentOfGrandTotal => (Some("percentOfTotal"), None),
                PivotShowAs::PercentOfColumnTotal => (Some("percentOfCol"), None),
                PivotShowAs::PercentOfRowTotal => (Some("percentOfRow"), None),
                PivotShowAs::PercentOf => (Some("percent"), None),
                PivotShowAs::DifferenceFrom => (Some("difference"), None),
                PivotShowAs::PercentDifferenceFrom => (Some("percentDiff"), None),
                PivotShowAs::RunningTotal => (Some("runTotal"), None),
                PivotShowAs::Index => (Some("index"), None),
                PivotShowAs::PercentOfParentRowTotal => (None, Some("percentOfParentRow")),
                PivotShowAs::PercentOfParentColumnTotal => (None, Some("percentOfParentCol")),
                PivotShowAs::PercentOfParentTotal => (None, Some("percentOfParent")),
                PivotShowAs::PercentRunningTotal => (None, Some("percentOfRunningTotal")),
                PivotShowAs::RankAscending => (None, Some("rankAscending")),
                PivotShowAs::Rank => (None, Some("rankDescending")),
            };
            if let Some(a) = data_as {
                let _ = write!(s, " showDataAs=\"{a}\"");
            }
            let _ = write!(s, " baseField=\"{base_field}\" baseItem=\"{base_item}\"");
            if let Some(id) = v.number_format.as_deref().and_then(fmt_id) {
                let _ = write!(s, " numFmtId=\"{id}\"");
            }
            match x14 {
                Some(x) => {
                    let _ = write!(
                        s,
                        "><extLst><ext uri=\"{EXT_X14_DATA_FIELD}\" xmlns:x14=\"{NS_X14}\"><x14:dataField pivotShowAs=\"{x}\"/></ext></extLst></dataField>"
                    );
                }
                None => s.push_str("/>"),
            }
        }
        s.push_str("</dataFields>");
    }
    s.push_str("<pivotTableStyleInfo");
    if !pt.style.is_empty() {
        let _ = write!(s, " name=\"{}\"", esc_attr(&pt.style));
    }
    s.push_str(" showRowHeaders=\"1\" showColHeaders=\"1\" showRowStripes=\"0\" showColStripes=\"0\" showLastColumn=\"1\"/>");
    let filter_field = |name: &str| match pt.rows.iter().chain(pt.columns.iter()).position(|a| same(&a.source_col, name)) {
        Some(p) => c.axis_map.get(p).copied().flatten(),
        None => find_page(name),
    };
    filters_xml(&mut s, pt, &filter_field, &vals, extra_filters);
    s.push_str("</pivotTableDefinition>");
    s
}

// ---------------------------------------------------------------- label, value and Top 10 filters

/// The `ST_PivotFilterType` suffix of a comparison (`captionBeginsWith`, `valueBetween`).
fn compare_suffix(op: PivotCompare) -> &'static str {
    match op {
        PivotCompare::Equals => "Equal",
        PivotCompare::NotEquals => "NotEqual",
        PivotCompare::BeginsWith => "BeginsWith",
        PivotCompare::NotBeginsWith => "NotBeginsWith",
        PivotCompare::EndsWith => "EndsWith",
        PivotCompare::NotEndsWith => "NotEndsWith",
        PivotCompare::Contains => "Contains",
        PivotCompare::NotContains => "NotContains",
        PivotCompare::Greater => "GreaterThan",
        PivotCompare::GreaterOrEqual => "GreaterThanOrEqual",
        PivotCompare::Less => "LessThan",
        PivotCompare::LessOrEqual => "LessThanOrEqual",
        PivotCompare::Between => "Between",
        PivotCompare::NotBetween => "NotBetween",
    }
}

/// The AutoFilter `customFilters` that express a comparison.
fn custom_filters(op: PivotCompare, a: &str, b: &str) -> String {
    let one = |oper: Option<&str>, v: &str| match oper {
        Some(o) => format!("<customFilter operator=\"{o}\" val=\"{}\"/>", esc_attr(v)),
        None => format!("<customFilter val=\"{}\"/>", esc_attr(v)),
    };
    let (and, parts) = match op {
        PivotCompare::Equals => (false, vec![one(None, a)]),
        PivotCompare::NotEquals => (false, vec![one(Some("notEqual"), a)]),
        PivotCompare::BeginsWith => (false, vec![one(None, &format!("{a}*"))]),
        PivotCompare::NotBeginsWith => (false, vec![one(Some("notEqual"), &format!("{a}*"))]),
        PivotCompare::EndsWith => (false, vec![one(None, &format!("*{a}"))]),
        PivotCompare::NotEndsWith => (false, vec![one(Some("notEqual"), &format!("*{a}"))]),
        PivotCompare::Contains => (false, vec![one(None, &format!("*{a}*"))]),
        PivotCompare::NotContains => (false, vec![one(Some("notEqual"), &format!("*{a}*"))]),
        PivotCompare::Greater => (false, vec![one(Some("greaterThan"), a)]),
        PivotCompare::GreaterOrEqual => (false, vec![one(Some("greaterThanOrEqual"), a)]),
        PivotCompare::Less => (false, vec![one(Some("lessThan"), a)]),
        PivotCompare::LessOrEqual => (false, vec![one(Some("lessThanOrEqual"), a)]),
        PivotCompare::Between => (true, vec![one(Some("greaterThanOrEqual"), a), one(Some("lessThanOrEqual"), b)]),
        PivotCompare::NotBetween => (false, vec![one(Some("lessThan"), a), one(Some("greaterThan"), b)]),
    };
    format!("<customFilters{}>{}</customFilters>", if and { " and=\"1\"" } else { "" }, parts.concat())
}

/// `<filters>`: active and dormant label, value and Top 10 filters. `fld` is the
/// cache field and `iMeasureFld` the position of the value field in `<dataFields>`.
fn filters_xml(s: &mut String, pt: &PivotTable, field: &dyn Fn(&str) -> Option<usize>, vals: &[(usize, &PivotValue)], extra: &[String]) {
    let measure = |name: &str| vals.iter().position(|(_, v)| same(&v.name, name));
    let mut out = String::new();
    let mut n = 0;
    for f in &pt.filters {
        let Some(rule) = &f.rule else { continue };
        let Some(fld) = field(&f.source_col) else { continue };
        let (ty, attrs, inner) = match rule {
            PivotRule::Label { op, value, value2 } => {
                let mut attrs = format!(" stringValue1=\"{}\"", esc_attr(value));
                if op.is_range() {
                    let _ = write!(attrs, " stringValue2=\"{}\"", esc_attr(value2));
                }
                (format!("caption{}", compare_suffix(*op)), attrs, custom_filters(*op, value, value2))
            }
            PivotRule::Value { op, field, value, value2 } => {
                let Some(m) = measure(field) else { continue };
                let (a, b) = (crate::xml::num(*value), crate::xml::num(*value2));
                (format!("value{}", compare_suffix(*op)), format!(" iMeasureFld=\"{m}\""), custom_filters(*op, &a, &b))
            }
            PivotRule::Top { top, count, by, field } => {
                let Some(m) = measure(field) else { continue };
                let ty = match by {
                    PivotTopBy::Items => "count",
                    PivotTopBy::Percent => "percent",
                    PivotTopBy::Sum => "sum",
                };
                let inner = format!(
                    "<top10 top=\"{}\" percent=\"{}\" val=\"{}\"/>",
                    u8::from(*top),
                    u8::from(*by == PivotTopBy::Percent),
                    crate::xml::num(*count)
                );
                (ty.to_string(), format!(" iMeasureFld=\"{m}\""), inner)
            }
        };
        n += 1;
        let _ = write!(
            out,
            "<filter fld=\"{fld}\" type=\"{ty}\" evalOrder=\"-1\" id=\"{n}\"{attrs}><autoFilter ref=\"A1\"><filterColumn colId=\"0\">{inner}</filterColumn></autoFilter></filter>"
        );
    }
    // Timelines' date filters come after the label, value and Top 10 filters.
    for x in extra.iter().filter(|x| !x.is_empty()) {
        n += 1;
        out.push_str(x);
    }
    if n > 0 {
        let _ = write!(s, "<filters count=\"{n}\">{out}</filters>");
    }
}

/// Reads a `<filter>` into a rule; `measure` gives a value field's caption by its position in
/// `<dataFields>`. `None` for the filter types the model does not have (date filters).
fn read_filter(e: &El, measure: &dyn Fn(u32) -> Option<String>) -> Option<PivotRule> {
    let ty = e.attr("type")?;
    let col = e.child("autoFilter").and_then(|a| a.child("filterColumn"));
    let compare = |suffix: &str| PivotCompare::ALL.into_iter().find(|c| compare_suffix(*c) == suffix);
    if let Some(op) = ty.strip_prefix("caption").and_then(compare) {
        let text = |k: &str| e.attr(k).unwrap_or("").to_string();
        return Some(PivotRule::Label { op, value: text("stringValue1"), value2: text("stringValue2") });
    }
    let field = e.attr_u32("iMeasureFld").and_then(measure)?;
    if let Some(op) = ty.strip_prefix("value").and_then(compare) {
        let nums: Vec<f64> = col
            .and_then(|c| c.child("customFilters"))
            .map(|c| c.kids("customFilter").filter_map(|x| x.attr("val").and_then(|v| v.parse::<f64>().ok())).take(2).collect())
            .unwrap_or_default();
        let n = |i: usize, k: &str| nums.get(i).copied().or_else(|| e.attr(k).and_then(|v| v.parse::<f64>().ok())).filter(|x| x.is_finite());
        return Some(PivotRule::Value { op, field, value: n(0, "stringValue1")?, value2: n(1, "stringValue2").unwrap_or(0.0) });
    }
    let by = match ty {
        "count" => PivotTopBy::Items,
        "percent" => PivotTopBy::Percent,
        "sum" => PivotTopBy::Sum,
        _ => return None,
    };
    let t = col.and_then(|c| c.child("top10"));
    let count = t.and_then(|t| t.attr("val")).and_then(|v| v.parse::<f64>().ok()).filter(|x| x.is_finite()).unwrap_or(10.0);
    let top = t.is_none_or(|t| t.flag("top", true));
    Some(PivotRule::Top { top, count, by, field })
}

/// `<rowFields>`/`<colFields>`: the axis's cache field indices, with Σ Values (`-2`) at `values_at`.
fn axis_fields_xml(s: &mut String, tag: &str, idx: &[usize], values_at: Option<usize>) {
    let n = idx.len() + usize::from(values_at.is_some());
    if n == 0 {
        return;
    }
    let _ = write!(s, "<{tag} count=\"{n}\">");
    for (j, i) in idx.iter().enumerate() {
        if values_at == Some(j) {
            s.push_str("<field x=\"-2\"/>");
        }
        let _ = write!(s, "<field x=\"{i}\"/>");
    }
    if values_at.is_some_and(|p| p >= idx.len()) {
        s.push_str("<field x=\"-2\"/>");
    }
    let _ = write!(s, "</{tag}>");
}

// ================================================================ reading

/// A cache field as read.
#[derive(Clone, Debug, Default)]
struct FieldIn {
    name: String,
    database: bool,
    /// Shared item captions.
    items: Vec<String>,
    /// (base field, grouping, group item captions).
    group: Option<(Option<usize>, GroupBy, Vec<String>)>,
    /// A calculated field's formula.
    formula: Option<String>,
}

/// What a cache field's `fieldGroup` groups by.
#[derive(Clone, Debug)]
enum GroupBy {
    /// A date or time part, with the Grouping dialog's bounds and day span when set.
    Date(PivotDateGroup, Option<PivotDateRange>),
    Range(PivotNumberGroup),
    /// Group Selection: the group item of each of the base field's shared items.
    Discrete(Vec<u32>),
    /// A grouping the model doesn't have (an unusable range).
    Unsupported,
}

impl FieldIn {
    fn labels(&self) -> &[String] {
        match &self.group {
            Some((_, _, items)) if !items.is_empty() => items,
            _ => &self.items,
        }
    }
}

#[derive(Clone, Debug, Default)]
struct CacheIn {
    source: String,
    fields: Vec<FieldIn>,
    /// Calculated items, in solve order.
    calc_items: Vec<PivotCalcItem>,
}

/// Workbook-level pivot cache lookup, filled from `<pivotCaches>`; caches are parsed once.
#[derive(Default)]
pub struct CachesIn {
    by_id: HashMap<u32, String>,
    parsed: HashMap<String, Option<CacheIn>>,
    next_id: u32,
    /// Filters a timeline keeps in a PivotTable: (PivotTable name, filter id). They are the
    /// timeline's selection, read with the timeline.
    pub timeline_filters: Vec<(String, u32)>,
}

impl CachesIn {
    pub fn new(wb_xml: &El, wb_rels: &[crate::package::Rel]) -> CachesIn {
        let mut by_id = HashMap::new();
        if let Some(pc) = wb_xml.child("pivotCaches") {
            for c in pc.kids("pivotCache") {
                if let (Some(id), Some(rid)) = (c.attr_u32("cacheId"), c.attr("id"))
                    && let Some(r) = wb_rels.iter().find(|r| r.id == rid)
                {
                    by_id.insert(id, r.target.clone());
                }
            }
        }
        CachesIn { by_id, parsed: HashMap::new(), next_id: 1, timeline_filters: vec![] }
    }
}

fn item_label_el(e: &El, sys: DateSystem) -> String {
    let v = e.attr("v").unwrap_or("");
    match e.name.as_str() {
        "m" => "(blank)".into(),
        "n" => v.trim().parse::<f64>().ok().filter(|n| n.is_finite()).map(number_to_text).unwrap_or_else(|| v.to_string()),
        "b" => if matches!(v.trim(), "1" | "true") { "TRUE" } else { "FALSE" }.into(),
        "d" => crate::sheet_read::parse_iso_datetime(v, sys).map(number_to_text).unwrap_or_else(|| v.to_string()),
        _ => v.to_string(),
    }
}

fn read_cache(cx: &mut Ctx<'_>, part: &str, sheet_name: &str) -> Option<CacheIn> {
    let x = cx.optional_xml(part).ok().flatten()?;
    if x.name != "pivotCacheDefinition" {
        cx.warn(format!("{part} is not a pivot cache definition; its PivotTable was kept as plain cells"));
        return None;
    }
    let src = x.child("cacheSource");
    let ty = src.and_then(|s| s.attr("type")).unwrap_or("worksheet");
    if ty != "worksheet" {
        cx.warn(format!("PivotTables with a {ty} data source are not supported; kept as plain cells"));
        return None;
    }
    let ws = src.and_then(|s| s.child("worksheetSource"));
    let source = match ws {
        Some(w) if w.attr("name").is_some_and(|n| !n.trim().is_empty()) => w.attr("name").unwrap_or("").trim().to_string(),
        Some(w) => {
            if w.attr("id").is_some() {
                cx.warn("a PivotTable's source is in another workbook; its source reference may not resolve");
            }
            let r = w.attr("ref").map(|r| r.replace('$', "")).and_then(|r| RangeRef::parse(&r));
            let Some(r) = r else {
                cx.warn("a PivotTable's source reference is missing or invalid; kept as plain cells");
                return None;
            };
            let sheet = w.attr("sheet").filter(|s| !s.is_empty()).unwrap_or(sheet_name);
            format!("{}!{}", quote_sheet(sheet), abs_range(&r))
        }
        None => {
            cx.warn("a PivotTable's cache has no worksheet source; kept as plain cells");
            return None;
        }
    };
    let sys = cx.date_system;
    let mut fields = Vec::new();
    if let Some(cf) = x.child("cacheFields") {
        for f in cf.kids("cacheField").take(MAX_FIELDS) {
            let items = f.child("sharedItems").map(|si| si.children.iter().map(|e| item_label_el(e, sys)).collect()).unwrap_or_default();
            // A `fieldGroup` with neither `rangePr` nor `discretePr` only names the field grouped
            // from this one (`par`): this field itself isn't grouped.
            let group = f.child("fieldGroup").filter(|g| g.child("rangePr").is_some() || g.child("discretePr").is_some()).map(|g| {
                let base = g.attr_u32("base").map(|b| b as usize);
                let rp = g.child("rangePr");
                let num = |k: &str| rp.and_then(|r| r.attr(k)).and_then(|v| v.trim().parse::<f64>().ok());
                // Starting at / Ending at set by hand, and Number of days.
                let date = |k: &str| rp.and_then(|r| r.attr(k)).and_then(|v| crate::sheet_read::parse_iso_datetime(v, sys));
                let start = rp.filter(|r| !r.flag("autoStart", true)).and_then(|_| date("startDate"));
                let end = rp.filter(|r| !r.flag("autoEnd", true)).and_then(|_| date("endDate"));
                let range = |level: PivotDateGroup| {
                    let days = if level == PivotDateGroup::Days {
                        num("groupInterval").filter(|d| d.is_finite() && *d >= 1.0).map_or(1, |d| d.min(f64::from(PivotDateRange::MAX_DAYS)) as u32)
                    } else {
                        1
                    };
                    let r = PivotDateRange { start, end, days };
                    GroupBy::Date(level, (start.is_some() || end.is_some() || days > 1).then_some(r))
                };
                let by = if let Some(d) = g.child("discretePr") {
                    GroupBy::Discrete(d.kids("x").take(MAX_ITEMS).map(|x| x.attr_u32("v").unwrap_or(u32::MAX)).collect())
                } else {
                    match rp.map(|r| r.attr("groupBy").unwrap_or("range")) {
                        Some("years") => range(PivotDateGroup::Years),
                        Some("quarters") => range(PivotDateGroup::Quarters),
                        Some("months") => range(PivotDateGroup::Months),
                        Some("days") => range(PivotDateGroup::Days),
                        Some("hours") => range(PivotDateGroup::Hours),
                        Some("minutes") => range(PivotDateGroup::Minutes),
                        Some("seconds") => range(PivotDateGroup::Seconds),
                        Some("range") => match (num("startNum"), num("endNum")) {
                            (Some(a), Some(b)) => {
                                PivotNumberGroup::checked(a, b, num("groupInterval").unwrap_or(1.0)).map_or(GroupBy::Unsupported, GroupBy::Range)
                            }
                            _ => GroupBy::Unsupported,
                        },
                        _ => GroupBy::Unsupported,
                    }
                };
                let gi = g.child("groupItems").map(|gi| gi.children.iter().map(|e| item_label_el(e, sys)).collect()).unwrap_or_default();
                (base, by, gi)
            });
            fields.push(FieldIn {
                name: f.attr("name").unwrap_or("").to_string(),
                database: f.flag("databaseField", true),
                items,
                group,
                formula: f.attr("formula").map(str::to_string),
            });
        }
    }
    if fields.is_empty() {
        cx.warn("a PivotTable's cache has no fields; kept as plain cells");
        return None;
    }
    // Calculated items: a formula and the shared item of its field it fills.
    let mut calc_items = Vec::new();
    for c in x.child("calculatedItems").into_iter().flat_map(|c| c.kids("calculatedItem")).take(MAX_FIELDS) {
        let Some(formula) = c.attr("formula").map(str::trim).filter(|f| !f.is_empty()) else { continue };
        let reference = c.path(&["pivotArea", "references", "reference"]);
        let fi = c.attr_u32("field").or_else(|| reference.and_then(|r| r.attr_u32("field"))).map(|f| f as usize);
        let item = reference.and_then(|r| r.child("x")).and_then(|x| x.attr_u32("v")).map(|v| v as usize);
        let (Some(f), Some(item)) = (fi.and_then(|i| fields.get(i)), item) else {
            cx.warn("a PivotTable calculated item without its field or item was skipped");
            continue;
        };
        let Some(name) = f.items.get(item).filter(|n| !n.trim().is_empty()) else { continue };
        let formula = if formula.starts_with('=') { formula.to_string() } else { format!("={formula}") };
        calc_items.push(PivotCalcItem { field: f.name.clone(), name: name.clone(), formula });
    }
    Some(CacheIn { source, fields, calc_items })
}

/// Reads the PivotTables related to a worksheet part.
pub fn read_pivots(cx: &mut Ctx<'_>, caches: &mut CachesIn, sheet_part: &str, sheet_name: &str) -> Result<Vec<PivotTable>, IoError> {
    let rels = cx.pkg.rels(sheet_part)?;
    let mut out = Vec::new();
    for rel in rels.iter().filter(|r| r.kind == "pivotTable" && !r.external) {
        let Some(x) = cx.optional_xml(&rel.target)? else { continue };
        if x.name != "pivotTableDefinition" {
            cx.warn(format!("{} is not a PivotTable definition; skipped", rel.target));
            continue;
        }
        // The cache: the table's own relationship, else the workbook's cache id.
        let trels = cx.pkg.rels(&rel.target)?;
        let cache_part = trels
            .iter()
            .find(|r| r.kind == "pivotCacheDefinition" && !r.external)
            .map(|r| r.target.clone())
            .or_else(|| x.attr_u32("cacheId").and_then(|id| caches.by_id.get(&id).cloned()));
        let Some(cache_part) = cache_part else {
            cx.warn(format!("PivotTable in {} has no cache; kept as plain cells", rel.target));
            continue;
        };
        let cache = match caches.parsed.get(&cache_part) {
            Some(c) => c.clone(),
            None => {
                let c = read_cache(cx, &cache_part, sheet_name);
                caches.parsed.insert(cache_part.clone(), c.clone());
                c
            }
        };
        let Some(cache) = cache else { continue };
        if let Some(pt) = read_table(cx, &x, &cache, caches.next_id, &caches.timeline_filters) {
            caches.next_id = caches.next_id.saturating_add(1);
            out.push(pt);
        }
    }
    Ok(out)
}

/// A pivot field's data items (not subtotal or grand-total items).
fn items_of(e: &El) -> Vec<&El> {
    e.child("items").map(|it| it.kids("item").filter(|i| i.attr("t").is_none_or(|t| t == "data")).collect()).unwrap_or_default()
}

/// A cache field as the model names it in an area.
struct FieldRef {
    /// The source field, or the Group Selection field's own name.
    name: String,
    date: PivotDateGroup,
    number: Option<PivotNumberGroup>,
    range: Option<PivotDateRange>,
    /// The Group Selection field this cache field is.
    manual: Option<PivotGroupField>,
}

/// The groups of a Group Selection cache field: each group item that gathers base items other
/// than a single item of its own caption.
fn manual_groups(cache: &CacheIn, f: &FieldIn, base: usize, map: &[u32], items: &[String]) -> Option<PivotGroupField> {
    let base_f = cache.fields.get(base)?;
    let base_labels = base_f.labels();
    let mut groups: Vec<PivotItemGroup> = Vec::new();
    for (k, caption) in items.iter().enumerate() {
        let members: Vec<String> = base_labels.iter().zip(map).filter(|(_, g)| **g as usize == k).map(|(l, _)| l.clone()).collect();
        let own = members.len() == 1 && members.first().is_some_and(|m| same(m, caption));
        if !members.is_empty() && !own {
            groups.push(PivotItemGroup { name: caption.clone(), items: members });
        }
    }
    Some(PivotGroupField { name: f.name.clone(), base: base_f.name.clone(), groups })
}

/// How the model names cache field `i` used in an area.
fn field_ref(cx: &mut Ctx<'_>, cache: &CacheIn, i: usize) -> Option<FieldRef> {
    let f = cache.fields.get(i)?;
    if f.formula.is_some() {
        cx.warn("calculated PivotTable fields are not supported");
        return None;
    }
    let plain = |name: String| FieldRef { name, date: PivotDateGroup::None, number: None, range: None, manual: None };
    match &f.group {
        Some((Some(base), GroupBy::Discrete(map), items)) if !f.database => {
            let Some(manual) = manual_groups(cache, f, *base, map, items) else {
                cx.warn(format!("PivotTable field \"{}\" (grouped items) has no base field and was skipped", f.name));
                return None;
            };
            Some(FieldRef { manual: Some(manual), ..plain(f.name.clone()) })
        }
        Some((_, GroupBy::Discrete(_), _)) => {
            cx.warn(format!("PivotTable field \"{}\" (grouped items) is not supported", f.name));
            None
        }
        Some((base, by, _)) => {
            let base_name = if f.database { f.name.clone() } else { base.and_then(|b| cache.fields.get(b))?.name.clone() };
            Some(match by {
                GroupBy::Date(d, range) => FieldRef { date: *d, range: *range, ..plain(base_name) },
                GroupBy::Range(g) => FieldRef { number: Some(*g), ..plain(base_name) },
                _ => {
                    cx.warn(format!("PivotTable grouping of \"{}\" is not supported; the field is ungrouped", f.name));
                    plain(base_name)
                }
            })
        }
        None if !f.database => {
            cx.warn(format!("PivotTable field \"{}\" (grouped items) is not supported", f.name));
            None
        }
        None => Some(plain(f.name.clone())),
    }
}

fn read_table(cx: &mut Ctx<'_>, x: &El, cache: &CacheIn, id: u32, timeline_filters: &[(String, u32)]) -> Option<PivotTable> {
    let mut pt = PivotTable { id, ..PivotTable::default() };
    if let Some(n) = x.attr("name").filter(|n| !n.trim().is_empty()) {
        pt.name = n.to_string();
    } else {
        pt.name = format!("PivotTable{id}");
    }
    pt.source = cache.source.clone();
    let Some(loc) = x.child("location").and_then(|l| l.attr("ref")).and_then(|r| RangeRef::parse(&r.replace('$', ""))) else {
        cx.warn(format!("PivotTable \"{}\" has no valid location; kept as plain cells", pt.name));
        return None;
    };
    let pfs: Vec<&El> = x.child("pivotFields").map(|p| p.kids("pivotField").collect()).unwrap_or_default();
    let pf = |i: usize| pfs.get(i).copied();
    let idx_list = |name: &str, attr: &str| -> Vec<i64> {
        x.child(name).map(|e| e.children.iter().filter_map(|f| f.attr_i64(attr)).collect()).unwrap_or_default()
    };
    let row_x = idx_list("rowFields", "x");
    let col_x = idx_list("colFields", "x");
    let pages: Vec<&El> = x.child("pageFields").map(|p| p.kids("pageField").collect()).unwrap_or_default();

    // Layout from the first axis field (pivotField defaults: compact and outline on).
    let first_axis = row_x.iter().chain(col_x.iter()).find(|i| **i >= 0).and_then(|i| pf(*i as usize));
    let (compact, outline) = match first_axis {
        Some(f) => (f.flag("compact", true), f.flag("outline", true)),
        None => (x.flag("compact", true), x.flag("outline", false)),
    };
    pt.layout = if compact {
        PivotLayout::Compact
    } else if outline {
        PivotLayout::Outline
    } else {
        PivotLayout::Tabular
    };
    if let Some(f) = first_axis {
        pt.subtotals = f.flag("defaultSubtotal", true);
        pt.subtotals_top = f.flag("subtotalTop", true);
    }
    pt.grand_totals_rows = x.flag("rowGrandTotals", true);
    pt.grand_totals_cols = x.flag("colGrandTotals", true);
    pt.show_headers = x.flag("showHeaders", true);
    pt.style = x.child("pivotTableStyleInfo").and_then(|s| s.attr("name")).unwrap_or("").to_string();

    let mut filters: Vec<PivotFilter> = Vec::new();
    // Group Selection fields met in the areas.
    let mut manual: Vec<PivotGroupField> = Vec::new();
    let keep_manual = |manual: &mut Vec<PivotGroupField>, r: &FieldRef| {
        if let Some(m) = &r.manual
            && !manual.iter().any(|x| same(&x.name, &m.name))
        {
            manual.push(m.clone());
        }
    };

    // (fields, position of Σ Values (`-2`) among them)
    let axis =
        |cx: &mut Ctx<'_>, list: &[i64], filters: &mut Vec<PivotFilter>, manual: &mut Vec<PivotGroupField>| -> (Vec<PivotField>, Option<usize>) {
            let mut v = Vec::new();
            let mut values_at = None;
            for &i in list {
                if i == -2 {
                    values_at = Some(v.len());
                    continue;
                }
                if i < 0 {
                    continue;
                }
                let i = i as usize;
                let Some(r) = field_ref(cx, cache, i) else { continue };
                keep_manual(manual, &r);
                let name = r.name.clone();
                let labels = cache.fields.get(i).map(|f| f.labels().to_vec()).unwrap_or_default();
                let mut f = PivotField {
                    source_col: name.clone(),
                    date_group: r.date,
                    number_group: r.number,
                    date_range: r.range,
                    sort: PivotSort::None,
                    ..PivotField::default()
                };
                if let Some(e) = pf(i) {
                    f.sort = match e.attr("sortType") {
                        Some("ascending") => PivotSort::Asc,
                        Some("descending") => PivotSort::Desc,
                        // Date groups in their own (chronological) order are ascending.
                        _ if f.date_group != PivotDateGroup::None => PivotSort::Asc,
                        _ => PivotSort::None,
                    };
                    let items = items_of(e);
                    let lab = |it: &El| it.attr_u32("x").and_then(|k| labels.get(k as usize)).cloned();
                    f.collapsed_items = items.iter().filter(|it| !it.flag("sd", true)).filter_map(|it| lab(it)).collect();
                    if items.iter().any(|it| it.flag("h", false)) {
                        let shown: Vec<String> = items.iter().filter(|it| !it.flag("h", false)).filter_map(|it| lab(it)).collect();
                        if !filters.iter().any(|ff| same(&ff.source_col, &name)) {
                            filters.push(PivotFilter { source_col: name.clone(), selected: Some(shown), rule: None });
                        }
                    }
                }
                v.push(f);
            }
            (v, values_at)
        };
    let mut page_filters = Vec::new();
    for p in &pages {
        let Some(i) = p.attr_i64("fld").filter(|i| *i >= 0).map(|i| i as usize) else { continue };
        let Some(r) = field_ref(cx, cache, i) else { continue };
        keep_manual(&mut manual, &r);
        let name = r.name;
        let labels = cache.fields.get(i).map(|f| f.labels().to_vec()).unwrap_or_default();
        let selected = if let Some(k) = p.attr_u32("item") {
            labels.get(k as usize).map(|l| vec![l.clone()])
        } else {
            pf(i).and_then(|e| {
                let items = items_of(e);
                items.iter().any(|it| it.flag("h", false)).then(|| {
                    items
                        .iter()
                        .filter(|it| !it.flag("h", false))
                        .filter_map(|it| it.attr_u32("x").and_then(|k| labels.get(k as usize)).cloned())
                        .collect()
                })
            })
        };
        page_filters.push(PivotFilter { source_col: name, selected, rule: None });
    }
    let mut axis_filters = Vec::new();
    let (rows, rows_values) = axis(cx, &row_x, &mut axis_filters, &mut manual);
    let (columns, cols_values) = axis(cx, &col_x, &mut axis_filters, &mut manual);
    pt.rows = rows;
    pt.columns = columns;
    // Σ Values: where its `-2` entry is, else `dataOnRows` and `dataPosition`; after the last
    // field by default. In Columns it always comes last, as the column layout puts it there.
    pt.values_on_rows = rows_values.is_some() || (cols_values.is_none() && x.flag("dataOnRows", false));
    pt.values_position =
        if pt.values_on_rows { rows_values.or_else(|| x.attr_u32("dataPosition").map(|p| p as usize)).filter(|p| *p < pt.rows.len()) } else { None };
    filters.extend(page_filters);
    filters.extend(axis_filters);
    pt.filters = filters;

    // Values.
    if let Some(df) = x.child("dataFields") {
        for d in df.kids("dataField") {
            let Some(i) = d.attr_u32("fld").map(|i| i as usize) else { continue };
            let Some(f) = cache.fields.get(i) else { continue };
            let func = match d.attr("subtotal") {
                None => PivotFunc::Sum,
                Some(s) => func_from(s).unwrap_or_else(|| {
                    cx.warn(format!("PivotTable function {s} is not supported; using Sum"));
                    PivotFunc::Sum
                }),
            };
            let ext = d
                .child("extLst")
                .into_iter()
                .flat_map(|l| l.kids("ext"))
                .filter_map(|e| e.child("dataField"))
                .find_map(|e| e.attr("pivotShowAs").map(str::to_string));
            let show_as = match (ext.as_deref(), d.attr("showDataAs")) {
                (Some("rankDescending"), _) => PivotShowAs::Rank,
                (Some("rankAscending"), _) => PivotShowAs::RankAscending,
                (Some("percentOfParentRow"), _) => PivotShowAs::PercentOfParentRowTotal,
                (Some("percentOfParentCol"), _) => PivotShowAs::PercentOfParentColumnTotal,
                (Some("percentOfParent"), _) => PivotShowAs::PercentOfParentTotal,
                (Some("percentOfRunningTotal"), _) => PivotShowAs::PercentRunningTotal,
                (Some(other), _) => {
                    cx.warn(format!("PivotTable show-values-as {other} is not supported"));
                    PivotShowAs::Normal
                }
                (None, None | Some("normal")) => PivotShowAs::Normal,
                (None, Some("percentOfTotal")) => PivotShowAs::PercentOfGrandTotal,
                (None, Some("percentOfCol")) => PivotShowAs::PercentOfColumnTotal,
                (None, Some("percentOfRow")) => PivotShowAs::PercentOfRowTotal,
                (None, Some("percent")) => PivotShowAs::PercentOf,
                (None, Some("difference")) => PivotShowAs::DifferenceFrom,
                (None, Some("percentDiff")) => PivotShowAs::PercentDifferenceFrom,
                (None, Some("runTotal")) => PivotShowAs::RunningTotal,
                (None, Some("index")) => PivotShowAs::Index,
                (None, Some(other)) => {
                    cx.warn(format!("PivotTable show-values-as {other} is not supported"));
                    PivotShowAs::Normal
                }
            };
            // Base field and item, where the transform uses them.
            let (mut base_field, mut base_item) = (None, None);
            if show_as.needs_base_field()
                && let Some(bf) = d.attr_u32("baseField").map(|b| b as usize)
                && let Some(r) = field_ref(cx, cache, bf)
            {
                base_field = Some(r.name);
                if show_as.needs_base_item() {
                    base_item = match d.attr_u32("baseItem") {
                        Some(1_048_828) => Some(BASE_PREVIOUS.to_string()),
                        Some(1_048_829) => Some(BASE_NEXT.to_string()),
                        Some(k) => cache.fields.get(bf).and_then(|f| f.labels().get(k as usize).cloned()),
                        None => None,
                    };
                }
            }
            let number_format = d
                .attr_u32("numFmtId")
                .filter(|id| *id != 0)
                .and_then(|id| crate::tables::builtin_format(id).map(str::to_string).or_else(|| cx.num_fmts.get(&id).cloned()));
            let name = d.attr("name").map(str::to_string).unwrap_or_else(|| format!("{} of {}", func_caption(func), f.name));
            pt.values.push(PivotValue { source_col: f.name.clone(), func, name, show_as, number_format, base_field, base_item });
        }
    }

    // Calculated fields of the cache (each PivotTable on it can use them).
    for f in &cache.fields {
        if let Some(formula) = &f.formula
            && !f.name.trim().is_empty()
            && !pt.calculated_fields.iter().any(|c| same(&c.name, &f.name))
        {
            let formula = formula.trim();
            let formula = if formula.starts_with('=') { formula.to_string() } else { format!("={formula}") };
            pt.calculated_fields.push(PivotCalcField { name: f.name.clone(), formula });
        }
    }

    // Keep rules even when their field is not currently on an axis: Excel reactivates them
    // when the field returns to Rows or Columns.
    if let Some(fs) = x.child("filters") {
        let names: Vec<String> = pt.values.iter().map(|v| v.name.clone()).collect();
        let measure = |i: u32| names.get(i as usize).cloned();
        for e in fs.kids("filter").take(MAX_FIELDS) {
            // A timeline's date filter is its selection, read with the timeline.
            if e.attr_u32("id").is_some_and(|id| timeline_filters.iter().any(|(p, f)| *f == id && same(p, &pt.name))) {
                continue;
            }
            let Some(r) = e.attr_u32("fld").and_then(|i| field_ref(cx, cache, i as usize)) else { continue };
            let Some(rule) = read_filter(e, &measure) else {
                cx.warn(format!("PivotTable \"{}\": filter type {} is not supported", pt.name, e.attr("type").unwrap_or("")));
                continue;
            };
            keep_manual(&mut manual, &r);
            let name = r.name;
            // One filter per field: the rule replaces any hidden items.
            pt.filters.retain(|f| !same(&f.source_col, &name));
            pt.filters.push(PivotFilter { source_col: name, selected: None, rule: Some(rule) });
        }
    }

    pt.group_fields = manual;
    pt.calculated_items = cache.calc_items.clone();
    pt.anchor = loc.start;
    let above = if pages.is_empty() { 0 } else { pages.len() as u32 + 1 };
    pt.last_range = Some(RangeRef::new(CellRef::new(loc.start.row.saturating_sub(above), loc.start.col), loc.end));
    Some(pt)
}
