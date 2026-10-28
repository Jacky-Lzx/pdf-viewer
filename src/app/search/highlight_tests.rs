use super::super::{CURRENT_SEARCH_COLOR, Pagewing, SEARCH_COLOR};
use crate::{navigation::Navigation, text::SearchHit, worker::Event};
use eframe::egui::{self, Context, Rect, Vec2, pos2};
use hayro::kurbo::Rect as PdfRect;

// Preserve the original drawing path to compare complete paint shapes,
// including original page clipping, corners, order and selected-hit colors.
fn reference(app: &Pagewing, painter: &egui::Painter, page: usize, page_rect: Rect) {
    let clip = painter.clip_rect().intersect(page_rect);
    if !clip.is_positive() {
        return;
    }
    painter.extend(crate::text::page_hits(&app.hits, page).flat_map(|index| {
        let color = if app.hit_index == Some(index) {
            CURRENT_SEARCH_COLOR
        } else {
            SEARCH_COLOR
        };
        app.hits[index].rects.iter().filter_map(move |&rect| {
            let rect = app
                .link_rect(rect, page_rect, [600.0, 800.0])
                .intersect(page_rect);
            (rect.is_positive() && clip.intersects(rect))
                .then(|| egui::Shape::rect_filled(rect, 1.0, color))
        })
    }));
}

fn draw(
    app: &mut Pagewing,
    context: &Context,
    page: usize,
    origin: egui::Pos2,
    original: bool,
) -> Vec<egui::epaint::ClippedShape> {
    let size = if app.reading.rotation.is_multiple_of(2) {
        Vec2::new(600.0, 800.0)
    } else {
        Vec2::new(800.0, 600.0)
    } * app.scale;
    let page_rect = Rect::from_min_size(origin, size);
    context
        .run(
            egui::RawInput {
                screen_rect: Some(Rect::from_min_size(
                    egui::Pos2::ZERO,
                    Vec2::new(800.0, 600.0),
                )),
                ..Default::default()
            },
            |context| {
                egui::CentralPanel::default()
                    .frame(egui::Frame::NONE)
                    .show(context, |ui| {
                        let painter =
                            ui.painter_at(Rect::from_min_max(pos2(80.0, 70.0), pos2(440.0, 380.0)));
                        if original {
                            reference(app, &painter, page, page_rect);
                        } else {
                            app.highlights(&painter, page, page_rect, [600.0, 800.0]);
                        }
                    });
            },
        )
        .shapes
}

fn hit(page: usize, x: f64, y: f64) -> SearchHit {
    SearchHit {
        page,
        rects: vec![PdfRect::new(x, y, x + 180.0, y + 20.0)],
        snippet: "search 中文😀".into(),
        snippet_match: 0..6,
    }
}

fn reader() -> (tempfile::TempDir, Context, Pagewing) {
    let directory = tempfile::tempdir().unwrap();
    let context = Context::default();
    let mut app =
        Pagewing::with_context(&context, None, None, directory.path().join("library.json"));
    app.nav = Navigation::new(3, 1);
    app.dimensions = vec![[600.0, 800.0]; 3];
    app.viewport = Vec2::new(600.0, 400.0);
    (directory, context, app)
}

#[test]
fn cached_search_paint_matches_original_across_views_neighbors_and_selected_hits() {
    let (_root, context, mut app) = reader();
    app.hits = (0..12)
        .map(|index| {
            let x = (index % 4) as f64 * 180.0 - 70.0;
            let y = (index % 4) as f64 * 220.0 - 60.0;
            let mut result = hit(index / 4, x, y);
            result.rects.extend([
                PdfRect::new(x, y + 30.0, x + 150.0, y + 60.0),
                PdfRect::new(-500.0, -500.0, -400.0, -400.0),
                PdfRect::ZERO,
                PdfRect::new(900.0, 950.0, 920.0, 970.0),
            ]);
            result
        })
        .collect();
    for rotation in 0..4 {
        app.reading.rotation = rotation;
        for scale in [0.5, 1.0, 2.0] {
            app.scale = scale;
            for origin in [pos2(-120.0, -150.0), pos2(20.0, 30.0), pos2(300.0, 250.0)] {
                for page in [1, 0, 2, 1] {
                    for selected in [None, Some(0), Some(5), Some(10)] {
                        app.hit_index = selected;
                        let expected = draw(&mut app, &context, page, origin, true);
                        for _ in 0..2 {
                            assert_eq!(
                                draw(&mut app, &context, page, origin, false),
                                expected,
                                "rotation {rotation}, scale {scale}, page {page}, selected {selected:?}"
                            );
                        }
                    }
                }
            }
        }
    }
}

#[test]
fn accepted_batches_query_replacement_reload_and_close_update_painted_regions() {
    for page in [0, 1, 2] {
        check_snapshot_changes(page);
    }
}

fn check_snapshot_changes(page: usize) {
    let (_root, context, mut app) = reader();
    let (sender, receiver) = std::sync::mpsc::channel();
    app.worker.events = receiver;
    let batch = |app: &Pagewing, revision, hits| {
        sender
            .send(Event::Search {
                id: app.id,
                revision,
                hits,
                scanned: 3,
                total: 3,
                complete: true,
                limited: false,
            })
            .unwrap();
    };
    let origin = pos2(20.0, 30.0);
    let check = |app: &mut Pagewing| {
        let expected = draw(app, &context, page, origin, true);
        let actual = draw(app, &context, page, origin, false);
        assert_eq!(actual, expected);
        actual
    };
    batch(&app, app.revision, vec![hit(page, 100.0, 100.0)]);
    app.events(&context);
    let first = check(&mut app);
    batch(&app, app.revision, vec![hit(page, 130.0, 140.0)]);
    app.events(&context);
    let appended = check(&mut app);
    assert_ne!(
        appended, first,
        "same-view accepted batches must add geometry"
    );
    batch(&app, app.revision + 1, vec![hit(page, 160.0, 180.0)]);
    batch(&app, app.revision, Vec::new());
    app.events(&context);
    assert_eq!(
        check(&mut app),
        appended,
        "stale queries and progress-only batches preserve hits"
    );

    app.query = "replacement".into();
    app.change_search();
    let empty = check(&mut app);
    assert_ne!(empty, appended);
    batch(
        &app,
        app.revision,
        vec![hit(page, 180.0, 200.0), hit(page, 210.0, 240.0)],
    );
    app.events(&context);
    let replaced = check(&mut app);
    assert_ne!(
        replaced, appended,
        "equal hit counts can describe different regions"
    );
    app.hit_index = Some(1);
    assert_ne!(
        check(&mut app),
        replaced,
        "current-hit color must not be cached with geometry"
    );

    app.reloading = true;
    sender
        .send(Event::Opened {
            id: app.id,
            dimensions: app.dimensions.clone(),
            sync_geometry: Vec::new(),
            info: Default::default(),
            outline: Vec::new(),
            labels: Default::default(),
        })
        .unwrap();
    app.events(&context);
    assert!(app.hits.is_empty());
    assert_eq!(
        check(&mut app),
        empty,
        "reload must discard the old search snapshot"
    );
    batch(&app, app.revision, vec![hit(page, 250.0, 280.0)]);
    app.events(&context);
    assert_ne!(check(&mut app), empty);
    app.clear_document(&context);
    assert_eq!(
        check(&mut app),
        empty,
        "closing must discard all old highlights"
    );
}
