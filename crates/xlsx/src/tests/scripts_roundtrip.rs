//! Scripts (Automate tab) in their custom XML part: round trips, a foreign custom XML part next to
//! them, and damaged parts.

use std::io::Read;

use gridcraft_model::{Script, ScriptLanguage, Workbook};

use super::minimal;
use crate::{read_xlsx, write_xlsx};

fn parts(bytes: &[u8]) -> Vec<(String, Vec<u8>)> {
    let mut z = zip::ZipArchive::new(std::io::Cursor::new(bytes)).unwrap();
    (0..z.len())
        .map(|i| {
            let mut f = z.by_index(i).unwrap();
            let mut v = vec![];
            f.read_to_end(&mut v).unwrap();
            (f.name().to_string(), v)
        })
        .collect()
}

fn part(bytes: &[u8], name: &str) -> Option<String> {
    parts(bytes).into_iter().find(|(n, _)| n == name).map(|(_, v)| String::from_utf8(v).unwrap())
}

fn book(scripts: Vec<Script>) -> Workbook {
    Workbook { scripts, ..Workbook::new() }
}

#[test]
fn scripts_round_trip_with_awkward_text() {
    let code = "fn main(workbook) {\r\n\tlet s = \"<b> & ]]> 'q' \\\"\";\n\tprint(s + \"_x0041_\" + \"\u{1}\" + \"ü€😀\");\n}\n";
    let scripts = vec![
        Script { name: "Format <header> & \"totals\"".into(), language: ScriptLanguage::Rhai, code: code.into() },
        Script { name: "Recorded".into(), language: ScriptLanguage::Commands, code: r#"[{"command":"home.bold","params":{"range":"A1"}}]"#.into() },
        Script { name: "Empty".into(), language: ScriptLanguage::Rhai, code: String::new() },
    ];
    let bytes = write_xlsx(&book(scripts.clone())).unwrap();
    let (back, report) = read_xlsx(&bytes).unwrap();
    assert_eq!(back.scripts, scripts, "{:?}", report.warnings);
    // A second round trip neither duplicates nor loses them (the part isn't also passed through).
    let again = write_xlsx(&back).unwrap();
    let (back2, _) = read_xlsx(&again).unwrap();
    assert_eq!(back2.scripts, scripts);
    assert_eq!(parts(&again).iter().filter(|(n, _)| n.starts_with("customXml/item") && !n.contains("Props")).count(), 1);
}

#[test]
fn scripts_part_is_a_custom_xml_data_store_item() {
    let bytes = write_xlsx(&book(vec![Script { name: "S".into(), language: ScriptLanguage::Rhai, code: "print(1);".into() }])).unwrap();
    let item = part(&bytes, "customXml/item1.xml").unwrap();
    assert!(item.contains("<scripts xmlns=\"urn:gridcraft:scripts:1\">"), "{item}");
    assert!(item.contains("<script name=\"S\" language=\"rhai\">print(1);</script>"), "{item}");
    let props = part(&bytes, "customXml/itemProps1.xml").unwrap();
    assert!(props.contains("ds:itemID=") && props.contains("urn:gridcraft:scripts:1"), "{props}");
    let item_rels = part(&bytes, "customXml/_rels/item1.xml.rels").unwrap();
    assert!(item_rels.contains("relationships/customXmlProps") && item_rels.contains("itemProps1.xml"), "{item_rels}");
    let wb_rels = part(&bytes, "xl/_rels/workbook.xml.rels").unwrap();
    assert!(wb_rels.contains("relationships/customXml\" Target=\"../customXml/item1.xml\""), "{wb_rels}");
    let types = part(&bytes, "[Content_Types].xml").unwrap();
    assert!(
        types.contains("/customXml/itemProps1.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.customXmlProperties+xml\""),
        "{types}"
    );
    // No scripts, no part.
    let plain = write_xlsx(&Workbook::new()).unwrap();
    assert!(!parts(&plain).iter().any(|(n, _)| n.starts_with("customXml/")));
}

/// A package with another application's custom XML part as `customXml/item1.xml`.
fn with_foreign_custom_xml() -> Vec<u8> {
    let rel = r#"<Relationship Id="rId9" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/customXml" Target="../customXml/item1.xml"/>"#;
    let item_rels = r#"<?xml version="1.0"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/customXmlProps" Target="itemProps1.xml"/></Relationships>"#;
    minimal(
        "<sheetData/>",
        &[
            ("customXml/item1.xml", r#"<?xml version="1.0"?><data xmlns="urn:example:other"><v>1</v></data>"#),
            ("customXml/_rels/item1.xml.rels", item_rels),
            (
                "customXml/itemProps1.xml",
                r#"<?xml version="1.0"?><ds:datastoreItem ds:itemID="{1}" xmlns:ds="http://schemas.openxmlformats.org/officeDocument/2006/customXml"/>"#,
            ),
        ],
        rel,
        "",
    )
}

#[test]
fn scripts_sit_next_to_another_custom_xml_part() {
    let (mut wb, _) = read_xlsx(&with_foreign_custom_xml()).unwrap();
    assert!(wb.scripts.is_empty());
    wb.scripts.push(Script { name: "Mine".into(), language: ScriptLanguage::Rhai, code: "1".into() });
    let bytes = write_xlsx(&wb).unwrap();
    // The other part keeps its name; the scripts take the next free item.
    assert!(part(&bytes, "customXml/item1.xml").unwrap().contains("urn:example:other"));
    assert!(part(&bytes, "customXml/item2.xml").unwrap().contains("urn:gridcraft:scripts:1"));
    let (back, _) = read_xlsx(&bytes).unwrap();
    assert_eq!(back.scripts.len(), 1);
    // Both survive another save, still once each.
    let again = write_xlsx(&back).unwrap();
    let items: Vec<String> =
        parts(&again).into_iter().filter(|(n, _)| n.starts_with("customXml/item") && !n.contains("Props")).map(|(n, _)| n).collect();
    assert_eq!(items.len(), 2, "{items:?}");
    let (back2, _) = read_xlsx(&again).unwrap();
    assert_eq!(back2.scripts, back.scripts);
}

#[test]
fn damaged_or_hostile_scripts_parts_are_skipped_with_a_warning() {
    let rel = r#"<Relationship Id="rId9" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/customXml" Target="../customXml/item1.xml"/>"#;
    // Not well-formed after the root: what parsed is kept, nothing panics.
    for body in [
        r#"<scripts xmlns="urn:gridcraft:scripts:1"><script name="A">1</script><script name="a">dup</script><script>no name</script><script name="B" language="??">2"#,
        r#"<scripts xmlns="urn:gridcraft:scripts:1"><<<"#,
        r#"<other xmlns="urn:gridcraft:scripts:1"/>"#,
    ] {
        let bytes = minimal("<sheetData/>", &[("customXml/item1.xml", body)], rel, "");
        let (wb, _) = read_xlsx(&bytes).unwrap();
        assert!(wb.scripts.len() <= 2, "{body}: {:?}", wb.scripts);
        for s in &wb.scripts {
            assert!(!s.name.is_empty());
        }
        // Saving again works.
        write_xlsx(&wb).unwrap();
    }
    let bytes = minimal(
        "<sheetData/>",
        &[("customXml/item1.xml", r#"<scripts xmlns="urn:gridcraft:scripts:1"><script name="A">1</script><script name="a">dup</script></scripts>"#)],
        rel,
        "",
    );
    let (wb, report) = read_xlsx(&bytes).unwrap();
    assert_eq!(wb.scripts.iter().map(|s| s.name.as_str()).collect::<Vec<_>>(), ["A"]);
    assert!(report.warnings.iter().any(|w| w.contains("repeated name")), "{:?}", report.warnings);
}
