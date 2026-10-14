//! Pratt parser: tokens → [`Expr`].

use std::sync::Arc;

use gridcraft_core::addr::{letters_to_col, parse_a1_prefix};
use gridcraft_core::{CellError, MAX_ROWS};

use crate::ast::*;
use crate::lexer::{Tok, Token, tokenize_locale};

#[derive(Clone, Debug, PartialEq, thiserror::Error)]
#[error("{msg}")]
pub struct ParseError {
    pub msg: String,
    pub pos: usize,
}

pub(crate) const MAX_DEPTH: usize = 256;

/// Parses formula text. A leading `=` (or `+`/`-` as Lotus users type) is accepted and skipped
/// for `=`.
pub fn parse(src: &str) -> Result<Expr, ParseError> {
    parse_syntax(src, crate::FormulaLocale::En)
}

pub(crate) fn parse_syntax(src: &str, locale: crate::FormulaLocale) -> Result<Expr, ParseError> {
    let body = src.strip_prefix('=').unwrap_or(src);
    let toks = tokenize_locale(body, locale).map_err(|e| ParseError { msg: e.msg, pos: e.pos })?;
    let toks = significant_spaces(toks);
    let mut p = Parser { toks, i: 0, depth: 0, input: locale == crate::FormulaLocale::Es };
    let e = p.expr(0, false)?;
    match p.peek() {
        Tok::Eof => Ok(e),
        t => Err(p.err(format!("unexpected {}", describe(t)))),
    }
}

fn describe(t: &Tok) -> String {
    match t {
        Tok::RParen => "`)`".into(),
        Tok::Comma => "`,`".into(),
        Tok::Eof => "end of formula".into(),
        other => format!("{other:?}"),
    }
}

/// Keeps a space token only where it is the intersection operator: between the end of an
/// operand and the start of another.
fn significant_spaces(toks: Vec<Token>) -> Vec<Token> {
    let mut out: Vec<Token> = Vec::with_capacity(toks.len());
    for (i, t) in toks.iter().enumerate() {
        if t.tok == Tok::Space {
            let prev_end = out.last().is_some_and(|p| matches!(p.tok, Tok::Word(_) | Tok::RParen | Tok::Struct(_) | Tok::Error(_)));
            let next_start =
                toks.get(i + 1).is_some_and(|n| matches!(n.tok, Tok::Word(_) | Tok::Sheet(..) | Tok::LParen | Tok::Func(_) | Tok::Error(_)));
            if prev_end && next_start {
                out.push(t.clone());
            }
            continue;
        }
        out.push(t.clone());
    }
    out
}

struct Parser {
    toks: Vec<Token>,
    i: usize,
    depth: usize,
    input: bool,
}

fn word_is_cols(w: &str) -> Option<(u32, bool)> {
    let abs = w.starts_with('$');
    letters_to_col(w.trim_start_matches('$')).map(|c| (c, abs))
}

fn word_is_row(w: &str) -> Option<(u32, bool)> {
    let abs = w.starts_with('$');
    let n: u32 = w.trim_start_matches('$').parse().ok()?;
    if n == 0 || n > MAX_ROWS { None } else { Some((n - 1, abs)) }
}

fn word_is_cell(w: &str) -> Option<Anchor> {
    let (col, row, col_abs, row_abs, rest) = parse_a1_prefix(w)?;
    if !rest.is_empty() {
        return None;
    }
    Some(Anchor { row, col, row_abs, col_abs })
}

/// `_xlfn.XLOOKUP` → `XLOOKUP`. `_xludf.` is kept (`_xludf.SUM`): it marks a user-defined
/// function, never the built-in of the same name, and files need it back.
pub fn normalize_function_name(name: &str) -> String {
    let u = name.to_ascii_uppercase();
    for p in ["_XLFN._XLWS.", "_XLFN.", "_XLWS."] {
        if let Some(r) = u.strip_prefix(p) {
            return r.to_string();
        }
    }
    match u.strip_prefix("_XLUDF.") {
        Some(r) => format!("_xludf.{r}"),
        None => u,
    }
}

impl Parser {
    fn peek(&self) -> &Tok {
        self.toks.get(self.i).map(|t| &t.tok).unwrap_or(&Tok::Eof)
    }
    fn peek_at(&self, k: usize) -> &Tok {
        self.toks.get(self.i + k).map(|t| &t.tok).unwrap_or(&Tok::Eof)
    }
    fn pos(&self) -> usize {
        self.toks.get(self.i).map(|t| t.start).unwrap_or(0)
    }
    fn next(&mut self) -> Tok {
        let t = self.peek().clone();
        if self.i < self.toks.len() {
            self.i += 1;
        }
        t
    }
    fn err(&self, msg: impl Into<String>) -> ParseError {
        ParseError { msg: msg.into(), pos: self.pos() }
    }
    fn expect(&mut self, t: Tok) -> Result<(), ParseError> {
        if *self.peek() == t {
            self.next();
            Ok(())
        } else {
            Err(self.err(format!("expected {}, found {}", describe(&t), describe(self.peek()))))
        }
    }

    fn binop(&self, allow_union: bool) -> Option<BinOp> {
        Some(match self.peek() {
            Tok::Op("+") => BinOp::Add,
            Tok::Op("-") => BinOp::Sub,
            Tok::Op("*") => BinOp::Mul,
            Tok::Op("/") => BinOp::Div,
            Tok::Op("^") => BinOp::Pow,
            Tok::Op("&") => BinOp::Concat,
            Tok::Op("=") => BinOp::Eq,
            Tok::Op("<>") => BinOp::Ne,
            Tok::Op("<") => BinOp::Lt,
            Tok::Op(">") => BinOp::Gt,
            Tok::Op("<=") => BinOp::Le,
            Tok::Op(">=") => BinOp::Ge,
            Tok::Colon => BinOp::Range,
            Tok::Space => BinOp::Intersect,
            Tok::Comma if allow_union => BinOp::Union,
            _ => return None,
        })
    }

    fn expr(&mut self, min_bp: u8, allow_union: bool) -> Result<Expr, ParseError> {
        self.depth += 1;
        if self.depth > MAX_DEPTH {
            return Err(self.err("formula is nested too deeply"));
        }
        let mut lhs = self.prefix(allow_union)?;
        loop {
            if *self.peek() == Tok::Op("%") {
                if 70 < min_bp {
                    break;
                }
                self.next();
                lhs = Expr::Unary(UnOp::Percent, Box::new(lhs));
                continue;
            }
            if *self.peek() == Tok::LParen && matches!(lhs, Expr::Call(..) | Expr::Invoke(..) | Expr::Paren(_) | Expr::Name(_)) {
                // Invoking a LAMBDA result.
                if let Expr::Name(_) = lhs {
                    // A name followed by '(' was lexed as Func; this is `(name)(…)` only.
                }
                self.next();
                let args = self.args()?;
                lhs = Expr::Invoke(Box::new(lhs), args);
                continue;
            }
            if let Tok::Trim(mode) = *self.peek() {
                // A trim reference is the range operator with a modifier: same binding power.
                let bp = BinOp::Range.precedence();
                if bp < min_bp {
                    break;
                }
                self.next();
                let rhs = self.expr(bp + 1, allow_union)?;
                lhs = Expr::Unary(UnOp::Trim(mode), Box::new(combine(BinOp::Range, lhs, rhs)));
                continue;
            }
            let Some(op) = self.binop(allow_union) else { break };
            let bp = op.precedence();
            if bp < min_bp {
                break;
            }
            self.next();
            let rhs = self.expr(bp + 1, allow_union)?;
            lhs = combine(op, lhs, rhs);
        }
        self.depth -= 1;
        Ok(lhs)
    }

    fn args(&mut self) -> Result<Vec<Expr>, ParseError> {
        let mut args = Vec::new();
        if *self.peek() == Tok::RParen {
            self.next();
            return Ok(args);
        }
        loop {
            let a = match self.peek() {
                Tok::Comma | Tok::RParen => Expr::Missing,
                _ => self.expr(0, false)?,
            };
            args.push(a);
            match self.next() {
                Tok::Comma => continue,
                Tok::RParen => break,
                t => return Err(self.err(format!("expected `,` or `)`, found {}", describe(&t)))),
            }
        }
        if args.len() > 255 {
            return Err(self.err("too many arguments"));
        }
        Ok(args)
    }

    fn prefix(&mut self, allow_union: bool) -> Result<Expr, ParseError> {
        match self.next() {
            Tok::Number(n) => Ok(Expr::Number(n)),
            Tok::Text(s) => Ok(Expr::Text(Arc::from(s))),
            Tok::Bool(b) => Ok(Expr::Bool(b)),
            Tok::Error(e) => Ok(Expr::Error(e)),
            Tok::Op("-") => Ok(Expr::Unary(UnOp::Neg, Box::new(self.expr(60, allow_union)?))),
            Tok::Op("+") => Ok(Expr::Unary(UnOp::Plus, Box::new(self.expr(60, allow_union)?))),
            Tok::Op("@") => Ok(Expr::Unary(UnOp::At, Box::new(self.expr(95, allow_union)?))),
            Tok::LParen => {
                let e = self.expr(0, true)?;
                self.expect(Tok::RParen)?;
                Ok(Expr::Paren(Box::new(e)))
            }
            Tok::LBrace => self.array(),
            Tok::Func(name) => {
                let name = normalize_function_name(&name);
                let mut args = self.args()?;
                // Files write `A1#` as `_xlfn.ANCHORARRAY(A1)`.
                if name == "ANCHORARRAY"
                    && let [Expr::Ref(Reference { kind: RefKind::Cell(_), .. })] = args.as_slice()
                    && let Some(r) = args.pop()
                {
                    return Ok(Expr::Unary(UnOp::Spill, Box::new(r)));
                }
                // Files write `@x` as `_xlfn.SINGLE(x)`.
                if name == "SINGLE"
                    && args.len() == 1
                    && let Some(x) = args.pop()
                {
                    return Ok(Expr::Unary(UnOp::At, Box::new(x)));
                }
                // Files may spell a trim reference as a function call (`_xlfn._TRO_ALL(A1:E10)`).
                let trim = match name.as_str() {
                    "_TRO_LEADING" => Some(TrimMode::Leading),
                    "_TRO_TRAILING" => Some(TrimMode::Trailing),
                    "_TRO_ALL" => Some(TrimMode::All),
                    _ => None,
                };
                if let Some(mode) = trim
                    && args.len() == 1
                    && let Some(x) = args.pop()
                {
                    return Ok(Expr::Unary(UnOp::Trim(mode), Box::new(x)));
                }
                Ok(Expr::Call(name, args))
            }
            Tok::Sheet(a, b) => {
                let sheet = match b {
                    Some(b) => SheetSel::Span(a, b),
                    None => SheetSel::Named(a),
                };
                match self.next() {
                    Tok::Word(w) => self.reference_from_word(sheet, &w),
                    Tok::Error(CellError::Ref) => Ok(Expr::Error(CellError::Ref)),
                    t => Err(self.err(format!("expected a reference after the sheet name, found {}", describe(&t)))),
                }
            }
            Tok::Word(w) => {
                if let Tok::Struct(_) = self.peek()
                    && word_is_cell(&w).is_none()
                {
                    let pos = self.pos();
                    let Tok::Struct(body) = self.next() else { return Err(self.err("internal")) };
                    return self.structured(&w, &body, pos);
                }
                self.reference_from_word(SheetSel::Current, &w)
            }
            Tok::Struct(body) => self.structured("", &body, self.pos()),
            t => Err(self.err(format!("unexpected {}", describe(&t)))),
        }
    }

    /// A word after an optional sheet: a cell, a column/row range (`A:C`, `3:5`) or a name. A
    /// trim operator (`A1:.E10`, `A:.A`) in place of the colon wraps the range in
    /// [`UnOp::Trim`].
    fn reference_from_word(&mut self, sheet: SheetSel, w: &str) -> Result<Expr, ParseError> {
        let trim = match self.peek() {
            Tok::Colon => Some(None),
            Tok::Trim(mode) => Some(Some(*mode)),
            _ => None,
        };
        let wrap = |kind: RefKind, sheet: SheetSel| {
            let r = Expr::Ref(Reference { sheet, kind });
            match trim {
                Some(Some(mode)) => Expr::Unary(UnOp::Trim(mode), Box::new(r)),
                _ => r,
            }
        };
        if let Some(a) = word_is_cell(w) {
            // Cell, possibly `A1:B2` (merged here so the sheet applies to both ends).
            if trim.is_some()
                && let Tok::Word(w2) = self.peek_at(1)
                && let Some(b) = word_is_cell(w2)
            {
                self.next();
                self.next();
                return Ok(wrap(RefKind::Range(a, b), sheet));
            }
            let cell = Expr::Ref(Reference { sheet, kind: RefKind::Cell(a) });
            if *self.peek() == Tok::Op("#") {
                self.next();
                return Ok(Expr::Unary(UnOp::Spill, Box::new(cell)));
            }
            return Ok(cell);
        }
        let second = match self.peek_at(1) {
            Tok::Word(w2) => Some(w2.clone()),
            Tok::Number(n) if n.fract() == 0.0 && *n >= 1.0 => Some(format!("{n}")),
            _ => None,
        };
        if trim.is_some()
            && let Some(w2) = second
        {
            if let (Some((c0, a0)), Some((c1, a1))) = (word_is_cols(w), word_is_cols(&w2)) {
                self.next();
                self.next();
                return Ok(wrap(RefKind::Cols(c0, a0, c1, a1), sheet));
            }
            if let (Some((r0, a0)), Some((r1, a1))) = (word_is_row(w), word_is_row(&w2)) {
                self.next();
                self.next();
                return Ok(wrap(RefKind::Rows(r0, a0, r1, a1), sheet));
            }
        }
        if sheet != SheetSel::Current {
            // Sheet-scoped name `Sheet1!MyName`.
            if let SheetSel::Named(s) = sheet {
                return Ok(Expr::Name(format!("{s}!{w}")));
            }
            return Err(self.err("invalid 3-D reference"));
        }
        if w.chars().next().is_some_and(|c| c.is_ascii_digit()) {
            return Err(self.err(format!("unexpected `{w}`")));
        }
        // A function passed by name is saved as `_xleta.SUM`.
        if let Some(f) = w.get(..7).filter(|p| p.eq_ignore_ascii_case("_xleta.")).and_then(|_| w.get(7..)) {
            return Ok(Expr::Name(f.to_ascii_uppercase()));
        }
        Ok(Expr::Name(w.to_string()))
    }

    fn array(&mut self) -> Result<Expr, ParseError> {
        let mut rows = vec![Vec::new()];
        loop {
            let el = match self.next() {
                Tok::Number(n) => Expr::Number(n),
                Tok::Op("-") => match self.next() {
                    Tok::Number(n) => Expr::Number(-n),
                    t => return Err(self.err(format!("expected a number in array, found {}", describe(&t)))),
                },
                Tok::Op("+") => match self.next() {
                    Tok::Number(n) => Expr::Number(n),
                    t => return Err(self.err(format!("expected a number in array, found {}", describe(&t)))),
                },
                Tok::Text(s) => Expr::Text(Arc::from(s)),
                Tok::Bool(b) => Expr::Bool(b),
                Tok::Word(w) if self.input => crate::input::boolean_alias(&w)
                    .map(Expr::Bool)
                    .ok_or_else(|| self.err(format!("array constants may only contain constants, found {}", describe(&Tok::Word(w)))))?,
                Tok::Error(e) => Expr::Error(e),
                t => return Err(self.err(format!("array constants may only contain constants, found {}", describe(&t)))),
            };
            if let Some(r) = rows.last_mut() {
                r.push(el);
            }
            match self.next() {
                Tok::Comma => {}
                Tok::Semicolon => rows.push(Vec::new()),
                Tok::RBrace => break,
                t => return Err(self.err(format!("expected `,`, `;` or `}}`, found {}", describe(&t)))),
            }
        }
        let w = rows.first().map_or(0, Vec::len);
        if rows.iter().any(|r| r.len() != w) {
            return Err(self.err("array rows must have the same length"));
        }
        Ok(Expr::Array(rows))
    }
}

/// Folds `A1:B2`-like range operations of two plain refs into a single reference.
fn combine(op: BinOp, lhs: Expr, rhs: Expr) -> Expr {
    if op == BinOp::Range
        && let (Expr::Ref(a), Expr::Ref(b)) = (&lhs, &rhs)
        && b.sheet == SheetSel::Current
        && let (RefKind::Cell(x), RefKind::Cell(y)) = (&a.kind, &b.kind)
    {
        return Expr::Ref(Reference { sheet: a.sheet.clone(), kind: RefKind::Range(*x, *y) });
    }
    Expr::Binary(op, Box::new(lhs), Box::new(rhs))
}

impl Parser {
    /// The inside of `Table[...]`: a structured reference, or a keyed one: `Table[Col,key]`, or
    /// `Table[Col,Stock>400,Name<>"Nut"]` with any number of conditions. Keys and values are
    /// expressions in file syntax in every locale, like the rest of the brackets.
    fn structured(&self, table: &str, body: &str, pos: usize) -> Result<Expr, ParseError> {
        let Some((column, column_abs, rest)) = split_keyed(body) else {
            return parse_struct(table, body).map(Expr::Struct).map_err(|msg| ParseError { msg, pos });
        };
        let err = |msg: &str| ParseError { msg: msg.into(), pos };
        // `Table[Qty,[#Prev Row]]`: a row near the formula's, not a key.
        let row = match bracketed(rest.trim()).map(|i| i.trim().to_ascii_lowercase()).as_deref() {
            Some("#this row") => Some(StructItem::ThisRow),
            Some("#prev row") => Some(StructItem::PrevRow),
            Some("#next row") => Some(StructItem::NextRow),
            _ => None,
        };
        if let Some(item) = row {
            return Ok(Expr::Struct(StructRef {
                table: table.to_string(),
                specifiers: vec![item],
                col_start: Some(column),
                col_end: None,
                col_abs: column_abs,
            }));
        }
        let parts = split_top_level(rest);
        let mut conditions = Vec::with_capacity(parts.len());
        for part in &parts {
            if part.trim().is_empty() {
                return Err(err("a keyed table reference needs a key or a condition after each `,`"));
            }
            let condition = match split_condition(part) {
                Some((name, column_abs, op, value)) => {
                    if value.trim().is_empty() {
                        return Err(err("a condition needs a value after its comparison"));
                    }
                    Condition { column: Some(name), column_abs, op, value: self.nested(value, pos)? }
                }
                None if parts.len() > 1 => return Err(err("with several conditions, each names its column: `Stock>400`")),
                None => Condition { column: None, column_abs: false, op: BinOp::Eq, value: self.nested(part, pos)? },
            };
            conditions.push(condition);
        }
        Ok(Expr::Keyed(KeyedRef { table: table.to_string(), column, column_abs, conditions }))
    }

    /// A key or value inside the brackets.
    fn nested(&self, src: &str, pos: usize) -> Result<Expr, ParseError> {
        // A key can hold another keyed reference: nesting counts against the same limit, and
        // more steeply, as each level parses with a parser of its own.
        let depth = self.depth.saturating_add(KEYED_DEPTH);
        if depth > MAX_DEPTH {
            return Err(ParseError { msg: "formula is nested too deeply".into(), pos });
        }
        let toks = tokenize_locale(src, crate::FormulaLocale::En).map_err(|e| ParseError { msg: e.msg, pos })?;
        let mut p = Parser { toks: significant_spaces(toks), i: 0, depth, input: false };
        let e = p.expr(0, false).map_err(|e| ParseError { msg: e.msg, pos })?;
        if *p.peek() != Tok::Eof {
            return Err(ParseError { msg: format!("unexpected {} in the key of a table reference", describe(p.peek())), pos });
        }
        Ok(e)
    }
}

const KEYED_DEPTH: usize = 8;

/// `'[` → `[`: the escapes of a column name.
fn unescape_column(s: &str) -> String {
    let mut out = String::new();
    let mut it = s.chars();
    while let Some(c) = it.next() {
        match c {
            '\'' => out.extend(it.next()),
            c => out.push(c),
        }
    }
    out
}

/// `$Qty` / `$[Unit Price]` → the column as written without its anchor, and whether it had one.
fn anchored(s: &str) -> (&str, bool) {
    match s.strip_prefix('$') {
        Some(rest) => (rest.trim_start(), true),
        None => (s, false),
    }
}

/// `[Name]` → `Name` when the whole text is one bracket group.
fn bracketed(s: &str) -> Option<&str> {
    let inner = s.strip_prefix('[')?.strip_suffix(']')?;
    let mut esc = false;
    for c in inner.chars() {
        match c {
            _ if esc => esc = false,
            '\'' => esc = true,
            '[' | ']' => return None,
            _ => {}
        }
    }
    (!esc).then_some(inner)
}

/// A keyed reference body `Col,key…` → the column and the text after the comma. Excel only
/// puts a comma after a `#` item (or inside a bracketed column name), so a column followed by
/// one is never an Excel structured reference.
fn split_keyed(body: &str) -> Option<(String, bool, &str)> {
    let mut depth = 0usize;
    let mut esc = false;
    let comma = body.char_indices().find_map(|(i, c)| {
        match c {
            _ if esc => esc = false,
            '\'' => esc = true,
            '[' => depth += 1,
            ']' => depth = depth.saturating_sub(1),
            ',' if depth == 0 => return Some(i),
            _ => {}
        }
        None
    })?;
    let (first, abs) = anchored(body.get(..comma)?.trim());
    let name = match bracketed(first) {
        Some(inner) => inner,
        None if first.contains(['[', ']']) => return None,
        None => first,
    };
    if name.is_empty() || name.starts_with(['#', '@']) {
        return None;
    }
    Some((unescape_column(name), abs, body.get(comma + 1..)?))
}

/// Steps through text after a keyed reference's column: `on(i, c, top)` sees each character
/// and whether it is outside brackets, parentheses and text literals. A quote inside brackets
/// belongs to a column name (`[5" pipe]`) or to a nested reference.
fn scan_top_level(s: &str, mut on: impl FnMut(usize, char, bool) -> bool) {
    let (mut brackets, mut parens) = (0usize, 0usize);
    let mut text = false;
    let mut esc = false;
    for (i, c) in s.char_indices() {
        let top = !text && brackets == 0 && parens == 0;
        match c {
            _ if esc => esc = false,
            '\'' if brackets > 0 => esc = true,
            '"' if brackets == 0 => text = !text,
            _ if text => {}
            '[' => brackets += 1,
            ']' => brackets = brackets.saturating_sub(1),
            '(' | '{' => parens += 1,
            ')' | '}' => parens = parens.saturating_sub(1),
            _ => {}
        }
        if on(i, c, top) {
            return;
        }
    }
}

/// `Stock>400,Name<>"Nut"` → its conditions' texts.
fn split_top_level(s: &str) -> Vec<&str> {
    let mut parts = Vec::new();
    let mut from = 0;
    scan_top_level(s, |i, c, top| {
        if top && c == ',' {
            parts.push(s.get(from..i).unwrap_or(""));
            from = i + 1;
        }
        false
    });
    parts.push(s.get(from..).unwrap_or(""));
    parts
}

/// `Stock>400` / `[In Stock]<>"no"` / `$Stock=1` → the column, its anchor, the comparison and
/// the value's text. Anything else before a comparison (a cell, a number, an expression) makes
/// the whole text a key that happens to compare.
fn split_condition(part: &str) -> Option<(String, bool, BinOp, &str)> {
    let mut at = None;
    scan_top_level(part, |i, c, top| {
        if top && matches!(c, '<' | '>' | '=') {
            at = Some(i);
            return true;
        }
        false
    });
    let at = at?;
    let after = part.get(at..)?;
    let (op, len) = match () {
        _ if after.starts_with("<=") => (BinOp::Le, 2),
        _ if after.starts_with(">=") => (BinOp::Ge, 2),
        _ if after.starts_with("<>") => (BinOp::Ne, 2),
        _ if after.starts_with('<') => (BinOp::Lt, 1),
        _ if after.starts_with('>') => (BinOp::Gt, 1),
        _ => (BinOp::Eq, 1),
    };
    let (lhs, abs) = anchored(part.get(..at)?.trim());
    let name = match bracketed(lhs) {
        Some(inner) if !inner.is_empty() && !inner.starts_with(['#', '@']) => unescape_column(inner),
        None if bare_key_column(lhs) => lhs.to_string(),
        _ => return None,
    };
    Some((name, abs, op, part.get(at + len..)?))
}

/// Whether a key column can be written without brackets before `=`: a plain word that is not
/// a cell address, a number or a boolean.
pub(crate) fn bare_key_column(name: &str) -> bool {
    name.chars().next().is_some_and(|c| c.is_alphabetic() || c == '_')
        && name.chars().all(|c| c.is_alphanumeric() || c == '_')
        && word_is_cell(name).is_none()
        && !name.eq_ignore_ascii_case("TRUE")
        && !name.eq_ignore_ascii_case("FALSE")
}

/// Parses the inside of `Table[...]`. Text between the bracketed items may only be the `,` and
/// `:` separators and the `$` that anchors the column after it, except one unbracketed column
/// after `[#This Row],` (`[[#This Row],Units]`, how some writers spell `[@Units]`); anything
/// else is an error rather than dropped.
pub fn parse_struct(table: &str, body: &str) -> Result<StructRef, String> {
    let mut specifiers = Vec::new();
    // Column names, and whether each has a `$` anchor.
    let mut cols: Vec<(String, bool)> = Vec::new();
    // Spaces at the edges of a column name are part of it (`Table1[Qty ]` names `Qty `), so the
    // untrimmed body is what a lone column is read from.
    let raw = body;
    let body = body.trim();
    let item = |s: &str| -> Option<StructItem> {
        match s.trim().to_ascii_lowercase().as_str() {
            "#all" => Some(StructItem::All),
            "#data" => Some(StructItem::Data),
            "#headers" => Some(StructItem::Headers),
            "#totals" => Some(StructItem::Totals),
            "#this row" => Some(StructItem::ThisRow),
            "#prev row" => Some(StructItem::PrevRow),
            "#next row" => Some(StructItem::NextRow),
            _ => None,
        }
    };
    let unexpected = |gap: &str| format!("unexpected `{gap}` in structured reference");
    // A column name may hold `[` and `]` only when escaped with `'`.
    let column = |s: &str| -> Result<String, String> {
        let mut esc = false;
        for c in s.chars() {
            match c {
                _ if esc => esc = false,
                '\'' => esc = true,
                '[' | ']' => return Err(unexpected(s)),
                _ => {}
            }
        }
        Ok(unescape_column(s))
    };
    // `Qty`, `[Qty]`, `$Qty` or `$[Unit Price]` after an `@`, or alone with an anchor.
    let one = |s: &str| -> Result<(String, bool), String> {
        let (s, abs) = anchored(s);
        let name = bracketed(s).unwrap_or(s);
        // An anchor is for a column, not for an item such as `#Headers`.
        if abs && name.starts_with('#') {
            return Err(unexpected("$"));
        }
        Ok((column(name)?, abs))
    };
    if !body.starts_with('[') {
        // Simple: [Col], [@Col], [#Headers], [@], [$Col]
        if let Some(rest) = body.strip_prefix('@') {
            specifiers.push(StructItem::ThisRow);
            let rest = rest.trim();
            if !rest.is_empty() {
                cols.push(one(rest)?);
            }
        } else if let Some(i) = item(body) {
            specifiers.push(i);
        } else if body.starts_with('$') {
            cols.push(one(body)?);
        } else if !body.is_empty() {
            cols.push((column(raw)?, false));
        }
    } else {
        // [[#Headers],[Col1]:[Col3]], [[#This Row],[Col 1]], [[#This Row],Col1] or [[#Headers],$[Col1]]
        let mut parts: Vec<(String, bool)> = Vec::new();
        let mut depth = 0;
        let mut cur = String::new();
        // Text between bracketed items.
        let mut gap = String::new();
        let mut abs = false;
        let mut esc = false;
        for c in body.chars() {
            if esc {
                cur.push(c);
                esc = false;
                continue;
            }
            match c {
                '\'' if depth > 0 => {
                    cur.push(c);
                    esc = true;
                }
                '[' => {
                    if depth == 0 {
                        let (separator, anchor) = match gap.trim().strip_suffix('$') {
                            Some(before) => (before.trim_end(), true),
                            None => (gap.trim(), false),
                        };
                        if !matches!(separator, "" | "," | ":") {
                            return Err(unexpected(gap.trim()));
                        }
                        abs = anchor;
                        gap.clear();
                    }
                    depth += 1;
                    if depth > 1 {
                        cur.push(c);
                    }
                }
                ']' => {
                    if depth == 0 {
                        return Err(unexpected("]"));
                    }
                    depth -= 1;
                    if depth > 0 {
                        cur.push(c);
                    } else {
                        parts.push((std::mem::take(&mut cur), abs));
                    }
                }
                _ if depth > 0 => cur.push(c),
                _ => gap.push(c),
            }
        }
        if depth > 0 {
            return Err("unclosed `[` in structured reference".to_string());
        }
        for (p, abs) in parts {
            match item(&p) {
                Some(_) if abs => return Err(unexpected("$")),
                Some(i) => specifiers.push(i),
                None => cols.push((column(&p)?, abs)),
            }
        }
        let tail = gap.trim();
        if !tail.is_empty() {
            let (bare, abs) = anchored(tail.strip_prefix(',').map(str::trim).unwrap_or(""));
            let simple = !bare.is_empty() && !bare.chars().any(|c| matches!(c, '[' | ']' | '#' | '\'' | ',' | ':' | '$'));
            if !(simple && specifiers == [StructItem::ThisRow] && cols.is_empty()) {
                return Err(unexpected(tail));
            }
            cols.push((bare.to_string(), abs));
        }
    }
    if cols.len() > 2 {
        return Err("a structured reference names at most two columns".to_string());
    }
    if specifiers.len() > 1 && specifiers.iter().any(|i| matches!(i, StructItem::PrevRow | StructItem::NextRow)) {
        return Err("`[#Prev Row]` and `[#Next Row]` go with columns only, not with other items".to_string());
    }
    let mut it = cols.into_iter();
    let (start, end) = (it.next(), it.next());
    let (col_start, col_end, col_abs) = match (start, end) {
        // Excel's idiom for a column that stays when filled sideways: `[[Qty]:[Qty]]`.
        (Some((a, _)), Some((b, _))) if a.eq_ignore_ascii_case(&b) => (Some(a), None, true),
        (Some((_, abs_a)), Some((_, abs_b))) if abs_a || abs_b => return Err("`$` anchors one column, not a range of columns".to_string()),
        (Some((a, _)), Some((b, _))) => (Some(a), Some(b), false),
        (Some((a, abs)), None) => (Some(a), None, abs),
        (None, _) => (None, None, false),
    };
    Ok(StructRef { table: table.to_string(), specifiers, col_start, col_end, col_abs })
}
