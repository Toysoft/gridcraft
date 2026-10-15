//! Data › What-If Analysis › Data Table…: the dialog makes a live `{=TABLE()}` over the selection,
//! the formula bar and the cell editor show the table's formula, and results can't be edited one
//! by one.

use egui::{accesskit::Role, vec2};
use egui_kittest::kittest::Queryable;
use gridcraft_engine::Session;
use gridcraft_ui_egui::SheetApp;
use gridcraft_ui_egui::control::{Outcome, handle};
use serde_json::{Value as Json, json};

type Harness = egui_kittest::Harness<'static, SheetApp>;

/// Rate B1, amount B2; amounts down D3:D5 and the formula `=B2*(1+B1)` in E2; D2:E5 selected.
fn harness() -> Harness {
    let mut session = Session::new();
    session.new_workbook();
    session.execute("range.setValues", json!({"range": "B1", "values": [[0.05], [1000]]})).unwrap();
    session.execute("range.setValues", json!({"range": "D3", "values": [[100], [200], [300]]})).unwrap();
    session.execute("cell.set", json!({"cell": "E2", "input": "=B2*(1+B1)"})).unwrap();
    session.execute("selection.set", json!({"range": "D2:E5"})).unwrap();
    let mut h = egui_kittest::Harness::builder().with_size(vec2(1200.0, 800.0)).build_ui_state(
        |ui, app: &mut SheetApp| {
            app.logic(&ui.ctx().clone());
            app.ui(ui);
        },
        SheetApp::new(session, Default::default()),
    );
    h.run_steps(6);
    h
}

fn control(h: &mut Harness, method: &str, params: Json) -> Json {
    let ctx = h.ctx.clone();
    match handle(h.state_mut(), &ctx, method, &params) {
        Outcome::Done(v) => v,
        Outcome::Screenshot { .. } => json!(null),
    }
}

fn value(h: &mut Harness, cell: &str) -> Json {
    h.state_mut().run("cell.get", json!({"cell": cell})).unwrap()["value"].clone()
}

fn tables(h: &Harness) -> usize {
    h.state().session.active().unwrap().wb.active().unwrap().data_tables.len()
}

#[test]
fn the_dialog_makes_a_live_table_over_the_selection() {
    let mut h = harness();
    h.state_mut().open_dialog("dataTable", json!({}));
    h.run_steps(6);
    // Excel's two fields; the column input cell receives the values down the left column.
    assert!(h.query_by_role_and_label(Role::TextInput, "Row input cell:").is_some());
    h.get_by_role_and_label(Role::TextInput, "Column input cell:").click();
    h.run_steps(2);
    h.get_by_role_and_label(Role::TextInput, "Column input cell:").type_text("$B$2");
    h.run_steps(4);
    h.get_by_label("   OK   ").click();
    h.run_steps(6);
    assert!(h.state().dialog.is_none(), "OK closes the dialog");
    assert!(h.state().message.is_none(), "{:?}", h.state().message);
    assert_eq!(tables(&h), 1);
    assert_eq!(value(&mut h, "E4"), json!(210.0));
    // The results follow their formula.
    h.state_mut().run("cell.set", json!({"cell": "B1", "input": "0.5"})).unwrap();
    assert_eq!(value(&mut h, "E4"), json!(300.0));
}

#[test]
fn an_input_cell_excel_refuses_keeps_the_dialog_open_behind_the_message() {
    let mut h = harness();
    control(&mut h, "ui.dialog", json!({"name": "dataTable"}));
    h.run_steps(6);
    // An input cell inside the table.
    control(&mut h, "ui.dialog.set", json!({"field": "columnInput", "value": "E3"}));
    h.run_steps(4);
    h.get_by_label("   OK   ").click();
    h.run_steps(6);
    assert_eq!(h.state().message.as_ref().map(|(_, m)| m.as_str()), Some("Input cell reference is not valid."));
    assert!(h.state().dialog.is_some(), "the dialog stays open for another try");
    assert_eq!(tables(&h), 0);
    // Over the control channel, the same command runs with the dialog's values.
    control(&mut h, "ui.message.dismiss", json!({}));
    control(&mut h, "ui.dialog.set", json!({"field": "columnInput", "value": "B2"}));
    let r = control(&mut h, "ui.dialog.confirm", json!({}));
    assert_eq!(r["ok"], json!(true), "{r}");
    assert_eq!(tables(&h), 1);
}

#[test]
fn the_dialog_speaks_german() {
    let mut h = harness();
    h.state_mut().run("app.language.set", json!({"language": "de"})).unwrap();
    h.state_mut().open_dialog("dataTable", json!({}));
    h.run_steps(6);
    assert!(h.query_by_label("Datentabelle").is_some(), "dialog title");
    assert!(h.query_by_role_and_label(Role::TextInput, "Row input cell:").is_some());
    assert!(!h.query_all_by_label("Zeileneingabewert:").collect::<Vec<_>>().is_empty());
    assert!(!h.query_all_by_label("Spalteneingabewert:").collect::<Vec<_>>().is_empty());
}

#[test]
fn the_formula_bar_shows_the_table_formula_in_the_formula_language() {
    let mut h = harness();
    h.state_mut().run("data.dataTable", json!({"range": "D2:E5", "columnInput": "B2"})).unwrap();
    h.state_mut().run("selection.set", json!({"range": "E4"})).unwrap();
    h.run_steps(4);
    assert!(h.query_by_label("{=TABLE(,B2)}").is_some(), "English formula bar");
    // A semicolon-separated formula language.
    h.state_mut().run("app.language.set", json!({"language": "es"})).unwrap();
    h.run_steps(4);
    assert!(h.query_by_label("{=TABLE(;B2)}").is_some(), "Spanish formula bar");
}

#[test]
fn editing_a_result_keeps_the_table_unless_it_changes() {
    let mut h = harness();
    h.state_mut().run("data.dataTable", json!({"range": "D2:E5", "columnInput": "B2"})).unwrap();
    h.state_mut().run("selection.set", json!({"range": "E4"})).unwrap();
    h.run_steps(2);
    // F2 shows the table's formula; Enter with it unchanged changes nothing.
    control(&mut h, "ui.edit.begin", json!({}));
    assert_eq!(h.state().editor.as_ref().map(|e| e.text.clone()), Some("=TABLE(,B2)".to_string()));
    control(&mut h, "ui.edit.commit", json!({}));
    h.run_steps(2);
    assert!(h.state().message.is_none(), "{:?}", h.state().message);
    assert!(h.state().editor.is_none());
    assert_eq!(tables(&h), 1);
    // Typing over one result is refused with Excel's message.
    h.state_mut().run("selection.set", json!({"range": "E4"})).unwrap();
    control(&mut h, "ui.edit.begin", json!({"text": "5"}));
    control(&mut h, "ui.edit.commit", json!({}));
    h.run_steps(2);
    assert_eq!(h.state().message.as_ref().map(|(_, m)| m.as_str()), Some("Cannot change part of a data table."));
    assert_eq!(value(&mut h, "E4"), json!(210.0));
}
