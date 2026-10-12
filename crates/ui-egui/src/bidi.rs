//! Right-to-left text in cells. egui shapes text (harfrust), so Arabic letters join, but it does
//! no bidirectional layout: it cuts a line into runs by font face and shapes each in a direction
//! guessed from its script, then places the runs left to right in stored order. So numbers inside
//! Arabic were reversed with it ("الرصيد في 28 فبراير" showed "82"), and Arabic in a Latin line,
//! or Arabic words split by a space from another font, came out in the wrong order.
//!
//! [`layout_job`] does the Unicode Bidirectional Algorithm (UAX #9) first. Each line is cut into
//! pieces given to egui as separate sections in left-to-right display order: left-to-right runs
//! as they are, right-to-left runs as their chunks of Arabic or Hebrew letters (kept in reading
//! order, which egui's shaper turns and joins) in reverse, with the spaces and punctuation between
//! them reversed and mirrored. The paragraph direction comes from the first strong character, like
//! Excel's "Context" reading order.

use egui::text::{ByteIndex, LayoutJob, LayoutSection, TextFormat};
use egui::{Color32, FontId};
use unicode_bidi::BidiInfo;

/// Whether `c` belongs to a right-to-left script block (Hebrew, Arabic, Syriac, Thaana, N'Ko, …).
fn is_rtl_char(c: char) -> bool {
    matches!(c as u32,
        0x0590..=0x08FF | 0xFB1D..=0xFDFF | 0xFE70..=0xFEFF | 0x10800..=0x10FFF | 0x1E800..=0x1EFFF)
}

/// Whether `s` holds any right-to-left character; text without one is laid out as before.
pub fn has_rtl(s: &str) -> bool {
    s.chars().any(is_rtl_char)
}

/// A no-wrap egui layout of `text` (lines split on `\n`) in display order.
pub fn layout_job(text: &str, font: FontId, color: Color32) -> LayoutJob {
    let format = TextFormat::simple(font, color);
    let mut job = LayoutJob { wrap: egui::text::TextWrapping::no_max_width(), ..Default::default() };
    for (i, line) in text.split('\n').enumerate() {
        if i > 0 {
            push_section(&mut job, "\n", &format);
        }
        for piece in visual_pieces(line) {
            push_section(&mut job, &piece, &format);
        }
    }
    job
}

/// Add `piece` as its own section. `LayoutJob::append` would merge it into the previous section
/// (same format), and the shaper would then turn the merged run around as a whole.
fn push_section(job: &mut LayoutJob, piece: &str, format: &TextFormat) {
    let start = ByteIndex(job.text.len());
    job.text.push_str(piece);
    job.sections.push(LayoutSection { leading_space: 0.0, byte_range: start..ByteIndex(job.text.len()), format: format.clone() });
}

/// Break `text` into lines no wider than `max_w` (by `width`) at spaces, in reading order. Wrapping
/// has to happen before reordering: a right-to-left paragraph starts on the right of its first line.
pub fn wrap(text: &str, max_w: f32, mut width: impl FnMut(&str) -> f32) -> String {
    let mut out: Vec<String> = Vec::new();
    for para in text.split('\n') {
        let mut line = String::new();
        for word in para.split(' ') {
            let candidate = if line.is_empty() { word.to_string() } else { format!("{line} {word}") };
            if !line.is_empty() && width(&candidate) > max_w {
                out.push(std::mem::replace(&mut line, word.to_string()));
            } else {
                line = candidate;
            }
        }
        out.push(line);
    }
    out.join("\n")
}

/// One line (no `\n`) as pieces in left-to-right display order, each to be shaped on its own.
fn visual_pieces(line: &str) -> Vec<String> {
    if !has_rtl(line) {
        return vec![line.to_string()];
    }
    let info = BidiInfo::new(line, None);
    let Some(para) = info.paragraphs.first() else {
        return vec![line.to_string()];
    };
    let (levels, runs) = info.visual_runs(para, para.range.clone());
    let mut out = Vec::new();
    for run in runs {
        let Some(s) = line.get(run.clone()) else { continue };
        if levels.get(run.start).is_some_and(|l| l.is_rtl()) {
            for (rtl_script, chunk) in chunks(s).into_iter().rev() {
                if rtl_script {
                    out.push(chunk);
                } else {
                    out.push(chunk.chars().rev().map(mirror).collect());
                }
            }
        } else {
            // A left-to-right run inside right-to-left text can still hold Arabic-Indic digits,
            // which the shaper would turn around as Arabic: give each one its own piece.
            let mut piece = String::new();
            for c in s.chars() {
                if is_rtl_char(c) {
                    out.extend((!piece.is_empty()).then(|| std::mem::take(&mut piece)));
                    out.push(c.to_string());
                } else {
                    piece.push(c);
                }
            }
            out.extend((!piece.is_empty()).then_some(piece));
        }
    }
    out
}

/// Split a right-to-left run into chunks of right-to-left script (with any combining marks that
/// ride on them) and everything else, in reading order; `true` marks right-to-left script.
fn chunks(s: &str) -> Vec<(bool, String)> {
    let mut out: Vec<(bool, String)> = Vec::new();
    for c in s.chars() {
        let rtl = is_rtl_char(c) || (is_mark(c) && out.last().is_some_and(|(r, _)| *r));
        match out.last_mut() {
            Some((r, chunk)) if *r == rtl => chunk.push(c),
            _ => out.push((rtl, c.to_string())),
        }
    }
    out
}

/// General combining marks outside the right-to-left blocks (those are inside them already).
fn is_mark(c: char) -> bool {
    matches!(c as u32, 0x0300..=0x036F | 0x1AB0..=0x1AFF | 0x1DC0..=0x1DFF | 0x20D0..=0x20FF | 0xFE20..=0xFE2F)
}

/// Mirrored glyph for paired punctuation in a right-to-left run (UAX #9 rule L4).
fn mirror(c: char) -> char {
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
        '‹' => '›',
        '›' => '‹',
        _ => c,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn latin_text_is_one_piece() {
        assert_eq!(visual_pieces("Sales 2026 (net)"), ["Sales 2026 (net)"]);
    }

    #[test]
    fn numbers_in_arabic_keep_their_digit_order() {
        // An Arabic paragraph: words right to left, the number left to right between them.
        assert_eq!(visual_pieces("الرصيد في 28 فبراير"), ["فبراير", " ", "28", " ", "في", " ", "الرصيد"]);
    }

    #[test]
    fn arabic_in_a_latin_line_stays_after_the_latin() {
        assert_eq!(visual_pieces("10102-customer-1111 - عبدالسلام"), ["10102-customer-1111 - ", "عبدالسلام"]);
        // Two Arabic words: the second goes to the left of the first.
        assert_eq!(visual_pieces("Cafe مقهى فليم"), ["Cafe ", "فليم", " ", "مقهى"]);
    }

    #[test]
    fn harakat_stay_in_their_word() {
        assert_eq!(visual_pieces("مُحَمَّد علي"), ["علي", " ", "مُحَمَّد"]);
    }

    #[test]
    fn brackets_mirror_in_rtl_runs() {
        // "(مرحبا)" in an Arabic paragraph: the closing bracket shows on the left as "(".
        assert_eq!(visual_pieces("مرحبا (عالم)"), ["(", "عالم", ") ", "مرحبا"]);
    }

    #[test]
    fn arabic_indic_digits_are_shaped_one_by_one() {
        assert_eq!(visual_pieces("في ٢٨"), ["٢", "٨", " ", "في"]);
    }

    #[test]
    fn wrapping_happens_in_reading_order() {
        // Every character counts 1 wide; lines hold 7.
        let w = wrap("الف باء تاء", 7.0, |s| s.chars().count() as f32);
        assert_eq!(w, "الف باء\nتاء");
    }

    /// What egui really draws, left to right on screen, with each Arabic word as one `◌` (which
    /// glyphs a word turns into depends on the font's ligatures; where it lands does not).
    fn on_screen(fonts: egui::FontDefinitions, family: egui::FontFamily, text: &str) -> String {
        let ctx = egui::Context::default();
        ctx.set_fonts(fonts);
        let mut out = ctx.run_ui(egui::RawInput::default(), |_ui| {});
        out.textures_delta.clear(); // nothing consumes the atlas in a headless test
        let galley = ctx.fonts_mut(|f| f.layout_job(layout_job(text, FontId::new(14.0, family), Color32::BLACK)));
        let mut glyphs: Vec<(f32, char)> = galley
            .rows
            .iter()
            .flat_map(|r| r.glyphs.iter())
            .filter(|g| g.advance_width > 0.0) // skip zero-width cluster continuations
            .map(|g| (g.pos.x, g.chr))
            .collect();
        glyphs.sort_by(|a, b| a.0.total_cmp(&b.0));
        let mut screen = String::new();
        for (_, c) in glyphs {
            let letter = is_rtl_char(c) && !matches!(c as u32, 0x0660..=0x0669 | 0x06F0..=0x06F9);
            if !(letter && screen.ends_with('◌')) {
                screen.push(if letter { '◌' } else { c });
            }
        }
        screen
    }

    #[test]
    fn egui_places_mixed_text_in_reading_order() {
        // Without bidi, egui drew "في 28" as "◌ 28", which reads right to left as "28 في".
        let cases = [
            ("في 28", "28 ◌"),
            ("الرصيد في 28 فبراير", "◌ 28 ◌ ◌"),
            ("فاتورة 2026 رقم ١٢٣", "١٢٣ ◌ 2026 ◌"),
            ("10102-customer-1111 - عبدالرحمن", "10102-customer-1111 - ◌"),
            ("Hello (مرحبا) world", "Hello (◌) world"),
            ("مرحبا (عالم)", "(◌) ◌"),
        ];
        // egui's own fonts (no Arabic face: one face, so one shaping run per piece) and the app's
        // cell fonts (Latin and Arabic in different faces where the machine has an Arabic font).
        let setups = [
            (egui::FontDefinitions::default(), egui::FontFamily::Proportional),
            (crate::theme::font_definitions(), egui::FontFamily::Name(crate::theme::CELL.into())),
        ];
        for (fonts, family) in setups {
            for (text, screen) in cases {
                assert_eq!(on_screen(fonts.clone(), family.clone(), text), screen, "{text:?} ({family:?})");
            }
        }
    }

    #[test]
    fn hostile_input_does_not_panic() {
        for s in ["", "\u{202E}", "\u{064E}", "( ) [", "\u{200F}\u{200E}x\u{0627}", "ا\n\nب"] {
            let _ = layout_job(s, FontId::default(), Color32::BLACK);
            let _ = wrap(s, 0.0, |_| 1.0);
        }
    }
}
