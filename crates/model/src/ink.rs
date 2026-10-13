//! Freehand ink: the stroke model, pen presets and the bounded stroke geometry shared by the
//! screen, XLSX and PDF output.

use serde::{Deserialize, Deserializer, Serialize};

use crate::{Anchor, Color, Shape, ShapeKind, Theme};

/// Most centerline points one stroke keeps (input, files and erasers alike).
pub const MAX_INK_POINTS: usize = 20_000;
/// Pen width bounds in sheet pixels.
pub const MIN_INK_WIDTH: f32 = 0.1;
pub const MAX_INK_WIDTH: f32 = 40.0;
/// Largest coordinate a point may have (sheet pixels); larger values are hostile input.
pub const MAX_INK_COORD: f32 = 1e7;

/// The kind of pen that drew a stroke: it sets the stroke's opacity.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum InkTool {
    #[default]
    Pen,
    Pencil,
    Highlighter,
}

impl InkTool {
    pub const ALL: [InkTool; 3] = [InkTool::Pen, InkTool::Pencil, InkTool::Highlighter];

    /// The id used in command parameters and files.
    pub fn id(self) -> &'static str {
        match self {
            InkTool::Pen => "pen",
            InkTool::Pencil => "pencil",
            InkTool::Highlighter => "highlighter",
        }
    }
    pub fn parse(id: &str) -> Option<InkTool> {
        InkTool::ALL.into_iter().find(|t| t.id() == id)
    }
    pub fn label(self) -> &'static str {
        match self {
            InkTool::Pen => "Pen",
            InkTool::Pencil => "Pencil",
            InkTool::Highlighter => "Highlighter",
        }
    }
    pub fn opacity(self) -> f32 {
        match self {
            InkTool::Pen => 1.0,
            InkTool::Pencil => 0.7,
            InkTool::Highlighter => 0.4,
        }
    }
    /// A new pen's width in sheet pixels (0.5 mm pens, 6 mm highlighters).
    pub fn default_width(self) -> f32 {
        mm_to_px(if self == InkTool::Highlighter { 6.0 } else { 0.5 })
    }
}

/// A procedural pen texture. The colours are GridCraft's own, computed in code.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum InkEffect {
    Rainbow,
    Galaxy,
    Lava,
    Ocean,
    RoseGold,
    Gold,
    Silver,
    Bronze,
}

impl InkEffect {
    pub const ALL: [InkEffect; 8] = [
        InkEffect::Rainbow,
        InkEffect::Galaxy,
        InkEffect::Lava,
        InkEffect::Ocean,
        InkEffect::RoseGold,
        InkEffect::Gold,
        InkEffect::Silver,
        InkEffect::Bronze,
    ];

    /// The id used in command parameters and files (InkML `inkEffects`).
    pub fn id(self) -> &'static str {
        match self {
            InkEffect::Rainbow => "rainbow",
            InkEffect::Galaxy => "galaxy",
            InkEffect::Lava => "lava",
            InkEffect::Ocean => "ocean",
            InkEffect::RoseGold => "roseGold",
            InkEffect::Gold => "gold",
            InkEffect::Silver => "silver",
            InkEffect::Bronze => "bronze",
        }
    }
    pub fn parse(id: &str) -> Option<InkEffect> {
        InkEffect::ALL.into_iter().find(|e| e.id() == id)
    }
    pub fn label(self) -> &'static str {
        match self {
            InkEffect::Rainbow => "Rainbow",
            InkEffect::Galaxy => "Galaxy",
            InkEffect::Lava => "Lava",
            InkEffect::Ocean => "Ocean",
            InkEffect::RoseGold => "Rose Gold",
            InkEffect::Gold => "Gold",
            InkEffect::Silver => "Silver",
            InkEffect::Bronze => "Bronze",
        }
    }
    /// The colour at `point` (sheet pixels in the stroke's original frame); deterministic, so
    /// a stroke split by an eraser keeps its pattern.
    pub fn color_at(self, point: [f32; 2]) -> [u8; 3] {
        let noise = ((point[0] * 12.9898 + point[1] * 78.233).sin() * 43758.547).fract().abs();
        let (dark, light) = match self {
            InkEffect::Rainbow => {
                let hue = (point[0] * 0.008 + point[1] * 0.004).rem_euclid(1.0) * 6.0;
                let x = ((1.0 - (hue.rem_euclid(2.0) - 1.0).abs()) * 220.0) as u8;
                return match hue as u32 {
                    0 => [235, x, 30],
                    1 => [x, 235, 30],
                    2 => [30, 235, x],
                    3 => [30, x, 235],
                    4 => [x, 30, 235],
                    _ => [235, 30, x],
                };
            }
            InkEffect::Galaxy => ([35, 20, 85], [80, 225, 225]),
            InkEffect::Lava => ([45, 5, 10], [245, 65, 10]),
            InkEffect::Ocean => ([5, 65, 75], [45, 220, 205]),
            InkEffect::RoseGold => ([170, 85, 75], [255, 215, 195]),
            InkEffect::Gold => ([160, 100, 10], [255, 230, 120]),
            InkEffect::Silver => ([110, 125, 145], [245, 250, 255]),
            InkEffect::Bronze => ([105, 45, 15], [235, 170, 90]),
        };
        std::array::from_fn(|i| (dark[i] as f32 * (1.0 - noise) + light[i] as f32 * noise) as u8)
    }
}

/// Reads an effect id leniently: `""`, `null` and unknown ids are no effect.
fn lenient_effect<'de, D: Deserializer<'de>>(d: D) -> Result<Option<InkEffect>, D::Error> {
    Ok(Option::<String>::deserialize(d)?.as_deref().and_then(InkEffect::parse))
}

pub fn mm_to_px(mm: f32) -> f32 {
    mm * 96.0 / 25.4
}

/// One freehand stroke: the payload of a [`ShapeKind::Ink`] shape. The stroke colour is the
/// shape's `line`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Ink {
    /// Centerline in sheet pixels, relative to the shape's anchor.
    pub points: Vec<[f32; 2]>,
    /// Nominal width in sheet pixels.
    pub width: f32,
    /// Normalized force (0..=1), one per point; empty draws a uniform width.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub pressures: Vec<f32>,
    pub tool: InkTool,
    #[serde(skip_serializing_if = "Option::is_none", deserialize_with = "lenient_effect")]
    pub effect: Option<InkEffect>,
    /// Where `points` sit in the stroke's original frame, so an effect's pattern stays put when
    /// an eraser splits the stroke.
    pub effect_origin: [f32; 2],
}

impl Default for Ink {
    fn default() -> Self {
        Ink { points: Vec::new(), width: 2.0, pressures: Vec::new(), tool: InkTool::Pen, effect: None, effect_origin: [0.0; 2] }
    }
}

impl Ink {
    /// The points that are safe to draw: finite, in range and capped.
    pub fn valid_points(&self) -> impl Iterator<Item = [f32; 2]> + '_ {
        self.points.iter().take(MAX_INK_POINTS).copied().filter(|p| p.iter().all(|v| v.is_finite() && v.abs() <= MAX_INK_COORD))
    }
    /// The nominal width, clamped to the supported range.
    pub fn width(&self) -> f32 {
        if self.width.is_finite() { self.width.clamp(MIN_INK_WIDTH, MAX_INK_WIDTH) } else { 2.0 }
    }
    /// The width at point `i`: force maps to 50%..150% of the nominal width.
    pub fn point_width(&self, i: usize) -> f32 {
        let force = self.pressures.get(i).copied().filter(|p| p.is_finite());
        self.width() * force.map_or(1.0, |p| 0.5 + p.clamp(0.0, 1.0))
    }
    /// The widest the stroke gets.
    pub fn max_width(&self) -> f32 {
        if self.has_pressure() { self.width() * 1.5 } else { self.width() }
    }
    pub fn has_pressure(&self) -> bool {
        !self.pressures.is_empty()
    }
    /// The force at point `i` for files that need one per point (0.5 when unknown).
    pub fn force(&self, i: usize) -> f32 {
        self.pressures.get(i).copied().filter(|p| p.is_finite()).unwrap_or(0.5).clamp(0.0, 1.0)
    }
    pub fn opacity(&self) -> f32 {
        self.tool.opacity()
    }
    /// The colour at `point` (relative to the anchor) for a stroke whose base colour is `base`.
    pub fn color_at(&self, point: [f32; 2], base: [u8; 3]) -> [u8; 3] {
        match self.effect {
            Some(effect) => effect.color_at([point[0] + self.effect_origin[0], point[1] + self.effect_origin[1]]),
            None => base,
        }
    }
    /// Bounding box of the centerline: `(min, max)`, or `None` for an empty stroke.
    pub fn bounds(&self) -> Option<([f32; 2], [f32; 2])> {
        self.valid_points().fold(None, |acc, p| {
            let (lo, hi) = acc.unwrap_or((p, p));
            Some(([lo[0].min(p[0]), lo[1].min(p[1])], [hi[0].max(p[0]), hi[1].max(p[1])]))
        })
    }

    /// The left and right edge of the ribbon at each point.
    fn edges(&self) -> (Vec<[f32; 2]>, Vec<[f32; 2]>, Vec<[f32; 2]>) {
        let points: Vec<_> = self.valid_points().collect();
        let mut left = Vec::with_capacity(points.len());
        let mut right = Vec::with_capacity(points.len());
        let mut normal = [0.0, 1.0];
        for (i, p) in points.iter().enumerate() {
            let a = points.get(i.saturating_sub(1)).copied().unwrap_or(*p);
            let b = points.get(i + 1).copied().unwrap_or(*p);
            let (dx, dy) = (b[0] - a[0], b[1] - a[1]);
            let length = dx.hypot(dy);
            if length > 0.0001 {
                normal = [-dy / length, dx / length];
            }
            let r = self.point_width(i) / 2.0;
            left.push([p[0] + normal[0] * r, p[1] + normal[1] * r]);
            right.push([p[0] - normal[0] * r, p[1] - normal[1] * r]);
        }
        (points, left, right)
    }

    /// Half a circle around `p`, from the direction of `from` going clockwise, without its ends.
    fn cap(p: [f32; 2], from: [f32; 2], r: f32) -> impl Iterator<Item = [f32; 2]> {
        let angle = (from[1] - p[1]).atan2(from[0] - p[0]);
        (1..12).map(move |i| {
            let a = angle - std::f32::consts::PI * i as f32 / 12.0;
            [p[0] + r * a.cos(), p[1] + r * a.sin()]
        })
    }

    /// A continuous ribbon with round caps as triangles: shared joins, no overlapping disks, so
    /// translucent ink stays even.
    pub fn polygons(&self) -> Vec<[[f32; 2]; 3]> {
        let (points, left, right) = self.edges();
        let mut triangles = Vec::new();
        for i in 1..points.len() {
            let (Some(l0), Some(l1), Some(r0), Some(r1)) = (left.get(i - 1), left.get(i), right.get(i - 1), right.get(i)) else { continue };
            triangles.push([*l0, *l1, *r1]);
            triangles.push([*l0, *r1, *r0]);
        }
        let ends = [(points.first(), right.first(), left.first(), 0), (points.last(), left.last(), right.last(), points.len().saturating_sub(1))];
        for (p, from, to, i) in ends {
            let (Some(p), Some(from), Some(to)) = (p, from, to) else { continue };
            let mut fan = vec![*from];
            fan.extend(Self::cap(*p, *from, self.point_width(i) / 2.0));
            fan.push(*to);
            triangles.extend(fan.windows(2).map(|w| [*p, w[0], w[1]]));
        }
        triangles
    }

    /// The ordered outer contour (left edge, end cap, right edge back, start cap), for
    /// antialiasing and lasso hit tests.
    pub fn outline(&self) -> Vec<[f32; 2]> {
        let (points, left, right) = self.edges();
        let mut outline = left.clone();
        if let (Some(p), Some(edge)) = (points.last(), left.last()) {
            outline.extend(Self::cap(*p, *edge, self.point_width(points.len().saturating_sub(1)) / 2.0));
        }
        outline.extend(right.iter().rev());
        if let (Some(p), Some(edge)) = (points.first(), right.first()) {
            outline.extend(Self::cap(*p, *edge, self.point_width(0) / 2.0));
        }
        outline
    }

    /// Parses the pre-`Ink` encoding: points as `x,y;x,y;…` text.
    fn parse_legacy_points(text: &str) -> Vec<[f32; 2]> {
        text.split(';')
            .take(MAX_INK_POINTS)
            .filter_map(|s| {
                let (x, y) = s.split_once(',')?;
                let p = [x.trim().parse::<f32>().ok()?, y.trim().parse::<f32>().ok()?];
                p.iter().all(|v| v.is_finite() && v.abs() <= MAX_INK_COORD).then_some(p)
            })
            .collect()
    }
}

impl Shape {
    /// An ink stroke at `anchor` in colour `line`.
    pub fn new_ink(id: u32, anchor: Anchor, line: Color, ink: Ink) -> Shape {
        Shape { id, kind: ShapeKind::Ink, anchor, fill: Color::Auto, line, text: String::new(), format: Default::default(), ink: Some(ink) }
    }
    /// The stroke's colour at `point` (relative to the anchor).
    pub fn ink_color_at(&self, point: [f32; 2], theme: &Theme) -> [u8; 3] {
        let base = self.line.resolve(theme).unwrap_or([0, 0, 0]);
        self.ink.as_ref().map_or(base, |ink| ink.color_at(point, base))
    }
    /// Moves an ink shape stored the old way (points as text, width in the red channel of
    /// `fill`) into [`Shape::ink`].
    fn upgrade_legacy_ink(&mut self) {
        if self.kind != ShapeKind::Ink || self.ink.is_some() {
            return;
        }
        let width = match self.fill {
            Color::Rgb(v) => ((v >> 16) & 0xFF).clamp(1, 40) as f32,
            _ => 2.0,
        };
        self.ink = Some(Ink { points: Ink::parse_legacy_points(&self.text), width, ..Ink::default() });
        self.text.clear();
        self.fill = Color::Auto;
    }
}

/// The serialized form of [`Shape`]; reading it upgrades legacy ink.
#[derive(Deserialize)]
pub(crate) struct ShapeRepr {
    id: u32,
    kind: ShapeKind,
    anchor: Anchor,
    fill: Color,
    line: Color,
    text: String,
    #[serde(default)]
    format: crate::ShapeFormat,
    #[serde(default)]
    ink: Option<Ink>,
}

impl From<ShapeRepr> for Shape {
    fn from(r: ShapeRepr) -> Shape {
        let mut shape = Shape { id: r.id, kind: r.kind, anchor: r.anchor, fill: r.fill, line: r.line, text: r.text, format: r.format, ink: r.ink };
        shape.upgrade_legacy_ink();
        shape
    }
}

/// A user-configurable Draw pen (an application preference, not part of a workbook).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct InkPen {
    #[serde(alias = "kind")]
    pub tool: InkTool,
    pub color: Color,
    /// Width in sheet pixels (96 dpi).
    pub width: f32,
    #[serde(default, skip_serializing_if = "Option::is_none", deserialize_with = "lenient_effect")]
    pub effect: Option<InkEffect>,
}

impl InkPen {
    pub fn new(tool: InkTool) -> InkPen {
        InkPen { tool, color: Color::Rgb(if tool == InkTool::Highlighter { 0xF5D33B } else { 0 }), width: tool.default_width(), effect: None }
    }
    /// The pens a new installation starts with.
    pub fn defaults() -> Vec<InkPen> {
        use InkEffect::*;
        use InkTool::*;
        [
            (Pen, 0xD13B2F, 0.5, None),
            (Pen, 0x1F5FC9, 0.25, None),
            (Pencil, 0x3C4043, 1.0, None),
            (Highlighter, 0xF5D33B, 6.0, None),
            (Pen, 0x4B3FA8, 1.0, Some(Galaxy)),
            (Pen, 0xF08A24, 0.5, Some(Rainbow)),
        ]
        .into_iter()
        .map(|(tool, color, mm, effect)| InkPen { tool, color: Color::Rgb(color), width: mm_to_px(mm), effect })
        .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stroke() -> Ink {
        Ink { points: vec![[0.0, 0.0], [100.0, 0.0], [100.0, 0.0]], width: 10.0, pressures: vec![0.0, 1.0, 1.0], ..Ink::default() }
    }

    #[test]
    fn pressure_geometry_has_taper_and_round_caps() {
        let ink = stroke();
        let polys = ink.polygons();
        assert_eq!(polys.len(), 4 + 2 * 12);
        assert_eq!(polys[0], [[0.0, 2.5], [100.0, 7.5], [100.0, -7.5]]);
        assert!(polys.iter().flatten().any(|p| (p[0] + 2.5).abs() < 0.001));
        assert!(polys.iter().flatten().any(|p| (p[0] - 107.5).abs() < 0.001));
        assert_eq!(ink.outline().len(), 3 + 11 + 3 + 11);
    }

    #[test]
    fn legacy_shape_json_upgrades_to_ink() {
        let legacy = serde_json::json!({
            "id": 3, "kind": "Ink", "anchor": Anchor::default(), "fill": Color::Rgb(0x070000),
            "line": Color::Rgb(0x123456), "text": "0,0;10.5,20;bad;NaN,1"
        });
        let shape: Shape = serde_json::from_value(legacy).unwrap();
        let ink = shape.ink.as_ref().unwrap();
        assert_eq!(ink.points, vec![[0.0, 0.0], [10.5, 20.0]]);
        assert_eq!(ink.width, 7.0);
        assert!(shape.text.is_empty() && shape.fill == Color::Auto);
        // The new form round-trips unchanged, and other shapes are untouched.
        let back: Shape = serde_json::from_slice(&serde_json::to_vec(&shape).unwrap()).unwrap();
        assert_eq!(back, shape);
        let rect = Shape { text: "1,2".into(), ..Shape::default() };
        assert_eq!(serde_json::from_value::<Shape>(serde_json::to_value(&rect).unwrap()).unwrap(), rect);
    }

    #[test]
    fn pens_read_old_string_fields() {
        let pen: InkPen =
            serde_json::from_value(serde_json::json!({"kind": "highlighter", "color": Color::Rgb(1), "width": 3.0, "effect": ""})).unwrap();
        assert_eq!((pen.tool, pen.effect), (InkTool::Highlighter, None));
        let pen: InkPen =
            serde_json::from_value(serde_json::json!({"tool": "pen", "color": Color::Rgb(1), "width": 3.0, "effect": "roseGold"})).unwrap();
        assert_eq!(pen.effect, Some(InkEffect::RoseGold));
        assert!(InkTool::ALL.iter().all(|t| InkTool::parse(t.id()) == Some(*t)));
        assert!(InkEffect::ALL.iter().all(|e| InkEffect::parse(e.id()) == Some(*e)));
    }

    #[test]
    fn hostile_values_never_create_invalid_geometry() {
        let ink = Ink {
            points: vec![[0.0, 0.0], [f32::NAN, 1.0], [f32::INFINITY, 0.0], [1e30, 0.0], [5.0, 5.0]],
            width: f32::INFINITY,
            pressures: vec![f32::NAN, -100.0, 1e30],
            ..Ink::default()
        };
        assert_eq!(ink.point_width(0), 2.0);
        assert_eq!(ink.point_width(1), 1.0);
        assert_eq!(ink.point_width(2), 3.0);
        assert_eq!(ink.valid_points().count(), 2);
        assert!(ink.polygons().iter().flatten().flatten().all(|n| n.is_finite()));
        assert!(Ink::default().polygons().is_empty() && Ink::default().outline().is_empty());
    }
}
