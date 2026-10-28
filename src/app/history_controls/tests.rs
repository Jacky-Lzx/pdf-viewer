use super::*;
use crate::navigation::{Fit, Navigation};
use egui::{Context, Event, Key, Modifiers, Pos2, RawInput, Rect, Vec2};

fn app() -> (tempfile::TempDir, Context, Pagewing) {
    let root = tempfile::tempdir().unwrap();
    let context = Context::default();
    let mut app = Pagewing::with_context(&context, None, None, root.path().join("library.json"));
    app.nav = Navigation::new(6, 0);
    app.dimensions = vec![[600.0, 800.0]; 6];
    app.viewport = Vec2::new(500.0, 400.0);
    app.reading.fit = Fit::Custom(2.0);
    app.pan = Vec2::new(90.0, 180.0);
    app.resolve_view();
    (root, context, app)
}

fn frame(app: &mut Pagewing, context: &Context, width: f32, events: Vec<Event>) -> [bool; 2] {
    let mut availability = [false; 2];
    let _ = context.run(
        RawInput {
            screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(width, 820.0))),
            events,
            ..Default::default()
        },
        |context| {
            app.keys(context);
            app.toolbar(context);
            availability = enabled(app, context);
            egui::CentralPanel::default().show(context, |_| {});
        },
    );
    availability
}

fn key(key: Key) -> Vec<Event> {
    [true, false]
        .map(|pressed| Event::Key {
            key,
            physical_key: None,
            pressed,
            repeat: false,
            modifiers: Modifiers::NONE,
        })
        .to_vec()
}

fn enabled(app: &Pagewing, context: &Context) -> [bool; 2] {
    [
        app.input_buttons.history_back,
        app.input_buttons.history_forward,
    ]
    .map(|id| context.read_response(id.unwrap()).unwrap().enabled())
}

fn assert_position(app: &mut Pagewing, expected: &ReadingMark) {
    let actual = app.current_position();
    assert_eq!(actual.page, expected.page);
    assert_eq!(actual.fit, expected.fit);
    assert_eq!(actual.rotation, expected.rotation);
    assert_eq!(actual.continuous, expected.continuous);
    assert_eq!(actual.facing, expected.facing);
    for (actual, expected) in actual.offset.into_iter().zip(expected.offset) {
        assert!((actual - expected).abs() < 0.0001, "{actual} != {expected}");
    }
}

#[test]
fn history_buttons_restore_full_views_and_disable_at_both_boundaries() {
    for activation in [Key::Enter, Key::Space] {
        for (continuous, facing) in [(false, false), (true, false), (false, true)] {
            let (_root, context, mut app) = app();
            assert_eq!(
                frame(&mut app, &context, 1180.0, Vec::new()),
                [false, false]
            );
            let first = app.current_position();
            app.jump(2);
            app.reading.rotation = 1;
            app.reading.fit = Fit::Custom(1.5);
            app.set_display_mode(continuous, facing);
            app.pending_offset = Some([0.3, 0.6]);
            let second = app.current_position();
            assert_eq!(frame(&mut app, &context, 1180.0, Vec::new()), [true, false]);
            context
                .memory_mut(|memory| memory.request_focus(app.input_buttons.history_back.unwrap()));
            frame(&mut app, &context, 1180.0, Vec::new());
            for _ in 0..5 {
                frame(&mut app, &context, 240.0, Vec::new());
            }
            let back = context
                .read_response(app.input_buttons.history_back.unwrap())
                .unwrap();
            assert!(back.has_focus() && back.interact_rect.contains_rect(back.rect));
            app.nav.prefix = "99".into();
            app.mark_action = Some(super::super::MarkAction::Set);
            frame(&mut app, &context, 240.0, key(activation));
            assert_position(&mut app, &first);
            assert!(app.nav.prefix.is_empty() && app.mark_action.is_none());
            assert_eq!(frame(&mut app, &context, 240.0, Vec::new()), [false, true]);
            context.memory_mut(|memory| {
                memory.request_focus(app.input_buttons.history_forward.unwrap())
            });
            frame(&mut app, &context, 240.0, Vec::new());
            frame(&mut app, &context, 240.0, key(activation));
            assert_position(&mut app, &second);
            assert_eq!(frame(&mut app, &context, 240.0, Vec::new()), [true, false]);
        }
    }
}

#[test]
fn button_availability_does_not_consume_pending_geometry_and_dialogs_guard_history() {
    let (_root, context, mut app) = app();
    app.jump(2);
    app.pending_offset = Some([0.3, 0.6]);
    let before = (app.pan, app.overflow, app.pending_offset);
    let availability = frame(&mut app, &context, 240.0, Vec::new());
    assert_eq!((app.pan, app.overflow, app.pending_offset), before);
    assert_eq!(availability, [true, false]);
    for guard in ["loading", "help", "properties", "recent", "page", "zoom"] {
        match guard {
            "loading" => app.loading = true,
            "help" => app.help = true,
            "properties" => app.properties_open = true,
            "recent" => app.toggle_recent(&context),
            "page" => app.open_goto(),
            "zoom" => app.open_zoom(&context),
            _ => unreachable!(),
        }
        assert_eq!(
            frame(&mut app, &context, 1180.0, Vec::new()),
            [false, false],
            "{guard}"
        );
        app.toolbar_history(&context, false);
        assert_eq!(app.nav.page, 2, "{guard} must not navigate behind a dialog");
        app.loading = false;
        app.help = false;
        app.properties_open = false;
        app.recent_picker = None;
        app.page_input = None;
        app.close_zoom(&context);
    }
    assert_eq!(frame(&mut app, &context, 1180.0, Vec::new()), [true, false]);
    app.clear_document(&context);
    app.nav = Navigation::new(6, 0);
    app.dimensions = vec![[600.0, 800.0]; 6];
    assert_eq!(
        frame(&mut app, &context, 1180.0, Vec::new()),
        [false, false]
    );
}

#[test]
fn click_restores_history_before_following_keys_and_keeps_search_query() {
    let (_root, context, mut app) = app();
    app.jump(2);
    app.query = "kept query".into();
    app.find();
    frame(&mut app, &context, 1180.0, Vec::new());
    let back = context
        .read_response(app.input_buttons.history_back.unwrap())
        .unwrap();
    let mut events = vec![Event::PointerMoved(back.rect.center())];
    events.extend([true, false].map(|pressed| Event::PointerButton {
        pos: back.rect.center(),
        button: egui::PointerButton::Primary,
        pressed,
        modifiers: Modifiers::NONE,
    }));
    events.extend(key(Key::J).into_iter().map(|mut event| {
        if let Event::Key { modifiers, .. } = &mut event {
            *modifiers = Modifiers::SHIFT;
        }
        event
    }));
    frame(&mut app, &context, 1180.0, events);
    assert_eq!(
        app.nav.page, 1,
        "Back to page 0 must precede following Shift+J"
    );
    assert_eq!(app.query, "kept query");
    assert_eq!(
        frame(&mut app, &context, 1180.0, Vec::new()),
        [true, false],
        "new navigation clears Forward"
    );
}
