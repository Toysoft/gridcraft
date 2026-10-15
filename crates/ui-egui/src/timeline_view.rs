//! Timelines on the sheet: painting, clicks (a period, Shift for a range, the clear button, the
//! time level menu, the scroll bar) and the Timeline contextual tab. The timeline itself and
//! its filtering live in the engine (`timeline.*` commands); this module draws it and acts
//! through those commands.

use egui::{Align2, Color32, Painter, Pos2, Rect, Stroke, StrokeKind, Ui, pos2, vec2};
use gridcraft_engine::cmd::timeline::{self as tlc, Period};
use gridcraft_engine::model::{Sheet, Timeline, TimelineLevel, Workbook};
use serde_json::json;

use crate::SheetApp;
use crate::chartview::anchor_rect;
use crate::grid::Geo;
use crate::theme;

/// The parts of a timeline on screen.
pub struct Layout {
    pub header: Option<Rect>,
    pub clear: Option<Rect>,
    pub label: Option<Rect>,
    pub level: Option<Rect>,
    /// The year (or month) captions above the tiles.
    pub groups: Vec<(Rect, String)>,
    /// One tile per visible period: (rect, period index).
    pub tiles: Vec<(Rect, usize)>,
    pub scrollbar: Option<Rect>,
    pub thumb: Option<Rect>,
}

/// Width of one period tile, in points, at each level.
fn tile_width(level: TimelineLevel) -> f32 {
    match level {
        TimelineLevel::Years => 56.0,
        TimelineLevel::Quarters => 40.0,
        TimelineLevel::Months => 34.0,
        TimelineLevel::Days => 24.0,
    }
}

/// The caption of a time level in the level menu.
pub fn level_caption(l: TimelineLevel) -> &'static str {
    match l {
        TimelineLevel::Years => "YEARS",
        TimelineLevel::Quarters => "QUARTERS",
        TimelineLevel::Months => "MONTHS",
        TimelineLevel::Days => "DAYS",
    }
}

pub fn layout(rect: Rect, z: f32, tl: &Timeline, periods: &[Period]) -> Layout {
    let pad = 6.0 * z;
    let mut y = rect.top();
    let (mut header, mut clear, mut label, mut level) = (None, None, None, None);
    if tl.show_header {
        let h = Rect::from_min_size(rect.min, vec2(rect.width(), 26.0 * z));
        let cb = 16.0 * z;
        clear = Some(Rect::from_min_size(pos2(h.right() - pad - cb, h.center().y - cb / 2.0), vec2(cb, cb)));
        header = Some(h);
        y = h.bottom();
    }
    if tl.show_selection_label || tl.show_time_level {
        let row = Rect::from_min_size(pos2(rect.left() + pad, y), vec2(rect.width() - 2.0 * pad, 20.0 * z));
        if tl.show_selection_label {
            label = Some(Rect::from_min_max(row.min, pos2(row.right() - 90.0 * z, row.bottom())));
        }
        if tl.show_time_level {
            level = Some(Rect::from_min_max(pos2(row.right() - 86.0 * z, row.top()), row.max));
        }
        y = row.bottom();
    }
    let bar_h = if tl.show_scrollbar { 10.0 * z } else { 0.0 };
    let strip = Rect::from_min_max(pos2(rect.left() + pad, y + 2.0 * z), pos2(rect.right() - pad, rect.bottom() - pad - bar_h));
    let tw = tile_width(tl.level) * z;
    let first = tl.scroll.and_then(|d| periods.iter().position(|p| p.end >= d)).unwrap_or(0);
    let fit = ((strip.width() / tw).floor() as usize).max(1);
    let first = first.min(periods.len().saturating_sub(fit));
    let group_h = 16.0 * z;
    let mut tiles = Vec::new();
    let mut groups: Vec<(Rect, String)> = Vec::new();
    for (k, (i, p)) in periods.iter().enumerate().skip(first).take(fit).enumerate() {
        let x = strip.left() + k as f32 * tw;
        let tile = Rect::from_min_size(pos2(x, strip.top() + group_h), vec2(tw - 2.0 * z, (strip.height() - group_h).max(8.0 * z)));
        tiles.push((tile, i));
        let start_group = groups.last().is_none_or(|(_, g)| *g != p.group);
        if !p.group.is_empty() && start_group {
            groups.push((Rect::from_min_size(pos2(x, strip.top()), vec2(tw, group_h)), p.group.clone()));
        } else if let Some((r, _)) = groups.last_mut().filter(|(_, g)| *g == p.group) {
            r.max.x = x + tw;
        }
    }
    let (mut scrollbar, mut thumb) = (None, None);
    if tl.show_scrollbar && !periods.is_empty() {
        let bar = Rect::from_min_size(pos2(strip.left(), rect.bottom() - pad - bar_h + 2.0 * z), vec2(strip.width(), bar_h - 4.0 * z));
        let n = periods.len() as f32;
        let a = bar.left() + bar.width() * first as f32 / n;
        let b = bar.left() + bar.width() * ((first + fit).min(periods.len()) as f32 / n);
        thumb = Some(Rect::from_min_max(pos2(a, bar.top()), pos2(b.max(a + 6.0 * z), bar.bottom())));
        scrollbar = Some(bar);
    }
    Layout { header, clear, label, level, groups, tiles, scrollbar, thumb }
}

/// The periods of a timeline, computed once per workbook state.
pub fn periods(ctx: &egui::Context, wb: &std::sync::Arc<Workbook>, tl: &Timeline) -> Vec<Period> {
    let key = egui::Id::new(("timeline_periods", tl.id, std::sync::Arc::as_ptr(wb) as usize, tl.level as u8));
    if let Some(v) = ctx.data(|d| d.get_temp::<std::sync::Arc<Vec<Period>>>(key)) {
        return (*v).clone();
    }
    let v = tlc::periods(wb, tl);
    ctx.data_mut(|d| d.insert_temp(key, std::sync::Arc::new(v.clone())));
    v
}

/// Colours of a timeline style: (selected tile, unselected tile, card, text).
fn style_colors(style: &str, t: &theme::Tokens) -> (Color32, Color32, Color32, Color32) {
    let n: usize = style.trim_start_matches(|c: char| !c.is_ascii_digit()).parse().unwrap_or(1);
    // Our own palette, one hue per style number.
    let hue = [
        Color32::from_rgb(0x3A, 0x7B, 0xD5),
        Color32::from_rgb(0xD9, 0x7B, 0x2B),
        Color32::from_rgb(0x7A, 0x7F, 0x87),
        Color32::from_rgb(0xC9, 0xA2, 0x27),
        Color32::from_rgb(0x4B, 0x9C, 0xC9),
        Color32::from_rgb(0x5A, 0x9E, 0x52),
    ];
    let accent = hue.get(n.saturating_sub(1) % hue.len()).copied().unwrap_or(t.accent);
    if style.contains("Dark") {
        (accent, Color32::from_gray(0x5A), Color32::from_gray(0x33), Color32::from_gray(0xF0))
    } else {
        (accent, Color32::from_gray(0xE4), t.window, t.text)
    }
}

pub fn paint(app: &SheetApp, p: &Painter, geo: &Geo, wb: &std::sync::Arc<Workbook>, sh: &Sheet) {
    let tk = theme::Tokens::get(p.ctx());
    for tl in &sh.timelines {
        let r = anchor_rect(geo, sh, &tl.anchor);
        if !r.intersects(p.clip_rect()) {
            continue;
        }
        let list = periods(p.ctx(), wb, tl);
        let lay = layout(r, geo.z, tl, &list);
        let (on, off, card, text) = style_colors(&tl.style, &tk);
        let radius = 4.0 * geo.z;
        p.rect_filled(r.translate(vec2(0.0, 1.5)), radius, Color32::from_black_alpha(18));
        p.rect_filled(r, radius, card);
        p.rect_stroke(r, radius, Stroke::new(1.0, tk.separator), StrokeKind::Inside);
        let clip = p.with_clip_rect(r.intersect(p.clip_rect()));
        if let Some(h) = lay.header {
            let cap = if tl.caption.is_empty() { tl.field.as_str() } else { tl.caption.as_str() };
            clip.text(h.left_center() + vec2(8.0 * geo.z, 0.0), Align2::LEFT_CENTER, cap, theme::ui_bold(12.5 * geo.z), text);
        }
        if let (Some(c), Some(icon)) = (lay.clear, crate::icons::from_name("filter")) {
            crate::icons::paint(&clip, c, icon, if tl.selection.is_some() { on } else { tk.text_dim });
        }
        if let Some(l) = lay.label {
            let label = tlc::selection_label(wb, tl);
            // "All Periods" is a fixed caption; a chosen period (`Mar - Aug 2026`) stays as the engine writes it.
            let label = app.ui.language.tr(&label);
            clip.text(l.left_center(), Align2::LEFT_CENTER, label, theme::ui_font(11.5 * geo.z), text);
        }
        if let Some(l) = lay.level {
            let level = app.ui.language.tr(level_caption(tl.level));
            clip.text(l.right_center(), Align2::RIGHT_CENTER, format!("{level} ▾"), theme::ui_font(10.5 * geo.z), on);
        }
        for (g, caption) in &lay.groups {
            clip.text(g.left_center() + vec2(2.0 * geo.z, 0.0), Align2::LEFT_CENTER, caption, theme::ui_font(10.5 * geo.z), text);
        }
        for (tile, i) in &lay.tiles {
            let Some(period) = list.get(*i) else { continue };
            let selected = tl.selection.is_some_and(|(a, b)| period.start <= b && period.end >= a);
            let fill = if selected || tl.selection.is_none() { on } else { off };
            let bar = Rect::from_min_max(pos2(tile.left(), tile.bottom() - 10.0 * geo.z), tile.max);
            clip.rect_filled(bar, 1.5 * geo.z, fill);
            clip.text(pos2(tile.center().x, bar.top() - 2.0 * geo.z), Align2::CENTER_BOTTOM, &period.label, theme::ui_font(10.0 * geo.z), text);
        }
        if let (Some(bar), Some(th)) = (lay.scrollbar, lay.thumb) {
            clip.rect_filled(bar, 2.0 * geo.z, tk.hover);
            clip.rect_filled(th, 2.0 * geo.z, tk.text_dim);
        }
        if app.selected_chart == Some(tl.id) {
            p.rect_stroke(r, 0.0, Stroke::new(1.0, Color32::from_gray(120)), StrokeKind::Outside);
        }
    }
}

/// What a click inside a timeline hit.
pub enum Action {
    /// A period: its first and last day.
    Period(f64, f64),
    Clear,
    /// The time level caption (opens the level menu).
    Level(Pos2),
    /// The scroll bar: scroll to this date.
    Scroll(f64),
}

/// The timeline under `p` and what the click hit there (`None`: the timeline's background).
pub fn click(geo: &Geo, sh: &Sheet, wb: &std::sync::Arc<Workbook>, ctx: &egui::Context, p: Pos2) -> Option<(u32, Option<Action>)> {
    if !geo.cells.contains(p) {
        return None;
    }
    for tl in sh.timelines.iter().rev() {
        let r = anchor_rect(geo, sh, &tl.anchor);
        if !r.contains(p) {
            continue;
        }
        let list = periods(ctx, wb, tl);
        let lay = layout(r, geo.z, tl, &list);
        if lay.clear.is_some_and(|c| c.contains(p)) {
            return Some((tl.id, Some(Action::Clear)));
        }
        if let Some(l) = lay.level.filter(|l| l.contains(p)) {
            return Some((tl.id, Some(Action::Level(l.left_bottom()))));
        }
        for (tile, i) in &lay.tiles {
            if tile.expand2(vec2(1.0, 0.0)).contains(p)
                && let Some(x) = list.get(*i)
            {
                return Some((tl.id, Some(Action::Period(x.start, x.end))));
            }
        }
        if let Some(bar) = lay.scrollbar.filter(|b| b.expand(3.0).contains(p)) {
            let f = ((p.x - bar.left()) / bar.width().max(1.0)).clamp(0.0, 1.0);
            let i = ((list.len() as f32) * f) as usize;
            if let Some(x) = list.get(i.min(list.len().saturating_sub(1))) {
                return Some((tl.id, Some(Action::Scroll(x.start))));
            }
        }
        return Some((tl.id, None));
    }
    None
}

/// Runs a click on a timeline: a period (with Shift: from the selection's start to it), the
/// clear button, the level menu or the scroll bar.
pub fn on_click(app: &mut SheetApp, id: u32, action: Action, shift: bool) {
    match action {
        Action::Period(a, b) => {
            let start = if shift {
                app.session
                    .active()
                    .and_then(|d| d.wb.sheets.iter().flat_map(|s| s.timelines.iter()).find(|t| t.id == id).and_then(|t| t.selection))
                    .map(|s| s.0)
            } else {
                None
            };
            let (lo, hi) = match start {
                Some(s) if s < a => (s, b),
                Some(s) => (a, s.max(b)),
                None => (a, b),
            };
            app.run_or_alert("timeline.select", json!({"timeline": id, "start": lo, "end": hi}));
        }
        Action::Clear => app.run_or_alert("timeline.clear", json!({"timeline": id})),
        Action::Level(at) => app.grid.timeline_menu = Some((id, at)),
        Action::Scroll(to) => app.run_or_alert("timeline.scroll", json!({"timeline": id, "to": to})),
    }
}

/// The time level menu of a timeline, opened from its level caption.
pub fn level_menu(app: &mut SheetApp, ui: &mut Ui) {
    let Some((id, at)) = app.grid.timeline_menu else { return };
    let lang = crate::i18n::current(ui.ctx());
    let mut close = false;
    let resp = egui::Area::new(egui::Id::new("timeline_level")).fixed_pos(at).order(egui::Order::Foreground).show(ui.ctx(), |ui| {
        egui::Frame::popup(ui.style()).show(ui, |ui| {
            for (level, name) in [
                (TimelineLevel::Years, "years"),
                (TimelineLevel::Quarters, "quarters"),
                (TimelineLevel::Months, "months"),
                (TimelineLevel::Days, "days"),
            ] {
                if ui.button(lang.tr(level_caption(level))).clicked() {
                    app.run_or_alert("timeline.level", json!({"timeline": id, "level": name}));
                    close = true;
                }
            }
        });
    });
    if close || resp.response.clicked_elsewhere() {
        app.grid.timeline_menu = None;
    }
}

/// The selected timeline on the active sheet, if any.
pub fn selected(app: &SheetApp) -> Option<Timeline> {
    let id = app.selected_chart?;
    app.session.active()?.wb.active()?.timelines.iter().find(|t| t.id == id).cloned()
}

/// The Timeline contextual tab: Timeline (caption, Report Connections), Timeline Styles, Arrange,
/// Size and Show, in Excel's order.
pub fn ribbon_tab(app: &mut SheetApp, ui: &mut Ui) {
    use crate::icons::Icon;
    use crate::widgets::small_button;
    let Some(tl) = selected(app) else { return };
    let id = tl.id;
    let lang = app.ui.language;
    ui.vertical(|ui| {
        ui.label(egui::RichText::new(lang.tr("Timeline Caption:")).small());
        let field = egui::Id::new(("gridcraft.timeline_caption", id));
        if let Some(caption) = crate::widgets::committed_text(ui, field, &tl.caption, Some(130.0))
            && caption != tl.caption
        {
            app.run_or_alert("timeline.caption", json!({"timeline": id, "caption": caption}));
        }
        if small_button(ui, Icon::PivotTable, "Report Connections", "Report Connections", false).clicked() {
            app.run_or_alert("timeline.reportConnections", json!({"timeline": id}));
        }
    });
    crate::ribbon::sep(ui);
    // Timeline Styles: the gallery of Light and Dark styles.
    egui::Grid::new("timeline_styles").spacing(vec2(3.0, 3.0)).show(ui, |ui| {
        for (i, name) in tlc::STYLES.iter().enumerate() {
            let tk = theme::Tokens::get(ui.ctx());
            let (on, off, card, _) = style_colors(name, &tk);
            let (r, resp) = ui.allocate_exact_size(vec2(30.0, 22.0), egui::Sense::click());
            ui.painter().rect_filled(r, 2.0, card);
            ui.painter().rect_filled(Rect::from_min_size(r.min + vec2(3.0, 12.0), vec2(12.0, 6.0)), 1.0, on);
            ui.painter().rect_filled(Rect::from_min_size(r.min + vec2(16.0, 12.0), vec2(11.0, 6.0)), 1.0, off);
            let border = if tl.style == *name { tk.accent } else { tk.separator };
            ui.painter().rect_stroke(r, 2.0, Stroke::new(1.0, border), StrokeKind::Inside);
            if resp.on_hover_text(*name).clicked() {
                app.run_or_alert("timeline.style", json!({"timeline": id, "style": name}));
            }
            if i % 6 == 5 {
                ui.end_row();
            }
        }
    });
    crate::ribbon::sep(ui);
    // Arrange: Excel's object arrangement, not available for timelines yet.
    ui.add_enabled_ui(false, |ui| {
        ui.vertical(|ui| {
            for label in ["Bring Forward", "Send Backward", "Selection Pane"] {
                small_button(ui, Icon::Shapes, label, "Not available for timelines yet", false);
            }
        });
        ui.vertical(|ui| {
            for label in ["Align", "Group", "Rotate"] {
                small_button(ui, Icon::Shapes, label, "Not available for timelines yet", true);
            }
        });
    });
    crate::ribbon::sep(ui);
    ui.vertical(|ui| {
        for (label, key, v) in [("Height:", "height", tl.anchor.height), ("Width:", "width", tl.anchor.width)] {
            ui.horizontal(|ui| {
                ui.label(lang.tr(label));
                let mut cm = f64::from(v) / 72.0 * 2.54;
                let resp = ui.add(egui::DragValue::new(&mut cm).speed(0.05).range(0.5..=100.0).suffix(" cm").max_decimals(2));
                if resp.drag_stopped() || (resp.changed() && !resp.dragged()) {
                    app.run_or_alert("timeline.size", json!({"timeline": id, key: cm / 2.54 * 72.0}));
                }
            });
        }
    });
    crate::ribbon::sep(ui);
    ui.vertical(|ui| {
        for (label, cmd, v) in
            [("Header", "timeline.showHeader", tl.show_header), ("Selection Label", "timeline.showSelectionLabel", tl.show_selection_label)]
        {
            let mut on = v;
            if ui.checkbox(&mut on, lang.tr(label)).changed() {
                app.run_or_alert(cmd, json!({"timeline": id, "on": on}));
            }
        }
    });
    ui.vertical(|ui| {
        for (label, cmd, v) in
            [("Scrollbar", "timeline.showScrollbar", tl.show_scrollbar), ("Time Level", "timeline.showTimeLevel", tl.show_time_level)]
        {
            let mut on = v;
            if ui.checkbox(&mut on, lang.tr(label)).changed() {
                app.run_or_alert(cmd, json!({"timeline": id, "on": on}));
            }
        }
    });
}
