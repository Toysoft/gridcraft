//! Controls placed on worksheets (ActiveX controls in `xl/activeX`): read for display and for
//! macros from the `<controls>` element and parts the sheet keeps for saving
//! ([`crate::passthrough`]), which stay the source of truth in the file.

use gridcraft_model::{Sheet, SheetControl};
use gridcraft_vba::forms::{ControlKind, read_activex};

use crate::xml::El;

fn kind_name(k: ControlKind) -> &'static str {
    match k {
        ControlKind::Label => "Label",
        ControlKind::TextBox => "TextBox",
        ControlKind::ListBox => "ListBox",
        ControlKind::ComboBox => "ComboBox",
        ControlKind::CheckBox => "CheckBox",
        ControlKind::OptionButton => "OptionButton",
        ControlKind::ToggleButton => "ToggleButton",
        ControlKind::CommandButton => "CommandButton",
        ControlKind::SpinButton => "SpinButton",
        ControlKind::ScrollBar => "ScrollBar",
        ControlKind::Image => "Image",
        _ => "Control",
    }
}

fn controls_in<'a>(e: &'a El, out: &mut Vec<&'a El>) {
    for c in &e.children {
        if c.name == "control" && c.child("controlPr").is_some() {
            out.push(c);
        } else {
            controls_in(c, out);
        }
    }
}

/// The sheet's ActiveX controls (empty when it has none or they can't be read).
pub fn read(sheet: &Sheet) -> Vec<SheetControl> {
    let Some(p) = sheet.preserved.as_deref() else { return vec![] };
    let mut out = Vec::new();
    for el in p.elements.iter().filter(|e| e.slot == "controls") {
        let Ok(x) = crate::xml::parse(el.xml.as_bytes()) else { continue };
        let mut found = Vec::new();
        controls_in(&x, &mut found);
        for c in found {
            let name = c.attr("name").unwrap_or("").to_string();
            let Some(pr) = c.child("controlPr") else { continue };
            let Some(anchor) = pr.child("anchor").and_then(|a| crate::drawing::from_to_anchor(a, sheet)) else { continue };
            // control r:id → activeX part → its r:id → the persisted .bin.
            let design = c
                .attr("id")
                .and_then(|rid| p.rels.iter().find(|r| r.id == rid))
                .and_then(|r| p.parts.iter().find(|part| part.name == r.target))
                .and_then(|ax| {
                    let root = crate::xml::parse(&ax.data).ok()?;
                    let rid = root.attr("id")?;
                    let bin_name = &ax.rels.iter().find(|r| r.id == rid)?.target;
                    let bin = p.parts.iter().find(|part| &part.name == bin_name)?;
                    read_activex(&name, &bin.data, 1252)
                });
            let Some(d) = design else { continue };
            let text = |k: &str| pr.attr(k).map(str::trim).filter(|v| !v.is_empty() && !v.contains("#REF!")).map(str::to_string);
            out.push(SheetControl {
                name: name.clone(),
                kind: kind_name(d.kind).into(),
                anchor,
                linked_cell: text("linkedCell"),
                list_fill_range: text("listFillRange"),
                value: d.value.clone(),
                caption: d.caption.clone(),
                font: d.font.as_ref().map(|f| f.name.clone()),
                font_size: d.font.as_ref().map(|f| f.size),
                bold: d.font.as_ref().is_some_and(|f| f.bold),
                fore: d.fore,
                back: d.back,
                drop_list: d.drop_list,
                enabled: d.enabled,
            });
        }
    }
    out
}
