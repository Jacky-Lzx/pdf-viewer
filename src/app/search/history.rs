use super::{Pagewing, appearance};
use eframe::egui::{
    self,
    text::{CCursor, CCursorRange},
};

impl Pagewing {
    /// Run after TextEdit so selecting a saved query cannot use stale results.
    pub(super) fn search_history_menu(&mut self, ui: &mut egui::Ui) -> bool {
        let mut chosen = None;
        let mut clear = false;
        let response = ui
            .add_enabled_ui(!self.query_history.is_empty(), |ui| {
                appearance::action_button(ui, "search-history", egui::Button::new("History"))
            })
            .inner
            .on_hover_text(
                "Reuse a recent search in this window; keeps the current Aa and Word options",
            )
            .on_disabled_hover_text(
                "Confirm a search with Enter to add it to this window's history.",
            );
        crate::app::shortcuts::show_menu(ui, &response, |ui, keyboard_open| {
            let mut focus = crate::app::menu_focus::MenuFocus::new(ui, keyboard_open);
            let size = ui.ctx().content_rect().size();
            ui.set_max_width((size.x - 40.0).clamp(100.0, 360.0));
            egui::ScrollArea::vertical()
                .id_salt("search-history-queries")
                .max_height((size.y - 110.0).clamp(32.0, 320.0))
                .show(ui, |ui| {
                    for query in self.query_history.queries() {
                        let response = ui
                            .push_id(query, |ui| {
                                ui.add(egui::Button::new(query).truncate())
                                    .on_hover_text(query)
                            })
                            .inner;
                        focus.observe(&response);
                        if response.clicked() {
                            chosen = Some(query.to_owned());
                            ui.close();
                        }
                    }
                    crate::app::shortcuts::reveal_focused_control(ui, ui.clip_rect().size());
                });
            ui.separator();
            let response = ui.button("Clear history");
            focus.observe(&response);
            if response.clicked() {
                clear = true;
                ui.close();
            }
            focus.finish(ui);
        });
        self.input_buttons.search_history = Some(response.id);
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
