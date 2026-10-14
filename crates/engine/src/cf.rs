//! Conditional formatting evaluation: what extra style, data bar, colour scale or icon a cell
//! gets.

use std::collections::HashMap;

use gridcraft_core::{CellRef, RangeRef, Value, compare};
use gridcraft_model::*;

/// The conditional look of one cell.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct CfLook {
    /// Differential style (only non-default parts apply).
    pub style: Option<Style>,
    /// Fill from a colour scale (RGB).
    pub scale_fill: Option<[u8; 3]>,
    /// Data bar: fraction 0..=1 of the cell width, colour, gradient.
    pub bar: Option<(f32, [u8; 3], bool)>,
    /// Icon: set name and index (0 = lowest).
    pub icon: Option<(String, usize)>,
    pub hide_value: bool,
}

/// Precomputed statistics per rule (min/max/percentiles/top-N thresholds/duplicates).
#[derive(Default)]
pub struct CfCache {
    stats: HashMap<usize, RuleStats>,
}

#[derive(Default, Clone)]
struct RuleStats {
    sorted: Vec<f64>,
    avg: f64,
    std: f64,
    counts: HashMap<String, usize>,
}

fn numbers_in(sh: &Sheet, ranges: &[RangeRef]) -> (Vec<f64>, HashMap<String, usize>) {
    let mut nums = Vec::new();
    let mut counts = HashMap::new();
    for r in ranges {
        let r = match sh.used_range().and_then(|u| r.intersection(&u)) {
            Some(x) => x,
            None => continue,
        };
        for c in r.iter().take(2_000_000) {
            let v = sh.value(c);
            if let Value::Number(n) = v {
                nums.push(n);
            }
            if !v.is_empty() {
                *counts.entry(v.display().to_lowercase()).or_insert(0) += 1;
            }
        }
    }
    nums.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    (nums, counts)
}

fn percentile(sorted: &[f64], p: f64) -> f64 {
    if sorted.is_empty() {
        return 0.0;
    }
    let rank = (p / 100.0).clamp(0.0, 1.0) * (sorted.len() - 1) as f64;
    let lo = rank.floor() as usize;
    let hi = rank.ceil() as usize;
    let a = sorted.get(lo).copied().unwrap_or(0.0);
    let b = sorted.get(hi).copied().unwrap_or(a);
    a + (b - a) * (rank - lo as f64)
}

fn threshold(wb: &Workbook, sheet: usize, at: CellRef, st: &RuleStats, k: &CfValueKind) -> f64 {
    let min = st.sorted.first().copied().unwrap_or(0.0);
    let max = st.sorted.last().copied().unwrap_or(0.0);
    match k {
        CfValueKind::Min => min,
        CfValueKind::Max => max,
        CfValueKind::Number(n) => *n,
        CfValueKind::Percent(p) => min + (max - min) * p / 100.0,
        CfValueKind::Percentile(p) => percentile(&st.sorted, *p),
        CfValueKind::Formula(f) => gridcraft_calc::evaluate(wb, sheet, at, f).to_number().unwrap_or(0.0),
    }
}

fn lerp(a: [u8; 3], b: [u8; 3], t: f64) -> [u8; 3] {
    let t = t.clamp(0.0, 1.0);
    [
        (a[0] as f64 + (b[0] as f64 - a[0] as f64) * t).round() as u8,
        (a[1] as f64 + (b[1] as f64 - a[1] as f64) * t).round() as u8,
        (a[2] as f64 + (b[2] as f64 - a[2] as f64) * t).round() as u8,
    ]
}

pub use gridcraft_model::icon_count;

impl CfCache {
    pub fn new() -> Self {
        Self::default()
    }

    /// Conditional look of `c`, or `None` when no rule applies.
    pub fn look(&mut self, wb: &Workbook, sheet: usize, c: CellRef) -> Option<CfLook> {
        let sh = wb.sheet(sheet)?;
        if sh.cond_formats.is_empty() {
            return None;
        }
        let mut out = CfLook::default();
        let mut any = false;
        let mut rules: Vec<(usize, &CondFormat)> =
            sh.cond_formats.iter().enumerate().filter(|(_, cf)| cf.ranges.iter().any(|r| r.contains(c))).collect();
        rules.sort_by_key(|(_, cf)| cf.priority);
        let v = sh.value(c);
        for (idx, cf) in rules {
            let first = cf.ranges.first().map(|r| r.start).unwrap_or(c);
            let st = self
                .stats
                .entry(idx)
                .or_insert_with(|| {
                    let (sorted, counts) = numbers_in(sh, &cf.ranges);
                    let n = sorted.len().max(1) as f64;
                    let avg = sorted.iter().sum::<f64>() / n;
                    let std = (sorted.iter().map(|x| (x - avg).powi(2)).sum::<f64>() / n).sqrt();
                    RuleStats { sorted, avg, std, counts }
                })
                .clone();
            let text = v.display();
            let matched_style: Option<&Style> = match &cf.rule {
                CfRule::CellIs { op, a, b, style } => {
                    let eval = |f: &str| gridcraft_calc::evaluate(wb, sheet, c, &relative(f, first, c));
                    let av = eval(a);
                    let ok = match op {
                        CfOperator::Between | CfOperator::NotBetween => {
                            let bv = b.as_deref().map(eval).unwrap_or_default();
                            let (lo, hi) = if compare(&av, &bv) == std::cmp::Ordering::Greater { (bv, av) } else { (av, bv) };
                            let inside = compare(&v, &lo) != std::cmp::Ordering::Less && compare(&v, &hi) != std::cmp::Ordering::Greater;
                            if *op == CfOperator::Between { inside } else { !inside }
                        }
                        CfOperator::Equal => compare(&v, &av) == std::cmp::Ordering::Equal,
                        CfOperator::NotEqual => compare(&v, &av) != std::cmp::Ordering::Equal,
                        CfOperator::Greater => compare(&v, &av) == std::cmp::Ordering::Greater,
                        CfOperator::Less => compare(&v, &av) == std::cmp::Ordering::Less,
                        CfOperator::GreaterOrEqual => compare(&v, &av) != std::cmp::Ordering::Less,
                        CfOperator::LessOrEqual => compare(&v, &av) != std::cmp::Ordering::Greater,
                    };
                    // As in Excel, a blank cell compares as 0 (or ""), and an error never matches.
                    (ok && !v.is_error()).then_some(style.as_ref())
                }
                CfRule::Expression { formula, style } => {
                    gridcraft_calc::evaluate(wb, sheet, c, &relative(formula, first, c)).to_bool().unwrap_or(false).then_some(style.as_ref())
                }
                CfRule::ContainsText { text: t, style } => text.to_lowercase().contains(&t.to_lowercase()).then_some(style.as_ref()),
                CfRule::NotContainsText { text: t, style } => (!text.to_lowercase().contains(&t.to_lowercase())).then_some(style.as_ref()),
                CfRule::BeginsWith { text: t, style } => text.to_lowercase().starts_with(&t.to_lowercase()).then_some(style.as_ref()),
                CfRule::EndsWith { text: t, style } => text.to_lowercase().ends_with(&t.to_lowercase()).then_some(style.as_ref()),
                CfRule::Blanks { style } => (v.is_empty() || text.trim().is_empty()).then_some(style.as_ref()),
                CfRule::NoBlanks { style } => (!v.is_empty() && !text.trim().is_empty()).then_some(style.as_ref()),
                CfRule::Errors { style } => v.is_error().then_some(style.as_ref()),
                CfRule::NoErrors { style } => (!v.is_error()).then_some(style.as_ref()),
                CfRule::Duplicate { style } => {
                    (!v.is_empty() && st.counts.get(&text.to_lowercase()).copied().unwrap_or(0) > 1).then_some(style.as_ref())
                }
                CfRule::Unique { style } => {
                    (!v.is_empty() && st.counts.get(&text.to_lowercase()).copied().unwrap_or(0) == 1).then_some(style.as_ref())
                }
                CfRule::Top10 { bottom, percent, rank, style } => match v.as_f64() {
                    Some(n) if !st.sorted.is_empty() => {
                        let k = if *percent { ((st.sorted.len() as f64) * (*rank as f64) / 100.0).floor().max(1.0) as usize } else { *rank as usize };
                        let k = k.clamp(1, st.sorted.len());
                        let ok = if *bottom {
                            n <= st.sorted.get(k - 1).copied().unwrap_or(n)
                        } else {
                            n >= st.sorted.get(st.sorted.len() - k).copied().unwrap_or(n)
                        };
                        ok.then_some(style.as_ref())
                    }
                    _ => None,
                },
                CfRule::AboveAverage { below, equal, std_dev, style } => match v.as_f64() {
                    Some(n) => {
                        let thr = st.avg + if *below { -1.0 } else { 1.0 } * st.std * *std_dev as f64;
                        let ok = if *below { n < thr || (*equal && n == thr) } else { n > thr || (*equal && n == thr) };
                        ok.then_some(style.as_ref())
                    }
                    None => None,
                },
                CfRule::TimePeriod { period, style } => match v.as_f64() {
                    Some(n) => {
                        let today = gridcraft_core::date::serial_in_system(wb.date_system, gridcraft_calc::now_serial()).floor();
                        let d = n.floor();
                        let wd = gridcraft_core::date::datetime_from_serial(wb.date_system, today).map(|x| x.weekday as f64).unwrap_or(0.0);
                        let week_start = today - wd;
                        let (ty, tm) =
                            gridcraft_core::date::datetime_from_serial(wb.date_system, today).map(|x| (x.year, x.month)).unwrap_or((2026, 1));
                        let ym = gridcraft_core::date::datetime_from_serial(wb.date_system, d).map(|x| (x.year, x.month));
                        let month_off = |k: i32| {
                            let m = tm as i32 + k;
                            let y = ty + (m - 1).div_euclid(12);
                            let m = (m - 1).rem_euclid(12) + 1;
                            ym == Some((y, m as u32))
                        };
                        let ok = match period.as_str() {
                            "yesterday" => d == today - 1.0,
                            "today" => d == today,
                            "tomorrow" => d == today + 1.0,
                            "last7Days" => d > today - 7.0 && d <= today,
                            "lastWeek" => d >= week_start - 7.0 && d < week_start,
                            "thisWeek" => d >= week_start && d < week_start + 7.0,
                            "nextWeek" => d >= week_start + 7.0 && d < week_start + 14.0,
                            "lastMonth" => month_off(-1),
                            "thisMonth" => month_off(0),
                            "nextMonth" => month_off(1),
                            _ => false,
                        };
                        ok.then_some(style.as_ref())
                    }
                    None => None,
                },
                CfRule::ColorScale { stops } => {
                    if let Some(n) = v.as_f64() {
                        let pts: Vec<(f64, [u8; 3])> = stops
                            .iter()
                            .map(|(k, col)| (threshold(wb, sheet, c, &st, k), col.resolve(&wb.theme).unwrap_or([255, 255, 255])))
                            .collect();
                        let rgb = match pts.as_slice() {
                            [(a, ca), (b, cb)] => lerp(*ca, *cb, if b > a { (n - a) / (b - a) } else { 0.0 }),
                            [(a, ca), (m, cm), (b, cb)] => {
                                if n <= *m {
                                    lerp(*ca, *cm, if m > a { (n - a) / (m - a) } else { 0.0 })
                                } else {
                                    lerp(*cm, *cb, if b > m { (n - m) / (b - m) } else { 1.0 })
                                }
                            }
                            _ => [255, 255, 255],
                        };
                        if out.scale_fill.is_none() {
                            out.scale_fill = Some(rgb);
                            any = true;
                        }
                    }
                    None
                }
                CfRule::DataBar { min, max, color, gradient, show_value } => {
                    if let Some(n) = v.as_f64() {
                        let lo = threshold(wb, sheet, c, &st, min);
                        let hi = threshold(wb, sheet, c, &st, max);
                        let lo = if lo > 0.0 { 0.0 } else { lo };
                        let frac = if hi > lo { ((n - lo) / (hi - lo)).clamp(0.0, 1.0) } else { 1.0 };
                        if out.bar.is_none() {
                            out.bar = Some((
                                (frac as f32) * 0.9 + 0.1 * (frac > 0.0) as u8 as f32,
                                color.resolve(&wb.theme).unwrap_or([0x63, 0x8E, 0xC6]),
                                *gradient,
                            ));
                            out.hide_value |= !show_value;
                            any = true;
                        }
                    }
                    None
                }
                CfRule::IconSet { set, thresholds, reverse, show_value } => {
                    if let Some(n) = v.as_f64() {
                        let k = icon_count(set);
                        // One threshold per icon, the first being the lower bound (as in the file);
                        // also accept the lower bound left out, else Excel's defaults.
                        let defaults;
                        let lower = match thresholds.len() {
                            n if n == k => thresholds.get(1..).unwrap_or_default(),
                            n if n + 1 == k => thresholds.as_slice(),
                            _ => {
                                defaults = default_icon_thresholds(set);
                                defaults.get(1..).unwrap_or_default()
                            }
                        };
                        let cuts: Vec<f64> = lower.iter().map(|t| threshold(wb, sheet, c, &st, t)).collect();
                        let mut idx = cuts.iter().filter(|cut| n >= **cut).count();
                        if *reverse {
                            idx = k - 1 - idx.min(k - 1);
                        }
                        if out.icon.is_none() {
                            out.icon = Some((set.clone(), idx.min(k - 1)));
                            out.hide_value |= !show_value;
                            any = true;
                        }
                    }
                    None
                }
            };
            if let Some(st) = matched_style {
                any = true;
                // Every matching rule applies; where two set the same part, the rule with the
                // higher priority (seen first) wins.
                match &mut out.style {
                    None => out.style = Some(st.clone()),
                    Some(prev) => layer_under(prev, st),
                }
                if cf.stop_if_true {
                    break;
                }
            }
        }
        any.then_some(out)
    }
}

/// Adds the parts `lower` sets that `style` leaves unset (differential styles: default parts
/// are unset).
fn layer_under(style: &mut Style, lower: &Style) {
    if style.fill.pattern == PatternType::None {
        style.fill = lower.fill;
    }
    if style.font.color == Color::Auto {
        style.font.color = lower.font.color;
    }
    style.font.bold |= lower.font.bold;
    style.font.italic |= lower.font.italic;
    style.font.strike |= lower.font.strike;
    if style.font.underline == Underline::None {
        style.font.underline = lower.font.underline;
    }
    let b = &mut style.borders;
    for (dst, src) in [
        (&mut b.left, lower.borders.left),
        (&mut b.right, lower.borders.right),
        (&mut b.top, lower.borders.top),
        (&mut b.bottom, lower.borders.bottom),
        (&mut b.diag_down, lower.borders.diag_down),
        (&mut b.diag_up, lower.borders.diag_up),
    ] {
        if dst.is_none() {
            *dst = src;
        }
    }
    if style.num_fmt.as_str() == "General" {
        style.num_fmt = lower.num_fmt.clone();
    }
}

/// Shifts a rule formula written for the top-left cell of the range to `c`.
fn relative(f: &str, first: CellRef, c: CellRef) -> String {
    if first == c {
        return f.to_string();
    }
    match gridcraft_formula::parse(f.trim_start_matches('=')) {
        Ok(e) => {
            gridcraft_formula::print(&gridcraft_formula::adjust::shift_relative(e, c.row as i64 - first.row as i64, c.col as i64 - first.col as i64))
        }
        Err(_) => f.to_string(),
    }
}

// ---------------------------------------------------------------- rules follow their cells
//
// In Excel a conditional format is part of the cells' formatting: it travels with copied, cut and
// filled cells, and Clear Formats removes it (measured in Excel for Microsoft 365, issue #1164).

/// A rule's formulas, rewritten for a first cell `dr` rows and `dc` columns away (they are
/// written for the top-left cell of the rule's first range).
fn shift_formulas(rule: &mut CfRule, dr: i64, dc: i64) {
    if dr == 0 && dc == 0 {
        return;
    }
    let shift = |f: &mut String| {
        if let Ok(e) = gridcraft_formula::parse(f.trim_start_matches('=')) {
            *f = gridcraft_formula::print(&gridcraft_formula::adjust::shift_relative(e, dr, dc));
        }
    };
    let shift_kind = |k: &mut CfValueKind| {
        if let CfValueKind::Formula(f) = k {
            shift(f);
        }
    };
    match rule {
        CfRule::CellIs { a, b, .. } => {
            shift(a);
            if let Some(b) = b {
                shift(b);
            }
        }
        CfRule::Expression { formula, .. } => shift(formula),
        CfRule::ColorScale { stops } => stops.iter_mut().for_each(|(k, _)| shift_kind(k)),
        CfRule::DataBar { min, max, .. } => {
            shift_kind(min);
            shift_kind(max);
        }
        CfRule::IconSet { thresholds, .. } => thresholds.iter_mut().for_each(shift_kind),
        _ => {}
    }
}

/// The bounding rectangle of `a` and `b` when it holds no other cell.
fn rect_union(a: &RangeRef, b: &RangeRef) -> Option<RangeRef> {
    let u = a.union(b);
    let both = a.intersection(b).map_or(0, |i| i.count());
    (u.count() == a.count() + b.count() - both).then_some(u)
}

/// Joins a rule's ranges whose union is a rectangle, as Excel does (`A1:A2,A5:A10` and `A3:A4`
/// become `A1:A10`). Long lists, such as cell-by-cell rules from a file, are left as they are.
fn coalesce(ranges: &mut Vec<RangeRef>) {
    if ranges.len() > 256 {
        return;
    }
    'again: loop {
        for i in 0..ranges.len() {
            for j in i + 1..ranges.len() {
                if let (Some(a), Some(b)) = (ranges.get(i), ranges.get(j))
                    && let Some(u) = rect_union(a, b)
                {
                    if let Some(a) = ranges.get_mut(i) {
                        *a = u;
                    }
                    ranges.remove(j);
                    continue 'again;
                }
            }
        }
        return;
    }
}

/// A sheet's rules while cells are cleared, pasted or filled: each rule's first cell before, and
/// whether its ranges changed.
struct Rules<'a> {
    cfs: &'a mut Vec<CondFormat>,
    first: Vec<Option<CellRef>>,
    touched: Vec<bool>,
}

impl<'a> Rules<'a> {
    fn new(cfs: &'a mut Vec<CondFormat>) -> Self {
        let first = cfs.iter().map(|cf| cf.ranges.first().map(|r| r.start)).collect();
        let touched = vec![false; cfs.len()];
        Rules { cfs, first, touched }
    }

    /// Takes the cells of `cut` out of every rule.
    fn subtract(&mut self, cut: &RangeRef) {
        for (cf, touched) in self.cfs.iter_mut().zip(self.touched.iter_mut()) {
            if cf.ranges.iter().any(|r| r.intersects(cut)) {
                cf.ranges = cf.ranges.iter().flat_map(|r| crate::cmd::data::subtract_range(*r, cut)).collect();
                *touched = true;
            }
        }
    }

    /// Adds cells to rule `i`.
    fn extend(&mut self, i: usize, cells: Vec<RangeRef>) {
        if let (Some(cf), Some(touched)) = (self.cfs.get_mut(i), self.touched.get_mut(i)) {
            cf.ranges.extend(cells);
            *touched = true;
        }
    }

    /// Joins the changed rules' ranges, rewrites the formulas of a rule whose first cell moved,
    /// drops rules left without cells and puts `new` on top, in their order.
    fn finish(self, new: Vec<CondFormat>) {
        for ((cf, first), touched) in self.cfs.iter_mut().zip(&self.first).zip(&self.touched) {
            if !touched {
                continue;
            }
            coalesce(&mut cf.ranges);
            if let (Some(b), Some(a)) = (first, cf.ranges.first().map(|r| r.start)) {
                shift_formulas(&mut cf.rule, a.row as i64 - b.row as i64, a.col as i64 - b.col as i64);
            }
        }
        self.cfs.retain(|cf| !cf.ranges.is_empty());
        let k = new.len() as u32;
        if k > 0 {
            for cf in self.cfs.iter_mut() {
                cf.priority = cf.priority.saturating_add(k);
            }
            for (i, mut cf) in new.into_iter().enumerate() {
                cf.priority = i as u32 + 1;
                self.cfs.insert(i, cf);
            }
        }
    }
}

/// Removes the cells of `cut` from every rule (Clear Rules from Selected Cells, Clear Formats,
/// Clear All). What is left of a rectangle is listed above, below, left, right, as in Excel;
/// a rule without cells is dropped.
pub fn remove_cells(cfs: &mut Vec<CondFormat>, cut: &RangeRef) {
    let mut rules = Rules::new(cfs);
    rules.subtract(cut);
    rules.finish(Vec::new());
}

/// Whether pasted cells keep the rules they came from (a cut) or copy them.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Carry {
    Copy,
    Move,
}

/// Rules follow cells pasted or painted with Format Painter: `src` are the source sheet's rules,
/// `block` the copied cells and `dest` the cells they land on, the block repeated from its
/// top-left corner (a partial repeat at the far edges). The destination cells lose their own
/// rules first.
///
/// Copied within a sheet (`same_sheet`), the cells join the rule they came from when it stays one
/// line of cells (`A1:A10` copied to `A11` gives `A1:A20`; to `B1`, a second rule), as Excel
/// does; otherwise they get a copy of the rule, on top. A cut keeps them in their rule. On
/// another sheet the caller takes a cut's cells out of the source sheet's rules.
pub fn paste(dst: &mut Vec<CondFormat>, src: &[CondFormat], block: RangeRef, dest: RangeRef, same_sheet: bool, carry: Carry) {
    // Pasting a block onto itself changes nothing.
    if same_sheet && carry == Carry::Copy && dest == block {
        return;
    }
    let (h, w) = (block.height(), block.width());
    // Each repeat of the block: how far it moved, and its cells.
    let (r0, c0) = (i64::from(block.start.row), i64::from(block.start.col));
    let tiles =
        (0..dest.height().div_ceil(h)).flat_map(move |tr| (0..dest.width().div_ceil(w)).map(move |tc| (tr, tc))).filter_map(move |(tr, tc)| {
            let t = dest.start.offset(i64::from(tr) * i64::from(h), i64::from(tc) * i64::from(w))?;
            let area = RangeRef::new(t, t.offset_clamped(i64::from(h) - 1, i64::from(w) - 1)).intersection(&dest)?;
            Some((i64::from(t.row) - r0, i64::from(t.col) - c0, area))
        });
    // What each source rule brings: its cells in the block, on every repeat. A rule on the whole
    // block covers all of `dest`; a partial one is repeated on at most this many tiles.
    const MAX_TILES: usize = 10_000;
    let mut plans: Vec<(Option<usize>, &CondFormat, Vec<RangeRef>)> = Vec::new();
    for s in src.iter().filter(|s| s.ranges.iter().any(|r| r.intersects(&block))) {
        let mut cells = Vec::new();
        for part in s.ranges.iter().filter_map(|r| r.intersection(&block)) {
            if part == block {
                cells.push(dest);
                continue;
            }
            for (dr, dc, area) in tiles.clone().take(MAX_TILES) {
                let moved = RangeRef::new(part.start.offset_clamped(dr, dc), part.end.offset_clamped(dr, dc));
                cells.extend(moved.intersection(&area));
            }
        }
        // The rule the cells came from, if it is still on this sheet as copied.
        let own = if same_sheet { dst.iter().position(|d| d.rule == s.rule && d.ranges == s.ranges) } else { None };
        let join = match carry {
            Carry::Move => own.is_some(),
            Carry::Copy => match (s.ranges.as_slice(), cells.as_slice()) {
                ([r], [c]) => rect_union(r, c).is_some_and(|u| u.height() == 1 || u.width() == 1),
                _ => false,
            },
        };
        plans.push((own.filter(|_| join), s, cells));
    }
    let mut rules = Rules::new(dst);
    if carry == Carry::Move && same_sheet {
        rules.subtract(&block);
    }
    rules.subtract(&dest);
    let mut new = Vec::new();
    for (own, s, cells) in plans {
        if cells.is_empty() {
            continue;
        }
        match own {
            Some(i) => rules.extend(i, cells),
            None => {
                let mut cf = s.clone();
                cf.ranges = cells;
                coalesce(&mut cf.ranges);
                // The formulas were written for the source rule's first cell.
                if let (Some(from), Some(to)) = (s.ranges.first().map(|r| r.start), cf.ranges.first().map(|r| r.start)) {
                    shift_formulas(&mut cf.rule, i64::from(to.row) - i64::from(from.row), i64::from(to.col) - i64::from(from.col));
                }
                new.push(cf);
            }
        }
    }
    rules.finish(new);
}

/// Rules follow a fill (Fill Down, Right, Up, Left; AutoFill with formatting) of `target` from
/// `src`: the filled cells lose their rules and join the rules of the source cells they repeat,
/// which widens those rules, as in Excel.
pub fn fill(cfs: &mut Vec<CondFormat>, src: RangeRef, target: RangeRef, vertical: bool, forward: bool) {
    // Bounds the work for a rule on part of the source repeated over a huge fill.
    const MAX_REPEATS: i64 = 10_000;
    let filled = crate::cmd::data::subtract_range(target, &src);
    let span = i64::from(if vertical { src.height() } else { src.width() });
    let step = if forward { span } else { -span };
    let mut plans: Vec<(usize, Vec<RangeRef>)> = Vec::new();
    for (i, cf) in cfs.iter().enumerate() {
        let mut cells = Vec::new();
        for part in cf.ranges.iter().filter_map(|r| r.intersection(&src)) {
            let whole = if vertical { part.height() == src.height() } else { part.width() == src.width() };
            if whole {
                // Every repeat joins up: the part's lanes over all the filled cells.
                for f in &filled {
                    let lanes = if vertical {
                        RangeRef::new(CellRef::new(f.start.row, part.start.col), CellRef::new(f.end.row, part.end.col))
                    } else {
                        RangeRef::new(CellRef::new(part.start.row, f.start.col), CellRef::new(part.end.row, f.end.col))
                    };
                    cells.extend(lanes.intersection(f));
                }
            } else {
                for k in 1..=MAX_REPEATS {
                    let (dr, dc) = if vertical { (k * step, 0) } else { (0, k * step) };
                    let (Some(a), Some(b)) = (part.start.offset(dr, dc), part.end.offset(dr, dc)) else { break };
                    let Some(x) = RangeRef::new(a, b).intersection(&target) else { break };
                    cells.push(x);
                }
            }
        }
        if !cells.is_empty() {
            plans.push((i, cells));
        }
    }
    let mut rules = Rules::new(cfs);
    for f in &filled {
        rules.subtract(f);
    }
    for (i, cells) in plans {
        rules.extend(i, cells);
    }
    rules.finish(Vec::new());
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn every_matching_rule_applies() {
        let mut s = crate::Session::new();
        s.new_workbook();
        s.execute("range.setValues", json!({"range": "A1", "values": [[200], [400], [510]]})).unwrap();
        let rule = |op: &str, value: &str, preset: &str| json!({"range": "A1:A3", "rule": {"type": "cellIs", "operator": op, "value": value, "preset": preset}});
        s.execute("home.conditionalFormat", rule("greaterThan", "300", "lightRedFill")).unwrap();
        s.execute("home.conditionalFormat", rule("greaterThan", "500", "redBorder")).unwrap();
        let d = s.doc().unwrap();
        let mut cache = CfCache::new();
        let mut look = |a: &str| cache.look(&d.wb, 0, CellRef::parse(a).unwrap()).and_then(|l| l.style);
        assert!(look("A1").is_none());
        let only_fill = look("A2").unwrap();
        assert_eq!(only_fill.fill.pattern, PatternType::Solid);
        assert!(only_fill.borders.top.is_none());
        let both = look("A3").unwrap();
        assert_eq!(both.fill, only_fill.fill);
        assert!(!both.borders.top.is_none());
    }

    #[test]
    fn cell_value_rules_on_blanks_and_errors() {
        let mut s = crate::Session::new();
        s.new_workbook();
        s.execute("range.setValues", json!({"range": "A1", "values": [[3, "=NA()"], [null, 7], ["=NA()", null], [" ", null], [7, null]]})).unwrap();
        let rule = |range: &str, r: serde_json::Value| json!({"range": range, "rule": r});
        s.execute("home.conditionalFormat", rule("A1:A5", json!({"type": "cellIs", "operator": "less", "value": "5", "preset": "lightRedFill"})))
            .unwrap();
        s.execute("home.conditionalFormat", rule("A1:A5", json!({"type": "noBlanks", "preset": "redBorder"}))).unwrap();
        s.execute("home.conditionalFormat", rule("B1:B2", json!({"type": "cellIs", "operator": "greater", "value": "5", "preset": "lightRedFill"})))
            .unwrap();
        let d = s.doc().unwrap();
        let mut cache = CfCache::new();
        let mut look = |a: &str| cache.look(&d.wb, 0, CellRef::parse(a).unwrap()).and_then(|l| l.style);
        let fill = |st: Option<Style>| st.is_some_and(|st| st.fill.pattern == PatternType::Solid);
        let border = |st: Option<Style>| st.is_some_and(|st| !st.borders.top.is_none());
        // Less than 5: 3 and the blank cell (as 0), not #N/A.
        assert_eq!([look("A1"), look("A2"), look("A3"), look("A5")].map(fill), [true, true, false, false]);
        // Greater than 5: 7, not #N/A.
        assert_eq!([look("B1"), look("B2")].map(fill), [false, true]);
        // No blanks: neither the empty cell nor the one holding a space.
        assert_eq!([look("A1"), look("A2"), look("A4")].map(border), [true, false, false]);
    }

    #[test]
    fn cell_value_operator_names() {
        let mut s = crate::Session::new();
        s.new_workbook();
        s.execute("range.setValues", json!({"range": "A1", "values": [[3], [7]]})).unwrap();
        let rule = |op: &str| json!({"range": "A1:A2", "rule": {"type": "cellIs", "operator": op, "value": "5", "preset": "lightRedFill"}});
        s.execute("home.conditionalFormat", rule("lessThan")).unwrap();
        assert!(s.execute("home.conditionalFormat", rule("smaller")).is_err());
        let d = s.doc().unwrap();
        let mut cache = CfCache::new();
        let mut look = |a: &str| cache.look(&d.wb, 0, CellRef::parse(a).unwrap()).and_then(|l| l.style).is_some();
        assert_eq!([look("A1"), look("A2")], [true, false]);
    }

    /// Runs one Excel scenario of issue #1164: a rule "greater than 5" (or the formula `=A1>5`)
    /// on `range`, values in A1:C10 and E1:E10, then `steps`. Returns the rules by priority,
    /// each as its ranges (and formula), joined with " | " as in the measurements.
    fn rules_after(formula: bool, range: &str, steps: &[&str]) -> String {
        let mut s = crate::Session::new();
        s.new_workbook();
        let rows: Vec<serde_json::Value> = (1..=10).map(|i| json!([i, i + 10, i + 20, null, i])).collect();
        s.execute("range.setValues", json!({"range": "A1", "values": rows})).unwrap();
        let rule =
            if formula { json!({"type": "expression", "formula": "=A1>5"}) } else { json!({"type": "cellIs", "operator": "greater", "value": "5"}) };
        s.execute("home.conditionalFormat", json!({"range": range, "rule": rule})).unwrap();
        for step in steps {
            let words: Vec<&str> = step.split_whitespace().collect();
            let r = |a: &str| json!({"range": a});
            match words.as_slice() {
                ["clearRules", a] => s.execute("home.clearRules", r(a)),
                ["clearFormats", a] => s.execute("edit.clearFormats", r(a)),
                ["clearAll", a] => s.execute("edit.clearAll", r(a)),
                ["clearContents", a] => s.execute("edit.clearContents", r(a)),
                ["fillDown", a] => s.execute("edit.fillDown", r(a)),
                ["autoFill", a, b] => s.execute("edit.autoFill", json!({"source": a, "target": b, "mode": "copy"})),
                [verb @ ("copy" | "cut"), a, b] => {
                    s.execute(if *verb == "cut" { "edit.cut" } else { "edit.copy" }, r(a)).unwrap();
                    if b.contains(':') {
                        // A destination larger than the copy: the copy is repeated over it.
                        s.execute("selection.set", r(b)).unwrap();
                        s.execute("edit.paste", json!({}))
                    } else {
                        s.execute("edit.paste", json!({"at": b}))
                    }
                }
                ["paint", a, b] => {
                    s.execute("selection.set", r(a)).unwrap();
                    s.execute("edit.formatPainter", json!({})).unwrap();
                    s.execute("edit.formatPainter", json!({"apply": b}))
                }
                ["pasteValues", a, b] => {
                    s.execute("edit.copy", r(a)).unwrap();
                    s.execute("edit.pasteSpecial", json!({"what": "values", "at": b}))
                }
                _ => panic!("unknown step {step}"),
            }
            .unwrap();
        }
        let mut cfs = s.doc().unwrap().wb.sheets[0].cond_formats.clone();
        cfs.sort_by_key(|cf| cf.priority);
        let show = |cf: &CondFormat| {
            let ranges = cf.ranges.iter().map(|r| r.a1()).collect::<Vec<_>>().join(",");
            match &cf.rule {
                CfRule::Expression { formula, .. } => format!("{ranges} ={formula}"),
                _ => ranges,
            }
        };
        cfs.iter().map(show).collect::<Vec<_>>().join(" | ")
    }

    #[test]
    fn rules_follow_their_cells_as_in_excel() {
        // Issue #1164: each rule's ranges after the action, measured in Excel for Microsoft 365
        // (Range.Copy/Cut with a destination, FillDown, AutoFill, FormatConditions.Delete,
        // ClearFormats, Clear; listed by priority).
        let cases: &[(&str, &[&str], &str)] = &[
            // Clear Rules from Selected Cells, Clear Formats, Clear All take the cells out.
            ("A1:A10", &["clearRules A5"], "A1:A4,A6:A10"),
            ("A1:A10", &["clearRules A1:A3"], "A4:A10"),
            ("A1:C10", &["clearRules B5"], "A1:C4,A6:C10,A5,C5"),
            ("A1:C10", &["clearRules A5:C5"], "A1:C4,A6:C10"),
            ("A1:C10", &["clearFormats B5:B6"], "A1:C4,A7:C10,A5:A6,C5:C6"),
            ("A1:A10", &["clearFormats A1:A10"], ""),
            ("A1:A10", &["clearAll A1:A10"], ""),
            ("A1:A10", &["clearContents A1:A10"], "A1:A10"),
            // A fill widens the rule.
            ("A1:A10", &["fillDown A10:A14"], "A1:A14"),
            ("A1:A10", &["fillDown A4:A6"], "A1:A10"),
            ("A1:A10", &["autoFill A1:A10 A1:A15"], "A1:A15"),
            ("A1:A10", &["autoFill A1:A10 A1:C10"], "A1:C10"),
            // A copy joins its rule when it stays one line of cells; otherwise a new rule, on top.
            ("A1:A10", &["copy A1:A10 C1"], "C1:C10 | A1:A10"),
            ("A1:A10", &["copy A2 A12"], "A12 | A1:A10"),
            ("A1:A10", &["copy A2 A11"], "A1:A11"),
            ("A1:A10", &["copy A3 A11"], "A1:A11"),
            ("A1:A10", &["copy A9:A10 A11"], "A1:A12"),
            ("A1:A10", &["copy A1:A10 A5"], "A1:A14"),
            ("A1:A10", &["copy A1:A10 A11"], "A1:A20"),
            ("A1:A10", &["copy A1:A10 A12"], "A12:A21 | A1:A10"),
            ("A1:A10", &["copy A5 A3"], "A1:A10"),
            ("A1:A10", &["copy A1:A10 B1"], "B1:B10 | A1:A10"),
            ("A1:A10", &["copy A10 B10"], "B10 | A1:A10"),
            ("A1:A10", &["copy A3:A4 C1"], "C1:C2 | A1:A10"),
            ("A1:A10", &["copy A1:B2 E1"], "E1:E2 | A1:A10"),
            ("A1:A10", &["copy A1:A2 C1:C6"], "C1:C6 | A1:A10"),
            ("A1:A10", &["copy A1:A10 C1", "copy A1:A10 C11"], "C11:C20 | C1:C10 | A1:A10"),
            ("A1:J1", &["copy A1:J1 A2"], "A2:J2 | A1:J1"),
            ("A1:J1", &["copy A1:J1 K1"], "A1:T1"),
            ("A1:J1", &["copy J1 K1"], "A1:K1"),
            ("A1:C10", &["copy A1:C10 D1"], "D1:F10 | A1:C10"),
            ("A1:C10", &["copy A1:C10 A11"], "A11:C20 | A1:C10"),
            ("A1:C10", &["copy A1:A10 D1"], "D1:D10 | A1:C10"),
            // Cells pasted over take their own formatting, without a rule.
            ("A1:A10", &["copy E1 A5"], "A1:A4,A6:A10"),
            ("A1:A10", &["copy E5 A3:A4"], "A1:A2,A5:A10"),
            // A cut moves the cells within their rule.
            ("A1:A10", &["cut A1:A10 C1"], "C1:C10"),
            ("A1:A10", &["cut A3:A4 C1"], "A1:A2,A5:A10,C1:C2"),
            ("A1:A10", &["cut A1:A10 A11"], "A11:A20"),
            ("A1:A10", &["cut A3:A4 A11"], "A1:A2,A5:A12"),
            ("A1:A10", &["cut E1:E10 A1"], ""),
        ];
        for (range, steps, excel) in cases {
            assert_eq!(rules_after(false, range, steps), *excel, "{range} {steps:?}");
        }
        // Formulas are rewritten for the rule's first cell.
        let formulas: &[(&[&str], &str)] = &[
            (&["copy A1:A10 C3"], "C3:C12 =C3>5 | A1:A10 =A1>5"),
            (&["cut A1:A10 C3"], "C3:C12 =C3>5"),
            (&["clearRules A1:A3"], "A4:A10 =A4>5"),
            (&["fillDown A10:A14"], "A1:A14 =A1>5"),
            (&["copy A1:A10 B1"], "B1:B10 =B1>5 | A1:A10 =A1>5"),
            (&["copy A1:A10 A11"], "A1:A20 =A1>5"),
        ];
        for (steps, excel) in formulas {
            assert_eq!(rules_after(true, "A1:A10", steps), *excel, "{steps:?}");
        }
    }

    #[test]
    fn format_painter_and_paste_special_carry_rules_like_a_paste() {
        // Excel's Format Painter can't be driven without the clipboard. It pastes formats, and
        // Paste Special › Formats copies conditional formats, so both follow a paste here.
        // Pasted values leave the destination's rules alone.
        assert_eq!(rules_after(false, "A1:A10", &["paint A1:A10 C1"]), "C1:C10 | A1:A10");
        assert_eq!(rules_after(false, "A1:A10", &["paint A1:A10 A11"]), "A1:A20");
        assert_eq!(rules_after(false, "A1:A10", &["paint A1:A2 C1:C5"]), "C1:C5 | A1:A10");
        assert_eq!(rules_after(false, "A1:A10", &["paint E1 A5"]), "A1:A4,A6:A10");
        assert_eq!(rules_after(false, "A1:A10", &["pasteValues E1:E10 A1"]), "A1:A10");
        assert_eq!(rules_after(false, "A1:A10", &["pasteValues A1:A10 C1"]), "A1:A10");
    }

    #[test]
    fn a_cut_to_another_sheet_takes_its_rule_along_and_undo_brings_it_back() {
        let mut s = crate::Session::new();
        s.new_workbook();
        s.execute("range.setValues", json!({"range": "A1", "values": [[1], [7]]})).unwrap();
        s.execute("home.conditionalFormat", json!({"range": "A1:A2", "rule": {"type": "expression", "formula": "=A1>5"}})).unwrap();
        let first = s.doc().unwrap().wb.active_sheet;
        s.execute("home.insertSheet", json!({"name": "Other"})).unwrap();
        let other = s.doc().unwrap().wb.sheets.iter().position(|sh| sh.name == "Other").unwrap();
        s.execute("edit.cut", json!({"range": "A1:A2", "sheet": first})).unwrap();
        s.execute("edit.paste", json!({"at": "B3", "sheet": other})).unwrap();
        let ranges = |s: &crate::Session, i: usize| -> Vec<String> {
            s.doc().unwrap().wb.sheets[i].cond_formats.iter().map(|cf| cf.ranges.iter().map(|r| r.a1()).collect::<Vec<_>>().join(",")).collect()
        };
        assert!(ranges(&s, first).is_empty());
        assert_eq!(ranges(&s, other), ["B3:B4"]);
        let moved = &s.doc().unwrap().wb.sheets[other].cond_formats[0];
        assert!(matches!(&moved.rule, CfRule::Expression { formula, .. } if formula == "B3>5"), "{moved:?}");
        // The moved rule formats B4 (7) on the other sheet.
        let d = s.doc().unwrap();
        let mut cache = CfCache::new();
        let mut look = |a: &str| cache.look(&d.wb, other, CellRef::parse(a).unwrap()).and_then(|l| l.style).is_some();
        assert_eq!([look("B3"), look("B4")], [false, true]);
        s.execute("edit.undo", json!({})).unwrap();
        assert_eq!(ranges(&s, first), ["A1:A2"]);
        assert!(ranges(&s, other).is_empty());
    }
}
