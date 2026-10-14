//! In-cell / formula-bar editing state: point mode, reference colouring, function
//! autocomplete and argument hints.

use egui::Color32;
use gridcraft_engine::core::{CellRef, RangeRef};

/// Colours for references in a formula being edited (our own palette).
pub const REF_COLORS: [Color32; 8] = [
    Color32::from_rgb(0x2E, 0x6F, 0xD8),
    Color32::from_rgb(0xD0, 0x3C, 0x3C),
    Color32::from_rgb(0x7A, 0x4F, 0xC9),
    Color32::from_rgb(0x2B, 0x9A, 0x4A),
    Color32::from_rgb(0xC2, 0x5E, 0x1E),
    Color32::from_rgb(0x1D, 0x92, 0xA6),
    Color32::from_rgb(0xB8, 0x3B, 0x8E),
    Color32::from_rgb(0x8A, 0x7A, 0x1F),
];

/// What accepting an autocomplete entry does.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Completion {
    /// A function name: the word before the caret becomes `NAME(`.
    Function,
    /// A table name, a column of a table or a key of a keyed table reference: `text` is
    /// written over the formula from byte `start` to the caret.
    Insert { start: usize, text: String },
}

#[derive(Clone, Debug)]
pub struct EditState {
    pub sheet: usize,
    /// Syntax of this edit, captured when it starts (prevents a language switch reinterpreting it).
    pub locale: gridcraft_engine::formula::FormulaLocale,
    pub cell: CellRef,
    pub text: String,
    /// Started by typing (Enter mode: arrows commit) rather than F2 (Edit mode: arrows move the caret).
    pub enter_mode: bool,
    pub from_formula_bar: bool,
    /// Caret position (char index), mirrored from the text widget.
    pub caret: usize,
    /// The reference most recently inserted by point mode (byte range) — arrows/clicks replace it.
    pub point: Option<(usize, usize)>,
    /// Where point mode's keyboard cursor is.
    pub point_cell: Option<CellRef>,
    /// The cells the reference at `point` covers, whatever it is written as (`B2:B9` or
    /// `Products[Stock]`): what extending it with Shift+arrows starts from.
    pub point_range: Option<RangeRef>,
    /// The reference at `point` started by table name; one that started as an address (outside
    /// a table) stays an address while arrows or a drag carry it on, into a table too.
    pub point_named: bool,
    pub autocomplete: Vec<String>,
    /// What each `autocomplete` entry does; an entry without one is a function name.
    pub completions: Vec<Completion>,
    pub ac_index: usize,
    pub request_focus: bool,
    /// The text changed programmatically: push `caret` into the widget on the next frame.
    pub sync_caret: bool,
    /// Column AutoComplete: the full entry the typed text completes to.
    pub completion: Option<String>,
    /// Move the caret to the end on the next frame.
    pub caret_to_end: bool,
}

impl EditState {
    pub fn new(sheet: usize, cell: CellRef, text: String, enter_mode: bool, from_formula_bar: bool) -> EditState {
        let caret = text.chars().count();
        EditState {
            sheet,
            locale: Default::default(),
            cell,
            text,
            enter_mode,
            from_formula_bar,
            caret,
            point: None,
            point_cell: None,
            point_range: None,
            point_named: false,
            autocomplete: vec![],
            completions: vec![],
            ac_index: 0,
            request_focus: true,
            sync_caret: true,
            caret_to_end: true,
            completion: None,
        }
    }

    pub fn is_formula(&self) -> bool {
        self.text.starts_with('=')
            || (self.text.starts_with('+') && self.text.len() > 1)
            || (self.text.starts_with('-') && self.text.len() > 1 && !self.text[1..].starts_with(|c: char| c.is_ascii_digit()))
    }

    fn byte_caret(&self) -> usize {
        self.text.char_indices().nth(self.caret).map(|(i, _)| i).unwrap_or(self.text.len())
    }

    /// Point mode: the caret follows an operator, `(`, `,` or `=` in a formula, so clicking a
    /// cell or pressing an arrow inserts a reference.
    pub fn can_point(&self) -> bool {
        if !self.is_formula() {
            return false;
        }
        if let Some((_, e)) = self.point
            && e == self.byte_caret()
        {
            return true;
        }
        let before = self.text.get(..self.byte_caret()).unwrap_or("");
        let last = before.trim_end().chars().last();
        matches!(last, Some('=' | '(' | ',' | '+' | '-' | '*' | '/' | '^' | '&' | '<' | '>' | ':' | ';' | '{'))
    }

    /// Inserts (or replaces the last pointed) reference text at the caret.
    pub fn insert_ref(&mut self, r: &str) {
        let caret_b = self.byte_caret();
        let (start, end) = match self.point {
            Some((s, e)) if e == caret_b => (s, e),
            _ => (caret_b, caret_b),
        };
        let (Some(a), Some(b)) = (self.text.get(..start), self.text.get(end..)) else { return };
        self.text = format!("{a}{r}{b}");
        let new_end = start + r.len();
        self.point = Some((start, new_end));
        self.caret = self.text.get(..new_end).map(|s| s.chars().count()).unwrap_or(0);
        self.caret_to_end = false;
        self.sync_caret = true;
        self.point_range = None;
        self.point_named = false;
    }

    /// Point mode: inserts (or replaces the last pointed) reference to `range`, written as `text`
    /// (by table name when `named`).
    pub fn insert_point(&mut self, text: &str, range: RangeRef, named: bool) {
        self.insert_ref(text);
        self.point_range = Some(range);
        self.point_named = named;
    }

    /// Whether the next point-mode reference replaces the one just pointed at (arrows, a drag).
    pub fn continues_point(&self) -> bool {
        self.point.is_some_and(|(_, e)| e == self.byte_caret())
    }

    /// Updates function autocomplete for the word before the caret.
    pub fn update_autocomplete(&mut self, names: &[String]) {
        self.autocomplete.clear();
        self.completions.clear();
        if !self.is_formula() {
            return;
        }
        let before = self.text.get(..self.byte_caret()).unwrap_or("");
        let word: String =
            before.chars().rev().take_while(|c| c.is_ascii_alphanumeric() || *c == '.' || *c == '_').collect::<String>().chars().rev().collect();
        if word.is_empty() || word.chars().next().is_some_and(|c| c.is_ascii_digit()) {
            return;
        }
        let u = word.to_ascii_uppercase();
        // Not after a column letter of a reference like A1 (digits follow) – fine either way.
        self.autocomplete = names.iter().filter(|n| n.starts_with(&u)).take(12).cloned().collect();
        if self.autocomplete.len() == 1 && self.autocomplete[0] == u {
            self.autocomplete.clear();
        }
        self.ac_index = self.ac_index.min(self.autocomplete.len().saturating_sub(1));
    }

    /// Adds what tables offer to the function names of [`EditState::update_autocomplete`]:
    /// table names for the word before the caret; inside a table's brackets its columns, row
    /// items, the entries of its first column as keys, and a column's entries after a
    /// comparison (`Products[Stock>4`).
    pub fn suggest_table_parts(&mut self, wb: &gridcraft_engine::model::Workbook) {
        if !self.is_formula() {
            return;
        }
        let caret = self.byte_caret();
        let Some(before) = self.text.get(..caret) else { return };
        let mut found: Vec<(String, Completion)> = Vec::new();
        let insert = |typed: &str, text: String| Completion::Insert { start: caret - typed.len(), text };
        let Some(cx) = table_context(before) else {
            let word_len: usize = before.chars().rev().take_while(|c| c.is_ascii_alphanumeric() || *c == '.' || *c == '_').map(char::len_utf8).sum();
            let word = before.get(caret - word_len..).unwrap_or("");
            // Not for a number, nor inside a text literal.
            if word.is_empty() || word.starts_with(|c: char| c.is_ascii_digit()) || before.matches('"').count() % 2 == 1 {
                return;
            }
            let lower = word.to_lowercase();
            let tables = wb.sheets.iter().flat_map(|sh| sh.tables.iter()).filter(|t| t.name.to_lowercase().starts_with(&lower) && t.name != word);
            for t in tables.take(12) {
                found.push((t.name.clone(), insert(word, t.name.clone())));
            }
            // Tables go before the function names.
            while self.completions.len() < self.autocomplete.len() {
                self.completions.push(Completion::Function);
            }
            for (label, completion) in found.into_iter().rev() {
                self.autocomplete.insert(0, label);
                self.completions.insert(0, completion);
            }
            self.autocomplete.truncate(12);
            self.completions.truncate(12);
            return;
        };
        let table = table_of(wb, self.sheet, self.cell, cx.table);
        let columns = |typed: &str| table.map(|(_, t)| columns_starting(t, typed)).unwrap_or_default();
        let typed = cx.typed;
        match cx.slot {
            Slot::Columns => {
                // `"Unit Price"`, `[Unit Price]` or bare, as it was started.
                let (open, name_typed) = match (typed.strip_prefix('"'), typed.strip_prefix('[')) {
                    (Some(rest), _) => ("\"", rest),
                    (_, Some(rest)) => ("[", rest),
                    _ => ("", typed),
                };
                for name in columns(name_typed) {
                    let written = match open {
                        "\"" => format!("\"{}\"", name.replace('"', "\"\"")),
                        "[" => format!("[{}]", escape_column(&name)),
                        _ => column_name(&name),
                    };
                    if written != typed {
                        found.push((name, insert(typed, written)));
                    }
                }
            }
            Slot::Name => {
                if typed.starts_with('#') {
                    for item in ROW_ITEMS {
                        if item.to_lowercase().starts_with(&typed.to_lowercase()) {
                            found.push((item.to_string(), insert(typed, format!("{item}]"))));
                        }
                    }
                }
                for name in columns(typed) {
                    found.push((name.clone(), insert(typed, format!("{}]", escape_column(&name)))));
                }
            }
            Slot::Rows => match &cx.column {
                // The value of a condition: that column's entries.
                Some(column) => {
                    // Nothing to offer once the one matching value is typed in full.
                    for value in table.map(|(sh, t)| column_values(sh, t, Some(column), typed)).unwrap_or_default().into_iter().filter(|v| v != typed)
                    {
                        found.push((value.clone(), insert(typed, value)));
                    }
                }
                None => {
                    let item = typed.strip_prefix('[').unwrap_or(typed);
                    if item.starts_with('#') {
                        let close = if typed.starts_with('[') { "]" } else { "" };
                        for i in ROW_ITEMS {
                            if i.to_lowercase().starts_with(&item.to_lowercase()) {
                                found.push((i.to_string(), insert(item, format!("{i}{close}"))));
                            }
                        }
                    } else if let Some(name) = typed.strip_prefix('@') {
                        for c in columns(name.trim_start_matches('[')) {
                            found.push((c.clone(), insert(name, word_name(&c))));
                        }
                    } else {
                        // A key: the entries of the first column.
                        if cx.first && literal_start(typed) {
                            for key in table.map(|(sh, t)| column_values(sh, t, None, typed)).unwrap_or_default().into_iter().filter(|k| k != typed) {
                                found.push((key.clone(), insert(typed, key)));
                            }
                        }
                        // A column: alone, or to start a condition (`Stock>400`).
                        if !typed.starts_with('"') {
                            for c in columns(typed) {
                                let written = word_name(&c);
                                if written != typed {
                                    found.push((c, insert(typed, written)));
                                }
                            }
                        }
                    }
                    // No function is being typed inside a text literal.
                    if found.is_empty() && !typed.starts_with('"') {
                        return;
                    }
                }
            },
        }
        found.truncate(12);
        (self.autocomplete, self.completions) = found.into_iter().unzip();
        self.ac_index = self.ac_index.min(self.autocomplete.len().saturating_sub(1));
    }

    /// Accepts the highlighted autocomplete entry: a function name replaces the word and adds
    /// `(`; a table name, column or key is written as the entry spells it.
    pub fn accept_autocomplete(&mut self) -> bool {
        let Some(name) = self.autocomplete.get(self.ac_index).cloned() else { return false };
        let caret_b = self.byte_caret();
        if let Some(Completion::Insert { start, text }) = self.completions.get(self.ac_index).cloned() {
            let (Some(a), Some(b)) = (self.text.get(..start), self.text.get(caret_b..)) else { return false };
            // A bracket the entry closes may be there already.
            let written = if b.starts_with(']') { text.strip_suffix(']').unwrap_or(&text) } else { &text };
            self.text = format!("{a}{written}{b}");
            self.caret = self.text.get(..start + text.len()).map(|s| s.chars().count()).unwrap_or(0);
            self.autocomplete.clear();
            self.completions.clear();
            self.caret_to_end = false;
            self.sync_caret = true;
            return true;
        }
        let before = self.text.get(..caret_b).unwrap_or("");
        let wlen: usize = before.chars().rev().take_while(|c| c.is_ascii_alphanumeric() || *c == '.' || *c == '_').map(char::len_utf8).sum();
        let start = caret_b - wlen;
        let (Some(a), Some(b)) = (self.text.get(..start), self.text.get(caret_b..)) else { return false };
        self.text = format!("{a}{name}({b}");
        self.caret = self.text.get(..start + name.len() + 1).map(|s| s.chars().count()).unwrap_or(0);
        self.autocomplete.clear();
        self.completions.clear();
        self.caret_to_end = false;
        self.sync_caret = true;
        true
    }

    /// F4 on a table reference: anchors its column (`Products[Qty]` ⇄ `Products[$Qty]`), so it
    /// stays when the formula is filled sideways. One with conditions goes through both, its
    /// columns only, the conditions' columns only, like the `$` of a cell address. `false` when
    /// the caret is not on a table reference with one-column spans.
    pub fn cycle_table_anchor(&mut self, wb: Option<&gridcraft_engine::model::Workbook>) -> bool {
        use gridcraft_engine::formula::{self, Expr, RowSel, StructItem, TableRef, lexer::Tok};
        let Some(body) = self.text.strip_prefix('=') else { return false };
        let caret = self.byte_caret().saturating_sub(1);
        let Ok(tokens) = formula::lexer::tokenize_locale(body, self.locale) else { return false };
        let mut i = 0;
        while let Some(t) = tokens.get(i) {
            i += 1;
            if !matches!(t.tok, Tok::Struct(_)) {
                continue;
            }
            // The table name before the brackets, and second brackets right after, are part of it.
            let start = match i.checked_sub(2).and_then(|p| tokens.get(p)) {
                Some(name) if matches!(name.tok, Tok::Word(_)) && name.end == t.start => name.start,
                _ => t.start,
            };
            let mut end = t.end;
            if let Some(next) = tokens.get(i).filter(|n| matches!(n.tok, Tok::Struct(_)) && n.start == t.end) {
                end = next.end;
                i += 1;
            }
            if caret < start || caret > end {
                continue;
            }
            let Some(reference) = body.get(start..end) else { return false };
            // One row of one column (or span): F4 goes through its row and column anchors like
            // `A1` → `$A$1` → `A$1` → `$A1`, the row anchored as a key (`["Bolt"][$Qty]`) and
            // relative as `[@Qty]` / `[#Prev Row][Qty]` / `[#Next Row][Qty]`.
            let parsed = formula::parse(reference);
            let row_state = match &parsed {
                Ok(Expr::Struct(r))
                    if r.col_start.is_some()
                        && matches!(r.specifiers.as_slice(), [StructItem::ThisRow | StructItem::PrevRow | StructItem::NextRow]) =>
                {
                    Some((false, r.col_abs))
                }
                Ok(Expr::Table(TableRef { rows: RowSel::Key(_), columns, .. })) if columns.len() == 1 => {
                    Some((true, columns.first().is_some_and(|c| c.abs && c.last.is_none())))
                }
                _ => None,
            };
            if let (Some((row, col)), Some(wb), Ok(e)) = (row_state, wb, &parsed) {
                let (want_row, want_col) = match (row, col) {
                    (false, false) => (true, true),
                    (true, true) => (true, false),
                    (true, false) => (false, true),
                    (false, true) => (false, false),
                };
                let toggled =
                    if want_row != row { gridcraft_engine::table_refs::toggle_row_anchor(wb, self.sheet, self.cell, e) } else { Some(e.clone()) };
                // A row with no other form keeps it; its column anchor alone goes on toggling.
                let (mut next, col) = match toggled {
                    Some(t) => (t, want_col),
                    None => (e.clone(), !col),
                };
                match &mut next {
                    Expr::Struct(r) if r.col_end.is_none() => r.col_abs = col,
                    Expr::Table(t) => t.columns.iter_mut().filter(|c| c.last.is_none()).for_each(|c| c.abs = col),
                    _ => {}
                }
                self.replace_reference(start, end, &formula::print(&next));
                return true;
            }
            let anchored = match parsed {
                Ok(Expr::Struct(mut r)) if r.col_start.is_some() && r.col_end.is_none() => {
                    r.col_abs = !r.col_abs;
                    Expr::Struct(r)
                }
                Ok(Expr::Table(mut t)) => {
                    let singles = t.columns.iter().filter(|c| c.last.is_none()).count();
                    let cols = singles > 0 && t.columns.iter().filter(|c| c.last.is_none()).all(|c| c.abs);
                    let (has_conditions, conds) = match &t.rows {
                        RowSel::Filter(c) => (true, c.iter().all(|c| c.column_abs)),
                        _ => (false, false),
                    };
                    if singles == 0 && !has_conditions {
                        return false;
                    }
                    let (cols, conds) = match (has_conditions, cols, conds) {
                        (false, cols, _) => (!cols, false),
                        (true, false, false) => (true, true),
                        (true, true, true) => (true, false),
                        (true, true, false) => (false, true),
                        (true, false, true) => (false, false),
                    };
                    for c in t.columns.iter_mut().filter(|c| c.last.is_none()) {
                        c.abs = cols;
                    }
                    if let RowSel::Filter(c) = &mut t.rows {
                        c.iter_mut().for_each(|c| c.column_abs = conds);
                    }
                    Expr::Table(t)
                }
                _ => return false,
            };
            self.replace_reference(start, end, &formula::print(&anchored));
            return true;
        }
        false
    }

    /// Writes `written` over the formula body's bytes `start..end` and puts the caret after it.
    fn replace_reference(&mut self, start: usize, end: usize, written: &str) {
        let body = self.text.get(1..).unwrap_or("");
        let (Some(a), Some(b)) = (body.get(..start), body.get(end..)) else { return };
        let new_end = 1 + start + written.len();
        self.text = format!("={a}{written}{b}");
        self.caret = self.text.get(..new_end).map(|s| s.chars().count()).unwrap_or(0);
        self.point = None;
        self.caret_to_end = false;
        self.request_focus = true;
        self.sync_caret = true;
    }

    /// The function call the caret is inside and the argument index (for the hint tooltip).
    pub fn current_function(&self) -> Option<(String, usize)> {
        if !self.is_formula() {
            return None;
        }
        let before = self.text.get(..self.byte_caret())?;
        let mut depth = 0i32;
        let mut arg = 0usize;
        let chars: Vec<char> = before.chars().collect();
        let mut in_str = false;
        let mut i = chars.len();
        while i > 0 {
            i -= 1;
            let c = chars[i];
            if c == '"' {
                in_str = !in_str;
                continue;
            }
            if in_str {
                continue;
            }
            match c {
                ')' | '}' | ']' => depth += 1,
                '{' | '[' => depth -= 1,
                '(' => {
                    if depth == 0 {
                        let name: String = chars[..i]
                            .iter()
                            .rev()
                            .take_while(|c| c.is_ascii_alphanumeric() || **c == '.' || **c == '_')
                            .collect::<String>()
                            .chars()
                            .rev()
                            .collect();
                        if name.is_empty() {
                            return None;
                        }
                        return Some((name.to_ascii_uppercase(), arg));
                    }
                    depth -= 1;
                }
                c if c == self.locale.list_separator() && depth == 0 => arg += 1,
                _ => {}
            }
        }
        None
    }
}

/// References in formula text with their byte spans and colour index (same reference text ⇒
/// same colour), for colouring the editor and outlining ranges on the grid.
pub fn formula_refs(text: &str) -> Vec<(usize, usize, RangeRef, Option<String>, usize)> {
    let body = text.strip_prefix('=').or_else(|| text.strip_prefix('+')).unwrap_or(text);
    let offset = text.len() - body.len();
    let Ok(toks) = gridcraft_engine::formula::lexer::tokenize(body) else { return vec![] };
    let mut out: Vec<(usize, usize, RangeRef, Option<String>, usize)> = Vec::new();
    let mut seen: Vec<String> = Vec::new();
    let mut i = 0;
    while i < toks.len() {
        use gridcraft_engine::formula::lexer::Tok;
        let (sheet, start, j) = match &toks[i].tok {
            Tok::Sheet(a, _) => (Some(a.clone()), toks[i].start, i + 1),
            _ => (None, toks[i].start, i),
        };
        if let Some(Tok::Word(w)) = toks.get(j).map(|t| &t.tok)
            && let Some(c) = CellRef::parse(w)
        {
            let mut end = toks[j].end;
            let mut range = RangeRef::cell(c);
            if let (Some(Tok::Colon | Tok::Trim(_)), Some(Tok::Word(w2))) = (toks.get(j + 1).map(|t| &t.tok), toks.get(j + 2).map(|t| &t.tok))
                && let Some(c2) = CellRef::parse(w2)
            {
                range = RangeRef::new(c, c2);
                end = toks[j + 2].end;
                i = j + 3;
            } else {
                i = j + 1;
            }
            let key = body.get(start..end).unwrap_or("").replace('$', "").to_ascii_uppercase();
            let idx = match seen.iter().position(|k| *k == key) {
                Some(k) => k,
                None => {
                    seen.push(key);
                    seen.len() - 1
                }
            };
            out.push((start + offset, end + offset, range, sheet, idx % REF_COLORS.len()));
            continue;
        }
        i += 1;
    }
    out
}

/// A reference in a formula being edited: where its text is, the cells it covers (sheet index,
/// range) and its colour in [`REF_COLORS`].
#[derive(Clone, Debug, PartialEq)]
pub struct ColoredRef {
    pub start: usize,
    pub end: usize,
    pub areas: Vec<(usize, RangeRef)>,
    pub color: usize,
}

/// Every reference of formula `text` being edited in `cell` of `sheet`, as Excel colours them:
/// cell addresses and ranges, and table references (`Products[Qty]`, `[@Qty]`,
/// `Products["Bolt"][Price]`, a bare table name). The same reference text gets the same colour.
pub fn colored_refs(text: &str, wb: &gridcraft_engine::model::Workbook, sheet: usize, cell: CellRef) -> Vec<ColoredRef> {
    use gridcraft_engine::formula::{self, Expr, RowSel, StructRef, lexer::Tok};
    let mut found: Vec<(usize, usize, Vec<(usize, RangeRef)>, String)> = Vec::new();
    for (start, end, range, sheet_name, _) in formula_refs(text) {
        let target = match &sheet_name {
            Some(name) => wb.sheet_index(name),
            None => Some(sheet),
        };
        let key = text.get(start..end).unwrap_or("").replace('$', "").to_uppercase();
        found.push((start, end, target.map(|s| vec![(s, range)]).unwrap_or_default(), key));
    }
    let body = text.strip_prefix('=').or_else(|| text.strip_prefix('+')).unwrap_or(text);
    let offset = text.len() - body.len();
    let areas = |e: &Expr| -> Vec<(usize, RangeRef)> {
        let resolve =
            |e: &Expr| gridcraft_engine::calc::reference_areas(wb, sheet, cell, e).into_iter().map(|a| (a.sheet, a.range)).collect::<Vec<_>>();
        match e {
            // A filter is computed, not a place: outline the columns it returns. A lookup that
            // finds no cell does the same.
            Expr::Table(t) => {
                let found = if matches!(t.rows, RowSel::Filter(_)) { vec![] } else { resolve(&t.lower()) };
                if !found.is_empty() {
                    return found;
                }
                let spans: Vec<Option<&formula::ColumnSpan>> = if t.columns.is_empty() { vec![None] } else { t.columns.iter().map(Some).collect() };
                spans
                    .into_iter()
                    .flat_map(|s| {
                        resolve(&Expr::Struct(StructRef {
                            table: t.table.clone(),
                            specifiers: vec![],
                            col_start: s.map(|s| s.first.clone()),
                            col_end: s.and_then(|s| s.last.clone()),
                            col_abs: false,
                        }))
                    })
                    .collect()
            }
            e => resolve(e),
        }
    };
    if let Ok(tokens) = formula::lexer::tokenize(body) {
        let mut i = 0;
        while let Some(t) = tokens.get(i) {
            i += 1;
            let start = t.start;
            let mut end = t.end;
            match &t.tok {
                // `Products[…]`, `Products[…][…]`, `[…]`, `[…][…]`.
                Tok::Word(w) if CellRef::parse(w).is_none() => {
                    let next = tokens.get(i);
                    match next.map(|n| &n.tok) {
                        Some(Tok::Struct(_)) if next.is_some_and(|n| n.start == end) => {
                            end = next.map_or(end, |n| n.end);
                            i += 1;
                        }
                        // A bare table name is its data.
                        Some(Tok::LParen) => continue,
                        _ if wb.table(w).is_some() => {}
                        _ => continue,
                    }
                }
                Tok::Struct(_) => {}
                _ => continue,
            }
            if let Some(n) = tokens.get(i).filter(|n| matches!(n.tok, Tok::Struct(_)) && n.start == end) {
                end = n.end;
                i += 1;
            }
            let Some(reference) = body.get(start..end) else { continue };
            let Ok(e) = formula::parse(reference) else { continue };
            if !matches!(e, Expr::Struct(_) | Expr::Table(_) | Expr::Name(_)) {
                continue;
            }
            found.push((start + offset, end + offset, areas(&e), formula::print(&e).to_uppercase()));
        }
    }
    found.sort_by_key(|f| f.0);
    let mut seen: Vec<String> = Vec::new();
    found
        .into_iter()
        .map(|(start, end, areas, key)| {
            let color = match seen.iter().position(|k| *k == key) {
                Some(k) => k,
                None => {
                    seen.push(key);
                    seen.len() - 1
                }
            };
            ColoredRef { start, end, areas, color: color % REF_COLORS.len() }
        })
        .collect()
}

/// Excel-style AutoComplete: the one distinct text entry in the same column (contiguous data
/// above and below) that starts with `typed` (case-insensitive). `None` when ambiguous.
pub fn column_completion(sheet: &gridcraft_engine::model::Sheet, at: CellRef, typed: &str) -> Option<String> {
    if typed.is_empty() || typed.starts_with('=') || typed.chars().all(|c| c.is_ascii_digit() || c == '.' || c == '-') {
        return None;
    }
    let lower = typed.to_lowercase();
    let mut found: Option<String> = None;
    let mut ambiguous = false;
    let mut scan = |row: u32| -> bool {
        match sheet.value(CellRef::new(row, at.col)) {
            gridcraft_engine::core::Value::Empty => false,
            gridcraft_engine::core::Value::Text(t) => {
                let folded = t.to_lowercase();
                if folded.starts_with(&lower) && folded.len() > lower.len() {
                    match &found {
                        Some(f) if f.to_lowercase() != folded => ambiguous = true,
                        None => found = Some(t.to_string()),
                        _ => {}
                    }
                }
                true
            }
            _ => true,
        }
    };
    let mut r = at.row;
    let mut n = 0;
    while r > 0 && n < 5000 {
        r -= 1;
        n += 1;
        if !scan(r) {
            break;
        }
    }
    let mut r = at.row + 1;
    n = 0;
    while n < 5000 && scan(r) {
        r += 1;
        n += 1;
    }
    if ambiguous { None } else { found }
}

/// Row items offered in a table's row brackets.
const ROW_ITEMS: [&str; 7] = ["#This Row", "#Prev Row", "#Next Row", "#Headers", "#Data", "#Totals", "#All"];

/// Which brackets of a table reference the caret is in.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Slot {
    /// The first brackets: rows, or the columns when no second brackets follow.
    Rows,
    /// The second brackets of `Table[rows][columns]`.
    Columns,
    /// A group inside Excel's outer brackets (`Table[[#Headers],[Pr`): a column or an item,
    /// which the entry closes.
    Name,
}

/// What is being typed inside a table reference.
#[derive(Debug, PartialEq, Eq)]
pub struct TableContext<'a> {
    /// Empty inside the table itself.
    pub table: &'a str,
    pub slot: Slot,
    /// The first entry of the row brackets, which may be a key.
    pub first: bool,
    /// In the row brackets after a comparison: the column compared (`Stock` of `Stock>4`).
    pub column: Option<String>,
    /// What is typed of the entry so far, up to the caret.
    pub typed: &'a str,
}

/// Reads `before` (the formula up to the caret) as ending inside a table reference's brackets.
pub fn table_context(before: &str) -> Option<TableContext<'_>> {
    let mut open: Vec<usize> = Vec::new();
    let mut text = false;
    for (i, c) in before.char_indices() {
        match c {
            '"' => text = !text,
            _ if text => {}
            '[' => open.push(i),
            ']' => {
                open.pop();
            }
            _ => {}
        }
    }
    let bracket = *open.last()?;
    let word = |end: usize| {
        let head = before.get(..end).unwrap_or("");
        let len: usize = head.chars().rev().take_while(|c| c.is_alphanumeric() || matches!(c, '_' | '.' | '\\')).map(char::len_utf8).sum();
        head.get(head.len() - len..).unwrap_or("")
    };
    // The table a bracket belongs to: the word before it, or before the first brackets of
    // `Table[rows][` when it is the second.
    let owner = |at: usize| -> Option<(&str, bool)> {
        let head = before.get(..at)?;
        if !head.ends_with(']') {
            return Some((word(at), false));
        }
        let mut depth = 0usize;
        for (i, c) in head.char_indices().rev() {
            match c {
                ']' => depth += 1,
                '[' => {
                    depth = depth.saturating_sub(1);
                    if depth == 0 {
                        return Some((word(i), true));
                    }
                }
                _ => {}
            }
        }
        None
    };
    let (table, second) = owner(bracket)?;
    let (table, slot) = if second {
        (table, Slot::Columns)
    } else if !table.is_empty() || open.len() == 1 {
        (table, Slot::Rows)
    } else {
        let outer = open.get(open.len().checked_sub(2)?).copied()?;
        (owner(outer)?.0, Slot::Name)
    };
    let content = before.get(bracket + 1..)?;
    // The entry being typed: after the last separator outside brackets, parentheses and text.
    let separators: &[char] = if slot == Slot::Columns { &[',', ':'] } else { &[','] };
    let (mut depth, mut text, mut last) = (0usize, false, None);
    for (i, c) in content.char_indices() {
        match c {
            '"' => text = !text,
            _ if text => {}
            '[' | '(' | '{' => depth += 1,
            ']' | ')' | '}' => depth = depth.saturating_sub(1),
            c if depth == 0 && separators.contains(&c) => last = Some(i),
            _ => {}
        }
    }
    let part = content.get(last.map_or(0, |i| i + 1)..)?.trim_start();
    let part = if slot == Slot::Columns { part.trim_start_matches('$').trim_start() } else { part };
    let mut column = None;
    let mut typed = part;
    if slot == Slot::Rows {
        // `Stock>4`: the column before the comparison.
        let (mut depth, mut text) = (0usize, false);
        let at = part.char_indices().find_map(|(i, c)| {
            match c {
                '"' => text = !text,
                _ if text => {}
                '[' | '(' => depth += 1,
                ']' | ')' => depth = depth.saturating_sub(1),
                '<' | '>' | '=' if depth == 0 => return Some(i),
                _ => {}
            }
            None
        });
        if let Some(at) = at {
            let name = part.get(..at)?.trim().trim_start_matches('$').trim_start();
            let name = name.strip_prefix('[').and_then(|n| n.strip_suffix(']')).unwrap_or(name);
            if name.is_empty() || name.contains(['[', ']', '"', '(', ')']) {
                return None;
            }
            let after = part.get(at..)?;
            let len = if after.starts_with("<=") || after.starts_with(">=") || after.starts_with("<>") { 2 } else { 1 };
            column = Some(name.to_string());
            typed = part.get(at + len..)?.trim_start();
            if !literal_start(typed) {
                return None;
            }
        }
    }
    Some(TableContext { table, slot, first: last.is_none(), column, typed })
}

/// Whether `typed` is the start of a literal: an unclosed text, or a word or number.
fn literal_start(typed: &str) -> bool {
    match typed.strip_prefix('"') {
        Some(open_text) => !open_text.contains('"'),
        None => typed.chars().all(|c| c.is_alphanumeric() || matches!(c, '.' | '-' | '_')),
    }
}

/// A column where it may start a condition (`Stock>400`): bare only when it is one word.
fn word_name(name: &str) -> String {
    let word = name.starts_with(|c: char| c.is_alphabetic() || c == '_') && name.chars().all(|c| c.is_alphanumeric() || c == '_');
    if word && CellRef::parse(name).is_none() { name.to_string() } else { format!("[{}]", escape_column(name)) }
}

/// A column as written in the column brackets: bare when it can be, else bracketed.
fn column_name(name: &str) -> String {
    let plain = !name.is_empty()
        && name.trim() == name
        && !name.chars().any(|c| matches!(c, '[' | ']' | '#' | '\'' | ',' | ':' | '"' | '$' | '@' | '=' | '<' | '>'));
    if plain { name.to_string() } else { format!("[{}]", escape_column(name)) }
}

/// `'[` for `[`: a column name as it is written inside brackets.
fn escape_column(name: &str) -> String {
    name.replace('\'', "''").replace('[', "'[").replace(']', "']").replace('#', "'#")
}

/// The named table, or the one `cell` is in when no name is given.
fn table_of<'a>(
    wb: &'a gridcraft_engine::model::Workbook,
    sheet: usize,
    cell: CellRef,
    name: &str,
) -> Option<(&'a gridcraft_engine::model::Sheet, &'a gridcraft_engine::model::Table)> {
    if name.is_empty() {
        wb.sheet(sheet).and_then(|sh| sh.table_at(cell).map(|t| (sh, t)))
    } else {
        wb.table(name).and_then(|(si, ti)| wb.sheet(si).and_then(|sh| sh.tables.get(ti).map(|t| (sh, t))))
    }
}

/// The table's column names that start with what is typed, in table order.
fn columns_starting(t: &gridcraft_engine::model::Table, typed: &str) -> Vec<String> {
    let typed = typed.to_lowercase();
    t.columns.iter().map(|c| c.name.clone()).filter(|n| n.to_lowercase().starts_with(&typed)).take(12).collect()
}

/// The entries of a column (the first when `None`) that start with what is typed, as the
/// literals to write (`"Washer"`, `1002`), in table order without repeats.
pub fn column_values(sh: &gridcraft_engine::model::Sheet, t: &gridcraft_engine::model::Table, column: Option<&str>, typed: &str) -> Vec<String> {
    use gridcraft_engine::core::Value;
    let index = match column {
        Some(name) => t.column_index(name),
        None => Some(0),
    };
    let (Some(index), Some(data)) = (index, t.data_range()) else { return vec![] };
    let col = t.range.start.col.saturating_add(index as u32);
    let quoted = typed.starts_with('"');
    let typed = typed.trim_start_matches('"').to_lowercase();
    let mut seen: Vec<String> = Vec::new();
    let mut out = Vec::new();
    for row in (data.start.row..=data.end.row).take(5000) {
        let (plain, literal) = match sh.value(CellRef::new(row, col)) {
            // `*`, `?` and `~` are wildcards to the lookup, not characters of a key.
            Value::Text(s) if !s.is_empty() && !s.contains(['*', '?', '~']) => (s.to_lowercase(), format!("\"{}\"", s.replace('"', "\"\""))),
            Value::Number(n) if n.is_finite() && !quoted => {
                let text = gridcraft_engine::core::number_to_text(n);
                (text.clone(), text)
            }
            Value::Bool(b) if !quoted => (b.to_string(), b.to_string().to_uppercase()),
            _ => continue,
        };
        if plain.starts_with(&typed) && !seen.contains(&plain) {
            seen.push(plain);
            out.push(literal);
            if out.len() == 12 {
                break;
            }
        }
    }
    out
}

/// The part of `full` after the `typed` prefix, counted in characters: case folding can change
/// a character's byte length ("ß" is two bytes, "ẞ" three).
pub fn completion_rest<'a>(full: &'a str, typed: &str) -> Option<&'a str> {
    let start = full.char_indices().nth(typed.chars().count())?.0;
    full.get(start..)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn table_context_reads_the_brackets() {
        let cx = |s: &'static str| table_context(s).map(|c| (c.table, c.slot, c.first, c.column, c.typed));
        assert_eq!(cx("=Products["), Some(("Products", Slot::Rows, true, None, "")));
        assert_eq!(cx("=SUM(1,Products[\"Wa"), Some(("Products", Slot::Rows, true, None, "\"Wa")));
        assert_eq!(cx("=Products[Stock>=4"), Some(("Products", Slot::Rows, true, Some("Stock".into()), "4")));
        assert_eq!(cx("=Products[Stock>1,[In Stock]<>\"y"), Some(("Products", Slot::Rows, false, Some("In Stock".into()), "\"y")));
        assert_eq!(cx("=Products[Stock>1,Pro"), Some(("Products", Slot::Rows, false, None, "Pro")));
        assert_eq!(cx("=Products[\"Bolt\"][Pr"), Some(("Products", Slot::Columns, true, None, "Pr")));
        assert_eq!(cx("=Products[\"Bolt\"][Price,$St"), Some(("Products", Slot::Columns, false, None, "St")));
        assert_eq!(cx("=Products[Stock>1][Price:\"Un"), Some(("Products", Slot::Columns, false, None, "\"Un")));
        assert_eq!(cx("=[#Prev Row][To"), Some(("", Slot::Columns, true, None, "To")));
        assert_eq!(cx("=Products[[#Headers],[Pr"), Some(("Products", Slot::Name, true, None, "Pr")));
        assert_eq!(cx("=[@Pr"), Some(("", Slot::Rows, true, None, "@Pr")));
        // A value that is more than a literal being typed.
        assert_eq!(cx("=Products[Stock>A1+"), None);
        assert_eq!(cx("=SUM(A1,"), None);
    }

    #[test]
    fn table_parts_are_suggested() {
        use gridcraft_engine::Session;
        use serde_json::json;
        let mut s = Session::new();
        s.new_workbook();
        let rows = json!([
            ["Product", "Unit Price", "Stock"],
            ["Nut", 0.1, 500],
            ["Washer", 0.05, 900],
            ["nut", 0.2, 410],
            ["Wing \"A\"", 1, 410],
            ["W*", 1, 1]
        ]);
        s.execute("range.setValues", json!({"range": "A1", "values": rows})).unwrap();
        s.execute("insert.table", json!({"range": "A1:C6", "header": true, "name": "Products"})).unwrap();
        let wb = &s.doc().unwrap().wb;
        // Typing `text` at `cell` (row, col) lists `labels`; accepting the entry at `pick` gives `after`.
        let check = |cell: (u32, u32), text: &str, labels: &[&str], pick: usize, after: &str| {
            let mut e = EditState::new(0, CellRef::new(cell.0, cell.1), text.into(), true, false);
            e.update_autocomplete(&["PRODUCT".to_string(), "SUM".to_string()]);
            e.suggest_table_parts(wb);
            assert_eq!(e.autocomplete, labels, "{text}");
            if !labels.is_empty() {
                e.ac_index = pick;
                assert!(e.accept_autocomplete());
                assert_eq!(e.text, after, "{text}");
                assert_eq!(e.caret, after.chars().count(), "{text}");
            }
        };
        // A table name, before the functions of the same start.
        check((0, 5), "=SUM(pro", &["Products", "PRODUCT"], 0, "=SUM(Products");
        check((0, 5), "=SUM(pro", &["Products", "PRODUCT"], 1, "=SUM(PRODUCT(");
        // First brackets: columns, and the first column's entries as keys.
        check((0, 5), "=Products[", &["\"Nut\"", "\"Washer\"", "\"Wing \"\"A\"\"\"", "Product", "Unit Price", "Stock"], 4, "=Products[[Unit Price]");
        check((0, 5), "=Products[\"w", &["\"Washer\"", "\"Wing \"\"A\"\"\""], 0, "=Products[\"Washer\"");
        check((0, 5), "=Products[st", &["Stock"], 0, "=Products[Stock");
        check((0, 5), "=Products[#p", &["#Prev Row"], 0, "=Products[#Prev Row");
        check((0, 5), "=Products[[#", &["#This Row", "#Prev Row", "#Next Row", "#Headers", "#Data", "#Totals", "#All"], 1, "=Products[[#Prev Row]");
        check((0, 5), "=Products[@un", &["Unit Price"], 0, "=Products[@[Unit Price]");
        // After a comparison: that column's entries.
        check((0, 5), "=Products[Stock=4", &["410"], 0, "=Products[Stock=410");
        check((0, 5), "=Products[Stock>1,[Unit Price]<0.1", &[], 0, "");
        check((0, 5), "=Products[Stock>1,Product<>\"N", &["\"Nut\""], 0, "=Products[Stock>1,Product<>\"Nut\"");
        // Second brackets: columns, as they were started.
        check((0, 5), "=Products[\"Nut\"][", &["Product", "Unit Price", "Stock"], 1, "=Products[\"Nut\"][Unit Price");
        check((0, 5), "=Products[\"Nut\"][Stock,\"un", &["Unit Price"], 0, "=Products[\"Nut\"][Stock,\"Unit Price\"");
        check((0, 5), "=Products[\"Nut\"][Product:[un", &["Unit Price"], 0, "=Products[\"Nut\"][Product:[Unit Price]");
        // Excel's outer brackets: the entry closes its group.
        check((0, 5), "=Products[[#Headers],[st", &["Stock"], 0, "=Products[[#Headers],[Stock]");
        // Inside the table, its name may be left out.
        check((1, 1), "=[#Prev Row][st", &["Stock"], 0, "=[#Prev Row][Stock");
        // Another table has no columns to offer; a key may still call a function.
        check((0, 5), "=Missing[p", &["PRODUCT"], 0, "=Missing[PRODUCT(");
        // Nothing once the name is complete.
        check((0, 5), "=Products[\"Nut\"][Stock", &[], 0, "");
    }

    #[test]
    fn f4_cycles_a_row_and_its_column() {
        use gridcraft_engine::Session;
        use serde_json::json;
        let mut s = Session::new();
        s.new_workbook();
        let rows = json!([["Product", "Qty", "Total"], ["Nut", 5, null], ["Bolt", 7, null], ["Washer", 9, null]]);
        s.execute("range.setValues", json!({"range": "A1", "values": rows})).unwrap();
        s.execute("insert.table", json!({"range": "A1:C4", "header": true, "name": "Stock"})).unwrap();
        let wb = &s.doc().unwrap().wb;
        let cycle = |cell: &str, start: &str| {
            let mut e = EditState::new(0, CellRef::parse(cell).unwrap(), start.into(), true, false);
            let mut seen = Vec::new();
            for _ in 0..4 {
                e.caret = e.text.chars().count();
                assert!(e.cycle_table_anchor(Some(wb)));
                seen.push(e.text.clone());
            }
            seen
        };
        // Inside the table, like `A1` → `$A$1` → `A$1` → `$A1`: the row anchored is its key.
        assert_eq!(cycle("C3", "=[@Qty]"), ["=[\"Bolt\"][$Qty]", "=[\"Bolt\"][Qty]", "=[@$Qty]", "=[@Qty]"]);
        assert_eq!(cycle("C3", "=2*[#Prev Row][Qty]"), ["=2*[\"Nut\"][$Qty]", "=2*[\"Nut\"][Qty]", "=2*[#Prev Row][$Qty]", "=2*[#Prev Row][Qty]"]);
        // From outside the table a key has no relative form: only its column anchor toggles.
        assert_eq!(
            cycle("E3", "=Stock[\"Bolt\"][Qty]"),
            ["=Stock[\"Bolt\"][$Qty]", "=Stock[\"Bolt\"][Qty]", "=Stock[\"Bolt\"][$Qty]", "=Stock[\"Bolt\"][Qty]"]
        );
    }

    #[test]
    fn f4_anchors_a_table_column() {
        let press = |text: &str, caret: usize| {
            let mut e = EditState::new(0, CellRef::new(0, 0), text.into(), true, false);
            e.caret = caret;
            e.cycle_table_anchor(None).then(|| (e.text.clone(), e.caret))
        };
        let end = |s: &str| s.chars().count();
        // With the caret anywhere on the reference, or right after it.
        for caret in [5, 9, 18] {
            assert_eq!(press("=SUM(Products[Qty])+A1", caret), Some(("=SUM(Products[$Qty])+A1".into(), 19)), "{caret}");
        }
        assert_eq!(press("=SUM(Products[$Qty])+A1", 12), Some(("=SUM(Products[Qty])+A1".into(), 18)));
        assert_eq!(press("=[@[Unit Price]]*2", 3), Some(("=[@$[Unit Price]]*2".into(), 17)));
        // Both brackets of `Table[rows][columns]` are the reference.
        let s = "=Products[\"Nut\"][Price,Stock]";
        assert_eq!(press(s, 4), Some(("=Products[\"Nut\"][$Price,$Stock]".into(), end("=Products[\"Nut\"][$Price,$Stock]"))));
        assert_eq!(press("=[#Prev Row][Total]+1", 15), Some(("=[#Prev Row][$Total]+1".into(), 20)));
        // Conditions too: both, the columns, the conditions, none.
        let mut text = "=Products[Stock>A2][Price]".to_string();
        let mut seen = Vec::new();
        for _ in 0..4 {
            text = press(&text, end(&text)).unwrap().0;
            seen.push(text.clone());
        }
        assert_eq!(
            seen,
            ["=Products[$Stock>A2][$Price]", "=Products[Stock>A2][$Price]", "=Products[$Stock>A2][Price]", "=Products[Stock>A2][Price]"]
        );
        // Elsewhere F4 is the `$` of a cell address.
        for (text, caret) in
            [("=SUM(Products[Qty])+A1", 22), ("=Products[[Qty]:[Price]]", 5), ("=Products[#Headers]", 5), ("=SUM(Products)", 8), ("Products[Qty]", 4)]
        {
            assert_eq!(press(text, caret), None, "{text}");
        }
    }

    #[test]
    fn point_mode() {
        let mut e = EditState::new(0, CellRef::new(0, 0), "=SUM(".into(), true, false);
        assert!(e.can_point());
        e.insert_ref("A1");
        assert_eq!(e.text, "=SUM(A1");
        e.insert_ref("A2");
        assert_eq!(e.text, "=SUM(A2");
        e.text.push(':');
        e.caret += 1;
        e.point = None;
        assert!(e.can_point());
        let e2 = EditState::new(0, CellRef::new(0, 0), "hello".into(), true, false);
        assert!(!e2.can_point());
    }

    #[test]
    fn autocomplete_and_hints() {
        let names: Vec<String> = ["SUM", "SUMIF", "SUMPRODUCT", "AVERAGE"].iter().map(|s| s.to_string()).collect();
        let mut e = EditState::new(0, CellRef::new(0, 0), "=su".into(), true, false);
        e.update_autocomplete(&names);
        assert_eq!(e.autocomplete.len(), 3);
        assert!(e.accept_autocomplete());
        assert_eq!(e.text, "=SUM(");
        let e = EditState::new(0, CellRef::new(0, 0), "=IF(A1>0,SUM(B1,".into(), true, false);
        assert_eq!(e.current_function(), Some(("SUM".into(), 1)));
    }

    #[test]
    fn table_references_are_coloured_and_outlined() {
        use gridcraft_engine::Session;
        use serde_json::json;
        let mut s = Session::new();
        s.new_workbook();
        let rows = json!([["Product", "Price", "Stock"], ["Nut", 0.1, 500], ["Bolt", 0.25, 320], ["Washer", 0.05, 900]]);
        s.execute("range.setValues", json!({"range": "A1", "values": rows})).unwrap();
        s.execute("insert.table", json!({"range": "A1:C4", "header": true, "name": "Products"})).unwrap();
        let wb = &s.doc().unwrap().wb;
        let text = "=SUM(Products[Stock])+A1+Products[\"Bolt\"][Price]+[@Price]+products[stock]+Products[Stock>400][Product,Price]+COUNT(Products)";
        // Edited in C3, inside the table, where `[@Price]` is its own row's.
        let refs = colored_refs(text, wb, 0, CellRef::new(2, 2));
        let spans: Vec<(&str, Vec<String>, usize)> =
            refs.iter().map(|r| (&text[r.start..r.end], r.areas.iter().map(|a| a.1.a1()).collect(), r.color)).collect();
        assert_eq!(
            spans,
            [
                ("Products[Stock]", vec!["C2:C4".to_string()], 0),
                ("A1", vec!["A1".to_string()], 1),
                // A lookup by key outlines the cell it finds.
                ("Products[\"Bolt\"][Price]", vec!["B3".to_string()], 2),
                ("[@Price]", vec!["B3".to_string()], 3),
                // The same reference, the same colour.
                ("products[stock]", vec!["C2:C4".to_string()], 0),
                // A filter outlines the columns it returns.
                ("Products[Stock>400][Product,Price]", vec!["A2:A4".to_string(), "B2:B4".to_string()], 4),
                ("Products", vec!["A2:C4".to_string()], 5),
            ]
        );
    }

    #[test]
    fn ref_spans() {
        let r = formula_refs("=A1+B2:C3*a1+Sheet2!D4");
        assert_eq!(r.len(), 4);
        assert_eq!(r[0].4, r[2].4); // A1 and a1 share a colour
        assert_eq!(r[1].2.a1(), "B2:C3");
        assert_eq!(r[3].3.as_deref(), Some("Sheet2"));
    }
}
