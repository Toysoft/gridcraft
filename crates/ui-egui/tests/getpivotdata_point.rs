//! Generate GetPivotData: pointing at a value of a PivotTable while typing a formula (a click,
//! a drag that ends on one cell, or the arrow keys) inserts GETPIVOTDATA, as Excel does; labels
//! and other cells, or the setting turned off, insert the plain reference.

use egui::{Event, Key, Modifiers, PointerButton, Pos2, pos2, vec2};
use gridcraft_engine::Session;
use gridcraft_engine::core::{CellRef, Value};
use gridcraft_ui_egui::{SheetApp, grid::Geo};
use serde_json::json;

type Harness = egui_kittest::Harness<'static, SheetApp>;

/// Sales by Region and Product on Sheet2 (report A3:B12): East A4/B4, its Apples A5/B5 (120).
/// The active cell is H2, outside the report.
fn harness() -> Harness {
    let mut s = Session::new();
    s.new_workbook();
    let rows = json!([
        ["Region", "Product", "Units", "Sales"],
        ["East", "Apples", 5, 100],
        ["East", "Pears", 3, 60],
        ["West", "Apples", 7, 140],
        ["West", "Plums", 2, 50],
        ["North", "Pears", 4, 80],
        ["East", "Apples", 1, 20]
    ]);
    s.execute("range.setValues", json!({"range": "A1", "values": rows})).unwrap();
    s.execute("insert.pivotTable", json!({"source": "A1:D7", "rows": ["Region", "Product"], "values": ["Sales"]})).unwrap();
    s.execute("selection.set", json!({"range": "H2"})).unwrap();
    let mut h = egui_kittest::Harness::builder().with_step_dt(1.0 / 60.0).with_size(vec2(1200.0, 800.0)).build_ui_state(
        |ui, app: &mut SheetApp| {
            app.logic(ui.ctx());
            app.ui(ui);
        },
        SheetApp::new(s, Default::default()),
    );
    h.run_steps(4);
    h
}

fn center(h: &Harness, a: &str) -> Pos2 {
    let c = CellRef::parse(a).unwrap();
    let app = h.state();
    let sheet = app.session.active().unwrap().wb.active().unwrap();
    let geo = Geo::new(sheet, app.grid.rect.unwrap(), app.view().scroll);
    pos2(geo.x(sheet, c.col) + sheet.col_width(c.col) * geo.z * 0.5, geo.y(sheet, c.row) + sheet.row_height(c.row) * geo.z * 0.5)
}

fn click(h: &mut Harness, pos: Pos2) {
    for pressed in [true, false] {
        h.input_mut()
            .events
            .extend([Event::PointerMoved(pos), Event::PointerButton { pos, button: PointerButton::Primary, pressed, modifiers: Modifiers::NONE }]);
        h.step();
    }
    h.run_steps(2);
}

fn key(h: &mut Harness, key: Key) {
    let ev = |pressed| Event::Key { key, physical_key: None, pressed, repeat: false, modifiers: Modifiers::NONE };
    h.input_mut().events.extend([ev(true), ev(false)]);
    h.run_steps(2);
}

fn text(h: &Harness) -> Option<String> {
    h.state().editor.as_ref().map(|e| e.text.clone())
}

fn start(h: &mut Harness, typed: &str) {
    h.state_mut().begin_edit(Some(typed.into()), false);
    h.run_steps(2);
}

const APPLES: &str = r#"GETPIVOTDATA("Sum of Sales",$A$3,"Region","East","Product","Apples")"#;

#[test]
fn clicking_a_value_inserts_getpivotdata() {
    let mut h = harness();
    start(&mut h, "=");
    let at = center(&h, "B5");
    click(&mut h, at);
    assert_eq!(text(&h).as_deref(), Some(format!("={APPLES}").as_str()));
    // A second click replaces it, like any pointed reference; a label is a plain reference.
    let at = center(&h, "A5");
    click(&mut h, at);
    assert_eq!(text(&h).as_deref(), Some("=A5"));
    let at = center(&h, "B4");
    click(&mut h, at);
    assert_eq!(text(&h).as_deref(), Some(r#"=GETPIVOTDATA("Sum of Sales",$A$3,"Region","East")"#));
    // The formula reads back the value.
    key(&mut h, Key::Enter);
    assert!(h.state().editor.is_none());
    let sheet = h.state().session.active().unwrap().wb.active().unwrap().clone();
    assert_eq!(sheet.value(CellRef::parse("H2").unwrap()), Value::Number(180.0));
}

#[test]
fn arrow_keys_point_with_getpivotdata_too() {
    let mut h = harness();
    start(&mut h, "=10+");
    // From H2 to B5: six columns left, three rows down.
    for _ in 0..6 {
        key(&mut h, Key::ArrowLeft);
    }
    assert_eq!(text(&h).as_deref(), Some("=10+B2"));
    for _ in 0..3 {
        key(&mut h, Key::ArrowDown);
    }
    assert_eq!(text(&h).as_deref(), Some(format!("=10+{APPLES}").as_str()));
    key(&mut h, Key::ArrowDown);
    assert_eq!(text(&h).as_deref(), Some(r#"=10+GETPIVOTDATA("Sum of Sales",$A$3,"Region","East","Product","Pears")"#));
    key(&mut h, Key::ArrowRight);
    assert_eq!(text(&h).as_deref(), Some("=10+C6"), "outside the report");
}

#[test]
fn turned_off_it_inserts_the_reference() {
    let mut h = harness();
    assert_eq!(h.state_mut().run("pivot.generateGetPivotData", json!({})), Ok(json!({"on": false})));
    start(&mut h, "=");
    let at = center(&h, "B5");
    click(&mut h, at);
    assert_eq!(text(&h).as_deref(), Some("=B5"));
    h.state_mut().editor = None;
    h.state_mut().run("pivot.generateGetPivotData", json!({"on": true})).unwrap();
    start(&mut h, "=");
    let at = center(&h, "B5");
    click(&mut h, at);
    assert_eq!(text(&h).as_deref(), Some(format!("={APPLES}").as_str()));
}

#[test]
fn from_another_sheet_the_pivottable_is_named_with_its_sheet() {
    let mut h = harness();
    h.state_mut().run("sheet.activate", json!({"sheet": "Sheet1"})).unwrap();
    h.state_mut().run("selection.set", json!({"range": "H2"})).unwrap();
    h.run_steps(2);
    start(&mut h, "=");
    h.state_mut().run("sheet.activate", json!({"sheet": "Sheet2"})).unwrap();
    h.run_steps(2);
    let at = center(&h, "B5");
    click(&mut h, at);
    assert_eq!(text(&h).as_deref(), Some(r#"=GETPIVOTDATA("Sum of Sales",Sheet2!$A$3,"Region","East","Product","Apples")"#));
}
