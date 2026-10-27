use super::Pagewing;
use eframe::egui;

impl Pagewing {
    /// The worker debounces disk writes. Consume each change once, and defer
    /// while an editor or gesture still owns the current document snapshot.
    pub(super) fn reload_if_changed(&mut self, context: &egui::Context) {
        if !self.auto_reload_pending
            || self.loading
            || self.nav.count == 0
            || self.comments.busy()
            || self.comments.editor.is_some()
            || self.selection.active
            || self.password_open
            || context.input(|i| i.pointer.any_down())
        {
            return;
        }
        self.reload();
        self.automatic_reload = self.loading;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{annotations::Comment, navigation::Fit};
    use egui::{Context, RawInput, Rect, Vec2};
    use std::{
        fs,
        path::Path,
        time::{Duration, Instant},
    };

    fn pdf(title: &str) -> Vec<u8> {
        let mut document =
            lopdf::Document::load_mem(include_bytes!("../../assets/pagewing-guide.pdf")).unwrap();
        let info = document
            .add_object(lopdf::dictionary! { "Title" => lopdf::Object::string_literal(title) });
        document.trailer.set("Info", info);
        let mut bytes = Vec::new();
        document.save_to(&mut bytes).unwrap();
        bytes
    }

    fn frame(app: &mut Pagewing, context: &Context) -> egui::FullOutput {
        frame_events(app, context, Vec::new())
    }

    fn frame_events(
        app: &mut Pagewing,
        context: &Context,
        events: Vec<egui::Event>,
    ) -> egui::FullOutput {
        context.run(
            RawInput {
                screen_rect: Some(Rect::from_min_size(
                    egui::Pos2::ZERO,
                    Vec2::new(1100.0, 800.0),
                )),
                events,
                ..Default::default()
            },
            |context| {
                app.events(context);
                app.keys(context);
                app.toolbar(context);
                app.search_bar(context);
                app.status(context);
                app.dialogs(context);
                app.comments_sidebar(context);
                app.sidebar(context);
                app.canvas(context);
            },
        )
    }

    #[test]
    fn starting_selection_in_the_change_frame_defers_refresh() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("document.pdf");
        let (mut app, context) = open(&path, &root.path().join("library.json"));
        fs::write(&path, pdf("Edition two")).unwrap();
        app.file_changed = true;
        app.auto_reload_pending = true;
        let events = [true, false].map(|pressed| egui::Event::Key {
            key: egui::Key::V,
            physical_key: None,
            pressed,
            repeat: false,
            modifiers: egui::Modifiers::NONE,
        });
        frame_events(&mut app, &context, events.into());
        assert!(app.selection.active && app.auto_reload_pending && !app.loading);
        assert_eq!(title(&app), "Edition one");
        app.selection.cancel();
        wait(&mut app, &context, |a| {
            !a.loading && title(a) == "Edition two"
        });
    }

    fn wait(app: &mut Pagewing, context: &Context, ready: impl Fn(&Pagewing) -> bool) {
        let deadline = Instant::now() + Duration::from_secs(6);
        while !ready(app) {
            assert!(
                Instant::now() < deadline,
                "reload timed out: {:?}",
                app.error
            );
            frame(app, context);
            std::thread::sleep(Duration::from_millis(10));
        }
    }

    fn title(app: &Pagewing) -> &str {
        app.info
            .fields
            .iter()
            .find(|(name, _)| *name == "Title")
            .map(|(_, value)| value.as_str())
            .unwrap_or("")
    }

    fn open(path: &Path, config: &Path) -> (Pagewing, Context) {
        fs::write(path, pdf("Edition one")).unwrap();
        let context = Context::default();
        let mut app = Pagewing::with_context(&context, Some(path.into()), None, config.into());
        wait(&mut app, &context, |a| !a.loading && a.comments.loaded);
        (app, context)
    }

    #[test]
    fn stable_atomic_replacement_refreshes_without_changing_view_or_search_focus() {
        for continuous in [false, true] {
            let root = tempfile::tempdir().unwrap();
            let path = root.path().join("中文 document.pdf");
            let (mut app, context) = open(&path, &root.path().join("library.json"));
            app.reading.continuous = continuous;
            app.reading.fit = Fit::Custom(1.8);
            app.reading.rotation = 1;
            app.jump(2);
            app.find();
            app.query = "no matching phrase".into();
            frame(&mut app, &context);
            app.pending_offset = Some([0.35, 0.4]);
            frame(&mut app, &context);
            let before = app.current_position();
            let focus = context.memory(|m| m.focused());
            assert_eq!(focus, Some(egui::Id::new("pagewing-search-input")));
            let stamp = fs::metadata(&path).unwrap();
            let replacement = root.path().join("replacement.pdf");
            fs::write(&replacement, pdf("Edition two")).unwrap();
            assert_eq!(fs::metadata(&replacement).unwrap().len(), stamp.len());
            fs::File::options()
                .write(true)
                .open(&replacement)
                .unwrap()
                .set_times(fs::FileTimes::new().set_modified(stamp.modified().unwrap()))
                .unwrap();
            fs::rename(replacement, &path).unwrap();
            wait(&mut app, &context, |a| {
                !a.loading && title(a) == "Edition two"
            });
            frame(&mut app, &context);
            let after = app.current_position();
            assert_eq!(
                (after.page, after.rotation, after.fit),
                (before.page, before.rotation, before.fit)
            );
            for axis in 0..2 {
                assert!(
                    (after.offset[axis] - before.offset[axis]).abs() < 1e-5,
                    "{before:?} -> {after:?}"
                );
            }
            assert_eq!(app.reading.continuous, continuous);
            assert_eq!(app.query, "no matching phrase");
            assert_eq!(context.memory(|m| m.focused()), focus);
            assert!(!app.file_changed && !app.auto_reload_pending);
            assert!(app.error.is_none(), "{:?}", app.error);
        }
    }

    #[test]
    fn hidden_comment_draft_and_selection_defer_reload_until_released() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("document.pdf");
        let (mut app, context) = open(&path, &root.path().join("library.json"));
        app.comments.editor = Some(Comment {
            page: 0,
            slot: None,
            parent: None,
            review: Default::default(),
            marked: false,
            states: Vec::new(),
            kind: "Highlight".into(),
            contents: "unsaved draft".into(),
            author: String::new(),
            rects: Vec::new(),
            has_appearance: false,
            truncated: false,
            locked: false,
        });
        app.comments.open = false;
        fs::write(&path, pdf("Edition two")).unwrap();
        wait(&mut app, &context, |a| a.auto_reload_pending);
        assert_eq!(title(&app), "Edition one");
        assert_eq!(
            app.comments.editor.as_ref().unwrap().contents,
            "unsaved draft"
        );
        assert!(!app.loading && !app.comments.open);
        app.comments.editor = None;
        app.selection.active = true;
        frame(&mut app, &context);
        assert!(!app.loading && app.auto_reload_pending);
        app.selection.cancel();
        wait(&mut app, &context, |a| {
            !a.loading && title(a) == "Edition two"
        });
        assert!(app.error.is_none());
    }

    #[test]
    fn incomplete_or_missing_pdf_keeps_snapshot_and_recovers_on_the_next_change() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("document.pdf");
        let (mut app, context) = open(&path, &root.path().join("library.json"));
        fs::write(&path, b"incomplete compiler output").unwrap();
        wait(&mut app, &context, |a| !a.loading && a.error.is_some());
        assert_eq!(title(&app), "Edition one");
        assert!(app.file_changed && !app.auto_reload_pending && !app.password_open);
        let error = app.error.clone();
        let until = Instant::now() + Duration::from_millis(1200);
        while Instant::now() < until {
            frame(&mut app, &context);
            assert!(!app.loading && !app.auto_reload_pending);
            std::thread::sleep(Duration::from_millis(20));
        }
        assert_eq!(app.error, error);
        fs::remove_file(&path).unwrap();
        wait(&mut app, &context, |a| {
            a.error.as_ref().is_some_and(|e| e.contains("Cannot read"))
        });
        assert_eq!(title(&app), "Edition one");
        fs::write(&path, pdf("Edition two")).unwrap();
        wait(&mut app, &context, |a| {
            !a.loading && title(a) == "Edition two"
        });
        assert!(app.error.is_none() && !app.file_changed);
    }

    #[test]
    fn encrypted_update_does_not_steal_focus_with_a_password_dialog() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("document.pdf");
        let (mut app, context) = open(&path, &root.path().join("library.json"));
        fs::write(&path, include_bytes!("../../tests/fixtures/encrypted.pdf")).unwrap();
        wait(&mut app, &context, |a| !a.loading && a.error.is_some());
        assert!(!app.password_open);
        assert_eq!(title(&app), "Edition one");
        assert!(app.error.as_ref().unwrap().contains("password"));
        app.reload();
        wait(&mut app, &context, |a| a.password_open);
        assert!(!app.automatic_reload);
    }
}
