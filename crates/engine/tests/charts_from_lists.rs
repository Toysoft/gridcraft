//! Charts and PivotTables made from a cell inside an everyday list: a title line above the
//! headers, dates and text columns before the numbers, a table, a second chart next to a first.
use gridcraft_engine::Session;
use serde_json::{Value, json};

fn list() -> Session {
    let mut s = Session::new();
    s.new_workbook();
    s.execute(
        "range.setValues",
        json!({"range": "A1", "values": [
            ["SALES REPORT", null, null, null],
            ["Date", "Region", "Product", "Revenue"],
            ["2026-01-05", "North", "Shirt", 1200],
            ["2026-01-07", "South", "Pants", 800],
            ["2026-02-03", "North", "Pants", 950],
            ["2026-02-11", "Central", "Shirt", 640],
            ["2026-03-02", "South", "Shirt", 1430]
        ]}),
    )
    .unwrap();
    s.execute("selection.set", json!({"range": "B4"})).unwrap();
    s
}

fn charts(s: &Session) -> Vec<gridcraft_engine::model::Chart> {
    s.doc().unwrap().wb.active().unwrap().charts.clone()
}

#[test]
fn a_chart_from_one_cell_of_a_titled_list_plots_the_numbers_over_the_dates() {
    let mut s = list();
    s.execute("insert.chart", json!({"type": "column"})).unwrap();
    let c = &charts(&s)[0];
    assert_eq!(c.source.as_deref(), Some("Sheet1!A2:D7"), "the title line is left out");
    assert_eq!(c.series.len(), 1, "one Revenue series, not dates and text: {:?}", c.series);
    let se = &c.series[0];
    assert_eq!(se.name.as_deref(), Some("Sheet1!$D$2"));
    assert_eq!(se.values, "Sheet1!$D$3:$D$7");
    assert_eq!(se.categories.as_deref(), Some("Sheet1!$A$3:$A$7"), "the dates label the categories");
    assert_eq!(c.title.as_deref(), Some("Revenue"));
}

#[test]
fn a_second_chart_does_not_land_on_the_first() {
    let mut s = list();
    s.execute("insert.chart", json!({"type": "column"})).unwrap();
    s.execute("selection.set", json!({"range": "B4"})).unwrap();
    s.execute("insert.chart", json!({"type": "line"})).unwrap();
    let cs = charts(&s);
    let sh_rows = |c: &gridcraft_engine::model::Chart| c.anchor.cell.row;
    assert!(sh_rows(&cs[1]) > sh_rows(&cs[0]) + 10, "below the first: {:?} vs {:?}", cs[1].anchor, cs[0].anchor);
    // An explicit place is kept.
    s.execute("insert.chart", json!({"type": "pie", "at": "F2"})).unwrap();
    assert_eq!(charts(&s)[2].anchor.cell.a1(), "F2");
}

#[test]
fn a_pivot_table_from_one_cell_of_a_titled_list_works() {
    let mut s = list();
    let r: Value = s.execute("insert.pivotTable", json!({"rows": ["Region"], "values": ["Revenue"]})).unwrap();
    assert!(r.get("id").is_some(), "{r}");
    // The report: one line per region and the grand total.
    let pv = s.execute("pivot.list", json!({})).unwrap();
    assert!(pv.to_string().contains("A2:D7") || pv.to_string().contains("$A$2:$D$7"), "{pv}");
}

#[test]
fn a_chart_from_a_cell_in_a_table_uses_the_table() {
    let mut s = list();
    s.execute("selection.set", json!({"range": "A2:D7"})).unwrap();
    s.execute("insert.table", json!({})).unwrap();
    s.execute("selection.set", json!({"range": "C5"})).unwrap();
    s.execute("insert.chart", json!({"type": "column"})).unwrap();
    assert_eq!(charts(&s)[0].source.as_deref(), Some("Sheet1!A2:D7"));
}
