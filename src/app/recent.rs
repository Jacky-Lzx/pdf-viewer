use super::Folio;
use eframe::egui::{self, Vec2};

impl Folio {
    pub(super) fn toggle_recent(&mut self, context: &egui::Context) {
        self.outline.cancel_input();
        if self.recent_picker.take().is_some() {
            context
                .memory_mut(|memory| memory.surrender_focus(egui::Id::new("folio-recent-input")));
            return;
        }
        self.nav.clear_prefix();
        self.hints.cancel();
        self.selection.cancel();
        self.mark_action = None;
        self.help = false;
        self.goto_open = false;
        self.password_open = false;
        self.search_open = false;
        self.pending_search_jump = None;
        self.recent_picker = Some(crate::recent::RecentPicker::new(
            self.library.recent.iter().map(|book| book.path.clone()),
        ));
    }

    pub(super) fn recent_dialog(&mut self, context: &egui::Context) {
        let Some(mut picker) = self.recent_picker.take() else {
            return;
        };
        let mut open = true;
        let mut cancelled = false;
        let mut chosen = None;
        egui::Window::new("Recent documents")
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .default_width(580.0)
            .anchor(egui::Align2::CENTER_CENTER, Vec2::ZERO)
            .show(context, |ui| {
                super::shortcuts::focus_text_input(
                    ui,
                    egui::Id::new("folio-recent-input"),
                    &mut picker.focus,
                    &mut self.deferred_input,
                );
                let input = ui.add(
                    egui::TextEdit::singleline(&mut picker.query)
                        .id(egui::Id::new("folio-recent-input"))
                        .desired_width(f32::INFINITY)
                        .hint_text("Filter by filename or path…"),
                );
                if input.changed() {
                    picker.filter();
                }
                if !ui.is_sizing_pass() && picker.motion != 0 {
                    let motion = std::mem::take(&mut picker.motion);
                    picker.step(motion);
                }
                if !ui.is_sizing_pass() && std::mem::take(&mut picker.confirm) {
                    chosen = picker.selected_path().map(ToOwned::to_owned);
                }
                ui.weak(format!(
                    "{} documents · ↑/↓ or Ctrl+J/K · Enter to open",
                    picker.len()
                ));
                let reveal = std::mem::take(&mut picker.reveal);
                egui::ScrollArea::vertical()
                    .id_salt("folio-recent-results")
                    .max_height((context.content_rect().height() - 250.0).clamp(80.0, 280.0))
                    .show(ui, |ui| {
                        if picker.len() == 0 {
                            ui.label(if self.library.recent.is_empty() {
                                "No recent documents yet. Open a PDF to get started."
                            } else {
                                "No matching documents."
                            });
                        }
                        for row in 0..picker.len() {
                            let response = ui
                                .add_sized(
                                    [ui.available_width(), 32.0],
                                    egui::Button::selectable(
                                        picker.selected == row,
                                        picker.label(row),
                                    )
                                    .truncate(),
                                )
                                .on_hover_text(picker.display(row));
                            if reveal && picker.selected == row {
                                response.scroll_to_me(Some(egui::Align::Center));
                            }
                            if response.clicked() {
                                picker.selected = row;
                                input.request_focus();
                            }
                            if response.double_clicked() {
                                chosen = picker.selected_path().map(ToOwned::to_owned);
                            }
                        }
                    });
                ui.horizontal(|ui| {
                    if ui
                        .add_enabled(picker.selected_path().is_some(), egui::Button::new("Open"))
                        .clicked()
                    {
                        chosen = picker.selected_path().map(ToOwned::to_owned);
                    }
                    if ui.button("Cancel").clicked() {
                        cancelled = true;
                    }
                    ui.weak("Ctrl/Cmd+P · Escape to close");
                });
            });
        if let Some(path) = chosen {
            self.open(path, context);
        } else if open && !cancelled {
            self.recent_picker = Some(picker);
        } else {
            context
                .memory_mut(|memory| memory.surrender_focus(egui::Id::new("folio-recent-input")));
        }
    }
}
