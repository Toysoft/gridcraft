//! Shared-string rich-text runs: splitting an `<si>` into text + run parts, reading `<rPr>`
//! deltas and assembling `CellRuns` under the caps (issue #774).

use gridcraft_model::{Color, MAX_RUNS_PER_CELL, RunFont};

use crate::runs::{Si, si_cell_runs};
use crate::styles::read_run_font;
use crate::tables::INDEXED;
use crate::xml::{El, parse, si_parts};

fn si(xml: &str) -> El {
    parse(xml.as_bytes()).unwrap()
}

fn none_font() -> RunFont {
    RunFont { name: None, size: None, bold: None, italic: None, underline: None, strike: None, color: None, vert: None }
}

#[test]
fn si_parts_plain() {
    let el = si("<si><t>plain text</t></si>");
    let (text, parts) = si_parts(&el);
    assert_eq!(text, "plain text");
    assert!(parts.is_empty());

    let mut budget = 1 << 20;
    let (t, runs) = si_cell_runs(&el, &INDEXED, &mut budget);
    assert_eq!(&*t, "plain text");
    assert!(runs.is_none());
    assert_eq!(budget, 1 << 20);
}

/// The #774 repro shape: three runs over "this is black, this is red, this is black" with the
/// three colour forms.
#[test]
fn si_parts_repro_colors() {
    let el = si(concat!(
        r#"<si><r><rPr><color theme="1"/></rPr><t>this is black,</t></r>"#,
        r#"<r><rPr><color indexed="2"/></rPr><t xml:space="preserve"> this is red,</t></r>"#,
        r#"<r><rPr><color rgb="FF00B050"/></rPr><t xml:space="preserve"> this is black</t></r></si>"#,
    ));
    let (text, parts) = si_parts(&el);
    assert_eq!(text, "this is black, this is red, this is black");
    assert_eq!(parts.len(), 3);
    assert_eq!((parts[0].start, parts[0].len), (0, 14));
    assert_eq!((parts[1].start, parts[1].len), (14, 13));
    assert_eq!((parts[2].start, parts[2].len), (27, 14));
    assert!(parts.iter().all(|p| p.rpr.is_some()));

    let mut budget = 1 << 20;
    let Si { text, runs } = Si::parse(&el, &INDEXED, &mut budget);
    assert_eq!(&*text, "this is black, this is red, this is black");
    let cr = runs.expect("runs parse");
    assert!(cr.valid_for(&text));
    assert_eq!(cr.run_text(&text, 0), Some("this is black,"));
    assert_eq!(cr.run_text(&text, 1), Some(" this is red,"));
    assert_eq!(cr.run_text(&text, 2), Some(" this is black"));
    assert_eq!(cr.runs[0].font.color, Some(Color::Auto));
    assert_eq!(cr.runs[1].font.color, Some(Color::Rgb(0xFF0000)));
    assert_eq!(cr.runs[2].font.color, Some(Color::Rgb(0x00B050)));
    assert_eq!(budget, (1 << 20) - 3);
}

#[test]
fn preserves_leading_space_offsets() {
    let el = si(concat!(
        r#"<si><r><rPr><b/></rPr><t>this is black,</t></r>"#,
        r#"<r><rPr><color rgb="FFFF0000"/></rPr><t xml:space="preserve"> this is red,</t></r></si>"#,
    ));
    let (text, parts) = si_parts(&el);
    assert_eq!(text, "this is black, this is red,");
    // The second run keeps its leading space: byte offsets, total == text length.
    assert_eq!((parts[1].start, parts[1].len), (14, 13));
    let total: usize = parts.iter().map(|p| p.len as usize).sum();
    assert_eq!(total, text.len());

    let mut budget = 1 << 20;
    let (_, runs) = si_cell_runs(&el, &INDEXED, &mut budget);
    assert!(runs.expect("valid").valid_for(text.as_str()));
}

#[test]
fn rph_ignored() {
    let el = si(concat!(
        r#"<si><r><rPr><b/></rPr><t>ab</t></r>"#,
        r#"<rPh sb="0" eb="2"><t>phonetic</t></rPh>"#,
        r#"<r><rPr><i/></rPr><t>cd</t></r></si>"#,
    ));
    let (text, parts) = si_parts(&el);
    assert_eq!(text, "abcd");
    assert_eq!(parts.len(), 2);
    assert_eq!((parts[1].start, parts[1].len), (2, 2));
}

#[test]
fn over_cell_cap_drops_runs() {
    let mut xml = String::from("<si>");
    for _ in 0..=MAX_RUNS_PER_CELL {
        xml.push_str("<r><rPr><b/></rPr><t>x</t></r>");
    }
    xml.push_str("</si>");
    let el = si(&xml);
    assert_eq!(si_parts(&el).1.len(), MAX_RUNS_PER_CELL + 1);

    let mut budget = 1 << 20;
    let (text, runs) = si_cell_runs(&el, &INDEXED, &mut budget);
    assert_eq!(text.len(), MAX_RUNS_PER_CELL + 1);
    assert!(runs.is_none());
    assert_eq!(budget, 1 << 20);
}

#[test]
fn over_workbook_budget_drops() {
    let el = si(r#"<si><r><rPr><b/></rPr><t>ab</t></r><r><rPr><i/></rPr><t>cd</t></r></si>"#);
    let mut budget = 0u64;
    let (text, runs) = si_cell_runs(&el, &INDEXED, &mut budget);
    assert_eq!(&*text, "abcd");
    assert!(runs.is_none());
    assert_eq!(budget, 0);
    // Too small by one: still dropped, budget untouched.
    let mut budget = 1u64;
    let (_, runs) = si_cell_runs(&el, &INDEXED, &mut budget);
    assert!(runs.is_none());
    assert_eq!(budget, 1);
}

#[test]
fn runs_without_rpr_dropped() {
    // Runs that carry no formatting delta hold no information: text only.
    let el = si(r#"<si><r><t>ab</t></r><r><t>cd</t></r></si>"#);
    let mut budget = 1 << 20;
    let (text, runs) = si_cell_runs(&el, &INDEXED, &mut budget);
    assert_eq!(&*text, "abcd");
    assert!(runs.is_none());
    assert_eq!(budget, 1 << 20);
}

#[test]
fn run_font_delta_fields() {
    // Absent properties stay None (inherit); present ones override.
    let rpr = si(r#"<rPr><rFont val="Georgia"/><sz val="12"/><b/><strike val="0"/><u val="double"/><vertAlign val="superscript"/></rPr>"#);
    let f = read_run_font(&rpr, &INDEXED);
    assert_eq!(f.name.as_deref(), Some("Georgia"));
    assert_eq!(f.size, Some(12.0));
    assert_eq!(f.bold, Some(true));
    assert_eq!(f.italic, None);
    assert_eq!(f.strike, Some(false));
    assert_eq!(f.underline, Some(gridcraft_model::Underline::Double));
    assert_eq!(f.vert, Some(gridcraft_model::VertAlign::Superscript));
    assert_eq!(f.color, None);

    let empty = si("<rPr/>");
    assert_eq!(read_run_font(&empty, &INDEXED), none_font());

    // Both automatic forms: auto="1" and theme 1 (text 1) with no tint.
    let auto = si(r#"<rPr><color auto="1"/></rPr>"#);
    assert_eq!(read_run_font(&auto, &INDEXED).color, Some(Color::Auto));
    let theme1 = si(r#"<rPr><color theme="1"/></rPr>"#);
    assert_eq!(read_run_font(&theme1, &INDEXED).color, Some(Color::Auto));

    // Size clamps to Excel's 1..=409 pt; theme 1 with a tint is a real theme colour.
    let clamped = si(r#"<rPr><sz val="9999"/><color theme="1" tint="-0.5"/></rPr>"#);
    let f = read_run_font(&clamped, &INDEXED);
    assert_eq!(f.size, Some(409.0));
    assert_eq!(f.color, Some(Color::Theme(1, -500)));

    // A run without <rPr> inherits everything.
    let mut budget = 1 << 20;
    let (_, runs) = si_cell_runs(&si(r#"<si><r><rPr><b/></rPr><t>a</t></r><r><t>bc</t></r></si>"#), &INDEXED, &mut budget);
    let cr = runs.expect("runs parse");
    assert_eq!(cr.runs[1].font, none_font());
    assert_eq!(cr.runs[0].font.bold, Some(true));
}
