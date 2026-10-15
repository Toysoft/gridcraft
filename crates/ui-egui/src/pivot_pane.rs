//! The PivotTable Fields task pane: shown on the right when the active cell is in a pivot
//! report. Ticking a field adds it (text → Rows, numbers → Values); each area lists its fields
//! with move/remove menus. Everything runs `pivot.*` commands.

use egui::{Color32, Ui, vec2};
use gridcraft_engine::model::PivotTable;
use serde_json::{Value as Json, json};

use crate::SheetApp;
use crate::i18n;
use crate::theme::{self, Tokens};

/// The PivotTable whose report holds the active cell (or whose anchor it is): the pane and the
/// PivotTable Analyze and Design tabs show while there is one.
pub fn at_active_cell(app: &SheetApp) -> Option<&PivotTable> {
    let d = app.session.active()?;
    let sh = d.wb.active()?;
    let a = d.selection.active;
    sh.pivots.iter().find(|p| p.last_range.is_some_and(|r| r.contains(a)) || p.anchor == a)
}

/// Whether the active cell is inside a pivot report (so the pane should show).
pub fn active_pivot(app: &SheetApp) -> Option<String> {
    at_active_cell(app).map(|p| p.name.clone())
}

pub fn show(app: &mut SheetApp, ui: &mut Ui) {
    // PivotTable Analyze › Show › Field List hides the pane.
    if !app.ui.pivot_field_list {
        return;
    }
    let Some(name) = active_pivot(app) else { return };
    let t = Tokens::get(ui.ctx());
    let lang = i18n::current(ui.ctx());
    let info = match app.session.run("pivot.fields", json!({"pivot": name})) {
        Ok(v) => v,
        Err(_) => return,
    };
    egui::Panel::right("pivot_pane").default_size(280.0).resizable(true).frame(egui::Frame::NONE.fill(t.ribbon).inner_margin(10)).show(ui, |ui| {
        ui.label(egui::RichText::new(lang.tr("PivotTable Fields")).font(theme::ui_bold(15.0)));
        ui.label(egui::RichText::new(lang.tr("Choose fields to add to report:")).small().color(t.text_dim));
        ui.add_space(4.0);
        let fields = info["fields"].as_array().cloned().unwrap_or_default();
        egui::ScrollArea::vertical().max_height(220.0).id_salt("pv_fields").show(ui, |ui| {
            for f in &fields {
                let fname = f["name"].as_str().unwrap_or("").to_string();
                let areas: Vec<String> =
                    f["areas"].as_array().map(|a| a.iter().filter_map(|x| x.as_str().map(str::to_string)).collect()).unwrap_or_default();
                let mut on = !areas.is_empty();
                let numeric = f["numeric"].as_bool().unwrap_or(false);
                let r = ui.checkbox(&mut on, egui::RichText::new(&fname).font(theme::ui_font(13.0)));
                if r.changed() {
                    let res = if on {
                        app.run("pivot.addField", json!({"pivot": name, "field": fname, "area": if numeric { "values" } else { "rows" }}))
                    } else {
                        app.run("pivot.removeField", json!({"pivot": name, "field": fname}))
                    };
                    if let Err(e) = res {
                        app.message = Some(("PivotTable".into(), crate::clean_error(&e)));
                    }
                }
                let samples: Vec<String> =
                    f["samples"].as_array().map(|a| a.iter().filter_map(|x| x.as_str().map(str::to_string)).collect()).unwrap_or_default();
                r.on_hover_text(samples.join(", "));
            }
        });
        ui.separator();
        ui.label(egui::RichText::new(lang.tr("Use a field's menu to move it between areas:")).small().color(t.text_dim));
        let area = |key: &str| -> Vec<String> {
            info["pivot"][key]
                .as_array()
                .or(info[key].as_array())
                .map(|a| {
                    a.iter()
                        // A row or column field's own filter is not a report filter.
                        .filter(|x| x["report"].as_bool() != Some(false))
                        .filter_map(|x| x["name"].as_str().or(x["field"].as_str()).or(x["sourceCol"].as_str()).or(x.as_str()).map(str::to_string))
                        .collect()
                })
                .unwrap_or_default()
        };
        // Σ Values (two or more value fields) sits in Rows or Columns; it moves only between them.
        const VALUES_FIELD: &str = "Σ Values";
        let values_key = if info["valuesOnRows"].as_bool().unwrap_or(false) { "rows" } else { "columns" };
        let values_at = info["valuesPosition"].as_u64().map(|p| p as usize);
        let area_with_values = |key: &str| -> Vec<String> {
            let mut v = area(key);
            if key == values_key && area("values").len() > 1 {
                let at = values_at.unwrap_or(v.len()).min(v.len());
                v.insert(at, VALUES_FIELD.to_string());
            }
            v
        };
        ui.columns(2, |cols| {
            for (i, (key, label)) in [("filters", "Filters"), ("columns", "Columns"), ("rows", "Rows"), ("values", "Values")].iter().enumerate() {
                let ui = &mut cols[i % 2];
                ui.label(egui::RichText::new(lang.tr(label)).strong());
                egui::Frame::NONE.fill(t.window).corner_radius(4.0).inner_margin(4.0).show(ui, |ui| {
                    ui.set_min_size(vec2(ui.available_width(), 60.0));
                    for f in area_with_values(key) {
                        let is_values = f == VALUES_FIELD;
                        let b = ui.add(
                            egui::Button::new(egui::RichText::new(format!("{f} ▾")).font(theme::ui_font(12.0)))
                                .fill(Color32::WHITE)
                                .min_size(vec2(ui.available_width(), 20.0)),
                        );
                        egui::Popup::menu(&b).show(|ui| {
                            for (to, l) in [
                                ("rows", "Move to Row Labels"),
                                ("columns", "Move to Column Labels"),
                                ("filters", "Move to Report Filter"),
                                ("values", "Move to Values"),
                            ] {
                                if to != *key && (!is_values || matches!(to, "rows" | "columns")) && ui.button(lang.tr(l)).clicked() {
                                    app.run_or_alert("pivot.moveField", json!({"pivot": name, "field": f, "from": key, "to": to}));
                                }
                            }
                            if is_values {
                                return;
                            }
                            if *key == "values" {
                                ui.separator();
                                for (func, l) in [
                                    ("sum", "Summarize by Sum"),
                                    ("count", "Summarize by Count"),
                                    ("average", "Summarize by Average"),
                                    ("max", "Summarize by Max"),
                                    ("min", "Summarize by Min"),
                                ] {
                                    if ui.button(lang.tr(l)).clicked() {
                                        app.run_or_alert("pivot.valueSettings", json!({"pivot": name, "field": f, "func": func}));
                                    }
                                }
                                for (sa, l) in [
                                    ("normal", "Show as No Calculation"),
                                    ("percentOfGrandTotal", "Show as % of Grand Total"),
                                    ("percentOfColumnTotal", "Show as % of Column Total"),
                                    ("percentOfRowTotal", "Show as % of Row Total"),
                                    ("percentOfParentRowTotal", "Show as % of Parent Row Total"),
                                    ("percentOfParentColumnTotal", "Show as % of Parent Column Total"),
                                    ("runningTotal", "Show as Running Total"),
                                    ("percentRunningTotal", "Show as % Running Total"),
                                    ("rankAscending", "Show as Rank Smallest to Largest"),
                                    ("rankDescending", "Show as Rank Largest to Smallest"),
                                    ("index", "Show as Index"),
                                ] {
                                    if ui.button(lang.tr(l)).clicked() {
                                        app.run_or_alert("pivot.valueSettings", json!({"pivot": name, "field": f, "showAs": sa}));
                                    }
                                }
                            } else {
                                ui.separator();
                                for (o, l) in [("asc", "Sort A to Z"), ("desc", "Sort Z to A")] {
                                    if ui.button(lang.tr(l)).clicked() {
                                        app.run_or_alert("pivot.sort", json!({"pivot": name, "field": f, "order": o}));
                                    }
                                }
                                for (g, l) in [
                                    ("years", "Group by Years"),
                                    ("quarters", "Group by Quarters"),
                                    ("months", "Group by Months"),
                                    ("range", "Group by Ranges of 10"),
                                    ("none", "Ungroup"),
                                ] {
                                    if ui.button(lang.tr(l)).clicked() {
                                        app.run_or_alert("pivot.group", json!({"pivot": name, "field": f, "by": g}));
                                    }
                                }
                                ui.separator();
                                if ui.button(format!("{} \"{f}\"", lang.tr("Clear Filter From"))).clicked() {
                                    app.run_or_alert("pivot.clearFilter", json!({"pivot": name, "field": f}));
                                }
                                if *key != "filters" {
                                    // Label Filters ▸ and Value Filters ▸, as on the report's field button.
                                    ui.menu_button(lang.tr("Label Filters"), |ui| {
                                        for (op, l, _) in crate::pivot_ui::LABEL_OPS {
                                            if ui.button(lang.tr(l)).clicked() {
                                                app.open_dialog("pivotLabelFilter", json!({"pivot": name, "field": f, "op": op}));
                                            }
                                        }
                                    });
                                    ui.menu_button(lang.tr("Value Filters"), |ui| {
                                        for op in crate::pivot_ui::VALUE_OPS {
                                            let label = crate::pivot_ui::LABEL_OPS.iter().find(|(o, ..)| *o == op).map_or(op, |(_, l, _)| l);
                                            if ui.button(lang.tr(label)).clicked() {
                                                app.open_dialog("pivotValueFilter", json!({"pivot": name, "field": f, "op": op}));
                                            }
                                        }
                                        ui.separator();
                                        if ui.button(lang.tr("Top 10…")).clicked() {
                                            app.open_dialog("pivotTop10", json!({"pivot": name, "field": f}));
                                        }
                                    });
                                }
                            }
                            ui.separator();
                            if ui.button(lang.tr("Remove Field")).clicked() {
                                app.run_or_alert("pivot.removeField", json!({"pivot": name, "field": f, "area": key}));
                            }
                        });
                    }
                });
            }
        });
        ui.separator();
        ui.horizontal(|ui| {
            if ui.button(lang.tr("Refresh")).clicked() {
                app.run_or_alert("pivot.refresh", json!({"pivot": name}));
            }
            if ui.button(lang.tr("Calculated Field…")).clicked() {
                app.run_or_alert("pivot.calculatedField", json!({"pivot": name}));
            }
            egui::ComboBox::from_id_salt("pv_layout").selected_text(lang.tr("Report Layout")).show_ui(ui, |ui| {
                for (l, v) in [("Compact", "compact"), ("Outline", "outline"), ("Tabular", "tabular")] {
                    if ui.button(lang.tr(l)).clicked() {
                        app.run_or_alert("pivot.layout", json!({"pivot": name, "layout": v}));
                    }
                }
            });
        });
        let _: Option<Json> = None;
    });
}
