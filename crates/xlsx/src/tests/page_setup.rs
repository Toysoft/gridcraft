//! A sheet without `<pageSetup>` is saved without one. `<pageSetup>` without `paperSize` is
//! Letter (ECMA-376 gives `paperSize` the default 1), while a sheet without it is printed on the
//! application's default paper (seen in LibreOffice), so adding one on save moved sheets from
//! that paper to Letter. The paper, orientation, scale, Fit To and printer settings the file has
//! are kept, including paper sizes GridCraft has no name for.

use std::io::Read;

use super::minimal;
use crate::{read_xlsx, write_xlsx};

const DATA: &str = r#"<sheetData><row r="1"><c r="A1"><v>1</v></c></row></sheetData>"#;
const MARGINS: &str = r#"<pageMargins left="0.7" right="0.7" top="0.75" bottom="0.75" header="0.3" footer="0.3"/>"#;

/// The sheet part of a saved workbook.
fn sheet_xml(bytes: &[u8]) -> String {
    let mut z = zip::ZipArchive::new(std::io::Cursor::new(bytes)).unwrap();
    let mut x = String::new();
    z.by_name("xl/worksheets/sheet1.xml").unwrap().read_to_string(&mut x).unwrap();
    x
}

/// The sheet XML after opening a workbook whose sheet holds `body` and saving it again.
fn resaved(body: &str) -> String {
    let (wb, _) = read_xlsx(&minimal(body, &[], "", "")).unwrap();
    sheet_xml(&write_xlsx(&wb).unwrap())
}

/// The `<pageSetup …/>` element of sheet XML `x`.
fn page_setup(x: &str) -> Option<&str> {
    let i = x.find("<pageSetup")?;
    let len = x[i..].find("/>")? + 2;
    x.get(i..i + len)
}

#[test]
fn a_sheet_without_page_setup_is_saved_without_one() {
    for body in [DATA.to_string(), format!("{DATA}{MARGINS}")] {
        let x = resaved(&body);
        assert_eq!(page_setup(&x), None, "{body}: {x}");
    }
    let x = sheet_xml(&write_xlsx(&gridcraft_model::Workbook::new()).unwrap());
    assert_eq!(page_setup(&x), None, "a new workbook: {x}");
}

#[test]
fn page_setup_from_the_file_is_kept() {
    let x = resaved(&format!(r#"{DATA}{MARGINS}<pageSetup paperSize="9" orientation="landscape"/>"#));
    let ps = page_setup(&x).unwrap_or_else(|| panic!("{x}"));
    assert!(ps.contains(r#"paperSize="9""#) && ps.contains(r#"orientation="landscape""#), "{ps}");
    // Without a paper size it is Letter, so it stays: dropping it would leave the paper to the
    // application.
    for attrs in [r#"orientation="portrait""#, r#"paperSize="1""#, ""] {
        let x = resaved(&format!("{DATA}{MARGINS}<pageSetup {attrs}/>"));
        let ps = page_setup(&x).unwrap_or_else(|| panic!("{attrs}: {x}"));
        assert!(!ps.contains("paperSize"), "{attrs}: {ps}");
    }
}

#[test]
fn paper_sizes_without_a_name_keep_their_number() {
    let x = resaved(&format!(r#"{DATA}{MARGINS}<pageSetup paperSize="70" orientation="landscape"/>"#));
    let ps = page_setup(&x).unwrap_or_else(|| panic!("{x}"));
    assert!(ps.contains(r#"paperSize="70""#) && ps.contains(r#"orientation="landscape""#), "{ps}");
}

#[test]
fn choosing_a_paper_replaces_a_number_kept_from_the_file() {
    let body = format!(r#"{DATA}{MARGINS}<pageSetup paperSize="70"/>"#);
    let (mut wb, _) = read_xlsx(&minimal(&body, &[], "", "")).unwrap();
    let pr = &mut wb.sheet_mut(0).unwrap().print;
    assert_eq!((pr.paper.as_str(), pr.paper_code, pr.page_setup), ("Letter", Some(70), true), "laid out as Letter, kept as 70");
    pr.set_paper("A4");
    let x = sheet_xml(&write_xlsx(&wb).unwrap());
    let ps = page_setup(&x).unwrap_or_else(|| panic!("{x}"));
    assert!(ps.contains(r#"paperSize="9""#) && !ps.contains("70"), "{ps}");
}

/// Workbooks built in code, or saved as JSON before `page_setup` existed, get `<pageSetup>` for
/// any value that isn't the default.
#[test]
fn values_that_are_not_the_default_are_saved_without_the_flag() {
    let mut wb = gridcraft_model::Workbook::new();
    let pr = &mut wb.sheet_mut(0).unwrap().print;
    assert!(!pr.page_setup);
    pr.scale = 80;
    pr.paper = "Legal".into();
    let x = sheet_xml(&write_xlsx(&wb).unwrap());
    let ps = page_setup(&x).unwrap_or_else(|| panic!("{x}"));
    assert!(ps.contains(r#"paperSize="5""#) && ps.contains(r#"scale="80""#), "{ps}");
}
