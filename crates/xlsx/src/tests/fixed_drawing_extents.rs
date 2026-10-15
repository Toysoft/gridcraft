//! A fixed-size drawing must not acquire the importing application's substitute column width.
use crate::{read_xlsx, write_xlsx, xml};
use gridcraft_core::CellRef;
use gridcraft_model::{Anchor, AnchorMode, Image, Sheet, Workbook};
use std::{io::Read, sync::Arc};

#[test]
fn fixed_picture_extents_survive_different_column_metrics() {
    for mode in [AnchorMode::Absolute, AnchorMode::MoveOnly, AnchorMode::MoveAndSize] {
        let mut sheet = Sheet::new("Synthetic");
        sheet.images.push(Image {
            id: 1,
            anchor: Anchor { cell: CellRef::new(2, 1), dx: 5.0, dy: 3.0, width: 320.0, height: 160.0, mode },
            data: vec![1, 2, 3],
            mime: "image/png".into(),
            alt: "synthetic extent".into(),
        });
        let book = Workbook { sheets: vec![Arc::new(sheet)], ..Default::default() };
        let bytes = write_xlsx(&book).unwrap();
        let mut input = zip::ZipArchive::new(std::io::Cursor::new(bytes)).unwrap();
        let mut drawing = String::new();
        input.by_name("xl/drawings/drawing1.xml").unwrap().read_to_string(&mut drawing).unwrap();
        let root = xml::parse(drawing.as_bytes()).unwrap();
        let anchor = &root.children[0];
        match mode {
            AnchorMode::Absolute => {
                assert_eq!(anchor.name, "absoluteAnchor");
                assert_eq!(anchor.child("pos").unwrap().attr("x"), Some("657225"));
            }
            AnchorMode::MoveOnly => assert_eq!(anchor.name, "oneCellAnchor"),
            AnchorMode::MoveAndSize => assert_eq!(anchor.name, "twoCellAnchor"),
        }
        if mode != AnchorMode::MoveAndSize {
            assert_eq!(anchor.child("ext").unwrap().attr("cx"), Some("3048000"));
            assert_eq!(anchor.child("ext").unwrap().attr("cy"), Some("1524000"));
        }
        // Simulate a different font's column metrics by changing the worksheet default
        // width after serialization, without touching any drawing XML or media.
        let mut output = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
        for i in 0..input.len() {
            let mut part = input.by_index(i).unwrap();
            let name = part.name().to_string();
            let mut data = Vec::new();
            part.read_to_end(&mut data).unwrap();
            if name == "xl/worksheets/sheet1.xml" {
                data = String::from_utf8(data).unwrap().replace("<sheetFormatPr ", "<sheetFormatPr defaultColWidth=\"20\" ").into_bytes();
            }
            output.start_file(name, zip::write::SimpleFileOptions::default()).unwrap();
            std::io::Write::write_all(&mut output, &data).unwrap();
        }
        let (reopened, _) = read_xlsx(&output.finish().unwrap().into_inner()).unwrap();
        let got = reopened.sheets[0].images[0].anchor;
        assert_eq!(got.mode, mode);
        assert_eq!(got.height, 160.0);
        if mode == AnchorMode::MoveAndSize {
            assert!(got.width > 320.0, "cell-relative drawings retain resize-with-cells behavior");
        } else {
            assert_eq!(got.width, 320.0, "explicit extents do not depend on substitute columns");
        }
        if mode == AnchorMode::Absolute {
            let x = reopened.sheets[0].col_left(got.cell.col) + got.dx as f64;
            assert_eq!(x, 69.0, "absolute position is independent of column metrics");
        } else {
            assert_eq!(got.cell, CellRef::new(2, 1));
            assert_eq!(got.dx, 5.0);
        }
    }
}
