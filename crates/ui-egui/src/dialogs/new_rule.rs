//! Home › Conditional Formatting › New Rule…: Excel's New Formatting Rule dialog. A rule type
//! from the list, then that type's description fields; OK runs `home.conditionalFormat` with the
//! `rule` that [`params`] builds from the dialog's values, so the control channel's
//! `ui.dialog.set` / `ui.dialog.confirm` drive it the same way.

use egui::{Color32, RichText, Ui, vec2};
use gridcraft_engine::model::{Color, PatternType, Theme};
use serde_json::{Map, Value as Json, json};

use super::{Dialog, ok_cancel};

/// Excel's rule types, in its order.
const KINDS: [(&str, &str); 6] = [
    ("values", "Format all cells based on their values"),
    ("contains", "Format only cells that contain"),
    ("top", "Format only top or bottom ranked values"),
    ("average", "Format only values that are above or below average"),
    ("unique", "Format only unique or duplicate values"),
    ("formula", "Use a formula to determine which cells to format"),
];

const SCALES: [(&str, &str); 4] = [("2color", "2-Color Scale"), ("3color", "3-Color Scale"), ("dataBar", "Data Bar"), ("iconSet", "Icon Sets")];

const CONTAINS: [(&str, &str); 7] = [
    ("cellIs", "Cell Value"),
    ("text", "Specific Text"),
    ("dates", "Dates Occurring"),
    ("blanks", "Blanks"),
    ("noBlanks", "No Blanks"),
    ("errors", "Errors"),
    ("noErrors", "No Errors"),
];

const OPERATORS: [(&str, &str); 8] = [
    ("between", "between"),
    ("notBetween", "not between"),
    ("equal", "equal to"),
    ("notEqual", "not equal to"),
    ("greater", "greater than"),
    ("less", "less than"),
    ("greaterOrEqual", "greater than or equal to"),
    ("lessOrEqual", "less than or equal to"),
];

const TEXT_OPERATORS: [(&str, &str); 4] =
    [("containsText", "containing"), ("notContainsText", "not containing"), ("beginsWith", "beginning with"), ("endsWith", "ending with")];

pub(super) const PERIODS: [(&str, &str); 10] = [
    ("yesterday", "Yesterday"),
    ("today", "Today"),
    ("tomorrow", "Tomorrow"),
    ("last7Days", "In the last 7 days"),
    ("lastWeek", "Last week"),
    ("thisWeek", "This week"),
    ("nextWeek", "Next week"),
    ("lastMonth", "Last month"),
    ("thisMonth", "This month"),
    ("nextMonth", "Next month"),
];

const AVERAGES: [(&str, &str); 10] = [
    ("above", "above"),
    ("below", "below"),
    ("equalAbove", "equal or above"),
    ("equalBelow", "equal or below"),
    ("above1", "1 std dev above"),
    ("below1", "1 std dev below"),
    ("above2", "2 std dev above"),
    ("below2", "2 std dev below"),
    ("above3", "3 std dev above"),
    ("below3", "3 std dev below"),
];

/// The icon sets a workbook file can hold in its main conditional-format part (Excel 2010's
/// 3 Stars, 3 Triangles and 5 Boxes live only in its extension).
const ICON_SETS: [(&str, &str); 17] = [
    ("3Arrows", "3 Arrows (Colored)"),
    ("3ArrowsGray", "3 Arrows (Gray)"),
    ("4Arrows", "4 Arrows (Colored)"),
    ("4ArrowsGray", "4 Arrows (Gray)"),
    ("5Arrows", "5 Arrows (Colored)"),
    ("5ArrowsGray", "5 Arrows (Gray)"),
    ("3TrafficLights1", "3 Traffic Lights (Unrimmed)"),
    ("3TrafficLights2", "3 Traffic Lights (Rimmed)"),
    ("3Signs", "3 Signs"),
    ("4TrafficLights", "4 Traffic Lights"),
    ("4RedToBlack", "Red To Black"),
    ("3Symbols", "3 Symbols (Circled)"),
    ("3Symbols2", "3 Symbols (Uncircled)"),
    ("3Flags", "3 Flags"),
    ("4Rating", "4 Ratings"),
    ("5Quarters", "5 Quarters"),
    ("5Rating", "5 Ratings"),
];

const THRESHOLD_TYPES: [(&str, &str); 4] = [("num", "Number"), ("percent", "Percent"), ("percentile", "Percentile"), ("formula", "Formula")];

/// The quick formats a rule applies (`preset` of `home.conditionalFormat`).
pub(super) const PRESETS: [(&str, &str); 6] = [
    ("lightRedFillDarkRedText", "Light Red Fill with Dark Red Text"),
    ("yellowFillDarkYellowText", "Yellow Fill with Dark Yellow Text"),
    ("greenFillDarkGreenText", "Green Fill with Dark Green Text"),
    ("lightRedFill", "Light Red Fill"),
    ("redText", "Red Text"),
    ("redBorder", "Red Border"),
];

/// Excel's default icon thresholds: even percent steps, lowest cut first.
fn default_thresholds(icons: usize) -> Json {
    let n = icons.max(2) as f64;
    Json::Array((1..icons).map(|i| json!({"type": "percent", "value": (i as f64 * 100.0 / n).round(), "operator": ">="})).collect())
}

/// The dialog's starting values. `params` may name the rule type (`kind`) and, for "Format all
/// cells based on their values", the format style (`style`), as each submenu's More Rules… does.
pub(super) fn defaults(params: &Json) -> Json {
    let pick = |key: &str, options: &[(&str, &str)], fallback: &'static str| -> String {
        params.get(key).and_then(Json::as_str).filter(|v| options.iter().any(|(o, _)| o == v)).unwrap_or(fallback).to_string()
    };
    json!({
        "kind": pick("kind", &KINDS, "values"),
        "scale": pick("style", &SCALES, "2color"),
        "min": "#F8696B", "mid": "#FFEB84", "max": "#63BE7B",
        "barColor": "#638EC6", "barFill": "gradient",
        "set": "3TrafficLights1", "reverse": false, "thresholds": default_thresholds(3),
        "contains": "cellIs", "operator": "between", "value": "", "value2": "",
        "textOp": "containsText", "text": "", "period": "today",
        "bottom": false, "rank": 10, "percent": false,
        "average": "above",
        "dupe": "duplicate",
        "formula": "",
        "preset": "lightRedFillDarkRedText",
    })
}

/// The `home.conditionalFormat` parameters for the dialog's values.
pub(super) fn params(values: &Map<String, Json>) -> Json {
    let s = |k: &str| values.get(k).and_then(Json::as_str).unwrap_or("").to_string();
    let flag = |k: &str| values.get(k).and_then(Json::as_bool).unwrap_or(false);
    let styled = |mut rule: Json| {
        rule["preset"] = json!(s("preset"));
        rule
    };
    let rule = match s("kind").as_str() {
        "values" => match s("scale").as_str() {
            "3color" => json!({"type": "colorScale", "colors": [s("min"), s("mid"), s("max")]}),
            "dataBar" => json!({"type": "dataBar", "color": s("barColor"), "gradient": s("barFill") != "solid"}),
            "iconSet" => json!({
                "type": "iconSet",
                "set": s("set"),
                "reverse": flag("reverse"),
                "thresholds": values.get("thresholds").cloned().unwrap_or_else(|| json!([])),
            }),
            _ => json!({"type": "colorScale", "colors": [s("min"), s("max")]}),
        },
        "contains" => match s("contains").as_str() {
            "text" => styled(json!({"type": s("textOp"), "text": s("text")})),
            "dates" => styled(json!({"type": "timePeriod", "period": s("period")})),
            k @ ("blanks" | "noBlanks" | "errors" | "noErrors") => styled(json!({"type": k})),
            _ => {
                let op = s("operator");
                let mut rule = json!({"type": "cellIs", "operator": op, "value": values.get("value").cloned().unwrap_or(Json::Null)});
                if matches!(op.as_str(), "between" | "notBetween") {
                    rule["value2"] = values.get("value2").cloned().unwrap_or(Json::Null);
                }
                styled(rule)
            }
        },
        "top" => styled(json!({
            "type": "top10",
            "bottom": flag("bottom"),
            "rank": values.get("rank").and_then(Json::as_u64).unwrap_or(10),
            "percent": flag("percent"),
        })),
        "average" => {
            let a = s("average");
            let std_dev = a.chars().last().and_then(|c| c.to_digit(10)).unwrap_or(0);
            styled(json!({"type": "aboveAverage", "below": a.contains("elow"), "equal": a.starts_with("equal"), "stdDev": std_dev}))
        }
        "unique" => styled(json!({"type": if s("dupe") == "unique" { "unique" } else { "duplicate" }})),
        _ => styled(json!({"type": "expression", "formula": s("formula")})),
    };
    json!({"rule": rule})
}

fn text(d: &Dialog, key: &str) -> String {
    d.values
        .get(key)
        .map(|v| v.as_str().map(str::to_string).unwrap_or_else(|| if v.is_null() { String::new() } else { v.to_string() }))
        .unwrap_or_default()
}

/// A drop-down over `options` (value, label) editing `key`.
fn combo(ui: &mut Ui, d: &mut Dialog, key: &str, options: &[(&str, &str)], width: f32) {
    let lang = crate::i18n::current(ui.ctx());
    let cur = text(d, key);
    let shown = options.iter().find(|(v, _)| *v == cur).map(|(_, l)| lang.tr(l)).unwrap_or_default();
    egui::ComboBox::from_id_salt(("newFormattingRule", key)).selected_text(shown).width(width).show_ui(ui, |ui| {
        for (v, l) in options {
            if ui.selectable_label(cur == *v, lang.tr(l)).clicked() {
                d.values.insert(key.into(), json!(v));
            }
        }
    });
}

fn line(ui: &mut Ui, d: &mut Dialog, key: &str, width: f32) {
    let mut s = text(d, key);
    if ui.add(egui::TextEdit::singleline(&mut s).desired_width(width)).changed() {
        d.values.insert(key.into(), json!(s));
    }
}

fn check(ui: &mut Ui, d: &mut Dialog, key: &str, label: &str) {
    let mut b = d.values.get(key).and_then(Json::as_bool).unwrap_or(false);
    if ui.checkbox(&mut b, crate::i18n::current(ui.ctx()).tr(label)).changed() {
        d.values.insert(key.into(), json!(b));
    }
}

fn hex(c: [u8; 3]) -> String {
    format!("#{:02X}{:02X}{:02X}", c[0], c[1], c[2])
}

fn parse_hex(s: &str) -> Option<[u8; 3]> {
    let s = s.trim_start_matches('#');
    let n = u32::from_str_radix(s.get(..6)?, 16).ok()?;
    Some([(n >> 16) as u8, (n >> 8) as u8, n as u8])
}

fn color(ui: &mut Ui, d: &mut Dialog, key: &str) {
    let mut c = parse_hex(&text(d, key)).unwrap_or([255, 255, 255]);
    if egui::color_picker::color_edit_button_srgb(ui, &mut c).changed() {
        d.values.insert(key.into(), json!(hex(c)));
    }
}

/// "Format all cells based on their values": colour scales, data bars, icon sets.
fn values_rule(ui: &mut Ui, d: &mut Dialog) {
    let lang = crate::i18n::current(ui.ctx());
    ui.horizontal(|ui| {
        ui.label(lang.tr("Format Style:"));
        combo(ui, d, "scale", &SCALES, 160.0);
    });
    match text(d, "scale").as_str() {
        scale @ ("2color" | "3color") => {
            let keys: &[(&str, &str)] = if scale == "3color" {
                &[("min", "Minimum"), ("mid", "Midpoint"), ("max", "Maximum")]
            } else {
                &[("min", "Minimum"), ("max", "Maximum")]
            };
            egui::Grid::new("newFormattingRule.scale").spacing(vec2(16.0, 4.0)).show(ui, |ui| {
                for (key, label) in keys {
                    ui.label(lang.tr(label));
                    color(ui, d, key);
                    ui.end_row();
                }
            });
            let note = if scale == "3color" {
                "Minimum: lowest value. Midpoint: 50th percentile. Maximum: highest value."
            } else {
                "Minimum: lowest value. Maximum: highest value."
            };
            ui.label(RichText::new(lang.tr(note)).small());
        }
        "dataBar" => {
            ui.horizontal(|ui| {
                ui.label(lang.tr("Fill:"));
                combo(ui, d, "barFill", &[("gradient", "Gradient Fill"), ("solid", "Solid Fill")], 140.0);
                ui.label(lang.tr("Color:"));
                color(ui, d, "barColor");
            });
        }
        _ => icon_rule(ui, d),
    }
}

/// Icon sets: the style, reverse order and each icon's threshold, top icon first as in Excel.
fn icon_rule(ui: &mut Ui, d: &mut Dialog) {
    let lang = crate::i18n::current(ui.ctx());
    ui.horizontal(|ui| {
        ui.label(lang.tr("Icon Style:"));
        combo(ui, d, "set", &ICON_SETS, 200.0);
        check(ui, d, "reverse", "Reverse Icon Order");
    });
    let set = text(d, "set");
    let icons = gridcraft_engine::cf::icon_count(&set);
    // One object per cut point; anything else (the control channel can set any value) starts
    // over from the defaults.
    let objects =
        |v: &Json| -> Vec<Map<String, Json>> { v.as_array().map(|a| a.iter().filter_map(|t| t.as_object().cloned()).collect()).unwrap_or_default() };
    let mut list = d.values.get("thresholds").map(objects).unwrap_or_default();
    if list.len() + 1 != icons {
        list = objects(&default_thresholds(icons));
    }
    let reverse = d.values.get("reverse").and_then(Json::as_bool).unwrap_or(false);
    let icon_at = |slot: usize| if reverse { icons - 1 - slot } else { slot };
    let paint_icon = |ui: &mut Ui, slot: usize| {
        let (r, _) = ui.allocate_exact_size(vec2(14.0, 14.0), egui::Sense::hover());
        crate::grid::paint_cf_icon(ui.painter(), r, &set, icon_at(slot));
    };
    egui::Grid::new("newFormattingRule.icons").spacing(vec2(8.0, 4.0)).show(ui, |ui| {
        for i in (0..list.len()).rev() {
            let Some(t) = list.get_mut(i) else { continue };
            paint_icon(ui, i + 1);
            ui.label(lang.tr("when value is"));
            let op = t.get("operator").and_then(Json::as_str).unwrap_or(">=").to_string();
            egui::ComboBox::from_id_salt(("newFormattingRule.op", i)).selected_text(&op).width(48.0).show_ui(ui, |ui| {
                for o in [">=", ">"] {
                    if ui.selectable_label(op == o, o).clicked() {
                        t.insert("operator".into(), json!(o));
                    }
                }
            });
            let ty = t.get("type").and_then(Json::as_str).unwrap_or("percent").to_string();
            if ty == "formula" {
                let mut f = t.get("value").and_then(Json::as_str).unwrap_or("").to_string();
                if ui.add(egui::TextEdit::singleline(&mut f).desired_width(90.0)).changed() {
                    t.insert("value".into(), json!(f));
                }
            } else {
                let mut n = t.get("value").and_then(Json::as_f64).unwrap_or(0.0);
                if ui.add(egui::DragValue::new(&mut n).speed(1.0)).changed() {
                    t.insert("value".into(), json!(n));
                }
            }
            let shown = THRESHOLD_TYPES.iter().find(|(v, _)| *v == ty).map(|(_, l)| lang.tr(l)).unwrap_or_default();
            egui::ComboBox::from_id_salt(("newFormattingRule.type", i)).selected_text(shown).width(90.0).show_ui(ui, |ui| {
                for (v, l) in THRESHOLD_TYPES {
                    if ui.selectable_label(ty == v, lang.tr(l)).clicked() {
                        t.insert("type".into(), json!(v));
                        // A formula threshold is text; the others are numbers.
                        t.insert("value".into(), if v == "formula" { json!("") } else { json!(0) });
                    }
                }
            });
            ui.end_row();
        }
        paint_icon(ui, 0);
        ui.label(lang.tr("otherwise"));
        ui.end_row();
    });
    d.values.insert("thresholds".into(), Json::Array(list.into_iter().map(Json::Object).collect()));
}

/// "Format only cells that contain".
fn contains_rule(ui: &mut Ui, d: &mut Dialog) {
    let lang = crate::i18n::current(ui.ctx());
    ui.label(lang.tr("Format only cells with:"));
    ui.horizontal(|ui| {
        combo(ui, d, "contains", &CONTAINS, 120.0);
        match text(d, "contains").as_str() {
            "cellIs" => {
                combo(ui, d, "operator", &OPERATORS, 150.0);
                line(ui, d, "value", 80.0);
                if matches!(text(d, "operator").as_str(), "between" | "notBetween") {
                    ui.label(lang.tr("and"));
                    line(ui, d, "value2", 80.0);
                }
            }
            "text" => {
                combo(ui, d, "textOp", &TEXT_OPERATORS, 120.0);
                line(ui, d, "text", 140.0);
            }
            "dates" => combo(ui, d, "period", &PERIODS, 150.0),
            _ => {}
        }
    });
}

/// The format a rule applies, with a preview as in Excel.
fn format_row(ui: &mut Ui, d: &mut Dialog) {
    let lang = crate::i18n::current(ui.ctx());
    ui.add_space(6.0);
    ui.horizontal(|ui| {
        ui.label(lang.tr("Format with:"));
        combo(ui, d, "preset", &PRESETS, 240.0);
    });
    let style = gridcraft_engine::cmd::insert::preset_style(&text(d, "preset"));
    let theme = Theme::default();
    let rgb = |c: Color| c.resolve(&theme).map(|[r, g, b]| Color32::from_rgb(r, g, b));
    let fill = if style.fill.pattern == PatternType::None { None } else { rgb(style.fill.fg) };
    let ink = rgb(style.font.color).unwrap_or(Color32::BLACK);
    let border = if style.borders.top.is_none() { None } else { rgb(style.borders.top.color) };
    ui.horizontal(|ui| {
        ui.label(lang.tr("Preview:"));
        let (r, _) = ui.allocate_exact_size(vec2(160.0, 22.0), egui::Sense::hover());
        let p = ui.painter();
        p.rect_filled(r, 0.0, fill.unwrap_or(Color32::WHITE));
        p.rect_stroke(r, 0.0, egui::Stroke::new(1.0, border.unwrap_or(Color32::from_gray(200))), egui::StrokeKind::Inside);
        p.text(r.center(), egui::Align2::CENTER_CENTER, "AaBbCcYyZz", egui::FontId::proportional(13.0), ink);
    });
}

pub(super) fn show(ui: &mut Ui, d: &mut Dialog, confirm: &mut bool, open: &mut bool) {
    let lang = crate::i18n::current(ui.ctx());
    ui.label(RichText::new(lang.tr("Select a Rule Type:")).strong());
    let kind = text(d, "kind");
    egui::Frame::group(ui.style()).show(ui, |ui| {
        ui.set_min_width(ui.available_width());
        for (v, label) in KINDS {
            if ui.selectable_label(kind == v, format!("► {}", lang.tr(label))).clicked() {
                d.values.insert("kind".into(), json!(v));
            }
        }
    });
    ui.add_space(6.0);
    ui.label(RichText::new(lang.tr("Edit the Rule Description:")).strong());
    egui::Frame::group(ui.style()).show(ui, |ui| {
        ui.set_min_width(ui.available_width());
        match text(d, "kind").as_str() {
            "values" => values_rule(ui, d),
            "contains" => {
                contains_rule(ui, d);
                format_row(ui, d);
            }
            "top" => {
                ui.label(lang.tr("Format values that rank in the:"));
                ui.horizontal(|ui| {
                    let bottom = d.values.get("bottom").and_then(Json::as_bool).unwrap_or(false);
                    egui::ComboBox::from_id_salt(("newFormattingRule", "topBottom"))
                        .selected_text(lang.tr(if bottom { "Bottom" } else { "Top" }))
                        .width(90.0)
                        .show_ui(ui, |ui| {
                            for (b, l) in [(false, "Top"), (true, "Bottom")] {
                                if ui.selectable_label(bottom == b, lang.tr(l)).clicked() {
                                    d.values.insert("bottom".into(), json!(b));
                                }
                            }
                        });
                    let mut rank = d.values.get("rank").and_then(Json::as_u64).unwrap_or(10).min(1000) as u32;
                    if ui.add(egui::DragValue::new(&mut rank).range(1..=1000)).changed() {
                        d.values.insert("rank".into(), json!(rank));
                    }
                    check(ui, d, "percent", "% of the selected range");
                });
                format_row(ui, d);
            }
            "average" => {
                ui.horizontal(|ui| {
                    ui.label(lang.tr("Format values that are:"));
                    combo(ui, d, "average", &AVERAGES, 150.0);
                    ui.label(lang.tr("the average for the selected range"));
                });
                format_row(ui, d);
            }
            "unique" => {
                ui.horizontal(|ui| {
                    ui.label(lang.tr("Format all:"));
                    combo(ui, d, "dupe", &[("duplicate", "duplicate"), ("unique", "unique")], 110.0);
                    ui.label(lang.tr("values in the selected range"));
                });
                format_row(ui, d);
            }
            _ => {
                ui.label(lang.tr("Format values where this formula is true:"));
                line(ui, d, "formula", ui.available_width() - 8.0);
                format_row(ui, d);
            }
        }
    });
    ok_cancel(ui, confirm, open);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rule(values: Json) -> Json {
        let mut v = defaults(&json!({})).as_object().cloned().unwrap_or_default();
        for (k, x) in values.as_object().cloned().unwrap_or_default() {
            v.insert(k, x);
        }
        params(&v)["rule"].clone()
    }

    #[test]
    fn each_rule_type_builds_its_rule() {
        assert_eq!(rule(json!({"kind": "values"}))["colors"], json!(["#F8696B", "#63BE7B"]));
        assert_eq!(rule(json!({"kind": "values", "scale": "dataBar", "barFill": "solid"}))["gradient"], json!(false));
        let icons = rule(json!({"kind": "values", "scale": "iconSet"}));
        assert_eq!((icons["type"].clone(), icons["thresholds"].as_array().map(Vec::len)), (json!("iconSet"), Some(2)));
        let between = rule(json!({"kind": "contains", "value": "2", "value2": "6"}));
        assert_eq!((between["operator"].clone(), between["value2"].clone()), (json!("between"), json!("6")));
        assert!(rule(json!({"kind": "contains", "operator": "greater", "value": "5"})).get("value2").is_none());
        assert_eq!(rule(json!({"kind": "contains", "contains": "text", "textOp": "endsWith", "text": "x"}))["type"], json!("endsWith"));
        assert_eq!(rule(json!({"kind": "contains", "contains": "errors"}))["type"], json!("errors"));
        assert_eq!(rule(json!({"kind": "top", "bottom": true, "rank": 5}))["rank"], json!(5));
        let avg = rule(json!({"kind": "average", "average": "below2"}));
        assert_eq!((avg["below"].clone(), avg["equal"].clone(), avg["stdDev"].clone()), (json!(true), json!(false), json!(2)));
        let avg = rule(json!({"kind": "average", "average": "equalAbove"}));
        assert_eq!((avg["below"].clone(), avg["equal"].clone(), avg["stdDev"].clone()), (json!(false), json!(true), json!(0)));
        assert_eq!(rule(json!({"kind": "unique", "dupe": "unique"}))["type"], json!("unique"));
        let f = rule(json!({"kind": "formula", "formula": "=A1>5", "preset": "redText"}));
        assert_eq!((f["formula"].clone(), f["preset"].clone()), (json!("=A1>5"), json!("redText")));
    }

    #[test]
    fn defaults_take_the_type_and_style_more_rules_asks_for() {
        let d = defaults(&json!({"kind": "values", "style": "iconSet"}));
        assert_eq!((d["kind"].clone(), d["scale"].clone()), (json!("values"), json!("iconSet")));
        // Unknown names fall back to the first type and style.
        let d = defaults(&json!({"kind": "bogus", "style": 7}));
        assert_eq!((d["kind"].clone(), d["scale"].clone()), (json!("values"), json!("2color")));
        assert_eq!(
            default_thresholds(5).as_array().map(|a| a.iter().map(|t| t["value"].clone()).collect::<Vec<_>>()),
            Some(vec![json!(20.0), json!(40.0), json!(60.0), json!(80.0)])
        );
    }

    #[test]
    fn colours_parse_and_print_as_hex() {
        assert_eq!(parse_hex("#638EC6"), Some([0x63, 0x8E, 0xC6]));
        assert_eq!(parse_hex("63be7b"), Some([0x63, 0xBE, 0x7B]));
        for bad in ["", "#", "#12", "#GGGGGG", "#ÄÄÄÄÄÄ"] {
            assert_eq!(parse_hex(bad), None, "{bad}");
        }
        assert_eq!(hex([0xF8, 0x69, 0x6B]), "#F8696B");
    }
}
