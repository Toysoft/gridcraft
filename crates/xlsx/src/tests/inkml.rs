//! Synthetic InkML packages: no Excel-authored fixtures are committed.
use super::minimal;
use crate::{read_xlsx, write_xlsx};
use gridcraft_model::{AnchorMode, Color, Shape, ShapeKind};

fn points(shape: &Shape) -> Vec<[f32; 2]> {
    shape.ink.as_ref().map(|i| i.points.clone()).unwrap_or_default()
}
fn assert_points(a: &[[f32; 2]], b: &[[f32; 2]]) {
    assert_eq!(a.len(), b.len(), "{a:?} vs {b:?}");
    assert!(a.iter().zip(b).all(|(p, q)| (p[0] - q[0]).abs() < 0.001 && (p[1] - q[1]).abs() < 0.001), "{a:?} vs {b:?}");
}

fn package(ink: &str, mode: &str, target: &str) -> Vec<u8> {
    let drawing = r##"<xdr:wsDr xmlns:xdr="http://schemas.openxmlformats.org/drawingml/2006/spreadsheetDrawing" xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships" xmlns:mc="http://schemas.openxmlformats.org/markup-compatibility/2006" xmlns:xdr14="http://schemas.microsoft.com/office/excel/2010/spreadsheetDrawing"><xdr:twoCellAnchor editAs="oneCell"><xdr:from><xdr:col>0</xdr:col><xdr:row>0</xdr:row></xdr:from><xdr:to><xdr:col>2</xdr:col><xdr:row>5</xdr:row></xdr:to><mc:AlternateContent><mc:Choice Requires="xdr14"><xdr:contentPart r:id="ink"><xdr14:xfrm><a:off x="95250" y="190500"/><a:ext cx="952500" cy="952500"/></xdr14:xfrm></xdr:contentPart></mc:Choice><mc:Fallback><xdr:pic><xdr:nvPicPr><xdr:cNvPr descr="fallback"/></xdr:nvPicPr><xdr:blipFill><a:blip r:embed="image"/></xdr:blipFill></xdr:pic></mc:Fallback></mc:AlternateContent><xdr:clientData/></xdr:twoCellAnchor></xdr:wsDr>"##;
    let rels = format!(
        r#"<Relationships><Relationship Id="ink" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/customXml" Target="{target}" {mode}/><Relationship Id="image" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/image" Target="../media/fallback.png"/></Relationships>"#
    );
    minimal(
        "<sheetData/><drawing r:id=\"draw\"/>",
        &[
            (
                "xl/worksheets/_rels/sheet1.xml.rels",
                r#"<Relationships><Relationship Id="draw" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/drawing" Target="../drawings/drawing1.xml"/></Relationships>"#,
            ),
            ("xl/drawings/drawing1.xml", drawing),
            ("xl/drawings/_rels/drawing1.xml.rels", &rels),
            ("xl/ink/ink1.xml", ink),
            ("xl/media/fallback.png", "synthetic fallback bytes"),
        ],
        "",
        "",
    )
}
fn ink(trace: &str) -> String {
    format!(
        r##"<ink xmlns="http://www.w3.org/2003/InkML"><definitions><context xml:id="ctx"><inkSource><traceFormat><channel name="Y" units="cm"/><channel name="F" units="dev"/><channel name="X" units="cm"/></traceFormat><channelProperties><channelProperty channel="X" name="resolution" value="1000" units="1/cm"/><channelProperty channel="Y" name="resolution" value="1000" units="1/cm"/></channelProperties></inkSource></context><brush xml:id="pen"><brushProperty name="color" value="#123456"/><brushProperty name="width" value="0.05" units="cm"/><brushProperty name="height" value="0.05" units="cm"/></brush></definitions><traceGroup contextRef="#ctx" brushRef="#pen">{trace}</traceGroup></ink>"##
    )
}

#[test]
fn native_ink_preferred_over_fallback_and_roundtrips() {
    let input = ink(r#"<trace>0 10 0,'100'0'200,"0"0"0</trace><trace>1000 10 1000,1100 10 1200</trace><trace type="penUp">garbage</trace>"#);
    let (wb, rep) = read_xlsx(&package(&input, "", "../ink/ink1.xml")).unwrap();
    assert!(!rep.warnings.iter().any(|w| w.contains("InkML")), "{:?}", rep.warnings);
    let sh = wb.sheet(0).unwrap();
    assert!(sh.images.is_empty());
    assert_eq!(sh.shapes.len(), 2);
    let first = &sh.shapes[0];
    assert_eq!(first.kind, ShapeKind::Ink);
    let unit = 96.0 / 2.54 / 1000.0;
    assert_points(&points(first), &[[0.0, 0.0], [200.0 * unit, 100.0 * unit], [400.0 * unit, 200.0 * unit]]);
    assert_eq!(first.line, Color::Rgb(0x123456));
    assert!((first.ink.as_ref().unwrap().width - 0.05 * 96.0 / 2.54).abs() < 0.001);
    assert_eq!(first.anchor.mode, AnchorMode::MoveOnly);
    assert!((first.anchor.dx - 10.0).abs() < 0.001);
    assert!((sh.row_top(first.anchor.cell.row) + first.anchor.dy as f64 - 20.0).abs() < 0.001);
    assert!((first.anchor.width - 400.0 * 96.0 / 2540.0).abs() < 0.001);
    assert_ne!(first.id, sh.shapes[1].id);
    let (back, _) = read_xlsx(&write_xlsx(&wb).unwrap()).unwrap();
    assert_eq!(back.sheet(0).unwrap().shapes.len(), 2);
    for (a, b) in sh.shapes.iter().zip(&back.sheet(0).unwrap().shapes) {
        assert_points(&points(a), &points(b));
        assert_eq!(a.line, b.line);
        assert!((a.anchor.width - b.anchor.width).abs() < 0.001);
    }
}

#[test]
fn oversized_ink_uses_image_instead_of_truncating_strokes() {
    let trace = format!("<trace>{}</trace>", std::iter::repeat_n("0 0 0", 20_001).collect::<Vec<_>>().join(","));
    let too_many_points = ink(&trace);
    let too_large_part = format!("{}{}", ink("<trace>0 0 0,1 0 1</trace>"), " ".repeat(8 * 1024 * 1024));
    for input in [too_many_points, too_large_part] {
        let (wb, rep) = read_xlsx(&package(&input, "", "../ink/ink1.xml")).unwrap();
        assert!(wb.sheet(0).unwrap().shapes.is_empty());
        assert_eq!(wb.sheet(0).unwrap().images.len(), 1);
        assert!(rep.warnings.iter().any(|w| w.contains("InkML")));
    }
}

#[test]
fn invalid_unsupported_missing_or_external_ink_keeps_image_atomically() {
    for input in [
        ink("<trace>0 0 0,1 0 1</trace><trace>NaN 0 0</trace>"),
        ink("<trace>'1'0'2</trace>"),
        ink("<trace>1 0 2,3 0</trace>"),
        ink("<trace>1e999 0 2</trace>"),
        ink("<trace continuation=\"middle\">1 0 2</trace>"),
        ink("<trace>0 0 0,1 0 1</trace>").replace("value=\"1000\"", "value=\"0\""),
        "<ink><trace>1 2</trace>".into(),
        "<other/>".into(),
        ink("<trace>0 0 0,1 0 1</trace>").replace("http://www.w3.org/2003/InkML", "https://example.invalid/foreign"),
        ink("<trace>0 0 0,1 0 1</trace>").replace("<trace>", "<trace xmlns=\"https://example.invalid\">"),
    ] {
        let (wb, rep) = read_xlsx(&package(&input, "", "../ink/ink1.xml")).unwrap();
        assert!(wb.sheet(0).unwrap().shapes.is_empty());
        assert_eq!(wb.sheet(0).unwrap().images.len(), 1);
        assert!(rep.warnings.iter().any(|w| w.contains("InkML")));
    }
    for (mode, target) in [("", "../ink/missing.xml"), ("TargetMode=\"External\"", "https://example.invalid/ink.xml")] {
        let (wb, _) = read_xlsx(&package(&ink("<trace>0 0 0,1 0 1</trace>"), mode, target)).unwrap();
        assert!(wb.sheet(0).unwrap().shapes.is_empty());
        assert_eq!(wb.sheet(0).unwrap().images.len(), 1);
    }
}

#[test]
fn force_samples_change_width_and_survive_xlsx() {
    let input =
        ink(r#"<trace>0 0 0,'0'50'100,"0"0"0,0-50 0</trace>"#).replace("name=\"F\" units=\"dev\"", "name=\"F\" units=\"dev\" min=\"0\" max=\"100\"");
    let (wb, _) = read_xlsx(&package(&input, "", "../ink/ink1.xml")).unwrap();
    let stroke = &wb.sheet(0).unwrap().shapes[0];
    let style = stroke.ink.as_ref().unwrap();
    assert_eq!(style.pressures, vec![0.0, 0.5, 1.0, 1.0]);
    assert!((style.point_width(0) - style.width * 0.5).abs() < 0.0001);
    assert!((style.point_width(1) - style.width).abs() < 0.0001);
    assert!((style.point_width(2) - style.width * 1.5).abs() < 0.0001);
    let bytes = write_xlsx(&wb).unwrap();
    let (back, _) = read_xlsx(&bytes).unwrap();
    let after = &back.sheet(0).unwrap().shapes[0];
    let (a, b) = (after.ink.as_ref().unwrap(), stroke.ink.as_ref().unwrap());
    assert_eq!((a.pressures.clone(), a.width, a.tool), (b.pressures.clone(), b.width, b.tool));
    assert_points(&a.points, &b.points);
    let ignored = input.replace("</brush>", "<brushProperty name=\"ignorePressure\" value=\"true\"/></brush>");
    let (wb, _) = read_xlsx(&package(&ignored, "", "../ink/ink1.xml")).unwrap();
    let stroke = &wb.sheet(0).unwrap().shapes[0];
    let style = stroke.ink.as_ref().unwrap();
    assert!(!style.has_pressure());
    assert_eq!(style.point_width(0), style.point_width(2));
}

#[test]
fn native_export_has_ink_parts_relationships_and_effects_without_duplicate_fallbacks() {
    use gridcraft_model::{Anchor, Color, Ink, InkEffect, InkTool, Workbook};
    let mut wb = Workbook::new();
    for (i, tool) in InkTool::ALL.into_iter().enumerate() {
        let ink = Ink {
            points: vec![[0.0, 0.0], [50.0, 30.0], [100.0, 0.0]],
            width: 3.25,
            pressures: vec![0.0, 0.5, 1.0],
            tool,
            effect: (i == 0).then_some(InkEffect::Rainbow),
            effect_origin: if i == 0 { [35.0, -15.0] } else { [0.0; 2] },
        };
        let anchor = Anchor { width: 100.0, height: 30.0, ..Anchor::default() };
        wb.sheet_mut(0).unwrap().shapes.push(Shape::new_ink(i as u32 + 1, anchor, Color::Rgb(0x123456), ink));
    }
    let bytes = write_xlsx(&wb).unwrap();
    let mut package = crate::package::Package::open(&bytes).unwrap();
    let xml = String::from_utf8(package.read("xl/drawings/drawing1.xml").unwrap().unwrap()).unwrap();
    assert_eq!(xml.matches("<xdr:contentPart").count(), 3);
    assert_eq!(xml.matches("<mc:Fallback>").count(), 3);
    let types = String::from_utf8(package.read("[Content_Types].xml").unwrap().unwrap()).unwrap();
    assert!(types.contains("application/inkml+xml"));
    let rels = package.rels("xl/drawings/drawing1.xml").unwrap();
    assert_eq!(rels.iter().filter(|r| r.kind == "customXml").count(), 3);
    let ink = String::from_utf8(package.read("xl/ink/ink1_1.xml").unwrap().unwrap()).unwrap();
    assert!(ink.contains("inkEffects\" value=\"rainbow"));
    assert!(ink.contains("contextRef=\"#ctx\" brushRef=\"#brush\""));
    let (back, report) = read_xlsx(&bytes).unwrap();
    assert!(report.warnings.is_empty(), "{:?}", report.warnings);
    assert!(back.active().unwrap().images.is_empty());
    assert_eq!(back.active().unwrap().shapes.len(), 3);
    for (before, after) in wb.active().unwrap().shapes.iter().zip(&back.active().unwrap().shapes) {
        let (a, b) = (before.ink.as_ref().unwrap(), after.ink.as_ref().unwrap());
        assert_eq!((a.tool, a.effect, a.effect_origin, &a.pressures), (b.tool, b.effect, b.effect_origin, &b.pressures));
        assert_points(&a.points, &b.points);
    }
}

/// Without InkML (an application that kept only the DrawingML fallback), the fallback alone
/// reads back as the same stroke, pen and effect included.
#[test]
fn drawingml_fallback_alone_reads_back_as_ink() {
    use gridcraft_model::{Anchor, Ink, InkEffect, InkTool, Theme};
    for pressures in [vec![], vec![0.25, 0.5, 1.0]] {
        let ink = Ink {
            points: vec![[0.0, 0.0], [30.0, 10.0], [60.0, 0.0]],
            width: 4.0,
            pressures,
            tool: InkTool::Highlighter,
            effect: Some(InkEffect::Gold),
            ..Ink::default()
        };
        let shape = Shape::new_ink(1, Anchor { width: 60.0, height: 10.0, ..Anchor::default() }, Color::Rgb(0x123456), ink.clone());
        let mut xml = String::new();
        crate::drawing::write_ink(&mut xml, &shape, &ink, 1, &Theme::default());
        let el = crate::xml::parse(xml.as_bytes()).unwrap();
        let back = crate::drawing::read_gridcraft_ink(el.child("spPr").unwrap(), shape.anchor).unwrap();
        assert_eq!(back.ink.as_ref(), Some(&ink));
        assert_eq!(back.line, shape.line);
    }
}
