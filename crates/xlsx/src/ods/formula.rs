//! OpenFormula (ODF 1.3 part 4) ⇄ GridCraft (Excel A1) formula text.
//! Format reference: <https://docs.oasis-open.org/office/OpenDocument/v1.3/os/part4-formula/OpenDocument-v1.3-os-part4-formula.html>.
//!
//! OpenFormula puts references in brackets (`[.A1]`, `[$Sheet2.A1:.B5]`), separates arguments
//! with `;`, writes union as `~` and intersection as `!`, and array rows with `|`. Formulas in
//! the `msoxl:` namespace are already in Excel syntax.

use gridcraft_core::{col_to_letters, number_to_text};
use gridcraft_formula::{Anchor, BinOp, Expr, RefKind, Reference, SheetSel, UnOp, quote_sheet};

/// Formulas longer than this are not translated (Excel's own limit is 8192 characters).
const MAX_FORMULA: usize = 64 * 1024;

/// Prefixes the ODF function namespace adds to application-specific function names.
const FUNCTION_PREFIXES: [&str; 4] = ["COM.MICROSOFT.", "ORG.OPENOFFICE.", "ORG.LIBREOFFICE.", "_XLFN."];

/// Excel functions that OpenFormula has no name for: ODF writers spell them with the
/// `COM.MICROSOFT.` prefix.
const MICROSOFT_FUNCTIONS: [&str; 15] = [
    "CONCAT",
    "TEXTJOIN",
    "IFS",
    "SWITCH",
    "MAXIFS",
    "MINIFS",
    "XLOOKUP",
    "XMATCH",
    "FILTER",
    "SORT",
    "SORTBY",
    "UNIQUE",
    "SEQUENCE",
    "RANDARRAY",
    "LET",
];

/// Converts a `table:formula` value (`of:=SUM([.A1:.B2])`) to formula text without `=`.
/// `None` when the namespace or syntax isn't understood.
pub(super) fn from_odf(src: &str) -> Option<String> {
    if src.len() > MAX_FORMULA {
        return None;
    }
    let (ns, body) = match src.split_once(':') {
        Some((prefix, rest)) if !prefix.is_empty() && prefix.bytes().all(|b| b.is_ascii_alphanumeric()) && rest.starts_with('=') => (prefix, rest),
        _ => ("of", src),
    };
    let body = body.strip_prefix('=').unwrap_or(body);
    match ns {
        "msoxl" => return Some(body.to_string()),
        "of" | "oooc" => {}
        _ => return None,
    }
    let chars: Vec<char> = body.chars().collect();
    let mut out = String::with_capacity(body.len());
    let mut i = 0usize;
    let mut braces = 0usize;
    while let Some(&c) = chars.get(i) {
        i += 1;
        match c {
            '"' => {
                out.push('"');
                loop {
                    match chars.get(i) {
                        None => return None,
                        Some('"') if chars.get(i + 1) == Some(&'"') => {
                            out.push_str("\"\"");
                            i += 2;
                        }
                        Some('"') => {
                            out.push('"');
                            i += 1;
                            break;
                        }
                        Some(&ch) => {
                            out.push(ch);
                            i += 1;
                        }
                    }
                }
            }
            '[' => {
                let start = i;
                let mut quoted = false;
                loop {
                    match chars.get(i) {
                        None => return None,
                        Some('\'') => quoted = !quoted,
                        Some(']') if !quoted => break,
                        _ => {}
                    }
                    i += 1;
                }
                let inner: String = chars.get(start..i)?.iter().collect();
                i += 1;
                out.push_str(&reference(&inner)?);
            }
            '{' => {
                braces += 1;
                out.push('{');
            }
            '}' => {
                braces = braces.checked_sub(1)?;
                out.push('}');
            }
            ';' => out.push(','),
            '|' if braces > 0 => out.push(';'),
            '~' => out.push(','),
            '!' => out.push(' '),
            c if c.is_whitespace() => {}
            c if c.is_alphabetic() || c == '_' => {
                let start = i - 1;
                while chars.get(i).is_some_and(|ch| ch.is_alphanumeric() || matches!(ch, '_' | '.')) {
                    i += 1;
                }
                let word: String = chars.get(start..i)?.iter().collect();
                // Whitespace may separate a function name from its parenthesis.
                let mut next = i;
                while chars.get(next).is_some_and(|ch| ch.is_whitespace()) {
                    next += 1;
                }
                if chars.get(next) == Some(&'(') {
                    let upper = word.to_ascii_uppercase();
                    let name = FUNCTION_PREFIXES.iter().find_map(|p| upper.strip_prefix(p)).unwrap_or(&upper).to_string();
                    let mut close = next + 1;
                    while chars.get(close).is_some_and(|ch| ch.is_whitespace()) {
                        close += 1;
                    }
                    // OpenFormula's TRUE() and FALSE() are Excel's TRUE and FALSE.
                    if matches!(name.as_str(), "TRUE" | "FALSE") && chars.get(close) == Some(&')') {
                        out.push_str(&name);
                        i = close + 1;
                    } else {
                        out.push_str(&name);
                        i = next;
                    }
                } else {
                    out.push_str(&word);
                }
            }
            c => out.push(c),
        }
    }
    (braces == 0).then_some(out)
}

/// The inside of an OpenFormula reference (`.A1`, `$Sheet1.$A$1:.B2`) as A1 text.
fn reference(inner: &str) -> Option<String> {
    let mut parts = Vec::new();
    let mut quoted = false;
    let mut start = 0usize;
    for (i, ch) in inner.char_indices() {
        match ch {
            '\'' => quoted = !quoted,
            ':' if !quoted => {
                parts.push(inner.get(start..i)?);
                start = i + 1;
            }
            _ => {}
        }
    }
    parts.push(inner.get(start..)?);
    if parts.len() > 2 || inner.contains("#REF!") {
        return (inner.contains("#REF!")).then(|| "#REF!".to_string());
    }
    let first = ref_part(parts.first()?)?;
    let second = match parts.get(1) {
        Some(p) => Some(ref_part(p)?),
        None => None,
    };
    let prefix = match (&first.0, second.as_ref().and_then(|s| s.0.as_ref())) {
        (Some(a), Some(b)) if a != b => {
            let span = format!("{a}:{b}");
            if quote_sheet(a) != *a || quote_sheet(b) != *b { format!("'{}'!", span.replace('\'', "''")) } else { format!("{span}!") }
        }
        (Some(a), _) => format!("{}!", quote_sheet(a)),
        (None, _) => String::new(),
    };
    Some(match second {
        Some((_, cell)) => format!("{prefix}{}:{cell}", first.1),
        None => format!("{prefix}{}", first.1),
    })
}

/// `$Sheet1.$A$1`, `'My sheet'.B2` or `.C3` → (sheet, cell).
fn ref_part(s: &str) -> Option<(Option<String>, String)> {
    let s = s.trim();
    let mut quoted = false;
    let mut dot = None;
    for (i, ch) in s.char_indices() {
        match ch {
            '\'' => quoted = !quoted,
            '.' if !quoted => dot = Some(i),
            _ => {}
        }
    }
    let (sheet, cell) = match dot {
        Some(d) => (s.get(..d)?, s.get(d + 1..)?),
        None => ("", s),
    };
    let sheet = sheet.strip_prefix('$').unwrap_or(sheet);
    let sheet = if sheet.is_empty() {
        None
    } else if let Some(q) = sheet.strip_prefix('\'') {
        // A quoted name must be the whole sheet part (an external `'file'#$Sheet` is not).
        Some(q.strip_suffix('\'')?.replace("''", "'"))
    } else if sheet.chars().all(|c| c.is_alphanumeric() || c == '_' || c == ' ') {
        Some(sheet.to_string())
    } else {
        return None;
    };
    let valid = !cell.is_empty() && cell.len() <= 16 && cell.chars().all(|c| c.is_ascii_alphanumeric() || c == '$');
    valid.then(|| (sheet, cell.to_ascii_uppercase()))
}

/// Formula text (without `=`) as a `table:formula` value: OpenFormula when every part has an
/// OpenFormula spelling, otherwise the Excel text in the `msoxl:` namespace.
pub(super) fn to_odf(text: &str) -> String {
    let Ok(expr) = gridcraft_formula::parse(text) else { return format!("msoxl:={text}") };
    let mut excel_only = false;
    expr.walk(&mut |e| {
        if matches!(e, Expr::Struct(_) | Expr::Invoke(..) | Expr::Unary(UnOp::At | UnOp::Spill | UnOp::Trim(_), _))
            || matches!(e, Expr::Name(n) if n.contains('!'))
        {
            excel_only = true;
        }
    });
    if excel_only {
        return format!("msoxl:={text}");
    }
    let mut out = String::from("of:=");
    write_expr(&mut out, &expr);
    out
}

fn odf_sheet(name: &str) -> String {
    if !name.is_empty() && name.chars().all(|c| c.is_alphanumeric() || c == '_') {
        name.to_string()
    } else {
        format!("'{}'", name.replace('\'', "''"))
    }
}

fn anchor(a: &Anchor) -> String {
    format!("{}{}{}{}", if a.col_abs { "$" } else { "" }, col_to_letters(a.col), if a.row_abs { "$" } else { "" }, a.row as u64 + 1)
}

fn odf_ref(r: &Reference) -> String {
    let (s1, s2) = match &r.sheet {
        SheetSel::Current => (String::new(), String::new()),
        SheetSel::Named(n) => (format!("${}", odf_sheet(n)), String::new()),
        SheetSel::Span(a, b) => (format!("${}", odf_sheet(a)), format!("${}", odf_sheet(b))),
    };
    let abs = |a: bool| if a { "$" } else { "" };
    match &r.kind {
        RefKind::Cell(a) => format!("[{s1}.{}]", anchor(a)),
        RefKind::Range(a, b) => format!("[{s1}.{}:{s2}.{}]", anchor(a), anchor(b)),
        RefKind::Rows(r0, a0, r1, a1) => format!("[{s1}.{}{}:{s2}.{}{}]", abs(*a0), r0 + 1, abs(*a1), r1 + 1),
        RefKind::Cols(c0, a0, c1, a1) => format!("[{s1}.{}{}:{s2}.{}{}]", abs(*a0), col_to_letters(*c0), abs(*a1), col_to_letters(*c1)),
    }
}

fn odf_op(op: &BinOp) -> &'static str {
    match op {
        BinOp::Union => "~",
        BinOp::Intersect => "!",
        other => other.symbol(),
    }
}

fn write_args(s: &mut String, args: &[Expr]) {
    s.push('(');
    for (i, a) in args.iter().enumerate() {
        if i > 0 {
            s.push(';');
        }
        write_expr(s, a);
    }
    s.push(')');
}

fn write_expr(s: &mut String, e: &Expr) {
    match e {
        Expr::Number(n) => s.push_str(&number_to_text(*n)),
        Expr::Text(t) => {
            s.push('"');
            s.push_str(&t.replace('"', "\"\""));
            s.push('"');
        }
        Expr::Bool(b) => s.push_str(if *b { "TRUE()" } else { "FALSE()" }),
        Expr::Error(err) => s.push_str(err.as_str()),
        Expr::Array(rows) => {
            s.push('{');
            for (ri, row) in rows.iter().enumerate() {
                if ri > 0 {
                    s.push('|');
                }
                for (ci, el) in row.iter().enumerate() {
                    if ci > 0 {
                        s.push(';');
                    }
                    write_expr(s, el);
                }
            }
            s.push('}');
        }
        Expr::Ref(r) => s.push_str(&odf_ref(r)),
        Expr::Name(n) => s.push_str(n),
        Expr::Unary(op, x) => match op {
            UnOp::Neg => {
                s.push('-');
                write_expr(s, x);
            }
            UnOp::Percent => {
                write_expr(s, x);
                s.push('%');
            }
            // Plus, and the Excel-only operators (excluded by `to_odf`), keep only the operand.
            _ => {
                if *op == UnOp::Plus {
                    s.push('+');
                }
                write_expr(s, x);
            }
        },
        Expr::Binary(..) => {
            // Iterative over the left spine, as the A1 printer: long flat chains stay shallow.
            let mut rest: Vec<(&BinOp, &Expr)> = Vec::new();
            let mut first = e;
            while let Expr::Binary(op, a, b) = first {
                rest.push((op, b));
                first = a;
            }
            write_expr(s, first);
            for (op, right) in rest.into_iter().rev() {
                s.push_str(odf_op(op));
                write_expr(s, right);
            }
        }
        Expr::Call(name, args) => {
            if MICROSOFT_FUNCTIONS.contains(&name.as_str()) {
                s.push_str("COM.MICROSOFT.");
            }
            s.push_str(name);
            write_args(s, args);
        }
        Expr::Invoke(c, args) => {
            write_expr(s, c);
            write_args(s, args);
        }
        Expr::Struct(_) | Expr::Missing => {}
        Expr::Paren(x) => {
            s.push('(');
            write_expr(s, x);
            s.push(')');
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn openformula_reads_as_excel_syntax() {
        let cases = [
            ("of:=SUM([.A1:.B2])", "SUM(A1:B2)"),
            ("of:=[$Sheet2.$A$1]*2", "Sheet2!$A$1*2"),
            ("of:=['My sheet'.A1:.B3]", "'My sheet'!A1:B3"),
            ("of:=IF([.A1]>0;\"a;b\";FALSE())", "IF(A1>0,\"a;b\",FALSE)"),
            ("of:={1;2|3;4}", "{1,2;3,4}"),
            ("of:=SUM([.A1]~[.B1])", "SUM(A1,B1)"),
            ("of:=[.A1:.C3]![.B2:.D4]", "A1:C3 B2:D4"),
            ("of:=COM.MICROSOFT.IFS([.A1]>1; 1)", "IFS(A1>1,1)"),
            ("of:=[.A:.A]", "A:A"),
            ("of:=[$Sheet1.A1:$Sheet3.B2]", "Sheet1:Sheet3!A1:B2"),
            ("of:=[.#REF!]+1", "#REF!+1"),
            ("msoxl:=Table1[Col]", "Table1[Col]"),
            ("=1+2", "1+2"),
        ];
        for (odf, excel) in cases {
            assert_eq!(from_odf(odf).as_deref(), Some(excel), "{odf}");
        }
        assert_eq!(from_odf("zz:=1"), None);
        assert_eq!(from_odf("of:=\"open"), None);
        assert_eq!(from_odf("of:=[.A1"), None);
        assert_eq!(from_odf("of:=['file:///x.ods'#$Sheet1.A1]"), None);
    }

    #[test]
    fn excel_syntax_writes_as_openformula_and_reads_back() {
        let cases = [
            ("SUM(A1:B2)", "of:=SUM([.A1:.B2])"),
            ("Sheet2!$A$1*2", "of:=[$Sheet2.$A$1]*2"),
            ("'My sheet'!A1:B3", "of:=[$'My sheet'.A1:.B3]"),
            ("IF(A1>0,\"x\",TRUE)", "of:=IF([.A1]>0;\"x\";TRUE())"),
            ("{1,2;3,4}", "of:={1;2|3;4}"),
            ("IFS(A1>1,1)", "of:=COM.MICROSOFT.IFS([.A1]>1;1)"),
            ("SUM(A:A)", "of:=SUM([.A:.A])"),
        ];
        for (excel, odf) in cases {
            assert_eq!(to_odf(excel), odf, "{excel}");
            let back = from_odf(odf).unwrap_or_default();
            assert_eq!(gridcraft_formula::parse(&back).ok(), gridcraft_formula::parse(excel).ok(), "{odf}");
        }
        assert_eq!(to_odf("Table1[Col]"), "msoxl:=Table1[Col]");
    }
}
