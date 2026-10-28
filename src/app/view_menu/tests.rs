use super::*;
use crate::navigation::Navigation;

fn frame(
    app: &mut Pagewing,
    context: &egui::Context,
    size: egui::Vec2,
    events: Vec<egui::Event>,
) -> egui::FullOutput {
    context.run(
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, size)),
            events,
            ..Default::default()
        },
        |context| {
            eframe::App::update(app, context, &mut eframe::Frame::_new_kittest());
        },
    )
}

fn settle(app: &mut Pagewing, context: &egui::Context, size: egui::Vec2) {
    for _ in 0..5 {
        frame(app, context, size, Vec::new());
    }
}

fn key(key: egui::Key) -> Vec<egui::Event> {
    [true, false]
        .map(|pressed| egui::Event::Key {
            key,
            physical_key: None,
            pressed,
            repeat: false,
            modifiers: egui::Modifiers::NONE,
        })
        .into()
}

fn fixture(size: egui::Vec2) -> (tempfile::TempDir, egui::Context, Pagewing) {
    let root = tempfile::tempdir().unwrap();
    let context = egui::Context::default();
    let mut app = Pagewing::with_context(&context, None, None, root.path().join("library.json"));
    app.nav = Navigation::new(6, 2);
    app.dimensions = vec![[600.0, 800.0]; 6];
    app.reading.fit = Fit::Custom(2.0);
    settle(&mut app, &context, size);
    (root, context, app)
}

#[test]
fn compact_search_tab_reaches_the_view_menu_without_reader_shortcuts() {
    let size = egui::vec2(240.0, 240.0);
    let (_root, context, mut app) = fixture(size);
    app.find();
    app.query = "menu draft".into();
    settle(&mut app, &context, size);
    context.memory_mut(|memory| memory.request_focus(app.input_buttons.close_search.unwrap()));
    settle(&mut app, &context, size);
    let sidebar = app.library.sidebar;
    // The horizontal search scrollbar precedes the bottom bar in compact UI.
    frame(&mut app, &context, size, key(egui::Key::Tab));
    settle(&mut app, &context, size);
    assert!(app.search_control_focused(&context));
    // The overflowing bottom row also has a focusable drag viewport.
    for index in 0..6 {
        frame(&mut app, &context, size, key(egui::Key::Tab));
        settle(&mut app, &context, size);
        let focused = context.memory(|memory| memory.focused());
        assert_eq!(
            app.library.sidebar, sidebar,
            "Tab {index}: focus {focused:?}"
        );
        assert!(
            app.status_control_focused(&context),
            "Tab {index}: focus {focused:?}"
        );
    }
    assert_eq!(
        context.memory(|memory| memory.focused()),
        app.input_buttons.view_menu
    );
    frame(&mut app, &context, size, key(egui::Key::Enter));
    assert!(egui::Popup::is_any_open(&context));
}

#[test]
fn keyboard_can_select_every_layout_and_fit_at_minimum_and_desktop_sizes() {
    for size in [egui::vec2(240.0, 240.0), egui::vec2(1180.0, 820.0)] {
        for activation in [egui::Key::Enter, egui::Key::Space] {
            for target in 0..7 {
                let (_root, context, mut app) = fixture(size);
                context.memory_mut(|memory| {
                    memory.request_focus(app.input_buttons.view_menu.unwrap())
                });
                settle(&mut app, &context, size);
                frame(&mut app, &context, size, key(activation));
                assert!(egui::Popup::is_any_open(&context));
                for index in 0..=target {
                    settle(&mut app, &context, size);
                    let focused = context.memory(|memory| memory.focused()).unwrap();
                    let response = context.read_response(focused).unwrap();
                    assert!(
                        response.interact_rect.contains_rect(response.rect),
                        "{size:?} item {index}: {:?} clipped by {:?}",
                        response.rect,
                        response.interact_rect
                    );
                    assert!(context.content_rect().contains_rect(response.rect));
                    assert_eq!(app.nav.page, 2, "focus traversal must not turn pages");
                    assert_eq!(app.reading.fit, Fit::Custom(2.0));
                    assert!(!app.reading.continuous && !app.reading.facing);
                    if index < target {
                        frame(&mut app, &context, size, key(egui::Key::ArrowDown));
                    }
                }
                frame(&mut app, &context, size, key(egui::Key::Enter));
                settle(&mut app, &context, size);
                assert!(!egui::Popup::is_any_open(&context));
                assert_eq!(app.reading.continuous, target == 1);
                assert_eq!(app.reading.facing, target == 2);
                assert_eq!(app.nav.page, 2);
                assert_eq!(
                    app.reading.fit,
                    match target {
                        3 => Fit::Page,
                        4 => Fit::Width,
                        5 => Fit::Height,
                        6 => Fit::Custom(1.0),
                        _ => Fit::Custom(2.0),
                    }
                );
            }
        }
    }
}

#[test]
fn escape_in_the_opening_click_batch_closes_only_view_and_reopening_still_works() {
    let size = egui::vec2(1180.0, 820.0);
    let (_root, context, mut app) = fixture(size);
    app.find();
    app.query = "retained draft".into();
    settle(&mut app, &context, size);
    let id = app.input_buttons.view_menu.unwrap();
    let pos = context.read_response(id).unwrap().rect.center();
    let mut events = vec![egui::Event::PointerMoved(pos)];
    for pressed in [true, false] {
        events.push(egui::Event::PointerButton {
            pos,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: egui::Modifiers::NONE,
        });
    }
    events.extend(key(egui::Key::Escape));
    frame(&mut app, &context, size, events);
    settle(&mut app, &context, size);
    assert!(!egui::Popup::is_any_open(&context));
    assert!(app.search_open);
    assert_eq!(app.query, "retained draft");
    assert_eq!(app.nav.page, 2);
    context.memory_mut(|memory| memory.request_focus(id));
    settle(&mut app, &context, size);
    frame(&mut app, &context, size, key(egui::Key::Enter));
    assert!(egui::Popup::is_any_open(&context));
    frame(&mut app, &context, size, key(egui::Key::Escape));
    assert!(app.search_open);
    assert!(!egui::Popup::is_any_open(&context));
}

#[test]
fn menu_motions_wrap_and_focus_remains_on_the_option_when_resizing() {
    let desktop = egui::vec2(1180.0, 820.0);
    let small = egui::vec2(240.0, 240.0);
    let (_root, context, mut app) = fixture(desktop);
    context.memory_mut(|memory| memory.request_focus(app.input_buttons.view_menu.unwrap()));
    frame(&mut app, &context, desktop, key(egui::Key::Enter));
    settle(&mut app, &context, desktop);
    let first = context.memory(|memory| memory.focused()).unwrap();
    frame(&mut app, &context, desktop, key(egui::Key::ArrowUp));
    settle(&mut app, &context, desktop);
    let last = context.memory(|memory| memory.focused()).unwrap();
    assert_ne!(first, last);
    for size in [small, desktop, small] {
        settle(&mut app, &context, size);
        assert_eq!(context.memory(|memory| memory.focused()), Some(last));
        let response = context.read_response(last).unwrap();
        assert!(response.interact_rect.contains_rect(response.rect));
        assert!(context.content_rect().contains_rect(response.rect));
    }
    frame(&mut app, &context, small, key(egui::Key::Tab));
    settle(&mut app, &context, small);
    assert_eq!(context.memory(|memory| memory.focused()), Some(first));
    let mut reverse = key(egui::Key::Tab);
    for event in &mut reverse {
        if let egui::Event::Key { modifiers, .. } = event {
            *modifiers = egui::Modifiers::SHIFT;
        }
    }
    frame(&mut app, &context, small, reverse);
    settle(&mut app, &context, small);
    assert_eq!(context.memory(|memory| memory.focused()), Some(last));
    frame(&mut app, &context, small, key(egui::Key::Space));
    assert_eq!(app.reading.fit, Fit::Custom(1.0));
    assert_eq!(app.nav.page, 2);
    assert!(!egui::Popup::is_any_open(&context));
}

#[test]
fn menu_activation_uses_only_motions_before_its_key_in_a_batch() {
    for (motions, expected_continuous, expected_facing) in [
        (
            vec![egui::Key::ArrowDown, egui::Key::ArrowDown, egui::Key::Enter],
            false,
            true,
        ),
        (
            vec![egui::Key::ArrowDown, egui::Key::Enter, egui::Key::ArrowDown],
            true,
            false,
        ),
    ] {
        let size = egui::vec2(240.0, 240.0);
        let (_root, context, mut app) = fixture(size);
        context.memory_mut(|memory| memory.request_focus(app.input_buttons.view_menu.unwrap()));
        frame(&mut app, &context, size, key(egui::Key::Enter));
        settle(&mut app, &context, size);
        frame(
            &mut app,
            &context,
            size,
            motions.into_iter().flat_map(key).collect(),
        );
        assert_eq!(app.reading.continuous, expected_continuous);
        assert_eq!(app.reading.facing, expected_facing);
        assert_eq!(app.nav.page, 2);
        assert!(!egui::Popup::is_any_open(&context));
    }
}

#[test]
fn first_down_after_mouse_opening_selects_the_first_option() {
    let size = egui::vec2(1180.0, 820.0);
    let (_root, context, mut app) = fixture(size);
    app.set_display_mode(true, false);
    settle(&mut app, &context, size);
    let pos = context
        .read_response(app.input_buttons.view_menu.unwrap())
        .unwrap()
        .rect
        .center();
    for pressed in [true, false] {
        frame(
            &mut app,
            &context,
            size,
            vec![
                egui::Event::PointerMoved(pos),
                egui::Event::PointerButton {
                    pos,
                    button: egui::PointerButton::Primary,
                    pressed,
                    modifiers: egui::Modifiers::NONE,
                },
            ],
        );
    }
    settle(&mut app, &context, size);
    frame(&mut app, &context, size, key(egui::Key::ArrowDown));
    frame(&mut app, &context, size, key(egui::Key::Enter));
    assert!(!app.reading.continuous && !app.reading.facing);
    assert_eq!(app.nav.page, 2);
}
