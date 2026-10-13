//! Excel keyboard shortcuts (crate::keymap) dispatched through real egui key events.

use egui::{Event, Key, Modifiers};
use gridcraft_engine::Session;
use gridcraft_engine::core::CellRef;
use gridcraft_engine::model::BorderStyle;
use gridcraft_ui_egui::SheetApp;
use gridcraft_ui_egui::i18n::Language;
use gridcraft_ui_egui::keymap::{Shortcut, lookup, lookup_in};
use serde_json::json;

type Harness = egui_kittest::Harness<'static, SheetApp>;

fn harness(session: Session) -> Harness {
    let mut h = Harness::builder().with_size(egui::vec2(1200.0, 800.0)).build_ui_state(
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

fn book(selected: &str) -> Session {
    let mut s = Session::new();
    s.new_workbook();
    s.execute("range.setValues", json!({"range": "A1", "values": [[1.5, 2], [3, 4], [5, 6]]})).unwrap();
    s.execute("cell.set", json!({"cell": "C1", "input": "=A1*2"})).unwrap();
    s.execute("selection.set", json!({"range": selected})).unwrap();
    s
}

/// A key press as a US keyboard reports it (the physical key is the unshifted one).
fn key(h: &mut Harness, k: Key, m: Modifiers) {
    h.input_mut().events.push(Event::Key { key: k, physical_key: Some(k), pressed: true, repeat: false, modifiers: m });
    h.input_mut().events.push(Event::Key { key: k, physical_key: Some(k), pressed: false, repeat: false, modifiers: m });
    h.run_steps(2);
}

fn style(h: &Harness, a1: &str) -> gridcraft_engine::model::Style {
    let d = h.state().session.active().unwrap();
    let sh = d.wb.active().unwrap();
    d.wb.styles.get(sh.style_id(CellRef::parse(a1).unwrap())).clone()
}

/// Ctrl on Windows and Linux sets both `ctrl` and `command`.
const CTRL: Modifiers = Modifiers { alt: false, ctrl: true, shift: false, mac_cmd: false, command: true };
const CTRL_SHIFT: Modifiers = Modifiers { shift: true, ..CTRL };

#[test]
fn ctrl_shift_symbols_apply_number_formats_and_borders() {
    let mut h = harness(book("A1:B2"));
    key(&mut h, Key::Num4, CTRL_SHIFT);
    assert_eq!(style(&h, "B2").num_fmt.as_str(), "$#,##0.00_);($#,##0.00)");
    key(&mut h, Key::Num5, CTRL_SHIFT);
    assert_eq!(style(&h, "A1").num_fmt.as_str(), "0%");
    key(&mut h, Key::Backtick, CTRL_SHIFT);
    assert_eq!(style(&h, "A1").num_fmt.as_str(), "General");
    key(&mut h, Key::Num7, CTRL_SHIFT);
    assert!(style(&h, "A1").borders.top.style != BorderStyle::None && style(&h, "B2").borders.right.style != BorderStyle::None);
    key(&mut h, Key::Minus, CTRL_SHIFT);
    assert!(style(&h, "A1").borders.top.style == BorderStyle::None);
    // Ctrl+Shift+End extends from the anchor to the last used cell.
    key(&mut h, Key::End, CTRL_SHIFT);
    assert_eq!(h.state().session.active().unwrap().selection.current().a1(), "A1:C3");
}

#[test]
fn copy_formula_from_above_and_function_keys() {
    let mut h = harness(book("C2"));
    key(&mut h, Key::Quote, CTRL);
    assert_eq!(h.state().editor.as_ref().map(|e| e.text.clone()).as_deref(), Some("=A1*2"));
    key(&mut h, Key::Escape, Modifiers::NONE);
    assert!(h.state().editor.is_none());
    let sheets = h.state().session.active().unwrap().wb.sheets.len();
    key(&mut h, Key::F11, Modifiers::SHIFT);
    assert_eq!(h.state().session.active().unwrap().wb.sheets.len(), sheets + 1);
    // Back on the data: Alt+F1 charts the current region, Ctrl+Alt+= zooms in.
    h.state_mut().session.run("sheet.previous", json!({})).unwrap();
    h.state_mut().session.run("selection.set", json!({"range": "A1:B3"})).unwrap();
    key(&mut h, Key::F1, Modifiers::ALT);
    assert_eq!(h.state().session.active().unwrap().wb.sheets[0].charts.len(), 1);
    key(&mut h, Key::Equals, Modifiers { alt: true, ..CTRL });
    assert_eq!(h.state().session.active().unwrap().wb.sheets[0].zoom, 110);
}

#[test]
fn windows_and_mac_variants() {
    let cmd = CTRL;
    let mac_cmd = Modifiers { mac_cmd: true, command: true, ..Modifiers::NONE };
    let ctrl = Modifiers::CTRL;
    let run = |id: &'static str| Some(Shortcut::Run(id, json!({})));
    assert_eq!(lookup(Key::L, Some(Key::L), cmd, false), run("insert.table"));
    assert_eq!(lookup(Key::L, Some(Key::L), mac_cmd, true), run("home.alignLeft"));
    assert_eq!(lookup(Key::E, Some(Key::E), mac_cmd, true), run("home.alignCenter"));
    assert_eq!(lookup(Key::E, Some(Key::E), ctrl, true), run("edit.flashFill"));
    assert_eq!(lookup(Key::Equals, Some(Key::Equals), Modifiers::ALT, false), run("formulas.autoSum"));
    // The shifted character doesn't matter: Ctrl+Shift+1 is the Number format wherever `!` is.
    assert_eq!(
        lookup(Key::Exclamationmark, Some(Key::Num1), CTRL_SHIFT, false),
        Some(Shortcut::Run("home.numberFormat", json!({"code": "#,##0.00"})))
    );
    // Navigation keys go by what they are, not where: the numpad's arrows and Page Down (Num
    // Lock off) report Num6 / Num3 as their physical keys.
    let alt_shift = Modifiers { alt: true, shift: true, ..Modifiers::NONE };
    assert_eq!(lookup(Key::ArrowRight, Some(Key::Num6), alt_shift, false), run("data.group"));
    assert_eq!(lookup(Key::ArrowLeft, Some(Key::Num4), alt_shift, false), run("data.ungroup"));
    assert_eq!(lookup(Key::PageDown, Some(Key::Num3), Modifiers::ALT, false), Some(Shortcut::ScreenRight));
    assert_eq!(lookup(Key::PageDown, Some(Key::Num3), CTRL_SHIFT, false), None);
    // Plain keys stay with the grid (typing, navigation).
    assert_eq!(lookup(Key::A, Some(Key::A), Modifiers::NONE, false), None);
    assert_eq!(lookup(Key::F2, Some(Key::F2), Modifiers::NONE, false), None);
}

#[test]
fn space_selection_shortcuts_do_not_start_an_edit() {
    let mut h = harness(book("B2"));
    // Shift+Space: the key press, then the space the OS types with it.
    let shift = Modifiers::SHIFT;
    h.input_mut().events.push(Event::Key { key: Key::Space, physical_key: Some(Key::Space), pressed: true, repeat: false, modifiers: shift });
    h.input_mut().events.push(Event::Text(" ".into()));
    h.run_steps(2);
    assert!(h.state().editor.is_none(), "Shift+Space started an edit");
    assert_eq!(h.state().session.active().unwrap().selection.ranges[0].a1(), "2:2");
    h.state_mut().session.run("selection.set", json!({"range": "B2"})).unwrap();
    // Ctrl+Space selects the column, also without starting an edit.
    h.input_mut().events.push(Event::Key { key: Key::Space, physical_key: Some(Key::Space), pressed: true, repeat: false, modifiers: CTRL });
    h.input_mut().events.push(Event::Text(" ".into()));
    h.run_steps(2);
    assert!(h.state().editor.is_none());
    assert_eq!(h.state().session.active().unwrap().selection.ranges[0].a1(), "B:B");
    // Plain typing still edits.
    h.input_mut().events.push(Event::Text("x".into()));
    h.run_steps(2);
    assert!(h.state().editor.is_some());
}

#[test]
fn space_shortcuts_per_platform() {
    let run = |id: &'static str| Some(Shortcut::Run(id, json!({})));
    let mac_cmd = Modifiers { mac_cmd: true, command: true, ..Modifiers::NONE };
    let space = |m, mac| lookup(Key::Space, Some(Key::Space), m, mac);
    assert_eq!(space(Modifiers::SHIFT, false), run("selection.row"));
    assert_eq!(space(Modifiers::SHIFT, true), run("selection.row"));
    assert_eq!(space(CTRL, false), run("selection.column"));
    assert_eq!(space(Modifiers::CTRL, true), run("selection.column"));
    // Cmd+Space is Spotlight on macOS.
    assert_eq!(space(mac_cmd, true), None);
}

#[test]
fn alt_down_opens_autofilter_headers_without_changing_the_workbook() {
    for rtl in [false, true] {
        for cell in ["A1", "B1"] {
            let mut session = book(cell);
            session.execute("data.filter", json!({"range": "A1:B3"})).unwrap();
            session.execute("view.rightToLeft", json!({"on": rtl})).unwrap();
            let mut h = harness(session);
            let before = h.state().session.active().unwrap().wb.clone();
            key(&mut h, Key::ArrowDown, Modifiers::ALT);
            let (column, _row, at) = h.state().grid.filter_menu.expect("Alt+Down must open the filter header menu");
            let d = h.state().session.active().unwrap();
            let active = CellRef::parse(cell).unwrap();
            assert_eq!(column, active.col);
            assert_eq!(d.selection.active, active);
            assert_eq!(d.wb, before);
            let sh = d.wb.active().unwrap();
            let geo = gridcraft_ui_egui::grid::Geo::new(sh, h.state().grid.rect.unwrap(), h.state().view().scroll);
            assert_eq!(at, geo.mirror_pos(gridcraft_ui_egui::grid::filter_button_rect(&geo, sh, active).center()));
            // Opening focuses the existing search box; typing must not edit the header.
            h.input_mut().events.push(Event::Text("1".into()));
            h.run_steps(2);
            assert_eq!(h.ctx.data(|data| data.get_temp::<String>(egui::Id::new(("filter_search", column)))).as_deref(), Some("1"));
            assert!(h.state().editor.is_none());
            assert_eq!(h.state().session.active().unwrap().wb, before);
            key(&mut h, Key::Escape, Modifiers::NONE);
            assert!(h.state().grid.filter_menu.is_none());
        }
    }
}

#[test]
fn alt_down_keeps_validation_priority_and_plain_down_navigation() {
    let mut session = book("A1");
    session.execute("data.filter", json!({"range": "A1:B3"})).unwrap();
    session.execute("data.validation", json!({"range": "A1", "type": "list", "formula1": "\"First,Second\""})).unwrap();
    let mut h = harness(session);
    key(&mut h, Key::ArrowDown, Modifiers::ALT);
    assert_eq!(h.state().grid.list_picker, Some(CellRef::parse("A1").unwrap()));
    assert!(h.state().grid.filter_menu.is_none());
    key(&mut h, Key::Escape, Modifiers::NONE);
    key(&mut h, Key::ArrowDown, Modifiers::NONE);
    assert_eq!(h.state().session.active().unwrap().selection.active.a1(), "A2");
    assert!(h.state().grid.filter_menu.is_none());
    key(&mut h, Key::ArrowDown, Modifiers::ALT);
    assert!(h.state().grid.filter_menu.is_none(), "a data cell has no filter button");
    assert_eq!(h.state().session.active().unwrap().selection.active.a1(), "A2");
}

#[test]
fn alt_down_only_opens_enabled_table_filter_buttons() {
    for enabled in [false, true] {
        let mut session = book("B1");
        session.execute("insert.table", json!({"range": "A1:B3"})).unwrap();
        if !enabled {
            session.execute("table.headerRow", json!({"on": false})).unwrap();
        }
        session.execute("selection.set", json!({"cell": "B1"})).unwrap();
        let mut h = harness(session);
        key(&mut h, Key::ArrowDown, Modifiers::ALT);
        assert_eq!(h.state().grid.filter_menu.map(|(column, _, _)| column), enabled.then_some(1));
        assert_eq!(h.state().session.active().unwrap().selection.active.a1(), "B1");
    }
}

/// A key press as a German keyboard reports it: `key` is what the layout types (egui falls back to
/// the key's position when it has no name for the character, as for `#` or `(`), `physical` the
/// position named after a US keyboard, `text` what the press types.
fn de_key(h: &mut Harness, key: Key, physical: Key, m: Modifiers, text: Option<&str>) {
    h.input_mut().events.push(Event::Key { key, physical_key: Some(physical), pressed: true, repeat: false, modifiers: m });
    if let Some(t) = text {
        h.input_mut().events.push(Event::Text(t.into()));
    }
    h.input_mut().events.push(Event::Key { key, physical_key: Some(physical), pressed: false, repeat: false, modifiers: m });
    h.run_steps(2);
}

fn german(session: Session) -> Harness {
    let mut h = harness(session);
    h.state_mut().ui.language = Language::De;
    h.run_steps(2);
    h
}

fn dialog(h: &Harness) -> Option<String> {
    h.state().dialog.as_ref().map(|d| d.name.clone())
}

fn sheet(h: &Harness) -> &gridcraft_engine::model::Sheet {
    h.state().session.active().unwrap().wb.active().unwrap()
}

#[test]
fn german_interface_uses_excels_german_shortcuts() {
    use gridcraft_engine::core::Value;
    let mut h = german(book("A1:B2"));
    // Strg+Umschalt+F and Strg+Umschalt+K: bold and italic, and off again on the second press.
    de_key(&mut h, Key::F, Key::F, CTRL_SHIFT, None);
    assert!(style(&h, "B2").font.bold);
    assert_eq!(dialog(&h), None);
    de_key(&mut h, Key::F, Key::F, CTRL_SHIFT, None);
    assert!(!style(&h, "B2").font.bold);
    de_key(&mut h, Key::K, Key::K, CTRL_SHIFT, None);
    assert!(style(&h, "A1").font.italic);
    assert_eq!(dialog(&h), None);
    // Strg+U fills down instead of underlining.
    de_key(&mut h, Key::U, Key::U, CTRL, None);
    assert_eq!(sheet(&h).value(CellRef::parse("A2").unwrap()), Value::Number(1.5));
    assert_eq!(style(&h, "A2").font.underline, gridcraft_engine::model::Underline::None);
    // Strg+# (no egui name: the key left of Enter) applies the date format.
    de_key(&mut h, Key::Backslash, Key::Backslash, CTRL, None);
    assert_eq!(style(&h, "B2").num_fmt.as_str(), "d-mmm-yy");
    // Strg+8 hides the columns, Strg+) (Umschalt+9) shows them again; Strg+9 and Strg+( rows.
    de_key(&mut h, Key::Num8, Key::Num8, CTRL, None);
    assert!(sheet(&h).is_col_hidden(0) && sheet(&h).is_col_hidden(1));
    de_key(&mut h, Key::Num9, Key::Num9, CTRL_SHIFT, Some(")"));
    assert!(!sheet(&h).is_col_hidden(0));
    de_key(&mut h, Key::Num9, Key::Num9, CTRL, None);
    assert!(sheet(&h).is_row_hidden(0));
    de_key(&mut h, Key::Num8, Key::Num8, CTRL_SHIFT, Some("("));
    assert!(!sheet(&h).is_row_hidden(0));
    // Strg+Umschalt+0 types `=` and does nothing; Strg++ opens Insert.
    de_key(&mut h, Key::Equals, Key::Num0, CTRL_SHIFT, Some("="));
    assert_eq!(dialog(&h), None);
    de_key(&mut h, Key::Plus, Key::CloseBracket, CTRL, Some("+"));
    assert_eq!(dialog(&h).as_deref(), Some("insertCells"));

    // Strg+. starts an entry with today's date.
    let mut h = german(book("D1"));
    de_key(&mut h, Key::Period, Key::Period, CTRL, None);
    assert!(h.state().editor.as_ref().is_some_and(|e| e.text.contains('/')));

    // Alt+= (Alt+Umschalt+0, which also types `=`) is AutoSum, not the start of an entry.
    let mut h = german(book("A1:A3"));
    let alt_shift = Modifiers { alt: true, shift: true, ..Modifiers::NONE };
    de_key(&mut h, Key::Equals, Key::Num0, alt_shift, Some("="));
    assert!(h.state().editor.is_none());
    assert_eq!(sheet(&h).input_text(CellRef::parse("A4").unwrap()), "=SUM(A1:A3)");

    // The English interface keeps Ctrl+Shift+F for Format Cells › Font; Ctrl with the numpad's +
    // inserts in every language.
    let mut h = harness(book("A1:B2"));
    de_key(&mut h, Key::F, Key::F, CTRL_SHIFT, None);
    assert_eq!(dialog(&h).as_deref(), Some("formatCells"));
    h.state_mut().dialog = None;
    de_key(&mut h, Key::Plus, Key::Plus, CTRL, None);
    assert_eq!(dialog(&h).as_deref(), Some("insertCells"));
}

#[test]
fn shortcuts_follow_the_interface_language() {
    let bold = Some(Shortcut::Run("home.bold", json!({})));
    let font = Some(Shortcut::Dialog("formatCells", json!({"tab": "Font"})));
    assert_eq!(lookup_in(Language::De, Key::F, Some(Key::F), CTRL_SHIFT, false), bold);
    assert_eq!(lookup_in(Language::En, Key::F, Some(Key::F), CTRL_SHIFT, false), font);
    // Languages without their own table yet keep the English shortcuts, and so does Excel for Mac.
    assert_eq!(lookup_in(Language::Fr, Key::F, Some(Key::F), CTRL_SHIFT, false), font);
    let mac_cmd_shift = Modifiers { mac_cmd: true, command: true, shift: true, ..Modifiers::NONE };
    assert_eq!(lookup_in(Language::De, Key::F, Some(Key::F), mac_cmd_shift, true), lookup(Key::F, Some(Key::F), mac_cmd_shift, true));
    // Keys German Excel binds like English Excel come from the English table.
    assert_eq!(lookup_in(Language::De, Key::Num4, Some(Key::Num4), CTRL, false), Some(Shortcut::Run("home.underline", json!({}))));
    // A key German Excel leaves free doesn't fall through to English.
    assert_eq!(lookup_in(Language::De, Key::Equals, Some(Key::Num0), CTRL_SHIFT, false), Some(Shortcut::Unbound));
}

#[test]
fn outline_symbols_and_next_corner_by_keyboard() {
    let mut s = book("A1:B2");
    s.execute("data.group", json!({"rows": "2:3"})).unwrap();
    s.execute("selection.set", json!({"range": "A1:B2"})).unwrap();
    let mut h = harness(s);
    let levels = |h: &Harness| gridcraft_ui_egui::grid::Geo::new(sheet(h), h.state().grid.rect.unwrap(), h.state().view().scroll).row_outline.levels;
    let active = |h: &Harness| h.state().session.active().unwrap().selection.active.a1();
    assert_eq!(levels(&h), 1);
    // Ctrl+8 hides the outline symbols, and the second press shows them again.
    key(&mut h, Key::Num8, CTRL);
    assert!(!sheet(&h).show_outline_symbols);
    assert_eq!(levels(&h), 0);
    key(&mut h, Key::Num8, CTRL);
    assert_eq!(levels(&h), 1);
    // Ctrl+. walks the corners of the selection clockwise.
    key(&mut h, Key::Period, CTRL);
    assert_eq!(active(&h), "B1");
    key(&mut h, Key::Period, CTRL);
    assert_eq!(active(&h), "B2");
    assert_eq!(h.state().session.active().unwrap().selection.a1(), "A1:B2");
    // German: Strg+7 and Strg+' (Strg+Umschalt+#).
    h.state_mut().ui.language = Language::De;
    de_key(&mut h, Key::Num7, Key::Num7, CTRL, None);
    assert!(!sheet(&h).show_outline_symbols);
    de_key(&mut h, Key::Quote, Key::Backslash, CTRL_SHIFT, Some("'"));
    assert_eq!(active(&h), "A2");
}
