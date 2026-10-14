//! Cell references into tables rewritten as table names: `C2:C9` → `Table1[Qty]`; one row of
//! the table from a formula inside it relative to the formula's row (`[@Qty]`,
//! `[#Prev Row][Qty]`), any other by its key in the first column (`Table1["Widget"][Qty]`),
//! when that column's entries are unique.

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
    // Inside its own table a reference leaves out the table name, as Excel writes it (`[@Qty]`).
    let inside = at.is_some_and(|c| t.range.contains(c));
    let name = if inside { String::new() } else { t.name.clone() };
    let reference = |specifiers: Vec<StructItem>| {
        Expr::Struct(StructRef { table: name.clone(), specifiers, col_start: col_start.clone(), col_end: col_end.clone(), col_abs: false })
    };
    let specifiers = match () {
        _ if Some(rows) == data => vec![],
        _ if rows == (top, bottom) => vec![StructItem::All],
        _ if t.header_row && rows == (top, top) => vec![StructItem::Headers],
        _ if t.totals_row && rows == (bottom, bottom) => vec![StructItem::Totals],
        _ if t.header_row && data.is_some_and(|(_, d1)| rows == (top, d1)) => vec![StructItem::Headers, StructItem::Data],
        _ if t.totals_row && data.is_some_and(|(d0, _)| rows == (d0, bottom)) => vec![StructItem::Data, StructItem::Totals],
        // One row, from a formula inside the table: its own row, the row above or below it,
        // which fill down with the formula (`[@Qty]`, `[#Prev Row][Qty]`).
        _ if in_data && rows.0 == rows.1 && inside => {
            let item = match at.map(|c| i64::from(rows.0) - i64::from(c.row)) {
                Some(0) => StructItem::ThisRow,
                Some(-1) => StructItem::PrevRow,
                Some(1) => StructItem::NextRow,
                _ => return keyed(sh, t, &name, rows.0, col_start.as_deref(), col_end.as_deref()),
            };
            return Some(reference(vec![item]));
        }
        // Any other row by its key in the first column, the same from anywhere.
        _ if in_data && rows.0 == rows.1 => return keyed(sh, t, &name, rows.0, col_start.as_deref(), col_end.as_deref()),
        _ => return None,
    };
    // The whole data body is the table's bare name, unless a defined name hides it.
    if specifiers.is_empty() && col_start.is_none() && wb.name(&t.name, formula_sheet).is_none() {
        return Some(Expr::Name(t.name.clone()));
    }
    Some(reference(specifiers))
}

/// The first column's entry on `row` as the literal of a key that finds that row: `None` when
/// it can't be one (empty, an error, a text with `*`, `?` or `~`, which an exact match takes as
/// wildcards) or when the column's entries aren't unique, so no key reliably finds a row.
pub fn row_key(sh: &Sheet, t: &Table, row: u32) -> Option<Expr> {
    let data = t.data_range()?;
    let key_col = t.range.start.col;
    // Keys as an exact MATCH compares them: texts ignoring case.
    let normal = |v: &Value| match v {
        Value::Text(s) if !s.is_empty() => Some(format!("t{}", s.to_lowercase())),
        Value::Number(n) => Some(format!("n{n}")),
        Value::Bool(b) => Some(format!("b{b}")),
        _ => None,
    };
    let mut seen = std::collections::HashSet::new();
    for r in data.start.row..=data.end.row {
        if let Some(k) = normal(&sh.value(CellRef::new(r, key_col)))
            && !seen.insert(k)
        {
            return None;
        }
    }
    match sh.value(CellRef::new(row, key_col)) {
        Value::Text(s) if !s.is_empty() && !s.contains(['*', '?', '~']) => Some(Expr::Text(s)),
        Value::Number(n) if n.is_finite() => Some(Expr::Number(n)),
        Value::Bool(b) => Some(Expr::Bool(b)),
        _ => None,
    }
}

/// `Table1["Widget"][Qty]` (`[Qty:Price]`, or `[]` for the whole row) for `row`, by its key.
fn keyed(sh: &Sheet, t: &Table, name: &str, row: u32, first: Option<&str>, last: Option<&str>) -> Option<Expr> {
    let key = row_key(sh, t, row)?;
    let columns = first.map(|first| ColumnSpan { first: first.to_string(), last: last.map(str::to_string), abs: false }).into_iter().collect();
    Some(Expr::Table(TableRef { table: name.to_string(), rows: RowSel::Key(Box::new(key)), columns }))
}

/// F4's row anchor for a reference to one row of a table, in a formula in `at` of `sheet`:
/// a relative row (`[@Qty]`, `[#Prev Row][Qty]`) becomes the same cells by key
/// (`["Bolt"][Qty]`), and a key back to the relative row when the formula is inside the table
/// on that row or next to it. `None` when the other form can't name those cells.
pub fn toggle_row_anchor(wb: &Workbook, sheet: usize, at: CellRef, e: &Expr) -> Option<Expr> {
    let table_of = |name: &str| -> Option<(usize, &Sheet, &Table)> {
        if name.is_empty() {
            let sh = wb.sheet(sheet)?;
            return sh.table_at(at).map(|t| (sheet, sh, t));
        }
        let (si, ti) = wb.table(name)?;
        let sh = wb.sheet(si)?;
        sh.tables.get(ti).map(|t| (si, sh, t))
    };
    match e {
        Expr::Struct(s) if s.col_start.is_some() => {
            let offset = match s.specifiers.as_slice() {
                [StructItem::ThisRow] => 0,
                [StructItem::PrevRow] => -1,
                [StructItem::NextRow] => 1,
                _ => return None,
            };
            let (_, sh, t) = table_of(&s.table)?;
            let row = u32::try_from(i64::from(at.row) + offset).ok()?;
            let data = t.data_range()?;
            if row < data.start.row || row > data.end.row {
                return None;
            }
            let span = ColumnSpan { first: s.col_start.clone()?, last: s.col_end.clone(), abs: s.col_abs };
            Some(Expr::Table(TableRef { table: s.table.clone(), rows: RowSel::Key(Box::new(row_key(sh, t, row)?)), columns: vec![span] }))
        }
        Expr::Table(TableRef { table, rows: RowSel::Key(key), columns }) => {
            let [span] = columns.as_slice() else { return None };
            let (si, sh, t) = table_of(table)?;
            if si != sheet || !t.range.contains(at) {
                return None;
            }
            // The row the key finds.
            let data = t.data_range()?;
            // An exact MATCH ignores the case of texts.
            let same = |a: &Expr| match (a, &**key) {
                (Expr::Text(a), Expr::Text(b)) => a.to_lowercase() == b.to_lowercase(),
                (a, b) => a == b,
            };
            let row = (data.start.row..=data.end.row).find(|&r| row_key(sh, t, r).as_ref().is_some_and(same))?;
            let item = match i64::from(row) - i64::from(at.row) {
                0 => StructItem::ThisRow,
                -1 => StructItem::PrevRow,
                1 => StructItem::NextRow,
                _ => return None,
            };
            Some(Expr::Struct(StructRef {
                table: table.clone(),
                specifiers: vec![item],
                col_start: Some(span.first.clone()),
                col_end: span.last.clone(),
                col_abs: span.abs,
            }))
        }
        _ => None,
    }
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
        // The first column's entries are unique once the second "bolt" is renamed.
        s.execute("range.setValues", json!({"range": "A5", "values": [["Screw"]]})).unwrap();
        let check = |s: &mut Session, cell: &str, input: &str, named: &str| {
            set(s, cell, input);
            let before = value(s, cell);
            let shown = s.execute("formulas.tableNames", json!({"formula": input, "cell": cell})).unwrap();
            assert_eq!(shown["formula"], named, "{input}");
            let changed = s.execute("formulas.useTableNames", json!({"range": cell})).unwrap()["changed"].as_u64();
            assert_eq!(changed, Some(u64::from(input != named)), "{input}");
            assert_eq!(formula(s, cell), named);
            assert_eq!(value(s, cell), before, "{input}");
        };
        // Outside the table every row is found by its key, even the formula's own row.
        check(&mut s, "E2", "=B2*$C$2", "=Stock[\"Nut\"][Qty]*Stock[\"Nut\"][Unit Price]");
        check(&mut s, "E3", "=SUM(B2:B5)+SUM(Sheet1!A2:C5)", "=SUM(Stock[Qty])+SUM(Stock)");
        check(&mut s, "E4", "=COUNTA(A1:C1)&B1&COUNTA(A1:C5)", "=COUNTA(Stock[#Headers])&Stock[[#Headers],[Qty]]&COUNTA(Stock[#All])");
        check(
            &mut s,
            "E5",
            "=SUM(B2:C5)+B2+C4+SUM(A3:C3)",
            "=SUM(Stock[[Qty]:[Unit Price]])+Stock[\"Nut\"][Qty]+Stock[\"Washer\"][Unit Price]+SUM(Stock[\"Bolt\"][])",
        );
        // Not a part of the table a name stands for: a block of rows, cells outside.
        check(&mut s, "E7", "=SUM(B2:B3)+F9+SUM(A:A)", "=SUM(B2:B3)+F9+SUM(A:A)");
        // When the first column repeats an entry, no key finds a row reliably: rows stay addresses.
        s.execute("range.setValues", json!({"range": "A5", "values": [["bolt"]]})).unwrap();
        check(&mut s, "E8", "=B2+SUM(C2:C5)", "=B2+SUM(Stock[Unit Price])");
        // Inside the table: its own row and the rows next to it relative to the formula's, which
        // fill down with it; others by key, without the table name.
        s.execute("range.setValues", json!({"range": "H1", "values": [["Code", "Twice"], [4, null], [6, null], [8, null]]})).unwrap();
        s.execute("insert.table", json!({"range": "H1:I4", "header": true, "name": "Codes"})).unwrap();
        check(&mut s, "I2", "=H2*2+H3+H4", "=[@Code]*2+[#Next Row][Code]+[8][Code]");
        assert_eq!(value(&s, "I2"), Value::Number(22.0));
        s.execute("edit.undo", json!({})).unwrap();
        assert_eq!(formula(&mut s, "I2"), "=H2*2+H3+H4");
    }

    #[test]
    fn f4_toggles_a_row_between_relative_and_key() {
        let mut s = stock();
        s.execute("range.setValues", json!({"range": "A5", "values": [["Screw"]]})).unwrap();
        let wb = &s.doc().unwrap().wb;
        let at = gridcraft_core::CellRef::parse("C3").unwrap();
        let toggle = |src: &str| super::toggle_row_anchor(wb, 0, at, &gridcraft_formula::parse(src).unwrap()).map(|e| gridcraft_formula::print(&e));
        assert_eq!(toggle("[@Qty]").as_deref(), Some("[\"Bolt\"][Qty]"));
        assert_eq!(toggle("[#Prev Row][$Qty]").as_deref(), Some("[\"Nut\"][$Qty]"));
        assert_eq!(toggle("Stock[#Next Row][Qty:Unit Price]").as_deref(), Some("Stock[\"Washer\"][Qty:Unit Price]"));
        assert_eq!(toggle("[\"BOLT\"][Qty]").as_deref(), Some("[@Qty]"));
        assert_eq!(toggle("Stock[\"Washer\"][Qty]").as_deref(), Some("Stock[#Next Row][Qty]"));
        // A key two rows away has no relative form; a whole column has no row to toggle.
        assert_eq!(toggle("[\"Screw\"][Qty]"), None);
        assert_eq!(toggle("Stock[Qty]"), None);
        // From outside the table a key stays a key.
        let outside = super::toggle_row_anchor(
            wb,
            0,
            gridcraft_core::CellRef::parse("E3").unwrap(),
            &gridcraft_formula::parse("Stock[\"Bolt\"][Qty]").unwrap(),
        );
        assert_eq!(outside, None);
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
