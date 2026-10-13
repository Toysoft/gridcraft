//! Draw input, editable pen settings and keyboard behavior through real UI events.
use egui::{Event, Key, Modifiers, PointerButton, pos2, vec2};
use egui_kittest::kittest::Queryable as _;
use gridcraft_engine::Session;
use gridcraft_engine::cmd::draw::DrawTool;
use gridcraft_engine::model::InkEffect;
use gridcraft_ui_egui::{SheetApp, TrackpadFrame, TrackpadPress};
use serde_json::json;
type Harness = egui_kittest::Harness<'static, SheetApp>;
fn harness() -> Harness {
    let mut session = Session::new();
    session.new_workbook();
    let mut app = SheetApp::new(session, Default::default());
    app.ui.ribbon_tab = "Draw".into();
    let mut h = Harness::builder().with_size(vec2(1200.0, 800.0)).build_ui_state(
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
/// A trackpad contact frame from a pad that doesn't measure pressure.
fn touch(fingers: Vec<[f32; 2]>) -> TrackpadFrame {
    TrackpadFrame { fingers, press: TrackpadPress::Unmeasured }
}
fn button(h: &mut Harness, p: egui::Pos2, pressed: bool) {
    h.input_mut().events.push(Event::PointerButton { pos: p, button: PointerButton::Primary, pressed, modifiers: Modifiers::NONE });
    h.run_steps(2);
}
#[test]
fn pen_second_click_opens_settings_and_updates_effect() {
    let mut h = harness();
    h.get_by_label("Pen 1").click();
    h.run_steps(2);
    assert_eq!(h.state().session.draw.tool, DrawTool::Pen);
    h.get_by_label("Pen 1").click();
    h.run_steps(2);
    h.get_by_label("Rainbow").click();
    h.run_steps(2);
    assert_eq!(h.state().session.prefs.ink_pens[0].effect, Some(InkEffect::Rainbow));
}
#[test]
fn drawing_creates_smoothed_pressure_stroke_and_escape_cancels() {
    let mut h = harness();
    h.state_mut().session.execute("draw.pen", json!({"index":0,"on":true})).unwrap();
    let r = h.state().grid.rect.unwrap();
    let start = r.min + vec2(150.0, 100.0);
    h.input_mut().events.push(Event::PointerMoved(start));
    h.run_steps(2);
    button(&mut h, start, true);
    for delta in [vec2(30.0, 15.0), vec2(60.0, -10.0), vec2(90.0, 25.0), vec2(120.0, 0.0)] {
        h.input_mut().events.push(Event::PointerMoved(start + delta));
        h.run_steps(2);
    }
    button(&mut h, start + vec2(120.0, 0.0), false);
    let shape = &h.state().session.doc().unwrap().wb.active().unwrap().shapes[0];
    let ink = shape.ink.as_ref().unwrap();
    assert!(ink.points.len() > 5);
    assert!(ink.has_pressure());
    h.key_press(Key::Escape);
    h.run_steps(2);
    assert_eq!(h.state().session.draw.tool, DrawTool::Select);
}
#[test]
fn selected_ink_delete_works_in_lasso_mode() {
    let mut h = harness();
    let result = h.state_mut().session.execute("draw.stroke", json!({"points":[[20,20],[100,100]]})).unwrap();
    let id = result["shape"].as_u64().unwrap() as u32;
    h.state_mut().session.draw.selection = vec![id];
    h.state_mut().session.draw.tool = DrawTool::Lasso;
    h.input_mut().events.push(Event::PointerMoved(pos2(600.0, 500.0)));
    h.run_steps(2);
    h.key_press(Key::Delete);
    h.run_steps(2);
    assert!(h.state().session.doc().unwrap().wb.active().unwrap().shapes.is_empty());
}
#[test]
fn moving_a_lasso_selection_past_its_bounds_moves_the_whole_group() {
    use gridcraft_ui_egui::grid::Geo;
    let mut h = harness();
    let mut ids = Vec::new();
    for y in [40, 80] {
        let r = h.state_mut().session.execute("draw.stroke", json!({"points":[[20,y],[100,y]]})).unwrap();
        ids.push(r["shape"].as_u64().unwrap() as u32);
    }
    h.state_mut().session.draw.selection = ids;
    h.state_mut().session.draw.tool = DrawTool::Lasso;
    h.run_steps(2);
    let app = h.state();
    let sheet = app.session.doc().unwrap().wb.active().unwrap();
    let geo = Geo::new(sheet, app.grid.rect.unwrap(), app.view().scroll);
    let start = geo.cells.min + vec2(60.0, 40.5) * geo.z;
    h.input_mut().events.push(Event::PointerMoved(start));
    h.run_steps(2);
    button(&mut h, start, true);
    let end = start + vec2(180.0, 30.0);
    h.input_mut().events.push(Event::PointerMoved(end));
    h.run_steps(3);
    button(&mut h, end, false);
    let sheet = h.state().session.doc().unwrap().wb.active().unwrap();
    assert_eq!(sheet.shapes.len(), 2);
    for shape in &sheet.shapes {
        let x = sheet.col_left(shape.anchor.cell.col) + shape.anchor.dx as f64;
        assert!(x > 200.0, "group member stayed at {x}");
    }
}
#[test]
fn add_and_delete_a_pencil_through_the_ribbon() {
    let mut h = harness();
    h.get_by_label("Add Pen").click();
    h.run_steps(2);
    h.get_by_label("Pencil").click();
    h.run_steps(2);
    assert_eq!(h.state().session.prefs.ink_pens.len(), 7);
    assert_eq!(h.state().session.draw.ink_tool, gridcraft_engine::model::InkTool::Pencil);
    h.get_by_label("Pen 7").click();
    h.run_steps(2);
    h.get_by_label("Delete").click();
    h.run_steps(2);
    assert_eq!(h.state().session.prefs.ink_pens.len(), 6);
}

#[test]
fn trackpad_without_a_capture_backend_is_disabled() {
    let mut h = harness();
    h.get_by_label("Draw with Trackpad").click();
    h.run_steps(2);
    assert!(!h.state().session.draw.trackpad);
}
#[test]
fn native_contact_release_saves_ink_and_exit_releases_capture() {
    use std::sync::{Arc, Mutex};
    let samples = Arc::new(Mutex::new(vec![vec![vec![[0.1, 0.1]], vec![[0.2, 0.3]]], vec![vec![[0.4, 0.5]]], vec![vec![]]]));
    let released = Arc::new(Mutex::new(false));
    let mut h = harness();
    let feed = samples.clone();
    let flag = released.clone();
    h.state_mut().services.trackpad_capture = Some(Box::new(move |on| {
        if !on {
            *flag.lock().unwrap() = true;
            return Ok(Vec::new());
        }
        let mut q = feed.lock().unwrap();
        Ok(if q.is_empty() { Vec::new() } else { q.remove(0).into_iter().map(touch).collect() })
    }));
    h.state_mut().session.execute("draw.trackpad", json!({"on": true})).unwrap();
    h.run_steps(6);
    // The ink lands under the canvas in the middle of the sheet
    // (it's a see-through window onto the sheet, not a copy of its top-left corner).
    let app = h.state();
    let sh = app.session.doc().unwrap().wb.active().unwrap();
    assert_eq!(sh.shapes.len(), 1);
    let geo = app.grid.geo.as_ref().unwrap();
    let origin = gridcraft_engine::cmd::draw::anchor_origin(sh, &sh.shapes[0].anchor);
    let at = geo.cells.min + vec2(origin[0], origin[1]) * geo.z;
    assert!(geo.cells.shrink2(geo.cells.size() / 4.0).expand(geo.cells.width() * 0.1).contains(at), "{at:?} in {:?}", geo.cells);
    let centre = geo.cells.center();
    // The pointer doesn't click the sheet underneath, and any key leaves Draw mode.
    button(&mut h, centre, true);
    button(&mut h, centre, false);
    assert!(h.state().session.draw.trackpad);
    h.key_press(Key::Escape);
    h.run_steps(2);
    assert!(!h.state().session.draw.trackpad);
    assert_eq!(h.state().session.draw.tool, DrawTool::Select);
    assert!(*released.lock().unwrap());
}

#[test]
fn force_touch_pressure_thickens_the_trackpad_stroke() {
    use std::sync::{Arc, Mutex};
    let at = |x: f32, press| TrackpadFrame { fingers: vec![[x, 0.5]], press };
    // Touched, then clicked down and pressed harder; pressing harder in place thickens the end.
    let frames = vec![
        at(0.2, TrackpadPress::Touched),
        at(0.3, TrackpadPress::Touched),
        at(0.4, TrackpadPress::Clicked(0.0)),
        at(0.5, TrackpadPress::Clicked(0.5)),
        at(0.6, TrackpadPress::Clicked(0.8)),
        at(0.6, TrackpadPress::Clicked(1.0)),
    ];
    let feed = Arc::new(Mutex::new(vec![frames, vec![touch(vec![])]]));
    let mut h = harness();
    let q = feed.clone();
    h.state_mut().services.trackpad_capture = Some(Box::new(move |on| {
        let mut q = q.lock().unwrap();
        Ok(if on && !q.is_empty() { q.remove(0) } else { Vec::new() })
    }));
    h.state_mut().session.execute("draw.trackpad", json!({"on": true})).unwrap();
    h.run_steps(6);
    let sh = h.state().session.doc().unwrap().wb.active().unwrap().clone();
    assert_eq!(sh.shapes.len(), 1);
    let ink = sh.shapes[0].ink.as_ref().unwrap();
    let (first, last) = (ink.pressures.first().copied().unwrap(), ink.pressures.last().copied().unwrap());
    assert!(first < 0.05, "a touch without a click is the lightest force: {:?}", ink.pressures);
    assert!(last > 0.9, "the hardest press is the strongest force: {:?}", ink.pressures);
    assert!(ink.point_width(ink.points.len() - 1) > ink.point_width(0) * 1.5);
}

#[test]
fn more_colors_text_opens_the_color_picker() {
    let mut h = harness();
    h.get_by_label("Pen 1").click();
    h.run_steps(2);
    h.get_by_label("Pen 1").click();
    h.run_steps(2);
    h.get_by_label("More Colors…").click();
    h.run_steps(2);
    assert_eq!(h.state().draw_ui.color_picker, Some(0));
}

#[test]
fn lasso_previews_fully_enclosed_ink_before_release() {
    use gridcraft_ui_egui::grid::Geo;
    let mut h = harness();
    let id = h.state_mut().session.execute("draw.stroke", json!({"points":[[20,30],[100,30]],"width":2})).unwrap()["shape"].as_u64().unwrap() as u32;
    h.state_mut().session.draw.tool = DrawTool::Lasso;
    h.run_steps(2);
    let app = h.state();
    let sh = app.session.doc().unwrap().wb.active().unwrap();
    let geo = Geo::new(sh, app.grid.rect.unwrap(), app.view().scroll);
    let p = |x, y| geo.cells.min + vec2(x, y) * geo.z;
    let start = p(10.0, 10.0);
    h.input_mut().events.push(Event::PointerMoved(start));
    h.run_steps(2);
    button(&mut h, start, true);
    for point in [p(110.0, 10.0), p(110.0, 50.0), p(10.0, 50.0)] {
        h.input_mut().events.push(Event::PointerMoved(point));
        h.run_steps(2);
    }
    assert_eq!(h.state().grid.ink_preview, vec![id]);
    assert!(h.state().session.draw.selection.is_empty());
    button(&mut h, p(10.0, 50.0), false);
    assert_eq!(h.state().session.draw.selection, vec![id]);
}

fn drag(h: &mut Harness, points: &[egui::Pos2]) {
    let (Some(&first), Some(&last)) = (points.first(), points.last()) else { return };
    h.input_mut().events.push(Event::PointerMoved(first));
    h.run_steps(2);
    button(h, first, true);
    for &p in points.iter().skip(1) {
        h.input_mut().events.push(Event::PointerMoved(p));
        h.run_steps(2);
    }
    button(h, last, false);
}

fn shapes(h: &Harness) -> Vec<gridcraft_engine::model::Shape> {
    h.state().session.doc().unwrap().wb.active().unwrap().shapes.clone()
}

#[test]
fn the_live_stroke_is_the_saved_stroke() {
    let mut h = harness();
    h.state_mut().session.execute("draw.pen", json!({"index":0,"on":true})).unwrap();
    let start = h.state().grid.rect.unwrap().min + vec2(150.0, 100.0);
    h.input_mut().events.push(Event::PointerMoved(start));
    h.run_steps(2);
    button(&mut h, start, true);
    for delta in [vec2(30.0, 15.0), vec2(60.0, -10.0), vec2(90.0, 25.0)] {
        h.input_mut().events.push(Event::PointerMoved(start + delta));
        h.run_steps(2);
    }
    let live = h.state().grid.ink_live.clone().expect("a preview while drawing");
    button(&mut h, start + vec2(90.0, 25.0), false);
    assert!(h.state().grid.ink_live.is_none());
    let saved = &shapes(&h)[0];
    assert_eq!(saved.anchor, live.anchor);
    assert_eq!(saved.ink, live.ink);
}

#[test]
fn a_lasso_crossing_selected_ink_stays_a_lasso() {
    use gridcraft_ui_egui::grid::Geo;
    let mut h = harness();
    let id = h.state_mut().session.execute("draw.stroke", json!({"points":[[40,30],[80,30]],"width":2})).unwrap()["shape"].as_u64().unwrap() as u32;
    h.state_mut().session.draw.selection = vec![id];
    h.state_mut().session.draw.tool = DrawTool::Lasso;
    h.run_steps(2);
    let app = h.state();
    let sh = app.session.doc().unwrap().wb.active().unwrap();
    let geo = Geo::new(sh, app.grid.rect.unwrap(), app.view().scroll);
    let p = |x, y| geo.cells.min + vec2(x, y) * geo.z;
    // The loop starts away from the stroke, passes over it and ends on it.
    drag(&mut h, &[p(10.0, 10.0), p(120.0, 10.0), p(120.0, 60.0), p(10.0, 60.0), p(60.0, 30.0)]);
    assert_eq!(shapes(&h).len(), 1, "the loop must not be saved as ink");
    // It selected what it enclosed: the stroke touches the loop's edge, so nothing.
    assert!(h.state().session.draw.selection.is_empty());
    assert_eq!(h.state().session.draw.tool, DrawTool::Lasso);
}

#[test]
fn one_eraser_drag_is_one_undo_step() {
    use gridcraft_ui_egui::grid::Geo;
    let mut h = harness();
    for y in [30, 60] {
        h.state_mut().session.execute("draw.stroke", json!({"points":[[20,y],[200,y]],"width":2})).unwrap();
    }
    h.state_mut().session.execute("draw.eraser", json!({"on":true,"mode":"stroke"})).unwrap();
    h.run_steps(2);
    let undo = h.state().session.doc().unwrap().undo.len();
    let app = h.state();
    let sh = app.session.doc().unwrap().wb.active().unwrap();
    let geo = Geo::new(sh, app.grid.rect.unwrap(), app.view().scroll);
    let p = |x, y| geo.cells.min + vec2(x, y) * geo.z;
    drag(&mut h, &[p(100.0, 10.0), p(100.0, 30.0), p(100.0, 45.0), p(100.0, 60.0), p(100.0, 80.0)]);
    assert!(shapes(&h).is_empty());
    assert_eq!(h.state().session.doc().unwrap().undo.len(), undo + 1);
    // A second drag is its own step.
    h.state_mut().session.execute("edit.undo", json!({})).unwrap();
    assert_eq!(shapes(&h).len(), 2);
}

#[test]
fn two_fingers_move_the_canvas_not_the_sheet() {
    use std::sync::{Arc, Mutex};
    let dot = |x: f32| vec![vec![[x, 0.5]], vec![[x + 0.01, 0.5]], vec![]];
    let mut frames = dot(0.5);
    frames.extend([vec![[0.2, 0.5], [0.4, 0.5]], vec![[0.3, 0.5], [0.5, 0.5]], vec![]]);
    frames.extend(dot(0.5));
    let feed = Arc::new(Mutex::new(frames));
    let mut h = harness();
    let q = feed.clone();
    h.state_mut().services.trackpad_capture = Some(Box::new(move |on| {
        let mut q = q.lock().unwrap();
        Ok(if on && !q.is_empty() { vec![touch(q.remove(0))] } else { Vec::new() })
    }));
    let scroll = h.state().view().scroll;
    h.state_mut().session.execute("draw.trackpad", json!({"on": true})).unwrap();
    h.run_steps(16);
    let app = h.state();
    assert_eq!(app.view().scroll, scroll);
    let sh = app.session.doc().unwrap().wb.active().unwrap();
    assert_eq!(sh.shapes.len(), 2);
    let x = |i: usize| gridcraft_engine::cmd::draw::anchor_origin(sh, &sh.shapes[i].anchor)[0];
    // The fingers moved a tenth of the pad: the canvas (and the second dot) moved a tenth of its width.
    let moved = (x(1) - x(0)) * app.grid.geo.as_ref().unwrap().z;
    assert!((moved - 43.2).abs() < 2.0, "moved {moved}");
}
