//! PivotTable dialogs and menus beyond the Fields pane: the field buttons on a report's
//! Row Labels / Column Labels / field header cells with their menu (sort, Label Filters ▸,
//! Value Filters ▸, the item checklist), the Label Filter, Value Filter and Top 10 dialogs, the
//! Grouping dialog, Insert Calculated Item, Calculated Item Solve Order, Insert Timelines and a
//! timeline's Report Connections. Every action runs an engine command.

use egui::{Pos2, Rect, Ui, pos2, vec2};
use gridcraft_engine::core::CellRef;
use gridcraft_engine::model::{PivotLayout, PivotTable, Sheet};
use serde_json::{Value as Json, json};

use crate::SheetApp;
use crate::dialogs::Dialog;
use crate::grid::Geo;

/// The dialogs this module draws.
pub const DIALOGS: [&str; 8] = [
    "pivotGrouping",
    "pivotCalculatedItem",
    "pivotSolveOrder",
    "insertTimeline",
    "timelineConnections",
    "pivotLabelFilter",
    "pivotValueFilter",
    "pivotTop10",
];

/// Excel's title of one of [`DIALOGS`].
pub fn title(name: &str) -> &'static str {
    match name {
        "pivotGrouping" => "Grouping",
        "pivotCalculatedItem" => "Insert Calculated Item",
        "pivotSolveOrder" => "Calculated Item Solve Order",
        "insertTimeline" => "Insert Timelines",
        "timelineConnections" => "Report Connections",
        "pivotLabelFilter" => "Label Filter",
        "pivotValueFilter" => "Value Filter",
        _ => "Top 10 Filter",
    }
}

/// The label comparisons, in the order of Excel's Label Filters menu: (command name, menu item, dialog choice).
pub const LABEL_OPS: [(&str, &str, &str); 14] = [
    ("equals", "Equals…", "equals"),
    ("doesNotEqual", "Does Not Equal…", "does not equal"),
    ("beginsWith", "Begins With…", "begins with"),
    ("doesNotBeginWith", "Does Not Begin With…", "does not begin with"),
    ("endsWith", "Ends With…", "ends with"),
    ("doesNotEndWith", "Does Not End With…", "does not end with"),
    ("contains", "Contains…", "contains"),
    ("doesNotContain", "Does Not Contain…", "does not contain"),
    ("greaterThan", "Greater Than…", "is greater than"),
    ("greaterThanOrEqual", "Greater Than Or Equal To…", "is greater than or equal to"),
    ("lessThan", "Less Than…", "is less than"),
    ("lessThanOrEqual", "Less Than Or Equal To…", "is less than or equal to"),
    ("between", "Between…", "is between"),
    ("notBetween", "Not Between…", "is not between"),
];

/// The value comparisons of Excel's Value Filters menu (Top 10… follows them).
pub const VALUE_OPS: [&str; 8] =
    ["equals", "doesNotEqual", "greaterThan", "greaterThanOrEqual", "lessThan", "lessThanOrEqual", "between", "notBetween"];

fn op_entry(op: &str) -> (&'static str, &'static str, &'static str) {
    LABEL_OPS.iter().copied().find(|(o, ..)| *o == op).unwrap_or(LABEL_OPS[0])
}

// ---------------------------------------------------------------- report field buttons

/// A field button on a report's header cell.
#[derive(Clone, Debug, PartialEq)]
pub struct HeaderButton {
    pub cell: CellRef,
    pub pivot: String,
    /// The fields it filters and sorts (Compact form's Row Labels covers every row field).
    pub fields: Vec<String>,
    /// A report filter's button (its menu is the item list only).
    pub page: bool,
    /// One of its fields is filtered.
    pub filtered: bool,
}

/// The field buttons of the PivotTables on a sheet, where Excel draws them: Row Labels and
/// Column Labels in Compact form, each field's header otherwise, and each report filter.
pub fn header_buttons(sh: &Sheet) -> Vec<HeaderButton> {
    let mut out = Vec::new();
    for pt in &sh.pivots {
        if pt.last_range.is_none() {
            continue;
        }
        let filtered = |f: &str| pt.filters.iter().any(|x| x.source_col.eq_ignore_ascii_case(f) && (x.selected.is_some() || x.rule.is_some()));
        let on_axis = |f: &str| pt.rows.iter().chain(pt.columns.iter()).any(|a| a.source_col.eq_ignore_ascii_case(f));
        let a = pt.anchor;
        // Report filters sit above the body, one row each, then a blank row.
        let pages: Vec<&str> = pt.filters.iter().filter(|f| f.rule.is_none() && !on_axis(&f.source_col)).map(|f| f.source_col.as_str()).collect();
        let top = a.row.saturating_sub(pages.len() as u32 + u32::from(!pages.is_empty()));
        for (i, f) in pages.iter().enumerate() {
            out.push(HeaderButton {
                cell: CellRef::new(top + i as u32, a.col.saturating_add(1)),
                pivot: pt.name.clone(),
                fields: vec![f.to_string()],
                page: true,
                filtered: filtered(f),
            });
        }
        if !pt.show_headers {
            continue;
        }
        let k = pt.values.len();
        let on_rows = pt.values_on_rows && k > 1;
        let rows: Vec<String> = dedup(pt.rows.iter().map(|f| f.source_col.clone()));
        let cols: Vec<String> = dedup(pt.columns.iter().map(|f| f.source_col.clone()));
        let row_levels = pt.rows.len() + usize::from(on_rows);
        let compact = pt.layout == PivotLayout::Compact;
        let l = if row_levels == 0 && cols.is_empty() {
            0
        } else if compact {
            1
        } else {
            row_levels.max(1)
        };
        let header_row = if cols.is_empty() { a.row } else { a.row + (pt.columns.len() + usize::from(k > 1 && !on_rows)) as u32 };
        if !cols.is_empty() {
            let fields = if compact { cols.clone() } else { cols.iter().take(1).cloned().collect() };
            let any = fields.iter().any(|f| filtered(f));
            out.push(HeaderButton {
                cell: CellRef::new(a.row, a.col.saturating_add(l as u32)),
                pivot: pt.name.clone(),
                fields,
                page: false,
                filtered: any,
            });
        }
        if rows.is_empty() {
            continue;
        }
        if compact {
            let any = rows.iter().any(|f| filtered(f));
            out.push(HeaderButton { cell: CellRef::new(header_row, a.col), pivot: pt.name.clone(), fields: rows, page: false, filtered: any });
        } else {
            let vpos = on_rows.then(|| pt.values_position.unwrap_or(pt.rows.len()));
            let mut col = a.col;
            for (i, f) in pt.rows.iter().enumerate() {
                if vpos == Some(i) {
                    col = col.saturating_add(1);
                }
                out.push(HeaderButton {
                    cell: CellRef::new(header_row, col),
                    pivot: pt.name.clone(),
                    fields: vec![f.source_col.clone()],
                    page: false,
                    filtered: filtered(&f.source_col),
                });
                col = col.saturating_add(1);
            }
        }
    }
    out
}

fn dedup(it: impl Iterator<Item = String>) -> Vec<String> {
    let mut v: Vec<String> = Vec::new();
    for s in it {
        if !v.iter().any(|x| x.eq_ignore_ascii_case(&s)) {
            v.push(s);
        }
    }
    v
}

/// The button rect of a header cell (the filter button's place).
pub fn button_rect(geo: &Geo, sh: &Sheet, c: CellRef) -> Rect {
    let rect = geo.cell_rect(sh, c);
    let s = (rect.height() - 4.0).clamp(10.0, 18.0);
    let pad = 2.0 * geo.z;
    Rect::from_min_size(pos2(rect.right() - s - pad, rect.bottom() - s - pad), vec2(s, s))
}

/// The field button under `p`, if any.
pub fn button_at(sh: &Sheet, geo: &Geo, p: Pos2) -> Option<HeaderButton> {
    header_buttons(sh).into_iter().find(|b| button_rect(geo, sh, b.cell).contains(p))
}

/// The open field menu.
#[derive(Clone, Debug, PartialEq)]
pub struct PivotMenu {
    pub button: HeaderButton,
    pub at: Pos2,
}

/// A PivotTable by name on the active workbook.
fn pivot_by_name(app: &SheetApp, name: &str) -> Option<PivotTable> {
    app.session.active()?.wb.sheets.iter().flat_map(|s| s.pivots.iter()).find(|p| p.name.eq_ignore_ascii_case(name)).cloned()
}

/// The menu of a report's field button: Select field (Compact form), Sort A to Z / Z to A,
/// Clear Filter From, Label Filters ▸, Value Filters ▸, then the searchable item checklist.
pub fn field_menu(app: &mut SheetApp, ui: &mut Ui, was_open: bool) {
    let Some(menu) = app.grid.pivot_menu.clone() else { return };
    let lang = crate::i18n::current(ui.ctx());
    let pivot = menu.button.pivot.clone();
    let field_key = egui::Id::new(("pivot_menu_field", &pivot, menu.button.cell.row, menu.button.cell.col));
    let mut field: String = ui.ctx().data(|d| d.get_temp(field_key)).unwrap_or_else(|| menu.button.fields.first().cloned().unwrap_or_default());
    if !menu.button.fields.iter().any(|f| f.eq_ignore_ascii_case(&field)) {
        field = menu.button.fields.first().cloned().unwrap_or_default();
    }
    let items_key = egui::Id::new(("pivot_menu_items", &pivot, &field));
    let search_key = egui::Id::new(("pivot_menu_search", &pivot, &field));
    let mut checks: Vec<(String, bool)> = match ui.ctx().data(|d| d.get_temp::<Vec<(String, bool)>>(items_key)) {
        Some(v) => v,
        None => app
            .session
            .run("pivot.fieldItems", json!({"pivot": pivot, "field": field}))
            .ok()
            .and_then(|r| r["items"].as_array().cloned())
            .unwrap_or_default()
            .iter()
            .map(|i| (i["label"].as_str().unwrap_or("").to_string(), i["shown"].as_bool().unwrap_or(true)))
            .collect(),
    };
    let mut close = false;
    let area = egui::Area::new(egui::Id::new("pivot_field_menu")).fixed_pos(menu.at + vec2(-200.0, 10.0)).order(egui::Order::Foreground);
    let response = area.show(ui.ctx(), |ui| {
        egui::Frame::popup(ui.style()).show(ui, |ui| {
            ui.set_width(250.0);
            if menu.button.fields.len() > 1 {
                ui.horizontal(|ui| {
                    ui.label(lang.tr("Select field:"));
                    egui::ComboBox::from_id_salt("pivot_menu_select").selected_text(field.as_str()).show_ui(ui, |ui| {
                        for f in &menu.button.fields {
                            if ui.selectable_label(f.eq_ignore_ascii_case(&field), f.as_str()).clicked() {
                                field = f.clone();
                            }
                        }
                    });
                });
            }
            if !menu.button.page {
                for (order, label) in [("asc", "Sort A to Z"), ("desc", "Sort Z to A")] {
                    if ui.button(lang.tr(label)).clicked() {
                        app.run_or_alert("pivot.sort", json!({"pivot": pivot, "field": field, "order": order}));
                        close = true;
                    }
                }
                ui.add_enabled(false, egui::Button::new(lang.tr("More Sort Options…")));
                ui.separator();
            }
            let clear_label = format!("{} \"{field}\"", lang.tr("Clear Filter From"));
            if ui.button(clear_label).clicked() {
                app.run_or_alert("pivot.clearFilter", json!({"pivot": pivot, "field": field}));
                close = true;
            }
            if !menu.button.page {
                ui.menu_button(lang.tr("Label Filters"), |ui| {
                    if ui.button(lang.tr("Clear Filter")).clicked() {
                        app.run_or_alert("pivot.clearFilter", json!({"pivot": pivot, "field": field}));
                        close = true;
                    }
                    ui.separator();
                    for (op, label, _) in LABEL_OPS {
                        if ui.button(lang.tr(label)).clicked() {
                            app.open_dialog("pivotLabelFilter", json!({"pivot": pivot, "field": field, "op": op}));
                            close = true;
                        }
                    }
                });
                ui.menu_button(lang.tr("Value Filters"), |ui| {
                    if ui.button(lang.tr("Clear Filter")).clicked() {
                        app.run_or_alert("pivot.clearFilter", json!({"pivot": pivot, "field": field}));
                        close = true;
                    }
                    ui.separator();
                    for op in VALUE_OPS {
                        if ui.button(lang.tr(op_entry(op).1)).clicked() {
                            app.open_dialog("pivotValueFilter", json!({"pivot": pivot, "field": field, "op": op}));
                            close = true;
                        }
                    }
                    ui.separator();
                    if ui.button(lang.tr("Top 10…")).clicked() {
                        app.open_dialog("pivotTop10", json!({"pivot": pivot, "field": field}));
                        close = true;
                    }
                });
            }
            ui.separator();
            let mut search: String = ui.ctx().data(|d| d.get_temp(search_key)).unwrap_or_default();
            let edit = ui.add(egui::TextEdit::singleline(&mut search).hint_text(lang.tr("Search")).desired_width(f32::INFINITY));
            if !was_open {
                edit.request_focus();
            }
            let needle = search.trim().to_lowercase();
            let matches = |v: &str| needle.is_empty() || v.to_lowercase().contains(&needle);
            let all = checks.iter().filter(|(v, _)| matches(v)).all(|(_, b)| *b);
            let mut all_new = all;
            if ui.checkbox(&mut all_new, lang.tr("(Select All)")).changed() {
                for c in checks.iter_mut().filter(|(v, _)| matches(v)) {
                    c.1 = all_new;
                }
            }
            egui::ScrollArea::vertical().max_height(200.0).auto_shrink([false, true]).show(ui, |ui| {
                for (v, on) in checks.iter_mut().filter(|(v, _)| matches(v)) {
                    ui.checkbox(on, v.as_str());
                }
            });
            ui.ctx().data_mut(|d| d.insert_temp(search_key, search.clone()));
            ui.separator();
            ui.horizontal(|ui| {
                if ui.button(lang.tr("OK")).clicked() {
                    let shown: Vec<String> = checks.iter().filter(|(v, on)| *on && matches(v)).map(|(v, _)| v.clone()).collect();
                    let every = checks.iter().all(|(v, on)| *on && matches(v));
                    let selected = if every { Json::Null } else { json!(shown) };
                    app.run_or_alert("pivot.filter", json!({"pivot": pivot, "field": field, "selected": selected}));
                    close = true;
                }
                if ui.button(lang.tr("Cancel")).clicked() {
                    close = true;
                }
            });
        });
    });
    ui.ctx().data_mut(|d| {
        d.insert_temp(field_key, field.clone());
        d.insert_temp(items_key, checks.clone());
    });
    let dismissed = response.response.ctx.input(|i| i.key_pressed(egui::Key::Escape)) || (was_open && response.response.clicked_elsewhere());
    if close || dismissed {
        ui.ctx().data_mut(|d| {
            d.remove::<Vec<(String, bool)>>(items_key);
            d.remove::<String>(search_key);
        });
        app.grid.pivot_menu = None;
    }
}

// ---------------------------------------------------------------- dialogs

/// Initial values of one of [`DIALOGS`], from the parameters it was opened with.
pub fn defaults(app: &SheetApp, name: &str, params: &Json) -> Json {
    let mut v = params.clone();
    if !v.is_object() {
        v = json!({});
    }
    let pivot = params["pivot"].as_str().unwrap_or("").to_string();
    let pt = pivot_by_name(app, &pivot);
    let value_fields: Vec<String> = pt.as_ref().map(|p| p.values.iter().map(|x| x.name.clone()).collect()).unwrap_or_default();
    if let Some(m) = v.as_object_mut() {
        match name {
            "pivotLabelFilter" => {
                m.entry("value").or_insert(json!(""));
                m.entry("value2").or_insert(json!(""));
            }
            "pivotValueFilter" => {
                m.entry("valueField").or_insert(json!(value_fields.first().cloned().unwrap_or_default()));
                m.insert("valueFields".into(), json!(value_fields));
                m.entry("value").or_insert(json!(""));
                m.entry("value2").or_insert(json!(""));
            }
            "pivotTop10" => {
                m.entry("top").or_insert(json!(true));
                m.entry("count").or_insert(json!(10));
                m.entry("by").or_insert(json!("items"));
                m.entry("valueField").or_insert(json!(value_fields.first().cloned().unwrap_or_default()));
                m.insert("valueFields".into(), json!(value_fields));
            }
            "pivotGrouping" => {
                let sys = app.session.active().map(|d| d.wb.date_system).unwrap_or_default();
                for key in ["start", "end"] {
                    if params["kind"] == "date"
                        && let Some(n) = params[key].as_f64()
                        && let Some(d) = gridcraft_engine::core::date::datetime_from_serial(sys, n)
                    {
                        m.insert(format!("{key}Text"), json!(format!("{}/{}/{}", d.month, d.day, d.year)));
                    } else if let Some(n) = params[key].as_f64() {
                        m.insert(format!("{key}Text"), json!(gridcraft_engine::core::number_to_text(n)));
                    }
                }
            }
            "insertTimeline" => {
                m.insert("checked".into(), json!([]));
            }
            "timelineConnections" => {
                let on: Vec<Json> = params["pivots"]
                    .as_array()
                    .cloned()
                    .unwrap_or_default()
                    .iter()
                    .filter(|p| p["connected"] == true)
                    .map(|p| p["name"].clone())
                    .collect();
                m.insert("checked".into(), json!(on));
            }
            _ => {}
        }
    }
    v
}

fn text_of(d: &Dialog, key: &str) -> String {
    match d.values.get(key) {
        Some(Json::String(s)) => s.clone(),
        Some(Json::Number(n)) => n.to_string(),
        _ => String::new(),
    }
}

/// A one-line text field bound to `key`.
fn text_field(ui: &mut Ui, d: &mut Dialog, key: &str, width: f32) {
    let mut s = text_of(d, key);
    if ui.add(egui::TextEdit::singleline(&mut s).desired_width(width)).changed() {
        d.values.insert(key.into(), json!(s));
    }
}

/// A drop-down bound to `key` over (value, caption) pairs.
fn combo(ui: &mut Ui, d: &mut Dialog, key: &str, options: &[(String, String)], width: f32) {
    let lang = crate::i18n::current(ui.ctx());
    let cur = d.values.get(key).and_then(Json::as_str).unwrap_or("").to_string();
    let shown = options.iter().find(|(v, _)| *v == cur).map(|(_, c)| lang.tr(c).to_string()).unwrap_or(cur.clone());
    egui::ComboBox::from_id_salt(("pivot_dialog", key)).selected_text(shown).width(width).show_ui(ui, |ui| {
        for (v, c) in options {
            if ui.selectable_label(*v == cur, lang.tr(c)).clicked() {
                d.values.insert(key.into(), json!(v));
            }
        }
    });
}

/// A checklist bound to the string list `key`.
fn checklist(ui: &mut Ui, d: &mut Dialog, key: &str, items: &[(String, String)]) {
    let mut on: Vec<String> =
        d.values.get(key).and_then(Json::as_array).map(|a| a.iter().filter_map(|x| x.as_str().map(str::to_string)).collect()).unwrap_or_default();
    egui::ScrollArea::vertical().max_height(220.0).show(ui, |ui| {
        for (value, caption) in items {
            let mut b = on.iter().any(|x| x == value);
            if ui.checkbox(&mut b, caption.as_str()).changed() {
                if b {
                    on.push(value.clone());
                } else {
                    on.retain(|x| x != value);
                }
            }
        }
    });
    d.values.insert(key.into(), json!(on));
}

/// Runs a dialog's command; an error keeps the dialog open with the message.
fn run(app: &mut SheetApp, d: &Dialog, cmd: &str, params: Json, confirm: &mut bool) -> bool {
    match app.run(cmd, params) {
        Ok(_) => true,
        Err(e) => {
            app.message = Some((d.title.clone(), crate::clean_error(&e)));
            *confirm = false;
            false
        }
    }
}

/// Draws one of [`DIALOGS`].
pub fn dialog(app: &mut SheetApp, ui: &mut Ui, d: &mut Dialog, confirm: &mut bool, open: &mut bool) {
    let lang = crate::i18n::current(ui.ctx());
    let s = |k: &str, d: &Dialog| d.values.get(k).and_then(Json::as_str).unwrap_or("").to_string();
    match d.name() {
        "pivotLabelFilter" => {
            ui.label(format!("{} ({})", lang.tr("Show items for which the label"), s("field", d)));
            ui.horizontal(|ui| {
                let ops: Vec<(String, String)> = LABEL_OPS.iter().map(|(o, _, c)| (o.to_string(), c.to_string())).collect();
                combo(ui, d, "op", &ops, 170.0);
                text_field(ui, d, "value", 120.0);
            });
            let range = matches!(s("op", d).as_str(), "between" | "notBetween");
            if range {
                ui.horizontal(|ui| {
                    ui.label(lang.tr("and"));
                    text_field(ui, d, "value2", 120.0);
                });
            }
            ui.label(egui::RichText::new(lang.tr("Use ? to represent any single character")).small());
            ui.label(egui::RichText::new(lang.tr("Use * to represent any series of characters")).small());
            crate::dialogs::ok_cancel(ui, confirm, open);
            if *confirm {
                let mut p = json!({"pivot": s("pivot", d), "field": s("field", d), "op": s("op", d), "value": text_of(d, "value")});
                if range {
                    p["value2"] = json!(text_of(d, "value2"));
                }
                run(app, d, "pivot.labelFilter", p, confirm);
            }
        }
        "pivotValueFilter" => {
            ui.label(format!("{} ({})", lang.tr("Show items for which"), s("field", d)));
            let fields: Vec<(String, String)> = d
                .values
                .get("valueFields")
                .and_then(Json::as_array)
                .cloned()
                .unwrap_or_default()
                .iter()
                .filter_map(|x| x.as_str())
                .map(|x| (x.to_string(), x.to_string()))
                .collect();
            ui.horizontal(|ui| {
                combo(ui, d, "valueField", &fields, 130.0);
                let ops: Vec<(String, String)> = VALUE_OPS.iter().map(|o| (o.to_string(), op_entry(o).2.to_string())).collect();
                combo(ui, d, "op", &ops, 150.0);
                text_field(ui, d, "value", 80.0);
            });
            let range = matches!(s("op", d).as_str(), "between" | "notBetween");
            if range {
                ui.horizontal(|ui| {
                    ui.label(lang.tr("and"));
                    text_field(ui, d, "value2", 80.0);
                });
            }
            crate::dialogs::ok_cancel(ui, confirm, open);
            if *confirm {
                let num = |k: &str| text_of(d, k).trim().replace(',', ".").parse::<f64>().ok();
                let Some(value) = num("value") else {
                    app.message = Some((d.title.clone(), "Enter a number.".into()));
                    *confirm = false;
                    return;
                };
                let mut p =
                    json!({"pivot": s("pivot", d), "field": s("field", d), "op": s("op", d), "value": value, "valueField": s("valueField", d)});
                if range {
                    p["value2"] = json!(num("value2").unwrap_or(0.0));
                }
                run(app, d, "pivot.valueFilter", p, confirm);
            }
        }
        "pivotTop10" => {
            ui.label(format!("{} ({})", lang.tr("Show"), s("field", d)));
            let fields: Vec<(String, String)> = d
                .values
                .get("valueFields")
                .and_then(Json::as_array)
                .cloned()
                .unwrap_or_default()
                .iter()
                .filter_map(|x| x.as_str())
                .map(|x| (x.to_string(), x.to_string()))
                .collect();
            ui.horizontal(|ui| {
                let mut top = d.values.get("top").and_then(Json::as_bool).unwrap_or(true);
                egui::ComboBox::from_id_salt("top10_side").selected_text(lang.tr(if top { "Top" } else { "Bottom" })).width(70.0).show_ui(ui, |ui| {
                    ui.selectable_value(&mut top, true, lang.tr("Top"));
                    ui.selectable_value(&mut top, false, lang.tr("Bottom"));
                });
                d.values.insert("top".into(), json!(top));
                let mut n = d.values.get("count").and_then(Json::as_f64).unwrap_or(10.0);
                if ui.add(egui::DragValue::new(&mut n).speed(1.0).range(0.0..=2_147_483_647.0)).changed() {
                    d.values.insert("count".into(), json!(n));
                }
                let by = [("items", "Items"), ("percent", "Percent"), ("sum", "Sum")].map(|(a, b)| (a.to_string(), b.to_string()));
                combo(ui, d, "by", &by, 80.0);
                ui.label(lang.tr("by"));
                combo(ui, d, "valueField", &fields, 130.0);
            });
            crate::dialogs::ok_cancel(ui, confirm, open);
            if *confirm {
                let p = json!({
                    "pivot": s("pivot", d), "field": s("field", d), "top": d.values.get("top").cloned().unwrap_or(json!(true)),
                    "count": d.values.get("count").cloned().unwrap_or(json!(10)), "by": s("by", d), "valueField": s("valueField", d),
                });
                run(app, d, "pivot.top10", p, confirm);
            }
        }
        "pivotGrouping" => grouping(app, ui, d, confirm, open),
        "pivotCalculatedItem" => calculated_item(app, ui, d, confirm, open),
        "pivotSolveOrder" => {
            ui.label(lang.tr("Solve order:"));
            let list: Vec<Json> = d.values.get("calculatedItems").and_then(Json::as_array).cloned().unwrap_or_default();
            let mut sel = d.values.get("selected").and_then(Json::as_u64).unwrap_or(0) as usize;
            egui::ScrollArea::vertical().max_height(160.0).show(ui, |ui| {
                for (i, c) in list.iter().enumerate() {
                    let text = format!("'{}' = {}", c["name"].as_str().unwrap_or(""), c["formula"].as_str().unwrap_or("").trim_start_matches('='));
                    if ui.selectable_label(i == sel, text).clicked() {
                        sel = i;
                    }
                }
            });
            d.values.insert("selected".into(), json!(sel));
            let name = list.get(sel).and_then(|c| c["name"].as_str()).unwrap_or("").to_string();
            let pivot = s("pivot", d);
            ui.horizontal(|ui| {
                for (label, step) in [("Move Up", "up"), ("Move Down", "down")] {
                    if ui.button(lang.tr(label)).clicked()
                        && !name.is_empty()
                        && let Ok(r) = app.run("pivot.solveOrder", json!({"pivot": pivot, "item": name, "move": step}))
                    {
                        let order: Vec<String> =
                            r["order"].as_array().cloned().unwrap_or_default().iter().filter_map(|x| x.as_str().map(str::to_string)).collect();
                        let mut sorted: Vec<Json> =
                            order.iter().filter_map(|n| list.iter().find(|c| c["name"].as_str() == Some(n)).cloned()).collect();
                        if sorted.len() != list.len() {
                            sorted = list.clone();
                        }
                        let at = order.iter().position(|n| *n == name).unwrap_or(sel);
                        d.values.insert("calculatedItems".into(), json!(sorted));
                        d.values.insert("selected".into(), json!(at));
                    }
                }
                if ui.button(lang.tr("Delete")).clicked() && !name.is_empty() {
                    let field = list.get(sel).and_then(|c| c["field"].as_str()).unwrap_or("").to_string();
                    if app.run("pivot.calculatedItem", json!({"pivot": pivot, "field": field, "name": name, "delete": true})).is_ok() {
                        let rest: Vec<Json> = list.iter().filter(|c| c["name"].as_str() != Some(name.as_str())).cloned().collect();
                        d.values.insert("calculatedItems".into(), json!(rest));
                        d.values.insert("selected".into(), json!(0));
                    }
                }
                if ui.button(lang.tr("Close")).clicked() {
                    *open = false;
                }
            });
        }
        "insertTimeline" => {
            let fields: Vec<(String, String)> = d
                .values
                .get("fields")
                .and_then(Json::as_array)
                .cloned()
                .unwrap_or_default()
                .iter()
                .filter_map(|x| x.as_str())
                .map(|x| (x.to_string(), x.to_string()))
                .collect();
            checklist(ui, d, "checked", &fields);
            crate::dialogs::ok_cancel(ui, confirm, open);
            if *confirm {
                let chosen = d.values.get("checked").cloned().unwrap_or(json!([]));
                if chosen.as_array().is_none_or(Vec::is_empty) {
                    return;
                }
                run(app, d, "insert.timeline", json!({"pivot": s("pivot", d), "fields": chosen}), confirm);
            }
        }
        "timelineConnections" => {
            let pivots: Vec<(String, String)> = d
                .values
                .get("pivots")
                .and_then(Json::as_array)
                .cloned()
                .unwrap_or_default()
                .iter()
                .map(|p| {
                    let n = p["name"].as_str().unwrap_or("").to_string();
                    (n.clone(), format!("{n}  ({})", p["sheet"].as_str().unwrap_or("")))
                })
                .collect();
            checklist(ui, d, "checked", &pivots);
            crate::dialogs::ok_cancel(ui, confirm, open);
            if *confirm {
                let chosen = d.values.get("checked").cloned().unwrap_or(json!([]));
                run(app, d, "timeline.reportConnections", json!({"timeline": s("timeline", d), "pivots": chosen}), confirm);
            }
        }
        _ => crate::dialogs::ok_cancel(ui, confirm, open),
    }
}

/// The Grouping dialog: Starting at, Ending at and By for numbers; for dates the same bounds,
/// the levels to group by and Number of days.
fn grouping(app: &mut SheetApp, ui: &mut Ui, d: &mut Dialog, confirm: &mut bool, open: &mut bool) {
    let lang = crate::i18n::current(ui.ctx());
    let date = d.values.get("kind").and_then(Json::as_str) == Some("date");
    ui.label(egui::RichText::new(lang.tr("Auto")).strong());
    for (auto, key, label) in [("autoStart", "startText", "Starting at:"), ("autoEnd", "endText", "Ending at:")] {
        ui.horizontal(|ui| {
            let mut a = d.values.get(auto).and_then(Json::as_bool).unwrap_or(true);
            if ui.checkbox(&mut a, lang.tr(label)).changed() {
                d.values.insert(auto.into(), json!(a));
            }
            ui.add_enabled_ui(!a, |ui| text_field(ui, d, key, 120.0));
        });
    }
    let levels_all = [
        ("seconds", "Seconds"),
        ("minutes", "Minutes"),
        ("hours", "Hours"),
        ("days", "Days"),
        ("months", "Months"),
        ("quarters", "Quarters"),
        ("years", "Years"),
    ];
    if date {
        ui.label(egui::RichText::new(lang.tr("By")).strong());
        let mut levels: Vec<String> = d
            .values
            .get("levels")
            .and_then(Json::as_array)
            .map(|a| a.iter().filter_map(|x| x.as_str().map(str::to_string)).collect())
            .unwrap_or_default();
        egui::Frame::group(ui.style()).show(ui, |ui| {
            for (v, label) in levels_all {
                let on = levels.iter().any(|x| x == v);
                if ui.selectable_label(on, lang.tr(label)).clicked() {
                    if on {
                        levels.retain(|x| x != v);
                    } else {
                        levels.push(v.to_string());
                    }
                }
            }
        });
        d.values.insert("levels".into(), json!(levels));
        let only_days = levels.len() == 1 && levels.first().is_some_and(|l| l == "days");
        ui.add_enabled_ui(only_days, |ui| {
            ui.horizontal(|ui| {
                ui.label(lang.tr("Number of days:"));
                let mut n = d.values.get("days").and_then(Json::as_f64).unwrap_or(1.0);
                if ui.add(egui::DragValue::new(&mut n).speed(1.0).range(1.0..=32767.0)).changed() {
                    d.values.insert("days".into(), json!(n.round()));
                }
            });
        });
    } else {
        ui.horizontal(|ui| {
            ui.label(lang.tr("By:"));
            let mut n = d.values.get("interval").and_then(Json::as_f64).unwrap_or(10.0);
            if ui.add(egui::DragValue::new(&mut n).speed(0.5)).changed() {
                d.values.insert("interval".into(), json!(n));
            }
        });
    }
    crate::dialogs::ok_cancel(ui, confirm, open);
    if !*confirm {
        return;
    }
    let s = |k: &str| d.values.get(k).and_then(Json::as_str).unwrap_or("").to_string();
    let auto = |k: &str| d.values.get(k).and_then(Json::as_bool).unwrap_or(true);
    let mut p = json!({"pivot": s("pivot"), "field": s("field")});
    if !auto("autoStart") {
        p["start"] = json!(text_of(d, "startText"));
    }
    if !auto("autoEnd") {
        p["end"] = json!(text_of(d, "endText"));
    }
    if date {
        let levels = d.values.get("levels").cloned().unwrap_or(json!([]));
        if levels.as_array().is_none_or(Vec::is_empty) {
            app.message = Some((d.title.clone(), "Select one or more time periods to group by.".into()));
            *confirm = false;
            return;
        }
        let days = d.values.get("days").and_then(Json::as_f64).unwrap_or(1.0);
        p["by"] = levels;
        if days > 1.0 {
            p["days"] = json!(days as u64);
        }
    } else {
        p["by"] = json!("range");
        p["interval"] = d.values.get("interval").cloned().unwrap_or(json!(10));
    }
    run(app, d, "pivot.group", p, confirm);
}

/// Insert Calculated Item: Name, Formula, the Fields and Items lists with Insert Item, and Add /
/// Delete for the field's calculated items.
fn calculated_item(app: &mut SheetApp, ui: &mut Ui, d: &mut Dialog, confirm: &mut bool, open: &mut bool) {
    let lang = crate::i18n::current(ui.ctx());
    let s = |k: &str, d: &Dialog| d.values.get(k).and_then(Json::as_str).unwrap_or("").to_string();
    let (pivot, field) = (s("pivot", d), s("field", d));
    ui.label(format!("{} \"{field}\"", lang.tr("Calculated item in")));
    let existing: Vec<Json> = d.values.get("calculatedItems").and_then(Json::as_array).cloned().unwrap_or_default();
    let mine: Vec<(String, String)> = existing
        .iter()
        .filter(|c| c["field"].as_str().is_some_and(|f| f.eq_ignore_ascii_case(&field)))
        .map(|c| (c["name"].as_str().unwrap_or("").to_string(), c["formula"].as_str().unwrap_or("").to_string()))
        .collect();
    ui.horizontal(|ui| {
        ui.label(lang.tr("Name:"));
        let mut name = s("name", d);
        egui::ComboBox::from_id_salt("calc_item_name").selected_text(name.clone()).width(160.0).show_ui(ui, |ui| {
            for (n, f) in &mine {
                if ui.selectable_label(*n == name, n.as_str()).clicked() {
                    name = n.clone();
                    d.values.insert("formula".into(), json!(f));
                }
            }
        });
        if ui.add(egui::TextEdit::singleline(&mut name).desired_width(140.0)).changed() || mine.iter().any(|(n, _)| *n == name) {
            d.values.insert("name".into(), json!(name));
        }
    });
    ui.horizontal(|ui| {
        ui.label(lang.tr("Formula:"));
        text_field(ui, d, "formula", 280.0);
    });
    let fields: Vec<String> =
        d.values.get("fields").and_then(Json::as_array).cloned().unwrap_or_default().iter().filter_map(|x| x.as_str().map(str::to_string)).collect();
    let items: Vec<String> =
        d.values.get("items").and_then(Json::as_array).cloned().unwrap_or_default().iter().filter_map(|x| x.as_str().map(str::to_string)).collect();
    let mut item_sel = d.values.get("itemSelected").and_then(Json::as_u64).unwrap_or(0) as usize;
    ui.columns(2, |cols| {
        if let Some(ui) = cols.get_mut(0) {
            ui.label(lang.tr("Fields:"));
            egui::ScrollArea::vertical().id_salt("calc_fields").max_height(140.0).show(ui, |ui| {
                for f in &fields {
                    if ui.selectable_label(f.eq_ignore_ascii_case(&field), f.as_str()).clicked() && !f.eq_ignore_ascii_case(&field) {
                        // Another field: the dialog for it (its items, its calculated items).
                        let _ = app.run("pivot.calculatedItem", json!({"pivot": pivot, "field": f}));
                        *open = false;
                    }
                }
            });
        }
        if let Some(ui) = cols.get_mut(1) {
            ui.label(lang.tr("Items:"));
            egui::ScrollArea::vertical().id_salt("calc_items").max_height(140.0).show(ui, |ui| {
                for (i, it) in items.iter().enumerate() {
                    if ui.selectable_label(i == item_sel, it.as_str()).clicked() {
                        item_sel = i;
                    }
                }
            });
        }
    });
    d.values.insert("itemSelected".into(), json!(item_sel));
    ui.horizontal(|ui| {
        if ui.button(lang.tr("Insert Item")).clicked()
            && let Some(it) = items.get(item_sel)
        {
            let plain = it.chars().all(|c| c.is_alphanumeric() || c == '_') && !it.chars().next().is_some_and(|c| c.is_ascii_digit());
            let name = if plain { it.clone() } else { format!("'{}'", it.replace('\'', "''")) };
            let mut f = text_of(d, "formula");
            if f.trim() == "= 0" || f.trim().is_empty() {
                f = "= ".into();
            }
            f.push_str(&name);
            d.values.insert("formula".into(), json!(f));
        }
        let name = s("name", d);
        let modify = mine.iter().any(|(n, _)| n.eq_ignore_ascii_case(&name));
        if ui.button(lang.tr(if modify { "Modify" } else { "Add" })).clicked() {
            let p = json!({"pivot": pivot, "field": field, "name": name, "formula": text_of(d, "formula")});
            match app.run("pivot.calculatedItem", p) {
                Ok(r) => {
                    d.values.insert("calculatedItems".into(), r["calculatedItems"].clone());
                }
                Err(e) => app.message = Some((d.title.clone(), crate::clean_error(&e))),
            }
        }
        if ui.add_enabled(modify, egui::Button::new(lang.tr("Delete"))).clicked()
            && let Ok(r) = app.run("pivot.calculatedItem", json!({"pivot": pivot, "field": field, "name": name, "delete": true}))
        {
            d.values.insert("calculatedItems".into(), r["calculatedItems"].clone());
        }
    });
    crate::dialogs::ok_cancel(ui, confirm, open);
    if *confirm {
        let name = s("name", d);
        let formula = text_of(d, "formula");
        let unchanged = mine.iter().any(|(n, f)| n.eq_ignore_ascii_case(&name) && f.trim() == formula.trim());
        if !name.trim().is_empty() && !unchanged && formula.trim() != "= 0" {
            run(app, d, "pivot.calculatedItem", json!({"pivot": pivot, "field": field, "name": name, "formula": formula}), confirm);
        }
    }
}
