//! Timelines ([MS-XLSX] 2.6.x, Excel 2013 and later): a timeline part per sheet
//! (`xl/timelines/timelineN.xml`, `x15:timeline` elements), a timeline cache per timeline
//! (`xl/timelineCaches/timelineCacheN.xml`: the date field, the PivotTables it filters, the
//! selected period and the data bounds), the sheet's `x15:timelineRefs` and the workbook's
//! `x15:timelineCacheRefs` extensions, a drawing frame (`tsle:timeslicer`, with a fallback shape
//! for older readers) and a `NativeTimeline_…` defined name per cache. The filtered PivotTable
//! carries the selection as a `dateBetween` filter that the cache's state names.
//!
//! GridCraft keeps one cache per timeline. A cache can only list PivotTables that share one
//! pivot cache, and GridCraft writes one pivot cache per PivotTable, so a timeline connected to
//! several PivotTables is saved connected to the first of them.

use std::collections::HashMap;
use std::fmt::Write as _;

use gridcraft_core::date::{datetime_from_serial, serial_from_ymd};
use gridcraft_core::{CellRef, DateSystem};
use gridcraft_model::{Anchor, AnchorMode, Sheet, Timeline, TimelineLevel, Workbook};

use crate::read::Ctx;
use crate::xml::{El, esc_attr};

pub const NS_X15: &str = "http://schemas.microsoft.com/office/spreadsheetml/2010/11/main";
pub const REL_TIMELINE: &str = "http://schemas.microsoft.com/office/2011/relationships/timeline";
pub const REL_TIMELINE_CACHE: &str = "http://schemas.microsoft.com/office/2011/relationships/timelineCache";
pub const CT_TIMELINE: &str = "application/vnd.ms-excel.timeline+xml";
pub const CT_TIMELINE_CACHE: &str = "application/vnd.ms-excel.timelineCache+xml";
/// The sheet extension listing its timeline parts.
pub const EXT_SHEET: &str = "{7E03D99C-DC04-49d9-9315-930204A7B6E9}";
/// The workbook extension listing the timeline caches.
pub const EXT_WORKBOOK: &str = "{D0CA8CA8-9F24-4464-BF8E-62219DCF47F9}";
/// `graphicData` URI of a timeline's drawing frame.
pub const GRAPHIC_URI: &str = "http://schemas.microsoft.com/office/drawing/2012/timeslicer";
/// The `x15:pivotFilter` extension of a timeline's PivotTable filter.
const EXT_PIVOT_FILTER: &str = "{0605FD5F-26C8-4aeb-8148-2DB25E43C511}";
/// Filter ids of timeline filters in a PivotTable start here (label, value and Top 10 filters
/// number from 1).
pub const FILTER_ID_BASE: u32 = 1000;
/// Most timelines read from one part or one workbook.
const MAX_TIMELINES: usize = 1_000;

/// The timeline caches' names, per sheet and timeline: `NativeTimeline_Date`, `NativeTimeline_Date1`…
pub fn cache_names(wb: &Workbook) -> Vec<Vec<String>> {
    let mut taken: Vec<String> = wb.names.iter().map(|n| n.name.to_lowercase()).collect();
    wb.sheets
        .iter()
        .map(|sh| {
            sh.timelines
                .iter()
                .map(|t| {
                    let field: String = t.field.chars().map(|c| if c.is_alphanumeric() || c == '_' { c } else { '_' }).collect();
                    let base = format!("NativeTimeline_{field}");
                    let name = std::iter::once(base.clone())
                        .chain((1..10_000).map(|k| format!("{base}{k}")))
                        .find(|n| !taken.contains(&n.to_lowercase()))
                        .unwrap_or(base);
                    taken.push(name.to_lowercase());
                    name
                })
                .collect()
        })
        .collect()
}

/// The timelines with a selection that filter PivotTable `pivot`, in workbook order: their
/// filters in that PivotTable get ids from [`FILTER_ID_BASE`] in this order.
pub fn filters_of(wb: &Workbook, pivot: u32) -> Vec<&Timeline> {
    wb.sheets.iter().flat_map(|s| s.timelines.iter()).filter(|t| t.selection.is_some() && t.pivots.first() == Some(&pivot)).collect()
}

fn level_num(l: TimelineLevel) -> u32 {
    match l {
        TimelineLevel::Years => 0,
        TimelineLevel::Quarters => 1,
        TimelineLevel::Months => 2,
        TimelineLevel::Days => 3,
    }
}

fn level_from(n: Option<u32>) -> TimelineLevel {
    match n {
        Some(0) => TimelineLevel::Years,
        Some(1) => TimelineLevel::Quarters,
        Some(3) => TimelineLevel::Days,
        _ => TimelineLevel::Months,
    }
}

/// `2026-03-01T00:00:00` for a date serial.
fn iso(sys: DateSystem, serial: f64) -> Option<String> {
    let d = datetime_from_serial(sys, serial)?;
    Some(format!("{:04}-{:02}-{:02}T{:02}:{:02}:{:02}", d.year, d.month, d.day, d.hour, d.minute, d.second))
}

// ---------------------------------------------------------------- writing

/// A sheet's timeline part.
pub fn timelines_xml(sheet: &Sheet, names: &[String], sys: DateSystem) -> String {
    let mut s = format!("{}<timelines xmlns=\"{NS_X15}\">", crate::write::XML_DECL);
    for (t, cache) in sheet.timelines.iter().zip(names) {
        let _ = write!(
            s,
            "<timeline name=\"{}\" cache=\"{}\" caption=\"{}\" level=\"{}\" selectionLevel=\"{}\"",
            esc_attr(&t.name),
            esc_attr(cache),
            esc_attr(&t.caption),
            level_num(t.level),
            level_num(t.selection_level)
        );
        if let Some(d) = t.scroll.and_then(|d| iso(sys, d)) {
            let _ = write!(s, " scrollPosition=\"{d}\"");
        }
        let _ = write!(s, " style=\"{}\"", esc_attr(&t.style));
        for (on, attr) in [
            (t.show_header, "showHeader"),
            (t.show_selection_label, "showSelectionLabel"),
            (t.show_time_level, "showTimeLevel"),
            (t.show_scrollbar, "showHorizontalScrollbar"),
        ] {
            if !on {
                let _ = write!(s, " {attr}=\"0\"");
            }
        }
        s.push_str("/>");
    }
    s.push_str("</timelines>");
    s
}

/// The PivotTable a timeline cache is written for: its sheet's `sheetId`, name, pivot cache id
/// and the id of the timeline's filter in it.
pub struct CachePivot {
    pub tab_id: usize,
    pub name: String,
    pub cache_id: u32,
    pub filter_id: u32,
}

/// A timeline cache part. `bounds` are the first and last date of the data.
pub fn cache_xml(t: &Timeline, name: &str, pivot: Option<&CachePivot>, bounds: Option<(f64, f64)>, sys: DateSystem) -> String {
    let mut s = format!(
        "{}<timelineCacheDefinition xmlns=\"{NS_X15}\" name=\"{}\" sourceName=\"{}\">",
        crate::write::XML_DECL,
        esc_attr(name),
        esc_attr(&t.field)
    );
    if let Some(p) = pivot {
        let _ = write!(s, "<pivotTables><pivotTable tabId=\"{}\" name=\"{}\"/></pivotTables>", p.tab_id, esc_attr(&p.name));
    }
    s.push_str("<state minimalRefreshVersion=\"6\" lastRefreshVersion=\"6\"");
    if let Some(p) = pivot {
        let _ = write!(s, " pivotCacheId=\"{}\"", p.cache_id);
    }
    let selection = t.selection.and_then(|(a, b)| Some((iso(sys, a.floor())?, iso(sys, b.floor())?)));
    match (&selection, pivot) {
        (Some(_), Some(p)) => {
            let _ = write!(
                s,
                " filterType=\"dateBetween\" filterId=\"{}\" filterPivotName=\"{}\" filterTabId=\"{}\"",
                p.filter_id,
                esc_attr(&p.name),
                p.tab_id
            );
        }
        _ => s.push_str(" filterType=\"unknown\""),
    }
    s.push('>');
    if let Some((a, b)) = &selection {
        let _ = write!(s, "<selection startDate=\"{a}\" endDate=\"{b}\"/>");
    }
    // The bounds run over whole years around the data.
    let year = |d: f64| datetime_from_serial(sys, d).map(|x| i64::from(x.year));
    if let Some((lo, hi)) = bounds
        && let (Some(y0), Some(y1)) = (year(lo), year(hi))
        && let (Some(a), Some(b)) =
            (serial_from_ymd(sys, y0, 1, 1).and_then(|d| iso(sys, d)), serial_from_ymd(sys, y1 + 1, 1, 1).and_then(|d| iso(sys, d)))
    {
        let _ = write!(s, "<bounds startDate=\"{a}\" endDate=\"{b}\"/>");
    }
    s.push_str("</state></timelineCacheDefinition>");
    s
}

/// A PivotTable's filter for a timeline's selection (`dateBetween` on cache field `fld`).
pub fn filter_xml(fld: usize, id: u32, t: &Timeline) -> String {
    let Some((a, b)) = t.selection else { return String::new() };
    format!(
        "<filter fld=\"{fld}\" type=\"dateBetween\" evalOrder=\"-1\" id=\"{id}\" name=\"{}\"><autoFilter ref=\"A1\"><filterColumn colId=\"0\"><customFilters and=\"1\"><customFilter operator=\"greaterThanOrEqual\" val=\"{}\"/><customFilter operator=\"lessThanOrEqual\" val=\"{}\"/></customFilters></filterColumn></autoFilter><extLst><ext uri=\"{EXT_PIVOT_FILTER}\" xmlns:x15=\"{NS_X15}\"><x15:pivotFilter useWholeDay=\"1\"/></ext></extLst></filter>",
        esc_attr(&t.field),
        crate::xml::num(a.floor()),
        crate::xml::num(b.floor())
    )
}

/// A timeline's drawing frame, with a fallback shape for readers without timelines.
pub fn frame_xml(t: &Timeline, id: usize, xfrm: &str, a_xfrm: &str) -> String {
    format!(
        "<mc:AlternateContent xmlns:mc=\"http://schemas.openxmlformats.org/markup-compatibility/2006\"><mc:Choice xmlns:tsle=\"{GRAPHIC_URI}\" Requires=\"tsle\"><xdr:graphicFrame macro=\"\"><xdr:nvGraphicFramePr><xdr:cNvPr id=\"{id}\" name=\"{name}\"/><xdr:cNvGraphicFramePr/></xdr:nvGraphicFramePr>{xfrm}<a:graphic><a:graphicData uri=\"{GRAPHIC_URI}\"><tsle:timeslicer name=\"{name}\"/></a:graphicData></a:graphic></xdr:graphicFrame></mc:Choice><mc:Fallback><xdr:sp macro=\"\" textlink=\"\"><xdr:nvSpPr><xdr:cNvPr id=\"0\" name=\"\"/><xdr:cNvSpPr><a:spLocks noTextEdit=\"1\"/></xdr:cNvSpPr></xdr:nvSpPr><xdr:spPr>{a_xfrm}<a:prstGeom prst=\"rect\"><a:avLst/></a:prstGeom></xdr:spPr><xdr:txBody><a:bodyPr/><a:lstStyle/><a:p><a:r><a:rPr lang=\"en-US\"/><a:t>A timeline filters a PivotTable by date. It needs Excel 2013 or later, or GridCraft.</a:t></a:r></a:p></xdr:txBody></xdr:sp></mc:Fallback></mc:AlternateContent>",
        name = esc_attr(&t.name)
    )
}

// ---------------------------------------------------------------- reading

/// A timeline element as read from a sheet's timeline part.
pub struct TimelineIn {
    pub name: String,
    pub cache: String,
    pub caption: String,
    pub level: TimelineLevel,
    pub selection_level: TimelineLevel,
    pub scroll: Option<f64>,
    pub style: String,
    pub show: [bool; 4],
}

/// A timeline cache as read.
pub struct CacheIn {
    pub name: String,
    pub field: String,
    /// (sheetId, PivotTable name) of each PivotTable it filters.
    pub pivots: Vec<(u32, String)>,
    pub selection: Option<(f64, f64)>,
    /// The id of its filter in each of its PivotTables.
    pub filter_id: Option<u32>,
}

/// Reads the workbook's timeline caches (`timelineCache` relationships).
pub fn read_caches(cx: &mut Ctx<'_>, wb_rels: &[crate::package::Rel]) -> Result<Vec<CacheIn>, crate::IoError> {
    let sys = cx.date_system;
    let mut out = Vec::new();
    for rel in wb_rels.iter().filter(|r| r.kind == "timelineCache" && !r.external).take(MAX_TIMELINES) {
        let Some(x) = cx.optional_xml(&rel.target)? else { continue };
        if x.name != "timelineCacheDefinition" {
            cx.warn(format!("{} is not a timeline cache; its timeline was skipped", rel.target));
            continue;
        }
        let (Some(name), Some(field)) = (x.attr("name"), x.attr("sourceName")) else {
            cx.warn("a timeline cache without its name or field was skipped");
            continue;
        };
        let pivots = x
            .child("pivotTables")
            .map(|p| p.kids("pivotTable").take(MAX_TIMELINES).filter_map(|t| Some((t.attr_u32("tabId")?, t.attr("name")?.to_string()))).collect())
            .unwrap_or_default();
        let state = x.child("state");
        let date = |e: Option<&El>, k: &str| e.and_then(|e| e.attr(k)).and_then(|v| crate::sheet_read::parse_iso_datetime(v, sys));
        let sel = state.and_then(|s| s.child("selection"));
        let selection = match (date(sel, "startDate"), date(sel, "endDate")) {
            (Some(a), Some(b)) if a <= b => Some((a.floor(), b.floor())),
            (Some(a), None) => Some((a.floor(), a.floor())),
            _ => None,
        };
        let filter_id = state.and_then(|s| s.attr_u32("filterId"));
        out.push(CacheIn { name: name.to_string(), field: field.to_string(), pivots, selection, filter_id });
    }
    Ok(out)
}

/// Reads a sheet's timeline parts (`timeline` relationships).
pub fn read_sheet(cx: &mut Ctx<'_>, sheet_part: &str) -> Result<Vec<TimelineIn>, crate::IoError> {
    let sys = cx.date_system;
    let rels = cx.pkg.rels(sheet_part)?;
    let mut out = Vec::new();
    for rel in rels.iter().filter(|r| r.kind == "timeline" && !r.external) {
        let Some(x) = cx.optional_xml(&rel.target)? else { continue };
        for t in x.kids("timeline").take(MAX_TIMELINES) {
            let (Some(name), Some(cache)) = (t.attr("name"), t.attr("cache")) else { continue };
            out.push(TimelineIn {
                name: name.to_string(),
                cache: cache.to_string(),
                caption: t.attr("caption").unwrap_or(name).to_string(),
                level: level_from(t.attr_u32("level")),
                selection_level: level_from(t.attr_u32("selectionLevel")),
                scroll: t.attr("scrollPosition").and_then(|v| crate::sheet_read::parse_iso_datetime(v, sys)).map(f64::floor),
                style: t.attr("style").filter(|s| !s.is_empty()).unwrap_or("TimeSlicerStyleLight1").to_string(),
                show: [
                    t.flag("showHeader", true),
                    t.flag("showSelectionLabel", true),
                    t.flag("showTimeLevel", true),
                    t.flag("showHorizontalScrollbar", true),
                ],
            });
        }
    }
    Ok(out)
}

/// Builds the model's timelines once every sheet and PivotTable is read. `sheets[i]` holds the
/// timelines read from the i-th model sheet with their drawing frames (timeline name → anchor);
/// `tab_ids` maps a `sheetId` to its model sheet.
pub fn attach(
    cx: &mut Ctx<'_>,
    wb: &mut Workbook,
    caches: &[CacheIn],
    sheets: Vec<(Vec<TimelineIn>, HashMap<String, Anchor>)>,
    tab_ids: &HashMap<u32, usize>,
) {
    for (si, (list, frames)) in sheets.into_iter().enumerate() {
        for t in list {
            let Some(cache) = caches.iter().find(|c| c.name.eq_ignore_ascii_case(&t.cache)) else {
                cx.warn(format!("timeline \"{}\" has no cache and was skipped", t.name));
                continue;
            };
            let pivots: Vec<u32> = cache
                .pivots
                .iter()
                .filter_map(|(tab, name)| {
                    let sheet = wb.sheet(*tab_ids.get(tab)?)?;
                    sheet.pivots.iter().find(|p| p.name.eq_ignore_ascii_case(name)).map(|p| p.id)
                })
                .collect();
            if pivots.is_empty() && !cache.pivots.is_empty() {
                cx.warn(format!("timeline \"{}\" filters a PivotTable that wasn't read; it filters nothing", t.name));
            }
            let anchor = frames.get(&t.name.to_lowercase()).copied().unwrap_or(Anchor {
                cell: CellRef::new(0, 0),
                dx: 0.0,
                dy: 0.0,
                width: 262.5,
                height: 108.0,
                mode: AnchorMode::MoveOnly,
            });
            let id = cx.next_object_id();
            let [show_header, show_selection_label, show_time_level, show_scrollbar] = t.show;
            let tl = Timeline {
                id,
                name: t.name,
                caption: t.caption,
                field: cache.field.clone(),
                pivots,
                anchor,
                level: t.level,
                selection: cache.selection,
                selection_level: t.selection_level,
                scroll: t.scroll,
                style: t.style,
                show_header,
                show_selection_label,
                show_time_level,
                show_scrollbar,
            };
            if let Some(sheet) = wb.sheet_mut(si) {
                sheet.timelines.push(tl);
            }
        }
    }
}
