//! ODF data styles (`number:number-style`, `number:date-style`…) ⇄ number format codes.
//! Format reference: ODF 1.3 part 3, section 16.29 (data styles).

use std::fmt::Write as _;

use crate::xml::{esc, esc_attr};

const MAX_DIGITS: u32 = 30;

/// One data style as read: its code (for the main section), text colour and conditional maps.
#[derive(Clone, Debug, Default)]
pub(super) struct DataStyle {
    pub kind: Kind,
    pub code: String,
    pub color: Option<String>,
    /// `style:map`: (condition such as `value()>=0`, style name).
    pub maps: Vec<(String, String)>,
    /// `number:truncate-on-overflow="false"`: hours (or minutes) grow past 24 (60).
    pub elapsed: bool,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) enum Kind {
    #[default]
    Number,
    Currency,
    Percentage,
    Date,
    Time,
    Boolean,
    Text,
}

impl Kind {
    pub fn from_element(local: &[u8]) -> Option<Kind> {
        Some(match local {
            b"number-style" => Kind::Number,
            b"currency-style" => Kind::Currency,
            b"percentage-style" => Kind::Percentage,
            b"date-style" => Kind::Date,
            b"time-style" => Kind::Time,
            b"boolean-style" => Kind::Boolean,
            b"text-style" => Kind::Text,
            _ => return None,
        })
    }
}

fn digits(n: Option<u32>) -> u32 {
    n.unwrap_or(0).min(MAX_DIGITS)
}

/// `number:number` → `#,##0.00`; `None` for the General format.
pub(super) fn number_part(decimals: Option<u32>, min_decimals: Option<u32>, min_int: Option<u32>, grouping: bool) -> String {
    if decimals.is_none() && min_decimals.is_none() && !grouping && min_int.unwrap_or(1) <= 1 {
        return "General".into();
    }
    let mut int: String = "0".repeat(digits(min_int) as usize);
    if grouping {
        while int.len() < 4 {
            int.insert(0, '#');
        }
        let at = int.len() - 3;
        int.insert(at, ',');
    } else if int.is_empty() {
        int.push('#');
    }
    let decimals = digits(decimals.or(min_decimals));
    let min = digits(min_decimals.or(Some(decimals))).min(decimals);
    if decimals > 0 {
        int.push('.');
        int.push_str(&"0".repeat(min as usize));
        int.push_str(&"#".repeat((decimals - min) as usize));
    }
    int
}

pub(super) fn scientific_part(decimals: Option<u32>, min_int: Option<u32>, exp: Option<u32>) -> String {
    let mut s = "0".repeat(digits(min_int).max(1) as usize);
    let d = digits(decimals);
    if d > 0 {
        s.push('.');
        s.push_str(&"0".repeat(d as usize));
    }
    s.push_str("E+");
    s.push_str(&"0".repeat(digits(exp).max(1) as usize));
    s
}

pub(super) fn fraction_part(min_int: Option<u32>, num: Option<u32>, den: Option<u32>, den_value: Option<u32>) -> String {
    let mut s = String::new();
    if min_int.is_some() {
        s.push_str(if digits(min_int) > 0 { "0 " } else { "# " });
    }
    s.push_str(&"?".repeat(digits(num).max(1) as usize));
    s.push('/');
    match den_value {
        Some(v) if v > 0 => {
            let _ = write!(s, "{v}");
        }
        _ => s.push_str(&"?".repeat(digits(den).max(1) as usize)),
    }
    s
}

/// A literal (`number:text`, `number:currency-symbol`) in format-code syntax.
pub(super) fn literal(text: &str, kind: Kind) -> String {
    let datetime = matches!(kind, Kind::Date | Kind::Time);
    let mut out = String::new();
    let mut run = String::new();
    let flush = |out: &mut String, run: &mut String| {
        if !run.is_empty() {
            let _ = write!(out, "\"{run}\"");
            run.clear();
        }
    };
    for ch in text.chars() {
        let raw = matches!(ch, ' ' | '-' | '(' | ')' | '+' | '$') || (datetime && matches!(ch, ':' | '/')) || (kind == Kind::Percentage && ch == '%');
        if raw {
            flush(&mut out, &mut run);
            out.push(ch);
        } else if ch == '"' {
            flush(&mut out, &mut run);
            out.push_str("\\\"");
        } else {
            run.push(ch);
        }
    }
    flush(&mut out, &mut run);
    out
}

/// `[Red]` and the other format-code colours for a `fo:color`.
fn color_code(hex: &str) -> Option<&'static str> {
    Some(match hex.trim_start_matches('#').to_ascii_lowercase().as_str() {
        "000000" => "[Black]",
        "ff0000" => "[Red]",
        "00ff00" => "[Green]",
        "0000ff" => "[Blue]",
        "ffff00" => "[Yellow]",
        "ff00ff" => "[Magenta]",
        "00ffff" => "[Cyan]",
        "ffffff" => "[White]",
        _ => return None,
    })
}

fn section(style: &DataStyle) -> String {
    let mut code = style.color.as_deref().and_then(color_code).unwrap_or("").to_string();
    code.push_str(&style.code);
    if code.is_empty() { "General".into() } else { code }
}

/// The number format code of data style `name`, following its conditional maps one level.
pub(super) fn code_for(name: &str, lookup: &dyn Fn(&str) -> Option<DataStyle>) -> Option<String> {
    let main = lookup(name)?;
    if main.kind == Kind::Boolean {
        return None;
    }
    let mapped: Vec<(String, String)> = main.maps.iter().filter_map(|(cond, n)| lookup(n).map(|s| (cond.replace(' ', ""), section(&s)))).collect();
    let main_code = section(&main);
    let code = match mapped.as_slice() {
        [] => main_code,
        [(c, p)] if c == "value()>=0" => format!("{p};{main_code}"),
        [(c1, p), (c2, n)] if c1 == "value()>0" && c2 == "value()<0" => format!("{p};{n};{main_code}"),
        sections => {
            let mut parts: Vec<String> = sections
                .iter()
                .take(2)
                .map(|(cond, code)| match cond.strip_prefix("value()") {
                    Some(op) => format!("[{op}]{code}"),
                    None => code.clone(),
                })
                .collect();
            parts.push(main_code);
            parts.join(";")
        }
    };
    (code != "General").then_some(code)
}

// ---------------------------------------------------------------- writing

#[derive(Clone, Debug, PartialEq)]
enum Part {
    Literal(String),
    Currency(String),
    Number { decimals: u32, min_decimals: u32, min_int: u32, grouping: bool, general: bool },
    Scientific { decimals: u32, min_int: u32, exp: u32 },
    Fraction { min_int: Option<u32>, num: u32, den: u32, den_value: Option<u32> },
    Year(bool),
    Month { long: bool, textual: bool },
    Day(bool),
    Weekday(bool),
    Hours(bool),
    Minutes(bool),
    Seconds(bool, u32),
    AmPm,
    TextContent,
}

#[derive(Default)]
struct Section {
    parts: Vec<Part>,
    color: Option<&'static str>,
    condition: Option<String>,
    percent: bool,
    elapsed: bool,
}

fn split_sections(code: &str) -> Vec<String> {
    let mut out = vec![String::new()];
    let mut quoted = false;
    let mut bracket = false;
    let mut escape = false;
    for ch in code.chars() {
        let cur = out.last_mut();
        if escape {
            escape = false;
        } else if quoted {
            quoted = ch != '"';
        } else if bracket {
            bracket = ch != ']';
        } else if ch == '"' {
            quoted = true;
        } else if ch == '[' {
            bracket = true;
        } else if ch == '\\' {
            escape = true;
        } else if ch == ';' {
            out.push(String::new());
            continue;
        }
        if let Some(cur) = cur {
            cur.push(ch);
        }
    }
    out
}

fn color_hex(name: &str) -> Option<&'static str> {
    Some(match name.to_ascii_lowercase().as_str() {
        "black" => "#000000",
        "red" => "#ff0000",
        "green" => "#00ff00",
        "blue" => "#0000ff",
        "yellow" => "#ffff00",
        "magenta" => "#ff00ff",
        "cyan" => "#00ffff",
        "white" => "#ffffff",
        _ => return None,
    })
}

fn push_literal(parts: &mut Vec<Part>, text: &str) {
    if let Some(Part::Literal(prev)) = parts.last_mut() {
        prev.push_str(text);
    } else {
        parts.push(Part::Literal(text.to_string()));
    }
}

fn parse_section(src: &str) -> Section {
    let chars: Vec<char> = src.chars().collect();
    let mut sec = Section::default();
    let mut i = 0usize;
    let at = |i: usize| chars.get(i).copied();
    while let Some(c) = at(i) {
        i += 1;
        match c {
            '"' => {
                let mut text = String::new();
                while let Some(ch) = at(i) {
                    i += 1;
                    if ch == '"' {
                        break;
                    }
                    text.push(ch);
                }
                push_literal(&mut sec.parts, &text);
            }
            '\\' => {
                if let Some(ch) = at(i) {
                    i += 1;
                    push_literal(&mut sec.parts, ch.encode_utf8(&mut [0; 4]));
                }
            }
            '_' => {
                i += 1;
                push_literal(&mut sec.parts, " ");
            }
            '*' => i += 1,
            '[' => {
                let mut inner = String::new();
                while let Some(ch) = at(i) {
                    i += 1;
                    if ch == ']' {
                        break;
                    }
                    inner.push(ch);
                }
                let lower = inner.to_ascii_lowercase();
                if let Some(hex) = color_hex(&inner) {
                    sec.color = Some(hex);
                } else if let Some(cur) = inner.strip_prefix('$') {
                    let symbol = cur.split('-').next().unwrap_or("");
                    if !symbol.is_empty() {
                        sec.parts.push(Part::Currency(symbol.to_string()));
                    }
                } else if inner.starts_with(['<', '>', '=']) {
                    sec.condition = Some(format!("value(){inner}"));
                } else if !lower.is_empty()
                    && lower.chars().all(|ch| ch == lower.chars().next().unwrap_or('x'))
                    && matches!(lower.chars().next(), Some('h' | 'm' | 's'))
                {
                    sec.elapsed = true;
                    let long = lower.len() > 1;
                    sec.parts.push(match lower.chars().next() {
                        Some('h') => Part::Hours(long),
                        Some('m') => Part::Minutes(long),
                        _ => Part::Seconds(long, 0),
                    });
                }
            }
            '0' | '#' | '?' => number_run(&chars, &mut i, &mut sec),
            '.' if matches!(at(i), Some('0' | '#' | '?')) => number_run(&chars, &mut i, &mut sec),
            '%' => {
                sec.percent = true;
                push_literal(&mut sec.parts, "%");
            }
            '@' => sec.parts.push(Part::TextContent),
            c if c.eq_ignore_ascii_case(&'g')
                && chars.get(i - 1..i + 6).is_some_and(|w| w.iter().collect::<String>().eq_ignore_ascii_case("general")) =>
            {
                i += 6;
                sec.parts.push(Part::Number { decimals: 0, min_decimals: 0, min_int: 1, grouping: false, general: true });
            }
            c if c.eq_ignore_ascii_case(&'a')
                && chars.get(i - 1..i + 4).is_some_and(|w| w.iter().collect::<String>().eq_ignore_ascii_case("AM/PM")) =>
            {
                i += 4;
                sec.parts.push(Part::AmPm);
            }
            c if c.eq_ignore_ascii_case(&'a')
                && chars.get(i - 1..i + 2).is_some_and(|w| w.iter().collect::<String>().eq_ignore_ascii_case("A/P")) =>
            {
                i += 2;
                sec.parts.push(Part::AmPm);
            }
            c if matches!(c.to_ascii_lowercase(), 'y' | 'm' | 'd' | 'h' | 's' | 'e') => {
                let lower = c.to_ascii_lowercase();
                let mut n = 1usize;
                while at(i).is_some_and(|ch| ch.to_ascii_lowercase() == lower) {
                    n += 1;
                    i += 1;
                }
                sec.parts.push(match lower {
                    'y' | 'e' => Part::Year(n > 2 || lower == 'e'),
                    'm' => Part::Month { long: n == 2 || n >= 4, textual: n >= 3 },
                    'd' if n >= 3 => Part::Weekday(n >= 4),
                    'd' => Part::Day(n >= 2),
                    'h' => Part::Hours(n >= 2),
                    _ => {
                        let mut dec = 0;
                        if at(i) == Some('.') && at(i + 1) == Some('0') {
                            i += 1;
                            while at(i) == Some('0') {
                                dec += 1;
                                i += 1;
                            }
                        }
                        Part::Seconds(n >= 2, dec)
                    }
                });
            }
            other => push_literal(&mut sec.parts, other.encode_utf8(&mut [0; 4])),
        }
    }
    // `m` after hours or before seconds means minutes.
    let times: Vec<usize> = sec.parts.iter().enumerate().filter(|(_, p)| !matches!(p, Part::Literal(_))).map(|(i, _)| i).collect();
    for (k, &idx) in times.iter().enumerate() {
        if let Some(Part::Month { long, textual: false }) = sec.parts.get(idx).cloned() {
            let prev = k.checked_sub(1).and_then(|p| times.get(p)).and_then(|&p| sec.parts.get(p));
            let next = times.get(k + 1).and_then(|&n| sec.parts.get(n));
            if (matches!(prev, Some(Part::Hours(_))) || matches!(next, Some(Part::Seconds(..))))
                && let Some(slot) = sec.parts.get_mut(idx)
            {
                *slot = Part::Minutes(long);
            }
        }
    }
    sec
}

/// A run of digit placeholders starting at `*i - 1`: a number, scientific or fraction part.
fn number_run(chars: &[char], i: &mut usize, sec: &mut Section) {
    let at = |i: usize| chars.get(i).copied();
    let mut i_ = *i;
    let start = i_ - 1;
    while at(i_).is_some_and(|ch| matches!(ch, '0' | '#' | '?' | '.' | ',')) {
        i_ += 1;
    }
    let run: String = chars.get(start..i_).unwrap_or_default().iter().collect();
    let (int, frac) = run.split_once('.').map_or((run.as_str(), ""), |(a, b)| (a, b));
    let int_digits = |s: &str| s.chars().filter(|ch| *ch == '0').count() as u32;
    let decimals = frac.chars().filter(|ch| matches!(ch, '0' | '#' | '?')).count() as u32;
    // Scientific notation: `0.00E+00`.
    if matches!(at(i_), Some('E' | 'e')) && matches!(at(i_ + 1), Some('+' | '-')) {
        i_ += 2;
        let mut exp = 0;
        while at(i_).is_some_and(|ch| matches!(ch, '0' | '#')) {
            exp += 1;
            i_ += 1;
        }
        sec.parts.push(Part::Scientific {
            decimals: decimals.min(MAX_DIGITS),
            min_int: int_digits(int).clamp(1, MAX_DIGITS),
            exp: exp.clamp(1, MAX_DIGITS),
        });
        *i = i_;
        return;
    }
    // Fraction: `# ?/?` (an integer part, then numerator/denominator).
    if at(i_) == Some('/') && frac.is_empty() {
        i_ += 1;
        let mut den = 0;
        let mut value = String::new();
        while let Some(ch) = at(i_) {
            if matches!(ch, '?' | '#' | '0') {
                den += 1;
            } else if ch.is_ascii_digit() {
                value.push(ch);
            } else {
                break;
            }
            i_ += 1;
        }
        // An integer part before the numerator was pushed as its own number.
        let min_int = match sec.parts.last() {
            Some(Part::Literal(l)) if l == " " => {
                sec.parts.pop();
                match sec.parts.pop() {
                    Some(Part::Number { min_int, .. }) => Some(min_int),
                    Some(other) => {
                        sec.parts.push(other);
                        None
                    }
                    None => None,
                }
            }
            _ => None,
        };
        sec.parts.push(Part::Fraction {
            min_int,
            num: (run.len() as u32).clamp(1, MAX_DIGITS),
            den: den.clamp(1, MAX_DIGITS),
            den_value: value.parse().ok(),
        });
        *i = i_;
        return;
    }
    let grouping = int.trim_end_matches(',').contains(',');
    sec.parts.push(Part::Number {
        decimals: decimals.min(MAX_DIGITS),
        min_decimals: frac.chars().filter(|ch| *ch == '0').count().min(MAX_DIGITS as usize) as u32,
        min_int: int_digits(int).min(MAX_DIGITS),
        grouping,
        general: false,
    });
    *i = i_;
}

fn kind_of(sec: &Section) -> Kind {
    let has = |f: &dyn Fn(&Part) -> bool| sec.parts.iter().any(f);
    if has(&|p| matches!(p, Part::Year(_) | Part::Month { .. } | Part::Day(_) | Part::Weekday(_))) {
        Kind::Date
    } else if has(&|p| matches!(p, Part::Hours(_) | Part::Minutes(_) | Part::Seconds(..) | Part::AmPm)) {
        Kind::Time
    } else if has(&|p| matches!(p, Part::TextContent)) {
        Kind::Text
    } else if sec.percent {
        Kind::Percentage
    } else if has(&|p| matches!(p, Part::Currency(_))) {
        Kind::Currency
    } else {
        Kind::Number
    }
}

fn style_xml(name: &str, sec: &Section, maps: &[(String, String)], volatile: bool) -> String {
    let kind = kind_of(sec);
    let element = match kind {
        Kind::Number | Kind::Boolean => "number:number-style",
        Kind::Currency => "number:currency-style",
        Kind::Percentage => "number:percentage-style",
        Kind::Date => "number:date-style",
        Kind::Time => "number:time-style",
        Kind::Text => "number:text-style",
    };
    let mut s = format!("<{element} style:name=\"{}\"", esc_attr(name));
    if volatile {
        s.push_str(" style:volatile=\"true\"");
    }
    if sec.elapsed && kind == Kind::Time {
        s.push_str(" number:truncate-on-overflow=\"false\"");
    }
    s.push('>');
    if let Some(hex) = sec.color {
        let _ = write!(s, "<style:text-properties fo:color=\"{hex}\"/>");
    }
    let long = |l: bool| if l { " number:style=\"long\"" } else { "" };
    for part in &sec.parts {
        match part {
            Part::Literal(t) => {
                let _ = write!(s, "<number:text>{}</number:text>", esc(t));
            }
            Part::Currency(t) => {
                let _ = write!(s, "<number:currency-symbol>{}</number:currency-symbol>", esc(t));
            }
            Part::Number { general: true, .. } => s.push_str("<number:number number:min-integer-digits=\"1\"/>"),
            Part::Number { decimals, min_decimals, min_int, grouping, .. } => {
                let _ = write!(
                    s,
                    "<number:number number:decimal-places=\"{decimals}\" number:min-decimal-places=\"{min_decimals}\" number:min-integer-digits=\"{min_int}\"{}/>",
                    if *grouping { " number:grouping=\"true\"" } else { "" }
                );
            }
            Part::Scientific { decimals, min_int, exp } => {
                let _ = write!(
                    s,
                    "<number:scientific-number number:decimal-places=\"{decimals}\" number:min-integer-digits=\"{min_int}\" number:min-exponent-digits=\"{exp}\"/>"
                );
            }
            Part::Fraction { min_int, num, den, den_value } => {
                s.push_str("<number:fraction");
                if let Some(m) = min_int {
                    let _ = write!(s, " number:min-integer-digits=\"{m}\"");
                }
                let _ = write!(s, " number:min-numerator-digits=\"{num}\" number:min-denominator-digits=\"{den}\"");
                if let Some(v) = den_value {
                    let _ = write!(s, " number:denominator-value=\"{v}\"");
                }
                s.push_str("/>");
            }
            Part::Year(l) => {
                let _ = write!(s, "<number:year{}/>", long(*l));
            }
            Part::Month { long: l, textual } => {
                let _ = write!(s, "<number:month{}{}/>", long(*l), if *textual { " number:textual=\"true\"" } else { "" });
            }
            Part::Day(l) => {
                let _ = write!(s, "<number:day{}/>", long(*l));
            }
            Part::Weekday(l) => {
                let _ = write!(s, "<number:day-of-week{}/>", long(*l));
            }
            Part::Hours(l) => {
                let _ = write!(s, "<number:hours{}/>", long(*l));
            }
            Part::Minutes(l) => {
                let _ = write!(s, "<number:minutes{}/>", long(*l));
            }
            Part::Seconds(l, dec) => {
                let _ =
                    write!(s, "<number:seconds{}{}/>", long(*l), if *dec > 0 { format!(" number:decimal-places=\"{dec}\"") } else { String::new() });
            }
            Part::AmPm => s.push_str("<number:am-pm/>"),
            Part::TextContent => s.push_str("<number:text-content/>"),
        }
    }
    for (cond, target) in maps {
        let _ = write!(s, "<style:map style:condition=\"{}\" style:apply-style-name=\"{}\"/>", esc_attr(cond), esc_attr(target));
    }
    let _ = write!(s, "</{element}>");
    s
}

/// Data style elements for number format `code` named `name` (with `{name}P{n}` for the
/// sections of a multi-section code). `None` for General.
pub(super) fn data_style_xml(name: &str, code: &str) -> Option<String> {
    if code.trim().is_empty() || code.eq_ignore_ascii_case("general") {
        return None;
    }
    let mut sections: Vec<Section> = split_sections(code).iter().take(3).map(|s| parse_section(s)).collect();
    let main = sections.pop()?;
    let mut out = String::new();
    let mut maps = Vec::new();
    let count = sections.len();
    for (i, sec) in sections.iter().enumerate() {
        let sub = format!("{name}P{i}");
        out.push_str(&style_xml(&sub, sec, &[], true));
        let cond = sec.condition.clone().unwrap_or_else(|| {
            match (count, i) {
                (1, _) => "value()>=0",
                (_, 0) => "value()>0",
                _ => "value()<0",
            }
            .to_string()
        });
        maps.push((cond, sub));
    }
    out.push_str(&style_xml(name, &main, &maps, false));
    Some(out)
}

/// Whether a format code shows a date or time (so cells are written with date/time values).
pub(super) fn code_kind(code: &str) -> Kind {
    if code.trim().is_empty() || code.eq_ignore_ascii_case("general") {
        return Kind::Number;
    }
    split_sections(code).first().map(|s| kind_of(&parse_section(s))).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn number_parts() {
        assert_eq!(number_part(Some(2), None, Some(1), true), "#,##0.00");
        assert_eq!(number_part(Some(0), None, Some(1), false), "0");
        assert_eq!(number_part(None, None, Some(1), false), "General");
        assert_eq!(number_part(Some(3), Some(1), Some(1), false), "0.0##");
        assert_eq!(scientific_part(Some(2), Some(1), Some(2)), "0.00E+00");
        assert_eq!(fraction_part(Some(0), Some(1), Some(1), None), "# ?/?");
        assert_eq!(literal("%", Kind::Percentage), "%");
        assert_eq!(literal(" €", Kind::Currency), " \"€\"");
    }

    #[test]
    fn codes_classify_and_write() {
        assert_eq!(code_kind("yyyy-mm-dd"), Kind::Date);
        assert_eq!(code_kind("[h]:mm:ss"), Kind::Time);
        assert_eq!(code_kind("0.00%"), Kind::Percentage);
        assert_eq!(code_kind("#,##0.00"), Kind::Number);
        let xml = data_style_xml("N1", "#,##0.00;[Red]-#,##0.00").unwrap_or_default();
        assert!(xml.contains("style:name=\"N1P0\""), "{xml}");
        assert!(xml.contains("value()&gt;=0"), "{xml}");
        assert!(xml.contains("fo:color=\"#ff0000\""), "{xml}");
        let xml = data_style_xml("N2", "hh:mm").unwrap_or_default();
        assert!(xml.contains("<number:hours number:style=\"long\"/><number:text>:</number:text><number:minutes number:style=\"long\"/>"), "{xml}");
        assert!(data_style_xml("N3", "General").is_none());
    }
}
