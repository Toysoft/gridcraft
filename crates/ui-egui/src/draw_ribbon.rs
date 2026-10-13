//! The Draw tab: tools, the pen gallery and pen settings. Pen and toolbox artwork is drawn in
//! code; every action is a `draw.*` command.

use egui::{Color32, Rect, Sense, Stroke, StrokeKind, Ui, pos2, vec2};
use gridcraft_engine::cmd::draw::{DrawTool, EraserMode};
use gridcraft_engine::model::{Color, InkEffect, InkPen, InkTool, Theme};
use serde_json::json;

use crate::{SheetApp, icons::Icon, theme::Tokens, widgets::big_button};

/// GridCraft's ink palette (original values).
const COLORS: [(&str, u32); 16] = [
    ("Black", 0x000000),
    ("Dark Gray", 0x3C4043),
    ("Light Gray", 0x9AA0A6),
    ("White", 0xFFFFFF),
    ("Red", 0xD13B2F),
    ("Orange", 0xF08A24),
    ("Yellow", 0xF5D33B),
    ("Light Green", 0x7CC242),
    ("Green", 0x1E8E3E),
    ("Light Blue", 0x7FD3F2),
    ("Sky Blue", 0x2BA3E0),
    ("Dark Blue", 0x1F5FC9),
    ("Indigo", 0x4B3FA8),
    ("Purple", 0x8E44AD),
    ("Plum", 0x9C2F6B),
    ("Pink", 0xEC4C8C),
];

/// Draw tab UI state that isn't engine state.
#[derive(Default)]
pub struct DrawUi {
    /// The pen whose More Colors… picker is open.
    pub color_picker: Option<usize>,
    pub trackpad: crate::draw_trackpad::TrackpadState,
}

fn run(app: &mut SheetApp, id: &str, p: serde_json::Value) {
    app.run_or_alert(id, p);
}
fn rgb(color: Color) -> Color32 {
    let c = color.resolve(&Theme::default()).unwrap_or([0, 0, 0]);
    Color32::from_rgb(c[0], c[1], c[2])
}
fn separator(ui: &mut Ui) {
    let t = Tokens::get(ui.ctx());
    let (r, _) = ui.allocate_exact_size(vec2(12.0, 72.0), Sense::hover());
    ui.painter().line_segment([r.center_top(), r.center_bottom()], Stroke::new(1.0, t.separator));
}
/// A tool button: outlined while its tool is active.
fn tool_button(app: &mut SheetApp, ui: &mut Ui, icon: Icon, label: &str, tip: &str, menu: bool, active: bool, command: &str) -> egui::Response {
    let t = Tokens::get(ui.ctx());
    let language = crate::i18n::current(ui.ctx());
    let r = big_button(ui, icon, label, tip, menu);
    let name = language.tr(label).replace('\n', " ");
    r.widget_info(|| egui::WidgetInfo::selected(egui::WidgetType::Button, true, active, &name));
    if active {
        ui.painter().rect_stroke(r.rect, 4.0, Stroke::new(1.0, t.accent), StrokeKind::Inside);
    }
    crate::ribbon::control_keytip(app, ui, &r, command);
    r
}

pub fn show(app: &mut SheetApp, ui: &mut Ui) {
    let t = Tokens::get(ui.ctx());
    let language = crate::i18n::current(ui.ctx());
    let tool = app.session.draw.tool;
    if tool_button(app, ui, Icon::Pen, "Draw", "Enter Draw mode", false, tool == DrawTool::Pen, "draw.toggle").clicked() {
        run(app, "draw.toggle", json!({}));
    }
    let eraser = tool_button(app, ui, Icon::Eraser, "Eraser", "Erase strokes or segments", true, tool == DrawTool::Eraser, "draw.eraser");
    egui::Popup::menu(&eraser).show(|ui| {
        for mode in EraserMode::ALL {
            if ui.selectable_label(app.session.draw.eraser == mode, language.tr(mode.label())).clicked() {
                run(app, "draw.eraser", json!({"on": true, "mode": mode.id()}));
                ui.close();
            }
        }
    });
    if tool_button(app, ui, Icon::Lasso, "Lasso\nSelect", "Select ink with a freehand loop", false, tool == DrawTool::Lasso, "draw.lasso").clicked() {
        run(app, "draw.lasso", json!({}));
    }
    separator(ui);
    let pens = app.session.prefs.ink_pens.clone();
    egui::Frame::new().fill(t.input_bg).corner_radius(5.0).inner_margin(4.0).show(ui, |ui| {
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 3.0;
            for (index, pen) in pens.iter().enumerate() {
                ui.push_id(index, |ui| pen_button(app, ui, index, pen));
            }
        });
    });
    let add = big_button(ui, Icon::Plus, "Add Pen", "Add Pen, Highlighter or Pencil", true);
    add.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, language.tr("Add Pen")));
    egui::Popup::menu(&add).show(|ui| {
        for tool in [InkTool::Pen, InkTool::Highlighter, InkTool::Pencil] {
            if ui.button(language.tr(tool.label())).clicked() {
                run(app, "draw.pen.add", json!({"tool": tool.id()}));
                ui.close();
            }
        }
    });
    separator(ui);
    trackpad_toggle(app, ui);
    separator(ui);
    let r = big_button(ui, Icon::Shapes, "Ink to\nShape", "Convert the last ink stroke to a shape", false);
    crate::ribbon::control_keytip(app, ui, &r, "draw.inkToShape");
    if r.clicked() {
        run(app, "draw.inkToShape", json!({}));
    }
}

fn pen_button(app: &mut SheetApp, ui: &mut Ui, index: usize, pen: &InkPen) {
    let t = Tokens::get(ui.ctx());
    let language = crate::i18n::current(ui.ctx());
    let active = app.session.draw.pen == index && app.session.draw.tool == DrawTool::Pen;
    let (rect, response) = ui.allocate_exact_size(vec2(48.0, 64.0), Sense::click());
    response.widget_info(|| egui::WidgetInfo::selected(egui::WidgetType::Button, true, active, format!("{} {}", language.tr("Pen"), index + 1)));
    if active || response.hovered() {
        ui.painter().rect_stroke(rect, 4.0, Stroke::new(1.5, if active { t.accent } else { t.text_dim }), StrokeKind::Inside);
    }
    paint_pen(ui, rect, pen);
    let mm = (pen.width * 25.4 / 96.0 * 100.0).round() / 100.0;
    let effect = pen.effect.map(|e| format!(", {}", language.tr(e.label()))).unwrap_or_default();
    let response = response.on_hover_text(format!("{}: {mm} {}{effect}", language.tr(pen.tool.label()), language.tr("mm")));
    if response.clicked() {
        run(app, "draw.pen", json!({"index": index, "on": true}));
    }
    // A selected pen's second click or its lower-right chevron opens its settings.
    let chevron =
        ui.interact(Rect::from_min_max(rect.right_bottom() - vec2(15.0, 16.0), rect.right_bottom()), ui.id().with("settings"), Sense::click());
    let c = chevron.rect.center();
    ui.painter().line_segment([c + vec2(-3.0, -2.0), c + vec2(0.0, 1.0)], Stroke::new(1.2, t.text));
    ui.painter().line_segment([c + vec2(0.0, 1.0), c + vec2(3.0, -2.0)], Stroke::new(1.2, t.text));
    let open = chevron.clicked() || (response.clicked() && active);
    egui::Popup::from_response(&response).open_memory(open.then_some(egui::SetOpenCommand::Toggle)).show(|ui| settings(app, ui, index, pen));
}

/// Black or white, whichever reads better on `bg` (WCAG contrast).
fn on_color(bg: Color32) -> Color32 {
    let lin = |c: u8| {
        let c = c as f32 / 255.0;
        if c <= 0.04045 { c / 12.92 } else { ((c + 0.055) / 1.055).powf(2.4) }
    };
    let l = 0.2126 * lin(bg.r()) + 0.7152 * lin(bg.g()) + 0.0722 * lin(bg.b());
    // Contrast with black is (l + 0.05) / 0.05, with white 1.05 / (l + 0.05).
    if (l + 0.05) * (l + 0.05) > 0.0525 { Color32::BLACK } else { Color32::WHITE }
}

/// Draw with Trackpad: a switch where a big button has its icon, the label where it has its
/// label, so it lines up with the tool buttons.
fn trackpad_toggle(app: &mut SheetApp, ui: &mut Ui) {
    let t = Tokens::get(ui.ctx());
    let language = crate::i18n::current(ui.ctx());
    let on = app.session.draw.trackpad;
    let available = app.services.trackpad_capture.is_some();
    let font = crate::theme::ui_font(11.5);
    let label = language.tr("Draw with\nTrackpad");
    let lines: Vec<_> = label.split('\n').map(|line| ui.painter().layout_no_wrap(line.to_string(), font.clone(), t.text)).collect();
    let width = lines.iter().map(|l| l.size().x).fold(56.0, f32::max) + 12.0;
    let (rect, response) = ui.allocate_exact_size(vec2(width, 72.0), Sense::click());
    response.widget_info(|| egui::WidgetInfo::selected(egui::WidgetType::Checkbox, available || on, on, label.replace('\n', " ")));
    if response.hovered() && (available || on) {
        ui.painter().rect_filled(rect, 5.0, t.hover);
    }
    // The switch: a pill with a knob, ON/OFF on the free side in a colour that reads on it.
    let track = Rect::from_center_size(pos2(rect.center().x, rect.top() + 21.0), vec2(52.0, 24.0));
    let fill = if on { t.accent } else { t.separator };
    let fill = if available || on { fill } else { fill.gamma_multiply(0.5) };
    let p = ui.painter();
    p.rect_filled(track, 12.0, fill);
    if !on {
        p.rect_stroke(track, 12.0, Stroke::new(1.0, t.input_border), StrokeKind::Inside);
    }
    let knob = pos2(if on { track.right() - 12.0 } else { track.left() + 12.0 }, track.center().y);
    p.circle_filled(knob + vec2(0.0, 1.0), 9.5, t.shadow);
    p.circle_filled(knob, 9.5, Color32::WHITE);
    p.circle_stroke(knob, 9.5, Stroke::new(0.5, t.input_border));
    let state = language.tr(if on { "ON" } else { "OFF" });
    let text = if on { on_color(fill) } else { t.text };
    let text = if available || on { text } else { t.text_disabled };
    let side = pos2(if on { track.left() + 15.0 } else { track.right() - 15.0 }, track.center().y);
    p.text(side, egui::Align2::CENTER_CENTER, state, egui::FontId::new(9.5, egui::FontFamily::Proportional), text);
    for (i, line) in lines.iter().enumerate() {
        let y = rect.top() + 46.0 + i as f32 * 13.0;
        p.galley(
            pos2(rect.center().x - line.size().x / 2.0, y - line.size().y / 2.0),
            line.clone(),
            if available || on { t.text } else { t.text_disabled },
        );
    }
    let hint = app.services.trackpad_unavailable.as_deref().unwrap_or(if available { "Draw with Trackpad" } else { "Trackpad capture unavailable" });
    if response.on_hover_text(language.tr(hint)).clicked() && (available || on) {
        run(app, "draw.trackpad", json!({"on": !on}));
    }
}

fn mix(a: Color32, b: Color32, t: f32) -> Color32 {
    let m = |x: u8, y: u8| (x as f32 + (y as f32 - x as f32) * t).round() as u8;
    Color32::from_rgb(m(a.r(), b.r()), m(a.g(), b.g()), m(a.b(), b.b()))
}

/// A pen in the gallery, drawn upright: a shaded barrel in the pen's colour (or effect), a
/// band, then the nib by tool — a metal cone and fine point (pen), sharpened wood and lead
/// (pencil), a wide chisel (highlighter). Original artwork.
fn paint_pen(ui: &Ui, rect: Rect, pen: &InkPen) {
    let t = Tokens::get(ui.ctx());
    let p = ui.painter();
    let ink = rgb(pen.color);
    let outline = Stroke::new(1.0, if t.dark { Color32::from_gray(20) } else { Color32::from_gray(120) });
    let cx = rect.center().x;
    let half = if pen.tool == InkTool::Highlighter { 8.5 } else { 7.0 };
    let top = rect.top() + 6.0;
    let barrel = Rect::from_min_max(pos2(cx - half, top), pos2(cx + half, top + 25.0));
    let rounding = egui::CornerRadius { nw: 5, ne: 5, sw: 0, se: 0 };
    match pen.effect {
        Some(effect) => {
            p.rect_filled(barrel, rounding, ink);
            effect_swatch(ui, barrel.shrink2(vec2(0.0, 2.0)).translate(vec2(0.0, 2.0)), effect);
        }
        None => {
            p.rect_filled(barrel, rounding, ink);
        }
    }
    // Light from the left: a highlight and a shaded right edge give the barrel some roundness.
    p.rect_filled(
        Rect::from_min_max(pos2(barrel.left() + 3.0, top + 3.0), pos2(barrel.left() + 5.0, barrel.bottom() - 2.0)),
        1.0,
        Color32::from_white_alpha(90),
    );
    p.rect_filled(
        Rect::from_min_max(pos2(barrel.right() - 3.5, top + 2.0), pos2(barrel.right(), barrel.bottom())),
        egui::CornerRadius { ne: 5, ..Default::default() },
        Color32::from_black_alpha(40),
    );
    p.rect_stroke(barrel, rounding, outline, StrokeKind::Inside);
    // The band where the nib meets the barrel (a pencil has none: its paint ends in a zigzag).
    let cone_top = if pen.tool == InkTool::Pencil {
        barrel.bottom()
    } else {
        let band = Rect::from_min_max(pos2(barrel.left() - 0.5, barrel.bottom()), pos2(barrel.right() + 0.5, barrel.bottom() + 4.0));
        p.rect_filled(band, 1.0, Color32::from_rgb(0x8A, 0x90, 0x99));
        p.rect_stroke(band, 1.0, outline, StrokeKind::Inside);
        band.bottom()
    };
    match pen.tool {
        InkTool::Pen => {
            let tip = pos2(cx, cone_top + 17.0);
            let cone =
                vec![pos2(cx - half + 1.0, cone_top), pos2(cx + half - 1.0, cone_top), pos2(cx + 1.5, tip.y - 4.0), pos2(cx - 1.5, tip.y - 4.0)];
            p.add(egui::epaint::PathShape::convex_polygon(cone, Color32::from_rgb(0xD5, 0xDA, 0xE1), outline));
            p.add(egui::epaint::PathShape::convex_polygon(
                vec![pos2(cx - 1.5, tip.y - 4.0), pos2(cx + 1.5, tip.y - 4.0), tip],
                mix(ink, Color32::BLACK, 0.15),
                Stroke::NONE,
            ));
        }
        InkTool::Pencil => {
            let lead = pos2(cx, cone_top + 21.0);
            // The wood is outlined on its sides only: the paint's edge is the zigzag below.
            let (wl, wr) = (pos2(cx - half, cone_top), pos2(cx + half, cone_top));
            let (nl, nr) = (pos2(cx - 2.2, lead.y - 5.0), pos2(cx + 2.2, lead.y - 5.0));
            p.add(egui::epaint::PathShape::convex_polygon(vec![wl, wr, nr, nl], Color32::from_rgb(0xE9, 0xC9, 0x9A), Stroke::NONE));
            p.line_segment([wl, nl], outline);
            p.line_segment([wr, nr], outline);
            // Cover the barrel outline's bottom edge so paint runs straight into the teeth.
            p.rect_filled(Rect::from_min_max(pos2(barrel.left() + 1.0, cone_top - 1.5), pos2(barrel.right() - 1.0, cone_top + 0.5)), 0.0, ink);
            p.add(egui::epaint::PathShape::convex_polygon(
                vec![pos2(cx - 2.2, lead.y - 5.0), pos2(cx + 2.2, lead.y - 5.0), lead],
                mix(ink, Color32::BLACK, 0.2),
                Stroke::NONE,
            ));
            // Sharpening leaves the paint in a zigzag over the bare wood.
            const TEETH: usize = 3;
            let tooth = 2.0 * half / TEETH as f32;
            let mut edge = vec![pos2(cx - half, cone_top)];
            for i in 0..TEETH {
                let x = cx - half + i as f32 * tooth;
                let (a, apex, b) = (pos2(x, cone_top), pos2(x + tooth / 2.0, cone_top + 4.0), pos2(x + tooth, cone_top));
                p.add(egui::epaint::PathShape::convex_polygon(vec![a, b, apex], ink, Stroke::NONE));
                edge.extend([apex, b]);
            }
            p.add(egui::epaint::PathShape::line(edge, outline));
        }
        InkTool::Highlighter => {
            let neck = vec![
                pos2(cx - half + 1.0, cone_top),
                pos2(cx + half - 1.0, cone_top),
                pos2(cx + 4.5, cone_top + 8.0),
                pos2(cx - 4.5, cone_top + 8.0),
            ];
            p.add(egui::epaint::PathShape::convex_polygon(neck, Color32::from_rgb(0xD5, 0xDA, 0xE1), outline));
            // A chisel nib, cut at an angle.
            let nib = vec![
                pos2(cx - 4.5, cone_top + 8.0),
                pos2(cx + 4.5, cone_top + 8.0),
                pos2(cx + 4.5, cone_top + 12.0),
                pos2(cx - 4.5, cone_top + 16.0),
            ];
            p.add(egui::epaint::PathShape::convex_polygon(nib, ink, outline));
        }
    }
}

fn effect_swatch(ui: &Ui, r: Rect, effect: InkEffect) {
    let columns = (r.width().ceil() as usize).clamp(1, 96);
    let rows = (r.height().ceil() as usize).clamp(1, 64);
    let mut mesh = egui::Mesh::default();
    for y in 0..=rows {
        for x in 0..=columns {
            let c = effect.color_at([x as f32 * 90.0 / columns as f32, y as f32 * 63.0 / rows as f32]);
            mesh.colored_vertex(
                pos2(r.left() + r.width() * x as f32 / columns as f32, r.top() + r.height() * y as f32 / rows as f32),
                Color32::from_rgb(c[0], c[1], c[2]),
            );
        }
    }
    for y in 0..rows {
        for x in 0..columns {
            let a = (y * (columns + 1) + x) as u32;
            let b = a + columns as u32 + 1;
            mesh.add_triangle(a, a + 1, b + 1);
            mesh.add_triangle(a, b + 1, b);
        }
    }
    ui.painter().add(mesh);
}

fn settings(app: &mut SheetApp, ui: &mut Ui, index: usize, pen: &InkPen) {
    let t = Tokens::get(ui.ctx());
    let language = crate::i18n::current(ui.ctx());
    ui.set_width(242.0);
    let (r, _) = ui.allocate_exact_size(vec2(230.0, 22.0), Sense::hover());
    match pen.effect {
        Some(effect) => effect_swatch(ui, Rect::from_center_size(r.center(), vec2(224.0, 5.0)), effect),
        None => {
            ui.painter().line_segment([r.left_center(), r.right_center()], Stroke::new(pen.width.min(12.0), rgb(pen.color)));
        }
    }
    ui.separator();
    ui.horizontal(|ui| {
        let widths: &[f32] = if pen.tool == InkTool::Highlighter { &[1.0, 2.0, 4.0, 6.0, 8.0, 10.0] } else { &[0.25, 0.35, 0.5, 1.0, 2.0, 3.5] };
        for (i, mm) in widths.iter().enumerate() {
            let (r, response) = ui.allocate_exact_size(vec2(33.0, 35.0), Sense::click());
            let label = format!("{mm} {}", language.tr("mm"));
            response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, &label));
            ui.painter().circle_filled(r.center(), 2.0 + i as f32 * 1.7, t.text_dim);
            let px = gridcraft_engine::model::mm_to_px(*mm);
            if (pen.width - px).abs() < 0.05 {
                ui.painter().circle_stroke(r.center(), 14.0, Stroke::new(1.3, t.text));
            }
            if response.on_hover_text(label).clicked() {
                run(app, "draw.pen.update", json!({"index": index, "width": px}));
            }
        }
    });
    ui.separator();
    ui.label(language.tr("Colors"));
    egui::Grid::new("ink_colors").spacing(vec2(3.0, 3.0)).show(ui, |ui| {
        for (i, (label, value)) in COLORS.iter().enumerate() {
            let (r, response) = ui.allocate_exact_size(vec2(52.0, 33.0), Sense::click());
            response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, language.tr(label)));
            ui.painter().rect_filled(r, 0.0, rgb(Color::Rgb(*value)));
            ui.painter().rect_stroke(r, 0.0, Stroke::new(1.0, t.input_border), StrokeKind::Inside);
            if pen.effect.is_none() && pen.color == Color::Rgb(*value) {
                ui.painter().rect_stroke(r.shrink(1.0), 0.0, Stroke::new(2.0, t.accent), StrokeKind::Inside);
            }
            if response.on_hover_text(language.tr(label)).clicked() {
                run(app, "draw.pen.update", json!({"index": index, "color": format!("#{value:06X}")}));
            }
            if i % 4 == 3 {
                ui.end_row();
            }
        }
    });
    if pen.tool == InkTool::Pen {
        ui.separator();
        ui.label(language.tr("Effects"));
        egui::Grid::new("ink_effects").spacing(vec2(3.0, 3.0)).show(ui, |ui| {
            for (i, effect) in InkEffect::ALL.into_iter().enumerate() {
                let (r, response) = ui.allocate_exact_size(vec2(52.0, 33.0), Sense::click());
                response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, language.tr(effect.label())));
                effect_swatch(ui, r, effect);
                if pen.effect == Some(effect) {
                    ui.painter().rect_stroke(r.shrink(1.0), 0.0, Stroke::new(2.0, t.accent), StrokeKind::Inside);
                }
                if response.on_hover_text(language.tr(effect.label())).clicked() {
                    run(app, "draw.pen.update", json!({"index": index, "effect": effect.id()}));
                }
                if i % 4 == 3 {
                    ui.end_row();
                }
            }
        });
    }
    ui.separator();
    if ui.button(language.tr("More Colors…")).clicked() {
        app.draw_ui.color_picker = Some(index);
        ui.close();
    }
    ui.separator();
    if ui.add_enabled(app.session.prefs.ink_pens.len() > 1, egui::Button::new(language.tr("Delete"))).clicked() {
        run(app, "draw.pen.delete", json!({"index": index}));
        ui.close();
    }
}

/// More Colors…: a full colour picker for one pen.
pub(crate) fn color_picker_window(app: &mut SheetApp, ctx: &egui::Context) {
    let Some(index) = app.draw_ui.color_picker else { return };
    let Some(pen) = app.session.prefs.ink_pens.get(index).cloned() else {
        app.draw_ui.color_picker = None;
        return;
    };
    let mut open = true;
    let lang = crate::i18n::current(ctx);
    egui::Window::new(lang.tr("More Colors…")).id(egui::Id::new("ink_custom_color")).collapsible(false).resizable(false).open(&mut open).show(
        ctx,
        |ui| {
            let mut c = rgb(pen.color);
            if egui::color_picker::color_picker_color32(ui, &mut c, egui::color_picker::Alpha::Opaque) {
                run(app, "draw.pen.update", json!({"index": index, "color": format!("#{:02X}{:02X}{:02X}", c.r(), c.g(), c.b())}));
            }
        },
    );
    if !open {
        app.draw_ui.color_picker = None;
    }
}
