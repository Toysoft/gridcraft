//! Automate tab › Scripting Tools: record, write, run and manage the workbook's scripts.
//!
//! Scripts are Rhai code run by [`crate::script`] (an object model shaped like Office Scripts,
//! plus `run(id, params)` for any command). They are stored in the workbook
//! ([`gridcraft_model::Workbook::scripts`]) and saved with it. Command lists recorded by earlier
//! versions still run (they are turned into the equivalent Rhai on the fly) and become Rhai when
//! they are edited and saved.
//!
//! A workbook opened from a file that holds scripts doesn't run them until the user enables
//! content once for that opened document (`automate.enableScripts`, Excel's Enable Content).

use gridcraft_model::{Script, ScriptLanguage, Workbook};
use serde_json::{Value as Json, json};

use super::*;
use crate::script;

pub fn specs() -> Vec<CommandSpec> {
    vec![
        cmd!(
            noundo "automate.recordActions",
            "Record Actions",
            ["Automate", "Scripting Tools"],
            None,
            "{stop?: bool, name?: script name when stopping} → the first call starts recording; the next (or stop: true) saves the commands run since as a Rhai script → {recording, saved?, steps?}",
            has_doc,
            record
        ),
        cmd!(
            "automate.newScript",
            "New Script",
            ["Automate", "Scripting Tools"],
            None,
            "{name?, code?: Rhai source (default: an empty `fn main(workbook)`), commands?: [{command, params}] (a command list, as older versions recorded)} → {name}; replaces a script of that name; with no params the interface opens the Code Editor",
            has_doc,
            new_script
        ),
        cmd!(
            query "automate.allScripts",
            "All Scripts",
            ["Automate", "Scripting Tools"],
            None,
            "{} → [{name, kind: rhai|commands|vba, lines|steps}] (the interface shows them in the Code Editor)",
            has_doc,
            all_scripts
        ),
        // Not journaled: the commands the script runs are, so replaying the journal (or a
        // recording made meanwhile) repeats the run's effect once.
        CommandSpec {
            id: "automate.runScript",
            label: "Run Script",
            menu: &[],
            shortcut: None,
            params: "{name} | {code, name?} → {name, output: [lines], result}: runs a saved script or Rhai source; all its changes are one undo step; a failing script is an error (with line, column and output)",
            enabled: has_doc,
            run: run_script,
            journal: false,
            undoable: true,
        },
        cmd!(query "automate.getScript", "Script Code", [], None, "{name} → {name, kind, code} (a command list comes back as the Rhai it becomes when saved)", has_doc, get_script),
        cmd!(
            "automate.saveScript",
            "Save Script",
            [],
            None,
            "{name, code} → {name, syntaxError?}: creates or replaces the Rhai script (a command list becomes Rhai); code that doesn't parse is saved and reported",
            has_doc,
            save_script
        ),
        cmd!("automate.renameScript", "Rename Script", [], None, "{name, newName}", has_doc, rename_script),
        cmd!("automate.deleteScript", "Delete Script", [], None, "{name}", has_doc, delete_script),
        cmd!(
            noundo "automate.enableScripts",
            "Enable Scripts",
            [],
            None,
            "{} → {enabled}: lets the scripts of a workbook opened from a file run in this session (Enable Content)",
            has_doc,
            enable_scripts
        ),
        cmd!(query "automate.lastRun", "Script Output", [], None, "{} → the last run's {name, output, error?}", always, last_run),
    ]
}

/// The hidden defined name older versions kept recorded scripts in (JSON in a string).
const LEGACY_NAME: &str = "_sc_scripts";
/// Longest script name.
const MAX_NAME: usize = 128;
/// Most steps of a command-list script that run.
const MAX_STEPS: usize = 100_000;
/// Most scripts a workbook keeps (as many as the XLSX reader reads back).
const MAX_SCRIPTS: usize = 10_000;

/// The skeleton of a new script.
pub const TEMPLATE: &str = "fn main(workbook) {\n    // Your script starts here, for example:\n    // let sheet = workbook.getActiveWorksheet();\n    // sheet.getRange(\"A1\").setValue(\"Hello\");\n}\n";

/// Moves scripts saved by earlier versions (in a hidden defined name) into the workbook's script
/// list as command-list scripts.
pub(crate) fn migrate_legacy(wb: &mut Workbook) {
    if !wb.names.iter().any(|n| n.name == LEGACY_NAME) {
        return;
    }
    let all = super::extra::stash_get(wb, LEGACY_NAME);
    wb.names.retain(|n| n.name != LEGACY_NAME);
    let Some(o) = all.as_object() else { return };
    for (name, steps) in o {
        let name: String = name.trim().chars().take(MAX_NAME).collect();
        if !steps.is_array() || name.is_empty() || wb.script(&name).is_some() || wb.scripts.len() >= MAX_SCRIPTS {
            continue;
        }
        wb.scripts.push(Script { name, language: ScriptLanguage::Commands, code: steps.to_string() });
    }
}

/// A script name as given, checked: not blank, not too long, no control characters.
fn check_name(cmd: &str, name: &str) -> Result<String> {
    let name = name.trim();
    if name.is_empty() {
        return Err(bad(cmd, "a script name can't be blank"));
    }
    if name.chars().count() > MAX_NAME {
        return Err(bad(cmd, format!("a script name can't be longer than {MAX_NAME} characters")));
    }
    if name.chars().any(char::is_control) {
        return Err(bad(cmd, "a script name can't contain control characters"));
    }
    Ok(name.to_string())
}

/// `Script N` with the lowest N not in use.
fn next_name(wb: &Workbook) -> String {
    (1..=100_000).map(|n| format!("Script {n}")).find(|n| wb.script(n).is_none()).unwrap_or_else(|| "Script".into())
}

/// Adds the script, or replaces the one with that name (keeping its place).
fn put(wb: &mut Workbook, script: Script) {
    match wb.scripts.iter_mut().find(|s| s.name.eq_ignore_ascii_case(&script.name)) {
        Some(s) => *s = script,
        None => wb.scripts.push(script),
    }
}

/// The steps of a command-list script (`[{command, params}]`), at most [`MAX_STEPS`].
fn steps_of(code: &str) -> Vec<(String, Json)> {
    let v: Json = serde_json::from_str(code).unwrap_or(Json::Null);
    v.as_array()
        .map(|a| {
            a.iter()
                .take(MAX_STEPS)
                .filter_map(|st| Some((st.get("command")?.as_str()?.to_string(), st.get("params").cloned().unwrap_or_else(|| json!({})))))
                .filter(|(id, _)| !id.starts_with("automate."))
                .collect()
        })
        .unwrap_or_default()
}

/// The Rhai a script runs as: its code, or its command list turned into Rhai.
fn rhai_of(s: &Script) -> String {
    match s.language {
        ScriptLanguage::Rhai => s.code.clone(),
        ScriptLanguage::Commands => script::record::commands_to_rhai(&steps_of(&s.code), "Converted from a recorded command list."),
    }
}

fn record(s: &mut Session, p: &Json) -> Result<Json> {
    if bool_param(p, "stop").unwrap_or(false) || s.recording.is_some() {
        let Some(start) = s.recording.take() else { return Ok(json!({"recording": false})) };
        let steps: Vec<(String, Json)> = s.journal.iter().skip(start).filter(|(id, _)| !id.starts_with("automate.")).cloned().collect();
        let wb = &s.doc()?.wb;
        let name = match str_param(p, "name") {
            Some(n) => check_name("automate.recordActions", n)?,
            None => next_name(wb),
        };
        let code = script::record::commands_to_rhai(&steps, "Recorded with Record Actions.");
        edit(s, |cx| {
            put(&mut cx.wb, Script { name: name.clone(), language: ScriptLanguage::Rhai, code });
            Ok(())
        })?;
        return Ok(json!({"recording": false, "saved": name, "steps": steps.len()}));
    }
    s.recording = Some(s.journal.len());
    Ok(json!({"recording": true}))
}

fn new_script(s: &mut Session, p: &Json) -> Result<Json> {
    let menu = p.as_object().is_none_or(|o| o.is_empty());
    let name = match str_param(p, "name") {
        Some(n) => check_name("automate.newScript", n)?,
        None => next_name(&s.doc()?.wb),
    };
    let script = match (str_param(p, "code"), p.get("commands")) {
        (Some(code), _) => {
            if code.len() > script::MAX_CODE {
                return Err(bad("automate.newScript", "the script is too long"));
            }
            Script { name: name.clone(), language: ScriptLanguage::Rhai, code: code.to_string() }
        }
        (None, Some(cmds)) => {
            if !cmds.is_array() {
                return Err(bad("automate.newScript", "`commands` must be an array"));
            }
            Script { name: name.clone(), language: ScriptLanguage::Commands, code: cmds.to_string() }
        }
        (None, None) => Script { name: name.clone(), language: ScriptLanguage::Rhai, code: TEMPLATE.into() },
    };
    edit(s, |cx| {
        put(&mut cx.wb, script);
        Ok(())
    })?;
    if menu {
        s.ui_requests.push(crate::UiRequest::Dialog("codeEditor".into(), json!({"script": name})));
    }
    Ok(json!({"name": name}))
}

pub(crate) fn all_scripts(s: &mut Session, _: &Json) -> Result<Json> {
    let wb = &s.doc()?.wb;
    let mut list: Vec<Json> = wb
        .scripts
        .iter()
        .map(|sc| match sc.language {
            ScriptLanguage::Rhai => json!({"name": sc.name, "kind": "rhai", "lines": sc.code.lines().count()}),
            ScriptLanguage::Commands => json!({"name": sc.name, "kind": "commands", "steps": steps_of(&sc.code).len()}),
        })
        .collect();
    // A preserved VBA project is reported alongside GridCraft's own scripts; it is carried on
    // save but never executed here, so it has no step count.
    if let Some(bytes) = &wb.vba_project {
        list.push(json!({"name": "(VBA)", "steps": 0, "kind": "vba", "size": bytes.len(), "executed": false}));
    }
    Ok(Json::Array(list))
}

fn run_script(s: &mut Session, p: &Json) -> Result<Json> {
    let d = s.doc()?;
    let (name, code) = match (str_param(p, "code"), str_param(p, "name")) {
        (Some(code), name) => (name.unwrap_or("Script").to_string(), code.to_string()),
        (None, Some(name)) => {
            let sc = d.wb.script(name).ok_or_else(|| bad("automate.runScript", format!("there is no script named \"{name}\"")))?;
            if d.scripts_blocked {
                return Err(EngineError::Other(script::BLOCKED.into()));
            }
            (sc.name.clone(), rhai_of(sc))
        }
        (None, None) => return Err(bad("automate.runScript", "missing `name` (or `code`)")),
    };
    let (run, value) = script::run(s, &name, &code);
    s.last_script = Some(run.clone());
    match &run.error {
        None => Ok(json!({"name": run.name, "output": run.output, "result": value})),
        Some(e) => {
            let mut msg = format!("Script \"{}\" stopped. {}", run.name, e.describe());
            if !run.output.is_empty() {
                let tail = run.output.len().saturating_sub(20);
                msg.push_str("\nOutput:");
                for l in run.output.iter().skip(tail) {
                    msg.push('\n');
                    msg.push_str(l);
                }
            }
            Err(EngineError::Other(msg))
        }
    }
}

fn get_script(s: &mut Session, p: &Json) -> Result<Json> {
    let name = str_param(p, "name").ok_or_else(|| bad("automate.getScript", "missing `name`"))?;
    let sc = s.doc()?.wb.script(name).ok_or_else(|| bad("automate.getScript", format!("there is no script named \"{name}\"")))?;
    let kind = match sc.language {
        ScriptLanguage::Rhai => "rhai",
        ScriptLanguage::Commands => "commands",
    };
    Ok(json!({"name": sc.name, "kind": kind, "code": rhai_of(sc)}))
}

fn save_script(s: &mut Session, p: &Json) -> Result<Json> {
    let name = check_name("automate.saveScript", str_param(p, "name").ok_or_else(|| bad("automate.saveScript", "missing `name`"))?)?;
    let code = str_param(p, "code").ok_or_else(|| bad("automate.saveScript", "missing `code`"))?.to_string();
    if code.len() > script::MAX_CODE {
        return Err(bad("automate.saveScript", "the script is too long"));
    }
    let syntax = script::check(&code);
    edit(s, |cx| {
        put(&mut cx.wb, Script { name: name.clone(), language: ScriptLanguage::Rhai, code });
        Ok(())
    })?;
    let mut out = json!({"name": name});
    if let Some(e) = syntax {
        out["syntaxError"] = json!(e);
    }
    Ok(out)
}

fn rename_script(s: &mut Session, p: &Json) -> Result<Json> {
    let name = str_param(p, "name").ok_or_else(|| bad("automate.renameScript", "missing `name`"))?.to_string();
    let new = check_name("automate.renameScript", str_param(p, "newName").ok_or_else(|| bad("automate.renameScript", "missing `newName`"))?)?;
    let wb = &s.doc()?.wb;
    if wb.script(&name).is_none() {
        return Err(bad("automate.renameScript", format!("there is no script named \"{name}\"")));
    }
    if !new.eq_ignore_ascii_case(&name) && wb.script(&new).is_some() {
        return Err(EngineError::Other(format!("There is already a script named \"{new}\".")));
    }
    edit(s, |cx| {
        if let Some(sc) = cx.wb.scripts.iter_mut().find(|sc| sc.name.eq_ignore_ascii_case(&name)) {
            sc.name = new.clone();
        }
        Ok(())
    })?;
    Ok(json!({"name": new}))
}

fn delete_script(s: &mut Session, p: &Json) -> Result<Json> {
    let name = str_param(p, "name").ok_or_else(|| bad("automate.deleteScript", "missing `name`"))?.to_string();
    if s.doc()?.wb.script(&name).is_none() {
        return Err(bad("automate.deleteScript", format!("there is no script named \"{name}\"")));
    }
    edit(s, |cx| {
        cx.wb.scripts.retain(|sc| !sc.name.eq_ignore_ascii_case(&name));
        Ok(Json::Null)
    })
}

fn enable_scripts(s: &mut Session, _: &Json) -> Result<Json> {
    s.doc_mut()?.scripts_blocked = false;
    Ok(json!({"enabled": true}))
}

fn last_run(s: &mut Session, _: &Json) -> Result<Json> {
    Ok(s.last_script.as_ref().map(|r| json!(r)).unwrap_or(Json::Null))
}
