use super::Pagewing;
use eframe::egui::{
    self,
    text::{CCursor, CCursorRange},
};

impl Pagewing {
    /// Run after TextEdit so selecting a saved query cannot use stale results.
    pub(super) fn search_history_menu(&mut self, ui: &mut egui::Ui) -> bool {
        let mut chosen = None;
        let mut clear = false;
        let state_id = ui.make_persistent_id("search-history-open");
        let was_open = ui.data(|data| data.get_temp::<bool>(state_id).unwrap_or(false));
        let keyboard_open = !was_open
            && ui.input(|input| {
                input.key_pressed(egui::Key::Enter) || input.key_pressed(egui::Key::Space)
            });
        let mut open = false;
        let response = ui
            .add_enabled(!self.query_history.is_empty(), egui::Button::new("History"))
            .on_hover_text(
                "Reuse a recent search in this window; keeps the current Aa and Word options",
            )
            .on_disabled_hover_text(
                "Confirm a search with Enter to add it to this window's history.",
            );
        // keys() toggles this popup before routing the rest of the input batch.
        // Drawing must not toggle it a second time or reopen it after Escape.
        egui::Popup::menu(&response).open_memory(None).show(|ui| {
            open = true;
            let size = ui.ctx().content_rect().size();
            ui.set_max_width((size.x - 40.0).clamp(100.0, 360.0));
            egui::ScrollArea::vertical()
                .id_salt("search-history-queries")
                .max_height((size.y - 110.0).clamp(32.0, 320.0))
                .show(ui, |ui| {
                    for (index, query) in self.query_history.queries().enumerate() {
                        let response = ui
                            .push_id(query, |ui| {
                                ui.add(egui::Button::new(query).truncate())
                                    .on_hover_text(query)
                            })
                            .inner;
                        if index == 0 && keyboard_open {
                            response.request_focus();
                            ui.memory_mut(|memory| memory.move_focus(egui::FocusDirection::None));
                        }
                        if response.clicked() {
                            chosen = Some(query.to_owned());
                            ui.close();
                        }
                    }
                    crate::app::shortcuts::reveal_focused_control(ui, ui.clip_rect().size());
                });
            ui.separator();
            if ui.button("Clear history").clicked() {
                clear = true;
                ui.close();
            }
        });
        self.input_buttons.search_history = Some(response.id);
        ui.data_mut(|data| data.insert_temp(state_id, open));
        if let Some(query) = chosen {
            self.query = query;
            self.query_history.remember(&self.query);
            self.change_search();
            self.search_focus = true;
            let id = egui::Id::new("pagewing-search-input");
            let mut state = egui::TextEdit::load_state(ui.ctx(), id).unwrap_or_default();
            state.cursor.set_char_range(Some(CCursorRange::two(
                CCursor::new(0),
                CCursor::new(self.query.chars().count()),
            )));
            state.store(ui.ctx(), id);
            true
        } else if clear {
            self.query_history.clear();
            self.search_focus = true;
            true
        } else {
            false
        }
    }
}

#[cfg(test)]
mod tests;
