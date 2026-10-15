//! Parsing what people type into a cell (and numeric text in formulas).
//!
//! Recognises numbers with thousands separators, percentages, currency, scientific notation,
//! fractions (`1 1/2`), dates (`3/4/2026`, `2026-03-04`, `4-Mar-2026`, `Mar 4, 2026`), times
//! (`10:30`, `10:30:15 PM`) and date-times. [`parse_input`] reads en-US, the canonical spelling;
//! [`parse_input_in`] reads a regional format ([`NumberLocale`]).

use crate::date::{DateSystem, MONTHS, serial_from_ymd, time_fraction};
use crate::locale::{DateOrder, NumberLocale};
use crate::value::{CellError, Value};

/// A parsed entry: its value and the number format Excel would apply automatically.
#[derive(Clone, Debug, PartialEq)]
pub struct Parsed {
    pub value: Value,
    pub format: Option<&'static str>,
}

/// Interprets typed cell input (not formulas). Text that isn't a number, date, boolean or error
/// stays text. A leading apostrophe forces text.
pub fn parse_input(s: &str, sys: DateSystem) -> Parsed {
    if s.is_empty() {
        return Parsed { value: Value::Empty, format: None };
    }
    if let Some(rest) = s.strip_prefix('\'') {
        return Parsed { value: Value::text(rest), format: None };
    }
    let t = s.trim();
    if t.eq_ignore_ascii_case("TRUE") {
        return Parsed { value: Value::Bool(true), format: None };
    }
    if t.eq_ignore_ascii_case("FALSE") {
        return Parsed { value: Value::Bool(false), format: None };
    }
    if let Some(e) = CellError::parse(t) {
        return Parsed { value: Value::Error(e), format: None };
    }
    if let Some((n, f)) = parse_numeric(t, sys) {
        return Parsed { value: Value::number(n), format: f };
    }
    Parsed { value: Value::text(s), format: None }
}

/// Interprets typed cell input in a regional format: `1,5`, `1.234,5`, `-3,2%`, `13.10.2026`,
/// `13. Okt 2026`, `5 €` in de-DE. Booleans and error values are read in their canonical
/// spelling (the formula language's words are the caller's business). The automatic formats are
/// canonical codes, as [`parse_input`] gives them. The canonical locale reads as [`parse_input`].
pub fn parse_input_in(s: &str, sys: DateSystem, loc: &NumberLocale) -> Parsed {
    if loc.is_canonical() {
        return parse_input(s, sys);
    }
    if s.is_empty() {
        return Parsed { value: Value::Empty, format: None };
    }
    if let Some(rest) = s.strip_prefix('\'') {
        return Parsed { value: Value::text(rest), format: None };
    }
    let t = s.trim();
    if t.eq_ignore_ascii_case("TRUE") {
        return Parsed { value: Value::Bool(true), format: None };
    }
    if t.eq_ignore_ascii_case("FALSE") {
        return Parsed { value: Value::Bool(false), format: None };
    }
    if let Some(e) = CellError::parse(t) {
        return Parsed { value: Value::Error(e), format: None };
    }
    if let Some((n, f)) = parse_numeric_in(t, sys, loc) {
        return Parsed { value: Value::number(n), format: f };
    }
    Parsed { value: Value::text(s), format: None }
}

/// Typed text longer than this is never a number or a date (keeps the scans below short).
const MAX_NUMERIC_INPUT: usize = 400;

fn parse_numeric_in(t: &str, sys: DateSystem, loc: &NumberLocale) -> Option<(f64, Option<&'static str>)> {
    if t.is_empty() || t.len() > MAX_NUMERIC_INPUT {
        return None;
    }
    // Parenthesised negatives: (1,5) = -1,5.
    if let Some(inner) = t.strip_prefix('(').and_then(|x| x.strip_suffix(')')) {
        return parse_numeric_in(inner.trim(), sys, loc).map(|(n, f)| (-n, f));
    }
    if let Some(r) = parse_plain_in(t, loc) {
        return Some(r);
    }
    if let Some(r) = parse_fraction(t) {
        return Some((r, Some("# ?/?")));
    }
    parse_datetime_in(t, sys, loc)
}

/// Signs, a currency symbol, thousands separators, the decimal separator, percent and exponent,
/// with the locale's separators. The automatic formats are the canonical ones.
fn parse_plain_in(t: &str, loc: &NumberLocale) -> Option<(f64, Option<&'static str>)> {
    let mut s = t;
    let mut neg = false;
    let mut currency = None;
    let mut percent = false;
    if let Some(r) = s.strip_prefix('-') {
        neg = true;
        s = r.trim_start();
    } else if let Some(r) = s.strip_prefix('+') {
        s = r.trim_start();
    }
    let region = loc.region;
    if let Some((symbol, whole, decimals)) = region.currency_after
        && let Some(r) = s.strip_suffix(symbol)
    {
        currency = Some((whole, decimals));
        s = r.trim_end();
    } else if let Some((symbol, ..)) = region.currency_after
        && let Some(r) = s.strip_prefix(symbol)
    {
        // Observed in de-DE: `€5` is the number 5, without a currency format.
        s = r.trim_start();
    } else if region.tag == crate::locale::EN_US.tag
        && let Some(r) = s.strip_prefix('$')
    {
        currency = Some(("\"$\"#,##0", "\"$\"#,##0.00"));
        s = r.trim_start();
    }
    if currency.is_none()
        && let Some(r) = s.strip_suffix('%')
    {
        percent = true;
        s = r.trim_end();
    }
    if s.is_empty() || !s.starts_with(|c: char| c.is_ascii_digit() || c == loc.decimal) {
        return None;
    }
    // Mantissa and exponent.
    let (mantissa, exp) = match s.find(['e', 'E']) {
        Some(i) => (s.get(..i)?, Some(s.get(i + 1..)?)),
        None => (s, None),
    };
    if let Some(e) = exp {
        let digits = e.strip_prefix(['+', '-']).unwrap_or(e);
        if digits.is_empty() || !digits.bytes().all(|b| b.is_ascii_digit()) {
            return None;
        }
    }
    let (int, frac) = match mantissa.split_once(loc.decimal) {
        Some((a, b)) => (a, Some(b)),
        None => (mantissa, None),
    };
    if frac.is_some_and(|f| !f.bytes().all(|b| b.is_ascii_digit())) {
        return None;
    }
    let grouped = int.contains(loc.thousands);
    let mut digits = String::with_capacity(s.len());
    if grouped {
        // Thousands separators sit between groups of three, before the decimal separator.
        let mut groups = int.split(loc.thousands);
        let first = groups.next()?;
        if first.is_empty() || first.len() > 3 || !first.bytes().all(|b| b.is_ascii_digit()) {
            return None;
        }
        digits.push_str(first);
        for g in groups {
            if g.len() != 3 || !g.bytes().all(|b| b.is_ascii_digit()) {
                return None;
            }
            digits.push_str(g);
        }
    } else {
        if !int.bytes().all(|b| b.is_ascii_digit()) {
            return None;
        }
        digits.push_str(int);
    }
    if digits.is_empty() && frac.is_none_or(str::is_empty) {
        return None;
    }
    let has_decimals = frac.is_some();
    if let Some(f) = frac {
        digits.push('.');
        digits.push_str(f);
    }
    if digits.starts_with('.') {
        digits.insert(0, '0');
    }
    if digits.ends_with('.') {
        digits.push('0');
    }
    if let Some(e) = exp {
        digits.push('e');
        digits.push_str(e);
    }
    let mut n: f64 = digits.parse().ok()?;
    if !n.is_finite() {
        return None;
    }
    if percent {
        n /= 100.0;
    }
    if neg {
        n = -n;
    }
    let fmt = if let Some((whole, decimals)) = currency {
        Some(if has_decimals { decimals } else { whole })
    } else if percent {
        Some(if has_decimals { "0.00%" } else { "0%" })
    } else if exp.is_some() {
        Some("0.00E+00")
    } else if grouped {
        Some(if has_decimals { "#,##0.00" } else { "#,##0" })
    } else {
        None
    };
    Some((n, fmt))
}

/// A month name of the region (full, short or an alias such as `Mär`), case-insensitively; a
/// trailing dot (`Okt.`) is allowed.
fn month_from_name_in(s: &str, loc: &NumberLocale) -> Option<u32> {
    let s = s.strip_suffix('.').unwrap_or(s).trim();
    if s.is_empty() {
        return None;
    }
    let eq = |a: &str| a.chars().flat_map(char::to_lowercase).eq(s.chars().flat_map(char::to_lowercase));
    let region = loc.region;
    (region.months.iter().position(|m| eq(m)))
        .or_else(|| region.months_short.iter().position(|m| eq(m)))
        .map(|i| i as u32 + 1)
        .or_else(|| region.month_aliases.iter().find(|(a, _)| eq(a)).map(|(_, m)| *m))
}

/// Excel's two-digit years: 00–29 are 2000–2029, 30–99 are 1930–1999.
fn expand_year(text: &str, y: i64) -> i64 {
    if text.len() <= 2 { if y < 30 { 2000 + y } else { 1900 + y } } else { y }
}

/// A date typed in the locale's order (`13.10.2026`, `13.10.26`, `13/10/2026`, `2026-10-13`,
/// `13.10` this year, `1.13` January 2013, `13. Okt 2026`, `Okt 26`, `13. Okt`). Observed in
/// de-DE: a trailing separator with no year (`13.10.`) is text, ISO dates get the short date format.
fn parse_date_in(t: &str, sys: DateSystem, loc: &NumberLocale) -> Option<(f64, &'static str)> {
    let t = t.trim();
    let num = |s: &str| if !s.is_empty() && s.len() <= 5 && s.bytes().all(|b| b.is_ascii_digit()) { s.parse::<i64>().ok() } else { None };
    if t.chars().any(char::is_alphabetic) {
        // Month names: `13. Okt 2026`, `13-Okt-26`, `Okt 26`, `13. Okt`.
        let words: Vec<&str> = t.split([' ', '.', '-', '/', ',']).filter(|w| !w.is_empty()).collect();
        return match words.as_slice() {
            [d, m, y] => {
                let (d, m, y) = (num(d)?, month_from_name_in(m, loc)?, (num(y)?, *y));
                valid_ymd(sys, expand_year(y.1, y.0), m as i64, d).map(|s| (s, "d-mmm-yy"))
            }
            [a, b] => {
                if let (Some(d), Some(m)) = (num(a), month_from_name_in(b, loc)) {
                    valid_ymd(sys, current_year(), m as i64, d).map(|s| (s, "d-mmm"))
                } else {
                    let (m, y) = (month_from_name_in(a, loc)?, num(b)?);
                    valid_ymd(sys, expand_year(b, y), m as i64, 1).map(|s| (s, "mmm-yy"))
                }
            }
            _ => None,
        };
    }
    let parts: Vec<&str> = t.split([loc.region.date_separator, '/', '-', '.']).map(str::trim).collect();
    match parts.as_slice() {
        [a, b, c] => {
            if a.len() == 4 {
                let (y, m, d) = (num(a)?, num(b)?, num(c)?);
                return valid_ymd(sys, y, m, d).map(|s| (s, "m/d/yyyy"));
            }
            let (x, y_, year) = (num(a)?, num(b)?, num(c)?);
            let (m, d) = match loc.region.date_order {
                DateOrder::Dmy => (y_, x),
                DateOrder::Mdy => (x, y_),
            };
            valid_ymd(sys, expand_year(c, year), m, d).map(|s| (s, "m/d/yyyy"))
        }
        [a, b] => {
            let (x, y_) = (num(a)?, num(b)?);
            let (m, d) = match loc.region.date_order {
                DateOrder::Dmy => (y_, x),
                DateOrder::Mdy => (x, y_),
            };
            if (1..=12).contains(&m)
                && let Some(s) = valid_ymd(sys, current_year(), m, d)
            {
                return Some((s, "d-mmm"));
            }
            // Month and a two-digit year that can't be a month (`1.13` = January 2013; observed:
            // `1.00` is text), or a four-digit year after `/` (`10/2026`).
            let (m, y) = (x, y_);
            let year_ok = (b.len() == 2 && y > 12) || (b.len() == 4 && t.contains('/'));
            if year_ok && (1..=12).contains(&m) {
                return valid_ymd(sys, expand_year(b, y), m, 1).map(|s| (s, "mmm-yy"));
            }
            None
        }
        _ => None,
    }
}

fn parse_datetime_in(t: &str, sys: DateSystem, loc: &NumberLocale) -> Option<(f64, Option<&'static str>)> {
    if let Some((tf, secs, ampm)) = parse_time(t) {
        let f = match (secs, ampm) {
            _ if tf >= 1.0 => "[h]:mm:ss",
            (false, false) => "h:mm",
            (true, false) => "h:mm:ss",
            (false, true) => "h:mm AM/PM",
            (true, true) => "h:mm:ss AM/PM",
        };
        return Some((tf, Some(f)));
    }
    if let Some((d, f)) = parse_date_in(t, sys, loc) {
        return Some((d, Some(f)));
    }
    // Date and time: split at a space before a time-looking tail (`13.10.2026 14:30`,
    // `13. Okt 2026 14:30`, `13.10.2026 2:30 PM`).
    for (i, _) in t.match_indices(' ').take(16) {
        let (a, b) = (t.get(..i)?, t.get(i + 1..)?);
        if let (Some((d, _)), Some((tf, _, _))) = (parse_date_in(a, sys, loc), parse_time(b))
            && tf < 1.0
        {
            return Some((d + tf, Some("m/d/yyyy h:mm")));
        }
    }
    None
}

/// Numeric text as arithmetic coerces it ("1,000", "50%", "$3", "1/2/2020", "10:00").
pub fn parse_number_text(s: &str) -> Option<f64> {
    parse_number_text_in(s, DateSystem::D1900)
}

/// Like [`parse_number_text`], with date text read in the given workbook date system.
pub fn parse_number_text_in(s: &str, sys: DateSystem) -> Option<f64> {
    parse_numeric(s.trim(), sys).map(|(n, _)| n)
}

fn parse_numeric(t: &str, sys: DateSystem) -> Option<(f64, Option<&'static str>)> {
    if t.is_empty() {
        return None;
    }
    // Parenthesised negatives: (100) = -100.
    if let Some(inner) = t.strip_prefix('(').and_then(|x| x.strip_suffix(')')) {
        return parse_numeric(inner.trim(), sys).map(|(n, f)| (-n, f));
    }
    if let Some(r) = parse_plain(t) {
        return Some(r);
    }
    if let Some(r) = parse_fraction(t) {
        return Some((r, Some("# ?/?")));
    }
    if let Some(r) = parse_datetime(t, sys) {
        return Some(r);
    }
    None
}

/// Signs, currency, thousands separators, percent, exponent.
fn parse_plain(t: &str) -> Option<(f64, Option<&'static str>)> {
    let mut s = t;
    let mut neg = false;
    let mut currency = false;
    let mut percent = false;
    if let Some(r) = s.strip_prefix('-') {
        neg = true;
        s = r.trim_start();
    } else if let Some(r) = s.strip_prefix('+') {
        s = r.trim_start();
    }
    if let Some(r) = s.strip_prefix('$') {
        currency = true;
        s = r.trim_start();
        if !neg && let Some(r2) = s.strip_prefix('-') {
            neg = true;
            s = r2;
        }
    }
    if let Some(r) = s.strip_suffix('%') {
        percent = true;
        s = r.trim_end();
    }
    if s.is_empty() {
        return None;
    }
    let has_sep = s.contains(',');
    let digits: String = if has_sep {
        // Thousands separators must be in groups of three before the decimal point.
        let (int, frac) = s.split_once('.').map_or((s, None), |(a, b)| (a, Some(b)));
        let groups: Vec<&str> = int.split(',').collect();
        let first = groups.first()?;
        if first.is_empty() || first.len() > 3 || groups.iter().skip(1).any(|g| g.len() != 3) {
            return None;
        }
        let mut d = groups.concat();
        if let Some(f) = frac {
            d.push('.');
            d.push_str(f);
        }
        d
    } else {
        s.to_string()
    };
    let b = digits.as_bytes();
    if !b.iter().all(|c| c.is_ascii_digit() || matches!(c, b'.' | b'e' | b'E' | b'+' | b'-')) {
        return None;
    }
    if !b.first().is_some_and(|c| c.is_ascii_digit() || *c == b'.') {
        return None;
    }
    let mut n: f64 = digits.parse().ok()?;
    if !n.is_finite() {
        return None;
    }
    let sci = digits.contains(['e', 'E']);
    if percent {
        n /= 100.0;
    }
    if neg {
        n = -n;
    }
    let fmt = if currency {
        Some(if digits.contains('.') { "\"$\"#,##0.00" } else { "\"$\"#,##0" })
    } else if percent {
        Some(if digits.contains('.') { "0.00%" } else { "0%" })
    } else if sci {
        Some("0.00E+00")
    } else if has_sep {
        Some(if digits.contains('.') { "#,##0.00" } else { "#,##0" })
    } else {
        None
    };
    Some((n, fmt))
}

/// `1/2` (only with a whole part, else it's a date), `1 1/2`, `0 3/4`.
fn parse_fraction(t: &str) -> Option<f64> {
    let (whole, frac) = t.split_once(' ')?;
    let neg = whole.starts_with('-');
    let w: f64 = whole.trim_start_matches('-').parse().ok()?;
    let (a, b) = frac.trim().split_once('/')?;
    let a: f64 = a.parse().ok()?;
    let b: f64 = b.parse().ok()?;
    if b == 0.0 || !w.is_finite() {
        return None;
    }
    let v = w + a / b;
    Some(if neg { -v } else { v })
}

fn month_from_name(s: &str) -> Option<u32> {
    let l = s.to_ascii_lowercase();
    if l.len() < 3 {
        return None;
    }
    MONTHS.iter().position(|m| m.to_ascii_lowercase().starts_with(&l)).map(|i| i as u32 + 1)
}

/// Time part: `h:mm`, `h:mm:ss`, `h:mm:ss.000`, `h AM`, with optional AM/PM.
fn parse_time(t: &str) -> Option<(f64, bool, bool)> {
    let t = t.trim();
    let lower = t.to_ascii_lowercase();
    let (body, ampm) = if let Some(b) = lower.strip_suffix("am").or_else(|| lower.strip_suffix('a')) {
        (b.trim().to_string(), Some(false))
    } else if let Some(b) = lower.strip_suffix("pm").or_else(|| lower.strip_suffix('p')) {
        (b.trim().to_string(), Some(true))
    } else {
        (lower.clone(), None)
    };
    let parts: Vec<&str> = body.split(':').collect();
    if parts.len() == 1 && ampm.is_none() {
        return None;
    }
    if parts.len() > 3 || parts.iter().any(|p| p.is_empty()) {
        return None;
    }
    let mut h: f64 = parts.first()?.parse().ok()?;
    let m: f64 = parts.get(1).map_or(Some(0.0), |p| p.parse().ok())?;
    let s: f64 = parts.get(2).map_or(Some(0.0), |p| p.parse().ok())?;
    if m >= 60.0 || s >= 60.0 || h < 0.0 || m < 0.0 || s < 0.0 {
        return None;
    }
    if let Some(pm) = ampm {
        if !(0.0..=12.0).contains(&h) {
            return None;
        }
        if h == 12.0 {
            h = 0.0;
        }
        if pm {
            h += 12.0;
        }
    } else if h >= 10000.0 {
        return None;
    }
    Some((time_fraction(h, m, s), parts.len() == 3, ampm.is_some()))
}

fn parse_date(t: &str, sys: DateSystem) -> Option<(f64, &'static str)> {
    let t = t.trim().trim_end_matches(',');
    let num = |s: &str| s.trim().parse::<i64>().ok();
    // ISO yyyy-mm-dd or yyyy/mm/dd
    let seps: &[char] = &['/', '-', '.'];
    let parts: Vec<&str> = t.split(seps).collect();
    if parts.len() == 3 {
        let (a, b, c) = (parts[0].trim(), parts[1].trim(), parts[2].trim());
        if a.len() == 4
            && let (Some(y), Some(m), Some(d)) = (num(a), num(b), num(c))
        {
            return valid_ymd(sys, y, m, d).map(|s| (s, "yyyy-mm-dd"));
        }
        if let (Some(m), Some(d), Some(y)) = (num(a), num(b), num(c)) {
            let y = if c.len() <= 2 { if y < 30 { 2000 + y } else { 1900 + y } } else { y };
            return valid_ymd(sys, y, m, d).map(|s| (s, "m/d/yyyy"));
        }
        // 4-Mar-2026
        if let (Some(d), Some(m), Some(y)) = (num(a), month_from_name(b), num(c)) {
            let y = if c.len() <= 2 { if y < 30 { 2000 + y } else { 1900 + y } } else { y };
            return valid_ymd(sys, y, m as i64, d).map(|s| (s, "d-mmm-yy"));
        }
    }
    if parts.len() == 2 {
        let (a, b) = (parts[0].trim(), parts[1].trim());
        // m/d in the current year is ambiguous without a clock; use month/day of 2026? Excel uses
        // the current year. We accept d-mmm / mmm-d only with month names, and m/d → current year.
        if let (Some(m), Some(d)) = (num(a), num(b)) {
            let y = current_year();
            if (1..=12).contains(&m) && (1..=31).contains(&d) {
                return valid_ymd(sys, y, m, d).map(|s| (s, "d-mmm"));
            }
            // m/yyyy
            if (1..=12).contains(&m) && b.len() == 4 {
                return valid_ymd(sys, d, m, 1).map(|s| (s, "mmm-yy"));
            }
        }
        if let (Some(d), Some(m)) = (num(a), month_from_name(b)) {
            return valid_ymd(sys, current_year(), m as i64, d).map(|s| (s, "d-mmm"));
        }
        if let (Some(m), Some(y)) = (month_from_name(a), num(b)) {
            let y = if b.len() <= 2 { if y < 30 { 2000 + y } else { 1900 + y } } else { y };
            return valid_ymd(sys, y, m as i64, 1).map(|s| (s, "mmm-yy"));
        }
    }
    // "Mar 4, 2026" / "March 4 2026" / "4 March 2026"
    let words: Vec<&str> = t.split([' ', ',']).filter(|w| !w.is_empty()).collect();
    if words.len() == 3 {
        // Two-digit years follow the same 2029 rule as the dashed forms above.
        let expand = |w: &str, y: i64| if w.len() <= 2 { if y < 30 { 2000 + y } else { 1900 + y } } else { y };
        if let (Some(m), Some(d), Some(y)) = (month_from_name(words[0]), num(words[1]), num(words[2])) {
            let y = expand(words[2], y);
            return valid_ymd(sys, y, m as i64, d).map(|s| (s, "d-mmm-yy"));
        }
        if let (Some(d), Some(m), Some(y)) = (num(words[0]), month_from_name(words[1]), num(words[2])) {
            let y = expand(words[2], y);
            return valid_ymd(sys, y, m as i64, d).map(|s| (s, "d-mmm-yy"));
        }
    }
    None
}

fn valid_ymd(sys: DateSystem, y: i64, m: i64, d: i64) -> Option<f64> {
    // Dates before 1900 stay text (`DATE()` would add 1900 to such a year).
    if !(1..=12).contains(&m) || d < 1 || y < 1900 {
        return None;
    }
    let max = crate::date::days_in_month_in(sys, y as i32, m as u32) as i64;
    if d > max {
        return None;
    }
    serial_from_ymd(sys, y, m, d)
}

/// The year used for dates typed without one, from the system clock (browser clock on wasm).
/// Falls back to 2026 only when no clock is available.
pub fn current_year() -> i64 {
    if let Some(ms) = crate::date::unix_now_ms() {
        let days = (ms / 86_400_000.0).floor() as i64;
        return crate::date::civil_from_days(days).0 as i64;
    }
    2026
}

fn parse_datetime(t: &str, sys: DateSystem) -> Option<(f64, Option<&'static str>)> {
    if let Some((tf, secs, ampm)) = parse_time(t) {
        let f = match (secs, ampm) {
            // 24 hours or more: elapsed hours, so 25:00 shows as 25:00:00, not 1:00.
            _ if tf >= 1.0 => "[h]:mm:ss",
            (false, false) => "h:mm",
            (true, false) => "h:mm:ss",
            (false, true) => "h:mm AM/PM",
            (true, true) => "h:mm:ss AM/PM",
        };
        return Some((tf, Some(f)));
    }
    if let Some((d, f)) = parse_date(t, sys) {
        return Some((d, Some(f)));
    }
    // date + time: split at the last space(s) before a time-looking tail.
    for (i, _) in t.match_indices(' ') {
        let (a, b) = (t.get(..i)?, t.get(i + 1..)?);
        if let (Some((d, _)), Some((tf, _, _))) = (parse_date(a, sys), parse_time(b)) {
            return Some((d + tf, Some("m/d/yyyy h:mm")));
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(s: &str) -> Parsed {
        parse_input(s, DateSystem::D1900)
    }

    #[test]
    fn month_name_short_year() {
        for s in ["Mar 4, 26", "Mar 4 26", "March 4 26", "4 March 26", "Mar 4, 2026"] {
            assert_eq!(p(s).value, Value::Number(46085.0), "{s}");
        }
        assert_eq!(p("Mar 4, 99").value, p("Mar 4, 1999").value);
        assert_eq!(p("Mar 4, 1776").value, Value::text("Mar 4, 1776"));
        assert_eq!(p("Feb 30, 26").value, Value::text("Feb 30, 26"));
    }

    #[test]
    fn numbers() {
        assert_eq!(p("42").value, Value::Number(42.0));
        assert_eq!(p("-3.5").value, Value::Number(-3.5));
        assert_eq!(p("1,234").format, Some("#,##0"));
        assert_eq!(p("1,234").value, Value::Number(1234.0));
        assert_eq!(p("1,23").value, Value::text("1,23"));
        assert_eq!(p("12%").value, Value::Number(0.12));
        assert_eq!(p("$1,200.50").value, Value::Number(1200.5));
        assert_eq!(p("$1,200.50").format, Some("\"$\"#,##0.00"));
        assert_eq!(p("(100)").value, Value::Number(-100.0));
        assert_eq!(p("1e3").value, Value::Number(1000.0));
        assert_eq!(p("1 1/2").value, Value::Number(1.5));
        assert_eq!(p(".5").value, Value::Number(0.5));
    }

    #[test]
    fn texts_and_specials() {
        assert_eq!(p("hello").value, Value::text("hello"));
        assert_eq!(p("'123").value, Value::text("123"));
        assert_eq!(p("true").value, Value::Bool(true));
        assert_eq!(p("#N/A").value, Value::Error(CellError::NA));
        assert_eq!(p("e").value, Value::text("e"));
        assert_eq!(p("-").value, Value::text("-"));
        assert_eq!(p("1.2.3.4").value, Value::text("1.2.3.4"));
    }

    #[test]
    fn dates_and_times() {
        assert_eq!(p("10/7/2026").value, Value::Number(46302.0));
        assert_eq!(p("2026-10-07").value, Value::Number(46302.0));
        assert_eq!(p("7-Oct-2026").value, Value::Number(46302.0));
        assert_eq!(p("Oct 7, 2026").value, Value::Number(46302.0));
        assert_eq!(p("2/30/2026").value, Value::text("2/30/2026"));
        assert_eq!(p("7/4/1776").value, Value::text("7/4/1776"));
        assert_eq!(p("1776-07-04").value, Value::text("1776-07-04"));
        assert_eq!(p("4-Jul-1776").value, Value::text("4-Jul-1776"));
        assert_eq!(p("12/31/1899").value, Value::text("12/31/1899"));
        assert_eq!(p("1/1/1900").value, Value::Number(1.0));
        assert_eq!(p("12:00").value, Value::Number(0.5));
        assert_eq!(p("6:00 PM").value, Value::Number(0.75));
        assert_eq!(p("12:00 AM").value, Value::Number(0.0));
        assert_eq!(p("10/7/2026 18:00").value, Value::Number(46302.75));
        assert_eq!(p("25:00").value, Value::Number(25.0 / 24.0));
        assert_eq!(p("25:00").format, Some("[h]:mm:ss"));
        assert_eq!(p("23:59").format, Some("h:mm"));
    }

    /// What Excel 365 (de-DE regional format) made of typed entries, observed through its
    /// automation interface on 2026-10-13: value and automatic format (canonical code).
    #[test]
    fn german_entries_read_as_excel_reads_them() {
        let de = NumberLocale::DE_DE;
        let year = current_year();
        let date = |y: i64, m: i64, d: i64| serial_from_ymd(DateSystem::D1900, y, m, d).unwrap();
        let num = |s: &str| parse_input_in(s, DateSystem::D1900, &de);
        for (input, value, format) in [
            ("1,5", 1.5, None),
            ("1.234,5", 1234.5, Some("#,##0.00")),
            ("1.234", 1234.0, Some("#,##0")),
            ("1,234", 1.234, None),
            ("-3,2%", -0.032, Some("0.00%")),
            ("12%", 0.12, Some("0%")),
            ("12,5 %", 0.125, Some("0.00%")),
            ("0,5%", 0.005, Some("0.00%")),
            ("1,5E+3", 1500.0, Some("0.00E+00")),
            ("1,5e3", 1500.0, Some("0.00E+00")),
            ("1E3", 1000.0, Some("0.00E+00")),
            ("1,23E-5", 1.23e-5, Some("0.00E+00")),
            ("1.234.567,89", 1234567.89, Some("#,##0.00")),
            ("-1.234,5", -1234.5, Some("#,##0.00")),
            ("1.234,", 1234.0, Some("#,##0.00")),
            ("1,", 1.0, None),
            ("0,5", 0.5, None),
            (",5", 0.5, None),
            ("+1,5", 1.5, None),
            ("- 1,5", -1.5, None),
            ("(100)", -100.0, None),
            ("(1,5)", -1.5, None),
            ("001,5", 1.5, None),
            ("1.000.000", 1_000_000.0, Some("#,##0")),
            ("2026", 2026.0, None),
            ("1 1/2", 1.5, Some("# ?/?")),
            ("5 €", 5.0, Some("#,##0 \"€\";[Red]-#,##0 \"€\"")),
            ("5€", 5.0, Some("#,##0 \"€\";[Red]-#,##0 \"€\"")),
            ("-5 €", -5.0, Some("#,##0 \"€\";[Red]-#,##0 \"€\"")),
            ("5,50 €", 5.5, Some("#,##0.00 \"€\";[Red]-#,##0.00 \"€\"")),
            ("1.234,56 €", 1234.56, Some("#,##0.00 \"€\";[Red]-#,##0.00 \"€\"")),
            ("€5", 5.0, None),
            ("13.10.2026", date(2026, 10, 13), Some("m/d/yyyy")),
            ("13.10.26", date(2026, 10, 13), Some("m/d/yyyy")),
            ("1.5.26", date(2026, 5, 1), Some("m/d/yyyy")),
            ("01.05.2026", date(2026, 5, 1), Some("m/d/yyyy")),
            ("13.10.29", date(2029, 10, 13), Some("m/d/yyyy")),
            ("13.10.30", date(1930, 10, 13), Some("m/d/yyyy")),
            ("13.10.0", date(2000, 10, 13), Some("m/d/yyyy")),
            ("1.2.3", date(2003, 2, 1), Some("m/d/yyyy")),
            ("13/10/2026", date(2026, 10, 13), Some("m/d/yyyy")),
            ("13-10-2026", date(2026, 10, 13), Some("m/d/yyyy")),
            ("3/4/2026", date(2026, 4, 3), Some("m/d/yyyy")),
            ("13. 10. 2026", date(2026, 10, 13), Some("m/d/yyyy")),
            ("2026-10-13", date(2026, 10, 13), Some("m/d/yyyy")),
            ("2026/10/13", date(2026, 10, 13), Some("m/d/yyyy")),
            ("2026.10.13", date(2026, 10, 13), Some("m/d/yyyy")),
            ("29.2.1900", 60.0, Some("m/d/yyyy")),
            ("13.10", date(year, 10, 13), Some("d-mmm")),
            ("1.5", date(year, 5, 1), Some("d-mmm")),
            ("31.12", date(year, 12, 31), Some("d-mmm")),
            ("1.13", date(2013, 1, 1), Some("mmm-yy")),
            ("12.13", date(2013, 12, 1), Some("mmm-yy")),
            ("10/2026", date(2026, 10, 1), Some("mmm-yy")),
            ("13-Okt-2026", date(2026, 10, 13), Some("d-mmm-yy")),
            ("13. Okt 2026", date(2026, 10, 13), Some("d-mmm-yy")),
            ("13. Oktober 2026", date(2026, 10, 13), Some("d-mmm-yy")),
            ("13. Okt. 2026", date(2026, 10, 13), Some("d-mmm-yy")),
            ("13. März 2026", date(2026, 3, 13), Some("d-mmm-yy")),
            ("13. Mär 2026", date(2026, 3, 13), Some("d-mmm-yy")),
            ("13-Mrz-2026", date(2026, 3, 13), Some("d-mmm-yy")),
            ("3. Juni 2026", date(2026, 6, 3), Some("d-mmm-yy")),
            ("Okt 26", date(2026, 10, 1), Some("mmm-yy")),
            ("Okt. 2026", date(2026, 10, 1), Some("mmm-yy")),
            ("Oktober 2026", date(2026, 10, 1), Some("mmm-yy")),
            ("Sept 26", date(2026, 9, 1), Some("mmm-yy")),
            ("Mär 26", date(2026, 3, 1), Some("mmm-yy")),
            ("13. Okt", date(year, 10, 13), Some("d-mmm")),
            ("13.Okt", date(year, 10, 13), Some("d-mmm")),
            ("13 Okt", date(year, 10, 13), Some("d-mmm")),
            ("14:30", 14.5 / 24.0, Some("h:mm")),
            ("14:30:15", (14.0 * 3600.0 + 30.0 * 60.0 + 15.0) / 86400.0, Some("h:mm:ss")),
            ("25:00", 25.0 / 24.0, Some("[h]:mm:ss")),
            ("2:30 PM", 14.5 / 24.0, Some("h:mm AM/PM")),
            ("13.10.2026 14:30", date(2026, 10, 13) + 14.5 / 24.0, Some("m/d/yyyy h:mm")),
            ("13.10.26 14:30", date(2026, 10, 13) + 14.5 / 24.0, Some("m/d/yyyy h:mm")),
            ("13. Okt 2026 14:30", date(2026, 10, 13) + 14.5 / 24.0, Some("m/d/yyyy h:mm")),
            ("13.10.2026 2:30 PM", date(2026, 10, 13) + 14.5 / 24.0, Some("m/d/yyyy h:mm")),
        ] {
            let p = num(input);
            let Value::Number(n) = p.value else { panic!("{input}: {:?}", p.value) };
            assert!((n - value).abs() < 1e-9, "{input}: {n} != {value}");
            assert_eq!(p.format, format, "{input}");
        }
        // Text in Excel de-DE, although some of them read as numbers or dates in en-US.
        for input in [
            "13.10.",
            "1.10.",
            "1.5.",
            "31.2.2026",
            "29.2.1900x",
            "13.10. 14:30",
            "10/13/2026",
            "13.13",
            "1.0",
            "1.00",
            "1.0.0",
            "1,5-",
            "1,5%%",
            "1,5E",
            "E5",
            "1e400",
            "1 234,5",
            "5 EUR",
            "$5",
            "5 $",
            "13 10 2026",
            "13:30 Uhr",
            "13.10.1899",
            "Okt",
            "abc",
            "1.2.3.4",
            "13.10.2026,5",
            "Oct 26",
        ] {
            assert_eq!(num(input).value, Value::text(input), "{input}");
        }
        // Canonical words stay what they are; the formula language's own words are the UI's.
        assert_eq!(num("TRUE").value, Value::Bool(true));
        assert_eq!(num("#N/A").value, Value::Error(CellError::NA));
        assert_eq!(num("'1,5").value, Value::text("1,5"));
        assert_eq!(num("").value, Value::Empty);
        // The canonical locale reads as `parse_input`.
        assert_eq!(parse_input_in("1,234", DateSystem::D1900, &NumberLocale::EN_US), p("1,234"));
        // Other separators (File › Options › Advanced) with the region's dates.
        let swiss = de.with_separators('.', '\'');
        assert_eq!(parse_input_in("1'234.5", DateSystem::D1900, &swiss).value, Value::Number(1234.5));
        assert_eq!(parse_input_in("13.10.2026", DateSystem::D1900, &swiss).value, Value::Number(date(2026, 10, 13)));
    }

    #[test]
    fn regional_parsing_survives_random_input() {
        // Random strings from the characters German numbers and dates are made of.
        let alphabet: Vec<char> = "0123456789.,;:-+ %€$()/eE'\u{a0}OktMärzJanuar".chars().collect();
        let mut seed: u64 = 0x0dde_c0de;
        let mut next = |n: usize| {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            (seed % n.max(1) as u64) as usize
        };
        let swiss = NumberLocale::DE_DE.with_separators('.', '\'');
        for _ in 0..20_000 {
            let len = next(16);
            let s: String = (0..len).map(|_| alphabet[next(alphabet.len())]).collect();
            for loc in [NumberLocale::DE_DE, swiss] {
                if let Value::Number(n) = parse_input_in(&s, DateSystem::D1900, &loc).value {
                    assert!(n.is_finite(), "{s:?}");
                }
            }
        }
    }

    #[test]
    fn regional_parsing_never_panics_on_odd_input() {
        let de = NumberLocale::DE_DE;
        let long = "1.".repeat(10_000);
        for input in [
            "",
            " ",
            ".",
            ",",
            "-",
            "--1",
            "(",
            "()",
            "((1))",
            "€",
            "€€",
            "%",
            "E",
            "1E",
            "1E+",
            ".:.",
            "1..2",
            "..",
            "1.2.",
            "...",
            "Okt Okt Okt",
            "13. . 2026",
            "99999999999.1.1",
            "1.99999999",
            "\u{0}",
            "١٢٣",
            long.as_str(),
        ] {
            let _ = parse_input_in(input, DateSystem::D1900, &de);
            let _ = parse_input_in(input, DateSystem::D1904, &de.with_separators('.', ' '));
        }
    }

    #[test]
    fn fictitious_1900_02_29() {
        // Excel accepts 29 February 1900 (serial 60) and keeps 1 March 1900 at 61.
        assert_eq!(p("2/29/1900").value, Value::Number(60.0));
        assert_eq!(p("1900-02-29").value, Value::Number(60.0));
        assert_eq!(p("29-Feb-1900").value, Value::Number(60.0));
        assert_eq!(p("2/28/1900").value, Value::Number(59.0));
        assert_eq!(p("3/1/1900").value, Value::Number(61.0));
        assert_eq!(p("2/29/1901").value, Value::text("2/29/1901"));
        assert_eq!(p("2/30/1900").value, Value::text("2/30/1900"));
        // The 1904 system starts after it.
        assert_eq!(parse_input("2/29/1900", DateSystem::D1904).value, Value::text("2/29/1900"));
        assert_eq!(parse_input("2/29/1904", DateSystem::D1904).value, Value::Number(59.0));
    }
}
