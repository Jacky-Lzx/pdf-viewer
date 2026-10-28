use super::{Pagewing, tooltips::LazyTooltip};
use crate::{document::LoadError, notice_panel::NoticePanel, worker::Failure};
use eframe::egui;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Recovery {
    Document,
    Page,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct Notice {
    pub message: String,
    recovery: Option<Recovery>,
}

impl Notice {
    pub fn retry_document(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            recovery: Some(Recovery::Document),
        }
    }
}

impl From<String> for Notice {
    fn from(message: String) -> Self {
        Self {
            message,
            recovery: None,
        }
    }
}

impl From<&str> for Notice {
    fn from(message: &str) -> Self {
        message.to_owned().into()
    }
}

impl Pagewing {
    pub(super) fn worker_error(&mut self, context: &egui::Context, failure: Failure) {
        if self.loading && !matches!(failure, Failure::Open(_)) {
            return;
        }
        self.error = Some(match failure {
            Failure::Open(error) => {
                let automatic = self.automatic_reload;
                if self.loading && self.reloading {
                    self.restart_search();
                    self.requested = None;
                    self.thumbnails.invalidate_requests();
                }
                self.loading = false;
                self.automatic_reload = false;
                if !error.needs_password() {
                    self.synctex.cancel_pending();
                } else if !automatic {
                    self.open_password(
                        context,
                        matches!(error, LoadError::PasswordRequired { rejected: true }),
                    );
                    self.error = None;
                    return;
                }
                Notice::retry_document(if automatic && error.needs_password() {
                    "PDF changed but needs a password. Retry reload to unlock the new version."
                        .into()
                } else {
                    error.to_string()
                })
            }
            Failure::Render(message) => Notice {
                message,
                recovery: Some(Recovery::Page),
            },
            Failure::Session => Notice::retry_document(
                "The PDF engine stopped processing this document. Reload it to continue.",
            ),
        });
    }

    pub(super) fn retry_error(&mut self, context: &egui::Context) {
        if self.loading {
            return;
        }
        match self.error.as_ref().and_then(|error| error.recovery) {
            Some(Recovery::Document) => self.reload(context),
            Some(Recovery::Page) => {
                self.requested = None;
                self.error = None;
            }
            None => {}
        }
    }

    pub(super) fn error_panels(&mut self, context: &egui::Context) {
        self.input_buttons.retry_error = None;
        self.input_buttons.dismiss_error = None;
        self.input_buttons.copy_error = None;
        self.input_buttons.dismiss_config = None;
        self.input_buttons.copy_config = None;
        let notices =
            usize::from(self.configuration_error.is_some()) + usize::from(self.error.is_some());
        if notices == 0 {
            return;
        }
        // Keep actions outside the scrolling message, including when both
        // notices share a minimum-size reader window.
        let budget = (context.available_rect().height() - 8.0) / notices as f32;
        if let Some(error) = &self.configuration_error {
            let mut dismissed = false;
            NoticePanel::new(context, "configuration-error", error, budget).show(
                context,
                "configuration-error-message",
                |ui| {
                    ui.horizontal_wrapped(|ui| {
                        let dismiss = ui.button("Dismiss");
                        self.input_buttons.dismiss_config = Some(dismiss.id);
                        if dismiss.clicked() {
                            dismissed = true;
                        }
                        let copy = copy_button(ui);
                        self.input_buttons.copy_config = Some(copy.id);
                        if copy.clicked() {
                            copy.request_focus();
                            context.copy_text(error.clone());
                        }
                    });
                },
            );
            if dismissed {
                self.configuration_error = None;
            }
        }
        if let Some(error) = &self.error {
            let mut dismissed = false;
            let mut retried = false;
            NoticePanel::new(context, "error", &error.message, budget).show(
                context,
                "reader-error-message",
                |ui| {
                    ui.horizontal_wrapped(|ui| {
                        let dismiss = ui.button("Dismiss");
                        self.input_buttons.dismiss_error = Some(dismiss.id);
                        if dismiss.clicked() {
                            dismissed = true;
                        }
                        if let Some(recovery) = error.recovery {
                            let label = match recovery {
                                Recovery::Document if self.nav.count == 0 => "Retry open",
                                Recovery::Document => "Retry reload",
                                Recovery::Page => "Retry page",
                            };
                            let retry = ui
                                .add_enabled(
                                    !self.loading,
                                    egui::Button::new(if ui.max_rect().width() < 360.0 {
                                        "Retry"
                                    } else {
                                        label
                                    }),
                                )
                                .on_hover_text_lazy(|| match recovery {
                                    Recovery::Document => {
                                        format!(
                                            "{label} — {}",
                                            self.library.shortcuts.label("reload")
                                        )
                                    }
                                    Recovery::Page => "Render the current view again".into(),
                                });
                            retry.widget_info(|| {
                                egui::WidgetInfo::labeled(
                                    egui::WidgetType::Button,
                                    !self.loading,
                                    label,
                                )
                            });
                            self.input_buttons.retry_error = Some(retry.id);
                            if retry.clicked() {
                                retried = true;
                            }
                        }
                        let copy = copy_button(ui);
                        self.input_buttons.copy_error = Some(copy.id);
                        if copy.clicked() {
                            copy.request_focus();
                            context.copy_text(error.message.clone());
                        }
                    });
                },
            );
            if dismissed {
                self.error = None;
            } else if retried {
                self.retry_error(context);
            }
        }
    }
}

fn copy_button(ui: &mut egui::Ui) -> egui::Response {
    let response = ui.button(if ui.max_rect().width() < 360.0 {
        "Copy"
    } else {
        "Copy error"
    });
    response.widget_info(|| {
        egui::WidgetInfo::labeled(egui::WidgetType::Button, ui.is_enabled(), "Copy error")
    });
    response.on_hover_text("Copy the complete error message")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::navigation::{Fit, Navigation};
    use std::time::{Duration, Instant};

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
            |context| {
                app.keys(context);
                app.toolbar(context);
                app.search_bar(context);
                app.status(context);
                let available = context.available_rect();
                context
                    .data_mut(|data| data.insert_temp(egui::Id::new("test-error-area"), available));
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

    #[test]
    fn recovery_follows_the_failure_source_even_after_a_failed_reload() {
        let directory = tempfile::tempdir().unwrap();
        let (mut app, context) = reader(directory.path());
        app.reloading = true;
        app.worker_error(&context, Failure::Render("page failed".into()));
        assert_eq!(app.error.as_ref().unwrap().recovery, Some(Recovery::Page));
        app.retry_error(&context);
        assert!(app.error.is_none() && !app.loading);
        app.error = Some("SyncTeX target is invalid".into());
        app.retry_error(&context);
        assert_eq!(app.error.as_ref().unwrap().recovery, None);
        assert!(!app.loading);
        app.worker_error(&context, Failure::Session);
        assert_eq!(
            app.error.as_ref().unwrap().recovery,
            Some(Recovery::Document)
        );
        app.loading = true;
        let error = app.error.clone();
        app.worker_error(&context, Failure::Render("obsolete render failure".into()));
        assert_eq!(
            app.error, error,
            "Old snapshot failures cannot replace the loading state"
        );
    }

    #[test]
    fn session_recovery_reloads_the_document_and_resumes_the_worker() {
        let directory = tempfile::tempdir().unwrap();
        let context = egui::Context::default();
        let mut app =
            Pagewing::with_context(&context, None, None, directory.path().join("library.json"));
        app.open(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("assets/pagewing-guide.pdf"),
            &context,
        );
        let wait = |app: &mut Pagewing| {
            let deadline = Instant::now() + Duration::from_secs(5);
            while app.loading {
                app.events(&context);
                assert!(Instant::now() < deadline, "{:?}", app.error);
                std::thread::sleep(Duration::from_millis(5));
            }
        };
        wait(&mut app);
        assert_eq!(app.nav.count, 6);
        app.worker_error(&context, Failure::Session);
        app.retry_error(&context);
        assert!(app.loading && app.reloading);
        wait(&mut app);
        assert!(app.error.is_none() && !app.reloading);
        app.worker.copy(0);
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            if let Ok(crate::worker::Event::Text { text, .. }) =
                app.worker.events.recv_timeout(Duration::from_millis(10))
            {
                assert!(text.contains("A little room to read"));
                break;
            }
            assert!(Instant::now() < deadline);
        }
    }

    #[test]
    fn long_errors_keep_both_panels_actions_visible_when_resizing() {
        let directory = tempfile::tempdir().unwrap();
        for dark in [false, true] {
            for recovery in [None, Some(Recovery::Document), Some(Recovery::Page)] {
                let (mut app, context) = reader(directory.path());
                super::super::appearance::apply(&context, dark);
                app.configuration_error = Some("Cannot load /长路径/configuration/".repeat(100));
                app.error = Some(Notice {
                    message: "PDF failure with a long diagnostic. ".repeat(100),
                    recovery,
                });
                for size in super::super::dialog::TEST_SIZES {
                    for search_open in [false, true] {
                        app.search_open = search_open;
                        for _ in 0..4 {
                            frame(&mut app, &context, size, Vec::new());
                        }
                        let screen = context.data(|data| {
                            data.get_temp::<egui::Rect>(egui::Id::new("test-error-area"))
                                .unwrap()
                        });
                        let actions = [
                            app.input_buttons.dismiss_config,
                            app.input_buttons.copy_config,
                            app.input_buttons.dismiss_error,
                            app.input_buttons.copy_error,
                            app.input_buttons.retry_error,
                        ];
                        let mut rects = Vec::new();
                        for id in actions.into_iter().flatten() {
                            let response = context.read_response(id).unwrap();
                            assert!(
                                screen.contains_rect(response.rect),
                                "{size:?}: {screen:?}, {:?}",
                                response.rect
                            );
                            assert!(
                                rects
                                    .iter()
                                    .all(|rect: &egui::Rect| !rect.intersects(response.rect)),
                                "Actions overlap at {size:?} ({recovery:?}): {rects:?}, {:?}",
                                response.rect
                            );
                            rects.push(response.rect);
                        }
                        assert_eq!(rects.len(), if recovery.is_some() { 5 } else { 4 });
                    }
                }
            }
        }
    }

    #[test]
    fn configured_or_disabled_reload_keys_keep_the_retry_button_usable() {
        let directory = tempfile::tempdir().unwrap();
        for binding in ["['Cmd+Alt+R']", "[]"] {
            std::fs::write(
                directory.path().join("config.toml"),
                format!("[shortcuts]\nreload = {binding}\n"),
            )
            .unwrap();
            let (mut app, context) = reader(directory.path());
            assert!(app.configuration_error.is_none());
            app.reading.path =
                std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("assets/pagewing-guide.pdf");
            app.error = Some(Notice::retry_document("Could not load the changed PDF"));
            let size = egui::vec2(240.0, 240.0);
            context.style_mut(|style| style.interaction.tooltip_delay = 0.0);
            for _ in 0..4 {
                frame(&mut app, &context, size, Vec::new());
            }
            let id = app.input_buttons.retry_error.unwrap();
            let position = context.read_response(id).unwrap().rect.center();
            let mut tooltip = String::new();
            for _ in 0..12 {
                let output = frame(
                    &mut app,
                    &context,
                    size,
                    vec![egui::Event::PointerMoved(position)],
                );
                for shape in output.shapes {
                    if let egui::Shape::Text(text) = shape.shape {
                        tooltip.push_str(text.galley.text());
                    }
                }
            }
            assert!(
                tooltip.contains(if binding == "[]" {
                    "Unassigned"
                } else {
                    "Ctrl/Cmd+Alt+R"
                }),
                "{tooltip}"
            );
            let key = |key, modifiers| {
                vec![egui::Event::Key {
                    key,
                    modifiers,
                    physical_key: None,
                    pressed: true,
                    repeat: false,
                }]
            };
            frame(
                &mut app,
                &context,
                size,
                key(egui::Key::R, egui::Modifiers::COMMAND),
            );
            assert!(!app.loading, "The replaced default must stay disabled");
            if binding == "[]" {
                context.memory_mut(|memory| memory.request_focus(id));
                frame(
                    &mut app,
                    &context,
                    size,
                    key(egui::Key::Enter, egui::Modifiers::NONE),
                );
            } else {
                frame(
                    &mut app,
                    &context,
                    size,
                    key(
                        egui::Key::R,
                        egui::Modifiers {
                            alt: true,
                            ..egui::Modifiers::COMMAND
                        },
                    ),
                );
            }
            assert!(app.loading && app.reloading);
        }
    }

    #[test]
    fn clicking_copy_gives_the_button_focus_for_following_keyboard_activation() {
        let directory = tempfile::tempdir().unwrap();
        let (mut app, context) = reader(directory.path());
        app.reading.fit = Fit::Custom(2.0);
        app.error = Some("complete error message".into());
        app.configuration_error = Some("configuration diagnostic".into());
        let size = egui::vec2(240.0, 240.0);
        for _ in 0..4 {
            frame(&mut app, &context, size, Vec::new());
        }
        for (id, message) in [
            (
                app.input_buttons.copy_error.unwrap(),
                "complete error message",
            ),
            (
                app.input_buttons.copy_config.unwrap(),
                "configuration diagnostic",
            ),
        ] {
            let position = context.read_response(id).unwrap().rect.center();
            let mut events = vec![egui::Event::PointerMoved(position)];
            events.extend([true, false].map(|pressed| egui::Event::PointerButton {
                pos: position,
                button: egui::PointerButton::Primary,
                pressed,
                modifiers: egui::Modifiers::NONE,
            }));
            frame(&mut app, &context, size, events);
            assert!(context.memory(|memory| memory.has_focus(id)));
            let output = frame(
                &mut app,
                &context,
                size,
                vec![egui::Event::Key {
                    key: egui::Key::Space,
                    physical_key: None,
                    pressed: true,
                    repeat: false,
                    modifiers: egui::Modifiers::NONE,
                }],
            );
            assert!(output.platform_output.commands.iter().any(
                |command| matches!(command, egui::OutputCommand::CopyText(text) if text == message)
            ));
            frame(
                &mut app,
                &context,
                size,
                vec![egui::Event::Key {
                    key: egui::Key::Space,
                    physical_key: None,
                    pressed: false,
                    repeat: false,
                    modifiers: egui::Modifiers::NONE,
                }],
            );
            assert_eq!(app.nav.page, 2);
            assert_eq!(app.pan, egui::Vec2::ZERO);
        }
    }

    #[test]
    fn keyboard_copy_and_dismiss_do_not_trigger_reader_space_commands() {
        let directory = tempfile::tempdir().unwrap();
        for key in [egui::Key::Space, egui::Key::Enter] {
            let (mut app, context) = reader(directory.path());
            app.reading.fit = Fit::Custom(2.0);
            let message = "Complete diagnostic — 长路径 📝\n".repeat(200);
            app.error = Some(message.clone().into());
            let size = egui::vec2(240.0, 240.0);
            for _ in 0..4 {
                frame(&mut app, &context, size, Vec::new());
            }
            let mut copied = false;
            for (id, copy) in [
                (app.input_buttons.copy_error.unwrap(), true),
                (app.input_buttons.dismiss_error.unwrap(), false),
            ] {
                context.memory_mut(|memory| memory.request_focus(id));
                for pressed in [true, false] {
                    let output = frame(
                        &mut app,
                        &context,
                        size,
                        vec![egui::Event::Key {
                            key,
                            physical_key: None,
                            pressed,
                            repeat: false,
                            modifiers: egui::Modifiers::NONE,
                        }],
                    );
                    copied |= output.platform_output.commands.iter().any(|command| matches!(command, egui::OutputCommand::CopyText(text) if text == &message));
                }
                frame(&mut app, &context, size, Vec::new());
                assert_eq!(app.error.is_some(), copy);
                assert_eq!(app.nav.page, 2);
                assert_eq!(app.pan, egui::Vec2::ZERO);
            }
            assert!(
                copied,
                "Copy must include the full diagnostic, including scrolled-out text"
            );
        }
    }
}
