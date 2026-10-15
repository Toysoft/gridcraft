//! PivotTable field buttons, filter dialogs, Grouping / Calculated Item dialogs and timelines,
//! driven through real egui input.

use egui::{Event, Key, Modifiers, PointerButton};
use egui_kittest::kittest::Queryable as _;
use gridcraft_engine::Session;
use gridcraft_engine::core::CellRef;
use gridcraft_ui_egui::{SheetApp, grid::Geo};
use serde_json::json;

fn harness(session: Session) -> egui_kittest::Harness<'static, SheetApp> {
    let mut h = egui_kittest::Harness::builder().with_size(egui::vec2(1400.0, 900.0)).build_ui_state(
        |ui, app: &mut SheetApp| {
            let ctx = ui.ctx().clone();
            app.logic(&ctx);
            app.ui(ui);
        },
        SheetApp::new(session, Default::default()),
    );
    h.run_steps(4);
    h
}

/// Sales by region on Sheet1 and a PivotTable of it at Sheet1!H3 (rows Region, values Sales).
fn sales_pivot() -> Session {
    let mut s = Session::new();
    s.new_workbook();
    let rows = json!([
        ["Region", "Date", "Sales"],
        ["East", "1/15/2026", 100],
        ["West", "2/10/2026", 50],
        ["East", "3/3/2026", 120],
        ["North", "4/20/2026", 30],
        ["West", "5/5/2026", 70]
    ]);
    s.execute("range.setValues", json!({"range": "A1", "values": rows})).unwrap();
    s.execute("insert.pivotTable", json!({"source": "Sheet1!$A$1:$C$6", "destination": "Sheet1!E3", "rows": ["Region"], "values": ["Sales"]}))
        .unwrap();
    s.execute("selection.set", json!({"range": "E4"})).unwrap();
    s
}

fn click_at(h: &mut egui_kittest::Harness<'static, SheetApp>, pos: egui::Pos2, modifiers: Modifiers) {
    h.input_mut().events.push(Event::ModifiersChanged(modifiers));
    h.input_mut().events.extend([
        Event::PointerMoved(pos),
        Event::PointerButton { pos, button: PointerButton::Primary, pressed: true, modifiers },
        Event::PointerButton { pos, button: PointerButton::Primary, pressed: false, modifiers },
    ]);
    h.run_steps(3);
    h.input_mut().events.push(Event::ModifiersChanged(Modifiers::NONE));
    h.run_steps(1);
}

fn geo(h: &egui_kittest::Harness<'static, SheetApp>) -> Geo {
    let app = h.state();
    let sheet = app.session.active().unwrap().wb.active().unwrap();
    Geo::new(sheet, app.grid.rect.unwrap(), app.view().scroll)
}

fn pivot(h: &egui_kittest::Harness<'static, SheetApp>) -> gridcraft_engine::model::PivotTable {
    h.state().session.active().unwrap().wb.active().unwrap().pivots[0].clone()
}

#[test]
fn dormant_filter_rule_has_no_report_filter_button() {
    use gridcraft_engine::model::{PivotCompare, PivotFilter, PivotRule};
    let s = sales_pivot();
    let mut sheet = s.active().unwrap().wb.active().unwrap().clone();
    let before = gridcraft_ui_egui::pivot_ui::header_buttons(&sheet);
    sheet.pivots[0].filters.push(PivotFilter {
        source_col: "Date".into(),
        selected: None,
        rule: Some(PivotRule::Label { op: PivotCompare::Equals, value: "1".into(), value2: String::new() }),
    });
    let after = gridcraft_ui_egui::pivot_ui::header_buttons(&sheet);
    assert_eq!(after.len(), before.len());
    assert_eq!(after[0].cell, before[0].cell);
    assert_eq!(after[0].fields, before[0].fields);
    assert!(!after[0].page);
}

#[test]
fn row_labels_button_opens_label_and_value_filters() {
    let mut h = harness(sales_pivot());
    let sheet = h.state().session.active().unwrap().wb.active().unwrap().clone();
    let buttons = gridcraft_ui_egui::pivot_ui::header_buttons(&sheet);
    // Compact form: one Row Labels button, on the header cell above the items.
    assert_eq!(buttons.len(), 1, "{buttons:?}");
    assert_eq!((buttons[0].cell, buttons[0].fields.clone()), (CellRef::parse("E3").unwrap(), vec!["Region".to_string()]));
    let g = geo(&h);
    let at = gridcraft_ui_egui::pivot_ui::button_rect(&g, &sheet, buttons[0].cell).center();
    click_at(&mut h, at, Modifiers::NONE);
    assert!(h.state().grid.pivot_menu.is_some(), "the field menu opens");
    h.get_by_label("Label Filters").click();
    h.run_steps(3);
    h.get_by_label("Begins With…").click();
    h.run_steps(3);
    assert_eq!(h.state().dialog.as_ref().map(|d| d.name().to_string()).as_deref(), Some("pivotLabelFilter"));
    h.state_mut().dialog.as_mut().unwrap().values.insert("value".into(), json!("E"));
    h.run_steps(2);
    h.get_by_label("   OK   ").click();
    h.run_steps(3);
    assert!(h.state().dialog.is_none());
    let rule = pivot(&h).filters.first().and_then(|f| f.rule.clone());
    assert!(matches!(rule, Some(gridcraft_engine::model::PivotRule::Label { .. })), "{rule:?}");
    // The button now shows the field filtered.
    let sheet = h.state().session.active().unwrap().wb.active().unwrap().clone();
    assert!(gridcraft_ui_egui::pivot_ui::header_buttons(&sheet)[0].filtered);
    // Value Filters › Top 10… opens the Top 10 dialog.
    let at = gridcraft_ui_egui::pivot_ui::button_rect(&geo(&h), &sheet, CellRef::parse("E3").unwrap()).center();
    click_at(&mut h, at, Modifiers::NONE);
    h.get_by_label("Value Filters").click();
    h.run_steps(3);
    h.get_by_label("Top 10…").click();
    h.run_steps(3);
    assert_eq!(h.state().dialog.as_ref().map(|d| d.name().to_string()).as_deref(), Some("pivotTop10"));
    h.get_by_label("   OK   ").click();
    h.run_steps(3);
    let rule = pivot(&h).filters.first().and_then(|f| f.rule.clone());
    assert!(matches!(rule, Some(gridcraft_engine::model::PivotRule::Top { .. })), "{rule:?}");
}

#[test]
fn insert_timeline_from_the_ribbon_then_pick_a_period() {
    let mut h = harness(sales_pivot());
    h.state_mut().ui.ribbon_tab = "Insert".into();
    h.run_steps(2);
    h.get_by_label("Timeline").click();
    h.run_steps(3);
    assert_eq!(h.state().dialog.as_ref().map(|d| d.name().to_string()).as_deref(), Some("insertTimeline"));
    // Tick Date (the Fields pane lists a Date check box too).
    h.state_mut().dialog.as_mut().unwrap().values.insert("checked".into(), json!(["Date"]));
    h.run_steps(2);
    h.get_by_label("   OK   ").click();
    h.run_steps(3);
    let tl = h.state().session.active().unwrap().wb.active().unwrap().timelines.first().cloned().expect("a timeline");
    // Selecting it shows the Timeline tab.
    h.state_mut().selected_chart = Some(tl.id);
    h.state_mut().ui.ribbon_tab = "Timeline".into();
    h.run_steps(3);
    assert!(h.query_by_label("Timeline Caption:").is_some(), "the Timeline tab shows");
    // A click on the March tile selects March: East 120.
    let wb = h.state().session.active().unwrap().wb.clone();
    let sheet = wb.active().unwrap().clone();
    let g = geo(&h);
    let a = tl.anchor;
    let r = egui::Rect::from_min_size(
        egui::pos2(g.x(&sheet, a.cell.col) + a.dx * g.z, g.y(&sheet, a.cell.row) + a.dy * g.z),
        egui::vec2(a.width * g.z, a.height.max(1.0) * g.z),
    );
    let periods = gridcraft_engine::cmd::timeline::periods(&wb, &tl);
    let lay = gridcraft_ui_egui::timeline_view::layout(r, g.z, &tl, &periods);
    let (tile, _) = lay.tiles.iter().find(|(_, i)| periods[*i].label == "MAR").copied().expect("a March tile");
    click_at(&mut h, tile.center(), Modifiers::NONE);
    let tl = h.state().session.active().unwrap().wb.active().unwrap().timelines[0].clone();
    assert_eq!(tl.selection, Some((46082.0, 46112.0)));
    // Shift+click on May extends the selection: March to May.
    let (tile, _) = lay.tiles.iter().find(|(_, i)| periods[*i].label == "MAY").copied().unwrap();
    click_at(&mut h, tile.center(), Modifiers::SHIFT);
    let tl = h.state().session.active().unwrap().wb.active().unwrap().timelines[0].clone();
    assert_eq!(tl.selection, Some((46082.0, 46173.0)));
    // The clear button selects all periods again.
    let clear = lay.clear.unwrap();
    click_at(&mut h, clear.center(), Modifiers::NONE);
    assert_eq!(h.state().session.active().unwrap().wb.active().unwrap().timelines[0].selection, None);
}

#[test]
fn pivot_dialogs_open_and_close_in_german() {
    let mut s = sales_pivot();
    s.execute("insert.timeline", json!({"field": "Date"})).unwrap();
    let mut h = harness(s);
    h.state_mut().run("app.language.set", json!({"language": "de"})).unwrap();
    h.run_steps(2);
    for (name, params) in [
        ("pivotGrouping", json!({"pivot": "PivotTable1", "field": "Date", "kind": "date", "levels": ["months"], "start": 46037, "end": 46148})),
        ("pivotGrouping", json!({"pivot": "PivotTable1", "field": "Sales", "kind": "number", "start": 30, "end": 120, "interval": 10})),
        (
            "pivotCalculatedItem",
            json!({"pivot": "PivotTable1", "field": "Region", "name": "Formula1", "formula": "= 0", "fields": ["Region"], "items": ["East", "West"]}),
        ),
        ("pivotSolveOrder", json!({"pivot": "PivotTable1", "calculatedItems": [{"field": "Region", "name": "EW", "formula": "=East+West"}]})),
        ("insertTimeline", json!({"pivot": "PivotTable1", "fields": ["Date"]})),
        ("timelineConnections", json!({"timeline": "Date", "pivots": [{"name": "PivotTable1", "sheet": "Sheet1", "connected": true}]})),
        ("pivotLabelFilter", json!({"pivot": "PivotTable1", "field": "Region", "op": "between"})),
        ("pivotValueFilter", json!({"pivot": "PivotTable1", "field": "Region", "op": "greaterThan"})),
        ("pivotTop10", json!({"pivot": "PivotTable1", "field": "Region"})),
    ] {
        h.state_mut().open_dialog(name, params);
        h.run_steps(2);
        assert!(h.state().dialog.is_some(), "{name} opens");
        h.input_mut().events.push(Event::Key { key: Key::Escape, physical_key: None, pressed: true, repeat: false, modifiers: Modifiers::NONE });
        h.run_steps(2);
        h.state_mut().dialog = None;
        h.state_mut().message = None;
    }
    // German labels: the Insert tab's Zeitachse and the Grouping dialog's title.
    h.state_mut().ui.ribbon_tab = "Insert".into();
    h.run_steps(2);
    assert!(h.query_by_label("Zeitachse").is_some());
    h.state_mut().open_dialog("pivotGrouping", json!({"pivot": "PivotTable1", "field": "Date", "kind": "date", "levels": ["months"]}));
    h.run_steps(2);
    assert!(h.query_by_label("Anzahl der Tage:").is_some());
}

#[test]
fn fields_pane_menu_has_label_and_value_filters() {
    let mut h = harness(sales_pivot());
    h.get_by_label("Region ▾").click();
    h.run_steps(2);
    assert!(h.query_by_label_contains("Label Filters").is_some());
    assert!(h.query_by_label_contains("Value Filters").is_some());
}

#[test]
fn pivottable_analyze_runs_grouping_calculated_items_and_timelines() {
    let mut s = sales_pivot();
    // The active cell on East, an item of Region.
    s.execute("selection.set", json!({"range": "E4"})).unwrap();
    let mut h = harness(s);
    h.state_mut().ui.ribbon_tab = gridcraft_ui_egui::ribbon::pivot_tabs::ANALYZE.into();
    h.run_steps(3);
    // Group Field opens the Grouping dialog only for number and date fields: Region is text.
    h.get_by_label("Group Field").click();
    h.run_steps(3);
    assert!(h.state().dialog.is_none());
    h.state_mut().message = None;
    // Fields, Items, & Sets › Calculated Item… opens Insert Calculated Item for Region.
    h.get_by_label("Fields, Items, & Sets").click();
    h.run_steps(3);
    h.get_by_label("Calculated Item…").click();
    h.run_steps(3);
    let d = h.state().dialog.as_ref().map(|d| (d.name().to_string(), d.values.get("field").cloned()));
    assert_eq!(d, Some(("pivotCalculatedItem".to_string(), Some(json!("Region")))));
    h.state_mut().dialog = None;
    // Insert Timeline lists the date fields.
    h.get_by_label("Insert Timeline").click();
    h.run_steps(3);
    assert_eq!(h.state().dialog.as_ref().map(|d| d.name().to_string()).as_deref(), Some("insertTimeline"));
    h.state_mut().dialog = None;
    // Group Selection on East and North.
    h.state_mut().run("selection.set", json!({"range": "E4,E5"})).unwrap();
    h.run_steps(2);
    h.get_by_label("Group Selection").click();
    h.run_steps(3);
    assert_eq!(pivot(&h).group_fields.first().map(|g| g.name.as_str()), Some("Region2"));
}
