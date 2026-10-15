//! DrawingML spreadsheet drawings (`xl/drawings/drawingN.xml`): anchors, pictures, shapes and
//! chart frames.

use std::fmt::Write as _;

use gridcraft_core::{CellRef, MAX_COLS, MAX_ROWS};
use gridcraft_model::{
    Anchor, AnchorMode, Color, Image, Ink, InkEffect, InkTool, Shape, ShapeFormat, ShapeKind, ShapeTextAlign, ShapeTextAnchor, Sheet, Theme,
};

use crate::IoError;
use crate::read::Ctx;
use crate::tables::EMU_PER_PX;
use crate::xml::{El, esc, esc_attr};

mod ink;

// ---------------------------------------------------------------- reading

fn emu_px(v: Option<i64>) -> f32 {
    v.map(|e| (e as f64 / EMU_PER_PX).clamp(-1e7, 1e7) as f32).unwrap_or(0.0)
}

fn marker(e: &El) -> (CellRef, f32, f32) {
    let num = |n: &str| e.child(n).and_then(|c| c.text.trim().parse::<i64>().ok());
    let col = num("col").unwrap_or(0).clamp(0, MAX_COLS as i64 - 1) as u32;
    let row = num("row").unwrap_or(0).clamp(0, MAX_ROWS as i64 - 1) as u32;
    (CellRef::new(row, col), emu_px(num("colOff")).max(0.0), emu_px(num("rowOff")).max(0.0))
}

fn ext_px(e: Option<&El>) -> (f32, f32) {
    match e {
        Some(x) => (emu_px(x.attr_i64("cx")).max(0.0), emu_px(x.attr_i64("cy")).max(0.0)),
        None => (0.0, 0.0),
    }
}

fn read_anchor(a: &El, sheet: &Sheet) -> Option<Anchor> {
    match a.name.as_str() {
        "twoCellAnchor" => {
            let (cell, dx, dy) = marker(a.child("from")?);
            let (to, tdx, tdy) = a.child("to").map(marker).unwrap_or((cell, dx, dy));
            let x1 = sheet.col_left(cell.col) + dx as f64;
            let y1 = sheet.row_top(cell.row) + dy as f64;
            let x2 = sheet.col_left(to.col) + tdx as f64;
            let y2 = sheet.row_top(to.row) + tdy as f64;
            // Excel stores "Move and size with cells" as a plain twoCellAnchor, "Move but don't
            // size" as editAs="oneCell", and "Don't move or size" as editAs="absolute".
            let mode = match a.attr("editAs") {
                Some("absolute") => AnchorMode::Absolute,
                Some("oneCell") => AnchorMode::MoveOnly,
                _ => AnchorMode::MoveAndSize,
            };
            Some(Anchor { cell, dx, dy, width: (x2 - x1).max(0.0) as f32, height: (y2 - y1).max(0.0) as f32, mode })
        }
        "oneCellAnchor" => {
            let (cell, dx, dy) = marker(a.child("from")?);
            let (width, height) = ext_px(a.child("ext"));
            Some(Anchor { cell, dx, dy, width, height, mode: AnchorMode::MoveOnly })
        }
        "absoluteAnchor" => {
            let pos = a.child("pos");
            let x = emu_px(pos.and_then(|p| p.attr_i64("x"))).max(0.0) as f64;
            let y = emu_px(pos.and_then(|p| p.attr_i64("y"))).max(0.0) as f64;
            let col = sheet.col_at(x);
            let row = sheet.row_at(y);
            let (width, height) = ext_px(a.child("ext"));
            Some(Anchor {
                cell: CellRef::new(row, col),
                dx: (x - sheet.col_left(col)).max(0.0) as f32,
                dy: (y - sheet.row_top(row)).max(0.0) as f32,
                width,
                height,
                mode: AnchorMode::Absolute,
            })
        }
        _ => None,
    }
}

/// Colour of a DrawingML colour choice inside `e` (e.g. a `solidFill`).
pub fn dml_color(e: &El) -> Option<Color> {
    if let Some(c) = e.child("srgbClr").and_then(|c| c.attr("val")) {
        return Color::from_hex(c);
    }
    if let Some(c) = e.child("sysClr").and_then(|c| c.attr("lastClr")) {
        return Color::from_hex(c);
    }
    if let Some(c) = e.child("schemeClr").and_then(|c| c.attr("val")) {
        let i = match c {
            "bg1" | "lt1" => 0,
            "tx1" | "dk1" => 1,
            "bg2" | "lt2" => 2,
            "tx2" | "dk2" => 3,
            "accent1" => 4,
            "accent2" => 5,
            "accent3" => 6,
            "accent4" => 7,
            "accent5" => 8,
            "accent6" => 9,
            "hlink" => 10,
            "folHlink" => 11,
            _ => return None,
        };
        return Some(Color::Theme(i, 0));
    }
    None
}

fn mime_for(name: &str) -> &'static str {
    let ext = name.rsplit('.').next().unwrap_or("").to_ascii_lowercase();
    match ext.as_str() {
        "png" => "image/png",
        "jpg" | "jpeg" | "jpe" => "image/jpeg",
        "gif" => "image/gif",
        "bmp" => "image/bmp",
        "tif" | "tiff" => "image/tiff",
        "svg" => "image/svg+xml",
        "emf" => "image/x-emf",
        "wmf" => "image/x-wmf",
        "webp" => "image/webp",
        _ => "application/octet-stream",
    }
}

pub fn read_drawing(cx: &mut Ctx<'_>, part: &str, sheet: &mut Sheet) -> Result<(), IoError> {
    let Some(x) = cx.optional_xml(part)? else { return Ok(()) };
    let rels = cx.pkg.rels(part)?;
    for a in &x.children {
        let Some(anchor) = read_anchor(a, sheet) else { continue };
        let mut objs = Vec::new();
        for obj in &a.children {
            if obj.name != "AlternateContent" {
                objs.push(obj);
                continue;
            }
            let mut imported = false;
            for choice in obj.kids("Choice") {
                if let Some(content) = choice.child("contentPart") {
                    imported = read_ink_part(cx, content, anchor, &rels, sheet)?;
                    if imported {
                        break;
                    }
                } else if choice.child("graphicFrame").is_some() {
                    objs.extend(&choice.children);
                    imported = true;
                    break;
                }
            }
            if !imported && let Some(fallback) = obj.child("Fallback") {
                objs.extend(&fallback.children);
            }
        }
        for obj in objs {
            match obj.name.as_str() {
                "contentPart" => {
                    read_ink_part(cx, obj, anchor, &rels, sheet)?;
                }
                "pic" => {
                    let Some(rid) = obj.path(&["blipFill", "blip"]).and_then(|b| b.attr("embed")) else { continue };
                    let Some(rel) = rels.iter().find(|r| r.id == rid && !r.external) else {
                        cx.warn("a linked (not embedded) picture was skipped");
                        continue;
                    };
                    let target = rel.target.clone();
                    let Some(data) = cx.pkg.read(&target)? else {
                        cx.warn(format!("missing picture {target}"));
                        continue;
                    };
                    let alt = obj.path(&["nvPicPr", "cNvPr"]).and_then(|c| c.attr("descr")).unwrap_or("").to_string();
                    let id = cx.next_object_id();
                    sheet.images.push(Image { id, anchor, data, mime: mime_for(&target).to_string(), alt });
                }
                "graphicFrame" => {
                    let Some(gd) = obj.path(&["graphic", "graphicData"]) else { continue };
                    let uri = gd.attr("uri").unwrap_or("");
                    if let Some(rid) = gd.child("chart").and_then(|c| c.attr("id")) {
                        let Some(rel) = rels.iter().find(|r| r.id == rid) else { continue };
                        let target = rel.target.clone();
                        if let Some(cx_xml) = cx.optional_xml(&target)? {
                            let chartex = uri.contains("chartex");
                            match if chartex { crate::chartex::read_chartex(&cx_xml) } else { crate::chart::read_chart(&cx_xml) } {
                                Some(mut ch) => {
                                    ch.id = cx.next_object_id();
                                    ch.anchor = anchor;
                                    sheet.charts.push(ch);
                                }
                                None => cx.warn(format!("chart {target} has no supported plot and was skipped")),
                            }
                        }
                    } else if uri.contains("chartex") || uri.contains("chartEx") {
                        cx.warn("a histogram, waterfall, treemap… chart without its chart part was skipped");
                    } else {
                        cx.warn("an embedded object (SmartArt, OLE…) was skipped");
                    }
                }
                "sp" | "cxnSp" => {
                    if let Some(s) = read_shape(obj, anchor) {
                        let id = cx.next_object_id();
                        sheet.shapes.push(Shape { id, ..s });
                    }
                }
                "grpSp" => cx.warn("grouped shapes are not supported and were skipped"),
                _ => {}
            }
        }
    }
    Ok(())
}

/// Resolve only internal package parts; never follow an InkML reference onto the network.
fn read_ink_part(cx: &mut Ctx<'_>, content: &El, anchor: Anchor, rels: &[crate::package::Rel], sheet: &mut Sheet) -> Result<bool, IoError> {
    let Some(rel) = content.attr("id").and_then(|id| rels.iter().find(|r| r.id == id && !r.external && r.kind == "customXml")) else {
        cx.warn("ink content has no internal relationship; retained image fallback");
        return Ok(false);
    };
    let target = &rel.target;
    let parsed = cx.pkg.read_limited(target, 8 * 1024 * 1024).and_then(|data| data.map(|b| crate::inkml::parse(&b)).transpose());
    let mut ink_anchor = anchor;
    if let Some(transform) = content.child("xfrm") {
        if transform.attr_i64("rot").unwrap_or(0) != 0 || transform.flag("flipH", false) || transform.flag("flipV", false) {
            cx.warn("unsupported ink transform; retained image fallback");
            return Ok(false);
        }
        let Some(off) = transform.child("off") else { return Ok(false) };
        let Some((x, y)) = off.attr_i64("x").zip(off.attr_i64("y")) else { return Ok(false) };
        if x < 0 || y < 0 {
            return Ok(false);
        }
        let x = emu_px(Some(x)) as f64;
        let y = emu_px(Some(y)) as f64;
        let col = sheet.col_at(x);
        let row = sheet.row_at(y);
        let (width, height) = ext_px(transform.child("ext"));
        ink_anchor = Anchor {
            cell: CellRef::new(row, col),
            dx: (x - sheet.col_left(col)) as f32,
            dy: (y - sheet.row_top(row)) as f32,
            width,
            height,
            mode: anchor.mode,
        };
    }
    match parsed.ok().flatten().and_then(|root| crate::inkml::read(&root, ink_anchor)) {
        Some(shapes) if !ink_budget(cx, &shapes) => {
            cx.warn("too much ink in the workbook; retained image fallback");
            Ok(false)
        }
        Some(shapes) => {
            for mut shape in shapes {
                let x = sheet.col_left(shape.anchor.cell.col) + shape.anchor.dx as f64;
                let y = sheet.row_top(shape.anchor.cell.row) + shape.anchor.dy as f64;
                // Ink can reach left of column A or above row 1: the anchor stays on the sheet and
                // the points keep their place, else the next save drops the negative offset.
                if (x < 0.0 || y < 0.0)
                    && let Some(ink) = shape.ink.as_mut()
                {
                    let (sx, sy) = (x.min(0.0) as f32, y.min(0.0) as f32);
                    for p in &mut ink.points {
                        p[0] += sx;
                        p[1] += sy;
                    }
                }
                let (x, y) = (x.max(0.0), y.max(0.0));
                let col = sheet.col_at(x);
                let row = sheet.row_at(y);
                shape.anchor.cell = CellRef::new(row, col);
                shape.anchor.dx = (x - sheet.col_left(col)) as f32;
                shape.anchor.dy = (y - sheet.row_top(row)) as f32;
                shape.id = cx.next_object_id();
                sheet.shapes.push(shape);
            }
            Ok(true)
        }
        None => {
            cx.warn(format!("unsupported or damaged InkML {target}; retained image fallback"));
            Ok(false)
        }
    }
}

/// Most ink points one workbook may bring in (many anchors can share one ink part).
const MAX_WORKBOOK_INK_POINTS: usize = 2_000_000;

/// Counts `shapes`' points against the workbook's ink budget; false when they don't fit.
fn ink_budget(cx: &mut Ctx<'_>, shapes: &[Shape]) -> bool {
    let n = shapes.iter().filter_map(|s| s.ink.as_ref()).map(|i| i.points.len()).sum::<usize>();
    let total = cx.ink_points.saturating_add(n);
    if total > MAX_WORKBOOK_INK_POINTS {
        return false;
    }
    cx.ink_points = total;
    true
}

/// A stroke GridCraft wrote as a DrawingML shape: the `urn:gridcraft:ink:v1` extension keeps
/// the centerline, force and pen; without it, the path itself is the centerline.
pub(crate) fn read_gridcraft_ink(sp: &El, anchor: Anchor) -> Option<Shape> {
    let meta = sp.child("extLst").and_then(|l| l.kids("ext").find(|e| e.attr("uri") == Some("urn:gridcraft:ink:v1"))).and_then(|e| e.child("ink"));
    if let Some(meta) = meta {
        let width = meta.attr_f64("width").filter(|w| *w > 0.0 && *w <= 40.0)? as f32;
        let mut ink = Ink {
            width,
            tool: meta.attr("tool").and_then(InkTool::parse).unwrap_or_default(),
            effect: meta.attr("effect").and_then(InkEffect::parse),
            ..Ink::default()
        };
        let force = meta.flag("pressure", true);
        for point in meta.kids("p").take(gridcraft_model::MAX_INK_POINTS) {
            let p = [point.attr_f64("x")?, point.attr_f64("y")?];
            if !p.iter().all(|v| v.is_finite() && v.abs() <= gridcraft_model::MAX_INK_COORD as f64) {
                return None;
            }
            ink.points.push([p[0] as f32, p[1] as f32]);
            if force {
                ink.pressures.push(point.attr_f64("force").filter(|p| (0.0..=1.0).contains(p))? as f32);
            }
        }
        let line = sp.child("solidFill").or_else(|| sp.path(&["ln", "solidFill"])).and_then(dml_color).unwrap_or(Color::Rgb(0));
        return (ink.points.len() >= 2).then(|| Shape::new_ink(0, anchor, line, ink));
    }
    let path = sp.path(&["custGeom", "pathLst", "path"])?;
    let w = path.attr_i64("w").filter(|v| *v > 0)? as f64;
    let h = path.attr_i64("h").filter(|v| *v > 0)? as f64;
    let mut points = Vec::new();
    for command in path.children.iter().take(gridcraft_model::MAX_INK_POINTS) {
        if !matches!(command.name.as_str(), "moveTo" | "lnTo") {
            return None;
        }
        let pt = command.child("pt")?;
        let x = pt.attr_i64("x")? as f64 / w * anchor.width as f64;
        let y = pt.attr_i64("y")? as f64 / h * anchor.height as f64;
        points.push([x as f32, y as f32]);
    }
    let ln = sp.child("ln");
    let width = ln.and_then(|l| l.attr_i64("w")).map(|v| (v as f64 / EMU_PER_PX).clamp(0.1, 40.0) as f32).unwrap_or(2.0);
    let line = ln.and_then(|l| l.child("solidFill")).and_then(dml_color).unwrap_or(Color::Rgb(0));
    (points.len() >= 2).then(|| Shape::new_ink(0, anchor, line, Ink { points, width, ..Ink::default() }))
}

fn read_shape(obj: &El, anchor: Anchor) -> Option<Shape> {
    let sp = obj.child("spPr")?;
    // GridCraft's own strokes are identified by name; other custom shapes become ink only when
    // `ink::read` finds the strict polyline subset the ink model can represent.
    if obj.path(&["nvSpPr", "cNvPr"]).and_then(|n| n.attr("name")).is_some_and(|n| n.starts_with("GridCraft Ink ")) {
        return read_gridcraft_ink(sp, anchor);
    }
    if let Some(shape) = ink::read(obj, anchor) {
        return Some(shape);
    }
    let prst = sp.child("prstGeom").and_then(|g| g.attr("prst")).unwrap_or("rect");
    let tx_box = obj.path(&["nvSpPr", "cNvSpPr"]).is_some_and(|c| c.flag("txBox", false));
    let ln = sp.child("ln");
    let arrow = ln.is_some_and(|l| l.child("tailEnd").or_else(|| l.child("headEnd")).is_some_and(|t| t.attr("type").is_some_and(|v| v != "none")));
    let kind = match prst {
        _ if tx_box => ShapeKind::TextBox,
        "roundRect" => ShapeKind::RoundedRectangle,
        "ellipse" => ShapeKind::Ellipse,
        "triangle" => ShapeKind::Triangle,
        "line" | "straightConnector1" if arrow => ShapeKind::Arrow,
        "line" | "straightConnector1" => ShapeKind::Line,
        "rightArrow" => ShapeKind::Arrow,
        _ => ShapeKind::Rectangle,
    };
    let fill = sp.child("solidFill").and_then(dml_color).unwrap_or_default();
    let line = ln.and_then(|l| l.child("solidFill")).and_then(dml_color).unwrap_or_default();
    let text = obj
        .child("txBody")
        .map(|tb| {
            tb.kids("p").map(|p| p.kids("r").filter_map(|r| r.child("t")).map(|t| t.text.as_str()).collect::<String>()).collect::<Vec<_>>().join("\n")
        })
        .unwrap_or_default();
    let mut format = read_text_format(obj.child("txBody"));
    format.no_line = ln.is_some_and(|l| l.child("noFill").is_some());
    Some(Shape { id: 0, kind, anchor, fill, line, text, format, ink: None })
}

/// A shape's text formatting: vertical anchor (`a:bodyPr anchor`), the first paragraph's
/// alignment (`a:pPr algn`) and the first run's properties (`a:rPr`: size, bold, italic, fill,
/// outline, `a:latin` typeface).
fn read_text_format(body: Option<&El>) -> ShapeFormat {
    let mut f = ShapeFormat::default();
    let Some(body) = body else { return f };
    f.anchor = body.child("bodyPr").and_then(|b| b.attr("anchor")).map(|a| match a {
        "ctr" => ShapeTextAnchor::Middle,
        "b" => ShapeTextAnchor::Bottom,
        _ => ShapeTextAnchor::Top,
    });
    let first_p = body.kids("p").next();
    f.align = first_p.and_then(|p| p.child("pPr")).and_then(|p| p.attr("algn")).map(|a| match a {
        "ctr" => ShapeTextAlign::Center,
        "r" => ShapeTextAlign::Right,
        _ => ShapeTextAlign::Left,
    });
    let rpr = body.kids("p").flat_map(|p| p.kids("r")).find_map(|r| r.child("rPr"));
    if let Some(r) = rpr {
        // `sz` is in hundredths of a point (100..=400000 in the schema).
        f.font_size = r.attr_f64("sz").filter(|v| v.is_finite()).map(|v| (v / 100.0).clamp(1.0, 4000.0) as f32);
        f.bold = r.flag("b", false);
        f.italic = r.flag("i", false);
        f.font = r.child("latin").and_then(|l| l.attr("typeface")).filter(|t| !t.is_empty() && !t.starts_with('+')).map(str::to_string);
        f.text_color = r.child("solidFill").and_then(dml_color);
        f.text_outline = r.child("ln").and_then(|l| {
            let c = l.child("solidFill").and_then(dml_color)?;
            // Width in EMU (12700 per point); Excel's default hairline is 0.75 pt.
            let w = l.attr_f64("w").filter(|w| w.is_finite()).map_or(0.75, |w| (w / 12700.0).clamp(0.0, 100.0)) as f32;
            Some((c, w))
        });
    }
    f
}

// ---------------------------------------------------------------- writing

/// The opening `<xdr:twoCellAnchor>` for a model anchor, carrying Excel's `editAs` when the mode
/// is not the default: omitted for "Move and size with cells", `editAs="oneCell"` for "Move but
/// don't size", `editAs="absolute"` for "Don't move or size". (Excel writes pictures this way;
/// the standalone `oneCellAnchor`/`absoluteAnchor` elements are read on import but not written.)
/// Returns the opening tag and the matching closing tag.
fn anchor_open(sheet: &Sheet, a: &Anchor) -> (String, &'static str) {
    let px = |v: f64| ((v.max(0.0)) * EMU_PER_PX).round() as i64;
    let x1 = sheet.col_left(a.cell.col) + a.dx as f64;
    let y1 = sheet.row_top(a.cell.row) + a.dy as f64;
    let x2 = x1 + a.width.max(0.0) as f64;
    let y2 = y1 + a.height.max(0.0) as f64;
    let tc = sheet.col_at(x2);
    let tr = sheet.row_at(y2);
    let tdx = x2 - sheet.col_left(tc);
    let tdy = y2 - sheet.row_top(tr);
    let edit = match a.mode {
        AnchorMode::MoveAndSize => "",
        AnchorMode::MoveOnly => " editAs=\"oneCell\"",
        AnchorMode::Absolute => " editAs=\"absolute\"",
    };
    let open = format!(
        "<xdr:twoCellAnchor{edit}><xdr:from><xdr:col>{}</xdr:col><xdr:colOff>{}</xdr:colOff><xdr:row>{}</xdr:row><xdr:rowOff>{}</xdr:rowOff></xdr:from><xdr:to><xdr:col>{tc}</xdr:col><xdr:colOff>{}</xdr:colOff><xdr:row>{tr}</xdr:row><xdr:rowOff>{}</xdr:rowOff></xdr:to>",
        a.cell.col,
        px(a.dx as f64),
        a.cell.row,
        px(a.dy as f64),
        px(tdx),
        px(tdy)
    );
    (open, "</xdr:twoCellAnchor>")
}

fn xfrm(a: &Anchor, tag: &str) -> String {
    let e = |v: f32| ((v.max(0.0) as f64) * EMU_PER_PX).round() as i64;
    format!("<{tag}><a:off x=\"0\" y=\"0\"/><a:ext cx=\"{}\" cy=\"{}\"/></{tag}>", e(a.width), e(a.height))
}

pub fn srgb(c: &Color, theme: &Theme) -> Option<String> {
    let [r, g, b] = c.resolve(theme)?;
    Some(format!("<a:srgbClr val=\"{r:02X}{g:02X}{b:02X}\"/>"))
}

/// One drawing object to write.
pub enum Obj<'a> {
    Image(&'a Image, String),
    Chart(&'a gridcraft_model::Chart, String),
    Shape(&'a Shape),
    Ink(&'a Shape, String),
}

/// The drawing part for a sheet's objects (relationship ids already assigned).
pub fn write_drawing(sheet: &Sheet, theme: &Theme, objs: &[Obj<'_>]) -> String {
    let mut s = String::from(
        "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n<xdr:wsDr xmlns:xdr=\"http://schemas.openxmlformats.org/drawingml/2006/spreadsheetDrawing\" xmlns:a=\"http://schemas.openxmlformats.org/drawingml/2006/main\" xmlns:r=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships\" xmlns:c=\"http://schemas.openxmlformats.org/drawingml/2006/chart\">",
    );
    for (i, o) in objs.iter().enumerate() {
        let id = i + 2;
        let close_tag;
        match o {
            Obj::Image(img, rid) => {
                let (open, close) = anchor_open(sheet, &img.anchor);
                close_tag = close;
                s.push_str(&open);
                let _ = write!(
                    s,
                    "<xdr:pic><xdr:nvPicPr><xdr:cNvPr id=\"{id}\" name=\"Picture {}\" descr=\"{}\"/><xdr:cNvPicPr><a:picLocks noChangeAspect=\"1\"/></xdr:cNvPicPr></xdr:nvPicPr><xdr:blipFill><a:blip r:embed=\"{rid}\"/><a:stretch><a:fillRect/></a:stretch></xdr:blipFill><xdr:spPr>{}<a:prstGeom prst=\"rect\"><a:avLst/></a:prstGeom></xdr:spPr></xdr:pic>",
                    id - 1,
                    esc_attr(&img.alt),
                    xfrm(&img.anchor, "a:xfrm")
                );
            }
            Obj::Chart(ch, rid) if crate::chartex::is_chartex(ch.kind) => {
                // Chartex frames come with a fallback for readers that don't know them.
                let (open, close) = anchor_open(sheet, &ch.anchor);
                close_tag = close;
                s.push_str(&open);
                let (prefix, ns) = crate::chartex::requires(ch.kind);
                let _ = write!(
                    s,
                    "<mc:AlternateContent xmlns:mc=\"http://schemas.openxmlformats.org/markup-compatibility/2006\"><mc:Choice xmlns:{prefix}=\"{ns}\" Requires=\"{prefix}\"><xdr:graphicFrame macro=\"\"><xdr:nvGraphicFramePr><xdr:cNvPr id=\"{id}\" name=\"Chart {}\"/><xdr:cNvGraphicFramePr/></xdr:nvGraphicFramePr>{}<a:graphic><a:graphicData uri=\"http://schemas.microsoft.com/office/drawing/2014/chartex\"><cx:chart xmlns:cx=\"http://schemas.microsoft.com/office/drawing/2014/chartex\" r:id=\"{rid}\"/></a:graphicData></a:graphic></xdr:graphicFrame></mc:Choice><mc:Fallback><xdr:sp macro=\"\" textlink=\"\"><xdr:nvSpPr><xdr:cNvPr id=\"0\" name=\"\"/><xdr:cNvSpPr><a:spLocks noTextEdit=\"1\"/></xdr:cNvSpPr></xdr:nvSpPr><xdr:spPr>{}<a:prstGeom prst=\"rect\"><a:avLst/></a:prstGeom></xdr:spPr><xdr:txBody><a:bodyPr/><a:lstStyle/><a:p><a:r><a:rPr lang=\"en-US\"/><a:t>This chart type can't be shown by this application.</a:t></a:r></a:p></xdr:txBody></xdr:sp></mc:Fallback></mc:AlternateContent>",
                    id - 1,
                    xfrm(&ch.anchor, "xdr:xfrm"),
                    xfrm(&ch.anchor, "a:xfrm")
                );
            }
            Obj::Chart(ch, rid) => {
                let (open, close) = anchor_open(sheet, &ch.anchor);
                close_tag = close;
                s.push_str(&open);
                let _ = write!(
                    s,
                    "<xdr:graphicFrame macro=\"\"><xdr:nvGraphicFramePr><xdr:cNvPr id=\"{id}\" name=\"Chart {}\"/><xdr:cNvGraphicFramePr/></xdr:nvGraphicFramePr>{}<a:graphic><a:graphicData uri=\"http://schemas.openxmlformats.org/drawingml/2006/chart\"><c:chart r:id=\"{rid}\"/></a:graphicData></a:graphic></xdr:graphicFrame>",
                    id - 1,
                    xfrm(&ch.anchor, "xdr:xfrm")
                );
            }
            Obj::Ink(sh, rid) => {
                let (open, close) = anchor_open(sheet, &sh.anchor);
                close_tag = close;
                s.push_str(&open);
                let x = (sheet.col_left(sh.anchor.cell.col) + sh.anchor.dx as f64) * EMU_PER_PX;
                let y = (sheet.row_top(sh.anchor.cell.row) + sh.anchor.dy as f64) * EMU_PER_PX;
                let w = sh.anchor.width as f64 * EMU_PER_PX;
                let h = sh.anchor.height as f64 * EMU_PER_PX;
                let _ = write!(
                    s,
                    "<mc:AlternateContent xmlns:mc=\"http://schemas.openxmlformats.org/markup-compatibility/2006\"><mc:Choice xmlns:xdr14=\"http://schemas.microsoft.com/office/excel/2010/spreadsheetDrawing\" Requires=\"xdr14\"><xdr:contentPart r:id=\"{rid}\"><xdr14:nvContentPartPr><xdr14:cNvPr id=\"{id}\" name=\"Ink {id}\"/><xdr14:cNvContentPartPr/></xdr14:nvContentPartPr><xdr14:nvPr macro=\"\"/><xdr14:xfrm><a:off x=\"{x:.0}\" y=\"{y:.0}\"/><a:ext cx=\"{w:.0}\" cy=\"{h:.0}\"/></xdr14:xfrm></xdr:contentPart></mc:Choice><mc:Fallback>"
                );
                if let Some(ink) = &sh.ink {
                    write_ink(&mut s, sh, ink, id, theme);
                }
                s.push_str("</mc:Fallback></mc:AlternateContent>");
            }
            Obj::Shape(sh) => {
                let (open, close) = anchor_open(sheet, &sh.anchor);
                close_tag = close;
                s.push_str(&open);
                let (prst, tx) = match sh.kind {
                    ShapeKind::Rectangle => ("rect", false),
                    ShapeKind::RoundedRectangle => ("roundRect", false),
                    ShapeKind::Ellipse => ("ellipse", false),
                    ShapeKind::Triangle => ("triangle", false),
                    ShapeKind::Line | ShapeKind::Arrow => ("line", false),
                    ShapeKind::TextBox => ("rect", true),
                    ShapeKind::Icon | ShapeKind::Ink => ("rect", false),
                };
                let _ = write!(
                    s,
                    "<xdr:sp macro=\"\" textlink=\"\"><xdr:nvSpPr><xdr:cNvPr id=\"{id}\" name=\"Shape {}\"/><xdr:cNvSpPr{}/></xdr:nvSpPr><xdr:spPr>{}<a:prstGeom prst=\"{prst}\"><a:avLst/></a:prstGeom>",
                    id - 1,
                    if tx { " txBox=\"1\"" } else { "" },
                    xfrm(&sh.anchor, "a:xfrm")
                );
                if let Some(c) = srgb(&sh.fill, theme) {
                    let _ = write!(s, "<a:solidFill>{c}</a:solidFill>");
                }
                let line_c = srgb(&sh.line, theme);
                if sh.format.no_line {
                    s.push_str("<a:ln><a:noFill/></a:ln>");
                } else if line_c.is_some() || sh.kind == ShapeKind::Arrow {
                    s.push_str("<a:ln w=\"9525\">");
                    match line_c {
                        Some(c) => {
                            let _ = write!(s, "<a:solidFill>{c}</a:solidFill>");
                        }
                        None => s.push_str("<a:solidFill><a:srgbClr val=\"000000\"/></a:solidFill>"),
                    }
                    if sh.kind == ShapeKind::Arrow {
                        s.push_str("<a:tailEnd type=\"triangle\"/>");
                    }
                    s.push_str("</a:ln>");
                }
                s.push_str("</xdr:spPr>");
                if !sh.text.is_empty() {
                    let f = &sh.format;
                    let anchor = match f.anchor {
                        Some(ShapeTextAnchor::Top) => " anchor=\"t\"",
                        Some(ShapeTextAnchor::Middle) => " anchor=\"ctr\"",
                        Some(ShapeTextAnchor::Bottom) => " anchor=\"b\"",
                        None if sh.kind == ShapeKind::TextBox => " anchor=\"t\"",
                        None => "",
                    };
                    let _ = write!(s, "<xdr:txBody><a:bodyPr wrap=\"square\" rtlCol=\"0\"{anchor}/><a:lstStyle/>");
                    let ppr = match f.align {
                        None => "",
                        Some(ShapeTextAlign::Left) => "<a:pPr algn=\"l\"/>",
                        Some(ShapeTextAlign::Center) => "<a:pPr algn=\"ctr\"/>",
                        Some(ShapeTextAlign::Right) => "<a:pPr algn=\"r\"/>",
                    };
                    let (rpr_attrs, rpr_body) = run_props(f, theme);
                    for line in sh.text.split('\n') {
                        if line.is_empty() {
                            let _ = write!(s, "<a:p>{ppr}<a:endParaRPr lang=\"en-US\"{rpr_attrs}/></a:p>");
                        } else if rpr_body.is_empty() {
                            let _ = write!(s, "<a:p>{ppr}<a:r><a:rPr lang=\"en-US\"{rpr_attrs}/><a:t>{}</a:t></a:r></a:p>", esc(line));
                        } else {
                            let _ =
                                write!(s, "<a:p>{ppr}<a:r><a:rPr lang=\"en-US\"{rpr_attrs}>{rpr_body}</a:rPr><a:t>{}</a:t></a:r></a:p>", esc(line));
                        }
                    }
                    s.push_str("</xdr:txBody>");
                }
                s.push_str("</xdr:sp>");
            }
        }
        s.push_str("<xdr:clientData/>");
        s.push_str(close_tag);
    }
    s.push_str("</xdr:wsDr>");
    s
}

/// A text run's `a:rPr` attributes (size, bold, italic) and children in schema order (outline,
/// fill, typeface).
fn run_props(f: &ShapeFormat, theme: &Theme) -> (String, String) {
    let mut attrs = String::new();
    if let Some(sz) = f.font_size.filter(|v| v.is_finite()) {
        let _ = write!(attrs, " sz=\"{}\"", (sz.clamp(1.0, 4000.0) * 100.0).round() as u32);
    }
    if f.bold {
        attrs.push_str(" b=\"1\"");
    }
    if f.italic {
        attrs.push_str(" i=\"1\"");
    }
    let mut body = String::new();
    if let Some((c, w)) = &f.text_outline
        && let Some(c) = srgb(c, theme)
    {
        let emu = (w.clamp(0.0, 100.0) * 12700.0).round() as u32;
        let _ = write!(body, "<a:ln w=\"{emu}\"><a:solidFill>{c}</a:solidFill></a:ln>");
    }
    if let Some(c) = f.text_color.as_ref().and_then(|c| srgb(c, theme)) {
        let _ = write!(body, "<a:solidFill>{c}</a:solidFill>");
    }
    if let Some(font) = &f.font {
        let _ = write!(body, "<a:latin typeface=\"{}\"/>", esc(font));
    }
    (attrs, body)
}

/// File extension for an image MIME type.
pub fn image_ext(mime: &str, data: &[u8]) -> &'static str {
    match mime.to_ascii_lowercase().as_str() {
        "image/png" => "png",
        "image/jpeg" | "image/jpg" => "jpeg",
        "image/gif" => "gif",
        "image/bmp" => "bmp",
        "image/tiff" => "tiff",
        "image/svg+xml" => "svg",
        "image/x-emf" => "emf",
        "image/x-wmf" => "wmf",
        "image/webp" => "webp",
        _ => {
            if data.starts_with(b"\x89PNG") {
                "png"
            } else if data.starts_with(&[0xFF, 0xD8]) {
                "jpeg"
            } else if data.starts_with(b"GIF8") {
                "gif"
            } else {
                "png"
            }
        }
    }
}

pub fn image_content_type(ext: &str) -> &'static str {
    match ext {
        "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "bmp" => "image/bmp",
        "tiff" => "image/tiff",
        "svg" => "image/svg+xml",
        "emf" => "image/x-emf",
        "wmf" => "image/x-wmf",
        "webp" => "image/webp",
        _ => "image/png",
    }
}

/// The stroke as DrawingML for applications without InkML: the filled silhouette for
/// pressure strokes, else the centerline with a round pen. The `urn:gridcraft:ink:v1`
/// extension keeps the editable centerline, force and pen.
pub(crate) fn write_ink(s: &mut String, sh: &Shape, ink: &Ink, id: usize, theme: &Theme) {
    let emu = |v: f32| (v.clamp(-1e7, 1e7) as f64 * EMU_PER_PX).round() as i64;
    let (w, h) = (emu(sh.anchor.width).max(1), emu(sh.anchor.height).max(1));
    let [r, g, b] = sh.line.resolve(theme).unwrap_or([0, 0, 0]);
    let alpha = if ink.opacity() < 1.0 { format!("<a:alpha val=\"{}\"/>", (ink.opacity() * 100_000.0).round() as i64) } else { String::new() };
    let color = format!("<a:srgbClr val=\"{r:02X}{g:02X}{b:02X}\">{alpha}</a:srgbClr>");
    let _ = write!(
        s,
        "<xdr:sp macro=\"\" textlink=\"\"><xdr:nvSpPr><xdr:cNvPr id=\"{id}\" name=\"GridCraft Ink {id}\"/><xdr:cNvSpPr/></xdr:nvSpPr><xdr:spPr>{}<a:custGeom><a:avLst/><a:gdLst/><a:ahLst/><a:cxnLst/><a:rect l=\"0\" t=\"0\" r=\"r\" b=\"b\"/><a:pathLst>",
        xfrm(&sh.anchor, "a:xfrm")
    );
    if ink.has_pressure() {
        for t in ink.polygons() {
            let _ = write!(s, "<a:path w=\"{w}\" h=\"{h}\" fill=\"norm\" stroke=\"0\" extrusionOk=\"0\">");
            for (i, p) in t.iter().enumerate() {
                let tag = if i == 0 { "moveTo" } else { "lnTo" };
                let _ = write!(s, "<a:{tag}><a:pt x=\"{}\" y=\"{}\"/></a:{tag}>", emu(p[0]), emu(p[1]));
            }
            s.push_str("<a:close/></a:path>");
        }
        let _ = write!(s, "</a:pathLst></a:custGeom><a:solidFill>{color}</a:solidFill><a:ln><a:noFill/></a:ln>");
    } else {
        let _ = write!(s, "<a:path w=\"{w}\" h=\"{h}\" fill=\"none\" stroke=\"1\" extrusionOk=\"0\">");
        for (i, p) in ink.valid_points().enumerate() {
            let tag = if i == 0 { "moveTo" } else { "lnTo" };
            let _ = write!(s, "<a:{tag}><a:pt x=\"{}\" y=\"{}\"/></a:{tag}>", emu(p[0]), emu(p[1]));
        }
        let width = emu(ink.width());
        let _ = write!(
            s,
            "</a:path></a:pathLst></a:custGeom><a:noFill/><a:ln w=\"{width}\" cap=\"rnd\"><a:solidFill>{color}</a:solidFill><a:round/></a:ln>"
        );
    }
    let _ = write!(
        s,
        "<a:extLst><a:ext uri=\"urn:gridcraft:ink:v1\"><gc:ink xmlns:gc=\"urn:gridcraft:ink:v1\" width=\"{}\" tool=\"{}\" pressure=\"{}\"",
        ink.width(),
        ink.tool.id(),
        u8::from(ink.has_pressure())
    );
    if let Some(effect) = ink.effect {
        let _ = write!(s, " effect=\"{}\"", effect.id());
    }
    s.push('>');
    for (i, p) in ink.valid_points().enumerate() {
        let _ = write!(s, "<gc:p x=\"{:.2}\" y=\"{:.2}\"", p[0], p[1]);
        if ink.has_pressure() {
            let _ = write!(s, " force=\"{}\"", ink.force(i));
        }
        s.push_str("/>");
    }
    s.push_str("</gc:ink></a:ext></a:extLst></xdr:spPr></xdr:sp>");
}
