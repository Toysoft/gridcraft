//! Reading DrawingML colours (with modifiers), fills and effects into the model's `dml` types:
//! shape fills and text fills, and the theme's format scheme that shape styles refer to.

use gridcraft_model::{ColorMod, DmlBase, DmlColor, DmlEffects, DmlFill, DmlShadow, GradientStop};

use crate::xml::El;

/// EMU per point.
const EMU_PER_PT: f64 = 12_700.0;

fn scheme_index(name: &str) -> Option<DmlBase> {
    let i = match name {
        "phClr" => return Some(DmlBase::Placeholder),
        "bg1" | "lt1" => 0,
        "tx1" | "dk1" => 1,
        "bg2" | "lt2" => 2,
        "tx2" | "dk2" => 3,
        "accent1" => 4,
        "accent2" => 5,
        "accent3" => 6,
        "accent4" => 7,
        "accent5" => 8,
        "accent6" => 9,
        "hlink" => 10,
        "folHlink" => 11,
        _ => return None,
    };
    Some(DmlBase::Scheme(i))
}

fn hex(v: &str) -> Option<u32> {
    u32::from_str_radix(v.trim(), 16).ok().map(|v| v & 0xFF_FFFF)
}

/// A percentage attribute (`val="50000"` → 0.5).
fn pct(e: &El) -> Option<f32> {
    e.attr_f64("val").filter(|v| v.is_finite()).map(|v| (v / 100_000.0).clamp(-100.0, 100.0) as f32)
}

/// One colour element (`a:srgbClr`, `a:schemeClr`, `a:sysClr`, `a:scrgbClr`, `a:prstClr`) with
/// its modifiers.
pub fn color_el(c: &El) -> Option<DmlColor> {
    let base = match c.name.as_str() {
        "srgbClr" => DmlBase::Rgb(hex(c.attr("val")?)?),
        "schemeClr" => scheme_index(c.attr("val")?)?,
        "sysClr" => match c.attr("lastClr").and_then(hex) {
            Some(v) => DmlBase::Rgb(v),
            None => DmlBase::Rgb(if c.attr("val") == Some("window") { 0xFF_FFFF } else { 0 }),
        },
        "scrgbClr" => {
            // Linear-light percentages.
            let ch = |k: &str| {
                let v = (c.attr_f64(k).unwrap_or(0.0) / 100_000.0).clamp(0.0, 1.0);
                let s = if v <= 0.003_130_8 { v * 12.92 } else { 1.055 * v.powf(1.0 / 2.4) - 0.055 };
                ((s * 255.0).round() as u32).min(255)
            };
            DmlBase::Rgb((ch("r") << 16) | (ch("g") << 8) | ch("b"))
        }
        "prstClr" => DmlBase::Rgb(match c.attr("val")? {
            "black" => 0,
            "white" => 0xFF_FFFF,
            "red" => 0xFF_0000,
            "green" => 0x00_8000,
            "blue" => 0x00_00FF,
            "yellow" => 0xFF_FF00,
            "gray" => 0x80_8080,
            _ => return None,
        }),
        _ => return None,
    };
    let mods = c
        .children
        .iter()
        .filter_map(|m| {
            let v = pct(m)?;
            Some(match m.name.as_str() {
                "tint" => ColorMod::Tint(v),
                "shade" => ColorMod::Shade(v),
                "lumMod" => ColorMod::LumMod(v),
                "lumOff" => ColorMod::LumOff(v),
                "satMod" => ColorMod::SatMod(v),
                "alpha" => ColorMod::Alpha(v),
                _ => return None,
            })
        })
        .collect();
    Some(DmlColor { base, mods })
}

/// The colour chosen inside `e` (a `solidFill`, `gs`, `fillRef`…).
pub fn color_in(e: &El) -> Option<DmlColor> {
    e.children.iter().find_map(color_el)
}

/// A fill element itself (`a:solidFill`, `a:gradFill`, `a:noFill`).
pub fn fill_el(f: &El) -> Option<DmlFill> {
    match f.name.as_str() {
        "noFill" => Some(DmlFill::None),
        "solidFill" => color_in(f).map(DmlFill::Solid),
        "gradFill" => {
            let mut stops: Vec<GradientStop> = f
                .child("gsLst")
                .map(|l| {
                    l.kids("gs")
                        .filter_map(|g| {
                            let pos = (g.attr_f64("pos").unwrap_or(0.0) / 100_000.0).clamp(0.0, 1.0) as f32;
                            Some(GradientStop { pos, color: color_in(g)? })
                        })
                        .collect()
                })
                .unwrap_or_default();
            if stops.is_empty() {
                return None;
            }
            stops.sort_by(|a, b| a.pos.total_cmp(&b.pos));
            // `lin ang` in 60000ths of a degree; path gradients are drawn top to bottom.
            let angle =
                f.child("lin").and_then(|l| l.attr_f64("ang")).filter(|a| a.is_finite()).map_or(90.0, |a| (a / 60_000.0).rem_euclid(360.0) as f32);
            Some(DmlFill::Gradient { stops, angle })
        }
        _ => None,
    }
}

/// The fill a properties element (`spPr`, `rPr`) gives, if it names one.
pub fn fill_of(props: &El) -> Option<DmlFill> {
    props.children.iter().find(|c| matches!(c.name.as_str(), "noFill" | "solidFill" | "gradFill")).and_then(fill_el)
}

/// Effects from an `a:effectLst` and an `a:sp3d` (either may be absent).
pub fn effects(effect_lst: Option<&El>, sp3d: Option<&El>) -> DmlEffects {
    let shadow = effect_lst.and_then(|l| l.child("outerShdw")).map(|s| {
        let pt = |k: &str| (s.attr_f64(k).unwrap_or(0.0) / EMU_PER_PT).clamp(0.0, 200.0) as f32;
        let dist = pt("dist");
        let dir = (s.attr_f64("dir").unwrap_or(0.0) / 60_000.0).to_radians() as f32;
        // Rounded to a thousandth of a point: `cos 90°` is not exactly 0.
        let round = |v: f32| (v * 1000.0).round() / 1000.0;
        DmlShadow {
            dx: round(dist * dir.cos()),
            dy: round(dist * dir.sin()),
            blur: pt("blurRad"),
            color: color_in(s).unwrap_or(DmlColor { base: DmlBase::Rgb(0), mods: vec![ColorMod::Alpha(0.35)] }),
        }
    });
    // `bevelT w` defaults to 76200 EMU (6 pt).
    let bevel = sp3d.and_then(|s| s.child("bevelT")).map(|b| (b.attr_f64("w").unwrap_or(76_200.0) / EMU_PER_PT).clamp(0.0, 100.0) as f32);
    DmlEffects { shadow, bevel }
}

/// A theme's `a:fmtScheme`: its fills and effect styles.
pub fn fmt_scheme(fmt: &El) -> (Vec<DmlFill>, Vec<DmlEffects>) {
    let fills = fmt.child("fillStyleLst").map(|l| l.children.iter().map(|f| fill_el(f).unwrap_or(DmlFill::None)).collect()).unwrap_or_default();
    let effects = fmt
        .child("effectStyleLst")
        .map(|l| l.kids("effectStyle").map(|e| effects(e.child("effectLst"), e.child("sp3d"))).collect())
        .unwrap_or_default();
    (fills, effects)
}

// ---------------------------------------------------------------- writing

fn color_xml(c: &DmlColor) -> String {
    const NAMES: [&str; 12] = ["lt1", "dk1", "lt2", "dk2", "accent1", "accent2", "accent3", "accent4", "accent5", "accent6", "hlink", "folHlink"];
    let (tag, val) = match c.base {
        DmlBase::Rgb(v) => ("srgbClr", format!("{:06X}", v & 0xFF_FFFF)),
        DmlBase::Scheme(i) => ("schemeClr", NAMES.get(i as usize).copied().unwrap_or("dk1").to_string()),
        DmlBase::Placeholder => ("schemeClr", "phClr".to_string()),
    };
    let mods: String = c
        .mods
        .iter()
        .map(|m| {
            let (n, v) = match *m {
                ColorMod::Tint(v) => ("tint", v),
                ColorMod::Shade(v) => ("shade", v),
                ColorMod::LumMod(v) => ("lumMod", v),
                ColorMod::LumOff(v) => ("lumOff", v),
                ColorMod::SatMod(v) => ("satMod", v),
                ColorMod::Alpha(v) => ("alpha", v),
            };
            format!("<a:{n} val=\"{}\"/>", (f64::from(v) * 100_000.0).round() as i64)
        })
        .collect();
    if mods.is_empty() { format!("<a:{tag} val=\"{val}\"/>") } else { format!("<a:{tag} val=\"{val}\">{mods}</a:{tag}>") }
}

/// A fill as DrawingML.
pub fn fill_xml(f: &DmlFill) -> String {
    match f {
        DmlFill::None => "<a:noFill/>".into(),
        DmlFill::Solid(c) => format!("<a:solidFill>{}</a:solidFill>", color_xml(c)),
        DmlFill::Gradient { stops, angle } => {
            let gs: String = stops
                .iter()
                .map(|s| format!("<a:gs pos=\"{}\">{}</a:gs>", (f64::from(s.pos) * 100_000.0).round() as i64, color_xml(&s.color)))
                .collect();
            format!(
                "<a:gradFill rotWithShape=\"1\"><a:gsLst>{gs}</a:gsLst><a:lin ang=\"{}\" scaled=\"0\"/></a:gradFill>",
                (f64::from(*angle) * 60_000.0).round() as i64
            )
        }
    }
}

fn shadow_xml(sh: &DmlShadow) -> String {
    let dist = f64::from(sh.dx.hypot(sh.dy));
    let dir = f64::from(sh.dy.atan2(sh.dx).to_degrees()).rem_euclid(360.0);
    format!(
        "<a:outerShdw blurRad=\"{}\" dist=\"{}\" dir=\"{}\" rotWithShape=\"0\">{}</a:outerShdw>",
        (f64::from(sh.blur) * EMU_PER_PT).round() as i64,
        (dist * EMU_PER_PT).round() as i64,
        (dir * 60_000.0).round() as i64,
        color_xml(&sh.color)
    )
}

/// A shape's own effects inside `xdr:spPr` (nothing when it has none).
pub fn shape_effects_xml(e: &DmlEffects) -> String {
    let mut s = String::new();
    if let Some(sh) = &e.shadow {
        s.push_str(&format!("<a:effectLst>{}</a:effectLst>", shadow_xml(sh)));
    }
    if let Some(w) = e.bevel {
        s.push_str(&format!("<a:sp3d><a:bevelT w=\"{}\"/></a:sp3d>", (f64::from(w) * EMU_PER_PT).round() as i64));
    }
    s
}

/// An `a:effectStyle` for a theme (an empty one for `None`).
pub fn effect_style_xml(e: Option<&DmlEffects>) -> String {
    let Some(e) = e else { return "<a:effectStyle><a:effectLst/></a:effectStyle>".into() };
    let mut s = String::from("<a:effectStyle><a:effectLst>");
    if let Some(sh) = &e.shadow {
        s.push_str(&shadow_xml(sh));
    }
    s.push_str("</a:effectLst>");
    if let Some(w) = e.bevel {
        s.push_str(&format!(
            "<a:scene3d><a:camera prst=\"orthographicFront\"/><a:lightRig rig=\"threePt\" dir=\"t\"/></a:scene3d><a:sp3d><a:bevelT w=\"{}\" h=\"{}\"/></a:sp3d>",
            (f64::from(w) * EMU_PER_PT).round() as i64,
            (f64::from(w) * EMU_PER_PT / 2.0).round() as i64
        ));
    }
    s.push_str("</a:effectStyle>");
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn colours_fills_and_effects() {
        let x = crate::xml::parse(
            br#"<a:spPr xmlns:a="a"><a:gradFill><a:gsLst><a:gs pos="100000"><a:schemeClr val="accent2"><a:shade val="45000"/></a:schemeClr></a:gs><a:gs pos="25000"><a:srgbClr val="C0504D"/></a:gs></a:gsLst><a:lin ang="5400000"/></a:gradFill><a:effectLst><a:outerShdw blurRad="76200" dist="50800" dir="5400000"><a:srgbClr val="000000"><a:alpha val="65000"/></a:srgbClr></a:outerShdw></a:effectLst><a:sp3d><a:bevelT w="25400"/></a:sp3d></a:spPr>"#,
        )
        .unwrap();
        let Some(DmlFill::Gradient { stops, angle }) = fill_of(&x) else { panic!("gradient expected") };
        assert_eq!(angle, 90.0);
        assert_eq!(stops.len(), 2);
        assert_eq!(stops[0].pos, 0.25, "stops are sorted");
        assert_eq!(stops[1].color, DmlColor { base: DmlBase::Scheme(5), mods: vec![ColorMod::Shade(0.45)] });
        let fx = effects(x.child("effectLst"), x.child("sp3d"));
        let sh = fx.shadow.unwrap();
        assert!((sh.dy - 4.0).abs() < 1e-3 && sh.dx.abs() < 1e-3, "{sh:?}");
        assert_eq!(fx.bevel, Some(2.0));
        // Garbage colours are skipped, never a panic.
        let bad = crate::xml::parse(br#"<a:solidFill xmlns:a="a"><a:srgbClr val="zz"/></a:solidFill>"#).unwrap();
        assert_eq!(fill_el(&bad), None);
    }

    #[test]
    fn fills_and_effects_write_back_as_read() {
        let t = gridcraft_model::Theme::default();
        for f in &t.fill_styles {
            let x = crate::xml::parse(format!("<r xmlns:a=\"a\">{}</r>", fill_xml(f)).as_bytes()).unwrap();
            let back = x.children.first().and_then(fill_el).unwrap();
            assert_eq!(&back, f);
        }
        for e in &t.effect_styles {
            let x = crate::xml::parse(effect_style_xml(Some(e)).as_bytes()).unwrap();
            let back = effects(x.child("effectLst"), x.child("sp3d"));
            assert_eq!(back.bevel, e.bevel);
            assert_eq!(back.shadow.is_some(), e.shadow.is_some());
        }
    }
}
