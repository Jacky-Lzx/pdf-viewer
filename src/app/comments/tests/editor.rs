use super::*;

fn native_frame(app: &mut Pagewing, context: &Context, events: Vec<Event>) {
    let mut input = RawInput {
        screen_rect: Some(egui::Rect::from_min_size(
            egui::Pos2::ZERO,
            Vec2::new(1180.0, 820.0),
        )),
        events,
        ..Default::default()
    };
    eframe::App::raw_input_hook(app, context, &mut input);
    let _ = context.run(input, |context| {
        eframe::App::update(app, context, &mut eframe::Frame::_new_kittest());
    });
}

#[test]
fn typing_before_clicking_the_filter_in_one_native_batch_stays_in_the_draft() {
    for (prefix, expected) in [
        (vec![Event::Text(" draft".into())], "Original comment draft"),
        (vec![Event::Paste(" 草稿".into())], "Original comment 草稿"),
        (key(Key::Backspace, Modifiers::NONE), "Original commen"),
    ] {
        let (_root, context, mut app) = saved_comment_editor();
        let original = std::fs::read(&app.reading.path).unwrap();
        frame(&mut app, &context, key(Key::End, Modifiers::CTRL));
        let point = context
            .read_response(egui::Id::new("pagewing-comment-filter"))
            .unwrap()
            .rect
            .center();
        let mut events = prefix;
        events.push(Event::PointerMoved(point));
        for pressed in [true, false] {
            events.push(Event::PointerButton {
                pos: point,
                button: egui::PointerButton::Primary,
                pressed,
                modifiers: Modifiers::NONE,
            });
        }
        events.push(Event::Text("Original".into()));
        events.extend(key(Key::Enter, Modifiers::NONE));
        native_frame(&mut app, &context, events);
        assert_eq!(app.comments.editor.as_ref().unwrap().contents, expected);
        assert!(app.comments.view.filter.query.is_empty());
        assert!(!app.comments.filter_input.is_empty());
        native_frame(&mut app, &context, Vec::new());
        assert!(app.comments.filter_input.is_empty());
        assert_eq!(app.comments.editor.as_ref().unwrap().contents, expected);
        assert_eq!(app.comments.view.filter.query, "Original");
        assert!(app.comments.unsaved());
        assert_eq!(std::fs::read(&app.reading.path).unwrap(), original);
    }
}

fn body_cursor_visible(output: &egui::FullOutput, body: &str, index: usize) -> bool {
    output.shapes.iter().any(|shape| {
        let egui::Shape::Text(text) = &shape.shape else {
            return false;
        };
        if text.galley.text() != body {
            return false;
        }
        let cursor = text
            .galley
            .pos_from_cursor(egui::text::CCursor::new(index))
            .translate(text.pos.to_vec2());
        shape.clip_rect.contains_rect(cursor)
    })
}

#[test]
fn long_comment_caret_scrolls_without_hiding_filters_or_status_controls() {
    let (_root, context, mut app) = saved_comment_editor();
    let original = std::fs::read(&app.reading.path).unwrap();
    let body = format!("{}TAIL", "Long comment Straße 中文 line\n".repeat(400));
    app.comments.editor.as_mut().unwrap().contents = body.clone();
    frame(&mut app, &context, key(Key::End, Modifiers::CTRL));
    let mut output = egui::FullOutput::default();
    for _ in 0..5 {
        output = frame(&mut app, &context, Vec::new());
    }
    assert!(body_cursor_visible(&output, &body, body.chars().count()));
    for id in [
        egui::Id::new("pagewing-comment-filter"),
        app.input_buttons.comment_status_menu.unwrap(),
    ] {
        let response = context.read_response(id).unwrap();
        assert!(
            response.interact_rect.contains_rect(response.rect),
            "{response:?}"
        );
    }
    assert!(
        output.shapes.iter().any(|shape| {
            let egui::Shape::Text(text) = &shape.shape else {
                return false;
            };
            text.galley.text() == "Marked (Acrobat checkmark)"
                && shape
                    .clip_rect
                    .contains_rect(egui::Rect::from_min_size(text.pos, text.galley.size()))
        }),
        "the marked control must fit above the fixed action footer"
    );
    let editor = context
        .read_response(egui::Id::new("pagewing-comment-input"))
        .unwrap();
    assert!(editor.interact_rect.height() <= 284.0);
    let filter = context
        .read_response(egui::Id::new("pagewing-comment-filter"))
        .unwrap()
        .rect
        .center();
    let mut events = vec![Event::PointerMoved(filter)];
    for pressed in [true, false] {
        events.push(Event::PointerButton {
            pos: filter,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: Modifiers::NONE,
        });
    }
    events.push(Event::Text("Original".into()));
    frame(&mut app, &context, events);
    assert_eq!(app.comments.view.filter.query, "Original");
    assert_eq!(app.comments.editor.as_ref().unwrap().contents, body);
    assert!(app.comments.unsaved());
    assert_eq!(std::fs::read(&app.reading.path).unwrap(), original);
}

#[test]
fn long_comment_caret_remains_visible_after_resizing_to_minimum_windows() {
    let (_root, context, mut app) = saved_comment_editor();
    let body = format!("{}TAIL", "Long comment Straße 中文 line\n".repeat(400));
    app.comments.editor.as_mut().unwrap().contents = body.clone();
    for size in [
        Vec2::new(500.0, 400.0),
        Vec2::splat(240.0),
        Vec2::new(1180.0, 820.0),
    ] {
        for key in [Key::End, Key::Home] {
            context.memory_mut(|memory| {
                memory.request_focus(egui::Id::new("pagewing-comment-input"));
            });
            frame_size(&mut app, &context, self::key(key, Modifiers::CTRL), size);
            let mut output = egui::FullOutput::default();
            for _ in 0..5 {
                output = frame_size(&mut app, &context, Vec::new(), size);
            }
            let index = if key == Key::End {
                body.chars().count()
            } else {
                0
            };
            assert!(
                body_cursor_visible(&output, &body, index),
                "{key:?} at {size:?}: {:?}; {:?}",
                context.read_response(egui::Id::new("pagewing-comment-input")),
                output
                    .shapes
                    .iter()
                    .filter_map(|shape| {
                        let egui::Shape::Text(text) = &shape.shape else {
                            return None;
                        };
                        (text.galley.text() == body).then(|| {
                            (
                                shape.clip_rect,
                                text.galley
                                    .pos_from_cursor(egui::text::CCursor::new(index))
                                    .translate(text.pos.to_vec2()),
                            )
                        })
                    })
                    .collect::<Vec<_>>()
            );
            assert_eq!(app.comments.editor.as_ref().unwrap().contents, body);
        }
    }
}

#[test]
fn manually_scrolling_a_long_comment_stays_put_until_caret_navigation() {
    let (_root, context, mut app) = saved_comment_editor();
    let body = format!("{}TAIL", "Long comment Straße 中文 line\n".repeat(400));
    app.comments.editor.as_mut().unwrap().contents = body.clone();
    frame(&mut app, &context, key(Key::End, Modifiers::CTRL));
    for _ in 0..5 {
        frame(&mut app, &context, Vec::new());
    }
    let point = context
        .read_response(egui::Id::new("pagewing-comment-input"))
        .unwrap()
        .interact_rect
        .center();
    frame(
        &mut app,
        &context,
        vec![
            Event::PointerMoved(point),
            Event::MouseWheel {
                unit: egui::MouseWheelUnit::Point,
                delta: Vec2::new(0.0, 400.0),
                modifiers: Modifiers::NONE,
            },
        ],
    );
    for _ in 0..10 {
        let output = frame(&mut app, &context, Vec::new());
        assert!(!body_cursor_visible(&output, &body, body.chars().count()));
    }
    for navigation in [Key::Home, Key::End] {
        frame(&mut app, &context, key(navigation, Modifiers::CTRL));
        let mut output = egui::FullOutput::default();
        for _ in 0..5 {
            output = frame(&mut app, &context, Vec::new());
        }
        let index = if navigation == Key::End {
            body.chars().count()
        } else {
            0
        };
        assert!(body_cursor_visible(&output, &body, index));
    }
    assert_eq!(app.comments.editor.as_ref().unwrap().contents, body);
}

#[test]
fn read_only_long_comments_can_scroll_without_changing_their_text() {
    let (_root, context, mut app) = saved_comment_editor();
    let original = std::fs::read(&app.reading.path).unwrap();
    let body = format!("{}TAIL", "Long comment Straße 中文 line\n".repeat(400));
    let editor = app.comments.editor.as_mut().unwrap();
    editor.contents = body.clone();
    editor.locked = true;
    context.memory_mut(|memory| memory.surrender_focus(egui::Id::new("pagewing-comment-input")));
    let origin = |output: &egui::FullOutput| {
        output
            .shapes
            .iter()
            .find_map(|shape| {
                let egui::Shape::Text(text) = &shape.shape else {
                    return None;
                };
                (text.galley.text() == body).then_some(text.pos.y)
            })
            .unwrap()
    };
    let mut output = egui::FullOutput::default();
    for _ in 0..5 {
        output = frame(&mut app, &context, Vec::new());
    }
    let before = origin(&output);
    let point = context
        .read_response(egui::Id::new("pagewing-comment-input"))
        .unwrap()
        .interact_rect
        .center();
    frame(
        &mut app,
        &context,
        vec![
            Event::PointerMoved(point),
            Event::MouseWheel {
                unit: egui::MouseWheelUnit::Point,
                delta: Vec2::new(0.0, -400.0),
                modifiers: Modifiers::NONE,
            },
        ],
    );
    for _ in 0..10 {
        output = frame(&mut app, &context, Vec::new());
    }
    assert!(
        origin(&output) < before - 100.0,
        "read-only text must scroll inside its viewport"
    );
    assert_eq!(app.comments.editor.as_ref().unwrap().contents, body);
    assert_eq!(std::fs::read(&app.reading.path).unwrap(), original);
}
