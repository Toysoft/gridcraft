//! Data › What-If Analysis › Scenario Manager: the list, Show, Add (with Scenario Values), Edit,
//! Delete and Summary, driven through the dialog as a user would.

use egui::{Event, Modifiers, PointerButton, Pos2, vec2};
use egui_kittest::kittest::Queryable as _;
use gridcraft_engine::Session;
use gridcraft_ui_egui::SheetApp;
use serde_json::{Value as Json, json};

type Harness = egui_kittest::Harness<'static, SheetApp>;

/// B4 = B2*B3 with scenarios Base (10, 20) and Best (100, 3).
fn harness() -> Harness {
    let mut session = Session::new();
    session.new_workbook();
    session.run("range.setValues", json!({"range": "B2", "values": [[10], [20]]})).unwrap();
    session.run("cell.set", json!({"cell": "B4", "input": "=B2*B3"})).unwrap();
    session.run("data.scenarioAdd", json!({"name": "Base", "changing": "B2:B3"})).unwrap();
    session.run("data.scenarioAdd", json!({"name": "Best", "changing": "B2:B3", "values": [100, 3], "comment": "optimistic"})).unwrap();
    let mut h = egui_kittest::Harness::builder().with_size(vec2(1200.0, 800.0)).build_ui_state(
        |ui, app: &mut SheetApp| {
            app.logic(ui.ctx());
            app.ui(ui);
        },
        SheetApp::new(session, Default::default()),
    );
    h.run_steps(4);
    h
}

fn click_at(h: &mut Harness, pos: Pos2) {
    let press = |pressed| Event::PointerButton { pos, button: PointerButton::Primary, pressed, modifiers: Modifiers::NONE };
    h.input_mut().events.extend([Event::PointerMoved(pos), press(true)]);
    h.step();
    h.input_mut().events.push(press(false));
    h.run_steps(3);
}

/// Clicks the widget labelled `label` with the pointer.
fn click(h: &mut Harness, label: &str) {
    let at = h.get_by_label(label).rect().center();
    click_at(h, Pos2::new(at.x, at.y));
}

fn value(h: &mut Harness, cell: &str) -> Json {
    h.state_mut().session.run("cell.get", json!({"cell": cell})).unwrap()["value"].clone()
}

fn dialog(h: &Harness) -> Option<String> {
    h.state().dialog.as_ref().map(|d| d.name.clone())
}

fn scenarios(h: &mut Harness) -> Vec<Json> {
    h.state_mut().session.run("data.scenarioManager", json!({})).unwrap()["scenarios"].as_array().cloned().unwrap()
}

#[test]
fn scenario_manager_lists_shows_and_deletes() {
    let mut h = harness();
    h.state_mut().open_dialog("scenarioManager", json!({}));
    h.run_steps(3);
    assert_eq!(dialog(&h).as_deref(), Some("scenarioManager"));
    // Select Best, see its details, show it.
    click(&mut h, "Best");
    assert!(h.query_by_label("optimistic").is_some(), "the comment of the selected scenario is shown");
    click(&mut h, "Show");
    assert_eq!(value(&mut h, "B4"), json!(300.0));
    assert_eq!(dialog(&h).as_deref(), Some("scenarioManager"), "Show leaves the manager open, as in Excel");
    // Delete it.
    click(&mut h, "Delete");
    assert_eq!(scenarios(&mut h).len(), 1);
    click(&mut h, " Close ");
    assert_eq!(dialog(&h), None);
}

#[test]
fn add_asks_for_the_values_and_returns_to_the_manager() {
    let mut h = harness();
    h.state_mut().open_dialog("scenarioManager", json!({}));
    h.run_steps(3);
    click(&mut h, "Add…");
    assert_eq!(dialog(&h).as_deref(), Some("scenarioAdd"));
    let d = h.state_mut().dialog.as_mut().unwrap();
    d.values.insert("name".into(), json!("Worst"));
    d.values.insert("changing".into(), json!("B2,$B$3"));
    d.values.insert("comment".into(), json!("pessimistic"));
    h.run_steps(2);
    h.get_by_label("   OK   ").click();
    h.run_steps(3);
    // Scenario Values, prefilled with the cells' current values.
    let d = h.state().dialog.as_ref().unwrap();
    assert_eq!((d.name.as_str(), d.title.as_str()), ("scenarioAdd", "Scenario Values"));
    assert_eq!(d.values["cells"], json!(["$B$2", "$B$3"]));
    assert_eq!(d.values["inputs"], json!(["10", "20"]));
    h.state_mut().dialog.as_mut().unwrap().values.insert("inputs".into(), json!(["1", "2"]));
    h.run_steps(2);
    h.get_by_label("   OK   ").click();
    h.run_steps(3);
    assert_eq!(dialog(&h).as_deref(), Some("scenarioManager"), "back in the manager");
    let list = scenarios(&mut h);
    let worst = list.iter().find(|s| s["name"] == "Worst").unwrap();
    assert_eq!((worst["changing"].as_str(), worst["comment"].as_str()), (Some("B2,B3"), Some("pessimistic")));
    h.state_mut().session.run("data.scenarioShow", json!({"name": "Worst"})).unwrap();
    assert_eq!(value(&mut h, "B4"), json!(2.0));
}

#[test]
fn edit_keeps_the_scenario_values_and_can_rename() {
    let mut h = harness();
    h.state_mut().open_dialog("scenarioManager", json!({}));
    h.run_steps(3);
    click(&mut h, "Best");
    click(&mut h, "Edit…");
    let d = h.state().dialog.as_ref().unwrap();
    assert_eq!((d.title.as_str(), d.values["name"].as_str()), ("Edit Scenario", Some("Best")));
    h.state_mut().dialog.as_mut().unwrap().values.insert("name".into(), json!("Upside"));
    h.run_steps(2);
    h.get_by_label("   OK   ").click();
    h.run_steps(3);
    // The same changing cells: the scenario's own values, not the cells' current ones.
    assert_eq!(h.state().dialog.as_ref().unwrap().values["inputs"], json!(["100", "3"]));
    h.get_by_label("   OK   ").click();
    h.run_steps(3);
    let names: Vec<String> = scenarios(&mut h).iter().map(|s| s["name"].as_str().unwrap().to_string()).collect();
    assert_eq!(names, ["Base", "Upside"]);
}

#[test]
fn summary_replaces_the_manager_and_adds_the_report_sheet() {
    let mut h = harness();
    h.state_mut().session.run("selection.set", json!({"range": "B4"})).unwrap();
    h.state_mut().open_dialog("scenarioManager", json!({}));
    h.run_steps(3);
    click(&mut h, "Summary…");
    assert_eq!(dialog(&h).as_deref(), Some("scenarioSummary"));
    assert_eq!(h.state().dialog.as_ref().unwrap().values["resultCells"], json!("B4"));
    h.run_steps(4);
    h.get_by_label("   OK   ").click();
    h.run_steps(3);
    assert_eq!(dialog(&h), None, "the manager does not come back after the report; message {:?}", h.state().message);
    assert_eq!(h.state().session.active().unwrap().wb.active().unwrap().name, "Scenario Summary");
}

#[test]
fn an_empty_sheet_says_how_to_start() {
    let mut session = Session::new();
    session.new_workbook();
    let mut h = egui_kittest::Harness::builder().with_size(vec2(1200.0, 800.0)).build_ui_state(
        |ui, app: &mut SheetApp| {
            app.logic(ui.ctx());
            app.ui(ui);
        },
        SheetApp::new(session, Default::default()),
    );
    h.state_mut().open_dialog("scenarioManager", json!({}));
    h.run_steps(4);
    assert!(h.query_by_label("No Scenarios defined. Choose Add to add scenarios.").is_some());
    // Without a name the first step stays open with a message.
    click(&mut h, "Add…");
    h.state_mut().dialog.as_mut().unwrap().values.insert("name".into(), json!(""));
    h.run_steps(2);
    h.get_by_label("   OK   ").click();
    h.run_steps(3);
    assert!(h.state().message.is_some());
    assert_eq!(h.state().dialog.as_ref().map(|d| d.title.clone()).as_deref(), Some("Add Scenario"));
}
