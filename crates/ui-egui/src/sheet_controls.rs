//! Controls placed on worksheets (ActiveX combo boxes and the like), drawn over the cells at their
//! anchors. Picking a value writes the linked cell and runs the control's `Change` handler.

use egui::{Color32, FontId, Rect, Stroke, StrokeKind};
use gridcraft_engine::model::Sheet;
use gridcraft_engine::vba as evba;

use crate::grid::Geo;
use crate::{SheetApp, theme};

pub fn show(app: &mut SheetApp, ui: &mut egui::Ui, geo: &Geo, sh: &Sheet) {
    if sh.controls.is_empty() {
        return;
    }
    let Some(uid) = app.session.active().map(|d| d.uid) else { return };
    let mut picked: Option<(String, String)> = None;
    for c in &sh.controls {
        let r = crate::chartview::anchor_rect(geo, sh, &c.anchor);
        if !r.intersects(geo.cells) || r.width() < 4.0 || r.height() < 4.0 {
            continue;
        }
        let size = c.font_size.unwrap_or(8.25) * 96.0 / 72.0 * geo.z;
        let font = FontId::new(size.clamp(6.0, 72.0), theme::cell_family(c.font.as_deref().unwrap_or(""), c.bold, false));
        let fore = c.fore.map_or(Color32::BLACK, |v| Color32::from_rgb((v >> 16) as u8, (v >> 8) as u8, v as u8));
        let value = evba::sheet_control_value(&app.session, uid, &sh.name, &c.name);
        let mut child = ui.new_child(egui::UiBuilder::new().max_rect(r).layout(egui::Layout::left_to_right(egui::Align::Center)));
        child.set_clip_rect(r.intersect(geo.cells));
        match c.kind.as_str() {
            "ComboBox" | "ListBox" => {
                let list = evba::sheet_control_list(&app.session, uid, &sh.name, &c.name);
                child.painter().rect_filled(r, 0.0, Color32::WHITE);
                child.painter().rect_stroke(r, 0.0, Stroke::new(1.0, Color32::from_gray(0x7A)), StrokeKind::Inside);
                child.spacing_mut().interact_size.y = r.height();
                egui::ComboBox::from_id_salt(("sheet_control", sh.name.as_str(), c.name.as_str()))
                    .width(r.width() - 6.0)
                    .height(400.0)
                    .selected_text(egui::RichText::new(&value).font(font.clone()).color(fore))
                    .show_ui(&mut child, |ui| {
                        egui::ScrollArea::vertical().max_height(360.0).show(ui, |ui| {
                            for item in &list {
                                if ui.selectable_label(*item == value, egui::RichText::new(item).font(font.clone())).clicked() {
                                    picked = Some((c.name.clone(), item.clone()));
                                }
                            }
                        });
                    });
            }
            _ => {
                // Other controls show their caption or value in a frame.
                child.painter().rect_stroke(r, 0.0, Stroke::new(1.0, Color32::from_gray(0x9A)), StrokeKind::Inside);
                let text = if c.caption.is_empty() { &value } else { &c.caption };
                child.painter().text(
                    Rect::from_min_max(r.min, r.max).left_center() + egui::vec2(4.0, 0.0),
                    egui::Align2::LEFT_CENTER,
                    text,
                    font,
                    fore,
                );
            }
        }
    }
    if let Some((name, value)) = picked {
        match evba::sheet_control_set_value(&mut app.session, uid, &sh.name, &name, value) {
            Ok(()) => {
                app.after_engine();
                crate::macros::flush_events(app, uid);
            }
            Err(e) => app.message = Some(("GridCraft".into(), e.to_string())),
        }
    }
}
