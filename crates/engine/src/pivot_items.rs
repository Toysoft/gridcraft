//! Calculated PivotTable items: a formula over the items of one field (`=East+West`,
//! `=East*1.1`, `='North East'/2`, `=Region[East]-Region['North East']`).
//!
//! As Microsoft documents for Excel, the formula runs on each item's summary for every
//! combination of the other fields' items, and its results then take part in subtotals and grand
//! totals like any item's. Items are written bare, in single quotes, or qualified by their field
//! (`Field[Item]`); only the calculated item's own field may be named. Cell references and
//! defined names are not allowed. The formula machinery is the calculated fields' (see
//! [`crate::pivot_calc`]).

use crate::pivot_calc::{self, Compiled};

/// Most calculated items per PivotTable.
pub const MAX_CALC_ITEMS: usize = 256;

/// Rewrites `Field[Item]` and `Field['Item']` references to the calculated item's own `field`
/// as quoted item names (`'Item'`). Any other bracket reference is an error.
fn unqualify(formula: &str, field: &str) -> Result<String, String> {
    let chars: Vec<char> = formula.chars().collect();
    let at = |i: usize| chars.get(i).copied();
    let mut out = String::with_capacity(formula.len());
    let mut i = 0;
    // A quoted run (`'…'` with `''` for a quote), returned with the index after it.
    let quoted = |mut i: usize| -> Result<(String, usize), String> {
        let mut name = String::new();
        i += 1;
        loop {
            match at(i) {
                None => return Err("The formula has an unterminated item name.".into()),
                Some('\'') if at(i + 1) == Some('\'') => {
                    name.push('\'');
                    i += 2;
                }
                Some('\'') => return Ok((name, i + 1)),
                Some(c) => {
                    name.push(c);
                    i += 1;
                }
            }
        }
    };
    let quote = |name: &str| format!("'{}'", name.replace('\'', "''"));
    while let Some(c) = at(i) {
        match c {
            '"' => {
                // Text constants pass through untouched.
                out.push('"');
                i += 1;
                while let Some(x) = at(i) {
                    out.push(x);
                    i += 1;
                    if x == '"' {
                        if at(i) == Some('"') {
                            out.push('"');
                            i += 1;
                        } else {
                            break;
                        }
                    }
                }
            }
            '\'' => {
                let (name, next) = quoted(i)?;
                if at(next) == Some('[') {
                    if !name.eq_ignore_ascii_case(field) {
                        return Err(format!("A calculated item in \"{field}\" can only use items of that field; '{name}' is another field."));
                    }
                    bracket(&chars, next, &mut out, &quote)?;
                    i = skip_bracket(&chars, next);
                } else {
                    out.push_str(&quote(&name));
                    i = next;
                }
            }
            '[' => return Err("The formula contains an error.".into()),
            c if c.is_alphabetic() || c == '_' => {
                let start = i;
                while at(i).is_some_and(|d| d.is_alphanumeric() || d == '_' || d == '.') {
                    i += 1;
                }
                let word: String = chars.get(start..i).map(|w| w.iter().collect()).unwrap_or_default();
                if at(i) == Some('[') {
                    if !word.eq_ignore_ascii_case(field) {
                        return Err(format!("A calculated item in \"{field}\" can only use items of that field; '{word}' is another field."));
                    }
                    bracket(&chars, i, &mut out, &quote)?;
                    i = skip_bracket(&chars, i);
                } else {
                    out.push_str(&word);
                }
            }
            c => {
                out.push(c);
                i += 1;
            }
        }
    }
    Ok(out)
}

/// Writes the item of the bracket starting at `open` (`[East]`, `['North East']`) as a quoted
/// name. Positions (`[1]`, `[-1]`) are refused.
fn bracket(chars: &[char], open: usize, out: &mut String, quote: &dyn Fn(&str) -> String) -> Result<usize, String> {
    let close = skip_bracket(chars, open);
    let inner: String = chars.get(open + 1..close.saturating_sub(1)).map(|w| w.iter().collect()).unwrap_or_default();
    if chars.get(close.saturating_sub(1)) != Some(&']') || close <= open + 1 {
        return Err("The formula has an unterminated item reference.".into());
    }
    let inner = inner.trim();
    let name = match inner.strip_prefix('\'').and_then(|x| x.strip_suffix('\'')) {
        Some(q) => q.replace("''", "'"),
        None => inner.to_string(),
    };
    if name.is_empty() {
        return Err("The formula contains an error.".into());
    }
    if name.trim_start_matches(['+', '-']).chars().all(|c| c.is_ascii_digit()) {
        return Err("Refer to items by name; item positions are not supported.".into());
    }
    out.push_str(&quote(&name));
    Ok(close)
}

/// The index just past the `]` closing the bracket opened at `open` (quotes inside are skipped);
/// the end of the text when it isn't closed.
fn skip_bracket(chars: &[char], open: usize) -> usize {
    let mut i = open + 1;
    let mut in_quote = false;
    while let Some(c) = chars.get(i) {
        match c {
            '\'' => in_quote = !in_quote,
            ']' if !in_quote => return i + 1,
            _ => {}
        }
        i += 1;
    }
    chars.len()
}

/// Parses a calculated item's formula. `item` maps a written name to the canonical caption of an
/// item of `field` (data items and calculated items).
pub fn compile(formula: &str, field: &str, item: &dyn Fn(&str) -> Option<String>) -> Result<Compiled, String> {
    let text = formula.trim();
    let text = text.strip_prefix('=').unwrap_or(text);
    if text.len() > pivot_calc::MAX_FORMULA_LEN {
        return Err("The formula is too long.".into());
    }
    let plain = unqualify(text, field)?;
    pivot_calc::compile_named(&plain, item, "item")
}

#[cfg(test)]
mod tests {
    use super::*;
    use gridcraft_core::Value;
    use gridcraft_model::Workbook;

    fn items(n: &str) -> Option<String> {
        ["East", "West", "North East", "Dave's"].iter().find(|f| f.eq_ignore_ascii_case(n.trim())).map(|f| f.to_string())
    }

    #[test]
    fn compiles_item_names() {
        let c = compile("=East+west", "Region", &items).unwrap();
        assert_eq!(c.refs, vec!["East", "West"]);
        let c = compile("= Region[East]*2 + 'North East' - Region['Dave''s']", "Region", &items).unwrap();
        assert_eq!(c.refs, vec!["East", "North East", "Dave's"]);
        let wb = Workbook::new();
        assert_eq!(pivot_calc::eval(&wb, &c, &[Value::Number(10.0), Value::Number(1.0), Value::Number(4.0)]), Value::Number(17.0));
        let c = compile("=SUM(East,West)/2", "Region", &items).unwrap();
        assert_eq!(pivot_calc::eval(&wb, &c, &[Value::Number(10.0), Value::Number(4.0)]), Value::Number(7.0));
        // Refusals: another field, positions, unknown items, cells, broken syntax.
        assert!(compile("=Product[Apples]", "Region", &items).unwrap_err().contains("Product"));
        assert!(compile("=Region[1]", "Region", &items).is_err());
        assert!(compile("=Region[-1]", "Region", &items).is_err());
        assert!(compile("=Foo+1", "Region", &items).unwrap_err().contains("item named 'Foo'"));
        assert!(compile("=A1*2", "Region", &items).is_err());
        assert!(compile("=East*(", "Region", &items).is_err());
        assert!(compile("=Region[East", "Region", &items).is_err());
        assert!(compile("='East", "Region", &items).is_err());
        assert!(compile("=[East]", "Region", &items).is_err());
        assert!(compile("=", "Region", &items).is_err());
        // Text constants keep their brackets and quotes.
        assert!(compile("=IF(East>0,\"a[b]'c\",West)", "Region", &items).is_ok());
    }
}
