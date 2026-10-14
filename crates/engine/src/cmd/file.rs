//! File menu: new, open, save, close, export, properties, options.

use serde_json::{Value as Json, json};

use super::*;
use crate::DocState;

pub fn specs() -> Vec<CommandSpec> {
    vec![
        cmd!(noundo "file.new", "New Workbook", ["File"], Some("Cmd+N"), "{sample?: \"budget\"|\"sales\"|\"grades\"}", always, new_workbook),
        cmd!(noundo "file.open", "Open…", ["File"], Some("Cmd+O"), "{path} | {name, base64}, plus {password} for a password-protected workbook (xlsx, xlsm, xls import, xlsb and ods data import, csv, tsv, txt, json)", always, open),
        cmd!(noundo "file.save", "Save", ["File"], Some("Cmd+S"), "{path?} (xlsx by default; .csv/.tsv/.json/.html/.pdf by extension)", has_doc, save),
        cmd!(noundo "file.saveAs", "Save As…", ["File"], Some("Cmd+Shift+S"), "{path} (.xlsx, .csv, .tsv, .json, .html, or .pdf: a PDF of the active sheet)", has_doc, save_as),
        cmd!(query "file.saveBytes", "Encode Workbook", [], None, "{format?: xlsx|csv|tsv|json|html} → {base64}", has_doc, save_bytes),
        cmd!(noundo "file.close", "Close", ["File"], Some("Cmd+W"), "{force?: bool}", has_doc, close),
        cmd!(noundo "file.exportCsv", "Export as CSV", ["File", "Export"], None, "{path, sheet?}", has_doc, |s, p| export(s, p, "csv")),
        cmd!(noundo "file.exportHtml", "Save as Web Page", ["File", "Export"], None, "{path}", has_doc, |s, p| export(s, p, "html")),
        cmd!("file.properties", "Properties", ["File"], None, "{title?, subject?, author?, company?, keywords?, description?}", has_doc, properties),
        cmd!(
            query "file.options",
            "Options",
            ["File"],
            None,
            "{} → {calculation: {mode, iterative, maxIterations, maxChange}} for the active workbook (the interface opens File › Options; change settings with their own commands, e.g. formulas.calculationOptions, options.userName, view.theme, app.language.set)",
            always,
            options
        ),
        cmd!(
            query "options.userName",
            "User Name",
            ["File", "Options"],
            None,
            "{name?: string} → {name}: who new notes and comments are by and who saves workbooks (Author on a first save, Last Modified By on every save); without `name` it reports the current one",
            always,
            user_name
        ),
        cmd!(noundo "window.activate", "Switch Window", ["Window"], None, "{index}", has_doc, activate_window),
    ]
}

fn new_workbook(s: &mut Session, p: &Json) -> Result<Json> {
    let i = match str_param(p, "sample") {
        Some(name) => {
            let wb = crate::sample::build(name).ok_or_else(|| bad("file.new", format!("unknown sample `{name}`")))?;
            let title = crate::sample::title(name);
            s.add_document(DocState::new(wb, None, title))
        }
        None => s.new_workbook(),
    };
    Ok(json!({"index": i, "title": s.doc()?.display_title()}))
}

fn open(s: &mut Session, p: &Json) -> Result<Json> {
    let (name, bytes) = if let Some(b) = str_param(p, "base64") {
        (str_param(p, "name").unwrap_or("Book.xlsx").to_string(), crate::io::base64_decode(b).ok_or_else(|| bad("file.open", "invalid base64"))?)
    } else if let Some(path) = str_param(p, "path") {
        // Already open? Switch to it.
        if let Some(i) = s.documents().iter().position(|d| d.path.as_deref() == Some(path)) {
            s.set_active(i);
            return Ok(json!({"index": i, "alreadyOpen": true}));
        }
        (path.to_string(), crate::io::read_file(path)?)
    } else {
        s.ui_requests.push(crate::UiRequest::Dialog("open".into(), json!({})));
        return ok();
    };
    let encrypted = gridcraft_xlsx::sniff(&bytes) == gridcraft_xlsx::Format::Encrypted;
    let (mut wb, warnings) = crate::io::open_bytes_with_password(&name, &bytes, str_param(p, "password"))?;
    wb.source_dir = str_param(p, "path").and_then(crate::io::source_dir);
    let imported =
        matches!(crate::io::FileKind::from_path(&name), Some(crate::io::FileKind::Ods | crate::io::FileKind::Xlsb | crate::io::FileKind::Xls))
            || matches!(gridcraft_xlsx::sniff(&bytes), gridcraft_xlsx::Format::Ods | gridcraft_xlsx::Format::Xlsb | gridcraft_xlsx::Format::Xls);
    let title = if imported {
        format!("{} (imported).xlsx", std::path::Path::new(&name).file_stem().and_then(|n| n.to_str()).unwrap_or("Book"))
    } else {
        std::path::Path::new(&name).file_name().and_then(|n| n.to_str()).unwrap_or("Book").to_string()
    };
    // An import has no save target: ordinary Save must ask for a new destination. Neither does a
    // workbook decrypted with its password, so Save never replaces the encrypted original with
    // an unprotected copy.
    let path = str_param(p, "path").filter(|_| !imported && !encrypted).map(str::to_string);
    // Replace an untouched blank Book1 like Excel does.
    if s.documents().len() == 1
        && s.active().is_some_and(|d| d.path.is_none() && !d.is_dirty() && d.undo.is_empty() && d.wb.sheets.iter().all(|sh| sh.cells.is_empty()))
    {
        s.close_document(0);
    }
    let mut doc = DocState::new(wb, path, title);
    if crate::io::FileKind::from_path(&name).is_none() && gridcraft_xlsx::sniff(&bytes) == gridcraft_xlsx::Format::Xlsx {
        doc.content_kind = Some(crate::io::FileKind::Xlsx);
    }
    let i = s.add_document(doc);
    // The decrypted-workbook notice is for the info bar, not a message box.
    let shown: Vec<&str> = warnings.iter().map(String::as_str).filter(|w| *w != crate::io::DECRYPTED_NOTICE).collect();
    if !shown.is_empty() {
        let mut message = shown.iter().take(5).copied().collect::<Vec<_>>().join("\n\n");
        if shown.len() > 5 {
            message.push_str(&format!("\n\n{} additional import warnings.", shown.len() - 5));
        }
        s.ui_requests.push(crate::UiRequest::Message(message));
    }
    let d = s.doc()?;
    Ok(
        json!({"index": i, "title": d.display_title(), "sheets": d.wb.sheets.iter().map(|s| s.name.clone()).collect::<Vec<_>>(), "warnings": warnings}),
    )
}

fn save(s: &mut Session, p: &Json) -> Result<Json> {
    let path = match str_param(p, "path").map(str::to_string).or_else(|| s.active().and_then(|d| d.path.clone())) {
        Some(p) => p,
        None => {
            s.ui_requests.push(crate::UiRequest::Dialog("saveAs".into(), json!({})));
            return ok();
        }
    };
    // A PDF is a print of the sheet (Page Setup applies), not a copy of the workbook: the
    // document keeps its file and its unsaved changes.
    if std::path::Path::new(&path).extension().and_then(|e| e.to_str()).is_some_and(|e| e.eq_ignore_ascii_case("pdf")) {
        return super::print::export_pdf(s, &json!({"path": path}));
    }
    let user = s.prefs.user_name.clone();
    let d = s.doc_mut()?;
    // The document's own file keeps the format its content had when opened; any other path
    // goes by its extension. Relative paths to linked workbooks are rewritten for the new folder.
    let kind = crate::io::FileKind::from_path(&path).or(d.content_kind.filter(|_| d.path.as_deref() == Some(path.as_str())));
    let rebased = crate::io::source_dir(&path).and_then(|f| gridcraft_xlsx::rebase_links(&d.wb, &f));
    let mut wb = rebased.unwrap_or_else(|| (*d.wb).clone());
    stamp_save(&mut wb.props, &user, save_time().as_deref());
    let bytes = match kind {
        Some(kind) => crate::io::save_bytes_as(&wb, &path, kind)?,
        None => crate::io::save_bytes(&wb, &path)?,
    };
    crate::io::write_file(&path, &bytes)?;
    // Only a full-fidelity format clears the dirty flag and becomes the document's file, and the
    // workbook keeps the properties that file now holds.
    if matches!(kind, Some(crate::io::FileKind::Xlsx | crate::io::FileKind::Json)) {
        std::sync::Arc::make_mut(&mut d.wb).props = wb.props;
        d.saved = d.wb.clone();
        d.path = Some(path.clone());
    }
    Ok(json!({"path": path, "bytes": bytes.len()}))
}

/// What a save records in the document properties, as Excel does: who saved the workbook and
/// when, and on its first save (no creation date yet) when it was created and, if it names none,
/// its author. `now` is `None` only where no clock can be read; the dates then stay as they are.
fn stamp_save(props: &mut gridcraft_model::DocProps, user: &str, now: Option<&str>) {
    if props.created.is_empty() {
        if props.author.is_empty() {
            props.author = user.to_string();
        }
        if let Some(now) = now {
            props.created = now.to_string();
        }
    }
    props.last_modified_by = user.to_string();
    if let Some(now) = now {
        props.modified = now.to_string();
    }
}

/// The current time as W3CDTF in UTC (`2026-10-14T13:45:00Z`), from the system clock (the
/// browser's on wasm).
fn save_time() -> Option<String> {
    let ms = gridcraft_core::date::unix_now_ms()?;
    if !ms.is_finite() {
        return None;
    }
    Some(w3cdtf(ms))
}

fn w3cdtf(unix_ms: f64) -> String {
    let secs = (unix_ms / 1000.0).floor() as i64;
    let (y, m, d) = gridcraft_core::date::civil_from_days(secs.div_euclid(86_400));
    let t = secs.rem_euclid(86_400);
    format!("{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}Z", t / 3600, t % 3600 / 60, t % 60)
}

fn save_as(s: &mut Session, p: &Json) -> Result<Json> {
    if str_param(p, "path").is_none() {
        s.ui_requests.push(crate::UiRequest::Dialog("saveAs".into(), json!({})));
        return ok();
    }
    save(s, p)
}

fn save_bytes(s: &mut Session, p: &Json) -> Result<Json> {
    let fmt = str_param(p, "format").unwrap_or("xlsx");
    let user = s.prefs.user_name.clone();
    let d = s.doc()?;
    // The encoded copy says who saved it and when; the document itself is left as it is (this
    // is a download, not the document's file).
    let mut wb = (*d.wb).clone();
    stamp_save(&mut wb.props, &user, save_time().as_deref());
    let bytes = crate::io::save_bytes(&wb, &format!("x.{fmt}"))?;
    let name = format!("{}.{fmt}", std::path::Path::new(&d.display_title()).file_stem().and_then(|x| x.to_str()).unwrap_or("Book"));
    Ok(json!({"name": name, "base64": crate::io::base64_encode(&bytes), "bytes": bytes.len()}))
}

fn close(s: &mut Session, p: &Json) -> Result<Json> {
    let force = bool_param(p, "force").unwrap_or(false);
    let d = s.doc()?;
    if d.is_dirty() && !force {
        s.ui_requests.push(crate::UiRequest::Dialog("saveChanges".into(), json!({"title": d.display_title()})));
        return Ok(json!({"closed": false, "dirty": true}));
    }
    let i = s.active_index();
    s.close_document(i);
    Ok(json!({"closed": true}))
}

fn export(s: &mut Session, p: &Json, ext: &str) -> Result<Json> {
    let path = str_param(p, "path").ok_or_else(|| bad("file.export", "missing `path`"))?.to_string();
    let d = s.doc()?;
    let mut wb = (*d.wb).clone();
    if p.get("sheet").is_some() {
        wb.active_sheet = target_sheet(s, p)?;
    }
    let target = if path.ends_with(&format!(".{ext}")) { path.clone() } else { format!("{path}.{ext}") };
    let bytes = crate::io::save_bytes(&wb, &target)?;
    crate::io::write_file(&target, &bytes)?;
    Ok(json!({"path": target, "bytes": bytes.len()}))
}

fn properties(s: &mut Session, p: &Json) -> Result<Json> {
    edit(s, |cx| {
        let pr = &mut cx.wb.props;
        for (k, slot) in [
            ("title", &mut pr.title),
            ("subject", &mut pr.subject),
            ("author", &mut pr.author),
            ("company", &mut pr.company),
            ("keywords", &mut pr.keywords),
            ("description", &mut pr.description),
        ] {
            if let Some(v) = str_param(p, k) {
                *slot = v.to_string();
            }
        }
        Ok(serde_json::to_value(&cx.wb.props).unwrap_or_default())
    })
}

/// File › Options: reports the workbook-level settings and asks the interface to show its Options
/// page. Nothing changes here; every setting has its own command.
fn options(s: &mut Session, _: &Json) -> Result<Json> {
    s.ui_requests.push(crate::UiRequest::Dialog("options".into(), json!({})));
    let calculation = s.active().map(|d| {
        let c = &d.wb.calc;
        let mode = match c.mode {
            gridcraft_model::CalcMode::Automatic => "automatic",
            gridcraft_model::CalcMode::AutomaticExceptTables => "automaticExceptTables",
            gridcraft_model::CalcMode::Manual => "manual",
        };
        json!({"mode": mode, "iterative": c.iterative, "maxIterations": c.max_iterations, "maxChange": c.max_change})
    });
    Ok(json!({"calculation": calculation}))
}

/// The longest user name kept, in characters (the web host caps `?author=` the same way).
const MAX_USER_NAME_CHARS: usize = 200;

/// File › Options › General › User name. Control characters become spaces and leading and
/// trailing spaces are dropped; a blank name is refused, since notes, comments and saves always
/// name someone.
fn user_name(s: &mut Session, p: &Json) -> Result<Json> {
    match p.get("name") {
        None | Some(Json::Null) => {}
        Some(Json::String(name)) => {
            let name: String =
                name.chars().map(|c| if c.is_control() { ' ' } else { c }).collect::<String>().trim().chars().take(MAX_USER_NAME_CHARS).collect();
            if name.is_empty() {
                return Err(bad("options.userName", "the user name can't be blank"));
            }
            s.prefs.user_name = name;
        }
        Some(_) => return Err(bad("options.userName", "`name` must be text")),
    }
    Ok(json!({"name": s.prefs.user_name}))
}

fn activate_window(s: &mut Session, p: &Json) -> Result<Json> {
    let i = u32_param(p, "index").ok_or_else(|| bad("window.activate", "missing `index`"))? as usize;
    if i >= s.documents().len() {
        return Err(bad("window.activate", "no such window"));
    }
    s.set_active(i);
    ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn save_times_are_w3cdtf_in_utc() {
        assert_eq!(w3cdtf(0.0), "1970-01-01T00:00:00Z");
        assert_eq!(w3cdtf(1_760_449_500_999.0), "2025-10-14T13:45:00Z");
        assert_eq!(w3cdtf(951_825_600_000.0), "2000-02-29T12:00:00Z");
        assert_eq!(w3cdtf(-1_000.0), "1969-12-31T23:59:59Z");
        assert!(save_time().is_some_and(|t| gridcraft_xlsx::is_w3cdtf(&t)));
    }

    #[test]
    fn a_first_save_names_the_author_and_a_later_one_only_who_saved() {
        let mut p = gridcraft_model::DocProps::default();
        stamp_save(&mut p, "Ann", Some("2026-01-02T03:04:05Z"));
        assert_eq!((p.author.as_str(), p.last_modified_by.as_str()), ("Ann", "Ann"));
        assert_eq!((p.created.as_str(), p.modified.as_str()), ("2026-01-02T03:04:05Z", "2026-01-02T03:04:05Z"));
        stamp_save(&mut p, "Bob", Some("2026-02-03T04:05:06Z"));
        assert_eq!((p.author.as_str(), p.created.as_str()), ("Ann", "2026-01-02T03:04:05Z"));
        assert_eq!((p.last_modified_by.as_str(), p.modified.as_str()), ("Bob", "2026-02-03T04:05:06Z"));
        // Without a clock the dates stay as they are; who saved is still recorded.
        stamp_save(&mut p, "Cy", None);
        assert_eq!((p.last_modified_by.as_str(), p.modified.as_str()), ("Cy", "2026-02-03T04:05:06Z"));
        let mut q = gridcraft_model::DocProps { author: "Dee".into(), ..Default::default() };
        stamp_save(&mut q, "Ann", None);
        assert_eq!((q.author.as_str(), q.created.as_str()), ("Dee", ""));
    }
}
