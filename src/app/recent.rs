use super::{Pagewing, dialog};
use eframe::egui;

impl Pagewing {
    pub(super) fn toggle_recent(&mut self, context: &egui::Context) {
        self.outline.cancel_input();
        self.close_zoom(context);
        if self.recent_picker.take().is_some() {
            context.memory_mut(|memory| {
                memory.surrender_focus(egui::Id::new("pagewing-recent-input"))
            });
            return;
        }
        self.nav.clear_prefix();
        self.hints.cancel();
        self.selection.cancel();
        self.mark_action = None;
        self.help = false;
        self.properties_open = false;
        self.page_input = None;
        self.password_input = None;
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
        let size = context.content_rect().size();
        let compact = size.x < 480.0 || size.y < 420.0;
        dialog::window(context, "Recent documents", "Recent", 580.0)
            .open(&mut open)
            .show(context, |ui| {
                let (input, _) = super::shortcuts::text_input(
                    ui,
                    egui::Id::new("pagewing-recent-input"),
                    &mut picker.focus,
                    &mut self.deferred_input,
                    egui::TextEdit::singleline(&mut picker.query)
                        .desired_width(f32::INFINITY)
                        .hint_text("Filter by filename or path…"),
                );
                if input.changed() {
                    picker.filter();
                }
                if !ui.is_sizing_pass() {
                    picker.apply_motions();
                }
                if !ui.is_sizing_pass() && std::mem::take(&mut picker.confirm) {
                    chosen = picker.selected_path().map(ToOwned::to_owned);
                }
                if compact {
                    ui.weak(format!("{} documents", picker.len()));
                } else {
                    ui.weak(format!(
                        "{} documents · ↑/↓ or Ctrl+J/K · Enter to open",
                        picker.len()
                    ));
                }
                let reveal = std::mem::take(&mut picker.reveal);
                egui::ScrollArea::vertical()
                    .id_salt("pagewing-recent-results")
                    .max_height((size.y - if compact { 180.0 } else { 230.0 }).clamp(32.0, 280.0))
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
                            if response.double_clicked()
                                || (response.clicked()
                                    && !response.clicked_by(egui::PointerButton::Primary))
                            {
                                chosen = picker.selected_path().map(ToOwned::to_owned);
                            }
                        }
                    });
                ui.separator();
                ui.horizontal(|ui| {
                    if ui
                        .add_enabled(picker.selected_path().is_some(), egui::Button::new("Open"))
                        .on_hover_text("Enter")
                        .clicked()
                    {
                        chosen = picker.selected_path().map(ToOwned::to_owned);
                    }
                    if ui.button("Cancel").on_hover_text("Escape").clicked() {
                        cancelled = true;
                    }
                    if !compact {
                        ui.weak("Ctrl/Cmd+P · Escape to close");
                    }
                });
            });
        if let Some(path) = chosen {
            self.open(path, context);
        } else if open && !cancelled {
            self.recent_picker = Some(picker);
        } else {
            context.memory_mut(|memory| {
                memory.surrender_focus(egui::Id::new("pagewing-recent-input"))
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::recent::RecentPicker;

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

    fn frame(app: &mut Pagewing, context: &egui::Context, mut events: Vec<egui::Event>) {
        let mut deferred = std::mem::take(&mut app.deferred_input);
        deferred.append(&mut events);
        let _ = context.run(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(1180.0, 820.0),
                )),
                events: deferred,
                ..Default::default()
            },
            |context| {
                app.keys(context);
                app.recent_dialog(context);
                egui::CentralPanel::default().show(context, |_| {});
            },
        );
    }

    #[test]
    fn tab_and_enter_activate_recent_rows_open_and_cancel_without_confirming_the_filter() {
        let directory = tempfile::tempdir().unwrap();
        let context = egui::Context::default();
        let mut app =
            Pagewing::with_context(&context, None, None, directory.path().join("library.json"));
        let path = directory.path().join("selected.pdf");
        for tabs in [3, 2, 1] {
            app.clear_document(&context);
            app.recent_picker = Some(RecentPicker::new(std::iter::once(path.clone())));
            for _ in 0..4 {
                frame(&mut app, &context, Vec::new());
            }
            for _ in 0..tabs {
                frame(&mut app, &context, key(egui::Key::Tab));
            }
            frame(&mut app, &context, key(egui::Key::Enter));
            assert!(app.recent_picker.is_none());
            if tabs == 3 {
                assert!(!app.loading, "Enter on Cancel must not load a document");
                assert!(app.reading.path.as_os_str().is_empty());
            } else {
                assert!(
                    app.loading,
                    "Enter on a row or Open must load the selected document"
                );
                assert_eq!(app.reading.path, path);
            }
        }
        app.clear_document(&context);
        let second = directory.path().join("second.pdf");
        app.recent_picker = Some(RecentPicker::new([path, second.clone()].into_iter()));
        for _ in 0..4 {
            frame(&mut app, &context, Vec::new());
        }
        for _ in 0..2 {
            frame(&mut app, &context, key(egui::Key::Tab));
        }
        frame(&mut app, &context, key(egui::Key::Enter));
        assert_eq!(
            app.reading.path, second,
            "activate the focused result, not the first one"
        );
    }

    #[test]
    fn recent_dialog_and_actions_fit_when_resizing() {
        let directory = tempfile::tempdir().unwrap();
        let context = egui::Context::default();
        let mut app =
            Pagewing::with_context(&context, None, None, directory.path().join("library.json"));
        for size in dialog::TEST_SIZES {
            for count in [0, 1, 24] {
                app.recent_picker = Some(RecentPicker::new((0..count).map(|index| {
                    directory
                        .path()
                        .join(format!("{index}-a-very-long-filename-without-spaces.pdf"))
                })));
                let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, size);
                let mut output = egui::FullOutput::default();
                for _ in 0..4 {
                    output = context.run(
                        egui::RawInput {
                            screen_rect: Some(screen),
                            ..Default::default()
                        },
                        |context| app.recent_dialog(context),
                    );
                }
                dialog::assert_visible_actions(
                    &context,
                    &output,
                    screen,
                    "Recent documents",
                    &["Open", "Cancel"],
                );
            }
        }
    }
}
