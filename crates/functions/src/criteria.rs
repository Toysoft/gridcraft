//! Excel criteria (COUNTIF, SUMIFS, DCOUNT…) and wildcard matching.

use std::cmp::Ordering;

use gridcraft_core::parse::parse_number_text;
use gridcraft_core::{CellError, Value, compare_numbers, compare_text};

/// Longest text criterion COUNTIF and its relatives accept, in UTF-16 code units as Excel
/// counts them (😀 is two units); a longer one is `#VALUE!`.
const MAX_CRITERION_UTF16_UNITS: usize = 255;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Op {
    Eq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,
}

#[derive(Clone, Debug)]
enum Operand {
    Number(f64),
    Bool(bool),
    Error(CellError),
    /// Text, with its compiled pattern when it is a wildcard criterion.
    Text(String, Option<Wildcard>),
    /// A criterion cut to its first 255 UTF-16 units in the middle of a surrogate pair: the
    /// complete characters before the cut, followed (in Excel) by a lone high surrogate.
    ///
    /// No cell text holds a lone surrogate, so `=` matches nothing and `<>` everything, as
    /// Excel gives 0 for SUMIFS with a criterion of 128 emoji. For `<`, `<=`, `>` and `>=` the
    /// criterion sorts just above the kept text: a cell matches `>`/`>=` when it sorts above
    /// that text, and `<`/`<=` otherwise. This is an approximation (Excel wasn't checked) and
    /// only matters for comparison criteria over 255 units.
    CutText(String),
    /// Nothing after the operator (`"="`, `"<>"`) or an empty criterion.
    Blank,
}

/// A parsed criterion.
#[derive(Clone, Debug)]
pub(crate) struct Criterion {
    op: Op,
    operand: Operand,
    /// The criterion was a bare `""` (or empty cell text): matches blanks and empty strings.
    bare_empty: bool,
}

impl Criterion {
    /// Parses a criterion value the way COUNTIF does.
    pub(crate) fn parse(v: &Value) -> Criterion {
        match v {
            Value::Number(n) => Criterion { op: Op::Eq, operand: Operand::Number(*n), bare_empty: false },
            Value::Bool(b) => Criterion { op: Op::Eq, operand: Operand::Bool(*b), bare_empty: false },
            Value::Error(e) => Criterion { op: Op::Eq, operand: Operand::Error(*e), bare_empty: false },
            Value::Empty => Criterion { op: Op::Eq, operand: Operand::Number(0.0), bare_empty: false },
            Value::Array(a) => Criterion::parse(a.data.first().unwrap_or(&Value::Empty)),
            Value::Text(t) => Criterion::parse_text(t),
        }
    }

    /// Parses the criteria argument of COUNTIF, SUMIF, AVERAGEIF and the single-pair *IFS
    /// functions: like `parse`, but text longer than 255 UTF-16 code units is `#VALUE!`, as in
    /// Excel (checked on Excel 2016 and Microsoft 365: `"a"×254 & "😀"` is 255 characters but
    /// 256 units, and is `#VALUE!`).
    pub(crate) fn parse_arg(v: &Value) -> Result<Criterion, CellError> {
        match v {
            Value::Array(a) => Criterion::parse_arg(a.data.first().unwrap_or(&Value::Empty)),
            // A UTF-8 string has at least as many bytes as UTF-16 units, so short text skips the count.
            Value::Text(t) if t.len() > MAX_CRITERION_UTF16_UNITS && t.encode_utf16().nth(MAX_CRITERION_UTF16_UNITS).is_some() => {
                Err(CellError::Value)
            }
            other => Ok(Criterion::parse(other)),
        }
    }

    /// Parses a criteria argument of an *IFS function with two or more criteria pairs. Excel
    /// doesn't reject a longer text criterion there: it matches its first 255 UTF-16 code units
    /// (checked on Excel 2016 and Microsoft 365 with COUNTIFS, SUMIFS, AVERAGEIFS, MAXIFS and
    /// MINIFS). A cut through a surrogate pair matches nothing; see `Operand::CutText`.
    pub(crate) fn parse_arg_truncated(v: &Value) -> Criterion {
        match v {
            Value::Array(a) => Criterion::parse_arg_truncated(a.data.first().unwrap_or(&Value::Empty)),
            Value::Text(t) if t.len() > MAX_CRITERION_UTF16_UNITS => {
                let mut units = 0;
                for (i, c) in t.char_indices() {
                    if units + c.len_utf16() > MAX_CRITERION_UTF16_UNITS {
                        if units == MAX_CRITERION_UTF16_UNITS {
                            return Criterion::parse_text(&t[..i]);
                        }
                        // The cut falls between the two halves of `c`.
                        let (op, rest) = split_op(&t[..i]);
                        return Criterion { op, operand: Operand::CutText(rest.to_string()), bare_empty: false };
                    }
                    units += c.len_utf16();
                }
                Criterion::parse_text(t)
            }
            other => Criterion::parse(other),
        }
    }

    fn parse_text(t: &str) -> Criterion {
        if t.is_empty() {
            return Criterion { op: Op::Eq, operand: Operand::Blank, bare_empty: true };
        }
        let (op, rest) = split_op(t);
        let operand = if rest.is_empty() {
            Operand::Blank
        } else if let Some(n) = parse_number_text(rest) {
            Operand::Number(n)
        } else if rest.eq_ignore_ascii_case("TRUE") {
            Operand::Bool(true)
        } else if rest.eq_ignore_ascii_case("FALSE") {
            Operand::Bool(false)
        } else if let Some(e) = CellError::parse(rest) {
            Operand::Error(e)
        } else {
            Operand::Text(rest.to_string(), is_wildcard_criterion(rest).then(|| Wildcard::new(rest, false)))
        };
        Criterion { op, operand, bare_empty: false }
    }

    /// Whether a cell value satisfies the criterion.
    pub(crate) fn matches(&self, v: &Value) -> bool {
        let v = match v {
            Value::Array(a) => a.data.first().cloned().unwrap_or(Value::Empty),
            other => other.clone(),
        };
        if self.bare_empty {
            return matches!(&v, Value::Empty) || matches!(&v, Value::Text(t) if t.is_empty());
        }
        match &self.operand {
            Operand::Blank => {
                let blank = matches!(v, Value::Empty);
                match self.op {
                    Op::Eq => blank,
                    Op::Ne => !blank,
                    // "<" / ">" with nothing: Excel compares against "" for text cells, but an
                    // empty string never matches ("<=" and ">=" count it in neither).
                    _ => match &v {
                        Value::Text(t) if !t.is_empty() => cmp_op(self.op, compare_text(t, "")),
                        _ => false,
                    },
                }
            }
            Operand::Number(n) => {
                let cell = match &v {
                    Value::Number(x) => Some(*x),
                    // Numeric text matches only for equality (COUNTIF("1") counts text "1").
                    Value::Text(t) if matches!(self.op, Op::Eq | Op::Ne) => parse_number_text(t),
                    _ => None,
                };
                match cell {
                    Some(x) => cmp_op(self.op, compare_numbers(x, *n)),
                    None => self.op == Op::Ne,
                }
            }
            Operand::Bool(b) => match &v {
                Value::Bool(x) => cmp_op(self.op, x.cmp(b)),
                _ => self.op == Op::Ne,
            },
            Operand::Error(e) => match &v {
                Value::Error(x) => cmp_op(self.op, x.code().cmp(&e.code())),
                _ => self.op == Op::Ne,
            },
            Operand::Text(pat, wild) => match self.op {
                Op::Eq | Op::Ne => {
                    // Blank cells hold no text: `"*"` skips them and `"<>*"` counts them.
                    let cell_text = match &v {
                        Value::Text(t) => Some(t.to_string()),
                        _ => None,
                    };
                    let m = cell_text.is_some_and(|t| match wild {
                        Some(w) => w.matches(&t),
                        None => compare_text(&t, pat) == Ordering::Equal,
                    });
                    if self.op == Op::Eq { m } else { !m }
                }
                _ => match &v {
                    Value::Text(t) => cmp_op(self.op, compare_text(t, pat)),
                    _ => false,
                },
            },
            Operand::CutText(kept) => match self.op {
                Op::Eq => false,
                Op::Ne => true,
                Op::Lt | Op::Le => matches!(&v, Value::Text(t) if compare_text(t, kept) != Ordering::Greater),
                Op::Gt | Op::Ge => matches!(&v, Value::Text(t) if compare_text(t, kept) == Ordering::Greater),
            },
        }
    }
}

/// Splits a leading comparison operator (`=` when there is none) off a text criterion.
fn split_op(t: &str) -> (Op, &str) {
    if let Some(r) = t.strip_prefix(">=") {
        (Op::Ge, r)
    } else if let Some(r) = t.strip_prefix("<=") {
        (Op::Le, r)
    } else if let Some(r) = t.strip_prefix("<>") {
        (Op::Ne, r)
    } else if let Some(r) = t.strip_prefix('=') {
        (Op::Eq, r)
    } else if let Some(r) = t.strip_prefix('>') {
        (Op::Gt, r)
    } else if let Some(r) = t.strip_prefix('<') {
        (Op::Lt, r)
    } else {
        (Op::Eq, t)
    }
}

fn cmp_op(op: Op, o: Ordering) -> bool {
    match op {
        Op::Eq => o == Ordering::Equal,
        Op::Ne => o != Ordering::Equal,
        Op::Lt => o == Ordering::Less,
        Op::Le => o != Ordering::Greater,
        Op::Gt => o == Ordering::Greater,
        Op::Ge => o != Ordering::Less,
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Tok {
    Char(char),
    Any,
    Star,
}

/// An Excel wildcard pattern, compiled once: `*` matches any run of characters, `?` any one
/// character, and `~` takes the next character literally. Excel lets `~` escape any character
/// (`h~ello` is `hello`) and drops a trailing `~` (`x~` is `x`), checked against Excel 365 in
/// COUNTIF, MATCH, SEARCH, AutoFilter and Find & Replace.
///
/// Matching is case-insensitive unless the pattern is built with `match_case`. Each character
/// folds to exactly one character (so positions in the text stay put), and a character whose
/// lowercase is longer keeps itself: `İ` (U+0130) only matches `İ`, as in Excel, where
/// `COUNTIF(…,"i")` skips it and `SEARCH("i","qİq")` is `#VALUE!`.
///
/// This is the one implementation behind the criteria functions, the lookups, SEARCH, and the
/// engine's Find & Replace, AutoFilter and Advanced Filter.
#[derive(Clone, Debug, PartialEq)]
pub struct Wildcard {
    toks: Vec<Tok>,
    match_case: bool,
}

impl Wildcard {
    pub fn new(pattern: &str, match_case: bool) -> Wildcard {
        let fold = |c: char| if match_case { c } else { fold_char(c) };
        let mut toks = Vec::new();
        let mut it = pattern.chars();
        while let Some(c) = it.next() {
            match c {
                // A trailing `~` escapes nothing and is dropped.
                '~' => toks.extend(it.next().map(|n| Tok::Char(fold(n)))),
                '*' => toks.push(Tok::Star),
                '?' => toks.push(Tok::Any),
                c => toks.push(Tok::Char(fold(c))),
            }
        }
        Wildcard { toks, match_case }
    }

    /// Whether nothing is left to match (`""`, or a lone `~`).
    pub fn is_empty(&self) -> bool {
        self.toks.is_empty()
    }

    fn chars(&self, text: &str) -> Vec<char> {
        if self.match_case { text.chars().collect() } else { text.chars().map(fold_char).collect() }
    }

    /// Whether all of `text` matches.
    pub fn matches(&self, text: &str) -> bool {
        match_from(&self.toks, &self.chars(text), true).is_some()
    }

    /// The first match in `text` that starts at or after char `start`, as char positions
    /// `(start, end)`. A `*` takes as few characters as it can, except a trailing `*`, which takes
    /// the rest of the text: Excel's Replace turns `hello world` into `X world` for `h*o` and into
    /// `heX` for `l*`. An empty pattern matches nothing.
    pub fn find(&self, text: &str, start: usize) -> Option<(usize, usize)> {
        self.find_in(&self.chars(text), start)
    }

    /// Every match in `text`, left to right and not overlapping, as char positions (what Replace
    /// replaces).
    pub fn spans(&self, text: &str) -> Vec<(usize, usize)> {
        let t = self.chars(text);
        let mut out = Vec::new();
        let mut i = 0;
        while let Some((start, end)) = self.find_in(&t, i) {
            out.push((start, end));
            if end <= start {
                break;
            }
            i = end;
        }
        out
    }

    fn find_in(&self, t: &[char], start: usize) -> Option<(usize, usize)> {
        if self.toks.is_empty() {
            return None;
        }
        let to_end = self.toks.last() == Some(&Tok::Star);
        (start..t.len()).find_map(|i| {
            let len = match_from(&self.toks, t.get(i..)?, false)?;
            Some((i, if to_end { t.len() } else { i + len }))
        })
    }
}

/// Whether a criterion (COUNTIF and the other *IF functions, the D-functions, AutoFilter and
/// Advanced Filter) is a wildcard pattern. Excel reads it as one only when it holds `*` or `?`,
/// escaped or not; any other text compares as it is, tildes included (`COUNTIF(…,"a~b")` counts
/// `a~b`, not `ab`). Lookups and SEARCH read every `*`, `?` and `~` as a pattern instead
/// ([`has_wildcards`]).
pub fn is_wildcard_criterion(text: &str) -> bool {
    text.contains(['*', '?'])
}

/// One character, lowercased to exactly one character; one whose lowercase is longer keeps
/// itself (`İ`).
fn fold_char(c: char) -> char {
    let mut l = c.to_lowercase();
    match (l.next(), l.next()) {
        (Some(x), None) => x,
        _ => c,
    }
}

/// Whether a lookup value (MATCH, VLOOKUP, XLOOKUP) is a pattern: it holds `*`, `?` or `~`.
pub(crate) fn has_wildcards(pattern: &str) -> bool {
    pattern.contains(['*', '?', '~'])
}

/// Case-insensitive whole-text match with `*`, `?` and `~` escapes.
pub(crate) fn wildcard_match(pattern: &str, text: &str) -> bool {
    Wildcard::new(pattern, false).matches(text)
}

/// Iterative matcher with backtracking on the last star: returns the matched length of `t`
/// when `p` matches a prefix of `t` (or all of `t` when `whole`). Stars take as few characters
/// as they can.
fn match_from(p: &[Tok], t: &[char], whole: bool) -> Option<usize> {
    let (mut pi, mut ti) = (0usize, 0usize);
    let mut star: Option<(usize, usize)> = None;
    loop {
        if pi == p.len() && (!whole || ti == t.len()) {
            return Some(ti);
        }
        let step = match (p.get(pi), t.get(ti)) {
            (Some(Tok::Star), _) => {
                star = Some((pi, ti));
                pi += 1;
                continue;
            }
            (Some(Tok::Any), Some(_)) => true,
            (Some(Tok::Char(c)), Some(x)) => c == x,
            _ => false,
        };
        if step {
            pi += 1;
            ti += 1;
            continue;
        }
        match star {
            Some((sp, st)) if st < t.len() => {
                star = Some((sp, st + 1));
                pi = sp + 1;
                ti = st + 1;
            }
            _ => return None,
        }
    }
}

/// SEARCH-style find: the char index (0-based) of the first position at or after `start` where
/// the wildcard pattern matches, case-insensitively. An empty pattern (`""` or `"~"`) is found
/// at `start`, as SEARCH finds empty text there.
pub(crate) fn wildcard_find(pattern: &str, text: &str, start: usize) -> Option<usize> {
    let w = Wildcard::new(pattern, false);
    if w.is_empty() {
        return (start <= text.chars().count()).then_some(start);
    }
    w.find(text, start).map(|(i, _)| i)
}

/// Exact-or-wildcard equality used by MATCH/VLOOKUP/XLOOKUP exact modes: text with wildcards
/// matches by pattern (only when `wild`), otherwise values compare case-insensitively.
pub(crate) fn lookup_equal(needle: &Value, hay: &Value, wild: bool) -> bool {
    match (needle, hay) {
        (Value::Text(p), Value::Text(t)) => {
            if wild {
                wildcard_match(p, t)
            } else {
                compare_text(p, t) == Ordering::Equal
            }
        }
        (Value::Number(a), Value::Number(b)) => compare_numbers(*a, *b) == Ordering::Equal,
        (Value::Bool(a), Value::Bool(b)) => a == b,
        (Value::Error(a), Value::Error(b)) => a == b,
        (Value::Empty, Value::Empty) => true,
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn m(c: &str, v: Value) -> bool {
        Criterion::parse(&Value::from(c)).matches(&v)
    }

    #[test]
    fn criteria() {
        assert!(m(">5", Value::Number(6.0)));
        assert!(!m(">5", Value::Number(5.0)));
        assert!(m(">=5", Value::Number(5.0)));
        assert!(m("<>5", Value::from("x")));
        assert!(m("<>5", Value::Empty));
        assert!(!m(">5", Value::from("9")));
        assert!(m("5", Value::from("5")));
        assert!(m("a*", Value::from("Apple")));
        assert!(!m("a*", Value::from("banana")));
        assert!(m("?pple", Value::from("apple")));
        assert!(m("~*", Value::from("*")));
        assert!(!m("~*", Value::from("x")));
        assert!(m("", Value::Empty));
        assert!(m("", Value::from("")));
        assert!(!m("", Value::Number(0.0)));
        assert!(m("=", Value::Empty));
        assert!(!m("=", Value::from("a")));
        assert!(m("<>", Value::from("a")));
        assert!(!m("<>", Value::Empty));
        assert!(m("TRUE", Value::Bool(true)));
        // A bare "<=" or ">=" never matches an empty string (Excel 365: COUNTIF(="", "<=") is 0).
        assert!(!m("<=", Value::from("")));
        assert!(!m(">=", Value::from("")));
        assert!(m("<b", Value::from("apple")));
        assert!(!m("<b", Value::Number(1.0)));
        assert!(m("#N/A", Value::Error(CellError::NA)));
        assert!(Criterion::parse(&Value::Number(3.0)).matches(&Value::Number(3.0)));
        assert!(Criterion::parse(&Value::Number(0.3)).matches(&Value::Number(0.1 + 0.2)));
        assert!(lookup_equal(&Value::Number(0.3), &Value::Number(0.1 + 0.2), false));
        assert!(m("<>a*", Value::from("bcd")));
        assert!(m(">1/1/2020", Value::Number(44000.0)));
        // Text patterns never match blank cells, but an empty string is text.
        assert!(!m("*", Value::Empty));
        assert!(m("*", Value::from("")));
        assert!(m("<>*", Value::Empty));
        assert!(!m("<>*", Value::from("")));
        assert!(!m("?*", Value::Empty));
        assert!(m("<>a*", Value::Empty));
    }

    #[test]
    fn wildcards() {
        assert!(wildcard_match("*", ""));
        assert!(wildcard_match("a*c", "abbbc"));
        assert!(!wildcard_match("a*c", "abbbd"));
        assert!(wildcard_match("*~?", "what?"));
        assert_eq!(wildcard_find("b?d", "abcd abd", 0), Some(1));
        assert_eq!(wildcard_find("b?d", "abxxd bcd", 0), Some(6));
        assert_eq!(wildcard_find("c*", "abcd", 0), Some(2));
        assert_eq!(wildcard_find("z", "abcd", 0), None);
        assert_eq!(wildcard_find("b", "a\u{130}bc", 0), Some(2));
        // Excel 365: `İ` only matches itself (COUNTIF "i*" and AutoFilter skip `İstanbul`).
        assert!(!wildcard_match("i*", "\u{130}stanbul"));
        assert!(wildcard_match("\u{130}*", "\u{130}stanbul"));
        assert!(has_wildcards("a*"));
        assert!(has_wildcards("a~b"));
        assert!(!has_wildcards("abc"));
    }

    /// Criteria as Excel 365 reads them (COUNTIF and AutoFilter over the same values).
    #[test]
    fn criteria_follow_excel_tilde_and_case_rules() {
        let t = |s: &str| Value::from(s);
        // A trailing `~` is dropped: "?~" is "?", "*~" is "*".
        assert!(m("?~", t("y")));
        assert!(!m("?~", t("x~")));
        assert!(m("*~", t("x~")) && m("*~", t("ab")));
        // `~` escapes any character inside a wildcard criterion.
        assert!(m("h~el*", t("HELLO")));
        assert!(m("?~x", t("~x")));
        assert!(m("*~~", t("xyz~")) && !m("*~~", t("xyz")));
        assert!(m("a~?b", t("a?b")) && !m("a~?b", t("axb")));
        // Without `*` or `?`, the criterion compares as it is, tildes included.
        assert!(m("a~b", t("a~b")) && !m("a~b", t("ab")));
        assert!(m("~x", t("~x")) && !m("~x", t("x")));
        assert!(!m("~~", t("~")));
        assert!(!m("h~ello", t("hello")));
        assert!(m("~", t("~")));
        // One character folds to one: `İ` and `ı` only match themselves; `ẞ` folds to `ß`.
        assert!(m("?", t("\u{130}")));
        assert!(m("i", t("I")) && !m("i", t("\u{130}")) && !m("i", t("\u{131}")));
        assert!(m("\u{130}", t("\u{130}")));
        assert!(m("\u{df}*", t("\u{1e9e}")));
    }

    /// Lookups and SEARCH read every `~` as an escape (Excel 365: MATCH("a~b", …, 0) finds `ab`).
    #[test]
    fn lookups_and_search_follow_excel_tilde_rules() {
        let (s, x) = (|v: &str| Value::from(v), true);
        assert!(lookup_equal(&s("a~b"), &s("ab"), x));
        assert!(!lookup_equal(&s("a~b"), &s("a~b"), x));
        assert!(lookup_equal(&s("x~"), &s("x"), x));
        assert!(!lookup_equal(&s("x~"), &s("x~"), x));
        assert!(lookup_equal(&s("~~"), &s("~"), x));
        assert!(lookup_equal(&s("a~~b"), &s("a~b"), x));
        assert_eq!(wildcard_find("~x", "xa~bx", 0), Some(0));
        assert_eq!(wildcard_find("x~", "x~x", 0), Some(0));
        assert_eq!(wildcard_find("a~~b", "xa~bx", 0), Some(1));
        assert_eq!(wildcard_find("~", "abc", 0), Some(0));
        assert_eq!(wildcard_find("\u{130}", "qiq", 0), None);
        assert_eq!(wildcard_find("i", "q\u{130}q", 0), None);
        assert_eq!(wildcard_find("\u{130}", "q\u{130}q", 0), Some(1));
    }

    /// Match spans as Excel 365's Replace uses them.
    #[test]
    fn wildcard_spans() {
        let w = |p: &str| Wildcard::new(p, false);
        assert_eq!(w("h*o").find("hello world", 0), Some((0, 5)));
        assert_eq!(w("l*").find("hello world", 0), Some((2, 11)));
        assert_eq!(w("a?b").find("xaxbyazb", 0), Some((1, 4)));
        assert_eq!(w("a?b").find("xaxbyazb", 4), Some((5, 8)));
        assert_eq!(w("~*").find("a*b", 0), Some((1, 2)));
        assert_eq!(w("~").find("abc", 0), None);
        assert_eq!(Wildcard::new("HELLO", true).find("hello HELLO", 0), Some((6, 11)));
        assert!(!Wildcard::new("hello", true).matches("HELLO"));
        assert!(w("hello").matches("HELLO"));
    }
}
