use super::{Pagewing, tooltips::LazyTooltip};
use eframe::egui;

impl Pagewing {
    pub(super) fn file_change_notice(&mut self, context: &egui::Context, canvas: egui::Rect) {
        // A narrow canvas may share the reader with a sidebar. Let the notice
        // use the window width while keeping it between the reader bars.
        let screen = context.content_rect();
        let bounds = egui::Rect::from_min_max(
            egui::pos2(screen.left() + 8.0, canvas.top() + 4.0),
            egui::pos2(screen.right() - 8.0, canvas.bottom() - 4.0),
        );
        egui::Area::new(egui::Id::new("file-change-notice"))
            .order(egui::Order::Foreground)
            .fixed_pos(canvas.center_top() + egui::vec2(0.0, 12.0))
            .pivot(egui::Align2::CENTER_TOP)
            .constrain_to(bounds)
            .show(context, |ui| {
                ui.set_max_width(bounds.width());
                let frame = egui::Frame::popup(ui.style());
                let width = (bounds.width() - frame.total_margin().sum().x).max(1.0);
                frame.show(ui, |ui| {
                    ui.set_max_width(width);
                    ui.horizontal_wrapped(|ui| {
                        ui.label("PDF changed on disk.");
                        let reload = ui
                            .add_enabled(!self.loading, egui::Button::new("Reload"))
                            .on_hover_text_lazy(|| {
                                format!(
                                    "{} — keep current reading position",
                                    self.library.shortcuts.label("reload")
                                )
                            });
                        self.input_buttons.reload_changed = Some(reload.id);
                        self.input_buttons.dismiss_change = Some(ui.button("Dismiss").id);
                    });
                });
            });
    }

    /// The worker debounces disk writes. Consume each change once, and defer
    /// while an editor or gesture still owns the current document snapshot.
    pub(super) fn reload_if_changed(&mut self, context: &egui::Context) {
        if !self.auto_reload_pending
            || self.loading
            || self.nav.count == 0
            || self.comments.busy()
            || self.comments.editor.is_some()
            || self.selection.active
            || self.password_input.is_some()
            || context.input(|i| i.pointer.any_down())
        {
            return;
        }
        self.reload(context);
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

    #[test]
    fn file_change_notice_keeps_its_actions_visible_in_narrow_windows() {
        let root = tempfile::tempdir().unwrap();
        let context = Context::default();
        let mut app =
            Pagewing::with_context(&context, None, None, root.path().join("library.json"));
        for size in super::super::dialog::TEST_SIZES {
            let screen = Rect::from_min_size(egui::Pos2::ZERO, size);
            for (sidebar, top) in [(false, 64.0), (true, 64.0), (false, 108.0), (true, 108.0)] {
                let canvas = Rect::from_min_max(
                    egui::pos2(if sidebar { size.x * 0.5 } else { 0.0 }, top),
                    egui::pos2(size.x, size.y - 36.0),
                );
                let mut output = egui::FullOutput::default();
                for _ in 0..4 {
                    output = context.run(
                        RawInput {
                            screen_rect: Some(screen),
                            ..Default::default()
                        },
                        |context| app.file_change_notice(context, canvas),
                    );
                }
                for label in ["Reload", "Dismiss"] {
                    let (clip, rect) = output
                        .shapes
                        .iter()
                        .find_map(|shape| match &shape.shape {
                            egui::Shape::Text(text) if text.galley.text() == label => Some((
                                shape.clip_rect,
                                Rect::from_min_size(text.pos, text.galley.size()),
                            )),
                            _ => None,
                        })
                        .unwrap();
                    assert!(
                        screen.contains_rect(rect) && clip.contains_rect(rect),
                        "{label} at {size:?} (sidebar: {sidebar}): {rect:?} clipped by {clip:?}"
                    );
                }
                for id in [
                    app.input_buttons.reload_changed.unwrap(),
                    app.input_buttons.dismiss_change.unwrap(),
                ] {
                    let response = context.read_response(id).unwrap();
                    assert!(screen.contains_rect(response.rect));
                    assert!(response.interact_rect.contains_rect(response.rect));
                }
                let notice = context
                    .memory(|memory| memory.area_rect(egui::Id::new("file-change-notice")))
                    .unwrap();
                let reader = Rect::from_min_max(egui::pos2(0.0, top), canvas.max);
                assert!(
                    reader.contains_rect(notice),
                    "{notice:?} outside {reader:?}"
                );

                let position = context
                    .read_response(app.input_buttons.dismiss_change.unwrap())
                    .unwrap()
                    .rect
                    .center();
                app.file_changed = true;
                app.auto_reload_pending = true;
                for pressed in [true, false] {
                    let _ = context.run(
                        RawInput {
                            screen_rect: Some(screen),
                            events: vec![
                                egui::Event::PointerMoved(position),
                                egui::Event::PointerButton {
                                    pos: position,
                                    button: egui::PointerButton::Primary,
                                    pressed,
                                    modifiers: egui::Modifiers::NONE,
                                },
                            ],
                            ..Default::default()
                        },
                        |context| {
                            app.keys(context);
                            app.file_change_notice(context, canvas);
                        },
                    );
                }
                assert!(!app.file_changed && !app.auto_reload_pending);
            }
        }
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
        assert!(app.file_changed && !app.auto_reload_pending && app.password_input.is_none());
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
            a.error
                .as_ref()
                .is_some_and(|e| e.message.contains("Cannot read"))
        });
        assert_eq!(title(&app), "Edition one");
        fs::write(&path, pdf("Edition two")).unwrap();
        wait(&mut app, &context, |a| {
            !a.loading && title(a) == "Edition two"
        });
        assert!(app.error.is_none() && !app.file_changed);
    }

    #[test]
    fn failed_initial_open_can_retry_after_the_file_is_repaired() {
        let root = tempfile::tempdir().unwrap();
        let context = Context::default();
        let mut app =
            Pagewing::with_context(&context, None, None, root.path().join("library.json"));
        app.reload(&context);
        assert!(!app.loading, "An empty reader has no file to retry");
        // A path containing an engine error name must remain an ordinary I/O failure.
        let path = root.path().join("Decryption.pdf");
        app.open(path.clone(), &context);
        wait(&mut app, &context, |a| !a.loading && a.error.is_some());
        assert_eq!(app.nav.count, 0);
        assert!(app.password_input.is_none());
        let output = frame(&mut app, &context);
        assert!(output.shapes.iter().any(|shape| matches!(&shape.shape, egui::Shape::Text(text) if text.galley.text() == "Retry open")));
        fs::write(&path, pdf("Repaired PDF")).unwrap();
        app.reload(&context);
        wait(&mut app, &context, |a| !a.loading && a.nav.count > 0);
        assert!(!app.reloading);
        assert!(app.error.is_none());
        assert_eq!(title(&app), "Repaired PDF");
    }

    #[test]
    fn terminal_encryption_errors_keep_the_snapshot_without_asking_for_a_password() {
        for (from, to) in [("/V 1", "/V 9"), ("/R 2", "/Q 2")] {
            for automatic in [false, true] {
                let root = tempfile::tempdir().unwrap();
                let path = root.path().join("document.pdf");
                let (mut app, context) = open(&path, &root.path().join("library.json"));
                let mut data = include_bytes!("../../tests/fixtures/encrypted.pdf").to_vec();
                let offset = data
                    .windows(from.len())
                    .position(|bytes| bytes == from.as_bytes())
                    .unwrap();
                data[offset..offset + from.len()].copy_from_slice(to.as_bytes());
                fs::write(&path, data).unwrap();
                app.reload(&context);
                app.automatic_reload = automatic;
                wait(&mut app, &context, |a| !a.loading && a.error.is_some());
                assert!(app.password_input.is_none());
                assert!(!app.error.as_ref().unwrap().message.contains("password"));
                assert_eq!(title(&app), "Edition one");
            }
        }
    }

    #[test]
    fn password_retries_show_feedback_in_the_dialog_and_cancel_explains_recovery() {
        let root = tempfile::tempdir().unwrap();
        let context = Context::default();
        let mut app =
            Pagewing::with_context(&context, None, None, root.path().join("library.json"));
        app.open(
            Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/encrypted.pdf"),
            &context,
        );
        wait(&mut app, &context, |a| a.password_input.is_some());
        assert!(app.error.is_none());
        // Allow the new field to acquire focus before sending the password.
        for _ in 0..4 {
            frame(&mut app, &context);
        }
        frame_events(&mut app, &context, vec![egui::Event::Text("wrong".into())]);
        frame_events(
            &mut app,
            &context,
            [true, false]
                .map(|pressed| egui::Event::Key {
                    key: egui::Key::Enter,
                    physical_key: None,
                    pressed,
                    repeat: false,
                    modifiers: egui::Modifiers::NONE,
                })
                .into(),
        );
        wait(&mut app, &context, |a| {
            !a.loading && a.password_input.is_some()
        });
        assert!(app.error.is_none());
        let output = frame(&mut app, &context);
        assert!(output.shapes.iter().any(|shape| matches!(&shape.shape, egui::Shape::Text(text) if text.galley.text() == "Password not accepted. Try again.")));
        frame_events(
            &mut app,
            &context,
            vec![egui::Event::Key {
                key: egui::Key::Escape,
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers: egui::Modifiers::NONE,
            }],
        );
        assert!(app.password_input.is_none());
        assert!(
            app.error
                .as_ref()
                .unwrap()
                .message
                .contains("needs a password")
        );
        app.reload(&context);
        wait(&mut app, &context, |a| a.password_input.is_some());
        assert!(app.error.is_none());
    }

    #[test]
    fn encrypted_update_does_not_steal_focus_with_a_password_dialog() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("document.pdf");
        let (mut app, context) = open(&path, &root.path().join("library.json"));
        fs::write(&path, include_bytes!("../../tests/fixtures/encrypted.pdf")).unwrap();
        wait(&mut app, &context, |a| !a.loading && a.error.is_some());
        assert!(app.password_input.is_none());
        assert_eq!(title(&app), "Edition one");
        assert!(app.error.as_ref().unwrap().message.contains("password"));
        app.reload(&context);
        wait(&mut app, &context, |a| a.password_input.is_some());
        assert!(!app.automatic_reload);
    }
}
