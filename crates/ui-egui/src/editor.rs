//! In-cell / formula-bar editing state: point mode, reference colouring, function
//! autocomplete and argument hints.

use egui::Color32;
use gridcraft_engine::core::{CellRef, RangeRef};
use gridcraft_engine::model::{CellRuns, Color, Font, Run, RunFont, Underline, VertAlign};
use serde_json::{Value as Json, json};

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

#[derive(Clone, Debug)]
pub struct EditState {
    pub sheet: usize,
    /// Syntax of this edit, captured when it starts (prevents a language switch reinterpreting it).
    pub locale: gridcraft_engine::formula::FormulaLocale,
    pub cell: CellRef,
    pub text: String,
    /// Rich-text runs over `text` (byte ranges), translated through every text change; `None` is
    /// flat text.
    pub runs: Option<CellRuns>,
    /// Started by typing (Enter mode: arrows commit) rather than F2 (Edit mode: arrows move the caret).
    pub enter_mode: bool,
    pub from_formula_bar: bool,
    /// Caret position (char index), mirrored from the text widget.
    pub caret: usize,
    /// The selection as char indices `(min, max)`, mirrored from the text widget; `None` when the
    /// caret is not a selection.
    pub sel: Option<(usize, usize)>,
    /// The reference most recently inserted by point mode (byte range) — arrows/clicks replace it.
    pub point: Option<(usize, usize)>,
    /// Where point mode's keyboard cursor is.
    pub point_cell: Option<CellRef>,
    pub autocomplete: Vec<String>,
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
    pub fn new(sheet: usize, cell: CellRef, text: String, enter_mode: bool, from_formula_bar: bool, runs: Option<CellRuns>) -> EditState {
        let caret = text.chars().count();
        EditState {
            sheet,
            locale: Default::default(),
            cell,
            text,
            runs,
            enter_mode,
            from_formula_bar,
            caret,
            sel: None,
            point: None,
            point_cell: None,
            autocomplete: vec![],
            ac_index: 0,
            request_focus: true,
            sync_caret: true,
            caret_to_end: true,
            completion: None,
        }
    }

    /// The single text-mutation point: translates the runs through the change (dropping them when
    /// the result is flat) and replaces the text. Runs never survive a turn into a formula: a
    /// committed formula is one value, so its commit must not carry runs.
    pub fn set_text(&mut self, new: String) {
        self.runs = self.runs.take().and_then(|r| r.edited(&self.text, &new));
        self.text = new;
        if self.is_formula() {
            self.runs = None;
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
        self.set_text(format!("{a}{r}{b}"));
        let new_end = start + r.len();
        self.point = Some((start, new_end));
        self.caret = self.text.get(..new_end).map(|s| s.chars().count()).unwrap_or(0);
        self.caret_to_end = false;
        self.sync_caret = true;
    }

    /// Updates function autocomplete for the word before the caret.
    pub fn update_autocomplete(&mut self, names: &[String]) {
        self.autocomplete.clear();
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

    /// Accepts the highlighted autocomplete entry: replaces the word and adds `(`.
    pub fn accept_autocomplete(&mut self) -> bool {
        let Some(name) = self.autocomplete.get(self.ac_index).cloned() else { return false };
        let caret_b = self.byte_caret();
        let before = self.text.get(..caret_b).unwrap_or("");
        let wlen: usize = before.chars().rev().take_while(|c| c.is_ascii_alphanumeric() || *c == '.' || *c == '_').map(char::len_utf8).sum();
        let start = caret_b - wlen;
        let (Some(a), Some(b)) = (self.text.get(..start), self.text.get(caret_b..)) else { return false };
        self.set_text(format!("{a}{name}({b}"));
        self.caret = self.text.get(..start + name.len() + 1).map(|s| s.chars().count()).unwrap_or(0);
        self.autocomplete.clear();
        self.caret_to_end = false;
        self.sync_caret = true;
        true
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

/// The selection as a byte range on `ed.text`: `None` without a selection (caret only). Char
/// indices map through `char_indices`; one at or past the end maps to the text end.
pub fn sel_bytes(ed: &EditState) -> Option<std::ops::Range<usize>> {
    let (from, to) = ed.sel?;
    if from >= to {
        return None;
    }
    let byte = |at: usize| ed.text.char_indices().nth(at).map(|(i, _)| i).unwrap_or(ed.text.len());
    Some(byte(from)..byte(to))
}

/// The Font-group commands that hit the edit buffer while an edit is active (the ribbon's
/// `act`/`invoke`, keytips and shortcuts all funnel through [`crate::SheetApp::run`]).
pub const RUN_FONT_IDS: &[&str] = &[
    "home.bold",
    "home.italic",
    "home.underline",
    "home.doubleUnderline",
    "home.strikethrough",
    "home.superscript",
    "home.subscript",
    "home.fontName",
    "home.fontSize",
    "home.increaseFontSize",
    "home.decreaseFontSize",
    "home.fontColor",
    "home.formatCells",
];

/// Decides whether a Font-group command while editing applies to the edit buffer instead of the
/// engine: `None` = do not intercept (fall through to the engine command — also for the
/// programmatic `cell`/`range`/`chars` forms, a formula edit, a caret-only selection and
/// commands missing their key param, which the engine validates); `Some(Ok(apps))` = apply each
/// `(byte range, delta)` to the buffer runs in order (one entry for most commands, one per
/// covered run for the size steppers); `Some(Err(msg))` = swallow with an alert (e.g. the run
/// cap). The engine fn twins live there ([`all_covered`] mirrors the private
/// `run_toggle_target`, `color_param`/`step_size` mirror theirs) so UI and headless agree.
pub fn intercept_run_delta(id: &str, params: &Json, ed: &EditState, base: &Font) -> Option<Result<Vec<(std::ops::Range<usize>, RunFont)>, String>> {
    // The programmatic forms always reach the engine; so does anything outside the Font group.
    if !RUN_FONT_IDS.contains(&id) || params.get("cell").is_some() || params.get("range").is_some() || params.get("chars").is_some() {
        return None;
    }
    // Runs never apply to a formula.
    if ed.is_formula() {
        return None;
    }
    let range = sel_bytes(ed)?;
    let on = params.get("on").and_then(Json::as_bool);
    match id {
        "home.bold" => {
            one_app(&range, RunFont { bold: Some(on.unwrap_or_else(|| !all_covered(ed, &range, base, |rf| rf.bold))), ..RunFont::default() })
        }
        "home.italic" => {
            one_app(&range, RunFont { italic: Some(on.unwrap_or_else(|| !all_covered(ed, &range, base, |rf| rf.italic))), ..RunFont::default() })
        }
        "home.strikethrough" => {
            one_app(&range, RunFont { strike: Some(on.unwrap_or_else(|| !all_covered(ed, &range, base, |rf| rf.strike))), ..RunFont::default() })
        }
        "home.underline" | "home.doubleUnderline" => {
            let style = if id == "home.doubleUnderline" {
                Underline::Double
            } else {
                match params.get("style").and_then(Json::as_str) {
                    Some("double") => Underline::Double,
                    Some("singleAccounting") => Underline::SingleAccounting,
                    Some("doubleAccounting") => Underline::DoubleAccounting,
                    Some("none") => Underline::None,
                    _ => Underline::Single,
                }
            };
            // The engine's chars path resolves a missing `on` via the all-covered toggle over
            // the requested style's underline; off stores the explicit clear.
            let on = on.unwrap_or_else(|| !all_covered(ed, &range, base, underline_get(style)));
            let u = if on { style } else { Underline::None };
            one_app(&range, RunFont { underline: Some(u), ..RunFont::default() })
        }
        "home.superscript" => {
            // Toggle like the engine's chars path: an all-superscript selection goes back to
            // baseline, anything else raises.
            let on = on.unwrap_or_else(|| !all_covered(ed, &range, base, |rf: &RunFont| rf.vert.map(|v| v == VertAlign::Superscript)));
            one_app(&range, RunFont { vert: Some(if on { VertAlign::Superscript } else { VertAlign::Baseline }), ..RunFont::default() })
        }
        "home.subscript" => {
            let on = on.unwrap_or_else(|| !all_covered(ed, &range, base, |rf: &RunFont| rf.vert.map(|v| v == VertAlign::Subscript)));
            one_app(&range, RunFont { vert: Some(if on { VertAlign::Subscript } else { VertAlign::Baseline }), ..RunFont::default() })
        }
        "home.fontName" => {
            params.get("name").and_then(Json::as_str).map(|name| Ok(vec![(range, RunFont { name: Some(name.to_string()), ..RunFont::default() })]))
        }
        "home.fontSize" => {
            let size = params.get("size").and_then(Json::as_f64)?;
            if !(1.0..=409.0).contains(&size) {
                return Some(Err("The number must be between 1 and 409.".into()));
            }
            let size = ((size * 2.0).round() / 2.0) as f32;
            one_app(&range, RunFont { size: Some(size), ..RunFont::default() })
        }
        "home.increaseFontSize" | "home.decreaseFontSize" => {
            let up = id == "home.increaseFontSize";
            // Without runs the buffer behaves like one all-default run over the whole text.
            let fallback = Run { start: 0, len: u32::try_from(ed.text.len()).unwrap_or(u32::MAX), font: RunFont::default() };
            let runs: Vec<&Run> = match &ed.runs {
                Some(r) => r.runs.iter().collect(),
                None => vec![&fallback],
            };
            let end = range.end.min(ed.text.len());
            let start = range.start.min(end);
            // Not one delta: every covered run steps from its own merged size (the engine's
            // `step_chars_font` ruling), so applying in order splits the selection edges.
            let mut apps = Vec::new();
            for run in runs {
                let rs = run.start as usize;
                let re = rs.saturating_add(run.len as usize);
                if re <= start || rs >= end {
                    continue;
                }
                let step = step_size(run.font.merged(base).size, up);
                apps.push((rs.max(start)..re.min(end), RunFont { size: Some(step), ..RunFont::default() }));
            }
            Some(Ok(apps))
        }
        "home.fontColor" => color_param(params.get("color")).map(|c| Ok(vec![(range, RunFont { color: Some(c), ..RunFont::default() })])),
        "home.formatCells" => {
            // Only `style.font` can apply to a character selection; without it the engine
            // handles the command (dialog or cell-level patch).
            let style = params.get("style")?;
            let font = style.get("font")?;
            if style.as_object().is_some_and(|o| o.keys().any(|k| k != "font")) {
                return Some(Err("home.formatCells: only `style.font` can apply to a character selection".into()));
            }
            match serde_json::from_value::<RunFont>(font.clone()) {
                Ok(delta) => one_app(&range, delta),
                Err(e) => Some(Err(format!("home.formatCells: {e}"))),
            }
        }
        _ => None,
    }
}

/// The single application most commands produce.
fn one_app(range: &std::ops::Range<usize>, delta: RunFont) -> Option<Result<Vec<(std::ops::Range<usize>, RunFont)>, String>> {
    Some(Ok(vec![(range.clone(), delta)]))
}

/// The all-covered rule, the local twin of the engine's private `run_toggle_target`: true only
/// when every covered character's run font, merged over `base`, has the property `get` reads.
/// `false` when any covered character lacks it — or nothing is covered (no runs) — so the next
/// press turns it on.
fn all_covered(ed: &EditState, range: &std::ops::Range<usize>, base: &Font, get: fn(&RunFont) -> Option<bool>) -> bool {
    let Some(runs) = &ed.runs else { return false };
    let end = range.end.min(ed.text.len());
    let start = range.start.min(end);
    let mut covered = false;
    for run in &runs.runs {
        let rs = run.start as usize;
        let re = rs.saturating_add(run.len as usize);
        if re <= start || rs >= end {
            continue;
        }
        // Straddling runs count: format_range keeps their font on the split pieces.
        covered = true;
        let f = run.font.merged(base);
        let resolved = RunFont {
            name: Some(f.name.clone()),
            size: Some(f.size),
            bold: Some(f.bold),
            italic: Some(f.italic),
            underline: Some(f.underline),
            strike: Some(f.strike),
            color: Some(f.color),
            vert: Some(f.vert),
        };
        if get(&resolved) != Some(true) {
            return false;
        }
    }
    covered
}

/// A [`all_covered`] property for underline styles: the resolved underline equals `u` (a plain
/// `fn`, since the helper takes a function pointer) — the local twin of the engine's
/// `underline_get`.
fn underline_get(u: Underline) -> fn(&RunFont) -> Option<bool> {
    match u {
        Underline::None => |rf: &RunFont| Some(rf.underline == Some(Underline::None)),
        Underline::Single => |rf: &RunFont| Some(rf.underline == Some(Underline::Single)),
        Underline::Double => |rf: &RunFont| Some(rf.underline == Some(Underline::Double)),
        Underline::SingleAccounting => |rf: &RunFont| Some(rf.underline == Some(Underline::SingleAccounting)),
        Underline::DoubleAccounting => |rf: &RunFont| Some(rf.underline == Some(Underline::DoubleAccounting)),
    }
}

/// The `color` param (`"#RRGGBB"`, `"auto"`, or `{theme, tint}`): the local twin of the engine's
/// crate-private param read, same forms and clamps.
fn color_param(v: Option<&Json>) -> Option<Color> {
    match v? {
        Json::String(s) if s.eq_ignore_ascii_case("auto") || s.eq_ignore_ascii_case("automatic") || s.eq_ignore_ascii_case("none") => {
            Some(Color::Auto)
        }
        Json::String(s) => Color::from_hex(s),
        Json::Object(o) => {
            let theme = o.get("theme")?.as_u64()? as u8;
            let tint = o.get("tint").and_then(Json::as_f64).unwrap_or(0.0);
            Some(Color::Theme(theme.min(11), (tint.clamp(-1.0, 1.0) * 1000.0) as i16))
        }
        _ => None,
    }
}

/// One size step from `cur`, saturating at the cell-level bounds (1–409) like the engine's
/// size steppers.
fn step_size(cur: f32, up: bool) -> f32 {
    if !cur.is_finite() {
        return if up { 409.0 } else { 1.0 };
    }
    if up { (cur + 1.0).clamp(1.0, 409.0) } else { (cur - 1.0).clamp(1.0, 409.0) }
}

/// The runs of flat text: one all-default run over `len` bytes (the engine's fallback too).
fn default_runs(len: usize) -> CellRuns {
    CellRuns { runs: vec![Run { start: 0, len: u32::try_from(len).unwrap_or(u32::MAX), font: RunFont::default() }] }
}

/// Applies intercepted `(byte range, delta)` pairs to the edit buffer's runs, in order. A flat
/// result — or a buffer without runs — behaves like one all-default run for the next pair; a
/// final flat result leaves `ed.runs` `None`. `Err` carries the alert message (the per-cell run
/// cap).
pub fn apply_run_fonts(ed: &mut EditState, apps: Vec<(std::ops::Range<usize>, RunFont)>) -> Result<(), String> {
    for (range, delta) in apps {
        let current = ed.runs.take().unwrap_or_else(|| default_runs(ed.text.len()));
        ed.runs = current.format_range(&ed.text, range, &delta).map_err(|_| "too many runs".to_string())?;
    }
    Ok(())
}

/// The `cell.set` params committing `ed`: the cell, the text, and the runs when present. A
/// formula commits one value and never carries runs, whatever path the text took (an edit can
/// only hold runs as text).
pub fn commit_params(ed: &EditState) -> Json {
    let mut params = json!({ "cell": ed.cell.a1(), "input": ed.text });
    if !ed.is_formula()
        && let Some(runs) = &ed.runs
        && let Ok(value) = serde_json::to_value(runs)
    {
        params["runs"] = value;
    }
    params
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
                if t.to_lowercase().starts_with(&lower) && t.len() > typed.len() {
                    match &found {
                        Some(f) if !f.eq_ignore_ascii_case(&t) => ambiguous = true,
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

#[cfg(test)]
mod tests {
    use super::*;
    use gridcraft_engine::model::{CellRuns, Color, Font, Run, RunFont};

    /// Two runs over "black red": "black" bold, " red" red.
    fn black_red_runs() -> CellRuns {
        CellRuns {
            runs: vec![
                Run { start: 0, len: 5, font: RunFont { bold: Some(true), ..RunFont::default() } },
                Run { start: 5, len: 4, font: RunFont { color: Some(Color::rgb(0xFF, 0, 0)), ..RunFont::default() } },
            ],
        }
    }

    /// An edit over `text` with the selection preset (the widget usually mirrors it).
    fn edit_sel(text: &str, sel: Option<(usize, usize)>, runs: Option<CellRuns>) -> EditState {
        let mut e = EditState::new(0, CellRef::new(0, 0), text.into(), true, false, runs);
        e.sel = sel;
        e
    }

    #[test]
    fn point_mode() {
        let mut e = EditState::new(0, CellRef::new(0, 0), "=SUM(".into(), true, false, None);
        assert!(e.can_point());
        e.insert_ref("A1");
        assert_eq!(e.text, "=SUM(A1");
        e.insert_ref("A2");
        assert_eq!(e.text, "=SUM(A2");
        e.text.push(':');
        e.caret += 1;
        e.point = None;
        assert!(e.can_point());
        let e2 = EditState::new(0, CellRef::new(0, 0), "hello".into(), true, false, None);
        assert!(!e2.can_point());
    }

    #[test]
    fn autocomplete_and_hints() {
        let names: Vec<String> = ["SUM", "SUMIF", "SUMPRODUCT", "AVERAGE"].iter().map(|s| s.to_string()).collect();
        let mut e = EditState::new(0, CellRef::new(0, 0), "=su".into(), true, false, None);
        e.update_autocomplete(&names);
        assert_eq!(e.autocomplete.len(), 3);
        assert!(e.accept_autocomplete());
        assert_eq!(e.text, "=SUM(");
        let e = EditState::new(0, CellRef::new(0, 0), "=IF(A1>0,SUM(B1,".into(), true, false, None);
        assert_eq!(e.current_function(), Some(("SUM".into(), 1)));
    }

    #[test]
    fn set_text_translates_runs() {
        let mut e = EditState::new(0, CellRef::new(0, 0), "black red".into(), true, false, Some(black_red_runs()));
        e.set_text("blackx red".into());
        assert!(e.runs.as_ref().is_some_and(|r| r.valid_for("blackx red")));
        // The " red" run shifted past the inserted `x`.
        assert_eq!(e.runs.as_ref().and_then(|r| r.runs.get(1)).map(|run| run.start), Some(6));
    }

    #[test]
    fn set_text_delete_all_drops_runs() {
        let mut e = EditState::new(0, CellRef::new(0, 0), "black red".into(), true, false, Some(black_red_runs()));
        e.set_text(String::new());
        assert!(e.runs.is_none());
        assert!(e.text.is_empty());
    }

    #[test]
    fn commit_params_includes_runs_when_present() {
        let e = EditState::new(0, CellRef::new(0, 0), "black red".into(), true, false, Some(black_red_runs()));
        let p = commit_params(&e);
        assert_eq!(p.get("cell"), Some(&json!("A1")));
        assert_eq!(p.get("input"), Some(&json!("black red")));
        assert_eq!(p.get("runs").and_then(|r| r.get("runs")).and_then(|r| r.as_array()).map(|a| a.len()), Some(2));
    }

    #[test]
    fn commit_params_omits_runs_when_none() {
        let e = EditState::new(0, CellRef::new(0, 0), "black red".into(), true, false, None);
        let p = commit_params(&e);
        assert_eq!(p.get("cell"), Some(&json!("A1")));
        assert_eq!(p.get("input"), Some(&json!("black red")));
        assert!(p.get("runs").is_none());
    }

    #[test]
    fn sel_bytes_maps_chars_to_bytes() {
        let mut e = EditState::new(0, CellRef::new(0, 0), "é ok".into(), true, false, None);
        e.sel = Some((1, 3));
        assert_eq!(sel_bytes(&e), Some(2..4));
    }

    #[test]
    fn sel_bytes_none_when_caret_only() {
        let e = EditState::new(0, CellRef::new(0, 0), "é ok".into(), true, false, None);
        assert_eq!(sel_bytes(&e), None);
    }

    #[test]
    fn sel_bytes_none_when_empty_selection() {
        let mut e = edit_sel("black red", Some((3, 3)), None);
        assert_eq!(sel_bytes(&e), None);
        e.sel = Some((4, 2));
        assert_eq!(sel_bytes(&e), None);
    }

    #[test]
    fn set_text_drops_runs_when_edit_becomes_formula() {
        let mut e = EditState::new(0, CellRef::new(0, 0), "black red".into(), true, false, Some(black_red_runs()));
        e.set_text("=SUM(A1)".into());
        assert!(e.runs.is_none());
        assert!(e.is_formula());
    }

    #[test]
    fn commit_params_omits_runs_for_a_formula() {
        // A path that bypassed `set_text` (control-driven text) leaves stale runs: the commit
        // must still not send them for a formula.
        let mut e = EditState::new(0, CellRef::new(0, 0), "black red".into(), true, false, Some(black_red_runs()));
        e.text = "=SUM(A1)".into();
        assert!(commit_params(&e).get("runs").is_none());
    }

    #[test]
    fn intercept_fontcolor() {
        let e = edit_sel("black red green", Some((6, 9)), None);
        let apps = intercept_run_delta("home.fontColor", &json!({"color": "#FF0000"}), &e, &Font::default()).unwrap().unwrap();
        assert_eq!(apps.len(), 1);
        assert_eq!(apps[0].0, 6..9);
        assert_eq!(apps[0].1.color, Some(Color::rgb(0xFF, 0, 0)));
    }

    #[test]
    fn intercept_resolves_toggle() {
        let e = edit_sel("black red", Some((0, 9)), Some(black_red_runs()));
        // Not all covered — bold "black", plain " red" — so the press turns bold on everywhere.
        let apps = intercept_run_delta("home.bold", &json!({}), &e, &Font::default()).unwrap().unwrap();
        assert_eq!(apps.len(), 1);
        assert_eq!(apps[0].1.bold, Some(true));
        // An explicit `on` wins over the all-covered rule.
        let apps = intercept_run_delta("home.bold", &json!({"on": false}), &e, &Font::default()).unwrap().unwrap();
        assert_eq!(apps[0].1.bold, Some(false));
    }

    #[test]
    fn intercept_skips() {
        let base = Font::default();
        // A formula edit is never intercepted.
        let f = edit_sel("=SUM(A1)", Some((2, 5)), None);
        assert!(intercept_run_delta("home.bold", &json!({}), &f, &base).is_none());
        // Caret only: nothing covered.
        let e = edit_sel("black red", None, Some(black_red_runs()));
        assert!(intercept_run_delta("home.bold", &json!({}), &e, &base).is_none());
        // The programmatic forms (cell/range/chars) always reach the engine.
        let e = edit_sel("black red", Some((0, 5)), None);
        for p in [json!({"cell": "A1"}), json!({"range": "A1:B2"}), json!({"chars": [0, 4]})] {
            assert!(intercept_run_delta("home.bold", &p, &e, &base).is_none());
        }
        // An id outside the Font group falls through.
        assert!(intercept_run_delta("home.fillColor", &json!({"color": "#FFFF00"}), &e, &base).is_none());
    }

    #[test]
    fn intercept_increase_steps_per_run() {
        let runs = CellRuns {
            runs: vec![
                Run { start: 0, len: 2, font: RunFont { size: Some(11.0), ..RunFont::default() } },
                Run { start: 2, len: 3, font: RunFont { size: Some(14.0), ..RunFont::default() } },
            ],
        };
        let e = edit_sel("aa bb", Some((0, 5)), Some(runs));
        let apps = intercept_run_delta("home.increaseFontSize", &json!({}), &e, &Font::default()).unwrap().unwrap();
        assert_eq!(apps.len(), 2);
        assert_eq!((apps[0].0.clone(), apps[0].1.size), (0..2, Some(12.0)));
        assert_eq!((apps[1].0.clone(), apps[1].1.size), (2..5, Some(15.0)));
    }

    #[test]
    fn intercept_underline_toggles_like_the_engine() {
        // All underlined: the press clears (explicit clear, `Underline::None`); the next applies
        // again. An explicit `on` still wins over the all-covered rule.
        let runs = CellRuns { runs: vec![Run { start: 0, len: 5, font: RunFont { underline: Some(Underline::Single), ..RunFont::default() } }] };
        let mut e = edit_sel("black", Some((0, 5)), Some(runs));
        let base = Font::default();
        let apps = intercept_run_delta("home.underline", &json!({}), &e, &base).unwrap().unwrap();
        assert_eq!(apps[0].1.underline, Some(Underline::None));
        apply_run_fonts(&mut e, apps).unwrap();
        // The explicit clear survives as one run over the whole text (an all-`None` font would
        // be flat, `Some(None)` is the stored "off", like the engine).
        assert!(e.runs.as_ref().is_some_and(|r| r.runs.iter().all(|run| run.font.underline == Some(Underline::None))));
        let apps = intercept_run_delta("home.underline", &json!({}), &e, &base).unwrap().unwrap();
        assert_eq!(apps[0].1.underline, Some(Underline::Single));
        apply_run_fonts(&mut e, apps).unwrap();
        assert!(e.runs.as_ref().is_some_and(|r| r.runs.iter().all(|run| run.font.underline == Some(Underline::Single))));
        // Explicit `on` is honored whatever the buffer holds.
        let apps = intercept_run_delta("home.underline", &json!({"on": true}), &e, &base).unwrap().unwrap();
        assert_eq!(apps[0].1.underline, Some(Underline::Single));
        let apps = intercept_run_delta("home.underline", &json!({"on": false}), &e, &base).unwrap().unwrap();
        assert_eq!(apps[0].1.underline, Some(Underline::None));
    }

    #[test]
    fn intercept_superscript_toggles_like_the_engine() {
        // All superscript: the press returns to baseline; the next raises again.
        let runs = CellRuns { runs: vec![Run { start: 0, len: 5, font: RunFont { vert: Some(VertAlign::Superscript), ..RunFont::default() } }] };
        let mut e = edit_sel("black", Some((0, 5)), Some(runs));
        let base = Font::default();
        let apps = intercept_run_delta("home.superscript", &json!({}), &e, &base).unwrap().unwrap();
        assert_eq!(apps[0].1.vert, Some(VertAlign::Baseline));
        apply_run_fonts(&mut e, apps).unwrap();
        // The explicit baseline survives as one run over the whole text (the engine's off
        // stores `Some(VertAlign::Baseline)`, which is not an all-`None` font).
        assert!(e.runs.as_ref().is_some_and(|r| r.runs.iter().all(|run| run.font.vert == Some(VertAlign::Baseline))));
        let apps = intercept_run_delta("home.superscript", &json!({}), &e, &base).unwrap().unwrap();
        assert_eq!(apps[0].1.vert, Some(VertAlign::Superscript));
        apply_run_fonts(&mut e, apps).unwrap();
        assert!(e.runs.as_ref().is_some_and(|r| r.runs.iter().all(|run| run.font.vert == Some(VertAlign::Superscript))));
    }

    #[test]
    fn run_routes_font_commands_into_the_buffer() {
        let mut session = gridcraft_engine::Session::new();
        session.new_workbook();
        let mut app = crate::SheetApp::new(session, crate::Services::default());
        assert!(app.session.run("cell.set", json!({"cell": "A1", "input": "black red"})).is_ok());
        app.begin_edit(Some("black red".into()), false);
        if let Some(ed) = app.editor.as_mut() {
            ed.sel = Some((0, 9));
        }
        // Ribbon, keytips and shortcuts all funnel through `run`: the bold hits the buffer.
        assert_eq!(app.run("home.bold", json!({})), Ok(Json::Null));
        let ed = app.editor.as_ref().unwrap();
        assert!(ed.runs.as_ref().is_some_and(|r| r.runs.iter().all(|run| run.font.bold == Some(true))));
        // Caret only: swallowed (Excel parity), the buffer untouched.
        if let Some(ed) = app.editor.as_mut() {
            ed.sel = None;
        }
        assert_eq!(app.run("home.italic", json!({})), Ok(Json::Null));
        let ed = app.editor.as_ref().unwrap();
        assert!(ed.runs.as_ref().is_some_and(|r| r.runs.iter().all(|run| run.font.italic.is_none())));
        // The cell on the sheet is untouched until the edit commits.
        let d = app.session.active().unwrap();
        assert!(!d.wb.active().unwrap().cell_runs.contains_key(&CellRef::new(0, 0)));
        // The programmatic form still reaches the engine.
        assert!(app.run("home.italic", json!({"cell": "A1", "chars": [0, 9], "on": true})).is_ok());
        let d = app.session.active().unwrap();
        assert!(d.wb.active().unwrap().cell_runs.contains_key(&CellRef::new(0, 0)));
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
