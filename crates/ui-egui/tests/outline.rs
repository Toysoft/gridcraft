//! The outline band of grouped rows and columns: − / + and level buttons (#373).

use egui::os::OperatingSystem;
use egui::{Event, Key, Modifiers, PointerButton, Pos2, vec2};
use egui_kittest::kittest::Queryable as _;
use gridcraft_engine::Session;
use gridcraft_ui_egui::SheetApp;
use gridcraft_ui_egui::outline::{Action, layout};
use serde_json::json;

type Harness = egui_kittest::Harness<'static, SheetApp>;

/// Windows / Linux key bindings (Alt+Shift+Right groups), whatever machine runs the tests.
fn harness() -> Harness {
    let mut session = Session::new();
    session.new_workbook();
    let builder = egui_kittest::Harness::builder().with_os(OperatingSystem::Nix);
    let mut h = builder.with_step_dt(1.0 / 60.0).with_size(vec2(1200.0, 800.0)).build_ui_state(
        |ui, app: &mut SheetApp| {
            app.logic(ui.ctx());
            app.ui(ui);
        },
        SheetApp::new(session, Default::default()),
    );
    h.run_steps(4);
    h
}

fn click(h: &mut Harness, pos: Pos2) {
    let press = |pressed| Event::PointerButton { pos, button: PointerButton::Primary, pressed, modifiers: Modifiers::NONE };
    h.input_mut().events.extend([Event::PointerMoved(pos), press(true)]);
    h.step();
    h.input_mut().events.push(press(false));
    h.run_steps(3);
}

fn key(h: &mut Harness, key: Key, modifiers: Modifiers) {
    let event = |pressed| Event::Key { key, physical_key: Some(key), pressed, repeat: false, modifiers };
    h.input_mut().events.extend([event(true), event(false)]);
    h.run_steps(3);
}

/// Grouped rows (or columns), 0-based.
fn grouped(h: &Harness, rows: bool) -> Vec<u32> {
    let sh = h.state().session.active().unwrap().wb.active().unwrap().clone();
    (if rows { &sh.rows } else { &sh.cols }).iter().filter(|(_, l)| l.outline > 0).map(|(i, _)| *i).collect()
}

fn dialog(h: &Harness) -> Option<String> {
    h.state().dialog.as_ref().map(|d| d.name.clone())
}

/// Picks Rows or Columns in the open Group / Ungroup dialog and clicks OK.
fn answer(h: &mut Harness, by: &str) {
    h.state_mut().dialog.as_mut().unwrap().values.insert("by".into(), json!(by));
    h.get_by_label("   OK   ").click();
    h.run_steps(3);
}

/// Where the button that does `which` is drawn this frame (screen coordinates).
fn button(h: &Harness, which: impl Fn(&Action) -> bool) -> Pos2 {
    let app = h.state();
    let geo = app.grid.geo.clone().unwrap();
    let sh = app.session.active().unwrap().wb.active().unwrap().clone();
    let b = layout(&geo, &sh).buttons.into_iter().find(|b| which(&b.action)).expect("button on screen");
    geo.mirror_pos(b.rect.center())
}

fn hidden_rows(h: &Harness) -> Vec<u32> {
    let sh = h.state().session.active().unwrap().wb.active().unwrap().clone();
    sh.rows.iter().filter(|(_, l)| l.hidden).map(|(r, _)| *r).collect()
}

fn toggle(rows: bool, level: u8) -> impl Fn(&Action) -> bool {
    move |a| matches!(a, Action::Toggle { rows: r, group } if *r == rows && group.level == level)
}

fn outline_buttons_collapse_and_expand(rtl: bool) {
    let mut h = harness();
    if rtl {
        h.state_mut().session.run("view.rightToLeft", json!({"on": true})).unwrap();
    }
    h.state_mut().session.run("data.group", json!({"rows": "5:8"})).unwrap();
    h.state_mut().session.run("data.group", json!({"cols": "C:E"})).unwrap();
    h.run_steps(2);
    let geo = h.state().grid.geo.clone().unwrap();
    assert!(geo.band_w > 0.0 && geo.band_h > 0.0);

    // − on row 9 hides rows 5:8 and turns into +.
    let minus = button(&h, toggle(true, 1));
    click(&mut h, minus);
    assert_eq!(hidden_rows(&h), [4, 5, 6, 7]);
    let plus = button(&h, toggle(true, 1));
    click(&mut h, plus);
    assert!(hidden_rows(&h).is_empty());

    // Level 1 for the columns collapses C:E; level 2 shows them again.
    let level1 = button(&h, |a| *a == Action::Level { rows: false, level: 1 });
    click(&mut h, level1);
    let col_hidden = |h: &Harness| (2..=4).all(|c| h.state().session.active().unwrap().wb.active().unwrap().col_width(c) == 0.0);
    assert!(col_hidden(&h));
    let level2 = button(&h, |a| *a == Action::Level { rows: false, level: 2 });
    click(&mut h, level2);
    assert!(!col_hidden(&h));

    // Every click was a command: undo walks them back.
    h.state_mut().session.run("edit.undo", json!({})).unwrap();
    assert!(col_hidden(&h));
}

#[test]
fn outline_buttons_collapse_expand_and_undo() {
    outline_buttons_collapse_and_expand(false);
}

#[test]
fn outline_buttons_work_on_right_to_left_sheets() {
    outline_buttons_collapse_and_expand(true);
}

#[test]
fn group_and_ungroup_ask_rows_or_columns_for_a_range_of_cells() {
    let mut h = harness();
    let alt_shift = Modifiers { alt: true, shift: true, ..Modifiers::NONE };
    // Whole rows are grouped straight away.
    h.state_mut().session.run("selection.set", json!({"range": "5:8"})).unwrap();
    key(&mut h, Key::ArrowRight, alt_shift);
    assert_eq!(dialog(&h), None);
    assert_eq!(grouped(&h, true), [4, 5, 6, 7]);
    // A range of cells asks first, for Group and for Ungroup.
    h.state_mut().session.run("selection.set", json!({"range": "B2:C3"})).unwrap();
    key(&mut h, Key::ArrowRight, alt_shift);
    assert_eq!(dialog(&h).as_deref(), Some("group"));
    answer(&mut h, "cols");
    assert_eq!(dialog(&h), None);
    assert_eq!(grouped(&h, false), [1, 2]);
    assert_eq!(grouped(&h, true), [4, 5, 6, 7]);
    key(&mut h, Key::ArrowLeft, alt_shift);
    assert_eq!(dialog(&h).as_deref(), Some("ungroup"));
    answer(&mut h, "cols");
    assert!(grouped(&h, false).is_empty());
    // The Data tab's keytips (Alt, A, G, G) ask the same.
    key(&mut h, Key::F10, Modifiers::NONE);
    for k in [Key::A, Key::G, Key::G] {
        let event = |pressed| Event::Key { key: k, physical_key: None, pressed, repeat: false, modifiers: Modifiers::NONE };
        h.input_mut().events.extend([event(true), Event::Text(k.name().to_ascii_lowercase()), event(false)]);
        h.run_steps(2);
    }
    assert_eq!(dialog(&h).as_deref(), Some("group"));
    answer(&mut h, "rows");
    assert_eq!(grouped(&h, true), [1, 2, 4, 5, 6, 7]);
}

#[test]
fn clicking_a_band_button_enters_the_cell_being_edited_first() {
    let mut h = harness();
    h.state_mut().session.run("data.group", json!({"rows": "5:8"})).unwrap();
    h.state_mut().session.run("selection.set", json!({"range": "B6"})).unwrap();
    h.run_steps(2);
    h.input_mut().events.push(Event::Text("123".into()));
    h.run_steps(3);
    assert!(h.state().editor.is_some());
    let minus = button(&h, toggle(true, 1));
    click(&mut h, minus);
    assert!(h.state().editor.is_none(), "the edit is entered, not left open over a hidden row");
    let b6 = h.state_mut().session.run("cell.get", json!({"cell": "B6"})).unwrap();
    assert_eq!(b6["value"], json!(123.0));
    assert_eq!(hidden_rows(&h), [4, 5, 6, 7]);
}
