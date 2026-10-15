//! [`Expr`] → formula text (without `=`), A1 or R1C1.

use std::fmt::Write;

use gridcraft_core::{CellRef, number_to_text};

use crate::ast::*;

/// Whether a sheet name needs quotes in a reference.
pub fn needs_quotes(name: &str) -> bool {
    if name.is_empty() {
        return true;
    }
    let first = name.chars().next().unwrap_or('_');
    if first.is_ascii_digit() || name.chars().any(|c| !(c.is_alphanumeric() || c == '_' || c == '.')) {
        return true;
    }
    // Looks like a cell ref (`A1`, `R1C1`) or a boolean.
    gridcraft_core::CellRef::parse(name).is_some() || name.eq_ignore_ascii_case("TRUE") || name.eq_ignore_ascii_case("FALSE")
}

pub fn quote_sheet(name: &str) -> String {
    if needs_quotes(name) { format!("'{}'", name.replace('\'', "''")) } else { name.to_string() }
}

fn sheet_prefix(s: &SheetSel) -> String {
    match s {
        SheetSel::Current => String::new(),
        SheetSel::Named(n) => format!("{}!", quote_sheet(n)),
        SheetSel::Span(a, b) => {
            let joined = format!("{a}:{b}");
            if needs_quotes(a) || needs_quotes(b) { format!("'{}'!", joined.replace('\'', "''")) } else { format!("{joined}!") }
        }
    }
}

/// Column letters (`A`, `XFD`) written straight into `s`, without a temporary string: this runs
/// for every reference of every formula printed (saving, copying, loading).
fn write_col(s: &mut String, col: u32) {
    let mut letters = [0u8; 8];
    let mut n = u64::from(col) + 1;
    let mut i = letters.len();
    while n > 0 && i > 0 {
        i -= 1;
        if let Some(slot) = letters.get_mut(i) {
            *slot = b'A' + ((n - 1) % 26) as u8;
        }
        n = (n - 1) / 26;
    }
    for &b in letters.get(i..).unwrap_or(&[]) {
        s.push(char::from(b));
    }
}

fn write_anchor_a1(s: &mut String, a: &Anchor) {
    if a.col_abs {
        s.push('$');
    }
    write_col(s, a.col);
    if a.row_abs {
        s.push('$');
    }
    let _ = write!(s, "{}", u64::from(a.row) + 1);
}

/// A1 text written into `s`, with `sep` between the two ends of a range (`:` or a trim
/// operator).
fn write_range_a1(s: &mut String, r: &Reference, sep: &str) {
    if !matches!(r.sheet, SheetSel::Current) {
        s.push_str(&sheet_prefix(&r.sheet));
    }
    let dollar = |s: &mut String, abs: bool| {
        if abs {
            s.push('$');
        }
    };
    match &r.kind {
        RefKind::Cell(a) => write_anchor_a1(s, a),
        RefKind::Range(a, b) => {
            write_anchor_a1(s, a);
            s.push_str(sep);
            write_anchor_a1(s, b);
        }
        RefKind::Rows(r0, a0, r1, a1) => {
            dollar(s, *a0);
            let _ = write!(s, "{}{sep}", u64::from(*r0) + 1);
            dollar(s, *a1);
            let _ = write!(s, "{}", u64::from(*r1) + 1);
        }
        RefKind::Cols(c0, a0, c1, a1) => {
            dollar(s, *a0);
            write_col(s, *c0);
            s.push_str(sep);
            dollar(s, *a1);
            write_col(s, *c1);
        }
    }
}

pub fn reference_a1(r: &Reference) -> String {
    range_a1(r, ":")
}

/// A1 text with `sep` between the two ends of a range (`:` or a trim operator).
fn range_a1(r: &Reference, sep: &str) -> String {
    let mut s = String::new();
    write_range_a1(&mut s, r, sep);
    s
}

fn r1c1_part(letter: char, v: u32, abs: bool, base: u32) -> String {
    if abs {
        format!("{letter}{}", v + 1)
    } else {
        let d = v as i64 - base as i64;
        if d == 0 { letter.to_string() } else { format!("{letter}[{d}]") }
    }
}

fn anchor_r1c1(a: &Anchor, at: CellRef) -> String {
    format!("{}{}", r1c1_part('R', a.row, a.row_abs, at.row), r1c1_part('C', a.col, a.col_abs, at.col))
}

pub fn reference_r1c1(r: &Reference, at: CellRef) -> String {
    range_r1c1(r, at, ":")
}

/// R1C1 text with `sep` between the two ends of a range; a trim operator never collapses
/// `R1:R1` to `R1`.
fn range_r1c1(r: &Reference, at: CellRef, sep: &str) -> String {
    let body = match &r.kind {
        RefKind::Cell(a) => anchor_r1c1(a, at),
        RefKind::Range(a, b) => format!("{}{sep}{}", anchor_r1c1(a, at), anchor_r1c1(b, at)),
        RefKind::Rows(r0, a0, r1, a1) => {
            let (x, y) = (r1c1_part('R', *r0, *a0, at.row), r1c1_part('R', *r1, *a1, at.row));
            if x == y && sep == ":" { x } else { format!("{x}{sep}{y}") }
        }
        RefKind::Cols(c0, a0, c1, a1) => {
            let (x, y) = (r1c1_part('C', *c0, *a0, at.col), r1c1_part('C', *c1, *a1, at.col));
            if x == y && sep == ":" { x } else { format!("{x}{sep}{y}") }
        }
    };
    format!("{}{}", sheet_prefix(&r.sheet), body)
}

pub fn struct_ref(s: &StructRef) -> String {
    write_struct_ref(s, true)
}

/// Structured reference as files spell it: `[@Col]` is only Excel's display shorthand, files
/// hold `[[#This Row],[Col]]` (and `[#This Row]` for `[@]`).
pub fn struct_ref_file(s: &StructRef) -> String {
    write_struct_ref(s, false)
}

fn write_struct_ref(s: &StructRef, at_shorthand: bool) -> String {
    let esc = |c: &str| c.replace('\'', "''").replace('[', "'[").replace(']', "']").replace('#', "'#");
    let item = |i: &StructItem| match i {
        StructItem::All => "#All",
        StructItem::Data => "#Data",
        StructItem::Headers => "#Headers",
        StructItem::Totals => "#Totals",
        StructItem::ThisRow => "#This Row",
    };
    let this_row = at_shorthand && s.specifiers == [StructItem::ThisRow];
    let mut out = s.table.clone();
    let simple_col = |c: &str| !c.chars().any(|ch| matches!(ch, ' ' | '[' | ']' | '#' | '\'' | ',' | ':'));
    match (&s.col_start, &s.col_end) {
        (None, None) if s.specifiers.is_empty() => out.push_str("[]"),
        (None, None) if this_row => out.push_str("[@]"),
        (None, None) if s.specifiers.len() == 1 => {
            let _ = write!(out, "[{}]", s.specifiers.first().map(item).unwrap_or(""));
        }
        (Some(c), None) if s.specifiers.is_empty() => {
            // Excel writes a name with spaces at its edges in the double-bracket form.
            if c.trim() == c {
                let _ = write!(out, "[{}]", esc(c));
            } else {
                let _ = write!(out, "[[{}]]", esc(c));
            }
        }
        (Some(c), None) if this_row => {
            if simple_col(c) {
                let _ = write!(out, "[@{}]", esc(c));
            } else {
                let _ = write!(out, "[@[{}]]", esc(c));
            }
        }
        _ => {
            let mut parts: Vec<String> = s.specifiers.iter().map(|i| format!("[{}]", item(i))).collect();
            match (&s.col_start, &s.col_end) {
                (Some(a), Some(b)) => parts.push(format!("[{}]:[{}]", esc(a), esc(b))),
                (Some(a), None) => parts.push(format!("[{}]", esc(a))),
                _ => {}
            }
            let _ = write!(out, "[{}]", parts.join(","));
        }
    }
    out
}

fn number(n: f64) -> String {
    number_to_text(n)
}

/// How to print: A1, or R1C1 relative to a cell; with the relative parts of references moved
/// by a shift (a formula copied that far, see [`print_shifted`]).
#[derive(Clone, Copy)]
struct Print {
    r1c1: Option<CellRef>,
    shift: (i64, i64),
}

impl Print {
    /// `r` moved by the shift, `None` when that pushes it off the sheet.
    fn moved<'a>(&self, r: &'a Reference) -> Option<std::borrow::Cow<'a, Reference>> {
        if self.shift == (0, 0) {
            return Some(std::borrow::Cow::Borrowed(r));
        }
        crate::adjust::shift_ref(r, self.shift.0, self.shift.1).map(std::borrow::Cow::Owned)
    }
}

/// Prints an expression in A1 notation.
pub fn print(e: &Expr) -> String {
    let mut s = String::new();
    write_expr(&mut s, e, Print { r1c1: None, shift: (0, 0) });
    s
}

/// Prints an expression in R1C1 notation relative to `at`.
pub fn print_r1c1(e: &Expr, at: CellRef) -> String {
    let mut s = String::new();
    write_expr(&mut s, e, Print { r1c1: Some(at), shift: (0, 0) });
    s
}

/// Prints `e` copied `dr` rows and `dc` columns away, in A1 notation: the same text as
/// `print(&shift_relative(e, dr, dc))` (references pushed off the sheet print `#REF!`), without
/// building the moved expression.
pub fn print_shifted(e: &Expr, dr: i64, dc: i64) -> String {
    let mut s = String::new();
    write_expr(&mut s, e, Print { r1c1: None, shift: (dr, dc) });
    s
}

fn write_expr(s: &mut String, e: &Expr, p: Print) {
    match e {
        Expr::Number(n) => s.push_str(&number(*n)),
        Expr::Text(t) => {
            s.push('"');
            s.push_str(&t.replace('"', "\"\""));
            s.push('"');
        }
        Expr::Bool(b) => s.push_str(if *b { "TRUE" } else { "FALSE" }),
        Expr::Error(err) => s.push_str(err.as_str()),
        Expr::Array(rows) => {
            s.push('{');
            for (ri, row) in rows.iter().enumerate() {
                if ri > 0 {
                    s.push(';');
                }
                for (ci, el) in row.iter().enumerate() {
                    if ci > 0 {
                        s.push(',');
                    }
                    write_expr(s, el, p);
                }
            }
            s.push('}');
        }
        Expr::Ref(r) => {
            let Some(r) = p.moved(r) else {
                s.push_str(gridcraft_core::CellError::Ref.as_str());
                return;
            };
            match p.r1c1 {
                Some(at) => s.push_str(&reference_r1c1(&r, at)),
                None => write_range_a1(s, &r, ":"),
            }
        }
        Expr::Name(n) => match n.rsplit_once('!') {
            // A sheet-qualified name is stored with the bare sheet name (`My Sheet!Total`): quote it so it parses back.
            Some((sheet, name)) => {
                s.push_str(&quote_sheet(sheet));
                s.push('!');
                s.push_str(name);
            }
            _ => s.push_str(n),
        },
        Expr::Struct(st) => s.push_str(&struct_ref(st)),
        Expr::Unary(op, x) => match op {
            UnOp::Neg => {
                s.push('-');
                write_expr(s, x, p);
            }
            UnOp::Plus => {
                s.push('+');
                write_expr(s, x, p);
            }
            UnOp::At => {
                s.push('@');
                write_expr(s, x, p);
            }
            UnOp::Percent => {
                write_expr(s, x, p);
                s.push('%');
            }
            UnOp::Spill => {
                write_expr(s, x, p);
                // A reference that became #REF! (its sheet or cell was deleted, or a copy moved it
                // off the sheet) stays `#REF!`.
                let off_sheet = p.shift != (0, 0) && matches!(&**x, Expr::Ref(r) if crate::adjust::shift_ref(r, p.shift.0, p.shift.1).is_none());
                if !matches!(**x, Expr::Error(_)) && !off_sheet {
                    s.push('#');
                }
            }
            UnOp::Trim(mode) => match &**x {
                Expr::Ref(r) if !matches!(r.kind, RefKind::Cell(_)) => match p.moved(r) {
                    // Copied off the sheet: `#REF!`, as `shift_relative` leaves it.
                    None => s.push_str(gridcraft_core::CellError::Ref.as_str()),
                    Some(r) => match p.r1c1 {
                        Some(at) => s.push_str(&range_r1c1(&r, at, mode.symbol())),
                        None => write_range_a1(s, &r, mode.symbol()),
                    },
                },
                Expr::Binary(BinOp::Range, a, b) => {
                    write_expr(s, a, p);
                    s.push_str(mode.symbol());
                    write_expr(s, b, p);
                }
                // A reference that became #REF! stays `#REF!`.
                Expr::Error(_) => write_expr(s, x, p),
                // No operator form (e.g. a range shrunk to one cell): the equivalent call.
                other => {
                    s.push_str("TRIMRANGE(");
                    write_expr(s, other, p);
                    let _ = write!(s, ",{0},{0})", mode.code());
                }
            },
        },
        Expr::Binary(..) => {
            // Walk the left spine iteratively: a flat chain such as `1+1+1+...` parses into a
            // tree one level deeper per operator, and recursing on it could exhaust the stack.
            let mut rest: Vec<(&BinOp, &Expr)> = Vec::new();
            let mut first = e;
            while let Expr::Binary(op, a, b) = first {
                rest.push((op, b));
                first = a;
            }
            write_expr(s, first, p);
            for (op, right) in rest.into_iter().rev() {
                s.push_str(op.symbol());
                write_expr(s, right, p);
            }
        }
        Expr::Call(name, args) => {
            s.push_str(name);
            s.push('(');
            for (i, a) in args.iter().enumerate() {
                if i > 0 {
                    s.push(',');
                }
                write_expr(s, a, p);
            }
            s.push(')');
        }
        Expr::Invoke(c, args) => {
            write_expr(s, c, p);
            s.push('(');
            for (i, a) in args.iter().enumerate() {
                if i > 0 {
                    s.push(',');
                }
                write_expr(s, a, p);
            }
            s.push(')');
        }
        Expr::Missing => {}
        Expr::Paren(x) => {
            s.push('(');
            write_expr(s, x, p);
            s.push(')');
        }
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn column_letters_match_core() {
        for col in 0..gridcraft_core::MAX_COLS {
            let mut s = String::new();
            super::write_col(&mut s, col);
            assert_eq!(s, gridcraft_core::col_to_letters(col), "{col}");
        }
    }
}
