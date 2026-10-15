//! Plugins (Home › Add-ins): load and unload out-of-process plugins, list them, and run their
//! commands. Their functions are called from formulas (see `gridcraft_plugin`).

use std::sync::Arc;

use gridcraft_core::RangeRef;
use serde_json::{Value as Json, json};

use super::{CommandSpec, always, bad, bool_param, cmd, str_param};
use crate::{Result, Session, UiRequest};

/// Most selected cells whose values a plugin command receives.
const MAX_CONTEXT_CELLS: u64 = 100_000;
/// Most defined names a plugin command receives (real models can carry tens of thousands).
const MAX_CONTEXT_NAMES: usize = 10_000;

pub fn specs() -> Vec<CommandSpec> {
    vec![
        cmd!(query "plugins.list", "Plugins", [], None, "{} → [{id, name, version, description, dir, status, functions, commands}]", always, |_, _| Ok(list())),
        cmd!(query "plugins.load", "Load Plugin", [], None, "{path} → the plugin. `path`: a folder holding plugin.json, or the manifest", always, load),
        cmd!(query "plugins.unload", "Unload Plugin", [], None, "{id}", always, unload),
        cmd!(
            query "plugins.install",
            "Install Plugin from GitHub",
            [],
            None,
            "{url, dir?} → the plugin. Downloads the plugin in a GitHub repository (the release zip for this platform, else its source) into `dir` (default: the app's plugins folder) and loads it",
            always,
            install
        ),
        cmd!(
            query "plugins.setEnabled",
            "Turn Plugin On or Off",
            [],
            None,
            "{id, enabled} → the plugin. Off, a plugin stays listed but its functions, commands, ribbon and menu items are gone; remembered in the preferences",
            always,
            set_enabled
        ),
        cmd!(query "plugins.openHomepage", "Plugin Documentation", [], None, "{id} → {url}. Opens the plugin's web page (its manifest's `homepage`) in the browser", always, open_homepage),
        cmd!(
            "plugins.run",
            "Run Plugin Command",
            [],
            None,
            "{plugin, command, params?} → {ran, message?, result?, open?, dialog?, collab?}. The plugin's commands run as one undoable step; `open` (a file) and `dialog` (protocol 1.2) and `collab.join` (a live session, protocol 2) are returned for the caller, and the interface performs them when a button, menu item or plugin dialog ran the command",
            always,
            run
        ),
    ]
}

/// `home.addIns` and `plugins.list`.
pub fn list() -> Json {
    Json::Array(gridcraft_plugin::list().iter().map(|p| p.info()).collect())
}

fn load(s: &mut Session, p: &Json) -> Result<Json> {
    let path = str_param(p, "path").filter(|x| !x.trim().is_empty()).ok_or_else(|| bad("plugins.load", "missing `path`"))?;
    load_from(s, "plugins.load", std::path::Path::new(path))
}

fn install(s: &mut Session, p: &Json) -> Result<Json> {
    const ID: &str = "plugins.install";
    let url = str_param(p, "url").filter(|x| !x.trim().is_empty()).ok_or_else(|| bad(ID, "missing `url`"))?;
    let root = match str_param(p, "dir").filter(|x| !x.trim().is_empty()) {
        Some(d) => std::path::PathBuf::from(d),
        None => gridcraft_plugin::install::install_dir().ok_or_else(|| bad(ID, "no plugins folder: pass `dir`"))?,
    };
    let dir = gridcraft_plugin::install::from_github(url, &root).map_err(|e| bad(ID, e))?;
    load_from(s, ID, &dir)
}

/// Loads the plugin in `path` (a folder or its manifest) and recalculates the formulas using it.
pub fn load_from(s: &mut Session, id: &str, path: &std::path::Path) -> Result<Json> {
    let plugin = gridcraft_plugin::load(path).map_err(|e| bad(id, e))?;
    // A plugin turned off stays off when it's loaded again.
    if s.prefs.disabled_plugins.contains(&plugin.manifest.id) {
        let _ = gridcraft_plugin::set_enabled(&plugin.manifest.id, false);
    }
    recalc_users(s, &plugin.manifest.functions.iter().map(|f| f.name.clone()).collect::<Vec<_>>());
    Ok(plugin.info())
}

fn unload(s: &mut Session, p: &Json) -> Result<Json> {
    let id = str_param(p, "id").ok_or_else(|| bad("plugins.unload", "missing `id`"))?;
    let plugin = gridcraft_plugin::get(id).ok_or_else(|| bad("plugins.unload", format!("no plugin `{id}` is loaded")))?;
    gridcraft_plugin::unload(id);
    recalc_users(s, &plugin.manifest.functions.iter().map(|f| f.name.clone()).collect::<Vec<_>>());
    Ok(json!({"unloaded": id}))
}

fn set_enabled(s: &mut Session, p: &Json) -> Result<Json> {
    const ID: &str = "plugins.setEnabled";
    let id = str_param(p, "id").ok_or_else(|| bad(ID, "missing `id`"))?;
    let on = bool_param(p, "enabled").ok_or_else(|| bad(ID, "missing `enabled` (true or false)"))?;
    let plugin = gridcraft_plugin::get(id).ok_or_else(|| bad(ID, format!("no plugin `{id}` is loaded")))?;
    let changed = gridcraft_plugin::set_enabled(id, on).map_err(|e| bad(ID, e))?;
    s.prefs.disabled_plugins.retain(|x| x != id);
    if !on {
        s.prefs.disabled_plugins.push(id.to_string());
    }
    if changed {
        recalc_users(s, &plugin.manifest.functions.iter().map(|f| f.name.clone()).collect::<Vec<_>>());
    }
    Ok(plugin.info())
}

fn open_homepage(s: &mut Session, p: &Json) -> Result<Json> {
    const ID: &str = "plugins.openHomepage";
    let id = str_param(p, "id").ok_or_else(|| bad(ID, "missing `id`"))?;
    let plugin = gridcraft_plugin::get(id).ok_or_else(|| bad(ID, format!("no plugin `{id}` is loaded")))?;
    let url = plugin.manifest.homepage.trim();
    if url.is_empty() {
        return Err(bad(ID, format!("plugin {id} has no web page")));
    }
    s.ui_requests.push(UiRequest::OpenUrl(url.to_string()));
    Ok(json!({"url": url}))
}

/// Turns off the loaded plugins the preferences list as off (at startup, after loading them).
pub fn apply_prefs(prefs: &crate::Prefs) {
    for id in &prefs.disabled_plugins {
        if gridcraft_plugin::get(id).is_some()
            && let Err(e) = gridcraft_plugin::set_enabled(id, false)
        {
            log::warn!("plugin {id}: {e}");
        }
    }
}

/// Recalculates the open workbooks with formulas that call one of `names`, so their cells pick
/// up a plugin that just arrived (or show `#NAME?` for one that left).
fn recalc_users(s: &mut Session, names: &[String]) {
    if names.is_empty() {
        return;
    }
    let calls: Vec<String> = names.iter().map(|n| format!("{}(", n.to_ascii_uppercase())).collect();
    for d in s.docs.iter_mut() {
        let uses = d.wb.sheets.iter().any(|sh| {
            sh.cells.iter().any(|(_, cell)| cell.formula.as_ref().is_some_and(|f| calls.iter().any(|c| f.text.to_ascii_uppercase().contains(c))))
        });
        if uses {
            let wb = Arc::make_mut(&mut d.wb);
            d.calc.recalc_all(wb);
        }
    }
}

/// An absolute form of `path` (as given when it can't be made absolute).
pub fn absolute_path(path: &str) -> String {
    std::path::absolute(path).map(|p| p.display().to_string()).unwrap_or_else(|_| path.to_string())
}

/// `context.document` (protocol 1.2): the local file the workbook was opened from or last saved
/// to (`null` for a new, never-saved one), its name, and whether it has unsaved changes.
pub fn document_json(d: &crate::DocState) -> Json {
    json!({"path": d.path.as_deref().map(absolute_path), "name": d.display_title(), "dirty": d.is_dirty()})
}

/// The context every plugin request about the workbook gets: its title, active sheet, active
/// cell, selection and `document` (macros get this much; commands get [`context`]).
pub fn brief_context(s: &Session) -> Json {
    let Some(d) = s.active() else { return json!({}) };
    let sel = &d.selection;
    let range = sel.ranges.last().copied().unwrap_or(RangeRef::cell(sel.active));
    json!({
        "workbook": d.display_title(),
        "sheet": d.wb.active().map(|sh| sh.name.clone()).unwrap_or_default(),
        "active": sel.active.to_string(),
        "selection": range.to_string(),
        "document": document_json(d),
    })
}

/// Sends a document event (protocol 1.2) to the plugins that declared it, after it happened. It
/// never waits for them: they answer on the events thread, and the interface shows what they say.
pub fn emit(event: &str, path: &str, format: Option<&str>) {
    let path = absolute_path(path);
    let name = std::path::Path::new(&path).file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_else(|| path.clone());
    let mut params = json!({"path": path, "name": name});
    if let Some(f) = format {
        params["format"] = json!(f.to_ascii_lowercase());
    }
    gridcraft_plugin::events::dispatch(event, params);
}

/// The app is quitting: `documentClosed` for every open workbook that has a file, waiting at most
/// 5 s for the plugins (best effort).
pub fn quitting(s: &Session) {
    for d in s.documents() {
        if let Some(p) = &d.path {
            emit(gridcraft_plugin::events::DOCUMENT_CLOSED, p, None);
        }
    }
    gridcraft_plugin::events::wait_idle(gridcraft_plugin::events::timeout(gridcraft_plugin::events::DOCUMENT_CLOSED));
}

/// Checks the file a plugin command asked to open: absolute, an existing regular file, with an
/// extension GridCraft opens.
fn check_open(o: &gridcraft_plugin::dialog::OpenRequest) -> std::result::Result<(), String> {
    let path = std::path::Path::new(&o.path);
    if !path.is_absolute() {
        return Err(format!("can't open {}: the path isn't absolute", o.path));
    }
    let opens = crate::io::FileKind::from_path(&o.path).is_some_and(|k| k != crate::io::FileKind::Html);
    if !opens {
        return Err(format!("can't open {}: GridCraft doesn't open this kind of file", o.path));
    }
    match std::fs::metadata(path) {
        Ok(m) if m.is_file() => Ok(()),
        Ok(_) => Err(format!("can't open {}: it isn't a file", o.path)),
        Err(e) => Err(format!("can't open {}: {e}", o.path)),
    }
}

/// What a plugin command sees of the workbook: its title, the active sheet, the selection and,
/// for a selection up to 100,000 cells, its values (rows of values, as functions get them) and
/// formulas (rows of formula text, `null` where a cell has none); plus every sheet (index, name,
/// visibility, used range) and up to 10,000 defined names (name, scope, hidden, refersTo).
fn context(s: &Session) -> Json {
    let Some(d) = s.active() else { return json!({}) };
    let sel = &d.selection;
    let range = sel.ranges.last().copied().unwrap_or(RangeRef::cell(sel.active));
    let mut ctx = brief_context(s);
    if let Some(sh) = d.wb.active()
        && range.count() <= MAX_CONTEXT_CELLS
    {
        let rows: Vec<Json> = (range.start.row..=range.end.row)
            .map(|r| {
                Json::Array(
                    (range.start.col..=range.end.col)
                        .map(|c| gridcraft_plugin::values::to_json(&sh.value(gridcraft_core::CellRef::new(r, c))))
                        .collect(),
                )
            })
            .collect();
        ctx["values"] = Json::Array(rows);
        // Formula text (`=A1*2`) in the same shape, `null` for cells without one, so plugins can
        // tell inputs from calculations (colour coding, auditing).
        let formulas: Vec<Json> = (range.start.row..=range.end.row)
            .map(|r| {
                Json::Array(
                    (range.start.col..=range.end.col)
                        .map(|c| {
                            sh.cell(gridcraft_core::CellRef::new(r, c))
                                .and_then(|cell| cell.formula.as_ref())
                                .map_or(Json::Null, |f| Json::String(format!("={}", f.text)))
                        })
                        .collect(),
                )
            })
            .collect();
        ctx["formulas"] = Json::Array(formulas);
    }
    // The workbook's shape, as `document.inspect` reports it, so a plugin can act beyond the
    // selection (unhide every sheet, list sheets, clean up names, trim used ranges).
    let wb = &d.wb;
    ctx["activeSheet"] = json!(wb.active_sheet);
    ctx["sheets"] = Json::Array(
        wb.sheets
            .iter()
            .enumerate()
            .map(|(i, sh)| {
                json!({
                    "index": i,
                    "name": sh.name,
                    "visibility": format!("{:?}", sh.visibility),
                    "usedRange": sh.used_range().map(|r| r.a1()),
                })
            })
            .collect(),
    );
    ctx["names"] = Json::Array(
        wb.names
            .iter()
            .take(MAX_CONTEXT_NAMES)
            .map(|n| json!({"name": n.name, "scope": n.scope, "hidden": n.hidden, "refersTo": format!("={}", n.formula)}))
            .collect(),
    );
    if wb.names.len() > MAX_CONTEXT_NAMES {
        ctx["namesTruncated"] = json!(true);
    }
    ctx
}

fn run(s: &mut Session, p: &Json) -> Result<Json> {
    const ID: &str = "plugins.run";
    let plugin = str_param(p, "plugin").ok_or_else(|| bad(ID, "missing `plugin`"))?;
    let command = str_param(p, "command").ok_or_else(|| bad(ID, "missing `command`"))?;
    let params = p.get("params").cloned().unwrap_or_else(|| json!({}));
    // The plugin may read and write the workbook while it works (`host/sync`, protocol 1.1); that
    // and its answer's commands are one step (below).
    let before_host = s.active().map(|d| (d.uid, d.wb.clone(), d.selection.clone(), d.undo.len()));
    let journal_at = s.journal.len();
    let ctx = context(s);
    let reply = gridcraft_plugin::run_command_with(plugin, command, params, ctx, &mut |m, p| super::developer::host_call(s, plugin, m, p));
    s.journal.truncate(journal_at);
    if let Some((uid, wb, sel, undo_len)) = before_host
        && let Some(d) = s.docs.iter_mut().find(|d| d.uid == uid)
    {
        d.undo.truncate(undo_len);
        if reply.is_err() {
            d.wb = wb;
            d.selection = sel;
            d.calc.rebuild(&d.wb);
        }
    }
    let reply = reply.map_err(|e| bad(ID, e))?;
    // A file the plugin asks to open must be one GridCraft can open; otherwise the command fails
    // before anything changed.
    if let Some(o) = &reply.open {
        check_open(o).map_err(|e| bad(ID, format!("{plugin} › {command}: {e}")))?;
    }
    // An Office add-in's command opens its task pane or calls its function: nothing runs here.
    if let Some(office) = reply.office {
        let title = gridcraft_plugin::get(plugin).map(|p| p.manifest.name.clone()).unwrap_or_default();
        s.ui_requests.push(match &office {
            gridcraft_plugin::office::OfficeCommand::ShowTaskpane { url, taskpane_id } => {
                UiRequest::OfficePane { plugin: plugin.to_string(), title, url: url.clone(), pane: taskpane_id.clone() }
            }
            gridcraft_plugin::office::OfficeCommand::ExecuteFunction { function, url } => {
                UiRequest::OfficeFunction { plugin: plugin.to_string(), url: url.clone(), function: function.clone() }
            }
        });
        return Ok(json!({"ran": 0, "office": office}));
    }
    if let Some((id, _)) = reply.commands.iter().find(|(id, _)| id.starts_with("plugins.")) {
        return Err(bad(ID, format!("a plugin command can't run `{id}`")));
    }
    // The engine commands run as one step: their own undo and journal entries are dropped (this
    // command's entry covers them), and if one fails the workbook goes back to how it was.
    let before = s.active().map(|d| (d.uid, d.wb.clone(), d.selection.clone(), d.undo.len()));
    let journal_len = s.journal.len();
    let mut failure = None;
    for (id, params) in &reply.commands {
        if let Err(e) = s.execute(id, params.clone()) {
            failure = Some(format!("{id}: {e}"));
            break;
        }
    }
    s.journal.truncate(journal_len);
    if let Some((uid, wb, sel, undo_len)) = before
        && let Some(d) = s.docs.iter_mut().find(|d| d.uid == uid)
    {
        d.undo.truncate(undo_len);
        if failure.is_some() {
            d.wb = wb;
            d.selection = sel;
            d.calc.rebuild(&d.wb);
        }
    }
    if let Some(why) = failure {
        return Err(bad(ID, format!("{plugin} › {command} failed, nothing was changed: {why}")));
    }
    if let Some(m) = &reply.message {
        s.ui_requests.push(UiRequest::Message(m.clone()));
    }
    let mut out = json!({"ran": reply.commands.len()});
    if let Some(m) = reply.message {
        out["message"] = json!(m);
    }
    if !reply.result.is_null() {
        out["result"] = reply.result;
    }
    // Protocol 1.2: returned for the caller. The interface opens the file and shows the dialog
    // when one of its buttons ran the command; programmatic callers act on them themselves.
    if let Some(o) = &reply.open {
        out["open"] = o.to_json();
    }
    if let Some(d) = &reply.dialog {
        out["dialog"] = d.to_json();
    }
    // Protocol 2: a room to join or start, for the caller (the interface runs `collab.join`).
    if let Some(j) = &reply.collab_join {
        out["collab"] = json!({"join": j});
    }
    Ok(out)
}
