//! Rhai code from commands: what Record Actions saves, and what an old command-list script turns
//! into when it is opened for editing.
//!
//! Common edits become object-model calls (`sheet.getRange("B2").setValue("Total")`); every
//! other command becomes `run("id", #{params})`, which replays it exactly.

use std::fmt::Write as _;

use serde_json::Value as Json;

/// Deepest JSON nesting written as a literal; deeper values become `()`.
const MAX_DEPTH: usize = 32;

/// Commands that may change which worksheet is active: after one of these, a recorded step
/// names the active worksheet afresh instead of reusing `sheet`.
fn may_switch_sheet(id: &str) -> bool {
    id.starts_with("sheet.") || id.starts_with("file.") || id.starts_with("window.") || id.contains("Sheet") || id.starts_with("view.")
}

/// A script whose `main` replays `steps` (command id, params) in order.
pub fn commands_to_rhai(steps: &[(String, Json)], comment: &str) -> String {
    let mut body = String::new();
    // `sheet` is the worksheet that was active when the script starts; it stays right as long as
    // no step switches sheets.
    let stable = !steps.iter().any(|(id, _)| may_switch_sheet(id));
    let sheet = if stable { "sheet" } else { "workbook.getActiveWorksheet()" };
    for (id, p) in steps {
        let line = object_call(id, p, sheet).unwrap_or_else(|| run_call(id, p));
        let _ = writeln!(body, "    {line}");
    }
    let mut out = String::new();
    for l in comment.lines() {
        let _ = writeln!(out, "// {l}");
    }
    out.push_str("fn main(workbook) {\n");
    if stable && !steps.is_empty() {
        out.push_str("    let sheet = workbook.getActiveWorksheet();\n");
    }
    out.push_str(&body);
    out.push_str("}\n");
    out
}

/// `run("id", #{…});`
fn run_call(id: &str, p: &Json) -> String {
    match p {
        Json::Object(o) if o.is_empty() => format!("run({});", string(id)),
        Json::Null => format!("run({});", string(id)),
        _ => format!("run({}, {});", string(id), literal(p, 0)),
    }
}

/// The object-model form of a few common commands, when their params say exactly what to do.
fn object_call(id: &str, p: &Json, sheet: &str) -> Option<String> {
    let o = p.as_object()?;
    let only = |keys: &[&str]| o.keys().all(|k| keys.contains(&k.as_str()));
    let range = |key: &str| o.get(key).and_then(Json::as_str).filter(|r| !r.contains('!') && !r.contains(','));
    match id {
        "cell.set" if only(&["cell", "input"]) => {
            let cell = range("cell")?;
            Some(format!("{sheet}.getRange({}).setValue({});", string(cell), value(o.get("input")?)))
        }
        "range.setValues" if only(&["range", "values"]) => {
            let at = range("range")?;
            let rows = o.get("values")?.as_array()?;
            let width = rows.iter().map(|r| r.as_array().map_or(1, Vec::len)).max()?;
            if rows.is_empty() || width == 0 || rows.iter().any(|r| r.as_array().is_none_or(|a| a.len() != width)) {
                return None;
            }
            let start = gridcraft_core::RangeRef::parse(at)?.start;
            let end = start.offset(rows.len() as i64 - 1, width as i64 - 1)?;
            let block = gridcraft_core::RangeRef::new(start, end).a1();
            Some(format!("{sheet}.getRange({}).setValues({});", string(&block), literal(o.get("values")?, 0)))
        }
        "selection.set" if only(&["range"]) => Some(format!("{sheet}.getRange({}).select();", string(range("range")?))),
        "home.bold" | "home.italic" | "home.strikethrough" if only(&["range", "on"]) => {
            let on = o.get("on")?.as_bool()?;
            let setter = match id {
                "home.bold" => "setBold",
                "home.italic" => "setItalic",
                _ => "setStrikethrough",
            };
            Some(format!("{sheet}.getRange({}).getFormat().getFont().{setter}({on});", string(range("range")?)))
        }
        "home.fillColor" if only(&["range", "color"]) => {
            let color = o.get("color")?.as_str()?;
            let r = string(range("range")?);
            Some(if color.eq_ignore_ascii_case("none") {
                format!("{sheet}.getRange({r}).getFormat().getFill().clear();")
            } else {
                format!("{sheet}.getRange({r}).getFormat().getFill().setColor({});", string(color))
            })
        }
        "home.fontColor" if only(&["range", "color"]) => {
            let color = o.get("color")?.as_str()?;
            Some(format!("{sheet}.getRange({}).getFormat().getFont().setColor({});", string(range("range")?), string(color)))
        }
        "home.numberFormat" if only(&["range", "code"]) => {
            let code = o.get("code")?.as_str()?;
            Some(format!("{sheet}.getRange({}).setNumberFormat({});", string(range("range")?), string(code)))
        }
        _ => None,
    }
}

/// Typed input as a value: a plain number stays a number, anything else is the text as typed.
fn value(v: &Json) -> String {
    match v {
        Json::String(s) => match s.trim().parse::<f64>() {
            Ok(n) if n.is_finite() && s.trim() == s && !s.starts_with('+') && !s.contains(['e', 'E']) && !s.starts_with('.') => number(n),
            _ => string(s),
        },
        other => literal(other, 0),
    }
}

fn number(n: f64) -> String {
    if n.fract() == 0.0 && n.abs() < 9e15 {
        format!("{}", n as i64)
    } else {
        // Rust prints floats without an exponent; Rhai needs a decimal point in a float literal.
        let t = format!("{n}");
        if t.contains('.') { t } else { format!("{t}.0") }
    }
}

/// A JSON value as a Rhai literal: objects become object maps (`#{key: value}`), `null` is `()`.
fn literal(v: &Json, depth: usize) -> String {
    if depth > MAX_DEPTH {
        return "()".into();
    }
    match v {
        Json::Null => "()".into(),
        Json::Bool(b) => b.to_string(),
        Json::Number(n) => match n.as_i64() {
            Some(i) => i.to_string(),
            None => number(n.as_f64().unwrap_or(0.0)),
        },
        Json::String(s) => string(s),
        Json::Array(a) => format!("[{}]", a.iter().map(|x| literal(x, depth + 1)).collect::<Vec<_>>().join(", ")),
        Json::Object(o) => {
            let fields: Vec<String> = o.iter().map(|(k, x)| format!("{}: {}", key(k), literal(x, depth + 1))).collect();
            format!("#{{{}}}", fields.join(", "))
        }
    }
}

/// A map key: bare when it is an identifier, quoted otherwise.
fn key(k: &str) -> String {
    let ident = k.chars().next().is_some_and(|c| c.is_ascii_alphabetic() || c == '_') && k.chars().all(|c| c.is_ascii_alphanumeric() || c == '_');
    if ident { k.to_string() } else { string(k) }
}

/// A Rhai string literal.
pub fn string(s: &str) -> String {
    let mut o = String::with_capacity(s.len() + 2);
    o.push('"');
    for c in s.chars() {
        match c {
            '"' => o.push_str("\\\""),
            '\\' => o.push_str("\\\\"),
            '\n' => o.push_str("\\n"),
            '\r' => o.push_str("\\r"),
            '\t' => o.push_str("\\t"),
            c if c.is_control() => {
                let _ = write!(o, "\\u{:04X}", c as u32);
            }
            c => o.push(c),
        }
    }
    o.push('"');
    o
}
