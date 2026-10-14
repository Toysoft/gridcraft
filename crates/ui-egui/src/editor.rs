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
    /// table names for the word before the caret, the columns of a table inside its brackets
    /// (`Products[Pr`), and the entries of the key column in the key of a keyed reference
    /// (`Products[Price,"Wa`).
    pub fn suggest_table_parts(&mut self, wb: &gridcraft_engine::model::Workbook) {
        if !self.is_formula() {
            return;
        }
        let caret = self.byte_caret();
        let Some(before) = self.text.get(..caret) else { return };
        let mut found: Vec<(String, Completion)> = Vec::new();
        let insert = |typed: &str, text: String| Completion::Insert { start: caret - typed.len(), text };
        if let Some(cx) = key_context(before) {
            let table = table_of(wb, self.sheet, self.cell, cx.table);
            let keys = if cx.key || cx.key_column.is_some() { table.map(|(sh, t)| key_suggestions(sh, t, &cx)) } else { None };
            for key in keys.unwrap_or_default() {
                found.push((key.clone(), insert(cx.typed, format!("{key}]"))));
            }
            // A plain word can also start a condition: `Stock>400`.
            if cx.key_column.is_none() && !cx.typed.starts_with('"') {
                for name in table.map(|(_, t)| columns_starting(t, cx.typed)).unwrap_or_default() {
                    let written = if plain_word(&name) { name.clone() } else { format!("[{}]", escape_column(&name)) };
                    found.push((name, insert(cx.typed, written)));
                }
            }
            // No function is being typed inside a text literal.
            if found.is_empty() && !cx.typed.starts_with('"') {
                return;
            }
        } else if let Some(cx) = column_context(before) {
            let table = table_of(wb, self.sheet, self.cell, cx.table);
            let close = if cx.in_name { "]" } else { "" };
            if cx.typed.starts_with('#') {
                for item in ["#All", "#Data", "#Headers", "#Totals", "#This Row", "#Prev Row", "#Next Row"] {
                    if item.to_lowercase().starts_with(&cx.typed.to_lowercase()) {
                        found.push((item.to_string(), insert(cx.typed, format!("{item}{close}"))));
                    }
                }
            }
            for name in table.map(|(_, t)| columns_starting(t, cx.typed)).unwrap_or_default() {
                let written = match (cx.in_name, cx.key_column) {
                    (true, _) => format!("{}]", escape_column(&name)),
                    (false, _) if name.contains([',', ':', '=', '"', '@']) || name.trim() != name => format!("[{}]", escape_column(&name)),
                    (false, _) => escape_column(&name),
                };
                // Nothing to offer once the one matching name is typed in full.
                if !cx.in_name && written == cx.typed {
                    continue;
                }
                found.push((name, insert(cx.typed, written)));
            }
        } else {
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
    /// stays when the formula is filled sideways. A keyed reference with conditions goes
    /// through both, its column only, the conditions' columns only, like the `$` of a cell address. `false`
    /// when the caret is not on a reference to one table column.
    pub fn cycle_table_anchor(&mut self) -> bool {
        use gridcraft_engine::formula::{self, Expr, lexer::Tok};
        let Some(body) = self.text.strip_prefix('=') else { return false };
        let caret = self.byte_caret().saturating_sub(1);
        let Ok(tokens) = formula::lexer::tokenize_locale(body, self.locale) else { return false };
        for (i, t) in tokens.iter().enumerate() {
            if !matches!(t.tok, Tok::Struct(_)) {
                continue;
            }
            // The table name before the brackets is part of the reference.
            let start = match i.checked_sub(1).and_then(|p| tokens.get(p)) {
                Some(name) if matches!(name.tok, Tok::Word(_)) && name.end == t.start => name.start,
                _ => t.start,
            };
            if caret < start || caret > t.end {
                continue;
            }
            let Some(reference) = body.get(start..t.end) else { return false };
            let anchored = match formula::parse(reference) {
                Ok(Expr::Struct(mut r)) if r.col_start.is_some() && r.col_end.is_none() => {
                    r.col_abs = !r.col_abs;
                    Expr::Struct(r)
                }
                Ok(Expr::Keyed(mut k)) => {
                    let named = k.conditions.iter().any(|c| c.column.is_some());
                    let conditions = named && k.conditions.iter().filter(|c| c.column.is_some()).all(|c| c.column_abs);
                    let (column, conditions) = match (named, k.column_abs, conditions) {
                        (false, column, _) => (!column, false),
                        (true, false, false) => (true, true),
                        (true, true, true) => (true, false),
                        (true, true, false) => (false, true),
                        (true, false, true) => (false, false),
                    };
                    k.column_abs = column;
                    for c in k.conditions.iter_mut().filter(|c| c.column.is_some()) {
                        c.column_abs = conditions;
                    }
                    Expr::Keyed(k)
                }
                _ => return false,
            };
            let written = formula::print(&anchored);
            let (Some(a), Some(b)) = (body.get(..start), body.get(t.end..)) else { return false };
            let end = 1 + start + written.len();
            self.text = format!("={a}{written}{b}");
            self.caret = self.text.get(..end).map(|s| s.chars().count()).unwrap_or(0);
            self.point = None;
            self.caret_to_end = false;
            self.request_focus = true;
            self.sync_caret = true;
            return true;
        }
        false
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

/// The key being typed in a keyed table reference.
#[derive(Debug, PartialEq, Eq)]
pub struct KeyContext<'a> {
    /// Empty for `[Price,…` inside the table itself.
    pub table: &'a str,
    /// `Stock` of `Products[Price,Stock>…`, the column a value is compared with.
    pub key_column: Option<String>,
    /// Typing the key of `Products[Price,…` (matched against the table's first column), rather
    /// than a further condition's column.
    pub key: bool,
    /// What is typed of the key so far, up to the caret: `"Wa`, `10`, or nothing.
    pub typed: &'a str,
}

/// Reads `before` (the formula up to the caret) as ending inside the key of a keyed table
/// reference. `None` elsewhere, and once the key is more than a literal being typed.
pub fn key_context(before: &str) -> Option<KeyContext<'_>> {
    // Byte ranges of `before` outside text literals, split at brackets.
    let top_level = |s: &str, stop: char| {
        let (mut depth, mut text) = (0usize, false);
        s.char_indices().find_map(|(i, c)| {
            match c {
                '"' => text = !text,
                _ if text => {}
                '[' | '(' | '{' => depth += 1,
                ']' | ')' | '}' => depth = depth.saturating_sub(1),
                c if c == stop && depth == 0 => return Some(i),
                _ => {}
            }
            None
        })
    };
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
    let head = before.get(..bracket)?;
    let name_len: usize = head.chars().rev().take_while(|c| c.is_alphanumeric() || matches!(c, '_' | '.' | '\\')).map(char::len_utf8).sum();
    let table = head.get(head.len() - name_len..)?;
    let body = before.get(bracket + 1..)?;
    let column = body.get(..top_level(body, ',')?)?.trim();
    if column.is_empty() || column.starts_with(['#', '@']) || column.starts_with("[#") {
        return None;
    }
    let rest = body.get(column_end(body)? + 1..)?;
    // The condition being typed: after the last comma outside brackets, parentheses and text.
    let (mut depth, mut text, mut last) = (0usize, false, None);
    for (i, c) in rest.char_indices() {
        match c {
            '"' => text = !text,
            _ if text => {}
            '[' | '(' | '{' => depth += 1,
            ']' | ')' | '}' => depth = depth.saturating_sub(1),
            ',' if depth == 0 => last = Some(i),
            _ => {}
        }
    }
    let part = rest.get(last.map_or(0, |i| i + 1)..)?;
    let comparison = ['<', '>', '='].into_iter().filter_map(|c| top_level(part, c)).min();
    let (key_column, typed) = match comparison {
        Some(at) => {
            let name = part.get(..at)?.trim();
            let name = name.strip_prefix('$').unwrap_or(name).trim_start();
            let name = name.strip_prefix('[').and_then(|n| n.strip_suffix(']')).unwrap_or(name);
            if name.is_empty() || name.contains(['[', ']', '"', '(', ')']) {
                return None;
            }
            let after = part.get(at..)?;
            let len = if after.starts_with("<=") || after.starts_with(">=") || after.starts_with("<>") { 2 } else { 1 };
            (Some(name.to_string()), part.get(at + len..)?)
        }
        None => (None, part),
    };
    let key = last.is_none() && key_column.is_none();
    let typed = typed.trim_start();
    let literal = match typed.strip_prefix('"') {
        Some(open_text) => !open_text.contains('"'),
        None => typed.chars().all(|c| c.is_alphanumeric() || matches!(c, '.' | '-' | '_')),
    };
    literal.then_some(KeyContext { table, key_column, key, typed })
}

/// Where the column of a keyed reference body ends: its first comma outside brackets.
fn column_end(body: &str) -> Option<usize> {
    let mut depth = 0usize;
    body.char_indices().find_map(|(i, c)| {
        match c {
            '[' => depth += 1,
            ']' => depth = depth.saturating_sub(1),
            ',' if depth == 0 => return Some(i),
            _ => {}
        }
        None
    })
}

/// The column being typed inside the brackets of a table reference.
#[derive(Debug, PartialEq, Eq)]
pub struct ColumnContext<'a> {
    /// Empty for `[@Qty]` inside the table itself.
    pub table: &'a str,
    /// What is typed of the name so far, up to the caret.
    pub typed: &'a str,
    /// Inside the name's own brackets (`Products[[Unit Pr`), which the name then closes.
    pub in_name: bool,
    /// The bracketed key column of a keyed reference (`Products[Price,[In St`), followed by `=`.
    pub key_column: bool,
}

/// Reads `before` (the formula up to the caret) as ending in a column name of a table
/// reference: `Products[Pr`, `Products[@Pr`, `Products[[#Headers],[Pr`, `[@[Unit Pr`.
pub fn column_context(before: &str) -> Option<ColumnContext<'_>> {
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
    let table_before = |bracket: usize| {
        let head = before.get(..bracket).unwrap_or("");
        let len: usize = head.chars().rev().take_while(|c| c.is_alphanumeric() || matches!(c, '_' | '.' | '\\')).map(char::len_utf8).sum();
        head.get(head.len() - len..).unwrap_or("")
    };
    let bracket = *open.last()?;
    let typed = before.get(bracket + 1..)?;
    if text || typed.contains([',', ':', '=', '(', ')', '"']) {
        return None;
    }
    let table = table_before(bracket);
    let outer = open.len().checked_sub(2).and_then(|i| open.get(i)).copied();
    match outer {
        // `Products[[Unit Pr`, `Products[[#Headers],[Pr`, and the key column `Products[Price,[In St`.
        Some(outer) if table.is_empty() && !typed.starts_with('@') => {
            let body = before.get(outer + 1..bracket)?;
            let keyed = column_end(body).is_some_and(|comma| {
                !body.trim_start().starts_with(['#', '@']) && !body.get(..comma).is_some_and(|c| c.trim_start().starts_with("[#"))
            });
            Some(ColumnContext { table: table_before(outer), typed, in_name: true, key_column: keyed })
        }
        // `Products[Pr`, `Products[@Pr`, and `[@Pr` for the table the formula is in.
        _ => {
            let typed = typed.strip_prefix('@').unwrap_or(typed).trim_start();
            Some(ColumnContext { table, typed: typed.strip_prefix('$').unwrap_or(typed), in_name: false, key_column: false })
        }
    }
}

/// `'[` for `[`: a column name as it is written inside brackets.
fn escape_column(name: &str) -> String {
    name.replace('\'', "''").replace('[', "'[").replace(']', "']").replace('#', "'#")
}

/// Whether a key column can be written without brackets before `=`.
fn plain_word(name: &str) -> bool {
    name.starts_with(|c: char| c.is_alphabetic() || c == '_')
        && name.chars().all(|c| c.is_alphanumeric() || c == '_')
        && CellRef::parse(name).is_none()
        && !name.eq_ignore_ascii_case("TRUE")
        && !name.eq_ignore_ascii_case("FALSE")
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

/// The entries of the key column that start with what is typed, as the literals to write
/// (`"Washer"`, `1002`), in table order without repeats.
pub fn key_suggestions(sh: &gridcraft_engine::model::Sheet, t: &gridcraft_engine::model::Table, cx: &KeyContext) -> Vec<String> {
    use gridcraft_engine::core::Value;
    let index = match &cx.key_column {
        Some(name) => t.column_index(name),
        None => Some(0),
    };
    let (Some(index), Some(data)) = (index, t.data_range()) else { return vec![] };
    let col = t.range.start.col.saturating_add(index as u32);
    let quoted = cx.typed.starts_with('"');
    let typed = cx.typed.trim_start_matches('"').to_lowercase();
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
    fn keys_of_a_keyed_table_reference_are_suggested() {
        use gridcraft_engine::Session;
        use serde_json::json;
        let cx = |s: &'static str| key_context(s).map(|c| (c.table, c.key_column, c.typed));
        assert_eq!(cx("=Products[Price,Stock>=4"), Some(("Products", Some("Stock".into()), "4")));
        assert_eq!(cx("=Products[Price,Stock>1,Product<>\"W"), Some(("Products", Some("Product".into()), "\"W")));
        assert_eq!(cx("=Products[Price,$Stock<4"), Some(("Products", Some("Stock".into()), "4")));
        assert!(key_context("=Products[Price,\"a\"").is_none());
        assert!(key_context("=Products[Price,Stock>1,Pro").is_some_and(|c| !c.key && c.key_column.is_none()));
        assert_eq!(cx("=Products[Price,"), Some(("Products", None, "")));
        assert_eq!(cx("=SUM(1,Products[[Unit Price], \"Wa"), Some(("Products", None, "\"Wa")));
        assert_eq!(cx("=Products[Price,Stock=41"), Some(("Products", Some("Stock".into()), "41")));
        assert_eq!(cx("=Products[Price,[In Stock]=\"y"), Some(("Products", Some("In Stock".into()), "\"y")));
        assert_eq!(cx("=[Price,\"a,b"), Some(("", None, "\"a,b")));
        // Not a key: Excel's forms, a column still being typed, a key that is an expression
        // or already complete.
        for s in ["=Products[[#Headers],", "=Products[Pri", "=Products[[Unit Pr", "=Products[Price,A1+", "=Products[Price,\"Nut\"", "=SUM(A1,"] {
            assert_eq!(cx(s), None, "{s}");
        }

        let mut s = Session::new();
        s.new_workbook();
        let rows = json!([["Product", "Stock"], ["Nut", 500], ["Washer", 900], ["nut", 410], ["Wing \"A\"", 410], ["W*", 1]]);
        s.execute("range.setValues", json!({"range": "A1", "values": rows})).unwrap();
        s.execute("insert.table", json!({"range": "A1:B6", "header": true, "name": "Products"})).unwrap();
        let wb = &s.doc().unwrap().wb;
        let mut e = EditState::new(0, CellRef::new(0, 4), "=Products[Stock,".into(), true, false);
        e.suggest_table_parts(wb);
        assert_eq!(e.autocomplete, ["\"Nut\"", "\"Washer\"", "\"Wing \"\"A\"\"\"", "Product", "Stock"]);
        e.text.push_str("\"w");
        e.caret += 2;
        e.suggest_table_parts(wb);
        assert_eq!(e.autocomplete, ["\"Washer\"", "\"Wing \"\"A\"\"\""]);
        assert!(e.accept_autocomplete());
        assert_eq!(e.text, "=Products[Stock,\"Washer\"]");
        assert_eq!(e.caret, e.text.chars().count());
        assert!(e.autocomplete.is_empty());
        // A named key column, a number, and an existing closing bracket.
        let mut e = EditState::new(0, CellRef::new(0, 4), "=Products[Product,Stock=4]*2".into(), true, false);
        e.caret = "=Products[Product,Stock=4".chars().count();
        e.suggest_table_parts(wb);
        assert_eq!(e.autocomplete, ["410"]);
        assert!(e.accept_autocomplete());
        assert_eq!(e.text, "=Products[Product,Stock=410]*2");
        // Inside the table, its name may be left out.
        let mut e = EditState::new(0, CellRef::new(1, 1), "=[Stock,\"wa".into(), true, false);
        e.suggest_table_parts(wb);
        assert_eq!(e.autocomplete, ["\"Washer\""]);
        // Function names are still offered where no key matches.
        let mut e = EditState::new(0, CellRef::new(0, 4), "=Products[Stock,SU".into(), true, false);
        e.update_autocomplete(&["SUM".to_string()]);
        e.suggest_table_parts(wb);
        assert_eq!(e.autocomplete, ["SUM"]);
        assert!(e.accept_autocomplete());
        assert_eq!(e.text, "=Products[Stock,SUM(");
    }

    #[test]
    fn table_and_column_names_are_suggested() {
        use gridcraft_engine::Session;
        use serde_json::json;
        let mut s = Session::new();
        s.new_workbook();
        s.execute("range.setValues", json!({"range": "A1", "values": [["Product", "Unit Price", "Pack, Size"], ["Nut", 0.1, 5]]})).unwrap();
        s.execute("insert.table", json!({"range": "A1:C2", "header": true, "name": "Products"})).unwrap();
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
        // Columns: all of them after `[`, then by what is typed.
        check((0, 5), "=Products[", &["Product", "Unit Price", "Pack, Size"], 1, "=Products[Unit Price");
        check((0, 5), "=Products[p", &["Product", "Pack, Size"], 1, "=Products[[Pack, Size]");
        check((0, 5), "=Products[@un", &["Unit Price"], 0, "=Products[@Unit Price");
        check((0, 5), "=Products[[un", &["Unit Price"], 0, "=Products[[Unit Price]");
        check((0, 5), "=Products[[#Headers],[pa", &["Pack, Size"], 0, "=Products[[#Headers],[Pack, Size]");
        check((0, 5), "=Products[#h", &["#Headers"], 0, "=Products[#Headers");
        check((0, 5), "=Products[[#", &["#All", "#Data", "#Headers", "#Totals", "#This Row", "#Prev Row", "#Next Row"], 4, "=Products[[#This Row]");
        check((0, 5), "=Products[Product,[#p", &["#Prev Row"], 0, "=Products[Product,[#Prev Row]");
        // Inside the table, its name may be left out.
        check((1, 1), "=[@pr", &["Product"], 0, "=[@Product");
        // The column of a condition, and its values after the comparison.
        check((0, 5), "=Products[Product,un", &["Unit Price"], 0, "=Products[Product,[Unit Price]");
        check((0, 5), "=Products[Product,[pa", &["Pack, Size"], 0, "=Products[Product,[Pack, Size]");
        check((0, 5), "=Products[Product,[Unit Price]>0,Pro", &["Product"], 0, "=Products[Product,[Unit Price]>0,Product");
        check((0, 5), "=Products[Product,[Unit Price]>0,Product<>\"N", &["\"Nut\""], 0, "=Products[Product,[Unit Price]>0,Product<>\"Nut\"]");
        // Nothing once the name is complete, for a table in a text literal, or for another table.
        check((0, 5), "=Products[Product", &[], 0, "");
        check((0, 5), "=\"Products[p", &["PRODUCT"], 0, "=\"Products[PRODUCT(");
        check((0, 5), "=\"pro", &["PRODUCT"], 0, "=\"PRODUCT(");
        check((0, 5), "=Missing[p", &[], 0, "");
    }

    #[test]
    fn f4_anchors_a_table_column() {
        let press = |text: &str, caret: usize| {
            let mut e = EditState::new(0, CellRef::new(0, 0), text.into(), true, false);
            e.caret = caret;
            e.cycle_table_anchor().then(|| (e.text.clone(), e.caret))
        };
        let end = |s: &str| s.chars().count();
        // With the caret anywhere on the reference, or right after it.
        for caret in [5, 9, 18] {
            assert_eq!(press("=SUM(Products[Qty])+A1", caret), Some(("=SUM(Products[$Qty])+A1".into(), 19)), "{caret}");
        }
        assert_eq!(press("=SUM(Products[$Qty])+A1", 12), Some(("=SUM(Products[Qty])+A1".into(), 18)));
        let s = "=[@[Unit Price]]*2";
        assert_eq!(press(s, 3), Some(("=[@$[Unit Price]]*2".into(), 17)));
        assert_eq!(press("=Products[Price,\"Nut\"]", 4), Some(("=Products[$Price,\"Nut\"]".into(), 23)));
        // A named key column: both, the first, the second, none.
        let mut text = "=Products[Price,Stock=A2]".to_string();
        let mut seen = Vec::new();
        for _ in 0..4 {
            text = press(&text, end(&text)).unwrap().0;
            seen.push(text.clone());
        }
        assert_eq!(seen, ["=Products[$Price,$Stock=A2]", "=Products[$Price,Stock=A2]", "=Products[Price,$Stock=A2]", "=Products[Price,Stock=A2]"]);
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
    fn ref_spans() {
        let r = formula_refs("=A1+B2:C3*a1+Sheet2!D4");
        assert_eq!(r.len(), 4);
        assert_eq!(r[0].4, r[2].4); // A1 and a1 share a colour
        assert_eq!(r[1].2.a1(), "B2:C3");
        assert_eq!(r[3].3.as_deref(), Some("Sheet2"));
    }
}
