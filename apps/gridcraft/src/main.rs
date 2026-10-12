//! GridCraft desktop app.
//!
//! Usage: `gridcraft [--control <port>] [--sample <name>] [files…]`
//!
//! `--control <port>` (or `GRIDCRAFT_CONTROL_PORT`) starts a localhost JSON-lines control
//! server: `{"id":1,"method":"ui.inspect","params":{}}` → `{"id":1,"ok":true,"result":…}`.
//! See `gridcraft_ui_egui::control` and `docs/control-protocol.md` for the methods.
#![cfg_attr(all(target_os = "windows", not(debug_assertions)), windows_subsystem = "windows")]
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]

#[cfg(target_os = "macos")]
mod apple_events;
mod control_server;
#[cfg(any(target_os = "windows", test))]
mod graphics;
mod logging;
#[cfg(target_os = "macos")]
mod native_menu;
mod print;

use std::cell::RefCell;
use std::rc::Rc;

use gridcraft_engine::{Prefs, Session};
use gridcraft_ui_egui::{Services, SheetApp, UiState, WorkbookWindowAction, WorkbookWindowInfo};
use serde_json::json;

#[derive(Clone)]
struct AppPreferences {
    ui: UiState,
    engine: Prefs,
}

impl AppPreferences {
    fn new(app: &SheetApp) -> Self {
        Self { ui: app.ui.clone(), engine: app.session.prefs.clone() }
    }

    fn update_from(&mut self, app: &SheetApp) {
        self.ui.dark = app.ui.dark;
        self.ui.system_theme = app.ui.system_theme;
        self.ui.status_bar = app.ui.status_bar;
        self.ui.animations = app.ui.animations;
        self.ui.recent.clone_from(&app.ui.recent);
        self.ui.language = app.ui.language;
        self.ui.interface_scale = app.ui.interface_scale;
        self.engine = app.session.prefs.clone();
    }

    fn apply_to(&self, app: &mut SheetApp) {
        app.ui.dark = self.ui.dark;
        app.ui.system_theme = self.ui.system_theme;
        app.ui.status_bar = self.ui.status_bar;
        app.ui.animations = self.ui.animations;
        app.ui.recent.clone_from(&self.ui.recent);
        app.ui.language = self.ui.language;
        app.ui.interface_scale = self.ui.interface_scale;
        app.session.prefs = self.engine.clone();
    }
}

struct App {
    windows: Vec<SheetApp>,
    preferences: AppPreferences,
    focused: Option<u64>,
    focus_request: Option<u64>,
    #[cfg(target_os = "macos")]
    root_hidden: bool,
    #[cfg(target_os = "macos")]
    native_menu: Option<native_menu::NativeMenu>,
    #[cfg(target_os = "macos")]
    apple_events: Option<fmv_macos_events::Inbox>,
}

impl App {
    fn new(app: SheetApp) -> Self {
        let focused = app.session.active().map(|d| d.uid);
        let preferences = AppPreferences::new(&app);
        let mut this = Self {
            windows: vec![app],
            preferences,
            focused,
            focus_request: None,
            #[cfg(target_os = "macos")]
            root_hidden: false,
            #[cfg(target_os = "macos")]
            native_menu: None,
            #[cfg(target_os = "macos")]
            apple_events: None,
        };
        this.split_workbooks();
        this
    }

    fn apply_preferences(&mut self) {
        for app in &mut self.windows {
            self.preferences.apply_to(app);
        }
    }

    fn capture_preferences(&mut self) {
        let index =
            self.focus_request.or(self.focused).and_then(|id| self.windows.iter().position(|app| Self::window_id(app) == Some(id))).unwrap_or(0);
        if let Some(app) = self.windows.get(index) {
            self.preferences.update_from(app);
        }
        self.apply_preferences();
    }

    fn viewport_id(id: u64) -> egui::ViewportId {
        egui::ViewportId::from_hash_of(("workbook", id))
    }

    fn viewport_for_window(&self, id: u64) -> egui::ViewportId {
        if self.windows.first().is_some_and(|app| Self::window_id(app) == Some(id)) { egui::ViewportId::ROOT } else { Self::viewport_id(id) }
    }

    fn window_id(app: &SheetApp) -> Option<u64> {
        app.session.active().map(|d| d.uid)
    }

    fn window_builder(app: &SheetApp, active: bool) -> egui::ViewportBuilder {
        let title = app.session.active().map(|d| d.display_title()).unwrap_or_else(|| "GridCraft".into());
        let mut viewport = egui::ViewportBuilder::default()
            .with_title(title)
            .with_inner_size([1440.0, 900.0])
            .with_min_inner_size([640.0, 420.0])
            .with_app_id("ai.storyteller.gridcraft")
            .with_active(active)
            .with_drag_and_drop(true);
        if cfg!(target_os = "macos") {
            viewport = viewport.with_fullsize_content_view(true).with_titlebar_shown(false).with_title_shown(false);
        }
        if let Some(icon) = icon() {
            viewport = viewport.with_icon(icon);
        }
        viewport
    }

    fn summaries(&self, active: u64) -> Vec<WorkbookWindowInfo> {
        self.windows
            .iter()
            .filter_map(|app| {
                let document = app.session.active()?;
                Some(WorkbookWindowInfo {
                    id: document.uid,
                    title: document.display_title(),
                    dirty: document.is_dirty(),
                    active: document.uid == active,
                })
            })
            .collect()
    }

    fn same_path(left: &str, right: &str) -> bool {
        let left = std::path::Path::new(left);
        let right = std::path::Path::new(right);
        left == right || left.canonicalize().ok().zip(right.canonicalize().ok()).is_some_and(|(left, right)| left == right)
    }

    fn open_path(&mut self, path: &str) {
        if let Some(id) = self.windows.iter().find_map(|app| {
            let document = app.session.active()?;
            document.path.as_deref().is_some_and(|open| Self::same_path(open, path)).then_some(document.uid)
        }) {
            self.focus_request = Some(id);
            return;
        }

        let target = self.focused.and_then(|id| self.windows.iter().position(|app| Self::window_id(app) == Some(id))).unwrap_or(0);
        let Some(app) = self.windows.get_mut(target) else { return };
        let reuse_window = app.session.documents().is_empty()
            || (app.session.documents().len() == 1 && app.session.active().is_some_and(|document| document.path.is_none() && !document.is_dirty()));
        if !app.open_path(path) {
            return;
        }
        self.preferences.update_from(app);
        if reuse_window {
            // The engine opens into an empty coordinator or replaces an untouched blank document
            // in place. Keep that workbook in the same native window and bring it forward.
            self.focus_request = Self::window_id(app);
        }
        self.split_workbooks();
    }

    fn split_workbooks(&mut self) {
        let mut extracted = Vec::new();
        for app in &mut self.windows {
            while app.session.documents().len() > 1 {
                let Some(document) = app.session.take_document(1) else { break };
                extracted.push((app.ui.clone(), app.session.prefs.clone(), document));
            }
        }
        for (ui_state, prefs, mut document) in extracted {
            if let Some(path) = document.path.as_deref()
                && let Some(existing) = self.windows.iter().find_map(|app| {
                    let open = app.session.active()?;
                    (open.path.as_deref() == Some(path)).then_some(open.uid)
                })
            {
                self.focus_request = Some(existing);
                continue;
            }
            if document.path.is_none()
                && document.title.strip_prefix("Book").is_some_and(|suffix| suffix.parse::<usize>().is_ok())
                && self.windows.iter().any(|app| app.session.active().is_some_and(|open| open.path.is_none() && open.title == document.title))
            {
                let number = (1..)
                    .find(|number| {
                        let title = format!("Book{number}");
                        !self.windows.iter().any(|app| app.session.active().is_some_and(|open| open.path.is_none() && open.title == title))
                    })
                    .unwrap_or(1);
                document.title = format!("Book{number}");
            }
            let id = document.uid;
            let mut session = Session::new();
            session.prefs = prefs;
            session.add_document(document);
            let mut app = SheetApp::new(session, services());
            app.ui = ui_state;
            self.preferences.apply_to(&mut app);
            self.windows.push(app);
            self.focus_request = Some(id);
        }
    }

    fn handle_action(&mut self, action: WorkbookWindowAction) {
        match action {
            WorkbookWindowAction::Focus(id) => self.focus_request = Some(id),
            WorkbookWindowAction::Close(id) => {
                self.focus_request = Some(id);
                if let Some(app) = self.windows.iter_mut().find(|app| Self::window_id(app) == Some(id)) {
                    app.close_document(app.session.active_index(), false);
                }
            }
        }
    }

    fn remove_closed(&mut self, ctx: &egui::Context) {
        let control_rx = self.windows.iter_mut().find(|app| app.session.documents().is_empty()).and_then(|app| app.control_rx.take());
        if self.windows.first().is_some_and(|app| app.session.documents().is_empty()) {
            if self.windows.len() == 1 {
                #[cfg(not(target_os = "macos"))]
                self.windows.clear();
            } else {
                let promoted_id = Self::window_id(&self.windows[1]);
                let geometry = promoted_id.map(|id| {
                    ctx.input_for(Self::viewport_id(id), |input| {
                        (
                            input.viewport().outer_rect.map(|rect| rect.min),
                            input.viewport().inner_rect.map(|rect| rect.size()),
                            input.viewport().maximized,
                            input.viewport().fullscreen,
                        )
                    })
                });
                self.windows.remove(0);
                if let Some((position, size, maximized, fullscreen)) = geometry {
                    if let Some(position) = position {
                        ctx.send_viewport_cmd(egui::ViewportCommand::OuterPosition(position));
                    }
                    if let Some(size) = size {
                        ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(size));
                    }
                    if let Some(maximized) = maximized {
                        ctx.send_viewport_cmd(egui::ViewportCommand::Maximized(maximized));
                    }
                    if let Some(fullscreen) = fullscreen {
                        ctx.send_viewport_cmd(egui::ViewportCommand::Fullscreen(fullscreen));
                    }
                }
                self.focus_request = promoted_id;
            }
        }
        let mut index = 1;
        while index < self.windows.len() {
            if self.windows[index].session.documents().is_empty() {
                self.windows.remove(index);
            } else {
                index += 1;
            }
        }
        if self.focused.is_some_and(|id| !self.windows.iter().any(|app| Self::window_id(app) == Some(id))) {
            self.focused = self.windows.first().and_then(Self::window_id);
        }
        if let Some(receiver) = control_rx {
            let target = self.focused.and_then(|id| self.windows.iter().position(|app| Self::window_id(app) == Some(id))).unwrap_or(0);
            if let Some(app) = self.windows.get_mut(target) {
                app.control_rx = Some(receiver);
            }
        }
    }

    fn move_control_channel_to_focused(&mut self) {
        let Some(focused) = self.focused else { return };
        let Some(target) = self.windows.iter().position(|app| Self::window_id(app) == Some(focused)) else { return };
        let Some(source) = self.windows.iter().position(|app| app.control_rx.is_some()) else { return };
        if source != target
            && let Some(receiver) = self.windows[source].control_rx.take()
        {
            self.windows[target].control_rx = Some(receiver);
        }
    }
}

impl eframe::App for App {
    fn logic(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.apply_preferences();
        #[cfg(target_os = "macos")]
        {
            if self.native_menu.is_none() && std::env::var_os("GRIDCRAFT_NO_NATIVE_MENU").is_none() {
                self.native_menu = Some(native_menu::NativeMenu::install(ctx));
            }
            if let Some(menu) = &self.native_menu {
                let target = self.focused.and_then(|id| self.windows.iter().position(|app| Self::window_id(app) == Some(id))).unwrap_or(0);
                if let Some(app) = self.windows.get_mut(target) {
                    menu.poll(app, ctx);
                }
            }
            let events = self.apple_events.as_ref().map(apple_events::drain).unwrap_or_default();
            for event in events {
                match event {
                    fmv_macos_events::Event::Open(paths) => {
                        for path in paths.iter().filter_map(|path| path.to_str()) {
                            self.open_path(path);
                        }
                    }
                    // #496 will provide the shared dirty-workbook quit flow. Preserve the
                    // existing close request until that PR is available to build on.
                    fmv_macos_events::Event::Quit => ctx.send_viewport_cmd(egui::ViewportCommand::Close),
                }
            }
        }
        self.capture_preferences();
        if let Some(app) = self.windows.first_mut() {
            app.logic_for_workbook_window(ctx);
            app.poll_printing(ctx);
        }
        if ctx.input(|i| i.viewport().close_requested()) {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            if let Some(app) = self.windows.first_mut()
                && !app.printing.busy()
            {
                app.close_document(app.session.active_index(), false);
            }
        }
        self.move_control_channel_to_focused();
    }
    fn raw_input_hook(&mut self, ctx: &egui::Context, raw: &mut egui::RawInput) {
        let viewport_id = ctx.viewport_id();
        let root_id = self.windows.first().and_then(Self::window_id);
        let Some(app) = self.windows.iter_mut().find(|app| {
            Self::window_id(app).is_some_and(|id| {
                let expected = if root_id == Some(id) { egui::ViewportId::ROOT } else { Self::viewport_id(id) };
                expected == viewport_id
            })
        }) else {
            return;
        };
        app.raw_input_hook(raw);
        // Files dropped on the window open as workbooks.
        for f in std::mem::take(&mut raw.dropped_files) {
            if let Some(p) = f.path().to_str().map(str::to_string).filter(|p| !p.is_empty()) {
                app.open_path(&p);
            }
        }
    }
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        self.apply_preferences();
        self.split_workbooks();
        let activate = self.focus_request.take();
        let ctx = ui.ctx().clone();
        let mut actions = Vec::new();
        let mut focused = self.focused;
        if let Some(root_id) = self.windows.first().and_then(Self::window_id) {
            let summaries = self.summaries(root_id);
            if ui.input(|i| i.viewport().focused.unwrap_or(false)) {
                focused = Some(root_id);
            }
            if let Some(action) = self.windows[0].ui_with_workbook_windows(ui, Some(&summaries)) {
                actions.push(action);
            }
        }
        for index in 1..self.windows.len() {
            let Some(id) = Self::window_id(&self.windows[index]) else { continue };
            let summaries = self.summaries(id);
            let builder = Self::window_builder(&self.windows[index], activate == Some(id));
            let app = &mut self.windows[index];
            ctx.show_viewport_immediate(Self::viewport_id(id), builder, |ui, _class| {
                app.logic_for_workbook_window(ui.ctx());
                app.poll_printing(ui.ctx());
                if ui.input(|i| i.viewport().focused.unwrap_or(false)) {
                    focused = Some(id);
                }
                if ui.input(|i| i.viewport().close_requested()) {
                    ui.ctx().send_viewport_cmd(egui::ViewportCommand::CancelClose);
                    if !app.printing.busy() {
                        actions.push(WorkbookWindowAction::Close(id));
                    }
                }
                if let Some(action) = app.ui_with_workbook_windows(ui, Some(&summaries)) {
                    actions.push(action);
                }
            });
        }
        self.focused = focused;
        self.capture_preferences();
        for action in actions {
            self.handle_action(action);
        }
        self.split_workbooks();
        self.remove_closed(&ctx);
        #[cfg(target_os = "macos")]
        if let Some(root_id) = self.windows.first().and_then(Self::window_id)
            && self.root_hidden
        {
            ctx.send_viewport_cmd(egui::ViewportCommand::Visible(true));
            self.root_hidden = false;
            self.focus_request = Some(root_id);
        }
        if let Some(id) = self.focus_request.or(activate)
            && self.windows.iter().any(|app| Self::window_id(app) == Some(id))
        {
            ctx.send_viewport_cmd_to(self.viewport_for_window(id), egui::ViewportCommand::Focus);
            self.focused = Some(id);
            if self.focus_request.is_some() {
                ctx.request_repaint();
            }
        }
        if self.windows.first().is_none_or(|app| app.session.documents().is_empty()) {
            self.focused = None;
            #[cfg(target_os = "macos")]
            if !self.root_hidden {
                ctx.send_viewport_cmd(egui::ViewportCommand::Visible(false));
                self.root_hidden = true;
            }
            #[cfg(not(target_os = "macos"))]
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
    }
    fn on_exit(&mut self) {
        print::finish_pending_jobs();
        save_prefs(&self.preferences.ui, &self.preferences.engine);
        // macOS (#117): end the process here, before eframe tears the window down. AccessKit's
        // macOS adapter subclasses the window's content view and restores its class on teardown;
        // AppKit's Touch Bar support observes that view (KVO), so in the next display cycle
        // `-[_NSTouchBarFinderObservation invalidate]` throws "not registered" and the quit ends
        // in a crash report. eframe calls `on_exit` on every way of quitting (Quit/⌘Q, the Dock,
        // the close button, `app.quit`), and nothing after it is needed: the preferences are
        // saved and the log file is unbuffered. eframe does the same when it doesn't
        // run-and-return. `on_exit` only runs once the app exists, never on a failed start.
        #[cfg(target_os = "macos")]
        std::process::exit(0);
    }
}

fn config_dir() -> Option<std::path::PathBuf> {
    if cfg!(target_os = "macos") {
        std::env::var_os("HOME").map(|h| std::path::PathBuf::from(h).join("Library/Application Support/GridCraft"))
    } else if cfg!(windows) {
        std::env::var_os("APPDATA").map(|a| std::path::PathBuf::from(a).join("GridCraft"))
    } else {
        std::env::var_os("XDG_CONFIG_HOME")
            .map(std::path::PathBuf::from)
            .or_else(|| std::env::var_os("HOME").map(|h| std::path::PathBuf::from(h).join(".config")))
            .map(|c| c.join("gridcraft"))
    }
}

/// Runs without preferences (`GRIDCRAFT_NO_PREFS`, agents' test runs) neither read nor write
/// them, and keep no log file.
fn prefs_enabled() -> bool {
    std::env::var_os("GRIDCRAFT_NO_PREFS").is_none()
}

/// Where the log files live: `logs` in the settings folder, next to `ui.json` (see `logging`).
fn log_dir() -> Option<std::path::PathBuf> {
    Some(config_dir()?.join("logs"))
}

fn load_prefs(app: &mut SheetApp) {
    if !prefs_enabled() {
        return;
    }
    // First run (or a ui.json without a usable language): follow the desktop's language.
    app.ui.language = gridcraft_ui_egui::i18n::Language::system();
    // ...and its light or dark appearance (#239), until a theme is chosen and saved in ui.json.
    app.ui.system_theme = true;
    let Some(dir) = config_dir() else { return };
    if let Ok(b) = std::fs::read(dir.join("ui.json"))
        && let Ok(ui) = serde_json::from_slice::<gridcraft_ui_egui::UiState>(&b)
    {
        let saved = serde_json::from_slice(&b).ok().and_then(|v| gridcraft_ui_egui::i18n::saved_language(&v));
        let language = saved.unwrap_or(app.ui.language);
        app.ui = ui;
        app.ui.language = language;
    }
    if let Ok(b) = std::fs::read(dir.join("prefs.json"))
        && let Ok(p) = serde_json::from_slice(&b)
    {
        app.session.prefs = p;
    }
}

fn save_prefs(ui: &UiState, prefs: &Prefs) {
    if !prefs_enabled() {
        return;
    }
    let Some(dir) = config_dir() else { return };
    let _ = std::fs::create_dir_all(&dir);
    if let Ok(b) = serde_json::to_vec_pretty(ui) {
        let _ = std::fs::write(dir.join("ui.json"), b);
    }
    if let Ok(b) = serde_json::to_vec_pretty(prefs) {
        let _ = std::fs::write(dir.join("prefs.json"), b);
    }
}

/// Read the system clipboard through the shared handle, creating it on first use.
fn read_clipboard<T>(
    handle: &RefCell<Option<arboard::Clipboard>>,
    read: impl FnOnce(&mut arboard::Clipboard) -> Result<T, arboard::Error>,
) -> Option<T> {
    let mut clipboard = handle.try_borrow_mut().ok()?;
    if clipboard.is_none() {
        *clipboard = arboard::Clipboard::new().ok();
    }
    read(clipboard.as_mut()?).ok()
}

fn services() -> Services {
    // Keep the owner alive: X11/Wayland serve clipboard data from this handle.
    let clipboard: Rc<RefCell<Option<arboard::Clipboard>>> = Rc::default();
    let (html_clipboard, text_clipboard) = (clipboard.clone(), clipboard.clone());
    Services {
        copy_html: Some(Box::new(move |html, text| {
            let mut clipboard = clipboard.borrow_mut();
            if clipboard.is_none() {
                *clipboard = Some(arboard::Clipboard::new().map_err(|e| e.to_string())?);
            }
            clipboard.as_mut().ok_or("clipboard unavailable")?.set_html(html, Some(text)).map_err(|e| e.to_string())
        })),
        paste_html: Some(Box::new(move || read_clipboard(&html_clipboard, |clipboard| clipboard.get().html()))),
        paste_text: Some(Box::new(move || read_clipboard(&text_clipboard, |clipboard| clipboard.get_text()))),
        pick_open: Some(Box::new(|| {
            rfd::FileDialog::new()
                .add_filter("Spreadsheets", &["xlsx", "xlsm", "xls", "xlsb", "ods", "csv", "tsv", "txt", "json"])
                .add_filter("Excel Workbook", &["xlsx", "xlsm"])
                .add_filter("Excel 97-2003 Workbook (import)", &["xls"])
                .add_filter("Excel Binary Workbook (data import)", &["xlsb"])
                .add_filter("OpenDocument Spreadsheet (data import)", &["ods"])
                .add_filter("CSV", &["csv"])
                .pick_file()
                .and_then(|p| p.to_str().map(str::to_string))
        })),
        pick_picture: Some(Box::new(|| {
            rfd::FileDialog::new()
                .add_filter("Pictures (PNG, JPEG)", &["png", "jpg", "jpeg"])
                .pick_file()
                .and_then(|p| p.to_str().map(str::to_string))
        })),
        pick_save: Some(Box::new(|suggested: &str| {
            let path = std::path::Path::new(suggested);
            let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or("Book1").to_string();
            // The suggested name's type (an export asks for .pdf, .csv or .html) comes first.
            const TYPES: [(&str, &str); 4] =
                [("Excel Workbook (.xlsx)", "xlsx"), ("CSV UTF-8 (.csv)", "csv"), ("Web Page (.html)", "html"), ("PDF (.pdf)", "pdf")];
            let ext = path.extension().and_then(|e| e.to_str()).map(str::to_ascii_lowercase).filter(|e| TYPES.iter().any(|(_, x)| x == e));
            let ext = ext.unwrap_or_else(|| "xlsx".to_string());
            let mut dialog = rfd::FileDialog::new().set_file_name(format!("{stem}.{ext}"));
            for (label, x) in TYPES.iter().filter(|(_, x)| *x == ext).chain(TYPES.iter().filter(|(_, x)| *x != ext)) {
                dialog = dialog.add_filter(*label, &[*x]);
            }
            dialog.save_file().and_then(|p| p.to_str().map(str::to_string))
        })),
        open_url: Some(Box::new(|url: &str| open_with_system(url))),
        printers: Some(print::discover()),
        print_native: Some(print::submit()),
        ..Default::default()
    }
}

/// Opens a URL or file with the system's default application.
fn open_with_system(target: &str) {
    // Last line of defence behind `link.open` and the click prompt: the system only ever gets
    // http, https and mailto addresses.
    let gridcraft_engine::links::LinkKind::External(target) = gridcraft_engine::links::classify(target) else {
        log::warn!("refused to open {target:?}: only http, https and mailto links are opened");
        return;
    };
    let target = target.as_str();
    let cmd = if cfg!(target_os = "macos") {
        "open"
    } else if cfg!(windows) {
        "explorer"
    } else {
        "xdg-open"
    };
    if let Err(e) = std::process::Command::new(cmd).arg(target).spawn() {
        log::warn!("{cmd} {target}: {e}");
    }
}

fn icon() -> Option<egui::IconData> {
    static ICON: std::sync::OnceLock<Option<egui::IconData>> = std::sync::OnceLock::new();
    ICON.get_or_init(|| {
        #[cfg(target_os = "macos")]
        let bytes = include_bytes!("../../../assets/app-icon/gridcraft-macos-512.png");
        #[cfg(not(target_os = "macos"))]
        let bytes = include_bytes!("../../../assets/app-icon/hicolor/256x256/apps/ai.storyteller.gridcraft.png");
        let img = image::load_from_memory(bytes).ok()?.to_rgba8();
        Some(egui::IconData { width: img.width(), height: img.height(), rgba: img.into_raw() })
    })
    .clone()
}

fn main() -> eframe::Result<()> {
    // First, so the panic hook and every start-up warning are recorded (`logging`).
    let logger = logging::install();
    // Commands catch their panics (`Session::execute`), but the default hook only prints to
    // standard error; this one sends every panic, caught or not, to the log file too, with a
    // backtrace when RUST_BACKTRACE is set.
    std::panic::set_hook(Box::new(|info| {
        let trace = std::backtrace::Backtrace::capture();
        let report = if trace.status() == std::backtrace::BacktraceStatus::Captured {
            format!("internal error: {info}\n{trace}")
        } else {
            format!("internal error: {info}")
        };
        // Standard error and the log file; standard error alone when RUST_LOG turned errors off.
        if log::log_enabled!(log::Level::Error) {
            log::error!("{report}");
        } else {
            // `eprintln!` panics on a broken stderr pipe, and a panic inside the panic hook aborts.
            let _ = std::io::Write::write_fmt(&mut std::io::stderr(), format_args!("gridcraft: {report}\n"));
        }
    }));
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.iter().any(|a| a == "--version" || a == "-V") {
        println!("GridCraft {}", env!("CARGO_PKG_VERSION"));
        return Ok(());
    }
    let mut control_port: Option<u16> = std::env::var("GRIDCRAFT_CONTROL_PORT").ok().and_then(|p| p.parse().ok());
    let mut sample: Option<String> = None;
    let mut files: Vec<String> = Vec::new();
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--control" => {
                i += 1;
                control_port = args.get(i).and_then(|p| p.parse().ok());
            }
            "--sample" => {
                i += 1;
                sample = Some(args.get(i).cloned().unwrap_or_else(|| "sales".into()));
            }
            a if a.starts_with("-psn_") => {}
            a => files.push(a.to_string()),
        }
        i += 1;
    }
    // The log file lives in the settings folder, next to the preferences; opened after the
    // arguments, so `--version` leaves no file behind. Records logged until now are written to it
    // first. Runs without preferences (agents' test runs) log to standard error only, so they
    // don't rotate away the user's own logs.
    if let Some(logger) = logger {
        match log_dir().filter(|_| prefs_enabled()) {
            Some(dir) => match logger.attach_dir(&dir) {
                Ok(path) => log::info!("GridCraft {}, log file {}", env!("CARGO_PKG_VERSION"), path.display()),
                // Standard error only by now (`attach_dir` gave up on the file); unlike `eprintln!`, never panics.
                Err(e) => log::warn!("no log file: {e}"),
            },
            None => logger.no_file(),
        }
    }
    let mut session = Session::new();
    if let Some(s) = &sample
        && let Err(e) = session.execute("file.new", json!({"sample": s}))
    {
        log::warn!("sample {s}: {e}");
    }
    if session.documents().is_empty() {
        session.new_workbook();
    }
    let mut app = SheetApp::new(session, services());
    app.watch_desktop_theme();
    app.after_engine(); // Show warnings from the sample.
    let control_port = control_port;
    load_prefs(&mut app);
    // Files named on the command line (a double-click on Windows and Linux) open as File › Open
    // opens them: a password-protected one asks for its password, and errors are shown, not
    // only logged. Each replaces the untouched blank Book1, as a file opened from Finder does.
    for f in &files {
        app.open_path(f);
    }
    let mut viewport = egui::ViewportBuilder::default()
        .with_title("GridCraft")
        .with_inner_size([1440.0, 900.0])
        .with_min_inner_size([640.0, 420.0])
        .with_app_id("ai.storyteller.gridcraft")
        .with_drag_and_drop(true);
    if cfg!(target_os = "macos") {
        viewport = viewport.with_fullsize_content_view(true).with_titlebar_shown(false).with_title_shown(false);
    }
    if let Some(icon) = icon() {
        viewport = viewport.with_icon(icon);
    }
    let options = eframe::NativeOptions { viewport, ..Default::default() };
    // Before eframe creates the wgpu instance: Windows starts on DirectX 12, or OpenGL when
    // DirectX 12 has no GPU driver, and never touches Vulkan unless asked (see graphics.rs).
    #[cfg(target_os = "windows")]
    let backend_override = eframe::wgpu::Backends::from_env();
    #[cfg(target_os = "windows")]
    let (options, first_backends) = {
        let mut options = options;
        let backends = graphics::configure(&mut options, backend_override);
        (options, backends)
    };
    #[cfg(target_os = "windows")]
    let retry_options = options.clone();
    // Registered before the event loop starts, so it catches the Finder event that launched us as
    // well as later ones. Lives until the event loop returns; the app creator only borrows it.
    #[cfg(target_os = "macos")]
    let apple_events = apple_events::AppleEvents::install();
    #[cfg(target_os = "macos")]
    let apple_events = &apple_events;
    // The app waits here until a start gets as far as creating it; a start that fails before
    // (no graphics adapter, device or surface) leaves it here for the next attempt.
    let pending = std::cell::Cell::new(Some(app));
    let run = |options: eframe::NativeOptions| {
        eframe::run_native(
            "GridCraft",
            options,
            Box::new(|cc| {
                let Some(mut app) = pending.take() else {
                    return Err("the app was already created".into());
                };
                SheetApp::setup_context_for_language(&cc.egui_ctx, app.ui.dark, app.ui.language);
                if let Some(port) = control_port {
                    match control_server::start(port, cc.egui_ctx.clone()) {
                        Ok(rx) => app.control_rx = Some(rx),
                        Err(e) => log::error!("control channel on port {port}: {e}"),
                    }
                }
                let mut shell = App::new(app);
                #[cfg(target_os = "macos")]
                {
                    shell.apple_events = Some(apple_events.connect(&cc.egui_ctx));
                }
                Ok(Box::new(shell))
            }),
        )
    };
    let result = run(options);
    // A graphics error before the app existed: start once more with the other backend (eframe
    // reuses its event loop for the second run). Errors after that, or from winit, are returned.
    #[cfg(target_os = "windows")]
    if let Err(eframe::Error::Wgpu(e)) = &result
        && let Some(app) = pending.take()
        && let Some(backends) = graphics::fallback_backends(backend_override, first_backends)
    {
        log::error!("graphics: the renderer didn't start on {first_backends:?} ({e}); trying {backends:?}");
        pending.set(Some(app));
        let mut options = retry_options;
        graphics::configure_backends(&mut options, backends);
        return run(options);
    }
    result
}

#[cfg(test)]
mod packaging_tests {
    use gridcraft_engine::io::FileKind;

    /// The text between `start` and `end` that follows each `marker` in `text`, sorted (e.g. the
    /// `xlsb` in `Classes\.xlsb\OpenWithProgids`).
    fn extensions(text: &str, marker: &str, start: &str, end: char) -> Vec<String> {
        let mut v: Vec<String> = text.split(marker).skip(1).filter_map(|s| s.split_once(start)?.1.split(end).next()).map(str::to_string).collect();
        v.sort();
        v
    }

    /// macOS Open With (`CFBundleDocumentTypes`) offers GridCraft for the same files as Windows
    /// Open with, and GridCraft opens each of them. #321 added `.xlsb` and `.ods` on Windows and
    /// Linux only.
    #[test]
    fn macos_and_windows_offer_the_same_file_types() {
        let plist = include_str!("../../../packaging/macos/Info.plist.in");
        let wxs = include_str!("../../../packaging/windows/gridcraft.wxs");
        let mac = extensions(plist, "<key>CFBundleTypeExtensions</key>", "<string>", '<');
        let win = extensions(wxs, "Key=\"Software\\Classes\\.", "", '\\');
        assert!(mac.iter().any(|e| e == "xlsx"), "Info.plist.in extensions not found: {mac:?}");
        assert_eq!(mac, win);
        for ext in &mac {
            assert!(FileKind::from_path(&format!("book.{ext}")).is_some(), "GridCraft doesn't open .{ext}");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn csv(name: &str) -> std::path::PathBuf {
        let path = std::env::temp_dir().join(format!("gridcraft-window-{name}-{}.csv", std::process::id()));
        std::fs::write(&path, "value\n1\n").unwrap();
        path
    }

    #[test]
    fn each_open_document_gets_its_own_window() {
        let mut session = Session::new();
        session.new_workbook();
        session.new_workbook();
        let ids: Vec<u64> = session.documents().iter().map(|document| document.uid).collect();

        let app = App::new(SheetApp::new(session, Services::default()));

        assert_eq!(app.windows.len(), 2);
        assert!(app.windows.iter().all(|window| window.session.documents().len() == 1));
        assert_eq!(app.windows.iter().filter_map(App::window_id).collect::<Vec<_>>(), ids);
    }

    #[test]
    fn blank_workbook_titles_are_unique_across_windows() {
        let mut session = Session::new();
        session.new_workbook();
        if let Some(document) = session.active_mut() {
            document.title = "Report".into();
        }
        session.new_workbook();
        let mut app = App::new(SheetApp::new(session, Services::default()));

        app.windows[0].session.new_workbook();
        app.split_workbooks();

        let titles: Vec<String> = app.windows.iter().filter_map(|window| window.session.active().map(|document| document.title.clone())).collect();
        assert_eq!(titles, ["Report", "Book1", "Book2"]);
    }

    #[test]
    fn opening_a_file_reuses_an_untouched_blank_window() {
        let path = csv("replace-blank");
        let mut session = Session::new();
        session.new_workbook();
        let mut app = App::new(SheetApp::new(session, Services::default()));

        app.open_path(path.to_str().unwrap());

        assert_eq!(app.windows.len(), 1);
        assert_eq!(app.windows[0].session.documents().len(), 1);
        assert_eq!(app.windows[0].session.active().and_then(|document| document.path.as_deref()), path.to_str());
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn opening_a_file_without_a_focused_workbook_uses_the_coordinator_window() {
        let path = csv("empty-coordinator");
        let mut app = App::new(SheetApp::new(Session::new(), Services::default()));
        assert_eq!(app.focused, None);

        app.open_path(path.to_str().unwrap());

        assert_eq!(app.windows.len(), 1);
        assert_eq!(app.windows[0].session.active().and_then(|document| document.path.as_deref()), path.to_str());
        assert!(app.focus_request.is_some());
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn opening_a_file_beside_a_changed_blank_uses_a_new_window() {
        let path = csv("keep-dirty-blank");
        let mut session = Session::new();
        session.new_workbook();
        session.execute("cell.set", json!({"cell": "A1", "input": "keep me"})).unwrap();
        let mut app = App::new(SheetApp::new(session, Services::default()));

        app.open_path(path.to_str().unwrap());

        assert_eq!(app.windows.len(), 2);
        assert!(app.windows.iter().any(|window| window.session.active().is_some_and(|document| document.path.is_none() && document.is_dirty())));
        assert!(app.windows.iter().any(|window| window.session.active().and_then(|document| document.path.as_deref()) == path.to_str()));
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn reopening_a_file_focuses_its_existing_window() {
        let first = csv("existing-first");
        let second = csv("existing-second");
        let mut session = Session::new();
        session.new_workbook();
        let mut app = App::new(SheetApp::new(session, Services::default()));
        app.open_path(first.to_str().unwrap());
        let first_id = App::window_id(&app.windows[0]).unwrap();
        app.open_path(second.to_str().unwrap());
        assert_eq!(app.windows.len(), 2);

        app.focus_request = None;
        app.focused = App::window_id(&app.windows[1]);
        app.open_path(first.to_str().unwrap());

        assert_eq!(app.windows.len(), 2);
        assert_eq!(app.focus_request, Some(first_id));
        let _ = std::fs::remove_file(first);
        let _ = std::fs::remove_file(second);
    }

    #[test]
    fn preferences_from_the_focused_window_are_shared_and_saved() {
        let mut session = Session::new();
        session.new_workbook();
        session.new_workbook();
        let mut app = App::new(SheetApp::new(session, Services::default()));
        let second = App::window_id(&app.windows[1]);
        app.focused = second;
        app.focus_request = None;
        app.windows[1].ui.dark = true;
        app.windows[1].ui.system_theme = false;
        app.windows[1].ui.language = gridcraft_ui_egui::i18n::Language::Ja;
        app.windows[1].ui.recent = vec!["second.xlsx".into()];
        app.windows[1].session.prefs.formula_bar = false;

        app.capture_preferences();

        assert!(app.preferences.ui.dark);
        assert_eq!(app.preferences.ui.language, gridcraft_ui_egui::i18n::Language::Ja);
        assert_eq!(app.preferences.ui.recent, ["second.xlsx"]);
        assert!(!app.preferences.engine.formula_bar);
        assert!(app.windows.iter().all(|window| window.ui.dark));
        assert!(app.windows.iter().all(|window| window.ui.language == gridcraft_ui_egui::i18n::Language::Ja));
        assert!(app.windows.iter().all(|window| !window.session.prefs.formula_bar));
    }

    #[test]
    fn egui_widget_ids_are_scoped_to_the_workbook_window() {
        let mut session = Session::new();
        session.new_workbook();
        session.new_workbook();
        let app = App::new(SheetApp::new(session, Services::default()));

        assert_ne!(app.windows[0].widget_id("formula_bar"), app.windows[1].widget_id("formula_bar"));
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn closing_the_last_workbook_keeps_an_empty_app_coordinator() {
        let mut session = Session::new();
        session.new_workbook();
        let mut app = App::new(SheetApp::new(session, Services::default()));

        app.windows[0].close_document(0, false);
        app.remove_closed(&egui::Context::default());

        assert_eq!(app.windows.len(), 1);
        assert!(app.windows[0].session.documents().is_empty());
        assert_eq!(app.focused, None);
    }
}
