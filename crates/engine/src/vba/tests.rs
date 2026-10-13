//! Macros run against real workbooks: the object model, forms and events end to end.

use std::sync::Arc;

use gridcraft_vba::forms::{ControlKind, FormDesign};
use gridcraft_vba::ovba::{Module, ModuleKind, Project};
use gridcraft_vba::{Event, Value};
use serde_json::json;

use super::*;
use crate::Session;

/// A session with a two-sheet workbook ("Invoice", "Trans") whose project holds `modules`.
fn session(modules: &[(&str, ModuleKind, &str)], forms: Vec<FormDesign>) -> (Session, u64) {
    let mut s = Session::new();
    s.new_workbook();
    s.execute("sheet.rename", json!({"name": "Invoice"})).unwrap();
    s.execute("home.insertSheet", json!({"name": "Trans"})).unwrap();
    s.execute("sheet.activate", json!({"sheet": "Invoice"})).unwrap();
    let uid = s.doc().unwrap().uid;
    let project = Project {
        name: "VBAProject".into(),
        code_page: 1252,
        modules: modules.iter().map(|(n, k, c)| Module { name: n.to_string(), stream: n.to_string(), kind: *k, code: c.to_string() }).collect(),
    };
    s.macros.doc_mut(uid).contents = Some(Arc::new(VbaContents { project, forms }));
    enable(&mut s, uid, true);
    (s, uid)
}

fn run(s: &mut Session, uid: u64, name: &str) -> Vec<String> {
    let mut ui = HeadlessUi::default();
    run_headless(s, uid, name, &mut ui).unwrap_or_else(|e| panic!("{name}: {e}"));
    ui.messages
}

fn text(s: &Session, sheet: &str, cell: &str) -> String {
    let wb = &s.doc().unwrap().wb;
    let si = wb.sheet_index(sheet).unwrap();
    let sh = wb.sheet(si).unwrap();
    crate::display::cell_text(wb, sh, gridcraft_core::CellRef::parse(cell).unwrap())
}

fn set(s: &mut Session, sheet: &str, cell: &str, input: &str) {
    s.execute("cell.set", json!({"sheet": sheet, "cell": cell, "input": input})).unwrap();
}

#[test]
fn invoice_posting_copies_values_between_sheets() {
    // The shape of a real "save the invoice" macro: select, copy, paste values on the next free
    // row of another sheet, formulas built from text, then clear the form and bump the number.
    let code = r#"
Sub Post()
    Dim b As Long
    Sheets("Invoice").Select
    Range("G2").Select
    Application.CutCopyMode = False
    Selection.Copy
    Sheets("Trans").Select
    b = Application.WorksheetFunction.CountA(Range("D:D")) + 1
    Range("A" & b).Select
    Selection.PasteSpecial Paste:=xlPasteValues, Operation:=xlNone, SkipBlanks:=False, Transpose:=False
    Worksheets("Trans").Range("f" & b).Value = "=d" & b & "-(e" & b & "+g" & b & ")"
    Worksheets("Trans").Range("d" & b).Value = Worksheets("Invoice").Range("G21").Value
    Sheets("Invoice").Select
    Range("A6:A20").Select
    Selection.ClearContents
    Dim e As Long, f As Long
    e = Len(Range("g3").Text) - 3
    f = Right(Range("g3"), e)
    f = f + 1
    Range("g3") = "SES" & f
End Sub
"#;
    let (mut s, uid) = session(&[("Module1", ModuleKind::Standard, code)], vec![]);
    set(&mut s, "Invoice", "G2", "Ma Ma");
    set(&mut s, "Invoice", "G3", "SES32964");
    set(&mut s, "Invoice", "G21", "1500");
    set(&mut s, "Invoice", "A6", "x1");
    set(&mut s, "Trans", "D1", "Due");
    set(&mut s, "Trans", "D2", "10");
    run(&mut s, uid, "Post");
    assert_eq!(text(&s, "Trans", "A3"), "Ma Ma", "pasted on the next free row");
    assert_eq!(text(&s, "Trans", "D3"), "1500");
    assert_eq!(text(&s, "Trans", "F3"), "1500", "the formula built from text works");
    assert_eq!(text(&s, "Invoice", "A6"), "", "the form was cleared");
    assert_eq!(text(&s, "Invoice", "G3"), "SES32965", "the invoice number advanced");
    assert_eq!(s.doc().unwrap().wb.active().unwrap().name, "Invoice", "the macro left Invoice selected");
}

#[test]
fn lookups_find_end_and_cells() {
    let code = r#"
Sub Look()
    Dim FoundCell As Range, last As Long
    Set FoundCell = Worksheets("Trans").Columns(2).Find("b2", LookIn:=xlValues, Lookat:=xlWhole)
    If FoundCell Is Nothing Then
        MsgBox "missing"
    Else
        Range("H1") = FoundCell.Row
    End If
    Set FoundCell = Worksheets("Trans").Columns(2).Find("zzz", LookIn:=xlValues, Lookat:=xlWhole)
    If FoundCell Is Nothing Then Range("H2") = "none"
    last = Worksheets("Trans").Cells(Rows.Count, 2).End(xlUp).Row
    Range("H3") = last
    Range("H4") = Application.WorksheetFunction.VLookup("b3", Worksheets("Trans").Range("B1:C5"), 2, False)
    Range("H5") = WorksheetFunction.Sum(Worksheets("Trans").Range("C1:C5"))
    Dim c As Range, n As Long
    For Each c In Worksheets("Trans").Range("B1:B3")
        n = n + Len(c.Value)
    Next
    Range("H6") = n
    Cells(7, 8).Value = Range("H6").Address(False, False)
    Range("H8").Formula = "=H5*2"
    Range("H9") = Range("H8").Value
End Sub
"#;
    let (mut s, uid) = session(&[("Module1", ModuleKind::Standard, code)], vec![]);
    for (r, v) in [(1, "1"), (2, "2"), (3, "3"), (4, "4")] {
        set(&mut s, "Trans", &format!("B{r}"), &format!("b{v}"));
        set(&mut s, "Trans", &format!("C{r}"), &format!("{}", r * 10));
    }
    let msgs = run(&mut s, uid, "Look");
    assert!(msgs.is_empty(), "{msgs:?}");
    assert_eq!(text(&s, "Invoice", "H1"), "2");
    assert_eq!(text(&s, "Invoice", "H2"), "none");
    assert_eq!(text(&s, "Invoice", "H3"), "4");
    assert_eq!(text(&s, "Invoice", "H4"), "30");
    assert_eq!(text(&s, "Invoice", "H5"), "100");
    assert_eq!(text(&s, "Invoice", "H6"), "6");
    assert_eq!(text(&s, "Invoice", "H7"), "H6");
    assert_eq!(text(&s, "Invoice", "H9"), "200");
}

fn control(name: &str, kind: ControlKind) -> gridcraft_vba::forms::ControlDesign {
    let mut c = crate::vba::forms::tests_support::control(name, kind);
    c.tab_index = 0;
    c
}

#[test]
fn forms_initialize_events_and_buttons() {
    let form = FormDesign {
        name: "PayForm".into(),
        caption: "Pay".into(),
        width: 200.0,
        height: 120.0,
        controls: vec![
            control("TextBox1", ControlKind::TextBox),
            control("TextBox2", ControlKind::TextBox),
            control("ListBox1", ControlKind::ListBox),
        ],
        ..Default::default()
    };
    let module = r#"
Sub Pay()
    PayForm.Show
    Range("B1") = "after show"
End Sub
"#;
    let form_code = r#"
Private Sub UserForm_Initialize()
    TextBox1.Value = Sheets("Invoice").Range("G21").Value
    ListBox1.AddItem "cash"
    ListBox1.AddItem "card"
    Me.Caption = "Pay " & TextBox1.Value
End Sub
Private Sub TextBox1_Change()
    Range("B2") = "changed to " & TextBox1.Text
End Sub
Private Sub CommandButton1_Click()
    Range("B3") = TextBox1.Value - TextBox2.Value
    Range("B4") = ListBox1.List(1) & ListBox1.ListCount
    Unload Me
End Sub
"#;
    let (mut s, uid) = session(&[("Module1", ModuleKind::Standard, module), ("PayForm", ModuleKind::Form, form_code)], vec![form]);
    set(&mut s, "Invoice", "G21", "500");
    // The user types 120 in TextBox2, then clicks the button.
    let mut ui = HeadlessUi::default();
    ui.script.push_back(Event { module: "PayForm".into(), proc: "__set:TextBox2=120".into(), args: vec![] });
    ui.script.push_back(Event { module: "PayForm".into(), proc: "CommandButton1_Click".into(), args: vec![] });
    struct Typing<'a>(&'a mut HeadlessUi);
    impl MacroUi for Typing<'_> {
        fn msgbox(&mut self, p: &str, b: i64, t: &str) -> i64 {
            self.0.msgbox(p, b, t)
        }
        fn inputbox(&mut self, p: &str, t: &str, d: &str) -> Option<String> {
            self.0.inputbox(p, t, d)
        }
        fn wait_event(&mut self, s: &mut Session, uid: u64) -> Option<Event> {
            // `__set:Control=value` stands for the user typing.
            let ev = self.0.script.pop_front()?;
            if let Some(rest) = ev.proc.strip_prefix("__set:") {
                let (ctl, v) = rest.split_once('=').unwrap_or((rest, ""));
                let _ = control_member(s, uid, &ev.module, ctl, "Value", &[], Some(Value::from(v)));
                return self.wait_event(s, uid);
            }
            Some(ev)
        }
    }
    let mut it = interpreter(&mut s, uid).unwrap();
    let mut typing = Typing(&mut ui);
    let mut host = DirectHost { s: &mut s, uid, ui: &mut typing };
    it.run(&mut host, "Pay", vec![]).unwrap();
    assert_eq!(text(&s, "Invoice", "B2"), "changed to 500", "Initialize set the box, which fired Change");
    assert_eq!(text(&s, "Invoice", "B3"), "380", "the click read both boxes");
    assert_eq!(text(&s, "Invoice", "B4"), "card2");
    assert_eq!(text(&s, "Invoice", "B1"), "after show", "Show returned after Unload");
    assert!(s.macros.doc(uid).unwrap().forms.iter().all(|f| f.unloaded || !f.visible), "the form was unloaded");
    assert!(s.macros.doc(uid).unwrap().forms.iter().all(|f| f.unloaded), "Terminate did not load it again");
}

#[test]
fn commands_list_and_run_macros() {
    let code = "Sub Hello()\n  Range(\"A1\") = \"hi\"\n  MsgBox \"done\"\nEnd Sub\nPrivate Sub Hidden()\nEnd Sub\n";
    let (mut s, uid) = session(&[("Module1", ModuleKind::Standard, code)], vec![]);
    let names = macro_names(&mut s, uid);
    assert_eq!(names, vec!["Hello".to_string()]);
    let r = s.execute("developer.runMacro", json!({"name": "Hello"})).unwrap();
    assert_eq!(r["messages"], json!(["done"]));
    assert_eq!(text(&s, "Invoice", "A1"), "hi");
    // Disabled macros don't run.
    enable(&mut s, uid, false);
    assert!(s.execute("developer.runMacro", json!({"name": "Hello"})).is_err());
    let src = s.execute("developer.macroSource", json!({})).unwrap();
    assert_eq!(src["modules"][0]["name"], "Module1");
}

#[test]
fn runtime_errors_name_the_problem() {
    let (mut s, uid) = session(&[("Module1", ModuleKind::Standard, "Sub Bad()\n  Sheets(\"Nope\").Select\nEnd Sub\n")], vec![]);
    let mut ui = HeadlessUi::default();
    let e = run_headless(&mut s, uid, "Bad", &mut ui).unwrap_err();
    assert!(matches!(e, VbaError::Runtime { number: 9, .. }), "{e:?}");
}

/// Opens `$GRIDCRAFT_MACRO_BOOK` and runs each macro headless, reporting what fails (a corpus
/// check for real workbooks kept outside the repo).
#[test]
#[ignore = "needs GRIDCRAFT_MACRO_BOOK"]
fn macro_corpus() {
    let Some(path) = std::env::var_os("GRIDCRAFT_MACRO_BOOK") else { return };
    let mut s = Session::new();
    s.execute("file.open", json!({"path": path.to_string_lossy()})).unwrap();
    let uid = s.doc().unwrap().uid;
    enable(&mut s, uid, true);
    let it = interpreter(&mut s, uid).unwrap();
    for (m, e) in &it.load_errors {
        println!("PARSE {m}: {e}");
    }
    let only = std::env::var("GRIDCRAFT_MACRO").ok();
    for name in it.macros() {
        if only.as_deref().is_some_and(|o| !o.eq_ignore_ascii_case(&name)) {
            continue;
        }
        let mut ui = HeadlessUi::default();
        match run_headless(&mut s, uid, &name, &mut ui) {
            Ok(_) => println!("OK    {name} msgs={:?}", ui.messages),
            Err(e) => println!("FAIL  {name}: {e}"),
        }
    }
}

#[test]
fn find_and_find_next_walk_the_matches() {
    let code = r#"
Sub F()
    Dim c As Range, first As String, n As Long
    Set c = Worksheets("Trans").Range("B1:B10").Find("x", LookIn:=xlValues, LookAt:=xlWhole)
    If Not c Is Nothing Then
        first = c.Address
        Do
            n = n + 1
            Set c = Worksheets("Trans").Range("B1:B10").FindNext(c)
        Loop While Not c Is Nothing And c.Address <> first
    End If
    Range("A1") = n & first
End Sub
"#;
    let (mut s, uid) = session(&[("Module1", ModuleKind::Standard, code)], vec![]);
    for r in [2, 5, 9] {
        set(&mut s, "Trans", &format!("B{r}"), "x");
    }
    set(&mut s, "Trans", "B3", "xx");
    run(&mut s, uid, "F");
    assert_eq!(text(&s, "Invoice", "A1"), "3$B$2", "three whole matches, starting after B1");
}
