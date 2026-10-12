//! Cached values of linked workbooks (`xl/externalLinks/externalLinkN.xml`).
//!
//! A formula reaching into another workbook (`[1]Prices!B2`, `'[1]P&L'!A7:A99`) names it by its
//! position in `<externalReferences>`. Excel stores the values it last read from each linked
//! workbook in the link part (`<externalBook><sheetDataSet>`) and calculates with them while the
//! source is closed. The parts themselves are carried through unchanged by `passthrough`.

use gridcraft_core::{CellError, CellRef, MAX_COLS, MAX_ROWS, Value};
use gridcraft_model::{ExternalBook, ExternalName, ExternalSheet};

use crate::IoError;
use crate::package::Rel;
use crate::read::Ctx;
use crate::xml::{self, El};

/// One book per `<externalReference>`, in order; a link that can't be read (or isn't to a
/// workbook: DDE, OLE) is an empty book so that the later ones keep their positions.
pub(crate) fn read(cx: &mut Ctx<'_>, wb_xml: &El, wb_rels: &[Rel]) -> Result<Vec<ExternalBook>, IoError> {
    let Some(refs) = wb_xml.child("externalReferences") else { return Ok(vec![]) };
    let mut books = vec![];
    for r in refs.kids("externalReference") {
        let target = r.attr("id").and_then(|id| wb_rels.iter().find(|x| x.id == id)).map(|x| x.target.clone());
        let book = match target {
            Some(t) => cx.optional_xml(&t)?.as_ref().and_then(|x| x.child("externalBook")).map(book).unwrap_or_default(),
            None => ExternalBook::default(),
        };
        books.push(book);
    }
    Ok(books)
}

fn book(x: &El) -> ExternalBook {
    let mut sheets: Vec<ExternalSheet> = x
        .child("sheetNames")
        .map(|n| {
            n.kids("sheetName")
                .map(|s| ExternalSheet { name: s.attr("val").map(xml::decode_xstring).unwrap_or_default(), ..Default::default() })
                .collect()
        })
        .unwrap_or_default();
    for data in x.child("sheetDataSet").into_iter().flat_map(|d| d.kids("sheetData")) {
        let Some(sheet) = data.attr_u32("sheetId").and_then(|i| sheets.get_mut(i as usize)) else { continue };
        sheet.refresh_error = data.flag("refreshError", false);
        for c in data.kids("row").flat_map(|r| r.kids("cell")) {
            let Some(pos) = c.attr("r").and_then(CellRef::parse).filter(|p| p.row < MAX_ROWS && p.col < MAX_COLS) else { continue };
            let v = value(c);
            if !v.is_empty() {
                sheet.cells.push((pos, v));
            }
        }
        sheet.cells.sort_by_key(|(p, _)| *p);
        sheet.cells.dedup_by_key(|(p, _)| *p);
    }
    let names = x
        .child("definedNames")
        .into_iter()
        .flat_map(|d| d.kids("definedName"))
        .filter_map(|d| {
            let refers_to = d.attr("refersTo")?;
            Some(ExternalName {
                name: d.attr("name")?.to_string(),
                sheet: d.attr_u32("sheetId").map(|i| i as usize),
                refers_to: refers_to.strip_prefix('=').unwrap_or(refers_to).to_string(),
            })
        })
        .collect();
    ExternalBook { sheets, names }
}

fn value(c: &El) -> Value {
    let Some(v) = c.child("v").map(|v| v.text.as_str()) else { return Value::Empty };
    match c.attr("t").unwrap_or("n") {
        "str" | "s" | "inlineStr" => Value::text(xml::decode_xstring(v)),
        "b" => Value::Bool(matches!(v.trim(), "1" | "true" | "TRUE")),
        "e" => Value::Error(CellError::parse(v.trim()).unwrap_or(CellError::Value)),
        _ => match v.trim().parse::<f64>() {
            Ok(n) if n.is_finite() => Value::Number(if n == 0.0 { 0.0 } else { n }),
            _ => Value::Empty,
        },
    }
}
