//! Sheet Right-to-Left: the grid is mirrored on screen and pointer/keyboard input follows it.

use egui::{Event, Key, Modifiers, PointerButton, Pos2};
use gridcraft_engine::Session;
use gridcraft_engine::core::{CellRef, RangeRef};
use gridcraft_ui_egui::{SheetApp, grid::Geo};
use serde_json::json;

type Harness = egui_kittest::Harness<'static, SheetApp>;

fn harness() -> Harness {
    let mut session = Session::new();
    session.new_workbook();
    session.execute("range.setValues", json!({"range": "A1", "values": [["first", 2], [3, "fourth"]]})).unwrap();
    session.execute("view.rightToLeft", json!({"on": true})).unwrap();
    session.execute("selection.set", json!({"cell": "B2"})).unwrap();
    let mut h = Harness::builder().with_size(egui::vec2(1200.0, 800.0)).build_ui_state(
        |ui, app: &mut SheetApp| {
            let ctx = ui.ctx().clone();
            app.logic(&ctx);
            app.ui(ui);
        },
        SheetApp::new(session, Default::default()),
    );
    h.run_steps(4);
    h
}

fn geo(h: &Harness) -> Geo {
    let app = h.state();
    let sheet = app.session.active().unwrap().wb.active().unwrap();
    Geo::new(sheet, app.grid.rect.unwrap(), app.view().scroll)
}

/// Where `a1` is drawn on screen.
fn point(h: &Harness, a1: &str) -> Pos2 {
    let app = h.state();
    let sheet = app.session.active().unwrap().wb.active().unwrap();
    geo(h).screen_cell_rect(sheet, CellRef::parse(a1).unwrap()).center()
}

fn frame(h: &mut Harness, event: Event) {
    h.input_mut().events.push(event);
    h.run_steps(1);
}

fn button(pos: Pos2, pressed: bool) -> Event {
    Event::PointerButton { pos, button: PointerButton::Primary, pressed, modifiers: Modifiers::NONE }
}

fn current(h: &Harness) -> RangeRef {
    h.state().session.active().unwrap().selection.current()
}

#[test]
fn column_a_is_drawn_on_the_right_and_clicks_hit_the_mirrored_cells() {
    let mut h = harness();
    let g = geo(&h);
    assert!(g.rtl);
    let (a1, b1) = (point(&h, "A1"), point(&h, "B1"));
    assert!(a1.x > b1.x, "A1 at {a1:?} should be right of B1 at {b1:?}");
    // Row headers sit on the right edge, so A1 ends where they begin.
    let sheet_a1 = {
        let app = h.state();
        let sheet = app.session.active().unwrap().wb.active().unwrap();
        g.screen_cell_rect(sheet, CellRef::parse("A1").unwrap())
    };
    assert!((sheet_a1.right() - (g.rect.right() - g.header_w)).abs() < 0.5);

    // Drag from C3 to F7 at their on-screen positions.
    let (from, to) = (point(&h, "C3"), point(&h, "F7"));
    frame(&mut h, Event::PointerMoved(from));
    frame(&mut h, button(from, true));
    frame(&mut h, Event::PointerMoved(to));
    frame(&mut h, button(to, false));
    h.run_steps(1);
    assert_eq!(current(&h), RangeRef::parse("C3:F7").unwrap());
}

#[test]
fn left_and_right_arrows_follow_the_screen() {
    let mut h = harness();
    h.key_press(Key::ArrowLeft);
    h.run_steps(1);
    assert_eq!(h.state().session.active().unwrap().selection.active, CellRef::parse("C2").unwrap());
    h.key_press(Key::ArrowRight);
    h.key_press(Key::ArrowRight);
    h.run_steps(1);
    assert_eq!(h.state().session.active().unwrap().selection.active, CellRef::parse("A2").unwrap());
}

mod arabic {
    //! Arabic interface language stays independent of worksheet direction and command parameters.

    use egui::{Event, Modifiers, PointerButton, Shape};
    use gridcraft_engine::Session;
    use gridcraft_ui_egui::{SheetApp, UiState, i18n::Language};
    use serde_json::json;

    fn harness() -> egui_kittest::Harness<'static, SheetApp> {
        harness_with_language(Language::Ar)
    }

    fn harness_with_language(language: Language) -> egui_kittest::Harness<'static, SheetApp> {
        let mut session = Session::new();
        session.new_workbook();
        let state = UiState { language, ribbon_tab: "View".into(), ..Default::default() };
        let mut app = SheetApp::new(session, Default::default());
        app.ui = state;
        let mut h = egui_kittest::Harness::builder().with_size(egui::vec2(1440.0, 900.0)).build_ui_state(
            |ui, app: &mut SheetApp| {
                let ctx = ui.ctx().clone();
                app.logic(&ctx);
                ctx.all_styles_mut(|style| {
                    style.visuals.text_cursor.stroke = egui::Stroke::new(1.5, egui::Color32::MAGENTA);
                    style.visuals.text_cursor.blink = false;
                });
                app.ui(ui);
            },
            app,
        );
        h.run_steps(6);
        h
    }

    fn caption(h: &egui_kittest::Harness<'static, SheetApp>, text: &str) -> egui::Rect {
        h.output()
            .shapes
            .iter()
            .find_map(|s| match &s.shape {
                Shape::Text(t) if t.galley.job.text.trim() == text => Some(t.visual_bounding_rect()),
                _ => None,
            })
            .unwrap_or_else(|| panic!("Arabic caption not drawn: {text}"))
    }

    #[test]
    fn arabic_view_control_toggles_direction_without_reordering_data() {
        let mut h = harness();
        assert!(!h.state().session.doc().unwrap().wb.active().unwrap().right_to_left);
        assert!(caption(&h, "الرئيسية").center().x > caption(&h, "عرض").center().x);
        assert!(caption(&h, "حفظ تلقائي").center().x > 720.0);
        assert!(caption(&h, "A1").center().x > 720.0, "the name box must move to the right");
        assert!(caption(&h, "جاهز").center().x > 720.0, "status starts on the right");
        h.state_mut().run("range.setValues", json!({"range": "A1", "values": [["مرحبا", "USD 123"]]})).unwrap();
        for on in [true, false] {
            let at = caption(&h, "الورقة من").center();
            h.input_mut().events.extend([
                Event::PointerMoved(at),
                Event::PointerButton { pos: at, button: PointerButton::Primary, pressed: true, modifiers: Modifiers::NONE },
            ]);
            h.step();
            h.input_mut().events.push(Event::PointerButton { pos: at, button: PointerButton::Primary, pressed: false, modifiers: Modifiers::NONE });
            h.run_steps(4);
            assert_eq!(h.state().session.doc().unwrap().wb.active().unwrap().right_to_left, on);
            assert_eq!(h.state().ui.language, Language::Ar);
        }
        assert_eq!(h.state_mut().run("cell.get", json!({"cell": "A1"})).unwrap()["value"], "مرحبا");
    }

    #[test]
    fn arabic_file_navigation_and_task_panes_swap_sides() {
        let mut h = harness();
        h.state_mut().open_backstage(gridcraft_ui_egui::backstage::Page::Options);
        h.run_steps(4);
        assert!(caption(&h, "رجوع").center().x > 1200.0);
        h.state_mut().backstage = None;
        h.state_mut().grid.pane = Some("watch".into());
        h.run_steps(4);
        assert!(caption(&h, "نافذة المراقبة").center().x < 400.0);
    }

    #[test]
    fn dialog_fields_and_command_search_use_arabic_labels() {
        let mut h = harness();
        h.state_mut().open_dialog("renameSheet", json!({}));
        h.run_steps(3);
        caption(&h, "الاسم:");
        caption(&h, "إلغاء");
        h.state_mut().open_dialog("commandSearch", json!({}));
        h.state_mut().dialog.as_mut().unwrap().search = "غامق".into();
        h.run_steps(3);
        assert!(h.output().shapes.iter().any(|s| matches!(&s.shape, Shape::Text(t) if t.galley.job.text.contains("غامق"))));
        h.state_mut().dialog.as_mut().unwrap().search = "تعبئة لأسفل".into();
        h.run_steps(3);
        assert!(h.output().shapes.iter().any(|s| matches!(&s.shape, Shape::Text(t)
            if t.galley.job.text.contains("تعبئة لأسفل"))));
        assert!(h.output().shapes.iter().any(|s| matches!(&s.shape, Shape::Text(t)
            if t.galley.job.text.contains("تحرير") && !t.galley.job.text.contains("Editing"))));
    }

    #[test]
    fn arabic_general_cells_and_editors_align_to_the_physical_right() {
        let mut h = harness();
        h.state_mut().run("range.setValues", json!({"range":"B2", "values":[[1231, "Latin", "مرحبا"]]})).unwrap();
        for rtl in [false, true] {
            h.state_mut().run("view.rightToLeft", json!({"on":rtl})).unwrap();
            h.run_steps(3);
            for (address, text) in [("B2", "1231"), ("C2", "Latin"), ("D2", "مرحبا")] {
                let app = h.state();
                let sheet = app.session.active().unwrap().wb.active().unwrap();
                let cell = super::geo(&h).screen_cell_rect(sheet, super::CellRef::parse(address).unwrap());
                let drawn = h
                    .output()
                    .shapes
                    .iter()
                    .find_map(|s| match &s.shape {
                        Shape::Text(t) if t.galley.job.text == text && cell.contains(t.visual_bounding_rect().center()) => {
                            Some(t.visual_bounding_rect())
                        }
                        _ => None,
                    })
                    .unwrap();
                assert!(drawn.center().x > cell.center().x, "{text} should be on the right, RTL sheet={rtl}: {drawn:?}, {cell:?}");
            }
            h.state_mut().run("selection.set", json!({"cell":"B2"})).unwrap();
            h.state_mut().begin_edit(Some("1231".into()), false);
            h.run_steps(3);
            let sheet = h.state().session.active().unwrap().wb.active().unwrap();
            let cell = super::geo(&h).screen_cell_rect(sheet, super::CellRef::parse("B2").unwrap());
            let drawn = h
                .output()
                .shapes
                .iter()
                .find_map(|s| match &s.shape {
                    Shape::Text(t) if t.galley.job.text == "1231" && cell.contains(t.visual_bounding_rect().center()) => {
                        Some(t.visual_bounding_rect())
                    }
                    _ => None,
                })
                .unwrap();
            assert!(drawn.center().x > cell.center().x);
            h.state_mut().cancel_edit();
        }
    }

    #[test]
    fn typing_caret_follows_text_direction_in_both_interface_languages() {
        for language in [Language::En, Language::Ar] {
            let mut h = harness_with_language(language);
            for text in ["English", "السلام عليكم"] {
                h.state_mut().begin_edit(Some(text.into()), false);
                h.run_steps(3);
                let cell =
                    super::geo(&h).screen_cell_rect(h.state().session.active().unwrap().wb.active().unwrap(), super::CellRef::parse("A1").unwrap());
                let drawn = h
                    .output()
                    .shapes
                    .iter()
                    .find_map(|s| match &s.shape {
                        Shape::Text(t) if t.galley.job.text == text && cell.contains(t.visual_bounding_rect().center()) => {
                            Some(t.visual_bounding_rect())
                        }
                        _ => None,
                    })
                    .unwrap();
                let caret = h
                    .output()
                    .shapes
                    .iter()
                    .find_map(|s| match &s.shape {
                        Shape::LineSegment { points, stroke }
                            if stroke.color == egui::Color32::MAGENTA
                                && (points[0].y + points[1].y) * 0.5 >= cell.top()
                                && (points[0].y + points[1].y) * 0.5 < cell.bottom() =>
                        {
                            Some(points[0].x)
                        }
                        _ => None,
                    })
                    .unwrap_or_else(|| panic!("no editor caret for {language:?}, {text}"));
                if text == "English" {
                    assert!(caret >= drawn.right() - 3.0, "English caret must follow the text: {caret}, {drawn:?}");
                } else {
                    assert!(caret <= drawn.left() + 3.0, "Arabic caret must follow the text on its left: {caret}, {drawn:?}");
                }
                h.state_mut().cancel_edit();
            }
        }
    }

    #[test]
    fn arabic_column_completion_is_a_joined_word_and_commits_on_enter() {
        for language in [Language::En, Language::Ar] {
            let mut h = harness_with_language(language);
            h.state_mut().run("cell.set", json!({"cell":"A1", "input":"تحيا مصر"})).unwrap();
            h.state_mut().run("view.rightToLeft", json!({"on":true})).unwrap();
            h.state_mut().run("selection.set", json!({"cell":"A2"})).unwrap();
            h.input_mut().events.push(Event::Text("ت".into()));
            h.run_steps(3);
            assert_eq!(h.state().editor.as_ref().unwrap().completion.as_deref(), Some("تحيا مصر"));
            let sheet = h.state().session.active().unwrap().wb.active().unwrap();
            let cell = super::geo(&h).screen_cell_rect(sheet, super::CellRef::parse("A2").unwrap());
            let suggestion = h
                .output()
                .shapes
                .iter()
                .find_map(|s| match &s.shape {
                    Shape::Text(t) if t.galley.job.text == "تحيا مصر" && cell.contains(t.visual_bounding_rect().center()) => {
                        assert!(s.clip_rect.contains_rect(t.visual_bounding_rect()), "the whole joined suggestion must be visible");
                        Some(t)
                    }
                    _ => None,
                })
                .unwrap();
            if language.is_rtl() {
                assert!(suggestion.visual_bounding_rect().right() > cell.center().x);
            } else {
                assert!(suggestion.visual_bounding_rect().left() < cell.center().x);
            }
            let boundary = suggestion.galley.pos_from_cursor(egui::text::CCursor::new("تحيا مصر".chars().count() - 1)).center().x + suggestion.pos.x;
            let caret = h
                .output()
                .shapes
                .iter()
                .find_map(|s| match &s.shape {
                    Shape::LineSegment { points, stroke }
                        if stroke.color == egui::Color32::MAGENTA && cell.contains((points[0] + points[1].to_vec2()) * 0.5) =>
                    {
                        Some(points[0].x)
                    }
                    _ => None,
                })
                .unwrap();
            assert!((caret - boundary).abs() < 1.0, "the caret must sit between the joined prefix and suggestion");
            assert!(
                suggestion.galley.rows.iter().any(|row| row
                    .visuals
                    .mesh
                    .vertices
                    .iter()
                    .any(|v| v.color == egui::Color32::from_rgb(0x2E, 0x6F, 0xD8)))
            );
            h.key_press(egui::Key::Enter);
            h.run_steps(3);
            assert_eq!(
                h.state().session.active().unwrap().wb.active().unwrap().value(super::CellRef::parse("A2").unwrap()),
                gridcraft_engine::core::Value::from("تحيا مصر")
            );
        }
    }

    #[test]
    fn formula_bar_alignment_follows_the_interface_language() {
        for language in [Language::En, Language::Fr, Language::Ar] {
            let mut h = harness_with_language(language);
            for text in ["السلام عليكم", "English"] {
                h.state_mut().begin_edit(Some(text.into()), true);
                h.run_steps(3);
                let grid_top = h.state().grid.rect.unwrap().top();
                let drawn = h
                    .output()
                    .shapes
                    .iter()
                    .find_map(|s| match &s.shape {
                        Shape::Text(t)
                            if t.galley.job.text == text
                                && t.visual_bounding_rect().center().y < grid_top
                                && t.visual_bounding_rect().center().y > grid_top - 35.0 =>
                        {
                            Some(t.visual_bounding_rect())
                        }
                        _ => None,
                    })
                    .unwrap();
                if language.is_rtl() {
                    assert!(drawn.left() > 720.0, "Arabic interface aligns the editor right: {drawn:?}");
                } else {
                    assert!(drawn.right() < 720.0, "LTR interface keeps Arabic and Latin text on the left: {drawn:?}");
                }
                h.state_mut().cancel_edit();
            }
        }
    }

    #[test]
    fn editing_can_move_from_a_cell_to_the_formula_bar() {
        for language in [Language::En, Language::Ar] {
            for text in ["English", "ليليل"] {
                let mut h = harness_with_language(language);
                h.input_mut().events.push(Event::Text(text.into()));
                h.run_steps(3);
                let grid_top = h.state().grid.rect.unwrap().top();
                let bar = h
                    .output()
                    .shapes
                    .iter()
                    .find_map(|s| match &s.shape {
                        Shape::Text(t)
                            if t.galley.job.text == text
                                && t.visual_bounding_rect().center().y < grid_top
                                && t.visual_bounding_rect().center().y > grid_top - 35.0 =>
                        {
                            Some(t.visual_bounding_rect())
                        }
                        _ => None,
                    })
                    .unwrap();
                let at = bar.center();
                h.input_mut().events.extend([
                    Event::PointerMoved(at),
                    Event::PointerButton { pos: at, button: PointerButton::Primary, pressed: true, modifiers: Modifiers::NONE },
                ]);
                h.step();
                h.input_mut().events.push(Event::PointerButton {
                    pos: at,
                    button: PointerButton::Primary,
                    pressed: false,
                    modifiers: Modifiers::NONE,
                });
                h.run_steps(3);
                assert!(h.state().editor.as_ref().unwrap().from_formula_bar);
                let extra = if text == "English" { "X" } else { "ب" };
                h.input_mut().events.push(Event::Text(extra.into()));
                h.run_steps(3);
                let edited = h.state().editor.as_ref().unwrap().text.clone();
                assert!(edited.contains(extra));
                h.key_press(egui::Key::Enter);
                h.run_steps(3);
                assert!(h.state().editor.is_none());
                assert_eq!(
                    h.state().session.active().unwrap().wb.active().unwrap().value(super::CellRef::parse("A1").unwrap()),
                    gridcraft_engine::core::Value::from(edited)
                );
            }
        }
    }

    #[test]
    fn arabic_sheet_context_menu_stays_compact() {
        fn rectangles(shape: &Shape, output: &mut Vec<egui::Rect>) {
            match shape {
                Shape::Rect(r) => output.push(r.rect),
                Shape::Vec(shapes) => {
                    for shape in shapes {
                        rectangles(shape, output);
                    }
                }
                _ => {}
            }
        }
        let mut h = harness();
        let at = caption(&h, "Sheet1").center();
        h.input_mut().events.extend([
            Event::PointerMoved(at),
            Event::PointerButton { pos: at, button: PointerButton::Secondary, pressed: true, modifiers: Modifiers::NONE },
        ]);
        h.step();
        h.input_mut().events.push(Event::PointerButton { pos: at, button: PointerButton::Secondary, pressed: false, modifiers: Modifiers::NONE });
        for _ in 0..4 {
            h.run_steps(2);
            let first = caption(&h, "إدراج ورقة");
            let last = caption(&h, "تحديد كل الأوراق");
            let mut frames = Vec::new();
            for shape in &h.output().shapes {
                rectangles(&shape.shape, &mut frames);
            }
            let menu = frames
                .into_iter()
                .filter(|r| r.contains(first.center()) && r.contains(last.center()))
                .min_by(|a, b| a.area().total_cmp(&b.area()))
                .unwrap();
            assert!(menu.width() < 320.0, "sheet menu must fit its contents: {menu:?}");
            assert!(first.right() - menu.left() > menu.width() * 0.5, "Arabic labels stay on the right");
        }
    }

    #[test]
    fn arabic_ribbon_and_dialog_layouts_survive_language_switching() {
        let mut h = harness();
        for tab in gridcraft_ui_egui::ribbon::TABS {
            h.state_mut().ui.ribbon_tab = (*tab).into();
            h.run_steps(3);
        }
        for dialog in ["formatCells", "find", "sort", "nameManager", "pageSetup", "pasteSpecial", "analysisToolPak", "solver"] {
            h.state_mut().open_dialog(dialog, json!({}));
            h.run_steps(3);
        }
        for tab in ["Number", "Alignment", "Font", "Border", "Fill", "Protection"] {
            h.state_mut().open_dialog("formatCells", json!({"tab": tab}));
            h.run_steps(3);
        }
        h.state_mut().dialog = None;
        h.state_mut().run("app.language.set", json!({"language":"en"})).unwrap();
        h.run_steps(4);
        assert!(caption(&h, "AutoSave").center().x < 720.0);
        assert!(caption(&h, "A1").center().x < 720.0);
    }
}
