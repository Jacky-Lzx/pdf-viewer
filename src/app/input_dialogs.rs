use super::{Pagewing, appearance, dialog, shortcuts};
use eframe::egui;

mod page_preview;

pub(super) fn page_id() -> egui::Id {
    egui::Id::new("pagewing-page-input")
}

pub(super) fn password_id() -> egui::Id {
    egui::Id::new("pagewing-password-input")
}

pub(super) struct PageInput {
    value: String,
    pub focus: bool,
    pub confirm: bool,
    invalid: bool,
    preview: page_preview::Preview,
}

impl Default for PageInput {
    fn default() -> Self {
        Self {
            value: String::new(),
            focus: true,
            confirm: false,
            invalid: false,
            preview: page_preview::Preview::default(),
        }
    }
}

impl PageInput {
    pub(super) fn invalidate_preview(&mut self) {
        self.preview.invalidate();
        self.invalid = false;
    }
}

pub(super) struct PasswordInput {
    value: String,
    pub focus: bool,
    pub confirm: bool,
    reveal: bool,
    rejected: bool,
}

impl Default for PasswordInput {
    fn default() -> Self {
        Self {
            value: String::new(),
            focus: true,
            confirm: false,
            reveal: false,
            rejected: false,
        }
    }
}

impl Pagewing {
    fn goto_target(&self, value: &str) -> Option<page_preview::Target> {
        let page = crate::navigation::relative_page(value, self.nav.page, self.nav.count)
            .or_else(|| crate::navigation::percentage_input(value, self.nav.count))
            .or_else(|| self.labels.resolve_input(value, self.nav.count))?;
        Some(page_preview::Target {
            page,
            caption: format!("Target: {}", self.labels.sidebar_caption(page)),
            tooltip: self.labels.sidebar_tooltip(page),
        })
    }

    pub(super) fn open_goto(&mut self) {
        if self.nav.count == 0 {
            return;
        }
        self.outline.cancel_input();
        self.zoom_input = None;
        self.nav.clear_prefix();
        self.hints.cancel();
        self.selection.cancel();
        self.mark_action = None;
        self.recent_picker = None;
        self.properties_open = false;
        self.help = false;
        self.password_input = None;
        self.search_open = false;
        self.search_focus = false;
        self.search_navigation.clear();
        self.page_input = Some(PageInput::default());
    }

    pub(super) fn close_page_input(&mut self, context: &egui::Context) {
        self.page_input = None;
        context.memory_mut(|memory| memory.surrender_focus(page_id()));
    }

    pub(super) fn open_password(&mut self, context: &egui::Context, rejected: bool) {
        self.outline.cancel_input();
        self.nav.clear_prefix();
        self.hints.cancel();
        self.mark_action = None;
        self.close_zoom(context);
        self.close_page_input(context);
        self.help = false;
        self.properties_open = false;
        self.recent_picker = None;
        self.password_input = Some(PasswordInput {
            rejected,
            ..Default::default()
        });
        context.memory_mut(|memory| {
            if let Some(id) = memory.focused() {
                memory.surrender_focus(id);
            }
            memory.move_focus(egui::FocusDirection::None);
        });
    }

    pub(super) fn close_password_input(&mut self, context: &egui::Context) {
        if self.password_input.take().is_some() {
            self.error = Some(super::errors::Notice::retry_document(
                "This PDF needs a password.",
            ));
        }
        context.memory_mut(|memory| memory.surrender_focus(password_id()));
    }

    pub(super) fn page_dialog(&mut self, context: &egui::Context) {
        let Some(mut input) = self.page_input.take() else {
            return;
        };
        if self.nav.count == 0 {
            self.close_page_input(context);
            return;
        }
        let mut open = true;
        let mut cancel = false;
        let mut chosen = None;
        let source = page_preview::Source {
            document: self.id,
            page: self.nav.page,
            count: self.nav.count,
        };
        dialog::window(context, "Go to page", "Go to page", 360.0)
            .open(&mut open)
            .show(context, |ui| {
                ui.label(format!("Physical page 1–{}", self.nav.count));
                let (response, _) = shortcuts::text_input(
                    ui,
                    page_id(),
                    &mut input.focus,
                    &mut self.deferred_input,
                    egui::TextEdit::singleline(&mut input.value)
                        .desired_width(f32::INFINITY)
                        .char_limit(4096)
                        .hint_text("42, +5, 50% or PDF label"),
                );
                if response.changed() {
                    input.invalid = false;
                }
                response.on_hover_text(
                    "Relative: +5, -5. Progress: 50%. PDF label: iv, A-3; =1 for label 1.",
                );
                if input.invalid {
                    ui.colored_label(
                        ui.visuals().error_fg_color,
                        "Use a page, +offset, 50% or label.",
                    );
                } else {
                    match input.preview.update(
                        &input.value,
                        source,
                        std::time::Instant::now(),
                        false,
                        |value| self.goto_target(value),
                    ) {
                        page_preview::State::Target(target) => {
                            ui.add(egui::Label::new(&target.caption).truncate())
                                .on_hover_text(&target.tooltip);
                        }
                        state => {
                            let text = match state {
                                page_preview::State::Empty => "Enter a page or PDF label.",
                                page_preview::State::Pending(delay) => {
                                    context.request_repaint_after(delay);
                                    "Checking target…"
                                }
                                page_preview::State::Missing => "No matching page.",
                                page_preview::State::Target(_) => unreachable!(),
                            };
                            ui.add(egui::Label::new(egui::RichText::new(text).weak()).truncate());
                        }
                    }
                }
                if context.content_rect().height() >= 420.0 {
                    ui.weak("Relative: +5, -5. Progress: 50%.");
                    ui.weak("PDF label: iv, A-3; =1 for label 1.");
                }
                ui.separator();
                ui.horizontal(|ui| {
                    let go =
                        appearance::action_button(ui, "page-go", egui::Button::new("Go")).clicked();
                    cancel =
                        appearance::action_button(ui, "page-cancel", egui::Button::new("Cancel"))
                            .on_hover_text("Escape")
                            .clicked();
                    if !ui.is_sizing_pass() && !cancel && (go || std::mem::take(&mut input.confirm))
                    {
                        if let page_preview::State::Target(target) = input.preview.update(
                            &input.value,
                            source,
                            std::time::Instant::now(),
                            true,
                            |value| self.goto_target(value),
                        ) {
                            chosen = Some(target.page);
                        }
                        if chosen.is_none() {
                            input.invalid = true;
                            input.focus = true;
                            context.request_repaint();
                        }
                    }
                });
            });
        if !open || cancel {
            chosen = None;
        }
        if let Some(page) = chosen {
            self.jump(page);
        }
        if open && !cancel && chosen.is_none() {
            self.page_input = Some(input);
        } else {
            self.close_page_input(context);
        }
    }

    pub(super) fn password_dialog(&mut self, context: &egui::Context) {
        let Some(mut input) = self.password_input.take() else {
            return;
        };
        let mut open = true;
        let mut cancel = false;
        let mut unlock = false;
        dialog::window(context, "Password required", "Unlock PDF", 320.0)
            .open(&mut open)
            .show(context, |ui| {
                if input.rejected {
                    ui.colored_label(
                        ui.visuals().error_fg_color,
                        "Password not accepted. Try again.",
                    );
                } else {
                    ui.label("This document is encrypted.");
                }
                shortcuts::text_input(
                    ui,
                    password_id(),
                    &mut input.focus,
                    &mut self.deferred_input,
                    egui::TextEdit::singleline(&mut input.value)
                        .desired_width(f32::INFINITY)
                        .hint_text("Password")
                        .password(!input.reveal),
                );
                ui.checkbox(&mut input.reveal, "Show password");
                ui.separator();
                ui.horizontal(|ui| {
                    let clicked = ui.button("Unlock").clicked();
                    cancel = ui.button("Cancel").on_hover_text("Escape").clicked();
                    if !ui.is_sizing_pass() && !cancel {
                        unlock = clicked || std::mem::take(&mut input.confirm);
                    }
                });
            });
        if !open || cancel {
            self.password_input = Some(input);
            self.close_password_input(context);
        } else if unlock {
            self.close_password_input(context);
            self.error = None;
            self.loading = true;
            if self.reloading {
                self.worker
                    .reload(self.id, self.reading.path.clone(), input.value);
            } else {
                self.worker
                    .open(self.id, self.reading.path.clone(), input.value);
            }
        } else {
            self.password_input = Some(input);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::navigation::Navigation;
    use eframe::egui;

    fn key(key: egui::Key) -> Vec<egui::Event> {
        [true, false]
            .into_iter()
            .map(|pressed| egui::Event::Key {
                key,
                physical_key: None,
                pressed,
                repeat: false,
                modifiers: egui::Modifiers::NONE,
            })
            .collect()
    }

    fn frame(
        app: &mut Pagewing,
        context: &egui::Context,
        size: egui::Vec2,
        mut events: Vec<egui::Event>,
    ) -> egui::FullOutput {
        let mut deferred = std::mem::take(&mut app.deferred_input);
        deferred.append(&mut events);
        context.run(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, size)),
                events: deferred,
                ..Default::default()
            },
            |context| {
                app.keys(context);
                app.dialogs(context);
                egui::CentralPanel::default().show(context, |_| {});
            },
        )
    }

    fn reader(directory: &std::path::Path) -> (Pagewing, egui::Context) {
        let context = egui::Context::default();
        let mut app = Pagewing::with_context(&context, None, None, directory.join("library.json"));
        app.nav = Navigation::new(6, 2);
        app.dimensions = vec![[600.0, 800.0]; 6];
        (app, context)
    }

    fn label_reader(directory: &std::path::Path) -> (Pagewing, egui::Context) {
        let (mut app, context) = reader(directory);
        app.labels = crate::page_labels::PageLabels::read(
            &hayro::hayro_syntax::Pdf::new(
                include_bytes!("../../assets/pagewing-guide.pdf").to_vec(),
            )
            .unwrap(),
        );
        (app, context)
    }

    // Advance the preview's scheduling clock without a wall-clock sleep; its
    // actual lookup and display still execute through the next dialog frame.
    fn due_preview(app: &mut Pagewing, context: &egui::Context, size: egui::Vec2) {
        // Opening-frame edits can be deferred until text focus is established.
        for _ in 0..4 {
            frame(app, context, size, Vec::new());
        }
        let source = page_preview::Source {
            document: app.id,
            page: app.nav.page,
            count: app.nav.count,
        };
        let input = app.page_input.as_mut().unwrap();
        assert!(!input.value.is_empty());
        input.preview.invalidate();
        input.preview.update(
            &input.value,
            source,
            std::time::Instant::now() - std::time::Duration::from_secs(1),
            false,
            |_| panic!("a new edit should wait before resolving"),
        );
    }

    fn preview_text(output: &egui::FullOutput) -> Option<&str> {
        output.shapes.iter().find_map(|shape| match &shape.shape {
            egui::Shape::Text(text) if text.galley.text().starts_with("Target: ") => {
                Some(text.galley.text())
            }
            _ => None,
        })
    }

    #[test]
    fn page_target_preview_distinguishes_labels_and_physical_pages_without_navigating() {
        let directory = tempfile::tempdir().unwrap();
        let (mut app, context) = label_reader(directory.path());
        let size = egui::vec2(1180.0, 820.0);
        app.viewport = egui::vec2(400.0, 300.0);
        app.reading.fit = crate::navigation::Fit::Custom(2.0);
        app.reading.rotation = 1;
        app.pan = egui::vec2(13.0, 27.0);
        app.query = "retained search".into();
        let position = app.current_position();
        for (value, expected) in [
            ("1", "Target: Page 1 · i"),
            ("=1", "Target: Page 3 · 1"),
            ("ii", "Target: Page 2 · ii"),
            ("A-2", "Target: Page 6 · A-2"),
            ("+2", "Target: Page 5 · A-1"),
            ("50%", "Target: Page 3 · 1"),
        ] {
            app.open_goto();
            frame(
                &mut app,
                &context,
                size,
                vec![egui::Event::Paste(value.into())],
            );
            due_preview(&mut app, &context, size);
            let mut output = egui::FullOutput::default();
            for _ in 0..4 {
                output = frame(&mut app, &context, size, Vec::new());
            }
            assert_eq!(preview_text(&output), Some(expected), "{value}");
            assert_eq!(app.nav.page, 2);
            assert_eq!(app.pan, egui::vec2(13.0, 27.0));
            assert_eq!(app.reading.fit, crate::navigation::Fit::Custom(2.0));
            assert_eq!(app.reading.rotation, 1);
            assert_eq!(app.query, "retained search");
            assert!(!app.history.can_go(false, &position, app.nav.count));
            assert!(!app.history.can_go(true, &position, app.nav.count));
            assert!(app.history.previous_jump(app.nav.count).is_none());
            frame(&mut app, &context, size, key(egui::Key::Escape));
        }
    }

    #[test]
    fn page_target_preview_keeps_actions_visible_and_submits_the_latest_edit() {
        let directory = tempfile::tempdir().unwrap();
        let (mut app, context) = label_reader(directory.path());
        app.open_goto();
        let size = egui::vec2(1180.0, 820.0);
        frame(
            &mut app,
            &context,
            size,
            vec![egui::Event::Paste("1".into())],
        );
        due_preview(&mut app, &context, size);
        for size in dialog::TEST_SIZES {
            let mut output = egui::FullOutput::default();
            for _ in 0..4 {
                output = frame(&mut app, &context, size, Vec::new());
            }
            dialog::assert_visible_actions(
                &context,
                &output,
                egui::Rect::from_min_size(egui::Pos2::ZERO, size),
                "Go to page",
                &["Go", "Cancel", "Target: Page 1 · i"],
            );
        }
        let mut events: Vec<_> = key(egui::Key::A)
            .into_iter()
            .map(|event| match event {
                egui::Event::Key {
                    key,
                    physical_key,
                    pressed,
                    repeat,
                    ..
                } => egui::Event::Key {
                    key,
                    physical_key,
                    pressed,
                    repeat,
                    modifiers: egui::Modifiers::COMMAND,
                },
                other => other,
            })
            .collect();
        events.push(egui::Event::Paste("=1".into()));
        events.extend(key(egui::Key::Enter));
        frame(&mut app, &context, size, events);
        for _ in 0..4 {
            frame(&mut app, &context, size, Vec::new());
        }
        assert!(app.page_input.is_none());
        assert_eq!(app.nav.page, 2, "Enter must use =1, not the preview of 1");
    }

    fn check_reloaded_preview(
        value: &str,
        old_caption: Option<&str>,
        range_start: i64,
        expected_page: usize,
        new_caption: &str,
    ) {
        use std::time::{Duration, Instant};
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("labels.pdf");
        std::fs::write(&path, include_bytes!("../../assets/pagewing-guide.pdf")).unwrap();
        let context = egui::Context::default();
        let mut app = Pagewing::with_context(
            &context,
            Some(path.clone()),
            None,
            directory.path().join("library.json"),
        );
        let wait = |app: &mut Pagewing| {
            let deadline = Instant::now() + Duration::from_secs(8);
            while app.loading && Instant::now() < deadline {
                app.events(&context);
                std::thread::sleep(Duration::from_millis(10));
            }
            assert!(!app.loading, "the PDF should finish loading");
            assert!(app.error.is_none());
        };
        wait(&mut app);
        let size = egui::vec2(240.0, 240.0);
        app.open_goto();
        frame(
            &mut app,
            &context,
            size,
            vec![egui::Event::Paste(value.into())],
        );
        due_preview(&mut app, &context, size);
        for _ in 0..4 {
            frame(&mut app, &context, size, Vec::new());
        }
        let output = frame(&mut app, &context, size, Vec::new());
        assert_eq!(preview_text(&output), old_caption);
        if old_caption.is_none() {
            frame(&mut app, &context, size, key(egui::Key::Enter));
            assert!(app.page_input.as_ref().unwrap().invalid);
        }
        let id = app.id;
        let mut document = lopdf::Document::load(&path).unwrap();
        let root = document
            .trailer
            .get(b"Root")
            .unwrap()
            .as_reference()
            .unwrap();
        document.get_object_mut(root).unwrap().as_dict_mut().unwrap().set("PageLabels",
            lopdf::dictionary! { "Nums" => vec![range_start.into(), lopdf::dictionary! { "S" => "D" }.into()] });
        document.save(&path).unwrap();
        app.reload(&context);
        wait(&mut app);
        assert_eq!((app.id, app.nav.count), (id, 6));
        assert_eq!(app.page_input.as_ref().unwrap().value, value);
        assert!(!app.page_input.as_ref().unwrap().invalid);
        frame(&mut app, &context, size, Vec::new());
        std::thread::sleep(Duration::from_millis(180));
        let mut output = egui::FullOutput::default();
        for _ in 0..4 {
            output = frame(&mut app, &context, size, Vec::new());
        }
        assert_eq!(preview_text(&output), Some(new_caption));
        frame(&mut app, &context, size, key(egui::Key::Enter));
        for _ in 0..4 {
            frame(&mut app, &context, size, Vec::new());
        }
        assert!(app.page_input.is_none());
        assert_eq!(
            app.nav.page, expected_page,
            "reload must invalidate the old preview or error"
        );
    }

    #[test]
    fn reloaded_page_labels_replace_a_preview_even_when_document_page_count_is_unchanged() {
        check_reloaded_preview("=1", Some("Target: Page 3 · 1"), 4, 4, "Target: Page 5 · 1");
        check_reloaded_preview("=3", None, 3, 5, "Target: Page 6 · 3");
    }

    #[test]
    fn input_windows_and_actions_fit_when_resizing() {
        let directory = tempfile::tempdir().unwrap();
        for password in [false, true] {
            let (mut app, context) = reader(directory.path());
            if password {
                app.open_password(&context, false);
            } else {
                app.open_goto();
            }
            for size in dialog::TEST_SIZES {
                for extra in [false, true] {
                    if let Some(input) = &mut app.page_input {
                        input.invalid = extra;
                    }
                    if let Some(input) = &mut app.password_input {
                        input.reveal = extra;
                        input.rejected = extra;
                    }
                    let mut output = egui::FullOutput::default();
                    for _ in 0..4 {
                        output = frame(&mut app, &context, size, Vec::new());
                    }
                    dialog::assert_visible_actions(
                        &context,
                        &output,
                        egui::Rect::from_min_size(egui::Pos2::ZERO, size),
                        if password {
                            "Password required"
                        } else {
                            "Go to page"
                        },
                        if password && extra {
                            &[
                                "Unlock",
                                "Cancel",
                                "Show password",
                                "Password not accepted. Try again.",
                            ]
                        } else if password {
                            &["Unlock", "Cancel", "Show password"]
                        } else if extra {
                            &["Go", "Cancel", "Use a page, +offset, 50% or label."]
                        } else {
                            &["Go", "Cancel"]
                        },
                    );
                }
            }
        }
    }

    #[test]
    fn escape_cancels_the_foreground_input_before_the_comments_panel() {
        let directory = tempfile::tempdir().unwrap();
        for password in [false, true] {
            let (mut app, context) = reader(directory.path());
            app.comments.open = true;
            if password {
                app.open_password(&context, false);
            } else {
                app.open_goto();
            }
            let size = egui::vec2(1180.0, 820.0);
            for _ in 0..4 {
                frame(&mut app, &context, size, Vec::new());
            }
            frame(&mut app, &context, size, key(egui::Key::Escape));
            assert!(
                app.page_input.is_none(),
                "Escape should close the page dialog"
            );
            assert!(app.password_input.is_none());
            assert!(
                app.comments.open,
                "the background comments panel should remain open"
            );
            assert!(!context.memory(|memory| memory.has_focus(if password {
                password_id()
            } else {
                page_id()
            })));
        }
    }

    #[test]
    fn page_submission_accepts_batched_open_text_and_enter_and_keeps_invalid_text_editable() {
        let directory = tempfile::tempdir().unwrap();
        let (mut app, context) = reader(directory.path());
        app.labels = crate::page_labels::PageLabels::read(
            &hayro::hayro_syntax::Pdf::new(
                include_bytes!("../../assets/pagewing-guide.pdf").to_vec(),
            )
            .unwrap(),
        );
        let size = egui::vec2(240.0, 240.0);
        for (value, expected) in [
            ("+2", 4),
            ("A-2", 5),
            ("=1", 2),
            ("50%", 2),
            ("51%", 3),
            ("0%", 0),
            ("101%", 5),
            (" 25 % ", 1),
            ("2", 1),
        ] {
            app.nav.page = 2;
            let mut events = key(egui::Key::Colon);
            events.push(egui::Event::Paste(value.into()));
            events.extend(key(egui::Key::Enter));
            frame(&mut app, &context, size, events);
            for _ in 0..4 {
                frame(&mut app, &context, size, Vec::new());
            }
            assert!(app.page_input.is_none(), "{value}");
            assert_eq!(app.nav.page, expected, "{value}");
        }
        let mut events = key(egui::Key::Colon);
        for (key_code, text) in [
            (egui::Key::J, "j"),
            (egui::Key::K, "k"),
            (egui::Key::D, "d"),
        ] {
            events.extend(key(key_code));
            events.push(egui::Event::Text(text.into()));
        }
        events.extend(key(egui::Key::Enter));
        frame(&mut app, &context, size, events);
        for _ in 0..4 {
            frame(&mut app, &context, size, Vec::new());
        }
        let input = app.page_input.as_ref().unwrap();
        assert!(input.invalid);
        assert_eq!(input.value, "jkd");
        assert_eq!(app.nav.page, 1);
        assert!(!app.library.dark);
    }

    #[test]
    fn percentage_submission_keeps_literal_pdf_labels_accessible() {
        let directory = tempfile::tempdir().unwrap();
        let (mut app, context) = reader(directory.path());
        let mut data = include_bytes!("../../assets/pagewing-guide.pdf").to_vec();
        // Preserve xref offsets while making the last range a literal 5% label.
        let original = b"/S /D /P (A-)";
        let replacement = b"      /P (5%)";
        let offset = data
            .windows(original.len())
            .position(|bytes| bytes == original)
            .unwrap();
        data[offset..offset + original.len()].copy_from_slice(replacement);
        app.labels =
            crate::page_labels::PageLabels::read(&hayro::hayro_syntax::Pdf::new(data).unwrap());
        let size = egui::vec2(240.0, 240.0);
        for (value, expected) in [("5%", 0), ("=5%", 4), ("50%", 2), ("=1", 2)] {
            let mut events = key(egui::Key::Colon);
            events.push(egui::Event::Paste(value.into()));
            events.extend(key(egui::Key::Enter));
            frame(&mut app, &context, size, events);
            for _ in 0..4 {
                frame(&mut app, &context, size, Vec::new());
            }
            assert!(app.page_input.is_none(), "{value}");
            assert_eq!(app.nav.page, expected, "{value}");
        }
    }

    #[test]
    fn page_actions_keep_focus_and_activation_when_the_window_changes_size() {
        let directory = tempfile::tempdir().unwrap();
        for activation in [egui::Key::Enter, egui::Key::Space] {
            for (tabs, expected) in [(1, 5), (2, 2)] {
                let (mut app, context) = reader(directory.path());
                app.open_goto();
                app.page_input.as_mut().unwrap().value = "6".into();
                let large = egui::vec2(1180.0, 820.0);
                for _ in 0..4 {
                    frame(&mut app, &context, large, Vec::new());
                }
                for _ in 0..tabs {
                    frame(&mut app, &context, large, key(egui::Key::Tab));
                }
                let focused = context.memory(|memory| memory.focused()).unwrap();
                for size in dialog::TEST_SIZES {
                    for _ in 0..4 {
                        frame(&mut app, &context, size, Vec::new());
                    }
                    assert_eq!(
                        context.memory(|memory| memory.focused()),
                        Some(focused),
                        "{tabs} tabs, {size:?}"
                    );
                }
                frame(&mut app, &context, large, key(activation));
                assert!(app.page_input.is_none());
                assert_eq!(app.nav.page, expected);
            }
        }
    }

    #[test]
    fn cancel_buttons_do_not_submit_the_text_and_drop_the_input_state() {
        let directory = tempfile::tempdir().unwrap();
        for password in [false, true] {
            let (mut app, context) = reader(directory.path());
            if password {
                app.open_password(&context, false);
            } else {
                app.open_goto();
            }
            let size = egui::vec2(240.0, 240.0);
            for _ in 0..4 {
                frame(&mut app, &context, size, Vec::new());
            }
            frame(
                &mut app,
                &context,
                size,
                vec![egui::Event::Text(
                    if password { "secret" } else { "6" }.into(),
                )],
            );
            for _ in 0..if password { 3 } else { 2 } {
                frame(&mut app, &context, size, key(egui::Key::Tab));
            }
            frame(&mut app, &context, size, key(egui::Key::Enter));
            assert!(app.page_input.is_none() && app.password_input.is_none());
            assert_eq!(app.nav.page, 2);
            assert!(!app.loading, "Cancel must not dispatch a password attempt");
        }
    }

    #[test]
    fn show_password_toggles_the_rendered_text_without_submitting_it() {
        let directory = tempfile::tempdir().unwrap();
        let (mut app, context) = reader(directory.path());
        app.open_password(&context, false);
        let size = egui::vec2(240.0, 240.0);
        for _ in 0..4 {
            frame(&mut app, &context, size, Vec::new());
        }
        frame(
            &mut app,
            &context,
            size,
            vec![egui::Event::Text("test password".into())],
        );
        let shows_value = |output: &egui::FullOutput| {
            output.shapes.iter().any(|shape| {
            matches!(&shape.shape, egui::Shape::Text(text) if text.galley.text() == "test password")
        })
        };
        assert!(!shows_value(&frame(&mut app, &context, size, Vec::new())));
        frame(&mut app, &context, size, key(egui::Key::Tab));
        frame(&mut app, &context, size, key(egui::Key::Space));
        assert!(shows_value(&frame(&mut app, &context, size, Vec::new())));
        assert!(!app.loading);
        frame(&mut app, &context, size, key(egui::Key::Space));
        assert!(!shows_value(&frame(&mut app, &context, size, Vec::new())));
    }
}
