//! Timelines (Insert › Filters › Timeline, PivotTable Analyze › Filter › Insert Timeline and the
//! Timeline contextual tab): floating filters of a PivotTable's date field by one contiguous
//! period at a time level (years, quarters, months or days).
//!
//! A timeline keeps its selection as a first and last day; the PivotTables it is connected to
//! (Report Connections) keep only the records whose date falls in it (see `pivot::compute`). As
//! in Excel, a selection is made of whole periods of the level it was made at, a new timeline
//! shows months and selects all periods, and each PivotTable has one timeline per date field.

use gridcraft_core::date::{datetime_from_serial, serial_from_ymd};
use gridcraft_core::{CellRef, DateSystem, Value};
use gridcraft_model::*;
use serde_json::{Value as Json, json};

use super::pivot::{find_pivot, other};
use super::*;
use crate::pivot as pv;

pub fn specs() -> Vec<CommandSpec> {
    vec![
        cmd!(
            "insert.timeline",
            "Timeline",
            ["Insert", "Filters"],
            None,
            "{pivot?, field?: date field | fields?: [..], at?: \"H2\", width?, height?, replace?} adds a timeline filtering the PivotTable by the date field; without `field` opens Insert Timelines (the PivotTable's date fields) → {timelines: [{id, name}]}",
            has_doc,
            insert_timeline
        ),
        cmd!(
            "pivot.insertTimeline",
            "Insert Timeline",
            ["PivotTable Analyze", "Filter"],
            None,
            "{pivot?, field?: date field | fields?: [..], at?, width?, height?, replace?} (same as insert.timeline)",
            has_doc,
            insert_timeline
        ),
        cmd!(
            "timeline.select",
            "Select Timeline Period",
            [],
            None,
            "{timeline?: id or name (default: the last on the active sheet), start: date, end?: date (default start), level?: years|quarters|months|days (default the timeline's), replace?} selects whole periods from start's to end's and filters the connected PivotTables → {selection, label}",
            has_doc,
            select
        ),
        cmd!("timeline.clear", "Clear Filter", [], Some("Alt+C"), "{timeline?, replace?} selects all periods (no filter)", has_doc, clear),
        cmd!(
            "timeline.level",
            "Time Level",
            [],
            None,
            "{timeline?, level: years|quarters|months|days} sets the level the timeline shows",
            has_doc,
            level
        ),
        cmd!("timeline.scroll", "Scroll Timeline", [], None, "{timeline?, to: date | null} sets the first date the timeline shows", has_doc, scroll),
        cmd!("timeline.caption", "Timeline Caption", ["Timeline", "Timeline"], None, "{timeline?, caption}", has_doc, caption),
        cmd!(
            "timeline.reportConnections",
            "Report Connections",
            ["Timeline", "Timeline"],
            None,
            "{timeline?, pivots?: [PivotTable names on the same source], replace?}; without `pivots` opens the Report Connections dialog",
            has_doc,
            report_connections
        ),
        cmd!(
            "timeline.style",
            "Timeline Styles",
            ["Timeline", "Timeline Styles"],
            None,
            "{timeline?, style: TimeSlicerStyleLight1…6 | TimeSlicerStyleDark1…6}",
            has_doc,
            style
        ),
        cmd!("timeline.size", "Size", ["Timeline", "Size"], None, "{timeline?, width?, height?} in points", has_doc, size),
        cmd!("timeline.showHeader", "Header", ["Timeline", "Show"], None, "{timeline?, on?: bool (default: toggle)}", has_doc, |s, p| show(
            s,
            p,
            Show::Header
        )),
        cmd!(
            "timeline.showSelectionLabel",
            "Selection Label",
            ["Timeline", "Show"],
            None,
            "{timeline?, on?: bool (default: toggle)}",
            has_doc,
            |s, p| show(s, p, Show::SelectionLabel)
        ),
        cmd!("timeline.showScrollbar", "Scrollbar", ["Timeline", "Show"], None, "{timeline?, on?: bool (default: toggle)}", has_doc, |s, p| show(
            s,
            p,
            Show::Scrollbar
        )),
        cmd!("timeline.showTimeLevel", "Time Level", ["Timeline", "Show"], None, "{timeline?, on?: bool (default: toggle)}", has_doc, |s, p| show(
            s,
            p,
            Show::TimeLevel
        )),
        cmd!("timeline.delete", "Delete Timeline", [], None, "{timeline?, replace?} removes the timeline and its filter", has_doc, delete),
        cmd!(
            query "timeline.periods",
            "Timeline Periods",
            [],
            None,
            "{timeline?} → {id, level, selection, label, periods: [{start, end, label, group, selected}]}",
            has_doc,
            periods_query
        ),
    ]
}

/// The timeline styles Excel offers.
pub const STYLES: [&str; 12] = [
    "TimeSlicerStyleLight1",
    "TimeSlicerStyleLight2",
    "TimeSlicerStyleLight3",
    "TimeSlicerStyleLight4",
    "TimeSlicerStyleLight5",
    "TimeSlicerStyleLight6",
    "TimeSlicerStyleDark1",
    "TimeSlicerStyleDark2",
    "TimeSlicerStyleDark3",
    "TimeSlicerStyleDark4",
    "TimeSlicerStyleDark5",
    "TimeSlicerStyleDark6",
];

/// Most periods a timeline lists (days over decades are capped).
const MAX_PERIODS: usize = 20_000;

const MONTHS: [&str; 12] = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];

pub fn level_name(l: TimelineLevel) -> &'static str {
    match l {
        TimelineLevel::Years => "years",
        TimelineLevel::Quarters => "quarters",
        TimelineLevel::Months => "months",
        TimelineLevel::Days => "days",
    }
}

fn level_param(cmd: &str, v: &str) -> Result<TimelineLevel> {
    Ok(match v.trim().to_ascii_lowercase().as_str() {
        "years" | "year" => TimelineLevel::Years,
        "quarters" | "quarter" => TimelineLevel::Quarters,
        "months" | "month" => TimelineLevel::Months,
        "days" | "day" => TimelineLevel::Days,
        other => return Err(bad(cmd, format!("unknown level `{other}` (years|quarters|months|days)"))),
    })
}

// ---------------------------------------------------------------- periods

/// A day serial as (year, month, day).
fn ymd(sys: DateSystem, day: f64) -> Option<(i32, u32, u32)> {
    let d = datetime_from_serial(sys, day)?;
    Some((d.year, d.month, d.day))
}

fn serial(sys: DateSystem, y: i32, m: i64, d: i64) -> Option<f64> {
    serial_from_ymd(sys, i64::from(y), m, d)
}

/// The first and last day of the period at `level` holding `day`.
pub fn period_of(sys: DateSystem, level: TimelineLevel, day: f64) -> Option<(f64, f64)> {
    let day = day.floor();
    let (y, m, _) = ymd(sys, day)?;
    let (first, months) = match level {
        TimelineLevel::Days => return Some((day, day)),
        TimelineLevel::Years => (1, 12),
        TimelineLevel::Quarters => ((m - 1) / 3 * 3 + 1, 3),
        TimelineLevel::Months => (m, 1),
    };
    let a = serial(sys, y, i64::from(first), 1)?;
    let b = serial(sys, y, i64::from(first) + months, 0)?;
    Some((a, b))
}

/// The dates of the timeline's field in its first connected PivotTable's source: (first, last day).
pub fn data_bounds(wb: &Workbook, tl: &Timeline) -> Option<(f64, f64)> {
    let (si, pi) = connected(wb, tl).into_iter().next()?;
    let pt = wb.sheet(si)?.pivots.get(pi)?;
    let src = pv::read_source(wb, &pt.source, si).ok()?;
    let c = src.col(&tl.field)?;
    let (lo, hi) = src
        .rows
        .iter()
        .filter_map(|r| match r.get(c) {
            Some(Value::Number(n)) => Some(n.floor()),
            _ => None,
        })
        .fold((f64::INFINITY, f64::NEG_INFINITY), |(lo, hi), n| (lo.min(n), hi.max(n)));
    (lo.is_finite() && hi.is_finite()).then_some((lo, hi))
}

/// One period a timeline shows.
#[derive(Clone, Debug, PartialEq)]
pub struct Period {
    pub start: f64,
    pub end: f64,
    /// The tile caption (`2026`, `Q1`, `JAN`, `15`).
    pub label: String,
    /// The caption of the row above (`2026`, `JAN 2026`; empty for years).
    pub group: String,
}

/// The periods of a timeline at its level, over whole years around its data.
pub fn periods(wb: &Workbook, tl: &Timeline) -> Vec<Period> {
    let sys = wb.date_system;
    let Some((lo, hi)) = data_bounds(wb, tl) else { return vec![] };
    let (Some((y0, _, _)), Some((y1, _, _))) = (ymd(sys, lo), ymd(sys, hi)) else { return vec![] };
    let mut out = Vec::new();
    let Some(mut day) = serial(sys, y0, 1, 1) else { return vec![] };
    let Some(last) = serial(sys, y1, 12, 31) else { return vec![] };
    while day <= last && out.len() < MAX_PERIODS {
        let Some((a, b)) = period_of(sys, tl.level, day) else { break };
        let Some((y, m, d)) = ymd(sys, a) else { break };
        let mon = MONTHS.get(m.saturating_sub(1) as usize).copied().unwrap_or("Jan").to_uppercase();
        let (label, group) = match tl.level {
            TimelineLevel::Years => (y.to_string(), String::new()),
            TimelineLevel::Quarters => (format!("Q{}", (m - 1) / 3 + 1), y.to_string()),
            TimelineLevel::Months => (mon, y.to_string()),
            TimelineLevel::Days => (d.to_string(), format!("{mon} {y}")),
        };
        out.push(Period { start: a, end: b, label, group });
        day = b + 1.0;
    }
    out
}

/// The selection as the timeline's label shows it: `All Periods`, `2026`, `Q1 - Q2 2026`,
/// `Mar - May 2026`, `Nov 2025 - Feb 2026`, `3/1/2026 - 3/7/2026`.
pub fn selection_label(wb: &Workbook, tl: &Timeline) -> String {
    let sys = wb.date_system;
    let Some((a, b)) = tl.selection else { return "All Periods".into() };
    let (Some((ya, ma, da)), Some((yb, mb, db))) = (ymd(sys, a), ymd(sys, b)) else { return String::new() };
    let mon = |m: u32| MONTHS.get(m.saturating_sub(1) as usize).copied().unwrap_or("Jan");
    let q = |m: u32| (m - 1) / 3 + 1;
    let (one, from, to) = match tl.selection_level {
        TimelineLevel::Years => return if ya == yb { ya.to_string() } else { format!("{ya} - {yb}") },
        TimelineLevel::Quarters => (q(ma) == q(mb) && ya == yb, format!("Q{}", q(ma)), format!("Q{}", q(mb))),
        TimelineLevel::Months => (ma == mb && ya == yb, mon(ma).to_string(), mon(mb).to_string()),
        TimelineLevel::Days => {
            return if a == b { format!("{ma}/{da}/{ya}") } else { format!("{ma}/{da}/{ya} - {mb}/{db}/{yb}") };
        }
    };
    if one {
        format!("{from} {ya}")
    } else if ya == yb {
        format!("{from} - {to} {ya}")
    } else {
        format!("{from} {ya} - {to} {yb}")
    }
}

// ---------------------------------------------------------------- lookup

/// (sheet, index) of each PivotTable a timeline is connected to.
pub fn connected(wb: &Workbook, tl: &Timeline) -> Vec<(usize, usize)> {
    let mut out = Vec::new();
    for (si, sh) in wb.sheets.iter().enumerate() {
        for (pi, pt) in sh.pivots.iter().enumerate() {
            if tl.pivots.contains(&pt.id) {
                out.push((si, pi));
            }
        }
    }
    out
}

/// (sheet, index) of the timeline a call names (`timeline`: id or name), else the last one on
/// the active sheet.
fn find_timeline(s: &Session, p: &Json) -> Result<(usize, usize)> {
    let d = s.doc()?;
    match p.get("timeline").filter(|v| !v.is_null()) {
        Some(v) => {
            for (si, sh) in d.wb.sheets.iter().enumerate() {
                let hit = sh.timelines.iter().position(|t| match v {
                    Json::Number(n) => n.as_u64() == Some(u64::from(t.id)),
                    Json::String(n) => t.name.eq_ignore_ascii_case(n.trim()),
                    _ => false,
                });
                if let Some(i) = hit {
                    return Ok((si, i));
                }
            }
            Err(bad("timeline", format!("no timeline `{}`", v.as_str().map(str::to_string).unwrap_or_else(|| v.to_string()))))
        }
        None => {
            let si = d.wb.active_sheet;
            let n = d.wb.active().map_or(0, |sh| sh.timelines.len());
            n.checked_sub(1).map(|i| (si, i)).ok_or_else(|| EngineError::Disabled("timeline".into(), "there's no timeline on this sheet".into()))
        }
    }
}

/// Applies `f` to a timeline and refreshes the PivotTables it filters (before and after), as one
/// undo step.
fn update_timeline(s: &mut Session, p: &Json, refresh: bool, f: impl FnOnce(&mut Timeline, &Workbook) -> Result<Json>) -> Result<Json> {
    let (si, ti) = find_timeline(s, p)?;
    let replace = bool_param(p, "replace").unwrap_or(false);
    edit(s, |cx| {
        let mut tl = cx.wb.sheet(si).and_then(|sh| sh.timelines.get(ti)).cloned().ok_or_else(|| other("There's no such timeline."))?;
        let before = connected(&cx.wb, &tl);
        let out = f(&mut tl, &cx.wb)?;
        if let Some(slot) = cx.sheet_mut(si)?.timelines.get_mut(ti) {
            *slot = tl.clone();
        }
        if refresh {
            let mut targets = before;
            for t in connected(&cx.wb, &tl) {
                if !targets.contains(&t) {
                    targets.push(t);
                }
            }
            for (psi, ppi) in targets {
                pv::refresh(&mut cx.wb, psi, ppi, replace).map_err(other)?;
            }
            cx.structural = true;
        }
        Ok(out)
    })
}

// ---------------------------------------------------------------- commands

/// Date fields of a PivotTable's source, in source order.
fn date_fields(src: &pv::Source) -> Vec<String> {
    src.headers.iter().enumerate().filter(|(c, _)| src.dates.get(*c).copied().unwrap_or(false)).map(|(_, h)| h.clone()).collect()
}

fn insert_timeline(s: &mut Session, p: &Json) -> Result<Json> {
    const C: &str = "insert.timeline";
    let (si, pi) = find_pivot(s, p)?;
    let (pt, src) = {
        let d = s.doc()?;
        let pt = d.wb.sheet(si).and_then(|sh| sh.pivots.get(pi)).cloned().ok_or_else(|| other("There's no such PivotTable."))?;
        let src = pv::read_source(&d.wb, &pt.source, si).map_err(other)?;
        (pt, src)
    };
    let mut fields: Vec<String> = match (str_param(p, "field"), p.get("fields").and_then(Json::as_array)) {
        (Some(f), _) => vec![f.trim().to_string()],
        (None, Some(a)) => a.iter().take(64).filter_map(|x| x.as_str().map(|t| t.trim().to_string())).collect(),
        (None, None) => {
            // Menu-style call: Insert Timelines lists the PivotTable's date fields.
            let dates = date_fields(&src);
            if dates.is_empty() {
                return Err(other("The PivotTable has no fields with dates. A timeline needs a field formatted as dates."));
            }
            s.ui_requests.push(crate::UiRequest::Dialog("insertTimeline".into(), json!({"pivot": pt.name, "fields": dates})));
            return ok();
        }
    };
    fields.dedup();
    if fields.is_empty() {
        return Err(bad(C, "missing `field`"));
    }
    let sheet = s.doc()?.wb.active_sheet;
    let at = cell_param(s, p, "at")?;
    let width = f64_param(p, "width").unwrap_or(262.5).clamp(60.0, 10_000.0) as f32;
    let height = f64_param(p, "height").unwrap_or(108.0).clamp(40.0, 10_000.0) as f32;
    edit(s, |cx| {
        let mut made = Vec::new();
        for (k, field) in fields.iter().enumerate() {
            let c = src.col(field).ok_or_else(|| bad(C, format!("no field `{field}` in the source data")))?;
            if !src.dates.get(c).copied().unwrap_or(false) {
                return Err(other(format!("\"{field}\" doesn't contain dates. A timeline can only filter a date field.")));
            }
            let field = src.headers.get(c).cloned().unwrap_or_default();
            let taken =
                cx.wb.sheets.iter().flat_map(|sh| sh.timelines.iter()).any(|t| t.field.eq_ignore_ascii_case(&field) && t.pivots.contains(&pt.id));
            if taken {
                return Err(other(format!("A timeline already filters this PivotTable by \"{field}\".")));
            }
            let names: Vec<String> = cx.wb.sheets.iter().flat_map(|sh| sh.timelines.iter().map(|t| t.name.to_lowercase())).collect();
            let name = std::iter::once(field.clone())
                .chain((1..10_000).map(|n| format!("{field} {n}")))
                .find(|n| !names.contains(&n.to_lowercase()))
                .unwrap_or_else(|| field.clone());
            // Beside the report, like Excel: right of it, one below another.
            let end = pt.last_range.map_or(pt.anchor, |r| r.end);
            let start_row = pt.last_range.map_or(pt.anchor.row, |r| r.start.row);
            let cell = at.unwrap_or_else(|| {
                CellRef::new(
                    start_row.saturating_add(8 * k as u32).min(gridcraft_core::MAX_ROWS - 1),
                    end.col.saturating_add(2).min(gridcraft_core::MAX_COLS - 1),
                )
            });
            let id = cx.wb.next_object_id();
            let tl = Timeline {
                id,
                name: name.clone(),
                caption: field.clone(),
                field,
                pivots: vec![pt.id],
                anchor: Anchor { cell, dx: 4.0, dy: 4.0, width, height, mode: AnchorMode::MoveOnly },
                ..Timeline::default()
            };
            cx.sheet_mut(sheet)?.timelines.push(tl);
            made.push(json!({"id": id, "name": name}));
        }
        Ok(json!({"timelines": made}))
    })
}

/// A date parameter: a serial or date text.
fn date_param(cmd: &str, p: &Json, key: &str, sys: DateSystem) -> Result<Option<f64>> {
    match p.get(key) {
        None | Some(Json::Null) => Ok(None),
        Some(Json::String(t)) => match gridcraft_core::parse::parse_input(t.trim(), sys).value {
            Value::Number(n) if n.is_finite() && datetime_from_serial(sys, n).is_some() => Ok(Some(n)),
            _ => Err(bad(cmd, format!("`{key}` must be a date"))),
        },
        Some(v) => {
            v.as_f64().filter(|n| datetime_from_serial(sys, *n).is_some()).map(Some).ok_or_else(|| bad(cmd, format!("`{key}` must be a date")))
        }
    }
}

fn select(s: &mut Session, p: &Json) -> Result<Json> {
    const C: &str = "timeline.select";
    let sys = s.doc()?.wb.date_system;
    let start = date_param(C, p, "start", sys)?.ok_or_else(|| bad(C, "missing `start`"))?;
    let end = date_param(C, p, "end", sys)?.unwrap_or(start);
    let lvl = match str_param(p, "level") {
        Some(l) => Some(level_param(C, l)?),
        None => None,
    };
    update_timeline(s, p, true, |tl, wb| {
        let level = lvl.unwrap_or(tl.level);
        let (a, b) = if start <= end { (start, end) } else { (end, start) };
        let first = period_of(sys, level, a).ok_or_else(|| bad(C, "`start` must be a date"))?.0;
        let last = period_of(sys, level, b).ok_or_else(|| bad(C, "`end` must be a date"))?.1;
        tl.selection = Some((first, last));
        tl.selection_level = level;
        if let Some(l) = lvl {
            tl.level = l;
        }
        Ok(json!({"selection": [first, last], "label": selection_label(wb, tl)}))
    })
}

fn clear(s: &mut Session, p: &Json) -> Result<Json> {
    update_timeline(s, p, true, |tl, _| {
        tl.selection = None;
        Ok(json!({}))
    })
}

fn level(s: &mut Session, p: &Json) -> Result<Json> {
    const C: &str = "timeline.level";
    let l = level_param(C, str_param(p, "level").ok_or_else(|| bad(C, "missing `level`"))?)?;
    update_timeline(s, p, false, |tl, _| {
        tl.level = l;
        Ok(json!({"level": level_name(l)}))
    })
}

fn scroll(s: &mut Session, p: &Json) -> Result<Json> {
    const C: &str = "timeline.scroll";
    let sys = s.doc()?.wb.date_system;
    let to = date_param(C, p, "to", sys)?;
    update_timeline(s, p, false, |tl, _| {
        tl.scroll = to.map(f64::floor);
        Ok(json!({}))
    })
}

fn caption(s: &mut Session, p: &Json) -> Result<Json> {
    const C: &str = "timeline.caption";
    let text = str_param(p, "caption").ok_or_else(|| bad(C, "missing `caption`"))?.to_string();
    if text.chars().count() > 255 {
        return Err(bad(C, "`caption` is at most 255 characters"));
    }
    update_timeline(s, p, false, |tl, _| {
        tl.caption = text;
        Ok(json!({}))
    })
}

fn style(s: &mut Session, p: &Json) -> Result<Json> {
    const C: &str = "timeline.style";
    let raw = str_param(p, "style").ok_or_else(|| bad(C, "missing `style`"))?;
    let name = STYLES.iter().find(|n| n.eq_ignore_ascii_case(raw.trim())).ok_or_else(|| bad(C, format!("unknown timeline style `{raw}`")))?;
    update_timeline(s, p, false, |tl, _| {
        tl.style = (*name).to_string();
        Ok(json!({}))
    })
}

fn size(s: &mut Session, p: &Json) -> Result<Json> {
    let (w, h) = (f64_param(p, "width"), f64_param(p, "height"));
    update_timeline(s, p, false, |tl, _| {
        if let Some(w) = w.filter(|w| w.is_finite()) {
            tl.anchor.width = w.clamp(10.0, 10_000.0) as f32;
        }
        if let Some(h) = h.filter(|h| h.is_finite()) {
            tl.anchor.height = h.clamp(10.0, 10_000.0) as f32;
        }
        Ok(json!({"width": tl.anchor.width, "height": tl.anchor.height}))
    })
}

#[derive(Clone, Copy)]
enum Show {
    Header,
    SelectionLabel,
    Scrollbar,
    TimeLevel,
}

fn show(s: &mut Session, p: &Json, what: Show) -> Result<Json> {
    let on = bool_param(p, "on");
    update_timeline(s, p, false, |tl, _| {
        let flag = match what {
            Show::Header => &mut tl.show_header,
            Show::SelectionLabel => &mut tl.show_selection_label,
            Show::Scrollbar => &mut tl.show_scrollbar,
            Show::TimeLevel => &mut tl.show_time_level,
        };
        *flag = on.unwrap_or(!*flag);
        Ok(json!({"on": *flag}))
    })
}

fn report_connections(s: &mut Session, p: &Json) -> Result<Json> {
    const C: &str = "timeline.reportConnections";
    let (si, ti) = find_timeline(s, p)?;
    // PivotTables on the timeline's source that have its field: (id, name, sheet name).
    let candidates: Vec<(u32, String, String)> = {
        let d = s.doc()?;
        let tl = d.wb.sheet(si).and_then(|sh| sh.timelines.get(ti)).ok_or_else(|| other("There's no such timeline."))?;
        let home = connected(&d.wb, tl).first().and_then(|(psi, ppi)| {
            let pt = d.wb.sheet(*psi)?.pivots.get(*ppi)?;
            pv::resolve_source(&d.wb, &pt.source, *psi).ok()
        });
        let mut v = Vec::new();
        for (psi, sh) in d.wb.sheets.iter().enumerate() {
            for pt in &sh.pivots {
                let Ok(src) = pv::read_source(&d.wb, &pt.source, psi) else { continue };
                let same = home.is_none_or(|h| pv::resolve_source(&d.wb, &pt.source, psi).ok() == Some(h));
                if same && src.col(&tl.field).is_some() {
                    v.push((pt.id, pt.name.clone(), sh.name.clone()));
                }
            }
        }
        v
    };
    let Some(list) = p.get("pivots").and_then(Json::as_array) else {
        let d = s.doc()?;
        let tl = d.wb.sheet(si).and_then(|sh| sh.timelines.get(ti)).ok_or_else(|| other("There's no such timeline."))?;
        let rows: Vec<Json> = candidates.iter().map(|(id, n, sh)| json!({"name": n, "sheet": sh, "connected": tl.pivots.contains(id)})).collect();
        let name = tl.name.clone();
        s.ui_requests.push(crate::UiRequest::Dialog("timelineConnections".into(), json!({"timeline": name, "pivots": rows})));
        return ok();
    };
    let mut ids = Vec::new();
    for v in list.iter().take(1000) {
        let n = v.as_str().ok_or_else(|| bad(C, "`pivots` is a list of PivotTable names"))?;
        let (id, ..) = candidates
            .iter()
            .find(|(_, name, _)| name.eq_ignore_ascii_case(n.trim()))
            .ok_or_else(|| bad(C, format!("`{n}` isn't a PivotTable on the same data with the field the timeline filters")))?;
        if !ids.contains(id) {
            ids.push(*id);
        }
    }
    update_timeline(s, p, true, |tl, _| {
        tl.pivots = ids;
        Ok(json!({"pivots": tl.pivots.len()}))
    })
}

fn delete(s: &mut Session, p: &Json) -> Result<Json> {
    let (si, ti) = find_timeline(s, p)?;
    let replace = bool_param(p, "replace").unwrap_or(false);
    edit(s, |cx| {
        let tl = cx.wb.sheet(si).and_then(|sh| sh.timelines.get(ti)).cloned().ok_or_else(|| other("There's no such timeline."))?;
        let targets = connected(&cx.wb, &tl);
        let sh = cx.sheet_mut(si)?;
        if ti < sh.timelines.len() {
            sh.timelines.remove(ti);
        }
        if tl.selection.is_some() {
            for (psi, ppi) in targets {
                pv::refresh(&mut cx.wb, psi, ppi, replace).map_err(other)?;
            }
        }
        cx.structural = true;
        Ok(json!({}))
    })
}

fn periods_query(s: &mut Session, p: &Json) -> Result<Json> {
    let (si, ti) = find_timeline(s, p)?;
    let d = s.doc()?;
    let tl = d.wb.sheet(si).and_then(|sh| sh.timelines.get(ti)).ok_or_else(|| other("There's no such timeline."))?;
    let list: Vec<Json> = periods(&d.wb, tl)
        .into_iter()
        .map(|x| {
            let on = tl.selection.is_some_and(|(a, b)| x.start <= b && x.end >= a);
            json!({"start": x.start, "end": x.end, "label": x.label, "group": x.group, "selected": on})
        })
        .collect();
    Ok(json!({
        "id": tl.id,
        "name": tl.name,
        "field": tl.field,
        "level": level_name(tl.level),
        "selection": tl.selection.map(|(a, b)| json!([a, b])),
        "label": selection_label(&d.wb, tl),
        "periods": list,
    }))
}

#[cfg(test)]
mod tests {
    use gridcraft_core::{CellRef, Value};
    use serde_json::json;

    use super::*;
    use crate::Session;

    fn sales() -> Session {
        let mut s = Session::new();
        s.new_workbook();
        let rows = json!([
            ["Region", "Product", "Date", "Units", "Sales"],
            ["East", "Apples", "1/15/2026", 10, 100],
            ["West", "Apples", "2/10/2026", 5, 50],
            ["East", "Pears", "3/3/2026", 8, 120],
            ["North", "Pears", "4/20/2026", 2, 30],
            ["West", "Plums", "5/5/2026", 7, 70],
            ["East", "Plums", "6/30/2026", 4, 60],
            ["North", "Apples", "7/4/2026", 9, 90],
            ["West", "Pears", "8/8/2026", 3, 45],
            ["East", "Apples", "9/9/2026", 6, 60],
            ["North", "Plums", "10/10/2026", 1, 15],
            ["West", "Apples", "11/11/2026", 12, 120],
            ["East", "Pears", "12/12/2026", 5, 75]
        ]);
        s.execute("range.setValues", json!({"range": "A1", "values": rows})).unwrap();
        s.execute("selection.set", json!({"range": "A1"})).unwrap();
        s
    }

    /// The Grand Total of the PivotTable named `name`.
    fn total(s: &Session, name: &str) -> Value {
        let wb = &s.doc().unwrap().wb;
        for sh in &wb.sheets {
            if let Some(pt) = sh.pivots.iter().find(|p| p.name == name) {
                let r = pt.last_range.unwrap();
                let row = (r.start.row..=r.end.row).find(|row| sh.value(CellRef::new(*row, r.start.col)).display() == "Grand Total").unwrap();
                return sh.value(CellRef::new(row, r.start.col + 1));
            }
        }
        Value::Empty
    }
    fn timeline(s: &Session) -> Timeline {
        s.doc().unwrap().wb.sheets.iter().flat_map(|sh| sh.timelines.iter()).last().unwrap().clone()
    }

    #[test]
    fn a_timeline_filters_its_pivot_tables_by_whole_periods() {
        let mut s = sales();
        s.execute("insert.pivotTable", json!({"rows": ["Region"], "values": ["Sales"]})).unwrap();
        // Without a field: the Insert Timelines dialog lists the date fields.
        s.execute("insert.timeline", json!({})).unwrap();
        match s.ui_requests.last() {
            Some(crate::UiRequest::Dialog(name, p)) => assert_eq!((name.as_str(), p["fields"].clone()), ("insertTimeline", json!(["Date"]))),
            other => panic!("expected Insert Timelines, got {other:?}"),
        }
        let r = s.execute("pivot.insertTimeline", json!({"field": "Date"})).unwrap();
        assert_eq!(r["timelines"][0]["name"], "Date");
        let tl = timeline(&s);
        assert_eq!((tl.caption.as_str(), tl.level, tl.selection, tl.style.as_str()), ("Date", TimelineLevel::Months, None, "TimeSlicerStyleLight1"));
        assert_eq!((tl.anchor.width, tl.anchor.height), (262.5, 108.0));
        // March to May, from dates inside the months: East 120, North 30, West 70.
        let r = s.execute("timeline.select", json!({"start": "3/15/2026", "end": "5/2/2026"})).unwrap();
        assert_eq!(r["label"], "Mar - May 2026");
        assert_eq!(timeline(&s).selection, Some((46082.0, 46173.0)));
        assert_eq!(total(&s, "PivotTable1"), Value::Number(220.0));
        // Quarters: Q1 is January to March.
        s.execute("timeline.select", json!({"start": "2/2/2026", "level": "quarters"})).unwrap();
        assert_eq!(total(&s, "PivotTable1"), Value::Number(270.0));
        assert_eq!(selection_label(&s.doc().unwrap().wb, &timeline(&s)), "Q1 2026");
        assert_eq!(timeline(&s).level, TimelineLevel::Quarters);
        // The level the timeline shows can change without changing the selection.
        s.execute("timeline.level", json!({"level": "days"})).unwrap();
        assert_eq!(total(&s, "PivotTable1"), Value::Number(270.0));
        let q = s.execute("timeline.periods", json!({})).unwrap();
        assert_eq!(q["periods"].as_array().map(Vec::len), Some(365));
        assert_eq!(q["periods"].as_array().map(|p| p.iter().filter(|x| x["selected"] == true).count()), Some(90));
        s.execute("timeline.level", json!({"level": "months"})).unwrap();
        let q = s.execute("timeline.periods", json!({})).unwrap();
        assert_eq!(q["periods"][0]["label"], "JAN");
        assert_eq!(q["periods"][0]["group"], "2026");
        // Clear Filter: all periods again; undo brings the selection back.
        s.execute("timeline.clear", json!({})).unwrap();
        assert_eq!(total(&s, "PivotTable1"), Value::Number(835.0));
        s.execute("edit.undo", json!({})).unwrap();
        assert_eq!(total(&s, "PivotTable1"), Value::Number(270.0));
        // Refusals: a field without dates, a second timeline on the same field, bad levels.
        assert!(s.execute("insert.timeline", json!({"field": "Region"})).unwrap_err().to_string().contains("dates"));
        assert!(s.execute("insert.timeline", json!({"field": "Date"})).unwrap_err().to_string().contains("already"));
        assert!(s.execute("timeline.level", json!({"level": "weeks"})).is_err());
        assert!(s.execute("timeline.select", json!({"start": "soon"})).is_err());
        assert!(s.execute("timeline.style", json!({"style": "SlicerStyleLight1"})).is_err());
    }

    #[test]
    fn report_connections_and_timeline_settings() {
        let mut s = sales();
        s.execute("insert.pivotTable", json!({"rows": ["Region"], "values": ["Sales"]})).unwrap();
        s.execute("insert.pivotTable", json!({"source": "Sheet1!$A$1:$E$13", "rows": ["Product"], "values": ["Units"]})).unwrap();
        s.execute("sheet.activate", json!({"sheet": "Sheet3"})).unwrap();
        s.execute("insert.timeline", json!({"field": "Date", "pivot": "PivotTable1"})).unwrap();
        s.execute("timeline.select", json!({"start": "1/1/2026", "end": "1/31/2026"})).unwrap();
        assert_eq!(total(&s, "PivotTable1"), Value::Number(100.0));
        assert_eq!(total(&s, "PivotTable2"), Value::Number(TOTAL_UNITS));
        // Without `pivots`: the Report Connections dialog lists both.
        s.execute("timeline.reportConnections", json!({})).unwrap();
        match s.ui_requests.last() {
            Some(crate::UiRequest::Dialog(name, p)) => {
                assert_eq!(name, "timelineConnections");
                assert_eq!(p["pivots"].as_array().map(Vec::len), Some(2));
            }
            other => panic!("expected Report Connections, got {other:?}"),
        }
        s.execute("timeline.reportConnections", json!({"pivots": ["PivotTable1", "PivotTable2"]})).unwrap();
        assert_eq!(total(&s, "PivotTable2"), Value::Number(10.0));
        assert!(s.execute("timeline.reportConnections", json!({"pivots": ["Nope"]})).is_err());
        // Disconnecting refreshes the PivotTable it no longer filters.
        s.execute("timeline.reportConnections", json!({"pivots": ["PivotTable2"]})).unwrap();
        assert_eq!(total(&s, "PivotTable1"), Value::Number(835.0));
        // Caption, style, size and the Show checkboxes.
        s.execute("timeline.caption", json!({"caption": "Order date"})).unwrap();
        s.execute("timeline.style", json!({"style": "timeslicerstyledark2"})).unwrap();
        s.execute("timeline.size", json!({"width": 300, "height": 1e9})).unwrap();
        s.execute("timeline.showHeader", json!({"on": false})).unwrap();
        s.execute("timeline.showScrollbar", json!({})).unwrap();
        let tl = timeline(&s);
        assert_eq!((tl.caption.as_str(), tl.style.as_str()), ("Order date", "TimeSlicerStyleDark2"));
        assert_eq!((tl.anchor.width, tl.anchor.height), (300.0, 10_000.0));
        assert!(!tl.show_header && !tl.show_scrollbar && tl.show_selection_label && tl.show_time_level);
        // Deleting a PivotTable disconnects it; deleting the timeline clears its filter.
        s.execute("timeline.reportConnections", json!({"pivots": ["PivotTable1", "PivotTable2"]})).unwrap();
        s.execute("pivot.delete", json!({"pivot": "PivotTable2"})).unwrap();
        assert_eq!(timeline(&s).pivots.len(), 1);
        s.execute("timeline.delete", json!({})).unwrap();
        assert_eq!(total(&s, "PivotTable1"), Value::Number(835.0));
        assert!(s.doc().unwrap().wb.sheets.iter().all(|sh| sh.timelines.is_empty()));
    }

    const TOTAL_UNITS: f64 = 72.0;
}
