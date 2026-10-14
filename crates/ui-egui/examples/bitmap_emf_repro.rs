//! Generate a wholly synthetic XLSX for bitmap EMF + external-link import-note reproduction.
//! No vendor-created workbook, picture, font or private data is used.
//! cargo run --release -p gridcraft-ui-egui --example bitmap_emf_repro -- out.xlsx

use gridcraft_engine::Session;
use gridcraft_engine::core::CellRef;
use gridcraft_engine::model::{Anchor, Image, Preserved, PreservedPart, PreservedRel, PreservedXml};
use serde_json::json;
use std::sync::Arc;

fn put(data: &mut [u8], at: usize, value: u32) {
    data[at..at + 4].copy_from_slice(&value.to_le_bytes());
}

fn bitmap_emf() -> Vec<u8> {
    let (width, height) = (160usize, 96usize);
    let bits = width * height * 4;
    let drawing_size = 120 + bits;
    let mut data = vec![0; 108 + drawing_size + 20];
    let size = data.len() as u32;
    put(&mut data, 0, 1);
    put(&mut data, 4, 108);
    put(&mut data, 16, width as u32 - 1);
    put(&mut data, 20, height as u32 - 1);
    put(&mut data, 40, 0x464d_4520);
    put(&mut data, 44, 0x0001_0000);
    put(&mut data, 48, size);
    put(&mut data, 52, 3);
    let draw = &mut data[108..108 + drawing_size];
    put(draw, 0, 81);
    put(draw, 4, drawing_size as u32);
    put(draw, 16, width as u32 - 1);
    put(draw, 20, height as u32 - 1);
    put(draw, 40, width as u32);
    put(draw, 44, height as u32);
    put(draw, 48, 80);
    put(draw, 52, 40);
    put(draw, 56, 120);
    put(draw, 60, bits as u32);
    put(draw, 68, 0x00cc_0020); // SRCCOPY
    put(draw, 72, width as u32);
    put(draw, 76, height as u32);
    put(draw, 80, 40); // BITMAPINFOHEADER
    put(draw, 84, width as u32);
    put(draw, 88, height as u32);
    put(draw, 92, (32 << 16) | 1); // BI_RGB BGRX; reserved alpha is zero
    put(draw, 100, bits as u32);
    for y in 0..height {
        for x in 0..width {
            let rgb = match (x >= width / 2, height - 1 - y >= height / 2) {
                (false, false) => [230, 60, 60],
                (true, false) => [50, 190, 100],
                (false, true) => [40, 110, 230],
                (true, true) => [245, 200, 40],
            };
            let at = 120 + (y * width + x) * 4;
            draw[at..at + 4].copy_from_slice(&[rgb[2], rgb[1], rgb[0], 0]);
        }
    }
    put(&mut data, 108 + drawing_size, 14);
    put(&mut data, 112 + drawing_size, 20);
    data
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let out = std::env::args().nth(1).ok_or("usage: bitmap_emf_repro out.xlsx")?;
    let mut session = Session::new();
    session.new_workbook();
    session.execute("range.setValues", json!({"range":"A1", "values":[["SYNTHETIC BITMAP EMF REPRO"],["Generated test data only"],["Expected: four colored quadrants below"],["External link is metadata only; do not refresh"]]}))?;
    session.execute("home.fontSize", json!({"range":"A1", "size":18}))?;
    session.execute("home.bold", json!({"range":"A1", "value":true}))?;
    let wb = &mut session.active_mut().ok_or("no document")?.wb;
    let wb = Arc::make_mut(wb);
    Arc::make_mut(wb.sheets.first_mut().ok_or("no sheet")?).images.push(Image {
        id: 1,
        anchor: Anchor { cell: CellRef::new(5, 1), width: 360.0, height: 216.0, ..Default::default() },
        data: bitmap_emf(),
        mime: "image/x-emf".into(),
        alt: "Synthetic red green blue yellow quadrants".into(),
    });
    let ns = "http://schemas.openxmlformats.org/officeDocument/2006/relationships";
    let rid = "syntheticLink";
    wb.preserved = Some(Arc::new(Preserved {
        rels: vec![PreservedRel { id:rid.into(), rel_type:format!("{ns}/externalLink"), target:"xl/externalLinks/externalLink1.xml".into(), external:false }],
        parts: vec![PreservedPart {
            name:"xl/externalLinks/externalLink1.xml".into(), content_type:"application/vnd.openxmlformats-officedocument.spreadsheetml.externalLink+xml".into(),
            data: Arc::from(format!("<externalLink xmlns=\"http://schemas.openxmlformats.org/spreadsheetml/2006/main\" xmlns:r=\"{ns}\"><externalBook r:id=\"source\"><sheetNames><sheetName val=\"SyntheticSource\"/></sheetNames></externalBook></externalLink>").into_bytes()),
            rels:vec![PreservedRel { id:"source".into(), rel_type:format!("{ns}/externalLinkPath"), target:"https://example.invalid/synthetic-source.xlsx".into(), external:true }],
        }],
        elements: vec![PreservedXml { slot:"externalReferences".into(), xml:format!("<externalReferences xmlns:r=\"{ns}\"><externalReference r:id=\"{rid}\"/></externalReferences>") }],
        ..Default::default()
    }));
    std::fs::write(out, gridcraft_xlsx::write_xlsx(wb)?)?;
    Ok(())
}
