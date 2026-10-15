//! The formula bar (Name Box, ✕ ✓ fx, formula field) and the shared cell editor widget.

use egui::{Color32, FontId, Key, Rect, Sense, Stroke, StrokeKind, TextEdit, pos2, vec2};
use serde_json::json;

use crate::SheetApp;
use crate::editor::{REF_COLORS, formula_refs};
use crate::icons::{self, Icon};
use crate::theme::{self, Tokens};

pub fn show(app: &mut SheetApp, ui: &mut egui::Ui) {
    let t = Tokens::get(ui.ctx());
    let expanded = app.ui.formula_bar_expanded;
    let h = if expanded { 72.0 } else { 30.0 };
    egui::Panel::top("formula_bar")
        .exact_size(h)
        .frame(egui::Frame::NONE.fill(t.window).inner_margin(egui::Margin { left: 8, right: 8, top: 3, bottom: 3 }))
        .show(ui, |ui| {
            crate::i18n::column(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 4.0;
                    name_box(app, ui, &t);
                    ui.add_space(4.0);
                    let editing = app.editor.is_some();
                    let c = if editing { t.text } else { t.text_disabled };
                    if crate::widgets::icon_button(ui, Icon::Close, c, "Cancel", vec2(22.0, 22.0)).clicked() && editing {
                        app.cancel_edit();
                    }
                    if crate::widgets::icon_button(ui, Icon::Check, c, "Enter", vec2(22.0, 22.0)).clicked() && editing {
                        app.commit_edit(0, 0, false, false);
                    }
                    if crate::widgets::icon_button(ui, Icon::Fx, t.text, "Insert Function", vec2(26.0, 22.0)).clicked() {
                        app.open_dialog("insertFunction", json!({}));
                    }
                    ui.add_space(4.0);
                    // The formula field.
                    let avail = ui.available_width() - 24.0;
                    let field = crate::i18n::leading_rect(ui, vec2(avail, h - 6.0));
                    ui.painter().rect_filled(field, 4.0, t.input_bg);
                    ui.painter().rect_stroke(field, 4.0, Stroke::new(1.0, t.input_border), StrokeKind::Inside);
                    let mut child = ui.new_child(egui::UiBuilder::new().max_rect(field.shrink2(vec2(6.0, 3.0))));
                    let id = egui::Id::new("gridcraft.formula_bar");
                    let editing_here = app.editor.as_ref().is_some_and(|e| e.from_formula_bar);
                    if app.editor.is_some() {
                        editor_widget(app, &mut child, id, theme::ui_font(13.0), expanded, field.width() - 12.0);
                    } else {
                        // Show the active cell's content; clicking starts editing in the bar.
                        let text = active_input(app);
                        let resp = child
                            .add(egui::Label::new(crate::i18n::single_line(&text, theme::ui_font(13.0), t.text)).truncate().sense(Sense::click()));
                        let click = child.interact(field, id.with("click"), Sense::click());
                        if resp.clicked() || click.clicked() {
                            app.begin_edit(None, true);
                        }
                    }
                    let _ = editing_here;
                    ui.allocate_space(vec2(avail, h - 6.0));
                    let chevron = if expanded { Icon::ChevronUp } else { Icon::Chevron };
                    if crate::widgets::icon_button(ui, chevron, t.text_dim, "Expand formula bar", vec2(20.0, 22.0)).clicked() {
                        app.ui.formula_bar_expanded = !app.ui.formula_bar_expanded;
                    }
                });
            });
        });
}

fn active_input(app: &SheetApp) -> String {
    let Some(d) = app.session.active() else { return String::new() };
    let Some(sh) = d.wb.active() else { return String::new() };
    let a = sh.merge_at(d.selection.active).map(|m| m.start).unwrap_or(d.selection.active);
    if let Some(picture) = sh.cell_pictures.get(&a) {
        return if picture.alt.is_empty() { "Picture in cell".into() } else { format!("Picture: {}", picture.alt) };
    }
    if let Some(c) = sh.cell(a) {
        if c.formula.is_some() && d.wb.styles.get(c.style).protection.hidden && sh.is_protected() {
            return String::new();
        }
        return if let Some(f) = &c.formula {
            let shown = crate::formula_locale::display(&c.input_text(), app.ui.language.formula_locale(), &d.wb, d.wb.active_sheet);
            // A legacy array formula shows in braces (`{=A1:A2*2}`), as `Cell::bar_text` does.
            if f.array.is_some() { format!("{{{shown}}}") } else { shown }
        } else {
            gridcraft_engine::display::edit_text(&d.wb, sh, a)
        };
    }
    // A spilled cell shows the anchor's formula greyed out in Excel; we show it plainly.
    for (anchor, r) in &sh.spill_ranges {
        if r.contains(a)
            && let Some(f) = sh.cell(*anchor).and_then(|c| c.formula.as_ref())
        {
            if gridcraft_engine::display::formula_hidden(&d.wb, sh, *anchor) {
                return String::new();
            }
            return crate::formula_locale::display(&format!("={}", f.text), app.ui.language.formula_locale(), &d.wb, d.wb.active_sheet);
        }
    }
    String::new()
}

fn name_box(app: &mut SheetApp, ui: &mut egui::Ui, t: &Tokens) {
    let id = egui::Id::new("gridcraft.name_box");
    let shown = app.name_box_text();
    let mut text = app.name_box.clone().unwrap_or(shown.clone());
    let rect = crate::i18n::leading_rect(ui, vec2(118.0, 24.0));
    ui.painter().rect_filled(rect, 4.0, t.input_bg);
    ui.painter().rect_stroke(rect, 4.0, Stroke::new(1.0, t.input_border), StrokeKind::Inside);
    let rtl = app.ui.language.is_rtl();
    let input_x = if rtl { rect.right() - 102.0 } else { rect.left() + 6.0 };
    let mut child = ui.new_child(egui::UiBuilder::new().max_rect(Rect::from_min_size(pos2(input_x, rect.top() + 3.0), vec2(96.0, 18.0))));
    let resp = child.add(
        TextEdit::singleline(&mut text).id(id).frame(egui::Frame::NONE).font(theme::ui_font(13.0)).desired_width(96.0).horizontal_align(if rtl {
            egui::Align::RIGHT
        } else {
            egui::Align::LEFT
        }),
    );
    if resp.gained_focus() {
        app.name_box = Some(shown.clone());
    }
    if resp.changed() {
        app.name_box = Some(text.clone());
    }
    if resp.lost_focus() {
        if ui.input(|i| i.key_pressed(Key::Enter)) {
            // The Enter that commits the Name Box is used up here. Left in the frame's input, the
            // grid (drawn after the formula bar, and focused again now the box let go) would read
            // it as its own Enter and move the active cell one row down from the jump target (#247).
            ui.input_mut(|i| i.events.retain(|e| !matches!(e, egui::Event::Key { key: Key::Enter, .. })));
            let target = text.trim().to_string();
            if !target.is_empty() && target != shown {
                // A new name for the selection, or a place to go.
                let is_ref = gridcraft_engine::core::RangeRef::parse(target.split('!').next_back().unwrap_or("")).is_some();
                let known = app.session.active().is_some_and(|d| d.wb.name(&target, d.wb.active_sheet).is_some() || d.wb.table(&target).is_some());
                let r = if is_ref || known {
                    app.run("edit.goTo", json!({"reference": target}))
                } else {
                    app.run("formulas.defineName", json!({"name": target}))
                };
                if let Err(e) = r {
                    app.message = Some(("GridCraft".into(), crate::clean_error(&e)));
                }
                app.grid.ensure_visible = true;
            }
        }
        app.name_box = None;
    }
    ui.allocate_space(vec2(118.0, 24.0));
    // Dropdown of names.
    let names: Vec<String> =
        app.session.active().map(|d| d.wb.names.iter().filter(|n| !n.hidden).map(|n| n.name.clone()).collect()).unwrap_or_default();
    let mb = Rect::from_min_size(pos2(if rtl { rect.left() + 2.0 } else { rect.right() - 16.0 }, rect.top() + 4.0), vec2(14.0, 16.0));
    icons::paint(ui.painter(), mb, Icon::Chevron, t.text_dim);
    let r = ui.interact(mb, id.with("drop"), Sense::click());
    crate::i18n::menu(&r).show(|ui| {
        crate::i18n::column(ui, |ui| {
            if names.is_empty() {
                ui.label(egui::RichText::new(app.ui.language.tr("No names defined")).italics());
            }
            for n in names {
                if ui.button(&n).clicked() {
                    let _ = app.run("edit.goTo", json!({"reference": n}));
                    app.grid.ensure_visible = true;
                }
            }
        });
    });
}

/// The text editor used in the cell and in the formula bar (they share `app.editor`).
pub fn editor_widget(app: &mut SheetApp, ui: &mut egui::Ui, id: egui::Id, font: FontId, multiline: bool, width: f32) {
    let Some(mut ed) = app.editor.take() else { return };
    let mine = ed.from_formula_bar == (id == egui::Id::new("gridcraft.formula_bar"));
    // Keys the editor handles itself (before the TextEdit sees them), only where it has focus.
    let focused = ui.ctx().memory(|m| m.focused());
    let has_focus = focused == Some(id) || (mine && (ed.request_focus || focused.is_none()));
    let mut action: Option<(i64, i64, bool, bool)> = None;
    let mut cancel = false;
    if has_focus && mine {
        let (enter, tab, esc, alt_enter, cmd_enter, ctrl_shift_enter, shift) = ui.input_mut(|i| {
            let m = i.modifiers;
            let alt_enter =
                i.consume_key(egui::Modifiers::ALT, Key::Enter) || i.consume_key(egui::Modifiers::CTRL | egui::Modifiers::ALT, Key::Enter);
            let ctrl_shift_enter = i.consume_key(egui::Modifiers::CTRL | egui::Modifiers::SHIFT, Key::Enter);
            let cmd_enter = i.consume_key(egui::Modifiers::COMMAND, Key::Enter);
            let enter = i.consume_key(egui::Modifiers::NONE, Key::Enter) || i.consume_key(egui::Modifiers::SHIFT, Key::Enter);
            let tab = i.consume_key(egui::Modifiers::NONE, Key::Tab) || i.consume_key(egui::Modifiers::SHIFT, Key::Tab);
            let esc = i.consume_key(egui::Modifiers::NONE, Key::Escape);
            // Backspace takes back the AutoComplete proposal (Excel's selected rest), not a typed character.
            if ed.completion.is_some() && i.consume_key(egui::Modifiers::NONE, Key::Backspace) {
                ed.completion = None;
            }
            (enter, tab, esc, alt_enter, cmd_enter, ctrl_shift_enter, m.shift)
        });
        if alt_enter {
            let b = ed.text.char_indices().nth(ed.caret).map(|(i, _)| i).unwrap_or(ed.text.len());
            ed.text.insert(b, '\n');
            ed.caret += 1;
            ed.caret_to_end = false;
            ed.sync_caret = true;
        }
        if !ed.autocomplete.is_empty() && tab {
            ed.accept_autocomplete();
        } else if enter {
            action = Some((if shift { -1 } else { 1 }, 0, false, false));
        } else if tab {
            action = Some((0, if shift { -1 } else { 1 }, false, false));
        }
        if cmd_enter {
            action = Some((0, 0, false, true));
        }
        if ctrl_shift_enter {
            action = Some((0, 0, true, false));
        }
        if esc {
            if !ed.autocomplete.is_empty() {
                ed.autocomplete.clear();
            } else if ed.completion.is_some() {
                ed.completion = None;
            } else {
                cancel = true;
            }
        }
        // Arrow keys: autocomplete navigation, point mode, or (Enter mode) commit and move.
        let arrows = ui.input(|i| {
            (
                i.key_pressed(Key::ArrowUp),
                i.key_pressed(Key::ArrowDown),
                i.key_pressed(Key::ArrowLeft),
                i.key_pressed(Key::ArrowRight),
                i.modifiers.shift,
            )
        });
        let (up, down, left, right, ashift) = arrows;
        if (up || down) && !ed.autocomplete.is_empty() {
            ui.input_mut(|i| {
                i.consume_key(egui::Modifiers::NONE, Key::ArrowUp);
                i.consume_key(egui::Modifiers::NONE, Key::ArrowDown);
            });
            let n = ed.autocomplete.len();
            ed.ac_index = if down { (ed.ac_index + 1) % n } else { (ed.ac_index + n - 1) % n };
        } else if ed.enter_mode && (up || down || left || right) && !ed.from_formula_bar {
            let (dr, dc) = if up {
                (-1, 0)
            } else if down {
                (1, 0)
            } else if left {
                (0, -1)
            } else {
                (0, 1)
            };
            // Left/Right follow the screen on a right-to-left sheet.
            let rtl = app.session.active().and_then(|d| d.wb.active()).is_some_and(|s| s.right_to_left);
            let dc = if rtl { -dc } else { dc };
            if ed.can_point() {
                ui.input_mut(|i| {
                    for k in [Key::ArrowUp, Key::ArrowDown, Key::ArrowLeft, Key::ArrowRight] {
                        i.consume_key(egui::Modifiers::NONE, k);
                        i.consume_key(egui::Modifiers::SHIFT, k);
                    }
                });
                crate::grid::point_move(app, &mut ed, dr, dc, ashift);
            } else {
                action = Some((dr, dc, false, false));
            }
        }
        // F4 cycles $ anchors of the reference before the caret.
        if ui.input_mut(|i| i.consume_key(egui::Modifiers::NONE, Key::F4)) {
            cycle_anchor(&mut ed);
        }
    }
    // The text widget.
    let refs = formula_refs(&ed.text);
    let text_color = Tokens::get(ui.ctx()).text;
    let cell_text_color = if id == egui::Id::new("gridcraft.cell_editor") { Color32::BLACK } else { text_color };
    let font2 = font.clone();
    let mut layouter = move |ui: &egui::Ui, buf: &dyn egui::TextBuffer, wrap_width: f32| {
        let s = buf.as_str();
        let mut job = egui::text::LayoutJob::default();
        let mut pos = 0usize;
        let mut spans: Vec<(usize, usize, usize)> = refs.iter().filter(|r| r.1 <= s.len()).map(|r| (r.0, r.1, r.4)).collect();
        spans.sort();
        for (a, b, idx) in spans {
            if a < pos || !s.is_char_boundary(a) || !s.is_char_boundary(b) {
                continue;
            }
            job.append(s.get(pos..a).unwrap_or(""), 0.0, egui::TextFormat::simple(font2.clone(), cell_text_color));
            job.append(s.get(a..b).unwrap_or(""), 0.0, egui::TextFormat::simple(font2.clone(), REF_COLORS[idx]));
            pos = b;
        }
        job.append(s.get(pos..).unwrap_or(""), 0.0, egui::TextFormat::simple(font2.clone(), cell_text_color));
        job.wrap.max_width = if multiline { wrap_width } else { f32::INFINITY };
        ui.fonts_mut(|f| f.layout_job(job))
    };
    let rtl_text = !ed.is_formula() && !multiline && !ed.text.contains('\n') && crate::i18n::starts_rtl(&ed.text);
    // egui's character cursors are logical, but shaped Arabic glyphs are drawn in reverse
    // order. Keep the editing state logical and paint its caret at the visual boundary.
    let cursor_stroke = ui.visuals().text_cursor.stroke;
    if rtl_text {
        ui.visuals_mut().text_cursor.stroke = Stroke::NONE;
    }
    let edit = if multiline || ed.text.contains('\n') { TextEdit::multiline(&mut ed.text) } else { TextEdit::singleline(&mut ed.text) };
    let editor_clip = ui.available_rect_before_wrap().intersect(ui.clip_rect());
    let mut output = edit
        .id(id)
        .frame(egui::Frame::NONE)
        .font(font)
        .horizontal_align(if app.ui.language.is_rtl() { egui::Align::RIGHT } else { egui::Align::LEFT })
        .desired_width(width)
        .layouter(&mut layouter)
        .lock_focus(true)
        .event_filter(egui::EventFilter { tab: true, horizontal_arrows: true, vertical_arrows: true, escape: true })
        .show(ui);
    ui.visuals_mut().text_cursor.stroke = cursor_stroke;
    let resp = output.response.clone();
    let synced = mine && (ed.request_focus || ed.sync_caret);
    if synced {
        if ed.request_focus {
            resp.request_focus();
        }
        ed.request_focus = false;
        ed.sync_caret = false;
        let cursor = egui::text::CCursor::new(ed.caret);
        output.state.cursor.set_char_range(Some(egui::text::CCursorRange::one(cursor)));
        output.state.clone().store(ui.ctx(), id);
    } else if resp.clicked() || resp.gained_focus() {
        // Clicking into the other editor moves editing there.
        ed.from_formula_bar = id == egui::Id::new("gridcraft.formula_bar");
        ed.enter_mode = false;
        app.session.mode = gridcraft_engine::Mode::Edit;
    }
    if !synced && let Some(r) = output.cursor_range {
        ed.caret = r.primary.index.into();
    }
    if rtl_text
        && resp.clicked()
        && let Some(pointer) = ui.input(|i| i.pointer.interact_pos())
    {
        let visual = output.galley.cursor_from_pos(pointer - output.galley_pos);
        ed.caret = ed.text.chars().count().saturating_sub(visual.index.into());
        output.state.cursor.set_char_range(Some(egui::text::CCursorRange::one(egui::text::CCursor::new(ed.caret))));
        output.state.store(ui.ctx(), id);
    }
    if resp.changed() {
        ed.completion = None;
        if app.ui.autocomplete
            && !ed.is_formula()
            && ed.caret == ed.text.chars().count()
            && let Some(sh) = app.session.active().and_then(|d| d.wb.sheet(ed.sheet))
        {
            ed.completion = crate::editor::column_completion(sh, ed.cell, &ed.text);
        }
        ed.point = None;
        let names: Vec<String> = function_names()
            .iter()
            .map(|name| {
                app.session.active().map(|d| crate::formula_locale::completion_name(name, ed.locale, &d.wb, ed.sheet)).unwrap_or(name).to_string()
            })
            .collect();
        ed.update_autocomplete(&names);
        app.session.mode = if ed.can_point() {
            gridcraft_engine::Mode::Point
        } else if ed.enter_mode {
            gridcraft_engine::Mode::Enter
        } else {
            gridcraft_engine::Mode::Edit
        };
    }
    // Shape the complete suggestion together: splitting an Arabic word breaks its joins,
    // and positioning the suffix relative to the widget's left edge separates the word.
    let mut completion_caret = None;
    if let Some(full) = &ed.completion
        && crate::editor::completion_rest(full, &ed.text).is_some()
    {
        let mut g = ui.painter().layout_no_wrap(
            full.clone(),
            output.galley.job.sections.first().map(|s| s.format.font_id.clone()).unwrap_or_default(),
            cell_text_color,
        );
        let typed = ed.text.chars().count();
        let total = full.chars().count();
        let rtl = crate::i18n::starts_rtl(full);
        let pos = output.galley_pos + vec2(if app.ui.language.is_rtl() { output.galley.size().x - g.size().x } else { 0.0 }, 0.0);
        let (start, end) = if rtl { (0, total.saturating_sub(typed)) } else { (typed, total) };
        if rtl {
            completion_caret = Some(g.pos_from_cursor(egui::text::CCursor::new(total.saturating_sub(ed.caret))).translate(pos.to_vec2()));
        }
        let selected = egui::text::CCursorRange::two(egui::text::CCursor::new(start), egui::text::CCursor::new(end));
        let mut visuals = ui.visuals().clone();
        visuals.selection.bg_fill = Color32::from_rgb(0x2E, 0x6F, 0xD8);
        visuals.selection.stroke.color = Color32::WHITE;
        egui::text_selection::visuals::paint_text_selection(&mut g, &visuals, &selected, None);
        let painter = ui.painter().with_clip_rect(editor_clip);
        let background = if id == egui::Id::new("gridcraft.cell_editor") { Color32::WHITE } else { Tokens::get(ui.ctx()).input_bg };
        painter.rect_filled(
            egui::Rect::from_min_size(pos, g.size()).union(egui::Rect::from_min_size(output.galley_pos, output.galley.size())),
            0.0,
            background,
        );
        painter.galley(pos, g, cell_text_color);
    }
    if rtl_text && mine && (resp.has_focus() || synced) {
        let visual_index = ed.text.chars().count().saturating_sub(ed.caret);
        let caret = completion_caret
            .unwrap_or_else(|| output.galley.pos_from_cursor(egui::text::CCursor::new(visual_index)).translate(output.galley_pos.to_vec2()));
        let now = ui.input(|i| i.time);
        let interacted = synced || resp.changed() || resp.clicked();
        let last = ui.ctx().data_mut(|data| {
            let key = id.with("rtl_cursor_interaction");
            if interacted {
                data.insert_temp(key, now);
            }
            data.get_temp::<f64>(key).unwrap_or(now)
        });
        egui::text_selection::visuals::paint_text_cursor(ui, &ui.painter().with_clip_rect(editor_clip), caret, now - last);
    }
    // Autocomplete list and argument hint under the editor.
    if mine && ui.is_rect_visible(resp.rect) && (!ed.autocomplete.is_empty() || ed.current_function().is_some()) {
        let below = resp.rect.left_bottom() + vec2(0.0, 4.0);
        // Areas have their own clip rect: keep cell helpers inside the worksheet.
        let bounds = if ed.from_formula_bar { ui.ctx().content_rect() } else { ui.clip_rect() };
        egui::Area::new(id.with("ac")).fixed_pos(below).constrain_to(bounds).order(egui::Order::Tooltip).show(ui.ctx(), |ui| {
            egui::Frame::popup(ui.style()).inner_margin(4.0).show(ui, |ui| {
                if !ed.autocomplete.is_empty() {
                    let mut pick = None;
                    for (i, n) in ed.autocomplete.iter().enumerate() {
                        let sel = i == ed.ac_index;
                        let r = ui.add(
                            egui::Button::selectable(sel, egui::RichText::new(format!("ƒ  {n}")).font(theme::ui_font(12.5)))
                                .min_size(vec2(220.0, 20.0)),
                        );
                        if r.clicked() {
                            pick = Some(i);
                        }
                        if sel && let Some(desc) = crate::formula_locale::description(n, ed.locale) {
                            r.on_hover_text(desc);
                        }
                    }
                    if let Some(desc) = ed.autocomplete.get(ed.ac_index).and_then(|n| crate::formula_locale::description(n, ed.locale)) {
                        ui.separator();
                        ui.add(egui::Label::new(egui::RichText::new(desc).small()).wrap());
                    }
                    if let Some(i) = pick {
                        ed.ac_index = i;
                        ed.accept_autocomplete();
                        ed.request_focus = true;
                    }
                } else if let Some((name, arg)) = ed.current_function()
                    && let Some(sig) = crate::formula_locale::signature(&name, ed.locale)
                {
                    ui.horizontal_wrapped(|ui| {
                        ui.spacing_mut().item_spacing.x = 0.0;
                        let (head, rest) = sig.split_once('(').unwrap_or((&sig, ""));
                        ui.label(egui::RichText::new(format!("{}(", ed.locale.function_name(head))).font(theme::ui_font(12.0)));
                        let inner = rest.trim_end_matches(')');
                        let parts: Vec<&str> = inner.split(&format!("{} ", ed.locale.list_separator())).collect();
                        for (i, part) in parts.iter().enumerate() {
                            let is_cur = i == arg.min(parts.len().saturating_sub(1)) || (part.contains("...") && arg >= i);
                            let txt = egui::RichText::new(*part).font(if is_cur { theme::ui_bold(12.0) } else { theme::ui_font(12.0) });
                            ui.label(txt);
                            if i + 1 < parts.len() {
                                ui.label(egui::RichText::new(format!("{} ", ed.locale.list_separator())).font(theme::ui_font(12.0)));
                            }
                        }
                        ui.label(egui::RichText::new(")").font(theme::ui_font(12.0)));
                    });
                    // The description too, so values an argument takes (DATEDIF's units, #363) show
                    // while it's typed; wrapped to the hint's width.
                    if let Some(desc) = crate::formula_locale::description(&name, ed.locale) {
                        ui.set_max_width(ui.min_rect().width().max(220.0));
                        ui.separator();
                        ui.add(egui::Label::new(egui::RichText::new(desc).small()).wrap());
                    }
                }
            });
        });
    }
    app.editor = Some(ed);
    if cancel {
        app.cancel_edit();
    } else if let Some((dr, dc, array, fill)) = action {
        app.commit_edit(dr, dc, array, fill);
    }
}

fn cycle_anchor(ed: &mut crate::editor::EditState) {
    let caret_b = ed.text.char_indices().nth(ed.caret).map(|(i, _)| i).unwrap_or(ed.text.len());
    for (a, b, ..) in formula_refs(&ed.text) {
        if a <= caret_b && caret_b <= b {
            let Some(r) = ed.text.get(a..b) else { return };
            let cycled: String = r
                .split(':')
                .map(|part| {
                    let p = part.split('!').next_back().unwrap_or(part);
                    let prefix = &part[..part.len() - p.len()];
                    let bare = p.replace('$', "");
                    let letters: String = bare.chars().take_while(|c| c.is_ascii_alphabetic()).collect();
                    let digits = &bare[letters.len()..];
                    let state = (p.starts_with('$'), p[letters.len().min(p.len())..].contains('$') || p.trim_start_matches('$').contains('$'));
                    let next = match state {
                        (false, false) => format!("${letters}${digits}"),
                        (true, true) => format!("{letters}${digits}"),
                        (false, true) => format!("${letters}{digits}"),
                        (true, false) => format!("{letters}{digits}"),
                    };
                    format!("{prefix}{next}")
                })
                .collect::<Vec<_>>()
                .join(":");
            let (Some(x), Some(y)) = (ed.text.get(..a), ed.text.get(b..)) else { return };
            ed.text = format!("{x}{cycled}{y}");
            ed.caret = ed.text.get(..a + cycled.len()).map(|s| s.chars().count()).unwrap_or(ed.caret);
            ed.caret_to_end = false;
            ed.request_focus = true;
            ed.sync_caret = true;
            return;
        }
    }
}

fn function_names() -> Vec<String> {
    static NAMES: std::sync::OnceLock<Vec<String>> = std::sync::OnceLock::new();
    NAMES
        .get_or_init(|| gridcraft_engine::cmd::formulas::function_list().iter().filter_map(|f| f["name"].as_str().map(str::to_string)).collect())
        .clone()
}

fn function_info(name: &str) -> Option<serde_json::Value> {
    static LIST: std::sync::OnceLock<Vec<serde_json::Value>> = std::sync::OnceLock::new();
    LIST.get_or_init(gridcraft_engine::cmd::formulas::function_list)
        .iter()
        .find(|f| f["name"].as_str().is_some_and(|n| n.eq_ignore_ascii_case(name)))
        .cloned()
}

pub fn function_signature(name: &str) -> Option<String> {
    function_info(name).and_then(|f| f["signature"].as_str().map(str::to_string))
}

pub fn function_description(name: &str) -> Option<String> {
    function_info(name).and_then(|f| f["description"].as_str().map(str::to_string))
}
