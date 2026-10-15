//! The PivotTable Analyze and PivotTable Design tabs: they come with the active cell in a
//! PivotTable, their key tips (JT, JY) switch to them, and their controls run the pivot
//! commands, driven through real egui input.

use egui::os::OperatingSystem;
use egui::{Event, Key, Modifiers};
use egui_kittest::kittest::Queryable as _;
use gridcraft_engine::Session;
use gridcraft_engine::model::PivotTable;
use gridcraft_ui_egui::SheetApp;
use gridcraft_ui_egui::ribbon::pivot_tabs::{ANALYZE, DESIGN};
use serde_json::json;

type Harness = egui_kittest::Harness<'static, SheetApp>;

fn harness(session: Session) -> Harness {
    let mut h = egui_kittest::Harness::builder().with_os(OperatingSystem::Nix).with_size(egui::vec2(1800.0, 800.0)).build_ui_state(
        |ui, app: &mut SheetApp| {
            app.logic(ui.ctx());
            app.ui(ui);
        },
        SheetApp::new(session, Default::default()),
    );
    h.run_steps(4);
    h
}

/// A PivotTable of Sales by Region and Product on a new sheet; the active cell is its anchor.
fn with_pivot() -> Session {
    let mut s = Session::new();
    s.new_workbook();
    let rows = json!([
        ["Region", "Product", "Units", "Sales"],
        ["East", "Apples", 5, 100],
        ["East", "Pears", 3, 60],
        ["West", "Apples", 7, 140],
        ["West", "Plums", 2, 50],
        ["North", "Pears", 4, 80]
    ]);
    s.execute("range.setValues", json!({"range": "A1", "values": rows})).unwrap();
    s.execute("insert.pivotTable", json!({"source": "A1:D6", "rows": ["Region", "Product"], "values": ["Sales"]})).unwrap();
    s
}

fn pivot(h: &Harness) -> PivotTable {
    h.state().session.active().unwrap().wb.active().unwrap().pivots[0].clone()
}

/// Presses and releases `key`; a letter also types its text, as a real keyboard does.
fn key(h: &mut Harness, key: Key) {
    let ev = |pressed| Event::Key { key, physical_key: None, pressed, repeat: false, modifiers: Modifiers::NONE };
    h.input_mut().events.push(ev(true));
    if key.name().len() == 1 {
        h.input_mut().events.push(Event::Text(key.name().to_ascii_lowercase()));
    }
    h.input_mut().events.push(ev(false));
    h.run_steps(2);
}

fn keytips(h: &mut Harness, seq: &str) {
    key(h, Key::F10);
    for ch in seq.chars() {
        key(h, Key::from_name(&ch.to_string()).unwrap());
    }
}

fn click(h: &mut Harness, label: &str) {
    h.get_by_label(label).click();
    h.run_steps(2);
}

#[test]
fn key_tips_switch_to_the_tabs_while_a_pivottable_is_selected() {
    let mut h = harness(with_pivot());
    keytips(&mut h, "JT");
    assert_eq!(h.state().ui.ribbon_tab, ANALYZE);
    // JT stays armed (Table Design's letters continue it): Escape pops J, T, then leaves.
    for _ in 0..3 {
        key(&mut h, Key::Escape);
    }
    assert_eq!(h.state().keytips.prefix(), None);
    keytips(&mut h, "JY");
    assert_eq!(h.state().ui.ribbon_tab, DESIGN);
    key(&mut h, Key::Escape);
    // Outside the PivotTable the tabs are gone: JT leads to Table Design's letters only.
    h.state_mut().run("selection.set", json!({"range": "K30"})).unwrap();
    h.run_steps(2);
    assert_eq!(h.state().ui.ribbon_tab, "Home", "leaving the PivotTable goes back to Home");
    keytips(&mut h, "JT");
    assert_eq!(h.state().ui.ribbon_tab, "Home");
}

#[test]
fn design_tab_menus_change_the_layout() {
    let mut h = harness(with_pivot());
    h.state_mut().run("ui.ribbonTab", json!({"tab": DESIGN})).unwrap();
    h.run_steps(2);
    click(&mut h, "Grand Totals");
    click(&mut h, "    Off for Rows and Columns");
    let p = pivot(&h);
    assert!(!p.grand_totals_rows && !p.grand_totals_cols);
    click(&mut h, "Subtotals");
    click(&mut h, "    Show all Subtotals at Bottom of Group");
    let p = pivot(&h);
    assert!(p.subtotals && !p.subtotals_top);
    click(&mut h, "Report Layout");
    click(&mut h, "    Show in Tabular Form");
    assert_eq!(pivot(&h).layout, gridcraft_engine::model::PivotLayout::Tabular);
    click(&mut h, "PivotTable Styles");
    click(&mut h, "PivotStyleMedium9");
    assert_eq!(pivot(&h).style, "PivotStyleMedium9");
    // One undo step each.
    h.state_mut().run("edit.undo", json!({})).unwrap();
    assert_eq!(pivot(&h).style, "PivotStyleLight16");
}

#[test]
fn analyze_tab_runs_the_pivot_commands() {
    let mut h = harness(with_pivot());
    h.state_mut().run("ui.ribbonTab", json!({"tab": ANALYZE})).unwrap();
    h.run_steps(2);
    // Field List hides and shows the PivotTable Fields pane.
    assert!(h.query_by_label("PivotTable Fields").is_some());
    click(&mut h, "Field List");
    assert!(h.query_by_label("PivotTable Fields").is_none());
    click(&mut h, "Field List");
    assert!(h.query_by_label("PivotTable Fields").is_some());
    // Field Headers toggles the headers.
    click(&mut h, "Field Headers");
    assert!(!pivot(&h).show_headers);
    // Select › Entire PivotTable.
    click(&mut h, "Select");
    click(&mut h, "Entire PivotTable");
    let range = pivot(&h).last_range.unwrap();
    assert_eq!(h.state().session.active().unwrap().selection.current(), range);
    // Clear › Clear All empties the PivotTable but keeps it.
    h.state_mut().run("selection.set", json!({"range": "A3"})).unwrap();
    h.run_steps(2);
    click(&mut h, "Clear");
    click(&mut h, "Clear All");
    let p = pivot(&h);
    assert!(p.rows.is_empty() && p.values.is_empty());
    assert!(h.state().message.is_none(), "{:?}", h.state().message);
}

#[test]
fn the_tabs_and_their_controls_speak_german() {
    let mut h = harness(with_pivot());
    h.state_mut().run("app.language.set", json!({"language": "de"})).unwrap();
    h.state_mut().run("ui.ribbonTab", json!({"tab": ANALYZE})).unwrap();
    h.run_steps(2);
    for label in ["Datenquelle ändern", "Felder, Elemente und Gruppen", "Empfohlene PivotTables", "Feldliste", "Datenschnitt einfügen"] {
        assert!(h.query_by_label(label).is_some(), "Analyze shows `{label}`");
    }
    click(&mut h, "Aktualisieren (Alt+F5)");
    assert!(h.query_by_label("Alle aktualisieren").is_some());
    h.state_mut().run("ui.ribbonTab", json!({"tab": DESIGN})).unwrap();
    h.run_steps(2);
    for label in ["Teilergebnisse", "Gesamtergebnisse", "Berichtslayout", "Leere Zeilen", "PivotTable-Formate"] {
        assert!(h.query_by_label(label).is_some(), "Design shows `{label}`");
    }
}

#[test]
fn active_field_group_works_on_the_active_cells_field() {
    let mut h = harness(with_pivot());
    h.state_mut().run("ui.ribbonTab", json!({"tab": ANALYZE})).unwrap();
    // B5 is a value of Sum of Sales: Field Settings opens Value Field Settings for it.
    h.state_mut().run("selection.set", json!({"range": "B5"})).unwrap();
    h.run_steps(2);
    click(&mut h, "Field Settings");
    let dialog = h.state().dialog.as_ref().expect("Value Field Settings opens");
    assert_eq!(dialog.name(), "pivotValueFieldSettings");
    assert_eq!(dialog.values["field"], "Sum of Sales");
    assert_eq!(dialog.values["func"], "sum");
    h.state_mut().dialog = None;
    // A4 is the East item of Region: Collapse Field and Expand Field act on Region.
    h.state_mut().run("selection.set", json!({"range": "A4"})).unwrap();
    h.run_steps(2);
    click(&mut h, "Collapse Field");
    assert_eq!(pivot(&h).rows[0].collapsed_items.len(), 3, "East, North and West");
    click(&mut h, "Expand Field");
    assert!(pivot(&h).rows[0].collapsed_items.is_empty());
    // Select › Values selects the value cells.
    click(&mut h, "Select");
    // The Fields pane has a "Values" area too: the menu item is the button.
    h.get_by_role_and_label(egui::accesskit::Role::Button, "Values").click();
    h.run_steps(2);
    assert_eq!(h.state().session.active().unwrap().selection.current().a1(), "B4:B12");
    // Options › Generate GetPivotData switches the setting.
    assert!(h.state().session.prefs.generate_get_pivot_data);
    click(&mut h, "PivotTable Options");
    click(&mut h, "✓ Generate GetPivotData");
    assert!(!h.state().session.prefs.generate_get_pivot_data);
}
