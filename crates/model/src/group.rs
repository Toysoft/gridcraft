//! Grouped drawing objects (Excel's Group, `<xdr:grpSp>` in a file). A group lists its members
//! back to front; the members stay ordinary charts, pictures and shapes in their sheet's lists,
//! each with its own resolved anchor, so they draw exactly as ungrouped objects do. Moving,
//! resizing or deleting a member acts on its outermost group.

use gridcraft_core::CellRef;
use serde::{Deserialize, Serialize};

use crate::{Anchor, AnchorMode, Sheet};

/// Groups nest at most this deep; deeper references are ignored (and a cycle in hand-written
/// JSON can't recurse forever).
pub const MAX_GROUP_DEPTH: usize = 64;

/// A floating object by kind and id: what a group holds.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum DrawingRef {
    Chart(u32),
    Image(u32),
    Shape(u32),
    Group(u32),
}

/// A group of floating objects.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct DrawingGroup {
    pub id: u32,
    /// The group's name (`cNvPr name`, e.g. "Group 3").
    pub name: String,
    /// The group's box on the sheet when it was laid out (opened, or last moved or sized as a
    /// whole). With `child_offset`/`child_extent` it fixes the group's child coordinate space;
    /// its current box is [`Sheet::drawing_rect`] of the group, the box around its members.
    pub anchor: Anchor,
    /// The child box (`a:chOff`/`a:chExt`, EMUs) that `anchor`'s box maps onto.
    pub child_offset: (i64, i64),
    pub child_extent: (i64, i64),
    /// Members, back to front (nested groups by [`DrawingRef::Group`]).
    pub members: Vec<DrawingRef>,
}

/// A floating object's box on the sheet, in points from the sheet's top-left corner.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct ObjectRect {
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
}

impl ObjectRect {
    fn union(self, o: ObjectRect) -> ObjectRect {
        let (x, y) = (self.x.min(o.x), self.y.min(o.y));
        ObjectRect { x, y, w: (self.x + self.w).max(o.x + o.w) - x, h: (self.y + self.h).max(o.y + o.h) - y }
    }
}

/// The axis-aligned map taking `from` onto `to` (an axis without extent keeps its scale).
fn map_rect(from: ObjectRect, to: ObjectRect, r: ObjectRect) -> ObjectRect {
    let sx = if from.w > 0.0 { to.w / from.w } else { 1.0 };
    let sy = if from.h > 0.0 { to.h / from.h } else { 1.0 };
    ObjectRect { x: to.x + (r.x - from.x) * sx, y: to.y + (r.y - from.y) * sy, w: r.w * sx, h: r.h * sy }
}

impl Sheet {
    /// An anchor's box on the sheet.
    pub fn anchor_rect(&self, a: &Anchor) -> ObjectRect {
        ObjectRect { x: self.col_left(a.cell.col) + a.dx as f64, y: self.row_top(a.cell.row) + a.dy as f64, w: a.width as f64, h: a.height as f64 }
    }

    /// The anchor (top-left cell plus offset, clamped onto the sheet) for a box, with `mode`.
    pub fn rect_anchor(&self, r: ObjectRect, mode: AnchorMode) -> Anchor {
        let px = |v: f64| if v.is_finite() { v.clamp(0.0, 1e7) } else { 0.0 };
        let (x, y) = (px(r.x), px(r.y));
        let col = self.col_at(x);
        let row = self.row_at(y);
        Anchor {
            cell: CellRef::new(row, col),
            dx: (x - self.col_left(col)).max(0.0) as f32,
            dy: (y - self.row_top(row)).max(0.0) as f32,
            width: px(r.w) as f32,
            height: px(r.h) as f32,
            mode,
        }
    }

    pub fn drawing_group(&self, id: u32) -> Option<&DrawingGroup> {
        self.groups.iter().find(|g| g.id == id)
    }

    /// The anchor of a chart, picture or shape (`None` for a group or a missing object).
    pub fn object_anchor(&self, r: DrawingRef) -> Option<Anchor> {
        match r {
            DrawingRef::Chart(id) => self.charts.iter().find(|o| o.id == id).map(|o| o.anchor),
            DrawingRef::Image(id) => self.images.iter().find(|o| o.id == id).map(|o| o.anchor),
            DrawingRef::Shape(id) => self.shapes.iter().find(|o| o.id == id).map(|o| o.anchor),
            DrawingRef::Group(_) => None,
        }
    }

    /// An object's box; a group's is the box around its members. `None` when the object is
    /// missing or a group holds none.
    pub fn drawing_rect(&self, r: DrawingRef) -> Option<ObjectRect> {
        self.drawing_rect_with(r, &|o| self.object_anchor(o).map(|a| self.anchor_rect(&a)))
    }

    /// [`Sheet::drawing_rect`] with the boxes of charts, pictures and shapes from `leaf` (a
    /// writer skipping some objects measures groups by the ones it keeps).
    pub fn drawing_rect_with(&self, r: DrawingRef, leaf: &dyn Fn(DrawingRef) -> Option<ObjectRect>) -> Option<ObjectRect> {
        self.rect_at_depth(r, leaf, 0)
    }

    fn rect_at_depth(&self, r: DrawingRef, leaf: &dyn Fn(DrawingRef) -> Option<ObjectRect>, depth: usize) -> Option<ObjectRect> {
        let DrawingRef::Group(id) = r else { return leaf(r) };
        if depth >= MAX_GROUP_DEPTH {
            return None;
        }
        let g = self.drawing_group(id)?;
        g.members.iter().filter_map(|m| self.rect_at_depth(*m, leaf, depth + 1)).reduce(ObjectRect::union)
    }

    /// A group's anchor: the box around its members, in the group's anchor mode.
    pub fn group_anchor(&self, id: u32) -> Option<Anchor> {
        let g = self.drawing_group(id)?;
        self.drawing_rect(DrawingRef::Group(id)).map(|r| self.rect_anchor(r, g.anchor.mode))
    }

    /// The group directly holding `r`.
    pub fn parent_group(&self, r: DrawingRef) -> Option<u32> {
        self.groups.iter().find(|g| g.members.contains(&r)).map(|g| g.id)
    }

    /// The outermost group holding `r` (`None` when `r` isn't grouped).
    pub fn outer_group(&self, r: DrawingRef) -> Option<u32> {
        let mut cur = self.parent_group(r)?;
        for _ in 0..MAX_GROUP_DEPTH {
            match self.parent_group(DrawingRef::Group(cur)) {
                Some(p) if p != cur => cur = p,
                _ => break,
            }
        }
        Some(cur)
    }

    /// Everything inside group `id`, nested groups included (not `id` itself).
    pub fn group_contents(&self, id: u32) -> Vec<DrawingRef> {
        let mut out = Vec::new();
        let mut stack = vec![(id, 0usize)];
        while let Some((gid, depth)) = stack.pop() {
            let Some(g) = self.drawing_group(gid) else { continue };
            for m in &g.members {
                if out.contains(m) {
                    continue;
                }
                out.push(*m);
                if let DrawingRef::Group(n) = m
                    && depth + 1 < MAX_GROUP_DEPTH
                {
                    stack.push((*n, depth + 1));
                }
            }
        }
        out
    }

    /// Moves and sizes group `id` with everything in it by the map taking `from` onto `to`.
    pub fn transform_group(&mut self, id: u32, from: ObjectRect, to: ObjectRect) {
        let mut groups = vec![id];
        for r in self.group_contents(id) {
            match r {
                DrawingRef::Group(g) if !groups.contains(&g) => groups.push(g),
                DrawingRef::Group(_) => {}
                leaf => {
                    let Some(a) = self.object_anchor(leaf) else { continue };
                    let na = self.rect_anchor(map_rect(from, to, self.anchor_rect(&a)), a.mode);
                    if let Some(slot) = self.object_anchor_mut(leaf) {
                        *slot = na;
                    }
                }
            }
        }
        for gid in groups {
            let Some(a) = self.drawing_group(gid).map(|g| g.anchor) else { continue };
            let na = self.rect_anchor(map_rect(from, to, self.anchor_rect(&a)), a.mode);
            if let Some(g) = self.groups.iter_mut().find(|g| g.id == gid) {
                g.anchor = na;
            }
        }
    }

    /// Sets the anchor mode of group `id`, everything in it and the group itself.
    pub fn set_group_anchor_mode(&mut self, id: u32, mode: AnchorMode) {
        let contents = self.group_contents(id);
        for r in &contents {
            if let Some(a) = self.object_anchor_mut(*r) {
                a.mode = mode;
            }
        }
        for g in self.groups.iter_mut().filter(|g| g.id == id || contents.contains(&DrawingRef::Group(g.id))) {
            g.anchor.mode = mode;
        }
    }

    /// Deletes group `id` with everything in it.
    pub fn delete_group(&mut self, id: u32) {
        let contents = self.group_contents(id);
        let has = |r: DrawingRef| contents.contains(&r);
        self.charts.retain(|o| !has(DrawingRef::Chart(o.id)));
        self.images.retain(|o| !has(DrawingRef::Image(o.id)));
        self.shapes.retain(|o| !has(DrawingRef::Shape(o.id)));
        self.groups.retain(|g| g.id != id && !has(DrawingRef::Group(g.id)));
        for g in &mut self.groups {
            g.members.retain(|m| *m != DrawingRef::Group(id));
        }
    }

    fn object_anchor_mut(&mut self, r: DrawingRef) -> Option<&mut Anchor> {
        match r {
            DrawingRef::Chart(id) => self.charts.iter_mut().find(|o| o.id == id).map(|o| &mut o.anchor),
            DrawingRef::Image(id) => self.images.iter_mut().find(|o| o.id == id).map(|o| &mut o.anchor),
            DrawingRef::Shape(id) => self.shapes.iter_mut().find(|o| o.id == id).map(|o| &mut o.anchor),
            DrawingRef::Group(_) => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Color, Shape, ShapeFormat, ShapeKind};

    fn shape(id: u32, x: f64, y: f64, sh: &Sheet) -> Shape {
        let anchor = sh.rect_anchor(ObjectRect { x, y, w: 20.0, h: 10.0 }, AnchorMode::MoveOnly);
        Shape { id, kind: ShapeKind::Rectangle, anchor, fill: Color::Auto, line: Color::Auto, text: String::new(), format: ShapeFormat::default() }
    }

    fn sheet_with_nested_group() -> Sheet {
        let mut sh = Sheet::new("S");
        let a = shape(1, 100.0, 50.0, &sh);
        let b = shape(2, 200.0, 90.0, &sh);
        let c = shape(3, 150.0, 70.0, &sh);
        sh.shapes = vec![a, b, c];
        let anchor = sh.rect_anchor(ObjectRect { x: 150.0, y: 70.0, w: 70.0, h: 30.0 }, AnchorMode::MoveOnly);
        sh.groups.push(DrawingGroup {
            id: 5,
            name: "Inner".into(),
            anchor,
            child_offset: (0, 0),
            child_extent: (1, 1),
            members: vec![DrawingRef::Shape(3), DrawingRef::Shape(2)],
        });
        let anchor = sh.rect_anchor(ObjectRect { x: 100.0, y: 50.0, w: 120.0, h: 50.0 }, AnchorMode::MoveOnly);
        sh.groups.push(DrawingGroup {
            id: 4,
            name: "Outer".into(),
            anchor,
            child_offset: (0, 0),
            child_extent: (1, 1),
            members: vec![DrawingRef::Shape(1), DrawingRef::Group(5)],
        });
        sh
    }

    #[test]
    fn groups_measure_move_and_delete_with_their_members() {
        let mut sh = sheet_with_nested_group();
        assert_eq!(sh.outer_group(DrawingRef::Shape(2)), Some(4));
        assert_eq!(sh.outer_group(DrawingRef::Group(5)), Some(4));
        assert_eq!(sh.outer_group(DrawingRef::Shape(9)), None);
        let r = sh.drawing_rect(DrawingRef::Group(4)).unwrap_or_default();
        assert_eq!((r.x, r.y, r.w, r.h), (100.0, 50.0, 120.0, 50.0));
        // Moving the group by (+30, +5) moves every member and both groups' boxes.
        sh.transform_group(4, r, ObjectRect { x: 130.0, y: 55.0, ..r });
        let moved = |sh: &Sheet, id| sh.drawing_rect(DrawingRef::Shape(id)).map(|r| (r.x, r.y));
        assert_eq!((moved(&sh, 1), moved(&sh, 2), moved(&sh, 3)), (Some((130.0, 55.0)), Some((230.0, 95.0)), Some((180.0, 75.0))));
        let inner = sh.drawing_group(5).map(|g| sh.anchor_rect(&g.anchor)).unwrap_or_default();
        assert_eq!((inner.x, inner.y), (180.0, 75.0));
        assert_eq!(sh.shapes.iter().map(|s| s.anchor.mode).collect::<Vec<_>>(), vec![AnchorMode::MoveOnly; 3]);
        sh.delete_group(4);
        assert!(sh.shapes.is_empty() && sh.groups.is_empty());
    }

    #[test]
    fn a_group_cycle_does_not_recurse_forever() {
        let mut sh = sheet_with_nested_group();
        if let Some(g) = sh.groups.iter_mut().find(|g| g.id == 5) {
            g.members.push(DrawingRef::Group(4));
        }
        assert!(sh.outer_group(DrawingRef::Shape(1)).is_some());
        assert!(sh.drawing_rect(DrawingRef::Group(4)).is_some());
        assert_eq!(sh.group_contents(4).len(), 5);
        sh.delete_group(4);
        assert!(sh.shapes.is_empty() && sh.groups.is_empty());
    }
}
