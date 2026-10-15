//! Original ODS packages assembled in Rust, not files from another spreadsheet application.

use gridcraft_core::{CellRef, RangeRef, Value};

use super::make_zip;
use crate::{Format, IoError, read_ods, read_xlsx, sniff, write_xlsx};

const MIME: &[u8] = b"application/vnd.oasis.opendocument.spreadsheet";
const MANIFEST: &[u8] = br#"<manifest:manifest xmlns:manifest="urn:oasis:names:tc:opendocument:xmlns:manifest:1.0"><manifest:file-entry manifest:full-path="/" manifest:media-type="application/vnd.oasis.opendocument.spreadsheet"/><manifest:file-entry manifest:full-path="content.xml" manifest:media-type="text/xml"/></manifest:manifest>"#;

fn content(tables: &str) -> String {
    format!(
        r#"<?xml version="1.0"?><office:document-content xmlns:office="urn:oasis:names:tc:opendocument:xmlns:office:1.0" xmlns:table="urn:oasis:names:tc:opendocument:xmlns:table:1.0" xmlns:text="urn:oasis:names:tc:opendocument:xmlns:text:1.0" xmlns:of="urn:oasis:names:tc:opendocument:xmlns:of:1.2"><office:body><office:spreadsheet>{tables}</office:spreadsheet></office:body></office:document-content>"#
    )
}
fn ods(tables: &str) -> Vec<u8> {
    make_zip(&[("mimetype", MIME), ("META-INF/manifest.xml", MANIFEST), ("content.xml", content(tables).as_bytes())])
}
fn row(cells: &str) -> Vec<u8> {
    ods(&format!(r#"<table:table table:name="Data"><table:table-row>{cells}</table:table-row></table:table>"#))
}
fn text(s: &str) -> Value {
    Value::Text(s.into())
}

/// A start tag with thousands of `xmlns:` declarations is refused instead of allocated
/// (RUSTSEC-2026-0195: the resolver kept a binding per declaration, with no cap).
#[test]
fn ods_namespace_declaration_bomb_is_rejected() {
    let bomb = |n: usize| -> String { (0..n).map(|i| format!(" xmlns:p{i}=\"urn:p{i}\"")).collect() };
    let doc = |decls: &str| {
        format!(
            r#"<?xml version="1.0"?><office:document-content xmlns:office="urn:oasis:names:tc:opendocument:xmlns:office:1.0" xmlns:table="urn:oasis:names:tc:opendocument:xmlns:table:1.0" xmlns:text="urn:oasis:names:tc:opendocument:xmlns:text:1.0"{decls}><office:body><office:spreadsheet><table:table table:name="Data"><table:table-row><table:table-cell office:value-type="float" office:value="1"><text:p>1</text:p></table:table-cell></table:table-row></table:table></office:spreadsheet></office:body></office:document-content>"#
        )
    };
    let build = |decls: &str| make_zip(&[("mimetype", MIME), ("META-INF/manifest.xml", MANIFEST), ("content.xml", doc(decls).as_bytes())]);
    let ok = read_ods(&build(&bomb(8)));
    assert!(ok.is_ok(), "a handful of namespaces still reads: {:?}", ok.err());
    assert!(read_ods(&build(&bomb(10_000))).is_err(), "a tag with thousands of declarations is refused");
}

#[test]
fn ods_typed_values_formulas_and_caches_survive_xlsx() {
    let bytes = row(r#"
        <table:table-cell office:value-type="float" office:value="42.5"/>
        <table:table-cell office:value-type="boolean" office:boolean-value="true"/>
        <table:table-cell office:value-type="percentage" office:value="0.25"/>
        <table:table-cell office:value-type="currency" office:currency="USD" office:value="19.99"/>
        <table:table-cell office:value-type="string" office:string-value="literal =1+1"/>
        <table:table-cell table:formula="of:=A1*2" office:value-type="float" office:value="85"><text:p>85</text:p></table:table-cell>
        <table:table-cell table:formula="of:=1/0" office:value-type="string"><text:p>#DIV/0!</text:p></table:table-cell>
        <table:table-cell table:formula="of:=UNKNOWN()"/>
        <table:table-cell office:value-type="float" office:value="NaN"><text:p>unavailable</text:p></table:table-cell>
        <table:table-cell office:value-type="boolean" office:boolean-value="0"/>
    "#);
    let (wb, report) = read_ods(&bytes).unwrap();
    assert!(report.warnings.iter().any(|w| w.contains("unsupported or missing")));
    let expected = [
        Value::Number(42.5),
        Value::Bool(true),
        Value::Number(0.25),
        Value::Number(19.99),
        text("literal =1+1"),
        Value::Number(85.0),
        text("#DIV/0!"),
        Value::Empty,
        text("unavailable"),
        Value::Bool(false),
    ];
    let formulas = [None, None, None, None, None, Some("A1*2"), Some("1/0"), Some("UNKNOWN()"), None, None];
    for (c, value) in expected.iter().enumerate() {
        let cell = wb.sheets[0].cell(CellRef::new(0, c as u32));
        assert_eq!(&wb.sheets[0].value(CellRef::new(0, c as u32)), value);
        assert_eq!(cell.and_then(|c| c.formula.as_ref()).map(|f| f.text.as_str()), formulas[c], "column {c}");
    }
    let (reopened, _) = read_xlsx(&write_xlsx(&wb).unwrap()).unwrap();
    for (c, value) in expected.iter().enumerate() {
        assert_eq!(&reopened.sheets[0].value(CellRef::new(0, c as u32)), value);
    }
}

#[test]
fn ods_ordered_text_preserves_paragraphs_runs_entities_spaces_tabs_and_breaks() {
    let bytes = row(
        r#"<table:table-cell office:value-type="string"><text:p>one <text:span>two &amp; three</text:span> four<text:s text:c="3"/><text:a>five</text:a><text:tab/>six<text:line-break/>seven&#33;<![CDATA[<eight>]]></text:p><text:p>nine</text:p><office:annotation><text:p>not cell text</text:p></office:annotation></table:table-cell>"#,
    );
    let (wb, _) = read_ods(&bytes).unwrap();
    assert_eq!(wb.sheets[0].value(CellRef::new(0, 0)), text("one two & three four   five\tsix\nseven!<eight>\nnine"));
    let bytes = row(r#"<table:table-cell office:value-type="string"><text:p>  one
        <text:span> two  </text:span>  three <text:s text:c="0"/>four<text:s text:c="2"/>  </text:p><text:p><text:s/>  five  </text:p></table:table-cell>"#);
    assert_eq!(read_ods(&bytes).unwrap().0.sheets[0].value(CellRef::new(0, 0)), text("one two three four  \n  five"));
}

#[test]
fn ods_repeats_merges_sheet_names_and_empty_tail_remain_sparse() {
    let bytes = ods(r#"
      <table:table table:name="First &amp; Second">
        <table:table-header-rows><table:table-row><table:table-cell table:number-columns-spanned="2" table:number-rows-spanned="2" office:value-type="string"><text:p>merged</text:p></table:table-cell><table:covered-table-cell office:value-type="float" office:value="99"/></table:table-row></table:table-header-rows>
        <table:table-row><table:covered-table-cell table:number-columns-repeated="2"/></table:table-row>
        <table:table-row-group><table:table-row table:number-rows-repeated="2"><table:table-cell table:number-columns-repeated="3" office:value-type="float" office:value="7"/></table:table-row></table:table-row-group>
        <table:table-row table:number-rows-repeated="1048572"><table:table-cell table:number-columns-repeated="16384"/></table:table-row>
      </table:table>
      <table:table table:name="Another"><table:table-row><table:table-cell office:value-type="string" office:string-value="second sheet"/></table:table-row></table:table>
    "#);
    let (wb, _) = read_ods(&bytes).unwrap();
    assert_eq!(wb.sheets.len(), 2);
    assert_eq!(wb.sheets[0].name, "First & Second");
    assert_eq!(wb.sheets[0].cells.len(), 8);
    assert_eq!(wb.sheets[0].value(CellRef::new(0, 1)), Value::Number(99.0));
    assert_eq!(wb.sheets[0].merges, vec![RangeRef::new(CellRef::new(0, 0), CellRef::new(1, 1))]);
    assert_eq!(wb.sheets[0].value(CellRef::new(3, 2)), Value::Number(7.0));
    assert_eq!(wb.sheets[1].value(CellRef::new(0, 0)), text("second sheet"));
}

#[test]
fn ods_dates_and_durations_use_formats_and_unsupported_values_stay_text() {
    let bytes = row(r#"
        <table:table-cell office:value-type="date" office:date-value="1900-02-28"/>
        <table:table-cell office:value-type="date" office:date-value="2026-10-07T18:30:00.125"/>
        <table:table-cell office:value-type="time" office:time-value="P1DT2H3M4.5S"/>
        <table:table-cell office:value-type="time" office:time-value="-PT2H"/>
        <table:table-cell office:value-type="date" office:date-value="1900-02-29"/>
        <table:table-cell office:value-type="date" office:date-value="2026-10-07T18:00:00Z"/>
        <table:table-cell office:value-type="time" office:time-value="P1M"/>
        <table:table-cell office:value-type="time" office:time-value="PT2M1H"/>
    "#);
    let (wb, report) = read_ods(&bytes).unwrap();
    let s = &wb.sheets[0];
    assert_eq!(s.value(CellRef::new(0, 0)), Value::Number(59.0));
    assert_eq!(s.value(CellRef::new(0, 1)), Value::Number(46302.0 + 66600.125 / 86400.0));
    assert_eq!(s.value(CellRef::new(0, 2)), Value::Number(1.0 + 7384.5 / 86400.0));
    assert_eq!(s.value(CellRef::new(0, 3)), Value::Number(-2.0 / 24.0));
    for (c, value) in [(4, "1900-02-29"), (5, "2026-10-07T18:00:00Z"), (6, "P1M"), (7, "PT2M1H")] {
        assert_eq!(s.value(CellRef::new(0, c)), text(value));
    }
    assert_eq!(wb.styles.get(s.cell(CellRef::new(0, 0)).unwrap().style).num_fmt.0.as_ref(), "yyyy-mm-dd");
    assert_eq!(wb.styles.get(s.cell(CellRef::new(0, 2)).unwrap().style).num_fmt.0.as_ref(), "[h]:mm:ss.000");
    assert_eq!(report.warnings.len(), 1);
}

#[test]
fn ods_namespaces_are_resolved_not_inferred_from_prefix_or_local_name() {
    let xml = content(r#"<table:table table:name="Actual"><table:table-row><table:table-cell office:value-type="float" office:value="9"/></table:table-row></table:table>"#)
        .replace("<office:", "<o:").replace("</office:", "</o:").replace(" office:", " o:").replace("xmlns:office=", "xmlns:o=")
        .replace("<table:", "<t:").replace("</table:", "</t:").replace(" table:", " t:").replace("xmlns:table=", "xmlns:t=");
    let bytes = make_zip(&[("mimetype", MIME), ("content.xml", xml.as_bytes())]);
    assert_eq!(read_ods(&bytes).unwrap().0.sheets[0].value(CellRef::new(0, 0)), Value::Number(9.0));
    let spoofed = xml.replace("urn:oasis:names:tc:opendocument:xmlns:table:1.0", "wrong-namespace");
    assert!(read_ods(&make_zip(&[("mimetype", MIME), ("content.xml", spoofed.as_bytes())])).is_err());
    let bytes =
        row(r#"<table:table-cell xmlns:fake="wrong-namespace" fake:value-type="float" fake:value="123"><text:p>kept</text:p></table:table-cell>"#);
    assert_eq!(read_ods(&bytes).unwrap().0.sheets[0].value(CellRef::new(0, 0)), text("kept"));
}

#[test]
fn ods_rejects_bad_packages_encryption_and_truncated_xml() {
    assert!(read_ods(b"not zip").is_err());
    let xml = content(r#"<table:table table:name="Data"/>"#);
    for mime in [b"application/zip".as_slice(), b"application/vnd.oasis.opendocument.spreadsheet "] {
        assert!(read_ods(&make_zip(&[("mimetype", mime), ("content.xml", xml.as_bytes())])).is_err());
    }
    let encrypted =
        br#"<manifest:manifest xmlns:manifest="urn:oasis:names:tc:opendocument:xmlns:manifest:1.0"><manifest:encryption-data/></manifest:manifest>"#;
    assert!(
        read_ods(&make_zip(&[("mimetype", MIME), ("META-INF/manifest.xml", encrypted), ("content.xml", xml.as_bytes())]))
            .unwrap_err()
            .to_string()
            .contains("encrypted")
    );
    for bad in [
        xml.trim_end_matches("</office:document-content>").to_owned(),
        xml.replace("</office:spreadsheet>", "</office:wrong>"),
        "<root/>".to_owned(),
        format!("{xml}{xml}"),
    ] {
        assert!(read_ods(&make_zip(&[("mimetype", MIME), ("content.xml", bad.as_bytes())])).is_err(), "{bad}");
    }
    let deep = content(&format!("{}{}", "<x>".repeat(300), "</x>".repeat(300)));
    assert!(read_ods(&make_zip(&[("mimetype", MIME), ("content.xml", deep.as_bytes())])).is_err());
}

#[test]
fn ods_expansion_bounds_stop_small_repeat_bombs() {
    for cells in [
        r#"<table:table-cell table:number-columns-repeated="4294967296"/>"#,
        r#"<table:table-cell table:number-columns-repeated="4294967295" office:value-type="float" office:value="1"/>"#,
        r#"<table:table-cell table:number-columns-repeated="0"/>"#,
        r#"<table:table-cell table:number-columns-repeated="16384"/><table:table-cell office:value-type="float" office:value="1"/>"#,
        r#"<table:table-cell table:number-columns-repeated="16384"/><table:table-cell table:number-columns-spanned="2"/>"#,
        r#"<table:table-cell table:number-columns-repeated="16384" table:number-columns-spanned="2" office:value-type="float" office:value="1"/>"#,
        r#"<table:table-cell office:value-type="string"><text:p><text:s text:c="4294967295"/></text:p></table:table-cell>"#,
    ] {
        assert!(read_ods(&row(cells)).is_err(), "{cells}");
    }
    let cells = ods(
        r#"<table:table table:name="Data"><table:table-row table:number-rows-repeated="1048576"><table:table-cell office:value-type="float" office:value="1"/></table:table-row></table:table>"#,
    );
    assert!(matches!(read_ods(&cells), Err(IoError::TooLarge(_))));
    let text = format!(
        r#"<table:table table:name="Data"><table:table-row table:number-rows-repeated="100000"><table:table-cell office:value-type="string" office:string-value="{}"/></table:table-row></table:table>"#,
        "x".repeat(1024)
    );
    assert!(matches!(read_ods(&ods(&text)), Err(IoError::TooLarge(_))));
}

#[test]
fn ods_sniff_requires_exact_bounded_mimetype() {
    assert_eq!(sniff(&row("")), Format::Ods);
    assert_eq!(sniff(&make_zip(&[("mimetype", b"application/zip")])), Format::Xlsx);
    assert_eq!(sniff(&make_zip(&[("elsewhere", MIME)])), Format::Xlsx);
    assert_eq!(sniff(&make_zip(&[("mimetype", &vec![b'x'; 1_000_000])])), Format::Xlsx);
    assert_eq!(sniff(b"a,b\n1,2"), Format::Csv);
}

#[test]
fn ods_invalid_sheet_names_are_made_safe_for_xlsx() {
    let long = "A".repeat(50);
    let bytes = ods(&format!(
        r#"<table:table table:name="{long}"/><table:table table:name="{long}"/><table:table table:name="'History'"/><table:table table:name="[]:*?"/>"#
    ));
    let (wb, report) = read_ods(&bytes).unwrap();
    for (i, sheet) in wb.sheets.iter().enumerate() {
        assert!(wb.check_sheet_name(&sheet.name, Some(i)).is_ok());
    }
    assert!(report.warnings.iter().any(|w| w.contains("sheet names")));
    assert_eq!(read_xlsx(&write_xlsx(&wb).unwrap()).unwrap().0.sheets.len(), 4);
}

#[test]
fn ods_extension_error_cache_reads_as_its_error_never_the_placeholder_zero() {
    let bytes = row(
        r#"<table:table-cell xmlns:calcext="urn:org:documentfoundation:names:experimental:calc:xmlns:calcext:1.0" calcext:value-type="error" office:value-type="float" office:value="0" table:formula="of:=1/0"><text:p>#DIV/0!</text:p></table:table-cell>"#,
    );
    let (wb, report) = read_ods(&bytes).unwrap();
    assert_eq!(wb.sheets[0].value(CellRef::new(0, 0)), Value::Error(gridcraft_core::CellError::Div0));
    assert_eq!(wb.sheets[0].cell(CellRef::new(0, 0)).unwrap().formula.as_ref().unwrap().text, "1/0");
    assert!(report.warnings.is_empty(), "{:?}", report.warnings);
}

#[test]
fn ods_blank_repeats_past_the_sheet_edge_are_clipped() {
    // Writers with larger sheets pad to their own edge; blank padding past ours carries no data.
    let bytes = ods(r#"<table:table table:name="Data">
        <table:table-row><table:table-cell office:value-type="float" office:value="1"/><table:table-cell table:number-columns-repeated="4294967295"/><table:table-cell/></table:table-row>
        <table:table-row table:number-rows-repeated="16777215"><table:table-cell table:number-columns-repeated="16777216"/></table:table-row>
        <table:table-row table:number-rows-repeated="5"><table:table-cell/></table:table-row>
      </table:table>"#);
    let (wb, _) = read_ods(&bytes).unwrap();
    assert_eq!(wb.sheets[0].cells.len(), 1);
    assert_eq!(wb.sheets[0].value(CellRef::new(0, 0)), Value::Number(1.0));
    // Data after the clipped edge is still an error, never silently dropped.
    let late = ods(
        r#"<table:table table:name="Data"><table:table-row table:number-rows-repeated="1048576"><table:table-cell/></table:table-row><table:table-row><table:table-cell office:value-type="float" office:value="1"/></table:table-row></table:table>"#,
    );
    assert!(matches!(read_ods(&late), Err(IoError::TooLarge(_))));
}

const NS: &str = r#"xmlns:office="urn:oasis:names:tc:opendocument:xmlns:office:1.0" xmlns:style="urn:oasis:names:tc:opendocument:xmlns:style:1.0" xmlns:table="urn:oasis:names:tc:opendocument:xmlns:table:1.0" xmlns:text="urn:oasis:names:tc:opendocument:xmlns:text:1.0" xmlns:fo="urn:oasis:names:tc:opendocument:xmlns:xsl-fo-compatible:1.0" xmlns:number="urn:oasis:names:tc:opendocument:xmlns:datastyle:1.0" xmlns:svg="urn:oasis:names:tc:opendocument:xmlns:svg-compatible:1.0" xmlns:tableooo="http://openoffice.org/2009/table""#;

/// A package with a `styles.xml` (common styles) and automatic styles in `content.xml`.
fn styled_ods(common: &str, automatic: &str, tables: &str) -> Vec<u8> {
    let styles = format!(r#"<?xml version="1.0"?><office:document-styles {NS}><office:styles>{common}</office:styles></office:document-styles>"#);
    let content = format!(
        r#"<?xml version="1.0"?><office:document-content {NS}><office:automatic-styles>{automatic}</office:automatic-styles><office:body><office:spreadsheet>{tables}</office:spreadsheet></office:body></office:document-content>"#
    );
    make_zip(&[("mimetype", MIME), ("META-INF/manifest.xml", MANIFEST), ("styles.xml", styles.as_bytes()), ("content.xml", content.as_bytes())])
}

#[test]
fn ods_cell_styles_number_formats_widths_and_heights_are_imported() {
    use gridcraft_model::{BorderStyle, Color, HAlign, PatternType, Underline, VAlign, Visibility};
    let common = r#"
        <style:default-style style:family="table-cell"><style:text-properties style:font-name="Body Font" fo:font-size="10pt"/></style:default-style>
        <style:style style:name="Default" style:family="table-cell"/>
        <style:style style:name="Heading" style:family="table-cell" style:parent-style-name="Default"><style:text-properties fo:font-weight="bold" fo:font-size="14pt"/></style:style>"#;
    let automatic = r##"
        <style:font-face style:name="Body Font" svg:font-family="'Body Sans'"/>
        <number:number-style style:name="N1P0" style:volatile="true"><number:number number:decimal-places="2" number:min-integer-digits="1" number:grouping="true"/></number:number-style>
        <number:number-style style:name="N1"><style:text-properties fo:color="#ff0000"/><number:text>-</number:text><number:number number:decimal-places="2" number:min-integer-digits="1" number:grouping="true"/><style:map style:condition="value()&gt;=0" style:apply-style-name="N1P0"/></number:number-style>
        <number:date-style style:name="N2"><number:day number:style="long"/><number:text>.</number:text><number:month number:style="long"/><number:text>.</number:text><number:year number:style="long"/></number:date-style>
        <number:percentage-style style:name="N3"><number:number number:decimal-places="1" number:min-integer-digits="1"/><number:text>%</number:text></number:percentage-style>
        <style:style style:name="ce1" style:family="table-cell" style:parent-style-name="Heading">
          <style:table-cell-properties fo:background-color="#1e3a5f" fo:border="0.74pt solid #000000" fo:wrap-option="wrap" style:vertical-align="middle"/>
          <style:paragraph-properties fo:text-align="center"/>
          <style:text-properties fo:color="#ffffff" fo:font-style="italic" style:text-underline-style="solid"/>
        </style:style>
        <style:style style:name="ce2" style:family="table-cell" style:data-style-name="N1"/>
        <style:style style:name="ce3" style:family="table-cell" style:data-style-name="N2"/>
        <style:style style:name="ce4" style:family="table-cell" style:data-style-name="N3"/>
        <style:style style:name="ce5" style:family="table-cell"><style:table-cell-properties fo:background-color="#ffff00"/></style:style>
        <style:style style:name="co1" style:family="table-column"><style:table-column-properties style:column-width="2.54cm"/></style:style>
        <style:style style:name="co2" style:family="table-column"><style:table-column-properties style:column-width="0.889in"/></style:style>
        <style:style style:name="ro1" style:family="table-row"><style:table-row-properties style:row-height="0.5in" style:use-optimal-row-height="false"/></style:style>
        <style:style style:name="ro2" style:family="table-row"><style:table-row-properties style:row-height="0.178in" style:use-optimal-row-height="true"/></style:style>
        <style:style style:name="ta1" style:family="table"><style:table-properties table:display="false" tableooo:tab-color="#00ff00"/></style:style>"##;
    let tables = r#"
        <table:table table:name="Styled">
          <table:table-column table:style-name="co1" table:default-cell-style-name="ce5"/>
          <table:table-column table:style-name="co2" table:number-columns-repeated="16383" table:default-cell-style-name="Default"/>
          <table:table-row table:style-name="ro1">
            <table:table-cell table:style-name="ce1" office:value-type="string"><text:p>Title</text:p></table:table-cell>
            <table:table-cell table:style-name="ce2" office:value-type="float" office:value="-1234.5"/>
            <table:table-cell table:style-name="ce3" office:value-type="date" office:date-value="2026-10-15"/>
            <table:table-cell table:style-name="ce4" office:value-type="percentage" office:value="0.125"/>
            <table:table-cell table:style-name="ce5"/>
          </table:table-row>
          <table:table-row table:style-name="ro2"><table:table-cell office:value-type="float" office:value="1"/></table:table-row>
          <table:table-row table:style-name="ro2" table:number-rows-repeated="1048574"><table:table-cell table:style-name="ce5" table:number-columns-repeated="16384"/></table:table-row>
        </table:table>
        <table:table table:name="Hidden" table:style-name="ta1"><table:table-row><table:table-cell/></table:table-row></table:table>"#;
    let (wb, report) = read_ods(&styled_ods(common, automatic, tables)).unwrap();
    let s = &wb.sheets[0];
    let style = |c: u32, r: u32| wb.styles.get(s.cell(CellRef::new(r, c)).unwrap().style).clone();
    let base = wb.styles.get(gridcraft_model::StyleId::DEFAULT);
    assert_eq!((base.font.name.as_str(), base.font.size), ("Body Sans", 10.0));
    let title = style(0, 0);
    assert!(title.font.bold && title.font.italic);
    assert_eq!(title.font.size, 14.0);
    assert_eq!(title.font.name, "Body Sans");
    assert_eq!(title.font.underline, Underline::Single);
    assert_eq!(title.font.color, Color::Rgb(0xffffff));
    assert_eq!((title.fill.pattern, title.fill.fg), (PatternType::Solid, Color::Rgb(0x1e3a5f)));
    assert_eq!(title.borders.left.style, BorderStyle::Thin);
    assert_eq!(title.borders.bottom.style, BorderStyle::Thin);
    assert_eq!((title.align.h, title.align.v, title.align.wrap), (HAlign::Center, VAlign::Center, true));
    assert_eq!(style(1, 0).num_fmt.as_str(), "#,##0.00;[Red]\"-\"#,##0.00".replace("\"-\"", "-"));
    assert_eq!(style(2, 0).num_fmt.as_str(), "dd\".\"mm\".\"yyyy");
    assert_eq!(style(3, 0).num_fmt.as_str(), "0.0%");
    // A styled blank cell is kept; the column's default style becomes the column style.
    assert_eq!(style(4, 0).fill.fg, Color::Rgb(0xffff00));
    assert!(s.cols.get(&0).and_then(|c| c.style).is_some());
    // The value in column A without its own style takes the column's default style.
    assert_eq!(style(0, 1).fill.fg, Color::Rgb(0xffff00));
    assert_eq!(s.cols.get(&0).and_then(|c| c.size), Some(96.0));
    assert_eq!(s.default_col_width, 85.0);
    assert_eq!(s.rows.get(&0).and_then(|r| r.size), Some(48.0));
    assert!(!s.rows.contains_key(&1));
    // The formatted padding rows are skipped, with a warning, instead of filling the sheet.
    assert!(s.cells.len() < 10);
    assert!(report.warnings.iter().any(|w| w.contains("large empty")));
    assert_eq!(wb.sheets[1].visibility, Visibility::Hidden);
    assert_eq!(wb.sheets[1].tab_color, Some(Color::Rgb(0x00ff00)));
}

#[test]
fn ods_formulas_names_and_arrays_are_imported() {
    let tables = r#"
        <table:table table:name="Data">
          <table:table-row>
            <table:table-cell office:value-type="float" office:value="2"/>
            <table:table-cell office:value-type="float" office:value="3"/>
            <table:table-cell table:formula="of:=SUM([.A1:.B1])*['Other sheet'.A1]" office:value-type="float" office:value="50"/>
            <table:table-cell table:formula="of:=Rate*2" office:value-type="float" office:value="0.2"/>
            <table:table-cell table:formula="of:=TRANSPOSE([.A1:.B1])" table:number-matrix-columns-spanned="1" table:number-matrix-rows-spanned="2" office:value-type="float" office:value="2"/>
          </table:table-row>
          <table:table-row><table:table-cell table:number-columns-repeated="4"/><table:table-cell office:value-type="float" office:value="3"/></table:table-row>
          <table:named-expressions><table:named-expression table:name="Local" table:expression="of:=[$Data.$A$1]"/></table:named-expressions>
        </table:table>
        <table:table table:name="Other sheet"><table:table-row><table:table-cell office:value-type="float" office:value="10"/></table:table-row></table:table>
        <table:named-expressions>
          <table:named-range table:name="Inputs" table:base-cell-address="$Data.$A$1" table:cell-range-address="$Data.$A$1:.$B$1"/>
          <table:named-expression table:name="Rate" table:expression="of:=0.1"/>
        </table:named-expressions>"#;
    let (wb, report) = read_ods(&ods(tables)).unwrap();
    assert!(report.warnings.is_empty(), "{:?}", report.warnings);
    let s = &wb.sheets[0];
    let f = |c: u32, r: u32| s.cell(CellRef::new(r, c)).unwrap().formula.clone().unwrap();
    assert_eq!(f(2, 0).text, "SUM(A1:B1)*'Other sheet'!A1");
    assert_eq!(s.value(CellRef::new(0, 2)), Value::Number(50.0));
    assert_eq!(f(3, 0).text, "Rate*2");
    assert_eq!(f(4, 0).array, Some(RangeRef::new(CellRef::new(0, 4), CellRef::new(1, 4))));
    assert_eq!(s.value(CellRef::new(1, 4)), Value::Number(3.0));
    let name = |n: &str| wb.names.iter().find(|d| d.name == n).unwrap().clone();
    assert_eq!((name("Inputs").formula.as_str(), name("Inputs").scope), ("Data!$A$1:$B$1", None));
    assert_eq!(name("Rate").formula, "0.1");
    assert_eq!((name("Local").formula.as_str(), name("Local").scope), ("Data!$A$1", Some(0)));
}

#[test]
fn ods_export_round_trips_values_formulas_styles_and_layout() {
    use gridcraft_model::{BorderLine, BorderStyle, Cell, Color, DefinedName, Fill, Formula, HAlign, LineInfo, NumFmt, StyleId, VAlign, Visibility};
    use std::sync::Arc;
    let mut wb = gridcraft_model::Workbook::new();
    let bold = wb.styles.derive(StyleId::DEFAULT, |s| {
        s.font.bold = true;
        s.font.italic = true;
        s.font.size = 14.0;
        s.font.name = "Serif Face".into();
        s.font.color = Color::Rgb(0x123456);
        s.fill = Fill::solid(Color::Rgb(0xd1fae5));
        s.borders.bottom = BorderLine { style: BorderStyle::Medium, color: Color::Rgb(0xff0000) };
        s.borders.top = BorderLine { style: BorderStyle::Dashed, color: Color::Auto };
        s.align.h = HAlign::Right;
        s.align.v = VAlign::Top;
        s.align.wrap = true;
        s.align.indent = 2;
        s.align.rotation = 45;
    });
    let money = wb.styles.derive(StyleId::DEFAULT, |s| s.num_fmt = NumFmt::new("#,##0.00;[Red]-#,##0.00"));
    let date = wb.styles.derive(StyleId::DEFAULT, |s| s.num_fmt = NumFmt::new("yyyy-mm-dd"));
    let time = wb.styles.derive(StyleId::DEFAULT, |s| s.num_fmt = NumFmt::new("[h]:mm:ss"));
    let pct = wb.styles.derive(StyleId::DEFAULT, |s| s.num_fmt = NumFmt::new("0.0%"));
    let other = Arc::new(gridcraft_model::Sheet::new("Second sheet"));
    wb.sheets.push(other);
    {
        let sh = Arc::make_mut(&mut wb.sheets[0]);
        sh.set_cell(CellRef::new(0, 0), Cell { style: bold, ..Cell::value(text("Header  with   spaces\nand a second line")) });
        sh.set_cell(CellRef::new(1, 0), Cell { style: money, ..Cell::value(Value::Number(-1234.5)) });
        sh.set_cell(CellRef::new(1, 1), Cell { style: date, ..Cell::value(Value::Number(46310.0)) });
        sh.set_cell(CellRef::new(1, 2), Cell { style: time, ..Cell::value(Value::Number(1.5)) });
        sh.set_cell(CellRef::new(1, 3), Cell { style: pct, ..Cell::value(Value::Number(0.125)) });
        sh.set_cell(CellRef::new(2, 0), Cell::value(Value::Bool(true)));
        sh.set_cell(CellRef::new(2, 1), Cell { value: Value::Number(10.0), ..Cell::formula(Formula::new("SUM(A2:B2)+'Second sheet'!A1")) });
        sh.set_cell(CellRef::new(2, 2), Cell { value: Value::Number(0.2), ..Cell::formula(Formula::new("IF(A3,Rate*2,0)")) });
        sh.set_cell(CellRef::new(5, 5), Cell { style: bold, ..Cell::value(Value::Empty) });
        sh.merges.push(RangeRef::new(CellRef::new(3, 0), CellRef::new(4, 2)));
        sh.set_cell(CellRef::new(3, 0), Cell::value(text("merged")));
        sh.cols.insert(1, LineInfo { size: Some(150.0), ..Default::default() });
        sh.cols.insert(2, LineInfo { hidden: true, ..Default::default() });
        sh.rows.insert(0, LineInfo { size: Some(40.0), custom: true, ..Default::default() });
        sh.rows.insert(6, LineInfo { hidden: true, ..Default::default() });
        sh.tab_color = Some(Color::Rgb(0x00ff00));
    }
    Arc::make_mut(&mut wb.sheets[1]).set_cell(CellRef::new(0, 0), Cell::value(Value::Number(7.0)));
    Arc::make_mut(&mut wb.sheets[1]).visibility = Visibility::Hidden;
    wb.names.push(DefinedName { name: "Rate".into(), scope: None, formula: "0.1".into(), comment: String::new(), hidden: false });
    wb.names.push(DefinedName { name: "Inputs".into(), scope: None, formula: "Sheet1!$A$2:$B$2".into(), comment: String::new(), hidden: false });

    let bytes = crate::write_ods(&wb).unwrap();
    assert_eq!(sniff(&bytes), Format::Ods);
    // The mimetype entry is first and stored, as ODF packages require.
    let mut zip = zip::ZipArchive::new(std::io::Cursor::new(&bytes)).unwrap();
    let first = zip.by_index(0).unwrap();
    assert_eq!((first.name(), first.compression()), ("mimetype", zip::CompressionMethod::Stored));
    drop(first);

    let (back, report) = read_ods(&bytes).unwrap();
    assert!(report.warnings.is_empty(), "{:?}", report.warnings);
    assert_eq!(back.sheets.len(), 2);
    for (a, b) in wb.sheets.iter().zip(&back.sheets) {
        assert_eq!(a.name, b.name);
        assert_eq!(a.visibility, b.visibility);
        assert_eq!(a.tab_color, b.tab_color);
        assert_eq!(a.merges, b.merges);
        assert_eq!(a.cells.len(), b.cells.len(), "{}", a.name);
        for (pos, cell) in a.cells.iter() {
            let other = b.cell(pos).unwrap_or_else(|| panic!("{pos:?} missing"));
            assert_eq!(cell.value, other.value, "{pos:?}");
            assert_eq!(cell.formula.as_ref().map(|f| &f.text), other.formula.as_ref().map(|f| &f.text), "{pos:?}");
            assert_eq!(wb.styles.get(cell.style), back.styles.get(other.style), "{pos:?}");
        }
    }
    let (a, b) = (&wb.sheets[0], &back.sheets[0]);
    assert_eq!(b.cols.get(&1).and_then(|c| c.size), Some(150.0));
    assert!(b.cols.get(&2).is_some_and(|c| c.hidden));
    assert_eq!(b.rows.get(&0).and_then(|r| r.size), Some(40.0));
    assert!(b.rows.get(&6).is_some_and(|r| r.hidden));
    assert_eq!(a.default_col_width, b.default_col_width);
    let names: Vec<(&str, &str)> = back.names.iter().map(|n| (n.name.as_str(), n.formula.as_str())).collect();
    assert_eq!(names, vec![("Rate", "0.1"), ("Inputs", "Sheet1!$A$2:$B$2")]);
}

#[test]
fn ods_export_of_an_empty_workbook_reads_back() {
    let wb = gridcraft_model::Workbook::new();
    let (back, _) = read_ods(&crate::write_ods(&wb).unwrap()).unwrap();
    assert_eq!(back.sheets.len(), 1);
    assert!(back.sheets[0].cells.is_empty());
}

/// #754: a sheet renamed on import (too long, or a forbidden character) keeps its formulas and
/// names pointing at it.
#[test]
fn ods_formulas_follow_sheets_renamed_on_import() {
    let long = "ThisSheetNameIsWayLongerThanThirtyOneChars";
    let tables = format!(
        r#"<table:table table:name="{long}"><table:table-row><table:table-cell office:value-type="float" office:value="5"/></table:table-row></table:table>
        <table:table table:name="Uses"><table:table-row><table:table-cell table:formula="of:=['{long}'.A1]*2" office:value-type="float" office:value="10"/></table:table-row></table:table>
        <table:named-expressions><table:named-range table:name="Src" table:base-cell-address="$Uses.$A$1" table:cell-range-address="$'{long}'.$A$1"/></table:named-expressions>"#
    );
    let (wb, report) = read_ods(&ods(&tables)).unwrap();
    let stored = wb.sheets[0].name.clone();
    assert_ne!(stored, long);
    assert!(report.warnings.iter().any(|w| w.contains("renamed")));
    let f = wb.sheets[1].cell(CellRef::new(0, 0)).unwrap().formula.clone().unwrap();
    assert_eq!(f.text, format!("{}!A1*2", gridcraft_formula::quote_sheet(&stored)));
    assert_eq!(wb.names[0].formula, format!("{}!$A$1", gridcraft_formula::quote_sheet(&stored)));
}
