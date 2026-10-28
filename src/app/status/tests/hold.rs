use super::*;

#[test]
fn paused_notice_restarts_the_full_interval_and_expired_notices_stay_expired() {
    let start = Instant::now();
    let mut notice = Notice::new("Removed");
    notice.at = start;
    assert_eq!(notice.remaining(start), Some(NOTICE_DURATION));
    notice.hold(true, start + NOTICE_DURATION - Duration::from_millis(1));
    let later = start + Duration::from_secs(3600);
    assert_eq!(notice.remaining(later), None);
    notice.hold(false, later);
    assert_eq!(notice.remaining(later), Some(NOTICE_DURATION));
    assert_eq!(
        notice.remaining(later + NOTICE_DURATION),
        Some(Duration::ZERO)
    );
    notice.hold(true, later + NOTICE_DURATION);
    assert!(
        !notice.held,
        "an expired action cannot revive from stale focus"
    );
    notice.hold(false, later + NOTICE_DURATION + Duration::from_secs(1));
    assert_eq!(
        notice.remaining(later + NOTICE_DURATION),
        Some(Duration::ZERO)
    );
}

#[test]
fn focused_undo_survives_a_quiet_window_then_activates_without_reader_navigation() {
    for mark in [false, true] {
        for activation in [egui::Key::Enter, egui::Key::Space] {
            for hidden in [false, true] {
                let (_directory, context, mut app) = app();
                app.controls_hidden = hidden;
                app.reading.fit = Fit::Custom(1.75);
                app.pan = egui::vec2(37.0, 53.0);
                if mark {
                    app.reading.marks.insert('z', saved_mark());
                    app.remove_mark('z');
                } else {
                    app.remove_bookmark(79);
                }
                let width = if hidden { 240.0 } else { 1180.0 };
                for _ in 0..3 {
                    frame(&mut app, &context, width, Vec::new());
                }
                let id = app.input_buttons.restore_deleted.unwrap();
                context.memory_mut(|memory| memory.request_focus(id));
                frame(&mut app, &context, width, Vec::new());
                assert!(app.message.as_ref().unwrap().held);
                // Simulate no native frames for longer than the original timeout.
                app.message.as_mut().unwrap().at = Instant::now() - Duration::from_secs(60);
                app.nav.prefix = "40".into();
                frame(
                    &mut app,
                    &context,
                    width,
                    vec![egui::Event::Key {
                        key: activation,
                        physical_key: None,
                        pressed: true,
                        repeat: false,
                        modifiers: egui::Modifiers::NONE,
                    }],
                );
                assert_eq!(app.bookmark_pages, [2, 4, 79]);
                if mark {
                    assert_eq!(
                        serde_json::to_value(&app.reading.marks[&'z']).unwrap(),
                        serde_json::to_value(saved_mark()).unwrap()
                    );
                }
                assert_eq!(app.nav.page, 2);
                assert_eq!(app.nav.prefix, "40");
                assert_eq!(app.reading.fit, Fit::Custom(1.75));
                assert_eq!(app.pan, egui::vec2(37.0, 53.0));
                assert_eq!(app.controls_hidden, hidden);
                let notice = app.message.as_ref().unwrap();
                assert!(!notice.held && notice.control.is_none() && notice.recovery.is_none());
            }
        }
    }
}

#[test]
fn pointer_hold_and_keyboard_focus_release_restart_the_timer_without_busy_repainting() {
    for keyboard in [false, true] {
        let (_directory, context, mut app) = app();
        app.remove_bookmark(79);
        let mut output = frame(&mut app, &context, 240.0, Vec::new());
        for _ in 0..3 {
            output = frame(&mut app, &context, 240.0, Vec::new());
        }
        let id = app.input_buttons.restore_deleted.unwrap();
        let events = if keyboard {
            context.memory_mut(|memory| memory.request_focus(id));
            Vec::new()
        } else {
            vec![egui::Event::PointerMoved(
                undo_button(&output).unwrap().center(),
            )]
        };
        frame(&mut app, &context, 240.0, events);
        assert!(app.message.as_ref().unwrap().held, "keyboard {keyboard}");
        app.message.as_mut().unwrap().at = Instant::now() - Duration::from_secs(60);
        for _ in 0..12 {
            output = frame(&mut app, &context, 240.0, Vec::new());
        }
        assert!(undo_button(&output).is_some());
        assert!(
            output.viewport_output[&egui::ViewportId::ROOT].repaint_delay > Duration::ZERO,
            "a held notice must not schedule a zero-delay repaint loop"
        );
        context.memory_mut(|memory| memory.surrender_focus(id));
        frame(&mut app, &context, 240.0, vec![egui::Event::PointerGone]);
        let notice = app.message.as_ref().unwrap();
        assert!(!notice.held, "keyboard {keyboard}");
        assert!(notice.remaining(Instant::now()).unwrap() > Duration::from_secs(4));
        app.message.as_mut().unwrap().at = Instant::now() - NOTICE_DURATION;
        assert!(undo_button(&frame(&mut app, &context, 240.0, Vec::new())).is_none());
        assert!(app.message.is_none());
        app.restore_deleted();
        assert!(!app.reading.bookmarks.contains(&79));
    }
}

#[test]
fn holding_cannot_keep_recovery_across_new_notices_reload_or_document_switch() {
    for boundary in ["notice", "reload", "switch", "new mark"] {
        let (directory, context, mut app) = app();
        app.reading.path = directory.path().join("current.pdf");
        app.reading.marks.insert('z', saved_mark());
        app.remove_mark('z');
        for _ in 0..3 {
            frame(&mut app, &context, 1180.0, Vec::new());
        }
        let id = app.input_buttons.restore_deleted.unwrap();
        context.memory_mut(|memory| memory.request_focus(id));
        frame(&mut app, &context, 1180.0, Vec::new());
        assert!(app.message.as_ref().unwrap().held);
        match boundary {
            "notice" => app.notify("Copied"),
            "reload" => {
                app.reload(&context);
                app.loading = false;
            }
            "switch" => {
                app.clear_document(&context);
                app.nav = Navigation::new(80, 0);
            }
            "new mark" => app.mark('z', MarkAction::Set),
            _ => unreachable!(),
        }
        app.restore_deleted();
        assert_eq!(
            app.reading.marks.get(&'z').map(|mark| mark.page),
            (boundary == "new mark").then_some(2),
            "{boundary}"
        );
        assert!(app.message.as_ref().is_none_or(|notice| !notice.held));
    }
}
