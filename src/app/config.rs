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
