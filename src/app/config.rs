use super::Pagewing;
use crate::{navigation::Fit, settings::Layout};
use eframe::egui::{self, Context, Event, FullOutput, Key, Modifiers, Pos2, RawInput, Rect, Vec2};

fn key(key: Key, modifiers: Modifiers) -> Vec<Event> {
    [true, false]
        .into_iter()
        .map(|pressed| Event::Key {
            key,
            physical_key: None,
            pressed,
            repeat: false,
            modifiers,
        })
        .collect()
}

fn frame(app: &mut Pagewing, context: &Context, events: Vec<Event>) -> FullOutput {
    context.run(
        RawInput {
            screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(1100.0, 820.0))),
            events,
            ..Default::default()
        },
        |context| {
            app.keys(context);
            app.toolbar(context);
            egui::CentralPanel::default().show(context, |_| {});
        },
    )
}

#[test]
fn text_weight_keys_apply_configured_steps_counts_limits_and_input_isolation() {
    let directory = tempfile::tempdir().unwrap();
    let preferences = directory.path().join("config.toml");
    let original = "[appearance]\ntext_embolden = 0.2\ntext_embolden_step = 0.125\n[shortcuts]\ntext_embolden_increase = ['X']\ntext_embolden_decrease = ['Shift+X']\n";
    std::fs::write(&preferences, original).unwrap();
    let context = Context::default();
    let mut app =
        Pagewing::with_context(&context, None, None, directory.path().join("library.json"));
    app.nav = crate::navigation::Navigation::new(1, 0);
    app.dimensions = vec![[600.0, 800.0]];
    frame(&mut app, &context, key(Key::Equals, Modifiers::ALT));
    assert_eq!(app.text_embolden, 0.2, "replaced default is disabled");
    frame(&mut app, &context, key(Key::X, Modifiers::NONE));
    assert!((app.text_embolden - 0.325).abs() < 0.00001);
    let mut events = key(Key::Num3, Modifiers::NONE);
    events.extend(key(Key::X, Modifiers::NONE));
    frame(&mut app, &context, events);
    assert!((app.text_embolden - 0.7).abs() < 0.00001);
    frame(&mut app, &context, key(Key::X, Modifiers::SHIFT));
    assert!((app.text_embolden - 0.575).abs() < 0.00001);
    app.search_open = true;
    app.search_focus = true;
    frame(&mut app, &context, key(Key::X, Modifiers::NONE));
    assert!((app.text_embolden - 0.575).abs() < 0.00001);
    app.search_open = false;
    app.search_focus = false;
    context.memory_mut(|memory| {
        if let Some(id) = memory.focused() {
            memory.surrender_focus(id);
        }
    });
    app.adjust_text_embolden(true, 100);
    assert_eq!(app.text_embolden, 1.0);
    let revision = app.appearance_revision;
    app.adjust_text_embolden(true, 1);
    assert_eq!(
        app.appearance_revision, revision,
        "limit does not invalidate caches again"
    );
    app.adjust_text_embolden(false, 100);
    assert_eq!(app.text_embolden, 0.0);
    assert!(app.message.as_ref().unwrap().0.contains("0.000"));
    app.save();
    assert_eq!(std::fs::read_to_string(preferences).unwrap(), original);
}

#[test]
fn default_text_weight_keys_work_and_stale_images_cannot_reenter_caches() {
    use crate::document::RenderKey;
    use crate::worker::Event as WorkerEvent;
    let directory = tempfile::tempdir().unwrap();
    let context = Context::default();
    let mut app =
        Pagewing::with_context(&context, None, None, directory.path().join("library.json"));
    app.nav = crate::navigation::Navigation::new(1, 0);
    app.dimensions = vec![[600.0, 800.0]];
    let key_value = RenderKey {
        page: 0,
        width: 1,
        rotation: 0,
        night: false,
        tile: None,
    };
    let image = egui::ColorImage::filled([1, 1], egui::Color32::BLACK);
    let texture = context.load_texture("old weight", image.clone(), egui::TextureOptions::LINEAR);
    app.cache.insert(key_value, texture);
    app.thumbnails.want(key_value);
    app.thumbnails
        .accept(key_value, Some(image.clone()), &context);
    frame(&mut app, &context, key(Key::Equals, Modifiers::ALT));
    assert_eq!(app.text_embolden, 0.05);
    assert!(!app.cache.contains(&key_value));
    assert!(app.thumbnails.want(key_value).is_none());
    // Simulate results that were already in the bounded queue when the user
    // changed weight. Session ID and render key still match the current view.
    let (tx, rx) = std::sync::mpsc::channel();
    app.worker.events = rx;
    for revision in [0, app.appearance_revision] {
        tx.send(WorkerEvent::Rendered {
            id: app.id,
            key: key_value,
            image: image.clone(),
            milliseconds: 0.0,
            appearance_revision: revision,
        })
        .unwrap();
        tx.send(WorkerEvent::Thumbnail {
            id: app.id,
            key: key_value,
            image: Some(image.clone()),
            appearance_revision: revision,
        })
        .unwrap();
        app.events(&context);
        assert_eq!(
            app.cache.contains(&key_value),
            revision == app.appearance_revision
        );
        assert_eq!(
            app.thumbnails.want(key_value).is_some(),
            revision == app.appearance_revision
        );
    }
    frame(&mut app, &context, key(Key::Minus, Modifiers::ALT));
    assert_eq!(app.text_embolden, 0.0);
    // Repeated decimal steps return exactly to each endpoint.
    for _ in 0..20 {
        app.adjust_text_embolden(true, 1);
    }
    assert_eq!(app.text_embolden, 1.0);
    for _ in 0..20 {
        app.adjust_text_embolden(false, 1);
    }
    assert_eq!(app.text_embolden, 0.0);
}

#[test]
fn zoom_roundtrip_does_not_save_roundoff_as_half_page_scroll() {
    let directory = tempfile::tempdir().unwrap();
    let context = Context::default();
    let mut app =
        Pagewing::with_context(&context, None, None, directory.path().join("library.json"));
    app.nav = crate::navigation::Navigation::new(1, 0);
    app.dimensions = vec![[600_000.0, 800_000.0]];
    app.viewport = Vec2::new(1132.0, 680.0);
    app.resolve_view();
    app.zoom_steps(1, false);
    app.resolve_view();
    assert!((app.current_offset()[1] - 0.5).abs() < 0.001);
    app.zoom_steps(1, true);
    app.resolve_view();
    assert_eq!(app.current_offset(), [0.0; 2]);
}

#[test]
fn toolbar_find_accepts_same_frame_text_at_the_desktop_smoke_position() {
    let directory = tempfile::tempdir().unwrap();
    let context = Context::default();
    let mut app =
        Pagewing::with_context(&context, None, None, directory.path().join("library.json"));
    app.nav = crate::navigation::Navigation::new(4, 1);
    app.dimensions = vec![[600.0, 800.0]; 4];
    let pos = egui::pos2(1066.0, 28.0);
    let mut events = vec![Event::PointerMoved(pos)];
    events.extend([true, false].map(|pressed| Event::PointerButton {
        pos,
        button: egui::PointerButton::Primary,
        pressed,
        modifiers: Modifiers::NONE,
    }));
    events.extend(key(Key::J, Modifiers::NONE));
    events.push(Event::Text("jkgditoolbar".into()));
    for events in [Vec::new(), Vec::new(), events] {
        let _ = context.run(
            RawInput {
                screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(1180.0, 820.0))),
                events,
                ..Default::default()
            },
            |context| {
                app.keys(context);
                app.toolbar(context);
                app.search_bar(context);
                egui::CentralPanel::default().show(context, |_| {});
            },
        );
    }
    assert!(app.search_open);
    assert_eq!(app.query, "jkgditoolbar");
    assert_eq!(app.nav.page, 1);
    assert_eq!(app.reading.fit, Fit::Page);
}

#[test]
fn configured_reader_keys_keep_counts_and_do_not_escape_text_fields_or_sequences() {
    let directory = tempfile::tempdir().unwrap();
    std::fs::write(
        directory.path().join("config.toml"),
        "[shortcuts]\nnext_page = ['X', 'Cmd+Comma']\nthumbnails = []\n",
    )
    .unwrap();
    let context = Context::default();
    let mut app =
        Pagewing::with_context(&context, None, None, directory.path().join("library.json"));
    app.nav = crate::navigation::Navigation::new(12, 0);
    app.dimensions = vec![[600.0, 800.0]; 12];
    frame(&mut app, &context, key(Key::J, Modifiers::SHIFT));
    assert_eq!(app.nav.page, 0, "replaced defaults must be inactive");
    let mut events = key(Key::Num3, Modifiers::NONE);
    events.extend(key(Key::X, Modifiers::NONE));
    frame(&mut app, &context, events);
    assert_eq!(app.nav.page, 3);
    frame(&mut app, &context, key(Key::Comma, Modifiers::COMMAND));
    assert_eq!(
        app.nav.page, 4,
        "the former Settings shortcut is configurable"
    );
    app.search_open = true;
    app.search_focus = true;
    frame(&mut app, &context, key(Key::X, Modifiers::NONE));
    assert_eq!(
        app.nav.page, 4,
        "plain custom keys must not navigate while typing"
    );
    app.search_open = false;
    app.search_focus = false;
    let mut events = key(Key::Z, Modifiers::NONE);
    events.extend(key(Key::T, Modifiers::NONE));
    frame(&mut app, &context, events);
    assert!(
        !app.nav.pending_z,
        "zt must still work when t has been unassigned"
    );
    assert!(!app.library.thumbnails);
    app.mark_action = Some(super::MarkAction::Set);
    frame(&mut app, &context, key(Key::Num0, Modifiers::COMMAND));
    assert!(
        app.mark_action.is_none(),
        "Command zoom cancels the pending mark prompt"
    );
    assert_eq!(app.reading.fit, Fit::Custom(1.0));
    app.mark_action = Some(super::MarkAction::Set);
    frame(&mut app, &context, key(Key::Equals, Modifiers::COMMAND));
    assert!(app.mark_action.is_none());
    assert_eq!(app.reading.fit, Fit::Custom(1.2));
}

#[test]
fn restarting_applies_edited_config_without_overwriting_it_or_reading_history() {
    let directory = tempfile::tempdir().unwrap();
    let state = directory.path().join("library.json");
    let preferences = directory.path().join("config.toml");
    let document = directory.path().join("paper.pdf");
    let mut library = crate::state::Library::default();
    library.remember(crate::state::ReadingState {
        path: document.clone(),
        page: 7,
        bookmarks: [3].into(),
        ..Default::default()
    });
    library.save(&state).unwrap();
    let original = "# My preferences\n[appearance]\ndark = true\n[display]\nlayout = 'continuous'\nfit = 'width'\nremember_documents = false\n";
    std::fs::write(&preferences, original).unwrap();
    let context = Context::default();
    let mut app = Pagewing::with_context(&context, Some(document.clone()), None, state.clone());
    assert!(app.library.dark);
    assert_eq!(app.reading.page, 0);
    assert!(app.reading.continuous);
    assert_eq!(app.reading.fit, Fit::Width);
    assert_eq!(app.reading.bookmarks, [3].into());
    app.library.dark = false;
    app.save();
    drop(app);
    assert_eq!(std::fs::read_to_string(&preferences).unwrap(), original);
    let edited = original
        .replace("continuous", "facing")
        .replace("'width'", "1.5");
    std::fs::write(&preferences, &edited).unwrap();
    let mut app = Pagewing::with_context(&context, Some(document), Some(2), state);
    assert!(app.library.dark);
    assert!(app.reading.facing && !app.reading.continuous);
    assert_eq!(app.reading.fit, Fit::Custom(1.5));
    assert_eq!(
        app.reading.page, 2,
        "an explicit starting page still takes precedence"
    );
    assert_eq!(app.reading.bookmarks, [3].into());
    let output = frame(&mut app, &context, Vec::new());
    assert!(!output.shapes.iter().any(
        |shape| matches!(&shape.shape, egui::Shape::Text(text) if text.galley.text() == "Settings")
    ));
}

#[test]
fn invalid_config_keeps_a_visible_error_after_opening_a_document() {
    let directory = tempfile::tempdir().unwrap();
    let preferences = directory.path().join("config.toml");
    std::fs::write(&preferences, "[shortcuts]\nnext_page = ['Escape']").unwrap();
    let context = Context::default();
    let mut app =
        Pagewing::with_context(&context, None, None, directory.path().join("library.json"));
    app.open(directory.path().join("paper.pdf"), &context);
    assert!(app.library.shortcuts.overrides.is_empty());
    assert!(app.configuration_error.as_ref().unwrap().contains("Escape"));
    assert_eq!(
        app.library.display,
        crate::settings::DisplaySettings::default()
    );
}

#[test]
fn display_defaults_respect_document_memory_and_keep_bookmarks() {
    let directory = tempfile::tempdir().unwrap();
    let config = directory.path().join("library.json");
    let first = directory.path().join("first.pdf");
    let second = directory.path().join("second.pdf");
    let mut library = crate::state::Library::default();
    library.display.layout = Layout::Facing;
    library.display.fit = Fit::Width;
    library.remember(crate::state::ReadingState {
        path: first.clone(),
        page: 7,
        fit: Fit::Height,
        continuous: true,
        bookmarks: [3].into(),
        ..Default::default()
    });
    library.save(&config).unwrap();
    let context = Context::default();
    let mut app = Pagewing::with_context(&context, Some(first.clone()), None, config);
    assert_eq!(app.reading.page, 7);
    assert_eq!(app.reading.fit, Fit::Height);
    assert!(app.reading.continuous);
    app.open(second, &context);
    assert!(app.reading.facing && !app.reading.continuous);
    assert_eq!(app.reading.fit, Fit::Width);
    app.library.display.remember_documents = false;
    app.open(first, &context);
    assert_eq!(app.reading.page, 0);
    assert!(app.reading.facing && !app.reading.continuous);
    assert_eq!(app.reading.fit, Fit::Width);
    assert_eq!(app.reading.bookmarks, [3].into());
}
