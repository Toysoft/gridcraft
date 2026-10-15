//! Language and region selection must affect UI entry, never the canonical engine/file boundary.
//! The words of a formula follow the interface language, its separators the region.
use gridcraft_engine::{
    Session,
    core::{CellError, CellRef, Value},
};
use gridcraft_ui_egui::{SheetApp, UiState, i18n::Language};
use serde_json::json;

fn app() -> SheetApp {
    let mut session = Session::new();
    session.new_workbook();
    let mut app = SheetApp::new(session, Default::default());
    app.ui.language = Language::En;
    app
}
fn formula(app: &SheetApp) -> String {
    app.session.active().unwrap().wb.active().unwrap().cell(CellRef::new(0, 0)).unwrap().formula.as_ref().unwrap().text.to_string()
}
fn enter(app: &mut SheetApp, text: &str) -> bool {
    app.begin_edit(Some(text.into()), false);
    app.commit_edit(0, 0, false, false)
}
/// An interface language with the region where it is spoken, as most of its users have it.
fn localize(app: &mut SheetApp, language: &str, region: &str) {
    app.run("app.language.set", json!({"code": language})).unwrap();
    app.run("app.region", json!({"region": region})).unwrap();
}

#[test]
fn language_controls_entry_display_persistence_and_xlsx_roundtrip() {
    let mut app = app();
    assert!(!enter(&mut app, "=SUM(1;2)"));
    localize(&mut app, "es", "es-ES");
    assert!(enter(&mut app, "=SI(VERDADERO;SUMA(1,5;2,25);0)"));
    assert_eq!(formula(&app), "IF(TRUE,SUM(1.5,2.25),0)");
    let saved = app.run("file.save", json!({"format":"xlsx"})).unwrap();
    app.run("file.open", json!({"name":"locale.xlsx", "base64":saved["base64"]})).unwrap();
    app.begin_edit(None, true);
    assert_eq!(app.editor.as_ref().unwrap().text, "=SI(VERDADERO;SUMA(1,5;2,25);0)");
    assert!(app.commit_edit(0, 0, false, false));
    assert_eq!(formula(&app), "IF(TRUE,SUM(1.5,2.25),0)");
    let state: UiState = serde_json::from_str(&serde_json::to_string(&app.ui).unwrap()).unwrap();
    assert_eq!(state.language, Language::Es);
    // English words, Spanish separators: the region still says `,` decimals and `;` arguments.
    app.run("app.language.set", json!({"language":"en"})).unwrap();
    app.begin_edit(None, false);
    assert_eq!(app.editor.as_ref().unwrap().text, "=IF(TRUE;SUM(1,5;2,25);0)");
    app.cancel_edit();
    app.run("app.region", json!({"region":"en-US"})).unwrap();
    app.begin_edit(None, false);
    assert_eq!(app.editor.as_ref().unwrap().text, "=IF(TRUE,SUM(1.5,2.25),0)");
}

#[test]
fn language_and_region_do_not_change_api_syntax_or_an_edit_already_in_progress() {
    let mut app = app();
    localize(&mut app, "es", "es-ES");
    assert!(app.run("cell.set", json!({"cell":"A1","input":"=SUM(1,2)"})).is_ok());
    assert!(app.run("cell.set", json!({"cell":"A2","input":"=SUMA(1;2)"})).is_err());
    app.begin_edit(Some("=SUMA(1,5;2)".into()), false);
    localize(&mut app, "en", "en-US");
    assert!(app.commit_edit(0, 0, false, false));
    assert_eq!(formula(&app), "SUM(1.5,2)");
}

#[test]
fn english_words_with_a_decimal_comma_region_and_local_words_with_a_us_region() {
    // Measured in Excel 365 with an English interface and a Croatian region (#447): `=SUM(1;2)`
    // and `=1,5*1,5` are formulas there. de-DE has the same separators.
    let mut app = app();
    app.run("app.region", json!({"region":"de-DE"})).unwrap();
    assert!(enter(&mut app, "=SUM(1,5;2)*1,5"));
    assert_eq!(formula(&app), "SUM(1.5,2)*1.5");
    assert_eq!(app.session.active().unwrap().wb.active().unwrap().value(CellRef::new(0, 0)), Value::Number(5.25));
    app.begin_edit(None, true);
    assert_eq!(app.editor.as_ref().unwrap().text, "=SUM(1,5;2)*1,5");
    app.cancel_edit();
    assert!(!enter(&mut app, "=SUM(1.5,2)"));
    // German words with a US region: German names, US separators.
    localize(&mut app, "de", "en-US");
    assert!(enter(&mut app, "=SUMME(1.5,WAHR)"));
    assert_eq!(formula(&app), "SUM(1.5,TRUE)");
    app.begin_edit(None, true);
    assert_eq!(app.editor.as_ref().unwrap().text, "=SUMME(1.5,WAHR)");
    app.cancel_edit();
    assert!(!enter(&mut app, "=SUMME(1;2)"));
    // The argument hint counts the region's list separator.
    app.begin_edit(Some("=SUMME(1.5,{1,2;3,4},".into()), false);
    assert_eq!(app.editor.as_ref().unwrap().current_function().unwrap().1, 2);
}

#[test]
fn defined_and_local_names_survive_localized_editing() {
    let mut app = app();
    app.run("formulas.defineName", json!({"name":"FALSO", "refersTo":"=7"})).unwrap();
    localize(&mut app, "es", "es-ES");
    assert!(enter(&mut app, "=LET(SUMA;LAMBDA(x;x+1);SUMA(2))+FALSO"));
    let original = formula(&app);
    app.begin_edit(None, false);
    assert_eq!(app.editor.as_ref().unwrap().text, "=LET(SUMA;LAMBDA(x;x+1);SUMA(2))+FALSO");
    assert!(app.commit_edit(0, 0, false, false));
    assert_eq!(formula(&app), original);
    assert_eq!(app.session.active().unwrap().wb.active().unwrap().value(CellRef::new(0, 0)), Value::Number(10.0));
}

#[test]
fn localized_argument_hints_ignore_decimal_commas_arrays_and_nested_calls() {
    let mut app = app();
    localize(&mut app, "es", "es-ES");
    for (text, expected) in [("=SI(1,5;SUMA(2;3);", 2), ("=SUMA({1,5\\2;3\\4};", 1), ("=SUMA(Table1[[a],[b]];", 1)] {
        app.begin_edit(Some(text.into()), false);
        assert_eq!(app.editor.as_ref().unwrap().current_function().unwrap().1, expected);
    }
}

#[test]
fn localized_autocomplete_is_rendered_and_inserts_the_selected_name() {
    let mut app = app();
    localize(&mut app, "es", "es-ES");
    let mut h = egui_kittest::Harness::builder().with_size(egui::vec2(1200.0, 800.0)).build_ui_state(
        |ui, app: &mut SheetApp| {
            let ctx = ui.ctx().clone();
            app.logic(&ctx);
            app.ui(ui);
        },
        app,
    );
    h.run_steps(4);
    h.state_mut().begin_edit(Some("=SU".into()), true);
    h.run_steps(4);
    h.input_mut().events.push(egui::Event::Text("M".into()));
    h.run_steps(4);
    assert!(h.state().editor.as_ref().unwrap().autocomplete.iter().any(|n| n == "SUMA"));
    assert!(!h.state().editor.as_ref().unwrap().autocomplete.iter().any(|n| n == "SUM"));
    if let Ok(path) = std::env::var("GRIDCRAFT_LOCALE_SCREENSHOT") {
        h.render().unwrap().save(path).unwrap();
    }
    let ed = h.state_mut().editor.as_mut().unwrap();
    ed.ac_index = ed.autocomplete.iter().position(|n| n == "SUMA").unwrap();
    assert!(ed.accept_autocomplete());
    assert_eq!(ed.text, "=SUMA(");
    h.run_steps(4);
    h.input_mut().events.push(egui::Event::Text("1,5;".into()));
    h.run_steps(4);
    assert_eq!(h.state().editor.as_ref().unwrap().current_function(), Some(("SUMA".into(), 1)));
    if let Ok(path) = std::env::var("GRIDCRAFT_LOCALE_SCREENSHOT") {
        h.render().unwrap().save(std::path::Path::new(&path).with_file_name("spanish-hints.png")).unwrap();
        h.state_mut().cancel_edit();
        h.state_mut().ui.ribbon_tab = "View".into();
        h.run_steps(4);
        h.render().unwrap().save(std::path::Path::new(&path).with_file_name("spanish-setting.png")).unwrap();
    }
    h.state_mut().cancel_edit();
    h.state_mut().run("app.language.set", json!({"language":"en"})).unwrap();
    h.state_mut().begin_edit(Some("=SU".into()), true);
    h.run_steps(4);
    h.input_mut().events.push(egui::Event::Text("M".into()));
    h.run_steps(4);
    assert!(h.state().editor.as_ref().unwrap().autocomplete.iter().any(|n| n == "SUM"));
    assert!(!h.state().editor.as_ref().unwrap().autocomplete.iter().any(|n| n == "SUMA"));
}

#[test]
fn locale_preserves_text_cells_and_existing_entry_conveniences() {
    let mut app = app();
    app.run("app.language.set", json!({"language":"es"})).unwrap();
    // Spanish words with the default en-US region: US separators.
    assert!(enter(&mut app, "=SUMA(1.5,2"));
    assert_eq!(formula(&app), "SUM(1.5,2)");
    // The interface language does not decide how a constant is read: the region does.
    assert!(enter(&mut app, "-1,5"));
    assert_eq!(app.session.active().unwrap().wb.active().unwrap().value(CellRef::new(0, 0)), Value::text("-1,5"));
    app.run("app.region", json!({"region":"es-ES"})).unwrap();
    assert!(enter(&mut app, "-1,5"));
    assert_eq!(app.session.active().unwrap().wb.active().unwrap().value(CellRef::new(0, 0)), Value::Number(-1.5));
    app.run("home.numberFormat", json!({"format":"@"})).unwrap();
    assert!(enter(&mut app, "=SUM(1,2)"));
    assert_eq!(app.session.active().unwrap().wb.active().unwrap().value(CellRef::new(0, 0)), Value::text("=SUM(1,2)"));
    app.begin_edit(None, true);
    assert_eq!(app.editor.as_ref().unwrap().text, "=SUM(1,2)");
}

#[test]
fn canonical_engine_templates_enter_the_selected_formula_language() {
    let mut app = app();
    localize(&mut app, "es", "es-ES");
    app.run("formulas.insertFunction", json!({"name":"SUM"})).unwrap();
    assert_eq!(app.editor.as_ref().unwrap().text, "=SUMA(");
    app.cancel_edit();
    app.run("cell.set", json!({"cell":"A1","input":"2"})).unwrap();
    app.run("selection.set", json!({"range":"A2"})).unwrap();
    app.run("formulas.autoSum", json!({"enter":false})).unwrap();
    assert!(app.editor.as_ref().unwrap().text.starts_with("=SUMA("));
    app.cancel_edit();
    // Text supplied by the user is already in the selected UI syntax; do not translate it twice.
    app.begin_edit(Some("=SUMA(1,5;2)".into()), false);
    assert_eq!(app.editor.as_ref().unwrap().text, "=SUMA(1,5;2)");
    assert!(app.commit_edit(0, 0, false, false));
    if let Ok(path) = std::env::var("GRIDCRAFT_LOCALE_SCREENSHOT") {
        app.ui.ribbon_tab = "Formulas".into();
        app.run("formulas.insertFunction", json!({"name":"SUM"})).unwrap();
        let mut h = egui_kittest::Harness::builder().with_size(egui::vec2(1200.0, 800.0)).build_ui_state(
            |ui, app: &mut SheetApp| {
                let ctx = ui.ctx().clone();
                app.logic(&ctx);
                app.ui(ui);
            },
            app,
        );
        h.run_steps(5);
        h.render().unwrap().save(std::path::Path::new(&path).with_file_name("spanish-ribbon-template.png")).unwrap();
    }
}

#[test]
fn portuguese_interface_enters_and_shows_portuguese_formulas() {
    let mut app = app();
    localize(&mut app, "pt-BR", "pt-BR");
    assert_eq!(app.ui.language, Language::PtBr);
    assert_eq!(app.ui.language.tr("Data"), "Dados");
    assert!(enter(&mut app, "=SOMA(1,5;2,25)"));
    assert_eq!(formula(&app), "SUM(1.5,2.25)");
    app.begin_edit(None, true);
    assert_eq!(app.editor.as_ref().unwrap().text, "=SOMA(1,5;2,25)");
    app.cancel_edit();
    // English separators are not the pt-BR region's.
    assert!(!enter(&mut app, "=SUM(1.5,2.25)"));
    let restored: UiState = serde_json::from_str(&serde_json::to_string(&app.ui).unwrap()).unwrap();
    assert_eq!(restored.language, Language::PtBr);
    assert_eq!(Language::Es.tr("Page Layout|Orientation"), "Orientación");
}

#[test]
fn german_and_russian_interfaces_enter_and_show_their_formulas() {
    for (code, region, local, canonical, value) in [
        ("de", "de-DE", "=WENN(WAHR;SUMME({1.2;3,5.4});#NV)", "IF(TRUE,SUM({1,2;3.5,4}),#N/A)", 10.5),
        ("ru", "ru-RU", "=ЕСЛИ(ИСТИНА;СУММ({1;2:3;4});#Н/Д)", "IF(TRUE,SUM({1,2;3,4}),#N/A)", 10.0),
    ] {
        let mut app = app();
        localize(&mut app, code, region);
        assert!(enter(&mut app, local), "{code}");
        assert_eq!(formula(&app), canonical, "{code}");
        assert_eq!(app.session.active().unwrap().wb.active().unwrap().value(CellRef::new(0, 0)), Value::Number(value), "{code}");
        app.begin_edit(None, true);
        assert_eq!(app.editor.as_ref().unwrap().text, local, "{code}");
        app.cancel_edit();
    }
}

#[test]
fn czech_interface_keeps_english_names_with_the_region_separators() {
    // The Czech table translates interface labels only: formulas keep canonical English names
    // until a Czech formula locale exists. Their separators are the region's.
    let mut app = app();
    app.run("app.language.set", json!({"code":"cs-CZ"})).unwrap();
    assert_eq!(app.ui.language, Language::Cs);
    assert_eq!(app.ui.language.tr("Formulas"), "Vzorce");
    assert!(enter(&mut app, "=SUM(1.5,2.25)"));
    app.begin_edit(None, true);
    assert_eq!(app.editor.as_ref().unwrap().text, "=SUM(1.5,2.25)");
    app.cancel_edit();
    assert!(!enter(&mut app, "=SUMA(1;2)"));
    // Czech Excel 2010 and later: `;` arguments, `,` decimals, `\` array columns.
    app.run("app.region", json!({"region":"cs-CZ"})).unwrap();
    assert!(enter(&mut app, "=SUM(1,5;{1\\2;3\\4})"));
    assert_eq!(formula(&app), "SUM(1.5,{1,2;3,4})");
    let restored: UiState = serde_json::from_str(&serde_json::to_string(&app.ui).unwrap()).unwrap();
    assert_eq!(restored.language, Language::Cs);
}

#[test]
fn user_defined_prefix_is_hidden_while_editing_and_kept_on_commit() {
    // Excel shows `_xludf.EOMONTH()` as `=EOMONTH()`; the cell still holds the user-defined call
    // (#726), and committing the edit unchanged must not turn it into the built-in.
    let mut app = app();
    app.run("cell.set", json!({"cell":"A1","input":"=_xludf.EOMONTH()&_xludf.CONCAT(\"_xludf.x\")"})).unwrap();
    app.begin_edit(None, true);
    assert_eq!(app.editor.as_ref().unwrap().text, "=EOMONTH()&CONCAT(\"_xludf.x\")");
    assert!(app.commit_edit(0, 0, false, false));
    assert_eq!(formula(&app), "_xludf.EOMONTH()&_xludf.CONCAT(\"_xludf.x\")");
    assert_eq!(app.session.active().unwrap().wb.active().unwrap().value(CellRef::new(0, 0)), Value::Error(CellError::Name));
    app.run("app.language.set", json!({"language":"es"})).unwrap();
    app.begin_edit(None, true);
    assert_eq!(app.editor.as_ref().unwrap().text, "=EOMONTH()&CONCAT(\"_xludf.x\")");
    assert!(app.commit_edit(0, 0, false, false));
    assert_eq!(formula(&app), "_xludf.EOMONTH()&_xludf.CONCAT(\"_xludf.x\")");
}

#[test]
fn a_number_the_region_reads_is_a_constant_in_the_formula_language_too() {
    // Spanish formulas and a pt-BR region: `-R$ 5` starts like a formula (`-` and a letter) but is
    // a currency amount the region reads, so the editor must hand it to the engine untouched.
    let mut app = app();
    app.run("app.language.set", json!({"language":"es"})).unwrap();
    app.run("app.region", json!({"region":"pt-BR"})).unwrap();
    assert!(enter(&mut app, "-R$ 5"));
    let cell = |app: &SheetApp| app.session.active().unwrap().wb.active().unwrap().value(CellRef::new(0, 0));
    assert_eq!(cell(&app), Value::Number(-5.0));
    // A real formula still goes through the Spanish parser.
    assert!(enter(&mut app, "=SUMA(1,5;2)"));
    assert_eq!(formula(&app), "SUM(1.5,2)");
    // The same text in en-US is not a number, so it is a (Spanish) formula and is refused.
    app.run("app.region", json!({"region":"en-US"})).unwrap();
    assert!(!enter(&mut app, "-R$ 5"));
}
