use super::*;
use crate::annotations::CommentId;
use hayro::kurbo::Rect as PdfRect;

// Keep the uncached drawing path as a reference for actual paint shapes and
// interaction bounds, including partially clipped note markers and highlights.
fn reference(
    app: &Pagewing,
    ui: &mut egui::Ui,
    page: usize,
    page_rect: egui::Rect,
    dimensions: [f32; 2],
) -> Vec<(egui::Id, egui::Rect, egui::Rect)> {
    let mut regions = Vec::new();
    for &index in app.comments.view.overlays(page) {
        let comment = &app.comments.comments.items[index];
        for (part, &rect) in comment.rects.iter().enumerate() {
            let rect = app
                .link_rect(rect, page_rect, dimensions)
                .intersect(page_rect);
            if !rect.is_positive() || !ui.clip_rect().intersects(rect) {
                continue;
            }
            if !comment.has_appearance {
                if comment.kind == "Highlight" {
                    ui.painter().rect_filled(
                        rect,
                        0.0,
                        Color32::from_rgba_unmultiplied(255, 210, 0, 75),
                    );
                } else {
                    ui.painter().circle_filled(
                        rect.left_top() + Vec2::splat(4.0),
                        4.0,
                        Color32::from_rgb(215, 145, 30),
                    );
                }
            }
            if !app.selection.active {
                let response = ui.interact(
                    rect,
                    ui.id().with(("pdf-comment", page, index, part)),
                    egui::Sense::click(),
                );
                regions.push((response.id, response.rect, response.interact_rect));
            }
        }
    }
    regions
}

fn draw(
    app: &mut Pagewing,
    context: &Context,
    page: usize,
    origin: egui::Pos2,
    events: Vec<Event>,
    reference_path: bool,
) -> (egui::FullOutput, Vec<(egui::Id, egui::Rect, egui::Rect)>) {
    let dimensions = [600.0, 800.0];
    let size = if app.reading.rotation.is_multiple_of(2) {
        Vec2::new(600.0, 800.0)
    } else {
        Vec2::new(800.0, 600.0)
    } * app.scale;
    let page_rect = egui::Rect::from_min_size(origin, size);
    let mut regions = Vec::new();
    let output = context.run(
        RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                Vec2::new(800.0, 600.0),
            )),
            events,
            ..Default::default()
        },
        |context| {
            egui::CentralPanel::default()
                .frame(egui::Frame::NONE)
                .show(context, |ui| {
                    ui.set_clip_rect(egui::Rect::from_min_max(
                        egui::pos2(80.0, 70.0),
                        egui::pos2(440.0, 380.0),
                    ));
                    if reference_path {
                        regions = reference(app, ui, page, page_rect, dimensions);
                    } else {
                        app.comment_overlays(ui, page, page_rect, dimensions);
                    }
                });
        },
    );
    (output, regions)
}

fn assert_matches_reference(
    app: &mut Pagewing,
    context: &Context,
    page: usize,
    origin: egui::Pos2,
) {
    let (expected, regions) = draw(app, context, page, origin, Vec::new(), true);
    for _ in 0..2 {
        let (actual, _) = draw(app, context, page, origin, Vec::new(), false);
        assert_eq!(
            actual.shapes, expected.shapes,
            "page={page}, origin={origin:?}"
        );
        for &(id, rect, interaction) in &regions {
            let response = context.read_response(id).expect("visible comment region");
            assert_eq!(response.rect, rect);
            assert_eq!(response.interact_rect, interaction);
        }
    }
}

#[test]
fn cached_overlay_paint_and_hit_regions_match_while_views_and_selection_change() {
    let (_root, context, mut app) = many_comments();
    let template = app.comments.comments.items[0].clone();
    let items = (0..48)
        .map(|index| {
            let mut comment = template.clone();
            comment.page = index / 16;
            comment.slot = Some(index);
            comment.parent = (index % 5 == 0).then_some(CommentId { page: 0, slot: 1 });
            comment.has_appearance = index % 3 == 0;
            comment.kind = if index % 4 == 0 { "Highlight" } else { "Text" }.into();
            let x = (index % 4) as f64 * 180.0 - 70.0;
            let y = (index % 16 / 4) as f64 * 220.0 - 60.0;
            comment.rects = vec![
                PdfRect::new(x, y, x + 160.0, y + 140.0),
                PdfRect::new(x, y + 150.0, x + 160.0, y + 175.0),
                PdfRect::new(900.0, 950.0, 920.0, 970.0),
                PdfRect::ZERO,
            ];
            comment
        })
        .collect();
    app.comments.set_comments(Comments {
        items,
        limited: false,
    });
    // Sidebar filtering must not hide annotations on the page.
    app.comments.view.filter.query = "not in any saved comment".into();
    app.comments.view.update(&app.comments.comments, 0);
    assert!(app.comments.view.rows().is_empty());
    for rotation in 0..4 {
        app.reading.rotation = rotation;
        for scale in [0.5, 1.0, 2.0] {
            app.scale = scale;
            for origin in [egui::pos2(20.0, 30.0), egui::pos2(-100.0, -120.0)] {
                for active in [false, true] {
                    app.selection.active = active;
                    // Reusing one neighboring-page buffer must not corrupt the
                    // cached current page or reuse another page's geometry.
                    for page in [0, 1, 2, 0] {
                        assert_matches_reference(&mut app, &context, page, origin);
                    }
                }
            }
        }
    }
    let retained = app.comments.other_visible.rects.capacity();
    assert!(retained > 0);
    app.comments.clear_multi_page_overlays();
    assert_eq!(app.comments.other_visible.rects.capacity(), 0);
}

#[test]
fn snapshot_replacement_refreshes_equal_count_geometry_and_clicks_the_correct_part() {
    let (_root, context, mut app) = many_comments();
    let original = std::fs::read(&app.reading.path).unwrap();
    let mut comment = app.comments.comments.items[0].clone();
    comment.rects = vec![
        PdfRect::new(0.0, 100.0, 180.0, 120.0),
        PdfRect::new(220.0, 200.0, 300.0, 220.0),
    ];
    comment.contents = "Original geometry".into();
    app.comments.set_comments(Comments {
        items: vec![comment.clone()],
        limited: false,
    });
    app.scale = 1.0;
    app.reading.rotation = 0;
    let origin = egui::pos2(20.0, 30.0);
    assert_matches_reference(&mut app, &context, 0, origin);
    let previous = app.comments.visible.rects.clone();
    for rect in &mut comment.rects {
        *rect = *rect + hayro::kurbo::Vec2::new(30.0, 50.0);
    }
    comment.contents = "Replacement geometry".into();
    app.comments.set_comments(Comments {
        items: vec![comment],
        limited: false,
    });
    assert_matches_reference(&mut app, &context, 0, origin);
    assert_ne!(app.comments.visible.rects, previous);
    let point = app.comments.visible.rects[1].center();
    // A page-clipped region may extend outside the UI clip. That hidden
    // portion must not receive clicks, and selection owns visible clicks.
    for (active, point, opens) in [
        (false, egui::pos2(70.0, 190.0), false),
        (true, point, false),
        (false, point, true),
    ] {
        app.selection.active = active;
        // Egui hits the preceding frame's widgets. Present the new mode before
        // sending a click, just as after entering or leaving selection in the UI.
        draw(&mut app, &context, 0, origin, Vec::new(), false);
        for pressed in [true, false] {
            draw(
                &mut app,
                &context,
                0,
                origin,
                vec![
                    Event::PointerMoved(point),
                    Event::PointerButton {
                        pos: point,
                        button: egui::PointerButton::Primary,
                        pressed,
                        modifiers: Modifiers::NONE,
                    },
                ],
                false,
            );
        }
        assert_eq!(app.comments.editor.is_some(), opens);
    }
    assert_eq!(
        app.comments.editor.as_ref().unwrap().contents,
        "Replacement geometry"
    );
    assert_eq!(std::fs::read(&app.reading.path).unwrap(), original);
    app.clear_document(&context);
    assert_eq!(app.comments.visible.rects.capacity(), 0);
    assert_eq!(app.comments.other_visible.rects.capacity(), 0);
}
