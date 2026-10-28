use super::*;
use crate::navigation::{Fit, Navigation};

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
        |context| eframe::App::update(app, context, &mut eframe::Frame::_new_kittest()),
    )
}

fn settle(app: &mut Pagewing, context: &egui::Context, size: egui::Vec2) -> egui::FullOutput {
    let mut output = egui::FullOutput::default();
    for _ in 0..5 {
        output = frame(app, context, size, Vec::new());
    }
    output
}

fn fixture() -> (tempfile::TempDir, egui::Context, Pagewing) {
    let root = tempfile::tempdir().unwrap();
    let context = egui::Context::default();
    let mut app = Pagewing::with_context(&context, None, None, root.path().join("library.json"));
    app.nav = Navigation::new(6, 2);
    app.dimensions = vec![[600.0, 800.0]; 6];
    (root, context, app)
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

fn activate(app: &mut Pagewing, context: &egui::Context, size: egui::Vec2) {
    let button = context
        .read_response(app.input_buttons.reading_controls.unwrap())
        .unwrap();
    assert!(context.content_rect().contains_rect(button.interact_rect));
    for pressed in [true, false] {
        frame(
            app,
            context,
            size,
            vec![
                egui::Event::PointerMoved(button.rect.center()),
                egui::Event::PointerButton {
                    pos: button.rect.center(),
                    button: egui::PointerButton::Primary,
                    pressed,
                    modifiers: egui::Modifiers::NONE,
                },
            ],
        );
    }
}

#[test]
fn hidden_bars_expand_every_layout_and_preserve_the_reading_position() {
    for (continuous, facing) in [(false, false), (true, false), (false, true)] {
        for fit in [Fit::Width, Fit::Custom(3.0)] {
            let (_root, context, mut app) = fixture();
            let size = egui::vec2(1180.0, 820.0);
            app.reading.fit = fit;
            app.dimensions.fill([600.0, 1200.0]);
            app.set_display_mode(continuous, facing);
            settle(&mut app, &context, size);
            app.pending_offset = Some([0.3, 0.6]);
            settle(&mut app, &context, size);
            let expected = app.current_offset();
            let viewport = app.viewport;
            for hidden in [true, false, true, false] {
                frame(&mut app, &context, size, key(egui::Key::F9));
                settle(&mut app, &context, size);
                assert_eq!(app.controls_hidden, hidden);
                assert_eq!(app.nav.page, 2);
                assert_eq!(app.reading.fit, fit);
                let added = app.viewport.y - viewport.y;
                assert!((added - if hidden { 92.0 } else { 0.0 }).abs() < 0.01);
                for (actual, expected) in app.current_offset().into_iter().zip(expected) {
                    assert!(
                        (actual - expected).abs() < 0.0001,
                        "{actual} != {expected}; {continuous}/{facing} {fit:?} hidden={hidden}"
                    );
                }
            }
        }
    }
}

#[test]
fn mouse_restoration_is_visible_at_minimum_size_with_disabled_shortcut() {
    for size in [egui::Vec2::splat(240.0), egui::vec2(1180.0, 820.0)] {
        let (_root, context, mut app) = fixture();
        app.library
            .shortcuts
            .overrides
            .insert("reading_controls".into(), vec![]);
        // Start hidden as the toolbar itself can scroll at minimum width.
        app.toggle_reading_controls(&context);
        settle(&mut app, &context, size);
        frame(&mut app, &context, size, key(egui::Key::F9));
        assert!(app.controls_hidden, "disabled F9 must stay disabled");
        let output = settle(&mut app, &context, size);
        assert!(output.shapes.iter().any(|shape| {
            matches!(&shape.shape, egui::Shape::Text(text)
                if text.galley.text() == "Show controls"
                    && shape.clip_rect.contains_rect(egui::Rect::from_min_size(text.pos, text.galley.size())))
        }));
        assert!(app.input_buttons.page.is_none());
        assert!(app.input_buttons.view_menu.is_none());
        assert!(app.status_region.is_none());
        activate(&mut app, &context, size);
        settle(&mut app, &context, size);
        assert!(!app.controls_hidden);
        assert!(app.input_buttons.page.is_some());
    }
}

#[test]
fn toolbar_hiding_and_keyboard_restoration_release_disappearing_focus() {
    let (_root, context, mut app) = fixture();
    let size = egui::vec2(1180.0, 820.0);
    settle(&mut app, &context, size);
    activate(&mut app, &context, size);
    settle(&mut app, &context, size);
    assert!(app.controls_hidden);
    context.memory_mut(|memory| memory.request_focus(app.input_buttons.reading_controls.unwrap()));
    frame(&mut app, &context, size, key(egui::Key::Space));
    settle(&mut app, &context, size);
    assert!(!app.controls_hidden);
    assert!(app.page_input.is_none());
    assert_eq!(app.nav.page, 2);
    assert_eq!(app.pan, egui::Vec2::ZERO);
}

#[test]
fn search_draft_survives_mouse_toggles_and_typing_keeps_its_shortcuts() {
    let (_root, context, mut app) = fixture();
    let size = egui::vec2(1180.0, 820.0);
    app.find();
    app.query = "Search draft".into();
    settle(&mut app, &context, size);
    frame(&mut app, &context, size, key(egui::Key::F9));
    assert!(
        !app.controls_hidden,
        "reader shortcuts do not run inside text inputs"
    );
    for hidden in [true, false] {
        activate(&mut app, &context, size);
        settle(&mut app, &context, size);
        assert_eq!(app.controls_hidden, hidden);
        assert!(app.search_open);
        assert_eq!(app.query, "Search draft");
        assert!(
            context
                .read_response(egui::Id::new("pagewing-search-input"))
                .is_some()
        );
    }
}

#[test]
fn hidden_bars_keep_deletion_recovery_and_modal_error_actions() {
    let (_root, context, mut app) = fixture();
    let size = egui::Vec2::splat(240.0);
    app.set_bookmark(4, true);
    app.remove_bookmark(4);
    app.controls_hidden = true;
    app.configuration_error = Some("Invalid setting".into());
    settle(&mut app, &context, size);
    assert!(app.input_buttons.restore_deleted.is_some());
    assert!(app.input_buttons.copy_config.is_some());
    assert!(app.input_buttons.dismiss_config.is_some());
    let id = app.input_buttons.restore_deleted.unwrap();
    context.memory_mut(|memory| memory.request_focus(id));
    frame(&mut app, &context, size, key(egui::Key::Enter));
    assert!(app.reading.bookmarks.contains(&4));
    assert_eq!(app.nav.page, 2);
    assert!(app.controls_hidden);
}

#[test]
fn remapped_shortcut_replaces_f9_and_comment_drafts_stay_open() {
    let (_root, context, mut app) = fixture();
    let size = egui::vec2(1180.0, 820.0);
    app.library.shortcuts.overrides.insert(
        "reading_controls".into(),
        vec![crate::settings::Binding::parse_config("F8").unwrap()],
    );
    settle(&mut app, &context, size);
    frame(&mut app, &context, size, key(egui::Key::F9));
    assert!(!app.controls_hidden);
    frame(&mut app, &context, size, key(egui::Key::F8));
    assert!(app.controls_hidden);
    app.comments.loaded = true;
    app.comments.open = true;
    app.comments.editor = Some(crate::annotations::Comment {
        page: 2,
        slot: None,
        parent: None,
        review: Default::default(),
        marked: false,
        states: Vec::new(),
        kind: "Text".into(),
        contents: "Unsaved comment".into(),
        author: "Reader".into(),
        rects: Vec::new(),
        has_appearance: false,
        truncated: false,
        locked: false,
    });
    settle(&mut app, &context, size);
    for hidden in [false, true] {
        activate(&mut app, &context, size);
        settle(&mut app, &context, size);
        assert_eq!(app.controls_hidden, hidden);
        assert!(app.comments.open && app.comments.unsaved());
        assert_eq!(
            app.comments.editor.as_ref().unwrap().contents,
            "Unsaved comment"
        );
    }
}

#[test]
fn a_native_toggle_batch_releases_old_bar_focus_before_later_activation() {
    let (_root, context, mut app) = fixture();
    let size = egui::vec2(1180.0, 820.0);
    settle(&mut app, &context, size);
    context.memory_mut(|memory| memory.request_focus(app.input_buttons.page.unwrap()));
    settle(&mut app, &context, size);
    let mut input = egui::RawInput {
        screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, size)),
        events: key(egui::Key::F9)
            .into_iter()
            .chain(key(egui::Key::Enter))
            .collect(),
        ..Default::default()
    };
    for _ in 0..6 {
        eframe::App::raw_input_hook(&mut app, &context, &mut input);
        let _ = context.run(input, |context| {
            eframe::App::update(&mut app, context, &mut eframe::Frame::_new_kittest());
        });
        input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, size)),
            ..Default::default()
        };
    }
    assert!(app.controls_hidden);
    assert!(
        app.page_input.is_none(),
        "Enter must not activate the hidden Page button"
    );
    assert_eq!(app.nav.page, 2);
}
