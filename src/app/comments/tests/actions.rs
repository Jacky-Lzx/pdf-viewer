use super::*;

fn focused_label(
    context: &Context,
    output: &egui::FullOutput,
    labels: &[&str],
) -> Option<egui::Id> {
    let id = context.memory(|memory| memory.focused())?;
    let response = context.read_response(id)?;
    if response.rect.height() > 40.0 {
        return None;
    }
    output.shapes.iter().find_map(|shape| {
        let egui::Shape::Text(text) = &shape.shape else {
            return None;
        };
        let rect = egui::Rect::from_min_size(text.pos, text.galley.size());
        (labels.contains(&text.galley.text()) && response.rect.contains_rect(rect)).then_some(id)
    })
}

#[test]
fn save_and_discard_keep_their_keyboard_targets_across_footer_reflow() {
    for (save, activation) in [
        (true, Key::Space),
        (true, Key::Enter),
        (false, Key::Space),
        (false, Key::Enter),
    ] {
        let (_root, context, mut app) = saved_comment_editor();
        let original = std::fs::read(&app.reading.path).unwrap();
        let draft = "Unsaved text before resizing";
        app.comments.editor.as_mut().unwrap().contents = draft.into();
        let labels = if save {
            ["Save comment", "Save"]
        } else {
            ["Discard / close editor", "Discard"]
        };
        let mut focused = None;
        for _ in 0..80 {
            frame(&mut app, &context, key(Key::Tab, Modifiers::NONE));
            let output = frame(&mut app, &context, Vec::new());
            if let Some(id) = focused_label(&context, &output, &labels) {
                focused = Some(id);
                break;
            }
        }
        let focused = focused.expect("editor action must be reachable with Tab");
        for size in [
            Vec2::new(500.0, 400.0),
            Vec2::new(240.0, 240.0),
            Vec2::new(1180.0, 820.0),
            Vec2::new(240.0, 240.0),
        ] {
            let mut output = egui::FullOutput::default();
            for _ in 0..5 {
                output = frame_size(&mut app, &context, Vec::new(), size);
            }
            assert_eq!(
                focused_label(&context, &output, &labels),
                Some(focused),
                "{labels:?} at {size:?}: focus must stay on the same action"
            );
            assert_eq!(app.comments.editor.as_ref().unwrap().contents, draft);
            assert_eq!(std::fs::read(&app.reading.path).unwrap(), original);
        }
        frame_size(
            &mut app,
            &context,
            key(activation, Modifiers::NONE),
            Vec2::new(240.0, 240.0),
        );
        if save {
            wait(&mut app, &context, |app| {
                !app.comments.busy()
                    && !app.loading
                    && app.comments.loaded
                    && app.comments.editor.is_none()
            });
            assert_eq!(app.comments.comments.items[0].contents, draft);
            assert_ne!(std::fs::read(&app.reading.path).unwrap(), original);
        } else {
            assert!(
                app.comments.editor.is_none(),
                "{activation:?} must discard the editor after resizing"
            );
            assert_eq!(std::fs::read(&app.reading.path).unwrap(), original);
        }
    }
}
