//! Painting ink strokes: an antialiased mesh for opaque pens, a coverage texture for
//! translucent ones (so bends and crossings don't stack alpha).

use egui::{Color32, Painter, Pos2, Rect, pos2, vec2};
use gridcraft_engine::model::{Ink, Shape, Theme};

/// Paints `shape`'s stroke with its anchor origin at `origin`, `scale` screen points per sheet
/// pixel. A `live` stroke (still being drawn) changes every frame, so its texture isn't cached.
pub(crate) fn paint_ink(p: &Painter, shape: &Shape, theme: &Theme, origin: Pos2, scale: f32, live: bool) {
    let Some(ink) = &shape.ink else { return };
    if !scale.is_finite() || scale <= 0.0 {
        return;
    }
    let base = shape.line.resolve(theme).unwrap_or([0, 0, 0]);
    let color = |q: [f32; 2]| {
        let c = ink.color_at(q, base);
        Color32::from_rgb(c[0], c[1], c[2])
    };
    if ink.opacity() < 1.0 {
        paint_translucent(p, ink, base, origin, scale, live);
        return;
    }
    let mut mesh = egui::Mesh::default();
    for t in ink.polygons() {
        let first = mesh.vertices.len() as u32;
        for q in t {
            mesh.colored_vertex(origin + vec2(q[0], q[1]) * scale, color(q));
        }
        mesh.add_triangle(first, first + 1, first + 2);
    }
    // A one-pixel transparent fringe along the outline antialiases the edge without seams
    // between the triangles.
    let outline = ink.outline();
    let n = outline.len();
    let area = outline.iter().zip(outline.iter().cycle().skip(1)).take(n).map(|(a, b)| a[0] * b[1] - b[0] * a[1]).sum::<f32>();
    let first = mesh.vertices.len() as u32;
    for (i, q) in outline.iter().enumerate() {
        let before = outline.get((i + n - 1) % n).copied().unwrap_or(*q);
        let after = outline.get((i + 1) % n).copied().unwrap_or(*q);
        let tangent = vec2(after[0] - before[0], after[1] - before[1]).normalized();
        let outward = vec2(tangent.y, -tangent.x) * if area >= 0.0 { 1.0 } else { -1.0 };
        let at = origin + vec2(q[0], q[1]) * scale;
        mesh.colored_vertex(at, color(*q));
        mesh.colored_vertex(at + outward, Color32::TRANSPARENT);
    }
    for i in 0..n {
        let a = first + (i * 2) as u32;
        let b = first + (((i + 1) % n) * 2) as u32;
        mesh.add_triangle(a, b, b + 1);
        mesh.add_triangle(a, b + 1, a + 1);
    }
    p.add(mesh);
}

type CacheEntry = (u64, egui::TextureHandle, [f32; 2], [f32; 2], usize);

/// Rasterizes the stroke's coverage (the union of its segments) and applies the opacity once.
fn paint_translucent(p: &Painter, ink: &Ink, base: [u8; 3], origin: Pos2, scale: f32, live: bool) {
    use std::hash::{Hash, Hasher};
    let points: Vec<_> = ink.valid_points().collect();
    if points.is_empty() {
        return;
    }
    let ppp = p.ctx().pixels_per_point();
    let mut hash = std::collections::hash_map::DefaultHasher::new();
    for q in points.iter().flatten().chain(&ink.pressures).chain(&ink.effect_origin).chain(&[ink.width, scale, ppp]) {
        q.to_bits().hash(&mut hash);
    }
    (ink.tool, ink.effect, base).hash(&mut hash);
    let key = hash.finish();
    let cache_id = egui::Id::new("translucent_ink_union");
    let live_id = egui::Id::new("translucent_ink_live");
    let cached = if live {
        p.ctx().data(|d| d.get_temp::<CacheEntry>(live_id)).filter(|e| e.0 == key)
    } else {
        p.ctx().data_mut(|d| d.get_temp_mut_or_default::<Vec<CacheEntry>>(cache_id).iter().find(|e| e.0 == key).cloned())
    };
    let (_, texture, min, extent, _) = match cached {
        Some(entry) => entry,
        None => {
            let entry = rasterize(p.ctx(), ink, &points, base, scale * ppp, key);
            p.ctx().data_mut(|d| {
                if live {
                    // One live texture at a time: the previous frame's is dropped.
                    d.insert_temp(live_id, entry.clone());
                } else {
                    let cache = d.get_temp_mut_or_default::<Vec<CacheEntry>>(cache_id);
                    while !cache.is_empty() && (cache.len() >= 512 || cache.iter().map(|e| e.4).sum::<usize>().saturating_add(entry.4) > 32_000_000) {
                        let _ = cache.remove(0);
                    }
                    cache.push(entry.clone());
                }
            });
            entry
        }
    };
    p.image(
        texture.id(),
        Rect::from_min_size(origin + vec2(min[0], min[1]) * scale, vec2(extent[0], extent[1]) * scale),
        Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)),
        Color32::WHITE,
    );
}

fn rasterize(ctx: &egui::Context, ink: &Ink, points: &[[f32; 2]], base: [u8; 3], pixels_per_unit: f32, key: u64) -> CacheEntry {
    let radius = ink.max_width() * 0.5 + 2.0 / pixels_per_unit.max(0.001);
    let lo = [points.iter().map(|q| q[0]).fold(f32::INFINITY, f32::min) - radius, points.iter().map(|q| q[1]).fold(f32::INFINITY, f32::min) - radius];
    let extent = [
        points.iter().map(|q| q[0]).fold(f32::NEG_INFINITY, f32::max) - lo[0] + radius,
        points.iter().map(|q| q[1]).fold(f32::NEG_INFINITY, f32::max) - lo[1] + radius,
    ];
    // Bound both memory and work, including hostile long diagonal paths.
    let work = points
        .windows(2)
        .map(|q| ((q[1][0] - q[0][0]).abs() + 2.0 * radius) as f64 * ((q[1][1] - q[0][1]).abs() + 2.0 * radius) as f64)
        .sum::<f64>()
        .max(1.0);
    let density = pixels_per_unit
        .min(2048.0 / extent[0].max(extent[1]))
        .min((1_000_000.0 / (extent[0] as f64 * extent[1] as f64)).sqrt() as f32)
        .min((4_000_000.0 / work).sqrt() as f32)
        .max(0.000001);
    let width = ((extent[0] * density).ceil() as usize).clamp(1, 2048);
    let height = ((extent[1] * density).ceil() as usize).clamp(1, 2048);
    let mut coverage = vec![0.0f32; width.saturating_mul(height)];
    let last = points.len().saturating_sub(1);
    for i in 0..last.max(1) {
        let Some(&a) = points.get(i) else { continue };
        let b = points.get(i + 1).copied().unwrap_or(a);
        let (ra, rb) = (ink.point_width(i) * 0.5, ink.point_width((i + 1).min(last)) * 0.5);
        let r = ra.max(rb) + 1.0 / density;
        let span = |lo_v: f32, hi_v: f32, origin: f32, limit: usize| {
            let from = (((lo_v - r - origin) * density).floor().max(0.0) as usize).min(limit);
            let to = (((hi_v + r - origin) * density).ceil().max(0.0) as usize).min(limit);
            from..to
        };
        let (dx, dy) = (b[0] - a[0], b[1] - a[1]);
        let length = dx * dx + dy * dy;
        for y in span(a[1].min(b[1]), a[1].max(b[1]), lo[1], height) {
            for x in span(a[0].min(b[0]), a[0].max(b[0]), lo[0], width) {
                let q = [lo[0] + (x as f32 + 0.5) / density, lo[1] + (y as f32 + 0.5) / density];
                let t = if length > 0.0 { ((q[0] - a[0]) * dx + (q[1] - a[1]) * dy) / length } else { 0.0 }.clamp(0.0, 1.0);
                let distance = (q[0] - a[0] - t * dx).hypot(q[1] - a[1] - t * dy);
                let alpha = ((ra + (rb - ra) * t - distance) * density + 0.5).clamp(0.0, 1.0);
                if let Some(v) = coverage.get_mut(y * width + x) {
                    *v = v.max(alpha);
                }
            }
        }
    }
    let pixels = coverage
        .iter()
        .enumerate()
        .map(|(i, &alpha)| {
            let c = ink.color_at([lo[0] + (i % width) as f32 / density, lo[1] + (i / width) as f32 / density], base);
            Color32::from_rgba_unmultiplied(c[0], c[1], c[2], (alpha * ink.opacity() * 255.0).round() as u8)
        })
        .collect();
    let texture = ctx.load_texture(format!("ink-{key}"), egui::ColorImage::new([width, height], pixels), egui::TextureOptions::LINEAR);
    (key, texture, lo, [width as f32 / density, height as f32 / density], width.saturating_mul(height))
}

#[cfg(test)]
mod tests {
    use super::*;
    use gridcraft_engine::model::{Color, InkTool};

    #[test]
    fn highlighter_bends_and_crossings_apply_alpha_once() {
        let ink = Ink {
            points: vec![[0.0, 0.0], [60.0, 60.0], [0.0, 60.0], [60.0, 0.0], [60.0, 60.0]],
            width: 20.0,
            tool: InkTool::Highlighter,
            ..Ink::default()
        };
        let shape = Shape::new_ink(1, Default::default(), Color::Rgb(0x808080), ink);
        let ctx = egui::Context::default();
        let mut output = ctx.run_ui(Default::default(), |ui| {
            let p = ui.ctx().layer_painter(egui::LayerId::new(egui::Order::Middle, egui::Id::new("ink")));
            paint_ink(&p, &shape, &Theme::default(), pos2(100.0, 100.0), 1.0, false);
        });
        let mut covered = 0;
        for (id, deltas) in &output.textures_delta.set {
            if *id == egui::TextureId::Managed(0) {
                continue;
            }
            for delta in deltas {
                let egui::ImageData::Color(image) = &delta.image;
                for color in &image.pixels {
                    assert!(color.a() <= 102);
                    covered += usize::from(color.a() == 102);
                }
            }
        }
        output.textures_delta.clear();
        assert!(covered > 100);
    }

    #[test]
    fn live_strokes_keep_one_texture() {
        let ctx = egui::Context::default();
        for n in 2..6 {
            let points = (0..n).map(|i| [i as f32 * 10.0, 0.0]).collect();
            let shape = Shape::new_ink(0, Default::default(), Color::Rgb(0), Ink { points, tool: InkTool::Highlighter, ..Ink::default() });
            let mut output = ctx.run_ui(Default::default(), |ui| {
                let p = ui.ctx().layer_painter(egui::LayerId::new(egui::Order::Middle, egui::Id::new("ink")));
                paint_ink(&p, &shape, &Theme::default(), pos2(0.0, 0.0), 1.0, true);
            });
            output.textures_delta.clear();
        }
        let cached = ctx.data_mut(|d| d.get_temp_mut_or_default::<Vec<CacheEntry>>(egui::Id::new("translucent_ink_union")).len());
        assert_eq!(cached, 0, "live strokes must not fill the stroke cache");
    }
}
