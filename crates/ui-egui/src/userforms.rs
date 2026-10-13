//! Drawing the workbook's UserForms and turning what the user does on them into VBA events.
//!
//! The form model (controls, text, lists, colours) lives in the engine
//! ([`gridcraft_engine::vba::forms`]); this draws it at the designed positions and sizes (points
//! scaled to pixels) and reports actions: typing sets a control's text (the engine fires
//! `Change`), clicks fire `Click`, focus moves fire `Enter`/`Exit`/`AfterUpdate`, Enter fires
//! `KeyDown`, the close box unloads the form.

use std::collections::HashMap;

use egui::{Color32, FontId, Rect, Sense, Stroke, StrokeKind, pos2, vec2};
use gridcraft_engine::vba::forms::{ControlState, FormState};
use gridcraft_engine::vba::{self as evba};
use gridcraft_engine::vbalang::forms::ControlKind;
use gridcraft_engine::vbalang::{Event, Value};

use crate::{SheetApp, theme};

/// Points to pixels (forms are designed in points at 96 dpi).
const K: f32 = 96.0 / 72.0;

/// Per-control UI memory: the text when the control got the focus (for `AfterUpdate`).
#[derive(Default)]
pub struct FormsUi {
    entered: HashMap<(String, String), String>,
    focus: HashMap<String, String>,
}

enum Action {
    Text(String, String, String),
    Click(String, String),
    DblClick(String, String),
    Select(String, String, i64),
    Check(String, String, bool),
    Spin(String, String, bool),
    Focus(String, String),
    Enter(String, String),
    Close(String),
}

fn rgb(c: Option<u32>, fallback: Color32) -> Color32 {
    match c {
        Some(v) => Color32::from_rgb((v >> 16) as u8, (v >> 8) as u8, v as u8),
        None => fallback,
    }
}

fn font(c: &ControlState, form: &FormState) -> FontId {
    let f = c.design.font.as_ref().or(form.design.font.as_ref());
    let size = c.font_size.or(f.map(|f| f.size)).unwrap_or(8.25).clamp(4.0, 200.0) * K;
    let name = f.map(|f| f.name.as_str()).unwrap_or("Tahoma");
    let bold = c.bold.unwrap_or(f.is_some_and(|f| f.bold));
    FontId::new(size, theme::cell_family(name, bold, f.is_some_and(|f| f.italic)))
}

/// Draws every visible form of the active workbook and applies what the user did.
pub fn show(app: &mut SheetApp, ctx: &egui::Context) {
    let Some(uid) = evba::active_uid(&app.session) else { return };
    let mut forms: Vec<FormState> = app.session.macros.doc(uid).map(|d| d.forms.iter().filter(|f| f.visible).cloned().collect()).unwrap_or_default();
    if forms.is_empty() {
        return;
    }
    forms.sort_by_key(|f| f.shown_order);
    // A modal form blocks the sheet and the forms under it.
    let top_modal = forms.iter().rposition(|f| f.modal);
    if top_modal.is_some() {
        let screen = ctx.content_rect();
        egui::Area::new(egui::Id::new("userform_block")).order(egui::Order::Middle).fixed_pos(screen.min).show(ctx, |ui| {
            let (r, _) = ui.allocate_exact_size(screen.size(), Sense::click_and_drag());
            ui.painter().rect_filled(r, 0.0, Color32::from_black_alpha(18));
        });
    }
    let mut actions: Vec<Action> = Vec::new();
    for (i, form) in forms.iter().enumerate() {
        let enabled = top_modal.is_none_or(|t| i >= t);
        draw_form(ctx, form, enabled, &mut actions);
    }
    apply(app, uid, actions);
}

fn draw_form(ctx: &egui::Context, form: &FormState, enabled: bool, actions: &mut Vec<Action>) {
    let name = form.name().to_string();
    let size = vec2(form.width.max(40.0), form.height.max(30.0)) * K;
    let back = rgb(form.back, Color32::from_rgb(0xF0, 0xF0, 0xF0));
    let mut open = true;
    egui::Window::new(egui::RichText::new(&form.caption).font(theme::ui_font(13.0)))
        .id(egui::Id::new(("userform", name.as_str())))
        .collapsible(false)
        .resizable(false)
        .order(egui::Order::Foreground)
        .pivot(egui::Align2::CENTER_CENTER)
        .default_pos(ctx.content_rect().center())
        .frame(egui::Frame::window(&ctx.global_style()).fill(back).inner_margin(0))
        .open(&mut open)
        .show(ctx, |ui| {
            ui.add_enabled_ui(enabled, |ui| {
                let (rect, surface) = ui.allocate_exact_size(size, Sense::click());
                ui.painter().rect_filled(rect, 0.0, back);
                // A click on the form itself (not a control) is the form's Click event.
                if surface.clicked() {
                    actions.push(Action::Click(name.clone(), "UserForm".into()));
                }
                let origin = rect.min;
                for c in form.controls.iter().filter(|c| c.visible) {
                    let r = Rect::from_min_size(origin + vec2(c.left, c.top) * K, vec2(c.width.max(1.0), c.height.max(1.0)) * K);
                    draw_control(ui, form, c, r, actions);
                }
            });
        });
    if !open {
        actions.push(Action::Close(name));
    }
}

fn draw_control(ui: &mut egui::Ui, form: &FormState, c: &ControlState, r: Rect, actions: &mut Vec<Action>) {
    let fname = form.name().to_string();
    let cname = c.name().to_string();
    let f = font(c, form);
    let fore = rgb(c.fore, Color32::BLACK);
    let painter = ui.painter_at(r);
    let enabled = c.enabled;
    let id = egui::Id::new(("uf", fname.as_str(), cname.as_str()));
    match c.kind() {
        ControlKind::Label => {
            if !c.design.transparent {
                painter.rect_filled(r, 0.0, rgb(c.back, Color32::TRANSPARENT));
            }
            let galley = ui.painter().layout(c.caption.clone(), f, fore, if c.design.word_wrap { r.width() } else { f32::INFINITY });
            let x = match c.design.align {
                3 => r.center().x - galley.size().x / 2.0,
                2 => r.right() - galley.size().x,
                _ => r.left(),
            };
            painter.galley(pos2(x, r.top()), galley, fore);
            let resp = ui.interact(r, id, Sense::click());
            if resp.clicked() {
                actions.push(Action::Click(fname, cname));
            }
        }
        ControlKind::CommandButton => {
            let back = rgb(c.back, Color32::from_rgb(0xE1, 0xE1, 0xE1));
            let resp = ui.put(
                r,
                egui::Button::new(egui::RichText::new(&c.caption).font(f).color(fore)).fill(back).stroke(Stroke::new(1.0, Color32::from_gray(0xAD))),
            );
            if resp.clicked() && enabled {
                actions.push(Action::Focus(fname.clone(), cname.clone()));
                actions.push(Action::Click(fname, cname));
            }
        }
        ControlKind::TextBox => {
            let mut text = c.text.clone();
            let back = rgb(c.back, Color32::WHITE);
            painter.rect_filled(r, 0.0, back);
            painter.rect_stroke(r, 0.0, Stroke::new(1.0, Color32::from_gray(0x7A)), StrokeKind::Inside);
            let mut edit = if c.design.multiline { egui::TextEdit::multiline(&mut text) } else { egui::TextEdit::singleline(&mut text) };
            edit = edit.id(id).font(f).text_color(fore).frame(egui::Frame::NONE).desired_width(r.width() - 6.0).interactive(enabled && !c.locked);
            if let Some(p) = c.design.password_char {
                edit = edit.password(p != '\0');
            }
            let resp = ui.put(r.shrink(2.0), edit);
            if resp.changed() {
                actions.push(Action::Text(fname.clone(), cname.clone(), text));
            }
            if resp.gained_focus() {
                actions.push(Action::Focus(fname.clone(), cname.clone()));
            }
            if resp.has_focus() || resp.lost_focus() {
                // Enter in a one-line box: KeyDown, then the focus moves on.
                let enter = ui.input(|i| i.key_pressed(egui::Key::Enter));
                if enter && !c.design.multiline {
                    actions.push(Action::Enter(fname, cname));
                }
            }
        }
        ControlKind::ListBox => {
            let back = rgb(c.back, Color32::WHITE);
            painter.rect_filled(r, 0.0, back);
            painter.rect_stroke(r, 0.0, Stroke::new(1.0, Color32::from_gray(0x7A)), StrokeKind::Inside);
            let row_h = f.size * 1.35;
            let widths = column_widths(c, r.width());
            let mut child = ui.new_child(egui::UiBuilder::new().max_rect(r.shrink(1.0)).layout(egui::Layout::top_down(egui::Align::Min)));
            egui::ScrollArea::vertical().id_salt(id).auto_shrink([false, false]).show(&mut child, |ui| {
                for (i, row) in c.list.iter().enumerate() {
                    let (rr, resp) = ui.allocate_exact_size(vec2(ui.available_width(), row_h), Sense::click());
                    let selected = c.list_index == i as i64 || c.selected.get(i).copied().unwrap_or(false);
                    if selected {
                        ui.painter().rect_filled(rr, 0.0, Color32::from_rgb(0x00, 0x78, 0xD7));
                    } else if resp.hovered() {
                        ui.painter().rect_filled(rr, 0.0, Color32::from_rgb(0xE5, 0xF1, 0xFB));
                    }
                    let color = if selected { Color32::WHITE } else { fore };
                    let mut x = rr.left() + 2.0;
                    for (k, cell) in row.iter().enumerate().take(widths.len().max(1)) {
                        let w = widths.get(k).copied().unwrap_or(rr.width());
                        let clip = Rect::from_min_size(pos2(x, rr.top()), vec2(w - 2.0, rr.height()));
                        ui.painter().with_clip_rect(clip).text(pos2(x, rr.center().y), egui::Align2::LEFT_CENTER, cell, f.clone(), color);
                        x += w;
                    }
                    if resp.clicked() && c.enabled {
                        actions.push(Action::Focus(fname.clone(), cname.clone()));
                        actions.push(Action::Select(fname.clone(), cname.clone(), i as i64));
                    }
                    if resp.double_clicked() && c.enabled {
                        actions.push(Action::DblClick(fname.clone(), cname.clone()));
                    }
                }
            });
        }
        ControlKind::ComboBox => {
            let mut chosen: Option<i64> = None;
            let mut child = ui.new_child(egui::UiBuilder::new().max_rect(r));
            egui::ComboBox::from_id_salt(id).width(r.width() - 4.0).selected_text(egui::RichText::new(&c.text).font(f.clone()).color(fore)).show_ui(
                &mut child,
                |ui| {
                    for (i, row) in c.list.iter().enumerate() {
                        let label = row.first().cloned().unwrap_or_default();
                        if ui.selectable_label(c.list_index == i as i64, egui::RichText::new(label).font(f.clone())).clicked() {
                            chosen = Some(i as i64);
                        }
                    }
                },
            );
            if let Some(i) = chosen {
                actions.push(Action::Select(fname, cname, i));
            }
        }
        ControlKind::CheckBox | ControlKind::OptionButton | ControlKind::ToggleButton => {
            let mut on = c.checked;
            let label = egui::RichText::new(&c.caption).font(f).color(fore);
            let resp = if c.kind() == ControlKind::OptionButton {
                ui.put(r, egui::RadioButton::new(on, label))
            } else {
                ui.put(r, egui::Checkbox::new(&mut on, label))
            };
            if resp.clicked() && enabled {
                let new = if c.kind() == ControlKind::OptionButton { true } else { on };
                actions.push(Action::Check(fname, cname, new));
            }
        }
        ControlKind::SpinButton | ControlKind::ScrollBar => {
            let vertical = r.height() >= r.width();
            let (a, b) = if vertical {
                (Rect::from_min_max(r.min, pos2(r.right(), r.center().y)), Rect::from_min_max(pos2(r.left(), r.center().y), r.max))
            } else {
                (Rect::from_min_max(pos2(r.center().x, r.top()), r.max), Rect::from_min_max(r.min, pos2(r.center().x, r.bottom())))
            };
            let up = ui.put(a, egui::Button::new(if vertical { "▲" } else { "▶" }).small());
            let down = ui.put(b, egui::Button::new(if vertical { "▼" } else { "◀" }).small());
            if up.clicked() && enabled {
                actions.push(Action::Spin(fname.clone(), cname.clone(), true));
            }
            if down.clicked() && enabled {
                actions.push(Action::Spin(fname, cname, false));
            }
        }
        ControlKind::Frame | ControlKind::Image | ControlKind::MultiPage | ControlKind::TabStrip | ControlKind::Other(_) => {
            if let Some(b) = c.back {
                painter.rect_filled(r, 0.0, rgb(Some(b), Color32::TRANSPARENT));
            }
            painter.rect_stroke(r, 0.0, Stroke::new(1.0, Color32::from_gray(0xA0)), StrokeKind::Inside);
            if !c.caption.is_empty() {
                painter.text(r.left_top() + vec2(6.0, 0.0), egui::Align2::LEFT_CENTER, &c.caption, f, fore);
            }
        }
    }
}

/// Column widths in pixels from `ColumnWidths` (`"50;80"` or `"50 pt;80 pt"`), the rest shared.
fn column_widths(c: &ControlState, total: f32) -> Vec<f32> {
    let n = c.column_count.max(1) as usize;
    let mut given: Vec<Option<f32>> =
        c.column_widths.split(';').map(|w| w.trim().trim_end_matches("pt").trim().parse::<f32>().ok().map(|v| v * K)).collect();
    given.resize(n, None);
    let used: f32 = given.iter().flatten().sum();
    let free = given.iter().filter(|g| g.is_none()).count().max(1) as f32;
    let share = ((total - used) / free).max(20.0);
    given.into_iter().take(n).map(|g| g.unwrap_or(share)).collect()
}

fn ev(module: &str, proc: String, args: Vec<Value>) -> Event {
    Event { module: module.to_string(), proc, args }
}

/// Applies the user's actions: model changes through the engine (which queues `Change`/`Click`),
/// other events to the macro.
fn apply(app: &mut SheetApp, uid: u64, actions: Vec<Action>) {
    for a in actions {
        match a {
            Action::Text(form, ctl, text) => {
                let _ = evba::control_member(&mut app.session, uid, &form, &ctl, "Text", &[], Some(Value::from(text)));
            }
            Action::Click(form, ctl) => crate::macros::event(app, uid, ev(&form, format!("{ctl}_Click"), vec![])),
            Action::DblClick(form, ctl) => crate::macros::event(app, uid, ev(&form, format!("{ctl}_DblClick"), vec![Value::Bool(false)])),
            Action::Select(form, ctl, i) => {
                let _ = evba::control_member(&mut app.session, uid, &form, &ctl, "ListIndex", &[], Some(Value::Int(i)));
            }
            Action::Check(form, ctl, on) => {
                let _ = evba::control_member(&mut app.session, uid, &form, &ctl, "Value", &[], Some(Value::Bool(on)));
            }
            Action::Spin(form, ctl, up) => {
                let pos =
                    evba::control_member(&mut app.session, uid, &form, &ctl, "Value", &[], None).ok().and_then(|v| v.to_int().ok()).unwrap_or(0);
                let step = app
                    .session
                    .macros
                    .doc(uid)
                    .and_then(|d| d.forms.iter().find(|f| f.name().eq_ignore_ascii_case(&form)))
                    .and_then(|f| f.controls.iter().find(|c| c.name().eq_ignore_ascii_case(&ctl)))
                    .map_or(1, |c| i64::from(c.design.small_change.max(1)));
                let _ = evba::control_member(
                    &mut app.session,
                    uid,
                    &form,
                    &ctl,
                    "Value",
                    &[],
                    Some(Value::Int(if up { pos + step } else { pos - step })),
                );
                crate::macros::event(app, uid, ev(&form, format!("{ctl}_{}", if up { "SpinUp" } else { "SpinDown" }), vec![]));
            }
            Action::Focus(form, ctl) => focus(app, uid, &form, &ctl),
            Action::Enter(form, ctl) => {
                crate::macros::event(app, uid, ev(&form, format!("{ctl}_KeyDown"), vec![Value::Int(13), Value::Int(0)]));
                // The focus moves to the next control in tab order.
                let next = app.session.macros.doc(uid).and_then(|d| d.forms.iter().find(|f| f.name().eq_ignore_ascii_case(&form))).and_then(|f| {
                    let i = f.control(&ctl)?;
                    let n = f.controls.len();
                    (1..n)
                        .map(|k| (i + k) % n)
                        .find(|&k| f.controls.get(k).is_some_and(|c| c.visible && c.enabled && c.design.tab_stop && c.kind() != ControlKind::Label))
                        .and_then(|k| f.controls.get(k))
                        .map(|c| c.name().to_string())
                });
                if let Some(next) = next {
                    focus(app, uid, &form, &next);
                    app.forms.focus_request = Some((form, next));
                }
            }
            Action::Close(form) => {
                crate::macros::event(app, uid, ev(&form, "UserForm_QueryClose".into(), vec![Value::Int(0), Value::Int(0)]));
                evba::unload(&mut app.session, uid, &form);
            }
        }
    }
}

/// The focus moves to `ctl`: `Exit` (and `AfterUpdate` when its text changed) on the control
/// that had it, `Enter` on the new one.
fn focus(app: &mut SheetApp, uid: u64, form: &str, ctl: &str) {
    let prev = app.forms.ui.focus.get(form).cloned();
    if prev.as_deref().is_some_and(|p| p.eq_ignore_ascii_case(ctl)) {
        return;
    }
    if let Some(p) = prev {
        let now = evba::control_member(&mut app.session, uid, form, &p, "Text", &[], None).ok().and_then(|v| v.to_text().ok()).unwrap_or_default();
        let before = app.forms.ui.entered.get(&(form.to_string(), p.clone())).cloned().unwrap_or_default();
        if now != before {
            crate::macros::event(app, uid, ev(form, format!("{p}_BeforeUpdate"), vec![Value::Bool(false)]));
            crate::macros::event(app, uid, ev(form, format!("{p}_AfterUpdate"), vec![]));
        }
        crate::macros::event(app, uid, ev(form, format!("{p}_Exit"), vec![Value::Bool(false)]));
    }
    let text = evba::control_member(&mut app.session, uid, form, ctl, "Text", &[], None).ok().and_then(|v| v.to_text().ok()).unwrap_or_default();
    app.forms.ui.entered.insert((form.to_string(), ctl.to_string()), text);
    app.forms.ui.focus.insert(form.to_string(), ctl.to_string());
    crate::macros::event(app, uid, ev(form, format!("{ctl}_Enter"), vec![]));
}

/// UI state for forms kept on the app.
#[derive(Default)]
pub struct Forms {
    pub ui: FormsUi,
    /// A control the focus should move to next frame.
    pub focus_request: Option<(String, String)>,
}

/// Moves egui's focus to a requested control.
pub fn apply_focus(app: &mut SheetApp, ctx: &egui::Context) {
    if let Some((form, ctl)) = app.forms.focus_request.take() {
        ctx.memory_mut(|m| m.request_focus(egui::Id::new(("uf", form.as_str(), ctl.as_str()))));
    }
}

/// The fonts the active workbook's forms use (for binding installed files).
pub fn fonts(app: &SheetApp) -> Vec<String> {
    let mut out = Vec::new();
    if let Some(d) = app.session.active().and_then(|d| app.session.macros.doc(d.uid))
        && let Some(c) = &d.contents
    {
        for f in &c.forms {
            out.extend(f.font.iter().map(|x| x.name.clone()));
            out.extend(f.controls.iter().filter_map(|c| c.font.as_ref().map(|x| x.name.clone())));
        }
    }
    out.sort();
    out.dedup();
    out
}
