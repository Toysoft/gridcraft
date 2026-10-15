//! A German interface enters and shows numbers, dates, booleans, errors and formulas as German
//! Excel does (observed in Excel 365 de-DE), while workbooks, files and engine commands stay
//! canonical.
use egui::vec2;
use egui_kittest::kittest::Queryable;
use gridcraft_engine::Session;
use gridcraft_engine::core::{CellError, CellRef, Value};
use gridcraft_ui_egui::control::{Outcome, handle};
use gridcraft_ui_egui::{SheetApp, UiState, i18n::Language};
use serde_json::{Value as Json, json};

fn app(language: &str) -> SheetApp {
    let mut session = Session::new();
    session.new_workbook();
    let mut app = SheetApp::new(session, Default::default());
    app.run("app.language.set", json!({"language": language})).unwrap();
    app
}

fn enter(app: &mut SheetApp, cell: &str, text: &str) -> bool {
    app.run("selection.set", json!({"cell": cell})).unwrap();
    app.begin_edit(Some(text.into()), false);
    app.commit_edit(0, 0, false, false)
}

fn value(app: &SheetApp, cell: &str) -> Value {
    app.session.active().unwrap().wb.active().unwrap().value(CellRef::parse(cell).unwrap())
}

fn format(app: &SheetApp, cell: &str) -> String {
    let d = app.session.active().unwrap();
    let sh = d.wb.active().unwrap();
    d.wb.styles.get(sh.style_id(CellRef::parse(cell).unwrap())).num_fmt.as_str().to_string()
}

fn edit_text(app: &mut SheetApp, cell: &str) -> String {
    app.run("selection.set", json!({"cell": cell})).unwrap();
    app.begin_edit(None, false);
    let text = app.editor.as_ref().unwrap().text.clone();
    app.cancel_edit();
    text
}

fn serial(y: i64, m: i64, d: i64) -> f64 {
    gridcraft_engine::core::date::serial_from_ymd(gridcraft_engine::core::DateSystem::D1900, y, m, d).unwrap()
}

#[test]
fn german_typing_enters_what_german_excel_enters() {
    let mut app = app("de");
    let year = gridcraft_engine::core::parse::current_year();
    for (cell, typed, expected, fmt) in [
        ("A1", "1,5", Value::Number(1.5), "General"),
        ("A2", "1.234,5", Value::Number(1234.5), "#,##0.00"),
        ("A3", "-3,2%", Value::Number(-0.032), "0.00%"),
        ("A4", "1,5E+3", Value::Number(1500.0), "0.00E+00"),
        ("A5", "13.10.2026", Value::Number(serial(2026, 10, 13)), "m/d/yyyy"),
        ("A6", "13.10.26", Value::Number(serial(2026, 10, 13)), "m/d/yyyy"),
        ("A7", "13.10", Value::Number(serial(year, 10, 13)), "d-mmm"),
        ("A8", "14:30", Value::Number(14.5 / 24.0), "h:mm"),
        ("A9", "WAHR", Value::Bool(true), "General"),
        ("A10", "falsch", Value::Bool(false), "General"),
        ("A11", "#NV", Value::Error(CellError::NA), "General"),
        ("A12", "#WERT!", Value::Error(CellError::Value), "General"),
        // English words and en-US spellings are text in German Excel.
        ("A13", "TRUE", Value::text("TRUE"), "General"),
        ("A14", "#N/A", Value::text("#N/A"), "General"),
        ("A15", "10/13/2026", Value::text("10/13/2026"), "General"),
        // Observed: a day and month with a trailing dot and no year stay text.
        ("A16", "13.10.", Value::text("13.10."), "General"),
        ("A17", "5 €", Value::Number(5.0), "#,##0 \"€\";[Red]-#,##0 \"€\""),
        ("A18", "13. Okt 2026", Value::Number(serial(2026, 10, 13)), "d-mmm-yy"),
        ("A19", "13.10.2026 14:30", Value::Number(serial(2026, 10, 13) + 14.5 / 24.0), "m/d/yyyy h:mm"),
    ] {
        assert!(enter(&mut app, cell, typed), "{typed}");
        let v = value(&app, cell);
        match (&v, &expected) {
            (Value::Number(a), Value::Number(b)) => assert!((a - b).abs() < 1e-9, "{typed}: {a} != {b}"),
            _ => assert_eq!(v, expected, "{typed}"),
        }
        assert_eq!(format(&app, cell), fmt, "{typed}");
    }
    // The editor starts from German text that reads back as the same value.
    for (cell, shown) in [("A1", "1,5"), ("A2", "1234,5"), ("A5", "13.10.2026"), ("A9", "WAHR"), ("A11", "#NV"), ("A18", "13. Okt 26")] {
        assert_eq!(edit_text(&mut app, cell), shown, "{cell}");
    }
    let before = value(&app, "A2");
    app.run("selection.set", json!({"cell": "A2"})).unwrap();
    app.begin_edit(None, false);
    assert!(app.commit_edit(0, 0, false, false));
    assert_eq!(value(&app, "A2"), before);
    // German formulas still work, with decimal commas.
    assert!(enter(&mut app, "B1", "=SUMME(A1;2,5)"));
    assert_eq!(value(&app, "B1"), Value::Number(4.0));
    // Engine commands stay canonical whatever the interface shows.
    app.run("cell.set", json!({"cell": "C1", "input": "1.5"})).unwrap();
    assert_eq!(value(&app, "C1"), Value::Number(1.5));
}

#[test]
fn english_typing_is_unchanged() {
    let mut app = app("en");
    for (cell, typed, expected) in [
        ("A1", "1,234", Value::Number(1234.0)),
        ("A2", "1,5", Value::text("1,5")),
        ("A3", "TRUE", Value::Bool(true)),
        ("A4", "10/13/2026", Value::Number(serial(2026, 10, 13))),
    ] {
        assert!(enter(&mut app, cell, typed));
        assert_eq!(value(&app, cell), expected, "{typed}");
    }
    assert_eq!(edit_text(&mut app, "A4"), "10/13/2026");
    assert_eq!(edit_text(&mut app, "A3"), "TRUE");
}

#[test]
fn other_formula_languages_keep_en_us_numbers_and_use_their_words() {
    // Spanish has a formula table but no observed regional format yet.
    let mut app = app("es");
    assert!(enter(&mut app, "A1", "1,5"));
    assert_eq!(value(&app, "A1"), Value::Number(1.5));
    assert_eq!(edit_text(&mut app, "A1"), "1.5");
    assert!(enter(&mut app, "A2", "verdadero"));
    assert_eq!(value(&app, "A2"), Value::Bool(true));
    assert_eq!(edit_text(&mut app, "A2"), "VERDADERO");
    assert!(enter(&mut app, "A3", "10/13/2026"));
    assert_eq!(edit_text(&mut app, "A3"), "10/13/2026");
}

#[test]
fn use_system_separators_is_a_preference_not_a_workbook_setting() {
    let mut app = app("de");
    assert_eq!(app.run("options.separators", json!({})).unwrap(), json!({"useSystem": true, "decimal": ",", "thousands": "."}));
    // Excel's Swiss-style separators with the German interface.
    app.run("options.separators", json!({"useSystem": false, "decimal": ".", "thousands": "'"})).unwrap();
    assert!(enter(&mut app, "A1", "1'234.5"));
    assert_eq!(value(&app, "A1"), Value::Number(1234.5));
    assert_eq!(edit_text(&mut app, "A1"), "1234.5");
    // Dates keep the region's order.
    assert!(enter(&mut app, "A2", "13.10.2026"));
    assert_eq!(value(&app, "A2"), Value::Number(serial(2026, 10, 13)));
    assert!(app.run("options.separators", json!({"decimal": "'"})).is_err());
    // Saved in ui.json, read back leniently.
    let restored: UiState = serde_json::from_str(&serde_json::to_string(&app.ui).unwrap()).unwrap();
    assert!(!restored.separators.use_system);
    assert_eq!(restored.separators.decimal, ".");
    let odd: UiState = serde_json::from_str(r#"{"dark": true, "separators": "garbage"}"#).unwrap();
    assert!(odd.dark && odd.separators.use_system);
    // The option is not part of the workbook: its file is the same either way.
    let xlsx = |app: &mut SheetApp| app.run("file.saveBytes", json!({"format": "xlsx"})).unwrap()["base64"].clone();
    let with_option = xlsx(&mut app);
    app.run("options.separators", json!({"useSystem": true})).unwrap();
    assert_eq!(xlsx(&mut app), with_option);
}

#[test]
fn csv_saved_from_a_german_interface_uses_semicolons_and_german_numbers() {
    let mut app = app("de");
    app.run("range.setValues", json!({"range": "A1", "values": [[1.5, true]]})).unwrap();
    let csv = app.run("file.saveBytes", json!({"format": "csv"})).unwrap();
    let bytes = gridcraft_engine::io::base64_decode(csv["base64"].as_str().unwrap()).unwrap();
    assert_eq!(String::from_utf8(bytes).unwrap(), "1,5;WAHR\r\n");
    app.run("app.language.set", json!({"language": "en"})).unwrap();
    let csv = app.run("file.saveBytes", json!({"format": "csv"})).unwrap();
    let bytes = gridcraft_engine::io::base64_decode(csv["base64"].as_str().unwrap()).unwrap();
    assert_eq!(String::from_utf8(bytes).unwrap(), "1.5,TRUE\r\n");
}

fn control(app: &mut SheetApp, method: &str, params: Json) -> Json {
    let ctx = egui::Context::default();
    match handle(app, &ctx, method, &params) {
        Outcome::Done(v) => v,
        Outcome::Screenshot { .. } => json!(null),
    }
}

#[test]
fn formula_fields_in_dialogs_speak_german() {
    let mut app = app("de");
    // Name Manager › New: Refers to is typed in German and stored canonically.
    app.open_dialog("defineName", json!({}));
    control(&mut app, "ui.dialog.set", json!({"field": "name", "value": "Preis"}));
    control(&mut app, "ui.dialog.set", json!({"field": "refersTo", "value": "=SUMME(Tabelle1!$A$1:$A$3;2,5)"}));
    let r = control(&mut app, "ui.dialog.confirm", json!({}));
    assert_eq!(r["ok"], json!(true), "{r}");
    let names = app.run("formulas.nameManager", json!({})).unwrap();
    assert_eq!(names[0]["refersTo"], "=SUM(Tabelle1!$A$1:$A$3,2.5)");
    // Edit… shows it in German again.
    app.open_dialog("defineName", json!({"name": "Preis", "refersTo": names[0]["refersTo"], "oldName": "Preis"}));
    assert_eq!(app.dialog.as_ref().unwrap().values["refersTo"], "=SUMME(Tabelle1!$A$1:$A$3;2,5)");
    app.dialog = None;
    // Data validation: a custom formula and decimal bounds.
    app.run("selection.set", json!({"cell": "B2"})).unwrap();
    app.open_dialog("dataValidation", json!({}));
    control(&mut app, "ui.dialog.set", json!({"field": "type", "value": "custom"}));
    control(&mut app, "ui.dialog.set", json!({"field": "formula1", "value": "=ISTZAHL(B2)"}));
    assert_eq!(control(&mut app, "ui.dialog.confirm", json!({}))["ok"], json!(true));
    app.open_dialog("dataValidation", json!({}));
    assert_eq!(app.dialog.as_ref().unwrap().values["formula1"], "ISTZAHL(B2)");
    control(&mut app, "ui.dialog.set", json!({"field": "type", "value": "decimal"}));
    control(&mut app, "ui.dialog.set", json!({"field": "operator", "value": "between"}));
    control(&mut app, "ui.dialog.set", json!({"field": "formula1", "value": "1,5"}));
    control(&mut app, "ui.dialog.set", json!({"field": "formula2", "value": "2,5"}));
    assert_eq!(control(&mut app, "ui.dialog.confirm", json!({}))["ok"], json!(true));
    let doc = app.session.active().unwrap();
    let dv = &doc.wb.active().unwrap().validations[0];
    assert_eq!((dv.f1.as_str(), dv.f2.as_deref()), ("1.5", Some("2.5")));
    app.open_dialog("dataValidation", json!({}));
    assert_eq!(app.dialog.as_ref().unwrap().values["formula1"], "1,5");
    app.dialog = None;
    // A list's items are separated by `;`.
    app.run("selection.set", json!({"cell": "C3"})).unwrap();
    app.open_dialog("dataValidation", json!({}));
    control(&mut app, "ui.dialog.set", json!({"field": "type", "value": "list"}));
    control(&mut app, "ui.dialog.set", json!({"field": "formula1", "value": "Ja;Nein"}));
    assert_eq!(control(&mut app, "ui.dialog.confirm", json!({}))["ok"], json!(true));
    let doc = app.session.active().unwrap();
    assert!(doc.wb.active().unwrap().validations.iter().any(|dv| dv.f1 == "Ja,Nein"));
}

#[test]
fn the_options_page_offers_the_separators_with_german_labels() {
    let mut session = Session::new();
    session.new_workbook();
    let mut app = SheetApp::new(session, Default::default());
    app.ui.language = Language::De;
    let mut h = egui_kittest::Harness::builder().with_size(vec2(1200.0, 1100.0)).build_ui_state(
        |ui, app: &mut SheetApp| {
            app.logic(&ui.ctx().clone());
            app.ui(ui);
        },
        app,
    );
    h.run_steps(4);
    h.state_mut().run("ui.backstage", json!({"page": "options"})).unwrap();
    h.run_steps(6);
    h.get_by_label("Erweitert");
    h.get_by_label("Dezimaltrennzeichen:");
    h.get_by_label("Tausendertrennzeichen:");
    h.get_by_label("Trennzeichen vom Betriebssystem übernehmen").click();
    h.run_steps(4);
    assert!(!h.state().ui.separators.use_system);
    if let Ok(path) = std::env::var("GRIDCRAFT_LOCALE_SCREENSHOT") {
        h.render().unwrap().save(std::path::Path::new(&path).with_file_name("german-options.png")).unwrap();
    }
}
