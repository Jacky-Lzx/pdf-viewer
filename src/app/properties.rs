use super::{Pagewing, dialog};
use eframe::egui;
use std::fmt::Write;

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
        self.goto_open = false;
        self.password_open = false;
        self.password_focus = false;
        self.password.clear();
        self.recent_picker = None;
        self.search_focus = false;
        self.pending_search_jump = None;
        self.properties_open = true;
        context.memory_mut(|memory| {
            if let Some(id) = memory.focused() {
                memory.surrender_focus(id);
            }
            memory.move_focus(egui::FocusDirection::None);
        });
    }

    fn property_rows(&self) -> Vec<(&str, String)> {
        let dimensions = self.dimensions[self.nav.page];
        let mut rows = vec![
            ("File", self.reading.path.display().to_string()),
            ("Format", format!("PDF {}", self.info.version)),
            ("Size", format!("{} bytes", self.info.bytes)),
            ("Pages", self.nav.count.to_string()),
            ("Current page", self.page_name(self.nav.page)),
            (
                "Page size",
                format!("{:.1} × {:.1} pt", dimensions[0], dimensions[1]),
            ),
        ];
        rows.extend(
            self.info
                .fields
                .iter()
                .map(|(label, value)| (*label, value.clone())),
        );
        rows
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
                                ui.weak(label);
                                ui.add(egui::Label::new(value).selectable(true).wrap());
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
                                            ui.weak(label);
                                        },
                                    );
                                    ui.allocate_ui_with_layout(
                                        egui::vec2(ui.available_width(), 0.0),
                                        egui::Layout::top_down(egui::Align::Min),
                                        |ui| {
                                            ui.add(egui::Label::new(value).selectable(true).wrap());
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
