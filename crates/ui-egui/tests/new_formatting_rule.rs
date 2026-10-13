//! Home › Conditional Formatting › New Rule…: the New Formatting Rule dialog (#1165).

use egui::vec2;
use egui_kittest::kittest::Queryable;
use gridcraft_engine::Session;
use gridcraft_engine::model::{CfRule, CfValueKind};
use gridcraft_ui_egui::SheetApp;
use gridcraft_ui_egui::control::{Outcome, handle};
use serde_json::{Value as Json, json};

fn harness() -> egui_kittest::Harness<'static, SheetApp> {
    let mut session = Session::new();
    session.new_workbook();
    let mut h = egui_kittest::Harness::builder().with_size(vec2(1200.0, 900.0)).build_ui_state(
        |ui, app: &mut SheetApp| {
            app.logic(&ui.ctx().clone());
            app.ui(ui);
        },
        SheetApp::new(session, Default::default()),
    );
    h.run_steps(6);
    h
}

fn control(h: &mut egui_kittest::Harness<'static, SheetApp>, method: &str, params: Json) -> Json {
    let ctx = h.ctx.clone();
    match handle(h.state_mut(), &ctx, method, &params) {
        Outcome::Done(v) => v,
        Outcome::Screenshot { .. } => json!(null),
    }
}

fn rules(h: &egui_kittest::Harness<'static, SheetApp>) -> Vec<CfRule> {
    h.state().session.active().unwrap().wb.active().unwrap().cond_formats.iter().map(|c| c.rule.clone()).collect()
}

fn dialog(h: &egui_kittest::Harness<'static, SheetApp>) -> Option<String> {
    h.state().dialog.as_ref().map(|d| d.name().to_string())
}

#[test]
fn the_command_without_a_rule_opens_the_dialog() {
    // What Alt, H, L, N and Search Commands run; before, nothing opened.
    let mut h = harness();
    h.state_mut().run("home.conditionalFormat", json!({})).unwrap();
    h.run_steps(4);
    assert_eq!(dialog(&h).as_deref(), Some("newFormattingRule"));
    assert!(rules(&h).is_empty());
    h.state_mut().dialog = None;
    // A submenu's More Rules… opens it on that submenu's rule type.
    control(&mut h, "ui.dialog", json!({"name": "newFormattingRule", "kind": "top"}));
    assert_eq!(h.state().dialog.as_ref().and_then(|d| d.values.get("kind").cloned()), Some(json!("top")));
}

#[test]
fn picking_a_rule_type_and_pressing_ok_adds_the_rule() {
    let mut h = harness();
    h.state_mut().run("range.setValues", json!({"range": "A1", "values": [[1], [1], [2]]})).unwrap();
    h.state_mut().run("selection.set", json!({"range": "A1:A3"})).unwrap();
    h.state_mut().open_dialog("newFormattingRule", json!({}));
    h.run_steps(6);
    h.get_by_label("► Format only unique or duplicate values").click();
    h.run_steps(6);
    h.get_by_label("   OK   ").click();
    h.run_steps(6);
    assert_eq!(dialog(&h), None);
    assert!(matches!(rules(&h).as_slice(), [CfRule::Duplicate { .. }]), "{:?}", rules(&h));
}

/// Opens the dialog, sets `fields` and confirms, all over the control channel.
fn add(h: &mut egui_kittest::Harness<'static, SheetApp>, fields: Json) {
    control(h, "ui.dialog", json!({"name": "newFormattingRule"}));
    for (field, value) in fields.as_object().cloned().unwrap_or_default() {
        control(h, "ui.dialog.set", json!({"field": field, "value": value}));
    }
    let r = control(h, "ui.dialog.confirm", json!({}));
    assert_eq!(r["ok"], json!(true), "{r}");
}

#[test]
fn the_control_channel_sets_up_and_confirms_each_rule_type() {
    let mut h = harness();
    h.state_mut().run("selection.set", json!({"range": "B2:B9"})).unwrap();
    add(&mut h, json!({"kind": "average", "average": "above1"}));
    add(&mut h, json!({"kind": "top", "bottom": true, "rank": 3, "percent": true}));
    add(&mut h, json!({"kind": "contains", "contains": "text", "textOp": "beginsWith", "text": "ab"}));
    add(
        &mut h,
        json!({"kind": "values", "scale": "iconSet", "set": "3Arrows", "thresholds": [
            {"type": "num", "value": 3, "operator": ">"}, {"type": "percent", "value": 70, "operator": ">="}
        ]}),
    );
    add(&mut h, json!({"kind": "formula", "formula": "=B2>5", "preset": "greenFillDarkGreenText"}));
    // Newest first.
    let r = rules(&h);
    assert!(matches!(&r[0], CfRule::Expression { formula, .. } if formula == "B2>5"), "{:?}", r[0]);
    match &r[1] {
        CfRule::IconSet { set, thresholds, greater, .. } => {
            assert_eq!(set, "3Arrows");
            assert_eq!(thresholds, &[CfValueKind::Number(3.0), CfValueKind::Percent(70.0)]);
            assert_eq!(greater, &[true, false]);
        }
        other => panic!("{other:?}"),
    }
    assert!(matches!(&r[2], CfRule::BeginsWith { text, .. } if text == "ab"), "{:?}", r[2]);
    assert!(matches!(&r[3], CfRule::Top10 { bottom: true, percent: true, rank: 3, .. }), "{:?}", r[3]);
    assert!(matches!(&r[4], CfRule::AboveAverage { below: false, equal: false, std_dev: 1, .. }), "{:?}", r[4]);
}

#[test]
fn any_value_the_control_channel_sets_draws_and_confirms_without_a_crash() {
    let mut h = harness();
    control(&mut h, "ui.dialog", json!({"name": "newFormattingRule", "kind": "values", "style": "iconSet"}));
    for (field, value) in [
        ("thresholds", json!([1, "x", null])),
        ("thresholds", json!({"type": "num"})),
        ("set", json!(42)),
        ("rank", json!(-5)),
        ("min", json!("#zz")),
        ("reverse", json!("yes")),
        ("average", json!(["below3"])),
    ] {
        control(&mut h, "ui.dialog.set", json!({"field": field, "value": value}));
        h.run_steps(2);
    }
    for kind in ["values", "contains", "top", "average", "unique", "formula", "bogus"] {
        control(&mut h, "ui.dialog.set", json!({"field": "kind", "value": kind}));
        for scale in ["2color", "3color", "dataBar", "iconSet"] {
            control(&mut h, "ui.dialog.set", json!({"field": "scale", "value": scale}));
            h.run_steps(2);
        }
    }
    // Confirming may be refused (an empty formula), but never crashes.
    let _ = control(&mut h, "ui.dialog.confirm", json!({}));
    h.run_steps(2);
}
