//! The PivotTable contextual tabs, PivotTable Analyze and PivotTable Design, shown while the
//! active cell is in a PivotTable report. Groups, controls and menu items follow Excel 365 in
//! order and control type. A control whose feature GridCraft doesn't have yet keeps its place,
//! disabled, with the reason on hover: wiring the feature in later only touches that control's
//! group function.

use egui::{Color32, Rect, RichText, Sense, Stroke, StrokeKind, Ui, pos2, vec2};
use gridcraft_engine::model::{PivotDateGroup, PivotFunc, PivotLayout, PivotShowAs, PivotTable, ReportPart, Workbook};
use serde_json::{Value as Json, json};

use super::{act, menu_items, sep};
use crate::SheetApp;
use crate::icons::Icon;
use crate::theme::{self, Tokens};
use crate::widgets::{big_button, small_button};

/// Excel's names of the two tabs (German Excel: "PivotTable-Analyse" and "Entwurf").
pub const ANALYZE: &str = "PivotTable Analyze";
pub const DESIGN: &str = "PivotTable Design";

/// Why a control is disabled (shown on hover).
const NOT_YET: &str = "GridCraft can't do this yet.";
const OLAP: &str = "Only for PivotTables based on OLAP or the Data Model, which GridCraft doesn't have.";
const NO_FIELD: &str = "Select an item or a value of the PivotTable first.";

/// The PivotTable at the active cell, copied so the tab can run commands while drawing.
fn active(app: &SheetApp) -> Option<PivotTable> {
    crate::pivot_pane::at_active_cell(app).cloned()
}

fn pivot_param(pt: &PivotTable) -> Json {
    json!({"pivot": pt.name})
}

/// A large ribbon button named by its (translated) tooltip, so tests and screen readers find it.
fn big(ui: &mut Ui, icon: Icon, label: &str, tip: &str, dropdown: bool) -> egui::Response {
    let r = big_button(ui, icon, label, tip, dropdown);
    named(ui, r, tip)
}

/// A small ribbon button named by its tooltip (see [`big`]).
fn small(ui: &mut Ui, icon: Icon, label: &str, tip: &str, dropdown: bool) -> egui::Response {
    let r = small_button(ui, icon, label, tip, dropdown);
    named(ui, r, tip)
}

fn named(ui: &Ui, r: egui::Response, tip: &str) -> egui::Response {
    let name = crate::i18n::current(ui.ctx()).tr(tip).to_string();
    let enabled = ui.is_enabled();
    r.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, enabled, &name));
    r
}

/// A control Excel has here that GridCraft can't run yet: greyed out, with `why` on hover.
fn unavailable(ui: &mut Ui, why: &str, add: impl FnOnce(&mut Ui) -> egui::Response) {
    let why = crate::i18n::current(ui.ctx()).tr(why);
    ui.add_enabled_ui(false, add).inner.on_disabled_hover_text(why);
}

/// A menu item GridCraft can't run yet: greyed out, with `why` on hover.
fn unavailable_item(ui: &mut Ui, label: &str, why: &str) {
    let lang = crate::i18n::current(ui.ctx());
    ui.add_enabled(false, egui::Button::new(lang.tr(label)).frame(false).min_size(vec2(220.0, 22.0))).on_disabled_hover_text(lang.tr(why));
}

/// Menu items of which one describes the current state (ticked, like Excel's highlighted
/// choice): `(label, command, params, current)`.
fn choice_items(app: &mut SheetApp, ui: &mut Ui, items: &[(&str, &str, Json, bool)]) {
    let lang = app.ui.language;
    for (label, id, params, current) in items {
        let text = if *current { format!("✓ {}", lang.tr(label)) } else { format!("    {}", lang.tr(label)) };
        if ui.add(egui::Button::new(text).frame(false).min_size(vec2(260.0, 22.0))).clicked() {
            act(app, id, params.clone());
            ui.close();
        }
    }
}

/// Marks a toggle button that is on.
fn toggled(ui: &Ui, r: &egui::Response, on: bool) {
    if on {
        ui.painter().rect_stroke(r.rect, 4.0, Stroke::new(1.0, Tokens::get(ui.ctx()).accent), StrokeKind::Inside);
    }
}

// ---------------------------------------------------------------- PivotTable Analyze

/// PivotTable Analyze: PivotTable, Active Field, Group, Filter, Data, Actions, Calculations,
/// Tools and Show.
pub fn analyze(app: &mut SheetApp, ui: &mut Ui) {
    let Some(pt) = active(app) else { return };
    pivottable_group(app, ui, &pt);
    sep(ui);
    active_field_group(app, ui, &pt);
    sep(ui);
    group_group(app, ui, &pt);
    sep(ui);
    filter_group(app, ui, &pt);
    sep(ui);
    data_group(app, ui, &pt);
    sep(ui);
    actions_group(app, ui, &pt);
    sep(ui);
    calculations_group(app, ui, &pt);
    sep(ui);
    tools_group(app, ui, &pt);
    sep(ui);
    show_group(app, ui, &pt);
}

/// PivotTable: the PivotTable's name and the Options menu.
fn pivottable_group(app: &mut SheetApp, ui: &mut Ui, pt: &PivotTable) {
    let lang = app.ui.language;
    ui.vertical(|ui| {
        ui.label(RichText::new(lang.tr("PivotTable Name:")).small());
        let mut name = pt.name.clone();
        ui.add_enabled(false, egui::TextEdit::singleline(&mut name).desired_width(96.0))
            .on_disabled_hover_text(lang.tr("Renaming a PivotTable isn't available in GridCraft yet."));
        let options = small(ui, Icon::Settings, "Options", "PivotTable Options", true);
        egui::Popup::menu(&options).show(|ui| {
            unavailable_item(ui, "Options…", NOT_YET);
            unavailable_item(ui, "Show Report Filter Pages…", NOT_YET);
            // A setting of GridCraft, as in Excel: clicking a value of a PivotTable while
            // typing a formula inserts GETPIVOTDATA instead of the cell's reference.
            let on = app.session.prefs.generate_get_pivot_data;
            let text = if on { format!("✓ {}", lang.tr("Generate GetPivotData")) } else { format!("    {}", lang.tr("Generate GetPivotData")) };
            if ui.add(egui::Button::new(text).frame(false).min_size(vec2(220.0, 22.0))).clicked() {
                act(app, "pivot.generateGetPivotData", json!({"on": !on}));
                ui.close();
            }
        });
    });
}

/// What the active cell of a PivotTable stands for, for the Active Field group.
#[derive(Clone, Debug, PartialEq)]
enum ActiveField {
    /// A value field: its caption ("Sum of Sales").
    Value(String),
    /// A row or column field: its name, and whether it's grouped (by dates or number ranges, or a
    /// Group Selection field).
    Axis(String, bool),
}

/// The field of the active cell: a value cell's value field, or the row or column field of an
/// item label (read off the report's index).
fn active_field_of(app: &SheetApp, pt: &PivotTable) -> Option<ActiveField> {
    let report = pt.report.get()?;
    let origin = pt.last_range?.start;
    let a = app.session.active()?.selection.active;
    let part = report.part_at(a.row.checked_sub(origin.row)?, a.col.checked_sub(origin.col)?)?;
    let axis = |fields: &[gridcraft_engine::model::ReportField], defs: &[gridcraft_engine::model::PivotField], f: u32| {
        let name = fields.get(f as usize)?.name.clone();
        let grouped = pt.group_field(&name).is_some()
            || defs
                .iter()
                .find(|d| d.source_col.eq_ignore_ascii_case(&name))
                .is_some_and(|d| d.date_group != PivotDateGroup::None || d.number_group.is_some());
        Some(ActiveField::Axis(name, grouped))
    };
    match part {
        ReportPart::Data(d) => Some(ActiveField::Value(report.data_fields.get(d as usize)?.name.clone())),
        ReportPart::RowField(f) => axis(&report.row_fields, &pt.rows, f),
        ReportPart::ColField(f) => axis(&report.col_fields, &pt.columns, f),
    }
}

/// `pivot.valueSettings`'s name for a summary function.
fn func_key(f: PivotFunc) -> &'static str {
    match f {
        PivotFunc::Sum => "sum",
        PivotFunc::Count => "count",
        PivotFunc::Average => "average",
        PivotFunc::Max => "max",
        PivotFunc::Min => "min",
        PivotFunc::Product => "product",
        PivotFunc::CountNumbers => "countNumbers",
        PivotFunc::StdDev => "stdDev",
        PivotFunc::StdDevP => "stdDevP",
        PivotFunc::Var => "var",
        PivotFunc::VarP => "varP",
    }
}

/// `pivot.valueSettings`'s name for a Show Values As choice.
fn show_as_key(v: PivotShowAs) -> &'static str {
    match v {
        PivotShowAs::Normal => "normal",
        PivotShowAs::PercentOfGrandTotal => "percentOfGrandTotal",
        PivotShowAs::PercentOfColumnTotal => "percentOfColumnTotal",
        PivotShowAs::PercentOfRowTotal => "percentOfRowTotal",
        PivotShowAs::RunningTotal => "runningTotal",
        PivotShowAs::Rank => "rankDescending",
        PivotShowAs::PercentOf => "percentOf",
        PivotShowAs::PercentOfParentRowTotal => "percentOfParentRowTotal",
        PivotShowAs::PercentOfParentColumnTotal => "percentOfParentColumnTotal",
        PivotShowAs::PercentOfParentTotal => "percentOfParentTotal",
        PivotShowAs::DifferenceFrom => "differenceFrom",
        PivotShowAs::PercentDifferenceFrom => "percentDifferenceFrom",
        PivotShowAs::PercentRunningTotal => "percentRunningTotal",
        PivotShowAs::RankAscending => "rankAscending",
        PivotShowAs::Index => "index",
    }
}

/// Active Field: the field's name and settings, drilling (OLAP only), expanding and collapsing.
fn active_field_group(app: &mut SheetApp, ui: &mut Ui, pt: &PivotTable) {
    let lang = app.ui.language;
    let field = active_field_of(app, pt);
    ui.vertical(|ui| {
        ui.label(RichText::new(lang.tr("Active Field:")).small());
        let mut text = match &field {
            Some(ActiveField::Value(n) | ActiveField::Axis(n, _)) => n.clone(),
            None => String::new(),
        };
        ui.add_enabled(false, egui::TextEdit::singleline(&mut text).desired_width(96.0));
        match &field {
            Some(ActiveField::Value(caption)) => {
                if small(ui, Icon::Settings, "Field Settings", "Field Settings", false).clicked() {
                    let v = pt.values.iter().find(|v| v.name == *caption);
                    let mut params = json!({"pivot": pt.name, "field": caption, "name": caption});
                    if let (Some(v), Some(m)) = (v, params.as_object_mut()) {
                        m.insert("func".into(), json!(func_key(v.func)));
                        m.insert("showAs".into(), json!(show_as_key(v.show_as)));
                        m.insert("baseField".into(), json!(v.base_field.clone().unwrap_or_default()));
                        m.insert("baseItem".into(), json!(v.base_item.clone().unwrap_or_default()));
                    }
                    app.open_dialog("pivotValueFieldSettings", params);
                }
            }
            Some(ActiveField::Axis(..)) => unavailable(ui, NOT_YET, |ui| small(ui, Icon::Settings, "Field Settings", "Field Settings", false)),
            None => unavailable(ui, NO_FIELD, |ui| small(ui, Icon::Settings, "Field Settings", "Field Settings", false)),
        }
    });
    unavailable(ui, OLAP, |ui| big(ui, Icon::Zoom, "Drill\nDown", "Drill Down", false));
    unavailable(ui, OLAP, |ui| big(ui, Icon::Zoom, "Drill\nUp", "Drill Up", false));
    ui.vertical(|ui| {
        let axis = match &field {
            Some(ActiveField::Axis(name, _)) => Some(name.clone()),
            _ => None,
        };
        for (icon, label, collapse) in [(Icon::Plus, "Expand Field", false), (Icon::Minus, "Collapse Field", true)] {
            match &axis {
                Some(name) => {
                    if small(ui, icon, label, label, false).clicked() {
                        act(app, "pivot.collapse", json!({"pivot": pt.name, "field": name, "collapse": collapse}));
                    }
                }
                None => unavailable(ui, NO_FIELD, |ui| small(ui, icon, label, label, false)),
            }
        }
    });
}

/// Group: Group Selection, Ungroup and Group Field.
fn group_group(app: &mut SheetApp, ui: &mut Ui, pt: &PivotTable) {
    let field = active_field_of(app, pt);
    ui.vertical(|ui| {
        // Group Selection takes the selected items of a row or column field (one item of a
        // number or date field opens the Grouping dialog, as in Excel).
        match &field {
            Some(ActiveField::Axis(..)) => {
                if small(ui, Icon::Group, "Group Selection", "Group Selection", false).clicked() {
                    act(app, "pivot.groupSelection", pivot_param(pt));
                }
            }
            _ => unavailable(ui, NO_FIELD, |ui| small(ui, Icon::Group, "Group Selection", "Group Selection", false)),
        }
        // Ungroup: the group at the active cell, or the field's date or number grouping.
        match &field {
            Some(ActiveField::Axis(_, true)) => {
                if small(ui, Icon::Ungroup, "Ungroup", "Ungroup", false).clicked() {
                    act(app, "pivot.ungroup", pivot_param(pt));
                }
            }
            _ => unavailable(ui, "Select an item of a grouped field first.", |ui| small(ui, Icon::Ungroup, "Ungroup", "Ungroup", false)),
        }
        // Group Field: the Grouping dialog for the field at the active cell.
        match &field {
            Some(ActiveField::Axis(..)) => {
                if small(ui, Icon::Group, "Group Field", "Group Field", false).clicked() {
                    act(app, "pivot.group", pivot_param(pt));
                }
            }
            _ => unavailable(ui, NO_FIELD, |ui| small(ui, Icon::Group, "Group Field", "Group Field", false)),
        }
    });
}

/// Filter: slicers, timelines and their connections.
fn filter_group(app: &mut SheetApp, ui: &mut Ui, pt: &PivotTable) {
    ui.vertical(|ui| {
        unavailable(ui, NOT_YET, |ui| small(ui, Icon::Filter, "Insert Slicer", "Insert Slicer", false));
        if small(ui, Icon::Calendar, "Insert Timeline", "Insert Timeline", false).clicked() {
            act(app, "pivot.insertTimeline", pivot_param(pt));
        }
        unavailable(ui, NOT_YET, |ui| small(ui, Icon::Link, "Filter Connections", "Filter Connections", false));
    });
}

/// Data: Refresh and Change Data Source.
fn data_group(app: &mut SheetApp, ui: &mut Ui, pt: &PivotTable) {
    let refresh = big(ui, Icon::Redo, "Refresh", "Refresh (Alt+F5)", true);
    egui::Popup::menu(&refresh).show(|ui| {
        menu_items(app, ui, &[("Refresh", "pivot.refresh", pivot_param(pt)), ("Refresh All", "data.refreshAll", json!({}))]);
        unavailable_item(ui, "Refresh Status", NOT_YET);
        unavailable_item(ui, "Cancel Refresh", NOT_YET);
        unavailable_item(ui, "Connection Properties…", NOT_YET);
    });
    let source = big(ui, Icon::Table, "Change Data\nSource", "Change Data Source", true);
    egui::Popup::menu(&source).show(|ui| {
        let lang = app.ui.language;
        if ui.add(egui::Button::new(lang.tr("Change Data Source…")).frame(false).min_size(vec2(220.0, 22.0))).clicked() {
            app.open_dialog("pivotChangeSource", json!({"pivot": pt.name, "source": pt.source}));
            ui.close();
        }
        unavailable_item(ui, "Connection Properties…", NOT_YET);
    });
}

/// Actions: Clear, Select and Move PivotTable.
fn actions_group(app: &mut SheetApp, ui: &mut Ui, pt: &PivotTable) {
    ui.vertical(|ui| {
        let clear = small(ui, Icon::Eraser, "Clear", "Clear", true);
        egui::Popup::menu(&clear).show(|ui| {
            menu_items(
                app,
                ui,
                &[("PivotTable|Clear All", "pivot.clearAll", pivot_param(pt)), ("Clear Filters", "pivot.clearFilters", pivot_param(pt))],
            );
        });
        let select = small(ui, Icon::Checkbox, "Select", "Select", true);
        egui::Popup::menu(&select).show(|ui| {
            // The body (without report filters) and the whole report.
            let whole = pt.last_range;
            let body = whole.map(|r| gridcraft_engine::core::RangeRef::new(pt.anchor, r.end));
            let pick = |app: &mut SheetApp, ui: &mut Ui, label: &str, range: Option<gridcraft_engine::core::RangeRef>| {
                let lang = app.ui.language;
                let resp = ui.add_enabled(range.is_some(), egui::Button::new(lang.tr(label)).frame(false).min_size(vec2(220.0, 22.0)));
                if resp.clicked()
                    && let Some(r) = range
                {
                    act(app, "selection.set", json!({"range": r.a1()}));
                    ui.close();
                }
            };
            pick(app, ui, "Labels and Values", body);
            // The value cells, totals included.
            let values = pt.report.get().and_then(|r| r.data_area()).zip(whole).and_then(|((r0, c0, r1, c1), w)| {
                let at = |r: u32, c: u32| Some(gridcraft_engine::core::CellRef::new(w.start.row.checked_add(r)?, w.start.col.checked_add(c)?));
                Some(gridcraft_engine::core::RangeRef::new(at(r0, c0)?, at(r1, c1)?))
            });
            pick(app, ui, "Values", values);
            unavailable_item(ui, "Labels", NOT_YET);
            ui.separator();
            pick(app, ui, "Entire PivotTable", whole);
            ui.separator();
            unavailable_item(ui, "Enable Selection", NOT_YET);
        });
        unavailable(ui, NOT_YET, |ui| small(ui, Icon::Right, "Move PivotTable", "Move PivotTable", false));
    });
}

/// Calculations: Fields, Items, & Sets; OLAP Tools; Relationships.
fn calculations_group(app: &mut SheetApp, ui: &mut Ui, pt: &PivotTable) {
    let fis = big(ui, Icon::Function, "Fields, Items,\n& Sets", "Fields, Items, & Sets", true);
    egui::Popup::menu(&fis).show(|ui| {
        menu_items(
            app,
            ui,
            &[
                ("Calculated Field…", "pivot.calculatedField", pivot_param(pt)),
                ("Calculated Item…", "pivot.calculatedItem", pivot_param(pt)),
                ("Solve Order…", "pivot.solveOrder", pivot_param(pt)),
                ("List Formulas", "pivot.listFormulas", pivot_param(pt)),
            ],
        );
        ui.separator();
        unavailable_item(ui, "Create Set Based on Row Items…", OLAP);
        unavailable_item(ui, "Create Set Based on Column Items…", OLAP);
        ui.separator();
        unavailable_item(ui, "Manage Sets…", OLAP);
    });
    // Excel stacks these two once the window is narrower than the whole tab.
    ui.vertical(|ui| {
        unavailable(ui, OLAP, |ui| small(ui, Icon::Calc, "OLAP Tools", "OLAP Tools", true));
        unavailable(ui, OLAP, |ui| small(ui, Icon::Link, "Relationships", "Relationships", false));
    });
}

/// Tools: PivotChart and Recommended PivotTables.
fn tools_group(app: &mut SheetApp, ui: &mut Ui, pt: &PivotTable) {
    if big(ui, Icon::ChartBar, "PivotChart", "PivotChart", false).clicked() {
        act(app, "insert.pivotChart", pivot_param(pt));
    }
    // Excel opens a dialog of suggestions for the PivotTable's source; here they are a menu.
    let rec = big(ui, Icon::PivotTable, "Recommended\nPivotTables", "Recommended PivotTables", true);
    let key = rec.id.with("suggestions");
    if rec.clicked() {
        let list = app.session.run("insert.recommendedPivotTables", json!({"source": pt.source})).ok().and_then(|r| r.get("suggestions").cloned());
        ui.data_mut(|d| d.insert_temp(key, list.unwrap_or_default()));
    }
    egui::Popup::menu(&rec).show(|ui| {
        let list: Json = ui.data(|d| d.get_temp(key)).unwrap_or_default();
        let items = list.as_array().cloned().unwrap_or_default();
        if items.is_empty() {
            unavailable_item(ui, "No recommendations for this data", NOT_YET);
        }
        for (i, s) in items.iter().enumerate() {
            let title = s.get("title").and_then(Json::as_str).unwrap_or_default();
            if ui.add(egui::Button::new(title).frame(false).min_size(vec2(240.0, 22.0))).clicked() {
                act(app, "insert.recommendedPivotTables", json!({"source": pt.source, "apply": i}));
                ui.close();
            }
        }
    });
}

/// Show: Field List, +/- Buttons and Field Headers, toggles stacked as in Excel's narrower
/// ribbon.
fn show_group(app: &mut SheetApp, ui: &mut Ui, pt: &PivotTable) {
    ui.vertical(|ui| {
        let on = app.ui.pivot_field_list;
        let list = small(ui, Icon::SheetList, "Field List", "Field List", false);
        toggled(ui, &list, on);
        if list.clicked() {
            act(app, "pivot.showFieldList", json!({"on": !on}));
        }
        unavailable(ui, NOT_YET, |ui| small(ui, Icon::Plus, "+/- Buttons", "+/- Buttons", false));
        let headers = small(ui, Icon::Header, "Field Headers", "Field Headers", false);
        toggled(ui, &headers, pt.show_headers);
        if headers.clicked() {
            act(app, "pivot.layout", json!({"pivot": pt.name, "showHeaders": !pt.show_headers}));
        }
    });
}

// ---------------------------------------------------------------- PivotTable Design

/// PivotTable Design: Layout, PivotTable Style Options and PivotTable Styles.
pub fn design(app: &mut SheetApp, ui: &mut Ui) {
    let Some(pt) = active(app) else { return };
    layout_group(app, ui, &pt);
    sep(ui);
    style_options_group(app, ui);
    sep(ui);
    styles_group(app, ui, &pt);
}

/// Layout: Subtotals, Grand Totals, Report Layout and Blank Rows.
fn layout_group(app: &mut SheetApp, ui: &mut Ui, pt: &PivotTable) {
    let p = |extra: Json| {
        let mut v = pivot_param(pt);
        if let (Some(m), Some(e)) = (v.as_object_mut(), extra.as_object()) {
            m.extend(e.clone());
        }
        v
    };
    let tabular = pt.layout == PivotLayout::Tabular;
    let subtotals = big(ui, Icon::Subtotal, "Subtotals", "Subtotals", true);
    egui::Popup::menu(&subtotals).show(|ui| {
        choice_items(
            app,
            ui,
            &[
                ("Do Not Show Subtotals", "pivot.subtotals", p(json!({"mode": "none"})), !pt.subtotals),
                ("Show all Subtotals at Bottom of Group", "pivot.subtotals", p(json!({"mode": "bottom"})), pt.subtotals && !pt.subtotals_top),
            ],
        );
        // Tabular form has no room for a subtotal above its group.
        if tabular {
            unavailable_item(ui, "Show all Subtotals at Top of Group", "Not available in Tabular Form.");
        } else {
            choice_items(
                app,
                ui,
                &[("Show all Subtotals at Top of Group", "pivot.subtotals", p(json!({"mode": "top"})), pt.subtotals && pt.subtotals_top)],
            );
        }
        ui.separator();
        unavailable_item(ui, "Include Filtered Items in Totals", NOT_YET);
    });
    let grand = big(ui, Icon::Sum, "Grand\nTotals", "Grand Totals", true);
    egui::Popup::menu(&grand).show(|ui| {
        let (r, c) = (pt.grand_totals_rows, pt.grand_totals_cols);
        choice_items(
            app,
            ui,
            &[
                ("Off for Rows and Columns", "pivot.grandTotals", p(json!({"mode": "off"})), !r && !c),
                ("On for Rows and Columns", "pivot.grandTotals", p(json!({"mode": "on"})), r && c),
                ("On for Rows Only", "pivot.grandTotals", p(json!({"mode": "rows"})), r && !c),
                ("On for Columns Only", "pivot.grandTotals", p(json!({"mode": "columns"})), !r && c),
            ],
        );
    });
    let report = big(ui, Icon::Table, "Report\nLayout", "Report Layout", true);
    egui::Popup::menu(&report).show(|ui| {
        choice_items(
            app,
            ui,
            &[
                ("Show in Compact Form", "pivot.reportLayout", p(json!({"layout": "compact"})), pt.layout == PivotLayout::Compact),
                ("Show in Outline Form", "pivot.reportLayout", p(json!({"layout": "outline"})), pt.layout == PivotLayout::Outline),
                ("Show in Tabular Form", "pivot.reportLayout", p(json!({"layout": "tabular"})), tabular),
            ],
        );
        ui.separator();
        unavailable_item(ui, "Repeat All Item Labels", NOT_YET);
        unavailable_item(ui, "Do Not Repeat Item Labels", NOT_YET);
    });
    let blank = big(ui, Icon::Insert, "Blank\nRows", "Blank Rows", true);
    egui::Popup::menu(&blank).show(|ui| {
        unavailable_item(ui, "Insert Blank Line after Each Item", NOT_YET);
        unavailable_item(ui, "Remove Blank Line after Each Item", NOT_YET);
    });
}

/// PivotTable Style Options: shown with Excel's defaults; GridCraft's styles don't vary by them.
fn style_options_group(app: &mut SheetApp, ui: &mut Ui) {
    let lang = app.ui.language;
    for (i, column) in
        [[("Row Headers", true), ("Column Headers", true)], [("Banded Rows", false), ("Banded Columns", false)]].into_iter().enumerate()
    {
        if i > 0 {
            ui.add_space(8.0);
        }
        ui.vertical(|ui| {
            for (label, on) in column {
                let mut on = on;
                ui.add_enabled(false, egui::Checkbox::new(&mut on, lang.tr(label))).on_disabled_hover_text(lang.tr(NOT_YET));
            }
        });
    }
}

/// PivotTable Styles: the gallery of `PivotStyleLight1…28`, `Medium1…28` and `Dark1…28`.
fn styles_group(app: &mut SheetApp, ui: &mut Ui, pt: &PivotTable) {
    let st = big(ui, Icon::FormatTable, "PivotTable\nStyles", "PivotTable Styles", true);
    egui::Popup::menu(&st).show(|ui| style_gallery(app, ui, pt));
}

fn style_gallery(app: &mut SheetApp, ui: &mut Ui, pt: &PivotTable) {
    ui.set_width(430.0);
    let Some(wb) = app.session.active().map(|d| d.wb.clone()) else { return };
    let lang = app.ui.language;
    egui::ScrollArea::vertical().max_height(420.0).show(ui, |ui| {
        for fam in ["Light", "Medium", "Dark"] {
            ui.label(RichText::new(lang.tr(fam)).strong());
            egui::Grid::new(("pivot_styles", fam)).spacing(vec2(4.0, 4.0)).show(ui, |ui| {
                for i in 1..=28u32 {
                    let name = format!("PivotStyle{fam}{i}");
                    let (r, resp) = ui.allocate_exact_size(vec2(54.0, 38.0), Sense::click());
                    resp.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, &name));
                    paint_style_swatch(ui, r, &wb, &name);
                    if name.eq_ignore_ascii_case(&pt.style) || resp.hovered() {
                        ui.painter().rect_stroke(r.expand(1.5), 2.0, Stroke::new(2.0, Color32::from_rgb(0xE8, 0x7D, 0x2B)), StrokeKind::Outside);
                    }
                    if resp.on_hover_text(&name).clicked() {
                        act(app, "pivot.style", json!({"pivot": pt.name, "style": name}));
                        ui.close();
                    }
                    if i % 7 == 0 {
                        ui.end_row();
                    }
                }
            });
        }
    });
}

/// A small picture of a report in a PivotTable style: header band, item rows, total row.
fn paint_style_swatch(ui: &Ui, r: Rect, wb: &Workbook, style: &str) {
    let lk = gridcraft_engine::pivot::look(style);
    let res = |c: gridcraft_engine::model::Color| c.resolve(&wb.theme).map(theme::color32);
    let p = ui.painter();
    p.rect_filled(r, 0.0, Color32::WHITE);
    let rows = 5;
    let rh = r.height() / rows as f32;
    for i in 0..rows {
        let rr = Rect::from_min_size(pos2(r.left(), r.top() + i as f32 * rh), vec2(r.width(), rh));
        if i == 0
            && let Some(f) = res(lk.header_fill)
        {
            p.rect_filled(rr, 0.0, f);
        }
        let line = res(lk.line).unwrap_or(Color32::from_gray(160));
        if i == 0 || i == rows - 2 {
            p.line_segment([rr.left_bottom(), rr.right_bottom()], Stroke::new(1.0, line));
        }
        for k in 0..2 {
            let x = rr.left() + 4.0 + k as f32 * (r.width() - 8.0) / 2.0;
            let col = if i == 0 { res(lk.header_font).unwrap_or(Color32::BLACK) } else { Color32::from_gray(120) };
            p.line_segment([pos2(x, rr.center().y), pos2(x + 14.0, rr.center().y)], Stroke::new(1.0, col));
        }
    }
    p.rect_stroke(r, 0.0, Stroke::new(1.0, Color32::from_gray(210)), StrokeKind::Inside);
}
