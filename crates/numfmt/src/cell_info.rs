//! What `CELL("format")`, `CELL("color")` and `CELL("parentheses")` report about a number format.
//!
//! The codes follow Microsoft's table for CELL (G, F2, ,0, C2, P0, S2, D1–D9, with `-` and `()`
//! appended), checked against Excel 365: fractions, text, elapsed times and date orders outside the
//! table are `G`; a date's code comes from the order of its first day, month and year parts.

use crate::NumberFormat;
use crate::parse::{DatePart, SecKind, Section, Tok};

/// [`NumberFormat::cell_info`]: the CELL function's view of a format.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CellFormatInfo {
    /// `CELL("format")`, e.g. `F2`, `,0`, `C0-`, `D4`, `F0()`.
    pub code: String,
    /// `CELL("color")`: the negative section has a colour.
    pub color: bool,
    /// `CELL("parentheses")`: positive numbers (the first section) show in parentheses.
    pub parentheses: bool,
}

impl NumberFormat {
    /// The CELL function's code and flags for this format.
    pub fn cell_info(&self) -> CellFormatInfo {
        let first = if self.is_general() { None } else { self.num_sections().first() };
        let color = self.num_sections().get(1).is_some_and(|s| s.color.is_some());
        let parentheses = first.is_some_and(|s| s.paren);
        let mut code = first.map_or_else(|| "G".to_string(), section_code);
        if color {
            code.push('-');
        }
        if parentheses {
            code.push_str("()");
        }
        CellFormatInfo { code, color, parentheses }
    }
}

fn section_code(s: &Section) -> String {
    match s.kind {
        SecKind::General | SecKind::Text => "G".into(),
        SecKind::Date => date_code(s).unwrap_or("G").into(),
        SecKind::Number if s.fraction => "G".into(),
        SecKind::Number => {
            let letter = if s.percent > 0 {
                "P"
            } else if s.exp {
                "S"
            } else if s.currency && s.toks.iter().any(|t| matches!(t, Tok::Lit(l) if l.contains('$'))) {
                "C"
            } else if s.thousands {
                ","
            } else {
                "F"
            };
            format!("{letter}{}", decimals(s))
        }
    }
}

/// Digit placeholders after the decimal point (before any exponent).
fn decimals(s: &Section) -> usize {
    s.toks
        .iter()
        .skip_while(|t| !matches!(t, Tok::Point))
        .take_while(|t| !matches!(t, Tok::Exp { .. }))
        .filter(|t| matches!(t, Tok::Digit(_)))
        .count()
}

/// D1–D9 from the order of the date parts; `None` for orders Excel reports as `G`.
fn date_code(s: &Section) -> Option<&'static str> {
    let parts = || s.toks.iter().filter_map(|t| if let Tok::Date(d) = t { Some(*d) } else { None });
    let calendar: Vec<char> = parts()
        .filter_map(|d| match d {
            DatePart::Day(_) => Some('d'),
            DatePart::Month(_) => Some('m'),
            DatePart::Year(_) => Some('y'),
            _ => None,
        })
        .collect();
    if !calendar.is_empty() {
        let first = |n: usize| calendar.get(..n).map(|p| p.iter().collect::<String>()).unwrap_or_default();
        return match (first(3).as_str(), first(2).as_str()) {
            ("dmy", _) => Some("D1"),
            ("mdy", _) => Some("D4"),
            (_, "dm") => Some("D2"),
            (_, "my") => Some("D3"),
            (_, "md") => Some("D5"),
            _ => None,
        };
    }
    if parts().any(|d| matches!(d, DatePart::ElapsedH(_) | DatePart::ElapsedM(_) | DatePart::ElapsedS(_))) {
        return None;
    }
    let ampm = parts().any(|d| matches!(d, DatePart::AmPm | DatePart::AP(..)));
    let time: String = parts()
        .filter_map(|d| match d {
            DatePart::Hour(_) => Some('h'),
            DatePart::Minute(_) => Some('m'),
            DatePart::Second(_) => Some('s'),
            _ => None,
        })
        .collect();
    match (time.as_str(), ampm) {
        ("hms", true) => Some("D6"),
        ("hm", true) => Some("D7"),
        ("hms", false) => Some("D8"),
        ("hm", false) => Some("D9"),
        _ => None,
    }
}
