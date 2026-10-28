use super::{Pagewing, appearance};
use eframe::egui;
use std::time::Instant;

#[derive(Clone, Copy)]
pub(in crate::app) enum RunAction {
    Stop,
    Restart,
}

impl Pagewing {
    pub(super) fn apply_search_run(&mut self, action: RunAction) {
        if self.loading || self.query.trim().is_empty() {
            return;
        }
        match action {
            RunAction::Stop if !self.search_complete => {
                // Retire queued/in-flight batches without touching accepted hits,
                // their selection, highlights or the reading/history position.
                self.revision += 1;
                self.search_due = None;
                self.worker
                    .search(self.revision, String::new(), self.library.search_options);
                self.search_complete = true;
                self.search_limited = false;
                self.search_stopped = true;
                self.search_navigation.clear();
                self.query_history.remember(&self.query);
            }
            RunAction::Restart if self.search_stopped => {
                self.change_search();
                self.search_due = Some(Instant::now());
            }
            // A Stop rendered before the last batch must not become a Restart.
            _ => {}
        }
    }

    pub(super) fn search_run_control(&mut self, ui: &mut egui::Ui) {
        if self.query.trim().is_empty() || (self.search_complete && !self.search_stopped) {
            return;
        }
        let restart = self.search_stopped;
        let enabled =
            !self.loading && !self.query.trim().is_empty() && (restart || !self.search_complete);
        let response = ui
            .add_enabled_ui(enabled, |ui| {
                appearance::action_button(
                    ui,
                    "search-run",
                    egui::Button::new(if restart { "Restart" } else { "Stop" }),
                )
            })
            .inner
            .on_hover_text(if restart {
                "Search again from the beginning; replaces the stopped results"
            } else {
                "Stop scanning and keep the available results and reading position"
            });
        if restart {
            self.input_buttons.search_restart = Some(response.id);
        } else {
            self.input_buttons.search_stop = Some(response.id);
        }
    }
}

#[cfg(test)]
mod tests;
