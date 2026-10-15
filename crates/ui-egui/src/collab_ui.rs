//! Live sessions (co-editing, plugin protocol 2) in the interface: the engine's pump each frame,
//! the room and the people in the status bar, others' selections drawn in their colours with
//! name tags, others' edits briefly highlighted, and the room's state kept for recovery.
//!
//! The engine does the work (`gridcraft_engine::collab`); this only shows it.

use egui::{Align2, Color32, Painter, Rect, Stroke, StrokeKind, Ui, pos2, vec2};
use gridcraft_engine::core::RangeRef;
use gridcraft_engine::model::Sheet;
use serde_json::json;

use crate::SheetApp;
use crate::grid::Geo;
use crate::theme::{self, Tokens};

/// How long others' edits stay highlighted (milliseconds).
const HIGHLIGHT_MS: f64 = 2_000.0;
/// How often the rooms' state is written for recovery.
const RECOVERY_MS: f64 = 30_000.0;

fn color(hex: &str) -> Color32 {
    let v = u32::from_str_radix(hex.trim_start_matches('#'), 16).unwrap_or(0x80_80_80);
    Color32::from_rgb((v >> 16) as u8, (v >> 8) as u8, v as u8)
}

/// Runs the engine's co-editing between frames.
pub fn pump(app: &mut SheetApp, ctx: &egui::Context) {
    if app.session.collab.rooms.is_empty() && !gridcraft_engine::plugin::push::pending() {
        return;
    }
    let now = crate::now_ms();
    if app.session.collab_poll(now.max(0.0) as u64) {
        ctx.request_repaint();
    }
    for (doc, sheet, cell) in app.session.collab.take_remote_edits() {
        app.collab_highlights.push((doc, sheet, cell, now));
    }
    app.collab_highlights.retain(|h| now - h.3 < HIGHLIGHT_MS);
    if app.collab_highlights.len() > 2_000 {
        let extra = app.collab_highlights.len() - 2_000;
        app.collab_highlights.drain(..extra);
    }
    for n in app.session.collab_take_notices() {
        app.status_message = Some((n, now));
    }
    if now - app.collab_recovery_at > RECOVERY_MS {
        app.collab_recovery_at = now;
        app.session.save_collab_recovery();
    }
    app.after_engine();
    if !app.session.collab.rooms.is_empty() {
        // Remote messages arrive between frames: keep looking.
        ctx.request_repaint_after(std::time::Duration::from_millis(50));
    }
}

/// The room and the people in it, for the status bar.
pub fn status(app: &mut SheetApp, ui: &mut Ui) {
    let t = Tokens::get(ui.ctx());
    let Some(room) = app.session.active_room() else { return };
    let state = if let Some(why) = gridcraft_engine::collab::read_only(&app.session) {
        why
    } else {
        match room.state {
            gridcraft_engine::collab::RoomState::Online => "Live".to_string(),
            gridcraft_engine::collab::RoomState::Connecting => "Connecting…".to_string(),
            gridcraft_engine::collab::RoomState::Offline => "Offline (editing goes on here)".to_string(),
            gridcraft_engine::collab::RoomState::Ended => "Session ended".to_string(),
        }
    };
    let message = room.message.clone().or_else(|| room.refused.clone());
    let peers = room.peers();
    ui.add_space(12.0);
    let dot = if room.encrypted() { "Encrypted · " } else { "" };
    ui.label(egui::RichText::new(format!("{dot}{state}")).font(theme::ui_font(12.5)).color(t.text_dim))
        .on_hover_text(format!("{} through {}", room.room, room.plugin));
    for p in peers.iter().take(8) {
        let c = color(&p.user.color);
        let initial: String = p.user.name.chars().next().map(|c| c.to_uppercase().collect()).unwrap_or_default();
        let (rect, resp) = ui.allocate_exact_size(vec2(18.0, 18.0), egui::Sense::hover());
        ui.painter().circle_filled(rect.center(), 8.0, c);
        ui.painter().text(rect.center(), Align2::CENTER_CENTER, initial, theme::ui_font(10.5), Color32::WHITE);
        resp.on_hover_text(&p.user.name);
    }
    if peers.len() > 8 {
        ui.label(egui::RichText::new(format!("+{}", peers.len() - 8)).font(theme::ui_font(12.5)).color(t.text_dim));
    }
    if let Some(m) = message {
        ui.add_space(8.0);
        ui.label(egui::RichText::new(m).font(theme::ui_font(12.5)).color(t.danger));
    }
}

/// Others' selections (outlined in their colours, with their names) and their recent edits.
pub fn paint_remote(p: &Painter, geo: &Geo, sh: &Sheet, app: &SheetApp, rows: (u32, u32), cols: (u32, u32)) {
    let Some(d) = app.session.active() else { return };
    let si = d.wb.active_sheet;
    let visible = |r: &RangeRef| r.start.row <= rows.1 && r.end.row >= rows.0 && r.start.col <= cols.1 && r.end.col >= cols.0;
    let now = crate::now_ms();
    for (doc, sheet, cell, at) in &app.collab_highlights {
        let r = RangeRef::cell(*cell);
        if *doc != d.uid || *sheet != si || !visible(&r) {
            continue;
        }
        let fade = (1.0 - (now - at) / HIGHLIGHT_MS).clamp(0.0, 1.0);
        let rect = geo.cell_rect(sh, *cell);
        p.rect_filled(rect, 0.0, Color32::from_rgba_unmultiplied(255, 196, 0, (90.0 * fade) as u8));
    }
    let Some(room) = app.session.active_room() else { return };
    for peer in room.peers() {
        let c = color(&peer.user.color);
        if let Some((s, ranges)) = &peer.selection
            && *s == si
        {
            for r in ranges.iter().filter(|r| visible(r)) {
                let rect = geo.range_rect(sh, *r);
                p.rect_filled(rect, 0.0, c.gamma_multiply(0.12));
                p.rect_stroke(rect, 0.0, Stroke::new(2.0, c), StrokeKind::Inside);
            }
        }
        let corner = match (&peer.cursor, &peer.selection) {
            (Some((s, cell)), _) if *s == si => Some(*cell),
            (_, Some((s, ranges))) if *s == si => ranges.first().map(|r| r.start),
            _ => None,
        };
        if let Some(cell) = corner.filter(|c| visible(&RangeRef::cell(*c))) {
            let rect = geo.cell_rect(sh, cell);
            p.rect_stroke(rect, 0.0, Stroke::new(2.0, c), StrokeKind::Inside);
            let font = theme::ui_font(10.5);
            let galley = p.layout_no_wrap(peer.user.name.clone(), font, Color32::WHITE);
            let tag = Rect::from_min_size(pos2(rect.left(), rect.top() - galley.size().y - 2.0), galley.size() + vec2(6.0, 2.0));
            p.rect_filled(tag, 2.0, c);
            p.galley(tag.min + vec2(3.0, 1.0), galley, Color32::WHITE);
        }
    }
}

/// The Share dialog's defaults: the transports, this person, a room name.
pub fn share_defaults(app: &mut SheetApp, mode: &str) -> serde_json::Value {
    let status = app.session.run("collab.status", json!({})).unwrap_or_default();
    let transports: Vec<String> = status["transports"]
        .as_array()
        .map(|a| a.iter().map(|t| format!("{} ({})", t["id"].as_str().unwrap_or(""), t["label"].as_str().unwrap_or(""))).collect())
        .unwrap_or_default();
    let first = status["transports"].get(0).and_then(|t| t["id"].as_str()).unwrap_or("").to_string();
    let user = &status["user"];
    json!({
        "mode": mode,
        "plugin": first,
        "room": "",
        "name": user["name"],
        "color": user["color"],
        "userId": user["id"],
        "encrypt": false,
        "key": "",
        "transports": if transports.is_empty() { "No co-editing transport plugin is loaded (File › Options › Add-ins).".to_string() } else { format!("Transports: {}", transports.join(", ")) },
    })
}
