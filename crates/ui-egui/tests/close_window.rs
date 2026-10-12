//! Closing the window (its close button, Quit, Alt+F4) asks about each workbook with unsaved
//! changes before it closes; `app.quit` closes without asking.

use std::path::PathBuf;

use egui_kittest::kittest::Queryable as _;
use gridcraft_engine::Session;
use gridcraft_ui_egui::{Services, SheetApp, control};
use serde_json::json;

type Harness = egui_kittest::Harness<'static, SheetApp>;

fn harness(services: Services) -> Harness {
    let mut session = Session::new();
    session.new_workbook();
    let mut h = egui_kittest::Harness::builder().with_size(egui::vec2(1200.0, 800.0)).build_ui_state(
        |ui, app: &mut SheetApp| {
            let ctx = ui.ctx().clone();
            app.logic(&ctx);
            app.ui(ui);
        },
        SheetApp::new(session, services),
    );
    h.run_steps(4);
    h
}

fn temp_path(name: &str) -> PathBuf {
    std::env::temp_dir().join(format!("gridcraft-close-window-{}-{name}.xlsx", std::process::id()))
}

/// Types `input` into A1 of the active workbook, leaving it with unsaved changes.
fn change(h: &mut Harness, input: &str) {
    h.state_mut().run("cell.set", json!({"cell": "A1", "input": input})).unwrap();
}

/// The viewport commands the app sent during the last frame.
fn commands(h: &Harness) -> Vec<egui::ViewportCommand> {
    h.output().viewport_output.get(&egui::ViewportId::ROOT).map(|v| v.commands.clone()).unwrap_or_default()
}

/// Runs `n` frames and tells whether the app asked the window to close in any of them.
fn closes_within(h: &mut Harness, n: usize) -> bool {
    let mut closed = false;
    for _ in 0..n {
        h.step();
        closed |= commands(h).contains(&egui::ViewportCommand::Close);
    }
    closed
}

/// The window's close button: the platform asks the app whether the window may close. True when
/// the app let it close (it did not cancel the request).
fn request_close(h: &mut Harness) -> bool {
    h.input_mut().viewports.entry(egui::ViewportId::ROOT).or_default().events.push(egui::ViewportEvent::Close);
    h.step();
    let allowed = !commands(h).contains(&egui::ViewportCommand::CancelClose);
    h.run_steps(2);
    allowed
}

fn asking(h: &Harness) -> Option<String> {
    h.state().dialog.as_ref().filter(|d| d.name() == "saveChanges").and_then(|d| d.values.get("title").and_then(|t| t.as_str()).map(str::to_string))
}

/// Clicks a button of the Save Changes? prompt.
fn answer(h: &mut Harness, button: &str) {
    h.get_by_label("Save Changes?").get_by_role_and_label(egui::accesskit::Role::Button, button).click();
    h.step();
}

#[test]
fn a_saved_workbook_closes_without_asking() {
    let mut h = harness(Services::default());
    assert!(request_close(&mut h));
    assert_eq!(asking(&h), None);
}

#[test]
fn unsaved_changes_cancel_the_close_and_ask() {
    let mut h = harness(Services::default());
    change(&mut h, "1");
    assert!(!request_close(&mut h), "the close is cancelled");
    assert_eq!(asking(&h).as_deref(), Some("Book1"));
    // Asking again (a second click on the close button) keeps the one prompt.
    assert!(!request_close(&mut h));
    assert_eq!(asking(&h).as_deref(), Some("Book1"));
}

#[test]
fn cancel_keeps_the_window_open_and_the_changes() {
    let mut h = harness(Services::default());
    change(&mut h, "1");
    request_close(&mut h);
    answer(&mut h, "Cancel");
    assert!(!closes_within(&mut h, 4));
    assert_eq!(asking(&h), None);
    assert!(h.state().session.active().is_some_and(|d| d.is_dirty()));
    // The next close asks again.
    assert!(!request_close(&mut h));
    assert_eq!(asking(&h).as_deref(), Some("Book1"));
}

#[test]
fn dont_save_closes_the_window() {
    let mut h = harness(Services::default());
    change(&mut h, "1");
    request_close(&mut h);
    answer(&mut h, "Don't Save");
    assert!(closes_within(&mut h, 4));
    assert!(request_close(&mut h), "nothing unsaved is left");
}

#[test]
fn save_saves_and_closes_the_window() {
    let path = temp_path("save");
    let picked = path.to_str().unwrap().to_string();
    let mut h = harness(Services { pick_save: Some(Box::new(move |_| Some(picked.clone()))), ..Default::default() });
    change(&mut h, "42");
    request_close(&mut h);
    // A workbook without a file asks for a name.
    answer(&mut h, "Save");
    assert!(closes_within(&mut h, 4));
    let (wb, _) = gridcraft_engine::io::open_bytes("saved.xlsx", &std::fs::read(&path).unwrap()).unwrap();
    assert_eq!(wb.active().unwrap().value(gridcraft_engine::core::CellRef::new(0, 0)), gridcraft_engine::core::Value::Number(42.0));
    let _ = std::fs::remove_file(&path);
}

#[test]
fn save_commits_the_cell_being_edited() {
    let path = temp_path("draft");
    let mut h = harness(Services::default());
    h.state_mut().run("file.save", json!({"path": path.to_str().unwrap()})).unwrap();
    change(&mut h, "1");
    h.input_mut().events.push(egui::Event::Text("7".into()));
    h.run_steps(2);
    assert!(h.state().editor.is_some());
    request_close(&mut h);
    answer(&mut h, "Save");
    assert!(closes_within(&mut h, 4));
    let (wb, _) = gridcraft_engine::io::open_bytes("saved.xlsx", &std::fs::read(&path).unwrap()).unwrap();
    assert_eq!(wb.active().unwrap().value(gridcraft_engine::core::CellRef::new(0, 0)), gridcraft_engine::core::Value::Number(7.0));
    let _ = std::fs::remove_file(&path);
}

#[test]
fn a_save_that_does_not_happen_keeps_the_window_open() {
    // The user cancels the file picker.
    let mut h = harness(Services { pick_save: Some(Box::new(|_| None)), ..Default::default() });
    change(&mut h, "1");
    request_close(&mut h);
    answer(&mut h, "Save");
    assert!(!closes_within(&mut h, 4));
    assert_eq!(asking(&h), None);
    assert!(h.state().session.active().is_some_and(|d| d.is_dirty()));
}

#[test]
fn each_unsaved_workbook_is_asked_about_the_active_one_first() {
    let mut h = harness(Services::default());
    change(&mut h, "1");
    h.state_mut().run("file.new", json!({})).unwrap();
    h.state_mut().run("file.new", json!({})).unwrap();
    change(&mut h, "3");
    h.run_steps(2);
    // Book2 has no changes, Book1 and Book3 have.
    assert_eq!(h.state().session.documents().len(), 3);
    request_close(&mut h);
    assert_eq!(asking(&h).as_deref(), Some("Book3"));
    answer(&mut h, "Don't Save");
    h.run_steps(2);
    assert_eq!(asking(&h).as_deref(), Some("Book1"));
    answer(&mut h, "Don't Save");
    assert!(closes_within(&mut h, 4));
}

#[test]
fn cancel_after_dont_save_keeps_asking_about_the_rest() {
    let mut h = harness(Services::default());
    change(&mut h, "1");
    h.state_mut().run("file.new", json!({})).unwrap();
    change(&mut h, "2");
    h.run_steps(2);
    request_close(&mut h);
    assert_eq!(asking(&h).as_deref(), Some("Book2"));
    // Don't Save closes Book2; Cancel on Book1 stops closing the window.
    answer(&mut h, "Don't Save");
    h.run_steps(2);
    assert_eq!(asking(&h).as_deref(), Some("Book1"));
    answer(&mut h, "Cancel");
    assert!(!closes_within(&mut h, 4));
    let docs = h.state().session.documents();
    assert_eq!(docs.len(), 1);
    assert!(docs[0].is_dirty());
    // Book1 is still protected: the next close asks about it again.
    assert!(!request_close(&mut h));
    assert_eq!(asking(&h).as_deref(), Some("Book1"));
}

#[test]
fn app_quit_closes_without_asking() {
    let mut h = harness(Services::default());
    change(&mut h, "1");
    let ctx = h.ctx.clone();
    control::handle(h.state_mut(), &ctx, "app.quit", &json!({}));
    assert!(request_close(&mut h));
    assert_eq!(asking(&h), None);
    assert!(h.state().session.active().is_some_and(|d| d.is_dirty()));
}
