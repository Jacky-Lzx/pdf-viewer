use super::{Pagewing, dialog};
use eframe::egui;
use std::borrow::Cow;
use std::fmt::Write;

/// Metadata selection follows its field across the stacked and column layouts.
/// Document IDs keep a selection from being reused after opening/reloading.
fn property_text(ui: &mut egui::Ui, id: impl std::hash::Hash, text: impl Into<egui::WidgetText>) {
    ui.scope_builder(egui::UiBuilder::new().id(id), |ui| {
        ui.add(egui::Label::new(text).selectable(true).wrap());
    });
}

impl Pagewing {
    pub(super) fn show_properties(&mut self, context: &egui::Context) {
        if self.nav.count == 0 {
            return;
        }
        self.outline.cancel_input();
        self.close_zoom(context);
        self.nav.clear_prefix();
        self.hints.cancel();
        self.selection.cancel();
        self.mark_action = None;
        self.help = false;
        self.page_input = None;
        self.password_input = None;
        self.recent_picker = None;
        self.search_focus = false;
        self.search_navigation.clear();
        self.properties_open = true;
        context.memory_mut(|memory| {
            if let Some(id) = memory.focused() {
                memory.surrender_focus(id);
            }
            memory.move_focus(egui::FocusDirection::None);
        });
    }

    fn property_rows(&self) -> impl Iterator<Item = (&str, Cow<'_, str>)> {
        let dimensions = self.dimensions[self.nav.page];
        [
            ("File", Cow::Owned(self.reading.path.display().to_string())),
            ("Format", Cow::Owned(format!("PDF {}", self.info.version))),
            ("Size", Cow::Owned(format!("{} bytes", self.info.bytes))),
            ("Pages", Cow::Owned(self.nav.count.to_string())),
            ("Current page", Cow::Owned(self.page_name(self.nav.page))),
            (
                "Page size",
                Cow::Owned(format!("{:.1} × {:.1} pt", dimensions[0], dimensions[1])),
            ),
        ]
        .into_iter()
        .chain(
            self.info
                .fields
                .iter()
                .map(|(label, value)| (*label, Cow::Borrowed(value.as_str()))),
        )
    }

    pub(super) fn copy_properties(&mut self, context: &egui::Context) {
        let mut text = String::new();
        for (label, value) in self.property_rows() {
            let _ = writeln!(text, "{label}: {value}");
        }
        context.copy_text(text);
        self.notify("Copied document information");
    }

    pub(super) fn properties_window(&mut self, context: &egui::Context) {
        if !self.properties_open || self.nav.count == 0 {
            return;
        }
        let mut open = true;
        let mut close = false;
        let size = context.content_rect().size();
        let compact = size.x < 480.0 || size.y < 420.0;
        dialog::window(context, "Document information", "PDF details", 520.0)
            .open(&mut open)
            .show(context, |ui| {
                let stacked = ui.available_width() < 420.0;
                egui::ScrollArea::vertical()
                    .id_salt("pagewing-document-info-fields")
                    .max_height((size.y - if compact { 140.0 } else { 180.0 }).max(40.0))
                    .show(ui, |ui| {
                        if stacked {
                            for (label, value) in self.property_rows() {
                                property_text(
                                    ui,
                                    ("property-name", self.id, label),
                                    egui::RichText::new(label).weak(),
                                );
                                property_text(ui, ("property-value", self.id, label), value);
                                ui.add_space(8.0);
                            }
                        } else {
                            for (label, value) in self.property_rows() {
                                ui.horizontal_top(|ui| {
                                    ui.spacing_mut().item_spacing.x = 16.0;
                                    ui.allocate_ui_with_layout(
                                        egui::vec2(84.0, 0.0),
                                        egui::Layout::top_down(egui::Align::Min),
                                        |ui| {
                                            ui.set_min_width(84.0);
                                            property_text(
                                                ui,
                                                ("property-name", self.id, label),
                                                egui::RichText::new(label).weak(),
                                            );
                                        },
                                    );
                                    ui.allocate_ui_with_layout(
                                        egui::vec2(ui.available_width(), 0.0),
                                        egui::Layout::top_down(egui::Align::Min),
                                        |ui| {
                                            property_text(
                                                ui,
                                                ("property-value", self.id, label),
                                                value,
                                            );
                                        },
                                    );
                                });
                                ui.add_space(2.0);
                            }
                        }
                    });
                ui.separator();
                ui.horizontal(|ui| {
                    if ui.button("Copy details").on_hover_text("y").clicked() {
                        self.copy_properties(context);
                    }
                    close = ui.button("Close").on_hover_text("Escape").clicked();
                });
            });
        self.properties_open = open && !close;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::navigation::Navigation;

    fn frame(
        app: &mut Pagewing,
        context: &egui::Context,
        size: egui::Vec2,
        events: Vec<egui::Event>,
    ) -> egui::FullOutput {
        context.run(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, size)),
                time: Some(context.input(|input| input.time) + 0.05),
                events,
                ..Default::default()
            },
            |context| {
                app.keys(context);
                app.properties_window(context);
                egui::CentralPanel::default().show(context, |_| {});
            },
        )
    }

    fn settle(app: &mut Pagewing, context: &egui::Context, size: egui::Vec2) -> egui::FullOutput {
        let mut output = egui::FullOutput::default();
        for _ in 0..4 {
            output = frame(app, context, size, Vec::new());
        }
        output
    }

    fn copied(output: &egui::FullOutput) -> Option<&str> {
        output
            .platform_output
            .commands
            .iter()
            .find_map(|command| match command {
                egui::OutputCommand::CopyText(text) => Some(text.as_str()),
                _ => None,
            })
    }

    fn reader() -> (tempfile::TempDir, egui::Context, Pagewing) {
        let directory = tempfile::tempdir().unwrap();
        let context = egui::Context::default();
        let mut app =
            Pagewing::with_context(&context, None, None, directory.path().join("library.json"));
        app.nav = Navigation::new(6, 2);
        app.dimensions = vec![[600.0, 800.0]; 6];
        app.reading.path = "document.pdf".into();
        app.show_properties(&context);
        (directory, context, app)
    }

    fn select_word(app: &mut Pagewing, context: &egui::Context, size: egui::Vec2, value: &str) {
        let output = settle(app, context, size);
        let position = output
            .shapes
            .iter()
            .find_map(|shape| match &shape.shape {
                egui::Shape::Text(text) if text.galley.text() == value => {
                    Some(text.pos + egui::vec2(20.0, 7.0))
                }
                _ => None,
            })
            .unwrap();
        for _ in 0..2 {
            for pressed in [true, false] {
                frame(
                    app,
                    context,
                    size,
                    vec![
                        egui::Event::PointerMoved(position),
                        egui::Event::PointerButton {
                            pos: position,
                            button: egui::PointerButton::Primary,
                            pressed,
                            modifiers: egui::Modifiers::NONE,
                        },
                    ],
                );
            }
        }
    }

    #[test]
    fn selected_metadata_keeps_its_text_when_fields_reflow_or_reorder() {
        for (value, word) in [
            ("MetadataNeedle selected metadata", "MetadataNeedle"),
            ("Straße 中文 metadata", "Straße"),
            ("中文 metadata", "中文"),
        ] {
            let (_directory, context, mut app) = reader();
            app.info.fields = vec![("Title", value.into())];
            let wide = egui::vec2(1180.0, 820.0);
            select_word(&mut app, &context, wide, value);
            let output = frame(&mut app, &context, wide, vec![egui::Event::Copy]);
            assert_eq!(copied(&output), Some(word));
            // Optional metadata before a selected field must not steal it.
            app.info
                .fields
                .insert(0, ("Creator", "Different metadata".into()));
            for size in [egui::vec2(360.0, 820.0), wide, egui::vec2(480.0, 820.0)] {
                settle(&mut app, &context, size);
                let output = frame(&mut app, &context, size, vec![egui::Event::Copy]);
                assert_eq!(copied(&output), Some(word), "{value}, {size:?}");
            }
            assert!(app.properties_open);
            assert_eq!(app.nav.page, 2);
        }
    }

    #[test]
    fn document_replacement_does_not_reuse_metadata_selection() {
        let (_directory, context, mut app) = reader();
        let wide = egui::vec2(1180.0, 820.0);
        let value = "MetadataNeedle selected metadata";
        app.info.fields = vec![("Title", value.into())];
        select_word(&mut app, &context, wide, value);
        let output = frame(&mut app, &context, wide, vec![egui::Event::Copy]);
        assert_eq!(copied(&output), Some("MetadataNeedle"));
        // Opening/reloading increments the document ID, even if the new field
        // occupies the old position and has the same label.
        app.id += 1;
        app.info.fields = vec![("Title", "Replacement title must not be selected".into())];
        settle(&mut app, &context, wide);
        let output = frame(&mut app, &context, wide, vec![egui::Event::Copy]);
        assert!(copied(&output).is_none());
    }

    #[test]
    fn long_document_information_and_actions_fit_when_resizing() {
        let directory = tempfile::tempdir().unwrap();
        let context = egui::Context::default();
        let mut app =
            Pagewing::with_context(&context, None, None, directory.path().join("library.json"));
        app.nav = Navigation::new(6, 2);
        app.dimensions = vec![[600.0, 800.0]; 6];
        app.reading.path = directory
            .path()
            .join("a-long-filename-without-spaces-".repeat(6));
        app.info.fields = vec![("Title", "中文 😀 very long title ".repeat(100))];
        for size in dialog::TEST_SIZES {
            app.show_properties(&context);
            let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, size);
            let mut output = egui::FullOutput::default();
            for _ in 0..4 {
                output = context.run(
                    egui::RawInput {
                        screen_rect: Some(screen),
                        ..Default::default()
                    },
                    |context| app.properties_window(context),
                );
            }
            dialog::assert_visible_actions(
                &context,
                &output,
                screen,
                "Document information",
                &["Copy details", "Close"],
            );
            if size.x > 1000.0 && size.y > 500.0 {
                let text_y = |value: &str| {
                    output
                        .shapes
                        .iter()
                        .find_map(|shape| match &shape.shape {
                            egui::Shape::Text(text) if text.galley.text() == value => {
                                Some(text.pos.y)
                            }
                            _ => None,
                        })
                        .unwrap()
                };
                assert!(
                    text_y("Title") <= text_y(&app.info.fields[0].1) + 1.0,
                    "the label must align with the start of a long field"
                );
            }
        }
    }
}
