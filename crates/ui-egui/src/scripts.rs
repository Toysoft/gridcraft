//! Scripts in the interface (Automate tab): the Code Editor task pane, which Excel opens on the
//! right, with the All Scripts list; running a script from the ribbon gallery, the Code Editor or
//! View › Macros; and the message bar that asks before a file's own scripts run.
//!
//! The pane is thin: it reads the workbook's scripts and the last run's output from the engine
//! and acts through `automate.*` commands.

use egui::text::{CCursor, CCursorRange, LayoutJob};
use egui::{Color32, FontId, TextFormat, Ui, vec2};
use serde_json::{Value as Json, json};

use crate::icons::Icon;
use crate::theme::{self, Tokens};
use crate::{Notice, NoticeKind, SheetApp};

/// The pane name in [`crate::grid::GridState::pane`].
pub const PANE: &str = "codeEditor";
/// The dialog name the message bar's Enable Content button opens (handled, not a dialog).
pub const ENABLE: &str = "__enableScripts";

/// The Code Editor's state: the script being edited (none: the All Scripts list).
#[derive(Clone, Debug, Default)]
pub struct CodeEditor {
    pub name: Option<String>,
    pub code: String,
    /// The code as last loaded or saved (unsaved changes differ).
    pub saved: String,
    /// A script waiting for Enable Content before it runs.
    pub pending: Option<String>,
    /// The workbook (uid) the editor's script belongs to.
    pub doc: u64,
}

impl CodeEditor {
    pub fn dirty(&self) -> bool {
        self.name.is_some() && self.code != self.saved
    }
}

/// Opens the Code Editor on `name` (or on the All Scripts list).
pub fn open(app: &mut SheetApp, name: Option<&str>) {
    app.grid.pane = Some(PANE.into());
    let doc = app.session.active().map(|d| d.uid).unwrap_or(0);
    match name {
        Some(name) => match app.session.run("automate.getScript", json!({"name": name})) {
            Ok(r) => {
                let code = r.get("code").and_then(Json::as_str).unwrap_or_default().to_string();
                let name = r.get("name").and_then(Json::as_str).unwrap_or(name).to_string();
                // A converted command list counts as changed: Save stores it as Rhai.
                let saved = if r.get("kind").and_then(Json::as_str) == Some("commands") { String::new() } else { code.clone() };
                app.scripts = CodeEditor { name: Some(name), code, saved, pending: app.scripts.pending.take(), doc };
            }
            Err(e) => app.message = Some(("Code Editor".into(), crate::clean_error(&e))),
        },
        None => {
            app.scripts.name = None;
            app.scripts.doc = doc;
        }
    }
}

/// Runs a saved script, first asking to enable content when the workbook came from a file with
/// scripts. Errors go to the Code Editor's output when it shows that script, otherwise to a
/// message box.
pub fn run(app: &mut SheetApp, name: &str) {
    if app.session.active().is_some_and(|d| d.scripts_blocked) {
        app.scripts.pending = Some(name.to_string());
        let lang = app.ui.language;
        app.notice = Some(Notice {
            text: lang
                .tr("Scripts in this workbook are turned off because it came from a file. Enable them only if you trust where it came from.")
                .into(),
            kind: NoticeKind::Warning,
            action: Some((lang.tr("Enable Content").into(), ENABLE.into())),
        });
        return;
    }
    let shown = app.grid.pane.as_deref() == Some(PANE) && app.scripts.name.as_deref().is_some_and(|n| n.eq_ignore_ascii_case(name));
    match app.run("automate.runScript", json!({"name": name})) {
        Ok(_) => app.toast = Some((format!("{} \"{name}\"", app.ui.language.tr("Script ran:")), crate::now_ms())),
        Err(e) if !shown => app.message = Some(("Run Script".into(), crate::clean_error(&e))),
        Err(_) => {}
    }
}

/// The message bar's Enable Content: lets this workbook's scripts run, then runs the one that
/// was waiting.
pub fn enable_content(app: &mut SheetApp) {
    app.notice = None;
    if let Err(e) = app.run("automate.enableScripts", json!({})) {
        app.message = Some(("GridCraft".into(), crate::clean_error(&e)));
        return;
    }
    if let Some(name) = app.scripts.pending.take() {
        run(app, &name);
    }
}

/// Saves the editor's code; `false` (with a message) when saving failed.
fn save(app: &mut SheetApp) -> bool {
    let Some(name) = app.scripts.name.clone() else { return true };
    match app.run("automate.saveScript", json!({"name": name, "code": app.scripts.code})) {
        Ok(_) => {
            app.scripts.saved = app.scripts.code.clone();
            true
        }
        Err(e) => {
            app.message = Some(("Code Editor".into(), crate::clean_error(&e)));
            false
        }
    }
}

/// The Code Editor pane's content (the pane frame and title come from [`crate::panes`]).
pub fn pane(app: &mut SheetApp, ui: &mut Ui) {
    // Another workbook became active: show its scripts.
    let doc = app.session.active().map(|d| d.uid).unwrap_or(0);
    if app.scripts.doc != doc {
        app.scripts = CodeEditor { doc, ..CodeEditor::default() };
    }
    // The script was deleted or renamed elsewhere (undo, a command): back to the list.
    if let Some(name) = app.scripts.name.clone()
        && !app.session.active().is_some_and(|d| d.wb.script(&name).is_some())
        && !app.scripts.dirty()
    {
        app.scripts.name = None;
    }
    match app.scripts.name.clone() {
        Some(name) => editor(app, ui, &name),
        None => list(app, ui),
    }
}

/// All Scripts: the workbook's scripts, each with Run, Edit and Delete.
fn list(app: &mut SheetApp, ui: &mut Ui) {
    let lang = app.ui.language;
    let t = Tokens::get(ui.ctx());
    if ui.button(format!("+ {}", lang.tr("New Script"))).clicked() {
        app.run_or_alert("automate.newScript", json!({}));
    }
    ui.add_space(6.0);
    ui.label(egui::RichText::new(lang.tr("All Scripts")).font(theme::ui_bold(13.0)));
    let scripts: Vec<(String, String)> = app
        .session
        .active()
        .map(|d| {
            d.wb.scripts
                .iter()
                .map(|s| {
                    let what = match s.language {
                        gridcraft_engine::model::ScriptLanguage::Rhai => {
                            let n = s.code.lines().count();
                            format!("{n} {}", lang.tr(if n == 1 { "line" } else { "lines" }))
                        }
                        gridcraft_engine::model::ScriptLanguage::Commands => lang.tr("Recorded commands").to_string(),
                    };
                    (s.name.clone(), what)
                })
                .collect()
        })
        .unwrap_or_default();
    if scripts.is_empty() {
        ui.label(egui::RichText::new(lang.tr("This workbook has no scripts yet. Record Actions or New Script creates one.")).color(t.text_dim));
    }
    let mut action: Option<(&str, String)> = None;
    egui::ScrollArea::vertical().id_salt("script_list").show(ui, |ui| {
        for (name, what) in &scripts {
            ui.horizontal(|ui| {
                if crate::widgets::icon_button(ui, Icon::Run, t.text, "Run", vec2(22.0, 22.0)).clicked() {
                    action = Some(("run", name.clone()));
                }
                if ui.link(egui::RichText::new(name).font(theme::ui_font(13.0))).on_hover_text(lang.tr("Edit")).clicked() {
                    action = Some(("edit", name.clone()));
                }
                ui.label(egui::RichText::new(what).small().color(t.text_dim));
            });
        }
    });
    match action {
        Some(("run", name)) => run(app, &name),
        Some((_, name)) => open(app, Some(&name)),
        None => {}
    }
}

/// The editor for one script: name, Run, Save, the code and the output.
fn editor(app: &mut SheetApp, ui: &mut Ui, name: &str) {
    let lang = app.ui.language;
    let t = Tokens::get(ui.ctx());
    ui.horizontal(|ui| {
        if ui.link(format!("‹ {}", lang.tr("All Scripts"))).clicked() {
            if app.scripts.dirty() && !save(app) {
                return;
            }
            app.scripts.name = None;
        }
    });
    if app.scripts.name.is_none() {
        return;
    }
    // The script's name, renamed in place (Enter or leaving the field).
    let field = egui::Id::new(("gridcraft.script_name", app.scripts.doc, name));
    if let Some(new) = crate::widgets::committed_text(ui, field, name, Some(ui.available_width()))
        && new.trim() != name
        && !new.trim().is_empty()
    {
        match app.run("automate.renameScript", json!({"name": name, "newName": new.trim()})) {
            Ok(_) => app.scripts.name = Some(new.trim().to_string()),
            Err(e) => app.message = Some(("Code Editor".into(), crate::clean_error(&e))),
        }
    }
    let name = app.scripts.name.clone().unwrap_or_default();
    ui.horizontal(|ui| {
        let run_clicked = ui.button(format!("▶ {}", lang.tr("Run"))).on_hover_text(lang.tr("Save and run this script")).clicked();
        let save_clicked = ui.add_enabled(app.scripts.dirty(), egui::Button::new(lang.tr("Save"))).clicked();
        if run_clicked && (!app.scripts.dirty() || save(app)) {
            run(app, &name);
        }
        if save_clicked {
            save(app);
        }
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui.button(lang.tr("Delete")).clicked() && app.run("automate.deleteScript", json!({"name": name})).is_ok() {
                app.scripts.name = None;
            }
        });
    });
    if app.scripts.name.is_none() {
        return;
    }
    ui.add_space(4.0);
    // Code above, output below; the output takes what it needs up to a third of the pane.
    let output = app.session.last_script.clone().filter(|r| r.name.eq_ignore_ascii_case(&name));
    let out_h = (ui.available_height() / 3.0).clamp(90.0, 260.0);
    let code_h = (ui.available_height() - out_h - 28.0).max(120.0);
    let code_id = egui::Id::new(("gridcraft.script_code", app.scripts.doc, &name));
    let font = FontId::new(12.5, egui::FontFamily::Name(theme::MONO.into()));
    let dark = t.dark;
    egui::ScrollArea::both().id_salt("script_code_scroll").max_height(code_h).show(ui, |ui| {
        let mut layouter = |ui: &Ui, text: &dyn egui::TextBuffer, wrap: f32| {
            let mut job = highlight(text.as_str(), &font, dark);
            job.wrap.max_width = wrap;
            ui.fonts_mut(|f| f.layout_job(job))
        };
        ui.add(
            egui::TextEdit::multiline(&mut app.scripts.code)
                .id(code_id)
                .code_editor()
                .desired_width(f32::INFINITY)
                .desired_rows(16)
                .lock_focus(true)
                .layouter(&mut layouter),
        );
    });
    ui.separator();
    ui.label(egui::RichText::new(lang.tr("Output")).font(theme::ui_bold(13.0)));
    egui::ScrollArea::vertical().id_salt("script_output").max_height(out_h).stick_to_bottom(true).show(ui, |ui| {
        let Some(run) = output else {
            ui.label(egui::RichText::new(lang.tr("Run the script to see what it prints here.")).color(t.text_dim));
            return;
        };
        for line in &run.output {
            ui.label(egui::RichText::new(line).font(FontId::new(12.0, egui::FontFamily::Name(theme::MONO.into()))));
        }
        match &run.error {
            Some(e) => {
                let text = egui::RichText::new(e.describe()).color(t.danger);
                // Clicking the error puts the cursor on its line.
                if let Some(line) = e.line {
                    if ui.link(text).clicked() {
                        goto_line(ui, code_id, &app.scripts.code, line);
                    }
                } else {
                    ui.label(text);
                }
            }
            None => {
                ui.label(egui::RichText::new(lang.tr("The script ran without errors.")).color(t.text_dim));
            }
        }
    });
}

/// Puts the code editor's cursor at the start of 1-based `line`.
fn goto_line(ui: &Ui, id: egui::Id, code: &str, line: usize) {
    let at: usize = code.split_inclusive('\n').take(line.saturating_sub(1)).map(|l| l.chars().count()).sum();
    if let Some(mut state) = egui::TextEdit::load_state(ui.ctx(), id) {
        state.cursor.set_char_range(Some(CCursorRange::one(CCursor::new(at))));
        state.store(ui.ctx(), id);
    }
    ui.ctx().memory_mut(|m| m.request_focus(id));
}

// ---------------------------------------------------------------- syntax colouring

const KEYWORDS: &[&str] = &[
    "fn", "let", "const", "if", "else", "while", "loop", "for", "in", "do", "until", "break", "continue", "return", "throw", "try", "catch",
    "switch", "true", "false", "this", "private", "global",
];

/// Colours for keywords, strings, numbers, comments and object-model calls (Rhai's syntax is close
/// to Rust's; this is a lexical approximation, enough to read code by).
fn highlight(code: &str, font: &FontId, dark: bool) -> LayoutJob {
    let (plain, keyword, string, number, comment, call) = if dark {
        (
            Color32::from_gray(220),
            Color32::from_rgb(0x56, 0x9C, 0xD6),
            Color32::from_rgb(0xCE, 0x91, 0x78),
            Color32::from_rgb(0xB5, 0xCE, 0xA8),
            Color32::from_rgb(0x6A, 0x99, 0x55),
            Color32::from_rgb(0xDC, 0xDC, 0xAA),
        )
    } else {
        (
            Color32::from_gray(30),
            Color32::from_rgb(0x00, 0x33, 0xB3),
            Color32::from_rgb(0xA3, 0x15, 0x15),
            Color32::from_rgb(0x09, 0x86, 0x58),
            Color32::from_rgb(0x00, 0x80, 0x00),
            Color32::from_rgb(0x79, 0x5E, 0x26),
        )
    };
    let mut job = LayoutJob::default();
    let fmt = |c: Color32| TextFormat { font_id: font.clone(), color: c, ..Default::default() };
    let chars: Vec<(usize, char)> = code.char_indices().collect();
    let slice = |a: usize, b: usize| {
        let start = chars.get(a).map_or(code.len(), |x| x.0);
        let end = chars.get(b).map_or(code.len(), |x| x.0);
        code.get(start..end).unwrap_or("")
    };
    let mut i = 0;
    while i < chars.len() {
        let c = chars.get(i).map_or(' ', |x| x.1);
        let next = chars.get(i + 1).map_or(' ', |x| x.1);
        let start = i;
        let color = if c == '/' && next == '/' {
            while i < chars.len() && chars.get(i).is_some_and(|x| x.1 != '\n') {
                i += 1;
            }
            comment
        } else if c == '/' && next == '*' {
            i += 2;
            while i < chars.len() && !(chars.get(i).is_some_and(|x| x.1 == '*') && chars.get(i + 1).is_some_and(|x| x.1 == '/')) {
                i += 1;
            }
            i = (i + 2).min(chars.len());
            comment
        } else if c == '"' || c == '`' || c == '\'' {
            i += 1;
            while i < chars.len() {
                let d = chars.get(i).map_or(c, |x| x.1);
                i += 1;
                if d == '\\' {
                    i += 1;
                } else if d == c {
                    break;
                }
            }
            i = i.min(chars.len());
            string
        } else if c.is_ascii_digit() {
            while i < chars.len() && chars.get(i).is_some_and(|x| x.1.is_ascii_alphanumeric() || x.1 == '.' || x.1 == '_') {
                i += 1;
            }
            number
        } else if c.is_alphabetic() || c == '_' {
            while i < chars.len() && chars.get(i).is_some_and(|x| x.1.is_alphanumeric() || x.1 == '_') {
                i += 1;
            }
            let word = slice(start, i);
            let followed_by_paren = chars.get(i).is_some_and(|x| x.1 == '(');
            if KEYWORDS.contains(&word) {
                keyword
            } else if followed_by_paren {
                call
            } else {
                plain
            }
        } else {
            i += 1;
            plain
        };
        job.append(slice(start, i.max(start + 1)), 0.0, fmt(color));
        i = i.max(start + 1);
    }
    job
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn highlighting_keeps_every_character() {
        let font = FontId::monospace(12.0);
        for code in ["", "fn main(workbook) { let x = \"a\\\"b\"; // c\n /* d */ 12.5 }", "\"unterminated", "/* open", "ü€ 😀 'x'"] {
            let job = highlight(code, &font, false);
            assert_eq!(job.text, code);
            let job = highlight(code, &font, true);
            assert_eq!(job.text, code);
        }
    }
}
