//! Scripts (Automate tab) in an XLSX package.
//!
//! Excel keeps Office Scripts outside the workbook, so the file format has no place for them.
//! GridCraft stores a workbook's scripts in one Custom XML Data Storage part (`customXml/itemN.xml`
//! with its `itemPropsN.xml`, ECMA-376 Part 1 §22.5) in its own namespace:
//!
//! ```xml
//! <scripts xmlns="urn:gridcraft:scripts:1">
//!   <script name="Format header" language="rhai">fn main(workbook) { … }</script>
//! </scripts>
//! ```
//!
//! Custom XML parts are application data that spreadsheet applications carry along without
//! interpreting them, and the code is plain text in the part. Characters XML can't hold use the
//! `_xHHHH_` escapes of cell text; carriage returns are written as character references so line
//! endings survive.

use std::fmt::Write as _;

use gridcraft_model::{Script, ScriptLanguage, Workbook};

use crate::package::{Rel, normalize};
use crate::read::Ctx;
use crate::write::{Out, Rels, XML_DECL};
use crate::xml::{self, esc, esc_attr};

/// Namespace of the scripts part.
pub const NS: &str = "urn:gridcraft:scripts:1";
const NS_DATASTORE: &str = "http://schemas.openxmlformats.org/officeDocument/2006/customXml";
const CT_PROPS: &str = "application/vnd.openxmlformats-officedocument.customXmlProperties+xml";
/// The data store item id of the scripts part (one per workbook, so a fixed id).
const ITEM_ID: &str = "{6D3E2A71-5B8C-4F0D-9A27-C41E8B5F2D63}";
/// Largest scripts part read.
const MAX_PART: u64 = 64 * 1024 * 1024;
/// Most scripts read from one workbook.
pub const MAX_SCRIPTS: usize = 10_000;
/// Longest script name kept.
const MAX_NAME: usize = 255;

/// The scripts stored in the package, and the custom XML parts that held them (normalised part
/// names), which the pass-through of unmodelled content must not keep a second copy of.
pub(crate) fn read(cx: &mut Ctx<'_>, wb_rels: &[Rel]) -> (Vec<Script>, Vec<String>) {
    let mut scripts: Vec<Script> = Vec::new();
    let mut parts = Vec::new();
    for r in wb_rels.iter().filter(|r| r.kind == "customXml" && !r.external) {
        let bytes = match cx.pkg.read_limited(&r.target, MAX_PART) {
            Ok(Some(b)) => b,
            Ok(None) => continue,
            Err(e) => {
                // Only a part that is ours is worth a warning; others are kept or reported by the
                // pass-through.
                if part_is_ours(cx, &r.target) {
                    cx.warn(format!("scripts could not be read and were dropped: {e}"));
                    parts.push(normalize(&r.target));
                }
                continue;
            }
        };
        if !contains(&bytes, NS.as_bytes()) {
            continue;
        }
        let root = match xml::parse(&bytes) {
            Ok(root) if root.name == "scripts" => root,
            Ok(_) => continue,
            Err(e) => {
                cx.warn(format!("scripts could not be read and were dropped: {e}"));
                parts.push(normalize(&r.target));
                continue;
            }
        };
        parts.push(normalize(&r.target));
        for el in root.kids("script") {
            if scripts.len() >= MAX_SCRIPTS {
                cx.warn(format!("only the first {MAX_SCRIPTS} scripts were read"));
                break;
            }
            let name: String = el.attr("name").unwrap_or("").trim().chars().take(MAX_NAME).collect();
            if name.is_empty() || scripts.iter().any(|s| s.name.eq_ignore_ascii_case(&name)) {
                cx.warn(format!("a script with a blank or repeated name (\"{name}\") was dropped"));
                continue;
            }
            let language = match el.attr("language") {
                Some("commands") => ScriptLanguage::Commands,
                _ => ScriptLanguage::Rhai,
            };
            scripts.push(Script { name, language, code: xml::decode_xstring(&el.text) });
        }
    }
    (scripts, parts)
}

/// Whether a part that couldn't be read whole starts like a scripts part.
fn part_is_ours(cx: &mut Ctx<'_>, part: &str) -> bool {
    cx.pkg.read_limited(part, 4096).ok().flatten().is_some_and(|b| contains(&b, NS.as_bytes()))
}

fn contains(hay: &[u8], needle: &[u8]) -> bool {
    !needle.is_empty() && hay.windows(needle.len()).any(|w| w == needle)
}

/// Writes the workbook's scripts (nothing when it has none).
pub(crate) fn write(wb: &Workbook, out: &mut Out, wb_rels: &mut Rels) {
    if wb.scripts.is_empty() {
        return;
    }
    // Kept custom XML parts keep their names: take the first free item number.
    let free = |out: &Out, n: u32| {
        !out.has_part(&format!("customXml/item{n}.xml"))
            && !out.has_part(&format!("customXml/itemProps{n}.xml"))
            && !out.has_part(&format!("customXml/_rels/item{n}.xml.rels"))
    };
    let Some(n) = (1..=100_000u32).find(|n| free(out, *n)) else { return };
    let mut x = format!("{XML_DECL}<scripts xmlns=\"{NS}\">");
    for s in &wb.scripts {
        let language = match s.language {
            ScriptLanguage::Rhai => "rhai",
            ScriptLanguage::Commands => "commands",
        };
        let code = esc(&xml::encode_xstring(&s.code)).replace('\r', "&#13;");
        let _ = write!(x, "<script name=\"{}\" language=\"{language}\">{code}</script>", esc_attr(&s.name));
    }
    x.push_str("</scripts>");
    let props = format!(
        "{XML_DECL}<ds:datastoreItem ds:itemID=\"{ITEM_ID}\" xmlns:ds=\"{NS_DATASTORE}\"><ds:schemaRefs><ds:schemaRef ds:uri=\"{NS}\"/></ds:schemaRefs></ds:datastoreItem>"
    );
    out.part(&format!("customXml/item{n}.xml"), None, x.into_bytes());
    out.part(&format!("customXml/itemProps{n}.xml"), Some(CT_PROPS), props.into_bytes());
    let mut rels = Rels::default();
    rels.add("customXmlProps", &format!("itemProps{n}.xml"));
    out.part(&format!("customXml/_rels/item{n}.xml.rels"), None, rels.xml().into_bytes());
    wb_rels.add("customXml", &format!("../customXml/item{n}.xml"));
}
