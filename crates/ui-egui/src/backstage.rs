//! The File page: what the File button opens. It fills the window, as the File view of Excel does
//! (Excel calls its own the Backstage view).
//!
//! A column on the left turns between the pages (Home, New, Open, Info, Export) and holds the
//! actions that leave for the workbook again (Save, Save As…, Close, About). Home shows what a
//! new workbook can start from and the workbooks opened before.
//!
//! To the rest of the app the page is a dialog named `backstage`, so that what holds for dialogs
//! holds for it: the grid and the ribbon's keys stand down while it is open, `ui.dialog
//! {name: "backstage", page?}` opens it, `ui.dialog.set {field: "page", value}` turns the page,
//! `ui.dialog.cancel` leaves it and `ui.inspect` reports it. It is drawn here, though, over the
//! whole window rather than in a window of its own.
//!
//! The page has no commands of its own: every button runs a file command (`file.new`,
//! `file.open`, `file.save`…), and the buttons carry the labels the File menu had before it.
//!
//! The drawings on the tiles are made here, in code, like the rest of GridCraft's artwork.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, PoisonError};

use egui::{
    Align, Align2, Color32, FontId, Key, Layout, Rect, Response, Sense, Stroke, StrokeKind, Ui, UiBuilder, WidgetInfo, WidgetType, pos2, vec2,
};
use serde_json::{Value as Json, json};

use crate::SheetApp;
use crate::dialogs::Dialog;
use crate::icons::{self, Icon};
use crate::theme::{self, Tokens};

/// The page's name as a dialog, as `ui.dialog {name}` takes it.
pub const NAME: &str = "backstage";

const NAV_WIDTH: f32 = 172.0;
const NAV_ROW: f32 = 38.0;
/// Room left at the top: on macOS the window's buttons sit over the content there.
const TOP: f32 = if cfg!(target_os = "macos") { 38.0 } else { 10.0 };
/// The strip at the top that moves the window when dragged, as the title bar does.
const DRAG_STRIP: f32 = 38.0;
/// The widest the pages' content gets, so that lines stay readable in a wide window.
const CONTENT_WIDTH: f32 = 1040.0;
const TILE: egui::Vec2 = vec2(136.0, 100.0);
const RECENT_ROW: f32 = 50.0;
const MODIFIED_COLUMN: f32 = 190.0;
/// The most workbooks the list keeps, as `SheetApp::open_path` does.
const MAX_RECENT: usize = 20;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Page {
    Home,
    New,
    Open,
    Info,
    Export,
}

impl Page {
    const ALL: [Page; 5] = [Page::Home, Page::New, Page::Open, Page::Info, Page::Export];

    fn code(self) -> &'static str {
        match self {
            Page::Home => "home",
            Page::New => "new",
            Page::Open => "open",
            Page::Info => "info",
            Page::Export => "export",
        }
    }

    fn label(self) -> &'static str {
        match self {
            Page::Home => "Home",
            Page::New => "New",
            Page::Open => "Open",
            Page::Info => "Info",
            Page::Export => "Export",
        }
    }

    /// The page of this code; anything else is Home.
    fn parse(code: &str) -> Page {
        Page::ALL.into_iter().find(|p| p.code() == code).unwrap_or(Page::Home)
    }

    fn of(values: &serde_json::Map<String, Json>) -> Page {
        Page::parse(values.get("page").and_then(Json::as_str).unwrap_or(""))
    }
}

/// What a click asks for. It is carried out after the page is drawn.
enum Act {
    /// Back to the workbook.
    Leave,
    Turn(Page),
    /// A command, run the way a menu runs it; the page leaves.
    Run(&'static str, Json),
    /// Another dialog takes the page's place.
    Dialog(&'static str, Json),
    OpenRecent(String),
    /// Takes a workbook off the list of recent ones.
    Forget(String),
}

/// The values the page starts from, or `None` when it can't open: what is being typed into a cell
/// is entered first (it belongs to the workbook the page is about), and an entry the cell refuses
/// keeps the user there, with the reason.
pub fn values(app: &mut SheetApp, params: &Json) -> Option<Json> {
    if !app.commit_edit(0, 0, false, false) {
        return None;
    }
    remember_open_workbooks(app);
    let page = Page::parse(params.get("page").and_then(Json::as_str).unwrap_or(""));
    Some(json!({"page": page.code()}))
}

/// A workbook is listed as recent when it is opened from a file. One that was saved under a name
/// since then belongs on the list as well.
fn remember_open_workbooks(app: &mut SheetApp) {
    let paths: Vec<String> = app.session.documents().iter().filter_map(|d| d.path.clone()).collect();
    for path in paths {
        if !app.ui.recent.contains(&path) {
            app.ui.recent.insert(0, path);
        }
    }
    app.ui.recent.truncate(MAX_RECENT);
}

/// Draws the page when it is the open dialog (called by `dialogs::show`). Returns whether it was.
pub fn show(app: &mut SheetApp, ctx: &egui::Context) -> bool {
    if app.dialog.as_ref().is_none_or(|d| d.name != NAME) {
        return false;
    }
    let Some(mut d) = app.dialog.take() else { return false };
    let page = Page::of(&d.values);
    let facts = facts(ctx, &app.ui.recent);
    let mut act = None;
    let screen = ctx.content_rect();
    egui::Area::new(egui::Id::new("gridcraft.backstage")).order(egui::Order::Middle).fixed_pos(screen.min).constrain(false).fade_in(false).show(
        ctx,
        |ui| {
            let t = Tokens::get(ui.ctx());
            // The page covers the window and takes the pointer from everything under it.
            let (rect, background) = ui.allocate_exact_size(screen.size(), Sense::click_and_drag());
            if background.drag_started() && ui.input(|i| i.pointer.press_origin()).is_some_and(|at| at.y < rect.top() + DRAG_STRIP) {
                ui.ctx().send_viewport_cmd(egui::ViewportCommand::StartDrag);
            }
            ui.painter().rect_filled(rect, 0.0, t.ribbon);
            let nav_rect = Rect::from_min_size(rect.min, vec2(NAV_WIDTH.min(rect.width()), rect.height()));
            ui.painter().rect_filled(nav_rect, 0.0, t.window);
            ui.painter().line_segment([nav_rect.right_top(), nav_rect.right_bottom()], Stroke::new(1.0, t.separator));
            ui.scope_builder(UiBuilder::new().max_rect(nav_rect), |ui| nav(app, ui, page, &mut act));
            let body = Rect::from_min_max(pos2(nav_rect.right() + 1.0, rect.top()), rect.max);
            if body.width() > 80.0 {
                ui.scope_builder(UiBuilder::new().max_rect(body), |ui| {
                    egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
                        let margin = egui::Margin { left: 44, right: 36, top: TOP as i8 + 18, bottom: 28 };
                        egui::Frame::NONE.inner_margin(margin).show(ui, |ui| {
                            ui.set_width(ui.available_width().min(CONTENT_WIDTH));
                            match page {
                                Page::Home => home(app, ui, &facts, &mut act),
                                Page::New => new(app, ui, &mut act),
                                Page::Open => open(app, ui, &facts, &mut act),
                                Page::Info => info(app, ui, &mut d),
                                Page::Export => export(app, ui, &mut act),
                            }
                        });
                    });
                });
            }
        },
    );
    // Escape goes back to the workbook, unless a message is up: then it answers the message.
    if act.is_none() && app.message.is_none() && ctx.input(|i| i.key_pressed(Key::Escape)) {
        act = Some(Act::Leave);
    }
    let leave = match act {
        None => false,
        Some(Act::Leave) => true,
        Some(Act::Turn(to)) => {
            d.values.insert("page".into(), json!(to.code()));
            false
        }
        Some(Act::Forget(path)) => {
            app.ui.recent.retain(|p| *p != path);
            false
        }
        Some(Act::Run(id, params)) => {
            // File operations must include the cell the user is still editing.
            if app.commit_edit(0, 0, false, false) {
                app.run_or_alert(id, params);
            }
            true
        }
        Some(Act::Dialog(name, params)) => {
            app.open_dialog(name, params);
            true
        }
        Some(Act::OpenRecent(path)) => open_recent(app, &path),
    };
    if !leave && app.dialog.is_none() {
        app.dialog = Some(d);
    }
    true
}

/// Opens a workbook from the list. Returns whether it opened: one that didn't (it was moved, or
/// its drive is away) leaves the page up, under the message that says why.
fn open_recent(app: &mut SheetApp, path: &str) -> bool {
    if !app.commit_edit(0, 0, false, false) {
        return true;
    }
    let before = app.session.active().map(|d| d.uid);
    app.open_path(path);
    // A workbook can open and still have something to say (what an import left out).
    app.message.is_none() || app.session.active().map(|d| d.uid) != before
}

// ───────────────────────────── The column on the left ─────────────────────────────

fn nav(app: &SheetApp, ui: &mut Ui, page: Page, act: &mut Option<Act>) {
    let lang = app.ui.language;
    let has_document = app.session.active().is_some();
    ui.spacing_mut().item_spacing = vec2(0.0, 0.0);
    ui.add_space(TOP);
    if back(ui).clicked() {
        *act = Some(Act::Leave);
    }
    ui.add_space(8.0);
    for (to, icon) in [(Page::Home, Icon::Home), (Page::New, Icon::Paper), (Page::Open, Icon::Folder)] {
        if nav_item(ui, lang.tr(to.label()), Some(icon), page == to, true).clicked() {
            *act = Some(Act::Turn(to));
        }
    }
    rule(ui);
    if nav_item(ui, lang.tr(Page::Info.label()), None, page == Page::Info, has_document).clicked() {
        *act = Some(Act::Turn(Page::Info));
    }
    for (label, id) in [("Save", "file.save"), ("Save As…", "file.saveAs")] {
        if nav_item(ui, lang.tr(label), None, false, has_document).clicked() {
            *act = Some(Act::Run(id, json!({})));
        }
    }
    if nav_item(ui, lang.tr(Page::Export.label()), None, page == Page::Export, has_document).clicked() {
        *act = Some(Act::Turn(Page::Export));
    }
    if nav_item(ui, lang.tr("Close"), None, false, has_document).clicked() {
        *act = Some(Act::Run("file.close", json!({})));
    }
    // What isn't about one workbook sits at the foot of the column.
    ui.with_layout(Layout::bottom_up(Align::Min), |ui| {
        ui.add_space(10.0);
        if nav_item(ui, "About", None, false, true).clicked() {
            *act = Some(Act::Dialog("about", json!({})));
        }
        rule(ui);
    });
}

/// The round button at the head of the column that goes back to the workbook.
fn back(ui: &mut Ui) -> Response {
    let t = Tokens::get(ui.ctx());
    let (row, _) = ui.allocate_exact_size(vec2(ui.available_width(), 36.0), Sense::hover());
    let circle = Rect::from_center_size(pos2(row.left() + 30.0, row.center().y), vec2(30.0, 30.0));
    let resp = ui.interact(circle, ui.id().with("back"), Sense::click());
    resp.widget_info(|| WidgetInfo::labeled(WidgetType::Button, true, "Back"));
    let p = ui.painter();
    if resp.is_pointer_button_down_on() {
        p.circle_filled(circle.center(), 15.0, t.pressed);
    } else if resp.hovered() || resp.has_focus() {
        p.circle_filled(circle.center(), 15.0, t.hover);
    }
    p.circle_stroke(circle.center(), 14.5, Stroke::new(1.2, t.text_dim));
    icons::paint(p, Rect::from_center_size(circle.center() - vec2(1.0, 0.0), vec2(16.0, 16.0)), Icon::Left, t.text);
    p.line_segment([circle.center() - vec2(5.0, 0.0), circle.center() + vec2(6.0, 0.0)], Stroke::new(1.4, t.text));
    resp.on_hover_text("Back to the workbook (Esc)")
}

/// One line of the column: a page (the one showing is marked) or an action.
fn nav_item(ui: &mut Ui, label: &str, icon: Option<Icon>, selected: bool, enabled: bool) -> Response {
    let t = Tokens::get(ui.ctx());
    let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), NAV_ROW), if enabled { Sense::click() } else { Sense::hover() });
    resp.widget_info(|| WidgetInfo::selected(WidgetType::Button, enabled, selected, label));
    let p = ui.painter();
    let pill = rect.shrink2(vec2(6.0, 2.0));
    if selected {
        p.rect_filled(pill, 4.0, t.ribbon);
        p.rect_filled(Rect::from_min_max(pos2(pill.left(), pill.top() + 7.0), pos2(pill.left() + 3.0, pill.bottom() - 7.0)), 1.5, t.accent);
    } else if enabled && resp.is_pointer_button_down_on() {
        p.rect_filled(pill, 4.0, t.pressed);
    } else if enabled && (resp.hovered() || resp.has_focus()) {
        p.rect_filled(pill, 4.0, t.hover);
    }
    let color = if enabled { t.text } else { t.text_disabled };
    if let Some(icon) = icon {
        icons::paint(p, Rect::from_center_size(pos2(rect.left() + 30.0, rect.center().y), vec2(16.0, 16.0)), icon, color);
    }
    let font = if selected { theme::ui_bold(13.5) } else { theme::ui_font(13.5) };
    p.text(pos2(rect.left() + 50.0, rect.center().y), Align2::LEFT_CENTER, label, font, color);
    resp
}

fn rule(ui: &mut Ui) {
    let t = Tokens::get(ui.ctx());
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), 15.0), Sense::hover());
    ui.painter().line_segment([pos2(rect.left() + 18.0, rect.center().y), pos2(rect.right() - 18.0, rect.center().y)], Stroke::new(1.0, t.separator));
}

// ───────────────────────────── The pages ─────────────────────────────

fn heading(ui: &mut Ui, text: &str) {
    let t = Tokens::get(ui.ctx());
    ui.add(egui::Label::new(egui::RichText::new(text).font(theme::ui_font(24.0)).color(t.text)).truncate());
    ui.add_space(16.0);
}

fn section(ui: &mut Ui, text: &str) {
    let t = Tokens::get(ui.ctx());
    ui.label(egui::RichText::new(text).font(theme::ui_bold(15.0)).color(t.text));
    ui.add_space(8.0);
}

fn dim(ui: &Ui, text: impl Into<String>) -> egui::RichText {
    egui::RichText::new(text).font(theme::ui_font(12.5)).color(Tokens::get(ui.ctx()).text_dim)
}

fn home(app: &SheetApp, ui: &mut Ui, facts: &Facts, act: &mut Option<Act>) {
    heading(ui, "GridCraft");
    section(ui, app.ui.language.tr("New"));
    tiles(app, ui, act);
    ui.add_space(22.0);
    recent(app, ui, facts, true, act);
}

fn new(app: &SheetApp, ui: &mut Ui, act: &mut Option<Act>) {
    heading(ui, app.ui.language.tr("New"));
    tiles(app, ui, act);
}

fn open(app: &SheetApp, ui: &mut Ui, facts: &Facts, act: &mut Option<Act>) {
    let lang = app.ui.language;
    heading(ui, lang.tr("Open"));
    if choice(ui, Icon::Folder, lang.tr("Open…"), "Look for a workbook on this computer").clicked() {
        *act = Some(Act::Run("file.open", json!({})));
    }
    ui.add_space(22.0);
    recent(app, ui, facts, false, act);
}

/// What a new workbook can start from: an empty one, and the sample workbooks.
fn tiles(app: &SheetApp, ui: &mut Ui, act: &mut Option<Act>) {
    let lang = app.ui.language;
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing = vec2(10.0, 10.0);
        if tile(ui, lang.tr("New Workbook"), None).clicked() {
            *act = Some(Act::Run("file.new", json!({})));
        }
        for (sample, title) in gridcraft_engine::sample::SAMPLES {
            if tile(ui, title, Some(sample)).clicked() {
                *act = Some(Act::Run("file.new", json!({"sample": sample})));
            }
        }
    });
}

/// A drawing of a workbook over its name.
fn tile(ui: &mut Ui, caption: &str, sample: Option<&str>) -> Response {
    let t = Tokens::get(ui.ctx());
    let (rect, resp) = ui.allocate_exact_size(vec2(TILE.x + 16.0, TILE.y + 48.0), Sense::click());
    resp.widget_info(|| WidgetInfo::labeled(WidgetType::Button, true, caption));
    let card = Rect::from_min_size(rect.min + vec2(8.0, 6.0), TILE);
    let p = ui.painter();
    // A sheet is white in the dark theme as well, as it is in the grid.
    p.rect_filled(card, 3.0, Color32::WHITE);
    sketch(p, card, sample);
    let edge = if resp.hovered() || resp.has_focus() { Stroke::new(2.0, t.accent) } else { Stroke::new(1.0, t.input_border) };
    p.rect_stroke(card, 3.0, edge, StrokeKind::Outside);
    let mut job = egui::text::LayoutJob::simple(caption.to_string(), theme::ui_font(12.5), t.text, TILE.x + 12.0);
    job.halign = Align::Center;
    job.wrap.max_rows = 2;
    p.galley(pos2(rect.center().x, card.bottom() + 8.0), p.layout_job(job), t.text);
    resp
}

/// Draws a small picture of a workbook into `card`: an empty sheet, or a hint of what a sample
/// holds. Positions are fractions of the card.
fn sketch(p: &egui::Painter, card: Rect, sample: Option<&str>) {
    let at = |x: f32, y: f32| pos2(card.left() + x * card.width(), card.top() + y * card.height());
    let bar = |x: f32, y: f32, w: f32, h: f32, c: Color32| p.rect_filled(Rect::from_min_max(at(x, y), at(x + w, y + h)), 1.0, c);
    let line = |a: (f32, f32), b: (f32, f32), c: Color32| p.line_segment([at(a.0, a.1), at(b.0, b.1)], Stroke::new(1.0, c));
    let faint = Color32::from_gray(0xE2);
    let grey = Color32::from_gray(0xC4);
    match sample {
        None => {
            // Headers along the top and the left, gridlines, and the first cell selected.
            bar(0.0, 0.0, 1.0, 0.14, Color32::from_gray(0xF1));
            bar(0.0, 0.0, 0.1, 1.0, Color32::from_gray(0xF1));
            for x in [0.1, 0.4, 0.7] {
                line((x, 0.0), (x, 1.0), faint);
            }
            for row in 1..7 {
                let y = 0.14 * row as f32;
                line((0.0, y), (1.0, y), faint);
            }
            p.rect_stroke(Rect::from_min_max(at(0.1, 0.14), at(0.4, 0.28)), 0.0, Stroke::new(1.5, icons::GREEN), StrokeKind::Inside);
        }
        Some("budget") => {
            // A title, lines of labels with their amounts, and a total.
            bar(0.08, 0.1, 0.46, 0.09, icons::GREEN);
            for row in 0..4 {
                let y = 0.3 + 0.125 * row as f32;
                bar(0.08, y, 0.28 + 0.05 * (row % 2) as f32, 0.05, grey);
                bar(0.7, y, 0.22, 0.05, faint);
            }
            line((0.08, 0.81), (0.92, 0.81), grey);
            bar(0.08, 0.85, 0.2, 0.06, Color32::from_gray(0x8A));
            bar(0.66, 0.85, 0.26, 0.06, icons::GREEN);
        }
        Some("sales") => {
            // A column chart beside a small table.
            line((0.08, 0.86), (0.56, 0.86), grey);
            for (i, (height, colour)) in
                [(0.34, icons::BLUE), (0.52, icons::GREEN), (0.42, icons::ORANGE), (0.66, icons::PURPLE)].into_iter().enumerate()
            {
                bar(0.11 + 0.115 * i as f32, 0.86 - height, 0.08, height, colour);
            }
            bar(0.64, 0.14, 0.28, 0.08, icons::BLUE);
            for row in 0..5 {
                bar(0.64, 0.3 + 0.115 * row as f32, 0.28, 0.05, faint);
            }
        }
        Some("grades") => {
            // A table: a header band, then a name and marks on each line.
            bar(0.08, 0.1, 0.84, 0.12, icons::BLUE);
            for row in 0..5 {
                let y = 0.29 + 0.125 * row as f32;
                if row % 2 == 1 {
                    bar(0.08, y - 0.03, 0.84, 0.11, Color32::from_gray(0xF4));
                }
                bar(0.11, y, 0.3, 0.05, grey);
                for mark in 0..3 {
                    bar(0.52 + 0.14 * mark as f32, y, 0.08, 0.05, faint);
                }
            }
        }
        // A sample this page has no drawing for yet: lines of text.
        Some(_) => {
            bar(0.08, 0.1, 0.5, 0.09, icons::GREEN);
            for row in 0..5 {
                bar(0.08, 0.3 + 0.125 * row as f32, 0.84 - 0.12 * (row % 3) as f32, 0.05, faint);
            }
        }
    }
}

/// The workbooks opened before, newest first: click one to open it.
fn recent(app: &SheetApp, ui: &mut Ui, facts: &Facts, open_button: bool, act: &mut Option<Act>) {
    let t = Tokens::get(ui.ctx());
    let lang = app.ui.language;
    ui.horizontal(|ui| {
        ui.label(egui::RichText::new(lang.tr("Recent")).font(theme::ui_bold(15.0)).color(t.text));
        if open_button {
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if ui.button(lang.tr("Open…")).on_hover_text("Look for a workbook on this computer").clicked() {
                    *act = Some(Act::Run("file.open", json!({})));
                }
            });
        }
    });
    ui.add_space(6.0);
    if app.ui.recent.is_empty() {
        ui.label(dim(ui, "The workbooks you open are listed here."));
        return;
    }
    let width = ui.available_width();
    // Without room for it the second column goes, and the names keep theirs.
    let modified_column = if width > 460.0 { MODIFIED_COLUMN } else { 0.0 };
    let (head, _) = ui.allocate_exact_size(vec2(width, 24.0), Sense::hover());
    let p = ui.painter().clone();
    p.text(pos2(head.left() + 44.0, head.center().y), Align2::LEFT_CENTER, lang.tr("Name"), theme::ui_font(12.0), t.text_dim);
    if modified_column > 0.0 {
        p.text(pos2(head.right() - modified_column, head.center().y), Align2::LEFT_CENTER, "Modified", theme::ui_font(12.0), t.text_dim);
    }
    p.line_segment([head.left_bottom(), head.right_bottom()], Stroke::new(1.0, t.separator));
    let now = crate::now_ms() / 1000.0;
    ui.spacing_mut().item_spacing.y = 0.0;
    for path in &app.ui.recent {
        let (rect, resp) = ui.allocate_exact_size(vec2(width, RECENT_ROW), Sense::click());
        resp.widget_info(|| WidgetInfo::labeled(WidgetType::Button, true, path));
        let inside = ui.rect_contains_pointer(rect);
        if inside || resp.has_focus() {
            p.rect_filled(rect.shrink2(vec2(0.0, 1.0)), 4.0, t.hover);
        }
        icons::paint(&p, Rect::from_center_size(pos2(rect.left() + 20.0, rect.center().y), vec2(22.0, 22.0)), Icon::Book, t.accent);
        let (name, folder) = name_and_folder(path);
        let text_width = (width - 44.0 - modified_column - 44.0).max(40.0);
        p.galley(pos2(rect.left() + 44.0, rect.top() + 8.0), one_line(&p, &name, theme::ui_font(13.5), t.text, text_width), t.text);
        p.galley(pos2(rect.left() + 44.0, rect.top() + 28.0), one_line(&p, &folder, theme::ui_font(11.5), t.text_dim, text_width), t.text_dim);
        if modified_column > 0.0 {
            let (text, colour) = match facts.get(path) {
                Some(Fact::Modified(at)) => (age(now - at), t.text_dim),
                Some(Fact::Missing) => ("Not available".to_string(), t.text_disabled),
                None => (String::new(), t.text_dim),
            };
            p.text(pos2(rect.right() - modified_column, rect.center().y), Align2::LEFT_CENTER, text, theme::ui_font(12.5), colour);
        }
        p.line_segment([rect.left_bottom(), rect.right_bottom()], Stroke::new(1.0, t.separator));
        // The row under the pointer offers to drop its workbook from the list.
        let mut forgotten = false;
        if inside {
            let spot = Rect::from_center_size(pos2(rect.right() - 20.0, rect.center().y), vec2(24.0, 24.0));
            let forget = ui.interact(spot, resp.id.with("forget"), Sense::click());
            forget.widget_info(|| WidgetInfo::labeled(WidgetType::Button, true, "Remove from the list"));
            if forget.hovered() {
                p.rect_filled(spot, 4.0, t.pressed);
            }
            icons::paint(&p, Rect::from_center_size(spot.center(), vec2(12.0, 12.0)), Icon::Close, t.text_dim);
            forgotten = forget.on_hover_text("Remove from the list").clicked();
        }
        if forgotten {
            *act = Some(Act::Forget(path.clone()));
        } else if resp.on_hover_text(path).clicked() {
            *act = Some(Act::OpenRecent(path.clone()));
        }
    }
}

/// A path as the list shows it: the file's name, and the folder it is in.
fn name_and_folder(path: &str) -> (String, String) {
    let whole = std::path::Path::new(path);
    let name = whole.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| path.to_string());
    let folder = whole.parent().map(|f| f.to_string_lossy().into_owned()).unwrap_or_default();
    (name, folder)
}

/// Text on one line, cut short with an ellipsis where it would pass `width`.
fn one_line(p: &egui::Painter, text: &str, font: FontId, color: Color32, width: f32) -> Arc<egui::Galley> {
    let mut job = egui::text::LayoutJob::simple(text.to_string(), font, color, width);
    job.wrap.max_rows = 1;
    job.wrap.break_anywhere = true;
    p.layout_job(job)
}

/// How long ago, in words. The clock's time zone isn't known here, so the list tells ages and not
/// dates, which could be a day out.
fn age(seconds: f64) -> String {
    let s = if seconds.is_finite() { seconds.max(0.0) } else { 0.0 };
    let count = |n: f64, unit: &str| {
        let n = n.floor() as u64;
        format!("{n} {unit}{} ago", if n == 1 { "" } else { "s" })
    };
    const DAY: f64 = 86_400.0;
    if s < 60.0 {
        "Just now".into()
    } else if s < 3600.0 {
        count(s / 60.0, "minute")
    } else if s < DAY {
        count(s / 3600.0, "hour")
    } else if s < 2.0 * DAY {
        "Yesterday".into()
    } else if s < 14.0 * DAY {
        count(s / DAY, "day")
    } else if s < 61.0 * DAY {
        count(s / (7.0 * DAY), "week")
    } else if s < 730.0 * DAY {
        count(s / (30.44 * DAY), "month")
    } else {
        count(s / (365.25 * DAY), "year")
    }
}

/// A wide button: a title over a line that says what it does.
fn choice(ui: &mut Ui, icon: Icon, title: &str, detail: &str) -> Response {
    let t = Tokens::get(ui.ctx());
    let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width().min(480.0), 56.0), Sense::click());
    resp.widget_info(|| WidgetInfo::labeled(WidgetType::Button, true, title));
    let p = ui.painter();
    let fill = if resp.is_pointer_button_down_on() {
        t.pressed
    } else if resp.hovered() || resp.has_focus() {
        t.hover
    } else {
        Color32::TRANSPARENT
    };
    p.rect_filled(rect, 6.0, fill);
    p.rect_stroke(rect, 6.0, Stroke::new(1.0, t.separator), StrokeKind::Inside);
    icons::paint(p, Rect::from_center_size(pos2(rect.left() + 28.0, rect.center().y), vec2(24.0, 24.0)), icon, t.text);
    let text_width = (rect.width() - 68.0).max(40.0);
    p.galley(pos2(rect.left() + 56.0, rect.top() + 10.0), one_line(p, title, theme::ui_bold(13.5), t.text, text_width), t.text);
    p.galley(pos2(rect.left() + 56.0, rect.top() + 31.0), one_line(p, detail, theme::ui_font(11.5), t.text_dim, text_width), t.text_dim);
    resp
}

/// The document properties `file.properties` keeps, with their labels.
const PROPERTIES: [(&str, &str); 6] = [
    ("title", "Title"),
    ("subject", "Subject"),
    ("author", "Author"),
    ("company", "Company"),
    ("keywords", "Keywords"),
    ("description", "Description"),
];

/// About the workbook: where it is, and the properties saved with it. A property is changed by
/// typing a new one; it is set (through `file.properties`, one undo step) when the field is left.
fn info(app: &mut SheetApp, ui: &mut Ui, d: &mut Dialog) {
    let Some(doc) = app.session.active() else { return };
    let (title, path, dirty, sheets, protected) =
        (doc.display_title(), doc.path.clone(), doc.is_dirty(), doc.wb.sheets.len(), doc.wb.protected_structure);
    let props = serde_json::to_value(&doc.wb.props).unwrap_or_default();
    heading(ui, &title);
    match &path {
        Some(path) => ui.add(egui::Label::new(dim(ui, path)).truncate()),
        None => ui.label(dim(ui, "This workbook hasn't been saved to a file yet.")),
    };
    if dirty {
        ui.label(dim(ui, "It has changes that aren't saved."));
    }
    ui.add_space(20.0);
    section(ui, "Properties");
    let mut edits: Vec<(&'static str, String)> = Vec::new();
    let escaping = ui.input(|i| i.key_pressed(Key::Escape));
    egui::Grid::new("gridcraft.backstage.properties").num_columns(2).spacing(vec2(16.0, 8.0)).show(ui, |ui| {
        for (key, label) in PROPERTIES {
            let name = ui.label(label);
            let current = props.get(key).and_then(Json::as_str).unwrap_or("");
            // What is being typed is kept with the page until the field is left.
            let draft = format!("property.{key}");
            let mut text = d.values.get(&draft).and_then(Json::as_str).unwrap_or(current).to_string();
            // A grid cell would squeeze a text field to nothing: it is given its size.
            let size = vec2(340.0, ui.spacing().interact_size.y);
            let field = ui.add_sized(size, egui::TextEdit::singleline(&mut text)).labelled_by(name.id);
            if field.changed() {
                d.values.insert(draft.clone(), json!(text));
            }
            if field.lost_focus() {
                d.values.remove(&draft);
                if text != current && !escaping {
                    edits.push((key, text));
                }
            }
            ui.end_row();
        }
    });
    ui.add_space(20.0);
    section(ui, "Workbook");
    egui::Grid::new("gridcraft.backstage.facts").num_columns(2).spacing(vec2(16.0, 6.0)).show(ui, |ui| {
        ui.label("Sheets");
        ui.label(sheets.to_string());
        ui.end_row();
        ui.label("Structure");
        ui.label(if protected { "Protected" } else { "Not protected" });
        ui.end_row();
    });
    for (key, value) in edits {
        app.run_or_alert("file.properties", json!({key: value}));
    }
}

/// The other kinds of file a workbook can be written as. Each hands over to the dialog the Share
/// menu opens, so the two can't come to differ.
fn export(app: &SheetApp, ui: &mut Ui, act: &mut Option<Act>) {
    heading(ui, app.ui.language.tr("Export"));
    ui.spacing_mut().item_spacing.y = 10.0;
    if choice(ui, Icon::TextColumns, "CSV (.csv)", "The active sheet as comma-separated text, for other programs to read").clicked() {
        *act = Some(Act::Dialog("saveCopy", json!({"format": "csv"})));
    }
    if choice(ui, Icon::Link, "Web Page (.html)", "A page that a web browser can show").clicked() {
        *act = Some(Act::Dialog("saveCopy", json!({"format": "html"})));
    }
    // A PDF can be handed over only where the host delivers files itself (the web build): the
    // desktop app has no way yet to ask where a PDF should go.
    if app.services.download.is_some() && choice(ui, Icon::Paper, "PDF (.pdf)", "Pages laid out by Page Setup, for printing and sharing").clicked() {
        *act = Some(Act::Run("file.exportPdf", json!({})));
    }
}

// ───────────────────────────── When the files were last changed ─────────────────────────────

/// What is known about a file on the list.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Fact {
    /// When it was last changed, in seconds since 1970.
    Modified(f64),
    /// It can't be reached: moved, deleted, or on a drive that is away.
    Missing,
}

type Facts = HashMap<String, Fact>;

/// The page's knowledge of the listed files, kept between frames. Asking a file system can take
/// long (a network drive that is asleep), so a thread asks and the page shows what has come back.
#[derive(Clone, Default)]
struct Lookup {
    /// The last frame the page was drawn in: after a gap it has been closed, and asks again.
    seen: u64,
    /// The list the answers are for.
    asked: Vec<String>,
    found: Arc<Mutex<Facts>>,
    /// A thread is still asking. There is one at a time, so a drive that never answers holds up
    /// one thread and not one for every time the page is opened.
    busy: Arc<AtomicBool>,
}

/// What is known this frame about the files in `recent`; starts asking when the list is new.
fn facts(ctx: &egui::Context, recent: &[String]) -> Facts {
    let id = egui::Id::new("gridcraft.backstage.files");
    let mut lookup: Lookup = ctx.data(|d| d.get_temp(id)).unwrap_or_default();
    let frame = ctx.cumulative_frame_nr();
    if frame > lookup.seen.saturating_add(1) {
        // A thread of the last visit that is still out reports to the map this one leaves behind.
        lookup.asked.clear();
        lookup.found = Arc::default();
    }
    lookup.seen = frame;
    if lookup.asked != recent && !lookup.busy.load(Ordering::SeqCst) {
        lookup.asked = recent.to_vec();
        ask(ctx, &lookup);
    }
    let known = lookup.found.lock().unwrap_or_else(PoisonError::into_inner).clone();
    ctx.data_mut(|d| d.insert_temp(id, lookup));
    known
}

#[cfg(not(target_arch = "wasm32"))]
fn ask(ctx: &egui::Context, lookup: &Lookup) {
    let (ctx, paths, found, busy) = (ctx.clone(), lookup.asked.clone(), lookup.found.clone(), lookup.busy.clone());
    lookup.busy.store(true, Ordering::SeqCst);
    let asking = std::thread::Builder::new().name("gridcraft-file-page".into()).spawn(move || {
        for path in paths {
            let modified = std::fs::metadata(&path).and_then(|m| m.modified()).ok().and_then(|at| at.duration_since(std::time::UNIX_EPOCH).ok());
            let fact = modified.map_or(Fact::Missing, |since| Fact::Modified(since.as_secs_f64()));
            found.lock().unwrap_or_else(PoisonError::into_inner).insert(path, fact);
            ctx.request_repaint();
        }
        busy.store(false, Ordering::SeqCst);
        ctx.request_repaint();
    });
    if asking.is_err() {
        // No thread to ask with: the list goes without its second column.
        lookup.busy.store(false, Ordering::SeqCst);
    }
}

/// The web build has no file system to ask.
#[cfg(target_arch = "wasm32")]
fn ask(_ctx: &egui::Context, _lookup: &Lookup) {}

#[cfg(test)]
mod tests {
    use super::*;

    /// A folder of its own for a test's files, removed afterwards.
    struct Temp(std::path::PathBuf);

    impl Temp {
        fn new(name: &str) -> Temp {
            let dir = std::env::temp_dir().join(format!("gridcraft-file-page-{name}-{}", std::process::id()));
            let _ = std::fs::remove_dir_all(&dir);
            std::fs::create_dir_all(&dir).unwrap();
            Temp(dir)
        }

        fn path(&self, file: &str) -> String {
            self.0.join(file).to_string_lossy().into_owned()
        }
    }

    impl Drop for Temp {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn the_list_learns_when_its_files_were_changed() {
        let dir = Temp::new("facts");
        let (there, gone) = (dir.path("There.xlsx"), dir.path("Gone.xlsx"));
        std::fs::write(&there, b"not a workbook, but a file").unwrap();
        let ctx = egui::Context::default();
        let recent = vec![there.clone(), gone.clone()];
        assert!(facts(&ctx, &[]).is_empty());
        let mut known = Facts::new();
        for _ in 0..1_000 {
            known = facts(&ctx, &recent);
            if known.len() == recent.len() {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        assert!(matches!(known.get(&there), Some(Fact::Modified(at)) if *at > 1.0e9), "{known:?}");
        assert_eq!(known.get(&gone), Some(&Fact::Missing));
    }

    #[test]
    fn ages_are_told_in_words() {
        const DAY: f64 = 86_400.0;
        for (seconds, words) in [
            (0.0, "Just now"),
            (59.0, "Just now"),
            (60.0, "1 minute ago"),
            (150.0, "2 minutes ago"),
            (3600.0, "1 hour ago"),
            (23.9 * 3600.0, "23 hours ago"),
            (DAY, "Yesterday"),
            (2.0 * DAY, "2 days ago"),
            (13.9 * DAY, "13 days ago"),
            (14.0 * DAY, "2 weeks ago"),
            (60.0 * DAY, "8 weeks ago"),
            (61.0 * DAY, "2 months ago"),
            (400.0 * DAY, "13 months ago"),
            (800.0 * DAY, "2 years ago"),
            // A clock that is behind the file's, or numbers that aren't numbers.
            (-5.0, "Just now"),
            (f64::NAN, "Just now"),
            (f64::NEG_INFINITY, "Just now"),
            (f64::INFINITY, "Just now"),
        ] {
            assert_eq!(age(seconds), words, "{seconds}");
        }
        assert!(age(f64::MAX).ends_with("years ago"));
    }

    #[test]
    fn a_path_is_shown_as_a_name_and_its_folder() {
        let whole = std::path::Path::new("Bank Details").join("Investments.xlsx").to_string_lossy().into_owned();
        assert_eq!(name_and_folder(&whole), ("Investments.xlsx".to_string(), "Bank Details".to_string()));
        assert_eq!(name_and_folder("Book.xlsx"), ("Book.xlsx".to_string(), String::new()));
        assert_eq!(name_and_folder(""), (String::new(), String::new()));
        assert_eq!(Page::parse("EXPORT"), Page::Home, "codes are exact");
        for page in Page::ALL {
            assert_eq!(Page::parse(page.code()), page);
        }
    }
}
