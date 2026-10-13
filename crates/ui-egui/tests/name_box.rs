//! The Name Box shows a defined name when the selection is exactly its range, also on a sheet
//! whose name holds an apostrophe (`'Bob''s'!$A$1:$B$2`).

use gridcraft_engine::Session;
use gridcraft_ui_egui::{Services, SheetApp};
use serde_json::json;

#[test]
fn name_box_shows_a_name_on_a_sheet_with_an_apostrophe() {
    let mut session = Session::new();
    session.new_workbook();
    session.execute("home.insertSheet", json!({"name": "Bob's"})).unwrap();
    session.execute("formulas.defineName", json!({"name": "Totals", "refersTo": "='Bob''s'!$A$1:$B$2"})).unwrap();
    session.execute("selection.set", json!({"range": "A1:B2"})).unwrap();
    let app = SheetApp::new(session, Services::default());
    assert_eq!(app.name_box_text(), "Totals");
}
