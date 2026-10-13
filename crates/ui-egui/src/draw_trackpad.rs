//! Draw with Trackpad: a see-through canvas over the middle of the sheet, fed by the platform's
//! raw touchpad contacts (`Services::trackpad_capture`). The sheet shows through the canvas and
//! ink lands where it's drawn; one finger draws, two fingers move the canvas over the sheet, any
//! key leaves Draw mode. On a Force Touch pad, pressing the pad down and harder thickens the line.

use egui::{Color32, Pos2, Rect, Stroke, StrokeKind, pos2, vec2};
use gridcraft_engine::cmd::draw::{StrokeInput, stroke_shape};
use serde_json::json;

use crate::{SheetApp, TrackpadFrame, TrackpadPress, theme::Tokens};

/// Most samples one trackpad stroke keeps.
const MAX_SAMPLES: usize = 5_000;
/// Most fingers looked at.
const MAX_FINGERS: usize = 16;
/// Most contact frames handled per UI frame.
const MAX_FRAMES: usize = 256;
/// Seconds the canvas takes to settle in, and to grow away when the mode ends.
const OPEN_SECS: f64 = 0.3;
const CLOSE_SECS: f64 = 0.22;
/// How much bigger the canvas is when it starts to appear (and when it has gone).
const GROW: f32 = 0.5;
const RADIUS: f32 = 10.0;
/// Stroke force on a pad that measures pressure: a finger that only touches draws the thinnest
/// line; clicking the pad down starts at `CLICK_FORCE`, and pressing harder goes up to 1.
const TOUCH_FORCE: f32 = 0.0;
const CLICK_FORCE: f32 = 0.25;

#[derive(Default)]
pub struct TrackpadState {
    pub(crate) active: bool,
    /// A key was pressed while drawing (the input hook keeps keys from the rest of the app).
    pub(crate) exit_requested: bool,
    /// The stroke being drawn, in sheet pixels, and when each point came.
    points: Vec<[f32; 2]>,
    times: Vec<f64>,
    /// The force at each point, and whether the pad measured it (else the width follows speed).
    forces: Vec<f32>,
    measured: bool,
    /// The two fingers' last centre while moving the canvas.
    centroid: Option<[f32; 2]>,
    /// How far the canvas has been moved from the middle of the cells area (screen points).
    pan: egui::Vec2,
    /// When the canvas started to open (`ctx` time; set a frame after the mode starts, so the
    /// device setup's slow frame doesn't eat the animation), and when it started closing.
    opened: Option<f64>,
    primed: bool,
    closing: Option<f64>,
}

/// Where the canvas sits on screen: the middle of the cells area, moved by `pan` and kept inside.
fn canvas_rect(app: &SheetApp, pan: egui::Vec2) -> Option<Rect> {
    let viewport = app.grid.geo.as_ref().map(|g| g.cells).or(app.grid.rect)?;
    let size = vec2(432.0_f32.min((viewport.width() - 32.0).max(32.0)), 292.0_f32.min((viewport.height() - 100.0).max(32.0)));
    let reach = ((viewport.size() - size) / 2.0).max(egui::Vec2::ZERO);
    let pan = pan.max(-reach).min(reach);
    Some(Rect::from_center_size(viewport.center() + pan, size))
}

/// The pan as far as the canvas can actually go.
fn clamped_pan(app: &SheetApp, pan: egui::Vec2) -> egui::Vec2 {
    let viewport = app.grid.geo.as_ref().map(|g| g.cells).or(app.grid.rect);
    match (viewport, canvas_rect(app, pan)) {
        (Some(v), Some(r)) => r.center() - v.center(),
        _ => pan,
    }
}

/// Freezes and hides the pointer while the trackpad draws (the trackpad still moves it, and a
/// visible cursor would wander off the window or grow when shaken), and gives it back.
fn grab_pointer(ctx: &egui::Context, grab: bool) {
    // Locking keeps the cursor still where the platform can (macOS, Wayland); elsewhere it's kept
    // inside the window.
    let mode = if !grab {
        egui::viewport::CursorGrab::None
    } else if cfg!(any(target_os = "macos", target_os = "linux", target_os = "freebsd")) {
        egui::viewport::CursorGrab::Locked
    } else {
        egui::viewport::CursorGrab::Confined
    };
    ctx.send_viewport_cmd(egui::ViewportCommand::CursorGrab(mode));
    ctx.send_viewport_cmd(egui::ViewportCommand::CursorVisible(!grab));
}

fn finish_stroke(app: &mut SheetApp) {
    let state = &mut app.draw_ui.trackpad;
    let mut points = std::mem::take(&mut state.points);
    let mut times = std::mem::take(&mut state.times);
    let mut forces = std::mem::take(&mut state.forces);
    let measured = std::mem::take(&mut state.measured);
    app.grid.ink_live = None;
    // A tap is a dot.
    if let (1, Some(&p)) = (points.len(), points.first()) {
        points.push(p);
        times.push(times.first().copied().unwrap_or(0.0) + 0.016);
        forces.push(forces.first().copied().unwrap_or(TOUCH_FORCE));
    }
    if points.len() > 1 {
        let mut params = json!({"points": points, "times": times, "smooth": true});
        if measured {
            params["pressures"] = json!(forces);
        }
        app.run_or_alert("draw.stroke", params);
    }
}

/// Saves the stroke in progress, releases the capture and ends the mode: a key leaves Draw mode
/// altogether (no pen selected), like Excel; turning the toggle off or losing focus only stops
/// the trackpad.
fn stop(app: &mut SheetApp, ctx: &egui::Context, now: f64, leave_draw: bool) {
    finish_stroke(app);
    grab_pointer(ctx, false);
    if let Some(capture) = app.services.trackpad_capture.as_mut()
        && let Err(error) = capture(false)
    {
        app.services.trackpad_capture = None;
        app.services.trackpad_unavailable = Some(error);
    }
    if leave_draw {
        let _ = app.session.run("draw.exit", json!({}));
    } else if app.session.draw.trackpad {
        let _ = app.session.run("draw.trackpad", json!({"on": false}));
    }
    let previous = std::mem::take(&mut app.draw_ui.trackpad);
    app.draw_ui.trackpad = TrackpadState { pan: previous.pan, closing: previous.opened.map(|_| now), ..Default::default() };
}

/// Reads the touchpad and updates the stroke. Runs before the grid is laid out, so the stroke and
/// any scrolling show in the same frame the fingers moved.
pub(crate) fn track(app: &mut SheetApp, ctx: &egui::Context) {
    let on = app.session.draw.trackpad;
    let now = ctx.input(|i| i.time);
    if !app.draw_ui.trackpad.active {
        if !on {
            return;
        }
        app.draw_ui.trackpad = TrackpadState { active: true, ..Default::default() };
        app.grid.ink_live = None;
        grab_pointer(ctx, true);
    }
    let key = app.draw_ui.trackpad.exit_requested || ctx.input(|i| i.events.iter().any(|e| matches!(e, egui::Event::Key { pressed: true, .. })));
    if key || !on || !ctx.input(|i| i.focused) || app.backstage.is_some() {
        stop(app, ctx, now, key);
        ctx.request_repaint();
        return;
    }
    let frames = match app.services.trackpad_capture.as_mut() {
        Some(capture) => capture(true),
        None => Err("Trackpad capture unavailable".into()),
    };
    match frames {
        Ok(frames) => {
            let changed = !frames.is_empty();
            for frame in frames.iter().take(MAX_FRAMES) {
                contacts(app, frame, now);
            }
            if changed {
                live_stroke(app);
            }
        }
        Err(error) => {
            app.services.trackpad_capture = None;
            app.services.trackpad_unavailable = Some(error);
            stop(app, ctx, now, false);
        }
    }
    // Contacts arrive outside egui's input: poll every frame.
    ctx.request_repaint();
}

/// The sheet point under a screen point, through the cell there so frozen panes and pages map
/// right.
fn sheet_point(app: &SheetApp, p: Pos2) -> Option<[f32; 2]> {
    let geo = app.grid.geo.as_ref()?;
    let sh = app.session.active()?.wb.active()?;
    let p = geo.mirror_pos(p);
    let c = geo.cell_at(sh, p);
    Some([sh.col_left(c.col) as f32 + (p.x - geo.x(sh, c.col)) / geo.z, sh.row_top(c.row) as f32 + (p.y - geo.y(sh, c.row)) / geo.z])
}

/// One frame of normalized finger positions (0..=1 across the touchpad) and the pad's pressure.
fn contacts(app: &mut SheetApp, frame: &TrackpadFrame, now: f64) {
    let fingers: Vec<_> =
        frame.fingers.iter().take(MAX_FINGERS).filter(|q| q.iter().all(|v| v.is_finite() && (0.0..=1.0).contains(v))).copied().collect();
    let Some(rect) = canvas_rect(app, app.draw_ui.trackpad.pan) else { return };
    let canvas = rect.size();
    if let [q] = fingers.as_slice() {
        app.draw_ui.trackpad.centroid = None;
        let Some(point) = sheet_point(app, rect.min + vec2(q[0], q[1]) * rect.size()) else { return };
        let state = &mut app.draw_ui.trackpad;
        let force = match frame.press {
            TrackpadPress::Unmeasured => None,
            TrackpadPress::Touched => Some(TOUCH_FORCE),
            TrackpadPress::Clicked(p) => Some(CLICK_FORCE + (1.0 - CLICK_FORCE) * if p.is_finite() { p.clamp(0.0, 1.0) } else { 0.0 }),
        };
        state.measured |= force.is_some();
        let pressed = matches!(frame.press, TrackpadPress::Clicked(_));
        let force = force.unwrap_or(TOUCH_FORCE);
        if state.points.len() < MAX_SAMPLES && state.points.last().is_none_or(|p| (p[0] - point[0]).hypot(p[1] - point[1]) > 0.1) {
            state.points.push(point);
            state.times.push(now);
            state.forces.push(force);
        } else if pressed && let Some(last) = state.forces.last_mut() {
            // Pressing harder without moving thickens the end of the line.
            *last = force;
        }
        return;
    }
    // A second finger landing right after the first starts a move, not a dot.
    if fingers.len() > 1 && app.draw_ui.trackpad.points.len() == 1 {
        let state = &mut app.draw_ui.trackpad;
        state.points.clear();
        state.times.clear();
        state.forces.clear();
        state.measured = false;
    }
    finish_stroke(app);
    let centre = match fingers.as_slice() {
        [a, b] => Some([(a[0] + b[0]) / 2.0, (a[1] + b[1]) / 2.0]),
        _ => None,
    };
    if let (Some(c), Some(before)) = (centre, app.draw_ui.trackpad.centroid) {
        // The canvas follows the fingers over the sheet.
        let moved = app.draw_ui.trackpad.pan + vec2((c[0] - before[0]) * canvas.x, (c[1] - before[1]) * canvas.y);
        app.draw_ui.trackpad.pan = clamped_pan(app, moved);
    }
    app.draw_ui.trackpad.centroid = centre;
}

/// The stroke in progress, exactly as it will be saved; the grid paints it in place.
fn live_stroke(app: &mut SheetApp) {
    let state = &app.draw_ui.trackpad;
    let input = StrokeInput {
        points: state.points.iter().map(|q| [q[0] as f64, q[1] as f64]).collect(),
        times: state.times.clone(),
        pressures: if state.measured { state.forces.clone() } else { Vec::new() },
        smooth: true,
    };
    app.grid.ink_live = app.session.active().and_then(|d| d.wb.active()).and_then(|sh| stroke_shape(sh, &app.session.draw, &input));
}

fn ease_out(t: f64) -> f32 {
    let t = t.clamp(0.0, 1.0);
    (1.0 - (1.0 - t).powi(3)) as f32
}

/// The dimmed sheet around the canvas, its border and shadow, and the hint; animated in and out.
pub(crate) fn paint(app: &mut SheetApp, ctx: &egui::Context) {
    let now = ctx.input(|i| i.time);
    let state = &mut app.draw_ui.trackpad;
    if state.active && state.opened.is_none() {
        // The frame that turned the mode on also set up the device: start the clock on the next.
        if state.primed {
            state.opened = Some(now);
        }
        state.primed = true;
    }
    let state = &app.draw_ui.trackpad;
    // How far in the canvas is (0 = gone, 1 = settled), and the hint's opacity.
    let (shown, hint) = if state.active {
        let since = state.opened.map_or(0.0, |opened| now - opened);
        (ease_out(since / OPEN_SECS), ((since - OPEN_SECS * 0.6) / 0.15).clamp(0.0, 1.0) as f32)
    } else if let Some(closing) = state.closing {
        let t = (now - closing) / CLOSE_SECS;
        if t >= 1.0 {
            app.draw_ui.trackpad.closing = None;
            return;
        }
        (1.0 - ease_out(t), (1.0 - (now - closing) / 0.08).clamp(0.0, 1.0) as f32)
    } else {
        return;
    };
    if state.active {
        ctx.set_cursor_icon(egui::CursorIcon::None);
    }
    ctx.request_repaint();
    let t = Tokens::get(ctx);
    let viewport = app.grid.rect.unwrap_or_else(|| ctx.content_rect());
    let Some(settled) = canvas_rect(app, app.draw_ui.trackpad.pan) else { return };
    let hole = Rect::from_center_size(settled.center(), settled.size() * (1.0 + GROW * (1.0 - shown)));
    let painter = ctx.layer_painter(egui::LayerId::new(egui::Order::Foreground, egui::Id::new("trackpad_backdrop"))).with_clip_rect(viewport);
    painter.add(backdrop(viewport.union(hole.expand(1.0)), hole, RADIUS, t.shadow.gamma_multiply(shown)));
    // A soft shadow that deepens as the canvas settles.
    for i in 0..8 {
        let fade = 1.0 - i as f32 / 8.0;
        let width = 2.0 * i as f32;
        let color = Color32::from_black_alpha(18).gamma_multiply(shown * shown * fade);
        painter.rect_stroke(hole.expand(width), RADIUS + width, Stroke::new(2.0, color), StrokeKind::Outside);
    }
    painter.rect_stroke(hole, RADIUS, Stroke::new(1.0, t.text.gamma_multiply(shown)), StrokeKind::Outside);
    if hint > 0.0 {
        let lang = crate::i18n::current(ctx);
        let text = format!("{}\n{}", lang.tr("Draw with one finger. Move with two fingers."), lang.tr("Press any key to exit."));
        let mut job =
            egui::text::LayoutJob::simple(text, egui::FontId::proportional(14.0), t.text.gamma_multiply(hint), (viewport.width() - 64.0).max(64.0));
        job.halign = egui::Align::Center;
        let galley = painter.layout_job(job);
        let hint_rect = Rect::from_center_size(pos2(viewport.center().x, viewport.top() + 38.0), galley.size() + vec2(28.0, 18.0));
        painter.rect_filled(hint_rect, 6.0, t.menu_bg.gamma_multiply(hint));
        painter.galley(pos2(hint_rect.center().x, hint_rect.top() + 9.0), galley, t.text);
    }
}

/// `outer` filled except for a rounded-rect hole, as one mesh (no seams between the pieces).
fn backdrop(outer: Rect, hole: Rect, radius: f32, color: Color32) -> egui::Shape {
    let mut mesh = egui::Mesh::default();
    let r = radius.min(hole.width() / 2.0).min(hole.height() / 2.0).max(0.0);
    for band in [
        Rect::from_min_max(outer.min, pos2(outer.right(), hole.top())),
        Rect::from_min_max(pos2(outer.left(), hole.bottom()), outer.max),
        Rect::from_min_max(pos2(outer.left(), hole.top()), pos2(hole.left(), hole.bottom())),
        Rect::from_min_max(pos2(hole.right(), hole.top()), pos2(outer.right(), hole.bottom())),
    ] {
        if band.is_positive() {
            mesh.add_colored_rect(band, color);
        }
    }
    // Each corner of the hole: the square between the corner and its arc.
    const STEPS: u32 = 8;
    for (corner, centre, start) in [
        (hole.left_top(), hole.left_top() + vec2(r, r), std::f32::consts::PI),
        (hole.right_top(), hole.right_top() + vec2(-r, r), 1.5 * std::f32::consts::PI),
        (hole.right_bottom(), hole.right_bottom() + vec2(-r, -r), 0.0),
        (hole.left_bottom(), hole.left_bottom() + vec2(r, -r), 0.5 * std::f32::consts::PI),
    ] {
        let apex = mesh.vertices.len() as u32;
        mesh.colored_vertex(corner, color);
        for i in 0..=STEPS {
            let a = start + std::f32::consts::FRAC_PI_2 * i as f32 / STEPS as f32;
            mesh.colored_vertex(centre + vec2(a.cos(), a.sin()) * r, color);
            if i > 0 {
                mesh.add_triangle(apex, apex + i, apex + i + 1);
            }
        }
    }
    egui::Shape::mesh(mesh)
}
