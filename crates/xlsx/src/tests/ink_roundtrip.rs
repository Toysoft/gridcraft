//! Plain DrawingML polylines from other packages: only representable ones become editable ink.
//! GridCraft's own ink export is covered by `inkml.rs` and `roundtrip.rs`.

use gridcraft_model::{Color, InkTool, Shape, ShapeKind, Workbook};

use super::minimal;
use crate::{read_xlsx, write_xlsx};

const POINTS: &str = r#"<a:moveTo><a:pt x="-20" y="10"/></a:moveTo><a:lnTo><a:pt x="100" y="80"/></a:lnTo><a:lnTo><a:pt x="180" y="20"/></a:lnTo>"#;
const PATH: &str = r#"<a:path w="200" h="100" fill="none">"#;

fn drawing() -> String {
    format!(
        r#"<xdr:wsDr xmlns:xdr="http://schemas.openxmlformats.org/drawingml/2006/spreadsheetDrawing" xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main">
        <xdr:oneCellAnchor><xdr:from><xdr:col>0</xdr:col><xdr:colOff>190500</xdr:colOff><xdr:row>0</xdr:row><xdr:rowOff>285750</xdr:rowOff></xdr:from><xdr:ext cx="952500" cy="476250"/>
        <xdr:sp><xdr:nvSpPr><xdr:cNvPr id="1" name="Independent polyline"/><xdr:cNvSpPr/></xdr:nvSpPr><xdr:spPr>
        <a:xfrm><a:off x="0" y="0"/><a:ext cx="952500" cy="476250"/></a:xfrm>
        <a:custGeom><a:avLst/><a:gdLst/><a:ahLst/><a:cxnLst/><a:rect l="0" t="0" r="r" b="b"/><a:pathLst>{PATH}{POINTS}</a:path></a:pathLst></a:custGeom>
        <a:noFill/><a:ln w="28575" cap="flat"><a:solidFill><a:srgbClr val="A12BCD"/></a:solidFill><a:miter/></a:ln>
        </xdr:spPr></xdr:sp><xdr:clientData/></xdr:oneCellAnchor></xdr:wsDr>"#
    )
}

fn read_drawing(drawing: &str) -> Workbook {
    let rels = r#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/drawing" Target="../drawings/drawing1.xml"/></Relationships>"#;
    let bytes = minimal(
        r#"<sheetData/><drawing r:id="rId1"/>"#,
        &[("xl/worksheets/_rels/sheet1.xml.rels", rels), ("xl/drawings/drawing1.xml", drawing)],
        "",
        "",
    );
    read_xlsx(&bytes).unwrap().0
}

fn points(shape: &Shape) -> Vec<[f64; 2]> {
    shape.ink.as_ref().unwrap().points.iter().map(|p| [p[0] as f64, p[1] as f64]).collect()
}

#[test]
fn numeric_polyline_from_an_independent_package_is_scaled_into_editable_ink() {
    for (width, alpha) in [(3, ""), (12, r#"<a:alpha val="45000"/>"#)] {
        let xml = drawing()
            .replace("w=\"28575\"", &format!("w=\"{}\"", width * 9525))
            .replace(r#"<a:srgbClr val="A12BCD"/>"#, &format!(r#"<a:srgbClr val="A12BCD">{alpha}</a:srgbClr>"#));
        let wb = read_drawing(&xml);
        let shapes = &wb.sheet(0).unwrap().shapes;
        assert_eq!(shapes.len(), 1);
        let shape = &shapes[0];
        assert_eq!(shape.kind, ShapeKind::Ink);
        assert_eq!(points(shape), [[-10.0, 5.0], [50.0, 40.0], [90.0, 10.0]]);
        assert_eq!(shape.line, Color::Rgb(0xA12BCD));
        let ink = shape.ink.as_ref().unwrap();
        assert_eq!(ink.width, width as f32);
        assert_eq!(ink.tool, if width > 8 { InkTool::Highlighter } else { InkTool::Pen });
        assert_eq!((shape.anchor.dx, shape.anchor.dy), (20.0, 30.0));
        assert_eq!((shape.anchor.width, shape.anchor.height), (100.0, 50.0));
    }
}

#[test]
fn imported_points_are_not_snapped_to_native_tenth_units() {
    let xml = drawing().replace("w=\"200\"", "w=\"2000000\"").replace(
        POINTS,
        r#"<a:moveTo><a:pt x="2001" y="10"/></a:moveTo><a:lnTo><a:pt x="1000000" y="80"/></a:lnTo><a:lnTo><a:pt x="1800000" y="20"/></a:lnTo>"#,
    );
    let wb = read_drawing(&xml);
    let shape = &wb.sheet(0).unwrap().shapes[0];
    assert_eq!(shape.kind, ShapeKind::Ink);
    let original = points(shape);
    assert!((original[0][0] - 0.10005).abs() < 0.00000001, "an imported coordinate is not necessarily a native tenth-unit point");
    let (reopened, _) = read_xlsx(&write_xlsx(&wb).unwrap()).unwrap();
    let after = &reopened.sheet(0).unwrap().shapes[0];
    assert_eq!(after.kind, ShapeKind::Ink);
    // Reopening may move the anchor to the stroke's bounds; the points stay put on the sheet.
    let sheet = reopened.sheet(0).unwrap();
    let on_sheet = |s: &Shape, p: [f64; 2]| {
        let a = &s.anchor;
        [sheet.col_left(a.cell.col) + a.dx as f64 + p[0], sheet.row_top(a.cell.row) + a.dy as f64 + p[1]]
    };
    for (actual, expected) in points(after).iter().zip(original) {
        let (actual, expected) = (on_sheet(after, *actual), on_sheet(shape, expected));
        for axis in 0..2 {
            // A saved coordinate is quantized by the file format, plus f32 model precision.
            assert!((actual[axis] - expected[axis]).abs() < 0.00006, "{actual:?} vs {expected:?}");
        }
    }
}

#[test]
fn unsupported_geometry_or_appearance_is_not_reclassified_as_editable_ink() {
    let original = drawing();
    let cases = [
        ("formula coordinate", original.replace("x=\"-20\"", "x=\"wd2\"")),
        ("nonfinite coordinate", original.replace("x=\"-20\"", "x=\"NaN\"")),
        ("overflow coordinate", original.replace("x=\"-20\"", "x=\"9223372036854775808\"")),
        ("zero path width", original.replace("w=\"200\"", "w=\"0\"")),
        ("missing point", original.replace(r#"<a:pt x="100" y="80"/>"#, "")),
        (
            "curve",
            original
                .replace(r#"<a:lnTo><a:pt x="100" y="80"/></a:lnTo>"#, r#"<a:quadBezTo><a:pt x="40" y="20"/><a:pt x="100" y="80"/></a:quadBezTo>"#),
        ),
        ("closed path", original.replace("</a:path>", "<a:close/></a:path>")),
        ("multiple paths", original.replace("</a:pathLst>", &format!("{PATH}{POINTS}</a:path></a:pathLst>"))),
        ("filled path", original.replace("fill=\"none\"", "fill=\"norm\"")),
        ("filled shape", original.replace("<a:noFill/>", "<a:solidFill><a:srgbClr val=\"112233\"/></a:solidFill>")),
        ("rotation", original.replace("<a:xfrm>", "<a:xfrm rot=\"60000\">")),
        ("reflection", original.replace("<a:xfrm>", "<a:xfrm flipH=\"1\">")),
        ("square caps", original.replace("cap=\"flat\"", "cap=\"sq\"")),
        ("round join", original.replace("<a:miter/>", "<a:round/>")),
        ("dash", original.replace("</a:ln>", "<a:prstDash val=\"dash\"/></a:ln>")),
        ("arrow", original.replace("</a:ln>", "<a:headEnd type=\"triangle\"/></a:ln>")),
        (
            "caption",
            original.replace("</xdr:sp>", "<xdr:txBody><a:bodyPr/><a:lstStyle/><a:p><a:r><a:t>Caption</a:t></a:r></a:p></xdr:txBody></xdr:sp>"),
        ),
        ("fractional width", original.replace("w=\"28575\"", "w=\"33338\"")),
        ("unsupported opacity", original.replace(r#"<a:srgbClr val="A12BCD"/>"#, r#"<a:srgbClr val="A12BCD"><a:alpha val="50000"/></a:srgbClr>"#)),
    ];
    for (name, xml) in cases {
        let wb = read_drawing(&xml);
        assert!(wb.sheet(0).unwrap().shapes.iter().all(|s| s.kind != ShapeKind::Ink), "{name} was silently converted to editable ink");
    }
}

#[test]
fn oversized_polyline_is_not_truncated_into_different_ink() {
    let mut commands = String::from(r#"<a:moveTo><a:pt x="0" y="0"/></a:moveTo>"#);
    for _ in 0..20_000 {
        commands.push_str(r#"<a:lnTo><a:pt x="100" y="80"/></a:lnTo>"#);
    }
    let wb = read_drawing(&drawing().replace(POINTS, &commands));
    assert!(wb.sheet(0).unwrap().shapes.iter().all(|s| s.kind != ShapeKind::Ink));
}
