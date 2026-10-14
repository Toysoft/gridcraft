//! Decode bitmap-only EMF pictures without a platform renderer or external processes.
//!
//! Some spreadsheet pictures contain just EMR_HEADER, EMR_STRETCHDIBITS and EMR_EOF.
//! Support that subset, with BI_RGB 24/32-bit pixels and SRCCOPY, and reject other drawing
//! records rather than silently rendering an incomplete vector metafile. Original bytes remain
//! in the workbook. Layout: MS-EMF 2.3.1.7 (EMR_STRETCHDIBITS).

const MAX_BYTES: usize = 64 * 1024 * 1024;
const MAX_SIDE: usize = 4_096;
const MAX_PIXELS: usize = MAX_BYTES / 4;

fn word(data: &[u8], offset: usize) -> Option<u32> {
    Some(u32::from_le_bytes(data.get(offset..offset.checked_add(4)?)?.try_into().ok()?))
}

fn number(data: &[u8], offset: usize) -> Option<usize> {
    usize::try_from(word(data, offset)?).ok()
}

fn positive(data: &[u8], offset: usize) -> Option<usize> {
    let value = usize::try_from(word(data, offset)? as i32).ok()?;
    (value > 0 && value <= MAX_SIDE).then_some(value)
}

fn record(data: &[u8], offset: usize, kind: u32) -> Option<&[u8]> {
    if word(data, offset)? != kind {
        return None;
    }
    let size = number(data, offset.checked_add(4)?)?;
    if size < 8 || size % 4 != 0 {
        return None;
    }
    data.get(offset..offset.checked_add(size)?)
}

/// A complete, single-bitmap EMF, or `None` for unsupported/malformed/oversized input.
pub(crate) fn decode(data: &[u8]) -> Option<image::RgbaImage> {
    if data.len() > MAX_BYTES {
        return None;
    }
    let header = record(data, 0, 1)?;
    if header.len() < 88 || word(header, 40)? != 0x464d_4520 || number(header, 48)? != data.len() || word(header, 52)? != 3 {
        return None;
    }
    let draw = record(data, header.len(), 81)?;
    let eof_at = header.len().checked_add(draw.len())?;
    let eof = record(data, eof_at, 14)?;
    if draw.len() < 80 || eof.len() < 20 || eof_at.checked_add(eof.len())? != data.len() {
        return None;
    }
    // No palette-index lookup or destination-dependent raster operation.
    if word(draw, 64)? != 0 || word(draw, 68)? != 0x00cc_0020 || word(draw, 24)? != 0 || word(draw, 28)? != 0 {
        return None;
    }
    let dest_w = positive(draw, 72)?;
    let dest_h = positive(draw, 76)?;
    // The bitmap must occupy the whole metafile bounds. Other placement/compositing is not
    // represented by painting one texture into the picture's anchor.
    for bounds in [header, draw] {
        if word(bounds, 8)? != 0
            || word(bounds, 12)? != 0
            || number(bounds, 16)? != dest_w.checked_sub(1)?
            || number(bounds, 20)? != dest_h.checked_sub(1)?
        {
            return None;
        }
    }
    let bmi_at = number(draw, 48)?;
    let bmi_len = number(draw, 52)?;
    let bits_at = number(draw, 56)?;
    let bits_len = number(draw, 60)?;
    let bmi_end = bmi_at.checked_add(bmi_len)?;
    let bits_end = bits_at.checked_add(bits_len)?;
    if bmi_at < 80 || bits_at < 80 || bmi_len != 40 || !(bmi_end <= bits_at || bits_end <= bmi_at) {
        return None;
    }
    let bmi = draw.get(bmi_at..bmi_end)?;
    let bits = draw.get(bits_at..bits_end)?;
    let pixel_format = word(bmi, 12)?;
    let bpp = pixel_format >> 16;
    if word(bmi, 0)? != 40 || pixel_format & 0xffff != 1 || !matches!(bpp, 24 | 32) || word(bmi, 16)? != 0 || word(bmi, 32)? != 0 {
        return None;
    }
    let width = positive(bmi, 4)?;
    let signed_height = word(bmi, 8)? as i32;
    let height = usize::try_from(signed_height.unsigned_abs()).ok()?;
    if height == 0 || height > MAX_SIDE || width.checked_mul(height)? > MAX_PIXELS {
        return None;
    }
    let bytes_per_pixel = usize::try_from(bpp / 8).ok()?;
    let stride = width.checked_mul(bytes_per_pixel)?.checked_add(3)? & !3;
    if bits.len() < stride.checked_mul(height)? {
        return None;
    }
    let src_x = usize::try_from(word(draw, 32)? as i32).ok()?;
    let src_y = usize::try_from(word(draw, 36)? as i32).ok()?;
    let src_w = positive(draw, 40)?;
    let src_h = positive(draw, 44)?;
    if src_x >= width || src_y >= height {
        return None;
    }
    let pixels = src_w.checked_mul(src_h)?;
    if pixels > MAX_PIXELS {
        return None;
    }
    let size = pixels.checked_mul(4)?;
    let mut rgba = Vec::new();
    rgba.try_reserve_exact(size).ok()?;
    rgba.resize(size, 0);
    // Source extents can include a few pixels outside the DIB. Preserve that transparent
    // border rather than reading beyond the bitmap or changing the picture's aspect ratio.
    for y in 0..src_h.min(height.checked_sub(src_y)?) {
        let source_y = src_y.checked_add(y)?;
        let row = if signed_height < 0 { source_y } else { height.checked_sub(source_y.checked_add(1)?)? };
        for x in 0..src_w.min(width.checked_sub(src_x)?) {
            let at = row.checked_mul(stride)?.checked_add(src_x.checked_add(x)?.checked_mul(bytes_per_pixel)?)?;
            let pixel = bits.get(at..at.checked_add(bytes_per_pixel)?)?;
            let out = y.checked_mul(src_w)?.checked_add(x)?.checked_mul(4)?;
            // BI_RGB 32-bit pixels are BGRX, not BGRA: the reserved byte is often zero.
            rgba.get_mut(out..out.checked_add(4)?)?.copy_from_slice(&[*pixel.get(2)?, *pixel.get(1)?, *pixel.first()?, 255]);
        }
    }
    image::RgbaImage::from_raw(u32::try_from(src_w).ok()?, u32::try_from(src_h).ok()?, rgba)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn put(data: &mut [u8], at: usize, value: u32) {
        data[at..at + 4].copy_from_slice(&value.to_le_bytes());
    }

    pub(super) fn fixture(bpp: u32, top_down: bool, border: bool) -> Vec<u8> {
        let stride = 8usize;
        let record_size = 120 + stride * 2;
        let mut data = vec![0; 88 + record_size + 20];
        let size = data.len() as u32;
        let canvas_w = if border { 3 } else { 2 };
        put(&mut data, 0, 1);
        put(&mut data, 4, 88);
        put(&mut data, 16, canvas_w - 1);
        put(&mut data, 20, 1);
        put(&mut data, 40, 0x464d_4520);
        put(&mut data, 44, 0x0001_0000);
        put(&mut data, 48, size);
        put(&mut data, 52, 3);
        let draw = &mut data[88..88 + record_size];
        put(draw, 0, 81);
        put(draw, 4, record_size as u32);
        put(draw, 16, canvas_w - 1);
        put(draw, 20, 1);
        put(draw, 40, canvas_w);
        put(draw, 44, 2);
        put(draw, 48, 80);
        put(draw, 52, 40);
        put(draw, 56, 120);
        put(draw, 60, (stride * 2) as u32);
        put(draw, 68, 0x00cc_0020);
        put(draw, 72, canvas_w);
        put(draw, 76, 2);
        put(draw, 80, 40);
        put(draw, 84, 2);
        put(draw, 88, if top_down { (-2i32) as u32 } else { 2 });
        put(draw, 92, (bpp << 16) | 1);
        put(draw, 100, (stride * 2) as u32);
        let rows = if top_down { [[255, 0, 0], [0, 0, 255]] } else { [[0, 0, 255], [255, 0, 0]] };
        for (y, rgb) in rows.iter().enumerate() {
            for x in 0..2 {
                let at = 120 + y * stride + x * (bpp / 8) as usize;
                draw[at..at + 3].copy_from_slice(&[rgb[2], rgb[1], rgb[0]]);
            }
        }
        put(&mut data, 88 + record_size, 14);
        put(&mut data, 92 + record_size, 20);
        data
    }

    #[test]
    fn bitmap_emf_renders_bgrx_and_padded_bgr_rows_in_both_orientations() {
        for bpp in [24, 32] {
            for top_down in [false, true] {
                let data = fixture(bpp, top_down, false);
                assert!(image::load_from_memory(&data).is_err(), "the ordinary image decoder cannot decode EMF");
                let img = crate::chartview::decode_picture(&data).expect("the picture renderer decodes bitmap EMF").to_rgba8();
                assert_eq!(img.dimensions(), (2, 2));
                assert_eq!(img.get_pixel(0, 0).0, [255, 0, 0, 255]);
                assert_eq!(img.get_pixel(1, 1).0, [0, 0, 255, 255]);
            }
        }
    }

    #[test]
    fn source_extents_beyond_the_dib_keep_a_transparent_border() {
        let img = decode(&fixture(32, false, true)).unwrap();
        assert_eq!(img.dimensions(), (3, 2));
        assert_eq!(img.get_pixel(1, 0).0, [255, 0, 0, 255]);
        assert_eq!(img.get_pixel(2, 0).0, [0, 0, 0, 0]);
    }

    #[test]
    fn truncated_hostile_or_non_bitmap_metafiles_are_refused() {
        let data = fixture(32, false, false);
        for end in 0..data.len() {
            assert!(decode(&data[..end]).is_none());
        }
        for offset in [4, 40, 48, 52, 88, 92, 88 + 48, 88 + 52, 88 + 56, 88 + 60, 88 + 64, 88 + 68, 88 + 72, 88 + 84, 88 + 96] {
            let mut bad = data.clone();
            put(&mut bad, offset, u32::MAX);
            assert!(decode(&bad).is_none(), "hostile field at {offset}");
        }
        let mut too_tall = data.clone();
        put(&mut too_tall, 88 + 88, i32::MIN as u32);
        assert!(decode(&too_tall).is_none());
        let mut vector = data;
        put(&mut vector, 88, 43); // EMR_RECTANGLE is not the single-bitmap subset.
        assert!(decode(&vector).is_none());
    }
}
