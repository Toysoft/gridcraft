//! ODF styles → GridCraft styles: cell styles (font, fill, borders, alignment, protection and
//! data style), column widths, row heights and sheet (table) properties.
//! Format reference: ODF 1.3 part 3, sections 16 (styles) and 20 (formatting properties).

use std::collections::HashMap;

use gridcraft_model::{BorderLine, BorderStyle, Color, HAlign, NumFmt, Style, Underline, VAlign, VertAlign};
use quick_xml::NsReader;
use quick_xml::XmlVersion;
use quick_xml::events::{BytesStart, Event};

use super::numfmt::{self, DataStyle, Kind};
use super::{attr, is_ns, xml_error};
use crate::IoError;

pub(super) const STYLE: &[u8] = b"urn:oasis:names:tc:opendocument:xmlns:style:1.0";
pub(super) const FO: &[u8] = b"urn:oasis:names:tc:opendocument:xmlns:xsl-fo-compatible:1.0";
pub(super) const NUMBER: &[u8] = b"urn:oasis:names:tc:opendocument:xmlns:datastyle:1.0";
pub(super) const SVG: &[u8] = b"urn:oasis:names:tc:opendocument:xmlns:svg-compatible:1.0";
pub(super) const TABLE_OOO: &[u8] = b"http://openoffice.org/2009/table";
use super::{OFFICE, TABLE};

/// Styles beyond this many are ignored (a hostile file can't make the tables grow without bound).
const MAX_STYLES: usize = 100_000;
/// Parent chains longer than this are cut (they may also be cyclic).
const MAX_PARENT_DEPTH: usize = 32;

/// A cell style's own properties; `None` = inherited from the parent.
#[derive(Clone, Debug, Default)]
pub(super) struct CellProps {
    parent: Option<String>,
    data_style: Option<String>,
    font: Option<String>,
    size: Option<f32>,
    bold: Option<bool>,
    italic: Option<bool>,
    underline: Option<Underline>,
    strike: Option<bool>,
    color: Option<Color>,
    vert: Option<VertAlign>,
    fill: Option<Color>,
    left: Option<BorderLine>,
    right: Option<BorderLine>,
    top: Option<BorderLine>,
    bottom: Option<BorderLine>,
    diag_down: Option<BorderLine>,
    diag_up: Option<BorderLine>,
    h: Option<HAlign>,
    v: Option<VAlign>,
    wrap: Option<bool>,
    shrink: Option<bool>,
    rotation: Option<i16>,
    indent: Option<u8>,
    locked: Option<bool>,
    hidden: Option<bool>,
}

macro_rules! inherit {
    ($dst:ident, $src:ident, $($f:ident),*) => { $( if $dst.$f.is_none() { $dst.$f = $src.$f.clone(); } )* };
}

impl CellProps {
    /// Fills the properties this style doesn't set from `parent`.
    fn inherit(&mut self, parent: &CellProps) {
        let me = self;
        inherit!(
            me, parent, data_style, font, size, bold, italic, underline, strike, color, vert, fill, left, right, top, bottom, diag_down, diag_up, h,
            v, wrap, shrink, rotation, indent, locked, hidden
        );
    }

    /// `base` changed by these properties.
    fn apply(&self, base: &Style, num_fmt: Option<String>) -> Style {
        let mut s = base.clone();
        if let Some(f) = &self.font {
            s.font.name = f.clone();
        }
        if let Some(v) = self.size {
            s.font.size = v;
        }
        if let Some(v) = self.bold {
            s.font.bold = v;
        }
        if let Some(v) = self.italic {
            s.font.italic = v;
        }
        if let Some(v) = self.underline {
            s.font.underline = v;
        }
        if let Some(v) = self.strike {
            s.font.strike = v;
        }
        if let Some(v) = self.color {
            s.font.color = v;
        }
        if let Some(v) = self.vert {
            s.font.vert = v;
        }
        if let Some(c) = self.fill {
            s.fill = if c == Color::Auto { Default::default() } else { gridcraft_model::Fill::solid(c) };
        }
        for (slot, value) in [
            (&mut s.borders.left, self.left),
            (&mut s.borders.right, self.right),
            (&mut s.borders.top, self.top),
            (&mut s.borders.bottom, self.bottom),
            (&mut s.borders.diag_down, self.diag_down),
            (&mut s.borders.diag_up, self.diag_up),
        ] {
            if let Some(v) = value {
                *slot = v;
            }
        }
        if let Some(v) = self.h {
            s.align.h = v;
        }
        if let Some(v) = self.v {
            s.align.v = v;
        }
        if let Some(v) = self.wrap {
            s.align.wrap = v;
        }
        if let Some(v) = self.shrink {
            s.align.shrink = v;
        }
        if let Some(v) = self.rotation {
            s.align.rotation = v;
        }
        if let Some(v) = self.indent {
            s.align.indent = v;
        }
        if let Some(v) = self.locked {
            s.protection.locked = v;
        }
        if let Some(v) = self.hidden {
            s.protection.hidden = v;
        }
        if let Some(code) = num_fmt {
            s.num_fmt = NumFmt::new(&code);
        }
        s
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub(super) struct RowProps {
    /// Height in px.
    pub height: Option<f32>,
    pub optimal: bool,
}

#[derive(Clone, Debug, Default)]
pub(super) struct TableProps {
    pub hidden: bool,
    pub tab_color: Option<Color>,
}

/// Every style of a document, by name.
#[derive(Default)]
pub(super) struct Styles {
    cells: HashMap<String, CellProps>,
    default_cell: CellProps,
    columns: HashMap<String, f32>,
    rows: HashMap<String, RowProps>,
    tables: HashMap<String, TableProps>,
    data: HashMap<String, DataStyle>,
    fonts: HashMap<String, String>,
    count: usize,
}

impl Styles {
    /// Reads the styles of `styles.xml` or of `content.xml` (up to its body).
    pub fn collect(&mut self, bytes: &[u8]) -> Result<(), IoError> {
        let mut reader = NsReader::from_reader(bytes);
        let mut depth = 0usize;
        // The style being read and the depth it opened at.
        let mut current: Option<(Current, usize)> = None;
        let mut text: Option<String> = None;
        loop {
            let event = reader.read_event().map_err(xml_error)?;
            let (e, empty) = match &event {
                Event::Start(e) => (Some(e.clone()), false),
                Event::Empty(e) => (Some(e.clone()), true),
                Event::End(_) => {
                    depth = depth.checked_sub(1).ok_or_else(|| xml_error("unexpected closing element"))?;
                    if let Some(t) = text.take()
                        && let Some((Current::Data(_, d), _)) = current.as_mut()
                    {
                        let lit = numfmt::literal(&t, d.kind);
                        d.code.push_str(&lit);
                    }
                    if current.as_ref().is_some_and(|(_, d)| *d == depth)
                        && let Some((cur, _)) = current.take()
                    {
                        self.finish(cur);
                    }
                    continue;
                }
                Event::Text(t) => {
                    if let Some(buf) = text.as_mut() {
                        let s = t.xml_content(XmlVersion::Explicit1_0).map_err(xml_error)?;
                        if buf.len().saturating_add(s.len()) <= 4096 {
                            buf.push_str(&s);
                        }
                    }
                    continue;
                }
                Event::GeneralRef(r) => {
                    if let Some(buf) = text.as_mut()
                        && let Ok(Some(ch)) = r.resolve_char_ref()
                    {
                        buf.push(ch);
                    } else if let Some(buf) = text.as_mut() {
                        buf.push(match r.as_ref() {
                            b"amp" => '&',
                            b"lt" => '<',
                            b"gt" => '>',
                            b"apos" => '\'',
                            _ => '"',
                        });
                    }
                    continue;
                }
                Event::DocType(_) => return Err(xml_error("DOCTYPE is not supported")),
                Event::Eof => break,
                _ => continue,
            };
            let Some(e) = e else { continue };
            let (ns, name) = reader.resolver_mut().resolve_element(e.name());
            let local = name.as_ref().to_vec();
            let (in_office, in_style, in_number) = (is_ns(ns.clone(), OFFICE), is_ns(ns.clone(), STYLE), is_ns(ns, NUMBER));
            // Styles never live in the body: stop there.
            if in_office && local == b"body" {
                break;
            }
            if in_style && (local == b"style" || local == b"default-style") && current.is_none() {
                current = self.start_style(&mut reader, &e, local == b"default-style")?.map(|c| (c, depth));
            } else if in_style && local == b"font-face" {
                if let Some(n) = attr(&mut reader, &e, STYLE, b"name")? {
                    let family = attr(&mut reader, &e, SVG, b"font-family")?.unwrap_or_else(|| n.clone());
                    if self.fonts.len() < MAX_STYLES {
                        self.fonts.insert(n, family.trim_matches(['\'', '"']).to_string());
                    }
                }
            } else if in_number
                && let Some(kind) = Kind::from_element(&local)
                && current.is_none()
            {
                if let Some(n) = attr(&mut reader, &e, STYLE, b"name")? {
                    let mut d = DataStyle { kind, ..Default::default() };
                    d.elapsed = attr(&mut reader, &e, NUMBER, b"truncate-on-overflow")?.as_deref() == Some("false");
                    current = Some((Current::Data(n, d), depth));
                }
            } else if let Some((cur, _)) = current.as_mut() {
                self.property(&mut reader, &e, &ns_of(in_style, in_number), &local, cur, &mut text)?;
            }
            if !empty {
                depth += 1;
                if depth > crate::xml::MAX_DEPTH {
                    return Err(xml_error("XML nested too deeply"));
                }
            } else if current.as_ref().is_some_and(|(_, d)| *d == depth)
                && let Some((cur, _)) = current.take()
            {
                self.finish(cur);
            }
        }
        Ok(())
    }

    fn start_style(&mut self, reader: &mut NsReader<&[u8]>, e: &BytesStart<'_>, default: bool) -> Result<Option<Current>, IoError> {
        let family = attr(reader, e, STYLE, b"family")?.unwrap_or_default();
        let name = attr(reader, e, STYLE, b"name")?;
        let name = match (default, name) {
            (true, _) => String::new(),
            (false, Some(n)) => n,
            (false, None) => return Ok(None),
        };
        Ok(Some(match family.as_str() {
            "table-cell" => {
                let props = CellProps {
                    parent: attr(reader, e, STYLE, b"parent-style-name")?,
                    data_style: attr(reader, e, STYLE, b"data-style-name")?,
                    ..Default::default()
                };
                Current::Cell(name, default, Box::new(props))
            }
            "table-column" => Current::Column(name, None),
            "table-row" => Current::Row(name, RowProps::default()),
            "table" => Current::Table(name, TableProps::default()),
            _ => return Ok(None),
        }))
    }

    fn property(
        &mut self,
        reader: &mut NsReader<&[u8]>,
        e: &BytesStart<'_>,
        ns: &Ns,
        local: &[u8],
        cur: &mut Current,
        text: &mut Option<String>,
    ) -> Result<(), IoError> {
        match cur {
            Current::Cell(_, _, p) if ns.style => match local {
                b"text-properties" => self.text_props(reader, e, p)?,
                b"table-cell-properties" => cell_props(reader, e, p)?,
                b"paragraph-properties" => {
                    if let Some(a) = attr(reader, e, FO, b"text-align")? {
                        p.h = Some(match a.as_str() {
                            "center" => HAlign::Center,
                            "end" | "right" => HAlign::Right,
                            "justify" => HAlign::Justify,
                            _ => HAlign::Left,
                        });
                    }
                    if let Some(m) = attr(reader, e, FO, b"margin-left")?.as_deref().and_then(length_px) {
                        // One indent level is about 10 pt (13 px).
                        p.indent = Some((m / 13.0).round().clamp(0.0, 250.0) as u8);
                    }
                }
                _ => {}
            },
            Current::Column(_, w) if ns.style && local == b"table-column-properties" => {
                *w = attr(reader, e, STYLE, b"column-width")?.as_deref().and_then(length_px).map(f32::round);
            }
            Current::Row(_, r) if ns.style && local == b"table-row-properties" => {
                r.height = attr(reader, e, STYLE, b"row-height")?.as_deref().and_then(length_px).map(f32::round);
                r.optimal = attr(reader, e, STYLE, b"use-optimal-row-height")?.as_deref() == Some("true");
            }
            Current::Table(_, t) if ns.style && local == b"table-properties" => {
                t.hidden = attr(reader, e, TABLE, b"display")?.as_deref() == Some("false");
                t.tab_color = attr(reader, e, TABLE_OOO, b"tab-color")?.as_deref().and_then(color);
            }
            Current::Data(_, d) => {
                if ns.style && local == b"text-properties" {
                    d.color = attr(reader, e, FO, b"color")?;
                    return Ok(());
                }
                if ns.style && local == b"map" {
                    if let (Some(c), Some(n)) = (attr(reader, e, STYLE, b"condition")?, attr(reader, e, STYLE, b"apply-style-name")?) {
                        d.maps.push((c, n));
                    }
                    return Ok(());
                }
                if !ns.number {
                    return Ok(());
                }
                let num = |reader: &mut NsReader<&[u8]>, key: &[u8]| -> Result<Option<u32>, IoError> {
                    Ok(attr(reader, e, NUMBER, key)?.and_then(|s| s.parse::<u32>().ok()))
                };
                let long = attr(reader, e, NUMBER, b"style")?.as_deref() == Some("long");
                let part = match local {
                    b"number" => Some(numfmt::number_part(
                        num(reader, b"decimal-places")?,
                        num(reader, b"min-decimal-places")?,
                        num(reader, b"min-integer-digits")?,
                        attr(reader, e, NUMBER, b"grouping")?.as_deref() == Some("true"),
                    )),
                    b"scientific-number" => Some(numfmt::scientific_part(
                        num(reader, b"decimal-places")?,
                        num(reader, b"min-integer-digits")?,
                        num(reader, b"min-exponent-digits")?,
                    )),
                    b"fraction" => Some(numfmt::fraction_part(
                        num(reader, b"min-integer-digits")?,
                        num(reader, b"min-numerator-digits")?,
                        num(reader, b"min-denominator-digits")?,
                        num(reader, b"denominator-value")?,
                    )),
                    b"year" => Some(if long { "yyyy" } else { "yy" }.into()),
                    b"month" => {
                        let textual = attr(reader, e, NUMBER, b"textual")?.as_deref() == Some("true");
                        Some(
                            match (textual, long) {
                                (true, true) => "mmmm",
                                (true, false) => "mmm",
                                (false, true) => "mm",
                                (false, false) => "m",
                            }
                            .into(),
                        )
                    }
                    b"day" => Some(if long { "dd" } else { "d" }.into()),
                    b"day-of-week" => Some(if long { "dddd" } else { "ddd" }.into()),
                    b"hours" | b"minutes" => {
                        let unit = if local == b"hours" { "h" } else { "m" };
                        let mut s = if long { unit.repeat(2) } else { unit.to_string() };
                        if d.elapsed && !d.code.contains('[') {
                            s = format!("[{s}]");
                        }
                        Some(s)
                    }
                    b"seconds" => {
                        let mut s = if long { "ss".to_string() } else { "s".to_string() };
                        let dec = num(reader, b"decimal-places")?.unwrap_or(0).min(9);
                        if dec > 0 {
                            s.push('.');
                            s.push_str(&"0".repeat(dec as usize));
                        }
                        Some(s)
                    }
                    b"am-pm" => Some("AM/PM".into()),
                    b"text-content" => Some("@".into()),
                    b"text" | b"currency-symbol" => {
                        *text = Some(String::new());
                        None
                    }
                    _ => None,
                };
                if let Some(p) = part {
                    // General inside a longer code is spelled as such.
                    d.code.push_str(&p);
                }
            }
            _ => {}
        }
        Ok(())
    }

    fn text_props(&self, reader: &mut NsReader<&[u8]>, e: &BytesStart<'_>, p: &mut CellProps) -> Result<(), IoError> {
        if let Some(name) = attr(reader, e, STYLE, b"font-name")? {
            // Resolved through the font-face declarations once every part is read.
            p.font = Some(name);
        } else if let Some(family) = attr(reader, e, FO, b"font-family")? {
            // A family, not a font-face name: unlikely to collide with a declaration.
            p.font = Some(family.trim_matches(['\'', '"']).to_string());
        }
        if let Some(size) = attr(reader, e, FO, b"font-size")?.as_deref().and_then(length_pt) {
            p.size = Some(size.clamp(1.0, 409.0));
        }
        if let Some(w) = attr(reader, e, FO, b"font-weight")? {
            p.bold = Some(w == "bold" || w.parse::<u32>().is_ok_and(|n| n >= 600));
        }
        if let Some(s) = attr(reader, e, FO, b"font-style")? {
            p.italic = Some(s == "italic" || s == "oblique");
        }
        if let Some(u) = attr(reader, e, STYLE, b"text-underline-style")? {
            let double = attr(reader, e, STYLE, b"text-underline-type")?.as_deref() == Some("double");
            p.underline = Some(match (u.as_str(), double) {
                ("none", _) => Underline::None,
                (_, true) => Underline::Double,
                _ => Underline::Single,
            });
        }
        if let Some(s) = attr(reader, e, STYLE, b"text-line-through-style")? {
            p.strike = Some(s != "none");
        }
        if let Some(c) = attr(reader, e, FO, b"color")? {
            p.color = color(&c);
        }
        if let Some(pos) = attr(reader, e, STYLE, b"text-position")? {
            let first = pos.split_whitespace().next().unwrap_or("");
            let n = first.trim_end_matches('%').parse::<f32>().unwrap_or(0.0);
            p.vert = Some(if first == "super" || n > 0.0 {
                VertAlign::Superscript
            } else if first == "sub" || n < 0.0 {
                VertAlign::Subscript
            } else {
                VertAlign::Baseline
            });
        }
        Ok(())
    }

    fn finish(&mut self, cur: Current) {
        if self.count >= MAX_STYLES {
            return;
        }
        self.count += 1;
        match cur {
            Current::Cell(_, true, p) => self.default_cell = *p,
            Current::Cell(n, false, p) => {
                self.cells.insert(n, *p);
            }
            Current::Column(n, w) => {
                if let Some(w) = w {
                    self.columns.insert(n, w);
                }
            }
            Current::Row(n, r) => {
                self.rows.insert(n, r);
            }
            Current::Table(n, t) => {
                self.tables.insert(n, t);
            }
            Current::Data(n, d) => {
                self.data.insert(n, d);
            }
        }
    }

    /// The cell style `name` with its parents (and the default style) folded in.
    fn resolved(&self, name: Option<&str>) -> CellProps {
        let mut out = CellProps::default();
        let mut next = name.map(str::to_string);
        for _ in 0..MAX_PARENT_DEPTH {
            let Some(n) = next.take() else { break };
            let Some(p) = self.cells.get(&n) else { break };
            out.inherit(p);
            next = p.parent.clone();
        }
        out.inherit(&self.default_cell);
        if let Some(f) = out.font.take() {
            out.font = Some(self.fonts.get(&f).cloned().unwrap_or(f));
        }
        out
    }

    /// The workbook's Normal style: GridCraft's default changed by the document's default cell
    /// style and its `Default` named style.
    pub fn base(&self) -> Style {
        let p = self.resolved(self.cells.contains_key("Default").then_some("Default"));
        p.apply(&Style::default(), p.data_style.as_deref().and_then(|d| self.number_format(d)))
    }

    /// The style a cell named `name` gets.
    pub fn cell_style(&self, name: &str, base: &Style) -> Style {
        let p = self.resolved(Some(name));
        let mut s = p.apply(base, p.data_style.as_deref().and_then(|d| self.number_format(d)));
        if p.data_style.is_none() {
            s.num_fmt = base.num_fmt.clone();
        }
        s
    }

    fn number_format(&self, name: &str) -> Option<String> {
        numfmt::code_for(name, &|n| self.data.get(n).cloned())
    }

    pub fn column_width(&self, name: &str) -> Option<f32> {
        self.columns.get(name).copied()
    }

    pub fn row(&self, name: &str) -> Option<RowProps> {
        self.rows.get(name).copied()
    }

    pub fn table(&self, name: &str) -> Option<&TableProps> {
        self.tables.get(name)
    }
}

enum Current {
    /// (name, is the default style, properties)
    Cell(String, bool, Box<CellProps>),
    Column(String, Option<f32>),
    Row(String, RowProps),
    Table(String, TableProps),
    Data(String, DataStyle),
}

struct Ns {
    style: bool,
    number: bool,
}

fn ns_of(style: bool, number: bool) -> Ns {
    Ns { style, number }
}

fn cell_props(reader: &mut NsReader<&[u8]>, e: &BytesStart<'_>, p: &mut CellProps) -> Result<(), IoError> {
    if let Some(bg) = attr(reader, e, FO, b"background-color")? {
        p.fill = Some(if bg == "transparent" { Color::Auto } else { color(&bg).unwrap_or(Color::Auto) });
    }
    if let Some(all) = attr(reader, e, FO, b"border")?.as_deref().map(border) {
        p.left = Some(all);
        p.right = Some(all);
        p.top = Some(all);
        p.bottom = Some(all);
    }
    for (key, slot) in
        [(&b"border-left"[..], &mut p.left), (b"border-right", &mut p.right), (b"border-top", &mut p.top), (b"border-bottom", &mut p.bottom)]
    {
        if let Some(b) = attr(reader, e, FO, key)? {
            *slot = Some(border(&b));
        }
    }
    if let Some(b) = attr(reader, e, STYLE, b"diagonal-tl-br")? {
        p.diag_down = Some(border(&b));
    }
    if let Some(b) = attr(reader, e, STYLE, b"diagonal-bl-tr")? {
        p.diag_up = Some(border(&b));
    }
    if let Some(w) = attr(reader, e, FO, b"wrap-option")? {
        p.wrap = Some(w == "wrap");
    }
    if let Some(v) = attr(reader, e, STYLE, b"vertical-align")? {
        p.v = Some(match v.as_str() {
            "top" => VAlign::Top,
            "middle" => VAlign::Center,
            "justify" => VAlign::Justify,
            _ => VAlign::Bottom,
        });
    }
    if let Some(s) = attr(reader, e, STYLE, b"shrink-to-fit")? {
        p.shrink = Some(s == "true");
    }
    if let Some(a) = attr(reader, e, STYLE, b"rotation-angle")? {
        let deg = a.trim_end_matches("deg").parse::<f64>().unwrap_or(0.0);
        if deg.is_finite() {
            let deg = deg.rem_euclid(360.0);
            // Excel keeps -90..=90: 270° is -90°.
            let r = if deg > 180.0 { deg - 360.0 } else { deg };
            p.rotation = Some(r.clamp(-90.0, 90.0).round() as i16);
        }
    }
    if attr(reader, e, STYLE, b"direction")?.as_deref() == Some("ttb") {
        p.rotation = Some(255);
    }
    if let Some(prot) = attr(reader, e, STYLE, b"cell-protect")? {
        p.locked = Some(prot.contains("protected"));
        p.hidden = Some(prot.contains("hidden"));
    }
    if attr(reader, e, STYLE, b"text-align-source")?.as_deref() == Some("value-type") {
        p.h = Some(HAlign::General);
    }
    Ok(())
}

/// `#RRGGBB` → colour.
pub(super) fn color(s: &str) -> Option<Color> {
    let h = s.trim().strip_prefix('#')?;
    (h.len() == 6).then(|| u32::from_str_radix(h, 16).ok().map(Color::Rgb)).flatten()
}

/// A length (`2.5cm`, `0.889in`, `12pt`…) in px at 96 dpi.
pub(super) fn length_px(s: &str) -> Option<f32> {
    length_pt(s).map(|pt| pt * 96.0 / 72.0)
}

/// A length in points.
pub(super) fn length_pt(s: &str) -> Option<f32> {
    let s = s.trim();
    let split = s.find(|c: char| c.is_ascii_alphabetic() || c == '%').unwrap_or(s.len());
    let (n, unit) = s.split_at(split);
    let n: f64 = n.trim().parse().ok()?;
    let pt = match unit {
        "pt" => n,
        "cm" => n * 72.0 / 2.54,
        "mm" => n * 72.0 / 25.4,
        "in" | "inch" => n * 72.0,
        "pc" => n * 12.0,
        "px" => n * 0.75,
        _ => return None,
    };
    (pt.is_finite() && (0.0..=100_000.0).contains(&pt)).then_some(pt as f32)
}

/// `0.74pt solid #000000` → a border line.
pub(super) fn border(s: &str) -> BorderLine {
    let mut width = 0.75f32;
    let mut kind = "solid";
    let mut col = Color::Auto;
    for tok in s.split_whitespace() {
        if let Some(c) = color(tok) {
            col = c;
        } else if let Some(w) = length_pt(tok) {
            width = w;
        } else {
            match tok {
                "thin" => width = 0.75,
                "medium" => width = 1.75,
                "thick" => width = 2.5,
                other => kind = if matches!(other, "none" | "hidden") { "none" } else { other },
            }
        }
    }
    let heavy = width >= 1.5;
    let style = match kind {
        "none" => BorderStyle::None,
        _ if width <= 0.0 => BorderStyle::None,
        "double" | "double-thin" => BorderStyle::Double,
        "dotted" => BorderStyle::Dotted,
        "dashed" | "fine-dashed" if heavy => BorderStyle::MediumDashed,
        "dashed" | "fine-dashed" => BorderStyle::Dashed,
        "dash-dot" if heavy => BorderStyle::MediumDashDot,
        "dash-dot" => BorderStyle::DashDot,
        "dash-dot-dot" if heavy => BorderStyle::MediumDashDotDot,
        "dash-dot-dot" => BorderStyle::DashDotDot,
        _ if width >= 2.25 => BorderStyle::Thick,
        _ if heavy => BorderStyle::Medium,
        _ if width < 0.5 => BorderStyle::Hair,
        _ => BorderStyle::Thin,
    };
    // Black is the automatic border colour.
    let color = if col == Color::Rgb(0) { Color::Auto } else { col };
    if style == BorderStyle::None { BorderLine::default() } else { BorderLine { style, color } }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lengths_and_borders() {
        assert_eq!(length_px("2.54cm").map(f32::round), Some(96.0));
        assert_eq!(length_px("1in"), Some(96.0));
        assert_eq!(length_pt("12pt"), Some(12.0));
        assert_eq!(length_pt("50%"), None);
        assert_eq!(border("0.74pt solid #ff0000"), BorderLine { style: BorderStyle::Thin, color: Color::Rgb(0xff0000) });
        assert_eq!(border("0.74pt solid #000000").color, Color::Auto);
        assert_eq!(border("1.76pt dashed #000000").style, BorderStyle::MediumDashed);
        assert_eq!(border("2.49pt solid #000000").style, BorderStyle::Thick);
        assert_eq!(border("none"), BorderLine::default());
    }
}
