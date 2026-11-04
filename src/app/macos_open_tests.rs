use super::*;

#[test]
fn finder_uses_an_empty_reader_once_and_opens_later_files_in_new_windows() {
    let directory = tempfile::tempdir().unwrap();
    let context = egui::Context::default();
    let mut app =
        Pagewing::with_context(&context, None, None, directory.path().join("library.json"));
    let first = directory.path().join("first.pdf");
    let second = directory.path().join("中文 paper #1 100%.pdf");
    let third = directory.path().join("-third.pdf");
    let mut windows = Vec::new();
    app.open_macos_paths(vec![first.clone(), second.clone()], &context, |path| {
        windows.push(path.to_owned());
        Ok(())
    });
    assert_eq!(app.reading.path, first);
    assert!(app.loading);
    assert_eq!(windows, vec![second.clone()]);
    let id = app.id;
    app.open_macos_paths(vec![third.clone()], &context, |path| {
        windows.push(path.to_owned());
        Ok(())
    });
    assert_eq!(windows, vec![second, third]);
    assert_eq!(app.reading.path, first);
    assert_eq!(app.id, id);
}

#[test]
fn finder_preserves_the_current_document_comment_draft_and_in_flight_save() {
    for loading in [false, true] {
        let directory = tempfile::tempdir().unwrap();
        let context = egui::Context::default();
        let mut app =
            Pagewing::with_context(&context, None, None, directory.path().join("library.json"));
        let current = directory.path().join("current.pdf");
        let target = directory.path().join("next.pdf");
        app.reading.path = current.clone();
        app.loading = loading;
        app.comments.saving = true;
        app.comments.editor = Some(crate::annotations::Comment {
            page: 0,
            slot: None,
            parent: None,
            review: Default::default(),
            marked: false,
            states: Vec::new(),
            kind: "Highlight".into(),
            contents: "keep this draft 📝".into(),
            author: "Pagewing".into(),
            rects: Vec::new(),
            has_appearance: false,
            truncated: false,
            locked: false,
        });
        app.query = "keep search".into();
        let id = app.id;
        let mut windows = Vec::new();
        app.open_macos_paths(vec![target.clone()], &context, |path| {
            windows.push(path.to_owned());
            Ok(())
        });
        assert_eq!(windows, vec![target]);
        assert_eq!(app.reading.path, current);
        assert_eq!(app.id, id);
        assert_eq!(app.loading, loading);
        assert!(app.comments.saving);
        assert_eq!(
            app.comments.editor.as_ref().unwrap().contents,
            "keep this draft 📝"
        );
        assert!(!app.comments.open);
        assert_eq!(app.query, "keep search");
    }
}

#[test]
fn finder_reports_launch_failure_without_replacing_the_document_or_losing_the_batch() {
    let directory = tempfile::tempdir().unwrap();
    let context = egui::Context::default();
    let mut app =
        Pagewing::with_context(&context, None, None, directory.path().join("library.json"));
    let current = directory.path().join("current.pdf");
    let failed = directory.path().join("failed.pdf");
    let next = directory.path().join("next.pdf");
    app.reading.path = current.clone();
    let mut attempts = Vec::new();
    app.open_macos_paths(vec![failed.clone(), next.clone()], &context, |path| {
        attempts.push(path.to_owned());
        if path == failed {
            Err(std::io::Error::other("launch failed"))
        } else {
            Ok(())
        }
    });
    assert_eq!(attempts, vec![failed, next]);
    assert_eq!(app.reading.path, current);
    assert!(!app.loading);
    assert!(app.message.as_ref().unwrap().text.contains("launch failed"));
}
