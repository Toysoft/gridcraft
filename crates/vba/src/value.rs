//! VBA values (Variants) and the conversions between them.

use std::any::Any;
use std::fmt;
use std::sync::{Arc, Mutex};

use crate::VbaError;

/// An object the host (the Excel object model, UserForms) hands out. The interpreter only stores
/// and passes it back; the host downcasts `inner` to its own type.
#[derive(Clone)]
pub struct HostObj {
    /// Type name for `TypeName()` and `TypeOf … Is`.
    pub class: Arc<str>,
    pub inner: Arc<dyn Any + Send + Sync>,
}

impl fmt::Debug for HostObj {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "<{}>", self.class)
    }
}

impl HostObj {
    pub fn new<T: Any + Send + Sync>(class: &str, inner: T) -> HostObj {
        HostObj { class: class.into(), inner: Arc::new(inner) }
    }
    pub fn get<T: Any>(&self) -> Option<&T> {
        self.inner.downcast_ref::<T>()
    }
}

/// An instance of a class module.
#[derive(Debug)]
pub struct Instance {
    pub class: String,
    pub fields: Vec<(String, Value)>,
}

/// A `Collection`.
#[derive(Debug, Default)]
pub struct Collection {
    pub items: Vec<(Option<String>, Value)>,
}

#[derive(Clone, Debug)]
pub enum Object {
    Host(HostObj),
    Instance(Arc<Mutex<Instance>>),
    Collection(Arc<Mutex<Collection>>),
    /// A user-defined `Type` value is a value, but kept here for member access.
    Record(Arc<Mutex<Instance>>),
}

impl Object {
    pub fn type_name(&self) -> String {
        match self {
            Object::Host(h) => h.class.to_string(),
            Object::Instance(i) | Object::Record(i) => i.lock().map(|i| i.class.clone()).unwrap_or_default(),
            Object::Collection(_) => "Collection".into(),
        }
    }
    /// The same object (`Is`).
    pub fn same(&self, other: &Object) -> bool {
        match (self, other) {
            (Object::Host(a), Object::Host(b)) => Arc::ptr_eq(&a.inner, &b.inner),
            (Object::Instance(a), Object::Instance(b)) | (Object::Record(a), Object::Record(b)) => Arc::ptr_eq(a, b),
            (Object::Collection(a), Object::Collection(b)) => Arc::ptr_eq(a, b),
            _ => false,
        }
    }
}

/// A VBA array: its dimensions (lower bound, length) and elements, first index fastest.
#[derive(Clone, Debug, PartialEq)]
pub struct Array {
    pub dims: Vec<(i64, usize)>,
    pub data: Vec<Value>,
}

/// Elements an array may hold (a `ReDim` of a huge size is refused, not an out-of-memory crash).
pub const MAX_ARRAY: usize = 50_000_000;

impl Array {
    pub fn new(dims: Vec<(i64, usize)>) -> Result<Array, VbaError> {
        let n = dims
            .iter()
            .try_fold(1usize, |acc, (_, len)| acc.checked_mul(*len))
            .filter(|n| *n <= MAX_ARRAY)
            .ok_or(VbaError::runtime(7, "Out of memory"))?;
        Ok(Array { dims, data: vec![Value::Empty; n] })
    }
    /// A one-dimensional array from values, lower bound `lower`.
    pub fn from_vec(lower: i64, data: Vec<Value>) -> Array {
        Array { dims: vec![(lower, data.len())], data }
    }
    /// The element offset of an index list, `None` when out of range.
    pub fn offset(&self, idx: &[i64]) -> Option<usize> {
        if idx.len() != self.dims.len() {
            return None;
        }
        let mut off = 0usize;
        let mut stride = 1usize;
        for (i, (lo, len)) in idx.iter().zip(self.dims.iter()) {
            let k = usize::try_from(i.checked_sub(*lo)?).ok()?;
            if k >= *len {
                return None;
            }
            off = off.checked_add(k.checked_mul(stride)?)?;
            stride = stride.checked_mul(*len)?;
        }
        Some(off)
    }
    pub fn lbound(&self, dim: usize) -> Option<i64> {
        self.dims.get(dim).map(|d| d.0)
    }
    pub fn ubound(&self, dim: usize) -> Option<i64> {
        self.dims.get(dim).map(|(lo, len)| lo + *len as i64 - 1)
    }
}

#[derive(Clone, Debug, Default)]
pub enum Value {
    #[default]
    Empty,
    Null,
    /// An optional argument that was not passed (`IsMissing`).
    Missing,
    Bool(bool),
    /// Integer, Long and Byte.
    Int(i64),
    /// Single, Double, Currency and Decimal.
    Double(f64),
    /// A date-time serial (days since 1899-12-30).
    Date(f64),
    Str(Arc<str>),
    Object(Object),
    Nothing,
    /// A `CVErr` value: an error number.
    Error(i64),
    Array(Arc<Array>),
}

impl PartialEq for Value {
    fn eq(&self, other: &Value) -> bool {
        match (self, other) {
            (Value::Empty, Value::Empty) | (Value::Null, Value::Null) | (Value::Missing, Value::Missing) | (Value::Nothing, Value::Nothing) => true,
            (Value::Bool(a), Value::Bool(b)) => a == b,
            (Value::Int(a), Value::Int(b)) => a == b,
            (Value::Double(a), Value::Double(b)) | (Value::Date(a), Value::Date(b)) => a == b,
            (Value::Str(a), Value::Str(b)) => a == b,
            (Value::Error(a), Value::Error(b)) => a == b,
            (Value::Array(a), Value::Array(b)) => a == b,
            (Value::Object(a), Value::Object(b)) => a.same(b),
            _ => false,
        }
    }
}

impl From<&str> for Value {
    fn from(s: &str) -> Value {
        Value::Str(s.into())
    }
}
impl From<String> for Value {
    fn from(s: String) -> Value {
        Value::Str(s.into())
    }
}
impl From<f64> for Value {
    fn from(n: f64) -> Value {
        Value::Double(n)
    }
}
impl From<i64> for Value {
    fn from(n: i64) -> Value {
        Value::Int(n)
    }
}
impl From<bool> for Value {
    fn from(b: bool) -> Value {
        Value::Bool(b)
    }
}

/// Days from 1970-01-01 to VBA's day 0 (1899-12-30).
const EPOCH: i64 = -25_569;

/// A date serial from a calendar date and time.
pub fn date_serial(y: i64, m: i64, d: i64) -> f64 {
    // Months outside 1..12 roll into the year (DateSerial(2020, 13, 1) = 2021-01-01).
    let y = y + (m - 1).div_euclid(12);
    let m = (m - 1).rem_euclid(12) + 1;
    let y = i32::try_from(y.clamp(-9999, 99_999)).unwrap_or(2000);
    (gridcraft_core::date::days_from_civil(y, m as u32, 1) - EPOCH + d - 1) as f64
}

/// (year, month, day, hour, minute, second) of a date serial.
pub fn date_parts(serial: f64) -> (i32, u32, u32, u32, u32, u32) {
    let s = if serial.is_finite() { serial.clamp(-1e7, 1e7) } else { 0.0 };
    let day = s.floor();
    let secs = ((s - day) * 86_400.0).round() as u32;
    let (day, secs) = if secs >= 86_400 { (day + 1.0, 0) } else { (day, secs) };
    let (y, m, d) = gridcraft_core::date::civil_from_days(day as i64 + EPOCH);
    (y, m, d, secs / 3600, secs / 60 % 60, secs % 60)
}

/// Parses a date or time as `CDate`/`#…#` do (m/d/y and ISO dates, `h:mm[:ss] [AM|PM]`).
pub fn parse_date(s: &str) -> Option<f64> {
    let s = s.trim();
    if s.is_empty() {
        return None;
    }
    let (date_part, time_part) = match s.find([' ', 'T']) {
        Some(i) if s[..i].contains(['/', '-']) => (Some(&s[..i]), Some(s[i + 1..].trim())),
        _ if s.contains(':') => (None, Some(s)),
        _ => (Some(s), None),
    };
    let mut serial = 0.0;
    if let Some(d) = date_part {
        let parts: Vec<&str> = d.split(['/', '-', '.']).collect();
        let nums: Vec<i64> = parts.iter().filter_map(|p| p.trim().parse::<i64>().ok()).collect();
        let month_name = |p: &str| {
            const M: [&str; 12] = ["jan", "feb", "mar", "apr", "may", "jun", "jul", "aug", "sep", "oct", "nov", "dec"];
            let l = p.trim().to_ascii_lowercase();
            M.iter().position(|m| l.starts_with(m)).map(|i| i as i64 + 1)
        };
        let (y, m, dd) = match (parts.len(), nums.len()) {
            (3, 3) if parts[0].len() == 4 => (nums[0], nums[1], nums[2]),
            (3, 3) => (nums[2], nums[0], nums[1]),
            (3, 2) => {
                // `1-Jan-2020` or `Jan-1-2020`.
                let m = parts.iter().find_map(|p| month_name(p))?;
                (nums[1], m, nums[0])
            }
            (2, 2) => (2000, nums[0], nums[1]),
            _ => return None,
        };
        let y = if (0..30).contains(&y) {
            y + 2000
        } else if (30..100).contains(&y) {
            y + 1900
        } else {
            y
        };
        if !(1..=12).contains(&m) || !(1..=31).contains(&dd) {
            return None;
        }
        serial = date_serial(y, m, dd);
    }
    if let Some(t) = time_part.filter(|t| !t.is_empty()) {
        let lower = t.to_ascii_lowercase();
        let pm = lower.ends_with("pm") || lower.ends_with("p");
        let am = lower.ends_with("am") || lower.ends_with("a");
        let core = lower.trim_end_matches(|c: char| c.is_alphabetic() || c == ' ');
        let f: Vec<f64> = core.split(':').map(|p| p.trim().parse::<f64>()).collect::<Result<_, _>>().ok()?;
        let (mut h, mi, se) = (*f.first()?, f.get(1).copied().unwrap_or(0.0), f.get(2).copied().unwrap_or(0.0));
        if pm && h < 12.0 {
            h += 12.0;
        }
        if am && h == 12.0 {
            h = 0.0;
        }
        serial += (h * 3600.0 + mi * 60.0 + se) / 86_400.0;
    }
    Some(serial)
}

/// A number as VBA prints it (`CStr`): up to 15 significant digits, no trailing zeros.
pub fn number_text(n: f64) -> String {
    if n == 0.0 {
        return "0".into();
    }
    if !n.is_finite() {
        return "0".into();
    }
    if n.fract() == 0.0 && n.abs() < 1e15 {
        return format!("{}", n as i64);
    }
    let a = n.abs();
    if !(1e-5..1e15).contains(&a) {
        // `1E+20`, `1E-07`.
        let s = format!("{:.14E}", n);
        let (m, e) = s.split_once('E').unwrap_or((&s, "0"));
        let m = m.trim_end_matches('0').trim_end_matches('.');
        let e: i32 = e.parse().unwrap_or(0);
        return format!("{m}E{}{:02}", if e < 0 { '-' } else { '+' }, e.abs());
    }
    let digits = 15 - (a.log10().floor() as i32 + 1);
    let s = format!("{:.*}", digits.clamp(0, 15) as usize, n);
    if s.contains('.') { s.trim_end_matches('0').trim_end_matches('.').to_string() } else { s }
}

/// A date as VBA prints it: `m/d/yyyy`, with ` h:mm:ss AM` when it has a time.
pub fn date_text(serial: f64) -> String {
    let (y, m, d, h, mi, s) = date_parts(serial);
    let has_date = serial.floor() != 0.0;
    let has_time = serial.fract() != 0.0;
    let time = || {
        let (h12, ap) = match h {
            0 => (12, "AM"),
            1..=11 => (h, "AM"),
            12 => (12, "PM"),
            _ => (h - 12, "PM"),
        };
        format!("{h12}:{mi:02}:{s:02} {ap}")
    };
    match (has_date, has_time) {
        (true, true) => format!("{m}/{d}/{y} {}", time()),
        (false, true) => time(),
        _ => format!("{m}/{d}/{y}"),
    }
}

impl Value {
    pub fn str(s: impl Into<Arc<str>>) -> Value {
        Value::Str(s.into())
    }

    pub fn is_object(&self) -> bool {
        matches!(self, Value::Object(_) | Value::Nothing)
    }

    pub fn type_name(&self) -> String {
        match self {
            Value::Empty => "Empty".into(),
            Value::Null => "Null".into(),
            Value::Missing => "Error".into(),
            Value::Bool(_) => "Boolean".into(),
            Value::Int(n) if i32::try_from(*n).is_ok() && (i16::try_from(*n).is_ok()) => "Integer".into(),
            Value::Int(n) if i32::try_from(*n).is_ok() => "Long".into(),
            Value::Int(_) => "LongLong".into(),
            Value::Double(_) => "Double".into(),
            Value::Date(_) => "Date".into(),
            Value::Str(_) => "String".into(),
            Value::Object(o) => o.type_name(),
            Value::Nothing => "Nothing".into(),
            Value::Error(_) => "Error".into(),
            Value::Array(_) => "Variant()".into(),
        }
    }

    /// `CStr`.
    pub fn to_text(&self) -> Result<String, VbaError> {
        Ok(match self {
            Value::Empty | Value::Missing => String::new(),
            Value::Null => return Err(VbaError::runtime(94, "Invalid use of Null")),
            Value::Bool(b) => if *b { "True" } else { "False" }.into(),
            Value::Int(n) => n.to_string(),
            Value::Double(n) => number_text(*n),
            Value::Date(d) => date_text(*d),
            Value::Str(s) => s.to_string(),
            Value::Error(e) => format!("Error {e}"),
            Value::Object(_) | Value::Nothing => return Err(VbaError::runtime(13, "Type mismatch")),
            Value::Array(_) => return Err(VbaError::runtime(13, "Type mismatch")),
        })
    }

    /// The number a value converts to (`CDbl`): numeric text and dates convert; other text is a
    /// type mismatch.
    pub fn to_num(&self) -> Result<f64, VbaError> {
        Ok(match self {
            Value::Empty | Value::Missing => 0.0,
            Value::Null => return Err(VbaError::runtime(94, "Invalid use of Null")),
            Value::Bool(b) => {
                if *b {
                    -1.0
                } else {
                    0.0
                }
            }
            Value::Int(n) => *n as f64,
            Value::Double(n) | Value::Date(n) => *n,
            Value::Str(s) => parse_num(s).or_else(|| parse_date(s)).ok_or_else(|| VbaError::runtime(13, "Type mismatch"))?,
            _ => return Err(VbaError::runtime(13, "Type mismatch")),
        })
    }

    /// `CLng`: rounds half to even, like VBA.
    pub fn to_int(&self) -> Result<i64, VbaError> {
        match self {
            Value::Int(n) => Ok(*n),
            v => {
                let n = v.to_num()?;
                let r = round_even(n);
                if !r.is_finite() || r.abs() > 9.2e18 {
                    return Err(VbaError::runtime(6, "Overflow"));
                }
                Ok(r as i64)
            }
        }
    }

    pub fn to_bool(&self) -> Result<bool, VbaError> {
        Ok(match self {
            Value::Bool(b) => *b,
            Value::Str(s) if s.eq_ignore_ascii_case("true") => true,
            Value::Str(s) if s.eq_ignore_ascii_case("false") => false,
            v => v.to_num()? != 0.0,
        })
    }

    pub fn to_date(&self) -> Result<f64, VbaError> {
        match self {
            Value::Str(s) => parse_date(s).or_else(|| parse_num(s)).ok_or_else(|| VbaError::runtime(13, "Type mismatch")),
            v => v.to_num(),
        }
    }

    /// Numeric in the `IsNumeric` sense.
    pub fn is_numeric(&self) -> bool {
        match self {
            Value::Int(_) | Value::Double(_) | Value::Bool(_) | Value::Empty => true,
            Value::Str(s) => parse_num(s).is_some(),
            _ => false,
        }
    }
}

/// Numeric text: decimal numbers with sign, thousands separators, a currency sign, or `&H` hex.
pub fn parse_num(s: &str) -> Option<f64> {
    let t = s.trim();
    if t.is_empty() {
        return None;
    }
    if let Some(h) = t.strip_prefix("&H").or_else(|| t.strip_prefix("&h")) {
        return i64::from_str_radix(h, 16).ok().map(|v| v as f64);
    }
    let cleaned: String = t.chars().filter(|c| *c != ',' && *c != '$').collect();
    let (neg, body) = match cleaned.strip_prefix('(').and_then(|x| x.strip_suffix(')')) {
        Some(b) => (true, b.to_string()),
        None => (false, cleaned),
    };
    let v: f64 = body.trim().parse().ok().filter(|v: &f64| v.is_finite())?;
    // `inf`/`nan` parse in Rust but aren't numbers in VBA.
    if body.chars().any(|c| c.is_ascii_alphabetic() && !matches!(c, 'e' | 'E')) {
        return None;
    }
    Some(if neg { -v } else { v })
}

/// Banker's rounding to an integer.
pub fn round_even(n: f64) -> f64 {
    let r = n.round();
    if (n - n.trunc()).abs() == 0.5 && r % 2.0 != 0.0 { r - n.signum() } else { r }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn conversions() {
        assert_eq!(Value::from("12").to_num().unwrap(), 12.0);
        assert!(Value::from("abc").to_num().is_err());
        assert_eq!(Value::Double(2.5).to_int().unwrap(), 2);
        assert_eq!(Value::Double(3.5).to_int().unwrap(), 4);
        assert_eq!(Value::Bool(true).to_num().unwrap(), -1.0);
        assert_eq!(Value::Double(0.1 + 0.2).to_text().unwrap(), "0.3");
        assert_eq!(Value::Double(1e20).to_text().unwrap(), "1E+20");
        assert_eq!(Value::Int(5).to_text().unwrap(), "5");
        assert_eq!(parse_num("1,234.5"), Some(1234.5));
        assert_eq!(parse_num("inf"), None);
        assert!(Value::from("12").is_numeric() && !Value::from("x").is_numeric());
    }

    #[test]
    fn dates() {
        let d = date_serial(2020, 1, 8);
        assert_eq!(d, 43838.0, "same serial as Excel");
        assert_eq!(date_text(d), "1/8/2020");
        assert_eq!(date_text(d + 0.5), "1/8/2020 12:00:00 PM");
        assert_eq!(parse_date("1/8/2020"), Some(d));
        assert_eq!(parse_date("2020-01-08"), Some(d));
        assert_eq!(parse_date("8-Jan-2020"), Some(d));
        assert_eq!(parse_date("13:30"), Some(13.5 / 24.0));
        assert_eq!(parse_date("1:30 PM"), Some(13.5 / 24.0));
        assert_eq!(date_serial(2020, 13, 1), date_serial(2021, 1, 1));
        assert_eq!(date_parts(d), (2020, 1, 8, 0, 0, 0));
        assert_eq!(parse_date("hello"), None);
    }

    #[test]
    fn arrays_are_bounded() {
        let a = Array::new(vec![(1, 3), (0, 2)]).unwrap();
        assert_eq!(a.data.len(), 6);
        assert_eq!(a.offset(&[1, 0]), Some(0));
        assert_eq!(a.offset(&[3, 1]), Some(5));
        assert_eq!(a.offset(&[4, 0]), None);
        assert_eq!(a.ubound(0), Some(3));
        assert!(Array::new(vec![(0, usize::MAX), (0, 2)]).is_err());
        assert!(Array::new(vec![(0, MAX_ARRAY + 1)]).is_err());
    }
}
