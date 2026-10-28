use super::*;
use crate::{navigation::Navigation, state::ReadingState};

fn fixture() -> (
    tempfile::TempDir,
    egui::Context,
    Pagewing,
    Vec<std::path::PathBuf>,
) {
    let directory = tempfile::tempdir().unwrap();
    let context = egui::Context::default();
    let mut app =
        Pagewing::with_context(&context, None, None, directory.path().join("library.json"));
    let paths: Vec<_> = ["one.pdf", "two.pdf", "active.pdf"]
        .map(|name| directory.path().join(name))
        .into();
    for path in paths.iter().rev() {
        std::fs::write(path, b"unchanged PDF fixture").unwrap();
        app.library.remember(ReadingState {
            path: path.clone(),
            page: 3,
            bookmarks: [1, 3].into(),
            ..Default::default()
        });
    }
    app.nav = Navigation::new(6, 2);
    app.dimensions = vec![[600.0, 800.0]; 6];
    app.reading.path = paths[2].clone();
    app.toggle_recent(&context);
    (directory, context, app, paths)
}

#[test]
fn remove_buttons_keep_nearest_selection_and_undo_restores_the_full_record_without_writing_pdf() {
    for size in [egui::vec2(1180.0, 820.0), egui::vec2(240.0, 240.0)] {
        let (_root, context, mut app, paths) = fixture();
        let original = serde_json::to_value(&app.library).unwrap();
        for _ in 0..4 {
            sized_frame(&mut app, &context, size, Vec::new());
        }
        click_action(&mut app, &context, size, "×", Vec::new());
        let picker = app.recent_picker.as_ref().unwrap();
        assert_eq!(picker.selected_path(), Some(paths[1].as_path()));
        assert_eq!(picker.query, "");
        assert!(picker.undo.is_some());
        assert_eq!(app.library.recent.len(), 2);
        assert_eq!(app.nav.page, 2);
        assert_eq!(app.reading.path, paths[2]);
        assert!(!app.loading);
        for _ in 0..4 {
            sized_frame(&mut app, &context, size, Vec::new());
        }
        click_action(
            &mut app,
            &context,
            size,
            if size.x < 480.0 {
                "Undo"
            } else {
                "Undo remove"
            },
            Vec::new(),
        );
        assert_eq!(serde_json::to_value(&app.library).unwrap(), original);
        assert_eq!(
            app.recent_picker.as_ref().unwrap().selected_path(),
            Some(paths[0].as_path())
        );
        assert!(app.recent_picker.as_ref().unwrap().undo.is_none());
        for path in &paths {
            assert_eq!(std::fs::read(path).unwrap(), b"unchanged PDF fixture");
        }
    }
}

#[test]
fn delete_edits_the_filter_and_only_removes_a_focused_inactive_row() {
    let (_root, context, mut app, paths) = fixture();
    for _ in 0..4 {
        frame(&mut app, &context, Vec::new());
    }
    frame(&mut app, &context, vec![egui::Event::Text("one".into())]);
    frame(
        &mut app,
        &context,
        modified_key(egui::Key::Home, egui::Modifiers::NONE),
    );
    frame(&mut app, &context, key(egui::Key::Delete));
    assert_eq!(app.recent_picker.as_ref().unwrap().query, "ne");
    assert_eq!(
        app.library.recent.len(),
        3,
        "Delete in the filter must only edit text"
    );
    frame(&mut app, &context, key(egui::Key::Tab));
    frame(&mut app, &context, key(egui::Key::Delete));
    assert_eq!(app.library.recent.len(), 2);
    assert!(!app.library.recent.iter().any(|item| item.path == paths[0]));
    assert_eq!(app.recent_picker.as_ref().unwrap().query, "ne");
    // Select the active document through the filter; its remove control and
    // row Delete must leave both the reading state and recent record intact.
    frame(
        &mut app,
        &context,
        modified_key(egui::Key::A, egui::Modifiers::COMMAND),
    );
    frame(&mut app, &context, vec![egui::Event::Text("active".into())]);
    frame(&mut app, &context, key(egui::Key::Tab));
    // Undo appears before the results in traversal order after a removal.
    frame(&mut app, &context, key(egui::Key::Tab));
    frame(&mut app, &context, key(egui::Key::Delete));
    click_action(
        &mut app,
        &context,
        egui::vec2(1180.0, 820.0),
        "×",
        Vec::new(),
    );
    assert_eq!(app.library.recent.len(), 2);
    assert_eq!(app.nav.page, 2);
    assert_eq!(app.reading.path, paths[2]);
    assert!(app.library.recent.iter().any(|item| item.path == paths[2]));
    assert!(!app.loading);
}

#[test]
fn overflowing_list_requires_row_focus_before_delete_removes_a_record() {
    let (_root, context, mut app, paths) = fixture();
    let size = egui::vec2(240.0, 240.0);
    for _ in 0..4 {
        sized_frame(&mut app, &context, size, Vec::new());
    }
    sized_frame(&mut app, &context, size, key(egui::Key::Tab));
    sized_frame(&mut app, &context, size, key(egui::Key::Delete));
    assert_eq!(
        app.library.recent.len(),
        3,
        "the scroll region cannot remove a record"
    );
    sized_frame(&mut app, &context, size, key(egui::Key::Tab));
    sized_frame(&mut app, &context, size, key(egui::Key::Delete));
    assert_eq!(app.library.recent.len(), 2);
    assert!(!app.library.recent.iter().any(|item| item.path == paths[0]));
}

#[test]
fn tab_reveals_each_recent_row_and_its_remove_button_at_minimum_size() {
    let (_root, context, mut app, _paths) = fixture();
    let size = egui::vec2(240.0, 240.0);
    for _ in 0..4 {
        sized_frame(&mut app, &context, size, Vec::new());
    }
    sized_frame(&mut app, &context, size, key(egui::Key::Tab)); // Scroll region.
    for _ in 0..5 {
        // Two removable rows, then the active row.
        sized_frame(&mut app, &context, size, key(egui::Key::Tab));
        for _ in 0..3 {
            sized_frame(&mut app, &context, size, Vec::new());
        }
        let focused = context.memory(|memory| memory.focused()).unwrap();
        let response = context.read_response(focused).unwrap();
        assert!(
            response.interact_rect.contains_rect(response.rect),
            "focused recent control {:?} clipped by {:?}",
            response.rect,
            response.interact_rect
        );
    }
}
