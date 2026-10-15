//! PivotTable Analyze › Group (Group Selection, Ungroup) and › Calculations › Fields, Items &
//! Sets (Calculated Item, Solve Order, List Formulas). Group Field is `pivot.group` in
//! [`super::pivot`].
//!
//! Behaviour follows Excel's documented rules, checked black-box against Excel 365:
//! - Group Selection gathers two or more items of a row or column field into a group (`Group1`)
//!   of a new field named after the base field (`Region2`), placed just outside it. Items not in
//!   a group stay items of their own. Ungroup on a group removes it, and the grouped field goes
//!   when its last group does.
//! - A calculated item is a formula over the items of its field. It can't be added while a field
//!   of the PivotTable is grouped by dates or number ranges, while a value field uses Average,
//!   StdDev or Var, or while a field is in the Values area twice; those changes are refused
//!   while the PivotTable has calculated items.

use gridcraft_core::{CellRef, Value};
use gridcraft_model::*;
use serde_json::{Value as Json, json};

use super::pivot::{find_pivot, in_axes, other, update};
use super::*;
use crate::pivot as pv;

pub fn specs() -> Vec<CommandSpec> {
    vec![
        cmd!(
            "pivot.groupSelection",
            "Group Selection",
            ["PivotTable Analyze", "Group"],
            None,
            "{pivot?, field?: row or column field, items?: [\"East\", \"North\"] (default: the item cells selected in the report), replace?} gathers two or more items into a new group (Group1) of a grouped field (Region2) → {field, group}",
            has_doc,
            group_selection
        ),
        cmd!(
            "pivot.ungroup",
            "Ungroup",
            ["PivotTable Analyze", "Group"],
            Some("Alt+Shift+Left"),
            "{pivot?, field?, items?: [group names] (default: the item at the active cell), replace?} removes groups of a grouped field (the field goes with its last group), or a field's date or number grouping",
            has_doc,
            ungroup
        ),
        cmd!(
            "pivot.calculatedItem",
            "Calculated Item",
            ["PivotTable Analyze", "Calculations"],
            None,
            "{pivot?, field?: row or column field (default: the field at the active cell), name, formula?: \"=East+West\" (items bare, 'quoted' or Field[Item]), delete?: bool, replace?} adds, changes or deletes a calculated item; without `name` opens the dialog → {name, field, formula, calculatedItems}",
            has_doc,
            calculated_item
        ),
        cmd!(
            "pivot.solveOrder",
            "Solve Order",
            ["PivotTable Analyze", "Calculations"],
            None,
            "{pivot?, order?: [calculated item names, first to last], item?: name, move?: up|down, replace?} reorders the calculated items (where two meet, the later formula sets the value); without `order` or `item` opens the dialog → {order}",
            has_doc,
            solve_order
        ),
        cmd!(
            query "pivot.fieldItems",
            "Field Items",
            [],
            None,
            "{pivot?, field} → {field, items: [{label, shown}]}: the field's items (grouped as the field is, with its calculated items) and whether its item selection shows each",
            has_doc,
            field_items_query
        ),
        cmd!(
            "pivot.listFormulas",
            "List Formulas",
            ["PivotTable Analyze", "Calculations"],
            None,
            "{pivot?} inserts a sheet after the PivotTable's listing its calculated fields and calculated items with their solve order and formulas → {sheet}",
            has_doc,
            list_formulas
        ),
    ]
}

/// Excel's refusal when a selection can't be grouped.
pub const CANNOT_GROUP: &str = "Cannot group that selection.";
/// Excel's refusal when a selection can't be ungrouped.
const CANNOT_UNGROUP: &str = "Cannot ungroup that selection.";
/// Calculated items and date or number grouping exclude each other.
pub const GROUPED_FIELD: &str = "This PivotTable report field is grouped. You cannot add a calculated item to a grouped field.";
/// A PivotTable with calculated items can't group by dates or number ranges.
pub const GROUP_WITH_CALC_ITEMS: &str = "You cannot group by dates or number ranges in a PivotTable report that has calculated items.";
/// Calculated items and Average, StdDev and Var exclude each other.
pub const NO_AVERAGES: &str = "Averages, standard deviations, and variances are not supported when a PivotTable report has calculated items.";
/// Calculated items and a field in Values twice exclude each other.
pub const NO_DUPLICATE_VALUES: &str = "Multiple data fields of the same field are not supported when a PivotTable report has calculated items.";
/// A field with calculated items stays in Rows or Columns.
pub const CALC_ITEM_AREA: &str = "A field with calculated items can only be in the Rows or Columns area.";
const ITEM_NOT_FOUND: &str = "An item name was not found. Check that you've typed the name correctly and that the item is in the PivotTable report.";
const NO_REFERENCES: &str = "References, names, and arrays are not supported in PivotTable formulas.";

/// Whether a function can't be used while the PivotTable has calculated items.
pub fn averages(f: PivotFunc) -> bool {
    matches!(f, PivotFunc::Average | PivotFunc::StdDev | PivotFunc::StdDevP | PivotFunc::Var | PivotFunc::VarP)
}

/// Whether any field of the PivotTable is grouped by dates or number ranges.
pub fn has_range_grouping(pt: &PivotTable) -> bool {
    pt.rows.iter().chain(pt.columns.iter()).any(|f| f.date_group != PivotDateGroup::None || f.number_group.is_some())
}

/// The item cells of the report that the selection covers: (field, item captions in report order).
fn selected_items(s: &Session, si: usize, pi: usize) -> Result<(String, Vec<String>)> {
    let d = s.doc()?;
    let cells = pv::report_items(&d.wb, si, pi);
    let mut field: Option<String> = None;
    let mut items: Vec<String> = Vec::new();
    for (c, it) in &cells {
        if !d.selection.ranges.iter().any(|r| r.contains(*c)) && d.selection.active != *c {
            continue;
        }
        match &field {
            Some(f) if !f.eq_ignore_ascii_case(&it.field) => return Err(other(CANNOT_GROUP)),
            Some(_) => {}
            None => field = Some(it.field.clone()),
        }
        if !items.iter().any(|x| x.eq_ignore_ascii_case(&it.label)) {
            items.push(it.label.clone());
        }
    }
    let field = field
        .ok_or_else(|| EngineError::Disabled("pivot.groupSelection".into(), "select items of a row or column field in the PivotTable".into()))?;
    Ok((field, items))
}

/// The field and item at the active cell of the report, if it is an item cell.
pub fn active_item(s: &Session, si: usize, pi: usize) -> Option<(String, String)> {
    let d = s.doc().ok()?;
    let at = d.selection.active;
    pv::report_items(&d.wb, si, pi).into_iter().find(|(c, _)| *c == at).map(|(_, it)| (it.field, it.label))
}

/// A list parameter of strings.
fn names_param(p: &Json, key: &str) -> Option<Vec<String>> {
    p.get(key).and_then(Json::as_array).map(|a| a.iter().take(100_000).filter_map(|x| x.as_str().map(|s| s.trim().to_string())).collect())
}

/// A field name not taken by a source, grouped or calculated field: `Region2`, `Region3`…
fn new_field_name(pt: &PivotTable, src: &pv::Source, base: &str) -> String {
    let taken = |n: &str| src.col(n).is_some() || pt.group_field(n).is_some() || pt.calculated_fields.iter().any(|c| c.name.eq_ignore_ascii_case(n));
    (2..100_000).map(|k| format!("{base}{k}")).find(|n| !taken(n)).unwrap_or_else(|| format!("{base}2"))
}

/// The next free group caption (`Group1`, `Group2`…); numbering runs across the grouped fields.
fn new_group_name(pt: &PivotTable) -> String {
    let taken = |n: &str| pt.group_fields.iter().any(|g| g.groups.iter().any(|x| x.name.eq_ignore_ascii_case(n)));
    (1..100_000).map(|k| format!("Group{k}")).find(|n| !taken(n)).unwrap_or_else(|| "Group1".into())
}

fn group_selection(s: &mut Session, p: &Json) -> Result<Json> {
    const C: &str = "pivot.groupSelection";
    let (si, pi) = find_pivot(s, p)?;
    let (field, items) = match (str_param(p, "field"), names_param(p, "items")) {
        (Some(f), Some(items)) => (f.trim().to_string(), items),
        (Some(_), None) | (None, Some(_)) => return Err(bad(C, "pass both `field` and `items`, or neither (the selected item cells)")),
        (None, None) => {
            let (field, items) = selected_items(s, si, pi)?;
            // One item of a number or date field: Excel opens the Grouping dialog.
            if items.len() < 2 {
                let numeric = s.doc().ok().and_then(|d| {
                    let pt = d.wb.sheet(si)?.pivots.get(pi)?;
                    let src = pv::read_source(&d.wb, &pt.source, si).ok()?;
                    let c = src.col(&field)?;
                    src.numeric.get(c).copied()
                });
                if numeric == Some(true) {
                    return super::pivot::group(s, &json!({"pivot": p.get("pivot").cloned().unwrap_or(Json::Null)}));
                }
            }
            (field, items)
        }
    };
    if items.len() < 2 {
        return Err(other(CANNOT_GROUP));
    }
    update(s, p, |pt, src, wb| {
        let entry = pt.rows.iter().chain(pt.columns.iter()).find(|f| f.source_col.eq_ignore_ascii_case(&field)).cloned();
        let Some(entry) = entry else { return Err(bad(C, format!("`{field}` isn't in the rows or columns area"))) };
        // Fields made by grouping, and fields grouped by dates or number ranges, can't be.
        if pt.group_field(&field).is_some() || entry.date_group != PivotDateGroup::None || entry.number_group.is_some() {
            return Err(other(CANNOT_GROUP));
        }
        let c = src.col(&field).ok_or_else(|| other(CANNOT_GROUP))?;
        let labels = pv::distinct_labels(wb, src, c, pv::Grouping::default(), 100_000);
        let mut members: Vec<String> = Vec::new();
        for it in &items {
            let label = labels.iter().find(|l| l.eq_ignore_ascii_case(it)).ok_or_else(|| other(CANNOT_GROUP))?;
            if !members.contains(label) {
                members.push(label.clone());
            }
        }
        if members.len() < 2 {
            return Err(other(CANNOT_GROUP));
        }
        let name = new_group_name(pt);
        let base = entry.source_col.clone();
        let existing = pt.group_fields.iter().position(|g| g.base.eq_ignore_ascii_case(&base));
        let gf_name = match existing.and_then(|i| pt.group_fields.get_mut(i)) {
            Some(gf) => {
                // Items move out of the groups they were in; a group left empty goes.
                for g in gf.groups.iter_mut() {
                    g.items.retain(|x| !members.iter().any(|m| m.eq_ignore_ascii_case(x)));
                }
                gf.groups.retain(|g| !g.items.is_empty());
                gf.groups.push(PivotItemGroup { name: name.clone(), items: members });
                gf.name.clone()
            }
            None => {
                let gf_name = new_field_name(pt, src, &base);
                pt.group_fields.push(PivotGroupField {
                    name: gf_name.clone(),
                    base: base.clone(),
                    groups: vec![PivotItemGroup { name: name.clone(), items: members }],
                });
                // The grouped field goes just outside its base field, in the same area.
                let new = PivotField { source_col: gf_name.clone(), sort: entry.sort, ..PivotField::default() };
                let rows = pt.rows.iter().any(|f| f.source_col.eq_ignore_ascii_case(&base));
                let list = if rows { &pt.rows } else { &pt.columns };
                let at = list.iter().position(|f| f.source_col.eq_ignore_ascii_case(&base)).unwrap_or(0);
                super::pivot::insert_axis_field(pt, rows, new, Some(at));
                gf_name
            }
        };
        Ok(json!({"field": gf_name, "group": name}))
    })
}

/// Drops a grouped field from the PivotTable: its groups, its place in the areas and its filters.
fn drop_group_field(pt: &mut PivotTable, name: &str) {
    pt.group_fields.retain(|g| !g.name.eq_ignore_ascii_case(name));
    super::pivot::take_from_axes(pt, name);
    pt.filters.retain(|f| !f.source_col.eq_ignore_ascii_case(name));
}

fn ungroup(s: &mut Session, p: &Json) -> Result<Json> {
    const C: &str = "pivot.ungroup";
    let (si, pi) = find_pivot(s, p)?;
    let (field, items) = match str_param(p, "field") {
        Some(f) => (f.trim().to_string(), names_param(p, "items")),
        None => {
            let (f, label) = active_item(s, si, pi).ok_or_else(|| EngineError::Disabled(C.into(), "select an item of a grouped field".into()))?;
            (f, Some(vec![label]))
        }
    };
    update(s, p, |pt, _, _| {
        if let Some(gi) = pt.group_fields.iter().position(|g| g.name.eq_ignore_ascii_case(&field)) {
            let name = pt.group_fields.get(gi).map(|g| g.name.clone()).unwrap_or_default();
            match &items {
                None => drop_group_field(pt, &name),
                Some(list) => {
                    let gf = pt.group_fields.get_mut(gi).ok_or_else(|| other(CANNOT_UNGROUP))?;
                    let before = gf.groups.len();
                    gf.groups.retain(|g| !list.iter().any(|x| x.eq_ignore_ascii_case(&g.name)));
                    if gf.groups.len() == before {
                        return Err(other(CANNOT_UNGROUP));
                    }
                    if gf.groups.is_empty() {
                        drop_group_field(pt, &name);
                    }
                }
            }
            return Ok(json!({"field": name}));
        }
        // A field grouped by dates or number ranges: its grouping goes (one plain field is left).
        let grouped = pt
            .rows
            .iter()
            .chain(pt.columns.iter())
            .any(|f| f.source_col.eq_ignore_ascii_case(&field) && (f.date_group != PivotDateGroup::None || f.number_group.is_some()));
        if !grouped {
            return Err(other(CANNOT_UNGROUP));
        }
        super::pivot::set_grouping(pt, &field, &[], None, None)?;
        Ok(json!({"field": field}))
    })
}

// ---------------------------------------------------------------- calculated items

/// Why the PivotTable can't take calculated items, if it can't.
pub fn calc_item_blocker(pt: &PivotTable) -> Option<&'static str> {
    if has_range_grouping(pt) {
        return Some(GROUPED_FIELD);
    }
    if pt.values.iter().any(|v| averages(v.func)) {
        return Some(NO_AVERAGES);
    }
    let mut seen: Vec<&str> = Vec::new();
    for v in &pt.values {
        if seen.iter().any(|x| x.eq_ignore_ascii_case(&v.source_col)) {
            return Some(NO_DUPLICATE_VALUES);
        }
        seen.push(&v.source_col);
    }
    None
}

/// The item captions of a source field (for a calculated item's formula), with its calculated items.
fn field_items(pt: &PivotTable, src: &pv::Source, wb: &Workbook, field: &str) -> Vec<String> {
    let mut v = match src.col(field) {
        Some(c) => pv::distinct_labels(wb, src, c, pv::Grouping::default(), 100_000),
        None => vec![],
    };
    v.extend(pt.calculated_items.iter().filter(|c| c.field.eq_ignore_ascii_case(field)).map(|c| c.name.clone()));
    v
}

fn calc_items_json(pt: &PivotTable) -> Json {
    json!(pt.calculated_items.iter().map(|c| json!({"field": c.field, "name": c.name, "formula": c.formula})).collect::<Vec<_>>())
}

fn calculated_item(s: &mut Session, p: &Json) -> Result<Json> {
    const C: &str = "pivot.calculatedItem";
    let (si, pi) = find_pivot(s, p)?;
    let field = match str_param(p, "field") {
        Some(f) => Some(f.trim().to_string()),
        None => active_item(s, si, pi).map(|(f, _)| f),
    };
    let Some(name) = str_param(p, "name").map(|n| n.trim().to_string()) else {
        // Menu-style call: the Insert Calculated Item dialog for the active field.
        let d = s.doc()?;
        let pt = d.wb.sheet(si).and_then(|sh| sh.pivots.get(pi)).ok_or_else(|| other("There's no such PivotTable."))?;
        let field = field.or_else(|| pt.rows.first().or(pt.columns.first()).map(|f| f.source_col.clone()));
        let Some(field) = field else {
            return Err(EngineError::Disabled(C.into(), "select an item of a row or column field in the PivotTable".into()));
        };
        if pt.group_field(&field).is_some() {
            return Err(other("Calculated items can't be added to a field made by Group Selection."));
        }
        if let Some(why) = calc_item_blocker(pt) {
            return Err(other(why));
        }
        let src = pv::read_source(&d.wb, &pt.source, si).map_err(other)?;
        let n =
            (1..).map(|k| format!("Formula{k}")).find(|n| !pt.calculated_items.iter().any(|c| c.name.eq_ignore_ascii_case(n))).unwrap_or_default();
        let fields: Vec<String> = pt.rows.iter().chain(pt.columns.iter()).map(|f| f.source_col.clone()).filter(|f| src.col(f).is_some()).collect();
        let items = field_items(pt, &src, &d.wb, &field);
        let pivot = pt.name.clone();
        let existing = calc_items_json(pt);
        s.ui_requests.push(crate::UiRequest::Dialog(
            "pivotCalculatedItem".into(),
            json!({"pivot": pivot, "field": field, "name": n, "formula": "= 0", "fields": fields, "items": items, "calculatedItems": existing}),
        ));
        return ok();
    };
    let field = field.ok_or_else(|| bad(C, "missing `field`"))?;
    if name.is_empty() || name.chars().count() > 255 {
        return Err(bad(C, "`name` must be 1 to 255 characters"));
    }
    let delete = bool_param(p, "delete").unwrap_or(false);
    let formula = str_param(p, "formula").map(|f| {
        let f = f.trim();
        if f.starts_with('=') { f.to_string() } else { format!("={f}") }
    });
    if !delete && formula.is_none() {
        return Err(bad(C, "missing `formula`"));
    }
    update(s, p, |pt, src, wb| {
        let existing = pt.calculated_items.iter().position(|c| c.field.eq_ignore_ascii_case(&field) && c.name.eq_ignore_ascii_case(&name));
        if delete {
            let i = existing.ok_or_else(|| bad(C, format!("no calculated item `{name}` in `{field}`")))?;
            let gone = pt.calculated_items.remove(i);
            // A selection of the field's items no longer lists it.
            for f in pt.filters.iter_mut().filter(|f| f.source_col.eq_ignore_ascii_case(&gone.field)) {
                if let Some(sel) = f.selected.as_mut() {
                    sel.retain(|x| !x.eq_ignore_ascii_case(&gone.name));
                }
            }
            return Ok(json!({"name": gone.name, "field": gone.field, "deleted": true, "calculatedItems": calc_items_json(pt)}));
        }
        if pt.group_field(&field).is_some() {
            return Err(other("Calculated items can't be added to a field made by Group Selection."));
        }
        let canon =
            src.col(&field).and_then(|c| src.headers.get(c).cloned()).ok_or_else(|| bad(C, format!("no field `{field}` in the source data")))?;
        if !in_axes(pt, &canon) {
            return Err(other(CALC_ITEM_AREA));
        }
        if let Some(why) = calc_item_blocker(pt) {
            return Err(other(why));
        }
        let data_items = match src.col(&canon) {
            Some(c) => pv::distinct_labels(wb, src, c, pv::Grouping::default(), 100_000),
            None => vec![],
        };
        if data_items.iter().any(|l| l.eq_ignore_ascii_case(&name)) {
            return Err(other(format!("\"{canon}\" already has an item named \"{name}\".")));
        }
        let formula = formula.clone().unwrap_or_default();
        let mut known = field_items(pt, src, wb, &canon);
        if existing.is_none() {
            known.push(name.clone());
        }
        let lookup = |n: &str| known.iter().find(|k| k.trim().eq_ignore_ascii_case(n.trim())).cloned();
        if let Err(e) = crate::pivot_items::compile(&formula, &canon, &lookup) {
            return Err(other(if e.contains("named") {
                ITEM_NOT_FOUND.to_string()
            } else if e.contains("refer to cells") {
                NO_REFERENCES.to_string()
            } else {
                e
            }));
        }
        let item_name = match existing.and_then(|i| pt.calculated_items.get_mut(i)) {
            Some(c) => {
                c.formula = formula.clone();
                c.name.clone()
            }
            None => {
                if pt.calculated_items.len() >= crate::pivot_items::MAX_CALC_ITEMS {
                    return Err(other(format!("A PivotTable can have at most {} calculated items.", crate::pivot_items::MAX_CALC_ITEMS)));
                }
                pt.calculated_items.push(PivotCalcItem { field: canon.clone(), name: name.clone(), formula: formula.clone() });
                // A selection of the field's items shows the new item too.
                for f in pt.filters.iter_mut().filter(|f| f.source_col.eq_ignore_ascii_case(&canon)) {
                    if let Some(sel) = f.selected.as_mut() {
                        sel.push(name.clone());
                    }
                }
                name.clone()
            }
        };
        Ok(json!({"name": item_name, "field": canon, "formula": formula, "calculatedItems": calc_items_json(pt)}))
    })
}

fn solve_order(s: &mut Session, p: &Json) -> Result<Json> {
    const C: &str = "pivot.solveOrder";
    let order = names_param(p, "order");
    let item = str_param(p, "item").map(|x| x.trim().to_string());
    if order.is_none() && item.is_none() {
        // Menu-style call: the Calculated Item Solve Order dialog.
        let (si, pi) = find_pivot(s, p)?;
        let d = s.doc()?;
        let pt = d.wb.sheet(si).and_then(|sh| sh.pivots.get(pi)).ok_or_else(|| other("There's no such PivotTable."))?;
        if pt.calculated_items.is_empty() {
            return Err(EngineError::Disabled(C.into(), "the PivotTable has no calculated items".into()));
        }
        let pivot = pt.name.clone();
        let list = calc_items_json(pt);
        s.ui_requests.push(crate::UiRequest::Dialog("pivotSolveOrder".into(), json!({"pivot": pivot, "calculatedItems": list})));
        return ok();
    }
    let up = match str_param(p, "move").map(str::to_ascii_lowercase).as_deref() {
        None | Some("up") => true,
        Some("down") => false,
        Some(m) => return Err(bad(C, format!("unknown move `{m}` (up|down)"))),
    };
    update(s, p, |pt, _, _| {
        let n = pt.calculated_items.len();
        match (&order, &item) {
            (Some(order), _) => {
                if order.len() != n {
                    return Err(bad(C, "`order` must name every calculated item once"));
                }
                let mut sorted: Vec<PivotCalcItem> = Vec::with_capacity(n);
                for name in order {
                    let i = pt
                        .calculated_items
                        .iter()
                        .position(|c| c.name.eq_ignore_ascii_case(name))
                        .ok_or_else(|| bad(C, format!("no calculated item `{name}`")))?;
                    sorted.push(pt.calculated_items.remove(i));
                }
                pt.calculated_items = sorted;
            }
            (None, Some(name)) => {
                let i = pt
                    .calculated_items
                    .iter()
                    .position(|c| c.name.eq_ignore_ascii_case(name))
                    .ok_or_else(|| bad(C, format!("no calculated item `{name}`")))?;
                let j = if up { i.checked_sub(1) } else { Some(i + 1).filter(|j| *j < n) };
                if let Some(j) = j {
                    pt.calculated_items.swap(i, j);
                }
            }
            (None, None) => {}
        }
        Ok(json!({"order": pt.calculated_items.iter().map(|c| c.name.clone()).collect::<Vec<_>>()}))
    })
}

fn field_items_query(s: &mut Session, p: &Json) -> Result<Json> {
    const C: &str = "pivot.fieldItems";
    let field = str_param(p, "field").ok_or_else(|| bad(C, "missing `field`"))?.trim().to_string();
    let (si, pi) = find_pivot(s, p)?;
    let d = s.doc()?;
    let pt = d.wb.sheet(si).and_then(|sh| sh.pivots.get(pi)).ok_or_else(|| other("There's no such PivotTable."))?;
    let src = pv::read_source(&d.wb, &pt.source, si).map_err(other)?;
    let (c, g) = pv::resolve_name(&src, pt, &field).ok_or_else(|| bad(C, format!("no field `{field}` in the PivotTable")))?;
    let mut labels = pv::distinct_labels(&d.wb, &src, c, g, 100_000);
    labels.extend(pt.calculated_items.iter().filter(|x| x.field.eq_ignore_ascii_case(&field)).map(|x| x.name.clone()));
    let selected = pt.filters.iter().find(|f| f.source_col.eq_ignore_ascii_case(&field)).and_then(|f| f.selected.as_ref());
    let items: Vec<Json> = labels
        .into_iter()
        .map(|l| {
            let shown = selected.is_none_or(|sel| sel.iter().any(|x| x.eq_ignore_ascii_case(&l)));
            json!({"label": l, "shown": shown})
        })
        .collect();
    Ok(json!({"field": field, "items": items}))
}

fn list_formulas(s: &mut Session, p: &Json) -> Result<Json> {
    let (si, pi) = find_pivot(s, p)?;
    if s.doc()?.wb.protected_structure {
        return Err(other("The workbook is protected and cannot be changed."));
    }
    let pt = s.doc()?.wb.sheet(si).and_then(|sh| sh.pivots.get(pi)).cloned().ok_or_else(|| other("There's no such PivotTable."))?;
    // (row, column, text, bold, italic) as Excel lays the sheet out.
    let mut cells: Vec<(u32, u32, String, bool, bool)> = Vec::new();
    let mut row = 0u32;
    let section = |cells: &mut Vec<(u32, u32, String, bool, bool)>, row: &mut u32, title: &str, what: &str, list: Vec<(String, String)>| {
        cells.push((*row, 0, title.into(), true, true));
        *row += 1;
        for (c, h) in ["Solve Order", what, "Formula"].iter().enumerate() {
            cells.push((*row, c as u32, (*h).into(), true, false));
        }
        *row += 1;
        for (i, (name, formula)) in list.into_iter().enumerate() {
            cells.push((*row, 0, (i + 1).to_string(), false, false));
            cells.push((*row, 1, name, false, false));
            cells.push((*row, 2, formula, false, false));
            *row += 1;
        }
        *row += 1;
    };
    let fields: Vec<(String, String)> = pt.calculated_fields.iter().map(|c| (c.name.clone(), c.formula.clone())).collect();
    let items: Vec<(String, String)> = pt.calculated_items.iter().map(|c| (c.name.clone(), c.formula.clone())).collect();
    section(&mut cells, &mut row, "Calculated Field", "Field", fields);
    section(&mut cells, &mut row, "Calculated Item", "Item", items);
    row += 1;
    cells.push((row, 0, "Note:".into(), true, true));
    cells.push((row, 1, "When more than one formula updates a cell,".into(), false, false));
    cells.push((row + 1, 1, "the formula last in the solve order sets its value.".into(), false, false));
    cells.push((row + 3, 1, "To change the solve order of calculated items or fields,".into(), false, false));
    cells.push((row + 4, 1, "use PivotTable Analyze › Calculations › Fields, Items & Sets › Solve Order.".into(), false, false));
    let name = s.doc()?.wb.next_sheet_name();
    let d = s.doc_mut()?;
    let at = edit_list(d, si, &name, &cells)?;
    d.sheet_selections.insert(at.min(d.sheet_selections.len()), crate::Selection::default());
    Ok(json!({"sheet": name}))
}

/// Inserts the List Formulas sheet after sheet `si` and writes `cells` as text; returns its index.
fn edit_list(d: &mut crate::DocState, si: usize, name: &str, cells: &[(u32, u32, String, bool, bool)]) -> Result<usize> {
    super::commit(d, |cx| {
        let at = (si + 1).min(cx.wb.sheets.len());
        cx.wb.sheets.insert(at, std::sync::Arc::new(Sheet::new(name.to_string())));
        for n in cx.wb.names.iter_mut() {
            if let Some(sc) = n.scope.as_mut()
                && *sc >= at
            {
                *sc += 1;
            }
        }
        for (r, c, text, bold, italic) in cells {
            let mut st = Style::default();
            st.font.bold = *bold;
            st.font.italic = *italic;
            let id = if *bold || *italic { cx.wb.styles.intern(st) } else { StyleId::default() };
            let sh = cx.sheet_mut(at)?;
            // Formulas are listed as text, as Excel lists them.
            sh.set_cell(CellRef::new(*r, *c), Cell { value: Value::text(text.as_str()), formula: None, style: id });
        }
        if let Some(sh) = cx.wb.sheet_mut(at) {
            for (c, w) in [(0u32, 80.0f32), (1, 140.0), (2, 200.0)] {
                sh.cols.entry(c).or_default().size = Some(w);
            }
        }
        cx.wb.active_sheet = at;
        *cx.sel = crate::Selection::default();
        cx.structural = true;
        Ok(at)
    })
}

#[cfg(test)]
mod tests {
    use gridcraft_core::{CellRef, Value};
    use serde_json::{Value as Json, json};

    use super::*;
    use crate::Session;

    /// Region, Product, Date, Units, Sales — 12 records over 2026 (as in `pivot`'s tests).
    fn sales() -> Session {
        let mut s = Session::new();
        s.new_workbook();
        let rows = json!([
            ["Region", "Product", "Date", "Units", "Sales"],
            ["East", "Apples", "1/15/2026", 10, 100],
            ["West", "Apples", "2/10/2026", 5, 50],
            ["East", "Pears", "3/3/2026", 8, 120],
            ["North", "Pears", "4/20/2026", 2, 30],
            ["West", "Plums", "5/5/2026", 7, 70],
            ["East", "Plums", "6/30/2026", 4, 60],
            ["North", "Apples", "7/4/2026", 9, 90],
            ["West", "Pears", "8/8/2026", 3, 45],
            ["East", "Apples", "9/9/2026", 6, 60],
            ["North", "Plums", "10/10/2026", 1, 15],
            ["West", "Apples", "11/11/2026", 12, 120],
            ["East", "Pears", "12/12/2026", 5, 75]
        ]);
        s.execute("range.setValues", json!({"range": "A1", "values": rows})).unwrap();
        s.execute("selection.set", json!({"range": "A1"})).unwrap();
        s
    }

    fn pt(s: &Session) -> PivotTable {
        s.doc().unwrap().wb.active().unwrap().pivots.last().unwrap().clone()
    }
    fn val(s: &Session, c: CellRef) -> Value {
        s.doc().unwrap().wb.active().unwrap().value(c)
    }
    /// The report's rows as (first column text, second column value).
    fn report(s: &Session) -> Vec<(String, Value)> {
        let r = pt(s).last_range.unwrap();
        (r.start.row..=r.end.row).map(|row| (val(s, CellRef::new(row, r.start.col)).display(), val(s, CellRef::new(row, r.start.col + 1)))).collect()
    }
    fn row_of(s: &Session, label: &str) -> CellRef {
        let r = pt(s).last_range.unwrap();
        (r.start.row..=r.end.row).map(|row| CellRef::new(row, r.start.col)).find(|c| val(s, *c).display() == label).unwrap()
    }
    fn amount(s: &Session, label: &str) -> Value {
        report(s).into_iter().find(|(l, _)| l == label).map(|(_, v)| v).unwrap_or(Value::Empty)
    }
    fn err(r: crate::Result<Json>) -> String {
        r.unwrap_err().to_string()
    }

    #[test]
    fn group_selection_makes_a_grouped_field_and_ungroup_removes_it() {
        let mut s = sales();
        s.execute("insert.pivotTable", json!({"rows": ["Region"], "values": ["Sales"]})).unwrap();
        // Select East and North in the report, as Excel's Group Selection takes them.
        let (east, north) = (row_of(&s, "East"), row_of(&s, "North"));
        s.execute("selection.set", json!({"range": format!("{},{}", east.a1(), north.a1())})).unwrap();
        let r = s.execute("pivot.groupSelection", json!({})).unwrap();
        assert_eq!((r["field"].as_str(), r["group"].as_str()), (Some("Region2"), Some("Group1")));
        let p = pt(&s);
        assert_eq!(p.rows.iter().map(|f| f.source_col.as_str()).collect::<Vec<_>>(), ["Region2", "Region"]);
        assert_eq!(p.group_fields[0].groups[0].items, ["East", "North"]);
        // Group1 East North / West West / Grand Total, as in Excel.
        assert_eq!(amount(&s, "Group1"), Value::Number(550.0));
        assert_eq!(amount(&s, "East"), Value::Number(415.0));
        assert_eq!(amount(&s, "Grand Total"), Value::Number(835.0));
        let labels: Vec<String> = report(&s).into_iter().map(|(l, _)| l).collect();
        assert_eq!(labels[1..], ["Group1", "East", "North", "West", "West", "Grand Total"]);
        // A filter on the grouped field selects groups.
        s.execute("pivot.filter", json!({"field": "Region2", "selected": ["West"]})).unwrap();
        assert_eq!(amount(&s, "Grand Total"), Value::Number(285.0));
        s.execute("pivot.filter", json!({"field": "Region2", "selected": null})).unwrap();
        // One item can't be grouped; neither can items of a grouped field or unknown items.
        assert!(err(s.execute("pivot.groupSelection", json!({"field": "Region", "items": ["West"]}))).contains(CANNOT_GROUP));
        assert!(err(s.execute("pivot.groupSelection", json!({"field": "Region2", "items": ["Group1", "West"]}))).contains(CANNOT_GROUP));
        assert!(err(s.execute("pivot.groupSelection", json!({"field": "Region", "items": ["West", "Nowhere"]}))).contains(CANNOT_GROUP));
        // Ungroup the group at the active cell: the grouped field goes with its last group.
        let g = row_of(&s, "Group1");
        s.execute("selection.set", json!({"range": g.a1()})).unwrap();
        s.execute("pivot.ungroup", json!({})).unwrap();
        let p = pt(&s);
        assert!(p.group_fields.is_empty());
        assert_eq!(p.rows.iter().map(|f| f.source_col.as_str()).collect::<Vec<_>>(), ["Region"]);
        assert_eq!(amount(&s, "Grand Total"), Value::Number(835.0));
        // Undo brings the group back.
        s.execute("edit.undo", json!({})).unwrap();
        assert_eq!(pt(&s).group_fields.len(), 1);
    }

    #[test]
    fn groups_of_a_number_field_number_on_and_survive_partial_ungrouping() {
        let mut s = sales();
        s.execute("insert.pivotTable", json!({"rows": ["Units"], "values": ["Sales"]})).unwrap();
        s.execute("pivot.groupSelection", json!({"field": "Units", "items": ["1", "2"]})).unwrap();
        let r = s.execute("pivot.groupSelection", json!({"field": "Units", "items": ["3", "4"]})).unwrap();
        assert_eq!(r["group"], "Group2");
        let p = pt(&s);
        assert_eq!(p.group_fields.len(), 1, "one grouped field holds both groups");
        assert_eq!(amount(&s, "Group1"), Value::Number(45.0));
        assert_eq!(amount(&s, "Group2"), Value::Number(105.0));
        // Regrouping an item moves it out of its old group.
        s.execute("pivot.groupSelection", json!({"field": "Units", "items": ["2", "3"]})).unwrap();
        let p = pt(&s);
        assert_eq!(p.group_fields[0].groups.iter().map(|g| g.items.len()).collect::<Vec<_>>(), [1, 1, 2]);
        s.execute("pivot.ungroup", json!({"field": "Units2", "items": ["Group1"]})).unwrap();
        assert_eq!(pt(&s).group_fields[0].groups.len(), 2, "the field stays while it has groups");
        s.execute("pivot.ungroup", json!({"field": "Units2"})).unwrap();
        assert!(pt(&s).group_fields.is_empty());
        assert!(s.execute("pivot.ungroup", json!({"field": "Units"})).is_err(), "Units isn't grouped");
    }

    #[test]
    fn date_and_time_grouping_like_the_grouping_dialog() {
        let mut s = sales();
        s.execute("insert.pivotTable", json!({"rows": ["Date"], "values": ["Sales"]})).unwrap();
        // Months, quarters and years: one field per level, coarsest first.
        s.execute("pivot.group", json!({"field": "Date", "by": ["months", "years", "quarters"]})).unwrap();
        let levels: Vec<PivotDateGroup> = pt(&s).rows.iter().map(|f| f.date_group).collect();
        assert_eq!(levels, [PivotDateGroup::Years, PivotDateGroup::Quarters, PivotDateGroup::Months]);
        let labels: Vec<String> = report(&s).into_iter().map(|(l, _)| l).collect();
        assert_eq!(labels[1..5], ["2026", "Qtr1", "Jan", "Feb"]);
        assert_eq!(amount(&s, "Qtr1"), Value::Number(270.0));
        // Seven-day spans from the first date; the last is cut at the day after the last date.
        s.execute("pivot.group", json!({"field": "Date", "by": "days", "days": 7})).unwrap();
        let labels: Vec<String> = report(&s).into_iter().map(|(l, _)| l).collect();
        assert_eq!(labels.get(1).map(String::as_str), Some("1/15/2026 - 1/21/2026"));
        assert!(labels.iter().any(|l| l == "12/10/2026 - 12/13/2026"), "{labels:?}");
        // Starting at / Ending at: dates outside go to one item each.
        s.execute("pivot.group", json!({"field": "Date", "by": "months", "start": "1/1/2026", "end": "6/30/2026"})).unwrap();
        assert_eq!(amount(&s, ">6/30/2026"), Value::Number(405.0));
        assert_eq!(amount(&s, "Jun"), Value::Number(60.0));
        assert_eq!(pt(&s).rows[0].date_range.and_then(|r| r.end), Some(46203.0));
        // Number of days only with Days alone; bad bounds and levels are refused.
        assert!(s.execute("pivot.group", json!({"field": "Date", "by": ["days", "months"], "days": 7})).is_err());
        assert!(s.execute("pivot.group", json!({"field": "Date", "by": "months", "start": "6/1/2026", "end": "1/1/2026"})).is_err());
        assert!(s.execute("pivot.group", json!({"field": "Date", "by": "weeks"})).is_err());
        assert!(s.execute("pivot.group", json!({"field": "Date", "by": "days", "days": 0})).is_err());
        // Ungroup leaves one plain field.
        s.execute("pivot.ungroup", json!({"field": "Date"})).unwrap();
        let p = pt(&s);
        assert_eq!(p.rows.len(), 1);
        assert_eq!((p.rows[0].date_group, p.rows[0].date_range), (PivotDateGroup::None, None));
        // Without parameters, Group Field opens the Grouping dialog for the field at the active cell.
        let first = pt(&s).last_range.unwrap().start;
        s.execute("selection.set", json!({"range": CellRef::new(first.row + 1, first.col).a1()})).unwrap();
        s.ui_requests.clear();
        s.execute("pivot.group", json!({})).unwrap();
        match s.ui_requests.last() {
            Some(crate::UiRequest::Dialog(name, p)) => {
                assert_eq!(name, "pivotGrouping");
                assert_eq!((p["kind"].as_str(), p["field"].as_str()), (Some("date"), Some("Date")));
            }
            other => panic!("expected the Grouping dialog, got {other:?}"),
        }
    }

    #[test]
    fn time_levels_group_hours_minutes_and_seconds() {
        let mut s = Session::new();
        s.new_workbook();
        let rows = json!([["When", "N"], ["1/15/2026 8:15:30", 1], ["1/16/2026 8:45:10", 2], ["1/16/2026 13:05:00", 4], ["1/17/2026 0:30:00", 8]]);
        s.execute("range.setValues", json!({"range": "A1", "values": rows})).unwrap();
        s.execute("insert.pivotTable", json!({"source": "Sheet1!A1:B5", "rows": ["When"], "values": ["N"]})).unwrap();
        s.execute("pivot.group", json!({"field": "When", "by": "hours"})).unwrap();
        assert_eq!(amount(&s, "8 AM"), Value::Number(3.0));
        assert_eq!(amount(&s, "1 PM"), Value::Number(4.0));
        assert_eq!(amount(&s, "12 AM"), Value::Number(8.0));
        s.execute("pivot.group", json!({"field": "When", "by": "minutes"})).unwrap();
        assert_eq!(amount(&s, ":15"), Value::Number(1.0));
        assert_eq!(amount(&s, ":05"), Value::Number(4.0));
        s.execute("pivot.group", json!({"field": "When", "by": "seconds"})).unwrap();
        assert_eq!(amount(&s, ":00"), Value::Number(12.0));
        assert_eq!(amount(&s, ":30"), Value::Number(1.0));
        s.execute("pivot.group", json!({"field": "When", "by": ["days", "hours"]})).unwrap();
        let labels: Vec<String> = report(&s).into_iter().map(|(l, _)| l).collect();
        assert_eq!(labels[1..4], ["15-Jan", "8 AM", "16-Jan"]);
    }

    #[test]
    fn calculated_items_add_up_like_excel() {
        let mut s = sales();
        s.execute("insert.pivotTable", json!({"rows": ["Region"], "columns": ["Product"], "values": ["Sales"]})).unwrap();
        let r = s.execute("pivot.calculatedItem", json!({"field": "Region", "name": "EastWest", "formula": "=East+West"})).unwrap();
        assert_eq!(r["calculatedItems"].as_array().map(Vec::len), Some(1));
        let ew = row_of(&s, "EastWest");
        let cell = |s: &Session, row: CellRef, k: u32| val(s, CellRef::new(row.row, row.col + k));
        // Apples, Pears, Plums, Grand Total — the totals include the calculated item, as in Excel.
        assert_eq!([1, 2, 3, 4].map(|k| cell(&s, ew, k)), [330.0, 240.0, 130.0, 700.0].map(Value::Number));
        let gt = row_of(&s, "Grand Total");
        assert_eq!([1, 2, 3, 4].map(|k| cell(&s, gt, k)), [750.0, 510.0, 275.0, 1535.0].map(Value::Number));
        s.execute("pivot.calculatedItem", json!({"field": "Region", "name": "EastPlus", "formula": "= Region[East]*1.1"})).unwrap();
        let ep = row_of(&s, "EastPlus");
        let got: Vec<f64> = [1, 2, 3, 4].iter().map(|k| cell(&s, ep, *k).as_f64().unwrap()).collect();
        for (a, b) in got.iter().zip([176.0, 214.5, 66.0, 456.5]) {
            assert!((a - b).abs() < 1e-9, "{got:?}");
        }
        // Changing a formula, then deleting the item.
        s.execute("pivot.calculatedItem", json!({"field": "Region", "name": "EastPlus", "formula": "=East*2"})).unwrap();
        assert_eq!(cell(&s, row_of(&s, "EastPlus"), 4), Value::Number(830.0));
        s.execute("pivot.calculatedItem", json!({"field": "Region", "name": "EastPlus", "delete": true})).unwrap();
        assert!(report(&s).iter().all(|(l, _)| l != "EastPlus"));
        // A selection of the field's items without it hides it.
        s.execute("pivot.filter", json!({"field": "Region", "selected": ["East", "West"]})).unwrap();
        assert!(report(&s).iter().all(|(l, _)| l != "EastWest"));
        s.execute("pivot.filter", json!({"field": "Region", "selected": null})).unwrap();
        // Under Count, the item counts its items' records.
        s.execute("pivot.valueSettings", json!({"field": "Sum of Sales", "func": "count"})).unwrap();
        assert_eq!(cell(&s, row_of(&s, "EastWest"), 4), Value::Number(9.0));
    }

    #[test]
    fn calculated_item_restrictions() {
        let mut s = sales();
        s.execute("insert.pivotTable", json!({"rows": ["Region"], "columns": ["Date"], "values": ["Sales"]})).unwrap();
        let add = |s: &mut Session, name: &str, formula: &str| {
            s.execute("pivot.calculatedItem", json!({"field": "Region", "name": name, "formula": formula}))
        };
        // A field grouped by dates anywhere in the PivotTable.
        s.execute("pivot.group", json!({"field": "Date", "by": "months"})).unwrap();
        assert!(err(add(&mut s, "X", "=East")).contains("grouped"));
        s.execute("pivot.group", json!({"field": "Date", "by": "none"})).unwrap();
        s.execute("pivot.removeField", json!({"field": "Date"})).unwrap();
        // Averages.
        s.execute("pivot.valueSettings", json!({"field": "Sum of Sales", "func": "average"})).unwrap();
        assert!(err(add(&mut s, "X", "=East")).contains("Averages"));
        s.execute("pivot.valueSettings", json!({"field": "Average of Sales", "func": "sum"})).unwrap();
        // Formula problems and names.
        assert!(err(add(&mut s, "X", "=Foo+1")).contains("item name was not found"));
        assert!(err(add(&mut s, "X", "=A1*2")).contains("not supported in PivotTable formulas"));
        assert!(err(add(&mut s, "X", "=Product[Apples]")).contains("only use items"));
        assert!(err(add(&mut s, "east", "=West")).contains("already has an item"));
        assert!(s.execute("pivot.calculatedItem", json!({"field": "Units", "name": "X", "formula": "=1"})).is_err(), "Units isn't in the layout");
        add(&mut s, "EW", "=East+West").unwrap();
        // While it has calculated items: no averages, no field twice in Values, no grouping, and
        // the field stays in Rows or Columns.
        assert!(err(s.execute("pivot.valueSettings", json!({"field": "Sum of Sales", "func": "stdDev"}))).contains("Averages"));
        assert!(err(s.execute("pivot.addField", json!({"field": "Sales", "area": "values"}))).contains("Multiple data fields"));
        assert!(err(s.execute("pivot.addField", json!({"field": "Units", "area": "values", "func": "average"}))).contains("Averages"));
        s.execute("pivot.addField", json!({"field": "Units", "area": "values"})).unwrap();
        s.execute("pivot.addField", json!({"field": "Date", "area": "columns"})).unwrap();
        assert!(err(s.execute("pivot.group", json!({"field": "Date", "by": "years"}))).contains("calculated items"));
        assert!(err(s.execute("pivot.moveField", json!({"field": "Region", "from": "rows", "to": "filters"}))).contains("Rows or Columns"));
        // Group Selection still works on a field with calculated items, as in Excel.
        s.execute("pivot.groupSelection", json!({"field": "Region", "items": ["East", "North"]})).unwrap();
        // Without a name: the Insert Calculated Item dialog for the field at the active cell.
        s.ui_requests.clear();
        let c = row_of(&s, "East");
        s.execute("selection.set", json!({"range": c.a1()})).unwrap();
        s.execute("pivot.calculatedItem", json!({})).unwrap();
        match s.ui_requests.last() {
            Some(crate::UiRequest::Dialog(name, p)) => {
                assert_eq!((name.as_str(), p["name"].as_str()), ("pivotCalculatedItem", Some("Formula1")));
                assert!(p["items"].as_array().is_some_and(|a| a.iter().any(|x| x == "EW")), "{p}");
            }
            other => panic!("expected the dialog, got {other:?}"),
        }
    }

    #[test]
    fn groups_calculated_items_and_timelines_survive_an_xlsx_round_trip() {
        let mut s = sales();
        s.execute("insert.pivotTable", json!({"rows": ["Region"], "columns": ["Product"], "values": ["Sales"]})).unwrap();
        s.execute("pivot.groupSelection", json!({"field": "Region", "items": ["East", "North"]})).unwrap();
        s.execute("pivot.calculatedItem", json!({"field": "Product", "name": "AP", "formula": "=Apples+Pears"})).unwrap();
        s.execute("insert.timeline", json!({"field": "Date"})).unwrap();
        s.execute("timeline.select", json!({"start": "3/1/2026", "end": "8/31/2026"})).unwrap();
        let before = report(&s);
        let p = pt(&s);
        let b64 = s.execute("file.saveBytes", json!({"format": "xlsx"})).unwrap()["base64"].as_str().unwrap().to_string();
        s.execute("file.open", json!({"name": "depth.xlsx", "base64": b64})).unwrap();
        s.execute("sheet.activate", json!({"sheet": 0})).unwrap();
        let q = pt(&s);
        assert_eq!((&q.group_fields, &q.calculated_items, &q.rows), (&p.group_fields, &p.calculated_items, &p.rows));
        let tl = s.doc().unwrap().wb.sheets.iter().flat_map(|sh| sh.timelines.iter()).next().cloned().unwrap();
        assert_eq!(tl.pivots, vec![q.id]);
        assert_eq!(tl.selection, Some((46082.0, 46265.0)));
        // Refreshed from the file, the report is the same.
        s.execute("pivot.refresh", json!({})).unwrap();
        assert_eq!(report(&s), before);
    }

    #[test]
    fn solve_order_decides_where_calculated_items_meet() {
        let mut s = sales();
        s.execute("insert.pivotTable", json!({"rows": ["Region"], "columns": ["Product"], "values": ["Sales"]})).unwrap();
        s.execute("pivot.calculatedItem", json!({"field": "Region", "name": "EW", "formula": "=East+West"})).unwrap();
        s.execute("pivot.calculatedItem", json!({"field": "Product", "name": "AP", "formula": "=Apples+1"})).unwrap();
        let meet = |s: &Session| {
            let row = row_of(s, "EW");
            let r = pt(s).last_range.unwrap();
            let col = (r.start.col..=r.end.col).find(|c| val(s, CellRef::new(r.start.row + 1, *c)).display() == "AP").unwrap();
            val(s, CellRef::new(row.row, col))
        };
        // AP last: Apples of EW (330) + 1.
        assert_eq!(meet(&s), Value::Number(331.0));
        s.execute("pivot.solveOrder", json!({"order": ["AP", "EW"]})).unwrap();
        // EW last: AP of East (161) + AP of West (171).
        assert_eq!(meet(&s), Value::Number(332.0));
        s.execute("pivot.solveOrder", json!({"item": "AP", "move": "down"})).unwrap();
        assert_eq!(pt(&s).calculated_items.iter().map(|c| c.name.as_str()).collect::<Vec<_>>(), ["EW", "AP"]);
        assert!(s.execute("pivot.solveOrder", json!({"order": ["AP"]})).is_err());
        // List Formulas: a sheet after the PivotTable's with both sections.
        s.execute("pivot.calculatedField", json!({"name": "Bonus", "formula": "=Sales*0.1", "values": false})).unwrap();
        let pivot_sheet = s.doc().unwrap().wb.active_sheet;
        let r = s.execute("pivot.listFormulas", json!({})).unwrap();
        let d = s.doc().unwrap();
        assert_eq!(d.wb.active_sheet, pivot_sheet + 1);
        let sh = d.wb.active().unwrap();
        assert_eq!(sh.name, r["sheet"].as_str().unwrap());
        let t = |a: &str| sh.value(CellRef::parse(a).unwrap()).display();
        assert_eq!([t("A1"), t("A2"), t("B2"), t("C2")], ["Calculated Field", "Solve Order", "Field", "Formula"]);
        assert_eq!([t("A3"), t("B3"), t("C3")], ["1", "Bonus", "=Sales*0.1"]);
        assert_eq!(
            [t("A5"), t("B6"), t("A7"), t("B7"), t("C7"), t("B8"), t("C8")],
            ["Calculated Item", "Item", "1", "EW", "=East+West", "AP", "=Apples+1"]
        );
        assert_eq!(t("A11"), "Note:");
        assert!(d.wb.styles.get(sh.style_id(CellRef::parse("A1").unwrap())).font.italic);
    }
}
