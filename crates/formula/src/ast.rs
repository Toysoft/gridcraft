//! Formula syntax tree.

use std::sync::Arc;

use gridcraft_core::{CellError, CellRef, MAX_COLS, MAX_ROWS, RangeRef};
use serde::{Deserialize, Serialize};

/// One end of a reference, with `$` anchors.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Anchor {
    pub row: u32,
    pub col: u32,
    pub row_abs: bool,
    pub col_abs: bool,
}

impl Anchor {
    pub fn cell(&self) -> CellRef {
        CellRef::new(self.row, self.col)
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum RefKind {
    Cell(Anchor),
    Range(Anchor, Anchor),
    /// Whole rows `3:5` (row, abs) pairs.
    Rows(u32, bool, u32, bool),
    /// Whole columns `A:C`.
    Cols(u32, bool, u32, bool),
}

#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum SheetSel {
    /// Unqualified: the formula's own sheet.
    Current,
    Named(String),
    /// 3-D span `Sheet1:Sheet3`.
    Span(String, String),
}

#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Reference {
    pub sheet: SheetSel,
    pub kind: RefKind,
}

impl Reference {
    pub fn range(&self) -> RangeRef {
        match &self.kind {
            RefKind::Cell(a) => RangeRef::cell(a.cell()),
            RefKind::Range(a, b) => RangeRef::new(a.cell(), b.cell()),
            RefKind::Rows(r0, _, r1, _) => RangeRef::rows((*r0).min(*r1), (*r0).max(*r1)),
            RefKind::Cols(c0, _, c1, _) => RangeRef::cols((*c0).min(*c1), (*c0).max(*c1)),
        }
    }
    pub fn sheet_name(&self) -> Option<&str> {
        match &self.sheet {
            SheetSel::Named(s) => Some(s),
            _ => None,
        }
    }
}

/// Structured reference `Table1[[#Headers],[Col1]:[Col3]]`, `[@Col]`.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct StructRef {
    /// Empty for `[@Col]` inside the table itself.
    pub table: String,
    pub specifiers: Vec<StructItem>,
    pub col_start: Option<String>,
    pub col_end: Option<String>,
    /// `Table1[$Qty]`: the column stays when the formula is filled sideways, where an
    /// unanchored one moves along with it. A GridCraft spelling of Excel's own idiom for that,
    /// the one-column range `Table1[[Qty]:[Qty]]`, which is what files hold. Only for one column.
    #[serde(default)]
    pub col_abs: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum StructItem {
    All,
    Data,
    Headers,
    Totals,
    ThisRow,
    /// `Table1[Qty,[#Prev Row]]`: the row above the formula's (a GridCraft extension, written
    /// to files as `OFFSET(Table1[[#This Row],[Qty]],-1,0)`).
    PrevRow,
    /// `Table1[Qty,[#Next Row]]`: the row below, `OFFSET(…,1,0)` in files.
    NextRow,
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
}

/// Rewrites GridCraft's extensions to structured references, keyed references and
/// `[#Prev Row]` / `[#Next Row]`, as the Excel formulas they stand for (for files).
pub fn lower_extensions(e: Expr) -> Expr {
    e.map(&mut |x| match x {
        Expr::Keyed(k) => k.lower(),
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
    match StructRef::raised(&e) {
        Some(s) => Expr::Struct(s),
        None => KeyedRef::raise(e),
    }
}

/// Keyed table reference, a GridCraft extension to structured references:
/// - `Table1[Price,"Widget"]`: by a key, matched against the table's first column; the first
///   matching row's cell, as an exact MATCH (or VLOOKUP) finds it;
/// - `Table1[Price,Stock>400,Product<>"Nut"]`: by conditions on named columns, joined by AND;
///   the cells of every matching row (one value, or an array that spills), as FILTER finds them.
///
/// It is evaluated as, and files hold, that Excel formula ([`KeyedRef::lower`]), which
/// [`KeyedRef::raise`] reads back. No match is `#N/A` either way.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct KeyedRef {
    /// Empty inside the table itself, as for [`StructRef`].
    pub table: String,
    pub column: String,
    /// `Table1[$Price,key]`: the column stays when the formula is filled sideways.
    #[serde(default)]
    pub column_abs: bool,
    /// One key (a condition without a column), or conditions that each name their column.
    pub conditions: Vec<Condition>,
}

/// `Stock>400` in `Table1[Price,Stock>400]`, or the key in `Table1[Price,"Widget"]`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Condition {
    /// `None` for a key, which is matched against the table's first column.
    pub column: Option<String>,
    /// `$Stock>400`.
    #[serde(default)]
    pub column_abs: bool,
    /// A comparison: `=`, `<>`, `<`, `>`, `<=` or `>=`.
    pub op: BinOp,
    pub value: Expr,
}

impl KeyedRef {
    /// The key of a lookup by key (`Table1[Price,"Widget"]`); `None` for a filter.
    pub fn key(&self) -> Option<&Expr> {
        match self.conditions.as_slice() {
            [Condition { column: None, op: BinOp::Eq, value, .. }] => Some(value),
            _ => None,
        }
    }

    /// The Excel formula this reference stands for. By key:
    /// `INDEX(Table1[Price],MATCH(key,INDEX(Table1[],0,1),0))`, an exact, case-insensitive match
    /// on the first column that works in every Excel. By conditions:
    /// `FILTER(Table1[Price],(Table1[Stock]>400)*(Table1[Product]<>"Nut"),NA())` (Excel 2021 and
    /// later). Both name their columns by structured references, which Excel renames with the
    /// table and its columns. An anchored column (`$Price`) is the one-column range
    /// `Table1[[Price]:[Price]]`.
    pub fn lower(&self) -> Expr {
        let part = |specifiers: Vec<StructItem>, col: Option<&str>, col_abs: bool| {
            Expr::Struct(StructRef { table: self.table.clone(), specifiers, col_start: col.map(str::to_string), col_end: None, col_abs })
        };
        if let Some(key) = self.key() {
            let first_column = Expr::Call("INDEX".into(), vec![part(vec![], None, false), Expr::Number(0.0), Expr::Number(1.0)]);
            let row = Expr::Call("MATCH".into(), vec![key.clone(), first_column, Expr::Number(0.0)]);
            return Expr::Call("INDEX".into(), vec![part(vec![], Some(&self.column), self.column_abs), row]);
        }
        let comparisons: Vec<Expr> = self
            .conditions
            .iter()
            .map(|c| {
                // A key without a column has no place among conditions; the parser refuses it.
                let column = part(vec![], Some(c.column.as_deref().unwrap_or("")), c.column_abs);
                let value = match &c.value {
                    Expr::Binary(op, ..) if op.precedence() <= BinOp::Eq.precedence() => Expr::Paren(Box::new(c.value.clone())),
                    v => v.clone(),
                };
                Expr::Binary(c.op, Box::new(column), Box::new(value))
            })
            .collect();
        let include = match comparisons.len() {
            1 => comparisons.into_iter().next().unwrap_or(Expr::Bool(true)),
            _ => comparisons
                .into_iter()
                .map(|c| Expr::Paren(Box::new(c)))
                .reduce(|a, b| Expr::Binary(BinOp::Mul, Box::new(a), Box::new(b)))
                .unwrap_or(Expr::Bool(true)),
        };
        Expr::Call("FILTER".into(), vec![part(vec![], Some(&self.column), self.column_abs), include, Expr::Call("NA".into(), vec![])])
    }

    /// The inverse of [`KeyedRef::lower`] for one node: a call of exactly the lowered shape
    /// becomes the keyed reference, anything else is returned unchanged. Lookups by key saved
    /// by earlier versions as `VLOOKUP(key,Table1[],MATCH("Price",Table1[#Headers],0),FALSE)`
    /// read back too.
    pub fn raise(e: Expr) -> Expr {
        match Self::raised(&e) {
            Some(k) => Expr::Keyed(k),
            None => e,
        }
    }

    fn raised(e: &Expr) -> Option<KeyedRef> {
        let Expr::Call(name, args) = e else { return None };
        // `Table1[]`, `Table1[#Headers]` or `Table1[Col]`: (table, specifiers, column, anchored).
        let part = |e: &Expr| match e {
            Expr::Struct(StructRef { table, specifiers, col_start, col_end: None, col_abs }) => {
                Some((table.clone(), specifiers.clone(), col_start.clone(), *col_abs))
            }
            _ => None,
        };
        match (name.as_str(), args.as_slice()) {
            ("INDEX", [values, Expr::Call(m, row)]) if m == "MATCH" => {
                let (table, specifiers, Some(column), column_abs) = part(values)? else { return None };
                let [key, Expr::Call(index, first), Expr::Number(mode)] = row.as_slice() else { return None };
                let [whole, Expr::Number(zero), Expr::Number(one)] = first.as_slice() else { return None };
                let (of, items, None, _) = part(whole)? else { return None };
                let first_column = index == "INDEX" && *zero == 0.0 && *one == 1.0 && items.is_empty() && of.eq_ignore_ascii_case(&table);
                if *mode != 0.0 || !specifiers.is_empty() || !first_column {
                    return None;
                }
                let key = Condition { column: None, column_abs: false, op: BinOp::Eq, value: key.clone() };
                Some(KeyedRef { table, column, column_abs, conditions: vec![key] })
            }
            ("VLOOKUP", [key, within, Expr::Call(m, index), exactly]) if m == "MATCH" => {
                let column_abs = match exactly {
                    Expr::Bool(false) => false,
                    Expr::Number(n) if *n == 0.0 => true,
                    _ => return None,
                };
                let (table, specifiers, None, _) = part(within)? else { return None };
                let [Expr::Text(header), headers, Expr::Number(mode)] = index.as_slice() else { return None };
                let (headers_of, items, None, _) = part(headers)? else { return None };
                if *mode != 0.0 || !specifiers.is_empty() || items != [StructItem::Headers] || !headers_of.eq_ignore_ascii_case(&table) {
                    return None;
                }
                let key = Condition { column: None, column_abs: false, op: BinOp::Eq, value: key.clone() };
                Some(KeyedRef { table, column: unescape_wildcards(header)?, column_abs, conditions: vec![key] })
            }
            ("FILTER", [values, include, Expr::Call(na, none)]) if na == "NA" && none.is_empty() => {
                let (table, specifiers, Some(column), column_abs) = part(values)? else { return None };
                if !specifiers.is_empty() {
                    return None;
                }
                // `T[a]>1`, or `(T[a]>1)*(T[b]<2)*…`.
                let mut factors = Vec::new();
                let mut rest = include;
                let several = matches!(include, Expr::Binary(BinOp::Mul, ..));
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
                    let comparing = op.precedence() == BinOp::Eq.precedence();
                    let (of, items, Some(name), column_abs) = part(column)? else { return None };
                    // A value that lowering would have wrapped must have been wrapped.
                    let bare_comparison = matches!(&**value, Expr::Binary(o, ..) if o.precedence() <= BinOp::Eq.precedence());
                    if !comparing || !items.is_empty() || !of.eq_ignore_ascii_case(&table) || bare_comparison {
                        return None;
                    }
                    conditions.push(Condition { column: Some(name), column_abs, op: *op, value: (**value).clone() });
                }
                Some(KeyedRef { table, column, column_abs, conditions })
            }
            _ => None,
        }
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

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum UnOp {
    Neg,
    Plus,
    Percent,
    /// Implicit intersection `@`.
    At,
    /// Spill range operator `A1#`: the whole spill range of the dynamic array anchored at the
    /// cell (`ANCHORARRAY` in files).
    Spill,
    /// Trim reference `A1.:E10` / `A1:.E10` / `A1.:.E10` around a range: the range without its
    /// blank leading, trailing or outer rows and columns (`TRIMRANGE` with both trim codes set).
    Trim(TrimMode),
}

/// Which blank edges a trim reference drops.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum TrimMode {
    /// `.:` — leading blank rows and columns.
    Leading,
    /// `:.` — trailing blank rows and columns.
    Trailing,
    /// `.:.` — both.
    All,
}

impl TrimMode {
    pub fn symbol(self) -> &'static str {
        match self {
            TrimMode::Leading => ".:",
            TrimMode::Trailing => ":.",
            TrimMode::All => ".:.",
        }
    }
    /// The equivalent `TRIMRANGE` trim_rows / trim_cols code.
    pub fn code(self) -> u8 {
        match self {
            TrimMode::Leading => 1,
            TrimMode::Trailing => 2,
            TrimMode::All => 3,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum BinOp {
    Add,
    Sub,
    Mul,
    Div,
    Pow,
    Concat,
    Eq,
    Ne,
    Lt,
    Gt,
    Le,
    Ge,
    /// `:` between two references.
    Range,
    /// `,` inside parentheses.
    Union,
    /// space between two references.
    Intersect,
}

impl BinOp {
    pub fn symbol(&self) -> &'static str {
        match self {
            BinOp::Add => "+",
            BinOp::Sub => "-",
            BinOp::Mul => "*",
            BinOp::Div => "/",
            BinOp::Pow => "^",
            BinOp::Concat => "&",
            BinOp::Eq => "=",
            BinOp::Ne => "<>",
            BinOp::Lt => "<",
            BinOp::Gt => ">",
            BinOp::Le => "<=",
            BinOp::Ge => ">=",
            BinOp::Range => ":",
            BinOp::Union => ",",
            BinOp::Intersect => " ",
        }
    }
    /// Binding power (higher binds tighter).
    pub fn precedence(&self) -> u8 {
        match self {
            BinOp::Range => 90,
            BinOp::Intersect => 85,
            BinOp::Union => 80,
            BinOp::Pow => 50,
            BinOp::Mul | BinOp::Div => 40,
            BinOp::Add | BinOp::Sub => 30,
            BinOp::Concat => 20,
            BinOp::Eq | BinOp::Ne | BinOp::Lt | BinOp::Gt | BinOp::Le | BinOp::Ge => 10,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum Expr {
    Number(f64),
    Text(Arc<str>),
    Bool(bool),
    Error(CellError),
    /// Array constant `{1,2;3,4}` (rows of literal expressions).
    Array(Vec<Vec<Expr>>),
    Ref(Reference),
    /// Defined name, table name or LET/LAMBDA parameter.
    Name(String),
    Struct(StructRef),
    Keyed(KeyedRef),
    Unary(UnOp, Box<Expr>),
    Binary(BinOp, Box<Expr>, Box<Expr>),
    /// Function call; the name is upper-case without `_xlfn.`.
    Call(String, Vec<Expr>),
    /// Calling the result of an expression, e.g. `LAMBDA(x,x+1)(2)`.
    Invoke(Box<Expr>, Vec<Expr>),
    /// An omitted argument `F(1,,3)`.
    Missing,
    Paren(Box<Expr>),
}

impl Expr {
    /// Visits every node depth-first.
    pub fn walk<'a>(&'a self, f: &mut dyn FnMut(&'a Expr)) {
        f(self);
        match self {
            Expr::Array(rows) => rows.iter().flatten().for_each(|e| e.walk(f)),
            Expr::Unary(_, e) | Expr::Paren(e) => e.walk(f),
            Expr::Keyed(k) => k.conditions.iter().for_each(|c| c.value.walk(f)),
            Expr::Binary(_, a, b) => {
                a.walk(f);
                b.walk(f);
            }
            Expr::Call(_, args) => args.iter().for_each(|e| e.walk(f)),
            Expr::Invoke(c, args) => {
                c.walk(f);
                args.iter().for_each(|e| e.walk(f));
            }
            _ => {}
        }
    }
    /// Rewrites every node bottom-up.
    pub fn map(self, f: &mut dyn FnMut(Expr) -> Expr) -> Expr {
        let e = match self {
            Expr::Array(rows) => Expr::Array(rows.into_iter().map(|r| r.into_iter().map(|e| e.map(f)).collect()).collect()),
            Expr::Unary(op, e) => Expr::Unary(op, Box::new(e.map(f))),
            Expr::Paren(e) => Expr::Paren(Box::new(e.map(f))),
            Expr::Keyed(k) => {
                Expr::Keyed(KeyedRef { conditions: k.conditions.into_iter().map(|c| Condition { value: c.value.map(f), ..c }).collect(), ..k })
            }
            Expr::Binary(op, a, b) => Expr::Binary(op, Box::new(a.map(f)), Box::new(b.map(f))),
            Expr::Call(n, args) => Expr::Call(n, args.into_iter().map(|e| e.map(f)).collect()),
            Expr::Invoke(c, args) => Expr::Invoke(Box::new(c.map(f)), args.into_iter().map(|e| e.map(f)).collect()),
            other => other,
        };
        f(e)
    }
    /// All references in the formula.
    pub fn references(&self) -> Vec<&Reference> {
        let mut v = Vec::new();
        self.walk(&mut |e| {
            if let Expr::Ref(r) = e {
                v.push(r);
            }
        });
        v
    }
    /// Function names used (upper-case).
    pub fn functions(&self) -> Vec<&str> {
        let mut v = Vec::new();
        self.walk(&mut |e| {
            if let Expr::Call(n, _) = e {
                v.push(n.as_str());
            }
        });
        v
    }
}

pub(crate) fn clamp_anchor(row: i64, col: i64) -> Option<(u32, u32)> {
    if (0..MAX_ROWS as i64).contains(&row) && (0..MAX_COLS as i64).contains(&col) { Some((row as u32, col as u32)) } else { None }
}
