//! Draw tab: pens, erasers, lasso selection, ink strokes, Ink to Shape.

use gridcraft_core::CellRef;
use gridcraft_model::*;
use serde_json::{Value as Json, json};

use super::*;

/// The active Draw tool.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum DrawTool {
    /// No Draw tool: clicks select cells and objects.
    #[default]
    Select,
    Pen,
    Eraser,
    Lasso,
}

impl DrawTool {
    pub fn id(self) -> &'static str {
        match self {
            DrawTool::Select => "",
            DrawTool::Pen => "pen",
            DrawTool::Eraser => "eraser",
            DrawTool::Lasso => "lasso",
        }
    }
}

/// What the eraser removes.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum EraserMode {
    /// Whole strokes.
    #[default]
    Stroke,
    /// The part of a stroke between its crossings with other strokes (or itself).
    Segment,
    /// The ink under a small point.
    Small,
    /// The ink under a larger point.
    Medium,
}

impl EraserMode {
    pub const ALL: [EraserMode; 4] = [EraserMode::Stroke, EraserMode::Segment, EraserMode::Small, EraserMode::Medium];
    pub fn id(self) -> &'static str {
        match self {
            EraserMode::Stroke => "stroke",
            EraserMode::Segment => "segment",
            EraserMode::Small => "small",
            EraserMode::Medium => "medium",
        }
    }
    pub fn parse(id: &str) -> Option<EraserMode> {
        EraserMode::ALL.into_iter().find(|m| m.id() == id)
    }
    pub fn label(self) -> &'static str {
        match self {
            EraserMode::Stroke => "Stroke Eraser",
            EraserMode::Segment => "Segment Eraser",
            EraserMode::Small => "Small Eraser",
            EraserMode::Medium => "Medium Eraser",
        }
    }
    /// How far from the pointer ink is hit, in sheet pixels (beyond the stroke's half width).
    fn radius(self) -> f32 {
        if self == EraserMode::Medium { 12.0 } else { 5.0 }
    }
}

/// The Draw tab's state: an application setting, not saved in workbooks.
#[derive(Clone, Debug, PartialEq)]
pub struct DrawState {
    pub tool: DrawTool,
    /// The selected pen (index into `Prefs::ink_pens`) and what new strokes look like.
    pub pen: usize,
    pub color: Color,
    /// Width in sheet pixels.
    pub width: f32,
    pub ink_tool: InkTool,
    pub effect: Option<InkEffect>,
    pub eraser: EraserMode,
    /// Draw with Trackpad is on.
    pub trackpad: bool,
    /// Ink selected with the lasso (shape ids).
    pub selection: Vec<u32>,
}

impl Default for DrawState {
    fn default() -> Self {
        DrawState {
            tool: DrawTool::Select,
            pen: 0,
            color: Color::Rgb(0x1F5FC9),
            width: 2.0,
            ink_tool: InkTool::Pen,
            effect: None,
            eraser: EraserMode::Stroke,
            trackpad: false,
            selection: Vec::new(),
        }
    }
}

impl DrawState {
    /// The selected ink on `sh` (selection ids that still name ink there).
    pub fn selected_on<'a>(&'a self, sh: &'a Sheet) -> impl Iterator<Item = &'a Shape> + 'a {
        sh.shapes.iter().filter(|s| s.kind == ShapeKind::Ink && self.selection.contains(&s.id))
    }
}

pub fn specs() -> Vec<CommandSpec> {
    vec![
        cmd!(noundo "draw.pen", "Pen", ["Draw", "Tools"], None, "{on?: bool, index?: pen index, color?: hex, width?: sheet pixels, tool?: pen|pencil|highlighter, effect?: effect id or \"\"}", always, pen),
        cmd!(noundo "draw.eraser", "Eraser", ["Draw", "Tools"], None, "{on?: bool, mode?: stroke|segment|small|medium}", always, eraser),
        cmd!(noundo "draw.lasso", "Lasso Select", ["Draw", "Tools"], None, "{on?: bool} (drag a loop around ink to select it)", always, |s, p| {
            let on = bool_param(p, "on").unwrap_or(s.draw.tool != DrawTool::Lasso);
            s.draw.tool = if on { DrawTool::Lasso } else { DrawTool::Select };
            Ok(json!({"tool": s.draw.tool.id()}))
        }),
        cmd!(noundo "draw.toggle", "Draw", ["Draw", "Tools"], None, "{on?: bool} (the selected pen)", always, |s, p| {
            let on = bool_param(p, "on").unwrap_or(s.draw.tool != DrawTool::Pen);
            pen(s, &json!({"on": on, "index": s.draw.pen}))
        }),
        cmd!(noundo "draw.trackpad", "Draw with Trackpad", ["Draw", "Tools"], None, "{on?: bool}", always, |s, p| {
            s.draw.trackpad = bool_param(p, "on").unwrap_or(!s.draw.trackpad);
            if s.draw.trackpad {
                pen(s, &json!({"on": true, "index": s.draw.pen}))?;
            }
            Ok(json!({"on": s.draw.trackpad}))
        }),
        cmd!(noundo "draw.exit", "Exit Draw Mode", [], None, "{} (no tool, trackpad off, nothing selected)", always, |s, _| {
            s.draw.tool = DrawTool::Select;
            s.draw.trackpad = false;
            s.draw.selection.clear();
            ok()
        }),
        cmd!(noundo "draw.pen.add", "Add Pen", ["Draw", "Pens"], None, "{tool?: pen|pencil|highlighter}", always, add_pen),
        cmd!(noundo "draw.pen.update", "Pen Settings", ["Draw", "Pens"], None, "{index?: pen index (default: the selected pen), color?: hex, width?: sheet pixels, effect?: effect id or \"\"}", always, update_pen),
        cmd!(noundo "draw.pen.delete", "Delete Pen", ["Draw", "Pens"], None, "{index?: pen index (default: the selected pen)}", always, delete_pen),
        cmd!(noundo "draw.select", "Select Ink", [], None, "{points: [[x,y],…] loop in sheet pixels} (selects the ink inside it)", has_doc, select_ink),
        cmd!("draw.selection.move", "Move Selected Ink", [], None, "{dx: sheet pixels, dy: sheet pixels}", has_doc, move_ink),
        cmd!("draw.selection.delete", "Delete Selected Ink", [], None, "{}", has_doc, delete_ink),
        cmd!("draw.erase", "Erase Ink", [], None, "{x, y: sheet pixels, mode?: stroke|segment|small|medium}", has_doc, erase_ink),
        cmd!(
            "draw.stroke",
            "Ink Stroke",
            [],
            None,
            "{points: [[x,y],…] in sheet pixels, color?: hex, width?: sheet pixels, times?: seconds per point, pressures?: 0..1 per point, smooth?: bool (default: with times)}",
            has_doc,
            stroke
        ),
        cmd!(
            "draw.inkToShape",
            "Ink to Shape",
            ["Draw", "Convert"],
            None,
            "{id?} converts an ink stroke (default: the last) into a rectangle, ellipse, triangle or line",
            has_doc,
            ink_to_shape
        ),
    ]
}

fn pen_index(s: &Session, p: &Json, cmd: &str) -> Result<usize> {
    let Some(v) = p.get("index") else { return Ok(s.draw.pen) };
    let i = v.as_u64().and_then(|i| usize::try_from(i).ok()).ok_or_else(|| bad(cmd, "index must be a pen number"))?;
    if i >= s.prefs.ink_pens.len() {
        return Err(bad(cmd, "pen not found"));
    }
    Ok(i)
}

fn effect_param(p: &Json, cmd: &str) -> Result<Option<Option<InkEffect>>> {
    match str_param(p, "effect") {
        None => Ok(None),
        Some("") => Ok(Some(None)),
        Some(id) => InkEffect::parse(id).map(|e| Some(Some(e))).ok_or_else(|| bad(cmd, "unknown effect")),
    }
}

fn pen(s: &mut Session, p: &Json) -> Result<Json> {
    const CMD: &str = "draw.pen";
    let on = bool_param(p, "on").unwrap_or(s.draw.tool != DrawTool::Pen);
    let effect = effect_param(p, CMD)?;
    let tool = str_param(p, "tool").map(|t| InkTool::parse(t).ok_or_else(|| bad(CMD, "unknown pen tool"))).transpose()?;
    let color = str_param(p, "color").map(|c| Color::from_hex(c).ok_or_else(|| bad(CMD, "invalid colour"))).transpose()?;
    if p.get("index").is_some() {
        let index = pen_index(s, p, CMD)?;
        if let Some(preset) = s.prefs.ink_pens.get(index) {
            s.draw.pen = index;
            s.draw.color = preset.color;
            s.draw.width = preset.width;
            s.draw.ink_tool = preset.tool;
            s.draw.effect = preset.effect;
        }
    }
    if let Some(c) = color {
        s.draw.color = c;
        s.draw.effect = None;
    }
    if let Some(w) = f64_param(p, "width") {
        s.draw.width = (w as f32).clamp(MIN_INK_WIDTH, MAX_INK_WIDTH);
    }
    if let Some(t) = tool {
        s.draw.ink_tool = t;
    }
    if let Some(e) = effect {
        s.draw.effect = e;
    }
    s.draw.tool = if on { DrawTool::Pen } else { DrawTool::Select };
    Ok(json!({"tool": s.draw.tool.id(), "pen": s.draw.pen, "width": s.draw.width}))
}

fn eraser(s: &mut Session, p: &Json) -> Result<Json> {
    if let Some(mode) = str_param(p, "mode") {
        s.draw.eraser = EraserMode::parse(mode).ok_or_else(|| bad("draw.eraser", "invalid mode"))?;
    }
    let on = bool_param(p, "on").unwrap_or(s.draw.tool != DrawTool::Eraser);
    s.draw.tool = if on { DrawTool::Eraser } else { DrawTool::Select };
    Ok(json!({"tool": s.draw.tool.id(), "mode": s.draw.eraser.id()}))
}

fn add_pen(s: &mut Session, p: &Json) -> Result<Json> {
    if s.prefs.ink_pens.len() >= 32 {
        return Err(bad("draw.pen.add", "maximum 32 pens"));
    }
    let tool = InkTool::parse(str_param(p, "tool").or_else(|| str_param(p, "kind")).unwrap_or("pen"))
        .ok_or_else(|| bad("draw.pen.add", "unknown pen tool"))?;
    s.prefs.ink_pens.push(InkPen::new(tool));
    let index = s.prefs.ink_pens.len().saturating_sub(1);
    pen(s, &json!({"index": index, "on": true}))
}

fn update_pen(s: &mut Session, p: &Json) -> Result<Json> {
    const CMD: &str = "draw.pen.update";
    let index = pen_index(s, p, CMD)?;
    let mut preset = s.prefs.ink_pens.get(index).cloned().ok_or_else(|| bad(CMD, "pen not found"))?;
    if let Some(color) = str_param(p, "color") {
        preset.color = Color::from_hex(color).ok_or_else(|| bad(CMD, "invalid colour"))?;
        preset.effect = None;
    }
    if let Some(width) = f64_param(p, "width") {
        preset.width = (width as f32).clamp(MIN_INK_WIDTH, MAX_INK_WIDTH);
    }
    if let Some(effect) = effect_param(p, CMD)? {
        preset.effect = effect;
    }
    if let Some(slot) = s.prefs.ink_pens.get_mut(index) {
        *slot = preset;
    }
    // Editing a pen selects it.
    let on = s.draw.tool == DrawTool::Pen;
    pen(s, &json!({"index": index, "on": on}))
}

fn delete_pen(s: &mut Session, p: &Json) -> Result<Json> {
    const CMD: &str = "draw.pen.delete";
    let index = pen_index(s, p, CMD)?;
    if s.prefs.ink_pens.len() <= 1 {
        return Err(bad(CMD, "keep at least one pen"));
    }
    s.prefs.ink_pens.remove(index);
    // The selected pen stays selected; deleting it selects its neighbour.
    let selected = if s.draw.pen > index { s.draw.pen - 1 } else { s.draw.pen };
    let selected = selected.min(s.prefs.ink_pens.len().saturating_sub(1));
    let on = s.draw.tool == DrawTool::Pen;
    pen(s, &json!({"index": selected, "on": on}))
}

/// Points from a `[[x,y],…]` parameter: `None` for an entry that isn't a finite point in range,
/// so callers can keep per-point arrays aligned.
fn points_param(p: &Json, key: &str) -> Vec<Option<[f64; 2]>> {
    let valid = |v: f64| v.is_finite() && v.abs() <= MAX_INK_COORD as f64;
    p.get(key)
        .and_then(Json::as_array)
        .map(|a| a.iter().map(|q| Some([q.get(0)?.as_f64()?, q.get(1)?.as_f64()?]).filter(|q| q.iter().all(|v| valid(*v)))).collect())
        .unwrap_or_default()
}

/// A stroke being drawn, in sheet pixels.
#[derive(Clone, Debug, Default)]
pub struct StrokeInput {
    pub points: Vec<[f64; 2]>,
    /// Seconds per point (for speed-dependent width), or empty.
    pub times: Vec<f64>,
    /// Stylus force per point (0..=1), or empty.
    pub pressures: Vec<f32>,
    pub smooth: bool,
}

/// The ink shape a stroke makes with the current pen (id 0, anchored where it was drawn).
/// Shared by `draw.stroke` and the live preview, so what you see is what is saved.
pub fn stroke_shape(sh: &Sheet, draw: &DrawState, input: &StrokeInput) -> Option<Shape> {
    let n = input.points.len().min(MAX_INK_POINTS);
    let raw = input.points.get(..n)?;
    if raw.len() < 2 {
        return None;
    }
    let mut force = if input.pressures.len() >= n {
        input.pressures.iter().take(n).map(|p| if p.is_finite() { p.clamp(0.0, 1.0) } else { 0.5 }).collect()
    } else if input.times.len() >= n {
        speed_pressures(raw, input.times.get(..n)?)
    } else {
        Vec::new()
    };
    let mut points = raw.to_vec();
    if input.smooth {
        (points, force) = smooth_stroke(&points, &force);
    }
    let min =
        [points.iter().map(|q| q[0]).fold(f64::INFINITY, f64::min).max(0.0), points.iter().map(|q| q[1]).fold(f64::INFINITY, f64::min).max(0.0)];
    let max = [points.iter().map(|q| q[0]).fold(f64::NEG_INFINITY, f64::max), points.iter().map(|q| q[1]).fold(f64::NEG_INFINITY, f64::max)];
    let ink = Ink {
        points: points.iter().map(|q| [(q[0] - min[0]) as f32, (q[1] - min[1]) as f32]).collect(),
        width: draw.width.clamp(MIN_INK_WIDTH, MAX_INK_WIDTH),
        pressures: force,
        tool: draw.ink_tool,
        effect: draw.effect,
        effect_origin: [0.0; 2],
    };
    let mut anchor = anchor_at(sh, min[0], min[1]);
    anchor.width = (max[0] - min[0]).max(1.0) as f32;
    anchor.height = (max[1] - min[1]).max(1.0) as f32;
    Some(Shape::new_ink(0, anchor, draw.color, ink))
}

/// An absolute anchor whose top-left is at sheet pixel `(x, y)`. Ink is a freehand mark: pinned
/// where it was drawn, it does not follow cells.
fn anchor_at(sh: &Sheet, x: f64, y: f64) -> Anchor {
    let (x, y) = (x.clamp(0.0, MAX_INK_COORD as f64), y.clamp(0.0, MAX_INK_COORD as f64));
    let cell = CellRef::new(sh.row_at(y), sh.col_at(x));
    Anchor {
        cell,
        dx: (x - sh.col_left(cell.col)) as f32,
        dy: (y - sh.row_top(cell.row)) as f32,
        width: 1.0,
        height: 1.0,
        mode: AnchorMode::Absolute,
    }
}

/// Where a shape's anchor sits on the sheet, in sheet pixels.
pub fn anchor_origin(sh: &Sheet, a: &Anchor) -> [f32; 2] {
    [sh.col_left(a.cell.col) as f32 + a.dx, sh.row_top(a.cell.row) as f32 + a.dy]
}

fn stroke(s: &mut Session, p: &Json) -> Result<Json> {
    const CMD: &str = "draw.stroke";
    let raw = points_param(p, "points");
    let per_point = |key: &str| -> Result<Vec<Json>> {
        match p.get(key) {
            None => Ok(Vec::new()),
            Some(Json::Array(a)) if a.len() == raw.len() => Ok(a.clone()),
            Some(_) => Err(bad(CMD, format!("{key} must have one value per point"))),
        }
    };
    let times = per_point("times")?;
    let pressures = per_point("pressures")?;
    // Drop invalid points together with their samples.
    let mut input = StrokeInput { smooth: bool_param(p, "smooth").unwrap_or(!times.is_empty()), ..Default::default() };
    for (i, q) in raw.iter().enumerate() {
        let Some(q) = q else { continue };
        input.points.push(*q);
        if let Some(t) = times.get(i) {
            input.times.push(t.as_f64().filter(|t| t.is_finite()).unwrap_or(0.0));
        }
        if let Some(f) = pressures.get(i) {
            input.pressures.push(f.as_f64().unwrap_or(0.5) as f32);
        }
    }
    if input.points.len() < 2 {
        return Err(bad(CMD, "a stroke needs at least two points"));
    }
    let mut draw = s.draw.clone();
    if let Some(c) = str_param(p, "color") {
        draw.color = Color::from_hex(c).ok_or_else(|| bad(CMD, "invalid colour"))?;
        draw.effect = None;
    }
    if let Some(w) = f64_param(p, "width") {
        draw.width = w as f32;
    }
    let si = s.doc()?.wb.active_sheet;
    edit(s, |cx| {
        let id = cx.wb.next_object_id();
        let sh = cx.sheet_mut(si)?;
        let mut shape = stroke_shape(sh, &draw, &input).ok_or_else(|| bad(CMD, "a stroke needs at least two points"))?;
        shape.id = id;
        sh.shapes.push(shape);
        Ok(json!({"shape": id}))
    })
}

fn ink_to_shape(s: &mut Session, p: &Json) -> Result<Json> {
    let si = s.doc()?.wb.active_sheet;
    let id = u32_param(p, "id");
    edit(s, |cx| {
        let sh = cx.sheet_mut(si)?;
        let ink_shape = match id {
            Some(id) => sh.shapes.iter_mut().find(|x| x.id == id && x.kind == ShapeKind::Ink),
            None => sh.shapes.iter_mut().rev().find(|x| x.kind == ShapeKind::Ink),
        }
        .ok_or_else(|| bad("draw.inkToShape", "no ink stroke to convert"))?;
        let pts: Vec<_> = ink_shape.ink.as_ref().map(|i| i.valid_points().collect()).unwrap_or_default();
        let (w, h) = (ink_shape.anchor.width.max(1.0), ink_shape.anchor.height.max(1.0));
        let closed = match (pts.first(), pts.last()) {
            (Some(a), Some(b)) => (a[0] - b[0]).hypot(a[1] - b[1]) < 0.25 * w.max(h),
            _ => false,
        };
        let kind = if !closed {
            ShapeKind::Line
        } else {
            // Corners hugging the bounding box mean a rectangle; one apex at the top a triangle.
            let near = |cx: f32, cy: f32| pts.iter().any(|q| (q[0] - cx).hypot(q[1] - cy) < 0.15 * w.min(h).max(8.0));
            let corners = [near(0.0, 0.0), near(w, 0.0), near(0.0, h), near(w, h)].iter().filter(|b| **b).count();
            if corners >= 3 {
                ShapeKind::Rectangle
            } else if near(w / 2.0, 0.0) && near(0.0, h) && near(w, h) {
                ShapeKind::Triangle
            } else {
                ShapeKind::Ellipse
            }
        };
        ink_shape.kind = kind;
        ink_shape.ink = None;
        ink_shape.fill = if kind == ShapeKind::Line { ink_shape.line } else { Color::Auto };
        ink_shape.text.clear();
        Ok(json!({"kind": format!("{kind:?}")}))
    })
}

/// Most one smoothing pass moves a point (sheet pixels): enough to settle hand and sensor
/// jitter, too little to pull a fast stroke's turns inward (Excel keeps them where they were drawn).
const JITTER: f64 = 1.5;
/// Sheet pixels between interpolated points, and the most points one segment gets.
const SPACING: f64 = 3.0;
const MAX_STEPS: usize = 16;

/// Two jitter-bounded smoothing passes, then a centripetal Catmull-Rom curve through every
/// sample: it passes through the samples, so sharp, far-apart turns keep their reach, and
/// unlike the uniform spline it never loops or overshoots between unevenly spaced samples.
/// Endpoints stay fixed.
fn smooth_stroke(points: &[[f64; 2]], pressure: &[f32]) -> (Vec<[f64; 2]>, Vec<f32>) {
    let points = points.get(..points.len().min(MAX_INK_POINTS)).unwrap_or_default();
    let mut filtered = points.to_vec();
    for _ in 0..2 {
        let before = filtered.clone();
        for i in 1..points.len().saturating_sub(1) {
            if let (Some(a), Some(b), Some(c), Some(out)) = (before.get(i - 1), before.get(i), before.get(i + 1), filtered.get_mut(i)) {
                let (dx, dy) = ((a[0] + 2.0 * b[0] + c[0]) / 4.0 - b[0], (a[1] + 2.0 * b[1] + c[1]) / 4.0 - b[1]);
                let d = dx.hypot(dy);
                let k = if d > JITTER { JITTER / d } else { 1.0 };
                *out = [b[0] + dx * k, b[1] + dy * k];
            }
        }
    }
    // Interpolation multiplies the points: keep the result under the stroke cap.
    let segments = filtered.len().saturating_sub(1).max(1);
    let budget = ((MAX_INK_POINTS - 1) / segments).clamp(1, MAX_STEPS);
    let mut result = Vec::new();
    let mut forces = Vec::new();
    for (i, pair) in filtered.windows(2).enumerate() {
        let (b, c) = (pair[0], pair[1]);
        let length = (c[0] - b[0]).hypot(c[1] - b[1]);
        // Past the ends, mirror the neighbour so the end segments curve like the rest.
        let a = i.checked_sub(1).and_then(|h| filtered.get(h)).copied().unwrap_or([2.0 * b[0] - c[0], 2.0 * b[1] - c[1]]);
        let d = filtered.get(i + 2).copied().unwrap_or([2.0 * c[0] - b[0], 2.0 * c[1] - b[1]]);
        let steps = ((length / SPACING).ceil() as usize).clamp(1, budget);
        for k in 0..steps {
            let t = k as f64 / steps as f64;
            result.push(centripetal(a, b, c, d, t));
            if let Some(&p) = pressure.get(i) {
                forces.push(p + (pressure.get(i + 1).copied().unwrap_or(p) - p) * t as f32);
            }
        }
    }
    if let Some(&last) = points.last() {
        result.push(last);
    }
    if let Some(&last) = pressure.last() {
        forces.push(last);
    }
    (result, forces)
}

/// The point at `t` (0..1) between `b` and `c` on the centripetal Catmull-Rom curve through
/// `a, b, c, d` (Barry and Goldman's pyramid, knots spaced by the square root of the distance).
/// Coincident samples fall back to the straight segment.
fn centripetal(a: [f64; 2], b: [f64; 2], c: [f64; 2], d: [f64; 2], t: f64) -> [f64; 2] {
    let lerp = |p: [f64; 2], q: [f64; 2], u: f64| [p[0] + (q[0] - p[0]) * u, p[1] + (q[1] - p[1]) * u];
    let knot = |p: [f64; 2], q: [f64; 2]| (q[0] - p[0]).hypot(q[1] - p[1]).sqrt();
    let (k1, k2, k3) = (knot(a, b), knot(b, c), knot(c, d));
    const EPS: f64 = 1e-6;
    if k1 < EPS || k2 < EPS || k3 < EPS {
        return lerp(b, c, t);
    }
    let (t0, t1) = (0.0, k1);
    let (t2, t3) = (t1 + k2, t1 + k2 + k3);
    let u = t1 + (t2 - t1) * t;
    let mix = |p: [f64; 2], q: [f64; 2], from: f64, to: f64| lerp(p, q, (u - from) / (to - from));
    let (a1, a2, a3) = (mix(a, b, t0, t1), mix(b, c, t1, t2), mix(c, d, t2, t3));
    let (b1, b2) = (mix(a1, a2, t0, t2), mix(a2, a3, t1, t3));
    let p = mix(b1, b2, t1, t2);
    if p.iter().all(|v| v.is_finite()) { p } else { lerp(b, c, t) }
}

/// Smoothed inverse speed response: slow drawing is thick, fast drawing thin.
fn speed_pressures(points: &[[f64; 2]], times: &[f64]) -> Vec<f32> {
    let mut previous = 0.5_f32;
    points
        .iter()
        .take(MAX_INK_POINTS)
        .enumerate()
        .map(|(i, point)| {
            let speed = if i > 0 {
                let before = points.get(i - 1).copied().unwrap_or(*point);
                let dt = times.get(i).copied().unwrap_or(0.0) - times.get(i - 1).copied().unwrap_or(0.0);
                let dt = if dt.is_finite() { dt.clamp(0.001, 1.0) } else { 0.016 };
                let speed = (point[0] - before[0]).hypot(point[1] - before[1]) / dt;
                if speed.is_finite() { speed } else { 300.0 }
            } else {
                300.0
            };
            previous += 0.35 * ((1.0 / (1.0 + speed / 300.0)) as f32 - previous);
            previous
        })
        .collect()
}

// ---------------------------------------------------------------- lasso

/// Most loop points a lasso keeps; longer loops are thinned evenly, never cut short.
const MAX_LASSO_POINTS: usize = 5_000;
/// Most point-against-loop tests one lasso may cost (keeps the live preview interactive).
const MAX_LASSO_WORK: usize = 50_000_000;

fn inside(p: [f32; 2], polygon: &[[f32; 2]]) -> bool {
    let mut inside = false;
    let Some(mut before) = polygon.last().copied() else { return false };
    for &next in polygon {
        if (next[1] > p[1]) != (before[1] > p[1]) && p[0] < (before[0] - next[0]) * (p[1] - next[1]) / (before[1] - next[1]) + next[0] {
            inside = !inside;
        }
        before = next;
    }
    inside
}

fn bbox(points: impl IntoIterator<Item = [f32; 2]>) -> Option<([f32; 2], [f32; 2])> {
    points.into_iter().fold(None, |acc, p| {
        let (lo, hi) = acc.unwrap_or((p, p));
        Some(([lo[0].min(p[0]), lo[1].min(p[1])], [hi[0].max(p[0]), hi[1].max(p[1])]))
    })
}

/// The ink on `sh` lying entirely inside the loop `polygon` (sheet pixels). Shared by
/// `draw.select` and the live preview.
pub fn lasso_candidates(sh: &Sheet, polygon: &[[f32; 2]]) -> Result<Vec<u32>> {
    if polygon.iter().flatten().any(|v| !v.is_finite() || v.abs() > MAX_INK_COORD) {
        return Err(bad("draw.select", "invalid loop"));
    }
    let step = polygon.len().div_ceil(MAX_LASSO_POINTS).max(1);
    let polygon: Vec<_> = polygon.iter().step_by(step).copied().collect();
    let Some((lo, hi)) = bbox(polygon.iter().copied()).filter(|_| polygon.len() >= 3) else { return Ok(Vec::new()) };
    let mut work = 0usize;
    let mut selected = Vec::new();
    for shape in sh.shapes.iter().filter(|s| s.kind == ShapeKind::Ink) {
        let Some(ink) = &shape.ink else { continue };
        let o = anchor_origin(sh, &shape.anchor);
        let contour: Vec<_> = ink.outline().iter().map(|q| [o[0] + q[0], o[1] + q[1]]).collect();
        // Cheap rejection first: the stroke's box must be inside the loop's box.
        let Some((a, b)) = bbox(contour.iter().copied()) else { continue };
        if a[0] < lo[0] || a[1] < lo[1] || b[0] > hi[0] || b[1] > hi[1] {
            continue;
        }
        work = work.saturating_add(contour.len().saturating_mul(polygon.len()).saturating_mul(2));
        if work > MAX_LASSO_WORK {
            return Err(bad("draw.select", "too much ink inside the loop to select at once"));
        }
        let edges = |pts: &[[f32; 2]]| pts.iter().copied().zip(pts.iter().copied().cycle().skip(1)).take(pts.len()).collect::<Vec<_>>();
        let loop_edges = edges(&polygon);
        let enclosed = contour.iter().all(|q| inside(*q, &polygon))
            && !edges(&contour).iter().any(|(a, b)| loop_edges.iter().any(|(c, d)| crossing(*a, *b, *c, *d).is_some()));
        if enclosed {
            selected.push(shape.id);
        }
    }
    Ok(selected)
}

fn select_ink(s: &mut Session, p: &Json) -> Result<Json> {
    let polygon: Vec<[f32; 2]> = points_param(p, "points").into_iter().flatten().map(|q| [q[0] as f32, q[1] as f32]).collect();
    let d = s.doc()?;
    let sh = d.wb.sheet(d.wb.active_sheet).ok_or(EngineError::NoDocument)?;
    s.draw.selection = lasso_candidates(sh, &polygon)?;
    Ok(json!({"ids": s.draw.selection}))
}

fn move_ink(s: &mut Session, p: &Json) -> Result<Json> {
    const CMD: &str = "draw.selection.move";
    let offset = |key| f64_param(p, key).filter(|v| v.is_finite()).ok_or_else(|| bad(CMD, format!("{key} required")));
    let (dx, dy) = (offset("dx")?, offset("dy")?);
    let si = s.doc()?.wb.active_sheet;
    let ids = s.draw.selection.clone();
    let d = s.doc()?;
    let sh = d.wb.sheet(si).ok_or(EngineError::NoDocument)?;
    if s.draw.selected_on(sh).next().is_none() {
        return Err(bad(CMD, "no ink selected"));
    }
    edit(s, |cx| {
        let sh = cx.sheet_mut(si)?;
        let moved: Vec<(usize, Anchor)> = sh
            .shapes
            .iter()
            .enumerate()
            .filter(|(_, x)| x.kind == ShapeKind::Ink && ids.contains(&x.id))
            .map(|(i, x)| {
                let o = anchor_origin(sh, &x.anchor);
                let a = anchor_at(sh, o[0] as f64 + dx, o[1] as f64 + dy);
                (i, Anchor { width: x.anchor.width, height: x.anchor.height, ..a })
            })
            .collect();
        for (i, anchor) in moved {
            if let Some(x) = sh.shapes.get_mut(i) {
                x.anchor = anchor;
            }
        }
        ok()
    })
}

fn delete_ink(s: &mut Session, _: &Json) -> Result<Json> {
    let ids = std::mem::take(&mut s.draw.selection);
    let d = s.doc()?;
    let si = d.wb.active_sheet;
    if !d.wb.sheet(si).is_some_and(|sh| sh.shapes.iter().any(|x| x.kind == ShapeKind::Ink && ids.contains(&x.id))) {
        return Err(bad("draw.selection.delete", "no ink selected"));
    }
    edit(s, |cx| {
        cx.sheet_mut(si)?.shapes.retain(|x| x.kind != ShapeKind::Ink || !ids.contains(&x.id));
        ok()
    })
}

// ---------------------------------------------------------------- erasing

fn segment_distance(p: [f32; 2], a: [f32; 2], b: [f32; 2]) -> (f32, f32) {
    let (dx, dy) = (b[0] - a[0], b[1] - a[1]);
    let length = dx * dx + dy * dy;
    let t = if length > 0.0 { ((p[0] - a[0]) * dx + (p[1] - a[1]) * dy) / length } else { 0.0 }.clamp(0.0, 1.0);
    ((p[0] - a[0] - t * dx).hypot(p[1] - a[1] - t * dy), t)
}

/// Where segment `ab` properly crosses segment `cd`: the parameter along `ab`.
fn crossing(a: [f32; 2], b: [f32; 2], c: [f32; 2], d: [f32; 2]) -> Option<f32> {
    let side = |a: [f32; 2], b: [f32; 2], p: [f32; 2]| {
        (b[0] as f64 - a[0] as f64) * (p[1] as f64 - a[1] as f64) - (b[1] as f64 - a[1] as f64) * (p[0] as f64 - a[0] as f64)
    };
    let (c1, c2) = (side(a, b, c), side(a, b, d));
    let (a1, a2) = (side(c, d, a), side(c, d, b));
    (c1 * c2 < 0.0 && a1 * a2 < 0.0).then(|| (a1 / (a1 - a2)) as f32)
}

/// The part of a polyline between positions `from` and `to` (segment index plus fraction),
/// with the matching pressures.
fn cut(points: &[[f32; 2]], pressures: &[f32], from: f32, to: f32) -> (Vec<[f32; 2]>, Vec<f32>) {
    let last = points.len().saturating_sub(1) as f32;
    let (from, to) = (from.clamp(0.0, last), to.clamp(0.0, last));
    let at = |pos: f32| {
        let i = (pos.floor() as usize).min(points.len().saturating_sub(2));
        let t = pos - i as f32;
        let (a, b) = (points.get(i).copied().unwrap_or_default(), points.get(i + 1).copied().unwrap_or_default());
        let p = [a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t];
        let f = match (pressures.get(i), pressures.get(i + 1)) {
            (Some(fa), Some(fb)) => Some(fa + (fb - fa) * t),
            _ => None,
        };
        (p, f)
    };
    let mut out = Vec::new();
    let mut force = Vec::new();
    let mut push = |(p, f): ([f32; 2], Option<f32>)| {
        out.push(p);
        force.extend(f);
    };
    push(at(from));
    for i in (from.floor() as usize + 1)..=(to.ceil() as usize).saturating_sub(1) {
        push((points.get(i).copied().unwrap_or_default(), pressures.get(i).copied()));
    }
    push(at(to));
    if force.len() != out.len() {
        force.clear();
    }
    (out, force)
}

/// Most segment pairs the segment eraser compares; beyond it the whole stroke is erased.
const MAX_SEGMENT_WORK: usize = 20_000_000;

/// The positions (segment index plus fraction) where `path` crosses `others` or itself.
fn crossings(path: &[[f32; 2]], others: &[Vec<[f32; 2]>]) -> Option<Vec<f32>> {
    let segments = path.len().saturating_sub(1);
    let work = others.iter().chain(std::iter::once(&path.to_vec())).map(|o| o.len()).sum::<usize>().saturating_mul(segments);
    if work > MAX_SEGMENT_WORK {
        return None;
    }
    let mut out = Vec::new();
    for (i, s) in path.windows(2).enumerate() {
        for other in others {
            out.extend(other.windows(2).filter_map(|o| crossing(s[0], s[1], o[0], o[1])).map(|t| i as f32 + t));
        }
        out.extend(
            path.windows(2)
                .enumerate()
                .filter(|(j, _)| j.abs_diff(i) > 1)
                .filter_map(|(_, o)| crossing(s[0], s[1], o[0], o[1]))
                .map(|t| i as f32 + t),
        );
    }
    Some(out)
}

/// A new stroke made of part of `base`: `points` are relative to `base`'s anchor origin `o`.
fn fragment(sh: &Sheet, base: &Shape, o: [f32; 2], points: &[[f32; 2]], pressures: Vec<f32>, id: u32) -> Option<Shape> {
    let ink = base.ink.as_ref()?;
    let (lo, hi) = bbox(points.iter().copied()).filter(|_| points.len() >= 2)?;
    let mut anchor = anchor_at(sh, (o[0] + lo[0]) as f64, (o[1] + lo[1]) as f64);
    let origin = anchor_origin(sh, &anchor);
    let shift = [origin[0] - o[0], origin[1] - o[1]];
    anchor.width = (hi[0] - lo[0]).max(1.0);
    anchor.height = (hi[1] - lo[1]).max(1.0);
    anchor.mode = base.anchor.mode;
    let ink = Ink {
        points: points.iter().map(|q| [q[0] - shift[0], q[1] - shift[1]]).collect(),
        pressures,
        effect_origin: [ink.effect_origin[0] + shift[0], ink.effect_origin[1] + shift[1]],
        ..ink.clone()
    };
    Some(Shape::new_ink(id, anchor, base.line, ink))
}

fn erase_ink(s: &mut Session, p: &Json) -> Result<Json> {
    const CMD: &str = "draw.erase";
    let x = f64_param(p, "x").ok_or_else(|| bad(CMD, "x required"))? as f32;
    let y = f64_param(p, "y").ok_or_else(|| bad(CMD, "y required"))? as f32;
    if !x.is_finite() || !y.is_finite() {
        return Err(bad(CMD, "invalid position"));
    }
    let mode = match str_param(p, "mode") {
        Some(m) => EraserMode::parse(m).ok_or_else(|| bad(CMD, "invalid mode"))?,
        None => s.draw.eraser,
    };
    let d = s.doc()?;
    let si = d.wb.active_sheet;
    let sh = d.wb.sheet(si).ok_or(EngineError::NoDocument)?;
    let mut next_id = d.wb.next_object_id();
    // Each ink stroke: its anchor origin and centerline.
    let strokes: Vec<(usize, [f32; 2], Vec<[f32; 2]>)> = sh
        .shapes
        .iter()
        .enumerate()
        .filter_map(|(i, x)| Some((i, anchor_origin(sh, &x.anchor), x.ink.as_ref().filter(|_| x.kind == ShapeKind::Ink)?.valid_points().collect())))
        .collect();
    let mut replace: Vec<(usize, Vec<Shape>)> = Vec::new();
    for (i, o, points) in &strokes {
        let (Some(shape), Some(ink)) = (sh.shapes.get(*i), sh.shapes.get(*i).and_then(|x| x.ink.as_ref())) else { continue };
        let q = [x - o[0], y - o[1]];
        let reach = |seg: usize| mode.radius() + ink.point_width(seg) * 0.5;
        let hits: Vec<Option<f32>> = points
            .windows(2)
            .enumerate()
            .map(|(seg, w)| {
                let (dist, t) = segment_distance(q, w[0], w[1]);
                (dist <= reach(seg)).then_some(seg as f32 + t)
            })
            .collect();
        let single = points.len() == 1 && points.first().is_some_and(|p0| (q[0] - p0[0]).hypot(q[1] - p0[1]) <= reach(0));
        let Some(hit) = hits.iter().flatten().copied().next().or(single.then_some(0.0)) else { continue };
        let pressures = &ink.pressures;
        let pieces = match mode {
            EraserMode::Stroke => Vec::new(),
            EraserMode::Segment => {
                let others: Vec<Vec<[f32; 2]>> = strokes
                    .iter()
                    .filter(|(j, ..)| j != i)
                    .map(|(_, oo, pts)| pts.iter().map(|p| [p[0] + oo[0] - o[0], p[1] + oo[1] - o[1]]).collect())
                    .collect();
                match crossings(points, &others) {
                    Some(cross) => {
                        let last = points.len().saturating_sub(1) as f32;
                        let before = cross.iter().copied().filter(|c| *c < hit).fold(None, |m: Option<f32>, c| Some(m.map_or(c, |m| m.max(c))));
                        let after = cross.iter().copied().filter(|c| *c > hit).fold(None, |m: Option<f32>, c| Some(m.map_or(c, |m| m.min(c))));
                        [before.map(|b| cut(points, pressures, 0.0, b)), after.map(|a| cut(points, pressures, a, last))]
                            .into_iter()
                            .flatten()
                            .collect()
                    }
                    None => Vec::new(),
                }
            }
            EraserMode::Small | EraserMode::Medium => {
                // Keep each run of segments the eraser missed.
                let mut runs = Vec::new();
                let mut start = 0;
                for seg in 0..=hits.len() {
                    if seg == hits.len() || hits.get(seg).is_some_and(Option::is_some) {
                        if seg > start {
                            runs.push(cut(points, pressures, start as f32, seg as f32));
                        }
                        start = seg + 1;
                    }
                }
                runs
            }
        };
        let fragments = pieces
            .into_iter()
            .filter_map(|(pts, force)| {
                let id = next_id;
                next_id = next_id.saturating_add(1);
                fragment(sh, shape, *o, &pts, force, id)
            })
            .collect();
        replace.push((*i, fragments));
    }
    if replace.is_empty() {
        // Nothing under the eraser: no edit, no undo step.
        return Ok(json!({"erased": 0}));
    }
    let erased = replace.len();
    edit(s, |cx| {
        let sh = cx.sheet_mut(si)?;
        let mut shapes = Vec::with_capacity(sh.shapes.len());
        for (i, shape) in std::mem::take(&mut sh.shapes).into_iter().enumerate() {
            match replace.iter_mut().find(|(j, _)| *j == i) {
                Some((_, fragments)) => shapes.append(fragments),
                None => shapes.push(shape),
            }
        }
        sh.shapes = shapes;
        Ok(json!({"erased": erased}))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn session() -> Session {
        let mut s = Session::new();
        s.new_workbook();
        s
    }
    fn shapes(s: &Session) -> Vec<Shape> {
        s.doc().unwrap().wb.active().unwrap().shapes.clone()
    }
    fn undo_len(s: &Session) -> usize {
        s.doc().unwrap().undo.len()
    }

    #[test]
    fn speed_thickness_and_smoothing_preserve_endpoints_and_roundtrip() {
        let points = vec![[20.0, 20.0], [30.0, 30.0], [40.0, 15.0], [50.0, 25.0], [60.0, 20.0]];
        let slow = speed_pressures(&points, &[0.0, 0.3, 0.6, 0.9, 1.2]);
        let fast = speed_pressures(&points, &[0.0, 0.01, 0.02, 0.03, 0.04]);
        assert!(slow.iter().skip(1).zip(fast.iter().skip(1)).all(|(a, b)| a > b));
        let (smooth, force) = smooth_stroke(&points, &slow);
        assert_eq!(smooth.first(), points.first());
        assert_eq!(smooth.last(), points.last());
        assert_eq!(smooth.len(), force.len());
        assert!(smooth.len() > points.len());
        let mut s = session();
        s.execute("draw.pen", json!({"index":0,"on":true})).unwrap();
        s.execute("draw.stroke", json!({"points":points,"times":[0.0,0.3,0.6,0.9,1.2]})).unwrap();
        let wb = &s.doc().unwrap().wb;
        let shape = &wb.active().unwrap().shapes[0];
        assert!(shape.ink.as_ref().unwrap().has_pressure());
        let (back, report) = gridcraft_xlsx::read_xlsx(&gridcraft_xlsx::write_xlsx(wb).unwrap()).unwrap();
        assert!(report.warnings.is_empty(), "{:?}", report.warnings);
        assert_eq!(back.active().unwrap().shapes[0].ink, shape.ink);
    }

    #[test]
    fn fast_zigzag_keeps_its_turns_and_never_loops() {
        // A fast zigzag: few samples, far apart. The smoothed stroke reaches every turn (within
        // the jitter bound) and stays between the turns' heights instead of curling past them.
        let points: Vec<[f64; 2]> = (0..8).map(|i| [i as f64 * 40.0, if i % 2 == 0 { 0.0 } else { 300.0 }]).collect();
        let (smooth, _) = smooth_stroke(&points, &[]);
        for turn in &points {
            let nearest = smooth.iter().map(|q| (q[0] - turn[0]).hypot(q[1] - turn[1])).fold(f64::INFINITY, f64::min);
            assert!(nearest <= 2.0 * JITTER + 0.01, "turn {turn:?} pulled in by {nearest}");
        }
        assert!(smooth.iter().all(|q| q[1] > -2.0 * JITTER - 5.0 && q[1] < 300.0 + 2.0 * JITTER + 5.0), "no overshoot loops");
        // Long segments are filled in, short ones aren't over-sampled, duplicates are harmless.
        assert!(smooth.len() >= points.len() * 8);
        let (dup, _) = smooth_stroke(&[[0.0, 0.0], [0.0, 0.0], [10.0, 0.0], [10.0, 0.0]], &[]);
        assert!(dup.iter().all(|q| q.iter().all(|v| v.is_finite())));
        // Hand jitter is still settled.
        let wobble: Vec<[f64; 2]> = (0..20).map(|i| [i as f64, if i % 2 == 0 { 0.0 } else { 1.0 }]).collect();
        let (calm, _) = smooth_stroke(&wobble, &[]);
        let spread = |v: &[[f64; 2]]| {
            v[2..v.len() - 2].iter().map(|q| q[1]).fold(f64::NEG_INFINITY, f64::max)
                - v[2..v.len() - 2].iter().map(|q| q[1]).fold(f64::INFINITY, f64::min)
        };
        assert!(spread(&calm) < 0.75, "jitter settles: {}", spread(&calm));
    }

    #[test]
    fn stroke_keeps_samples_aligned_with_valid_points() {
        let mut s = session();
        let r = s.execute("draw.stroke", json!({"points":[[10,10],[20,20]],"times":[0.0]}));
        assert!(r.is_err(), "mismatched times are refused, not ignored");
        s.execute("draw.stroke", json!({"points":[[10,10],["x",1],[20,20],[30,10]],"pressures":[0.0,0.5,1.0,0.25],"smooth":false})).unwrap();
        let ink = shapes(&s)[0].ink.clone().unwrap();
        assert_eq!(ink.points.len(), 3);
        assert_eq!(ink.pressures, vec![0.0, 1.0, 0.25]);
    }

    #[test]
    fn stroke_ids_are_unique_across_sheets() {
        let mut s = session();
        s.execute("home.insertSheet", json!({})).unwrap();
        s.execute("draw.stroke", json!({"points":[[10,10],[50,10]]})).unwrap();
        let other = shapes(&s)[0].id;
        s.execute("sheet.activate", json!({"sheet":0})).unwrap();
        s.execute("draw.stroke", json!({"points":[[10,10],[30,10],[50,10],[70,10],[100,10]]})).unwrap();
        s.execute("draw.erase", json!({"x":50,"y":10,"mode":"small"})).unwrap();
        let ids: Vec<u32> = shapes(&s).iter().map(|x| x.id).collect();
        assert_eq!(ids.len(), 2);
        assert!(!ids.contains(&other), "{ids:?} reuses {other}");
    }

    #[test]
    fn pen_settings_validate_and_persist_in_preferences() {
        let mut s = session();
        s.execute("draw.pen.add", json!({"tool":"pencil"})).unwrap();
        let index = s.draw.pen;
        s.execute("draw.pen.update", json!({"width":3.25,"color":"#123456"})).unwrap();
        assert_eq!(s.draw.width, 3.25);
        assert_eq!(s.draw.ink_tool, InkTool::Pencil);
        assert!(s.execute("draw.pen.update", json!({"effect":"untrusted"})).is_err());
        assert!(s.execute("draw.pen", json!({"index":-1})).is_err());
        assert!(s.execute("draw.pen.update", json!({"index":"2"})).is_err());
        assert_eq!(s.prefs.ink_pens[index].color, Color::Rgb(0x123456));
        let prefs: crate::Prefs = serde_json::from_str(&serde_json::to_string(&s.prefs).unwrap()).unwrap();
        assert_eq!(prefs.ink_pens, s.prefs.ink_pens);
        s.execute("draw.pen.delete", json!({"index":index})).unwrap();
        assert_eq!(s.prefs.ink_pens.len(), 6);
    }

    #[test]
    fn deleting_an_earlier_pen_keeps_the_selected_one() {
        let mut s = session();
        s.execute("draw.pen", json!({"index":3,"on":true})).unwrap();
        let selected = s.prefs.ink_pens[3].clone();
        s.execute("draw.pen.delete", json!({"index":0})).unwrap();
        assert_eq!(s.draw.pen, 2);
        assert_eq!(s.prefs.ink_pens[s.draw.pen], selected);
        assert_eq!(s.draw.ink_tool, selected.tool);
    }

    #[test]
    fn pen_effect_parameter_applies() {
        let mut s = session();
        s.execute("draw.pen", json!({"effect":"rainbow","tool":"pen"})).unwrap();
        assert_eq!(s.draw.effect, Some(InkEffect::Rainbow));
        s.execute("draw.stroke", json!({"points":[[20,40],[200,40]]})).unwrap();
        assert_eq!(shapes(&s)[0].ink.as_ref().unwrap().effect, Some(InkEffect::Rainbow));
        s.execute("draw.pen", json!({"effect":""})).unwrap();
        assert_eq!(s.draw.effect, None);
    }

    #[test]
    fn lasso_selects_multiple_strokes_and_eraser_splits_only_real_hits() {
        let mut s = session();
        for y in [20, 40] {
            s.execute("draw.stroke", json!({"points":[[20,y],[40,y],[60,y],[80,y],[100,y]],"width":2})).unwrap();
        }
        s.execute("draw.select", json!({"points":[[0,0],[120,0],[120,60],[0,60]]})).unwrap();
        assert_eq!(s.draw.selection.len(), 2);
        let undo = undo_len(&s);
        assert_eq!(s.execute("draw.erase", json!({"x":60,"y":80,"mode":"stroke"})).unwrap()["erased"], 0);
        assert_eq!(undo_len(&s), undo, "a miss records no undo step");
        s.execute("draw.erase", json!({"x":60,"y":20,"mode":"small"})).unwrap();
        assert_eq!(shapes(&s).len(), 3);
        let original = &s.doc().unwrap().wb;
        let (back, report) = gridcraft_xlsx::read_xlsx(&gridcraft_xlsx::write_xlsx(original).unwrap()).unwrap();
        assert!(report.warnings.is_empty());
        let (first, second) = (original.active().unwrap(), back.active().unwrap());
        for (a, b) in first.shapes.iter().zip(&second.shapes) {
            assert_eq!(a.ink.as_ref().unwrap().points, b.ink.as_ref().unwrap().points);
            assert_eq!(anchor_origin(first, &a.anchor), anchor_origin(second, &b.anchor));
        }
        s.execute("edit.undo", json!({})).unwrap();
        assert_eq!(shapes(&s).len(), 2);
        s.execute("draw.selection.delete", json!({})).unwrap();
        assert!(shapes(&s).is_empty());
        let undo = undo_len(&s);
        assert!(s.execute("draw.selection.delete", json!({})).is_err());
        assert_eq!(undo_len(&s), undo);
    }

    #[test]
    fn segment_eraser_cuts_between_crossings() {
        let mut s = session();
        // A horizontal stroke crossed by two verticals at x=50 and x=150.
        s.execute("draw.stroke", json!({"points":[[10,100],[100,100],[190,100]],"smooth":false})).unwrap();
        for x in [50, 150] {
            s.execute("draw.stroke", json!({"points":[[x,50],[x,150]],"smooth":false})).unwrap();
        }
        s.execute("draw.erase", json!({"x":100,"y":100,"mode":"segment"})).unwrap();
        let sh = s.doc().unwrap().wb.active().unwrap().clone();
        let mut spans: Vec<(f32, f32)> = sh
            .shapes
            .iter()
            .filter(|x| x.anchor.height <= 1.0)
            .map(|x| {
                let o = anchor_origin(&sh, &x.anchor);
                (o[0], o[0] + x.anchor.width)
            })
            .collect();
        spans.sort_by(|a, b| a.0.total_cmp(&b.0));
        assert_eq!(spans.len(), 2, "{spans:?}");
        assert!((spans[0].0 - 10.0).abs() < 0.01 && (spans[0].1 - 50.0).abs() < 0.01, "{spans:?}");
        assert!((spans[1].0 - 150.0).abs() < 0.01 && (spans[1].1 - 190.0).abs() < 0.01, "{spans:?}");
        // With no crossings the whole stroke goes.
        let mut s = session();
        s.execute("draw.stroke", json!({"points":[[10,100],[190,100]]})).unwrap();
        s.execute("draw.erase", json!({"x":100,"y":100,"mode":"segment"})).unwrap();
        assert!(shapes(&s).is_empty());
    }

    #[test]
    fn lasso_rejects_partial_and_concave_crossing_strokes() {
        let mut s = session();
        s.execute("draw.stroke", json!({"points":[[20,40],[100,40]],"width":2})).unwrap();
        for polygon in
            [json!([[0, 0], [50, 0], [50, 80], [0, 80]]), json!([[0, 0], [120, 0], [120, 80], [80, 80], [80, 20], [40, 20], [40, 80], [0, 80]])]
        {
            s.execute("draw.select", json!({"points":polygon})).unwrap();
            assert!(s.draw.selection.is_empty());
        }
        s.execute("draw.select", json!({"points":[[0,0],[120,0],[120,80],[0,80]]})).unwrap();
        assert_eq!(s.draw.selection.len(), 1);
    }

    #[test]
    fn long_lassos_are_thinned_not_cut_short() {
        let mut s = session();
        s.execute("draw.stroke", json!({"points":[[400,40],[420,40]]})).unwrap();
        // A 6000-point loop whose far side is past the 5000th point.
        let mut ring = Vec::new();
        for i in 0..6000 {
            let a = i as f64 / 6000.0 * std::f64::consts::TAU;
            ring.push(json!([410.0 + 100.0 * a.cos(), 40.0 + 30.0 * a.sin()]));
        }
        s.execute("draw.select", json!({"points":ring})).unwrap();
        assert_eq!(s.draw.selection.len(), 1);
    }

    #[test]
    fn selection_moves_by_offset() {
        let mut s = session();
        s.execute("draw.stroke", json!({"points":[[20,40],[100,40]]})).unwrap();
        s.execute("draw.select", json!({"points":[[0,0],[120,0],[120,80],[0,80]]})).unwrap();
        s.execute("draw.selection.move", json!({"dx":30,"dy":-100})).unwrap();
        let sh = s.doc().unwrap().wb.active().unwrap().clone();
        let o = anchor_origin(&sh, &sh.shapes[0].anchor);
        assert!((o[0] - 49.0).abs() < 1.5 && o[1] == 0.0, "{o:?}");
        s.execute("draw.exit", json!({})).unwrap();
        assert!(s.execute("draw.selection.move", json!({"dx":1,"dy":1})).is_err());
    }

    #[test]
    fn effect_phase_survives_repeated_erasures() {
        let mut s = session();
        s.execute("draw.pen", json!({"effect":"rainbow"})).unwrap();
        let pts: Vec<_> = (1..=10).map(|i| json!([i * 20, 40])).collect();
        s.execute("draw.stroke", json!({"points":pts,"width":2})).unwrap();
        let sh = s.doc().unwrap().wb.active().unwrap().clone();
        let original = sh.shapes[0].clone();
        let o0 = anchor_origin(&sh, &original.anchor);
        let theme = Theme::default();
        for (x, mode) in [(80, "small"), (160, "medium")] {
            s.execute("draw.erase", json!({"x":x,"y":40,"mode":mode})).unwrap();
            let sh = s.doc().unwrap().wb.active().unwrap();
            for fragment in &sh.shapes {
                let o = anchor_origin(sh, &fragment.anchor);
                for q in &fragment.ink.as_ref().unwrap().points {
                    let in_original = [q[0] + o[0] - o0[0], q[1] + o[1] - o0[1]];
                    assert_eq!(fragment.ink_color_at(*q, &theme), original.ink_color_at(in_original, &theme));
                }
            }
        }
    }
}
