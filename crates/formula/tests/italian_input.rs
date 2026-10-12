use gridcraft_formula::{FormulaLocale, parse, parse_input, print, print_input};

fn input(src: &str) -> String {
    print(&parse_input(src, FormulaLocale::It, |_| false).unwrap())
}

#[test]
fn italian_aliases_and_argument_separators() {
    for (src, expected) in [
        ("=somma(A1:A10)", "SUM(A1:A10)"),
        ("MEDIA(A1:A10)", "AVERAGE(A1:A10)"),
        ("SE(A1>0;\"Sì\";\"No\")", "IF(A1>0,\"Sì\",\"No\")"),
        ("CONTA.NUMERI(A1:A10)", "COUNT(A1:A10)"),
        ("SOMMA.SE(A1:A10;\">0\")", "SUMIF(A1:A10,\">0\")"),
        ("CONTA.SE(A1:A10;\">0\")", "COUNTIF(A1:A10,\">0\")"),
        ("CERCA.VERT(A2;D:E;2;FALSO)", "VLOOKUP(A2,D:E,2,FALSE)"),
        ("OGGI()", "TODAY()"),
        ("=vero", "TRUE"),
        ("=falso", "FALSE"),
        ("VERO()", "TRUE()"),
        ("FALSO()", "FALSE()"),
        ("SOMMA(1;SUM(2;3);MEDIA(4;6))", "SUM(1,SUM(2,3),AVERAGE(4,6))"),
        ("SE(VERO;;)", "IF(TRUE,,)"),
        ("LAMBDA(x;x+1)(2)", "LAMBDA(x,x+1)(2)"),
    ] {
        assert_eq!(input(src), expected, "{src}");
    }
}

#[test]
fn italian_input_preserves_reference_and_array_syntax() {
    for (src, expected) in [
        ("SOMMA({1\\2;3\\4};5)", "SUM({1,2;3,4},5)"),
        ("{VERO\\FALSO;TRUE\\FALSE}", "{TRUE,FALSE;TRUE,FALSE}"),
        ("SUM((A1:A3;C1:C3))", "SUM((A1:A3,C1:C3))"),
        ("\"SOMMA;FALSO\"&\"a\"\"b\"", "\"SOMMA;FALSO\"&\"a\"\"b\""),
        ("'SOMMA;FALSO'!A1+SOMMA!B2+SOMMA!FALSO", "'SOMMA;FALSO'!A1+SOMMA!B2+SOMMA!FALSO"),
    ] {
        assert_eq!(input(src), expected, "{src}");
    }
    for bad in [r"SOMMA({1;2\3})", "{SOMMA(A1)}", "{FALSO;}"] {
        assert!(parse_input(bad, FormulaLocale::It, |_| false).is_err(), "{bad}");
    }
    assert_eq!(print(&parse("SOMMA(1,2)+FALSO").unwrap()), "SOMMA(1,2)+FALSO");
    assert!(parse("SUM(1;2)").is_err());
    assert!(parse("{VERO}").is_err());
}

#[test]
fn italian_input_preserves_defined_and_local_names() {
    let known = |name: &str| ["SOMMA", "FALSO"].iter().any(|n| n.eq_ignore_ascii_case(name));
    let expr = parse_input("SOMMA(2;3)+FALSO+SE(VERO;4;5)", FormulaLocale::It, known).unwrap();
    assert_eq!(print(&expr), "SOMMA(2,3)+FALSO+IF(TRUE,4,5)");
    for (src, expected) in [
        ("LET(FALSO;7;FALSO)", "LET(FALSO,7,FALSO)"),
        ("LET(x;1;falso;2;x+falso)", "LET(x,1,falso,2,x+falso)"),
        ("LAMBDA(FALSO;FALSO+1)(2)", "LAMBDA(FALSO,FALSO+1)(2)"),
    ] {
        assert_eq!(input(src), expected, "{src}");
    }
}

#[test]
fn italian_display_round_trips_text_containing_separators() {
    for (canonical, expected_display) in [
        ("SUM(1.5,2.25)", "SOMMA(1,5;2,25)"),
        ("IF(TRUE,1,0)", "SE(VERO;1;0)"),
        ("VLOOKUP(A1,B:C,2,FALSE)", "CERCA.VERT(A1;B:C;2;FALSO)"),
    ] {
        let expr = parse(canonical).unwrap();
        let rendered = print_input(&expr, FormulaLocale::It, |_| false).unwrap();
        assert_eq!(rendered, expected_display, "from {canonical}");
        let re_parsed = parse_input(&rendered, FormulaLocale::It, |_| false).unwrap();
        assert_eq!(print(&re_parsed), canonical, "roundtrip of {expected_display}");
    }
}
