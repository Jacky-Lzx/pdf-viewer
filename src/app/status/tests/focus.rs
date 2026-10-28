use super::*;
use egui::{Event, Key, Modifiers};

fn key(key: Key, modifiers: Modifiers) -> Vec<Event> {
    [true, false]
        .map(|pressed| Event::Key {
            key,
            physical_key: None,
            pressed,
            repeat: false,
            modifiers,
        })
        .to_vec()
}

#[test]
fn page_and_zoom_keep_their_focus_when_status_content_changes() {
    for (zoom, activation) in [
        (false, Key::Enter),
        (false, Key::Space),
        (true, Key::Enter),
        (true, Key::Space),
    ] {
        let (_directory, context, mut app) = app();
        app.dimensions = vec![[600.0, 800.0]; app.nav.count];
        app.viewport = egui::vec2(400.0, 320.0);
        frame(&mut app, &context, 1180.0, Vec::new());
        let target = |app: &Pagewing| {
            if zoom {
                app.input_buttons.zoom
            } else {
                app.input_buttons.page
            }
            .unwrap()
        };
        let id = target(&app);
        context.memory_mut(|memory| memory.request_focus(id));
        for (width, status) in [
            (240.0, "prefix"),
            (1180.0, "undo"),
            (240.0, "expired"),
            (360.0, "prefix"),
        ] {
            match status {
                "prefix" => app.nav.prefix = "40".into(),
                "undo" => {
                    app.nav.clear_prefix();
                    app.remove_bookmark(79);
                }
                "expired" => app.message.as_mut().unwrap().at = Instant::now() - NOTICE_DURATION,
                _ => unreachable!(),
            }
            for _ in 0..5 {
                frame(&mut app, &context, width, Vec::new());
            }
            assert_eq!(
                target(&app),
                id,
                "zoom {zoom}, {status}: keep the action identity"
            );
            assert_eq!(context.memory(|memory| memory.focused()), Some(id));
            let response = context.read_response(id).unwrap();
            assert!(response.interact_rect.contains_rect(response.rect));
            assert_eq!(app.nav.page, 2);
            assert!(app.page_input.is_none() && app.zoom_input.is_none());
        }
        frame(&mut app, &context, 240.0, key(activation, Modifiers::NONE));
        assert_eq!(app.page_input.is_some(), !zoom);
        assert_eq!(app.zoom_input.is_some(), zoom);
        assert_eq!(app.nav.page, 2, "activation must open the retained target");
    }
}

#[test]
fn focused_zoom_buttons_apply_only_one_zoom_without_reader_pan() {
    for (out, activation) in [
        (false, Key::Enter),
        (false, Key::Space),
        (true, Key::Enter),
        (true, Key::Space),
    ] {
        let (_directory, context, mut app) = app();
        app.dimensions = vec![[600.0, 800.0]; app.nav.count];
        app.viewport = egui::vec2(400.0, 320.0);
        app.reading.fit = Fit::Custom(2.0);
        app.pan = egui::vec2(237.0, 353.0);
        frame(&mut app, &context, 1180.0, Vec::new());
        context.memory_mut(|memory| memory.request_focus(app.input_buttons.zoom.unwrap()));
        frame(&mut app, &context, 1180.0, Vec::new());
        let modifiers = if out {
            Modifiers::NONE
        } else {
            Modifiers::SHIFT
        };
        frame(&mut app, &context, 1180.0, key(Key::Tab, modifiers));
        frame(&mut app, &context, 1180.0, Vec::new());
        let focused = context.memory(|memory| memory.focused()).unwrap();
        for _ in 0..5 {
            frame(&mut app, &context, 240.0, Vec::new());
        }
        assert_eq!(context.memory(|memory| memory.focused()), Some(focused));
        let factor = if out { 1.0 / 1.2 } else { 1.2 };
        app.zoom(factor);
        let expected_fit = app.reading.fit;
        let expected_pan = app.pan;
        app.reading.fit = Fit::Custom(2.0);
        app.pan = egui::vec2(237.0, 353.0);
        app.nav.prefix = "40".into();
        frame(&mut app, &context, 240.0, key(activation, Modifiers::NONE));
        assert_eq!(app.reading.fit, expected_fit);
        assert_eq!(
            app.pan, expected_pan,
            "activation must not add reader scrolling"
        );
        assert_eq!(app.nav.page, 2);
        assert_eq!(app.nav.prefix, "40");
    }
}

#[test]
fn bottom_bar_tab_and_view_activation_leave_reading_commands_alone() {
    for activation in [Key::Enter, Key::Space] {
        let (_directory, context, mut app) = app();
        app.dimensions = vec![[600.0, 800.0]; app.nav.count];
        app.viewport = egui::vec2(400.0, 320.0);
        app.reading.fit = Fit::Custom(2.0);
        app.pan = egui::vec2(37.0, 53.0);
        let sidebar = app.library.sidebar;
        frame(&mut app, &context, 1180.0, Vec::new());
        context.memory_mut(|memory| memory.request_focus(app.input_buttons.zoom.unwrap()));
        frame(&mut app, &context, 1180.0, Vec::new());
        // Zoom percentage -> minus -> View menu.
        for _ in 0..2 {
            frame(&mut app, &context, 1180.0, key(Key::Tab, Modifiers::NONE));
            assert_eq!(
                app.library.sidebar, sidebar,
                "Tab must traverse bottom controls"
            );
        }
        let id = context.memory(|memory| memory.focused()).unwrap();
        for width in [240.0, 1180.0, 240.0] {
            for _ in 0..5 {
                frame(&mut app, &context, width, Vec::new());
            }
            assert_eq!(context.memory(|memory| memory.focused()), Some(id));
            let response = context.read_response(id).unwrap();
            assert!(response.interact_rect.contains_rect(response.rect));
        }
        app.nav.prefix = "40".into();
        frame(&mut app, &context, 240.0, key(activation, Modifiers::NONE));
        assert!(
            egui::Popup::is_any_open(&context),
            "{activation:?} opens View"
        );
        assert_eq!(app.nav.page, 2);
        assert_eq!(app.nav.prefix, "40");
        assert_eq!(app.pan, egui::vec2(37.0, 53.0));
        assert_eq!(app.reading.fit, Fit::Custom(2.0));
        assert_eq!(app.library.sidebar, sidebar);
    }
}
