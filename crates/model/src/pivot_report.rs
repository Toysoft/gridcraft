//! What a PivotTable report shows, line by line: which value field and which items each line
//! and each data column of the written report stands for. The engine builds it with the report;
//! GETPIVOTDATA reads it ([`PivotReport::lookup`]), and the PivotTable tabs use it to name the
//! active cell's field and to write GETPIVOTDATA formulas. It is runtime only: not saved, and
//! rebuilt when a workbook is opened.

use std::sync::Arc;

use gridcraft_core::{CellError, Value};

/// A PivotTable's report index, shared cheaply between workbook snapshots. It caches what the
/// written report shows, so comparing two PivotTables ignores it.
#[derive(Clone, Debug, Default)]
pub struct PivotReportIndex(pub Option<Arc<PivotReport>>);

impl PartialEq for PivotReportIndex {
    fn eq(&self, _: &Self) -> bool {
        true
    }
}

impl PivotReportIndex {
    pub fn get(&self) -> Option<&PivotReport> {
        self.0.as_deref()
    }
}

/// The layout of a written PivotTable report. Offsets count from the report's top-left cell
/// (`PivotTable::last_range`'s start, report filters included).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct PivotReport {
    /// Value fields, in report order.
    pub data_fields: Vec<ReportDataField>,
    /// Row fields, outer to inner (those found in the source).
    pub row_fields: Vec<ReportField>,
    /// Column fields, outer to inner.
    pub col_fields: Vec<ReportField>,
    /// Report filters: the field and the single item it shows (`None`: (All) or several items).
    pub page_fields: Vec<(String, Option<String>)>,
    /// The lines (report rows) that hold values: subtotals and grand totals included.
    pub rows: Vec<ReportKey>,
    /// The data columns.
    pub cols: Vec<ReportKey>,
    /// The report row of each column field's item labels.
    pub col_label_rows: Vec<u32>,
    /// How many columns the row labels take (one in Compact form).
    pub label_cols: u32,
    pub compact: bool,
}

/// A value field: its caption ("Sum of Sales") and the source field it summarizes.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ReportDataField {
    pub name: String,
    pub source: String,
}

/// A row or column field and the items the report shows for it (hidden items aren't there).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ReportField {
    pub name: String,
    /// Its numeric items are dates.
    pub dates: bool,
    pub items: Vec<ReportItem>,
}

/// An item: its value (a number, text, logical or error; text for grouped dates) and the label
/// the report shows.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ReportItem {
    pub value: Value,
    pub label: String,
}

/// What a line or a data column stands for.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ReportKey {
    /// Report row of a line, report column of a data column.
    pub offset: u32,
    /// The value field it shows, when that is decided here (Σ Values on this axis).
    pub data: Option<u32>,
    /// One entry per field of its axis: the item, or `None` for all items (a total).
    pub items: Vec<Option<u32>>,
}

impl ReportKey {
    fn specific(&self) -> usize {
        self.items.iter().filter(|i| i.is_some()).count()
    }
}

/// What a report cell belongs to, for the Active Field box.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReportPart {
    /// A value cell of this value field.
    Data(u32),
    /// An item label of this row field.
    RowField(u32),
    /// An item label of this column field.
    ColField(u32),
}

fn same_text(a: &str, b: &str) -> bool {
    a == b || a.to_lowercase() == b.to_lowercase()
}

impl ReportField {
    /// The item GETPIVOTDATA's `item` argument names: a number matches a numeric item (a date's
    /// serial) or an item labelled with that number; text matches an item's label, ignoring
    /// case; a logical matches a logical item.
    pub fn find(&self, item: &Value) -> Option<u32> {
        let i = match item {
            Value::Number(n) => self.items.iter().position(|it| match &it.value {
                Value::Number(m) => m == n,
                Value::Text(_) => it.label == gridcraft_core::number_to_text(*n),
                _ => false,
            }),
            Value::Text(t) => self.items.iter().position(|it| same_text(&it.label, t)),
            Value::Bool(b) => self.items.iter().position(|it| it.value == Value::Bool(*b)),
            Value::Array(_) => return self.find(&item.scalar()),
            Value::Empty | Value::Error(_) => None,
        }?;
        u32::try_from(i).ok()
    }
}

/// The least specific of the keys that match `want` (a `None` entry matches anything), when it
/// is the only one that specific.
fn least_specific<'a>(keys: &'a [ReportKey], want: &[Option<u32>], data: u32) -> Option<&'a ReportKey> {
    let mut best: Option<(&ReportKey, usize)> = None;
    let mut tie = false;
    for k in keys {
        if k.data.is_some_and(|d| d != data) || k.items.len() != want.len() {
            continue;
        }
        if !want.iter().zip(&k.items).all(|(w, i)| w.is_none() || w == i) {
            continue;
        }
        let n = k.specific();
        match best {
            Some((_, b)) if n > b => {}
            Some((_, b)) if n == b => tie = true,
            _ => {
                best = Some((k, n));
                tie = false;
            }
        }
    }
    if tie { None } else { best.map(|(k, _)| k) }
}

impl PivotReport {
    /// The value field GETPIVOTDATA's `data_field` names: a caption, else a source field, case
    /// ignored.
    pub fn data_field(&self, name: &str) -> Option<u32> {
        let i = self
            .data_fields
            .iter()
            .position(|d| same_text(&d.name, name))
            .or_else(|| self.data_fields.iter().position(|d| same_text(&d.source, name)))?;
        u32::try_from(i).ok()
    }

    /// The report cell (row and column offsets) GETPIVOTDATA reads, or `#REF!`. As in Excel,
    /// each pair names a row, column or report filter field and one of its shown items; fields
    /// left out stand for their total, or, where the report shows no such total, for the one item
    /// that the named items leave. The cell must be in the report: hidden, filtered-out and
    /// collapsed items, and totals turned off, give `#REF!`.
    pub fn lookup(&self, data_field: &str, pairs: &[(&str, &Value)]) -> Result<(u32, u32), CellError> {
        let data = self.data_field(data_field).ok_or(CellError::Ref)?;
        let mut rows: Vec<Option<u32>> = vec![None; self.row_fields.len()];
        let mut cols: Vec<Option<u32>> = vec![None; self.col_fields.len()];
        for (field, item) in pairs {
            let axis = [(&self.row_fields, &mut rows), (&self.col_fields, &mut cols)]
                .into_iter()
                .find_map(|(fields, want)| fields.iter().position(|f| same_text(&f.name, field)).map(|i| (fields, want, i)));
            match axis {
                Some((fields, want, i)) => {
                    let it = fields.get(i).and_then(|f| f.find(item)).ok_or(CellError::Ref)?;
                    let slot = want.get_mut(i).ok_or(CellError::Ref)?;
                    if slot.is_some_and(|x| x != it) {
                        return Err(CellError::Ref);
                    }
                    *slot = Some(it);
                }
                None => {
                    // A report filter counts only when it shows exactly that one item.
                    let (_, shown) = self.page_fields.iter().find(|(n, _)| same_text(n, field)).ok_or(CellError::Ref)?;
                    let shown = shown.as_deref().ok_or(CellError::Ref)?;
                    let named = match item {
                        Value::Text(t) => same_text(t, shown),
                        Value::Number(n) => gridcraft_core::number_to_text(*n) == shown,
                        _ => false,
                    };
                    if !named {
                        return Err(CellError::Ref);
                    }
                }
            }
        }
        let row = least_specific(&self.rows, &rows, data).ok_or(CellError::Ref)?;
        let col = least_specific(&self.cols, &cols, data).ok_or(CellError::Ref)?;
        if col.data.or(row.data) != Some(data) {
            return Err(CellError::Ref);
        }
        Ok((row.offset, col.offset))
    }

    fn row_at(&self, row: u32) -> Option<&ReportKey> {
        self.rows.iter().find(|k| k.offset == row)
    }

    fn col_at(&self, col: u32) -> Option<&ReportKey> {
        self.cols.iter().find(|k| k.offset == col)
    }

    /// The value cell at (row, column) offsets: its value field and the items of its line and
    /// column, row fields first (`None` when the cell isn't a value cell).
    #[allow(clippy::type_complexity)]
    pub fn value_cell(&self, row: u32, col: u32) -> Option<(u32, Vec<(&ReportField, &ReportItem)>)> {
        let (r, c) = (self.row_at(row)?, self.col_at(col)?);
        let data = c.data.or(r.data)?;
        let mut pairs = Vec::new();
        for (fields, key) in [(&self.row_fields, r), (&self.col_fields, c)] {
            for (f, it) in fields.iter().zip(&key.items) {
                if let Some(it) = it.and_then(|i| f.items.get(i as usize)) {
                    pairs.push((f, it));
                }
            }
        }
        Some((data, pairs))
    }

    /// The field a report cell belongs to: a value cell's value field, a row label's row field
    /// or a column label's column field.
    pub fn part_at(&self, row: u32, col: u32) -> Option<ReportPart> {
        if let (Some(r), Some(c)) = (self.row_at(row), self.col_at(col)) {
            return c.data.or(r.data).map(ReportPart::Data);
        }
        if col < self.label_cols
            && let Some(r) = self.row_at(row)
        {
            let deepest = r.items.iter().rposition(Option::is_some)?;
            // Outline and Tabular form give each row field its own column.
            let f = if !self.compact && r.items.get(col as usize).is_some_and(Option::is_some) { col as usize } else { deepest };
            return u32::try_from(f).ok().map(ReportPart::RowField);
        }
        let level = self.col_label_rows.iter().position(|r| *r == row)?;
        let c = self.col_at(col)?;
        c.items.get(level).copied().flatten()?;
        u32::try_from(level).ok().map(ReportPart::ColField)
    }

    /// The value cells' area: (first row, first column, last row, last column) offsets.
    pub fn data_area(&self) -> Option<(u32, u32, u32, u32)> {
        let r0 = self.rows.iter().map(|k| k.offset).min()?;
        let r1 = self.rows.iter().map(|k| k.offset).max()?;
        let c0 = self.cols.iter().map(|k| k.offset).min()?;
        let c1 = self.cols.iter().map(|k| k.offset).max()?;
        Some((r0, c0, r1, c1))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn item(label: &str) -> ReportItem {
        ReportItem { value: Value::text(label), label: label.into() }
    }

    fn key(offset: u32, data: Option<u32>, items: &[Option<u32>]) -> ReportKey {
        ReportKey { offset, data, items: items.to_vec() }
    }

    /// Region (East, West) › Product (Apples, Pears, Plums) in rows, Sum of Sales; East sells
    /// Apples and Pears, West Apples and Plums; subtotals on top, grand total.
    fn report() -> PivotReport {
        PivotReport {
            data_fields: vec![ReportDataField { name: "Sum of Sales".into(), source: "Sales".into() }],
            row_fields: vec![
                ReportField { name: "Region".into(), dates: false, items: vec![item("East"), item("West")] },
                ReportField { name: "Product".into(), dates: false, items: vec![item("Apples"), item("Pears"), item("Plums")] },
            ],
            col_fields: vec![],
            page_fields: vec![("Channel".into(), Some("Online".into())), ("Year".into(), None)],
            rows: vec![
                key(1, None, &[Some(0), None]),
                key(2, None, &[Some(0), Some(0)]),
                key(3, None, &[Some(0), Some(1)]),
                key(4, None, &[Some(1), None]),
                key(5, None, &[Some(1), Some(0)]),
                key(6, None, &[Some(1), Some(2)]),
                key(7, None, &[None, None]),
            ],
            cols: vec![key(1, Some(0), &[])],
            col_label_rows: vec![],
            label_cols: 1,
            compact: true,
        }
    }

    fn t(s: &str) -> Value {
        Value::text(s)
    }

    #[test]
    fn named_items_pick_the_cell_and_fields_left_out_are_totals() {
        let r = report();
        assert_eq!(r.lookup("Sum of Sales", &[]), Ok((7, 1)), "grand total");
        assert_eq!(r.lookup("sales", &[]), Ok((7, 1)), "by source field, case ignored");
        assert_eq!(r.lookup("Units", &[]), Err(CellError::Ref));
        assert_eq!(r.lookup("Sales", &[("region", &t("EAST"))]), Ok((1, 1)), "subtotal");
        assert_eq!(r.lookup("Sales", &[("Product", &t("Pears")), ("Region", &t("East"))]), Ok((3, 1)), "pairs in any order");
        // Plums only sells in the West: the region follows. Apples sells in both: ambiguous.
        assert_eq!(r.lookup("Sales", &[("Product", &t("Plums"))]), Ok((6, 1)));
        assert_eq!(r.lookup("Sales", &[("Product", &t("Apples"))]), Err(CellError::Ref));
        assert_eq!(r.lookup("Sales", &[("Region", &t("East")), ("Product", &t("Plums"))]), Err(CellError::Ref));
        assert_eq!(r.lookup("Sales", &[("Region", &t("South"))]), Err(CellError::Ref));
        assert_eq!(r.lookup("Sales", &[("Nope", &t("East"))]), Err(CellError::Ref));
        // The same item twice is fine; two items of one field are not.
        assert_eq!(r.lookup("Sales", &[("Region", &t("East")), ("Region", &t("East"))]), Ok((1, 1)));
        assert_eq!(r.lookup("Sales", &[("Region", &t("East")), ("Region", &t("West"))]), Err(CellError::Ref));
        // A report filter showing one item may be named with that item only.
        assert_eq!(r.lookup("Sales", &[("Channel", &t("online"))]), Ok((7, 1)));
        assert_eq!(r.lookup("Sales", &[("Channel", &t("Store"))]), Err(CellError::Ref));
        assert_eq!(r.lookup("Sales", &[("Year", &t("2026"))]), Err(CellError::Ref), "(All) shows no single item");
    }

    #[test]
    fn totals_that_are_turned_off_are_not_there() {
        let mut r = report();
        r.rows.retain(|k| k.items != [None, None]);
        assert_eq!(r.lookup("Sales", &[]), Err(CellError::Ref), "two regions, no grand total");
        r.rows.retain(|k| k.items.iter().all(Option::is_some));
        assert_eq!(r.lookup("Sales", &[("Region", &t("East"))]), Err(CellError::Ref), "no subtotal for East's two products");
        assert_eq!(r.lookup("Sales", &[("Region", &t("West")), ("Product", &t("Plums"))]), Ok((6, 1)));
    }

    #[test]
    fn items_match_by_value_or_label() {
        let f = ReportField {
            name: "Units".into(),
            dates: false,
            items: vec![
                ReportItem { value: Value::Number(5.0), label: "5".into() },
                ReportItem { value: Value::text("2026"), label: "2026".into() },
                ReportItem { value: Value::Bool(true), label: "TRUE".into() },
            ],
        };
        assert_eq!(f.find(&Value::Number(5.0)), Some(0));
        assert_eq!(f.find(&t("5")), Some(0), "text matches the label");
        assert_eq!(f.find(&Value::Number(5.000_000_1)), None);
        assert_eq!(f.find(&Value::Number(2026.0)), Some(1), "a number names a text item labelled with it");
        assert_eq!(f.find(&Value::Bool(true)), Some(2));
        assert_eq!(f.find(&Value::Empty), None);
    }

    #[test]
    fn cells_know_their_field_and_items() {
        let r = report();
        assert_eq!(r.part_at(3, 1), Some(ReportPart::Data(0)));
        assert_eq!(r.part_at(3, 0), Some(ReportPart::RowField(1)));
        assert_eq!(r.part_at(4, 0), Some(ReportPart::RowField(0)));
        assert_eq!(r.part_at(7, 0), None, "the grand total line has no item");
        assert_eq!(r.part_at(0, 0), None);
        let (data, pairs) = r.value_cell(5, 1).unwrap();
        assert_eq!(data, 0);
        let names: Vec<(&str, &str)> = pairs.iter().map(|(f, i)| (f.name.as_str(), i.label.as_str())).collect();
        assert_eq!(names, [("Region", "West"), ("Product", "Apples")]);
        assert!(r.value_cell(5, 0).is_none());
        assert_eq!(r.data_area(), Some((1, 1, 7, 1)));
        // The index is a cache: PivotTables compare equal with or without it.
        assert_eq!(PivotReportIndex(Some(Arc::new(r))), PivotReportIndex::default());
    }
}
