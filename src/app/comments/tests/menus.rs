use super::*;

fn full_frame(
    app: &mut Pagewing,
    context: &Context,
    size: Vec2,
    events: Vec<Event>,
) -> egui::FullOutput {
    context.run(
        RawInput {
            screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, size)),
            events,
            ..Default::default()
        },
        |context| eframe::App::update(app, context, &mut eframe::Frame::_new_kittest()),
    )
}

fn settle(app: &mut Pagewing, context: &Context, size: Vec2) -> egui::FullOutput {
    let mut output = egui::FullOutput::default();
    for _ in 0..5 {
        output = full_frame(app, context, size, Vec::new());
    }
    output
}

#[test]
fn opening_review_filter_owns_reader_keys_in_the_same_click_batch() {
    let (_root, context, mut app) = many_comments();
    let size = Vec2::new(1180.0, 820.0);
    let output = settle(&mut app, &context, size);
    let point = output
        .shapes
        .iter()
        .find_map(|shape| match &shape.shape {
            egui::Shape::Text(text) if text.galley.text() == "All statuses" => {
                Some(text.pos + text.galley.size() * 0.5)
            }
            _ => None,
        })
        .unwrap();
    let mut events = vec![Event::PointerMoved(point)];
    for pressed in [true, false] {
        events.push(Event::PointerButton {
            pos: point,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: Modifiers::NONE,
        });
    }
    events.extend(key(Key::J, Modifiers::SHIFT));
    full_frame(&mut app, &context, size, events);
    assert_eq!(app.nav.page, 0, "review menu must own the opening batch");
    assert!(egui::Popup::is_any_open(&context));
}

fn open(app: &mut Pagewing, context: &Context, size: Vec2, id: egui::Id, activation: Key) {
    context.memory_mut(|memory| memory.request_focus(id));
    settle(app, context, size);
    full_frame(app, context, size, key(activation, Modifiers::NONE));
    settle(app, context, size);
    assert!(egui::Popup::is_any_open(context));
}

fn visible_focus(context: &Context) -> egui::Id {
    let id = context
        .memory(|memory| memory.focused())
        .expect("menu item focus");
    let response = context.read_response(id).unwrap();
    assert!(
        response.interact_rect.contains_rect(response.rect),
        "{:?} clipped by {:?}",
        response.rect,
        response.interact_rect
    );
    assert!(context.content_rect().contains_rect(response.rect));
    id
}

#[test]
fn opening_keyboard_batch_keeps_the_first_navigation_keys() {
    let (_root, context, mut app) = many_comments();
    let size = Vec2::new(1180.0, 820.0);
    settle(&mut app, &context, size);
    context
        .memory_mut(|memory| memory.request_focus(app.input_buttons.comment_filter_menu.unwrap()));
    settle(&mut app, &context, size);
    let events = [Key::Enter, Key::ArrowDown, Key::ArrowDown]
        .into_iter()
        .flat_map(|value| key(value, Modifiers::NONE))
        .collect();
    full_frame(&mut app, &context, size, events);
    settle(&mut app, &context, size);
    full_frame(&mut app, &context, size, key(Key::Enter, Modifiers::NONE));
    assert_eq!(
        app.comments.view.filter.review,
        Some(ReviewStatus::Accepted)
    );
}

#[test]
fn keyboard_selects_every_review_filter_and_editor_status_at_minimum_size() {
    for size in [Vec2::splat(240.0), Vec2::new(1180.0, 820.0)] {
        let (_root, context, mut app) = many_comments();
        for index in 0usize..6 {
            settle(&mut app, &context, size);
            let id = app.input_buttons.comment_filter_menu.unwrap();
            open(
                &mut app,
                &context,
                size,
                id,
                if index % 2 == 0 {
                    Key::Enter
                } else {
                    Key::Space
                },
            );
            for row in 0..=index {
                visible_focus(&context);
                if row < index {
                    full_frame(
                        &mut app,
                        &context,
                        size,
                        key(Key::ArrowDown, Modifiers::NONE),
                    );
                    settle(&mut app, &context, size);
                }
            }
            full_frame(&mut app, &context, size, key(Key::Enter, Modifiers::NONE));
            assert_eq!(
                app.comments.view.filter.review,
                index.checked_sub(1).map(|i| ReviewStatus::ALL[i])
            );
            assert_eq!(app.nav.page, 0);
            assert!(!egui::Popup::is_any_open(&context));
        }
        let (_root, context, mut app) = saved_comment_editor();
        let original = std::fs::read(&app.reading.path).unwrap();
        for (index, review) in ReviewStatus::ALL.into_iter().enumerate() {
            settle(&mut app, &context, size);
            let id = app.input_buttons.comment_status_menu.unwrap();
            open(&mut app, &context, size, id, Key::Space);
            for row in 0..=index {
                visible_focus(&context);
                if row < index {
                    full_frame(
                        &mut app,
                        &context,
                        size,
                        key(Key::ArrowDown, Modifiers::NONE),
                    );
                    settle(&mut app, &context, size);
                }
            }
            full_frame(&mut app, &context, size, key(Key::Enter, Modifiers::NONE));
            let editor = app.comments.editor.as_ref().unwrap();
            assert_eq!(editor.review, review);
            assert_eq!(editor.contents, "Original comment");
            assert_eq!(app.comments.unsaved(), review != ReviewStatus::None);
            assert!(!app.comments.busy());
            assert!(!egui::Popup::is_any_open(&context));
        }
        assert_eq!(
            std::fs::read(&app.reading.path).unwrap(),
            original,
            "menu edits wait for Save"
        );
    }
}

#[test]
fn review_menus_wrap_keep_focus_on_resize_and_escape_preserves_the_editor() {
    for editing in [false, true] {
        let (_root, context, mut app) = saved_comment_editor();
        if !editing {
            app.comments.editor = None;
            app.comments.focus = false;
        }
        let wide = Vec2::new(1180.0, 820.0);
        settle(&mut app, &context, wide);
        let id = if editing {
            app.input_buttons.comment_status_menu
        } else {
            app.input_buttons.comment_filter_menu
        }
        .unwrap();
        open(&mut app, &context, wide, id, Key::Enter);
        let first = visible_focus(&context);
        full_frame(&mut app, &context, wide, key(Key::ArrowUp, Modifiers::NONE));
        settle(&mut app, &context, wide);
        let last = visible_focus(&context);
        assert_ne!(first, last);
        for size in [
            Vec2::splat(240.0),
            wide,
            Vec2::new(360.0, 300.0),
            Vec2::splat(240.0),
        ] {
            settle(&mut app, &context, size);
            assert!(egui::Popup::is_any_open(&context));
            assert_eq!(visible_focus(&context), last);
        }
        let small = Vec2::splat(240.0);
        full_frame(&mut app, &context, small, key(Key::Tab, Modifiers::NONE));
        settle(&mut app, &context, small);
        assert_eq!(visible_focus(&context), first);
        full_frame(&mut app, &context, small, key(Key::Tab, Modifiers::SHIFT));
        settle(&mut app, &context, small);
        assert_eq!(visible_focus(&context), last);
        full_frame(&mut app, &context, small, key(Key::Escape, Modifiers::NONE));
        assert!(!egui::Popup::is_any_open(&context));
        assert!(app.comments.open);
        assert_eq!(app.comments.editor.is_some(), editing);
        assert_eq!(app.comments.view.filter.review, None);
        if editing {
            assert_eq!(
                app.comments.editor.as_ref().unwrap().review,
                ReviewStatus::None
            );
        }
    }
}

#[test]
fn compact_more_menu_selects_reply_and_delete_with_logical_focus() {
    for delete in [false, true] {
        let (_root, context, mut app) = saved_comment_editor();
        let small = Vec2::splat(240.0);
        settle(&mut app, &context, small);
        let id = app.input_buttons.comment_actions_menu.unwrap();
        open(&mut app, &context, small, id, Key::Enter);
        let first = visible_focus(&context);
        full_frame(
            &mut app,
            &context,
            small,
            key(Key::ArrowUp, Modifiers::NONE),
        );
        settle(&mut app, &context, small);
        let last = visible_focus(&context);
        assert_ne!(first, last);
        for size in [Vec2::new(360.0, 300.0), small] {
            settle(&mut app, &context, size);
            assert_eq!(visible_focus(&context), last);
        }
        if !delete {
            full_frame(&mut app, &context, small, key(Key::Tab, Modifiers::NONE));
            settle(&mut app, &context, small);
            assert_eq!(visible_focus(&context), first);
        }
        full_frame(&mut app, &context, small, key(Key::Space, Modifiers::NONE));
        assert!(!egui::Popup::is_any_open(&context));
        if delete {
            wait(&mut app, &context, |app| {
                !app.comments.busy() && !app.loading && app.comments.loaded
            });
            assert!(app.comments.comments.items.is_empty());
            let pdf = crate::document::load(&app.reading.path, "").unwrap();
            assert!(crate::annotations::read(&pdf, || false).items.is_empty());
        } else {
            let editor = app.comments.editor.as_ref().unwrap();
            assert!(editor.slot.is_none() && editor.parent.is_some());
        }
    }
}
