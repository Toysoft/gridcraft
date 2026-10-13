//! Bounded import of Excel's archival InkML profile (W3C InkML §3.2, §4.2, §4.3).
//! Unsupported brush effects, transforms or channel layouts use the drawing's image fallback.
use gridcraft_model::{Anchor, Color, Ink, InkEffect, InkTool, Shape};

use crate::xml::El;

const MAX_POINTS: usize = 20_000;
const MAX_TOTAL_POINTS: usize = 200_000;
const MAX_TRACES: usize = 2_000;
const MAX_NODES: usize = 10_000;

/// Reject foreign XML, DTDs and malformed nesting before the generic local-name tree reader.
pub fn parse(bytes: &[u8]) -> Result<El, crate::IoError> {
    use quick_xml::{NsReader, events::Event, name::ResolveResult};
    let invalid = || crate::IoError::Format("unsupported or malformed InkML".into());
    let mut reader = NsReader::from_reader(bytes);
    let mut count = 0usize;
    let mut depth = 0usize;
    let mut root_seen = false;
    loop {
        let event = reader.read_event().map_err(|e| crate::IoError::Xml(e.to_string()))?;
        match &event {
            Event::Start(e) | Event::Empty(e) => {
                let (ns, name) = reader.resolver().resolve_element(e.name());
                if !matches!(ns, ResolveResult::Bound(n) if n.as_ref() == b"http://www.w3.org/2003/InkML") {
                    return Err(invalid());
                }
                if depth == 0 {
                    if root_seen || name.as_ref() != b"ink" {
                        return Err(invalid());
                    }
                    root_seen = true;
                }
                count += 1;
                if count > MAX_NODES {
                    return Err(invalid());
                }
                if matches!(event, Event::Start(_)) {
                    depth += 1;
                    if depth > crate::xml::MAX_DEPTH {
                        return Err(invalid());
                    }
                }
            }
            Event::End(_) => depth = depth.checked_sub(1).ok_or_else(invalid)?,
            Event::DocType(_) => return Err(invalid()),
            Event::Eof => break,
            _ => {}
        }
    }
    if !root_seen || depth != 0 {
        return Err(invalid());
    }
    crate::xml::parse(bytes)
}

fn length(units: &str) -> Option<f64> {
    match units {
        "cm" => Some(96.0 / 2.54),
        "mm" => Some(96.0 / 25.4),
        "in" => Some(96.0),
        "pt" => Some(96.0 / 72.0),
        "m" => Some(96.0 / 0.0254),
        _ => None,
    }
}

fn reference<'a>(nodes: &[&'a El], value: &str, kind: &str) -> Option<&'a El> {
    let id = value.strip_prefix('#')?;
    nodes.iter().copied().find(|e| e.name == kind && e.attr("id") == Some(id))
}

#[derive(Clone, Copy, Default)]
struct Channel {
    value: f64,
    delta: f64,
    acceleration: f64,
    mode: u8,
    seen: bool,
    has_delta: bool,
}

/// Longest-token numeric lexer: Excel omits spaces around signs and mode prefixes.
fn token(s: &str) -> Option<(f64, &str)> {
    let bytes = s.as_bytes();
    let mut i = usize::from(matches!(bytes.first(), Some(b'+' | b'-')));
    let mut digits = 0;
    while bytes.get(i).is_some_and(u8::is_ascii_digit) {
        i += 1;
        digits += 1;
    }
    if bytes.get(i) == Some(&b'.') {
        i += 1;
        while bytes.get(i).is_some_and(u8::is_ascii_digit) {
            i += 1;
            digits += 1;
        }
    }
    if digits == 0 {
        return None;
    }
    if matches!(bytes.get(i), Some(b'e' | b'E')) {
        i += 1;
        if matches!(bytes.get(i), Some(b'+' | b'-')) {
            i += 1;
        }
        let begin = i;
        while bytes.get(i).is_some_and(u8::is_ascii_digit) {
            i += 1;
        }
        if i == begin {
            return None;
        }
    }
    let value = s.get(..i)?.parse::<f64>().ok()?;
    value.is_finite().then_some((value, s.get(i..)?))
}

fn points(text: &str, count: usize, x: usize, y: usize, force: Option<usize>) -> Option<Vec<[f64; 3]>> {
    let mut channels = vec![Channel::default(); count];
    let mut out = Vec::new();
    for record in text.trim().split(',') {
        if out.len() >= MAX_POINTS {
            return None;
        }
        let mut rest = record.trim();
        for channel in &mut channels {
            rest = rest.trim_start();
            if let Some(prefix) = rest.as_bytes().first() {
                let mode = match prefix {
                    b'!' => Some(0),
                    b'\'' => Some(1),
                    b'"' => Some(2),
                    _ => None,
                };
                if let Some(mode) = mode {
                    channel.mode = mode;
                    rest = rest.get(1..)?.trim_start();
                }
            }
            if !channel.seen && channel.mode != 0 || channel.mode == 2 && !channel.has_delta {
                return None;
            }
            let wildcard = rest.starts_with('*');
            let value = if wildcard {
                if !channel.seen {
                    return None;
                }
                rest = rest.get(1..)?;
                match channel.mode {
                    0 => channel.value,
                    1 => channel.delta,
                    _ => channel.acceleration,
                }
            } else {
                let (v, tail) = token(rest)?;
                rest = tail;
                v
            };
            match channel.mode {
                0 => {
                    channel.delta = if channel.seen { value - channel.value } else { 0.0 };
                    channel.value = value;
                }
                1 => {
                    channel.delta = value;
                    channel.value += value;
                    channel.has_delta = true;
                }
                _ => {
                    channel.acceleration = value;
                    channel.delta += value;
                    channel.value += channel.delta;
                }
            }
            channel.seen = true;
            if !channel.value.is_finite() || channel.value.abs() > 1e12 || !channel.delta.is_finite() {
                return None;
            }
        }
        if !rest.trim().is_empty() {
            return None;
        }
        out.push([channels.get(x)?.value, channels.get(y)?.value, force.and_then(|f| channels.get(f)).map(|c| c.value).unwrap_or(0.0)]);
    }
    (!out.is_empty()).then_some(out)
}

fn scale(source: Option<&El>, channel: &El) -> Option<f64> {
    let units = channel.attr("units").unwrap_or("dev");
    let property = source
        .and_then(|s| s.child("channelProperties"))
        .and_then(|p| p.kids("channelProperty").find(|p| p.attr("channel") == channel.attr("name") && p.attr("name") == Some("resolution")));
    match property {
        Some(p) => {
            let resolution = p.attr_f64("value").filter(|v| *v > 0.0)?;
            let unit = p.attr("units")?;
            if let Some(unit) = unit.strip_prefix("1/") { Some(length(unit)? / resolution) } else { Some(length(unit)? * resolution) }
        }
        None => length(units),
    }
}

/// Returns None atomically on unsupported/damaged content so the caller can retain the PNG.
/// `anchor` already refers to the contentPart transform, rather than the padded PNG fallback.
pub fn read(root: &El, anchor: Anchor) -> Option<Vec<Shape>> {
    if root.name != "ink" {
        return None;
    }
    let mut nodes = Vec::new();
    let mut pending = vec![root];
    while let Some(e) = pending.pop() {
        if nodes.len() >= MAX_NODES {
            return None;
        }
        if matches!(e.name.as_str(), "mapping" | "canvasTransform" | "traceView" | "intermittentChannels") {
            return None;
        }
        nodes.push(e);
        if pending.len().saturating_add(e.children.len()) > MAX_NODES {
            return None;
        }
        pending.extend(e.children.iter().rev());
    }
    let mut shapes = Vec::new();
    let mut total = 0usize;
    let mut pending = vec![(root, root.attr("contextRef"), root.attr("brushRef"))];
    while let Some((e, inherited_context, inherited_brush)) = pending.pop() {
        let context_ref = e.attr("contextRef").or(inherited_context);
        let brush_ref = e.attr("brushRef").or(inherited_brush);
        if e.name != "trace" {
            if matches!(e.name.as_str(), "ink" | "traceGroup") {
                pending.extend(e.children.iter().rev().map(|c| (c, context_ref, brush_ref)));
            }
            continue;
        }
        if e.attr("continuation").is_some() {
            return None;
        }
        if e.attr("type").is_some_and(|t| !matches!(t, "penUp" | "penDown")) {
            return None;
        }
        if e.attr("type") == Some("penUp") {
            continue;
        }
        let context = match context_ref {
            Some(r) => Some(reference(&nodes, r, "context")?),
            None => root.child("context"),
        };
        if context.is_some_and(|c| c.attr("contextRef").is_some() || c.child("canvas").is_some()) {
            return None;
        }
        let source = match context.and_then(|c| c.child("inkSource")) {
            Some(s) => Some(s),
            None => match context.and_then(|c| c.attr("inkSourceRef")) {
                Some(r) => Some(reference(&nodes, r, "inkSource")?),
                None => None,
            },
        };
        let format = context
            .and_then(|c| c.child("traceFormat"))
            .or_else(|| source.and_then(|s| s.child("traceFormat")))
            .or_else(|| root.child("traceFormat"));
        let format = match format {
            Some(f) => f,
            None => reference(&nodes, context?.attr("traceFormatRef")?, "traceFormat")?,
        };
        let channels: Vec<_> = format.kids("channel").take(65).collect();
        if channels.len() < 2 || channels.len() > 64 {
            return None;
        }
        if channels.iter().any(|c| c.attr("name") == Some("S")) {
            return None;
        }
        let x = channels.iter().position(|c| c.attr("name") == Some("X"))?;
        let y = channels.iter().position(|c| c.attr("name") == Some("Y"))?;
        let sx = scale(source, channels.get(x)?)?;
        let sy = scale(source, channels.get(y)?)?;
        let brush_ref = brush_ref.or_else(|| context.and_then(|c| c.attr("brushRef")));
        let brush = match brush_ref {
            Some(r) => reference(&nodes, r, "brush")?,
            None => root.child("brush")?,
        };
        if brush.attr("brushRef").is_some() {
            return None;
        }
        let mut color = Color::Rgb(0);
        let mut width = 2.0;
        let mut ignore_pressure = false;
        let mut tool = InkTool::Pen;
        let mut effect = None;
        let mut effect_origin = [0.0; 2];
        for p in brush.kids("brushProperty") {
            match p.attr("name")? {
                "color" => color = Color::from_hex(p.attr("value")?)?,
                "width" | "height" => {
                    let size = p.attr_f64("value")? * length(p.attr("units")?)?;
                    if size <= 0.0 || !size.is_finite() {
                        return None;
                    }
                    if p.attr("name") == Some("width") {
                        width = size;
                    }
                }
                "ignorePressure" => {
                    ignore_pressure = match p.attr("value")? {
                        "true" | "1" => true,
                        "false" | "0" => false,
                        _ => return None,
                    }
                }
                "inkEffects" => effect = Some(InkEffect::parse(p.attr("value")?)?),
                "inkKind" => tool = InkTool::parse(p.attr("value")?)?,
                "transparency" => {
                    let value = p.attr_f64("value")?;
                    if !(0.0..=255.0).contains(&value) {
                        return None;
                    }
                    if value > 80.0 {
                        tool = InkTool::Highlighter;
                    } else if (value - 77.0).abs() < 0.5 {
                        tool = InkTool::Pencil;
                    }
                }
                "tip" if matches!(p.attr("value"), Some("ellipse" | "rectangle")) => {}
                "rasterOp" if p.attr("value") == Some("copyPen") => {}
                "antiAliased" | "fitToCurve" if matches!(p.attr("value"), Some("true" | "false" | "0" | "1")) => {}
                "inkOriginX" | "inkOriginY" => {
                    let v = p.attr_f64("value")?;
                    if !v.is_finite() || v.abs() > 1e7 {
                        return None;
                    }
                    effect_origin[usize::from(p.attr("name") == Some("inkOriginY"))] = v as f32;
                }
                "anchorX" | "anchorY" | "scaleFactor" => {
                    p.attr_f64("value")?;
                }
                // Other brush effects are not representable by our model.
                _ => return None,
            }
        }
        let force = channels.iter().position(|c| c.attr("name") == Some("F"));
        let pressure_range = if ignore_pressure {
            None
        } else {
            force.and_then(|f| channels.get(f)).and_then(|c| c.attr_f64("max").map(|max| (c.attr_f64("min").unwrap_or(0.0), max)))
        };
        if pressure_range.is_some_and(|(min, max)| max <= min) {
            return None;
        }
        let mut pts = points(&e.text, channels.len(), x, y, force)?;
        let mut pressures = pressure_range
            .map(|(min, max)| pts.iter().map(|p| ((p[2] - min) / (max - min)).clamp(0.0, 1.0) as f32).collect::<Vec<_>>())
            .unwrap_or_default();
        total = total.saturating_add(pts.len());
        if total > MAX_TOTAL_POINTS || shapes.len() >= MAX_TRACES {
            return None;
        }
        for pt in &mut pts {
            pt[0] *= sx;
            pt[1] *= sy;
            if pt.iter().take(2).any(|v| !v.is_finite() || v.abs() > 1e7) {
                return None;
            }
        }
        let minx = pts.iter().map(|p| p[0]).fold(f64::INFINITY, f64::min);
        let miny = pts.iter().map(|p| p[1]).fold(f64::INFINITY, f64::min);
        let maxx = pts.iter().map(|p| p[0]).fold(f64::NEG_INFINITY, f64::max);
        let maxy = pts.iter().map(|p| p[1]).fold(f64::NEG_INFINITY, f64::max);
        let mut stroke_anchor = anchor;
        stroke_anchor.dx += minx as f32;
        stroke_anchor.dy += miny as f32;
        stroke_anchor.width = (maxx - minx).max(1.0) as f32;
        stroke_anchor.height = (maxy - miny).max(1.0) as f32;
        let mut points: Vec<[f32; 2]> = pts.iter().map(|p| [(p[0] - minx) as f32, (p[1] - miny) as f32]).collect();
        // A tap is a dot: a stroke needs two points.
        if let (1, Some(&p)) = (points.len(), points.first()) {
            points.push(p);
            if let Some(&f) = pressures.first() {
                pressures.push(f);
            }
        }
        let ink = Ink { points, width: width.clamp(0.1, 40.0) as f32, pressures, tool, effect, effect_origin };
        shapes.push(Shape::new_ink(0, stroke_anchor, color, ink));
    }
    (!shapes.is_empty()).then_some(shapes)
}

/// Native Excel archival ink: explicit contexts/brushes on each trace (MS-ODRAWXML 2.1.4).
pub fn write(shape: &Shape, ink: &Ink, theme: &gridcraft_model::Theme) -> String {
    use std::fmt::Write as _;
    let mut xml = String::from(crate::write::XML_DECL);
    xml.push_str(r##"<ink xmlns="http://www.w3.org/2003/InkML"><definitions><context xml:id="ctx"><inkSource xml:id="source"><traceFormat><channel name="X" type="decimal" units="cm"/><channel name="Y" type="decimal" units="cm"/><channel name="F" type="decimal" units="dev" min="0" max="32767"/></traceFormat><channelProperties><channelProperty channel="X" name="resolution" value="1000" units="1/cm"/><channelProperty channel="Y" name="resolution" value="1000" units="1/cm"/></channelProperties></inkSource></context><brush xml:id="brush">"##);
    let color = shape.line.resolve(theme).unwrap_or([0, 0, 0]);
    let width = ink.width() as f64 * 2.54 / 96.0;
    let _ = write!(
        xml,
        "<brushProperty name=\"width\" value=\"{width}\" units=\"cm\"/><brushProperty name=\"height\" value=\"{width}\" units=\"cm\"/><brushProperty name=\"color\" value=\"#{:02X}{:02X}{:02X}\"/><brushProperty name=\"ignorePressure\" value=\"{}\"/>",
        color[0],
        color[1],
        color[2],
        !ink.has_pressure()
    );
    let _ = write!(
        xml,
        "<brushProperty name=\"inkKind\" value=\"{}\"/><brushProperty name=\"inkOriginX\" value=\"{}\"/><brushProperty name=\"inkOriginY\" value=\"{}\"/>",
        ink.tool.id(),
        ink.effect_origin[0],
        ink.effect_origin[1]
    );
    if let Some(effect) = ink.effect {
        let _ = write!(
            xml,
            "<brushProperty name=\"inkEffects\" value=\"{}\"/><brushProperty name=\"anchorX\" value=\"0\"/><brushProperty name=\"anchorY\" value=\"0\"/><brushProperty name=\"scaleFactor\" value=\"1\"/>",
            effect.id()
        );
    }
    let (transparency, tip) = match ink.tool {
        InkTool::Highlighter => (153, "rectangle"),
        InkTool::Pencil => (77, "ellipse"),
        InkTool::Pen => (0, "ellipse"),
    };
    let _ = write!(
        xml,
        "<brushProperty name=\"transparency\" value=\"{transparency}\"/><brushProperty name=\"tip\" value=\"{tip}\"/><brushProperty name=\"rasterOp\" value=\"copyPen\"/><brushProperty name=\"antiAliased\" value=\"true\"/><brushProperty name=\"fitToCurve\" value=\"false\"/>"
    );
    xml.push_str("</brush></definitions><trace xml:id=\"stroke\" contextRef=\"#ctx\" brushRef=\"#brush\">");
    // Points are relative to the content part's offset, in thousandths of a centimetre.
    for (i, p) in ink.valid_points().enumerate() {
        if i > 0 {
            xml.push(',');
        }
        let _ = write!(xml, "{} {} {}", p[0] as f64 * 2540.0 / 96.0, p[1] as f64 * 2540.0 / 96.0, ink.force(i) as f64 * 32767.0);
    }
    xml.push_str("</trace></ink>");
    xml
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn differential_channels_and_wildcards() {
        let p = points("10 20 9,'2'-3'0,\"1\"2\"0,1-2 0,!5!6!0,* * *", 3, 0, 1, None).unwrap();
        assert_eq!(p, vec![[10.0, 20.0, 0.0], [12.0, 17.0, 0.0], [15.0, 16.0, 0.0], [19.0, 13.0, 0.0], [5.0, 6.0, 0.0], [5.0, 6.0, 0.0]]);
        assert_eq!(points("1e2 .5,'-1'+2,\"0\"0", 2, 0, 1, None).unwrap(), vec![[100.0, 0.5, 0.0], [99.0, 2.5, 0.0], [98.0, 4.5, 0.0]]);
    }
    #[test]
    fn hostile_traces_rejected() {
        for s in ["", "1", "NaN 2", "1e999 2", "'1'2", "1 2,\"1\"1", "1 2,3", "1 2,3 4 garbage", "* *", "1e12 0,'1'0"] {
            assert!(points(s, 2, 0, 1, None).is_none(), "{s}");
        }
        assert!(points(&std::iter::repeat_n("1 2", MAX_POINTS + 1).collect::<Vec<_>>().join(","), 2, 0, 1, None).is_none());
    }
}
