//! ODS export: sheets, values, formulas (OpenFormula), cell styles, number formats, column widths,
//! row heights, hidden rows/columns/sheets, tab colours, merges and named ranges.
//! Format reference: ODF 1.3 part 3 (schema) and part 2 (packages).

use std::collections::{BTreeSet, HashMap};
use std::fmt::Write as _;
use std::io::{Cursor, Write as _};

use gridcraft_core::date::datetime_from_serial;
use gridcraft_core::{CellRef, MAX_COLS, RangeRef, Value, number_to_text};
use gridcraft_model::{BorderLine, BorderStyle, Cell, Color, HAlign, Sheet, Style, StyleId, Underline, VAlign, VertAlign, Visibility, Workbook};

use super::formula::to_odf;
use super::numfmt::{Kind, code_kind, data_style_xml};
use crate::IoError;
use crate::xml::{esc, esc_attr};

const NAMESPACES: &str = concat!(
    r#"xmlns:office="urn:oasis:names:tc:opendocument:xmlns:office:1.0" "#,
    r#"xmlns:style="urn:oasis:names:tc:opendocument:xmlns:style:1.0" "#,
    r#"xmlns:text="urn:oasis:names:tc:opendocument:xmlns:text:1.0" "#,
    r#"xmlns:table="urn:oasis:names:tc:opendocument:xmlns:table:1.0" "#,
    r#"xmlns:number="urn:oasis:names:tc:opendocument:xmlns:datastyle:1.0" "#,
    r#"xmlns:fo="urn:oasis:names:tc:opendocument:xmlns:xsl-fo-compatible:1.0" "#,
    r#"xmlns:svg="urn:oasis:names:tc:opendocument:xmlns:svg-compatible:1.0" "#,
    r#"xmlns:of="urn:oasis:names:tc:opendocument:xmlns:of:1.2" "#,
    r#"xmlns:calcext="urn:org:documentfoundation:names:experimental:calc:xmlns:calcext:1.0" "#,
    r#"xmlns:tableooo="http://openoffice.org/2009/table" "#,
    r#"office:version="1.3""#
);

/// Writes a workbook as an OpenDocument Spreadsheet (`.ods`) package.
pub fn write_ods(wb: &Workbook) -> Result<Vec<u8>, IoError> {
    let mut w = Writer::new(wb);
    let mut body = String::new();
    for sheet in &wb.sheets {
        w.table(&mut body, sheet);
    }
    w.names(&mut body, None);
    let content = format!(
        r#"<?xml version="1.0" encoding="UTF-8"?><office:document-content {NAMESPACES}>{}<office:automatic-styles>{}</office:automatic-styles><office:body><office:spreadsheet>{body}</office:spreadsheet></office:body></office:document-content>"#,
        w.font_decls(),
        w.automatic
    );
    let styles = w.styles_xml();
    let meta = meta_xml(wb);
    let manifest = concat!(
        r#"<?xml version="1.0" encoding="UTF-8"?><manifest:manifest xmlns:manifest="urn:oasis:names:tc:opendocument:xmlns:manifest:1.0" manifest:version="1.3">"#,
        r#"<manifest:file-entry manifest:full-path="/" manifest:version="1.3" manifest:media-type="application/vnd.oasis.opendocument.spreadsheet"/>"#,
        r#"<manifest:file-entry manifest:full-path="content.xml" manifest:media-type="text/xml"/>"#,
        r#"<manifest:file-entry manifest:full-path="styles.xml" manifest:media-type="text/xml"/>"#,
        r#"<manifest:file-entry manifest:full-path="meta.xml" manifest:media-type="text/xml"/>"#,
        r#"</manifest:manifest>"#
    );
    let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
    let zerr = |e: zip::result::ZipError| IoError::Zip(e.to_string());
    // ODF part 2, 3.3: `mimetype` comes first, uncompressed, so the type can be sniffed.
    let stored =
        zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored).last_modified_time(zip::DateTime::default());
    zip.start_file("mimetype", stored).map_err(zerr)?;
    zip.write_all(super::MIME).map_err(|e| IoError::Zip(e.to_string()))?;
    let deflated = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated)
        .compression_level(Some(6))
        .last_modified_time(zip::DateTime::default());
    for (name, data) in [
        ("META-INF/manifest.xml", manifest.as_bytes()),
        ("content.xml", content.as_bytes()),
        ("styles.xml", styles.as_bytes()),
        ("meta.xml", meta.as_bytes()),
    ] {
        zip.start_file(name, deflated.large_file(data.len() as u64 >= u32::MAX as u64)).map_err(zerr)?;
        zip.write_all(data).map_err(|e| IoError::Zip(e.to_string()))?;
    }
    zip.finish().map(Cursor::into_inner).map_err(zerr)
}

struct Writer<'a> {
    wb: &'a Workbook,
    automatic: String,
    cell_styles: HashMap<StyleId, String>,
    data_styles: HashMap<String, String>,
    columns: HashMap<u32, String>,
    rows: HashMap<(u32, bool), String>,
    tables: HashMap<(bool, Option<u32>, bool), String>,
    fonts: BTreeSet<String>,
}

fn hex(c: Color, wb: &Workbook) -> Option<String> {
    c.resolve(&wb.theme).map(|[r, g, b]| format!("#{r:02x}{g:02x}{b:02x}"))
}

fn inches(px: f32) -> String {
    format!("{:.4}in", (px.max(0.0) / 96.0))
}

fn border(line: &BorderLine, wb: &Workbook) -> String {
    let (width, kind) = match line.style {
        BorderStyle::None => return "none".into(),
        BorderStyle::Hair => ("0.26pt", "solid"),
        BorderStyle::Thin => ("0.74pt", "solid"),
        BorderStyle::Medium => ("1.76pt", "solid"),
        BorderStyle::Thick => ("2.49pt", "solid"),
        BorderStyle::Dashed => ("0.74pt", "dashed"),
        BorderStyle::MediumDashed => ("1.76pt", "dashed"),
        BorderStyle::Dotted => ("0.74pt", "dotted"),
        BorderStyle::Double => ("2.49pt", "double"),
        BorderStyle::DashDot => ("0.74pt", "dash-dot"),
        BorderStyle::MediumDashDot | BorderStyle::SlantDashDot => ("1.76pt", "dash-dot"),
        BorderStyle::DashDotDot => ("0.74pt", "dash-dot-dot"),
        BorderStyle::MediumDashDotDot => ("1.76pt", "dash-dot-dot"),
    };
    format!("{width} {kind} {}", hex(line.color, wb).unwrap_or_else(|| "#000000".into()))
}

fn font_family(name: &str) -> String {
    if name.contains(' ') { format!("'{name}'") } else { name.to_string() }
}

impl<'a> Writer<'a> {
    fn new(wb: &'a Workbook) -> Self {
        let mut fonts = BTreeSet::new();
        fonts.insert(wb.styles.get(StyleId::DEFAULT).font.name.clone());
        Writer {
            wb,
            automatic: String::new(),
            cell_styles: HashMap::new(),
            data_styles: HashMap::new(),
            columns: HashMap::new(),
            rows: HashMap::new(),
            tables: HashMap::new(),
            fonts,
        }
    }

    fn font_decls(&self) -> String {
        let mut s = String::from("<office:font-face-decls>");
        for f in &self.fonts {
            let _ = write!(s, r#"<style:font-face style:name="{}" svg:font-family="{}"/>"#, esc_attr(f), esc_attr(&font_family(f)));
        }
        s.push_str("</office:font-face-decls>");
        s
    }

    /// The data style for format `code`, added to the automatic styles once.
    fn data_style(&mut self, code: &str, out: &mut String) -> Option<String> {
        if let Some(n) = self.data_styles.get(code) {
            return Some(n.clone());
        }
        let name = format!("N{}", self.data_styles.len() + 1);
        let xml = data_style_xml(&name, code)?;
        out.push_str(&xml);
        self.data_styles.insert(code.to_string(), name.clone());
        Some(name)
    }

    /// Text, cell and paragraph properties of `s` that differ from `base` (all of them when `full`).
    fn properties(&mut self, s: &Style, base: &Style, full: bool) -> String {
        let wb = self.wb;
        let mut cell = String::new();
        let mut para = String::new();
        let mut text = String::new();
        let f = &s.font;
        let bf = &base.font;
        if full || f.name != bf.name {
            self.fonts.insert(f.name.clone());
            let _ = write!(text, r#" style:font-name="{}""#, esc_attr(&f.name));
        }
        if full || f.size != bf.size {
            let _ = write!(text, r#" fo:font-size="{}pt" style:font-size-asian="{0}pt" style:font-size-complex="{0}pt""#, f.size);
        }
        if f.bold != bf.bold || (full && f.bold) {
            let w = if f.bold { "bold" } else { "normal" };
            let _ = write!(text, r#" fo:font-weight="{w}" style:font-weight-asian="{w}" style:font-weight-complex="{w}""#);
        }
        if f.italic != bf.italic || (full && f.italic) {
            let w = if f.italic { "italic" } else { "normal" };
            let _ = write!(text, r#" fo:font-style="{w}" style:font-style-asian="{w}" style:font-style-complex="{w}""#);
        }
        if f.underline != bf.underline || (full && f.underline != Underline::None) {
            match f.underline {
                Underline::None => text.push_str(r#" style:text-underline-style="none""#),
                Underline::Single | Underline::SingleAccounting => {
                    text.push_str(r#" style:text-underline-style="solid" style:text-underline-width="auto" style:text-underline-color="font-color""#)
                }
                Underline::Double | Underline::DoubleAccounting => text.push_str(
                    r#" style:text-underline-style="solid" style:text-underline-type="double" style:text-underline-width="auto" style:text-underline-color="font-color""#,
                ),
            }
        }
        if f.strike != bf.strike || (full && f.strike) {
            text.push_str(if f.strike {
                r#" style:text-line-through-style="solid" style:text-line-through-type="single""#
            } else {
                r#" style:text-line-through-style="none""#
            });
        }
        if f.color != bf.color || (full && f.color != Color::Auto) {
            let _ = write!(text, r#" fo:color="{}""#, hex(f.color, wb).unwrap_or_else(|| "#000000".into()));
        }
        if f.vert != bf.vert || (full && f.vert != VertAlign::Baseline) {
            text.push_str(match f.vert {
                VertAlign::Baseline => r#" style:text-position="0% 100%""#,
                VertAlign::Superscript => r#" style:text-position="super 58%""#,
                VertAlign::Subscript => r#" style:text-position="sub 58%""#,
            });
        }
        if s.fill != base.fill || (full && s.fill != Default::default()) {
            let c = if s.fill.pattern == gridcraft_model::PatternType::None { None } else { hex(s.fill.fg, wb).or_else(|| hex(s.fill.bg, wb)) };
            let _ = write!(cell, r#" fo:background-color="{}""#, c.unwrap_or_else(|| "transparent".into()));
        }
        let b = &s.borders;
        if *b != base.borders || (full && *b != Default::default()) {
            if b.left == b.right && b.left == b.top && b.left == b.bottom {
                let _ = write!(cell, r#" fo:border="{}""#, border(&b.left, wb));
            } else {
                for (name, line) in [("left", &b.left), ("right", &b.right), ("top", &b.top), ("bottom", &b.bottom)] {
                    let _ = write!(cell, r#" fo:border-{name}="{}""#, border(line, wb));
                }
            }
            if b.diag_down != base.borders.diag_down || (full && !b.diag_down.is_none()) {
                let _ = write!(cell, r#" style:diagonal-tl-br="{}""#, border(&b.diag_down, wb));
            }
            if b.diag_up != base.borders.diag_up || (full && !b.diag_up.is_none()) {
                let _ = write!(cell, r#" style:diagonal-bl-tr="{}""#, border(&b.diag_up, wb));
            }
        }
        let a = &s.align;
        let ba = &base.align;
        if a.wrap != ba.wrap || (full && a.wrap) {
            let _ = write!(cell, r#" fo:wrap-option="{}""#, if a.wrap { "wrap" } else { "no-wrap" });
        }
        if a.v != ba.v || (full && a.v != VAlign::Bottom) {
            let v = match a.v {
                VAlign::Top => "top",
                VAlign::Center => "middle",
                VAlign::Bottom => "bottom",
                VAlign::Justify | VAlign::Distributed => "justify",
            };
            let _ = write!(cell, r#" style:vertical-align="{v}""#);
        }
        if a.shrink != ba.shrink || (full && a.shrink) {
            let _ = write!(cell, r#" style:shrink-to-fit="{}""#, a.shrink);
        }
        if a.rotation != ba.rotation || (full && a.rotation != 0) {
            if a.rotation == 255 {
                cell.push_str(r#" style:direction="ttb""#);
            } else {
                let _ = write!(cell, r#" style:rotation-angle="{}""#, i32::from(a.rotation).rem_euclid(360));
            }
        }
        if s.protection != base.protection || (full && s.protection != Default::default()) {
            let p = match (s.protection.locked, s.protection.hidden) {
                (false, false) => "none",
                (true, false) => "protected",
                (true, true) => "hidden-and-protected",
                (false, true) => "formula-hidden",
            };
            let _ = write!(cell, r#" style:cell-protect="{p}""#);
        }
        if a.h != ba.h || (full && a.h != HAlign::General) {
            if a.h == HAlign::General {
                cell.push_str(r#" style:text-align-source="value-type""#);
            } else {
                cell.push_str(r#" style:text-align-source="fix""#);
                if a.h == HAlign::Fill {
                    cell.push_str(r#" style:repeat-content="true""#);
                }
                let h = match a.h {
                    HAlign::Center | HAlign::CenterAcross => "center",
                    HAlign::Right => "end",
                    HAlign::Justify | HAlign::Distributed => "justify",
                    _ => "start",
                };
                let _ = write!(para, r#" fo:text-align="{h}""#);
            }
        }
        if a.indent != ba.indent || (full && a.indent != 0) {
            let _ = write!(para, r#" fo:margin-left="{}pt""#, u32::from(a.indent) * 10);
        }
        let mut out = String::new();
        if !cell.is_empty() {
            let _ = write!(out, "<style:table-cell-properties{cell}/>");
        }
        if !para.is_empty() {
            let _ = write!(out, "<style:paragraph-properties{para}/>");
        }
        if !text.is_empty() {
            let _ = write!(out, "<style:text-properties{text}/>");
        }
        out
    }

    /// The automatic cell style for `id`; `None` for the default style.
    fn cell_style(&mut self, id: StyleId) -> Option<String> {
        if id == StyleId::DEFAULT {
            return None;
        }
        if let Some(n) = self.cell_styles.get(&id) {
            return Some(n.clone());
        }
        let wb = self.wb;
        let style = wb.styles.get(id);
        let base = wb.styles.get(StyleId::DEFAULT);
        let name = format!("ce{}", self.cell_styles.len() + 1);
        let mut automatic = std::mem::take(&mut self.automatic);
        let data = if style.num_fmt != base.num_fmt { self.data_style(style.num_fmt.as_str(), &mut automatic) } else { None };
        let props = self.properties(style, base, false);
        let _ = write!(automatic, r#"<style:style style:name="{name}" style:family="table-cell" style:parent-style-name="Default""#);
        if let Some(d) = data {
            let _ = write!(automatic, r#" style:data-style-name="{d}""#);
        } else if style.num_fmt != base.num_fmt {
            // Back to General under a Default style with a number format.
            automatic.push_str(r#" style:data-style-name="N0""#);
        }
        let _ = write!(automatic, ">{props}</style:style>");
        self.automatic = automatic;
        self.cell_styles.insert(id, name.clone());
        Some(name)
    }

    fn column_style(&mut self, width: f32) -> String {
        let key = (width.max(0.0) * 100.0).round() as u32;
        if let Some(n) = self.columns.get(&key) {
            return n.clone();
        }
        let name = format!("co{}", self.columns.len() + 1);
        let _ = write!(
            self.automatic,
            r#"<style:style style:name="{name}" style:family="table-column"><style:table-column-properties fo:break-before="auto" style:column-width="{}"/></style:style>"#,
            inches(width)
        );
        self.columns.insert(key, name.clone());
        name
    }

    fn row_style(&mut self, height: f32, custom: bool) -> String {
        let key = ((height.max(0.0) * 100.0).round() as u32, custom);
        if let Some(n) = self.rows.get(&key) {
            return n.clone();
        }
        let name = format!("ro{}", self.rows.len() + 1);
        let _ = write!(
            self.automatic,
            r#"<style:style style:name="{name}" style:family="table-row"><style:table-row-properties style:row-height="{}" fo:break-before="auto" style:use-optimal-row-height="{}"/></style:style>"#,
            inches(height),
            !custom
        );
        self.rows.insert(key, name.clone());
        name
    }

    fn table_style(&mut self, sheet: &Sheet) -> String {
        let tab = sheet.tab_color.and_then(|c| c.resolve(&self.wb.theme)).map(|[r, g, b]| (u32::from(r) << 16) | (u32::from(g) << 8) | u32::from(b));
        let key = (sheet.visibility != Visibility::Visible, tab, sheet.right_to_left);
        if let Some(n) = self.tables.get(&key) {
            return n.clone();
        }
        let name = format!("ta{}", self.tables.len() + 1);
        let _ = write!(
            self.automatic,
            r#"<style:style style:name="{name}" style:family="table"><style:table-properties table:display="{}" style:writing-mode="{}""#,
            !key.0,
            if key.2 { "rl-tb" } else { "lr-tb" }
        );
        if let Some(t) = tab {
            let _ = write!(self.automatic, r##" tableooo:tab-color="#{t:06x}""##);
        }
        self.automatic.push_str("/></style:style>");
        self.tables.insert(key, name.clone());
        name
    }

    fn table(&mut self, out: &mut String, sheet: &Sheet) {
        let style = self.table_style(sheet);
        let _ = write!(out, r#"<table:table table:name="{}" table:style-name="{style}">"#, esc_attr(&sheet.name));
        let used = sheet.cells.used_range();
        let mut last_col = used.map_or(0, |u| u.end.col);
        let mut last_row = used.map_or(0, |u| u.end.row);
        for m in &sheet.merges {
            last_col = last_col.max(m.end.col);
            last_row = last_row.max(m.end.row);
        }
        if let Some((c, _)) = sheet.cols.iter().next_back() {
            last_col = last_col.max(*c);
        }
        if let Some((r, _)) = sheet.rows.iter().next_back() {
            last_row = last_row.max(*r);
        }
        let last_col = last_col.min(MAX_COLS - 1);

        // Columns, grouped while width, visibility and default style repeat.
        let mut prev: Option<(String, u32)> = None;
        let flush = |out: &mut String, prev: &mut Option<(String, u32)>| {
            if let Some((attrs, n)) = prev.take() {
                let rep = if n > 1 { format!(r#" table:number-columns-repeated="{n}""#) } else { String::new() };
                let _ = write!(out, "<table:table-column{attrs}{rep}/>");
            }
        };
        for c in 0..=last_col {
            let info = sheet.cols.get(&c);
            let width = info.and_then(|i| i.size).unwrap_or(sheet.default_col_width);
            let mut attrs = format!(r#" table:style-name="{}""#, self.column_style(width));
            if info.is_some_and(|i| i.hidden) {
                attrs.push_str(r#" table:visibility="collapse""#);
            }
            let default = info.and_then(|i| i.style).and_then(|s| self.cell_style(s)).unwrap_or_else(|| "Default".into());
            let _ = write!(attrs, r#" table:default-cell-style-name="{default}""#);
            match prev.as_mut() {
                Some((a, n)) if *a == attrs => *n += 1,
                _ => {
                    flush(out, &mut prev);
                    prev = Some((attrs, 1));
                }
            }
        }
        flush(out, &mut prev);

        // Rows: merged areas by row, the anchor carries the spans.
        let mut merges: Vec<&RangeRef> = sheet.merges.iter().collect();
        merges.sort_by_key(|m| m.start.row);
        let mut next_merge = 0usize;
        let mut active: Vec<&RangeRef> = Vec::new();
        let mut pending: Option<(String, u32)> = None;
        let flush_rows = |out: &mut String, pending: &mut Option<(String, u32)>| {
            if let Some((xml, n)) = pending.take() {
                if n > 1 {
                    let _ = write!(out, "{}", xml.replacen("<table:table-row", &format!(r#"<table:table-row table:number-rows-repeated="{n}""#), 1));
                } else {
                    out.push_str(&xml);
                }
            }
        };
        let cols_with_col_style = |c: u32| sheet.cols.get(&c).is_some_and(|i| i.style.is_some());
        for r in 0..=last_row {
            while let Some(m) = merges.get(next_merge).filter(|m| m.start.row <= r) {
                active.push(m);
                next_merge += 1;
            }
            active.retain(|m| m.end.row >= r);
            let info = sheet.rows.get(&r);
            let height = info.and_then(|i| i.size).unwrap_or(sheet.default_row_height);
            let custom = info.is_some_and(|i| i.size.is_some() && i.custom);
            let mut row = format!(r#"<table:table-row table:style-name="{}""#, self.row_style(height, custom));
            if info.is_some_and(|i| i.hidden) {
                row.push_str(r#" table:visibility="collapse""#);
            }
            let row_style = info.and_then(|i| i.style);
            if let Some(name) = row_style.and_then(|s| self.cell_style(s)) {
                let _ = write!(row, r#" table:default-cell-style-name="{name}""#);
            }
            row.push('>');
            // (column, cell xml) in column order.
            let mut items: Vec<(u32, String)> = Vec::new();
            let mut covered: Vec<(u32, u32)> = Vec::new();
            let mut anchors: HashMap<u32, (u32, u32)> = HashMap::new();
            for m in &active {
                if m.start.row == r {
                    anchors.insert(m.start.col, (m.end.col.saturating_sub(m.start.col) + 1, m.end.row.saturating_sub(m.start.row) + 1));
                    if m.end.col > m.start.col {
                        covered.push((m.start.col + 1, m.end.col));
                    }
                } else {
                    covered.push((m.start.col, m.end.col));
                }
            }
            let is_covered = |c: u32| covered.iter().any(|(a, b)| c >= *a && c <= *b);
            let mut cells: Vec<(u32, &Cell)> = sheet.cells.row(r, 0, last_col).collect();
            for c in anchors.keys() {
                if !cells.iter().any(|(cc, _)| cc == c) {
                    cells.push((*c, &EMPTY));
                }
            }
            cells.sort_by_key(|(c, _)| *c);
            for (c, cell) in cells {
                if is_covered(c) {
                    continue;
                }
                let inherited = row_style.is_some() || cols_with_col_style(c);
                let xml = self.cell(sheet, CellRef::new(r, c), cell, anchors.get(&c).copied(), inherited);
                items.push((c, xml));
            }
            for (a, b) in &covered {
                for c in *a..=*b {
                    items.push((c, "<table:covered-table-cell/>".into()));
                }
            }
            items.sort_by_key(|(c, _)| *c);
            // Gaps become empty cells; equal neighbours are merged into one repeated cell.
            let mut col = 0u32;
            let mut run: Option<(String, u32)> = None;
            let emit = |row: &mut String, run: &mut Option<(String, u32)>| {
                if let Some((xml, n)) = run.take() {
                    if n > 1 {
                        let tag = if xml.starts_with("<table:covered-table-cell") { "<table:covered-table-cell" } else { "<table:table-cell" };
                        row.push_str(&xml.replacen(tag, &format!(r#"{tag} table:number-columns-repeated="{n}""#), 1));
                    } else {
                        row.push_str(&xml);
                    }
                }
            };
            for (c, xml) in items {
                if c > col {
                    let gap = c - col;
                    emit(&mut row, &mut run);
                    run = Some(("<table:table-cell/>".into(), gap));
                }
                match run.as_mut() {
                    Some((x, n)) if *x == xml => *n += 1,
                    _ => {
                        emit(&mut row, &mut run);
                        run = Some((xml, 1));
                    }
                }
                col = c + 1;
            }
            emit(&mut row, &mut run);
            if col == 0 {
                row.push_str("<table:table-cell/>");
            }
            row.push_str("</table:table-row>");
            match pending.as_mut() {
                Some((x, n)) if *x == row => *n += 1,
                _ => {
                    flush_rows(out, &mut pending);
                    pending = Some((row, 1));
                }
            }
        }
        flush_rows(out, &mut pending);
        let index = self.wb.sheets.iter().position(|s| std::ptr::eq(s.as_ref(), sheet));
        if index.is_some() {
            self.names(out, index);
        }
        out.push_str("</table:table>");
    }

    fn cell(&mut self, sheet: &Sheet, pos: CellRef, cell: &Cell, span: Option<(u32, u32)>, inherited: bool) -> String {
        let mut s = String::from("<table:table-cell");
        match self.cell_style(cell.style) {
            Some(name) => {
                let _ = write!(s, r#" table:style-name="{name}""#);
            }
            None if inherited => s.push_str(r#" table:style-name="Default""#),
            None => {}
        }
        if let Some((w, h)) = span {
            if w > 1 {
                let _ = write!(s, r#" table:number-columns-spanned="{w}""#);
            }
            if h > 1 {
                let _ = write!(s, r#" table:number-rows-spanned="{h}""#);
            }
        }
        let formula = cell.formula.as_ref().map(|f| f.text.as_str()).or(match &cell.value {
            // A constant error is written as the formula that gives it.
            Value::Error(e) => Some(e.as_str()),
            _ => None,
        });
        if let Some(text) = formula {
            let _ = write!(s, r#" table:formula="{}""#, esc_attr(&to_odf(text)));
            if let Some(a) = cell.formula.as_ref().and_then(|f| f.array) {
                let _ = write!(
                    s,
                    r#" table:number-matrix-columns-spanned="{}" table:number-matrix-rows-spanned="{}""#,
                    a.end.col.saturating_sub(a.start.col) + 1,
                    a.end.row.saturating_sub(a.start.row) + 1
                );
            }
        }
        let value = match &cell.value {
            Value::Array(a) => a.data.first().cloned().unwrap_or(Value::Empty),
            Value::Empty => sheet.spill.get(&pos).cloned().filter(|_| cell.formula.is_some()).unwrap_or(Value::Empty),
            v => v.clone(),
        };
        let display = match &value {
            Value::Empty => None,
            Value::Number(n) => {
                let code = self.wb.styles.get(cell.style).num_fmt.as_str();
                let date = match code_kind(code) {
                    k @ (Kind::Date | Kind::Time) => datetime_from_serial(self.wb.date_system, *n).map(|d| (k, d)),
                    _ => None,
                };
                match date {
                    Some((Kind::Date, d)) => {
                        let _ = write!(s, r#" office:value-type="date" office:date-value="{:04}-{:02}-{:02}"#, d.year, d.month, d.day);
                        if n.fract() != 0.0 {
                            let _ = write!(s, "T{:02}:{:02}:{:02}", d.hour, d.minute, d.second);
                            if d.milli > 0 {
                                let _ = write!(s, ".{:03}", d.milli);
                            }
                        }
                        s.push('"');
                    }
                    Some((_, _)) => {
                        let total = (n * 86_400_000.0).round() as i64;
                        let (sign, total) = if total < 0 { ("-", -total) } else { ("", total) };
                        let _ = write!(
                            s,
                            r#" office:value-type="time" office:time-value="{sign}PT{}H{:02}M{:02}.{:03}S""#,
                            total / 3_600_000,
                            (total / 60_000) % 60,
                            (total / 1000) % 60,
                            total % 1000
                        );
                    }
                    None => {
                        let kind = if code_kind(code) == Kind::Percentage { "percentage" } else { "float" };
                        let _ = write!(s, r#" office:value-type="{kind}" office:value="{}""#, number_to_text(*n));
                    }
                }
                Some(number_to_text(*n))
            }
            Value::Text(t) => {
                s.push_str(r#" office:value-type="string""#);
                Some(t.to_string())
            }
            Value::Bool(b) => {
                let _ = write!(s, r#" office:value-type="boolean" office:boolean-value="{b}""#);
                Some(if *b { "TRUE" } else { "FALSE" }.into())
            }
            Value::Error(e) => {
                s.push_str(r#" office:value-type="float" office:value="0" calcext:value-type="error""#);
                Some(e.as_str().into())
            }
            Value::Array(_) => None,
        };
        match display {
            None => s.push_str("/>"),
            Some(text) => {
                s.push('>');
                for line in text.split('\n') {
                    s.push_str("<text:p>");
                    s.push_str(&paragraph(line));
                    s.push_str("</text:p>");
                }
                s.push_str("</table:table-cell>");
            }
        }
        s
    }

    /// Named ranges of `scope` (workbook-wide for `None`).
    fn names(&mut self, out: &mut String, scope: Option<usize>) {
        let base = self.wb.sheets.first().map(|s| format!("${}.$A$1", odf_sheet(&s.name))).unwrap_or_default();
        let mut xml = String::new();
        for n in self.wb.names.iter().filter(|n| n.scope == scope && !n.name.starts_with("_xlnm.")) {
            let odf = to_odf(&n.formula);
            let pure_ref =
                gridcraft_formula::parse(&n.formula).is_ok_and(|e| matches!(e, gridcraft_formula::Expr::Ref(ref r) if r.sheet_name().is_some()));
            match odf.strip_prefix("of:=[").and_then(|a| a.strip_suffix(']')).filter(|_| pure_ref) {
                Some(addr) => {
                    let _ = write!(
                        xml,
                        r#"<table:named-range table:name="{}" table:base-cell-address="{}" table:cell-range-address="{}"/>"#,
                        esc_attr(&n.name),
                        esc_attr(&base),
                        esc_attr(addr)
                    );
                }
                None => {
                    let _ = write!(
                        xml,
                        r#"<table:named-expression table:name="{}" table:base-cell-address="{}" table:expression="{}"/>"#,
                        esc_attr(&n.name),
                        esc_attr(&base),
                        esc_attr(&odf)
                    );
                }
            }
        }
        if !xml.is_empty() {
            let _ = write!(out, "<table:named-expressions>{xml}</table:named-expressions>");
        }
    }

    fn styles_xml(&mut self) -> String {
        let wb = self.wb;
        let base = wb.styles.get(StyleId::DEFAULT);
        let mut data = String::new();
        let default_data = self.data_style_standalone(base.num_fmt.as_str(), &mut data);
        let props = self.properties(base, &Style::default(), true);
        let mut s = format!(r#"<?xml version="1.0" encoding="UTF-8"?><office:document-styles {NAMESPACES}>{}<office:styles>"#, self.font_decls());
        // `N0` is General, for cells that drop the Default style's number format.
        s.push_str(r#"<number:number-style style:name="N0"><number:number number:min-integer-digits="1"/></number:number-style>"#);
        s.push_str(&data);
        let _ = write!(
            s,
            r#"<style:default-style style:family="table-cell"><style:text-properties style:font-name="{}" fo:font-size="{}pt"/></style:default-style>"#,
            esc_attr(&base.font.name),
            base.font.size
        );
        s.push_str(r#"<style:style style:name="Default" style:family="table-cell""#);
        if let Some(d) = default_data {
            let _ = write!(s, r#" style:data-style-name="{d}""#);
        }
        let _ = write!(s, ">{props}</style:style></office:styles></office:document-styles>");
        s
    }

    /// A data style for styles.xml, named apart from the automatic ones.
    fn data_style_standalone(&self, code: &str, out: &mut String) -> Option<String> {
        let xml = data_style_xml("NDefault", code)?;
        out.push_str(&xml);
        Some("NDefault".into())
    }
}

static EMPTY: Cell = Cell { value: Value::Empty, formula: None, style: StyleId::DEFAULT };

fn odf_sheet(name: &str) -> String {
    if !name.is_empty() && name.chars().all(|c| c.is_alphanumeric() || c == '_') {
        name.to_string()
    } else {
        format!("'{}'", name.replace('\'', "''"))
    }
}

/// Text of one paragraph: runs of spaces and tabs as ODF elements (ODF 1.3 part 3, 6.1.2).
fn paragraph(line: &str) -> String {
    let mut out = String::new();
    let mut spaces = 0usize;
    let mut at_start = true;
    let flush = |out: &mut String, spaces: &mut usize, at_start: bool| {
        match (*spaces, at_start) {
            (0, _) => {}
            (1, false) => out.push(' '),
            (n, false) => {
                let _ = write!(out, r#" <text:s text:c="{}"/>"#, n - 1);
            }
            (n, true) => {
                let _ = write!(out, r#"<text:s text:c="{n}"/>"#);
            }
        }
        *spaces = 0;
    };
    for ch in line.chars() {
        match ch {
            ' ' => spaces += 1,
            '\t' => {
                flush(&mut out, &mut spaces, at_start);
                out.push_str("<text:tab/>");
                at_start = false;
            }
            c => {
                flush(&mut out, &mut spaces, at_start);
                out.push_str(&esc(c.encode_utf8(&mut [0; 4])));
                at_start = false;
            }
        }
    }
    // Trailing spaces would collapse: keep them as elements.
    if spaces > 0 {
        let _ = write!(out, r#"<text:s text:c="{spaces}"/>"#);
    }
    out
}

fn meta_xml(wb: &Workbook) -> String {
    let p = &wb.props;
    let mut s = String::from(
        r#"<?xml version="1.0" encoding="UTF-8"?><office:document-meta xmlns:office="urn:oasis:names:tc:opendocument:xmlns:office:1.0" xmlns:meta="urn:oasis:names:tc:opendocument:xmlns:meta:1.0" xmlns:dc="http://purl.org/dc/elements/1.1/" office:version="1.3"><office:meta><meta:generator>GridCraft</meta:generator>"#,
    );
    for (tag, value) in [
        ("dc:title", &p.title),
        ("dc:subject", &p.subject),
        ("meta:initial-creator", &p.author),
        ("dc:description", &p.description),
        ("meta:keyword", &p.keywords),
    ] {
        if !value.is_empty() {
            let _ = write!(s, "<{tag}>{}</{tag}>", esc(value));
        }
    }
    s.push_str("</office:meta></office:document-meta>");
    s
}
