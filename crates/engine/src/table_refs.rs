//! Cell references into tables rewritten as table names: `C2:C9` → `Table1[Qty]`, `C5` →
//! `Table1[@Qty]` on its own row and `Table1["Widget"][Qty]` by its key on any other.

use gridcraft_core::{CellRef, RangeRef, Value};
use gridcraft_formula::{ColumnSpan, Expr, RefKind, RowSel, SheetSel, StructItem, StructRef, TableRef};
use gridcraft_model::{Sheet, Table, Workbook};

/// Rewrites every A1 reference of `e` (a formula in cell `at` of `sheet`) that names a part of
/// a table as the structured reference to it. References that are no such part (a block of
/// rows inside the data, cells outside any table) stay; `None` when nothing changed.
pub fn use_table_names(wb: &Workbook, sheet: usize, at: CellRef, e: Expr) -> Option<Expr> {
    let mut changed = false;
    let out = e.map(&mut |x| match x {
        Expr::Ref(r) => {
            let target = match &r.sheet {
                SheetSel::Current => Some(sheet),
                SheetSel::Named(n) => wb.sheet_index(n),
                SheetSel::Span(..) => None,
            };
            let named = match (target, &r.kind) {
                (Some(si), RefKind::Cell(_) | RefKind::Range(..)) => {
                    wb.sheet(si).and_then(|sh| named(wb, sh, r.range(), (si == sheet).then_some(at), sheet))
                }
                _ => None,
            };
            match named {
                Some(n) => {
                    changed = true;
                    n
                }
                None => Expr::Ref(r),
            }
        }
        other => other,
    });
    changed.then_some(out)
}

/// The same for formula text (with or without `=`); `None` when it doesn't parse or nothing changed.
pub fn use_table_names_in_text(wb: &Workbook, sheet: usize, at: CellRef, text: &str) -> Option<String> {
    let body = text.strip_prefix('=')?;
    let e = gridcraft_formula::parse(body).ok()?;
    use_table_names(wb, sheet, at, e).map(|e| format!("={}", gridcraft_formula::print(&e)))
}

/// `range` on `sh` as a reference by table name. `at` is the formula's cell when it is on `sh`.
fn named(wb: &Workbook, sh: &Sheet, range: RangeRef, at: Option<CellRef>, formula_sheet: usize) -> Option<Expr> {
    let t = sh.tables.iter().find(|t| t.range.contains(range.start) && t.range.contains(range.end))?;
    let column = |col: u32| t.columns.get(col.checked_sub(t.range.start.col)? as usize).map(|c| c.name.clone());
    let whole_width = range.start.col == t.range.start.col && range.end.col == t.range.end.col;
    let (col_start, col_end) = match () {
        _ if whole_width && t.columns.len() > 1 => (None, None),
        _ if range.start.col == range.end.col => (Some(column(range.start.col)?), None),
        _ => (Some(column(range.start.col)?), Some(column(range.end.col)?)),
    };
    let rows = (range.start.row, range.end.row);
    let data = t.data_range().map(|d| (d.start.row, d.end.row));
    let (top, bottom) = (t.range.start.row, t.range.end.row);
    let in_data = data.is_some_and(|(d0, d1)| d0 <= rows.0 && rows.1 <= d1);
    let reference = |table: String, specifiers: Vec<StructItem>| {
        Expr::Struct(StructRef { table, specifiers, col_start: col_start.clone(), col_end: col_end.clone(), col_abs: false })
    };
    let specifiers = match () {
        _ if Some(rows) == data => vec![],
        _ if rows == (top, bottom) => vec![StructItem::All],
        _ if t.header_row && rows == (top, top) => vec![StructItem::Headers],
        _ if t.totals_row && rows == (bottom, bottom) => vec![StructItem::Totals],
        _ if t.header_row && data.is_some_and(|(_, d1)| rows == (top, d1)) => vec![StructItem::Headers, StructItem::Data],
        _ if t.totals_row && data.is_some_and(|(d0, _)| rows == (d0, bottom)) => vec![StructItem::Data, StructItem::Totals],
        // The formula's own row: `Table1[@Qty]`, and `[@Qty]` inside the table.
        _ if in_data && rows.0 == rows.1 && at.is_some_and(|c| c.row == rows.0) => {
            let inside = at.is_some_and(|c| t.range.contains(c));
            return Some(reference(if inside { String::new() } else { t.name.clone() }, vec![StructItem::ThisRow]));
        }
        // The row above or below the formula's: `Table1[#Prev Row][Qty]`, which fills down.
        _ if in_data
            && rows.0 == rows.1
            && col_end.is_none()
            && at.is_some_and(|c| t.range.start.row <= c.row && c.row <= bottom && c.row.abs_diff(rows.0) == 1) =>
        {
            let inside = at.is_some_and(|c| t.range.contains(c));
            let item = if at.is_some_and(|c| c.row > rows.0) { StructItem::PrevRow } else { StructItem::NextRow };
            return Some(reference(if inside { String::new() } else { t.name.clone() }, vec![item]));
        }
        // One cell of another row: by the key in the table's first column.
        _ if in_data && range.is_single() => return keyed(sh, t, range.start, col_start.clone()?),
        _ => return None,
    };
    // The whole data body is the table's bare name, unless a defined name hides it.
    if specifiers.is_empty() && col_start.is_none() && wb.name(&t.name, formula_sheet).is_none() {
        return Some(Expr::Name(t.name.clone()));
    }
    Some(reference(t.name.clone(), specifiers))
}

/// `Table1["Widget"][Qty]` for a data cell, when the first column's entry on its row is a key
/// that finds this row: a text, number or boolean no earlier row repeats.
fn keyed(sh: &Sheet, t: &Table, cell: CellRef, column: String) -> Option<Expr> {
    let data = t.data_range()?;
    let key_col = t.range.start.col;
    let key = sh.value(CellRef::new(cell.row, key_col));
    let literal = match &key {
        // `*`, `?` and `~` are wildcards to an exact-match lookup.
        Value::Text(s) if !s.is_empty() && !s.contains(['*', '?', '~']) => Expr::Text(s.clone()),
        Value::Number(n) if n.is_finite() => Expr::Number(*n),
        Value::Bool(b) => Expr::Bool(*b),
        _ => return None,
    };
    let same = |v: &Value| match (v, &key) {
        (Value::Text(a), Value::Text(b)) => a.to_lowercase() == b.to_lowercase(),
        (Value::Number(a), Value::Number(b)) => a == b,
        (Value::Bool(a), Value::Bool(b)) => a == b,
        _ => false,
    };
    if (data.start.row..cell.row).any(|row| same(&sh.value(CellRef::new(row, key_col)))) {
        return None;
    }
    Some(Expr::Table(TableRef { table: t.name.clone(), rows: RowSel::Key(Box::new(literal)), columns: vec![ColumnSpan::one(&column)] }))
}

#[cfg(test)]
mod tests {
    use gridcraft_core::{CellError, Value};
    use serde_json::json;

    use crate::Session;

    /// `Stock` in A1:C5: Product, Qty, Price over four rows, "Bolt" twice.
    fn stock() -> Session {
        let mut s = Session::new();
        s.new_workbook();
        let rows = json!([["Product", "Qty", "Unit Price"], ["Nut", 5, 0.1], ["Bolt", 7, 0.25], ["Washer", 9, 0.05], ["bolt", 1, 9]]);
        s.execute("range.setValues", json!({"range": "A1", "values": rows})).unwrap();
        s.execute("insert.table", json!({"range": "A1:C5", "header": true, "name": "Stock"})).unwrap();
        s
    }

    fn set(s: &mut Session, cell: &str, input: &str) {
        s.execute("cell.set", json!({"cell": cell, "input": input})).unwrap();
    }

    fn formula(s: &mut Session, cell: &str) -> String {
        s.execute("cell.get", json!({"cell": cell})).unwrap()["formula"].as_str().unwrap_or_default().to_string()
    }

    fn value(s: &Session, cell: &str) -> Value {
        s.doc().unwrap().wb.sheet(0).unwrap().value(gridcraft_core::CellRef::parse(cell).unwrap())
    }

    #[test]
    fn keyed_references_look_a_cell_up_by_its_row_key() {
        let mut s = stock();
        set(&mut s, "E1", "=Stock[\"Washer\"][Qty]");
        set(&mut s, "E2", "=Stock[Qty=7][Product]");
        set(&mut s, "E3", "=Stock[\"BOLT\"][Unit Price]*Stock[E2][Qty]");
        set(&mut s, "E4", "=Stock[\"Screw\"][Qty]");
        set(&mut s, "E5", "=Stock[\"Nut\"][Colour]");
        set(&mut s, "E6", "=Stock[[Unit Price]=0.05][Qty]");
        assert_eq!(formula(&mut s, "E3"), "=Stock[\"BOLT\"][Unit Price]*Stock[E2][Qty]");
        assert_eq!(value(&s, "E1"), Value::Number(9.0));
        assert_eq!(value(&s, "E2"), Value::from("Bolt"));
        assert_eq!(value(&s, "E3"), Value::Number(1.75));
        assert_eq!(value(&s, "E4"), Value::Error(CellError::NA));
        // No such key is #N/A; no such column #REF!, as for `Stock[Colour]`.
        assert_eq!(value(&s, "E5"), Value::Error(CellError::Ref));
        assert_eq!(value(&s, "E6"), Value::Number(9.0));
        // They follow the table's data, and its table and column names.
        set(&mut s, "B4", "40");
        assert_eq!(value(&s, "E1"), Value::Number(40.0));
        set(&mut s, "B1", "Count");
        s.execute("formulas.defineName", json!({"name": "WasherQty", "refersTo": "=Stock[\"Washer\"][Count]+SUM(Stock[Count])*0"})).unwrap();
        set(&mut s, "E7", "=WasherQty");
        s.execute("table.rename", json!({"table": "Stock", "name": "Parts"})).unwrap();
        // Defined names follow too.
        assert_eq!(s.doc().unwrap().wb.names[0].formula, "Parts[\"Washer\"][Count]+SUM(Parts[Count])*0");
        assert_eq!(value(&s, "E7"), Value::Number(40.0));
        set(&mut s, "B1", "Units");
        assert_eq!(s.doc().unwrap().wb.names[0].formula, "Parts[\"Washer\"][Units]+SUM(Parts[Units])*0");
        assert_eq!(value(&s, "E7"), Value::Number(40.0));
        set(&mut s, "B1", "Count");
        assert_eq!(formula(&mut s, "E1"), "=Parts[\"Washer\"][Count]");
        assert_eq!(formula(&mut s, "E2"), "=Parts[Count=7][Product]");
        assert_eq!(value(&s, "E1"), Value::Number(40.0));
    }

    #[test]
    fn keyed_references_are_lookups_in_files() {
        let mut s = stock();
        set(&mut s, "E1", "=Stock[\"Washer\"][Qty]");
        set(&mut s, "E2", "=Stock[Qty=7][Product]");
        let bytes = gridcraft_xlsx::write_xlsx(&s.doc().unwrap().wb).unwrap();
        let mut sheet = String::new();
        let mut zip = zip::ZipArchive::new(std::io::Cursor::new(bytes)).unwrap();
        std::io::Read::read_to_string(&mut zip.by_name("xl/worksheets/sheet1.xml").unwrap(), &mut sheet).unwrap();
        // By key, a plain formula every Excel version reads; by conditions, a dynamic-array FILTER.
        assert!(sheet.contains("<f>INDEX(Stock[Qty],MATCH(\"Washer\",INDEX(Stock[],0,1),0))</f>"), "{sheet}");
        assert!(sheet.contains("<f t=\"array\" ref=\"E2\">_xlfn._xlws.FILTER(Stock[Product],Stock[Qty]=7,NA())</f>"), "{sheet}");
        let r = s.execute("file.saveBytes", json!({"format": "xlsx"})).unwrap();
        s.execute("file.open", json!({"name": "x.xlsx", "base64": r["base64"]})).unwrap();
        assert_eq!(formula(&mut s, "E1"), "=Stock[\"Washer\"][Qty]");
        assert_eq!(formula(&mut s, "E2"), "=Stock[Qty=7][Product]");
        assert_eq!((value(&s, "E1"), value(&s, "E2")), (Value::Number(9.0), Value::from("Bolt")));
    }

    #[test]
    fn a1_references_become_table_names() {
        let mut s = stock();
        for (cell, input, named) in [
            ("E2", "=B2*$C$2", "=Stock[@Qty]*Stock[@[Unit Price]]"),
            ("E3", "=SUM(B2:B5)+SUM(Sheet1!A2:C5)", "=SUM(Stock[Qty])+SUM(Stock)"),
            ("E4", "=COUNTA(A1:C1)&B1&COUNTA(A1:C5)", "=COUNTA(Stock[#Headers])&Stock[[#Headers],[Qty]]&COUNTA(Stock[#All])"),
            (
                "E5",
                "=SUM(B2:C5)+B2+C4+A3",
                "=SUM(Stock[[Qty]:[Unit Price]])+Stock[\"Nut\"][Qty]+Stock[#Prev Row][Unit Price]+Stock[\"Bolt\"][Product]",
            ),
            // Not a part of the table a name stands for: a block of rows, the second "bolt"
            // (the lookup would find the first), cells outside.
            ("E7", "=SUM(B2:B3)+B5+F9+SUM(A:A)", "=SUM(B2:B3)+B5+F9+SUM(A:A)"),
        ] {
            set(&mut s, cell, input);
            let before = value(&s, cell);
            let shown = s.execute("formulas.tableNames", json!({"formula": input, "cell": cell})).unwrap();
            assert_eq!(shown["formula"], named, "{input}");
            let changed = s.execute("formulas.useTableNames", json!({"range": cell})).unwrap()["changed"].as_u64();
            assert_eq!(changed, Some(u64::from(input != named)), "{input}");
            assert_eq!(formula(&mut s, cell), named);
            assert_eq!(value(&s, cell), before, "{input}");
        }
        // Inside the table, its own row needs no table name.
        s.execute("range.setValues", json!({"range": "H1", "values": [["Code", "Twice"], [4, null], [6, null]]})).unwrap();
        s.execute("insert.table", json!({"range": "H1:I3", "header": true, "name": "Codes"})).unwrap();
        set(&mut s, "I2", "=H2*2+H3");
        s.execute("formulas.useTableNames", json!({"range": "I2"})).unwrap();
        assert_eq!(formula(&mut s, "I2"), "=[@Code]*2+[#Next Row][Code]");
        assert_eq!(value(&s, "I2"), Value::Number(14.0));
        s.execute("edit.undo", json!({})).unwrap();
        assert_eq!(formula(&mut s, "I2"), "=H2*2+H3");
    }

    #[test]
    fn anchored_columns_stay_when_filled_sideways() {
        let mut s = stock();
        set(&mut s, "E2", "=Stock[@Qty]&SUM(Stock[$Qty])&Stock[\"Nut\"][$Qty]&Stock[$Product=\"Nut\"][Qty]");
        s.execute("edit.autoFill", json!({"source": "E2", "target": "E2:G2"})).unwrap();
        assert_eq!(formula(&mut s, "F2"), "=Stock[@[Unit Price]]&SUM(Stock[$Qty])&Stock[\"Nut\"][$Qty]&Stock[$Product=\"Nut\"][Unit Price]");
        // Past the last column the table starts again.
        assert_eq!(formula(&mut s, "G2"), "=Stock[@Product]&SUM(Stock[$Qty])&Stock[\"Nut\"][$Qty]&Stock[$Product=\"Nut\"][Product]");
        assert_eq!(value(&s, "F2"), Value::from("0.12250.1"));
        // Down a column nothing moves, nor when the formula is copied.
        s.execute("edit.autoFill", json!({"source": "E2", "target": "E2:E3"})).unwrap();
        assert_eq!(formula(&mut s, "E3"), formula(&mut s, "E2"));
        s.execute("selection.set", json!({"range": "E2"})).unwrap();
        s.execute("edit.copy", json!({})).unwrap();
        s.execute("selection.set", json!({"range": "H2"})).unwrap();
        s.execute("edit.paste", json!({})).unwrap();
        assert_eq!(formula(&mut s, "H2"), formula(&mut s, "E2"));
        // Files hold Excel's own spelling of an anchored column, the one-column range.
        let bytes = gridcraft_xlsx::write_xlsx(&s.doc().unwrap().wb).unwrap();
        let mut sheet = String::new();
        let mut zip = zip::ZipArchive::new(std::io::Cursor::new(bytes)).unwrap();
        std::io::Read::read_to_string(&mut zip.by_name("xl/worksheets/sheet1.xml").unwrap(), &mut sheet).unwrap();
        assert!(sheet.contains("SUM(Stock[[Qty]:[Qty]])&amp;INDEX(Stock[[Qty]:[Qty]],MATCH(\"Nut\",INDEX(Stock[],0,1),0))"), "{sheet}");
        assert!(sheet.contains("_xlfn._xlws.FILTER(Stock[Qty],Stock[[Product]:[Product]]=\"Nut\",NA())"), "{sheet}");
        let before = formula(&mut s, "E2");
        let r = s.execute("file.saveBytes", json!({"format": "xlsx"})).unwrap();
        s.execute("file.open", json!({"name": "x.xlsx", "base64": r["base64"]})).unwrap();
        assert_eq!(formula(&mut s, "E2"), before);
        assert_eq!(value(&s, "F2"), Value::from("0.12250.1"));
    }

    #[test]
    fn conditions_filter_every_matching_row() {
        let mut s = stock();
        // Several rows match: the values spill down.
        set(&mut s, "E1", "=Stock[Qty>=5,[Unit Price]<1][Product]");
        assert_eq!(
            [value(&s, "E1"), value(&s, "E2"), value(&s, "E3"), value(&s, "E4")],
            [Value::from("Nut"), Value::from("Bolt"), Value::from("Washer"), Value::Empty]
        );
        // One row: one value. None: #N/A, as for a key.
        set(&mut s, "G1", "=Stock[Product=\"bolt\",[Unit Price]>1][Qty]");
        set(&mut s, "G2", "=Stock[Qty>100][Qty]");
        set(&mut s, "G3", "=SUM(Stock[Product<>\"Nut\"][Qty])");
        assert_eq!((value(&s, "G1"), value(&s, "G2"), value(&s, "G3")), (Value::Number(1.0), Value::Error(CellError::NA), Value::Number(17.0)));
        // Conditions follow column renames, and the data.
        set(&mut s, "B5", "8");
        assert_eq!(value(&s, "G1"), Value::Number(8.0));
        set(&mut s, "C1", "Cost");
        assert_eq!(formula(&mut s, "E1"), "=Stock[Qty>=5,Cost<1][Product]");
        // They survive a file.
        let r = s.execute("file.saveBytes", json!({"format": "xlsx"})).unwrap();
        s.execute("file.open", json!({"name": "x.xlsx", "base64": r["base64"]})).unwrap();
        assert_eq!(formula(&mut s, "E1"), "=Stock[Qty>=5,Cost<1][Product]");
        assert_eq!(formula(&mut s, "G1"), "=Stock[Product=\"bolt\",Cost>1][Qty]");
        assert_eq!((value(&s, "E3"), value(&s, "G3")), (Value::from("Washer"), Value::Number(24.0)));
    }

    #[test]
    fn keyed_references_inside_their_own_table() {
        let mut s = stock();
        s.execute("range.setValues", json!({"range": "D1", "values": [["Vs Bolt"]]})).unwrap();
        s.execute("table.resize", json!({"table": "Stock", "range": "A1:D5"})).unwrap();
        // The table name may be left out inside the table, as for `[@Qty]`.
        set(&mut s, "D2", "=[@Qty]-[\"Bolt\"][Qty]");
        set(&mut s, "D3", "=COUNT([Qty>[@Qty]][Qty])");
        assert_eq!(formula(&mut s, "D2"), "=[@Qty]-[\"Bolt\"][Qty]");
        assert_eq!((value(&s, "D2"), value(&s, "D3")), (Value::Number(-2.0), Value::Number(1.0)));
        // Excel reads these without the table name too (observed in Excel 16.113.4).
        let bytes = gridcraft_xlsx::write_xlsx(&s.doc().unwrap().wb).unwrap();
        let mut sheet = String::new();
        let mut zip = zip::ZipArchive::new(std::io::Cursor::new(bytes)).unwrap();
        std::io::Read::read_to_string(&mut zip.by_name("xl/worksheets/sheet1.xml").unwrap(), &mut sheet).unwrap();
        assert!(sheet.contains("[[#This Row],[Qty]]-INDEX([Qty],MATCH(\"Bolt\",INDEX([],0,1),0))"), "{sheet}");
        let r = s.execute("file.saveBytes", json!({"format": "xlsx"})).unwrap();
        s.execute("file.open", json!({"name": "x.xlsx", "base64": r["base64"]})).unwrap();
        assert_eq!(formula(&mut s, "D3"), "=COUNT([Qty>[@Qty]][Qty])");
        assert_eq!(value(&s, "D3"), Value::Number(1.0));
    }

    #[test]
    fn neighbouring_rows_of_a_table() {
        let mut s = Session::new();
        s.new_workbook();
        s.execute(
            "range.setValues",
            json!({"range": "A1", "values": [["Day", "Qty", "Total", "Change"], [1, 5, null, null], [2, 7, null, null], [3, 9, null, null]]}),
        )
        .unwrap();
        s.execute("insert.table", json!({"range": "A1:D4", "header": true, "name": "Log"})).unwrap();
        // A running total and a change to the next day, filled down.
        set(&mut s, "C2", "=[@Qty]");
        set(&mut s, "C3", "=[@Qty]+[#Prev Row][Total]");
        set(&mut s, "D2", "=[#Next Row][Qty]-[@Qty]");
        s.execute("edit.autoFill", json!({"source": "C3", "target": "C3:C4"})).unwrap();
        s.execute("edit.autoFill", json!({"source": "D2", "target": "D2:D4"})).unwrap();
        assert_eq!(formula(&mut s, "C4"), "=[@Qty]+[#Prev Row][Total]");
        assert_eq!([value(&s, "C2"), value(&s, "C3"), value(&s, "C4")], [Value::Number(5.0), Value::Number(12.0), Value::Number(21.0)]);
        // Below the last row is the cell under the table, as OFFSET finds it in Excel.
        assert_eq!([value(&s, "D2"), value(&s, "D3"), value(&s, "D4")], [Value::Number(2.0), Value::Number(2.0), Value::Number(-9.0)]);
        // From outside the table, on a row of it.
        set(&mut s, "F3", "=Log[#Prev Row][Qty]");
        assert_eq!(value(&s, "F3"), Value::Number(5.0));
        // Files hold OFFSET, which reads back.
        let bytes = gridcraft_xlsx::write_xlsx(&s.doc().unwrap().wb).unwrap();
        let mut sheet = String::new();
        let mut zip = zip::ZipArchive::new(std::io::Cursor::new(bytes)).unwrap();
        std::io::Read::read_to_string(&mut zip.by_name("xl/worksheets/sheet1.xml").unwrap(), &mut sheet).unwrap();
        assert!(sheet.contains("<f>[[#This Row],[Qty]]+OFFSET([[#This Row],[Total]],-1,0)</f>"), "{sheet}");
        assert!(sheet.contains("<f>OFFSET(Log[[#This Row],[Qty]],-1,0)</f>"), "{sheet}");
        let r = s.execute("file.saveBytes", json!({"format": "xlsx"})).unwrap();
        s.execute("file.open", json!({"name": "x.xlsx", "base64": r["base64"]})).unwrap();
        assert_eq!(formula(&mut s, "C3"), "=[@Qty]+[#Prev Row][Total]");
        assert_eq!(formula(&mut s, "F3"), "=Log[#Prev Row][Qty]");
        assert_eq!(value(&s, "C4"), Value::Number(21.0));
        // Shift+F4 writes the cell above or below the formula's as a neighbouring row.
        set(&mut s, "C3", "=B3+C2");
        set(&mut s, "D3", "=B4-B3");
        s.execute("formulas.useTableNames", json!({"range": "C3:D3"})).unwrap();
        assert_eq!(formula(&mut s, "C3"), "=[@Qty]+[#Prev Row][Total]");
        assert_eq!(formula(&mut s, "D3"), "=[#Next Row][Qty]-[@Qty]");
    }

    #[test]
    fn several_columns_and_whole_rows() {
        let mut s = stock();
        let cells =
            |s: &Session, range: &str| -> Vec<Value> { gridcraft_core::RangeRef::parse(range).unwrap().iter().map(|c| value(s, &c.a1())).collect() };
        // Columns in the order named, spilling right; `[]` is every column.
        set(&mut s, "E1", "=Stock[\"Bolt\"][Qty, Product]");
        set(&mut s, "E2", "=Stock[\"Washer\"][]");
        set(&mut s, "E4", "=Stock[Qty>5][Product:Qty]");
        set(&mut s, "E7", "=SUM(Stock[Qty, \"Unit Price\"])");
        assert_eq!(cells(&s, "E1:F1"), [Value::Number(7.0), Value::from("Bolt")]);
        assert_eq!(cells(&s, "E2:G2"), [Value::from("Washer"), Value::Number(9.0), Value::Number(0.05)]);
        assert_eq!(cells(&s, "E4:F5"), [Value::from("Bolt"), Value::Number(7.0), Value::from("Washer"), Value::Number(9.0)]);
        assert!(matches!(value(&s, "E7"), Value::Number(n) if (n - 31.4).abs() < 1e-9));
        assert_eq!(formula(&mut s, "E7"), "=SUM(Stock[][Qty,Unit Price])");
        let before: Vec<String> = ["E1", "E2", "E4", "E7"].iter().map(|c| formula(&mut s, c)).collect();
        let r = s.execute("file.saveBytes", json!({"format": "xlsx"})).unwrap();
        s.execute("file.open", json!({"name": "x.xlsx", "base64": r["base64"]})).unwrap();
        let after: Vec<String> = ["E1", "E2", "E4", "E7"].iter().map(|c| formula(&mut s, c)).collect();
        assert_eq!(after, before);
        assert_eq!(cells(&s, "E4:F5"), [Value::from("Bolt"), Value::Number(7.0), Value::from("Washer"), Value::Number(9.0)]);
    }
}
