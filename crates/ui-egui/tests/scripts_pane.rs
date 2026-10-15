//! Automate › scripts in the interface: the Code Editor pane, running from the Macros dialog, and
//! Enable Content before a file's scripts run.

use egui::vec2;
use egui_kittest::kittest::Queryable;
use gridcraft_engine::Session;
use gridcraft_ui_egui::SheetApp;
use serde_json::json;

type Harness = egui_kittest::Harness<'static, SheetApp>;

fn harness(session: Session) -> Harness {
    let mut h = egui_kittest::Harness::builder().with_step_dt(1.0 / 60.0).with_size(vec2(1300.0, 850.0)).build_ui_state(
        |ui, app: &mut SheetApp| {
            app.logic(ui.ctx());
            app.ui(ui);
        },
        SheetApp::new(session, Default::default()),
    );
    h.run_steps(4);
    h
}

fn blank() -> Harness {
    let mut s = Session::new();
    s.new_workbook();
    harness(s)
}

fn value(h: &Harness, a1: &str) -> String {
    let d = h.state().session.active().unwrap();
    d.wb.active().unwrap().value(gridcraft_engine::core::CellRef::parse(a1).unwrap()).display()
}

#[test]
fn new_script_opens_the_code_editor_and_run_shows_the_output() {
    let mut h = blank();
    h.state_mut().run_or_alert("automate.newScript", json!({}));
    h.run_steps(3);
    assert_eq!(h.state().grid.pane.as_deref(), Some(gridcraft_ui_egui::scripts::PANE));
    assert_eq!(h.state().scripts.name.as_deref(), Some("Script 1"));
    assert!(h.state().scripts.code.starts_with("fn main(workbook) {"));
    // Edit and run: Run saves first, then the output shows under the code.
    h.state_mut().scripts.code =
        "fn main(workbook) {\n    workbook.getActiveWorksheet().getRange(\"B2\").setValue(7);\n    print(\"hello from the script\");\n}\n".into();
    h.run_steps(2);
    h.get_by_label("▶ Run").click();
    h.run_steps(4);
    assert_eq!(value(&h, "B2"), "7");
    assert!(!h.state().scripts.dirty());
    h.get_by_label("hello from the script");
    // An error names its line in the output.
    h.state_mut().scripts.code = "let x = 1;\nthrow \"broken\";\n".into();
    h.run_steps(2);
    h.get_by_label("▶ Run").click();
    h.run_steps(4);
    h.get_by_label("Line 2, column 1: broken");
    assert!(h.state().message.is_none(), "the error shows in the pane, not a message box");
    // Back to the list of all scripts.
    h.get_by_label("‹ All Scripts").click();
    h.run_steps(3);
    assert!(h.state().scripts.name.is_none());
    h.get_by_label("Script 1");
}

#[test]
fn the_macros_dialog_runs_a_script() {
    let mut h = blank();
    h.state_mut()
        .run("automate.saveScript", json!({"name": "Fill", "code": "workbook.getActiveWorksheet().getRange(\"A1\").setValue(\"done\");"}))
        .unwrap();
    h.state_mut().open_dialog("macros", json!({}));
    h.run_steps(3);
    h.get_by_label("Fill").click();
    h.run_steps(2);
    h.get_by_label("Run").click();
    h.run_steps(4);
    assert_eq!(value(&h, "A1"), "done");
    assert!(h.state().dialog.is_none());
}

#[test]
fn a_files_scripts_wait_for_enable_content() {
    // A workbook with a script, saved and opened again: its scripts are off until enabled.
    let mut s = Session::new();
    s.new_workbook();
    s.execute("automate.saveScript", json!({"name": "Mark", "code": "workbook.getActiveWorksheet().getRange(\"C3\").setValue(1);"})).unwrap();
    let dir = std::env::temp_dir().join(format!("gridcraft-ui-scripts-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("with-script.xlsx");
    s.execute("file.saveAs", json!({"path": path.to_string_lossy()})).unwrap();
    let mut h = blank();
    let p = path.to_string_lossy().to_string();
    h.state_mut().open_path(&p);
    h.run_steps(3);
    gridcraft_ui_egui::scripts::run(h.state_mut(), "Mark");
    h.run_steps(3);
    assert_eq!(value(&h, "C3"), "", "nothing runs before Enable Content");
    assert!(h.state().notice.is_some());
    h.get_by_label("Enable Content").click();
    h.run_steps(4);
    assert_eq!(value(&h, "C3"), "1", "the waiting script runs once content is enabled");
    assert!(h.state().notice.is_none());
    // From now on it runs straight away.
    h.state_mut().run("edit.undo", json!({})).unwrap();
    gridcraft_ui_egui::scripts::run(h.state_mut(), "Mark");
    h.run_steps(2);
    assert_eq!(value(&h, "C3"), "1");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn record_actions_opens_the_recording_in_the_code_editor() {
    let mut h = blank();
    h.state_mut().run("automate.recordActions", json!({})).unwrap();
    h.state_mut().run("cell.set", json!({"cell": "A1", "input": "x"})).unwrap();
    h.state_mut().run("ui.ribbonTab", json!({"tab": "Automate"})).unwrap();
    h.run_steps(3);
    // The ribbon shows the stop state while recording; stopping saves and opens the script.
    let r = h.state_mut().run("automate.recordActions", json!({})).unwrap();
    gridcraft_ui_egui::scripts::open(h.state_mut(), r["saved"].as_str());
    h.run_steps(3);
    assert!(h.state().scripts.code.contains("sheet.getRange(\"A1\").setValue(\"x\");"), "{}", h.state().scripts.code);
}

#[test]
fn automate_labels_are_german_in_german() {
    use gridcraft_ui_egui::i18n::Language;
    let de = Language::De;
    for (en, want) in [
        ("Automate", "Automatisieren"),
        ("Record\nActions", "Aktionen\naufzeichnen"),
        ("New\nScript", "Neues\nSkript"),
        ("All\nScripts", "Alle\nSkripts"),
        ("Code Editor", "Code-Editor"),
        ("Run", "Ausführen"),
        ("Save", "Speichern"),
        ("Output", "Ausgabe"),
        ("Enable Content", "Inhalt aktivieren"),
    ] {
        assert_eq!(de.tr(en), want, "{en:?}");
    }
}
