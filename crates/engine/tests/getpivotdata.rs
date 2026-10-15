//! GETPIVOTDATA end to end: PivotTables built with the pivot commands, formulas entered with
//! `cell.set`, results checked against Excel's documented and observed behaviour.

use gridcraft_engine::Session;
use gridcraft_engine::core::{CellError, CellRef, Value};
use gridcraft_engine::formula::FormulaLocale;
use serde_json::{Value as Json, json};

const REF: Value = Value::Error(CellError::Ref);

/// Region, Product, Date, Units, Sales. East: Apples 120, Pears 60; West: Apples 140, Plums 50;
/// North: Pears 80. 450 in all, 22 units.
fn sales() -> Session {
    let mut s = Session::new();
    s.new_workbook();
    let rows = json!([
        ["Region", "Product", "Date", "Units", "Sales"],
        ["East", "Apples", "1/15/2026", 5, 100],
        ["East", "Pears", "2/10/2026", 3, 60],
        ["West", "Apples", "1/20/2026", 7, 140],
        ["West", "Plums", "3/15/2026", 2, 50],
        ["North", "Pears", "2/25/2026", 4, 80],
        ["East", "Apples", "3/5/2026", 1, 20]
    ]);
    s.execute("range.setValues", json!({"range": "A1", "values": rows})).unwrap();
    s
}

/// A PivotTable on a new sheet (Sheet2, report at A3), which becomes the active sheet.
fn pivot(p: Json) -> Session {
    let mut s = sales();
    let mut p = p;
    p["source"] = json!("Sheet1!$A$1:$E$7");
    s.execute("insert.pivotTable", p).unwrap();
    s
}

fn at(a: &str) -> CellRef {
    CellRef::parse(a).unwrap()
}

/// Enters `formula` in Z1 of the active sheet and returns its value.
fn eval(s: &mut Session, formula: &str) -> Value {
    s.execute("cell.set", json!({"cell": "Z1", "input": formula})).unwrap();
    s.doc().unwrap().wb.active().unwrap().value(at("Z1"))
}

fn n(v: f64) -> Value {
    Value::Number(v)
}

#[test]
fn grand_total_items_and_subtotals() {
    let mut s = pivot(json!({"rows": ["Region", "Product"], "values": ["Sales"]}));
    // The value field by its caption or its source field, case ignored.
    assert_eq!(eval(&mut s, r#"=GETPIVOTDATA("Sum of Sales",$A$3)"#), n(450.0));
    assert_eq!(eval(&mut s, r#"=GETPIVOTDATA("sales",$A$3)"#), n(450.0));
    assert_eq!(eval(&mut s, r#"=GETPIVOTDATA("SUM OF SALES",B7)"#), n(450.0), "any cell of the report");
    assert_eq!(eval(&mut s, r#"=GETPIVOTDATA("Sales",A2:A3)"#), n(450.0), "a range that reaches into the report");
    assert_eq!(eval(&mut s, r#"=GETPIVOTDATA("Units",$A$3)"#), REF, "not a value field");
    // Items: row items, an outer item's subtotal, pairs in any order, items by reference.
    assert_eq!(eval(&mut s, r#"=GETPIVOTDATA("Sales",$A$3,"Region","East")"#), n(180.0));
    assert_eq!(eval(&mut s, r#"=GETPIVOTDATA("Sales",$A$3,"region","EAST","Product","apples")"#), n(120.0));
    assert_eq!(eval(&mut s, r#"=GETPIVOTDATA("Sales",$A$3,"Product","Pears","Region","North")"#), n(80.0));
    assert_eq!(eval(&mut s, r#"=GETPIVOTDATA("Sales",$A$3,"Region",A4)"#), n(180.0), "A4 holds East");
    // An inner item alone: found when it belongs to one outer item only (Plums: West).
    assert_eq!(eval(&mut s, r#"=GETPIVOTDATA("Sales",$A$3,"Product","Plums")"#), n(50.0));
    assert_eq!(eval(&mut s, r#"=GETPIVOTDATA("Sales",$A$3,"Product","Apples")"#), REF, "East and West sell Apples");
    // Unknown items and fields, items not in the report, odd pairs, no PivotTable.
    assert_eq!(eval(&mut s, r#"=GETPIVOTDATA("Sales",$A$3,"Region","South")"#), REF);
    assert_eq!(eval(&mut s, r#"=GETPIVOTDATA("Sales",$A$3,"Region","East","Product","Plums")"#), REF);
    assert_eq!(eval(&mut s, r#"=GETPIVOTDATA("Sales",$A$3,"Color","East")"#), REF);
    assert_eq!(eval(&mut s, r#"=GETPIVOTDATA("Sales",$A$3,"Region")"#), REF);
    assert_eq!(eval(&mut s, r#"=GETPIVOTDATA("Sales",$H$30)"#), REF);
    assert_eq!(eval(&mut s, r#"=GETPIVOTDATA("Sales","A3")"#), REF);
    assert_eq!(eval(&mut s, r#"=GETPIVOTDATA("Sales",Sheet1!A1)"#), REF, "the source isn't the report");
    // Errors in the arguments come through.
    assert_eq!(eval(&mut s, r#"=GETPIVOTDATA(NA(),$A$3)"#), Value::Error(CellError::NA));
    assert_eq!(eval(&mut s, r#"=GETPIVOTDATA("Sales",$A$3,"Region",1/0)"#), Value::Error(CellError::Div0));
    // From another sheet.
    s.execute("sheet.activate", json!({"sheet": "Sheet1"})).unwrap();
    assert_eq!(eval(&mut s, r#"=GETPIVOTDATA("Sales",Sheet2!$A$3,"Region","West")"#), n(190.0));
}

#[test]
fn rows_and_columns_and_empty_cells() {
    let mut s = pivot(json!({"rows": ["Region"], "columns": ["Product"], "values": ["Sales"]}));
    assert_eq!(eval(&mut s, r#"=GETPIVOTDATA("Sales",$A$3,"Region","West","Product","Apples")"#), n(140.0));
    assert_eq!(eval(&mut s, r#"=GETPIVOTDATA("Sales",$A$3,"Product","Apples")"#), n(260.0), "column total");
    assert_eq!(eval(&mut s, r#"=GETPIVOTDATA("Sales",$A$3,"Region","North")"#), n(80.0), "row total");
    // East sells no Plums: the report shows an empty cell, which counts as 0.
    assert_eq!(eval(&mut s, r#"=GETPIVOTDATA("Sales",$A$3,"Region","East","Product","Plums")"#), n(0.0));
    // Without the row totals, North's total isn't there; Plums' column total still is.
    s.execute("pivot.grandTotals", json!({"mode": "columns"})).unwrap();
    assert_eq!(eval(&mut s, r#"=GETPIVOTDATA("Sales",$A$3,"Region","North")"#), REF);
    assert_eq!(eval(&mut s, r#"=GETPIVOTDATA("Sales",$A$3,"Product","Plums")"#), n(50.0));
    assert_eq!(eval(&mut s, r#"=GETPIVOTDATA("Sales",$A$3)"#), REF);
}

#[test]
fn totals_turned_off_collapsed_and_filtered_items_are_ref() {
    let mut s = pivot(json!({"rows": ["Region", "Product"], "values": ["Sales"]}));
    let east = r#"=GETPIVOTDATA("Sales",$A$3,"Region","East")"#;
    s.execute("pivot.subtotals", json!({"mode": "none"})).unwrap();
    assert_eq!(eval(&mut s, east), REF, "no subtotal shown");
    s.execute("pivot.subtotals", json!({"mode": "bottom"})).unwrap();
    assert_eq!(eval(&mut s, east), n(180.0), "subtotal at the bottom of the group");
    s.execute("pivot.grandTotals", json!({"mode": "off"})).unwrap();
    assert_eq!(eval(&mut s, r#"=GETPIVOTDATA("Sales",$A$3)"#), REF);
    s.execute("pivot.grandTotals", json!({"mode": "on"})).unwrap();
    // Collapsed: the region's total shows, its products don't.
    s.execute("pivot.collapse", json!({"field": "Region", "items": ["East"]})).unwrap();
    assert_eq!(eval(&mut s, east), n(180.0));
    assert_eq!(eval(&mut s, r#"=GETPIVOTDATA("Sales",$A$3,"Region","East","Product","Apples")"#), REF);
    s.execute("pivot.collapse", json!({"field": "Region", "collapse": false})).unwrap();
    // Hidden items aren't in the report (North sells only Pears, so it goes too), and the
    // totals count what is shown.
    s.execute("pivot.filter", json!({"field": "Product", "selected": ["Apples", "Plums"]})).unwrap();
    assert_eq!(eval(&mut s, r#"=GETPIVOTDATA("Sales",$A$3,"Product","Pears","Region","East")"#), REF);
    assert_eq!(eval(&mut s, r#"=GETPIVOTDATA("Sales",$A$3,"Region","North")"#), REF);
    assert_eq!(eval(&mut s, r#"=GETPIVOTDATA("Sales",$A$3)"#), n(310.0));
}

#[test]
fn report_filters() {
    let mut s = pivot(json!({"rows": ["Product"], "values": ["Sales"], "filters": ["Region"]}));
    // (All): the whole data, but no single region to name.
    assert_eq!(eval(&mut s, r#"=GETPIVOTDATA("Sales",$A$3)"#), n(450.0));
    assert_eq!(eval(&mut s, r#"=GETPIVOTDATA("Sales",$A$3,"Region","East")"#), REF);
    s.execute("pivot.filter", json!({"field": "Region", "selected": ["East"]})).unwrap();
    // The report body now starts lower; any cell of the report still names it.
    assert_eq!(eval(&mut s, r#"=GETPIVOTDATA("Sales",$A$1)"#), n(180.0));
    assert_eq!(eval(&mut s, r#"=GETPIVOTDATA("Sales",$A$3,"Region","east")"#), n(180.0));
    assert_eq!(eval(&mut s, r#"=GETPIVOTDATA("Sales",$A$3,"Region","West")"#), REF);
    assert_eq!(eval(&mut s, r#"=GETPIVOTDATA("Sales",$A$3,"Product","Apples","Region","East")"#), n(120.0));
}

#[test]
fn dates_numbers_and_grouped_items() {
    let mut s = pivot(json!({"rows": ["Date"], "values": ["Sales"]}));
    // Dates by serial or DATE; 46037 is 15 Jan 2026.
    assert_eq!(eval(&mut s, r#"=GETPIVOTDATA("Sales",$A$3,"Date",DATE(2026,1,15))"#), n(100.0));
    assert_eq!(eval(&mut s, r#"=GETPIVOTDATA("Sales",$A$3,"Date",46037)"#), n(100.0));
    assert_eq!(eval(&mut s, r#"=GETPIVOTDATA("Sales",$A$3,"Date",46037.5)"#), REF);
    s.execute("pivot.group", json!({"field": "Date", "by": "months"})).unwrap();
    assert_eq!(eval(&mut s, r#"=GETPIVOTDATA("Sales",$A$3,"Date","Jan")"#), n(240.0));
    assert_eq!(eval(&mut s, r#"=GETPIVOTDATA("Sales",$A$3,"Date","Mar")"#), n(70.0));
    s.execute("pivot.removeField", json!({"field": "Date", "area": "rows"})).unwrap();
    s.execute("pivot.addField", json!({"field": "Units", "area": "rows"})).unwrap();
    assert_eq!(eval(&mut s, r#"=GETPIVOTDATA("Sales",$A$3,"Units",5)"#), n(100.0));
    assert_eq!(eval(&mut s, r#"=GETPIVOTDATA("Sales",$A$3,"Units","5")"#), n(100.0), "text names the item's label");
    assert_eq!(eval(&mut s, r#"=GETPIVOTDATA("Sales",$A$3,"Units",6)"#), REF);
    assert_eq!(eval(&mut s, r#"=GETPIVOTDATA("Sales",$A$3,"Units",TRUE)"#), REF);
}

#[test]
fn value_fields_captions_and_show_values_as() {
    let mut s = pivot(json!({"rows": ["Region"], "values": ["Sales", "Units"]}));
    assert_eq!(eval(&mut s, r#"=GETPIVOTDATA("Units",$A$3)"#), n(22.0));
    assert_eq!(eval(&mut s, r#"=GETPIVOTDATA("Sum of Units",$A$3,"Region","West")"#), n(9.0));
    assert_eq!(eval(&mut s, r#"=GETPIVOTDATA("Sales",$A$3,"Region","West")"#), n(190.0));
    // Σ Values in rows.
    s.execute("pivot.moveField", json!({"field": "Σ Values", "from": "columns", "to": "rows"})).unwrap();
    assert_eq!(eval(&mut s, r#"=GETPIVOTDATA("Units",$A$3,"Region","West")"#), n(9.0));
    assert_eq!(eval(&mut s, r#"=GETPIVOTDATA("Sales",$A$3)"#), n(450.0));
    // A custom caption replaces the automatic one; the source field still names it.
    s.execute("pivot.valueSettings", json!({"field": "Sum of Sales", "name": "Revenue"})).unwrap();
    assert_eq!(eval(&mut s, r#"=GETPIVOTDATA("Revenue",$A$3)"#), n(450.0));
    assert_eq!(eval(&mut s, r#"=GETPIVOTDATA("Sales",$A$3)"#), n(450.0));
    assert_eq!(eval(&mut s, r#"=GETPIVOTDATA("Sum of Sales",$A$3)"#), REF);
    // Show Values As: the value as shown.
    s.execute("pivot.valueSettings", json!({"field": "Revenue", "showAs": "percentOfGrandTotal"})).unwrap();
    let share = eval(&mut s, r#"=GETPIVOTDATA("Revenue",$A$3,"Region","East")"#).as_f64().unwrap();
    assert!((share - 0.4).abs() < 1e-12, "{share}");
}

#[test]
fn follows_refresh_and_source_changes() {
    let mut s = pivot(json!({"rows": ["Region"], "values": ["Sales"]}));
    let east = r#"=GETPIVOTDATA("Sales",$A$3,"Region","East")"#;
    assert_eq!(eval(&mut s, east), n(180.0));
    // Editing the source changes nothing until the PivotTable is refreshed, as in Excel.
    s.execute("sheet.activate", json!({"sheet": "Sheet1"})).unwrap();
    s.execute("cell.set", json!({"cell": "E2", "input": "1000"})).unwrap();
    s.execute("sheet.activate", json!({"sheet": "Sheet2"})).unwrap();
    let z1 = |s: &Session| s.doc().unwrap().wb.active().unwrap().value(at("Z1"));
    assert_eq!(z1(&s), n(180.0));
    s.execute("pivot.refresh", json!({})).unwrap();
    assert_eq!(z1(&s), n(1080.0), "recalculated by the refresh");
    // A new source with a new region.
    s.execute("sheet.activate", json!({"sheet": "Sheet1"})).unwrap();
    s.execute("range.setValues", json!({"range": "A8", "values": [["South", "Plums", "4/1/2026", 3, 33]]})).unwrap();
    s.execute("sheet.activate", json!({"sheet": "Sheet2"})).unwrap();
    s.execute("cell.set", json!({"cell": "Z2", "input": r#"=GETPIVOTDATA("Sales",$A$3,"Region","South")"#})).unwrap();
    let z2 = |s: &Session| s.doc().unwrap().wb.active().unwrap().value(at("Z2"));
    assert_eq!(z2(&s), REF);
    s.execute("pivot.changeSource", json!({"source": "Sheet1!$A$1:$E$8"})).unwrap();
    assert_eq!(z2(&s), n(33.0));
    // Moving the field to the columns keeps the formulas right; undo brings the old report back.
    s.execute("pivot.moveField", json!({"field": "Region", "from": "rows", "to": "columns"})).unwrap();
    assert_eq!(z1(&s), n(1080.0));
    s.execute("edit.undo", json!({})).unwrap();
    s.execute("edit.undo", json!({})).unwrap();
    assert_eq!(z2(&s), REF);
    // Deleting the PivotTable leaves nothing to read.
    s.execute("pivot.delete", json!({})).unwrap();
    assert_eq!(z1(&s), REF);
}

/// Every value cell of a report: the formula Generate GetPivotData writes for it gives the
/// cell's value back.
fn check_generated_formulas(s: &mut Session) -> usize {
    let (wb, sheet) = {
        let d = s.doc().unwrap();
        (d.wb.clone(), d.wb.active_sheet)
    };
    let pt = wb.active().unwrap().pivots[0].clone();
    let area = pt.last_range.unwrap();
    let mut n = 0;
    for c in area.iter() {
        let Some(f) = gridcraft_engine::pivot::getpivotdata_formula(&wb, sheet, sheet, c, FormulaLocale::En) else { continue };
        let want = match wb.active().unwrap().value(c) {
            Value::Empty => Value::Number(0.0),
            v => v,
        };
        assert_eq!(eval(s, &format!("={f}")), want, "{} {f}", c.a1());
        n += 1;
    }
    n
}

#[test]
fn generated_formulas_read_back_their_cell_in_every_layout() {
    for (p, layouts) in [
        (json!({"rows": ["Region", "Product"], "values": ["Sales"]}), &["compact", "outline", "tabular"][..]),
        (json!({"rows": ["Region"], "columns": ["Product"], "values": ["Sales", "Units"]}), &["compact", "tabular"][..]),
        (json!({"rows": ["Region", "Product"], "columns": ["Date"], "values": ["Sales"]}), &["outline"][..]),
        (json!({"rows": ["Product"], "values": ["Sales"], "filters": ["Region"]}), &["compact"][..]),
    ] {
        let mut s = pivot(p.clone());
        for layout in layouts {
            s.execute("pivot.reportLayout", json!({"layout": layout})).unwrap();
            assert!(check_generated_formulas(&mut s) > 0, "{p} {layout}");
        }
        s.execute("pivot.subtotals", json!({"mode": "bottom"})).unwrap();
        check_generated_formulas(&mut s);
    }
    // Two value fields in rows, a date field grouped by quarters in the columns.
    let mut s = pivot(json!({"rows": ["Region"], "columns": ["Date"], "values": ["Sales", "Units"]}));
    s.execute("pivot.moveField", json!({"field": "Σ Values", "from": "columns", "to": "rows", "position": 0})).unwrap();
    s.execute("pivot.group", json!({"field": "Date", "by": "quarters"})).unwrap();
    assert!(check_generated_formulas(&mut s) > 0);
}

#[test]
fn generated_formula_text_is_excels() {
    let mut s = pivot(json!({"rows": ["Region", "Date"], "values": ["Sales"]}));
    let d = s.doc().unwrap();
    let (wb, sh) = (d.wb.clone(), d.wb.active_sheet);
    let f = |c: &str, from: usize, locale| gridcraft_engine::pivot::getpivotdata_formula(&wb, from, sh, at(c), locale);
    // A4 East (subtotal on top), A5 its first date.
    assert_eq!(f("B4", sh, FormulaLocale::En).as_deref(), Some(r#"GETPIVOTDATA("Sum of Sales",$A$3,"Region","East")"#));
    assert_eq!(f("B5", sh, FormulaLocale::En).as_deref(), Some(r#"GETPIVOTDATA("Sum of Sales",$A$3,"Region","East","Date",DATE(2026,1,15))"#));
    assert_eq!(f("B5", 1, FormulaLocale::En).as_deref(), Some(r#"GETPIVOTDATA("Sum of Sales",Sheet2!$A$3,"Region","East","Date",DATE(2026,1,15))"#));
    assert_eq!(f("B5", sh, FormulaLocale::Es).as_deref(), Some(r#"GETPIVOTDATA("Sum of Sales";$A$3;"Region";"East";"Date";DATE(2026;1;15))"#));
    let last = wb.active().unwrap().pivots[0].last_range.unwrap().end;
    assert_eq!(f(&last.a1(), sh, FormulaLocale::En).as_deref(), Some(r#"GETPIVOTDATA("Sum of Sales",$A$3)"#));
    assert_eq!(f("A4", sh, FormulaLocale::En), None, "a label isn't a value");
    assert_eq!(f("B3", sh, FormulaLocale::En), None, "nor a header");
    assert_eq!(f("C4", sh, FormulaLocale::En), None, "outside the report");
    let _ = &mut s;
}

#[test]
fn xlsx_round_trip_keeps_and_evaluates_getpivotdata() {
    let mut s = pivot(json!({"rows": ["Region", "Product"], "values": ["Sales"]}));
    s.execute("cell.set", json!({"cell": "Z1", "input": r#"=GETPIVOTDATA("Sales",$A$3,"Region","West")"#})).unwrap();
    s.execute("cell.set", json!({"cell": "Z2", "input": r#"=GETPIVOTDATA("Sum of Sales",$A$3,"Product","Plums")"#})).unwrap();
    let b64 = s.execute("file.saveBytes", json!({"format": "xlsx"})).unwrap()["base64"].as_str().unwrap().to_string();
    let mut t = Session::new();
    t.execute("file.open", json!({"name": "pivot.xlsx", "base64": b64})).unwrap();
    let d = t.doc().unwrap();
    let si = d.wb.sheet_index("Sheet2").unwrap();
    let sh = d.wb.sheet(si).unwrap();
    assert_eq!(sh.cell(at("Z1")).unwrap().formula.as_ref().unwrap().text, r#"GETPIVOTDATA("Sales",$A$3,"Region","West")"#);
    assert_eq!(sh.value(at("Z1")), n(190.0), "evaluated on open");
    assert_eq!(sh.value(at("Z2")), n(50.0));
    // Still right after a refresh in the reopened workbook.
    t.execute("sheet.activate", json!({"sheet": "Sheet2"})).unwrap();
    t.execute("selection.set", json!({"range": "A3"})).unwrap();
    t.execute("pivot.refresh", json!({})).unwrap();
    assert_eq!(t.doc().unwrap().wb.active().unwrap().value(at("Z1")), n(190.0));
}

#[test]
fn a_report_that_no_longer_matches_its_file_isnt_read() {
    // A workbook whose report cells don't hold what the PivotTable computes (a stale file):
    // GETPIVOTDATA gives #REF! until the PivotTable is refreshed.
    let mut s = pivot(json!({"rows": ["Region"], "values": ["Sales"]}));
    let mut wb = (*s.doc().unwrap().wb).clone();
    let si = wb.active_sheet;
    let sh = wb.sheet_mut(si).unwrap();
    sh.pivots[0].report = Default::default();
    sh.cells.set(at("B4"), gridcraft_engine::model::Cell::value(n(1.0)));
    sh.cells.set(
        at("Z1"),
        gridcraft_engine::model::Cell {
            formula: Some(std::sync::Arc::new(gridcraft_engine::model::Formula::new(r#"GETPIVOTDATA("Sales",$A$3)"#))),
            ..Default::default()
        },
    );
    let b64 = gridcraft_engine::io::base64_encode(&gridcraft_engine::io::save_bytes(&wb, "stale.xlsx").unwrap());
    let mut t = Session::new();
    t.execute("file.open", json!({"name": "stale.xlsx", "base64": b64})).unwrap();
    t.execute("sheet.activate", json!({"sheet": "Sheet2"})).unwrap();
    assert_eq!(t.doc().unwrap().wb.active().unwrap().value(at("Z1")), REF);
    t.execute("selection.set", json!({"range": "A3"})).unwrap();
    t.execute("pivot.refresh", json!({})).unwrap();
    assert_eq!(t.doc().unwrap().wb.active().unwrap().value(at("Z1")), n(450.0));
    let _ = &mut s;
}
