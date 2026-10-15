use gridcraft_core::{CellError, CellRef, Value};
use gridcraft_model::{Cell, Formula, Workbook};

use crate::Calc;

fn c(s: &str) -> CellRef {
    CellRef::parse(s).unwrap()
}

struct T {
    wb: Workbook,
    calc: Calc,
}

impl T {
    fn new() -> T {
        T { wb: Workbook::new(), calc: Calc::new() }
    }
    fn set(&mut self, at: &str, input: &str) {
        let cell = if input.starts_with('=') {
            Cell::formula(Formula::new(input))
        } else {
            Cell::value(gridcraft_core::parse::parse_input(input, self.wb.date_system).value)
        };
        self.wb.sheet_mut(0).unwrap().cells.set(c(at), cell);
        self.calc.cells_changed(&mut self.wb, &[(0, c(at))]);
    }
    fn get(&self, at: &str) -> Value {
        self.wb.sheet(0).unwrap().value(c(at))
    }
    fn num(&self, at: &str) -> f64 {
        self.get(at).as_f64().unwrap_or_else(|| panic!("{at} = {:?}", self.get(at)))
    }
}

#[test]
fn spill_range_operator() {
    let mut t = T::new();
    t.set("A1", "3");
    t.set("B1", "=SEQUENCE(A1)");
    t.set("C1", "=SUM(B1#)");
    t.set("C2", "=ROWS(B1#)");
    assert_eq!((t.num("C1"), t.num("C2")), (6.0, 3.0));
    // Growing and shrinking the spill recalculates its users.
    t.set("A1", "5");
    assert_eq!((t.num("C1"), t.num("C2")), (15.0, 5.0));
    t.set("A1", "2");
    assert_eq!((t.num("C1"), t.num("C2")), (3.0, 2.0));
    // A blocked spill, or a cell that doesn't spill, gives #REF!.
    t.set("B2", "x");
    assert_eq!(t.get("C1"), Value::Error(CellError::Ref));
    t.set("B2", "");
    assert_eq!(t.num("C1"), 3.0);
    t.set("D1", "=SUM(A1#)");
    assert_eq!(t.get("D1"), Value::Error(CellError::Ref));
    // Through a defined name.
    t.wb.names.push(gridcraft_model::DefinedName {
        name: "Seq".into(),
        scope: None,
        formula: "Sheet1!$B$1#".into(),
        comment: String::new(),
        hidden: false,
    });
    t.set("E1", "=SUM(Seq)*10");
    assert_eq!(t.num("E1"), 30.0);
    t.set("A1", "4");
    assert_eq!(t.num("E1"), 100.0);
}

#[test]
fn arithmetic_and_dependencies() {
    let mut t = T::new();
    t.set("A1", "2");
    t.set("A2", "3");
    t.set("A3", "=A1*A2+1");
    assert_eq!(t.num("A3"), 7.0);
    t.set("A1", "10");
    assert_eq!(t.num("A3"), 31.0);
    t.set("B1", "=SUM(A1:A3)");
    assert_eq!(t.num("B1"), 44.0);
    t.set("A2", "0");
    assert_eq!(t.num("A3"), 1.0);
    assert_eq!(t.num("B1"), 11.0);
}

#[test]
fn chains_out_of_order() {
    let mut t = T::new();
    t.set("A5", "=A4+1");
    t.set("A4", "=A3+1");
    t.set("A3", "=A2+1");
    t.set("A2", "=A1+1");
    t.set("A1", "1");
    assert_eq!(t.num("A5"), 5.0);
}

#[test]
fn xlookup_returns_a_reference() {
    let mut t = T::new();
    for (i, (k, v)) in [("a", 1), ("b", 2), ("c", 3), ("d", 4)].iter().enumerate() {
        t.set(&format!("A{}", i + 1), k);
        t.set(&format!("B{}", i + 1), &v.to_string());
    }
    t.set("D1", "=SUM(XLOOKUP(\"b\",A1:A4,B1:B4):XLOOKUP(\"d\",A1:A4,B1:B4))");
    assert_eq!(t.num("D1"), 9.0);
    t.set("D2", "=SUM(B1:XLOOKUP(\"c\",A1:A4,B1:B4))");
    assert_eq!(t.num("D2"), 6.0);
    t.set("D3", "=CELL(\"address\",XLOOKUP(\"c\",A1:A4,B1:B4))");
    assert_eq!(t.get("D3"), Value::from("$B$3"));
    // Not found: if_not_found, else #N/A, as before.
    t.set("D4", "=XLOOKUP(\"z\",A1:A4,B1:B4,\"none\")");
    assert_eq!(t.get("D4"), Value::from("none"));
    t.set("D5", "=XLOOKUP(\"z\",A1:A4,B1:B4)");
    assert_eq!(t.get("D5"), Value::Error(CellError::NA));
    // A match on an empty cell is a blank reference: ISBLANK is TRUE and & adds nothing (#374).
    t.set("A5", "e");
    t.set("D6", "=ISBLANK(XLOOKUP(\"e\",A1:A5,B1:B5))");
    assert_eq!(t.get("D6"), Value::Bool(true));
    t.set("D7", "=\"x\"&XLOOKUP(\"e\",A1:A5,B1:B5)&\"y\"");
    assert_eq!(t.get("D7"), Value::from("xy"));
    // An empty match_mode keeps the search_mode after it: search last to first.
    t.set("A6", "b");
    t.set("B6", "20");
    t.set("D8", "=XLOOKUP(\"b\",A1:A6,B1:B6,,,-1)");
    assert_eq!(t.num("D8"), 20.0);
    t.set("D9", "=XLOOKUP(\"b\",A1:A6,B1:B6)");
    assert_eq!(t.num("D9"), 2.0);
}

#[test]
fn long_chain() {
    let mut t = T::new();
    t.set("A1", "1");
    for r in 2..=3000 {
        t.set(&format!("A{r}"), &format!("=A{}+1", r - 1));
    }
    assert_eq!(t.num("A3000"), 3000.0);
    t.set("A1", "10");
    assert_eq!(t.num("A3000"), 3009.0);
}

#[test]
fn index_with_arrays_of_positions() {
    let mut t = T::new();
    for (i, row) in [[1, 2, 3], [4, 5, 6], [7, 8, 9]].iter().enumerate() {
        for (j, v) in row.iter().enumerate() {
            t.set(&format!("{}{}", ["A", "B", "C"][j], i + 1), &v.to_string());
        }
    }
    t.set("E1", "=INDEX(A1:C3,{1;3},{1,3})");
    assert_eq!([t.num("E1"), t.num("F1"), t.num("E2"), t.num("F2")], [1.0, 3.0, 7.0, 9.0]);
    t.set("E4", "=SUM(INDEX(A1:C3,{1,2,3},2))");
    assert_eq!(t.num("E4"), 15.0);
    t.set("E5", "=INDEX({10,20,30},{3,1})");
    assert_eq!([t.num("E5"), t.num("F5")], [30.0, 10.0]);
    t.set("E7", "=INDEX(A1:C3,2,3)");
    assert_eq!(t.num("E7"), 6.0);
}

#[test]
fn index_area_number_overflow() {
    let mut t = T::new();
    t.set("A1", "7");
    t.set("D1", "9");
    for (area, expected) in [("1", 7.0), ("2", 9.0)] {
        t.set("G5", &format!("=INDEX((A1:B2,D1:E2),1,1,{area})"));
        assert_eq!(t.num("G5"), expected, "area {area}");
    }
    for area in ["3", "1E300", "-1", "-1E300"] {
        t.set("G5", &format!("=INDEX((A1:B2,D1:E2),1,1,{area})"));
        assert_eq!(t.get("G5"), Value::Error(CellError::Ref), "area {area}");
    }
}

#[test]
fn errors_and_cycles() {
    let mut t = T::new();
    t.set("A1", "=1/0");
    assert_eq!(t.get("A1"), Value::Error(CellError::Div0));
    t.set("B1", "=B2");
    t.set("B2", "=B1");
    assert!(t.get("B1").is_error());
    t.set("C1", "=NOSUCHFN(1)");
    assert_eq!(t.get("C1"), Value::Error(CellError::Name));
    // `_xludf.` marks a user-defined function, never the built-in of the same name (#726).
    for f in ["=_xludf.CONCAT(\"a\",\"b\")", "=_xludf.SUM(1,2)", "=_xludf.EOMONTH()"] {
        t.set("C3", f);
        assert_eq!(t.get("C3"), Value::Error(CellError::Name), "{f}");
    }
    t.set("C2", "=1+");
    assert_eq!(t.get("C2"), Value::Error(CellError::Name));
}

impl T {
    fn iterative(max_iterations: u32, max_change: f64) -> T {
        let mut t = T::new();
        t.wb.calc.iterative = true;
        t.wb.calc.set_max_iterations(max_iterations);
        t.wb.calc.set_max_change(max_change);
        t
    }
}

#[test]
fn iterative_calculation_stops_at_maximum_iterations() {
    // A1 =A1+1 never settles: every recalculation adds Maximum Iterations.
    let mut t = T::iterative(100, 0.001);
    t.set("A1", "=A1+1");
    assert_eq!(t.num("A1"), 100.0);
    assert!(t.calc.circular.is_empty());
    // An edit outside the loop doesn't recalculate it.
    t.set("B1", "7");
    assert_eq!(t.num("A1"), 100.0);
    // Each recalculation starts from the value the cell shows.
    t.calc.recalc_all(&mut t.wb);
    assert_eq!(t.num("A1"), 200.0);
    t.wb.calc.set_max_iterations(5);
    t.calc.recalc_all(&mut t.wb);
    assert_eq!(t.num("A1"), 205.0);
    // Through a range that holds the formula itself.
    t.set("C1", "1");
    t.set("C2", "2");
    t.set("C3", "=SUM(C1:C3)");
    assert_eq!(t.num("C3"), 15.0);
    // Excel's largest Maximum Iterations.
    let mut t = T::iterative(32767, 0.001);
    t.set("A1", "=A1+1");
    assert_eq!(t.num("A1"), 32767.0);
}

#[test]
fn iterative_calculation_converges_within_maximum_change() {
    let mut t = T::iterative(100, 0.001);
    t.set("A1", "=(A2+10)/2");
    t.set("A2", "=A1");
    assert!((t.num("A1") - 10.0).abs() < 0.002, "{}", t.num("A1"));
    assert!((t.num("A2") - 10.0).abs() < 0.002, "{}", t.num("A2"));
    assert!(t.calc.circular.is_empty());
    // A loop fed by a constant, and a formula that reads the loop, follow a change.
    t.set("D1", "10");
    t.set("A1", "=(A2+D1)/2");
    t.set("E1", "=A1*2");
    t.set("D1", "30");
    assert!((t.num("A1") - 30.0).abs() < 0.002, "{}", t.num("A1"));
    assert!((t.num("E1") - 60.0).abs() < 0.004, "{}", t.num("E1"));
    // Already settled: recalculating changes it by less than Maximum Change.
    let a1 = t.num("A1");
    t.calc.recalc_all(&mut t.wb);
    assert!((t.num("A1") - a1).abs() < 0.001);
}

#[test]
fn iterative_calculation_stops_once_changes_are_below_maximum_change() {
    // A1 halves its distance to 10 each pass, from 5: 7.5, 8.75, 9.375, 9.6875, and A2 trails
    // one pass behind. Maximum Change 1 stops once neither moved by 1 or more.
    let mut t = T::iterative(100, 1.0);
    t.set("A1", "=(A2+10)/2");
    assert_eq!(t.num("A1"), 5.0);
    t.set("A2", "=A1");
    assert_eq!((t.num("A1"), t.num("A2")), (9.6875, 9.375));
    // Maximum Iterations 2 stops first.
    let mut t = T::iterative(2, 1.0);
    t.set("A1", "=(A2+10)/2");
    t.set("A2", "=A1");
    assert_eq!((t.num("A1"), t.num("A2")), (8.75, 7.5));
}

#[test]
fn iterative_calculation_of_text_and_errors() {
    let mut t = T::iterative(100, 0.001);
    t.set("A1", "=IF(ISTEXT(A1),A1&\"\",\"go\")");
    assert_eq!(t.get("A1"), Value::from("go"));
    t.set("B1", "=B1+1/0");
    assert_eq!(t.get("B1"), Value::Error(CellError::Div0));
}

#[test]
fn circular_references_without_iterative_calculation() {
    let mut t = T::new();
    t.set("A1", "=A1+1");
    assert_eq!(t.get("A1"), Value::Error(CellError::Circ));
    assert_eq!(t.calc.circular, vec![(0, c("A1"))]);
    // Switched on, the loop is calculated from 0 rather than from the error.
    t.wb.calc.iterative = true;
    t.calc.recalc_all(&mut t.wb);
    assert_eq!(t.num("A1"), 100.0);
    assert!(t.calc.circular.is_empty());
    t.wb.calc.iterative = false;
    t.calc.recalc_all(&mut t.wb);
    assert_eq!(t.get("A1"), Value::Error(CellError::Circ));
}

#[test]
fn date_text_coerces_in_the_workbook_date_system() {
    for (sys, serial) in [(gridcraft_core::DateSystem::D1900, 1462.0), (gridcraft_core::DateSystem::D1904, 0.0)] {
        let mut t = T::new();
        t.wb.date_system = sys;
        t.set("A1", "'1/1/1904");
        t.set("B1", "=A1+0");
        t.set("C1", "=VALUE(A1)");
        t.set("D1", "=-A1");
        t.set("E1", "=DATEVALUE(A1)");
        t.set("F1", "=NUMBERVALUE(A1)");
        assert_eq!(t.get("A1"), Value::from("1/1/1904"));
        assert_eq!(t.num("B1"), serial, "{sys:?}");
        assert_eq!(t.num("C1"), serial, "{sys:?}");
        assert_eq!(t.num("D1"), -serial, "{sys:?}");
        assert_eq!(t.num("E1"), serial, "{sys:?}");
        assert_eq!(t.num("F1"), serial, "{sys:?}");
    }
}

#[test]
fn operators() {
    let mut t = T::new();
    t.set("A1", "=\"a\"&1&TRUE");
    assert_eq!(t.get("A1"), Value::from("a1TRUE"));
    t.set("A2", "=-2^2");
    assert_eq!(t.num("A2"), 4.0);
    t.set("A3", "=50%");
    assert_eq!(t.num("A3"), 0.5);
    t.set("A4", "=\"abc\"=\"ABC\"");
    assert_eq!(t.get("A4"), Value::Bool(true));
    t.set("A5", "=\"5\"+1");
    assert_eq!(t.num("A5"), 6.0);
    t.set("A6", "=\"x\"+1");
    assert_eq!(t.get("A6"), Value::Error(CellError::Value));
    t.set("A7", "=B99");
    assert_eq!(t.num("A7"), 0.0);
}

#[test]
fn blocked_spill_reads_as_spill_error() {
    // Calculated in the same pass, a formula over a blocked spill read the array that couldn't
    // spill.
    let mut t = T::new();
    let s = t.wb.sheet_mut(0).unwrap();
    for a in ["A1", "B1", "C1", "A2", "C2", "A3", "B3", "C3"] {
        s.cells.set(c(a), Cell::value(Value::Number(1.0)));
    }
    s.cells.set(c("B2"), Cell::formula(Formula::new("=SEQUENCE(2,2,9)")));
    s.cells.set(c("E1"), Cell::formula(Formula::new("=SUM(A1:C3)")));
    t.calc.recalc_all(&mut t.wb);
    assert_eq!(t.get("B2"), Value::Error(CellError::Spill));
    assert_eq!(t.get("E1"), Value::Error(CellError::Spill));
    assert_eq!(t.num("C3"), 1.0);
}

#[test]
fn blocked_spill_read_through_offset() {
    // The reader's reference is only known while evaluating, so it isn't in the dependency graph.
    let mut t = T::new();
    t.set("E4", "5");
    t.set("E8", "=SUM(OFFSET(E2,0,0,2,2))");
    t.set("E3", "=SEQUENCE(2,2,3)");
    t.calc.recalc_all(&mut t.wb);
    assert_eq!(t.get("E3"), Value::Error(CellError::Spill));
    assert_eq!(t.get("E8"), Value::Error(CellError::Spill));
}

#[test]
fn spill_into_the_old_area_of_a_blocked_neighbour() {
    // C7's array becomes blocked by C8, whose array then takes over C7's old area.
    let mut t = T::new();
    t.set("C7", "=SEQUENCE(2,2)");
    t.set("C8", "=SEQUENCE(2,2)");
    assert_eq!(t.get("C7"), Value::Error(CellError::Spill));
    assert_eq!([t.num("C8"), t.num("D8"), t.num("C9"), t.num("D9")], [1.0, 2.0, 3.0, 4.0]);
}

#[test]
fn arrays_do_not_spill_in_tables() {
    // Excel: spilled array formulas aren't supported in tables.
    let mut t = T::new();
    t.wb.sheet_mut(0).unwrap().tables.push(gridcraft_model::Table {
        id: 1,
        name: "Table1".into(),
        range: gridcraft_core::RangeRef::parse("B3:C6").unwrap(),
        header_row: true,
        totals_row: false,
        columns: vec![],
        style: String::new(),
        banded_rows: false,
        banded_cols: false,
        first_col: false,
        last_col: false,
        filter_button: false,
        filter: vec![],
        table_type: None,
        connection_id: None,
    });
    t.set("C4", "=SEQUENCE(2)");
    t.set("E1", "=SEQUENCE(4)");
    t.set("H1", "=SUM(C4:C5)");
    assert_eq!(t.get("C4"), Value::Error(CellError::Spill));
    assert_eq!(t.get("C5"), Value::Empty);
    assert_eq!(t.get("H1"), Value::Error(CellError::Spill));
    assert_eq!(t.num("E4"), 4.0);
}

#[test]
fn ifs_and_lazy() {
    let mut t = T::new();
    t.set("A1", "5");
    t.set("B1", "=IF(A1>3,\"big\",1/0)");
    assert_eq!(t.get("B1"), Value::from("big"));
    t.set("B2", "=IFERROR(1/0,\"oops\")");
    assert_eq!(t.get("B2"), Value::from("oops"));
    t.set("B3", "=CHOOSE(2,\"a\",\"b\",\"c\")");
    assert_eq!(t.get("B3"), Value::from("b"));
    t.set("B4", "=SWITCH(A1,1,\"one\",5,\"five\",\"other\")");
    assert_eq!(t.get("B4"), Value::from("five"));
}

#[test]
fn choose_with_an_array_index() {
    let mut t = T::new();
    for (i, (k, v)) in [(1, 10), (2, 20), (3, 30)].iter().enumerate() {
        t.set(&format!("A{}", i + 1), &k.to_string());
        t.set(&format!("B{}", i + 1), &v.to_string());
    }
    t.set("D1", "=CHOOSE({1,2},\"a\",\"b\")");
    assert_eq!(t.get("D1"), Value::from("a"));
    assert_eq!(t.get("E1"), Value::from("b"));
    // The usual "look left" idiom: a two-column array from two ranges.
    t.set("D3", "=VLOOKUP(20,CHOOSE({1,2},B1:B3,A1:A3),2,FALSE)");
    assert_eq!(t.num("D3"), 2.0);
    t.set("D4", "=SUM(CHOOSE({1,2},A1:A3,B1:B3))");
    assert_eq!(t.num("D4"), 66.0);
    t.set("D5", "=CHOOSE(2,A1:A3,B1:B3)");
    assert_eq!(t.num("D7"), 30.0);
}

#[test]
fn references_functions() {
    let mut t = T::new();
    for (i, v) in [10, 20, 30, 40].iter().enumerate() {
        t.set(&format!("A{}", i + 1), &v.to_string());
    }
    t.set("B1", "=ROW(A3)");
    assert_eq!(t.num("B1"), 3.0);
    t.set("B2", "=ROWS(A1:A4)*COLUMNS(A1:C1)");
    assert_eq!(t.num("B2"), 12.0);
    t.set("B3", "=SUM(OFFSET(A1,1,0,2,1))");
    assert_eq!(t.num("B3"), 50.0);
    t.set("B4", "=INDIRECT(\"A\"&4)");
    assert_eq!(t.num("B4"), 40.0);
    // With a1 = FALSE the text must be R1C1: A1-style text is #REF!, not read as A1.
    t.set("C1", "=INDIRECT(\"R3C1\",FALSE)");
    assert_eq!(t.num("C1"), 30.0);
    t.set("C2", "=INDIRECT(\"A3\",FALSE)");
    assert_eq!(t.get("C2"), Value::Error(CellError::Ref));
    // Defined names don't depend on the reference style.
    t.wb.names.push(gridcraft_model::DefinedName {
        name: "Third".into(),
        scope: None,
        formula: "Sheet1!$A$3".into(),
        comment: String::new(),
        hidden: false,
    });
    t.calc.rebuild(&t.wb);
    t.set("C3", "=INDIRECT(\"Third\",FALSE)");
    assert_eq!(t.num("C3"), 30.0);
    t.set("B5", "=SUM(A1:INDEX(A1:A4,3))");
    assert_eq!(t.num("B5"), 60.0);
    t.set("B6", "=SUM(A:A)");
    assert_eq!(t.num("B6"), 100.0);
    t.set("A2", "25");
    assert_eq!(t.num("B3"), 55.0);
    assert_eq!(t.num("B6"), 105.0);
}

#[test]
fn indirect_r1c1_out_of_bounds() {
    for reference in ["R[9223372036854775807]C", "RC[9223372036854775807]", "R[4294967296]C", "R[4294967291]C", "R[2000000]C", "RC[20000]"] {
        let mut t = T::new();
        t.set("D5", &format!("=ROW(INDIRECT(\"{reference}\",FALSE))"));
        assert_eq!(t.get("D5"), Value::Error(CellError::Ref), "{reference}");
    }
}

#[test]
fn indirect_r1c1_past_the_sheet_does_not_fall_back_to_a1() {
    let mut t = T::new();
    t.set("RC16385", "78");
    for reference in ["RC16385", "rc16385", "RC1:RC16385"] {
        t.set("D5", &format!("=INDIRECT(\"{reference}\",FALSE)"));
        assert_eq!(t.get("D5"), Value::Error(CellError::Ref), "{reference}");
    }
}

#[test]
fn indirect_r1c1_valid_references() {
    let mut t = T::new();
    t.set("E6", "11");
    t.set("A1", "22");
    t.set("D4", "33");
    for (reference, expected) in [("R[1]C[1]", 11.0), ("R1C1", 22.0), ("R[-1]C", 33.0)] {
        t.set("D5", &format!("=INDIRECT(\"{reference}\",FALSE)"));
        assert_eq!(t.num("D5"), expected, "{reference}");
    }
}

#[test]
fn relative_names() {
    let mut t = T::new();
    // As files store them: relative to A1. `Left` is the cell to the left, `Above3` the three
    // cells above, `Mixed` column A of the same row.
    for (name, formula) in [("Left", "Sheet1!XFD1"), ("Above3", "Sheet1!A1048574:A1048576"), ("Mixed", "Sheet1!$A1")] {
        t.wb.names.push(gridcraft_model::DefinedName {
            name: name.into(),
            scope: None,
            formula: formula.into(),
            comment: String::new(),
            hidden: false,
        });
    }
    t.calc.rebuild(&t.wb);
    for r in 1..=5 {
        t.set(&format!("A{r}"), &(r * 10).to_string());
    }
    t.set("B2", "=Left");
    assert_eq!(t.num("B2"), 20.0);
    t.set("B4", "=Left+Mixed");
    assert_eq!(t.num("B4"), 80.0);
    t.set("A6", "=SUM(Above3)");
    assert_eq!(t.num("A6"), 120.0);
}

#[test]
fn spills() {
    let mut t = T::new();
    t.set("A1", "=SEQUENCE(3)");
    assert_eq!(t.num("A1"), 1.0);
    assert_eq!(t.num("A3"), 3.0);
    t.set("B1", "=SUM(A1:A3)");
    assert_eq!(t.num("B1"), 6.0);
    t.set("A2", "x");
    assert_eq!(t.get("A1"), Value::Error(CellError::Spill));
    t.set("A2", "");
    assert_eq!(t.num("A3"), 3.0);
    t.set("C1", "={1,2;3,4}*10");
    assert_eq!(t.num("D2"), 40.0);
}

#[test]
fn references_keep_their_size_past_the_used_range() {
    let mut t = T::new();
    t.set("A1", "5");
    t.set("A2", "6");
    t.set("C1", "=ROWS(A1:A10*1)");
    t.set("C2", "=SUMPRODUCT(--(A1:A10=\"\"))");
    t.set("C3", "=COUNTBLANK(A1:A10)");
    t.set("C4", "=COUNTIF(A1:A10,\"\")");
    t.set("C5", "=COLUMNS(TRANSPOSE(A1:A10))");
    for (at, want) in [("C1", 10.0), ("C2", 8.0), ("C3", 8.0), ("C4", 8.0), ("C5", 10.0)] {
        assert_eq!(t.num(at), want, "{at}");
    }
    // Whole columns are still read up to the used range.
    t.set("D1", "=ROWS(FILTER(A:A,A:A<>\"\"))");
    assert_eq!(t.num("D1"), 2.0);
}

#[test]
fn spilled_blank_cells_show_zero() {
    let mut t = T::new();
    t.set("A1", "5");
    t.set("A3", "7");
    t.set("C1", "=A1:A4");
    assert_eq!(t.get("C2"), Value::Number(0.0));
    assert_eq!(t.get("C4"), Value::Number(0.0));
    t.set("D1", "=COUNT(C1:C4)");
    assert_eq!(t.num("D1"), 4.0);
    t.set("E1", "=A2:A3");
    assert_eq!(t.get("E1"), Value::Number(0.0));
    t.set("F1", "=SORT(A1:A3)");
    assert_eq!(t.get("F3"), Value::Number(0.0));
}

#[test]
fn let_lambda() {
    let mut t = T::new();
    t.set("A1", "=LET(x,2,y,3,x*y)");
    assert_eq!(t.num("A1"), 6.0);
    t.set("A2", "=LAMBDA(a,b,a+b)(2,3)");
    assert_eq!(t.num("A2"), 5.0);
    t.set("A3", "=REDUCE(0,{1,2,3},LAMBDA(acc,v,acc+v))");
    assert_eq!(t.num("A3"), 6.0);
    t.set("A4", "=SUM(MAP({1,2,3},LAMBDA(v,v*v)))");
    assert_eq!(t.num("A4"), 14.0);
}

#[test]
fn functions_given_by_name() {
    let mut t = T::new();
    for (i, (k, v)) in [("a", 1), ("b", -2), ("a", 3)].iter().enumerate() {
        t.set(&format!("A{}", i + 1), k);
        t.set(&format!("B{}", i + 1), &v.to_string());
    }
    t.set("D1", "=BYROW(B1:B3,SUM)");
    assert_eq!(t.num("D2"), -2.0);
    t.set("E1", "=MAP(B1:B3,ABS)");
    assert_eq!(t.num("E2"), 2.0);
    t.set("F1", "=REDUCE(0,B1:B3,SUM)");
    assert_eq!(t.num("F1"), 2.0);
    t.set("G1", "=GROUPBY(A1:A3,B1:B3,SUM,0,0)");
    assert_eq!(t.get("G1"), Value::from("a"));
    assert_eq!(t.num("H1"), 4.0);
    // A LET variable of that name is still the variable.
    t.set("J1", "=LET(ABS,LAMBDA(x,x*10),SUM(MAP(B1:B3,ABS)))");
    assert_eq!(t.num("J1"), 20.0);
}

#[test]
fn lambda_argument_count() {
    let mut t = T::new();
    t.wb.names.push(gridcraft_model::DefinedName {
        name: "AddPair".into(),
        scope: None,
        formula: "LAMBDA(a,b,a+b)".into(),
        comment: String::new(),
        hidden: false,
    });
    t.calc.rebuild(&t.wb);
    let omit = "LAMBDA(x,y,IF(ISOMITTED(y),\"Missing\",x+y))";
    t.set("A1", "=LAMBDA(a,b,a+b)(2)");
    t.set("A2", "=LAMBDA(a,b,a+b)(2,3,4)");
    t.set("A3", &format!("={omit}(1,)"));
    t.set("A4", &format!("={omit}(1)"));
    t.set("A5", &format!("={omit}(1,2)"));
    t.set("A6", "=LAMBDA(7)()");
    t.set("A7", "=AddPair(2)");
    t.set("A8", "=AddPair(2,3)");
    assert_eq!(t.get("A1"), Value::Error(CellError::Value));
    assert_eq!(t.get("A2"), Value::Error(CellError::Value));
    assert_eq!(t.get("A3"), Value::Text("Missing".into()));
    assert_eq!(t.get("A4"), Value::Error(CellError::Value));
    assert_eq!(t.num("A5"), 3.0);
    assert_eq!(t.num("A6"), 7.0);
    assert_eq!(t.get("A7"), Value::Error(CellError::Value));
    assert_eq!(t.num("A8"), 5.0);
    t.set("A9", "=LAMBDA(x,ISOMITTED(x))()");
    assert_eq!(t.get("A9"), Value::Bool(true));
}

#[test]
fn names_and_sheets() {
    let mut t = T::new();
    t.wb.names.push(gridcraft_model::DefinedName { name: "Rate".into(), scope: None, formula: "0.5".into(), comment: String::new(), hidden: false });
    t.wb.sheets.push(std::sync::Arc::new(gridcraft_model::Sheet::new("Data")));
    t.wb.sheet_mut(1).unwrap().set_value(c("A1"), Value::Number(8.0));
    t.calc.rebuild(&t.wb);
    t.set("A1", "=Rate*Data!A1");
    assert_eq!(t.num("A1"), 4.0);
    t.wb.sheet_mut(1).unwrap().set_value(c("A1"), Value::Number(10.0));
    t.calc.cells_changed(&mut t.wb, &[(1, c("A1"))]);
    assert_eq!(t.num("A1"), 5.0);
}

#[test]
fn legacy_array_formulas_fill_their_range() {
    let mut t = T::new();
    t.set("A1", "1");
    t.set("A2", "2");
    let cse = |t: &mut T, at: &str, formula: &str, range: &str| {
        let mut f = Formula::new(formula);
        f.array = gridcraft_core::RangeRef::parse(range);
        t.wb.sheet_mut(0).unwrap().cells.set(c(at), Cell::formula(f));
        t.calc.cells_changed(&mut t.wb, &[(0, c(at))]);
    };
    // Past the result: #N/A. A single column repeats across the range.
    cse(&mut t, "C1", "=A1:A2*2", "C1:D3");
    cse(&mut t, "F1", "=A1:A2*2", "F1");
    cse(&mut t, "G1", "=5", "G1:G2");
    assert_eq!(t.num("C2"), 4.0);
    assert_eq!(t.num("D2"), 4.0);
    assert_eq!(t.get("C3"), Value::Error(CellError::NA));
    // A smaller range shows only part of the result and never spills.
    assert_eq!(t.num("F1"), 2.0);
    assert_eq!(t.get("F2"), Value::Empty);
    assert_eq!(t.num("G2"), 5.0);
    // A range too large to fill is an error, not an allocation of every cell.
    cse(&mut t, "H1", "=1", "H1:XFD1048576");
    assert_eq!(t.get("H1"), Value::Error(CellError::Num));
}

#[test]
fn text_function() {
    let mut t = T::new();
    t.set("A1", "=TEXT(1234.5,\"#,##0.00\")");
    assert_eq!(t.get("A1"), Value::from("1,234.50"));
}

#[test]
fn long_operator_chains() {
    let mut t = T::new();
    t.set("A1", "1");
    // 1,000 additions: a formula of 3,000 characters, well within Excel's 8,192.
    t.set("B1", &format!("={}", vec!["A1"; 1000].join("+")));
    assert_eq!(t.num("B1"), 1000.0);
    t.set("B2", &format!("=5{}", "+1-1".repeat(600)));
    assert_eq!(t.num("B2"), 5.0);
}

#[test]
fn subtotal_skips_hidden() {
    let mut t = T::new();
    t.set("A1", "1");
    t.set("A2", "2");
    t.set("A3", "3");
    t.wb.sheet_mut(0).unwrap().rows.insert(1, gridcraft_model::LineInfo { hidden: true, ..Default::default() });
    t.set("B1", "=SUBTOTAL(109,A1:A3)");
    t.set("B2", "=SUBTOTAL(9,A1:A3)");
    assert_eq!(t.num("B1"), 4.0);
    assert_eq!(t.num("B2"), 6.0);
}

#[test]
fn subtotal_109_skips_nested_subtotal() {
    let mut t = T::new();
    t.set("A1", "2");
    t.set("A2", "=SUBTOTAL(9,A1)");
    t.set("C1", "=SUBTOTAL(109,A1:A2)");
    t.set("C2", "=SUBTOTAL(9,A1:A2)");
    t.set("C3", "=SUBTOTAL(109,A1)");
    assert_eq!(t.num("C1"), 2.0);
    assert_eq!(t.num("C2"), 2.0);
    assert_eq!(t.num("C3"), 2.0);
    // Manually hidden rows are still left out by 109 only.
    let mut t = T::new();
    t.set("A1", "1");
    t.set("A2", "2");
    t.set("A3", "=SUBTOTAL(9,A1:A2)");
    t.wb.sheet_mut(0).unwrap().rows.insert(1, gridcraft_model::LineInfo { hidden: true, ..Default::default() });
    t.set("B1", "=SUBTOTAL(109,A1:A3)");
    t.set("B2", "=SUBTOTAL(9,A1:A3)");
    assert_eq!(t.num("B1"), 1.0);
    assert_eq!(t.num("B2"), 3.0);
}

/// SUBTOTAL leaves out rows a filter hides for every function number, while 1-11 keep rows
/// hidden by hand (as in ConditionalFormattingSamples.xlsx, where `SUBTOTAL(9,Table69[Sales])`
/// under a filtered table adds only the visible rows).
#[test]
fn subtotal_skips_filtered_rows() {
    use gridcraft_model::{AutoFilter, FilterCriterion, LineInfo};
    let mut t = T::new();
    for (i, v) in [1, 2, 4, 8, 16, 32].iter().enumerate() {
        t.set(&format!("A{}", i + 2), &v.to_string());
        t.set(&format!("C{}", i + 2), &v.to_string());
    }
    let crit = || vec![(0, FilterCriterion::Custom { a: (">".into(), "1".into()), b: None, and: false })];
    let sh = t.wb.sheet_mut(0).unwrap();
    sh.tables.push(gridcraft_model::Table {
        id: 1,
        name: "Sales".into(),
        range: gridcraft_core::RangeRef::parse("A1:A7").unwrap(),
        header_row: true,
        totals_row: false,
        columns: vec![gridcraft_model::TableColumn {
            name: "Amount".into(),
            totals: Default::default(),
            totals_label: None,
            formula: None,
            unique_name: None,
            xml: None,
        }],
        style: String::new(),
        banded_rows: false,
        banded_cols: false,
        first_col: false,
        last_col: false,
        filter_button: true,
        filter: crit(),
        table_type: None,
        connection_id: None,
    });
    sh.autofilter = Some(AutoFilter { range: gridcraft_core::RangeRef::parse("C1:C7").unwrap(), criteria: crit() });
    // Rows 3 and 5 are filtered out; row 9 (outside both filters) is hidden by hand.
    for row in [2, 4, 8] {
        sh.rows.insert(row, LineInfo { hidden: true, ..Default::default() });
    }
    t.set("A9", "64");
    t.set("E1", "=SUBTOTAL(9,Sales[Amount])");
    t.set("E2", "=SUBTOTAL(109,Sales[Amount])");
    t.set("E3", "=SUBTOTAL(9,C2:C7)");
    t.set("E4", "=SUBTOTAL(9,A2:A9)");
    t.set("E5", "=SUBTOTAL(109,A2:A9)");
    t.set("E6", "=SUBTOTAL(2,A2:A7)");
    t.set("E7", "=AGGREGATE(9,4,A2:A7)");
    for (at, want) in [("E1", 53.0), ("E2", 53.0), ("E3", 53.0), ("E4", 117.0), ("E5", 53.0), ("E6", 4.0), ("E7", 63.0)] {
        assert_eq!(t.num(at), want, "{at}");
    }
}

#[test]
fn aggregate_without_data_argument_is_an_error() {
    let mut t = T::new();
    t.set("A1", "1");
    t.set("A2", "2");
    t.set("D1", "=AGGREGATE(9,0)");
    t.set("D2", "=AGGREGATE(4,0)");
    t.set("D3", "=AGGREGATE(9,0,A1:A2)");
    t.set("D4", "=AGGREGATE(9,0,A3:A4)");
    t.set("D5", "=AGGREGATE(9,0,A1:A2,A1:A2)");
    assert_eq!(t.get("D1"), Value::Error(CellError::Value));
    assert_eq!(t.get("D2"), Value::Error(CellError::Value));
    assert_eq!(t.num("D3"), 3.0);
    assert_eq!(t.num("D4"), 0.0);
    assert_eq!(t.num("D5"), 6.0);
}

#[test]
fn recalc_all_after_load() {
    let mut t = T::new();
    let s = t.wb.sheet_mut(0).unwrap();
    s.cells.set(c("A1"), Cell::value(Value::Number(4.0)));
    s.cells.set(c("A2"), Cell::formula(Formula::new("=A1*2")));
    s.cells.set(c("A3"), Cell::formula(Formula::new("=A2+A1")));
    t.calc.recalc_all(&mut t.wb);
    assert_eq!(t.num("A3"), 12.0);
}

#[test]
fn sumif_sum_range_takes_the_criteria_size() {
    // Excel adds up the range that starts at sum_range's top-left cell with the criteria range's
    // height and width, whatever size sum_range has.
    let mut t = T::new();
    for (r, (k, v)) in [("a", 1), ("b", 2), ("a", 3)].iter().enumerate() {
        t.set(&format!("A{}", r + 1), k);
        t.set(&format!("B{}", r + 1), &v.to_string());
    }
    t.set("D1", "=SUMIF(A1:A3,\"a\",B1)");
    assert_eq!(t.num("D1"), 4.0);
    t.set("D2", "=AVERAGEIF(A1:A3,\"a\",B1)");
    assert_eq!(t.num("D2"), 2.0);
    t.set("D3", "=SUMIF(A1:A3,\"a\",B1:B2)");
    assert_eq!(t.num("D3"), 4.0);
    t.set("D4", "=SUMIF(A1:A3,\"a\",B1:F1)");
    assert_eq!(t.num("D4"), 4.0);
    t.set("D5", "=SUMIF(A1:A3,\"a\",B1:B99)");
    assert_eq!(t.num("D5"), 4.0);
    t.set("D6", "=SUMIF(A:A,\"a\",B1)");
    assert_eq!(t.num("D6"), 4.0);
    t.set("D7", "=SUMIF(A1:A3,\"a\",$B$2)");
    assert_eq!(t.num("D7"), 2.0); // B2:B4: rows 1 and 3 match → B2 + B4 (empty)
    t.set("D8", "=SUMIF(A1:A3,\"a\")");
    assert_eq!(t.num("D8"), 0.0);
    // A horizontal criteria range resizes across.
    t.set("H1", "1");
    t.set("I1", "0");
    t.set("J1", "1");
    t.set("H2", "10");
    t.set("I2", "20");
    t.set("J2", "30");
    t.set("D9", "=SUMIF(H1:J1,1,H2)");
    assert_eq!(t.num("D9"), 40.0);
    // The resized cells are precedents: editing one recalculates.
    t.set("B3", "5");
    assert_eq!(t.num("D1"), 6.0);
    assert_eq!(t.num("D2"), 3.0);
    assert_eq!(t.num("D3"), 6.0);
    assert_eq!(t.num("D4"), 6.0);
    assert_eq!(t.num("D6"), 6.0);
    // SUMIFS and AVERAGEIFS don't resize: different sizes are #VALUE!.
    t.set("D11", "=SUMIFS(B1,A1:A3,\"a\")");
    assert_eq!(t.get("D11"), Value::Error(CellError::Value));
    t.set("D12", "=AVERAGEIFS(B1:B2,A1:A3,\"a\")");
    assert_eq!(t.get("D12"), Value::Error(CellError::Value));
    t.set("D13", "=SUMIFS(B1:B3,A1:A3,\"a\")");
    assert_eq!(t.num("D13"), 6.0);
}

#[test]
fn sumif_resized_sum_range_stops_at_the_sheet_edge() {
    let mut t = T::new();
    t.set("A1", "a");
    t.set("A2", "a");
    t.set("B1048575", "5");
    t.set("B1048576", "7");
    t.set("C1", "=SUMIF(A1:A2,\"a\",B1048576)");
    assert_eq!(t.num("C1"), 7.0);
    t.set("C2", "=SUMIF(A1:A2,\"a\",B1048575)");
    assert_eq!(t.num("C2"), 12.0);
    t.set("C3", "=SUMIF(A1:B1,\"a\",XFD1)");
    assert_eq!(t.num("C3"), 0.0);
}

#[test]
fn sumif_resized_sum_range_on_another_sheet_and_through_a_name() {
    let mut t = T::new();
    t.wb.sheets.push(std::sync::Arc::new(gridcraft_model::Sheet::new("Data")));
    t.wb.names.push(gridcraft_model::DefinedName {
        name: "Amounts".into(),
        scope: None,
        formula: "Data!$B$1".into(),
        comment: String::new(),
        hidden: false,
    });
    for (r, k) in ["a", "b", "a"].iter().enumerate() {
        t.set(&format!("A{}", r + 1), k);
        t.wb.sheet_mut(1).unwrap().set_value(CellRef::new(r as u32, 1), Value::Number((r + 1) as f64));
    }
    t.calc.rebuild(&t.wb);
    t.set("C1", "=SUMIF(A1:A3,\"a\",Data!B1)");
    assert_eq!(t.num("C1"), 4.0);
    t.set("C2", "=AVERAGEIF(A1:A3,\"a\",Amounts)");
    assert_eq!(t.num("C2"), 2.0);
    t.wb.sheet_mut(1).unwrap().set_value(c("B3"), Value::Number(9.0));
    t.calc.cells_changed(&mut t.wb, &[(1, c("B3"))]);
    assert_eq!(t.num("C1"), 10.0);
    assert_eq!(t.num("C2"), 5.0);
}

#[test]
fn trimrange() {
    let mut t = T::new();
    t.set("C3", "1");
    t.set("D5", "2");
    let cases = [
        ("J1", "=ROWS(TRIMRANGE(A1:F10))", 3.0),
        ("J2", "=COLUMNS(TRIMRANGE(A1:F10))", 2.0),
        ("J3", "=ROWS(TRIMRANGE(A1:F10,2))", 5.0),
        ("J4", "=ROWS(TRIMRANGE(A1:F10,1,0))", 8.0),
        ("J5", "=COLUMNS(TRIMRANGE(A1:F10,1,0))", 6.0),
        ("J6", "=ROWS(TRIMRANGE(A1:F10,0,0))", 10.0),
        ("J7", "=SUM(TRIMRANGE(C:C))", 1.0),
        ("J8", "=ROWS(TRIMRANGE(C:C))", 1.0),
        ("J9", "=ROWS(TRIMRANGE(C:C,2))", 3.0),
        ("J10", "=MIN(ROW(TRIMRANGE(A1:F10)))", 3.0),
        ("J11", "=COLUMNS(TRIMRANGE({1,2;3,4}))", 2.0),
    ];
    for (at, f, _) in cases {
        t.set(at, f);
    }
    for (at, f, want) in cases {
        assert_eq!(t.num(at), want, "{f}");
    }
    // The result spills as the trimmed block and follows edits.
    t.set("L1", "=TRIMRANGE(A1:F10)");
    assert_eq!((t.num("L1"), t.num("M3")), (1.0, 2.0));
    t.set("E6", "3");
    assert_eq!((t.num("J1"), t.num("J2")), (4.0, 3.0));
    // Errors: a bad trim code, nothing but blanks, several areas.
    t.set("K1", "=TRIMRANGE(A1:F10,4)");
    assert_eq!(t.get("K1"), Value::Error(CellError::Value));
    t.set("K2", "=TRIMRANGE(Q1:Q9)");
    assert_eq!(t.get("K2"), Value::Error(CellError::Calc));
    t.set("K3", "=TRIMRANGE((A1:B2,C3:D4))");
    assert_eq!(t.get("K3"), Value::Error(CellError::Value));
}

/// `formula` in AB1 over a small mixed sheet. The ranges in the cases below are cut from it:
///
/// ```text
///      A      B      C       E
///  1   1.5    10     =A1*4        (C1 is a formula cell inside the ranges)
///  2   2      TRUE           #DIV/0!   (E2 only when `error` is set)
///  3   text   3
///  4   -0.25
/// ```
fn aggregate_of(error: bool, formula: &str) -> Value {
    let mut t = T::new();
    for (at, v) in [("A1", "1.5"), ("A2", "2"), ("A3", "skipped"), ("A4", "-0.25"), ("B1", "10"), ("B2", "TRUE"), ("B3", "3"), ("C1", "=A1*4")] {
        t.set(at, v);
    }
    if error {
        t.set("E2", "#DIV/0!");
    }

    // AB1 is outside every range used, so the formula can't read itself.
    t.set("AB1", formula);
    t.get("AB1")
}

#[test]
fn sum_count_average_over_ranges() {
    // Text, booleans, blanks and a formula cell inside ranges; direct arguments; unions; whole
    // columns; blocks holding an error or nothing numeric. The expected values are what the
    // array path (the ranges copied into an array) gave before the ranges were folded in place,
    // so any difference is a change of behavior.
    let n = Value::Number;
    let e = Value::Error;

    // (formula written with SUM, error in E2?, [SUM, COUNT, AVERAGE] of that formula)
    #[rustfmt::skip]
    let cases: &[(&str, bool, [Value; 3])] = &[
        ("=SUM(A1:A1,B3)", false, [n(4.5), n(2.0), n(2.25)]),
        ("=SUM(A1:A1,B3)", true, [n(4.5), n(2.0), n(2.25)]),
        ("=SUM((A1:A2,B1:B3))", false, [n(16.5), n(4.0), n(4.125)]),
        ("=SUM((A1:A2,B1:B3))", true, [n(16.5), n(4.0), n(4.125)]),
        ("=SUM(A1:A4,B1:B3)", false, [n(16.25), n(5.0), n(3.25)]),
        ("=SUM(A1:A4,B1:B3)", true, [n(16.25), n(5.0), n(3.25)]),
        ("=SUM(A1:A4,\"x\")", false, [e(CellError::Value), n(3.0), e(CellError::Value)]),
        ("=SUM(A1:A4,\"x\")", true, [e(CellError::Value), n(3.0), e(CellError::Value)]),
        ("=SUM(A1:B4,5,\"7\",TRUE)", false, [n(29.25), n(8.0), n(3.65625)]),
        ("=SUM(A1:B4,5,\"7\",TRUE)", true, [n(29.25), n(8.0), n(3.65625)]),
        ("=SUM(A1:C4)+SUM(B1:B3)", false, [n(35.25), n(8.0), n(10.208333333333334)]),
        ("=SUM(A1:C4)+SUM(B1:B3)", true, [n(35.25), n(8.0), n(10.208333333333334)]),
        ("=SUM(A1:C4)", false, [n(22.25), n(6.0), n(3.7083333333333335)]),
        ("=SUM(A1:C4)", true, [n(22.25), n(6.0), n(3.7083333333333335)]),
        ("=SUM(A1:E4)", false, [n(22.25), n(6.0), n(3.7083333333333335)]),
        ("=SUM(A1:E4)", true, [e(CellError::Div0), n(6.0), e(CellError::Div0)]),
        ("=SUM(A1)", false, [n(1.5), n(1.0), n(1.5)]),
        ("=SUM(A1)", true, [n(1.5), n(1.0), n(1.5)]),
        ("=SUM(A3,B2)", false, [n(0.0), n(0.0), e(CellError::Div0)]),
        ("=SUM(A3,B2)", true, [n(0.0), n(0.0), e(CellError::Div0)]),
        ("=SUM(A:C)", false, [n(22.25), n(6.0), n(3.7083333333333335)]),
        ("=SUM(A:C)", true, [n(22.25), n(6.0), n(3.7083333333333335)]),
        ("=SUM(E1:E3)", false, [n(0.0), n(0.0), e(CellError::Div0)]),
        ("=SUM(E1:E3)", true, [e(CellError::Div0), n(0.0), e(CellError::Div0)]),
        ("=SUM(Z1:Z9)", false, [n(0.0), n(0.0), e(CellError::Div0)]),
        ("=SUM(Z1:Z9)", true, [n(0.0), n(0.0), e(CellError::Div0)]),
    ];

    for (formula, error, want) in cases {
        for (func, want) in ["SUM", "COUNT", "AVERAGE"].into_iter().zip(want) {
            // The same formula for each function: `SUM(` becomes `COUNT(` or `AVERAGE(`.
            let f = formula.replace("SUM", func);

            assert_eq!(&aggregate_of(*error, &f), want, "{f} (error in E2: {error})");
        }
    }
}

#[test]
fn sum_count_average_have_no_size_limit() {
    // One stored cell far down stretches the used range to 27 million cells (27 columns by a
    // million rows), past the 16M-cell limit of ranges copied into an array. Only two cells are
    // stored, so the sheet itself is tiny.
    for (func, want) in [("SUM", Value::Number(2.5)), ("COUNT", Value::Number(2.0)), ("AVERAGE", Value::Number(1.25))] {
        for range in ["A1:AA1000000", "A:AA"] {
            let mut t = T::new();
            t.set("A1", "1.5");
            t.set("Z1000000", "1");

            t.set("AB1", &format!("={func}({range})"));

            assert_eq!(t.get("AB1"), want, "{func}({range})");
        }
    }
}

#[test]
fn sum_count_average_over_spilled_values() {
    // A1 spills over A1:A3, so the block holds values that exist only as a spill (they have no
    // stored cell of their own). The host must not fold such a block in place: it falls back
    // to the array path, which sees the spill. Expected: 1 + 2 + 3 + 10.
    let mut t = T::new();
    t.set("A1", "=SEQUENCE(3)");
    t.set("A4", "10");

    for (f, want) in [("SUM", 16.0), ("COUNT", 4.0), ("AVERAGE", 4.0)] {
        t.set("C1", &format!("={f}(A1:A5)"));

        assert_eq!(t.num("C1"), want, "{f}");
    }
}

#[test]
fn aggregate_arguments_are_read_in_order() {
    // Volatile cells draw a random number each time they are calculated, so the order in which
    // the arguments are read decides which draw each one gets. `B1:B2` comes first, so B2 must
    // draw before the `RAND()` argument does. (Reading the plain `RAND()` first once gave
    // 7.9043 here.) The expected values are what the array path gave; they depend on the random
    // generator and its seed, and must be updated if either changes.
    let mut t = T::new();
    t.set("B1", "1");
    t.set("B2", "=RAND()*10");

    t.set("A1", "=SUM(B1:B2,RAND())");

    assert_eq!((t.num("A1"), t.num("B2")), (4.968520165759784, 3.3112028100185356));
}

#[test]
fn sum_count_average_follow_edits_inside_the_range() {
    // A range read in place must still count as a precedent of the formula: if it didn't, an
    // edit inside it would leave the formula showing a stale value. Each step below changes one
    // cell inside the range and checks all three functions; the last one changes a cell outside.
    for range in ["A1:A5", "A:A"] {
        let mut t = T::new();
        t.set("A1", "1");
        t.set("A3", "3");
        t.set("C1", &format!("=SUM({range})"));
        t.set("C2", &format!("=COUNT({range})"));
        t.set("C3", &format!("=AVERAGE({range})"));
        let all = |t: &T| (t.num("C1"), t.num("C2"), t.num("C3"));
        assert_eq!(all(&t), (4.0, 2.0, 2.0), "{range}");

        t.set("A1", "5");
        assert_eq!(all(&t), (8.0, 2.0, 4.0), "{range}: edit");

        t.set("A2", "10");
        assert_eq!(all(&t), (18.0, 3.0, 6.0), "{range}: fill a blank");

        t.set("A3", "text");
        assert_eq!(all(&t), (15.0, 2.0, 7.5), "{range}: number to text");

        t.set("A2", "");
        assert_eq!(all(&t), (5.0, 1.0, 5.0), "{range}: clear");

        t.set("A5", "=A1*2");
        assert_eq!(all(&t), (15.0, 2.0, 7.5), "{range}: formula in the range");

        t.set("A1", "1");
        assert_eq!(all(&t), (3.0, 2.0, 1.5), "{range}: its precedent changes");

        // A7 is inside the whole column A:A but outside A1:A5.
        t.set("A7", "100");
        let outside = if range == "A:A" { (103.0, 3.0, 103.0 / 3.0) } else { (3.0, 2.0, 1.5) };
        assert_eq!(all(&t), outside, "{range}: a cell outside the range");
    }
}

#[test]
fn aggregates_over_a_shared_range_match_the_walked_one() {
    // The first SUM(A:A) walks the column in place; the later ones read the pass's shared array.
    // Both must treat text, logicals, blanks and errors alike.
    let mut t = T::new();
    for r in 1..=200u32 {
        let v = match r % 5 {
            0 => Value::from("text"),
            1 => Value::Bool(true),
            2 => continue,
            _ => Value::Number(f64::from(r)),
        };
        t.wb.sheet_mut(0).unwrap().cells.set(c(&format!("A{r}")), Cell::value(v));
    }
    let formulas = ["=SUM(A:A)", "=COUNT(A:A)", "=AVERAGE(A:A)", "=SUM(A1:A200)"];
    t.load((1..=50).flat_map(|r| formulas.iter().enumerate().map(move |(i, f)| (format!("{}{r}", ["C", "D", "E", "F"][i]), f.to_string()))));
    let want_sum: f64 = (1..=200).filter(|r| matches!(r % 5, 3 | 4)).map(f64::from).sum();
    let want_count = (1..=200).filter(|r| matches!(r % 5, 3 | 4)).count() as f64;
    for r in 1..=50 {
        assert_eq!(t.num(&format!("C{r}")), want_sum, "C{r}");
        assert_eq!(t.num(&format!("D{r}")), want_count, "D{r}");
        assert_eq!(t.num(&format!("E{r}")), want_sum / want_count, "E{r}");
        assert_eq!(t.num(&format!("F{r}")), want_sum, "F{r}");
    }
    // An error in the range is the result of every one of them.
    t.set("A7", "=1/0");
    for r in [1, 2, 50] {
        assert_eq!(t.get(&format!("C{r}")), Value::Error(CellError::Div0), "C{r}");
        assert_eq!(t.get(&format!("F{r}")), Value::Error(CellError::Div0), "F{r}");
    }
}

#[test]
fn visit_range_declines_when_a_formula_in_the_block_spills_into_it() {
    // The sheet: D1 = SEQUENCE(3) and D4 = 10, and D1 has not been calculated yet when the
    // block D1:D5 is read.
    let mut wb = Workbook::new();
    let sh = wb.sheet_mut(0).unwrap();
    sh.cells.set(c("D1"), Cell::formula(Formula::new("=SEQUENCE(3)")));
    sh.cells.set(c("D4"), Cell::value(Value::Number(10.0)));
    let block = gridcraft_core::RangeRef::parse("D1:D5").unwrap();

    // Reading the block calculates D1, which spills over D2:D3. A walk over the stored cells
    // would never see 2 and 3, so the host must decline and leave the block to `range_values`.
    // (`Calc` recalculates a pass that produced new spills, so final results are right either
    // way; only this direct call shows the host's contract.)
    let walked = crate::recalc::tests::probe_visit_range(&wb, &[("D1", "=SEQUENCE(3)")], block);

    assert_eq!(walked, None, "walked a block holding a spill it can't see");
}

/// Trim references evaluate as `TRIMRANGE(range, code, code)`.
#[test]
fn trim_references() {
    let mut t = T::new();
    t.set("C3", "1");
    t.set("D5", "2");
    t.set("A20", "5");
    t.set("A21", "6");
    let cases = [
        ("J1", "=ROWS(A1.:.F10)", "=ROWS(TRIMRANGE(A1:F10,3,3))", 3.0),
        ("J2", "=COLUMNS(A1.:.F10)", "=COLUMNS(TRIMRANGE(A1:F10,3,3))", 2.0),
        ("J3", "=ROWS(A1:.F10)", "=ROWS(TRIMRANGE(A1:F10,2,2))", 5.0),
        ("J4", "=COLUMNS(A1:.F10)", "=COLUMNS(TRIMRANGE(A1:F10,2,2))", 4.0),
        ("J5", "=ROWS(A1.:F10)", "=ROWS(TRIMRANGE(A1:F10,1,1))", 8.0),
        ("J6", "=MIN(ROW(A1.:F10))", "=MIN(ROW(TRIMRANGE(A1:F10,1,1)))", 3.0),
        ("J7", "=SUM(A:.A)", "=SUM(TRIMRANGE(A:A,2,2))", 11.0),
        ("J8", "=ROWS(A:.A)", "=ROWS(TRIMRANGE(A:A,2,2))", 21.0),
        ("J9", "=ROWS(A.:.A)", "=ROWS(TRIMRANGE(A:A,3,3))", 2.0),
        ("J10", "=ROWS(20.:.30)", "=ROWS(TRIMRANGE(20:30,3,3))", 2.0),
    ];
    for (at, f, _, _) in cases {
        t.set(at, f);
    }
    for (i, (at, f, same, want)) in cases.iter().enumerate() {
        assert_eq!(t.num(at), *want, "{f}");
        let k = format!("K{}", i + 1);
        t.set(&k, same);
        assert_eq!(t.num(&k), *want, "{same}");
    }
    // The trimmed block spills and follows edits to the full range.
    t.set("L1", "=C1.:.F10");
    assert_eq!((t.num("L1"), t.num("M3")), (1.0, 2.0));
    t.set("A22", "4");
    assert_eq!((t.num("J7"), t.num("J8")), (15.0, 22.0));
    // Nothing but blanks.
    t.set("K20", "=SUM(Q1:.Q9)");
    assert_eq!(t.get("K20"), Value::Error(CellError::Calc));
}

#[test]
fn cell_width_is_the_column_width_dialog_value_rounded() {
    // #735: Excel 365 values. Widths in px as `home.columnWidth {"chars"}` sets them
    // (chars * 7 + 5): 10.71 -> 80, 12 -> 89, 10.57 -> 79, 2.86 -> 25; B keeps the default 8.43 (64).
    let mut t = T::new();
    let sh = t.wb.sheet_mut(0).unwrap();
    for (col, px) in [(2, 80.0), (3, 89.0), (4, 79.0), (5, 25.0)] {
        sh.cols.insert(col, gridcraft_model::LineInfo { size: Some(px), ..Default::default() });
    }
    for (at, col) in [("H1", "B"), ("H2", "C"), ("H3", "D"), ("H4", "E"), ("H5", "F")] {
        t.set(at, &format!("=CELL(\"width\",{col}1)"));
    }
    assert_eq!([t.num("H1"), t.num("H2"), t.num("H3"), t.num("H4"), t.num("H5")], [8.0, 11.0, 12.0, 11.0, 3.0]);
}

/// Expected results are Excel's, from Apache POI's `DGet.xls` (rows 46-49): a criteria cell holding
/// a formula under a label that names no field is evaluated for every record.
#[test]
fn database_functions_with_computed_criteria() {
    let mut t = T::new();
    let db = [
        ["name", "age", "size", "headgear", "glasses"],
        ["Huey", "6", "45", "cap~", "FALSE"],
        ["Dewey", "7", "fourty-six", "cap?", "FALSE"],
        ["Louie", "8", "47", "cap*", "FALSE"],
        ["Donald", "32", "70", "sailor's hat", "FALSE"],
        ["Scrooge", "62", "65", "top hat", "TRUE"],
    ];
    for (r, row) in db.iter().enumerate() {
        for (c, v) in row.iter().enumerate() {
            t.set(&format!("{}{}", ["F", "G", "H", "I", "J"][c], r + 2), v);
        }
    }
    t.set("G45", "7");
    let cases = [
        // Relative references are to the first record's row and move to each record's row;
        // `$G$45` stays put.
        ("G", "=G2<$G$45", "Dewey"),
        // Column labels stand for the record's fields.
        ("H", "=glasses=TRUE", "Scrooge"),
        ("I", "=age<7", "Huey"),
        ("J", "=size/7=10", "Donald"),
        // An absolute reference to a cell that is not the first record's.
        ("K", "=H3=$L$1", "Donald"),
    ];
    t.set("L1", "70");
    for (i, (col, formula, _)) in cases.iter().enumerate() {
        t.set(&format!("{col}46"), "Formula");
        t.set(&format!("{col}47"), formula);
        t.set(&format!("B{}", 46 + i), &format!("=DGET($F$2:$J$7,\"name\",{col}46:{col}47)"));
    }
    for (i, (_, formula, want)) in cases.iter().enumerate() {
        assert_eq!(t.get(&format!("B{}", 46 + i)), Value::text(*want), "{formula}");
    }
    // Every database function takes them, ANDed with the other criteria of their row.
    // Text sorts after numbers, so Dewey's "fourty-six" is > 40.
    t.set("L47", "=size>40");
    t.set("M46", "age");
    t.set("M47", ">6");
    t.set("B51", "=DCOUNT($F$2:$J$7,\"age\",I46:I47)");
    t.set("B52", "=DSUM($F$2:$J$7,\"age\",G46:G47)");
    t.set("B53", "=DCOUNTA($F$2:$J$7,\"name\",L46:M47)");
    assert_eq!(t.num("B51"), 1.0);
    assert_eq!(t.num("B52"), 7.0);
    assert_eq!(t.num("B53"), 4.0);
}

#[test]
fn offset_extreme_arguments_return_ref() {
    let wb = Workbook::new();
    for formula in [
        "=OFFSET(B2,1E300,0)",
        "=OFFSET(B2,0,1E300)",
        "=OFFSET(B2,0,0,1E300,1)",
        "=OFFSET(B2,0,0,1,1E300)",
        "=OFFSET(B2,-1E300,0)",
        "=OFFSET(B2,0,-1E300)",
        "=OFFSET(B2,0,0,-1E300,1)",
        "=OFFSET(B2,0,0,1,-1E300)",
        "=OFFSET(B2,1E300,0,-1E300,1)",
        "=OFFSET(B2,0,1E300,1,-1E300)",
        "=OFFSET(B2,-1E300,0,-1E300,1)",
    ] {
        assert_eq!(crate::evaluate(&wb, 0, c("D1"), formula), Value::Error(CellError::Ref), "{formula}");
    }
    for (formula, expected) in [
        ("=ROW(OFFSET(B2,1.9,0))", 3.0),
        ("=COLUMN(OFFSET(B2,0,-1.9))", 1.0),
        ("=ROWS(OFFSET(B2,0,0,-2,1))", 2.0),
        ("=COLUMNS(OFFSET(B2,0,0,1,-2))", 2.0),
        ("=ROWS(OFFSET(A1,0,0,1048576,1))", 1048576.0),
    ] {
        assert_eq!(crate::evaluate(&wb, 0, c("D1"), formula), Value::Number(expected), "{formula}");
    }
}

impl T {
    /// Stores formulas without recalculating, then recalculates everything at once (as loading
    /// a file does).
    fn load(&mut self, cells: impl IntoIterator<Item = (String, String)>) {
        for (at, f) in cells {
            self.wb.sheet_mut(0).unwrap().cells.set(c(&at), Cell::formula(Formula::new(&f)));
        }
        self.calc.recalc_all(&mut self.wb);
    }
}

#[test]
fn long_chains_compute_in_either_direction() {
    let n = 20_000;
    let mut t = T::new();
    // C1 = C2+1, C2 = C3+1, …: every formula reads the row below, the reverse of row order.
    t.load((1..n).map(|r| (format!("C{r}"), format!("=C{}+1", r + 1))).chain([(format!("C{n}"), "=1".to_string())]));
    assert_eq!(t.num("C1"), n as f64);
    assert!(t.calc.circular.is_empty());
    // An edit at the far end recalculates the whole chain.
    t.set(&format!("C{n}"), "=10");
    assert_eq!(t.num("C1"), (n + 9) as f64);
}

#[test]
fn long_chains_need_no_call_stack() {
    // Evaluation sets formulas aside on a heap stack instead of recursing, so a long reverse
    // chain through a defined name (no static precedents to order by) fits a tiny thread stack.
    let run = || {
        let n = 5_000;
        let mut t = T::new();
        t.wb.names.push(gridcraft_model::DefinedName {
            name: "Step".into(),
            scope: None,
            formula: "1".into(),
            comment: String::new(),
            hidden: false,
        });
        t.load((1..n).map(|r| (format!("A{r}"), format!("=A{}+Step", r + 1))).chain([(format!("A{n}"), "=Step".to_string())]));
        t.num("A1")
    };
    let a1 = std::thread::Builder::new().stack_size(512 * 1024).spawn(run).unwrap().join().unwrap();
    assert_eq!(a1, 5_000.0);
}

#[test]
fn cycles_are_circular_and_so_is_what_reads_them() {
    let mut t = T::new();
    t.set("A1", "=B1+1");
    t.set("B1", "=A1+1");
    assert!(!t.calc.circular.is_empty());
    t.set("C1", "=A1*2");
    t.set("D1", "=5");
    for at in ["A1", "B1", "C1"] {
        assert_eq!(t.get(at), Value::Error(CellError::Circ), "{at}");
    }
    assert_eq!(t.num("D1"), 5.0);
    // Breaking the cycle recalculates everything that was stuck behind it.
    t.set("B1", "=D1");
    assert_eq!((t.num("A1"), t.num("B1"), t.num("C1")), (6.0, 5.0, 12.0));
    assert!(t.calc.circular.is_empty());
    // A formula that reads itself.
    t.set("E1", "=E1+1");
    assert_eq!(t.get("E1"), Value::Error(CellError::Circ));
}

#[test]
fn ranges_over_formulas_wait_for_all_of_them() {
    // SUM reads a range of formulas below it that are dirty in the same pass.
    let mut t = T::new();
    t.load((2..=1001).map(|r| (format!("A{r}"), format!("=ROW()+B{r}"))).chain([("A1".to_string(), "=SUM(A2:A1001)".to_string())]));
    let want: f64 = (2..=1001).map(f64::from).sum();
    assert_eq!(t.num("A1"), want);
    t.set("B5", "100");
    assert_eq!(t.num("A1"), want + 100.0);
}

#[test]
fn indexed_lookups_match_scanning_ones() {
    // Each lookup appears three times over the same table, so the later copies go through the
    // shared range and its index (built on the second search): all copies must agree with what a
    // scan finds.
    let mut t = T::new();
    let mut cells: Vec<(String, String)> = (1..=100).map(|r| (format!("A{r}"), format!("=\"Key{r}\""))).collect();
    cells.extend([
        ("A5".to_string(), "=\"key3\"".to_string()),
        ("A10".to_string(), "=0.3".to_string()),
        ("A11".to_string(), "=TRUE".to_string()),
        ("A12".to_string(), "=7".to_string()),
    ]);
    cells.extend((1..=100).map(|r| (format!("B{r}"), format!("={r}"))));
    cells.extend((1..=100).map(|r| (format!("C{r}"), format!("={}", r * 10))));
    let cases = [
        ("=VLOOKUP(\"KEY3\",$A$1:$B$100,2,FALSE)", Value::Number(3.0)),
        ("=XLOOKUP(\"key3\",$A$1:$A$100,$B$1:$B$100,,0,-1)", Value::Number(5.0)),
        ("=MATCH(0.1+0.2,$A$1:$A$100,0)", Value::Number(10.0)),
        ("=MATCH(TRUE,$A$1:$A$100,0)", Value::Number(11.0)),
        ("=XMATCH(7,$A$1:$A$100)", Value::Number(12.0)),
        ("=MATCH(\"7\",$A$1:$A$100,0)", Value::Error(CellError::NA)),
        ("=VLOOKUP(\"Key2*\",$A$1:$B$100,2,FALSE)", Value::Number(2.0)),
        ("=MATCH(\"nope\",$A$1:$A$100,0)", Value::Error(CellError::NA)),
        ("=MATCH(255,$C$1:$C$100,1)", Value::Number(25.0)),
        ("=XLOOKUP(255,$C$1:$C$100,$B$1:$B$100,,1)", Value::Number(26.0)),
        ("=XMATCH(990,$C$1:$C$100,0,2)", Value::Number(99.0)),
        ("=VLOOKUP(5,$C$1:$C$100,1,TRUE)", Value::Error(CellError::NA)),
    ];
    for (i, (f, _)) in cases.iter().enumerate() {
        for col in ["E", "F", "G"] {
            cells.push((format!("{col}{}", i + 1), f.to_string()));
        }
    }
    t.load(cells);
    for (i, (f, want)) in cases.iter().enumerate() {
        for col in ["E", "F", "G"] {
            assert_eq!(&t.get(&format!("{col}{}", i + 1)), want, "{col}{}: {f}", i + 1);
        }
    }
    // An edit to the table: the next recalculation searches the new values.
    t.set("A3", "Moved");
    assert_eq!(t.num("E1"), 5.0);
    assert_eq!(t.num("G1"), 5.0);
}

#[test]
fn a_spill_landing_in_a_shared_range_replaces_it() {
    // F1:F3 read G2:G100, which G1's spill fills during the same recalculation; a range shared
    // before the spill landed must not be reused after it.
    let mut t = T::new();
    t.load([("G1".to_string(), "=SEQUENCE(100)".to_string())].into_iter().chain((1..=3).map(|r| (format!("F{r}"), "=SUM(G2:G100)".to_string()))));
    for r in 1..=3 {
        assert_eq!(t.num(&format!("F{r}")), 5049.0);
    }
}

/// A workbook of ~3,000 rows mixing what multi-threaded levels must get right: lookups over a
/// shared table, INDIRECT and a defined name reading cells of the same level (so workers hand
/// formulas back), spills and their readers, running sums, text, a cycle.
fn mixed_workbook(threads: u32) -> T {
    let n = 3_000;
    let mut t = T::new();
    t.wb.calc.threads = threads;
    t.wb.names.push(gridcraft_model::DefinedName {
        name: "Base".into(),
        scope: None,
        formula: "Sheet1!$B$7".into(),
        comment: String::new(),
        hidden: false,
    });
    let mut cells: Vec<(String, String)> = Vec::new();
    for r in 1..=n {
        cells.push((format!("A{r}"), format!("={}", (r * 37) % 1000)));
        cells.push((format!("B{r}"), format!("=A{r}*2+ROW()")));
        cells.push((format!("C{r}"), format!("=VLOOKUP({},$A$1:$B${n},2,FALSE)", (r * 37) % 1000)));
        cells.push((format!("D{r}"), "=INDIRECT(\"B\"&ROW())+Base".to_string()));
        cells.push((format!("E{r}"), format!("=IF(MOD(A{r},2)=0,\"even\"&A{r},LEN(\"odd\"&B{r}))")));
        cells.push((format!("F{r}"), if r == 1 { "=B1".to_string() } else { format!("=F{}+B{r}", r - 1) }));
        cells.push((format!("G{r}"), format!("=SUM($B$1:$B${n})/ROW()")));
    }
    cells.push(("J1".to_string(), "=SEQUENCE(50,1,A3)".to_string()));
    cells.push(("K1".to_string(), "=SUM(J1#)+COUNT(J2:J50)".to_string()));
    cells.push(("L1".to_string(), "=L2+1".to_string()));
    cells.push(("L2".to_string(), "=L1+1".to_string()));
    t.load(cells);
    t
}

#[test]
fn multi_threaded_recalc_matches_one_thread() {
    let one = mixed_workbook(1);
    let many = mixed_workbook(8);
    let (a, b) = (one.wb.sheet(0).unwrap(), many.wb.sheet(0).unwrap());
    let mut cells = 0;
    for (c, cell) in a.cells.iter() {
        assert_eq!(cell.value, b.value(c), "{}", c.a1());
        cells += 1;
    }
    assert_eq!(cells, b.cells.iter().count());
    assert_eq!(a.spill, b.spill);
    // Spot checks against values worked out by hand.
    assert_eq!(one.num("D10"), one.num("B10") + one.num("B7"));
    assert_eq!(one.num("C5"), one.num("B5"));
    assert_eq!(one.num("K1"), (0..50).map(|i| f64::from(i) + one.num("A3")).sum::<f64>() + 49.0);
    assert_eq!(many.get("L1"), Value::Error(CellError::Circ));
    // An edit recalculates thousands of formulas in large levels again.
    let mut many = many;
    let mut one = one;
    many.set("A1", "=999");
    one.set("A1", "=999");
    for (c, cell) in one.wb.sheet(0).unwrap().cells.iter() {
        assert_eq!(cell.value, many.wb.sheet(0).unwrap().value(c), "after edit {}", c.a1());
    }
}

#[test]
fn thread_count_follows_the_settings() {
    let mut s = gridcraft_model::CalcSettings { threads: 3, ..Default::default() };
    assert_eq!(crate::recalc::thread_count(&s), if cfg!(target_arch = "wasm32") { 1 } else { 3 });
    s.multi_threaded = false;
    assert_eq!(crate::recalc::thread_count(&s), 1);
    s.multi_threaded = true;
    s.threads = 0;
    assert!(crate::recalc::thread_count(&s) >= 1);
}

#[test]
fn clearing_a_far_blocker_unblocks_a_spill() {
    // The blocker is 79 rows below the anchor: further than the 64-cell window typed edits look
    // at, so only the index of blocked anchors finds it when the cell is cleared.
    let mut t = T::new();
    t.set("A1", "=SEQUENCE(100)");
    t.set("A80", "x");
    assert_eq!(t.get("A1"), Value::Error(CellError::Spill));
    t.set("A80", "");
    assert_eq!((t.num("A1"), t.num("A80")), (1.0, 80.0));
    // Blocked again, then the graph rebuilt from the workbook (as undo does): the index is
    // rebuilt from the cells' values.
    t.set("A90", "y");
    assert_eq!(t.get("A1"), Value::Error(CellError::Spill));
    t.calc.rebuild(&t.wb);
    t.set("A90", "");
    assert_eq!(t.num("A90"), 90.0);
}

#[test]
fn relative_names_in_shared_formulas() {
    // A filled column of `=Left*2` shares one parse; each copy's `Left` is the cell to the left
    // of that copy, not of the formula it was filled from.
    let mut t = T::new();
    t.wb.names.push(gridcraft_model::DefinedName {
        name: "Left".into(),
        scope: None,
        formula: "Sheet1!XFD1".into(),
        comment: String::new(),
        hidden: false,
    });
    let n = 300;
    let base = Formula::new("Left*2");
    {
        let sh = t.wb.sheet_mut(0).unwrap();
        for r in 1..=n {
            sh.cells.set(c(&format!("A{r}")), Cell::value(Value::Number(f64::from(r))));
            sh.cells.set(c(&format!("B{r}")), Cell::formula(base.moved(i64::from(r) - 1, 0)));
        }
    }
    assert!(t.wb.sheet(0).unwrap().cell(c("B150")).unwrap().formula.as_ref().unwrap().is_shared());
    t.calc.rebuild(&t.wb);
    t.calc.recalc_all(&mut t.wb);
    for r in [1, 2, 150, n] {
        assert_eq!(t.num(&format!("B{r}")), f64::from(r) * 2.0, "B{r}");
    }
    t.set("A150", "1000");
    assert_eq!(t.num("B150"), 2000.0);
    assert_eq!(t.num("B151"), 302.0);
}

#[test]
fn xlookup_references_over_an_indexed_table_on_several_threads() {
    // 600 formulas read the same 600-row table: it is shared and indexed, and the level is
    // large enough to run on worker threads. Each returns a reference that SUM reads through.
    let n = 600;
    for threads in [1, 4] {
        let mut t = T::new();
        t.wb.calc.threads = threads;
        let mut cells = Vec::new();
        for r in 1..=n {
            cells.push((format!("A{r}"), format!("=\"k\"&{r}")));
            cells.push((format!("B{r}"), format!("={r}")));
            cells.push((format!("C{r}"), format!("=SUM(B$1:XLOOKUP(\"k\"&ROW(),A$1:A${n},B$1:B${n}))")));
        }
        t.load(cells);
        let tri = |r: u32| f64::from(r * (r + 1) / 2);
        for r in [1, 10, 299, n] {
            assert_eq!(t.num(&format!("C{r}")), tri(r), "C{r}, {threads} threads");
        }
        t.set("B10", "0");
        assert_eq!(t.num("C9"), tri(9), "{threads} threads");
        assert_eq!(t.num("C10"), tri(10) - 10.0, "{threads} threads");
        assert_eq!(t.num(&format!("C{n}")), tri(n) - 10.0, "{threads} threads");
    }
}

#[test]
fn formulas_queued_together_are_not_a_cycle() {
    // E1 reads A2 and A3 before either is done; A3 reads A2. Both are queued behind E1, and A3
    // reading A2 while A2 only waits its turn is not a circular reference.
    let mut t = T::new();
    t.set("D1", "1");
    t.set("A2", "=D1");
    t.set("A3", "=A2+1");
    t.set("E1", "=INDIRECT(\"A2\")+INDIRECT(\"A3\")");
    t.calc.recalc_all(&mut t.wb);
    assert_eq!(t.num("E1"), 3.0);
    assert!(t.calc.circular.is_empty());
    // The same through a defined name over a chain: a SUM of three cells.
    let mut t = T::new();
    t.wb.names.push(gridcraft_model::DefinedName {
        name: "Block".into(),
        scope: None,
        formula: "Sheet1!$A$2:$A$4".into(),
        comment: String::new(),
        hidden: false,
    });
    t.set("C1", "=SUM(Block)");
    t.set("A2", "=1");
    t.set("A3", "=A2+1");
    t.set("A4", "=A3+1");
    t.calc.recalc_all(&mut t.wb);
    assert_eq!(t.num("C1"), 6.0);
    assert!(t.calc.circular.is_empty());
}

fn xorshift(s: &mut u64) -> u64 {
    *s ^= *s << 13;
    *s ^= *s >> 7;
    *s ^= *s << 17;
    *s
}

fn a1(row: u32, col: u32) -> String {
    format!("{}{}", char::from(b'A' + col as u8), row + 1)
}

/// An acyclic sheet whose formulas read cells entered before them, placed anywhere (so row-major
/// order isn't a topological order) and mostly recent ones (so they read chains of one another),
/// directly, through ranges, defined names and INDIRECT. Returns the cells in entry order and the
/// names.
fn random_acyclic_sheet(seed: u64, rows: u32, cols: u32) -> (Vec<(String, String)>, Vec<(String, String)>) {
    let mut s = seed.max(1);
    let mut cells: Vec<(u32, u32)> = (0..rows).flat_map(|r| (0..cols).map(move |c| (r, c))).collect();
    for i in (1..cells.len()).rev() {
        let j = (xorshift(&mut s) % (i as u64 + 1)) as usize;
        cells.swap(i, j);
    }
    let rank: std::collections::HashMap<(u32, u32), usize> = cells.iter().enumerate().map(|(i, c)| (*c, i)).collect();
    let mut out = Vec::new();
    let mut names = Vec::new();
    for (i, &(r, c)) in cells.iter().enumerate() {
        if i < 20 || xorshift(&mut s).is_multiple_of(5) {
            out.push((a1(r, c), format!("{}", xorshift(&mut s) % 100)));
            continue;
        }
        let pick = |s: &mut u64| {
            let back = if xorshift(s).is_multiple_of(4) { xorshift(s) % i as u64 } else { 1 + xorshift(s) % 6 };
            cells[i - back.max(1) as usize]
        };
        let mut terms = Vec::new();
        for _ in 0..1 + xorshift(&mut s) % 3 {
            let (pr, pc) = pick(&mut s);
            terms.push(match xorshift(&mut s) % 4 {
                0 => format!("INDIRECT(\"{}\")", a1(pr, pc)),
                1 => {
                    let name = format!("N{}", names.len());
                    names.push((name.clone(), format!("Sheet1!${}${}", char::from(b'A' + pc as u8), pr + 1)));
                    name
                }
                2 => {
                    // A column range ending at the cell, if every cell in it was entered before.
                    let top = pr.saturating_sub((xorshift(&mut s) % 4) as u32);
                    if (top..=pr).all(|rr| rank.get(&(rr, pc)).is_some_and(|&k| k < i)) {
                        format!("SUM({}:{})", a1(top, pc), a1(pr, pc))
                    } else {
                        a1(pr, pc)
                    }
                }
                _ => a1(pr, pc),
            });
        }
        out.push((a1(r, c), format!("=MOD({},1000)", terms.join("+"))));
    }
    (out, names)
}

#[test]
fn full_recalc_matches_entering_cells_one_by_one_at_any_thread_count() {
    // `GRIDCRAFT_ORDER_FUZZ_SEEDS=50` soaks it.
    let seeds: u64 = std::env::var("GRIDCRAFT_ORDER_FUZZ_SEEDS").ok().and_then(|v| v.parse().ok()).unwrap_or(2);
    for seed in 1..=seeds {
        let (cells, names) = random_acyclic_sheet(seed * 7919, 400, 8);
        let book = |threads: u32| {
            let mut t = T::new();
            t.wb.calc.multi_threaded = threads > 1;
            t.wb.calc.threads = threads;
            for (name, formula) in &names {
                t.wb.names.push(gridcraft_model::DefinedName {
                    name: name.clone(),
                    scope: None,
                    formula: formula.clone(),
                    comment: String::new(),
                    hidden: false,
                });
            }
            t
        };
        // Reference: entered in order, each formula reading only finished cells.
        let mut want = book(1);
        for (at, input) in &cells {
            want.set(at, input);
        }
        for threads in [1, 8] {
            let mut t = book(threads);
            for (at, input) in &cells {
                let cell = if input.starts_with('=') {
                    Cell::formula(Formula::new(input))
                } else {
                    Cell::value(gridcraft_core::parse::parse_input(input, t.wb.date_system).value)
                };
                t.wb.sheet_mut(0).unwrap().cells.set(c(at), cell);
            }
            t.calc.rebuild(&t.wb);
            t.calc.recalc_all(&mut t.wb);
            let wrong: Vec<_> =
                cells.iter().filter(|(at, _)| t.get(at) != want.get(at)).map(|(at, f)| (at, f, t.get(at), want.get(at))).take(5).collect();
            assert!(wrong.is_empty(), "seed {seed}, {threads} threads: {wrong:?}");
        }
    }
}
