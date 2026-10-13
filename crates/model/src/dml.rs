//! DrawingML colours and fills for shapes: a base colour (RGB, a theme slot or the style's
//! placeholder colour) with modifiers (tint, shade, luminance and saturation changes, alpha),
//! solid and linear-gradient fills, and the outer shadow / bevel effects shapes draw with.
//!
//! Theme format schemes (`fillStyleLst`, `effectStyleLst`) are stored the same way, with the
//! placeholder base standing for the colour a shape's style reference names.

use serde::{Deserialize, Serialize};

use crate::style::{Theme, hsl_to_rgb, rgb_to_hsl};

/// A colour modifier, values as fractions (`<a:tint val="50000"/>` → `Tint(0.5)`).
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub enum ColorMod {
    /// Mix with white, keeping this fraction of the colour.
    Tint(f32),
    /// Mix with black, keeping this fraction of the colour.
    Shade(f32),
    LumMod(f32),
    LumOff(f32),
    SatMod(f32),
    Alpha(f32),
}

/// What a DrawingML colour starts from.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub enum DmlBase {
    /// 0xRRGGBB.
    Rgb(u32),
    /// Theme slot, indexed like [`Theme::colors`].
    Scheme(u8),
    /// `phClr`: the colour a style reference supplies.
    Placeholder,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct DmlColor {
    pub base: DmlBase,
    #[serde(default)]
    pub mods: Vec<ColorMod>,
}

impl DmlColor {
    pub fn rgb(v: u32) -> DmlColor {
        DmlColor { base: DmlBase::Rgb(v & 0xFF_FFFF), mods: Vec::new() }
    }

    /// RGBA after the modifiers. A placeholder base takes `ph` (black when there is none).
    pub fn resolve(&self, theme: &Theme, ph: Option<&DmlColor>) -> [u8; 4] {
        let (mut rgb, mut alpha) = match self.base {
            DmlBase::Rgb(v) => (split(v), 1.0f32),
            DmlBase::Scheme(i) => (split(theme.colors.get(i as usize).copied().unwrap_or(0)), 1.0),
            DmlBase::Placeholder => match ph {
                // A placeholder never refers to another placeholder.
                Some(p) if p.base != DmlBase::Placeholder => {
                    let [r, g, b, a] = p.resolve(theme, None);
                    ([r, g, b], f32::from(a) / 255.0)
                }
                _ => ([0, 0, 0], 1.0),
            },
        };
        for m in &self.mods {
            match *m {
                ColorMod::Tint(t) => rgb = mix_linear(rgb, [255, 255, 255], t),
                ColorMod::Shade(s) => rgb = mix_linear(rgb, [0, 0, 0], s),
                ColorMod::LumMod(v) => rgb = map_hsl(rgb, |h, s, l| (h, s, l * f64::from(v))),
                ColorMod::LumOff(v) => rgb = map_hsl(rgb, |h, s, l| (h, s, l + f64::from(v))),
                ColorMod::SatMod(v) => rgb = map_hsl(rgb, |h, s, l| (h, s * f64::from(v), l)),
                ColorMod::Alpha(a) => alpha = a,
            }
        }
        let a = (alpha.clamp(0.0, 1.0) * 255.0).round() as u8;
        [rgb[0], rgb[1], rgb[2], a]
    }
}

fn split(v: u32) -> [u8; 3] {
    [(v >> 16) as u8, (v >> 8) as u8, v as u8]
}

fn to_linear(c: u8) -> f32 {
    let c = f32::from(c) / 255.0;
    if c <= 0.04045 { c / 12.92 } else { ((c + 0.055) / 1.055).powf(2.4) }
}

fn from_linear(c: f32) -> u8 {
    let c = c.clamp(0.0, 1.0);
    let s = if c <= 0.003_130_8 { c * 12.92 } else { 1.055 * c.powf(1.0 / 2.4) - 0.055 };
    (s * 255.0).round().clamp(0.0, 255.0) as u8
}

/// `keep` of `rgb` and the rest of `other`, mixed in linear light.
fn mix_linear(rgb: [u8; 3], other: [u8; 3], keep: f32) -> [u8; 3] {
    let k = keep.clamp(0.0, 1.0);
    let m = |a: u8, b: u8| from_linear(to_linear(a) * k + to_linear(b) * (1.0 - k));
    [m(rgb[0], other[0]), m(rgb[1], other[1]), m(rgb[2], other[2])]
}

fn map_hsl(rgb: [u8; 3], f: impl Fn(f64, f64, f64) -> (f64, f64, f64)) -> [u8; 3] {
    let (h, s, l) = rgb_to_hsl(rgb);
    let (h, s, l) = f(h, s, l);
    hsl_to_rgb(h, s.clamp(0.0, 1.0), l.clamp(0.0, 1.0))
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct GradientStop {
    /// 0..=1 along the gradient.
    pub pos: f32,
    pub color: DmlColor,
}

/// How an area is filled.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum DmlFill {
    None,
    Solid(DmlColor),
    /// Linear gradient; `angle` in degrees clockwise from left-to-right (90 = top to bottom).
    /// Path (radial) gradients are kept as linear ones at their first-to-last stop.
    Gradient {
        stops: Vec<GradientStop>,
        angle: f32,
    },
}

impl DmlFill {
    /// The fill with every placeholder colour replaced by `ph` (a style reference's colour).
    pub fn with_placeholder(&self, ph: &DmlColor) -> DmlFill {
        let sub = |c: &DmlColor| match c.base {
            DmlBase::Placeholder => DmlColor { base: ph.base, mods: ph.mods.iter().chain(c.mods.iter()).copied().collect() },
            _ => c.clone(),
        };
        match self {
            DmlFill::None => DmlFill::None,
            DmlFill::Solid(c) => DmlFill::Solid(sub(c)),
            DmlFill::Gradient { stops, angle } => {
                DmlFill::Gradient { stops: stops.iter().map(|s| GradientStop { pos: s.pos, color: sub(&s.color) }).collect(), angle: *angle }
            }
        }
    }

    /// The colour at `t` (0..=1) along the fill; `None` for no fill.
    pub fn color_at(&self, theme: &Theme, t: f32) -> Option<[u8; 4]> {
        match self {
            DmlFill::None => None,
            DmlFill::Solid(c) => Some(c.resolve(theme, None)),
            DmlFill::Gradient { stops, .. } => {
                let first = stops.first()?;
                let t = t.clamp(0.0, 1.0);
                let mut prev = first;
                if t <= first.pos {
                    return Some(first.color.resolve(theme, None));
                }
                for s in stops.iter().skip(1) {
                    if t <= s.pos {
                        let span = (s.pos - prev.pos).max(f32::EPSILON);
                        let k = (t - prev.pos) / span;
                        let (a, b) = (prev.color.resolve(theme, None), s.color.resolve(theme, None));
                        let lerp = |x: u8, y: u8| (f32::from(x) + (f32::from(y) - f32::from(x)) * k).round() as u8;
                        return Some([lerp(a[0], b[0]), lerp(a[1], b[1]), lerp(a[2], b[2]), lerp(a[3], b[3])]);
                    }
                    prev = s;
                }
                Some(prev.color.resolve(theme, None))
            }
        }
    }
}

/// An outer shadow: offset in points, blur radius in points, colour.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct DmlShadow {
    pub dx: f32,
    pub dy: f32,
    pub blur: f32,
    pub color: DmlColor,
}

/// The effects a theme effect style (or a shape) gives: an outer shadow and a top bevel width in
/// points.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct DmlEffects {
    pub shadow: Option<DmlShadow>,
    pub bevel: Option<f32>,
}

impl DmlEffects {
    pub fn with_placeholder(&self, ph: &DmlColor) -> DmlEffects {
        let shadow = self.shadow.as_ref().map(|s| {
            let color = match s.color.base {
                DmlBase::Placeholder => DmlColor { base: ph.base, mods: ph.mods.iter().chain(s.color.mods.iter()).copied().collect() },
                _ => s.color.clone(),
            };
            DmlShadow { color, ..s.clone() }
        });
        DmlEffects { shadow, bevel: self.bevel }
    }
}

fn ph(mods: Vec<ColorMod>) -> DmlColor {
    DmlColor { base: DmlBase::Placeholder, mods }
}

/// GridCraft's own format-scheme fills (the Craft theme): flat, a gentle top-lit gradient and a
/// stronger one.
pub fn default_fill_styles() -> Vec<DmlFill> {
    vec![
        DmlFill::Solid(ph(vec![])),
        DmlFill::Gradient {
            stops: vec![GradientStop { pos: 0.0, color: ph(vec![ColorMod::Tint(0.8)]) }, GradientStop { pos: 1.0, color: ph(vec![]) }],
            angle: 90.0,
        },
        DmlFill::Gradient {
            stops: vec![
                GradientStop { pos: 0.0, color: ph(vec![ColorMod::Tint(0.65)]) },
                GradientStop { pos: 0.6, color: ph(vec![]) },
                GradientStop { pos: 1.0, color: ph(vec![ColorMod::Shade(0.8)]) },
            ],
            angle: 90.0,
        },
    ]
}

/// GridCraft's own format-scheme effects: none, a soft drop shadow, and a shadow with a bevel.
pub fn default_effect_styles() -> Vec<DmlEffects> {
    let shadow = || DmlShadow { dx: 0.0, dy: 1.5, blur: 3.0, color: DmlColor { base: DmlBase::Rgb(0), mods: vec![ColorMod::Alpha(0.3)] } };
    vec![DmlEffects::default(), DmlEffects { shadow: Some(shadow()), bevel: None }, DmlEffects { shadow: Some(shadow()), bevel: Some(3.0) }]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn modifiers_lighten_darken_and_fade() {
        let t = Theme::default();
        let red = DmlColor::rgb(0xC0_0000);
        assert_eq!(red.resolve(&t, None), [0xC0, 0, 0, 255]);
        let tinted = DmlColor { mods: vec![ColorMod::Tint(0.5)], ..red.clone() };
        let [r, g, b, _] = tinted.resolve(&t, None);
        assert!(r > 0xC0 && g > 0 && g == b, "tint mixes towards white: {r} {g} {b}");
        let shaded = DmlColor { mods: vec![ColorMod::Shade(0.5)], ..red.clone() };
        assert!(shaded.resolve(&t, None)[0] < 0xC0, "shade darkens");
        let faded = DmlColor { mods: vec![ColorMod::Alpha(0.5)], ..red };
        assert_eq!(faded.resolve(&t, None)[3], 128);
    }

    #[test]
    fn placeholders_take_the_style_colour() {
        let mut t = Theme::default();
        t.colors[8] = 0x4BACC6;
        let ph = DmlColor { base: DmlBase::Scheme(8), mods: vec![] };
        let fill = DmlFill::Gradient {
            stops: vec![
                GradientStop { pos: 0.0, color: DmlColor { base: DmlBase::Placeholder, mods: vec![ColorMod::Shade(0.5)] } },
                GradientStop { pos: 1.0, color: DmlColor { base: DmlBase::Placeholder, mods: vec![] } },
            ],
            angle: 90.0,
        }
        .with_placeholder(&ph);
        assert_eq!(fill.color_at(&t, 1.0), Some([0x4B, 0xAC, 0xC6, 255]));
        let top = fill.color_at(&t, 0.0).unwrap_or_default();
        assert!(top[2] < 0xC6, "the shaded stop is darker");
        let mid = fill.color_at(&t, 0.5).unwrap_or_default();
        assert!(mid[2] > top[2] && mid[2] < 0xC6, "stops interpolate");
        assert_eq!(DmlFill::None.color_at(&t, 0.5), None);
        // A placeholder without a style colour is black, never a panic or a loop.
        assert_eq!(DmlColor { base: DmlBase::Placeholder, mods: vec![] }.resolve(&t, None), [0, 0, 0, 255]);
    }
}
