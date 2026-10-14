//! UI formula syntax boundary. Engine commands, control/API callers and XLSX keep English.
//!
//! Like Excel, the words of a formula (function names, booleans, errors, table specifiers) follow
//! the interface language and its separators follow the regional format.
use gridcraft_engine::formula::{self, FormulaLocale, ParseError, Separators};
use gridcraft_engine::model::Workbook;

/// The separators formulas are typed and shown with: the language's array separators, the
/// workbook region's list and decimal separators.
pub(crate) fn separators(locale: FormulaLocale, wb: &Workbook) -> Separators {
    Separators::new(locale, wb.region)
}

pub(crate) fn canonical(
    text: &str,
    locale: FormulaLocale,
    separators: Separators,
    wb: &Workbook,
    sheet: usize,
    cell: gridcraft_engine::core::CellRef,
) -> Result<String, ParseError> {
    // The editor hid the `_xludf.` prefixes of this cell's formula; confirming it unchanged keeps
    // the user-defined calls instead of binding them to the built-ins of the same name.
    if let Some(stored) = wb.sheet(sheet).and_then(|sh| sh.cell(cell)).filter(|c| c.formula.is_some()).map(|c| c.input_text())
        && has_udf_prefix(&stored)
        && text == display(&stored, locale, wb, sheet)
    {
        return Ok(stored);
    }
    if (locale == FormulaLocale::En && separators == Separators::EN_US)
        || wb.sheet(sheet).is_some_and(|sh| wb.styles.get(sh.style_id(cell)).num_fmt.as_str() == "@")
    {
        return Ok(text.to_string());
    }
    // The engine's own formula detection (no trimming: " =1" is text), which also lets a number
    // the region reads, such as `-R$ 5` in pt-BR, through as a constant.
    let body = text;
    if gridcraft_engine::cmd::edit::is_formula_input(body, wb.date_system, wb.region) {
        let known = |name: &str| wb.name(name, sheet).is_some() || wb.table(name).is_some();
        // Match the engine's existing convenience for missing closing parentheses.
        let mut body = body.to_string();
        let mut expr = formula::parse_input(&body, locale, separators, known);
        for _ in 0..8 {
            if expr.is_ok() {
                break;
            }
            body.push(')');
            expr = formula::parse_input(&body, locale, separators, known);
        }
        return Ok(format!("={}", formula::print(&expr?)));
    }
    // Typed constants (numbers, dates, text) pass through: the engine reads them in the workbook's
    // region (Options › Regional format), like the formula separators.
    Ok(text.to_string())
}

pub(crate) fn display(text: &str, locale: FormulaLocale, wb: &Workbook, sheet: usize) -> String {
    let separators = separators(locale, wb);
    // English words with canonical separators show the stored text, `_xludf.` prefixes aside,
    // and skip a parse per frame.
    if locale == FormulaLocale::En && separators == Separators::EN_US {
        return hide_udf_prefix(text, locale, separators);
    }
    let Some(body) = text.strip_prefix('=') else { return text.to_string() };
    let shown = formula::parse(body)
        .and_then(|expr| formula::print_input(&expr, locale, separators, |name| wb.name(name, sheet).is_some() || wb.table(name).is_some()))
        .map(|body| format!("={body}"))
        .unwrap_or_else(|_| text.to_string());
    hide_udf_prefix(&shown, locale, separators)
}

const UDF_PREFIX: &str = "_xludf.";

fn has_udf_prefix(text: &str) -> bool {
    text.to_ascii_lowercase().contains(UDF_PREFIX)
}

/// Excel shows a user-defined call such as `_xludf.EOMONTH()` as `=EOMONTH()`. Only function
/// names lose the prefix: text literals and names keep it.
fn hide_udf_prefix(text: &str, locale: FormulaLocale, separators: Separators) -> String {
    let Some(body) = text.strip_prefix('=').filter(|b| has_udf_prefix(b)) else { return text.to_string() };
    let Ok(tokens) = formula::lexer::tokenize_locale(body, locale, separators) else { return text.to_string() };
    let mut out = String::from("=");
    let mut from = 0;
    for t in tokens {
        if matches!(t.tok, formula::lexer::Tok::Func(_))
            && body.get(t.start..t.start + UDF_PREFIX.len()).is_some_and(|p| p.eq_ignore_ascii_case(UDF_PREFIX))
        {
            out.push_str(body.get(from..t.start).unwrap_or(""));
            from = t.start + UDF_PREFIX.len();
        }
    }
    out.push_str(body.get(from..).unwrap_or(""));
    out
}

// Descriptions/signatures for common Spanish functions; other functions keep their English help
// under their local name. The `;` between parameters becomes the region's list separator.
const SPANISH_HELP: &[(&str, &str, &str)] = &[
    ("SUM", "SUMA(número1; [número2]; ...)", "Suma los números."),
    ("AVERAGE", "PROMEDIO(número1; [número2]; ...)", "Devuelve el promedio de los números."),
    ("IF", "SI(prueba_lógica; valor_si_verdadero; [valor_si_falso])", "Devuelve un valor según una condición."),
    ("COUNT", "CONTAR(valor1; [valor2]; ...)", "Cuenta las celdas que contienen números."),
    ("SUMIF", "SUMAR.SI(rango; criterio; [rango_suma])", "Suma las celdas que cumplen un criterio."),
    ("COUNTIF", "CONTAR.SI(rango; criterio)", "Cuenta las celdas que cumplen un criterio."),
    ("VLOOKUP", "BUSCARV(valor_buscado; matriz; indicador_columnas; [ordenado])", "Busca un valor en la primera columna de una tabla."),
    ("TODAY", "HOY()", "Devuelve la fecha actual."),
    ("TRUE", "VERDADERO()", "Devuelve el valor lógico VERDADERO."),
    ("FALSE", "FALSO()", "Devuelve el valor lógico FALSO."),
];

pub(crate) fn signature(name: &str, locale: FormulaLocale, separators: Separators) -> Option<String> {
    let canonical = locale.canonical_name(name);
    if locale == FormulaLocale::Es
        && let Some((_, signature, _)) = SPANISH_HELP.iter().find(|(name, ..)| *name == canonical)
    {
        return Some(signature.replace(';', &separators.list.to_string()));
    }
    let signature = crate::formula_bar::function_signature(canonical)?.replace(',', &separators.list.to_string());
    Some(match signature.strip_prefix(canonical) {
        Some(params) if locale != FormulaLocale::En => format!("{}{params}", locale.function_name(canonical)),
        _ => signature,
    })
}

pub(crate) fn description(name: &str, locale: FormulaLocale) -> Option<String> {
    let canonical = locale.canonical_name(name);
    if locale == FormulaLocale::Es
        && let Some((_, _, description)) = SPANISH_HELP.iter().find(|(name, ..)| *name == canonical)
    {
        return Some((*description).into());
    }
    crate::formula_bar::function_description(canonical)
}

/// Avoid suggesting an alias that would call a workbook/table name instead of the function.
pub(crate) fn completion_name<'a>(name: &'a str, locale: FormulaLocale, wb: &Workbook, sheet: usize) -> &'a str {
    let alias = locale.function_name(name);
    if wb.name(alias, sheet).is_some() || wb.table(alias).is_some() { name } else { alias }
}

/// Engine-generated edit templates use canonical syntax. The only incomplete template is
/// the bare function opening emitted by Insert Function; typed user input never enters here.
pub(crate) fn display_template(text: &str, locale: FormulaLocale, wb: &Workbook, sheet: usize) -> String {
    if let Some(name) = text.strip_prefix('=').and_then(|s| s.strip_suffix('('))
        && !name.is_empty()
        && name.chars().all(|c| c.is_alphanumeric() || matches!(c, '_' | '.'))
    {
        return format!("={}(", completion_name(name, locale, wb, sheet));
    }
    display(text, locale, wb, sheet)
}
