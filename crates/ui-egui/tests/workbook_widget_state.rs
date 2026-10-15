//! Fixed egui state belongs to a workbook, even when workbooks share one context.

use gridcraft_engine::Session;
use gridcraft_ui_egui::SheetApp;

#[test]
fn widget_ids_are_stable_within_a_workbook_and_distinct_between_workbooks() {
    let mut session = Session::new();
    session.new_workbook();
    session.new_workbook();
    let mut app = SheetApp::new(session, Default::default());

    app.session.set_active(0);
    let first = app.widget_id("formula_bar");
    assert_eq!(first, app.widget_id("formula_bar"));

    app.session.set_active(1);
    assert_ne!(first, app.widget_id("formula_bar"));
}
