//! German, Brazilian Portuguese, Russian and the full Spanish table: entry and display syntax.
use gridcraft_core::CellError;
use gridcraft_formula::FormulaLocale::{self, De, Es, PtBr, Ru};
use gridcraft_formula::{parse, parse_input, print, print_input};

const LOCALIZED: [FormulaLocale; 4] = [Es, De, PtBr, Ru];

fn canonical(src: &str, locale: FormulaLocale) -> String {
    print(&parse_input(src, locale, |_| false).unwrap_or_else(|e| panic!("{locale:?} `{src}`: {e}")))
}

fn shown(canonical: &str, locale: FormulaLocale) -> String {
    print_input(&parse(canonical).unwrap(), locale, |_| false).unwrap()
}

#[test]
fn each_language_spells_formulas_like_its_excel() {
    for (locale, local, english) in [
        (De, "SUMME(A1;2,5)", "SUM(A1,2.5)"),
        (De, "WENN(WAHR;SVERWEIS(A2;D:E;2;FALSCH);#NV)", "IF(TRUE,VLOOKUP(A2,D:E,2,FALSE),#N/A)"),
        (De, "SUMME({1.2;3,5.4})", "SUM({1,2;3.5,4})"),
        (De, "ZÄHLENWENN(Tabelle1[[#Kopfzeilen];[Spalte1]];\">1,5\")", "COUNTIF(Tabelle1[[#Headers],[Spalte1]],\">1,5\")"),
        (De, "WENNFEHLER(1/0;#WERT!)", "IFERROR(1/0,#VALUE!)"),
        (PtBr, "SE(VERDADEIRO;SOMA(1,5;2);#N/D)", "IF(TRUE,SUM(1.5,2),#N/A)"),
        (PtBr, "PROCV(A2;Tabela1[#Tudo];2;FALSO)", "VLOOKUP(A2,Tabela1[#All],2,FALSE)"),
        (PtBr, "SOMA({1\\2;3\\4})", "SUM({1,2;3,4})"),
        (Ru, "СУММ({1;2:3;4})", "SUM({1,2;3,4})"),
        (Ru, "СУММ(Таблица1[[#Итого];[Сумма]])", "SUM(Таблица1[[#Totals],[Сумма]])"),
        (Es, "CONTAR.SI.CONJUNTO(A1:A3;\">1\")", "COUNTIFS(A1:A3,\">1\")"),
        (Es, "SI.ERROR(1/0;#¡VALOR!)", "IFERROR(1/0,#VALUE!)"),
    ] {
        assert_eq!(canonical(local, locale), english, "{locale:?} entry");
        assert_eq!(shown(english, locale), local, "{locale:?} display");
    }
}

#[test]
fn every_language_round_trips_canonical_formulas() {
    for formula in [
        "SUM(1.5,-2.25,A1:B2 B2:C3)",
        r#"IF(AND(TRUE,NOT(FALSE)),"a,b;c",#DIV/0!)"#,
        r#"{1.5,TRUE;#N/A,"x;y"}"#,
        "SUM((A1,B1),Sheet2!A:A)",
        "Table1[[#This Row],[a]]+SUM(Table1[#Totals])+Table1[[#Headers],[a]:[b]]",
        "LET(x,1,LAMBDA(y,x+y)(2))",
        "XLOOKUP(1,A:A,B:B,#N/A)+IFNA(#GETTING_DATA,#SPILL!)",
        "SUM(1:3)+ROWS($2:$2)",
        "CEILING.MATH(2.5)+T.TEST(A1:A3,B1:B3,2,1)",
        "BYROW(A1:B2,SUM)+GROUPBY(A1:A3,B1:B3,AVERAGE)",
    ] {
        let expected = print(&parse(formula).unwrap());
        for locale in FormulaLocale::ALL {
            let local = shown(formula, locale);
            assert_eq!(canonical(&local, locale), expected, "{locale:?}: {formula} shown as {local}");
        }
    }
}

#[test]
fn english_names_and_any_letter_case_are_accepted() {
    assert_eq!(canonical("SUM(1;2)+SUMME(3)", De), "SUM(1,2)+SUM(3)");
    assert_eq!(canonical("zählenwenn(A1:A3;1)", De), "COUNTIF(A1:A3,1)");
    assert_eq!(canonical("сумм(1;2)+#н/д", Ru), "SUM(1,2)+#N/A");
    assert_eq!(canonical("{истина:ложь}", Ru), "{TRUE;FALSE}");
    // A local name that is another function's English name means the local function, as in
    // Spanish Excel: DECIMAL is FIXED there, and the base conversion is CONV.DECIMAL.
    assert_eq!(canonical("DECIMAL(1,5;1)", Es), "FIXED(1.5,1)");
    assert_eq!(canonical("CONV.DECIMAL(\"FF\";16)", Es), "DECIMAL(\"FF\",16)");
    // English separators are not the language's syntax.
    assert!(parse_input("SUMME(1.5;2)", De, |_| false).is_err());
    assert!(parse_input("SUM(1,2)", Ru, |_| false).is_ok_and(|e| print(&e) == "SUM(1.2)"));
}

#[test]
fn names_shadow_local_function_names_in_any_script() {
    assert_eq!(canonical("LET(zählenwenn;LAMBDA(x;x*2);ZÄHLENWENN(3))", De), "LET(zählenwenn,LAMBDA(x,x*2),ZÄHLENWENN(3))");
    let known = |name: &str| name == "СУММ";
    assert_eq!(print(&parse_input("СУММ(1)", Ru, known).unwrap()), "СУММ(1)");
    assert_eq!(print_input(&parse("SUM(1)").unwrap(), Ru, known).unwrap(), "SUM(1)");
}

#[test]
fn functions_passed_by_name_are_spelled_in_the_language() {
    // GROUPBY(…,SUM) and BYROW(…,SUM) take a worksheet function by name, saved as `_xleta.SUM`.
    assert_eq!(canonical("NACHZEILE(A1:B3;SUMME)", De), "BYROW(A1:B3,SUM)");
    assert_eq!(canonical("AGRUPARPOR(A2:A9;B2:B9;soma)", PtBr), "GROUPBY(A2:A9,B2:B9,SUM)");
    assert_eq!(shown("_xlfn.GROUPBY(A2:A9,B2:B9,_xleta.SUM)", PtBr), "AGRUPARPOR(A2:A9;B2:B9;SOMA)");
    // A defined name with the local spelling is still the name.
    let known = |name: &str| name == "SUMME";
    assert_eq!(print(&parse_input("NACHZEILE(A1:B3;SUMME)", De, known).unwrap()), "BYROW(A1:B3,SUMME)");
    assert_eq!(print_input(&parse("BYROW(A1:B3,SUM)").unwrap(), De, known).unwrap(), "NACHZEILE(A1:B3;SUM)");
}

#[test]
fn language_tables_are_sorted_unique_and_unambiguous() {
    for locale in LOCALIZED {
        let table = locale.translations();
        assert!(table.len() > 400, "{locale:?}: {} names", table.len());
        assert!(table.windows(2).all(|w| w[0].0 < w[1].0), "{locale:?}: not sorted by canonical name");
        let mut locals: Vec<String> = table.iter().map(|(_, local)| local.to_uppercase()).collect();
        locals.sort();
        let all = locals.len();
        locals.dedup();
        assert_eq!(locals.len(), all, "{locale:?}: two functions share a local name");
        for (en, local) in table {
            assert_ne!(en, local, "{locale:?}: {en} is listed though it keeps its English name");
            assert_eq!(locale.function_name(en), *local);
            assert_eq!(locale.canonical_name(local), *en);
            // The TRUE() and FALSE() functions are spelled like the booleans; nothing else may be.
            let upper = local.to_uppercase();
            let boolean = upper == locale.boolean_name(true) || upper == locale.boolean_name(false);
            assert_eq!(boolean, matches!(*en, "TRUE" | "FALSE"), "{locale:?}: {en} = {local}");
        }
        for e in CellError::ALL {
            assert_eq!(canonical(locale.error_name(e), locale), e.as_str(), "{locale:?}");
        }
    }
}

#[test]
fn malformed_localized_input_is_an_error() {
    for (locale, bad) in [
        (De, "SUMME({1.}"),
        (De, "Tabelle1[[#Kopfzeilen];"),
        (De, "#WERT"),
        (Ru, "{ИСТИНА:"),
        (Ru, "СУММ(1:"),
        (PtBr, "SOMA(1,"),
        (PtBr, "{SOMA(1)}"),
        (Es, "#¡VALOR"),
    ] {
        assert!(parse_input(bad, locale, |_| false).is_err(), "{locale:?}: {bad}");
    }
    for locale in LOCALIZED {
        let chain = format!("{}+", locale.boolean_name(true)).repeat(300) + "1";
        assert!(parse_input(&chain, locale, |_| false).is_err(), "{locale:?}");
    }
}
