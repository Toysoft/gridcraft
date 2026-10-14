//! GridCraft's extensions to structured references, and the Excel formulas files hold for them.
//!
//! `Table1[ROWS][COLUMNS]` (also `Table1[[ROWS],[COLUMNS]]`): the first brackets pick rows,
//! the second columns.
//! - Rows: Excel's items (`#Headers`, `#This Row`, `@`, …) and `#Prev Row` / `#Next Row`; a key
//!   matched against the first column (`"Bolt"`); or conditions on named columns
//!   (`Stock>400, Product<>"Nut"`, joined by AND).
//! - Columns: names, bare, `[bracketed]` or `"quoted"`, separated by `,`; `A:B` for a span;
//!   `$` anchors one; `[]` is every column.
//!
//! One pair of brackets is Excel's: columns (`Table1[Qty]`, `Table1[Qty, Price]`) or Excel's
//! own items (`Table1[#Headers]`, `Table1[@Qty]`).
//!
//! Each is evaluated as, and files hold, the Excel formula it stands for ([`lower_extensions`]),
//! which [`raise_extension`] reads back from exactly that shape.

use serde::{Deserialize, Serialize};

use crate::ast::{BinOp, Expr, StructItem, StructRef, UnOp};

/// A table reference that Excel's syntax can't express: rows by key or conditions, or several
/// columns. The ones it can are [`StructRef`]s.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct TableRef {
    /// Empty inside the table itself, as for [`StructRef`].
    pub table: String,
    pub rows: RowSel,
    /// Empty for every column (`[]`).
    pub columns: Vec<ColumnSpan>,
}

/// The rows of a [`TableRef`].
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum RowSel {
    /// Excel's row items; none for the data rows.
    Items(Vec<StructItem>),
    /// `["Bolt"]`: the first row whose first column matches, as an exact MATCH finds it.
    Key(Box<Expr>),
    /// `[Stock>400, Product<>"Nut"]`: every row meeting all the conditions.
    Filter(Vec<Condition>),
}

/// `Stock>400` in `Table1[Stock>400][Price]`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Condition {
    pub column: String,
    /// `$Stock>400`: the column stays when the formula is filled sideways.
    #[serde(default)]
    pub column_abs: bool,
    /// A comparison: `=`, `<>`, `<`, `>`, `<=` or `>=`.
    pub op: BinOp,
    pub value: Expr,
}

/// `Price`, `$Price` or `Price:Stock` in the column brackets.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ColumnSpan {
    pub first: String,
    /// The other end of `Price:Stock`.
    pub last: Option<String>,
    /// `$Price`, for one column only.
    #[serde(default)]
    pub abs: bool,
}

impl ColumnSpan {
    pub fn one(name: &str) -> ColumnSpan {
        ColumnSpan { first: name.to_string(), last: None, abs: false }
    }
}

impl RowSel {
    /// The expressions inside: a key, or the conditions' values.
    pub fn values(&self) -> Vec<&Expr> {
        match self {
            RowSel::Items(_) => vec![],
            RowSel::Key(k) => vec![k],
            RowSel::Filter(c) => c.iter().map(|c| &c.value).collect(),
        }
    }

    pub fn map_values(self, f: &mut dyn FnMut(Expr) -> Expr) -> RowSel {
        match self {
            RowSel::Key(k) => RowSel::Key(Box::new(k.map(f))),
            RowSel::Filter(c) => RowSel::Filter(c.into_iter().map(|c| Condition { value: c.value.map(f), ..c }).collect()),
            items => items,
        }
    }
}

impl StructRef {
    /// The row a `[#Prev Row]` / `[#Next Row]` reference is from the formula's: -1 or 1.
    pub fn row_offset(&self) -> Option<i8> {
        match self.specifiers.as_slice() {
            [StructItem::PrevRow] => Some(-1),
            [StructItem::NextRow] => Some(1),
            _ => None,
        }
    }

    /// The Excel formula a `[#Prev Row]` / `[#Next Row]` reference stands for:
    /// `OFFSET(Table1[[#This Row],[Qty]],-1,0)`, the cell that many rows from the formula's
    /// row of the column (above the first data row that is the header, below the last the
    /// totals row or the cell under the table). `None` for Excel's own references.
    pub fn lower(&self) -> Option<Expr> {
        let rows = self.row_offset()?;
        let this_row = Expr::Struct(StructRef { specifiers: vec![StructItem::ThisRow], ..self.clone() });
        let rows = if rows < 0 { Expr::Unary(UnOp::Neg, Box::new(Expr::Number(1.0))) } else { Expr::Number(1.0) };
        Some(Expr::Call("OFFSET".into(), vec![this_row, rows, Expr::Number(0.0)]))
    }

    /// The inverse of [`StructRef::lower`] for one node.
    fn raised(e: &Expr) -> Option<StructRef> {
        let Expr::Call(name, args) = e else { return None };
        let [Expr::Struct(r), rows, Expr::Number(cols)] = args.as_slice() else { return None };
        let item = match rows {
            Expr::Number(n) if *n == 1.0 => StructItem::NextRow,
            Expr::Number(n) if *n == -1.0 => StructItem::PrevRow,
            Expr::Unary(UnOp::Neg, n) if matches!(**n, Expr::Number(n) if n == 1.0) => StructItem::PrevRow,
            _ => return None,
        };
        (name == "OFFSET" && *cols == 0.0 && r.specifiers == [StructItem::ThisRow]).then(|| StructRef { specifiers: vec![item], ..r.clone() })
    }

    /// The column brackets of this reference, as spans.
    fn spans(&self) -> Vec<ColumnSpan> {
        match &self.col_start {
            Some(first) => vec![ColumnSpan { first: first.clone(), last: self.col_end.clone(), abs: self.col_abs }],
            None => vec![],
        }
    }
}

impl TableRef {
    /// `table[rows][columns]` as the simplest node: Excel's rows with at most one span of
    /// columns is a [`StructRef`].
    pub fn build(table: &str, rows: RowSel, mut columns: Vec<ColumnSpan>) -> Expr {
        match rows {
            RowSel::Items(specifiers) if columns.len() <= 1 => {
                let span = columns.pop();
                let (col_start, col_end, col_abs) = match span {
                    Some(s) => (Some(s.first), s.last, s.abs),
                    None => (None, None, false),
                };
                Expr::Struct(StructRef { table: table.to_string(), specifiers, col_start, col_end, col_abs })
            }
            rows => Expr::Table(TableRef { table: table.to_string(), rows, columns }),
        }
    }

    /// The key of a lookup by key returning one column: the form every Excel reads unaided.
    pub fn single_key(&self) -> Option<&Expr> {
        match (&self.rows, self.columns.as_slice()) {
            (RowSel::Key(k), [ColumnSpan { last: None, .. }]) => Some(k),
            _ => None,
        }
    }

    fn part(&self, specifiers: Vec<StructItem>, span: Option<&ColumnSpan>) -> StructRef {
        StructRef {
            table: self.table.clone(),
            specifiers,
            col_start: span.map(|s| s.first.clone()),
            col_end: span.and_then(|s| s.last.clone()),
            col_abs: span.is_some_and(|s| s.abs),
        }
    }

    /// The columns on the given rows: `Table1[Price]`, `Table1[]`, or several side by side,
    /// `HSTACK(Table1[Price],Table1[Product])`.
    fn values(&self, specifiers: &[StructItem]) -> Expr {
        let lowered = |s: StructRef| s.lower().unwrap_or(Expr::Struct(s));
        match self.columns.as_slice() {
            [] => lowered(self.part(specifiers.to_vec(), None)),
            [one] => lowered(self.part(specifiers.to_vec(), Some(one))),
            many => Expr::Call("HSTACK".into(), many.iter().map(|s| lowered(self.part(specifiers.to_vec(), Some(s)))).collect()),
        }
    }

    /// The Excel formula this reference stands for:
    /// - by key, `INDEX(Table1[Price],MATCH(key,INDEX(Table1[],0,1),0))` (every Excel), with a
    ///   `,0` after the MATCH when it returns several columns;
    /// - by conditions, `FILTER(Table1[Price],(Table1[Stock]>400)*(Table1[Product]<>"Nut"),NA())`
    ///   (Excel 2021 and later);
    /// - several columns, `HSTACK(…)` of one reference per column (Excel 2024 and 365).
    ///
    /// Columns are named only by structured references, which Excel renames with the table and
    /// its columns. An anchored column (`$Price`) is the one-column range `Table1[[Price]:[Price]]`.
    pub fn lower(&self) -> Expr {
        match &self.rows {
            RowSel::Items(items) => self.values(items),
            RowSel::Key(key) => {
                let first_column = Expr::Call("INDEX".into(), vec![Expr::Struct(self.part(vec![], None)), Expr::Number(0.0), Expr::Number(1.0)]);
                let row = Expr::Call("MATCH".into(), vec![(**key).clone(), first_column, Expr::Number(0.0)]);
                let mut args = vec![self.values(&[]), row];
                if self.single_key().is_none() {
                    args.push(Expr::Number(0.0));
                }
                Expr::Call("INDEX".into(), args)
            }
            RowSel::Filter(conditions) => {
                let comparisons: Vec<Expr> = conditions
                    .iter()
                    .map(|c| {
                        let span = ColumnSpan { first: c.column.clone(), last: None, abs: c.column_abs };
                        let value = match &c.value {
                            Expr::Binary(op, ..) if op.precedence() <= BinOp::Eq.precedence() => Expr::Paren(Box::new(c.value.clone())),
                            v => v.clone(),
                        };
                        Expr::Binary(c.op, Box::new(Expr::Struct(self.part(vec![], Some(&span)))), Box::new(value))
                    })
                    .collect();
                let include = if comparisons.len() == 1 {
                    comparisons.into_iter().next().unwrap_or(Expr::Bool(true))
                } else {
                    comparisons
                        .into_iter()
                        .map(|c| Expr::Paren(Box::new(c)))
                        .reduce(|a, b| Expr::Binary(BinOp::Mul, Box::new(a), Box::new(b)))
                        .unwrap_or(Expr::Bool(true))
                };
                Expr::Call("FILTER".into(), vec![self.values(&[]), include, Expr::Call("NA".into(), vec![])])
            }
        }
    }

    /// Reads one node of a lowered formula back: `None` unless it has exactly a lowered shape.
    /// Works bottom-up (with [`Expr::map`]): the parts of a formula are already raised.
    fn raised(e: &Expr) -> Option<Expr> {
        let Expr::Call(name, args) = e else { return None };
        match (name.as_str(), args.as_slice()) {
            // Several columns: `HSTACK(Table1[Price],Table1[Product])`.
            ("HSTACK", parts) if parts.len() > 1 => {
                let mut table: Option<(&str, &[StructItem])> = None;
                let mut columns = Vec::new();
                for p in parts {
                    let Expr::Struct(s) = p else { return None };
                    let span = s.spans().pop()?;
                    match table {
                        Some((t, items)) if !t.eq_ignore_ascii_case(&s.table) || items != s.specifiers.as_slice() => return None,
                        _ => table = Some((&s.table, &s.specifiers)),
                    }
                    columns.push(span);
                }
                let (table, items) = table?;
                Some(Expr::Table(TableRef { table: table.to_string(), rows: RowSel::Items(items.to_vec()), columns }))
            }
            ("INDEX", [values, Expr::Call(m, row), rest @ ..]) if m == "MATCH" => {
                let (table, columns) = data_columns(values)?;
                let [key, Expr::Call(index, first), Expr::Number(mode)] = row.as_slice() else { return None };
                let [Expr::Struct(whole), Expr::Number(zero), Expr::Number(one)] = first.as_slice() else { return None };
                let first_column = index == "INDEX"
                    && *zero == 0.0
                    && *one == 1.0
                    && whole.specifiers.is_empty()
                    && whole.col_start.is_none()
                    && whole.table.eq_ignore_ascii_case(&table);
                let t = TableRef { table, rows: RowSel::Key(Box::new(key.clone())), columns };
                let several = matches!(rest, [Expr::Number(z)] if *z == 0.0);
                (*mode == 0.0 && first_column && (rest.is_empty() == t.single_key().is_some()) && (rest.is_empty() || several))
                    .then_some(Expr::Table(t))
            }
            ("FILTER", [values, include, Expr::Call(na, none)]) if na == "NA" && none.is_empty() => {
                let (table, columns) = data_columns(values)?;
                // `T[a]>1`, or `(T[a]>1)*(T[b]<2)*…`.
                let several = matches!(include, Expr::Binary(BinOp::Mul, ..));
                let mut factors = Vec::new();
                let mut rest = include;
                while let Expr::Binary(BinOp::Mul, a, b) = rest {
                    factors.push(&**b);
                    rest = a;
                }
                factors.push(rest);
                factors.reverse();
                let mut conditions = Vec::new();
                for f in factors {
                    let comparison = match (several, f) {
                        (true, Expr::Paren(x)) => &**x,
                        (false, x @ Expr::Binary(..)) => x,
                        _ => return None,
                    };
                    let Expr::Binary(op, column, value) = comparison else { return None };
                    let Expr::Struct(c) = &**column else { return None };
                    let (Some(name), None) = (&c.col_start, &c.col_end) else { return None };
                    // A value that lowering would have wrapped must have been wrapped.
                    let bare_comparison = matches!(&**value, Expr::Binary(o, ..) if o.precedence() <= BinOp::Eq.precedence());
                    if op.precedence() != BinOp::Eq.precedence()
                        || !c.specifiers.is_empty()
                        || !c.table.eq_ignore_ascii_case(&table)
                        || bare_comparison
                    {
                        return None;
                    }
                    conditions.push(Condition { column: name.clone(), column_abs: c.col_abs, op: *op, value: (**value).clone() });
                }
                Some(Expr::Table(TableRef { table, rows: RowSel::Filter(conditions), columns }))
            }
            // Saved by earlier builds: `VLOOKUP(key,Table1[],MATCH("Price",Table1[#Headers],0),FALSE)`
            // (`0` for `FALSE` when the column was anchored).
            ("VLOOKUP", [key, Expr::Struct(within), Expr::Call(m, index), exactly]) if m == "MATCH" => {
                let abs = match exactly {
                    Expr::Bool(false) => false,
                    Expr::Number(n) if *n == 0.0 => true,
                    _ => return None,
                };
                let [Expr::Text(header), Expr::Struct(headers), Expr::Number(mode)] = index.as_slice() else { return None };
                let whole = within.specifiers.is_empty() && within.col_start.is_none();
                let heads =
                    headers.specifiers == [StructItem::Headers] && headers.col_start.is_none() && headers.table.eq_ignore_ascii_case(&within.table);
                if *mode != 0.0 || !whole || !heads {
                    return None;
                }
                let column = ColumnSpan { first: unescape_wildcards(header)?, last: None, abs };
                Some(Expr::Table(TableRef { table: within.table.clone(), rows: RowSel::Key(Box::new(key.clone())), columns: vec![column] }))
            }
            _ => None,
        }
    }
}

/// The data columns a lowered lookup or filter returns: `Table1[]`, `Table1[Price]`,
/// `Table1[[A]:[C]]`, or (already raised) several of them.
fn data_columns(e: &Expr) -> Option<(String, Vec<ColumnSpan>)> {
    match e {
        Expr::Struct(s) if s.specifiers.is_empty() => Some((s.table.clone(), s.spans())),
        Expr::Table(TableRef { table, rows: RowSel::Items(items), columns }) if items.is_empty() => Some((table.clone(), columns.clone())),
        _ => None,
    }
}

/// `~*` → `*`; `None` when the text holds a live wildcard, which no column name lowers to.
fn unescape_wildcards(s: &str) -> Option<String> {
    let mut out = String::new();
    let mut it = s.chars();
    while let Some(c) = it.next() {
        match c {
            '~' => out.push(it.next()?),
            '*' | '?' => return None,
            c => out.push(c),
        }
    }
    Some(out)
}

/// Rewrites GridCraft's table reference extensions as the Excel formulas they stand for (for
/// files and evaluation).
pub fn lower_extensions(e: Expr) -> Expr {
    e.map(&mut |x| match x {
        Expr::Table(t) => t.lower(),
        Expr::Struct(s) => match s.lower() {
            Some(lowered) => lowered,
            None => Expr::Struct(s),
        },
        other => other,
    })
}

/// The inverse of [`lower_extensions`] for one node (use with [`Expr::map`]): a formula of
/// exactly a lowered shape becomes the extension again, anything else is returned unchanged.
pub fn raise_extension(e: Expr) -> Expr {
    if let Some(s) = StructRef::raised(&e) {
        return Expr::Struct(s);
    }
    TableRef::raised(&e).unwrap_or(e)
}
