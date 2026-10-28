mod actions;
mod filtering;
mod menus;
mod overlays;

use super::*;
use egui::{Context, Event, Key, Modifiers, RawInput};
use std::time::{Duration, Instant};

fn frame(app: &mut Pagewing, context: &Context, events: Vec<Event>) -> egui::FullOutput {
    frame_size(app, context, events, Vec2::new(1180.0, 820.0))
}

fn frame_size(
    app: &mut Pagewing,
    context: &Context,
    events: Vec<Event>,
    size: Vec2,
) -> egui::FullOutput {
    context.run(
        RawInput {
            screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, size)),
            events,
            ..Default::default()
        },
        |context| {
            if !app.deferred_input.is_empty() {
                let mut events = std::mem::take(&mut app.deferred_input);
                context.input_mut(|input| {
                    events.append(&mut input.events);
                    input.events = events;
                });
            }
            app.events(context);
            app.keys(context);
            app.toolbar(context);
            app.status(context);
            app.comments_sidebar(context);
            app.sidebar(context);
            app.canvas(context);
        },
    )
}

fn key(key: Key, modifiers: Modifiers) -> Vec<Event> {
    vec![
        Event::Key {
            key,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers,
        },
        Event::Key {
            key,
            physical_key: None,
            pressed: false,
            repeat: false,
            modifiers,
        },
    ]
}

fn wait(app: &mut Pagewing, context: &Context, condition: impl Fn(&Pagewing) -> bool) {
    let deadline = Instant::now() + Duration::from_secs(5);
    while !condition(app) {
        assert!(Instant::now() < deadline, "UI worker did not finish");
        frame(app, context, Vec::new());
        std::thread::sleep(Duration::from_millis(5));
    }
}

fn click_text(app: &mut Pagewing, context: &Context, label: &str) {
    click_text_size(app, context, label, Vec2::new(1180.0, 820.0));
}

fn click_text_size(app: &mut Pagewing, context: &Context, label: &str, size: Vec2) {
    let output = frame_size(app, context, Vec::new(), size);
    // Prefer footer actions over matching card previews. An open menu
    // paints above both, so choose its last matching label instead.
    let mut points = output.shapes.iter().filter_map(|shape| {
        if let egui::Shape::Text(text) = &shape.shape
            && text.galley.text() == label
        {
            Some(text.pos + text.galley.size() * 0.5)
        } else {
            None
        }
    });
    let point = if egui::Popup::is_any_open(context) {
        points.next_back()
    } else {
        points.next()
    }
    .unwrap_or_else(|| {
        let texts: Vec<_> = output
            .shapes
            .iter()
            .filter_map(|shape| match &shape.shape {
                egui::Shape::Text(text) => Some((text.galley.text(), text.pos)),
                _ => None,
            })
            .collect();
        panic!("Missing visible button: {label} at {size:?}; {texts:?}")
    });
    for pressed in [true, false] {
        frame_size(
            app,
            context,
            vec![
                Event::PointerMoved(point),
                Event::PointerButton {
                    pos: point,
                    button: egui::PointerButton::Primary,
                    pressed,
                    modifiers: Modifiers::NONE,
                },
            ],
            size,
        );
    }
}

fn saved_comment_editor() -> (tempfile::TempDir, Context, Pagewing) {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("comments.pdf");
    std::fs::write(&path, include_bytes!("../../../assets/pagewing-guide.pdf")).unwrap();
    let pdf = crate::document::load(&path, "").unwrap();
    crate::annotations::save(
        &pdf,
        &Comment {
            page: 0,
            slot: None,
            parent: None,
            review: ReviewStatus::None,
            marked: false,
            states: Vec::new(),
            kind: "Highlight".into(),
            contents: "Original comment".into(),
            author: "Pagewing".into(),
            rects: vec![hayro::kurbo::Rect::new(50.0, 80.0, 150.0, 100.0)],
            has_appearance: false,
            truncated: false,
            locked: false,
        },
        &path,
    )
    .unwrap();
    let context = Context::default();
    let mut app = Pagewing::with_context(
        &context,
        Some(path.clone()),
        None,
        root.path().join("library.json"),
    );
    wait(&mut app, &context, |app| app.comments.loaded);
    app.edit_comment(0, &context);
    for _ in 0..3 {
        frame(&mut app, &context, Vec::new());
    }
    assert!(!app.comments.unsaved());
    (root, context, app)
}

#[test]
fn pending_editor_text_survives_document_shortcuts() {
    for (action_key, modifiers) in [
        (Key::W, Modifiers::COMMAND),
        (Key::R, Modifiers::COMMAND),
        (Key::T, Modifiers::COMMAND | Modifiers::SHIFT),
    ] {
        for (edits, expected) in [
            (vec![Event::Text("Final draft 📝".into())], "Final draft 📝"),
            (vec![Event::Paste("Pasted draft".into())], "Pasted draft"),
            (vec![Event::Paste("\n".into())], "\n"),
            (vec![Event::Cut], ""),
            (key(Key::Backspace, Modifiers::NONE), ""),
            (key(Key::Delete, Modifiers::NONE), ""),
            (key(Key::Enter, Modifiers::NONE), "\n"),
            (
                vec![
                    Event::Ime(egui::ImeEvent::Enabled),
                    Event::Ime(egui::ImeEvent::Preedit("草稿".into())),
                    Event::Ime(egui::ImeEvent::Commit("草稿".into())),
                ],
                "草稿",
            ),
        ] {
            let (root, context, mut app) = saved_comment_editor();
            let path = app.reading.path.clone();
            let target = root.path().join("closed.pdf");
            app.last_closed = Some(target.clone());
            let mut events = key(Key::A, Modifiers::COMMAND);
            events.extend(edits);
            let mut action = key(action_key, modifiers);
            // Shortcut-generated text must be consumed without swallowing
            // the user's earlier draft in this same event batch.
            action.insert(1, Event::Text(format!("{action_key:?}")));
            events.extend(action);
            frame(&mut app, &context, events);
            assert_eq!(app.reading.path, path, "{action_key:?}: {expected:?}");
            assert!(!app.loading, "{action_key:?}");
            assert!(
                app.comments.unsaved(),
                "{action_key:?}: {expected:?}, {:?}",
                app.comments.editor
            );
            assert_eq!(
                app.comments.editor.as_ref().unwrap().contents,
                expected,
                "{action_key:?}"
            );
            assert_eq!(app.last_closed.as_ref(), Some(&target));
            // A previous input batch must not block a later clean close.
            app.comments.editor.as_mut().unwrap().contents = "Original comment".into();
            let mut close = key(Key::W, Modifiers::COMMAND);
            close.insert(1, Event::Text("w".into()));
            frame(&mut app, &context, close);
            assert_eq!(app.nav.count, 0);
            assert!(app.comments.editor.is_none());
        }
    }
}

#[test]
fn pointer_transitions_preserve_pending_editor_input() {
    for action in ["open", "reload", "close", "hide"] {
        let (root, context, mut app) = saved_comment_editor();
        let path = app.reading.path.clone();
        let mut button = egui::Rect::NOTHING;
        let mut draw = |events| {
            let _ = context.run(
                RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        Vec2::new(1180.0, 820.0),
                    )),
                    events,
                    ..Default::default()
                },
                |context| {
                    app.events(context);
                    egui::TopBottomPanel::top("transition").show(context, |ui| {
                        let response = ui.button(action);
                        button = response.rect;
                        if response.clicked() {
                            match action {
                                "open" => app.open(root.path().join("another.pdf"), context),
                                "reload" => app.reload(context),
                                "close" => app.close_document(context),
                                "hide" => app.set_comments_open(context, false),
                                _ => unreachable!(),
                            }
                        }
                    });
                    app.comments_sidebar(context);
                    app.canvas(context);
                },
            );
            button.center()
        };
        let mut pos = egui::Pos2::ZERO;
        for _ in 0..3 {
            pos = draw(Vec::new());
        }
        let mut events = key(Key::A, Modifiers::COMMAND);
        events.push(Event::Text("Pointer draft 📝".into()));
        events.push(Event::PointerMoved(pos));
        events.extend([true, false].map(|pressed| Event::PointerButton {
            pos,
            pressed,
            button: egui::PointerButton::Primary,
            modifiers: Modifiers::NONE,
        }));
        draw(events);
        assert_eq!(app.reading.path, path, "{action}");
        assert!(!app.loading, "{action}");
        assert_eq!(
            app.comments.editor.as_ref().unwrap().contents,
            "Pointer draft 📝",
            "{action}"
        );
        assert_eq!(app.comments.open, action != "hide");
    }
}

#[test]
fn toolbar_history_preserves_the_comment_draft_and_its_page() {
    let (_root, context, mut app) = saved_comment_editor();
    app.jump(2);
    app.comments.editor.as_mut().unwrap().contents = "Unsaved draft 📝".into();
    let mut position = egui::Pos2::ZERO;
    let mut draw = |events| {
        let _ = context.run(
            RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    Vec2::new(1180.0, 820.0),
                )),
                events,
                ..Default::default()
            },
            |context| {
                app.events(context);
                app.keys(context);
                app.toolbar(context);
                position = context
                    .read_response(app.input_buttons.history_back.unwrap())
                    .unwrap()
                    .rect
                    .center();
                app.comments_sidebar(context);
                app.canvas(context);
            },
        );
        position
    };
    let mut pos = egui::Pos2::ZERO;
    for _ in 0..3 {
        pos = draw(Vec::new());
    }
    let mut events = key(Key::A, Modifiers::COMMAND);
    events.push(Event::Text("Final unsaved draft 📝".into()));
    events.push(Event::PointerMoved(pos));
    events.extend([true, false].map(|pressed| Event::PointerButton {
        pos,
        pressed,
        button: egui::PointerButton::Primary,
        modifiers: Modifiers::NONE,
    }));
    draw(events);
    assert_eq!(app.nav.page, 0);
    assert!(app.comments.open && app.comments.unsaved());
    let draft = app.comments.editor.as_ref().unwrap();
    assert_eq!(draft.contents, "Final unsaved draft 📝");
    assert_eq!(draft.page, 0);
    assert!(!app.comments.saving && !app.loading);
}

#[test]
fn queued_save_protects_the_editor_and_final_input_until_the_worker_starts() {
    for action in [
        Some((Key::W, Modifiers::COMMAND)),
        Some((Key::R, Modifiers::COMMAND)),
        Some((Key::T, Modifiers::COMMAND | Modifiers::SHIFT)),
        None,
    ] {
        let (root, context, mut app) = saved_comment_editor();
        let path = app.reading.path.clone();
        let output = frame(&mut app, &context, Vec::new());
        let target = root.path().join("closed.pdf");
        app.last_closed = Some(target.clone());
        let expected = if action.is_some() {
            "Final comment 📝"
        } else {
            "Original comment"
        };
        let mut events = Vec::new();
        if action.is_some() {
            events.extend(key(Key::A, Modifiers::COMMAND));
            events.push(Event::Text(expected.into()));
        }
        events.extend(key(Key::Enter, Modifiers::COMMAND));
        if let Some((key, modifiers)) = action {
            events.extend(self::key(key, modifiers));
        } else {
            let pos = output
                .shapes
                .iter()
                .find_map(|shape| match &shape.shape {
                    egui::Shape::Text(text) if text.galley.text() == "Discard / close editor" => {
                        Some(text.pos + text.galley.size() * 0.5)
                    }
                    _ => None,
                })
                .expect("visible discard action");
            events.push(Event::PointerMoved(pos));
            events.extend([true, false].map(|pressed| Event::PointerButton {
                pos,
                pressed,
                button: egui::PointerButton::Primary,
                modifiers: Modifiers::NONE,
            }));
        }
        frame(&mut app, &context, events);
        assert_eq!(app.reading.path, path, "{action:?}");
        assert!(!app.loading, "{action:?}");
        assert!(app.comments.saving, "{action:?}");
        assert_eq!(
            app.comments.editor.as_ref().unwrap().contents,
            expected,
            "{action:?}"
        );
        assert_eq!(app.last_closed.as_ref(), Some(&target));
        wait(&mut app, &context, |app| {
            !app.comments.saving && !app.loading && app.comments.loaded
        });
        assert_eq!(app.comments.comments.items[0].contents, expected);
    }
}

#[test]
fn reply_buttons_nested_threads_status_controls_and_draft_protection() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("threads.pdf");
    std::fs::write(&path, include_bytes!("../../../assets/pagewing-guide.pdf")).unwrap();
    let pdf = crate::document::load(&path, "").unwrap();
    crate::annotations::save(
        &pdf,
        &Comment {
            page: 0,
            slot: None,
            parent: None,
            review: ReviewStatus::None,
            marked: false,
            states: Vec::new(),
            kind: "Highlight".into(),
            contents: "Root comment".into(),
            author: "Pagewing".into(),
            rects: vec![hayro::kurbo::Rect::new(50.0, 80.0, 150.0, 100.0)],
            has_appearance: false,
            truncated: false,
            locked: false,
        },
        &path,
    )
    .unwrap();
    let context = Context::default();
    let mut app = Pagewing::with_context(
        &context,
        Some(path.clone()),
        Some(0),
        root.path().join("library.json"),
    );
    wait(&mut app, &context, |app| app.comments.loaded);
    app.edit_comment(0, &context);
    for depth in 1..=2 {
        click_text(&mut app, &context, "Reply");
        let editor = app.comments.editor.as_ref().unwrap();
        assert!(editor.slot.is_none() && editor.parent.is_some());
        assert!(app.comments.unsaved());
        frame(
            &mut app,
            &context,
            vec![Event::Text(format!("Nested reply {depth}"))],
        );
        click_text(&mut app, &context, "Save comment");
        wait(&mut app, &context, |app| {
            !app.comments.busy()
                && !app.loading
                && app.comments.loaded
                && app.comments.editor.is_none()
        });
        assert_eq!(app.comments.comments.items.len(), depth + 1);
        app.edit_comment(depth, &context);
    }
    assert_eq!(
        app.comments.comments.thread_rows(),
        [(0, 0), (1, 1), (2, 2)]
    );
    click_text(&mut app, &context, "Status: None");
    click_text(&mut app, &context, "Completed / resolved");
    assert_eq!(
        app.comments.editor.as_ref().unwrap().review,
        ReviewStatus::Completed
    );
    assert!(app.comments.unsaved(), "status changes are drafts too");
    app.reload(&context);
    assert!(!app.reloading, "status drafts must defer reloads");
    app.edit_comment(0, &context);
    assert_eq!(
        app.comments.editor.as_ref().unwrap().slot,
        app.comments.comments.items[2].slot,
        "changing selection must preserve the draft"
    );
    click_text(&mut app, &context, "Marked (Acrobat checkmark)");
    assert!(app.comments.editor.as_ref().unwrap().marked);
    app.comments.view.filter.query = "Nested".into();
    click_text(&mut app, &context, "Save comment");
    wait(&mut app, &context, |app| {
        !app.comments.busy() && !app.loading && app.comments.loaded && app.comments.editor.is_none()
    });
    assert_eq!(
        app.comments.comments.items[2].review,
        ReviewStatus::Completed
    );
    assert!(app.comments.comments.items[2].marked);
    assert_eq!(
        app.comments.view.filter.query, "Nested",
        "saving must retain the active filters"
    );
    assert_eq!(app.comments.comments.items.len(), 3);
    app.edit_comment(1, &context);
    click_text(&mut app, &context, "Delete reply & replies");
    wait(&mut app, &context, |app| {
        !app.comments.busy() && !app.loading && app.comments.loaded && app.comments.editor.is_none()
    });
    assert_eq!(app.comments.comments.items.len(), 1);
    assert_eq!(app.comments.comments.items[0].contents, "Root comment");
}

#[test]
fn select_add_type_save_and_reread_comments_through_the_ui_worker() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("source.pdf");
    std::fs::write(&path, include_bytes!("../../../assets/pagewing-guide.pdf")).unwrap();
    // The reader canonicalizes paths on open (e.g. /var -> /private/var on macOS).
    let path = path.canonicalize().unwrap();
    let context = Context::default();
    let mut app = Pagewing::with_context(
        &context,
        Some(path.clone()),
        Some(1),
        root.path().join("library.json"),
    );
    wait(&mut app, &context, |app| {
        app.comments.loaded && app.nav.count > 0
    });
    assert!(!app.comments.open, "comments must stay closed on open");
    frame(&mut app, &context, key(Key::A, Modifiers::COMMAND));
    wait(&mut app, &context, |app| !app.selection.rects.is_empty());
    // Exercise the same visible toolbar button a mouse user activates.
    let output = frame(&mut app, &context, Vec::new());
    let button = output
        .shapes
        .iter()
        .find_map(|shape| {
            if let egui::Shape::Text(text) = &shape.shape
                && text.galley.text() == "Add comment"
            {
                Some(text.pos + text.galley.size() * 0.5)
            } else {
                None
            }
        })
        .expect("Add comment toolbar button");
    for pressed in [true, false] {
        frame(
            &mut app,
            &context,
            vec![
                Event::PointerMoved(button),
                Event::PointerButton {
                    pos: button,
                    button: egui::PointerButton::Primary,
                    pressed,
                    modifiers: Modifiers::NONE,
                },
            ],
        );
    }
    assert!(app.comments.editor.is_some());
    frame(&mut app, &context, Vec::new());
    frame(
        &mut app,
        &context,
        vec![Event::Paste(
            "中文 comment\nj k / ? should stay in the comment".into(),
        )],
    );
    assert_eq!(
        app.comments.editor.as_ref().unwrap().contents,
        "中文 comment\nj k / ? should stay in the comment"
    );
    assert_eq!(app.nav.page, 1);
    let sidebar = app.library.sidebar;
    frame(&mut app, &context, key(Key::Tab, Modifiers::NONE));
    assert_eq!(
        app.library.sidebar, sidebar,
        "Tab must belong to the comment editor"
    );
    app.close_document(&context);
    assert_eq!(app.nav.count, 6, "unsaved comment must block closing");
    app.reload(&context);
    assert!(!app.reloading, "unsaved comment must block reloading");
    frame(&mut app, &context, key(Key::Escape, Modifiers::NONE));
    assert!(!app.comments.open);
    assert!(app.comments.unsaved());
    frame(
        &mut app,
        &context,
        key(Key::M, Modifiers::COMMAND | Modifiers::SHIFT),
    );
    assert!(app.comments.open);
    app.comments.focus = true;
    let mut events = vec![Event::Text(" last 📝".into())];
    events.extend(key(Key::Enter, Modifiers::COMMAND));
    events.extend(key(Key::M, Modifiers::COMMAND | Modifiers::SHIFT));
    events.extend(key(Key::J, Modifiers::SHIFT));
    frame(&mut app, &context, events);
    wait(&mut app, &context, |app| {
        !app.comments.saving && app.comments.loaded && !app.loading && app.comments.editor.is_none()
    });
    assert_eq!(
        app.reading.path, path,
        "saving must keep the same document path"
    );
    assert_eq!(
        app.nav.page, 2,
        "saving preserves navigation after the save request"
    );
    assert_eq!(app.comments.comments.items.len(), 1);
    assert!(
        !app.comments.open,
        "a later hide request must keep the comments sidebar closed"
    );
    app.edit_comment(0, &context);
    assert_eq!(
        app.comments.editor.as_ref().unwrap().contents,
        "中文 comment\nj k / ? should stay in the comment last 📝"
    );
    assert!(!app.comments.unsaved());
    assert_ne!(
        std::fs::read(&path).unwrap(),
        include_bytes!("../../../assets/pagewing-guide.pdf")
    );
    assert!(!root.path().join("source-annotated.pdf").exists());
    // Failed deletion must leave the selected comment available for retry.
    let snapshot = std::fs::read(&path).unwrap();
    std::fs::write(&path, b"changed by another application").unwrap();
    app.delete_comment();
    wait(&mut app, &context, |app| !app.comments.busy());
    assert!(
        app.error
            .as_ref()
            .is_some_and(|error| error.message.contains("changed on disk"))
    );
    assert!(app.comments.editor.is_some());
    assert_eq!(
        std::fs::read(&path).unwrap(),
        b"changed by another application"
    );
    std::fs::write(&path, snapshot).unwrap();
    let output = frame(&mut app, &context, Vec::new());
    let delete_button = output
        .shapes
        .iter()
        .find_map(|shape| {
            if let egui::Shape::Text(text) = &shape.shape
                && text.galley.text() == "Delete comment & highlight"
            {
                Some(text.pos + text.galley.size() * 0.5)
            } else {
                None
            }
        })
        .expect("Delete comment button");
    for pressed in [true, false] {
        frame(
            &mut app,
            &context,
            vec![
                Event::PointerMoved(delete_button),
                Event::PointerButton {
                    pos: delete_button,
                    button: egui::PointerButton::Primary,
                    pressed,
                    modifiers: Modifiers::NONE,
                },
            ],
        );
    }
    assert!(
        app.comments.saving,
        "the visible delete button must submit deletion"
    );
    wait(&mut app, &context, |app| {
        !app.comments.busy() && !app.loading && app.comments.loaded
    });
    assert!(app.comments.editor.is_none());
    assert!(app.comments.comments.items.is_empty());
    assert!(app.comments.open);
    assert_eq!(app.nav.page, 1, "deletion must preserve the current page");
    let saved = crate::document::load(&path, "").unwrap();
    assert!(crate::annotations::read(&saved, || false).items.is_empty());
}

fn many_comments() -> (tempfile::TempDir, Context, Pagewing) {
    let (root, context, mut app) = saved_comment_editor();
    let template = app.comments.comments.items[0].clone();
    app.comments.editor = None;
    app.comments.focus = false;
    app.comments.set_comments(Comments {
        items: (0..80)
            .map(|index| Comment {
                slot: Some(index),
                contents: format!("Row {index}"),
                ..template.clone()
            })
            .collect(),
        limited: false,
    });
    (root, context, app)
}

fn visible_selected_card(
    context: &Context,
    output: &egui::FullOutput,
    body: &str,
) -> Option<egui::Rect> {
    let text_rect = output.shapes.iter().find_map(|shape| match &shape.shape {
        egui::Shape::Text(text) if text.galley.text() == body => {
            Some(egui::Rect::from_min_size(text.pos, text.galley.size()))
        }
        _ => None,
    })?;
    let fill = context.style().visuals.selection.bg_fill;
    output.shapes.iter().find_map(|shape| match &shape.shape {
        egui::Shape::Rect(rect)
            if rect.fill == fill
                && rect.rect.height() > text_rect.height() * 2.0
                && rect.rect.contains_rect(text_rect)
                && shape.clip_rect.contains_rect(rect.rect) =>
        {
            Some(rect.rect)
        }
        _ => None,
    })
}

fn fully_visible_cards(context: &Context, output: &egui::FullOutput, body: &str) -> usize {
    let first = visible_selected_card(context, output, body).unwrap();
    let body_offset = output
        .shapes
        .iter()
        .find_map(|shape| match &shape.shape {
            egui::Shape::Text(text) if text.galley.text() == body => Some(text.pos.y - first.top()),
            _ => None,
        })
        .unwrap();
    output
        .shapes
        .iter()
        .filter(|shape| match &shape.shape {
            egui::Shape::Text(text)
                if text
                    .galley
                    .text()
                    .strip_prefix("Row ")
                    .is_some_and(|row| row.parse::<usize>().is_ok()) =>
            {
                let card = egui::Rect::from_min_size(
                    egui::pos2(first.left(), text.pos.y - body_offset),
                    first.size(),
                );
                // ScrollArea expands its paint clip beyond the viewport for
                // strokes. Exclude that margin, allowing one pixel for layout
                // rounding, so a card in the paint overflow is not counted.
                let viewport = shape.clip_rect.shrink2(egui::vec2(
                    0.0,
                    context.style().visuals.clip_rect_margin - 1.0 / context.pixels_per_point(),
                ));
                viewport.contains_rect(card)
            }
            _ => false,
        })
        .count()
}

fn focused_card(
    context: &Context,
    output: &egui::FullOutput,
    body: &str,
) -> Option<egui::Response> {
    let label = output.shapes.iter().find_map(|shape| match &shape.shape {
        egui::Shape::Text(text) if text.galley.text() == body => {
            Some(egui::Rect::from_min_size(text.pos, text.galley.size()))
        }
        _ => None,
    })?;
    context
        .memory(|memory| memory.focused())
        .and_then(|id| context.read_response(id))
        .filter(|response| response.rect.height() <= 100.0 && response.rect.contains_rect(label))
}

#[test]
fn tab_reaches_virtual_comment_cards_and_keeps_the_target_when_resizing() {
    let (_root, context, mut app) = many_comments();
    let original = std::fs::read(&app.reading.path).unwrap();
    let page = app.nav.page;
    let size = Vec2::new(1180.0, 820.0);
    context.memory_mut(|memory| memory.request_focus(egui::Id::new("pagewing-comment-filter")));
    frame(&mut app, &context, Vec::new());
    let mut focused = None;
    for _ in 0..50 {
        frame(&mut app, &context, key(Key::Tab, Modifiers::NONE));
        let mut output = egui::FullOutput::default();
        for _ in 0..4 {
            output = frame(&mut app, &context, Vec::new());
        }
        if let Some(response) = focused_card(&context, &output, "Row 24") {
            focused = Some(response.id);
            break;
        }
    }
    let focused = focused.expect("Tab must reach a comment beyond the initial virtual range");
    for resized in [Vec2::new(500.0, 400.0), Vec2::new(240.0, 240.0), size] {
        for _ in 0..6 {
            frame_size(&mut app, &context, Vec::new(), resized);
        }
        assert_eq!(context.memory(|memory| memory.focused()), Some(focused));
        assert_eq!(
            app.nav.page, page,
            "Tab and resizing must preserve the reading page"
        );
        assert!(
            app.comments.editor.is_none(),
            "focusing must not open the editor"
        );
        let response = context.read_response(focused).unwrap();
        assert!(
            response
                .interact_rect
                .contains_rect(response.rect.shrink(1.0)),
            "the focused comment must be fully visible at {resized:?}"
        );
    }
    for target in (0..24).rev() {
        frame(&mut app, &context, key(Key::Tab, Modifiers::SHIFT));
        let mut output = egui::FullOutput::default();
        for _ in 0..4 {
            output = frame(&mut app, &context, Vec::new());
        }
        assert!(
            focused_card(&context, &output, &format!("Row {target}")).is_some(),
            "Shift+Tab must reach the preceding virtual card: Row {target}"
        );
    }
    frame(&mut app, &context, key(Key::Space, Modifiers::NONE));
    assert_eq!(app.comments.editor.as_ref().unwrap().contents, "Row 0");
    assert_eq!(std::fs::read(&app.reading.path).unwrap(), original);
}

#[test]
fn focused_comment_card_takes_resize_priority_over_an_open_editor() {
    let (_root, context, mut app) = many_comments();
    let original = std::fs::read(&app.reading.path).unwrap();
    let page = app.nav.page;
    app.edit_comment(0, &context);
    for _ in 0..4 {
        frame(&mut app, &context, Vec::new());
    }
    let mut focused = None;
    for _ in 0..8 {
        frame(&mut app, &context, key(Key::Tab, Modifiers::SHIFT));
        let mut output = egui::FullOutput::default();
        for _ in 0..4 {
            output = frame(&mut app, &context, Vec::new());
        }
        if let Some(response) = focused_card(&context, &output, "Row 1") {
            focused = Some(response.id);
            break;
        }
    }
    let focused = focused.expect("Shift+Tab must move from the editor into the list");
    app.comments.editor.as_mut().unwrap().contents = "Unsaved draft".into();
    for size in [Vec2::new(500.0, 400.0), Vec2::new(240.0, 240.0)] {
        for _ in 0..6 {
            frame_size(&mut app, &context, Vec::new(), size);
        }
        assert_eq!(context.memory(|memory| memory.focused()), Some(focused));
        let response = context.read_response(focused).unwrap();
        assert!(
            response
                .interact_rect
                .contains_rect(response.rect.shrink(1.0)),
            "{size:?}: card={:?}, visible={:?}",
            response.rect,
            response.interact_rect
        );
        assert_eq!(
            app.comments.editor.as_ref().unwrap().contents,
            "Unsaved draft"
        );
        assert_eq!(app.nav.page, page);
    }
    frame_size(
        &mut app,
        &context,
        key(Key::Space, Modifiers::NONE),
        Vec2::new(240.0, 240.0),
    );
    assert_eq!(
        app.comments.editor.as_ref().unwrap().contents,
        "Unsaved draft"
    );
    assert_eq!(context.memory(|memory| memory.focused()), Some(focused));
    app.comments.editor.as_mut().unwrap().contents = "Row 0".into();
    frame_size(
        &mut app,
        &context,
        key(Key::Space, Modifiers::NONE),
        Vec2::new(240.0, 240.0),
    );
    assert_eq!(app.comments.editor.as_ref().unwrap().contents, "Row 1");
    assert_eq!(std::fs::read(&app.reading.path).unwrap(), original);
}

#[test]
fn page_navigation_uses_visible_card_geometry_and_keeps_drafts_safe() {
    let (_root, context, mut app) = many_comments();
    let original = std::fs::read(&app.reading.path).unwrap();
    context.memory_mut(|memory| {
        memory.request_focus(egui::Id::new("pagewing-comment-filter"));
    });
    frame(&mut app, &context, vec![Event::Paste("Row".into())]);
    for size in [Vec2::new(1180.0, 820.0), Vec2::new(500.0, 400.0)] {
        frame_size(&mut app, &context, key(Key::Home, Modifiers::COMMAND), size);
        let mut output = egui::FullOutput::default();
        for _ in 0..6 {
            output = frame_size(&mut app, &context, Vec::new(), size);
        }
        let visible = fully_visible_cards(&context, &output, "Row 0");
        assert!(visible > 0);
        // Return to a different viewport, then resize and navigate in the
        // same input frame. The movement must use the destination layout.
        frame(&mut app, &context, Vec::new());
        let mut events = key(Key::Home, Modifiers::COMMAND);
        events.extend(key(Key::PageDown, Modifiers::NONE));
        frame_size(&mut app, &context, events, size);
        assert_eq!(app.comments.view.selected(), Some(visible), "{size:?}");
        for _ in 0..6 {
            output = frame_size(&mut app, &context, Vec::new(), size);
        }
        assert!(visible_selected_card(&context, &output, &format!("Row {visible}")).is_some());
        frame_size(&mut app, &context, key(Key::PageUp, Modifiers::NONE), size);
        assert_eq!(app.comments.view.selected(), Some(0));
    }
    let mut events = key(Key::End, Modifiers::COMMAND);
    events.extend(key(Key::PageDown, Modifiers::NONE));
    events.extend(key(Key::ArrowUp, Modifiers::NONE));
    events.extend(key(Key::Enter, Modifiers::NONE));
    frame(&mut app, &context, events);
    assert_eq!(app.comments.editor.as_ref().unwrap().contents, "Row 78");
    assert!(!app.comments.unsaved());
    for _ in 0..4 {
        frame(&mut app, &context, Vec::new());
    }
    context.memory_mut(|memory| {
        memory.request_focus(egui::Id::new("pagewing-comment-filter"));
    });
    frame(&mut app, &context, key(Key::End, Modifiers::COMMAND));
    let mut output = egui::FullOutput::default();
    for _ in 0..6 {
        output = frame(&mut app, &context, Vec::new());
    }
    let visible = fully_visible_cards(&context, &output, "Row 79");
    assert!(visible > 0);
    let mut events = key(Key::PageUp, Modifiers::NONE);
    events.extend(key(Key::Enter, Modifiers::NONE));
    frame(&mut app, &context, events);
    assert_eq!(app.comments.view.selected(), Some(79 - visible));
    assert_eq!(
        app.comments.editor.as_ref().unwrap().contents,
        format!("Row {}", 79 - visible)
    );
    for _ in 0..4 {
        frame(&mut app, &context, Vec::new());
    }
    let draft_slot = app.comments.editor.as_ref().unwrap().slot;
    app.comments.editor.as_mut().unwrap().contents = "unsaved draft".into();
    context.memory_mut(|memory| {
        memory.request_focus(egui::Id::new("pagewing-comment-filter"));
    });
    let mut events = key(Key::Home, Modifiers::COMMAND);
    events.extend(key(Key::PageUp, Modifiers::NONE));
    events.extend(key(Key::ArrowDown, Modifiers::NONE));
    events.extend(key(Key::Enter, Modifiers::NONE));
    frame(&mut app, &context, events);
    assert_eq!(app.comments.view.selected(), Some(1));
    assert_eq!(
        app.comments.editor.as_ref().unwrap().contents,
        "unsaved draft"
    );
    assert_eq!(app.comments.editor.as_ref().unwrap().slot, draft_slot);
    assert!(app.comments.unsaved());
    let mut events = key(Key::A, Modifiers::COMMAND);
    events.push(Event::Paste("missing".into()));
    events.extend(key(Key::End, Modifiers::COMMAND));
    events.extend(key(Key::PageDown, Modifiers::NONE));
    events.extend(key(Key::Enter, Modifiers::NONE));
    frame(&mut app, &context, events);
    assert!(app.comments.view.selected().is_none());
    assert_eq!(
        app.comments.editor.as_ref().unwrap().contents,
        "unsaved draft"
    );
    context.memory_mut(|memory| {
        memory.request_focus(egui::Id::new("pagewing-comment-input"));
    });
    let mut events = key(Key::Home, Modifiers::COMMAND);
    events.push(Event::Text("prefix ".into()));
    frame(&mut app, &context, events);
    assert_eq!(
        app.comments.editor.as_ref().unwrap().contents,
        "prefix unsaved draft"
    );
    assert!(app.comments.view.selected().is_none());
    assert_eq!(std::fs::read(&app.reading.path).unwrap(), original);
}

#[test]
fn resizing_keeps_the_selected_comment_visible_without_undoing_manual_scroll() {
    let (_root, context, mut app) = many_comments();
    context.memory_mut(|memory| {
        memory.request_focus(egui::Id::new("pagewing-comment-filter"));
    });
    frame(&mut app, &context, vec![Event::Paste("Row".into())]);
    app.comments.view.step(79);
    let mut output = egui::FullOutput::default();
    let size = Vec2::new(500.0, 400.0);
    for size in [Vec2::new(1180.0, 820.0), size] {
        for _ in 0..4 {
            output = frame_size(&mut app, &context, Vec::new(), size);
        }
        assert_eq!(app.comments.view.selected(), Some(79));
        assert_eq!(app.comments.view.filter.query, "Row");
        assert!(app.comment_filter_focused(&context));
        assert!(
            visible_selected_card(&context, &output, "Row 79").is_some(),
            "selected comment must remain visible at {size:?}"
        );
    }
    let pos = visible_selected_card(&context, &output, "Row 79")
        .unwrap()
        .center();
    frame_size(
        &mut app,
        &context,
        vec![
            Event::PointerMoved(pos),
            Event::MouseWheel {
                unit: egui::MouseWheelUnit::Point,
                delta: Vec2::new(0.0, 4000.0),
                modifiers: Modifiers::NONE,
            },
        ],
        size,
    );
    for _ in 0..30 {
        output = frame_size(&mut app, &context, Vec::new(), size);
    }
    assert!(
        visible_selected_card(&context, &output, "Row 79").is_none(),
        "manual scrolling must not be undone by the retained selection"
    );
    frame_size(&mut app, &context, key(Key::K, Modifiers::CTRL), size);
    for _ in 0..4 {
        output = frame_size(&mut app, &context, Vec::new(), size);
    }
    assert_eq!(app.comments.view.selected(), Some(78));
    let previous = visible_selected_card(&context, &output, "Row 78").unwrap();
    frame_size(&mut app, &context, key(Key::ArrowUp, Modifiers::NONE), size);
    for _ in 0..4 {
        output = frame_size(&mut app, &context, Vec::new(), size);
    }
    assert_eq!(app.comments.view.selected(), Some(77));
    let current = visible_selected_card(&context, &output, "Row 77").unwrap();
    assert!(
        current.top() < previous.top(),
        "a row already in view must not be moved to the top on each keypress"
    );
    assert_eq!(app.comments.view.filter.query, "Row");
    assert!(app.comment_filter_focused(&context));
}

#[test]
fn opening_and_resizing_the_editor_reveals_its_comment_card() {
    let (_root, context, mut app) = many_comments();
    for _ in 0..3 {
        frame(&mut app, &context, Vec::new());
    }
    // Opening a page overlay may choose a card outside the virtual rows.
    app.edit_comment(79, &context);
    for size in [
        Vec2::new(1180.0, 820.0),
        Vec2::new(500.0, 400.0),
        Vec2::new(640.0, 480.0),
    ] {
        frame_size(&mut app, &context, key(Key::A, Modifiers::COMMAND), size);
        frame_size(&mut app, &context, key(Key::C, Modifiers::COMMAND), size);
        let mut output = egui::FullOutput::default();
        for _ in 0..30 {
            output = frame_size(&mut app, &context, Vec::new(), size);
        }
        assert!(
            visible_selected_card(&context, &output, "Row 79").is_some(),
            "the editor's active card must be fully visible at {size:?}"
        );
        assert_eq!(app.comments.editor.as_ref().unwrap().contents, "Row 79");
        assert!(
            context.memory(|memory| { memory.has_focus(egui::Id::new("pagewing-comment-input")) })
        );
    }
    context.memory_mut(|memory| {
        memory.request_focus(egui::Id::new("pagewing-comment-filter"));
    });
    frame_size(
        &mut app,
        &context,
        vec![Event::Paste("Row".into())],
        Vec2::new(640.0, 480.0),
    );
    assert_eq!(app.comments.view.selected(), Some(0));
    context.memory_mut(|memory| {
        memory.request_focus(egui::Id::new("pagewing-comment-input"));
    });
    let mut output = egui::FullOutput::default();
    for _ in 0..4 {
        output = frame(&mut app, &context, Vec::new());
    }
    assert!(
        visible_selected_card(&context, &output, "Row 79").is_some(),
        "with editor focus, resize reveals the active draft rather than the filter's first match"
    );
    assert_eq!(app.comments.view.selected(), Some(0));
    assert_eq!(app.comments.editor.as_ref().unwrap().contents, "Row 79");
}

#[test]
fn growing_after_editor_selection_in_minimum_windows_reveals_its_card() {
    let (_root, context, mut app) = many_comments();
    app.comments.view.filter.query = "Row".into();
    app.edit_comment(79, &context);
    for size in [
        Vec2::new(1180.0, 820.0),
        Vec2::new(640.0, 480.0),
        Vec2::new(240.0, 400.0),
        Vec2::new(240.0, 240.0),
    ] {
        for _ in 0..4 {
            frame_size(&mut app, &context, Vec::new(), size);
        }
        frame_size(&mut app, &context, key(Key::Home, Modifiers::COMMAND), size);
        frame_size(
            &mut app,
            &context,
            key(Key::ArrowRight, Modifiers::SHIFT),
            size,
        );
        for _ in 0..30 {
            frame_size(&mut app, &context, Vec::new(), size);
        }
    }
    let mut output = egui::FullOutput::default();
    for _ in 0..30 {
        output = frame_size(&mut app, &context, Vec::new(), Vec2::new(640.0, 480.0));
    }
    assert!(
        visible_selected_card(&context, &output, "Row 79").is_some(),
        "growing the window must reveal the editor's card inside both scroll areas"
    );
    assert!(context.memory(|memory| memory.has_focus(egui::Id::new("pagewing-comment-input"))));
    assert_eq!(app.comments.editor.as_ref().unwrap().contents, "Row 79");
}

#[test]
fn editor_actions_stay_visible_with_long_drafts_in_short_windows() {
    let (_root, context, mut app) = saved_comment_editor();
    app.comments.editor.as_mut().unwrap().contents = "Long draft line\n".repeat(100);
    let expected = app.comments.editor.as_ref().unwrap().contents.clone();
    let mut save_pos = egui::Pos2::ZERO;
    for size in [
        Vec2::new(640.0, 480.0),
        Vec2::new(500.0, 360.0),
        Vec2::new(240.0, 240.0),
    ] {
        let mut output = egui::FullOutput::default();
        for _ in 0..3 {
            output = frame_size(&mut app, &context, Vec::new(), size);
        }
        assert!(
            app.viewport.x >= 60.0,
            "editor collapsed the PDF viewport at {size:?}: {:?}",
            app.viewport
        );
        let compact = output.shapes.iter().any(|shape| {
            matches!(&shape.shape,
            egui::Shape::Text(text) if text.galley.text() == "Save")
        });
        save_pos = output
            .shapes
            .iter()
            .find_map(|shape| match &shape.shape {
                egui::Shape::Text(text)
                    if text.galley.text() == if compact { "Save" } else { "Save comment" } =>
                {
                    Some(text.pos + text.galley.size() * 0.5)
                }
                _ => None,
            })
            .expect("visible save action");
        for label in if compact {
            ["Save", "Discard"]
        } else {
            ["Save comment", "Discard / close editor"]
        } {
            assert!(
                output.shapes.iter().any(|shape| {
                    if let egui::Shape::Text(text) = &shape.shape {
                        let rect = egui::Rect::from_min_size(text.pos, text.galley.size());
                        text.galley.text() == label
                            && shape.clip_rect.contains_rect(rect)
                            && egui::Rect::from_min_size(egui::Pos2::ZERO, size).contains_rect(rect)
                    } else {
                        false
                    }
                }),
                "{label} must remain visible at {size:?}"
            );
        }
    }
    for pressed in [true, false] {
        frame_size(
            &mut app,
            &context,
            vec![
                Event::PointerMoved(save_pos),
                Event::PointerButton {
                    pos: save_pos,
                    pressed,
                    button: egui::PointerButton::Primary,
                    modifiers: Modifiers::NONE,
                },
            ],
            Vec2::new(240.0, 240.0),
        );
    }
    // A fast worker may already have saved and started reloading between
    // pointer frames. Verify the resulting PDF, not that transient state.
    wait(&mut app, &context, |app| {
        !app.comments.saving && !app.loading && app.comments.loaded
    });
    assert_eq!(app.comments.comments.items[0].contents, expected);
}

#[test]
fn compact_editor_menu_can_reply_and_delete_after_resizing() {
    let (_root, context, mut app) = saved_comment_editor();
    let size = Vec2::new(240.0, 240.0);
    for _ in 0..3 {
        frame_size(&mut app, &context, Vec::new(), size);
    }
    click_text_size(&mut app, &context, "More…", size);
    click_text_size(&mut app, &context, "Reply", size);
    let editor = app.comments.editor.as_ref().unwrap();
    assert!(editor.slot.is_none() && editor.parent.is_some());
    frame_size(
        &mut app,
        &context,
        vec![Event::Text("Compact reply".into())],
        size,
    );
    click_text_size(&mut app, &context, "Save", size);
    wait(&mut app, &context, |app| {
        !app.comments.busy() && !app.loading && app.comments.loaded
    });
    assert_eq!(app.comments.comments.items[1].contents, "Compact reply");
    app.edit_comment(1, &context);
    for _ in 0..3 {
        frame_size(&mut app, &context, Vec::new(), size);
    }
    click_text_size(&mut app, &context, "More…", size);
    click_text_size(&mut app, &context, "Delete reply & replies", size);
    wait(&mut app, &context, |app| {
        !app.comments.busy() && !app.loading && app.comments.loaded
    });
    assert_eq!(app.comments.comments.items.len(), 1);
    assert_eq!(app.comments.comments.items[0].contents, "Original comment");
}

#[test]
fn reloaded_comment_cards_replace_text_author_status_and_page_labels() {
    use lopdf::dictionary;
    let (_root, context, mut app) = saved_comment_editor();
    let path = app.reading.path.clone();
    app.comments.editor = None;
    let mut before = egui::FullOutput::default();
    for _ in 0..3 {
        before = frame(&mut app, &context, Vec::new());
    }
    let header = format!("Page {} · Pagewing", app.page_name(0));
    assert!(before.shapes.iter().any(|shape| matches!(&shape.shape,
        egui::Shape::Text(text) if text.galley.text() == header)));
    let mut changed = app.comments.comments.items[0].clone();
    changed.contents = "Replaced on disk".into();
    changed.review = ReviewStatus::Completed;
    changed.marked = true;
    crate::annotations::save(&crate::document::load(&path, "").unwrap(), &changed, &path).unwrap();
    let mut pdf = lopdf::Document::load(&path).unwrap();
    for object in pdf.objects.values_mut() {
        if let Ok(dict) = object.as_dict_mut()
            && dict.get(b"Subtype").and_then(lopdf::Object::as_name).ok() == Some(b"Highlight")
        {
            dict.set("T", lopdf::Object::string_literal("Another reviewer"));
        }
    }
    pdf.catalog_mut().unwrap().set(
        "PageLabels",
        lopdf::dictionary! {
            "Nums" => vec![lopdf::Object::Integer(0), lopdf::dictionary! { "S" => "r" }.into()],
        },
    );
    pdf.save(&path).unwrap();
    app.comments.editor = None;
    app.reload(&context);
    wait(&mut app, &context, |app| {
        !app.loading && app.comments.loaded
    });
    let after = frame(&mut app, &context, Vec::new());
    for expected in [
        "Page i · 1 · Another reviewer",
        "Replaced on disk",
        "Highlight · Completed / resolved · ✓ Marked",
    ] {
        assert!(
            after.shapes.iter().any(|shape| matches!(&shape.shape,
            egui::Shape::Text(text) if text.galley.text() == expected)),
            "missing refreshed card line: {expected}"
        );
    }
    // The Comments event can replace metadata without a new document
    // identity. Reusing index zero must still rebuild its painted text.
    let mut replacement = app.comments.comments.items[0].clone();
    replacement.contents = "Another metadata snapshot".into();
    app.comments.set_comments(Comments {
        items: vec![replacement],
        limited: false,
    });
    let refreshed = frame(&mut app, &context, Vec::new());
    assert!(refreshed.shapes.iter().any(|shape| matches!(&shape.shape,
        egui::Shape::Text(text) if text.galley.text() == "Another metadata snapshot")));
}

#[test]
fn right_sidebar_previews_comments_and_keeps_full_text_in_the_editor() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("comments.pdf");
    std::fs::write(&path, include_bytes!("../../../assets/pagewing-guide.pdf")).unwrap();
    let long = format!("{}END OF FULL COMMENT", "A longer comment. ".repeat(9));
    for (page, contents) in [(0, long.clone()), (4, "Comment on another page".to_owned())] {
        let pdf = crate::document::load(&path, "").unwrap();
        crate::annotations::save(
            &pdf,
            &Comment {
                page,
                slot: None,
                parent: None,
                review: Default::default(),
                marked: false,
                states: Vec::new(),
                kind: "Highlight".into(),
                contents,
                author: "Pagewing".into(),
                rects: vec![hayro::kurbo::Rect::new(50.0, 80.0, 150.0, 100.0)],
                has_appearance: false,
                truncated: false,
                locked: false,
            },
            &path,
        )
        .unwrap();
    }
    let context = Context::default();
    let mut app = Pagewing::with_context(
        &context,
        Some(path),
        Some(0),
        root.path().join("library.json"),
    );
    wait(&mut app, &context, |app| app.comments.loaded);
    assert!(
        !app.comments.open,
        "the right sidebar stays closed even when the PDF contains comments"
    );
    frame(
        &mut app,
        &context,
        key(Key::M, Modifiers::COMMAND | Modifiers::SHIFT),
    );
    assert!(app.comments.open, "the shortcut opens the comments sidebar");
    let output = frame(&mut app, &context, Vec::new());
    let texts: Vec<_> = output
        .shapes
        .iter()
        .filter_map(|shape| {
            if let egui::Shape::Text(text) = &shape.shape {
                Some(text)
            } else {
                None
            }
        })
        .collect();
    assert!(texts.iter().any(|text| text.galley.text() == "2 total"));
    let first = texts
        .iter()
        .find(|text| text.galley.text().contains("A longer comment."))
        .unwrap();
    assert_eq!(
        first.galley.text(),
        crate::comment_cards::preview_text(&long, 160),
        "the summary must be a separately painted line, not truncated with its header"
    );
    assert!(
        !first.galley.text().contains("END OF FULL COMMENT"),
        "cards keep long comments compact"
    );
    let first_point = first.pos + Vec2::new(10.0, 8.0);
    let other_page = texts
        .iter()
        .find(|text| text.galley.text().contains("Comment on another page"))
        .unwrap();
    let point = other_page.pos + Vec2::new(10.0, 8.0);
    assert!(
        point.x > 800.0,
        "the list must be on the right side of the canvas"
    );
    assert_eq!(app.nav.page, 0);
    // A permanently visible list must not capture document navigation keys.
    frame(&mut app, &context, key(Key::J, Modifiers::SHIFT));
    assert_eq!(app.nav.page, 1);
    for pressed in [true, false] {
        frame(
            &mut app,
            &context,
            vec![
                Event::PointerMoved(point),
                Event::PointerButton {
                    pos: point,
                    button: egui::PointerButton::Primary,
                    pressed,
                    modifiers: Modifiers::NONE,
                },
            ],
        );
    }
    assert_eq!(app.nav.page, 4);
    assert_eq!(
        app.comments.editor.as_ref().unwrap().contents,
        "Comment on another page"
    );
    assert!(app.comments.open);
    app.comments.editor = None;
    for pressed in [true, false] {
        frame(
            &mut app,
            &context,
            vec![
                Event::PointerMoved(first_point),
                Event::PointerButton {
                    pos: first_point,
                    button: egui::PointerButton::Primary,
                    pressed,
                    modifiers: Modifiers::NONE,
                },
            ],
        );
    }
    assert_eq!(
        app.comments.editor.as_ref().unwrap().contents,
        long,
        "opening a compact card retains the entire annotation"
    );
    // Make sure both sidebars leave a usable PDF area in small windows.
    app.comments.editor = None;
    app.comments.focus = false;
    app.library.sidebar = true;
    for width in [500.0, 240.0] {
        frame_size(&mut app, &context, Vec::new(), Vec2::new(width, 500.0));
        assert!(app.viewport.is_finite());
        assert!(
            app.viewport.x >= 60.0,
            "PDF viewport collapsed at width {width}: {:?}",
            app.viewport
        );
    }
}

#[test]
fn comment_filter_input_isolated_from_reader_keys_and_preserves_drafts() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("filters.pdf");
    std::fs::write(&path, include_bytes!("../../../assets/pagewing-guide.pdf")).unwrap();
    let context = Context::default();
    let mut app = Pagewing::with_context(
        &context,
        Some(path),
        Some(0),
        root.path().join("library.json"),
    );
    wait(&mut app, &context, |app| app.comments.loaded);
    let comment = Comment {
        page: 0,
        slot: Some(0),
        parent: None,
        review: ReviewStatus::None,
        marked: false,
        states: Vec::new(),
        kind: "Highlight".into(),
        contents: "jkg draft".into(),
        author: "Straße".into(),
        rects: Vec::new(),
        has_appearance: false,
        truncated: false,
        locked: false,
    };
    app.comments.set_comments(Comments {
        items: vec![comment],
        limited: false,
    });
    app.comments.open = true;
    click_text(&mut app, &context, "Filter text or author…");
    let mut events = key(Key::J, Modifiers::NONE);
    events.extend(key(Key::K, Modifiers::NONE));
    events.extend(key(Key::G, Modifiers::NONE));
    events.push(Event::Text("jkg STRASSE".into()));
    frame(&mut app, &context, events);
    assert_eq!(app.nav.page, 0);
    assert_eq!(app.reading.rotation, 0);
    assert_eq!(app.comments.view.matches(), 1);
    assert_eq!(app.comments.view.filter.query, "jkg STRASSE");
    app.edit_comment(0, &context);
    app.comments.editor.as_mut().unwrap().contents = "unsaved draft".into();
    click_text(&mut app, &context, "Current page");
    click_text(&mut app, &context, "Marked only");
    assert_eq!(app.comments.view.matches(), 0);
    assert!(app.comments.unsaved());
    click_text(&mut app, &context, "Clear filters");
    assert_eq!(app.comments.view.matches(), 1);
    assert_eq!(
        app.comments.editor.as_ref().unwrap().contents,
        "unsaved draft"
    );
    frame(&mut app, &context, key(Key::Escape, Modifiers::NONE));
    assert!(!app.comments.open);
    assert!(
        app.comments.unsaved(),
        "hiding filtered comments preserves the editor"
    );
}

#[test]
fn hiding_comments_releases_filter_focus_before_later_keys_in_the_same_frame() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("focus.pdf");
    std::fs::write(&path, include_bytes!("../../../assets/pagewing-guide.pdf")).unwrap();
    let context = Context::default();
    let mut app = Pagewing::with_context(
        &context,
        Some(path),
        Some(0),
        root.path().join("library.json"),
    );
    wait(&mut app, &context, |app| app.comments.loaded);
    frame(
        &mut app,
        &context,
        key(Key::M, Modifiers::COMMAND | Modifiers::SHIFT),
    );
    click_text(&mut app, &context, "Filter text or author…");
    frame(&mut app, &context, vec![Event::Text("draft filter".into())]);
    let mut events = key(Key::M, Modifiers::COMMAND | Modifiers::SHIFT);
    events.extend(key(Key::J, Modifiers::SHIFT));
    frame(&mut app, &context, events);
    assert!(!app.comments.open);
    assert_eq!(
        app.nav.page, 1,
        "closing a filter must route the following key to the reader"
    );
    assert_eq!(app.comments.view.filter.query, "draft filter");
    for page in 2..=3 {
        frame(
            &mut app,
            &context,
            key(Key::M, Modifiers::COMMAND | Modifiers::SHIFT),
        );
        context.memory_mut(|memory| memory.request_focus(egui::Id::new("pagewing-comment-filter")));
        frame(&mut app, &context, Vec::new());
        let id = if page == 2 {
            app.input_buttons.comments
        } else {
            app.input_buttons.close_comments
        }
        .unwrap();
        let point = context.read_response(id).unwrap().rect.center();
        let mut events = vec![Event::PointerMoved(point)];
        events.extend([true, false].map(|pressed| Event::PointerButton {
            pos: point,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: Modifiers::NONE,
        }));
        events.extend(key(Key::J, Modifiers::SHIFT));
        frame(&mut app, &context, events);
        assert!(
            !app.comments.open,
            "both mouse buttons must close before routing keys"
        );
        assert_eq!(app.nav.page, page);
        assert_eq!(app.comments.view.filter.query, "draft filter");
    }
}

#[test]
fn comment_filter_keyboard_confirms_new_query_and_protects_editor_drafts() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("keyboard.pdf");
    std::fs::write(&path, include_bytes!("../../../assets/pagewing-guide.pdf")).unwrap();
    let context = Context::default();
    let mut app = Pagewing::with_context(
        &context,
        Some(path),
        Some(0),
        root.path().join("library.json"),
    );
    wait(&mut app, &context, |app| app.comments.loaded);
    let parent = Comment {
        page: 0,
        slot: Some(0),
        parent: None,
        review: ReviewStatus::None,
        marked: false,
        states: Vec::new(),
        kind: "Text".into(),
        contents: "Parent context".into(),
        author: "Reviewer".into(),
        rects: Vec::new(),
        has_appearance: false,
        truncated: false,
        locked: false,
    };
    let mut other = parent.clone();
    other.page = 2;
    other.contents = "Another needle".into();
    let mut reply = parent.reply().unwrap();
    reply.page = 4;
    reply.slot = Some(0);
    reply.contents = "Nested needle".into();
    app.comments.set_comments(Comments {
        items: vec![parent, other, reply],
        limited: false,
    });
    app.comments.open = true;
    click_text(&mut app, &context, "Filter text or author…");
    let mut events = vec![Event::Text("needle".into())];
    events.extend(key(Key::Enter, Modifiers::NONE));
    frame(&mut app, &context, events);
    assert_eq!(
        app.nav.page, 4,
        "Enter follows the new query's first match, not its ancestor"
    );
    assert_eq!(
        app.comments.editor.as_ref().unwrap().contents,
        "Nested needle"
    );
    frame(&mut app, &context, Vec::new());
    app.comments.editor.as_mut().unwrap().contents = "Unsaved draft".into();
    context.memory_mut(|memory| memory.request_focus(egui::Id::new("pagewing-comment-filter")));
    let mut events = key(Key::J, Modifiers::CTRL);
    events.extend(key(Key::Enter, Modifiers::NONE));
    frame(&mut app, &context, events);
    assert_eq!(app.comments.view.selected(), Some(1));
    assert_eq!(
        app.comments.editor.as_ref().unwrap().contents,
        "Unsaved draft"
    );
    assert_eq!(app.nav.page, 4, "keyboard selection cannot discard a draft");
    app.comments.editor = None;
    frame(&mut app, &context, key(Key::Enter, Modifiers::NONE));
    assert_eq!(app.nav.page, 2);
    assert_eq!(
        app.comments.editor.as_ref().unwrap().contents,
        "Another needle"
    );
    frame(&mut app, &context, Vec::new());
    context.memory_mut(|memory| memory.request_focus(egui::Id::new("pagewing-comment-filter")));
    app.comments.view.filter.query = "missing".into();
    let mut events = key(Key::ArrowDown, Modifiers::NONE);
    events.extend(key(Key::Enter, Modifiers::NONE));
    frame(&mut app, &context, events);
    assert_eq!(app.nav.page, 2);
    assert_eq!(app.comments.view.selected(), None);
    // A later global Find action must cancel a queued comment confirmation.
    app.comments.view.filter.query = "needle".into();
    let mut events = key(Key::Enter, Modifiers::NONE);
    events.extend(key(Key::F, Modifiers::COMMAND));
    frame(&mut app, &context, events);
    assert!(app.search_open);
    assert_eq!(app.nav.page, 2);
    // A match beneath many retained ancestors must actually be painted in
    // the virtual viewport, rather than merely selected outside it.
    app.search_open = false;
    app.search_focus = false;
    app.comments.editor = None;
    let template = app.comments.comments.items[0].clone();
    let items = (0..128)
        .map(|slot| {
            let mut comment = template.clone();
            comment.slot = Some(slot);
            comment.parent = (slot > 0).then(|| crate::annotations::CommentId {
                page: 0,
                slot: slot - 1,
            });
            comment.contents = if slot == 127 {
                "Deepest needle"
            } else {
                "Ancestor"
            }
            .into();
            comment
        })
        .collect();
    app.comments.set_comments(Comments {
        items,
        limited: false,
    });
    app.comments.view.filter.query = "deepest".into();
    context.memory_mut(|memory| memory.request_focus(egui::Id::new("pagewing-comment-filter")));
    frame(&mut app, &context, Vec::new());
    let output = frame(&mut app, &context, Vec::new());
    assert!(output.shapes.iter().any(|shape| matches!(&shape.shape, egui::Shape::Text(text) if text.galley.text() == "Deepest needle")));
    frame(&mut app, &context, key(Key::Enter, Modifiers::NONE));
    assert_eq!(app.comments.editor.as_ref().unwrap().slot, Some(127));
}

#[test]
fn copy_shown_comments_fits_resizing_and_keeps_unsaved_draft_and_pdf() {
    let (_root, context, mut app) = saved_comment_editor();
    let original_pdf = std::fs::read(&app.reading.path).unwrap();
    app.comments.editor.as_mut().unwrap().contents = "Unpublished draft 中文😀".into();
    app.comments.view.filter.query = "Original".into();
    let mut sizes = super::super::dialog::TEST_SIZES.to_vec();
    sizes.push(egui::vec2(240.0, 240.0));
    for size in sizes {
        let mut output = egui::FullOutput::default();
        for _ in 0..4 {
            output = frame_size(&mut app, &context, Vec::new(), size);
        }
        let (bounds, clip) = output
            .shapes
            .iter()
            .find_map(|shape| match &shape.shape {
                egui::Shape::Text(text) if text.galley.text() == "Copy" => Some((
                    egui::Rect::from_min_size(text.pos, text.galley.size()),
                    shape.clip_rect,
                )),
                _ => None,
            })
            .expect("copy action must be visible");
        assert!(
            clip.contains_rect(bounds),
            "{size:?}: {bounds:?} / {clip:?}"
        );
        assert!(egui::Rect::from_min_size(egui::Pos2::ZERO, size).contains_rect(bounds));
        let mut copied = None;
        for pressed in [true, false] {
            let output = frame_size(
                &mut app,
                &context,
                vec![
                    Event::PointerMoved(bounds.center()),
                    Event::PointerButton {
                        pos: bounds.center(),
                        button: egui::PointerButton::Primary,
                        pressed,
                        modifiers: Modifiers::NONE,
                    },
                ],
                size,
            );
            for command in output.platform_output.commands {
                if let egui::OutputCommand::CopyText(text) = command {
                    copied = Some(text);
                }
            }
        }
        let copied = copied.expect("click must copy the saved filtered list");
        assert!(copied.contains("Original comment"));
        assert!(!copied.contains("Unpublished draft"));
        assert_eq!(
            app.comments.editor.as_ref().unwrap().contents,
            "Unpublished draft 中文😀"
        );
        assert!(app.comments.unsaved());
        assert_eq!(app.comments.view.filter.query, "Original");
        assert_eq!(app.nav.page, 0);
        assert_eq!(std::fs::read(&app.reading.path).unwrap(), original_pdf);
    }
}

#[test]
fn copy_shown_comments_includes_virtual_rows_and_leaves_clipboard_alone_when_empty() {
    let (_root, context, mut app) = many_comments();
    frame(&mut app, &context, Vec::new());
    let selected = app.comments.view.selected();
    let output = context.run(RawInput::default(), |context| {
        app.copy_shown_comments(context)
    });
    let text = output
        .platform_output
        .commands
        .iter()
        .find_map(|command| match command {
            egui::OutputCommand::CopyText(text) => Some(text),
            _ => None,
        })
        .unwrap();
    assert!(text.contains("Row 0\n\n"));
    assert!(text.contains("Row 79\n\n"));
    assert_eq!(text.matches("Page 1 · Highlight\n").count(), 80);
    assert_eq!(app.comments.view.selected(), selected);
    app.comments.view.filter.query = "missing".into();
    app.comments.view.update(&app.comments.comments, 0);
    let output = context.run(RawInput::default(), |context| {
        app.copy_shown_comments(context)
    });
    assert!(
        !output
            .platform_output
            .commands
            .iter()
            .any(|command| matches!(command, egui::OutputCommand::CopyText(_)))
    );
}

#[test]
fn copying_saved_list_preserves_text_typed_in_the_pointer_press_batch() {
    let (_root, context, mut app) = saved_comment_editor();
    let output = frame(&mut app, &context, Vec::new());
    let point = output
        .shapes
        .iter()
        .find_map(|shape| match &shape.shape {
            egui::Shape::Text(text) if text.galley.text() == "Copy" => {
                Some(text.pos + text.galley.size() * 0.5)
            }
            _ => None,
        })
        .unwrap();
    for pressed in [true, false] {
        let mut events = vec![Event::PointerMoved(point)];
        if pressed {
            events.push(Event::Text(" Final 中文😀".into()));
        }
        events.push(Event::PointerButton {
            pos: point,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: Modifiers::NONE,
        });
        frame(&mut app, &context, events);
    }
    assert!(
        app.comments
            .editor
            .as_ref()
            .unwrap()
            .contents
            .contains("Final 中文😀")
    );
    assert!(app.comments.unsaved());
}

#[test]
fn focused_comment_copy_uses_activation_keys_without_scrolling_or_toggling_sidebar() {
    for (size, activation) in [Vec2::new(1180.0, 820.0), Vec2::new(240.0, 240.0)]
        .into_iter()
        .flat_map(|size| [Key::Enter, Key::Space, Key::Tab].map(|key| (size, key)))
    {
        let (_root, context, mut app) = many_comments();
        app.reading.fit = crate::navigation::Fit::Custom(2.0);
        let mut output = egui::FullOutput::default();
        for _ in 0..3 {
            output = frame_size(&mut app, &context, Vec::new(), size);
        }
        let copy_center = output
            .shapes
            .iter()
            .find_map(|shape| match &shape.shape {
                egui::Shape::Text(text) if text.galley.text() == "Copy" => {
                    Some(text.pos + text.galley.size() * 0.5)
                }
                _ => None,
            })
            .unwrap();
        let copy_focused = |context: &Context| {
            context
                .memory(|memory| memory.focused())
                .and_then(|id| context.read_response(id))
                .is_some_and(|response| response.rect.contains(copy_center))
        };
        context.memory_mut(|memory| memory.request_focus(egui::Id::new("pagewing-comment-filter")));
        frame_size(&mut app, &context, Vec::new(), size);
        // A narrow panel also exposes its scroll area's focus stop. Traverse
        // real controls rather than assuming one fixed Tab count at every size.
        for _ in 0..4 {
            frame_size(&mut app, &context, key(Key::Tab, Modifiers::SHIFT), size);
            // egui applies backward traversal at the next frame boundary.
            frame_size(&mut app, &context, Vec::new(), size);
            if copy_focused(&context) {
                break;
            }
        }
        assert!(copy_focused(&context), "Copy must be reachable at {size:?}");
        let pan = app.pan;
        let sidebar = app.library.sidebar;
        let output = frame_size(&mut app, &context, key(activation, Modifiers::NONE), size);
        assert_eq!(
            app.library.sidebar, sidebar,
            "{activation:?} belongs to the focused control"
        );
        assert_eq!(app.pan, pan, "{activation:?} must not scroll the PDF");
        assert_eq!(app.nav.page, 0);
        if activation != Key::Tab {
            assert!(output.platform_output.commands.iter().any(|command| matches!(command, egui::OutputCommand::CopyText(text) if text.contains("Row 79"))), "{activation:?} must activate Copy");
        } else {
            for _ in 0..3 {
                if app.comment_filter_focused(&context) {
                    break;
                }
                frame_size(&mut app, &context, key(Key::Tab, Modifiers::NONE), size);
                assert_eq!(
                    app.library.sidebar, sidebar,
                    "scroll-area Tab stays in the controls"
                );
            }
            assert!(
                app.comment_filter_focused(&context),
                "Tab returns to the filter"
            );
        }
    }
}

#[test]
fn reader_tab_binding_still_toggles_sidebar_after_leaving_comment_controls() {
    let (_root, context, mut app) = many_comments();
    context.memory_mut(|memory| {
        if let Some(id) = memory.focused() {
            memory.surrender_focus(id);
        }
    });
    frame(&mut app, &context, Vec::new());
    let before = app.library.sidebar;
    frame(&mut app, &context, key(Key::Tab, Modifiers::NONE));
    assert_eq!(
        app.library.sidebar, !before,
        "reader Tab keeps its binding while comments are open"
    );
    context.memory_mut(|memory| memory.request_focus(egui::Id::new("pagewing-comment-filter")));
    frame(&mut app, &context, Vec::new());
    frame(&mut app, &context, key(Key::Tab, Modifiers::SHIFT));
    frame(&mut app, &context, Vec::new());
    app.set_comments_open(&context, false);
    let before = app.library.sidebar;
    frame(&mut app, &context, key(Key::Tab, Modifiers::NONE));
    assert_eq!(
        app.library.sidebar, !before,
        "hidden controls cannot claim Tab"
    );
    assert!(!app.comments.open);
}

#[test]
fn comment_filters_keep_the_panel_inside_narrow_windows_for_every_status() {
    let (_root, context, mut app) = many_comments();
    let template = app.comments.comments.items[0].clone();
    for editing in [false, true] {
        app.comments.editor = editing.then(|| template.clone());
        app.comments.focus = false;
        for size in [
            Vec2::new(240.0, 240.0),
            Vec2::new(240.0, 400.0),
            Vec2::new(360.0, 540.0),
            Vec2::new(480.0, 420.0),
            Vec2::new(1180.0, 820.0),
        ] {
            for review in std::iter::once(None).chain(ReviewStatus::ALL.map(Some)) {
                app.comments.view.filter.review = review;
                if let Some(editor) = &mut app.comments.editor {
                    editor.review = review.unwrap_or_default();
                }
                for _ in 0..4 {
                    frame_size(&mut app, &context, Vec::new(), size);
                }
                let (_, panel) = app.comments.region.unwrap();
                let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, size);
                assert!(
                    screen.contains_rect(panel),
                    "{size:?}, {review:?}, editor {editing}: {panel:?} outside screen"
                );
                // The sidebar reserves 120 logical pixels for the canvas;
                // its own page padding is excluded from Pagewing::viewport.
                assert!(
                    panel.left() >= 120.0,
                    "{size:?}, {review:?}: {panel:?} collapsed the canvas"
                );
            }
        }
    }
}

#[test]
fn narrow_comment_status_menu_keeps_full_labels_selectable() {
    let (_root, context, mut app) = many_comments();
    for size in [Vec2::new(240.0, 400.0), Vec2::new(360.0, 540.0)] {
        for _ in 0..4 {
            frame_size(&mut app, &context, Vec::new(), size);
        }
        click_text_size(&mut app, &context, "All statuses", size);
        let mut output = egui::FullOutput::default();
        for _ in 0..4 {
            output = frame_size(&mut app, &context, Vec::new(), size);
        }
        assert!(egui::Popup::is_any_open(&context));
        let label = "Completed / resolved";
        let label_rect = |output: &egui::FullOutput| {
            output
                .shapes
                .iter()
                .find_map(|shape| match &shape.shape {
                    egui::Shape::Text(text) if text.galley.text() == label => Some((
                        egui::Rect::from_min_size(text.pos, text.galley.size()),
                        shape.clip_rect,
                    )),
                    _ => None,
                })
                .expect("the longest status keeps its full label in the menu")
        };
        let (text, clip) = label_rect(&output);
        // Small windows scroll the menu; its last option must become fully
        // readable and clickable rather than losing its wrapped lines.
        frame_size(
            &mut app,
            &context,
            vec![
                Event::PointerMoved(clip.shrink(5.0).clamp(text.center())),
                Event::MouseWheel {
                    unit: egui::MouseWheelUnit::Point,
                    delta: Vec2::new(0.0, -400.0),
                    modifiers: Modifiers::NONE,
                },
            ],
            size,
        );
        for _ in 0..30 {
            output = frame_size(&mut app, &context, Vec::new(), size);
        }
        let (text, clip) = label_rect(&output);
        assert!(
            clip.contains_rect(text),
            "{size:?}: {text:?} clipped by {clip:?}"
        );
        assert!(egui::Rect::from_min_size(egui::Pos2::ZERO, size).contains_rect(text));
        click_text_size(&mut app, &context, label, size);
        assert_eq!(
            app.comments.view.filter.review,
            Some(ReviewStatus::Completed)
        );
        assert!(app.comments.view.rows().is_empty());
        assert!(!egui::Popup::is_any_open(&context));
        click_text_size(&mut app, &context, "Clear filters", size);
        assert_eq!(app.comments.view.filter, CommentFilter::default());
        assert_eq!(app.comments.view.rows().len(), 80);
    }
}
