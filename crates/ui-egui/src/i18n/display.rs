//! Display order for the labels of a right-to-left interface language (the Arabic catalog).
//!
//! egui shapes each run of one font face on its own, in the script's direction, so the letters of
//! an Arabic word join and run right to left. It places the runs themselves left to right, in the
//! order they are written. A caption such as `حفظ باسم` would therefore show its two words
//! swapped. [`label`] puts the runs into display order and leaves the letters of each word alone
//! for the shaper. The catalog is written in normal typing order and converted once, at load.

use unicode_bidi::{BidiInfo, Level};

/// First of the private-use characters that stand for placeholders while a line is reordered.
/// They read as strong left-to-right text, like the names and numbers they replace.
const STAND_IN_BASE: u32 = 0xE000;
/// Placeholders and shortcuts protected per line; a line with more keeps the rest as plain text.
const MAX_ATOMS: usize = 64;

/// `text` (one catalog entry, in logical order) rearranged for egui: each line reads right to
/// left, with Latin words and numbers inside it left to right.
///
/// - `{name}` placeholders stay whole and unmirrored, so the code that fills them in still finds
///   them, and what it inserts is drawn left to right at that place.
/// - Keyboard shortcuts (`⌘⇧T`) stay whole too: their symbols have no direction of their own.
/// - Spaces and separator dots at either end stay at that end, because the code appends to a label
///   on its right-hand end whatever the language.
/// - A caption's closing colon ends up on the left, where the caption ends: the window is
///   mirrored, so the widget it captions is on that side.
///
/// Text without right-to-left characters comes back unchanged. This is for painting one line: a
/// label that egui wraps breaks at the wrong end.
pub fn label(text: &str) -> String {
    if !text.chars().any(is_rtl_script) {
        return text.to_owned();
    }
    let mut out = String::with_capacity(text.len());
    for (i, line) in text.split('\n').enumerate() {
        if i > 0 {
            out.push('\n');
        }
        push_line(&mut out, line);
    }
    out
}

fn push_line(out: &mut String, line: &str) {
    let edge = |c: char| c.is_whitespace() || c == '·';
    let core = line.trim_start_matches(edge);
    let lead = line.get(..line.len().saturating_sub(core.len())).unwrap_or_default();
    let core_end = core.trim_end_matches(edge);
    let tail = core.get(core_end.len()..).unwrap_or_default();
    let core = core_end;
    // A line that already uses the stand-in characters is reordered with its atoms as plain text.
    let reserved = core.chars().any(|c| (STAND_IN_BASE..STAND_IN_BASE.saturating_add(MAX_ATOMS as u32)).contains(&u32::from(c)));
    let (masked, atoms) = if reserved { (core.to_owned(), Vec::new()) } else { mask_atoms(core) };
    let masked = bind_leading_dots(&masked);
    let mut visual = String::with_capacity(masked.len());
    push_visual(&mut visual, &masked);
    out.push_str(lead);
    for c in visual.chars() {
        let index = u32::from(c).checked_sub(STAND_IN_BASE).and_then(|i| usize::try_from(i).ok());
        match index.and_then(|i| atoms.get(i)) {
            Some(atom) => out.push_str(atom),
            None => out.push(c),
        }
    }
    out.push_str(tail);
}

/// `text` with a left-to-right mark before each dot that starts a Latin word (`.xlsx`, `.{format}`):
/// left alone, the dot has no direction of its own and would be drawn on the wrong side of it.
fn bind_leading_dots(text: &str) -> String {
    let stand_in = STAND_IN_BASE..STAND_IN_BASE.saturating_add(MAX_ATOMS as u32);
    let mut out = String::with_capacity(text.len());
    let mut previous: Option<char> = None;
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        let starts_word = chars.peek().is_some_and(|next| next.is_ascii_alphanumeric() || stand_in.contains(&u32::from(*next)));
        if c == '.' && starts_word && previous.is_none_or(char::is_whitespace) {
            out.push('\u{200E}');
        }
        out.push(c);
        previous = Some(c);
    }
    out
}

/// `text` in display order for a right-to-left paragraph, appended to `out`.
fn push_visual(out: &mut String, text: &str) {
    let info = BidiInfo::new(text, Some(Level::rtl()));
    for para in &info.paragraphs {
        let (levels, runs) = info.visual_runs(para, para.range.clone());
        for run in runs {
            let rtl = levels.get(run.start).is_some_and(Level::is_rtl);
            // Runs lie on character boundaries; a missing slice is skipped, never a panic.
            let Some(piece) = text.get(run) else { continue };
            if rtl {
                push_rtl_run(out, piece);
            } else {
                out.extend(piece.chars().filter(|c| !is_bidi_control(*c)));
            }
        }
    }
}

/// One right-to-left run in display order. Stretches of right-to-left script stay in logical
/// order (egui hands each to the shaper as one face run, which reverses and joins it); what lies
/// between them (spaces and punctuation, drawn by the Latin face and so left to right) is
/// reversed here, with brackets mirrored.
fn push_rtl_run(out: &mut String, run: &str) {
    let mut pieces: Vec<(bool, String)> = Vec::new();
    for c in run.chars().filter(|c| !is_bidi_control(*c)) {
        let script = is_rtl_script(c);
        match pieces.last_mut() {
            Some((kind, piece)) if *kind == script => piece.push(c),
            _ => pieces.push((script, c.to_string())),
        }
    }
    for (script, piece) in pieces.iter().rev() {
        if *script {
            out.push_str(piece);
        } else {
            out.extend(piece.chars().rev().map(mirrored));
        }
    }
}

/// `text` with each `{name}` placeholder and each keyboard shortcut replaced by one stand-in
/// character, and what they replaced, in order.
fn mask_atoms(text: &str) -> (String, Vec<&str>) {
    let mut masked = String::with_capacity(text.len());
    let mut atoms: Vec<&str> = Vec::new();
    let mut rest = text;
    while atoms.len() < MAX_ATOMS
        && let Some((start, end)) = next_atom(rest)
    {
        let (Some(before), Some(atom), Some(after)) = (rest.get(..start), rest.get(start..end), rest.get(end..)) else { break };
        // `atoms.len()` is below MAX_ATOMS, so the sum is a private-use scalar.
        let Some(stand_in) = u32::try_from(atoms.len()).ok().and_then(|i| char::from_u32(STAND_IN_BASE.saturating_add(i))) else { break };
        masked.push_str(before);
        masked.push(stand_in);
        atoms.push(atom);
        rest = after;
    }
    masked.push_str(rest);
    (masked, atoms)
}

/// The byte range of the first placeholder or keyboard shortcut in `text`.
fn next_atom(text: &str) -> Option<(usize, usize)> {
    let placeholder = text.find('{').and_then(|open| {
        let len = text.get(open..)?.find('}')?;
        Some((open, open.saturating_add(len).saturating_add(1)))
    });
    let shortcut = text.find(is_modifier_key).map(|at| {
        // The shortcut is the stretch around the symbol up to a space, a bracket or script text.
        let outside = |c: char| c.is_whitespace() || is_rtl_script(c) || matches!(c, '(' | ')' | '[' | ']' | '{' | '}');
        let start = text
            .get(..at)
            .and_then(|before| before.rfind(outside).and_then(|i| before.get(i..)?.chars().next().map(|c| i.saturating_add(c.len_utf8()))))
            .unwrap_or(0);
        let end = text.get(at..).and_then(|after| after.find(outside)).map_or(text.len(), |i| at.saturating_add(i));
        (start, end)
    });
    match (placeholder, shortcut) {
        (Some(p), Some(s)) => Some(if s.0 < p.0 { s } else { p }),
        (p, s) => p.or(s),
    }
}

/// The modifier-key symbols of keyboard shortcuts: ⌘ ⇧ ⌥ ⌃.
fn is_modifier_key(c: char) -> bool {
    matches!(c, '\u{2318}' | '\u{21E7}' | '\u{2325}' | '\u{2303}')
}

/// Characters of the right-to-left scripts' blocks: letters, marks and the scripts' own
/// punctuation, all drawn by the script's face.
pub(crate) fn is_rtl_script(c: char) -> bool {
    matches!(u32::from(c), 0x0590..=0x08FF | 0xFB1D..=0xFDFF | 0xFE70..=0xFEFC | 0x1_0800..=0x1_0FFF | 0x1_E800..=0x1_EFFF)
}

/// Direction marks, embeddings, overrides and isolates: they steer the reordering and are not drawn.
fn is_bidi_control(c: char) -> bool {
    matches!(c, '\u{061C}' | '\u{200E}' | '\u{200F}' | '\u{202A}'..='\u{202E}' | '\u{2066}'..='\u{2069}')
}

fn mirrored(c: char) -> char {
    match c {
        '(' => ')',
        ')' => '(',
        '[' => ']',
        ']' => '[',
        '{' => '}',
        '}' => '{',
        '<' => '>',
        '>' => '<',
        '«' => '»',
        '»' => '«',
        other => other,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn text_without_right_to_left_script_is_unchanged() {
        for s in ["", "Save as…", "{n} pages", "日本語", "A (b) [c]"] {
            assert_eq!(label(s), s);
        }
    }

    #[test]
    fn words_swap_and_letters_stay_in_logical_order_for_the_shaper() {
        assert_eq!(label("حفظ"), "حفظ");
        assert_eq!(label("حفظ باسم…"), "…باسم حفظ");
        assert_eq!(label("إدراج دالة جديدة"), "جديدة دالة إدراج");
    }

    #[test]
    fn latin_words_and_numbers_keep_their_own_order() {
        assert_eq!(label("إنشاء PDF من ورقة"), "ورقة من PDF إنشاء");
        assert_eq!(label("صالح لمدة 5 سنوات"), "سنوات 5 لمدة صالح");
        assert_eq!(label("نص (عادي)"), "(عادي) نص");
    }

    #[test]
    fn shortcuts_and_placeholders_stay_whole() {
        assert_eq!(label("لصق (⌘V)"), "(⌘V) لصق");
        assert_eq!(label("جمع تلقائي (⌘⇧T)"), "(⌘⇧T) تلقائي جمع");
        assert_eq!(label("الصفحة {page} من {pages}"), "{pages} من {page} الصفحة");
        assert_eq!(label("{count} خلايا"), "خلايا {count}");
        assert_eq!(label("جارٍ البحث… {a}/{b}"), "{a}/{b} …البحث جارٍ");
        // Braces that are not a placeholder are ordinary punctuation.
        assert_eq!(label("حرف } واحد"), "واحد { حرف");
        // A dot that starts a Latin word stays on its left.
        assert_eq!(label("حفظ بصيغة .xlsx"), ".xlsx بصيغة حفظ");
        assert_eq!(label("حفظ بصيغة .{format}"), ".{format} بصيغة حفظ");
    }

    #[test]
    fn edges_and_colons_land_where_the_code_appends() {
        assert_eq!(label(" (صفحة {p})"), " ({p} صفحة)");
        assert_eq!(label("الاسم:"), ":الاسم");
        assert_eq!(label("البحث عن:"), ":عن البحث");
        // Each line is its own paragraph.
        assert_eq!(label("ورقة\nعمل جديد"), "ورقة\nجديد عمل");
    }

    #[test]
    fn odd_input_never_panics() {
        let long = "ا {n} ".repeat(20_000);
        for s in ["ا{", "ا}", "ا{}", "ا{{{", "}ا{", "ا\u{E000}{n}", "\u{202B}ا\u{202C}", "ا ⌘", "⌘ا", &long] {
            let _ = label(s);
        }
        let many = format!("ا {}", "{a} ".repeat(200));
        assert_eq!(label(&many).matches("{a}").count(), 200);
    }
}
