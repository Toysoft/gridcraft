//! Equality indexes for criteria functions (SUMIF, COUNTIF, SUMIFS…) and exact lookups
//! (VLOOKUP, HLOOKUP, MATCH).
//!
//! A workbook often has thousands of formulas like `SUMIF(codes, A2, amounts)` that run the same
//! range against a different criterion each. Scanning the range per formula is quadratic. When the
//! host hands out the same array for the same range (a calculation pass caches them), the second
//! formula that tests that array builds a hash index of it: key → positions. An `=` criterion then
//! costs one lookup plus its matches. Keys follow [`crate::criteria::Criterion::matches`] exactly:
//! numbers compare at 15 significant digits, text case-insensitively, and numeric text also
//! matches the number it reads as. Exact lookups use a first-position index keyed the same way,
//! except that types must agree (`lookup_equal`: the number 5 doesn't find the text "5").

use std::collections::HashMap;
use std::sync::Arc;

use gridcraft_core::parse::parse_number_text;
use gridcraft_core::{Array, Value};

use crate::Ctx;
use crate::criteria::Criterion;

/// The equality class of a cell value.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub(crate) enum Key {
    /// Bits of the number rounded to 15 significant digits (zero unsigned).
    Number(u64),
    /// Lower-cased text.
    Text(Box<str>),
    Bool(bool),
    /// `ERROR.TYPE` code.
    Error(u32),
}

impl Key {
    pub(crate) fn number(n: f64) -> Key {
        Key::Number(round15(n).to_bits())
    }
}

/// `n` rounded to 15 significant digits, the precision `compare_numbers` treats as equal.
fn round15(n: f64) -> f64 {
    if n == 0.0 {
        return 0.0;
    }
    // Whole numbers below 10^15 already have at most 15 digits.
    if !n.is_finite() || (n.fract() == 0.0 && n.abs() < 1e15) {
        return n;
    }
    format!("{n:.14e}").parse().unwrap_or(n)
}

/// Positions (row-major) of an array's cells by key.
#[derive(Debug, Default)]
pub struct EqIndex {
    map: HashMap<Key, Arc<[u32]>>,
}

impl EqIndex {
    /// `None` when the array holds nested arrays or is too big for `u32` positions.
    fn build(a: &Array) -> Option<EqIndex> {
        if u32::try_from(a.data.len()).is_err() {
            return None;
        }
        let mut lists: HashMap<Key, Vec<u32>> = HashMap::new();
        for (i, v) in (0u32..).zip(a.data.iter()) {
            match v {
                Value::Empty => {}
                Value::Number(n) => lists.entry(Key::number(*n)).or_default().push(i),
                Value::Bool(b) => lists.entry(Key::Bool(*b)).or_default().push(i),
                Value::Error(e) => lists.entry(Key::Error(e.code())).or_default().push(i),
                Value::Text(t) => {
                    let lower: String = t.chars().flat_map(char::to_lowercase).collect();
                    lists.entry(Key::Text(lower.into())).or_default().push(i);
                    // `=5` matches the text "5" too.
                    if let Some(n) = parse_number_text(t) {
                        lists.entry(Key::number(n)).or_default().push(i);
                    }
                }
                Value::Array(_) => return None,
            }
        }
        Some(EqIndex { map: lists.into_iter().map(|(k, v)| (k, Arc::from(v))).collect() })
    }

    fn get(&self, k: &Key) -> Arc<[u32]> {
        self.map.get(k).cloned().unwrap_or_else(|| Arc::from([]))
    }
}

/// The line of an array an exact lookup searches.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) enum Line {
    /// Every cell, row-major (a 1-D array for MATCH).
    All,
    /// The first column (VLOOKUP).
    FirstColumn,
    /// The first row (HLOOKUP).
    FirstRow,
}

/// The key a lookup needle or cell is found by (types must agree), `None` for blanks.
fn lookup_key(v: &Value) -> Option<Key> {
    match v {
        Value::Number(n) => Some(Key::number(*n)),
        Value::Text(t) => Some(Key::Text(t.chars().flat_map(char::to_lowercase).collect::<String>().into())),
        Value::Bool(b) => Some(Key::Bool(*b)),
        Value::Error(e) => Some(Key::Error(e.code())),
        Value::Empty | Value::Array(_) => None,
    }
}

/// First position of each key along a line.
#[derive(Debug, Default)]
pub struct FirstIndex {
    map: HashMap<Key, u32>,
}

impl FirstIndex {
    fn build(a: &Array, line: Line) -> Option<FirstIndex> {
        let cells: Box<dyn Iterator<Item = Option<&Value>>> = match line {
            Line::All => Box::new(a.data.iter().map(Some)),
            Line::FirstColumn => Box::new((0..a.rows).map(|r| a.get(r, 0))),
            Line::FirstRow => Box::new((0..a.cols).map(|c| a.get(0, c))),
        };
        let mut map = HashMap::new();
        for (i, v) in cells.enumerate() {
            let i = u32::try_from(i).ok()?;
            if let Some(k) = v.and_then(lookup_key) {
                map.entry(k).or_insert(i);
            }
        }
        Some(FirstIndex { map })
    }
}

/// What a cache entry indexes.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum Kind {
    Criteria,
    Lookup(Line),
}

#[derive(Debug)]
enum Built {
    Criteria(Option<Arc<EqIndex>>),
    Lookup(Option<Arc<FirstIndex>>),
}

/// Smallest array worth indexing.
const MIN_CELLS: usize = 64;
/// Cells a cache keeps alive (seen or indexed arrays) before it stops taking more.
const MAX_CELLS: usize = 8_000_000;

/// Indexes over the arrays a calculation pass reads more than once, keyed by the array's address.
/// Every array it knows is kept alive, so an address can't be reused by another array while the
/// cache holds it; arrays are immutable, so an index never goes stale.
#[derive(Debug, Default)]
pub struct CriteriaCache {
    seen: HashMap<(usize, Kind), Arc<Array>>,
    built: HashMap<(usize, Kind), (Arc<Array>, Built)>,
    cells: usize,
}

impl CriteriaCache {
    pub fn new() -> CriteriaCache {
        CriteriaCache::default()
    }

    pub fn clear(&mut self) {
        *self = CriteriaCache::default();
    }

    /// The entry for `a`: built the second time `a` is asked for, `None` before that (or when
    /// the cache is full).
    fn entry(&mut self, a: &Arc<Array>, kind: Kind) -> Option<&Built> {
        let len = a.data.len();
        if len < MIN_CELLS {
            return None;
        }
        let key = (Arc::as_ptr(a) as usize, kind);
        if !self.built.contains_key(&key) {
            if self.seen.remove(&key).is_none() {
                if self.cells.saturating_add(len) <= MAX_CELLS {
                    self.cells += len;
                    self.seen.insert(key, Arc::clone(a));
                }
                return None;
            }
            let built = match kind {
                Kind::Criteria => Built::Criteria(EqIndex::build(a).map(Arc::new)),
                Kind::Lookup(line) => Built::Lookup(FirstIndex::build(a, line).map(Arc::new)),
            };
            self.built.insert(key, (Arc::clone(a), built));
        }
        self.built.get(&key).map(|(_, b)| b)
    }

    fn criteria(&mut self, a: &Arc<Array>) -> Option<Arc<EqIndex>> {
        match self.entry(a, Kind::Criteria)? {
            Built::Criteria(ix) => ix.clone(),
            Built::Lookup(_) => None,
        }
    }

    fn lookup(&mut self, a: &Arc<Array>, line: Line) -> Option<Arc<FirstIndex>> {
        match self.entry(a, Kind::Lookup(line))? {
            Built::Lookup(ix) => ix.clone(),
            Built::Criteria(_) => None,
        }
    }
}

/// Exact lookup of `needle` along `line` of `table` from an index: `Some(None)` when it isn't
/// there, `None` when no index answers (the caller searches). Wildcard needles aren't indexed.
pub(crate) fn find_first(ctx: &mut dyn Ctx, table: &Value, line: Line, needle: &Value) -> Option<Option<usize>> {
    let Value::Array(a) = table else { return None };
    let key = lookup_key(needle)?;
    let ix = ctx.criteria_cache()?.lookup(a, line)?;
    Some(ix.map.get(&key).map(|p| *p as usize))
}

/// Positions (row-major) of the cells of `range` that match `c`, from an index; `None` when no
/// index answers (the caller scans).
pub(crate) fn matching_positions(ctx: &mut dyn Ctx, range: &Value, c: &Criterion) -> Option<Arc<[u32]>> {
    let Value::Array(a) = range else { return None };
    let (key, negate) = c.eq_key()?;
    let ix = ctx.criteria_cache()?.criteria(a)?;
    let hits = ix.get(&key);
    if !negate {
        return Some(hits);
    }
    // `<>`: every other position.
    let n = u32::try_from(a.data.len()).ok()?;
    let mut skip = hits.iter().copied().peekable();
    let mut rest = Vec::with_capacity(a.data.len().saturating_sub(hits.len()));
    for i in 0..n {
        if skip.next_if_eq(&i).is_none() {
            rest.push(i);
        }
    }
    Some(rest.into())
}

#[cfg(test)]
mod tests {
    use super::*;
    use gridcraft_core::{CellError, DateSystem};

    struct Cx(CriteriaCache);
    impl Ctx for Cx {
        fn date_system(&self) -> DateSystem {
            DateSystem::D1900
        }
        fn now_serial(&self) -> f64 {
            0.0
        }
        fn random(&mut self) -> f64 {
            0.0
        }
        fn criteria_cache(&mut self) -> Option<&mut CriteriaCache> {
            Some(&mut self.0)
        }
    }

    fn column() -> Value {
        let mut data = vec![
            Value::Number(5.0),
            Value::from("5"),
            Value::from("Apple"),
            Value::from("APPLE"),
            Value::Number(0.1 + 0.2),
            Value::Bool(true),
            Value::from("TRUE"),
            Value::Error(CellError::NA),
            Value::Empty,
            Value::from(""),
            Value::from("é"),
            Value::from("É"),
            Value::Number(-0.0),
            Value::Number(0.0),
        ];
        data.resize(100, Value::Number(7.0));
        let n = data.len();
        Value::from(Array::new(n, 1, data).unwrap_or_else(|| Array::scalar(Value::Empty)))
    }

    /// The index gives exactly the cells the criterion matches when scanning.
    #[test]
    fn index_agrees_with_scan() {
        let col = column();
        let Value::Array(arr) = &col else { return };
        let mut cx = Cx(CriteriaCache::new());
        let criteria = [
            Value::Number(5.0),
            Value::from("5"),
            Value::from("=5"),
            Value::from("apple"),
            Value::from("=APPLE"),
            Value::Number(0.3),
            Value::from("0.3"),
            Value::Bool(true),
            Value::from("TRUE"),
            Value::from("#N/A"),
            Value::Error(CellError::NA),
            Value::from("É"),
            Value::Number(0.0),
            Value::Empty,
            Value::Number(7.0),
            Value::from("missing"),
        ];
        // First call only records the array; the second builds the index.
        assert!(matching_positions(&mut cx, &col, &Criterion::parse(&Value::Number(1.0))).is_none());
        for crit in criteria {
            let c = Criterion::parse(&crit);
            let scan: Vec<u32> = (0u32..).zip(arr.data.iter()).filter(|(_, v)| c.matches(v)).map(|(i, _)| i).collect();
            let indexed = matching_positions(&mut cx, &col, &c).map(|p| p.to_vec());
            assert_eq!(indexed, Some(scan), "criterion {crit:?}");
        }
        // `<>` is the complement of `=`.
        for crit in ["<>5", "<>apple", "<>TRUE", "<>#N/A", "<>0.3", "<>missing"] {
            let c = Criterion::parse(&Value::from(crit));
            let scan: Vec<u32> = (0u32..).zip(arr.data.iter()).filter(|(_, v)| c.matches(v)).map(|(i, _)| i).collect();
            assert_eq!(matching_positions(&mut cx, &col, &c).map(|p| p.to_vec()), Some(scan), "criterion {crit}");
        }
        // Criteria that aren't plain equality fall back to scanning.
        for crit in [">3", "a*", "<>a*", "", "=", "<>"] {
            assert!(matching_positions(&mut cx, &col, &Criterion::parse(&Value::from(crit))).is_none(), "{crit}");
        }
    }

    /// The lookup index finds what a linear `lookup_equal` search finds, along each line.
    #[test]
    fn lookup_index_agrees_with_search() {
        let Value::Array(col) = column() else { return };
        // Two columns: the test column, then row numbers.
        let data: Vec<Value> = col.data.iter().enumerate().flat_map(|(i, v)| [v.clone(), Value::Number(i as f64)]).collect();
        let table = Value::from(Array::new(col.data.len(), 2, data).unwrap_or_else(|| Array::scalar(Value::Empty)));
        let Value::Array(t) = &table else { return };
        let mut cx = Cx(CriteriaCache::new());
        let needles = [
            Value::Number(5.0),
            Value::from("5"),
            Value::from("apple"),
            Value::Number(0.3),
            Value::Bool(true),
            Value::from("true"),
            Value::Error(CellError::NA),
            Value::from("é"),
            Value::Number(0.0),
            Value::Number(7.0),
            Value::from(""),
            Value::from("missing"),
        ];
        for line in [Line::FirstColumn, Line::FirstRow, Line::All] {
            let hay: Vec<Value> = match line {
                Line::FirstColumn => (0..t.rows).filter_map(|r| t.get(r, 0).cloned()).collect(),
                Line::FirstRow => (0..t.cols).filter_map(|c| t.get(0, c).cloned()).collect(),
                Line::All => t.data.clone(),
            };
            assert!(find_first(&mut cx, &table, line, &Value::Number(1.0)).is_none(), "first use only records");
            for x in &needles {
                let want = hay.iter().position(|v| crate::criteria::lookup_equal(x, v, false));
                assert_eq!(find_first(&mut cx, &table, line, x), Some(want), "{line:?} {x:?}");
            }
        }
    }
}
