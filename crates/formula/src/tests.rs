use super::*;
use crate::adjust::{Axis, Edit, adjust, rename_sheet, shift_relative};

fn rt(src: &str) -> String {
    print(&parse(src).unwrap_or_else(|e| panic!("{src}: {e}")))
}

#[test]
fn quoted_sheet_qualified_name_keeps_quotes() {
    assert_eq!(rt("'My Sheet'!Total"), "'My Sheet'!Total");
    assert_eq!(rt("'Data'!Total"), "Data!Total");
    assert_eq!(rt("'It''s'!Total+1"), "'It''s'!Total+1");
}

#[test]
fn intersection_with_deleted_operand_reparses() {
    // Deleting the columns of an intersection operand prints `A1:B2 #REF!`; that text must parse back.
    assert_eq!(rt("A1:B2 #REF!"), "A1:B2 #REF!");
    assert_eq!(rt("#REF! A1:B2"), "#REF! A1:B2");
    assert_eq!(parse("A1:B2 #REF!"), parse("A1:B2 (#REF!)").map(|e| strip_paren(&e)));
}

fn strip_paren(e: &Expr) -> Expr {
    match e {
        Expr::Binary(op, a, b) => Expr::Binary(*op, Box::new(strip_paren(a)), Box::new(strip_paren(b))),
        Expr::Paren(x) => strip_paren(x),
        other => other.clone(),
    }
}

#[test]
fn roundtrips() {
    for f in [
        "1+2*3",
        "SUM(A1:B2)",
        "SUM($A$1:B$2,C3)",
        "IF(A1>0,\"yes\",\"no\")",
        "-2^2",
        "A1%",
        "Sheet2!A1+'My Sheet'!$B$3",
        "SUM(Sheet1:Sheet3!A1)",
        "SUM(A:A)",
        "SUM(3:5)",
        "{1,2;3,4}",
        "{-1,\"a\",TRUE,#N/A}",
        "VLOOKUP(A1,Table1[#All],2,FALSE)",
        "Table1[Sales]",
        "Table1[@Sales]",
        "Table1[[#Headers],[A]:[C]]",
        "\"a\"\"b\"&C1",
        "SUM((A1:A3,C1:C3))",
        "A1:C3 B2:B5",
        "LAMBDA(x,x+1)(2)",
        "F(1,,3)",
        "@A1:A10",
        "A1<>B1",
        "A1<=B1",
        "#REF!+1",
        "1E+20",
        "MyName*2",
        "Sheet1!MyName",
        "SUM(A1:B2) ",
    ] {
        let p = rt(f);
        assert_eq!(p, f.trim(), "round trip of {f}");
    }
}

#[test]
fn normalises() {
    assert_eq!(rt("=sum(a1:b2)"), "SUM(A1:B2)");
    assert_eq!(rt("= 1 + 2"), "1+2");
    assert_eq!(rt("_xlfn.XLOOKUP(1,A:A,B:B)"), "XLOOKUP(1,A:A,B:B)");
    assert_eq!(rt("_xlfn.GROUPBY(A1:A3,B1:B3,_xleta.SUM)"), "GROUPBY(A1:A3,B1:B3,SUM)");
    assert_eq!(rt("_XLUDF.concat(\"a\",\"b\")"), "_xludf.CONCAT(\"a\",\"b\")");
    assert_eq!(rt("true"), "TRUE");
    assert_eq!(rt("'Sheet1'!A1"), "Sheet1!A1");
}

#[test]
fn spill_operator() {
    for f in ["A1#", "Sheet1!$D$1#", "SUM(B2#)", "'My Sheet'!A1#*2", "ROWS(A1#)+1", "IF(A1,#N/A,#REF!)"] {
        assert_eq!(rt(f), f);
    }
    assert!(matches!(parse("A1#").unwrap(), Expr::Unary(UnOp::Spill, _)));
    // Files spell it ANCHORARRAY.
    assert_eq!(rt("_xlfn.ANCHORARRAY(Sheet1!$D$1)"), "Sheet1!$D$1#");
    assert_eq!(rt("SUM(ANCHORARRAY(B2))"), "SUM(B2#)");
    // Only a cell can spill.
    assert!(parse("A1:B2#").is_err());
    assert!(parse("Name#").is_err());
    // Deleting the anchor's sheet leaves #REF!.
    assert_eq!(print(&crate::adjust::delete_sheet(parse("SUM(Data!A1#)").unwrap(), "Data", &[])), "SUM(#REF!)");
}

#[test]
fn delete_sheet_shrinks_3d_span() {
    let order: Vec<String> = ["Sheet1", "Sheet2", "Sheet3", "Results"].iter().map(|s| s.to_string()).collect();
    let del = |f: &str, n: &str| print(&crate::adjust::delete_sheet(parse(f).unwrap(), n, &order));
    assert_eq!(del("SUM(Sheet1:Sheet3!A1)", "Sheet1"), "SUM(Sheet2:Sheet3!A1)");
    assert_eq!(del("SUM(Sheet1:Sheet3!A1)", "Sheet3"), "SUM(Sheet1:Sheet2!A1)");
    assert_eq!(del("SUM(Sheet3:Sheet1!A1)", "Sheet1"), "SUM(Sheet3:Sheet2!A1)");
    assert_eq!(del("SUM(Sheet1:Sheet3!A1)", "Sheet2"), "SUM(Sheet1:Sheet3!A1)");
    assert_eq!(del("SUM(Sheet1:Sheet2!A1)", "Sheet1"), "SUM(Sheet2!A1)");
    assert_eq!(del("Sheet1!A1+Sheet3!A1", "Sheet1"), "#REF!+Sheet3!A1");
}

#[test]
fn precedence() {
    let e = parse("1+2*3").unwrap();
    assert!(matches!(e, Expr::Binary(BinOp::Add, _, _)));
    let e = parse("-2^2").unwrap();
    assert!(matches!(e, Expr::Binary(BinOp::Pow, _, _)));
    let e = parse("1&2=\"12\"").unwrap();
    assert!(matches!(e, Expr::Binary(BinOp::Eq, _, _)));
    let e = parse("2^3^2").unwrap(); // left associative: (2^3)^2
    if let Expr::Binary(BinOp::Pow, l, _) = e {
        assert!(matches!(*l, Expr::Binary(BinOp::Pow, _, _)));
    } else {
        panic!()
    }
}

#[test]
fn errors() {
    for bad in ["1+", "SUM(1,2", "(1", "{1,2;3}", "\"abc", "A1:", "1 2", "{A1}", ")", "Sheet1!", "#FOO!"] {
        assert!(parse(bad).is_err(), "{bad} should fail");
    }
}

#[test]
fn no_panic_on_garbage() {
    let samples =
        ["(((((", "[[[", "'''", "!!!", "$$$", "=@@@", "A1:B2:C3:D4", "{,}", "{;}", "F(,,,,)", ":::", "1e", "1e+", ".", "..", "$A$", "XFD1048577"];
    for s in samples {
        let _ = parse(s);
    }
    let deep = "(".repeat(5000) + "1" + &")".repeat(5000);
    assert!(parse(&deep).is_err());
}

#[test]
fn references() {
    let e = parse("SUM(A1:B2)+Sheet2!C3*D4").unwrap();
    let refs: Vec<String> = e.references().iter().map(|r| printer::reference_a1(r)).collect();
    assert_eq!(refs, ["A1:B2", "Sheet2!C3", "D4"]);
}

#[test]
fn copy_shift() {
    let e = parse("A1+$A1+A$1+$A$1+SUM(B2:C3)").unwrap();
    assert_eq!(print(&shift_relative(e, 2, 1)), "B3+$A3+B$1+$A$1+SUM(C4:D5)");
    let e = parse("A1").unwrap();
    assert_eq!(print(&shift_relative(e, -1, 0)), "#REF!");
    let e = parse("SUM(A:A)").unwrap();
    assert_eq!(print(&shift_relative(e, 5, 1)), "SUM(B:B)");
}

#[test]
fn insert_delete() {
    let ins = Edit::Insert { axis: Axis::Rows, at: 2, count: 2 };
    let e = parse("A1+A3+SUM(A2:A5)+Other!A3").unwrap();
    assert_eq!(print(&adjust(e, "Sheet1", "Sheet1", &ins)), "A1+A5+SUM(A2:A7)+Other!A3");
    let del = Edit::Delete { axis: Axis::Rows, at: 2, count: 2 }; // rows 3-4
    let e = parse("A1+A3+SUM(A2:A5)+A6").unwrap();
    assert_eq!(print(&adjust(e, "Sheet1", "Sheet1", &del)), "A1+#REF!+SUM(A2:A3)+A4");
    let del = Edit::Delete { axis: Axis::Cols, at: 0, count: 1 };
    let e = parse("SUM(A1:C1)+B2").unwrap();
    assert_eq!(print(&adjust(e, "S", "S", &del)), "SUM(A1:B1)+A2");
    let e = parse("Sheet1!B2").unwrap();
    assert_eq!(print(&adjust(e, "Other", "Sheet1", &del)), "Sheet1!A2");
    let e = parse("SUM(A:A)").unwrap();
    assert_eq!(print(&adjust(e, "S", "S", &del)), "SUM(#REF!)");
}

#[test]
fn insert_delete_cells() {
    let range = |a: &str| gridcraft_core::RangeRef::parse(a).unwrap();
    let adj = |f: &str, e: &Edit| print(&adjust(parse(f).unwrap(), "S", "S", e));
    // Insert A3:A4, shifting down: column A moves, other columns and wider ranges don't.
    let ins = Edit::InsertCells { axis: Axis::Rows, range: range("A3:A4") };
    assert_eq!(adj("SUM(A1:A5)+A2+A3+$A$9+B3+SUM(A1:B5)+SUM(A:A)", &ins), "SUM(A1:A7)+A2+A5+$A$11+B3+SUM(A1:B5)+SUM(A:A)");
    // Insert B2 shifting right.
    let ins = Edit::InsertCells { axis: Axis::Cols, range: range("B2") };
    assert_eq!(adj("A2+B2+C2+B3+SUM(A2:C2)", &ins), "A2+C2+D2+B3+SUM(A2:D2)");
    // Delete A3:A4 shifting up: references into it become #REF!, those below move up.
    let del = Edit::DeleteCells { axis: Axis::Rows, range: range("A3:A4") };
    assert_eq!(adj("A2+A3+A4+A5+B5+SUM(A1:A10)+SUM(A3:A4)+SUM(A1:B10)", &del), "A2+#REF!+#REF!+A3+B5+SUM(A1:A8)+SUM(#REF!)+SUM(A1:B10)");
    // Delete B1:C2 shifting left.
    let del = Edit::DeleteCells { axis: Axis::Cols, range: range("B1:C2") };
    assert_eq!(adj("A1+B1+D1+D2+D3+SUM(A1:E1)", &del), "A1+#REF!+B1+B2+D3+SUM(A1:C1)");
}

#[test]
fn moves_and_renames() {
    let mv = Edit::Move { from: gridcraft_core::RangeRef::parse("A1:B2").unwrap(), to_row: 9, to_col: 3 };
    let e = parse("A1+B2+C3+SUM(A1:B2)").unwrap();
    assert_eq!(print(&adjust(e, "S", "S", &mv)), "D10+E11+C3+SUM(D10:E11)");
    let mv = Edit::Move { from: gridcraft_core::RangeRef::parse("B:C").unwrap(), to_row: 0, to_col: 4 };
    assert_eq!(print(&adjust(parse("SUM(B:B)+SUM($C:$C)+D1").unwrap(), "S", "S", &mv)), "SUM(E:E)+SUM($F:$F)+D1");
    let e = parse("Old!A1+'Old'!B2").unwrap();
    assert_eq!(print(&rename_sheet(e, "Old", "New Name")), "'New Name'!A1+'New Name'!B2");
}

#[test]
fn rename_sheet_updates_qualified_names() {
    let e = parse("Data!MyName+MyName+Other!MyName").unwrap();
    assert_eq!(print(&rename_sheet(e, "data", "Numbers")), "Numbers!MyName+MyName+Other!MyName");
    let e = parse("Data!MyName").unwrap();
    assert_eq!(print(&rename_sheet(e, "Data", "New Name")), "'New Name'!MyName");
    // Stored bare, like a parsed name, so the evaluator finds the sheet and a second rename still matches.
    let e = rename_sheet(parse("Data!MyName").unwrap(), "Data", "It's");
    assert_eq!(e, Expr::Name("It's!MyName".into()));
    assert_eq!(print(&rename_sheet(e, "It's", "Totals")), "Totals!MyName");
}

#[test]
fn moves_between_sheets() {
    let mv = Edit::MoveToSheet { from: gridcraft_core::RangeRef::parse("A1:B2").unwrap(), to_sheet: "New Name".into(), to_row: 9, to_col: 3 };
    for (host, input, expected) in [
        ("Source", "A1+$B$2+C3+SUM(A1:B2)", "'New Name'!D10+'New Name'!$E$11+C3+SUM('New Name'!D10:E11)"),
        ("Other", "A1+sOuRcE!$B2+Other!B2", "A1+'New Name'!$E11+Other!B2"),
        ("Source", "SUM(A1:C3)+SUM(Source:Other!A1)", "SUM(A1:C3)+SUM(Source:Other!A1)"),
    ] {
        assert_eq!(print(&adjust(parse(input).unwrap(), host, "Source", &mv)), expected);
    }
    let edge = Edit::MoveToSheet {
        from: gridcraft_core::RangeRef::parse("A1:B2").unwrap(),
        to_sheet: "New Name".into(),
        to_row: gridcraft_core::MAX_ROWS - 1,
        to_col: 3,
    };
    assert_eq!(print(&adjust(parse("B2").unwrap(), "Source", "Source", &edge)), "#REF!");
    for (from, input, row, col, expected) in [("A:A", "$A:$A", 0, 2, "Dest!$C:$C"), ("1:1", "$1:$1", 2, 0, "Dest!$3:$3")] {
        let mv = Edit::MoveToSheet { from: gridcraft_core::RangeRef::parse(from).unwrap(), to_sheet: "Dest".into(), to_row: row, to_col: col };
        assert_eq!(print(&adjust(parse(input).unwrap(), "Source", "Source", &mv)), expected);
    }
}

#[test]
fn r1c1() {
    let e = parse("A1+$B$2+SUM(C:C)").unwrap();
    assert_eq!(print_r1c1(&e, gridcraft_core::CellRef::new(1, 1)), "R[-1]C[-1]+R2C2+SUM(C[1])");
}

#[test]
fn spans() {
    let toks = lexer::tokenize("SUM(A1, B2)").unwrap();
    let a1 = toks.iter().find(|t| t.tok == lexer::Tok::Word("A1".into())).unwrap();
    assert_eq!((a1.start, a1.end), (4, 6));
}

#[test]
fn unbracketed_this_row_column() {
    // Writers such as xlsxwriter spell `[@Units]` as `[[#This Row],Units]`.
    let p = |f: &str| parse(f).unwrap_or_else(|e| panic!("{f}: {e}"));
    let Expr::Binary(BinOp::Mul, a, _) = p("[[#This Row],Units]*[[#This Row],Price]") else { panic!() };
    let Expr::Struct(s) = *a else { panic!() };
    assert_eq!((s.specifiers.as_slice(), s.col_start.as_deref()), ([StructItem::ThisRow].as_slice(), Some("Units")));
    assert_eq!(rt("[[#This Row],Units]*[[#This Row],Price]"), "[@Units]*[@Price]");
    assert_eq!(rt("Sales[[#This Row],Units]"), "Sales[@Units]");
    assert_eq!(rt("Sales[[#This Row], Units ]"), "Sales[@Units]");
    assert_eq!(rt("[[#This Row],Units]+[@[Unit Price]]+Sales[[#This Row],[Qty]]"), "[@Units]+[@[Unit Price]]+Sales[@Qty]");
    // Unexpected text between the brackets is an error, never silently dropped.
    for bad in [
        "[[#This Row]x]",
        "[[#This Row],[A]]x",
        "[[#Data],Col]",
        "[[#This Row],A:B]",
        "[x[#This Row]]",
        "[[#This Row],]x",
        "Sales[[#All],[A]]junk",
        "Sales[[A],[B],[C]]",
        "Sales[@[A]x]",
    ] {
        assert!(parse(bad).is_err(), "{bad} should fail");
    }
}

#[test]
fn struct_column_keeps_edge_spaces() {
    // The spaces are part of the column name: `Table1[Qty ]` names `Qty `, never `Qty`.
    let Expr::Struct(s) = parse("Table1[Qty ]").unwrap() else { panic!() };
    assert_eq!(s.col_start.as_deref(), Some("Qty "));
    let Expr::Struct(s) = parse("Table1[ Qty]").unwrap() else { panic!() };
    assert_eq!(s.col_start.as_deref(), Some(" Qty"));
    // Excel writes such a column in the double-bracket form.
    assert_eq!(rt("Table1[Qty ]"), "Table1[[Qty ]]");
    for f in ["Table1[[Qty ]]", "Table1[[ Sum for Q2 ]]", "Table1[[#Totals],[Qty ]]", "Table1[@[Qty ]]", "Table1[Unit Price]"] {
        assert_eq!(rt(f), f);
    }
}

#[test]
fn trim_references() {
    for f in [
        "SUM(A1:.E10)",
        "SUM(A1.:E10)",
        "SUM(A1.:.E10)",
        "SUM(A:.A)",
        "SUM($A.:.$C)",
        "ROWS(1:.5)",
        "ROWS($1.:$5)",
        "ROWS(1.:.5)",
        "Sheet2!$A$1:.B9",
        "Q1.2026!A1.:.B2",
        "'Q1 2026'!A1:.B2",
        "SUM(A1:.A9,B1.:B9)",
        "A1:.A9*2",
    ] {
        assert_eq!(rt(f), f, "round trip of {f}");
    }
    let Expr::Call(_, args) = parse("SUM(A1:.E10)").unwrap() else { panic!() };
    assert!(matches!(args.as_slice(), [Expr::Unary(UnOp::Trim(TrimMode::Trailing), r)] if matches!(**r, Expr::Ref(_))));
    // Decimals, row ranges and dotted names keep their meaning.
    assert_eq!(parse("1.5+.5").unwrap(), Expr::Binary(BinOp::Add, Box::new(Expr::Number(1.5)), Box::new(Expr::Number(0.5))));
    for f in ["SUM(1:5)", "Q1.2026!A1:B2", "my.name+1", "SUM(A1:B2)*1.5", "1.:5"] {
        assert_eq!(rt(f), f);
    }
    assert_eq!(rt("'Q1.2026'!A1:B2"), "Q1.2026!A1:B2");
    assert!(matches!(parse("Q1.2026!A1:B2").unwrap(), Expr::Ref(_)));
    // File call forms.
    assert_eq!(rt("SUM(_xlfn._TRO_TRAILING(A1:E10))"), "SUM(A1:.E10)");
    assert_eq!(rt("_xlfn._TRO_LEADING(A:A)"), "A.:A");
    assert_eq!(rt("_xlfn._TRO_ALL(Sheet1!A1:B2)"), "Sheet1!A1.:.B2");
    // Not a range: printed as the equivalent call.
    assert_eq!(rt("_xlfn._TRO_ALL(B2)"), "TRIMRANGE(B2,3,3)");
    for bad in ["A1.:", ".:A1", "A1:.:B2", "A1.:.:B2"] {
        assert!(parse(bad).is_err(), "{bad} should fail");
    }
    // R1C1 keeps the operator.
    assert_eq!(print_r1c1(&parse("SUM(A1:.B2)+ROWS(2.:2)").unwrap(), gridcraft_core::CellRef::new(0, 0)), "SUM(RC:.R[1]C[1])+ROWS(R[1].:R[1])");
}

#[test]
fn trim_references_adjust() {
    let ins = Edit::Insert { axis: Axis::Rows, at: 2, count: 2 };
    let e = parse("SUM(A1:.A5)+SUM(B:.B)+ROWS(2.:.4)").unwrap();
    assert_eq!(print(&adjust(e, "S", "S", &ins)), "SUM(A1:.A7)+SUM(B:.B)+ROWS(2.:.6)");
    let del = Edit::Delete { axis: Axis::Cols, at: 0, count: 1 };
    let e = parse("SUM(A1.:C1)+SUM(A:.A)").unwrap();
    assert_eq!(print(&adjust(e, "S", "S", &del)), "SUM(A1.:B1)+SUM(#REF!)");
    assert_eq!(print(&shift_relative(parse("SUM(A1.:.B2)").unwrap(), 1, 1)), "SUM(B2.:.C3)");
    let refs: Vec<String> = parse("SUM(Sheet2!A1:.B5)").unwrap().references().iter().map(|r| printer::reference_a1(r)).collect();
    assert_eq!(refs, ["Sheet2!A1:B5"]);
}

#[test]
fn prints_long_flat_chain_without_deep_recursion() {
    // Small stack: a recursive printer overflows long before 20000 levels.
    let handle = std::thread::Builder::new()
        .stack_size(256 * 1024)
        .spawn(|| {
            let mut e = Expr::Number(1.0);
            for _ in 0..20000 {
                e = Expr::Binary(BinOp::Add, Box::new(e), Box::new(Expr::Number(1.0)));
            }
            let text = print(&e);
            let r1c1 = print_r1c1(&e, gridcraft_core::CellRef { row: 0, col: 0 });
            // Dropping the deep tree recurses; leak it so only the printer is under test.
            std::mem::forget(e);
            (text, r1c1)
        })
        .expect("spawn");
    let (text, r1c1) = handle.join().expect("printer must not overflow the stack");
    assert_eq!(text, vec!["1"; 20001].join("+"));
    assert_eq!(r1c1, text);
}

/// Parses, checks the printed text, that it reads back the same, and what files hold for it.
fn table_case(src: &str, printed: &str, file: &str) {
    let e = parse(src).unwrap_or_else(|x| panic!("{src}: {x}"));
    assert_eq!(print(&e), printed, "{src}");
    assert_eq!(parse(printed).unwrap(), e, "{src}");
    let low = lower_extensions(e.clone());
    assert_eq!(print(&low), file, "{src}");
    assert_eq!(parse(file).unwrap().map(&mut raise_extension), e, "{src}");
}

#[test]
fn rows_then_columns() {
    // A key in the first column, the first row it matches.
    table_case("Table1[\"Bolt\"][Price]", "Table1[\"Bolt\"][Price]", "INDEX(Table1[Price],MATCH(\"Bolt\",INDEX(Table1[],0,1),0))");
    table_case("Table1[ [\"Bolt\"] , [Price] ]", "Table1[\"Bolt\"][Price]", "INDEX(Table1[Price],MATCH(\"Bolt\",INDEX(Table1[],0,1),0))");
    table_case("Table1[A2&\"x\"][Unit Price]", "Table1[A2&\"x\"][Unit Price]", "INDEX(Table1[Unit Price],MATCH(A2&\"x\",INDEX(Table1[],0,1),0))");
    table_case("Products[[@Item]][Price]", "Products[[@Item]][Price]", "INDEX(Products[Price],MATCH([@Item],INDEX(Products[],0,1),0))");
    table_case("Table1[(A1=5)][Price]", "Table1[(A1=5)][Price]", "INDEX(Table1[Price],MATCH((A1=5),INDEX(Table1[],0,1),0))");
    // Several columns, a span, every column.
    table_case(
        "Table1[\"Bolt\"][Price, Stock]",
        "Table1[\"Bolt\"][Price,Stock]",
        "INDEX(HSTACK(Table1[Price],Table1[Stock]),MATCH(\"Bolt\",INDEX(Table1[],0,1),0),0)",
    );
    table_case(
        "Table1[\"Bolt\"][Price:Stock]",
        "Table1[\"Bolt\"][Price:Stock]",
        "INDEX(Table1[[Price]:[Stock]],MATCH(\"Bolt\",INDEX(Table1[],0,1),0),0)",
    );
    table_case("Table1[\"Bolt\"][]", "Table1[\"Bolt\"][]", "INDEX(Table1[],MATCH(\"Bolt\",INDEX(Table1[],0,1),0),0)");
    // Conditions: every matching row.
    table_case("Table1[Stock>400][Price]", "Table1[Stock>400][Price]", "FILTER(Table1[Price],Table1[Stock]>400,NA())");
    table_case(
        "Table1[Stock >= 400 , [Product Name]<>\"Nut\", Qty<A2*2][Price, \"Unit Cost\":Margin]",
        "Table1[Stock>=400,[Product Name]<>\"Nut\",Qty<A2*2][Price,Unit Cost:Margin]",
        "FILTER(HSTACK(Table1[Price],Table1[[Unit Cost]:[Margin]]),(Table1[Stock]>=400)*(Table1[Product Name]<>\"Nut\")*(Table1[Qty]<A2*2),NA())",
    );
    table_case(
        "Table1[Code=\"a,b]\",Stock>MAX(1,2)][]",
        "Table1[Code=\"a,b]\",Stock>MAX(1,2)][]",
        "FILTER(Table1[],(Table1[Code]=\"a,b]\")*(Table1[Stock]>MAX(1,2)),NA())",
    );
    table_case("Table1[Stock<=(A1>2)][Price]", "Table1[Stock<=(A1>2)][Price]", "FILTER(Table1[Price],Table1[Stock]<=(A1>2),NA())");
    table_case(
        "T[Stock>U[Kind=[@Kind]][Min]][Price]",
        "T[Stock>U[Kind=[@Kind]][Min]][Price]",
        "FILTER(T[Price],T[Stock]>FILTER(U[Min],U[Kind]=[@Kind],NA()),NA())",
    );
    // Neighbouring rows, and the other row items with several columns.
    table_case("Table1[#Prev Row][Qty]", "Table1[#Prev Row][Qty]", "OFFSET(Table1[@Qty],-1,0)");
    table_case("[[#next row]][Unit Price]", "[#Next Row][Unit Price]", "OFFSET([@[Unit Price]],1,0)");
    table_case("Table1[[#Prev Row],[Qty]]", "Table1[#Prev Row][Qty]", "OFFSET(Table1[@Qty],-1,0)");
    table_case("Table1[#Next Row][A:C]", "Table1[#Next Row][A:C]", "OFFSET(Table1[[#This Row],[A]:[C]],1,0)");
    table_case("Table1[#Prev Row][Qty, Price]", "Table1[#Prev Row][Qty,Price]", "HSTACK(OFFSET(Table1[@Qty],-1,0),OFFSET(Table1[@Price],-1,0))");
    table_case("Table1[@][Qty, Price]", "Table1[@][Qty,Price]", "HSTACK(Table1[@Qty],Table1[@Price])");
    table_case("Table1[#Headers][Qty, Price]", "Table1[#Headers][Qty,Price]", "HSTACK(Table1[[#Headers],[Qty]],Table1[[#Headers],[Price]])");
    // One pair of brackets is always columns: lists, spans and quoted names.
    table_case("Table1[Qty, Price]", "Table1[][Qty,Price]", "HSTACK(Table1[Qty],Table1[Price])");
    table_case("Table1[@Qty, Price]", "Table1[@][Qty,Price]", "HSTACK(Table1[@Qty],Table1[@Price])");
    // ... and when Excel's forms can say it, they are Excel's forms.
    for (src, printed) in [
        ("Table1[Price:Stock]", "Table1[[Price]:[Stock]]"),
        ("Table1[\"Unit Price\"]", "Table1[Unit Price]"),
        ("Table1[\"Bolt\"]", "Table1[Bolt]"),
        ("Table1[#Headers][Qty]", "Table1[[#Headers],[Qty]]"),
        ("Table1[@][Qty]", "Table1[@Qty]"),
        ("Table1[#This Row][$Qty]", "Table1[@$Qty]"),
        ("Table1[][Qty]", "Table1[Qty]"),
        ("Table1[[#Data]][]", "Table1[#Data]"),
        ("Table1[[#Headers],[#Data],[Qty]]", "Table1[[#Headers],[#Data],[Qty]]"),
        ("Table1[[#This Row],Qty]", "Table1[@Qty]"),
        ("Table1[[Qty]]", "Table1[Qty]"),
        ("Table1[[Qty]:[Price]]", "Table1[[Qty]:[Price]]"),
    ] {
        assert_eq!(rt(src), printed, "{src}");
        assert!(matches!(parse(src), Ok(Expr::Struct(_))), "{src}");
    }
    let Expr::Table(t) = parse("Table1[Stock>400,Name<>\"x\"][Price]").unwrap() else { panic!() };
    let RowSel::Filter(c) = &t.rows else { panic!() };
    assert_eq!(c.iter().map(|c| c.op).collect::<Vec<_>>(), [BinOp::Gt, BinOp::Ne]);
    for bad in [
        "Table1[\"Nut\",Stock>1][Price]",
        "Table1[Stock>][Price]",
        "Table1[Stock>1,][Price]",
        "Table1[Stock>1 2][Price]",
        "Table1[#Headers,#Prev Row][Qty]",
        "Table1[\"x\"][#Headers]",
        "Table1[\"x\"][$A:B]",
        "Table1[\"x\"][A:B:C]",
        "Table1[\"x\"][\"A\" B]",
    ] {
        assert!(parse(bad).is_err(), "{bad}");
    }
    // Nesting is bounded.
    let deep = format!("{}1{}", "T[".repeat(5000), "][a]".repeat(5000));
    assert!(parse(&deep).is_err());
}

#[test]
fn quotes_inside_table_brackets() {
    // A quote in a column name is no text literal, whatever follows the reference.
    for src in ["T[[x,\"y]]&\"k\"", "T[[#Headers],[x,\"y]]&\"k\"", "T[[#This Row],5\" x]&\"k\"", "T[5\" pipe]&\"k\""] {
        let Ok(Expr::Binary(_, a, _)) = parse(src) else { panic!("{src}") };
        assert!(matches!(*a, Expr::Struct(_)), "{src}");
    }
    let Expr::Table(t) = parse("T[[5\" pipe]=3][Price]").unwrap() else { panic!() };
    let RowSel::Filter(c) = &t.rows else { panic!() };
    assert_eq!(c[0].column, "5\" pipe");
    assert_eq!(rt("T[[5\" pipe]=3][Price]&\"k\""), "T[[5\" pipe]=3][Price]&\"k\"");
    // A column name with a comma, or starting with `$`, is written in its own brackets.
    let comma = Expr::Struct(StructRef { table: "T".into(), specifiers: vec![], col_start: Some("a,b".into()), col_end: None, col_abs: false });
    assert_eq!(print(&comma), "T[[a,b]]");
    assert_eq!(parse("T[[a,b]]").unwrap(), comma);
}

#[test]
fn files_from_earlier_builds_read_back() {
    for (old, now) in [
        ("VLOOKUP(A2,Table1[],MATCH(\"What~?\",Table1[#Headers],0),FALSE)", "Table1[A2][What?]"),
        ("VLOOKUP(A2,Table1[],MATCH(\"Price\",Table1[#Headers],0),0)", "Table1[A2][$Price]"),
    ] {
        assert_eq!(print(&parse(old).unwrap().map(&mut raise_extension)), now, "{old}");
    }
    // Formulas of any other shape stay as written.
    for src in [
        "VLOOKUP(A2,Table1[],2,FALSE)",
        "VLOOKUP(A2,Table1[],MATCH(\"Q*\",Table1[#Headers],0),FALSE)",
        "INDEX(Table1[Qty],MATCH(A2,Table1[Product],0))",
        "INDEX(Table1[Qty],MATCH(A2,INDEX(Table1[],0,2),0))",
        "INDEX(Table1[Qty],MATCH(A2,INDEX(Table2[],0,1),0))",
        "INDEX(Table1[Qty],MATCH(A2,INDEX(Table1[],0,1),1))",
        "INDEX(Table1[Qty],MATCH(A2,INDEX(Table1[],0,1),0),0)",
        "INDEX(Table1[],MATCH(A2,INDEX(Table1[],0,1),0))",
        "FILTER(Table1[Qty],Table1[Product]=A2)",
        "FILTER(Table1[Qty],(Table1[Product]=A2),NA())",
        "FILTER(Table1[Qty],Table2[Product]=A2,NA())",
        "FILTER(Table1[Qty],A2=Table1[Product],NA())",
        "HSTACK(Table1[Qty],Table2[Price])",
        "HSTACK(Table1[Qty],Table1[@Price])",
        "HSTACK(Table1[Qty])",
        "OFFSET(Table1[@Qty],-2,0)",
        "OFFSET(Table1[Qty],-1,0)",
    ] {
        let e = parse(src).unwrap();
        assert_eq!(e.clone().map(&mut raise_extension), e, "{src}");
    }
}

#[test]
fn anchored_table_columns() {
    for (src, printed) in [
        ("Table1[$Qty]", "Table1[$Qty]"),
        ("Table1[$[Unit Price]]", "Table1[$[Unit Price]]"),
        ("Table1[@$Qty]", "Table1[@$Qty]"),
        ("Table1[[#Headers],$[Qty]]", "Table1[[#Headers],$[Qty]]"),
        ("Table1[[Qty]:[Qty]]", "Table1[$Qty]"),
        ("Table1[[#This Row],[Unit Price]:[Unit Price]]", "Table1[@$[Unit Price]]"),
        ("Table1[\"Bolt\"][$Price,Stock]", "Table1[\"Bolt\"][$Price,Stock]"),
        ("Table1[$Stock>1,Qty<2][$\"Unit Price\"]", "Table1[$Stock>1,Qty<2][$Unit Price]"),
        ("Table1[[$ Amount]]", "Table1[[$ Amount]]"),
    ] {
        assert_eq!(rt(src), printed, "{src}");
    }
    table_case("Table1[A2][$Price]", "Table1[A2][$Price]", "INDEX(Table1[$Price],MATCH(A2,INDEX(Table1[],0,1),0))");
    table_case("Table1[$Code=A2][$Price]", "Table1[$Code=A2][$Price]", "FILTER(Table1[$Price],Table1[$Code]=A2,NA())");
    let Expr::Struct(s) = parse("Table1[@$Qty]").unwrap() else { panic!() };
    assert_eq!(printer::struct_ref_file(&s), "Table1[[#This Row],[Qty]:[Qty]]");
    for bad in ["Table1[$[Qty]:[Price]]", "Table1[$[#Headers]]", "Table1[[#Headers],$[#Data]]"] {
        assert!(parse(bad).is_err(), "{bad}");
    }
}

#[test]
fn table_columns_move_with_a_fill_sideways() {
    let columns = |table: &str| (table == "T" || table.is_empty()).then(|| vec!["A".to_string(), "B".to_string(), "C".to_string()]);
    let fill = |src: &str, dc: i64| print(&adjust::shift_table_columns(parse(src).unwrap(), dc, &columns));
    assert_eq!(fill("T[A]+T[@B]+[@C]+T[[A]:[B]]", 1), "T[B]+T[@C]+[@A]+T[[B]:[C]]");
    assert_eq!(fill("T[$A]+T[@$B]+SUM(T[[#Totals],$[C]])", 2), "T[$A]+T[@$B]+SUM(T[[#Totals],$[C]])");
    assert_eq!(fill("T[B=[@C]][A,$B]+T[$B=1][A:B]", 1), "T[C=[@A]][B,$B]+T[$B=1][B:C]");
    assert_eq!(fill("T[#Prev Row][A]", 1), "T[#Prev Row][B]");
    assert_eq!(fill("U[A]+T[Z]+T[]+T[#Headers]", 1), "U[A]+T[Z]+T[]+T[#Headers]");
    assert_eq!(fill("T[B]", i64::MAX), "T[B]");
    assert_eq!(fill("T[B]", i64::MIN), "T[C]");
}

#[test]
fn table_reference_keys_follow_copies() {
    assert_eq!(print(&adjust::shift_relative(parse("Table1[A2][Price]").unwrap(), 1, 1)), "Table1[B3][Price]");
}
