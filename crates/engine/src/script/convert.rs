//! Values between Rhai, JSON (command params and results) and cells.

use gridcraft_core::Value;
use rhai::{Array, Dynamic, Map};
use serde_json::Value as Json;

use super::model;

/// Deepest nesting converted; deeper values become `null` (JSON) or `()` (Rhai).
const MAX_DEPTH: usize = 64;

/// A script value as JSON: objects of the object model become their address or name.
pub(crate) fn to_json(v: &Dynamic) -> Json {
    to_json_at(v, 0)
}

fn to_json_at(v: &Dynamic, depth: usize) -> Json {
    if depth > MAX_DEPTH || v.is_unit() {
        return Json::Null;
    }
    if let Ok(b) = v.as_bool() {
        return Json::Bool(b);
    }
    if let Ok(i) = v.as_int() {
        return Json::from(i);
    }
    if let Ok(f) = v.as_float() {
        return serde_json::Number::from_f64(f).map(Json::Number).unwrap_or(Json::Null);
    }
    if let Ok(c) = v.as_char() {
        return Json::String(c.to_string());
    }
    if v.is_string() {
        return Json::String(v.clone().into_string().unwrap_or_default());
    }
    if let Some(a) = v.read_lock::<Array>() {
        return Json::Array(a.iter().map(|x| to_json_at(x, depth + 1)).collect());
    }
    if let Some(m) = v.read_lock::<Map>() {
        return Json::Object(m.iter().map(|(k, x)| (k.to_string(), to_json_at(x, depth + 1))).collect());
    }
    match model::describe(v) {
        Some(t) => Json::String(t),
        None => Json::String(v.type_name().to_string()),
    }
}

/// JSON (a command's result) as a script value.
pub(crate) fn from_json(v: &Json) -> Dynamic {
    from_json_at(v, 0)
}

fn from_json_at(v: &Json, depth: usize) -> Dynamic {
    if depth > MAX_DEPTH {
        return Dynamic::UNIT;
    }
    match v {
        Json::Null => Dynamic::UNIT,
        Json::Bool(b) => Dynamic::from(*b),
        Json::Number(n) => match n.as_i64() {
            Some(i) => Dynamic::from(i),
            None => Dynamic::from(n.as_f64().unwrap_or(0.0)),
        },
        Json::String(s) => Dynamic::from(s.clone()),
        Json::Array(a) => Dynamic::from_array(a.iter().map(|x| from_json_at(x, depth + 1)).collect()),
        Json::Object(o) => Dynamic::from_map(o.iter().map(|(k, x)| (k.as_str().into(), from_json_at(x, depth + 1))).collect()),
    }
}

/// How a value prints: strings without quotes, objects of the object model by address or name.
pub(crate) fn text(v: &Dynamic) -> String {
    if v.is_string() {
        return v.clone().into_string().unwrap_or_default();
    }
    model::describe(v).unwrap_or_else(|| v.to_string())
}

/// A number as a script sees it: whole numbers in the exact integer range are integers, so they
/// work as counters and indexes; everything else is a float.
pub(crate) fn number(n: f64) -> Dynamic {
    const EXACT: f64 = 9_007_199_254_740_992.0; // 2^53
    if n.fract() == 0.0 && n.abs() < EXACT { Dynamic::from(n as i64) } else { Dynamic::from(n) }
}

/// A cell value as a script sees it: numbers, text, booleans; an error as its text (`#DIV/0!`);
/// an empty cell as `""` (as in Office Scripts).
pub(crate) fn cell_value(v: &Value) -> Dynamic {
    match v {
        Value::Empty => Dynamic::from(String::new()),
        Value::Number(n) => number(*n),
        Value::Text(t) => Dynamic::from(t.to_string()),
        Value::Bool(b) => Dynamic::from(*b),
        Value::Error(e) => Dynamic::from(e.as_str().to_string()),
        Value::Array(a) => a.get(0, 0).map(cell_value).unwrap_or_else(|| Dynamic::from(String::new())),
    }
}

/// A script value written to a cell, as `range.setValues` takes it: numbers, booleans, text
/// (parsed like typing: `=` starts a formula) and `()` (clears).
pub(crate) fn cell_input(v: &Dynamic) -> Result<Json, String> {
    if v.is_unit() {
        return Ok(Json::Null);
    }
    if let Ok(b) = v.as_bool() {
        return Ok(Json::Bool(b));
    }
    if let Ok(i) = v.as_int() {
        return Ok(Json::from(i));
    }
    if let Ok(f) = v.as_float() {
        return serde_json::Number::from_f64(f).map(Json::Number).ok_or_else(|| "A cell can't hold an infinite or undefined number.".to_string());
    }
    if let Ok(c) = v.as_char() {
        return Ok(Json::String(c.to_string()));
    }
    if v.is_string() {
        return Ok(Json::String(v.clone().into_string().unwrap_or_default()));
    }
    Err(format!("A cell value must be a number, text, a boolean or () (to clear), not {}.", v.type_name()))
}
