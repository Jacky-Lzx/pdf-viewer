use super::Pagewing;
use eframe::egui::{self, Vec2};
use std::fmt::Write;

impl Pagewing {
    pub(super) fn show_properties(&mut self, context: &egui::Context) {
        if self.nav.count == 0 {
            return;
        }
        self.outline.cancel_input();
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
        egui::Window::new("Document information")
            .open(&mut open)
            .collapsible(false)
            .default_width(520.0)
            .max_width((context.content_rect().width() - 80.0).max(280.0))
            .anchor(egui::Align2::CENTER_CENTER, Vec2::ZERO)
            .show(context, |ui| {
                egui::ScrollArea::vertical()
                    .max_height((context.content_rect().height() - 180.0).max(160.0))
                    .show(ui, |ui| {
                        egui::Grid::new("document-info-fields")
                            .num_columns(2)
                            .max_col_width(
                                (context.content_rect().width() - 210.0).clamp(150.0, 420.0),
                            )
                            .spacing([16.0, 10.0])
                            .show(ui, |ui| {
                                for (label, value) in self.property_rows() {
                                    ui.weak(label);
                                    ui.add(egui::Label::new(value).selectable(true).wrap());
                                    ui.end_row();
                                }
                            });
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
