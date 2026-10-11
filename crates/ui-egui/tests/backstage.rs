//! The File page: the File button opens it over the whole window, its column turns the pages and
//! runs the file commands, and agents drive it as the dialog `backstage`.

use egui::accesskit::Role;
use egui::{Key, vec2};
use egui_kittest::kittest::Queryable as _;
use gridcraft_engine::core::{CellRef, Value};
use gridcraft_ui_egui::control::{self, Outcome};
use gridcraft_ui_egui::{Services, SheetApp};
use serde_json::{Value as Json, json};

const NAME: &str = "backstage";

type Harness = egui_kittest::Harness<'static, SheetApp>;

fn app() -> SheetApp {
    let mut session = gridcraft_engine::Session::new();
    session.new_workbook();
    SheetApp::new(session, Services::default())
}

fn harness(app: SheetApp) -> Harness {
    let mut h = egui_kittest::Harness::builder().with_size(vec2(1200.0, 800.0)).build_ui_state(
        |ui, app: &mut SheetApp| {
            let ctx = ui.ctx().clone();
            app.logic(&ctx);
            app.ui(ui);
        },
        app,
    );
    h.run_steps(4);
    h
}

/// Clicks go by position, so the page is given a moment to be laid out first.
fn press(h: &mut Harness, label: &str) {
    h.run_steps(3);
    h.get_by_role_and_label(Role::Button, label).click();
    h.run_steps(4);
}

/// The page showing, when the File page is up.
fn page(h: &Harness) -> Option<String> {
    let d = h.state().dialog.as_ref()?;
    (d.name == NAME).then(|| d.values.get("page").and_then(Json::as_str).unwrap_or("").to_string())
}

fn reply(outcome: Outcome) -> Json {
    match outcome {
        Outcome::Done(v) => v,
        _ => Json::Null,
    }
}

/// A folder of its own for a test's files, removed afterwards.
struct Temp(std::path::PathBuf);

impl Temp {
    fn new(name: &str) -> Temp {
        let dir = std::env::temp_dir().join(format!("gridcraft-file-page-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        Temp(dir)
    }

    fn path(&self, file: &str) -> String {
        self.0.join(file).to_string_lossy().into_owned()
    }
}

impl Drop for Temp {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[test]
fn file_opens_the_page_and_back_or_escape_returns_to_the_workbook() {
    let mut h = harness(app());
    press(&mut h, "File");
    assert_eq!(page(&h).as_deref(), Some("home"));
    // What the File menu offered is on the page under the same names, each once.
    for label in ["New Workbook", "Open…", "Save", "Save As…", "Close"] {
        h.get_by_label(label);
    }
    h.key_press(Key::Escape);
    h.run_steps(3);
    assert!(h.state().dialog.is_none(), "Escape goes back to the workbook");
    assert!(h.query_by_label("New Workbook").is_none(), "and the page is gone");
    press(&mut h, "File");
    press(&mut h, "Back");
    assert!(h.state().dialog.is_none(), "so does the button at the head of the column");
    assert_eq!(h.state().session.documents().len(), 1, "nothing else happened");
}

#[test]
fn the_column_turns_the_pages_and_agents_turn_them_too() {
    let mut h = harness(app());
    press(&mut h, "File");
    for (label, code) in [("Open", "open"), ("Info", "info"), ("Export", "export"), ("New", "new"), ("Home", "home")] {
        press(&mut h, label);
        assert_eq!(page(&h).as_deref(), Some(code));
    }
    // An agent opens it at a page, turns the page, sees it reported, and leaves.
    let ctx = egui::Context::default();
    let a = h.state_mut();
    let opened = reply(control::handle(a, &ctx, "ui.dialog", &json!({"name": "backstage", "page": "export"})));
    assert_eq!(opened["result"]["dialog"], json!("backstage"));
    let seen = reply(control::handle(a, &ctx, "ui.inspect", &json!({})));
    assert_eq!(seen["result"]["dialog"]["values"]["page"], json!("export"));
    reply(control::handle(a, &ctx, "ui.dialog.set", &json!({"field": "page", "value": "info"})));
    let refused = reply(control::handle(a, &ctx, "ui.dialog.confirm", &json!({})));
    assert_eq!(refused["ok"], json!(false), "there is no OK to press: its buttons are commands");
    h.run_steps(3);
    assert_eq!(page(&h).as_deref(), Some("info"));
    h.get_by_label("Properties");
    // A page that doesn't exist shows Home rather than failing.
    for nonsense in [json!("nowhere"), json!(42), json!(null), json!({"page": ["home"]})] {
        h.state_mut().dialog.as_mut().unwrap().values.insert("page".into(), nonsense);
        h.run_steps(3);
        h.get_by_label("New Workbook");
    }
    let a = h.state_mut();
    reply(control::handle(a, &ctx, "ui.dialog.cancel", &json!({})));
    assert!(a.dialog.is_none());
}

#[test]
fn a_tile_starts_a_workbook_and_the_page_leaves() {
    let mut h = harness(app());
    press(&mut h, "File");
    press(&mut h, "Gradebook");
    assert!(h.state().dialog.is_none());
    assert_eq!(h.state().session.documents().len(), 2);
    assert_eq!(h.state().session.active().unwrap().display_title(), "Gradebook");
    // The New page offers the same.
    h.state_mut().open_dialog(NAME, json!({"page": "new"}));
    press(&mut h, "New Workbook");
    assert_eq!(h.state().session.documents().len(), 3);
}

#[test]
fn what_is_being_typed_is_entered_before_the_page_opens() {
    let mut h = harness(app());
    h.input_mut().events.push(egui::Event::Text("123".into()));
    h.run_steps(2);
    assert!(h.state().editor.is_some());
    press(&mut h, "File");
    assert_eq!(page(&h).as_deref(), Some("home"));
    assert!(h.state().editor.is_none());
    assert_eq!(h.state().session.active().unwrap().wb.active().unwrap().value(CellRef::new(0, 0)), Value::Number(123.0));
    // With the page up, the keyboard and the pointer don't reach the workbook under it.
    h.input_mut().events.push(egui::Event::Text("9".into()));
    h.run_steps(2);
    assert!(h.state().editor.is_none(), "typing doesn't start an entry");
    let before = h.state().session.active().unwrap().selection.active;
    let grid = h.state().grid.cells_rect.unwrap();
    let button =
        |pressed| egui::Event::PointerButton { pos: grid.center(), button: egui::PointerButton::Primary, pressed, modifiers: Default::default() };
    h.hover_at(grid.center());
    h.input_mut().events.push(button(true));
    h.run_steps(2);
    h.input_mut().events.push(button(false));
    h.run_steps(2);
    assert_eq!(h.state().session.active().unwrap().selection.active, before, "a click doesn't move the selection");
    assert_eq!(page(&h).as_deref(), Some("home"), "and the page is still up");
    // The same click does move it once the page has gone.
    h.key_press(Key::Escape);
    h.run_steps(3);
    h.input_mut().events.push(button(true));
    h.run_steps(2);
    h.input_mut().events.push(button(false));
    h.run_steps(2);
    assert_ne!(h.state().session.active().unwrap().selection.active, before);
}

#[test]
fn an_entry_the_cell_refuses_keeps_the_user_in_the_cell() {
    let mut h = harness(app());
    h.input_mut().events.push(egui::Event::Text("=1+*".into()));
    h.run_steps(2);
    press(&mut h, "File");
    assert!(h.state().dialog.is_none(), "the page doesn't open over an entry that can't be made");
    assert!(h.state().message.is_some(), "the reason is given");
    assert_eq!(h.state().editor.as_ref().map(|e| e.text.as_str()), Some("=1+*"), "and the entry is still there to mend");
}

#[test]
fn recent_workbooks_open_from_the_list_and_one_that_is_gone_can_be_removed() {
    let dir = Temp::new("recent");
    let (kept, gone) = (dir.path("Kept.xlsx"), dir.path("Gone.xlsx"));
    let mut a = app();
    a.run("cell.set", json!({"cell": "A1", "input": "7"})).unwrap();
    a.run("file.save", json!({"path": kept})).unwrap();
    a.run("file.close", json!({"force": true})).unwrap();
    a.ui.recent = vec![gone.clone(), kept.clone()];
    let mut h = harness(a);
    press(&mut h, "File");
    press(&mut h, &kept);
    assert!(h.state().dialog.is_none() && h.state().message.is_none(), "{:?}", h.state().message);
    let doc = h.state().session.active().unwrap();
    assert_eq!((doc.path.as_deref(), doc.wb.active().unwrap().value(CellRef::new(0, 0))), (Some(kept.as_str()), Value::Number(7.0)));
    assert_eq!(h.state().ui.recent.first(), Some(&kept), "the one just opened heads the list");
    // One that can't be opened says why, and the page stays for another choice.
    press(&mut h, "File");
    press(&mut h, &gone);
    assert!(h.state().message.is_some());
    assert_eq!(page(&h).as_deref(), Some("home"));
    h.key_press(Key::Escape);
    h.run_steps(3);
    assert!(h.state().message.is_none(), "Escape answers the message");
    assert_eq!(page(&h).as_deref(), Some("home"), "not the page under it");
    // The row under the pointer offers to drop its workbook from the list.
    h.get_by_role_and_label(Role::Button, &gone).hover();
    h.run_steps(3);
    press(&mut h, "Remove from the list");
    assert_eq!(h.state().ui.recent, vec![kept.clone()]);
    assert_eq!(page(&h).as_deref(), Some("home"));
    assert!(h.query_by_label(&gone).is_none());
}

#[test]
fn a_workbook_saved_under_a_name_joins_the_list() {
    let dir = Temp::new("saved");
    let path = dir.path("Budget.xlsx");
    let mut a = app();
    a.run("file.save", json!({"path": path})).unwrap();
    assert!(a.ui.recent.is_empty(), "opening from a file is what lists a workbook");
    a.open_dialog(NAME, json!({}));
    assert_eq!(a.ui.recent, vec![path.clone()]);
    a.open_dialog(NAME, json!({}));
    assert_eq!(a.ui.recent, vec![path], "once");
    // The list never grows past its limit.
    a.ui.recent = (0..60).map(|i| format!("old-{i}.xlsx")).collect();
    a.open_dialog(NAME, json!({}));
    assert_eq!(a.ui.recent.len(), 20);
}

#[test]
fn about_and_export_hand_over_to_their_dialogs() {
    let mut h = harness(app());
    let name = |h: &Harness| h.state().dialog.as_ref().map(|d| d.name.clone());
    press(&mut h, "File");
    press(&mut h, "About");
    assert_eq!(name(&h).as_deref(), Some("about"));
    h.state_mut().open_dialog(NAME, json!({"page": "export"}));
    h.run_steps(3);
    assert!(h.query_by_label("PDF (.pdf)").is_none(), "offered only where the host delivers files");
    press(&mut h, "CSV (.csv)");
    assert_eq!(name(&h).as_deref(), Some("saveCopy"));
    assert_eq!(h.state().dialog.as_ref().unwrap().values.get("format"), Some(&json!("csv")));
}

#[test]
fn a_property_is_set_when_its_field_is_left() {
    let mut h = harness(app());
    let title = |h: &Harness| h.state().session.active().unwrap().wb.props.title.clone();
    h.state_mut().open_dialog(NAME, json!({"page": "info"}));
    h.run_steps(4);
    h.get_by_label("This workbook hasn't been saved to a file yet.");
    h.get_by_label("Title").focus();
    h.run_steps(2);
    h.get_by_label("Title").type_text("Plan for 2027");
    h.run_steps(2);
    assert_eq!(title(&h), "", "nothing is set while typing");
    h.key_press(Key::Enter);
    h.run_steps(3);
    assert_eq!(title(&h), "Plan for 2027");
    assert_eq!(page(&h).as_deref(), Some("info"), "the page stays");
    assert!(h.state().session.active().unwrap().is_dirty(), "it is a change to the workbook, and can be undone");
    h.state_mut().run("edit.undo", json!({})).unwrap();
    assert_eq!(title(&h), "");
    // Escape leaves the page without setting what was being typed.
    h.get_by_label("Subject").focus();
    h.run_steps(2);
    h.get_by_label("Subject").type_text("Dropped");
    h.run_steps(2);
    h.key_press(Key::Escape);
    h.run_steps(3);
    assert!(h.state().dialog.is_none());
    assert_eq!(h.state().session.active().unwrap().wb.props.subject, "");
}
