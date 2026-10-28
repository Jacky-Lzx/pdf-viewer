use super::*;
use crate::navigation::{Fit, Navigation};

mod focus;
mod hold;

fn app() -> (tempfile::TempDir, egui::Context, Pagewing) {
    let directory = tempfile::tempdir().unwrap();
    let context = egui::Context::default();
    let mut app =
        Pagewing::with_context(&context, None, None, directory.path().join("library.json"));
    app.nav = Navigation::new(80, 2);
    app.reading.bookmarks.extend([2, 4, 79]);
    app.refresh_bookmarks();
    (directory, context, app)
}

fn frame(
    app: &mut Pagewing,
    context: &egui::Context,
    width: f32,
    events: Vec<egui::Event>,
) -> egui::FullOutput {
    context.run(
        egui::RawInput {
            time: Some(context.input(|input| input.time) + 0.25),
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(width, 400.0),
            )),
            events,
            ..Default::default()
        },
        |context| {
            app.keys(context);
            app.status(context);
            egui::CentralPanel::default().show(context, |_| {});
        },
    )
}

fn undo_button(output: &egui::FullOutput) -> Option<egui::Rect> {
    output.shapes.iter().find_map(|shape| match &shape.shape {
        egui::Shape::Text(text) if matches!(text.galley.text(), "Undo bookmark" | "Undo mark") => {
            let rect = egui::Rect::from_min_size(text.pos, text.galley.size());
            assert!(shape.clip_rect.contains_rect(rect));
            Some(rect)
        }
        _ => None,
    })
}

#[test]
fn undo_button_restores_another_page_without_moving_the_view() {
    for width in [240.0, 360.0, 1180.0] {
        let (_directory, context, mut app) = app();
        app.reading.fit = Fit::Custom(1.75);
        app.reading.rotation = 1;
        app.pan = egui::vec2(37.0, 53.0);
        app.pending_offset = Some([0.4, 0.6]);
        app.remove_bookmark(79);
        for _ in 0..3 {
            frame(&mut app, &context, width, Vec::new());
        }
        let output = frame(&mut app, &context, width, Vec::new());
        let button = undo_button(&output).expect("recovery must be visible");
        assert!(button.left() >= 0.0 && button.right() <= width);
        app.dirty = false;
        for pressed in [true, false] {
            frame(
                &mut app,
                &context,
                width,
                vec![
                    egui::Event::PointerMoved(button.center()),
                    egui::Event::PointerButton {
                        pos: button.center(),
                        button: egui::PointerButton::Primary,
                        pressed,
                        modifiers: egui::Modifiers::NONE,
                    },
                ],
            );
        }
        assert_eq!(app.bookmark_pages, [2, 4, 79]);
        assert_eq!(app.reading.bookmarks, [2, 4, 79].into_iter().collect());
        assert!(app.dirty, "restored bookmark must be saved");
        assert_eq!(app.nav.page, 2);
        assert_eq!(app.pan, egui::vec2(37.0, 53.0));
        assert_eq!(app.pending_offset, Some([0.4, 0.6]));
        assert_eq!(app.reading.fit, Fit::Custom(1.75));
        assert_eq!(app.reading.rotation, 1);
        assert!(app.reveal_page.is_none());
        assert!(app.restorable_deletion().is_none());
        let output = frame(&mut app, &context, width, Vec::new());
        assert!(undo_button(&output).is_none());
    }
}

#[test]
fn keyboard_toggle_can_be_undone_and_later_edits_replace_recovery() {
    let (_directory, _context, mut app) = app();
    app.bookmark();
    assert!(matches!(
        app.restorable_deletion(),
        Some(Recovery::Bookmark(2))
    ));
    app.restore_deleted();
    assert_eq!(app.bookmark_pages, [2, 4, 79]);
    app.remove_bookmark(4);
    app.remove_bookmark(79);
    assert!(matches!(
        app.restorable_deletion(),
        Some(Recovery::Bookmark(79))
    ));
    app.restore_deleted();
    assert_eq!(app.bookmark_pages, [2, 79]);
    app.remove_bookmark(79);
    app.set_bookmark(79, true);
    app.set_bookmark(79, false);
    app.restore_deleted();
    assert_eq!(
        app.bookmark_pages,
        [2],
        "an old action must not undo later edits"
    );
    app.remove_bookmark(2);
    app.notify("A newer notice");
    app.restore_deleted();
    assert!(app.bookmark_pages.is_empty());
}

fn saved_mark() -> ReadingMark {
    ReadingMark {
        page: 79,
        fit: Fit::Custom(1.75),
        rotation: 1,
        continuous: true,
        facing: false,
        offset: [0.4, 0.6],
    }
}

#[test]
fn mark_recovery_preserves_every_saved_field_without_applying_the_view() {
    for width in [240.0, 360.0, 1180.0] {
        let (_directory, context, mut app) = app();
        let original = saved_mark();
        app.reading.marks.insert('z', original.clone());
        app.reading.fit = Fit::Width;
        app.pan = egui::vec2(37.0, 53.0);
        app.pending_offset = Some([0.2, 0.3]);
        app.remove_mark('z');
        for _ in 0..3 {
            frame(&mut app, &context, width, Vec::new());
        }
        let output = frame(&mut app, &context, width, Vec::new());
        let button = undo_button(&output).expect("mark recovery must be visible");
        assert!(button.left() >= 0.0 && button.right() <= width);
        app.dirty = false;
        for pressed in [true, false] {
            frame(
                &mut app,
                &context,
                width,
                vec![
                    egui::Event::PointerMoved(button.center()),
                    egui::Event::PointerButton {
                        pos: button.center(),
                        button: egui::PointerButton::Primary,
                        pressed,
                        modifiers: egui::Modifiers::NONE,
                    },
                ],
            );
        }
        assert!(app.dirty);
        assert_eq!(
            serde_json::to_value(&app.reading.marks[&'z']).unwrap(),
            serde_json::to_value(&original).unwrap()
        );
        assert_eq!(app.nav.page, 2);
        assert_eq!(app.reading.fit, Fit::Width);
        assert_eq!(app.pan, egui::vec2(37.0, 53.0));
        assert_eq!(app.pending_offset, Some([0.2, 0.3]));
        assert!(app.restorable_deletion().is_none());
        app.mark('z', MarkAction::Jump);
        assert_eq!(app.nav.page, original.page);
        assert_eq!(app.reading.fit, original.fit);
        assert_eq!(app.reading.rotation, original.rotation);
        assert_eq!(app.reading.continuous, original.continuous);
        assert_eq!(app.reading.facing, original.facing);
        assert_eq!(app.pending_offset, Some(original.offset));
    }
}

#[test]
fn recovery_tracks_the_latest_edit_across_bookmarks_and_marks() {
    let (_directory, _context, mut app) = app();
    app.reading.marks.insert('z', saved_mark());
    app.remove_bookmark(4);
    app.remove_mark('z');
    app.restore_deleted();
    assert!(app.reading.marks.contains_key(&'z'));
    assert!(!app.reading.bookmarks.contains(&4));
    app.remove_mark('z');
    app.remove_bookmark(79);
    app.restore_deleted();
    assert!(app.reading.bookmarks.contains(&79));
    assert!(!app.reading.marks.contains_key(&'z'));
    app.reading.marks.insert('z', saved_mark());
    app.remove_mark('z');
    app.mark('z', MarkAction::Set);
    app.restore_deleted();
    assert_eq!(
        app.reading.marks[&'z'].page, 2,
        "a newly saved mark must win"
    );
    app.remove_mark('z');
    app.set_bookmark(4, true);
    app.restore_deleted();
    assert!(!app.reading.marks.contains_key(&'z'));
}

#[test]
fn stale_mark_recovery_never_crosses_a_document_boundary_or_overwrites_a_mark() {
    for boundary in [
        "expiry", "loading", "shorter", "reload", "switch", "new mark",
    ] {
        let (directory, context, mut app) = app();
        app.reading.marks.insert('z', saved_mark());
        app.remove_mark('z');
        match boundary {
            "expiry" => app.message.as_mut().unwrap().at = Instant::now() - NOTICE_DURATION,
            "loading" => app.loading = true,
            "shorter" => app.nav.resize(3),
            "reload" => {
                app.reading.path = directory.path().join("current.pdf");
                app.reload(&context);
                app.loading = false;
            }
            "switch" => {
                app.clear_document(&context);
                app.nav = Navigation::new(80, 0);
            }
            "new mark" => {
                app.reading.marks.insert(
                    'z',
                    ReadingMark {
                        page: 1,
                        ..Default::default()
                    },
                );
            }
            _ => unreachable!(),
        }
        app.dirty = false;
        app.restore_deleted();
        assert_eq!(
            app.reading.marks.get(&'z').map(|mark| mark.page),
            (boundary == "new mark").then_some(1),
            "{boundary}"
        );
        assert!(!app.dirty, "{boundary}");
    }
}

#[test]
fn focused_recovery_consumes_activation_keys_before_reader_navigation() {
    for mark in [false, true] {
        for key in [egui::Key::Enter, egui::Key::Space] {
            let (_directory, context, mut app) = app();
            if mark {
                app.reading.marks.insert('z', saved_mark());
                app.remove_mark('z');
            } else {
                app.remove_bookmark(79);
            }
            frame(&mut app, &context, 1180.0, Vec::new());
            let id = app.input_buttons.restore_deleted.unwrap();
            context.memory_mut(|memory| memory.request_focus(id));
            // A pending page count makes an accidental Enter observable.
            app.nav.prefix = "40".into();
            frame(
                &mut app,
                &context,
                1180.0,
                vec![egui::Event::Key {
                    key,
                    physical_key: None,
                    pressed: true,
                    repeat: false,
                    modifiers: egui::Modifiers::NONE,
                }],
            );
            assert_eq!(app.bookmark_pages, [2, 4, 79]);
            if mark {
                assert_eq!(app.reading.marks[&'z'].page, 79);
            }
            assert_eq!(app.nav.page, 2);
            assert_eq!(app.nav.prefix, "40");
        }
    }
}

#[test]
fn expired_or_invalid_recovery_never_mutates_bookmarks() {
    let (_directory, context, mut app) = app();
    app.remove_bookmark(79);
    app.message.as_mut().unwrap().at = Instant::now() - NOTICE_DURATION;
    app.restore_deleted();
    assert_eq!(app.bookmark_pages, [2, 4]);
    assert!(undo_button(&frame(&mut app, &context, 1180.0, Vec::new())).is_none());
    assert!(
        app.message.is_none(),
        "expiry releases the notice and action"
    );
    app.remove_bookmark(4);
    app.loading = true;
    app.restore_deleted();
    assert_eq!(app.bookmark_pages, [2]);
    app.loading = false;
    app.nav.resize(3);
    app.restore_deleted();
    assert_eq!(app.bookmark_pages, [2]);
    app.loading = true;
    app.remove_bookmark(2);
    app.loading = false;
    app.restore_deleted();
    assert!(
        app.bookmark_pages.is_empty(),
        "reload-time edits cannot offer stale recovery"
    );
}

#[test]
fn reload_and_document_switch_discard_recovery() {
    let (directory, context, mut app) = app();
    app.reading.path = directory.path().join("current.pdf");
    app.remove_bookmark(4);
    app.reload(&context);
    assert!(app.loading);
    app.loading = false;
    app.restore_deleted();
    assert_eq!(app.bookmark_pages, [2, 79]);
    app.remove_bookmark(79);
    app.clear_document(&context);
    app.nav = Navigation::new(80, 0);
    app.restore_deleted();
    assert!(app.bookmark_pages.is_empty());
    assert!(app.reading.bookmarks.is_empty());
    assert!(app.message.is_none());
}
