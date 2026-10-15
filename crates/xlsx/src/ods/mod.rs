//! OpenDocument Spreadsheet import and export: sheets, typed values, formulas (OpenFormula),
//! cell styles and number formats, column widths, row heights, merges and named ranges. Charts,
//! images, comments, conditional formats and validations are not translated. All fixtures are
//! authored in Rust.
//! Format reference: ODF 1.3, part 3, sections 6.1 and 9.1:
//! <https://docs.oasis-open.org/office/OpenDocument/v1.3/os/part3-schema/OpenDocument-v1.3-os-part3-schema.html>.

mod formula;
mod numfmt;
mod style;
mod write;

pub use write::write_ods;

use std::collections::HashMap;
use std::io::{Cursor, Read};
use std::sync::Arc;

use gridcraft_core::date::{days_in_month, serial_from_ymd};
use gridcraft_core::{CellRef, DateSystem, MAX_COLS, MAX_ROWS, RangeRef, Value};
use gridcraft_model::{Cell, DefinedName, Formula, NumFmt, Sheet, Style, StyleId, StyleTable, Visibility, Workbook};
use quick_xml::NsReader;
use quick_xml::XmlVersion;
use quick_xml::events::{BytesStart, Event};
use quick_xml::name::ResolveResult;

use crate::package::{MAX_ENTRIES, Package};
use crate::{IoError, ReadReport};

const MIME: &[u8] = b"application/vnd.oasis.opendocument.spreadsheet";
const OFFICE: &[u8] = b"urn:oasis:names:tc:opendocument:xmlns:office:1.0";
const TABLE: &[u8] = b"urn:oasis:names:tc:opendocument:xmlns:table:1.0";
const TEXT: &[u8] = b"urn:oasis:names:tc:opendocument:xmlns:text:1.0";
const CALC_EXT: &[u8] = b"urn:org:documentfoundation:names:experimental:calc:xmlns:calcext:1.0";
const MANIFEST: &[u8] = b"urn:oasis:names:tc:opendocument:xmlns:manifest:1.0";
const MAX_CELLS: u64 = 1_000_000;
const MAX_MERGES: u64 = 100_000;
const MAX_CELL_TEXT: usize = 1024 * 1024;
const MAX_TEXT: u64 = 64 * 1024 * 1024;
const MAX_SHEETS: usize = 1024;
/// Rows given a height or style one by one; larger runs (blank padding to the sheet edge) are skipped.
const MAX_LINE_RUN: u32 = 10_000;
const MAX_NAMES: usize = 100_000;

/// Reads only the small, exact mimetype entry, including when the ZIP uses compression.
/// Looking at a filename or at arbitrary bytes inside the archive is not sufficient.
pub(crate) fn has_mimetype(bytes: &[u8]) -> bool {
    let Ok(mut zip) = zip::ZipArchive::new(Cursor::new(bytes)) else { return false };
    if zip.len() > MAX_ENTRIES {
        return false;
    }
    let Ok(mut file) = zip.by_name("mimetype") else { return false };
    if file.size() != MIME.len() as u64 {
        return false;
    }
    let mut value = Vec::new();
    (&mut file).take(MIME.len() as u64 + 1).read_to_end(&mut value).is_ok() && value == MIME
}

/// Imports ODS sheets, typed values, formulas, styles, column widths, row heights, merges and
/// named ranges. A formula GridCraft can't read keeps its cached result as a constant. Always
/// inspect the returned warnings.
pub fn read_ods(bytes: &[u8]) -> Result<(Workbook, ReadReport), IoError> {
    let mut package = Package::open(bytes)?;
    if !has_mimetype(bytes) {
        return Err(IoError::Format("missing or invalid ODS mimetype".into()));
    }
    if let Some(manifest) = package.read("META-INF/manifest.xml")? {
        check_manifest(&manifest)?;
    }
    let content = package.read("content.xml")?.ok_or_else(|| IoError::Format("ODS content.xml is missing".into()))?;
    let mut styles = style::Styles::default();
    if let Some(shared) = package.read("styles.xml")? {
        styles.collect(&shared)?;
    }
    styles.collect(&content)?;
    let mut import = Import::new(styles);
    import.parse(&content)?;
    import.finish_names();
    if import.wb.sheets.is_empty() {
        return Err(IoError::Format("ODS contains no worksheets".into()));
    }
    Ok((import.wb, import.report))
}

fn is_ns(ns: ResolveResult<'_>, expected: &[u8]) -> bool {
    matches!(ns, ResolveResult::Bound(n) if n.as_ref() == expected)
}

fn attr(reader: &mut NsReader<&[u8]>, e: &BytesStart<'_>, ns: &[u8], name: &[u8]) -> Result<Option<String>, IoError> {
    let mut found = None;
    for a in e.attributes() {
        let a = a.map_err(|e| IoError::Xml(e.to_string()))?;
        let (resolved, local) = reader.resolver_mut().resolve_attribute(a.key);
        if is_ns(resolved, ns) && local.as_ref() == name {
            let value = a.normalized_value(XmlVersion::Explicit1_0).map_err(|e| IoError::Xml(e.to_string()))?;
            if value.len() > MAX_CELL_TEXT {
                return Err(IoError::TooLarge("ODS attribute exceeds 1 MiB".into()));
            }
            found = Some(value.into_owned());
        }
    }
    Ok(found)
}

fn positive(value: Option<String>, default: u32, limit: u32) -> Result<u32, IoError> {
    let n = match value {
        Some(s) => s.parse::<u32>().map_err(|_| IoError::Format("invalid ODS repeat or span count".into()))?,
        None => default,
    };
    if n == 0 || n > limit {
        return Err(IoError::TooLarge("ODS repeat or span exceeds worksheet bounds".into()));
    }
    Ok(n)
}

fn xml_error(e: impl std::fmt::Display) -> IoError {
    IoError::Xml(format!("ODS: {e}"))
}

fn check_manifest(bytes: &[u8]) -> Result<(), IoError> {
    let mut reader = NsReader::from_reader(bytes);
    let mut depth = 0usize;
    let mut root_seen = false;
    loop {
        let event = reader.read_event().map_err(xml_error)?;
        match &event {
            Event::Start(e) | Event::Empty(e) => {
                let (ns, name) = reader.resolver_mut().resolve_element(e.name());
                if is_ns(ns.clone(), MANIFEST) && name.as_ref() == b"encryption-data" {
                    return Err(IoError::Format("encrypted ODS files are not supported".into()));
                }
                if depth == 0 {
                    if root_seen || !is_ns(ns, MANIFEST) || name.as_ref() != b"manifest" {
                        return Err(xml_error("expected one manifest:manifest root"));
                    }
                    root_seen = true;
                }
                if matches!(event, Event::Start(_)) {
                    depth += 1;
                    if depth > crate::xml::MAX_DEPTH {
                        return Err(xml_error("manifest nested too deeply"));
                    }
                }
            }
            Event::End(_) => depth = depth.checked_sub(1).ok_or_else(|| xml_error("unexpected manifest closing element"))?,
            Event::DocType(_) => return Err(xml_error("DOCTYPE is not supported")),
            Event::Eof => break,
            _ => {}
        }
    }
    if !root_seen || depth != 0 {
        return Err(xml_error("incomplete manifest"));
    }
    Ok(())
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Tag {
    Document,
    Body,
    Spreadsheet,
    Table,
    Columns,
    Column,
    Rows,
    Row,
    Cell,
    Covered,
    Paragraph,
    Inline,
    Space,
    Tab,
    Break,
    NamedRange,
    NamedExpression,
    Annotation,
    Drawing,
    Unsupported,
    Other,
}

const DRAW: &[u8] = b"urn:oasis:names:tc:opendocument:xmlns:drawing:1.0";

fn tag(reader: &mut NsReader<&[u8]>, e: &BytesStart<'_>) -> Tag {
    let (ns, name) = reader.resolver_mut().resolve_element(e.name());
    let local = name.as_ref();
    if is_ns(ns.clone(), OFFICE) {
        return match local {
            b"document-content" => Tag::Document,
            b"body" => Tag::Body,
            b"spreadsheet" => Tag::Spreadsheet,
            b"annotation" => Tag::Annotation,
            _ => Tag::Other,
        };
    }
    if is_ns(ns.clone(), TABLE) {
        return match local {
            b"table" => Tag::Table,
            b"table-column-group" | b"table-header-columns" | b"table-columns" => Tag::Columns,
            b"table-column" => Tag::Column,
            b"table-row-group" | b"table-header-rows" | b"table-rows" => Tag::Rows,
            b"table-row" => Tag::Row,
            b"table-cell" => Tag::Cell,
            b"covered-table-cell" => Tag::Covered,
            b"named-range" => Tag::NamedRange,
            b"named-expression" => Tag::NamedExpression,
            b"content-validations" => Tag::Unsupported,
            _ => Tag::Other,
        };
    }
    if is_ns(ns.clone(), TEXT) {
        return match local {
            b"p" | b"h" => Tag::Paragraph,
            b"span" | b"a" | b"meta" | b"meta-field" | b"ruby" | b"ruby-base" => Tag::Inline,
            b"s" => Tag::Space,
            b"tab" => Tag::Tab,
            b"line-break" => Tag::Break,
            _ => Tag::Other,
        };
    }
    if is_ns(ns.clone(), DRAW) && matches!(local, b"frame" | b"rect" | b"line" | b"custom-shape" | b"ellipse" | b"g" | b"polygon" | b"path") {
        return Tag::Drawing;
    }
    if is_ns(ns, CALC_EXT) && local == b"conditional-formats" {
        return Tag::Unsupported;
    }
    Tag::Other
}

struct SheetDraft {
    sheet: Sheet,
    depth: usize,
    row: u32,
    col: u32,
    /// Column runs: (first column, column count, width px, hidden, default cell style).
    columns: Vec<(u32, u32, Option<f32>, bool, Option<StyleId>)>,
}

impl SheetDraft {
    fn column_style(&self, col: u32) -> Option<StyleId> {
        self.columns.iter().find(|(c, n, ..)| col >= *c && col - c < *n).and_then(|c| c.4)
    }
}

struct RowDraft {
    depth: usize,
    repeat: u32,
    col: u32,
    cells: Vec<RepeatedCell>,
    height: Option<f32>,
    hidden: bool,
    style: Option<StyleId>,
}
struct RepeatedCell {
    col: u32,
    repeat: u32,
    width: u32,
    height: u32,
    cell: Cell,
}
struct CellDraft {
    depth: usize,
    repeat: u32,
    width: u32,
    height: u32,
    covered: bool,
    kind: Option<String>,
    raw: Option<String>,
    formula: Option<String>,
    matrix: Option<(u32, u32)>,
    style: Option<StyleId>,
    text: String,
    paragraph: Option<usize>,
    paragraphs: usize,
    paragraph_started: bool,
    pending_space: bool,
}
struct Import {
    wb: Workbook,
    report: ReadReport,
    stack: Vec<Tag>,
    sheet: Option<SheetDraft>,
    row: Option<RowDraft>,
    cell: Option<CellDraft>,
    cells: u64,
    merges: u64,
    text: u64,
    styles: style::Styles,
    base: Style,
    style_ids: HashMap<String, StyleId>,
    /// (style, format code) → style with that number format, for typed values in General cells.
    formats: Vec<(StyleId, &'static str, StyleId)>,
    /// (name, scope sheet, formula text) gathered while reading; added once every sheet is known.
    names: Vec<(String, Option<usize>, String)>,
    /// Sheets stored under another name than the file's: (file name, stored name).
    renames: Vec<(String, String)>,
    spreadsheet_seen: bool,
}

impl Import {
    fn new(styles: style::Styles) -> Self {
        let mut wb = Workbook::new();
        wb.sheets.clear();
        let base = styles.base();
        wb.styles = StyleTable::with_default(base.clone());
        Self {
            wb,
            report: ReadReport::default(),
            stack: vec![],
            sheet: None,
            row: None,
            cell: None,
            cells: 0,
            merges: 0,
            text: 0,
            styles,
            base,
            style_ids: HashMap::new(),
            formats: vec![],
            names: vec![],
            renames: vec![],
            spreadsheet_seen: false,
        }
    }

    /// The workbook style of the ODF cell style `name`.
    fn style_id(&mut self, name: Option<String>) -> Option<StyleId> {
        let name = name?;
        if let Some(id) = self.style_ids.get(&name) {
            return Some(*id);
        }
        let style = self.styles.cell_style(&name, &self.base);
        let id = self.wb.styles.intern(style);
        if self.style_ids.len() < 100_000 {
            self.style_ids.insert(name, id);
        }
        Some(id)
    }

    fn parse(&mut self, bytes: &[u8]) -> Result<(), IoError> {
        let mut reader = NsReader::from_reader(bytes);
        let mut root_seen = false;
        loop {
            match reader.read_event().map_err(xml_error)? {
                Event::Start(e) => {
                    if self.stack.len() >= crate::xml::MAX_DEPTH {
                        return Err(xml_error("XML nested too deeply"));
                    }
                    if self.stack.is_empty() {
                        if root_seen || tag(&mut reader, &e) != Tag::Document {
                            return Err(xml_error("expected one office:document-content root"));
                        }
                        root_seen = true;
                    }
                    self.start(&mut reader, &e)?;
                }
                Event::Empty(e) => {
                    if self.stack.is_empty() {
                        return Err(xml_error("expected office:document-content with a spreadsheet"));
                    }
                    self.start(&mut reader, &e)?;
                    self.end()?;
                }
                Event::End(_) => self.end()?,
                Event::Text(t) => {
                    let text = t.xml_content(XmlVersion::Explicit1_0).map_err(xml_error)?;
                    if self.stack.is_empty() && !text.trim().is_empty() {
                        return Err(xml_error("text outside the document"));
                    }
                    self.append(&text)?;
                }
                Event::CData(t) => self.append(&t.decode().map_err(xml_error)?)?,
                Event::GeneralRef(r) => {
                    let ch = match r.resolve_char_ref().map_err(xml_error)? {
                        Some(c) => c,
                        None => match r.as_ref() {
                            b"amp" => '&',
                            b"lt" => '<',
                            b"gt" => '>',
                            b"apos" => '\'',
                            b"quot" => '"',
                            _ => return Err(xml_error("unsupported XML entity")),
                        },
                    };
                    self.append(ch.encode_utf8(&mut [0; 4]))?;
                }
                Event::DocType(_) => return Err(xml_error("DOCTYPE is not supported")),
                Event::Eof => break,
                _ => {}
            }
        }
        if !root_seen || !self.stack.is_empty() || !self.spreadsheet_seen {
            return Err(xml_error("incomplete ODS document"));
        }
        Ok(())
    }

    /// Whether the element about to open sits directly in the sheet, through only `container`s.
    fn in_sheet(&self, container: Tag) -> bool {
        self.sheet.as_ref().is_some_and(|s| self.stack.get(s.depth + 1..).is_some_and(|p| p.iter().all(|t| *t == container)))
    }

    fn start(&mut self, reader: &mut NsReader<&[u8]>, e: &BytesStart<'_>) -> Result<(), IoError> {
        let t = tag(reader, e);
        let depth = self.stack.len();
        match t {
            Tag::Spreadsheet if self.stack == [Tag::Document, Tag::Body] => {
                if self.spreadsheet_seen {
                    return Err(xml_error("multiple spreadsheets in one document"));
                }
                self.spreadsheet_seen = true;
            }
            Tag::Table if self.stack == [Tag::Document, Tag::Body, Tag::Spreadsheet] => {
                if self.wb.sheets.len() >= MAX_SHEETS {
                    return Err(IoError::TooLarge("ODS contains more than 1024 sheets".into()));
                }
                let original = attr(reader, e, TABLE, b"name")?.unwrap_or_default();
                let mut name = crate::read::unique_name(&self.wb, original.trim_matches('\''), self.wb.sheets.len());
                if self.wb.check_sheet_name(&name, None).is_err() {
                    name = self.wb.next_sheet_name();
                }
                if name != original {
                    self.report.warn("Some ODS sheet names were shortened or renamed to valid, unique XLSX sheet names; formulas and names referring to them follow.");
                    if self.renames.len() < MAX_SHEETS {
                        self.renames.push((original, name.clone()));
                    }
                }
                let mut sheet = Sheet::new(name);
                if let Some(props) = attr(reader, e, TABLE, b"style-name")?.and_then(|n| self.styles.table(&n).cloned()) {
                    if props.hidden {
                        sheet.visibility = Visibility::Hidden;
                    }
                    sheet.tab_color = props.tab_color;
                }
                self.sheet = Some(SheetDraft { sheet, depth, row: 0, col: 0, columns: vec![] });
            }
            Tag::Column if self.in_sheet(Tag::Columns) => {
                let repeat = positive(attr(reader, e, TABLE, b"number-columns-repeated")?, 1, u32::MAX)?;
                let width = attr(reader, e, TABLE, b"style-name")?.and_then(|n| self.styles.column_width(&n));
                let hidden = matches!(attr(reader, e, TABLE, b"visibility")?.as_deref(), Some("collapse" | "filter"));
                let default = attr(reader, e, TABLE, b"default-cell-style-name")?;
                let style = self.style_id(default).filter(|id| *id != StyleId::DEFAULT);
                if let Some(sheet) = self.sheet.as_mut()
                    && sheet.col < MAX_COLS
                {
                    let count = repeat.min(MAX_COLS - sheet.col);
                    sheet.columns.push((sheet.col, count, width, hidden, style));
                    sheet.col += count;
                }
            }
            Tag::Row if self.in_sheet(Tag::Rows) => {
                let repeat = positive(attr(reader, e, TABLE, b"number-rows-repeated")?, 1, u32::MAX)?;
                let props = attr(reader, e, TABLE, b"style-name")?.and_then(|n| self.styles.row(&n));
                let hidden = matches!(attr(reader, e, TABLE, b"visibility")?.as_deref(), Some("collapse" | "filter"));
                let default = attr(reader, e, TABLE, b"default-cell-style-name")?;
                let style = self.style_id(default);
                let height = props.filter(|p| !p.optimal).and_then(|p| p.height);
                self.row = Some(RowDraft { depth, repeat, col: 0, cells: vec![], height, hidden, style });
            }
            Tag::Cell | Tag::Covered if self.row.as_ref().is_some_and(|r| depth == r.depth + 1) => {
                let kind = if attr(reader, e, CALC_EXT, b"value-type")?.as_deref() == Some("error") {
                    Some("error".into())
                } else {
                    attr(reader, e, OFFICE, b"value-type")?
                };
                let key: &[u8] = match kind.as_deref() {
                    Some("boolean") => b"boolean-value",
                    Some("date") => b"date-value",
                    Some("time") => b"time-value",
                    Some("string") => b"string-value",
                    _ => b"value",
                };
                let own = attr(reader, e, TABLE, b"style-name")?;
                let style = match own {
                    Some(n) => self.style_id(Some(n)),
                    None => self.row.as_ref().and_then(|r| r.style),
                };
                let matrix = match (attr(reader, e, TABLE, b"number-matrix-columns-spanned")?, attr(reader, e, TABLE, b"number-matrix-rows-spanned")?)
                {
                    (None, None) => None,
                    (c, r) => Some((positive(c, 1, MAX_COLS)?, positive(r, 1, MAX_ROWS)?)),
                };
                self.cell = Some(CellDraft {
                    depth,
                    repeat: positive(attr(reader, e, TABLE, b"number-columns-repeated")?, 1, u32::MAX)?,
                    width: positive(attr(reader, e, TABLE, b"number-columns-spanned")?, 1, MAX_COLS)?,
                    height: positive(attr(reader, e, TABLE, b"number-rows-spanned")?, 1, MAX_ROWS)?,
                    covered: t == Tag::Covered,
                    kind,
                    raw: attr(reader, e, OFFICE, key)?,
                    formula: attr(reader, e, TABLE, b"formula")?,
                    matrix,
                    style,
                    text: String::new(),
                    paragraph: None,
                    paragraphs: 0,
                    paragraph_started: false,
                    pending_space: false,
                });
            }
            Tag::Paragraph if self.cell.as_ref().is_some_and(|c| depth == c.depth + 1) => {
                if let Some(cell) = self.cell.as_mut() {
                    if cell.paragraphs > 0 {
                        push_text(&mut cell.text, "\n")?;
                    }
                    cell.paragraph = Some(depth);
                    cell.paragraphs += 1;
                    cell.paragraph_started = false;
                    cell.pending_space = false;
                }
            }
            Tag::Space | Tag::Tab | Tag::Break if self.captures_text() => {
                let count = if t == Tag::Space {
                    let n = attr(reader, e, TEXT, b"c")?.map(|s| s.parse::<u32>()).transpose().map_err(xml_error)?.unwrap_or(1);
                    if n as usize > MAX_CELL_TEXT {
                        return Err(IoError::TooLarge("ODS repeated spaces exceed 1 MiB".into()));
                    }
                    n
                } else {
                    1
                };
                let value = match t {
                    Tag::Space => " ",
                    Tag::Tab => "\t",
                    _ => "\n",
                };
                if let Some(cell) = self.cell.as_mut() {
                    if cell.pending_space {
                        push_text(&mut cell.text, " ")?;
                        cell.pending_space = false;
                    }
                    cell.paragraph_started = true;
                    if cell.text.len().saturating_add(count as usize) > MAX_CELL_TEXT {
                        return Err(IoError::TooLarge("ODS cell text exceeds 1 MiB".into()));
                    }
                    for _ in 0..count {
                        cell.text.push_str(value);
                    }
                }
            }
            Tag::NamedRange | Tag::NamedExpression if self.stack.contains(&Tag::Spreadsheet) => {
                let name = attr(reader, e, TABLE, b"name")?.unwrap_or_default();
                let body = if t == Tag::NamedRange {
                    attr(reader, e, TABLE, b"cell-range-address")?.map(|a| format!("of:=[{a}]"))
                } else {
                    attr(reader, e, TABLE, b"expression")?
                };
                // A name inside a table belongs to that sheet.
                let scope = self.sheet.as_ref().map(|_| self.wb.sheets.len());
                if let Some(body) = body
                    && self.names.len() < MAX_NAMES
                {
                    match formula::from_odf(&body).filter(|f| !name.is_empty() && name.len() <= 255 && gridcraft_formula::parse(f).is_ok()) {
                        Some(f) => self.names.push((name, scope, f)),
                        None => self.report.warn("Some ODS named ranges use syntax GridCraft can't read and were not imported."),
                    }
                }
            }
            Tag::Annotation if self.cell.is_some() => self.report.warn("ODS cell comments are not imported."),
            Tag::Drawing if self.sheet.is_some() => self.report.warn("ODS charts, images and shapes are not imported."),
            Tag::Unsupported => self.report.warn("ODS conditional formatting and data validation are not imported."),
            _ => {}
        }
        self.stack.push(t);
        Ok(())
    }

    fn captures_text(&self) -> bool {
        self.cell
            .as_ref()
            .and_then(|c| c.paragraph)
            .is_some_and(|depth| self.stack.get(depth..).is_some_and(|p| p.iter().all(|t| matches!(t, Tag::Paragraph | Tag::Inline))))
    }

    fn append(&mut self, text: &str) -> Result<(), IoError> {
        if self.captures_text()
            && let Some(cell) = self.cell.as_mut()
        {
            // ODF 1.3 section 6.1.2: collapse XML whitespace across inline runs,
            // but expand explicit text:s/tab/line-break elements separately.
            for ch in text.chars() {
                if matches!(ch, ' ' | '\t' | '\n' | '\r') {
                    cell.pending_space = cell.paragraph_started;
                } else {
                    if cell.pending_space {
                        push_text(&mut cell.text, " ")?;
                        cell.pending_space = false;
                    }
                    push_text(&mut cell.text, ch.encode_utf8(&mut [0; 4]))?;
                    cell.paragraph_started = true;
                }
            }
        }
        Ok(())
    }

    fn end(&mut self) -> Result<(), IoError> {
        self.stack.pop().ok_or_else(|| xml_error("unexpected closing element"))?;
        let depth = self.stack.len();
        if let Some(cell) = self.cell.as_mut()
            && cell.paragraph == Some(depth)
        {
            cell.paragraph = None;
        }
        if self.cell.as_ref().is_some_and(|c| c.depth == depth) {
            self.finish_cell()?;
        }
        if self.row.as_ref().is_some_and(|r| r.depth == depth) {
            self.finish_row()?;
        }
        if self.sheet.as_ref().is_some_and(|s| s.depth == depth)
            && let Some(sheet) = self.sheet.take()
        {
            self.wb.sheets.push(Arc::new(finish_columns(sheet)));
        }
        Ok(())
    }

    fn finish_cell(&mut self) -> Result<(), IoError> {
        let Some(mut draft) = self.cell.take() else { return Ok(()) };
        // Formatting set on the cell or its row is kept even on a blank cell; a column's default
        // style only applies to cells with content (the column itself carries it).
        let styled = draft.style.is_some_and(|s| s != StyleId::DEFAULT);
        if draft.style.is_none() {
            let col = self.row.as_ref().map_or(0, |r| r.col);
            draft.style = self.sheet.as_ref().and_then(|s| s.column_style(col));
        }
        let cell = self.convert(&draft);
        let (width, height) = if draft.covered { (1, 1) } else { (draft.width, draft.height) };
        let Some(row) = self.row.as_mut() else { return Ok(()) };
        let content = has_content(&cell);
        let kept = content || width > 1 || height > 1 || styled;
        let cell = if content || styled { cell } else { Cell { style: StyleId::DEFAULT, ..cell } };
        // Writers pad rows with blank repeats up to their own (possibly wider) sheet edge;
        // blank cells past ours carry no data, so clip them instead of rejecting the file.
        let end = match row.col.checked_add(draft.repeat).filter(|n| *n <= MAX_COLS) {
            Some(end) => end,
            None if !content && width == 1 && height == 1 => MAX_COLS,
            None => return Err(IoError::TooLarge("ODS row exceeds worksheet width".into())),
        };
        if kept {
            row.cells.push(RepeatedCell { col: row.col, repeat: end - row.col, width, height, cell });
        }
        row.col = end;
        Ok(())
    }

    /// The style of a typed value: the cell's own, or a General cell's with the format the type implies.
    fn typed_style(&mut self, style: StyleId, code: Option<&'static str>) -> StyleId {
        let Some(code) = code else { return style };
        if !self.wb.styles.get(style).num_fmt.as_str().eq_ignore_ascii_case("General") {
            return style;
        }
        if let Some((_, _, id)) = self.formats.iter().find(|(s, f, _)| *s == style && *f == code) {
            return *id;
        }
        let id = self.wb.styles.derive(style, |s| s.num_fmt = NumFmt::new(code));
        self.formats.push((style, code, id));
        id
    }

    fn convert(&mut self, draft: &CellDraft) -> Cell {
        let raw = draft.raw.as_deref();
        let mut format = None;
        let formula = match draft.formula.as_deref() {
            Some(src) => match formula::from_odf(src).map(|text| Formula::new(&text)).filter(|f| f.expr.is_some()) {
                Some(mut f) => {
                    if let Some((cols, rows)) = draft.matrix {
                        f.array = Some(RangeRef::new(CellRef::new(0, 0), CellRef::new(rows - 1, cols - 1)));
                    }
                    Some(f)
                }
                None => {
                    self.report.warn("Some ODS formulas use syntax GridCraft can't read; their saved results were kept as values.");
                    None
                }
            },
            None => None,
        };
        let has_formula = draft.formula.is_some();
        let value = match draft.kind.as_deref() {
            Some("float" | "percentage" | "currency") => raw.and_then(|s| s.parse::<f64>().ok()).filter(|n| n.is_finite()).map(|n| {
                if draft.kind.as_deref() == Some("percentage") {
                    format = Some("0.00%");
                }
                Value::Number(n)
            }),
            Some("boolean") => match raw {
                Some("true" | "1") => Some(Value::Bool(true)),
                Some("false" | "0") => Some(Value::Bool(false)),
                _ => None,
            },
            Some("string") if raw.is_some() || draft.paragraphs > 0 || !has_formula => Some(Value::Text(Arc::from(raw.unwrap_or(&draft.text)))),
            Some("void") if !has_formula => Some(Value::Empty),
            Some("date") => raw.and_then(parse_date).map(|(n, f)| {
                format = Some(f);
                Value::Number(n)
            }),
            Some("time") => raw.and_then(parse_duration).map(|n| {
                format = Some("[h]:mm:ss.000");
                Value::Number(n)
            }),
            Some("error") if formula.is_some() => {
                Some(Value::Error(gridcraft_core::CellError::parse(draft.text.trim()).unwrap_or(gridcraft_core::CellError::Value)))
            }
            None if !has_formula => Some(if draft.text.is_empty() { Value::Empty } else { Value::Text(Arc::from(draft.text.as_str())) }),
            // A readable formula without a usable cache is calculated after opening.
            _ if formula.is_some() => Some(Value::Empty),
            _ => None,
        };
        let value = value.unwrap_or_else(|| {
            self.report.warn("Some ODS values or formula caches were unsupported or missing; their displayed text (or raw value) was kept as text. A formula without either is marked [Unavailable ODS formula result].");
            let text = if !draft.text.is_empty() { draft.text.as_str() } else if let Some(raw) = raw { raw } else if has_formula { "[Unavailable ODS formula result]" } else { "[Unsupported ODS value]" };
            Value::Text(Arc::from(text))
        });
        let style = self.typed_style(draft.style.unwrap_or(StyleId::DEFAULT), format);
        Cell { style, formula: formula.map(Arc::new), ..Cell::value(value) }
    }

    fn finish_row(&mut self) -> Result<(), IoError> {
        let Some(row) = self.row.take() else { return Ok(()) };
        let Some(sheet) = self.sheet.as_mut() else { return Ok(()) };
        // Blank trailing row repeats past the sheet edge (e.g. from larger-sheet writers) are clipped.
        let has_data = row.cells.iter().any(|c| has_content(&c.cell) || c.width > 1 || c.height > 1);
        let end = match sheet.row.checked_add(row.repeat).filter(|n| *n <= MAX_ROWS) {
            Some(end) => end,
            None if !has_data => MAX_ROWS,
            None => return Err(IoError::TooLarge("ODS exceeds worksheet height".into())),
        };
        let run = end - sheet.row;
        // Heights and visibility of short runs; long blank runs are the writer's padding.
        let height = row.height.filter(|h| (h - sheet.sheet.default_row_height).abs() > 0.5);
        if (height.is_some() || row.hidden) && run <= MAX_LINE_RUN {
            for r in sheet.row..end {
                let info = sheet.sheet.rows.entry(r).or_default();
                if height.is_some() {
                    info.size = height;
                    info.custom = true;
                }
                info.hidden = row.hidden;
            }
        }
        for entry in row.cells {
            let blank = !has_content(&entry.cell) && entry.width == 1 && entry.height == 1;
            let count = u64::from(entry.repeat) * u64::from(run);
            // Formatting spread over a large empty area (whole rows or columns) is skipped.
            if blank && count > u64::from(MAX_LINE_RUN) * 10 {
                self.report.warn("Formatting of some large empty ODS areas was not imported.");
                continue;
            }
            self.cells = self.cells.saturating_add(count);
            if let Value::Text(t) = &entry.cell.value {
                self.text = self.text.saturating_add(count.saturating_mul(t.len() as u64));
            }
            let merged = entry.width > 1 || entry.height > 1;
            if merged {
                self.merges = self.merges.saturating_add(count);
            }
            if self.cells > MAX_CELLS || self.merges > MAX_MERGES || self.text > MAX_TEXT {
                return Err(IoError::TooLarge("ODS expands beyond the import limit (1 million cells, 100000 merges or 64 MiB of text)".into()));
            }
            let last_col = entry.col.checked_add(entry.repeat - 1).and_then(|c| c.checked_add(entry.width - 1));
            let last_row = end.checked_add(entry.height - 1);
            if last_col.is_none_or(|c| c >= MAX_COLS) || last_row.is_none_or(|r| r > MAX_ROWS) {
                return Err(IoError::TooLarge("ODS merged cell exceeds worksheet bounds".into()));
            }
            if merged && ((entry.repeat > 1 && entry.width > 1) || (run > 1 && entry.height > 1)) {
                return Err(IoError::Format("repeated ODS cells have overlapping merges".into()));
            }
            for c in entry.col..entry.col + entry.repeat {
                for r in sheet.row..end {
                    let pos = CellRef::new(r, c);
                    if !entry.cell.is_blank() {
                        let mut placed = entry.cell.clone();
                        if let Some(f) = placed.formula.as_mut()
                            && let Some(a) = f.array
                        {
                            let end = CellRef::new(
                                pos.row.saturating_add(a.end.row).min(MAX_ROWS - 1),
                                pos.col.saturating_add(a.end.col).min(MAX_COLS - 1),
                            );
                            Arc::make_mut(f).array = Some(RangeRef::new(pos, end));
                        }
                        sheet.sheet.cells.set(pos, placed);
                    }
                    if merged {
                        sheet.sheet.merges.push(RangeRef::new(pos, CellRef::new(r + entry.height - 1, c + entry.width - 1)));
                    }
                }
            }
        }
        sheet.row = end;
        Ok(())
    }

    /// Adds the named ranges once every sheet exists, and points formulas and names at renamed sheets.
    fn finish_names(&mut self) {
        let renames = std::mem::take(&mut self.renames);
        if !renames.is_empty() {
            for sheet in &mut self.wb.sheets {
                let formulas: Vec<(CellRef, Arc<Formula>)> = sheet.cells.iter().filter_map(|(pos, c)| c.formula.clone().map(|f| (pos, f))).collect();
                if formulas.is_empty() {
                    continue;
                }
                let sheet = Arc::make_mut(sheet);
                for (pos, f) in formulas {
                    let Some(expr) = f.expr() else { continue };
                    let mut renamed = Formula::from_expr(rename_sheets(expr, &renames));
                    renamed.array = f.array;
                    if let Some(cell) = sheet.cells.get_mut(pos) {
                        cell.formula = Some(Arc::new(renamed));
                    }
                }
            }
            for (_, _, formula) in &mut self.names {
                if let Ok(expr) = gridcraft_formula::parse(formula) {
                    *formula = gridcraft_formula::print(&rename_sheets(expr, &renames));
                }
            }
        }
        for (name, scope, formula) in std::mem::take(&mut self.names) {
            if self.wb.names.iter().any(|n| n.name.eq_ignore_ascii_case(&name) && n.scope == scope) {
                continue;
            }
            self.wb.names.push(DefinedName {
                name,
                scope: scope.filter(|s| *s < self.wb.sheets.len()),
                formula,
                comment: String::new(),
                hidden: false,
            });
        }
    }
}

/// Applies the column runs: the widest-spread width becomes the default, the others are
/// stored per column; default cell styles become column styles.
fn finish_columns(draft: SheetDraft) -> Sheet {
    let mut sheet = draft.sheet;
    let mut spread: Vec<(u32, u64)> = Vec::new();
    for (_, n, w, ..) in &draft.columns {
        if let Some(w) = w {
            let key = w.round() as u32;
            match spread.iter_mut().find(|(k, _)| *k == key) {
                Some(e) => e.1 += u64::from(*n),
                None => spread.push((key, u64::from(*n))),
            }
        }
    }
    // Writers pad to their own sheet edge with one long run of default columns.
    if let Some((w, n)) = spread.iter().max_by_key(|(_, n)| *n).copied()
        && n > u64::from(MAX_LINE_RUN)
        && w > 0
    {
        sheet.default_col_width = w as f32;
    }
    for (start, n, w, hidden, style) in draft.columns {
        let width = w.filter(|w| (w - sheet.default_col_width).abs() > 0.5);
        if width.is_none() && !hidden && style.is_none() {
            continue;
        }
        if n > MAX_LINE_RUN && !hidden && style.is_none() {
            continue;
        }
        for c in start..start.saturating_add(n.min(MAX_COLS)).min(MAX_COLS) {
            let info = sheet.cols.entry(c).or_default();
            info.size = width;
            info.hidden = hidden;
            info.style = style;
        }
    }
    sheet
}

/// References to renamed sheets, in one pass (a stored name may equal another sheet's file name).
fn rename_sheets(e: gridcraft_formula::Expr, renames: &[(String, String)]) -> gridcraft_formula::Expr {
    use gridcraft_formula::{Expr, SheetSel};
    let find = |n: &str| renames.iter().find(|(file, _)| file.eq_ignore_ascii_case(n)).map(|(_, stored)| stored.clone());
    e.map(&mut |x| match x {
        Expr::Ref(mut r) => {
            match &mut r.sheet {
                SheetSel::Named(n) => {
                    if let Some(s) = find(n) {
                        *n = s;
                    }
                }
                SheetSel::Span(a, b) => {
                    if let Some(s) = find(a) {
                        *a = s;
                    }
                    if let Some(s) = find(b) {
                        *b = s;
                    }
                }
                SheetSel::Current => {}
            }
            Expr::Ref(r)
        }
        other => other,
    })
}

/// A value or a formula (a style alone is formatting, not content).
fn has_content(cell: &Cell) -> bool {
    !cell.value.is_empty() || cell.formula.is_some()
}

fn push_text(out: &mut String, text: &str) -> Result<(), IoError> {
    if out.len().saturating_add(text.len()) > MAX_CELL_TEXT {
        return Err(IoError::TooLarge("ODS cell text exceeds 1 MiB".into()));
    }
    out.push_str(text);
    Ok(())
}

/// Support unzoned ISO calendar dates and times. A timezone, invalid date or out-of-range
/// calendar year remains text; never silently discard an offset or roll an invalid date.
fn parse_date(s: &str) -> Option<(f64, &'static str)> {
    let (date, time) = s.split_once('T').map_or((s, None), |(d, t)| (d, Some(t)));
    if date.len() != 10 || date.get(4..5)? != "-" || date.get(7..8)? != "-" {
        return None;
    }
    let year: i32 = digits(date.get(..4)?)?.parse().ok()?;
    let month: u32 = digits(date.get(5..7)?)?.parse().ok()?;
    let day: u32 = digits(date.get(8..10)?)?.parse().ok()?;
    if !(1900..=9999).contains(&year) || !(1..=12).contains(&month) || day == 0 || day > days_in_month(year, month) {
        return None;
    }
    let day = serial_from_ymd(DateSystem::D1900, year.into(), month.into(), day.into())?;
    let Some(time) = time else { return Some((day, "yyyy-mm-dd")) };
    let mut parts = time.split(':');
    let h = parts.next()?;
    let m = parts.next()?;
    let sec = parts.next()?;
    if parts.next().is_some() || h.len() != 2 || m.len() != 2 || sec.split('.').next()?.len() != 2 {
        return None;
    }
    let h: u32 = digits(h)?.parse().ok()?;
    let m: u32 = digits(m)?.parse().ok()?;
    let sec = decimal(sec)?;
    if h >= 24 || m >= 60 || sec >= 60.0 {
        return None;
    }
    Some((day + (h as f64 * 3600.0 + m as f64 * 60.0 + sec) / 86400.0, "yyyy-mm-dd hh:mm:ss.000"))
}

fn digits(s: &str) -> Option<&str> {
    (!s.is_empty() && s.bytes().all(|b| b.is_ascii_digit())).then_some(s)
}
fn decimal(s: &str) -> Option<f64> {
    match s.split_once('.') {
        Some((a, b)) => {
            digits(a)?;
            digits(b)?;
        }
        None => {
            digits(s)?;
        }
    }
    s.parse::<f64>().ok().filter(|n| n.is_finite())
}

/// A bounded ISO day/time duration. Calendar years/months cannot be represented as days.
fn parse_duration(s: &str) -> Option<f64> {
    let (negative, s) = s.strip_prefix('-').map_or((false, s), |s| (true, s));
    let mut rest = s.strip_prefix('P')?;
    let mut total = 0.0;
    let mut any = false;
    if let Some((days, tail)) = rest.split_once('D') {
        total += digits(days)?.parse::<f64>().ok()? * 86400.0;
        rest = tail;
        any = true;
    }
    if let Some(mut time) = rest.strip_prefix('T') {
        let mut time_any = false;
        for (unit, factor) in [('H', 3600.0), ('M', 60.0), ('S', 1.0)] {
            if let Some((n, tail)) = time.split_once(unit) {
                let value = if unit == 'S' { decimal(n)? } else { digits(n)?.parse::<f64>().ok()? };
                total += value * factor;
                time = tail;
                time_any = true;
            }
        }
        if !time.is_empty() || !time_any {
            return None;
        }
        any = true;
    } else if !rest.is_empty() {
        return None;
    }
    let days = total / 86400.0;
    (any && days.is_finite() && days <= 2_958_465.0).then_some(if negative { -days } else { days })
}
