//! Code-generated geometry/font/picture fixture for fixed drawing export comparison.
//! cargo run --release -p gridcraft-ui-egui --example fixed_picture_repro -- scale.xlsx
use gridcraft_engine::Session;
use gridcraft_engine::core::CellRef;
use gridcraft_engine::model::{Anchor, AnchorMode, Image};
use serde_json::json;
use std::io::Cursor;
use std::sync::Arc;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let out = std::env::args().nth(1).ok_or("usage: fixed_picture_repro out.xlsx")?;
    let mut s = Session::new();
    s.new_workbook();
    s.execute("range.setValues", json!({"range":"A1","values":[["SYNTHETIC LINUX SCALE"],["Compare at identical zoom"],["Hg0123456789"],["12 pt DejaVu Sans"],["中文尺寸測試 012345"],["12 pt Noto Sans CJK TC"]]}))?;
    s.execute("home.fontName", json!({"range":"A1:A4","name":"DejaVu Sans"}))?;
    s.execute("home.fontName", json!({"range":"A5:A6","name":"Noto Sans CJK TC"}))?;
    s.execute("home.fontSize", json!({"range":"A1:A6","size":12}))?;
    s.execute("home.columnWidth", json!({"cols":"A:L","width":96}))?;
    s.execute("home.rowHeight", json!({"rows":"1:30","height":24}))?;
    s.execute("home.fillColor", json!({"range":"A8:D8","color":"#20B0D0"}))?;
    s.execute("view.zoom", json!({"percent":100}))?;
    let png = image::RgbImage::from_fn(320, 160, |_, _| image::Rgb([208, 32, 224]));
    let mut encoded = Cursor::new(Vec::new());
    image::DynamicImage::ImageRgb8(png).write_to(&mut encoded, image::ImageFormat::Png)?;
    let wb = Arc::make_mut(&mut s.active_mut().ok_or("no document")?.wb);
    Arc::make_mut(wb.sheets.first_mut().ok_or("no sheet")?).images.push(Image {
        id: 1,
        anchor: Anchor { cell: CellRef::new(9, 1), width: 320.0, height: 160.0, mode: AnchorMode::Absolute, ..Default::default() },
        data: encoded.into_inner(),
        mime: "image/png".into(),
        alt: "Original synthetic 320 by 160 pixel rectangle".into(),
    });
    std::fs::write(out, gridcraft_xlsx::write_xlsx(wb)?)?;
    Ok(())
}
