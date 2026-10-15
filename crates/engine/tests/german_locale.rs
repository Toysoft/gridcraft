//! A German interface: what people see and type is German (`1.234,56`, `13.10.2026`, `WAHR`,
//! `#NV`, `=SUMME(A1;2,5)`), while the workbook, its files and every command stay canonical.
use gridcraft_core::{CellRef, NumberLocale, Value};
use gridcraft_engine::Session;
use gridcraft_engine::display::{self, DisplayLocale};
use gridcraft_engine::formula::FormulaLocale;
use serde_json::json;

const GERMAN: DisplayLocale = DisplayLocale { number: NumberLocale::DE_DE, formula: FormulaLocale::De };

fn session() -> Session {
    let mut s = Session::new();
    s.new_workbook();
    s
}

fn shown(s: &Session, cell: &str) -> String {
    let d = s.doc().unwrap();
    display::value_text_in(&d.wb, d.wb.active().unwrap(), CellRef::parse(cell).unwrap(), &GERMAN)
}

fn edit_text(s: &Session, cell: &str) -> String {
    let d = s.doc().unwrap();
    display::edit_text_in(&d.wb, d.wb.active().unwrap(), CellRef::parse(cell).unwrap(), &GERMAN)
}

#[test]
fn values_show_in_the_regional_format_and_the_formula_language() {
    let mut s = session();
    for (cell, input) in [
        ("A1", "1234.56"),
        ("A2", "TRUE"),
        ("A3", "=NA()"),
        ("A4", "=1/0"),
        ("A5", "=\"a\"+1"),
        ("A6", "10/13/2026"),
        ("A7", "10/13/2026 14:30"),
        ("A8", "=SQRT(-1)"),
        ("A9", "=FALSE"),
    ] {
        s.execute("cell.set", json!({"cell": cell, "input": input})).unwrap();
    }
    s.execute("home.numberFormat", json!({"range": "A1", "code": "#,##0.00"})).unwrap();
    for (cell, text) in [
        ("A1", "1.234,56"),
        ("A2", "WAHR"),
        ("A3", "#NV"),
        ("A4", "#DIV/0!"),
        ("A5", "#WERT!"),
        ("A6", "13.10.2026"),
        ("A7", "13.10.2026 14:30"),
        ("A8", "#ZAHL!"),
        ("A9", "FALSCH"),
    ] {
        assert_eq!(shown(&s, cell), text, "{cell}");
    }
    // Agents and files read the canonical text.
    let d = s.doc().unwrap();
    assert_eq!(display::value_text(&d.wb, d.wb.active().unwrap(), CellRef::parse("A1").unwrap()), "1,234.56");
    assert_eq!(s.execute("cell.get", json!({"cell": "A2"})).unwrap()["text"], "TRUE");
}

#[test]
fn the_editor_starts_from_text_that_reads_back_in_german() {
    let mut s = session();
    for (cell, input) in
        [("A1", "1234.5"), ("A2", "10/13/2026"), ("A3", "10/13/2026 14:30:15"), ("A4", "14:30"), ("A5", "13-Oct-26"), ("A6", "FALSE"), ("A7", "#N/A")]
    {
        s.execute("cell.set", json!({"cell": cell, "input": input})).unwrap();
    }
    s.execute("home.numberFormat", json!({"range": "A1", "code": "#,##0.00"})).unwrap();
    for (cell, text) in [
        ("A1", "1234,5"),
        ("A2", "13.10.2026"),
        ("A3", "13.10.2026 14:30:15"),
        ("A4", "14:30"),
        ("A5", "13. Okt 26"),
        ("A6", "FALSCH"),
        ("A7", "#NV"),
    ] {
        assert_eq!(edit_text(&s, cell), text, "{cell}");
    }
    // A month-and-year format would read back as the first of the month: the short date instead.
    s.execute("cell.set", json!({"cell": "B1", "input": "10/13/2026"})).unwrap();
    s.execute("home.numberFormat", json!({"range": "B1", "code": "mmm-yy"})).unwrap();
    assert_eq!(shown(&s, "B1"), "Okt 26");
    assert_eq!(edit_text(&s, "B1"), "13.10.2026");
}

#[test]
fn typed_values_enter_with_the_format_german_typing_picks() {
    let mut s = session();
    // The interface reads `13.10.2026` and enters the serial with the short date format.
    s.execute("cell.set", json!({"cell": "A1", "input": "46308", "autoFormat": "m/d/yyyy"})).unwrap();
    s.execute("cell.set", json!({"cell": "A2", "input": "5.5", "autoFormat": "#,##0.00 \"€\";[Red]-#,##0.00 \"€\""})).unwrap();
    s.execute("selection.set", json!({"range": "B1:B2"})).unwrap();
    s.execute("range.fill", json!({"range": "B1:B2", "input": "-0.032", "autoFormat": "0.00%"})).unwrap();
    let d = s.doc().unwrap();
    let sh = d.wb.active().unwrap();
    let fmt = |c: &str| d.wb.styles.get(sh.style_id(CellRef::parse(c).unwrap())).num_fmt.as_str().to_string();
    assert_eq!(sh.value(CellRef::parse("A1").unwrap()), Value::Number(46308.0));
    assert_eq!(fmt("A1"), "m/d/yyyy");
    assert_eq!(shown(&s, "A2"), "5,50 €");
    assert_eq!(shown(&s, "B2"), "-3,20%");
    // A cell already formatted keeps its format, as automatic formats do; formulas ignore it.
    s.execute("home.numberFormat", json!({"range": "C1", "code": "0.000"})).unwrap();
    s.execute("cell.set", json!({"cell": "C1", "input": "1.5", "autoFormat": "0%"})).unwrap();
    s.execute("cell.set", json!({"cell": "C2", "input": "=1+1", "autoFormat": "0%"})).unwrap();
    let d = s.doc().unwrap();
    let sh = d.wb.active().unwrap();
    let fmt = |c: &str| d.wb.styles.get(sh.style_id(CellRef::parse(c).unwrap())).num_fmt.as_str().to_string();
    assert_eq!(fmt("C1"), "0.000");
    assert_eq!(fmt("C2"), "General");
    // Programmatic calls without it are unchanged, and a format longer than Excel allows is ignored.
    s.execute("cell.set", json!({"cell": "D1", "input": "12%", "autoFormat": "0".repeat(300)})).unwrap();
    let d = s.doc().unwrap();
    let sh = d.wb.active().unwrap();
    assert_eq!(d.wb.styles.get(sh.style_id(CellRef::parse("D1").unwrap())).num_fmt.as_str(), "0%");
}

#[test]
fn show_formulas_and_print_text_follow_the_formula_language() {
    let mut s = session();
    s.execute("cell.set", json!({"cell": "A1", "input": "=SUM(1.5,2,{1,2;3,4})+IF(TRUE,1,#N/A)"})).unwrap();
    std::sync::Arc::make_mut(&mut s.active_mut().unwrap().wb).sheet_mut(0).unwrap().show_formulas = true;
    let d = s.doc().unwrap();
    assert_eq!(display::cell_text_in(&d.wb, 0, CellRef::parse("A1").unwrap(), &GERMAN), "=SUMME(1,5;2;{1.2;3.4})+WENN(WAHR;1;#NV)");
    assert_eq!(display::cell_text(&d.wb, d.wb.active().unwrap(), CellRef::parse("A1").unwrap()), "=SUM(1.5,2,{1,2;3,4})+IF(TRUE,1,#N/A)");
    // Headers and footers print the date as the region's short date.
    let (date, _) = gridcraft_engine::cmd::print::hf_date_time_in(&d.wb, &GERMAN);
    assert!(date.split('.').count() == 3 && date.len() == 10, "{date}");
}

#[test]
fn csv_is_written_and_read_as_german_excel_does_in_a_german_session() {
    let mut s = session();
    s.execute("range.setValues", json!({"range": "A1", "values": [[1234.5, "x;y", true], ["10/13/2026", 0.25, "=NA()"]]})).unwrap();
    s.execute("home.numberFormat", json!({"range": "B2", "code": "0%"})).unwrap();
    let wb = s.doc().unwrap().wb.clone();
    // A canonical session writes commas and en-US numbers, as before.
    let plain = String::from_utf8(gridcraft_engine::io::save_bytes(&wb, "out.csv").unwrap()).unwrap();
    assert_eq!(plain, "1234.5,x;y,TRUE\r\n10/13/2026,25%,#N/A\r\n");
    // A German one writes `;` (the list separator where the decimal separator is `,`) and German values.
    let german = String::from_utf8(gridcraft_engine::io::save_bytes_in(&wb, "out.csv", &GERMAN).unwrap()).unwrap();
    assert_eq!(german, "1234,5;\"x;y\";WAHR\r\n13.10.2026;25%;#NV\r\n");
    // ... and reads it back as the same values.
    let (back, _) = gridcraft_engine::io::open_bytes_with_password_in("out.csv", german.as_bytes(), None, &GERMAN).unwrap();
    let sh = back.sheet(0).unwrap();
    assert_eq!(back.csv_delimiter, Some(b';'));
    assert_eq!(sh.value(CellRef::parse("A1").unwrap()), Value::Number(1234.5));
    assert_eq!(sh.value(CellRef::parse("C1").unwrap()), Value::Bool(true));
    assert_eq!(sh.value(CellRef::parse("A2").unwrap()), Value::Number(46308.0));
    assert_eq!(sh.value(CellRef::parse("B2").unwrap()), Value::Number(0.25));
    assert_eq!(sh.value(CellRef::parse("C2").unwrap()), Value::Error(gridcraft_core::CellError::NA));
    // A comma-separated file is en-US style whoever opens it.
    let (us, _) = gridcraft_engine::io::open_bytes_with_password_in("us.csv", b"1.5,10/13/2026\r\n", None, &GERMAN).unwrap();
    assert_eq!(us.sheet(0).unwrap().value(CellRef::parse("A1").unwrap()), Value::Number(1.5));
    assert_eq!(us.sheet(0).unwrap().value(CellRef::parse("B1").unwrap()), Value::Number(46308.0));
    // Workbook formats never change: XLSX written from a German session equals a canonical one.
    assert_eq!(gridcraft_engine::io::save_bytes_in(&wb, "b.json", &GERMAN).unwrap(), gridcraft_engine::io::save_bytes(&wb, "b.json").unwrap());
}

#[test]
fn file_commands_use_the_session_locale_and_default_to_canonical() {
    let dir = std::env::temp_dir().join(format!("gridcraft-german-csv-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("Liste.csv").to_string_lossy().into_owned();
    let mut s = session();
    s.execute("range.setValues", json!({"range": "A1", "values": [[1.5, 2.25]]})).unwrap();
    s.execute("file.saveAs", json!({"path": path})).unwrap();
    assert_eq!(std::fs::read_to_string(&path).unwrap(), "1.5,2.25\r\n");
    s.display_locale = GERMAN;
    s.execute("file.saveAs", json!({"path": path})).unwrap();
    assert_eq!(std::fs::read_to_string(&path).unwrap(), "1,5;2,25\r\n");
    s.execute("file.open", json!({"path": path})).unwrap();
    let d = s.doc().unwrap();
    assert_eq!(d.wb.sheet(0).unwrap().value(CellRef::parse("B1").unwrap()), Value::Number(2.25));
    let _ = std::fs::remove_dir_all(&dir);
}
