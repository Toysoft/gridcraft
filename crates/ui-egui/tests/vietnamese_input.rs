//! Vietnamese typed straight into a selected cell: the Windows Telex input method composes the first
//! word (IME preedit, then commit), and Unikey/EVKey rewrite a letter with Backspace plus the accented
//! letter, possibly in the same frame as the key that started the edit.
use egui::{Event, ImeEvent, Key, Modifiers};
use gridcraft_engine::Session;
use gridcraft_engine::core::{CellRef, Value};
use gridcraft_ui_egui::{Services, SheetApp};
use serde_json::json;

type Harness = egui_kittest::Harness<'static, SheetApp>;

fn harness() -> Harness {
    let mut session = Session::new();
    session.new_workbook();
    session.execute("selection.set", json!({"range": "B2"})).unwrap();
    let mut h = egui_kittest::Harness::builder().with_size(egui::vec2(1200.0, 800.0)).build_ui_state(
        |ui, app: &mut SheetApp| {
            let ctx = ui.ctx().clone();
            app.logic(&ctx);
            app.ui(ui);
        },
        SheetApp::new(session, Services::default()),
    );
    h.run_steps(4);
    h
}

/// One frame, with the events the desktop shell would inject from the app's queue first.
fn frame(h: &mut Harness, events: Vec<Event>) {
    if let Some(batch) = h.state_mut().synthetic.pop_front() {
        h.input_mut().events.extend(batch);
    }
    h.input_mut().events.extend(events);
    h.step();
}

fn key(key: Key) -> Vec<Event> {
    [true, false].map(|pressed| Event::Key { key, physical_key: None, pressed, repeat: false, modifiers: Modifiers::NONE }).to_vec()
}

fn b2(h: &Harness) -> Option<Value> {
    let d = h.state().session.active()?;
    d.wb.active()?.cell(CellRef::new(1, 1)).map(|c| c.value.clone())
}

#[test]
fn the_grid_allows_input_methods_over_the_active_cell() {
    let mut h = harness();
    frame(&mut h, vec![]);
    assert!(h.output().platform_output.ime.is_some(), "IME allowed while a cell is selected");
}

#[test]
fn an_input_method_composition_starts_the_edit_and_commits_into_the_cell() {
    let mut h = harness();
    // Telex: a, w (ă), n, then space commits "ăn".
    frame(&mut h, vec![Event::Ime(ImeEvent::Preedit { text: "a".into(), active_range_chars: None })]);
    assert!(h.state().editor.is_some(), "the first composition opens the cell editor");
    frame(&mut h, vec![]);
    frame(&mut h, vec![Event::Ime(ImeEvent::Preedit { text: "ă".into(), active_range_chars: None })]);
    frame(&mut h, vec![Event::Ime(ImeEvent::Preedit { text: "ăn".into(), active_range_chars: None })]);
    frame(&mut h, vec![Event::Ime(ImeEvent::Commit("ăn".into()))]);
    frame(&mut h, vec![Event::Text(" cơm".into())]);
    frame(&mut h, key(Key::Enter));
    frame(&mut h, vec![]);
    assert_eq!(b2(&h), Some(Value::from("ăn cơm")));
}

#[test]
fn a_commit_without_preedit_types_into_the_cell() {
    let mut h = harness();
    frame(&mut h, vec![Event::Ime(ImeEvent::Commit("Việt".into()))]);
    frame(&mut h, vec![]);
    frame(&mut h, key(Key::Enter));
    frame(&mut h, vec![]);
    assert_eq!(b2(&h), Some(Value::from("Việt")));
}

#[test]
fn backspace_rewrites_in_the_starting_frame_are_not_lost() {
    let mut h = harness();
    // Unikey: "a", then "a" again → Backspace + "â", all arriving in one frame.
    let mut burst = vec![Event::Text("a".into())];
    burst.extend(key(Key::Backspace));
    burst.push(Event::Text("â".into()));
    frame(&mut h, burst);
    frame(&mut h, vec![]);
    frame(&mut h, vec![]);
    frame(&mut h, key(Key::Enter));
    frame(&mut h, vec![]);
    assert_eq!(b2(&h), Some(Value::from("â")));
}
