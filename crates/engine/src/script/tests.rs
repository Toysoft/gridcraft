use serde_json::json;

use crate::Session;

fn book() -> Session {
    let mut s = Session::new();
    s.new_workbook();
    s
}

/// Runs inline Rhai code; panics with the error when it fails.
fn run(s: &mut Session, code: &str) -> serde_json::Value {
    s.execute("automate.runScript", json!({"code": code})).unwrap_or_else(|e| panic!("{e}"))
}

fn fails(s: &mut Session, code: &str) -> String {
    match s.execute("automate.runScript", json!({"code": code})) {
        Ok(v) => panic!("expected an error, got {v}"),
        Err(e) => e.to_string(),
    }
}

fn value(s: &Session, a1: &str) -> String {
    let d = s.doc().unwrap();
    d.wb.active().unwrap().value(gridcraft_core::CellRef::parse(a1).unwrap()).display()
}

#[test]
fn object_model_reads_and_writes_values_and_formulas() {
    let mut s = book();
    let out = run(
        &mut s,
        r#"
        fn main(workbook) {
            let sheet = workbook.getActiveWorksheet();
            let r = sheet.getRange("A1:B3");
            r.setValues([[1, 2], [3, 4.5], ["x", true]]);
            sheet.getRange("C1").setFormula("=A1+B1");
            let v = r.getValues();
            console.log(v[1][1], sheet.getRange("C1").getValue(), sheet.getRange("C1").getFormula());
            print(r.getAddress() + " " + r.getRowCount() + "x" + r.getColumnCount());
            print(sheet.getRange("A3").getText() + "|" + sheet.getRange("D9").getValue() + "|");
            let next = r.getOffsetRange(1, 2).getResizedRange(-1, 0);
            print(next.getAddress() + " " + r.getCell(2, 1).getAddress() + " " + r.getLastCell().getAddress());
            print(sheet.getUsedRange().getAddress());
            v[0][0] + v[0][1]
        }
        "#,
    );
    assert_eq!(value(&s, "C1"), "3");
    assert_eq!(out["result"], json!(3));
    assert_eq!(out["output"], json!(["4.5 3 =A1+B1", "Sheet1!A1:B3 3x2", "x||", "Sheet1!C2:D3 Sheet1!B3 Sheet1!B3", "Sheet1!A1:C3"]), "{out}");
}

#[test]
fn setvalue_fills_every_cell_and_text_is_parsed_like_typing() {
    let mut s = book();
    run(
        &mut s,
        r#"let r = workbook.getActiveWorksheet().getRange("A1:B2"); r.setValue("5%"); workbook.getActiveWorksheet().getRange("C1").setValue(());"#,
    );
    for a in ["A1", "A2", "B1", "B2"] {
        assert_eq!(value(&s, a), "0.05");
    }
    // A shape that doesn't match the range is refused (as in Office Scripts).
    let e = fails(&mut s, r#"workbook.getActiveWorksheet().getRange("A1:B2").setValues([[1, 2]]);"#);
    assert!(e.contains("2 row(s)"), "{e}");
}

#[test]
fn formatting_goes_through_commands() {
    let mut s = book();
    let out = run(
        &mut s,
        r##"
        fn main(workbook) {
            let r = workbook.getActiveWorksheet().getRange("A1:C1");
            let f = r.getFormat();
            f.getFont().setBold(true);
            f.getFont().setItalic(true);
            f.getFont().setColor("#C00000");
            f.getFont().setSize(14);
            f.getFill().setColor("#FFFF00");
            f.setHorizontalAlignment("Center");
            r.setNumberFormat("0.00");
            print(`${f.getFont().getBold()} ${f.getFont().getItalic()} ${f.getFont().getColor()} ${f.getFont().getSize()}`);
            print(`${f.getFill().getColor()} ${f.getHorizontalAlignment()} ${r.getNumberFormat()}`);
            f.getFill().clear();
            print(f.getFill().getColor() == "");
        }
        "##,
    );
    assert_eq!(out["output"], json!(["true true #C00000 14", "#FFFF00 Center 0.00", "true"]), "{out}");
    let d = s.doc().unwrap();
    let sh = d.wb.active().unwrap();
    let st = d.wb.styles.get(sh.style_id(gridcraft_core::CellRef::parse("C1").unwrap()));
    assert!(st.font.bold && st.font.italic && st.font.size == 14.0);
}

#[test]
fn worksheets_names_and_run() {
    let mut s = book();
    let out = run(
        &mut s,
        r#"
        fn main(workbook) {
            let added = workbook.addWorksheet("Data");
            added.getRange("B2").setValue(42);
            print(workbook.getActiveWorksheet().getName());
            print(workbook.getWorksheets().len());
            print(workbook.getWorksheet("nope") == ());
            added.setName("Numbers");
            print(workbook.getWorksheet("Numbers").getRange("B2").getValue());
            workbook.addNamedItem("Answer", "=Numbers!$B$2");
            print(workbook.getNamedItem("Answer").getRange().getAddress());
            run("home.bold", #{range: "A1:C1"});
            let r = run("cell.get", #{cell: "A1"});
            print(type_of(r));
        }
        "#,
    );
    assert_eq!(out["output"], json!(["Sheet1", "2", "true", "42", "Numbers!B2", "map"]), "{out}");
    // The new sheet went to the end and the active sheet stayed.
    let d = s.doc().unwrap();
    assert_eq!(d.wb.active().unwrap().name, "Sheet1");
    assert_eq!(d.wb.sheets.last().unwrap().name, "Numbers");
}

#[test]
fn a_script_run_is_one_undo_step() {
    let mut s = book();
    s.execute("cell.set", json!({"cell": "A1", "input": "before"})).unwrap();
    let undo = s.doc().unwrap().undo.len();
    run(
        &mut s,
        r#"let sh = workbook.getActiveWorksheet();
           for i in 0..5 { sh.getCell(i, 1).setValue(i * 10); }
           sh.getRange("A1").setValue("after");
           sh.getRange("B1:B5").getFormat().getFont().setBold(true);"#,
    );
    assert_eq!(s.doc().unwrap().undo.len(), undo + 1);
    assert_eq!(s.doc().unwrap().undo.last().unwrap().label, "Run Script");
    assert_eq!(value(&s, "B5"), "40");
    s.execute("edit.undo", json!({})).unwrap();
    assert_eq!(value(&s, "A1"), "before");
    assert_eq!(value(&s, "B5"), "");
    s.execute("edit.redo", json!({})).unwrap();
    assert_eq!(value(&s, "B5"), "40");
    // A failing run keeps what it did before the error, still as one step.
    let undo = s.doc().unwrap().undo.len();
    let e =
        fails(&mut s, "let sh = workbook.getActiveWorksheet(); sh.getRange(\"C1\").setValue(1); sh.getRange(\"C2\").setValue(2); throw \"stop\";");
    assert!(e.contains("stop"), "{e}");
    assert_eq!(value(&s, "C2"), "2");
    assert_eq!(s.doc().unwrap().undo.len(), undo + 1);
    s.execute("edit.undo", json!({})).unwrap();
    assert_eq!(value(&s, "C1"), "");
    // The commands a script ran are journaled; the run itself is not (replay repeats it once).
    assert!(s.journal.iter().any(|(id, _)| id == "range.setValues"));
    assert!(!s.journal.iter().any(|(id, _)| id == "automate.runScript"));
}

#[test]
fn errors_name_line_and_column_and_keep_the_output() {
    let mut s = book();
    let e = fails(&mut s, "fn main(workbook) {\n    print(\"one\");\n    let r = workbook.getActiveWorksheet().getRange(\"not a range\");\n}\n");
    assert!(e.contains("Line 3"), "{e}");
    assert!(e.contains("not a cell or range address"), "{e}");
    assert!(e.contains("Output:\none"), "{e}");
    let last = s.last_script.clone().unwrap();
    assert_eq!(last.output, ["one"]);
    assert_eq!(last.error.as_ref().and_then(|e| e.line), Some(3));
    // Syntax errors too.
    let e = fails(&mut s, "let x = ;");
    assert!(e.contains("Line 1"), "{e}");
    assert!(super::check("let x = ;").is_some_and(|e| e.line == Some(1)));
    assert!(super::check("fn main(workbook) { }").is_none());
}

#[test]
fn runaway_scripts_stop_with_an_error() {
    let mut s = book();
    // Smaller limits than the defaults keep the test quick; the mechanisms are the same.
    s.script_limits = super::Limits { operations: 2_000_000, seconds: 2, array_size: 20_000, map_size: 20_000, ..super::LIMITS };
    let e = fails(&mut s, "loop { }");
    assert!(e.contains("ran too long"), "{e}");
    let e = fails(&mut s, "fn f(n) { f(n + 1) } f(0);");
    assert!(e.contains("nested too deeply"), "{e}");
    let e = fails(&mut s, "let t = \"x\"; loop { t += t; }");
    assert!(e.contains("larger than scripts allow"), "{e}");
    let e = fails(&mut s, "let a = []; loop { a.push(1); }");
    assert!(e.contains("larger than scripts allow") || e.contains("ran too long"), "{e}");
    // Deeply nested arrays stay within the array limit and don't exhaust the stack.
    let e = fails(&mut s, "let a = []; loop { a = [a]; }");
    assert!(e.contains("larger than scripts allow") || e.contains("ran too long"), "{e}");
    let e = fails(&mut s, "let m = #{}; loop { m = #{m: m}; }");
    assert!(e.contains("larger than scripts allow") || e.contains("ran too long"), "{e}");
    // The session is whole afterwards.
    run(&mut s, r#"workbook.getActiveWorksheet().getRange("A1").setValue(1);"#);
    assert_eq!(value(&s, "A1"), "1");
}

#[test]
fn scripts_cant_touch_files_or_start_scripts() {
    let mut s = book();
    let dir = std::env::temp_dir().join(format!("gridcraft-script-sandbox-{}", std::process::id()));
    let path = dir.join("out.csv");
    let p = path.to_string_lossy().replace('\\', "/");
    for code in [
        format!(r#"run("file.saveAs", #{{path: "{p}"}});"#),
        format!(r#"run("file.exportCsv", #{{path: "{p}"}});"#),
        format!(r#"run("data.getData", #{{path: "{p}"}});"#),
        "run(\"file.save\");".to_string(),
        "run(\"file.open\");".to_string(),
        "run(\"automate.runScript\", #{code: \"1\"});".to_string(),
        "run(\"automate.enableScripts\");".to_string(),
    ] {
        let e = fails(&mut s, &code);
        assert!(e.contains("may not") || e.contains("can't run"), "{code}: {e}");
    }
    assert!(!path.exists());
    // No modules, no eval.
    assert!(fails(&mut s, "import \"x\" as y;").contains("Line 1"));
    assert!(!fails(&mut s, "eval(\"1\")").is_empty());
    // Harmless file.* commands are fine.
    run(&mut s, "let b = run(\"file.saveBytes\"); print(type_of(b));");
    // The CLI's --allow-files lifts the file rule (not the automate one).
    s.script_file_access = true;
    std::fs::create_dir_all(&dir).unwrap();
    run(&mut s, &format!(r#"run("file.exportCsv", #{{path: "{p}"}});"#));
    assert!(path.exists());
    assert!(fails(&mut s, "run(\"automate.allScripts\");").contains("can't run"));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn saved_scripts_run_by_name_and_round_trip_through_xlsx() {
    let mut s = book();
    let code = "fn main(workbook) {\n    workbook.getActiveWorksheet().getRange(\"A1\").setValue(\"hi\");\n}\n";
    s.execute("automate.saveScript", json!({"name": "Greet", "code": code})).unwrap();
    s.execute("automate.newScript", json!({"name": "Old", "commands": [{"command": "cell.set", "params": {"cell": "B1", "input": "7"}}]})).unwrap();
    s.execute("automate.runScript", json!({"name": "greet"})).unwrap();
    s.execute("automate.runScript", json!({"name": "Old"})).unwrap();
    assert_eq!(value(&s, "A1"), "hi");
    assert_eq!(value(&s, "B1"), "7");
    let list = s.execute("automate.allScripts", json!({})).unwrap();
    assert_eq!(list, json!([{"name": "Greet", "kind": "rhai", "lines": 3}, {"name": "Old", "kind": "commands", "steps": 1}]));
    // Saved with the file, read back, and held back until content is enabled.
    let bytes = crate::io::save_bytes(&s.doc().unwrap().wb, "book.xlsx").unwrap();
    let dir = std::env::temp_dir().join(format!("gridcraft-script-xlsx-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("scripts.xlsx");
    std::fs::write(&path, &bytes).unwrap();
    let mut o = Session::new();
    o.execute("file.open", json!({"path": path.to_string_lossy()})).unwrap();
    let wb = &o.doc().unwrap().wb;
    assert_eq!(wb.scripts, s.doc().unwrap().wb.scripts);
    assert!(o.doc().unwrap().scripts_blocked);
    let e = o.execute("automate.runScript", json!({"name": "Greet"})).unwrap_err().to_string();
    assert!(e.contains("Enable Content"), "{e}");
    // Code the caller supplies isn't the file's: it runs.
    o.execute("automate.runScript", json!({"code": "1 + 1"})).unwrap();
    o.execute("automate.enableScripts", json!({})).unwrap();
    o.execute("automate.runScript", json!({"name": "Greet"})).unwrap();
    assert_eq!(value(&o, "A1"), "hi");
    // A new workbook's own scripts run without asking.
    assert!(!s.doc().unwrap().scripts_blocked);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn command_lists_migrate_to_rhai_when_saved() {
    let mut s = book();
    let steps = json!([
        {"command": "cell.set", "params": {"cell": "A1", "input": "Total"}},
        {"command": "home.bold", "params": {"range": "A1", "on": true}},
        {"command": "home.fillColor", "params": {"range": "A1", "color": "#FFFF00"}},
        {"command": "automate.runScript", "params": {"name": "x"}},
    ]);
    s.execute("automate.newScript", json!({"name": "Old", "commands": steps})).unwrap();
    let got = s.execute("automate.getScript", json!({"name": "Old"})).unwrap();
    assert_eq!(got["kind"], "commands");
    let code = got["code"].as_str().unwrap().to_string();
    assert!(code.contains("sheet.getRange(\"A1\").setValue(\"Total\");"), "{code}");
    assert!(code.contains("getFont().setBold(true)"), "{code}");
    assert!(!code.contains("automate."), "{code}");
    s.execute("automate.saveScript", json!({"name": "Old", "code": code})).unwrap();
    let list = s.execute("automate.allScripts", json!({})).unwrap();
    assert_eq!(list[0]["kind"], "rhai");
    let mut fresh = book();
    fresh.execute("automate.saveScript", json!({"name": "Old", "code": code})).unwrap();
    fresh.execute("automate.runScript", json!({"name": "Old"})).unwrap();
    assert_eq!(value(&fresh, "A1"), "Total");
}

#[test]
fn recorded_actions_become_a_runnable_rhai_script() {
    let mut s = book();
    s.execute("automate.recordActions", json!({})).unwrap();
    s.execute("cell.set", json!({"cell": "A1", "input": "Name"})).unwrap();
    s.execute("cell.set", json!({"cell": "B1", "input": "12.5"})).unwrap();
    s.execute("cell.set", json!({"cell": "C1", "input": "=B1*2"})).unwrap();
    s.execute("range.setValues", json!({"range": "A2", "values": [["a", 1], ["b \"q\"", 2]]})).unwrap();
    s.execute("home.bold", json!({"range": "A1:C1", "on": true})).unwrap();
    s.execute("home.borders", json!({"range": "A1:C3", "preset": "all"})).unwrap();
    s.execute("selection.set", json!({"range": "B2"})).unwrap();
    let r = s.execute("automate.recordActions", json!({})).unwrap();
    assert_eq!(r["saved"], "Script 1");
    assert_eq!(r["steps"], 7);
    let code = s.execute("automate.getScript", json!({"name": "Script 1"})).unwrap()["code"].as_str().unwrap().to_string();
    assert!(code.starts_with("// Recorded with Record Actions.\nfn main(workbook) {\n    let sheet = workbook.getActiveWorksheet();\n"), "{code}");
    assert!(code.contains("sheet.getRange(\"B1\").setValue(12.5);"), "{code}");
    assert!(code.contains("sheet.getRange(\"A2:B3\").setValues([[\"a\", 1], [\"b \\\"q\\\"\", 2]]);"), "{code}");
    assert!(code.contains("run(\"home.borders\", #{preset: \"all\", range: \"A1:C3\"});"), "{code}");
    assert!(code.contains("sheet.getRange(\"B2\").select();"), "{code}");
    // Replaying it on a blank workbook gives the same sheet.
    let mut fresh = book();
    fresh.execute("automate.saveScript", json!({"name": "Rec", "code": code})).unwrap();
    fresh.execute("automate.runScript", json!({"name": "Rec"})).unwrap();
    for a in ["A1", "B1", "C1", "A3", "B3"] {
        assert_eq!(value(&fresh, a), value(&s, a), "{a}");
    }
    assert_eq!(fresh.doc().unwrap().selection.active, s.doc().unwrap().selection.active);
}

#[test]
fn legacy_script_name_is_migrated_on_open() {
    let mut s = book();
    let legacy = r#""{""Lock"":[{""command"":""cell.set"",""params"":{""cell"":""A1"",""input"":""x""}}],""bad"":5}""#;
    s.execute("formulas.defineName", json!({"name": "_sc_scripts", "refersTo": format!("={legacy}")})).unwrap();
    let bytes = crate::io::save_bytes(&s.doc().unwrap().wb, "book.xlsx").unwrap();
    let (wb, _) = crate::io::open_bytes("book.xlsx", &bytes).unwrap();
    assert!(!wb.names.iter().any(|n| n.name == "_sc_scripts"));
    assert_eq!(wb.scripts.len(), 1);
    assert_eq!(wb.scripts[0].name, "Lock");
    assert_eq!(wb.scripts[0].language, gridcraft_model::ScriptLanguage::Commands);
}

#[test]
fn script_management_commands_validate_names() {
    let mut s = book();
    let r = s.execute("automate.newScript", json!({})).unwrap();
    assert_eq!(r["name"], "Script 1");
    assert!(s.take_ui_requests().iter().any(|q| matches!(q, crate::UiRequest::Dialog(n, _) if n == "codeEditor")));
    let r = s.execute("automate.newScript", json!({"name": "Mine"})).unwrap();
    assert_eq!(r["name"], "Mine");
    assert!(s.take_ui_requests().is_empty(), "programmatic calls open nothing");
    assert!(s.execute("automate.renameScript", json!({"name": "Mine", "newName": "script 1"})).is_err());
    s.execute("automate.renameScript", json!({"name": "Mine", "newName": "Yours"})).unwrap();
    assert!(s.execute("automate.saveScript", json!({"name": "  ", "code": ""})).is_err());
    assert!(s.execute("automate.saveScript", json!({"name": "a\u{1}b", "code": ""})).is_err());
    let r = s.execute("automate.saveScript", json!({"name": "Broken", "code": "fn main( {"})).unwrap();
    assert!(r["syntaxError"]["line"].is_number(), "{r}");
    s.execute("automate.deleteScript", json!({"name": "yours"})).unwrap();
    let names: Vec<String> = s.doc().unwrap().wb.scripts.iter().map(|x| x.name.clone()).collect();
    assert_eq!(names, ["Script 1", "Broken"]);
    assert!(s.execute("automate.deleteScript", json!({"name": "gone"})).is_err());
    // Script edits are undoable workbook changes.
    s.execute("edit.undo", json!({})).unwrap();
    assert!(s.doc().unwrap().wb.script("Yours").is_some());
}

#[test]
fn hostile_object_model_arguments_are_errors_not_crashes() {
    let mut s = book();
    for code in [
        "workbook.getActiveWorksheet().getCell(-1, 0);",
        "workbook.getActiveWorksheet().getCell(2000000, 0);",
        "workbook.getActiveWorksheet().getCell(1.5, 0);",
        "workbook.getActiveWorksheet().getRangeByIndexes(0, 0, 0, 5);",
        "workbook.getActiveWorksheet().getRange(\"A1\").getOffsetRange(-5, 0);",
        "workbook.getActiveWorksheet().getRange(\"A1:B2\").getResizedRange(-3, 0);",
        "workbook.getActiveWorksheet().getRange().getValues();",
        "workbook.getActiveWorksheet().getRange(\"A:A\").setValue(1);",
        "workbook.getActiveWorksheet().getRange(\"A1\").setValues([1]);",
        "workbook.getActiveWorksheet().getRange(\"A1\").setValues([[#{a: 1}]]);",
        "workbook.getActiveWorksheet().getRange(\"A1\").setValue(1.0 / 0.0);",
        "workbook.getActiveWorksheet().getRange(\"Other!A1\");",
        "workbook.getWorksheet(\"Sheet1\").delete(); workbook.getActiveWorksheet();",
        "let s = workbook.getActiveWorksheet(); run(\"sheet.rename\", #{name: \"X\"}); s.getRange(\"A1\").setValue(1);",
        "run(\"no.such.command\");",
        "run(\"home.bold\", [1, 2]);",
        "workbook.getActiveWorksheet().getRange(\"A1\").getFormat().getFont().setSize(\"big\");",
        "workbook.getActiveWorksheet().getRange(\"A1\").getFormat().setHorizontalAlignment(\"sideways\");",
        "workbook.getNamedItem(\"missing\").getRange();",
        "main(1, 2);",
    ] {
        let r = s.execute("automate.runScript", json!({"code": code}));
        assert!(!matches!(r, Err(crate::EngineError::Internal(..))), "{code}: {r:?}");
    }
    let e = fails(&mut s, "fn main(a, b) { }");
    assert!(e.contains("one parameter"), "{e}");
}

#[test]
fn recursion_within_the_limit_runs_on_the_script_stack() {
    let mut s = book();
    // 120 nested calls (the native limit is 128) need more stack than a test thread has.
    let out = run(&mut s, "fn depth(n) { if n == 0 { 0 } else { 1 + depth(n - 1) } } depth(120)");
    assert_eq!(out["result"], json!(120));
    // Nested data is built, measured and dropped on the script's stack too.
    s.script_limits = super::Limits { operations: 12_000, ..super::LIMITS };
    let e = fails(&mut s, "let a = []; loop { a = [a]; }");
    assert!(e.contains("ran too long"), "{e}");
}
