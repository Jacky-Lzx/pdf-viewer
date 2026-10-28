use super::{Pagewing, appearance, tooltips::LazyTooltip};
use crate::state::ReadingMark;
use eframe::egui;

impl Pagewing {
    fn history_controls_ready(&self, context: &egui::Context) -> bool {
        self.nav.count > 0
            && !self.loading
            && !self.help
            && !self.properties_open
            && self.page_input.is_none()
            && self.zoom_input.is_none()
            && self.password_input.is_none()
            && self.recent_picker.is_none()
            && !egui::Popup::is_any_open(context)
    }

    pub(super) fn history_controls(&mut self, ui: &mut egui::Ui) {
        // Drawing button availability must not resolve pending geometry before
        // the canvas establishes this frame's viewport, especially on resize.
        let current = ReadingMark {
            page: self.nav.page,
            fit: self.reading.fit,
            rotation: self.reading.rotation,
            continuous: self.reading.continuous,
            facing: self.reading.facing,
            offset: self.pending_offset.unwrap_or_else(|| self.current_offset()),
        };
        let ready = self.history_controls_ready(ui.ctx());
        for (forward, icon, label, binding) in [
            (
                false,
                appearance::Icon::HistoryBack,
                "Back to previous view",
                "history_back",
            ),
            (
                true,
                appearance::Icon::HistoryForward,
                "Forward to next view",
                "history_forward",
            ),
        ] {
            let enabled = ready && self.history.can_go(forward, &current, self.nav.count);
            let response = ui
                .add_enabled_ui(enabled, |ui| {
                    appearance::toolbar_tool(ui, icon, label, false)
                })
                .inner
                .on_hover_text_lazy(|| {
                    format!("{label}\n{}", self.library.shortcuts.label(binding))
                })
                .on_disabled_hover_text(if ready {
                    if forward {
                        "No later view in this document"
                    } else {
                        "No earlier view in this document"
                    }
                } else {
                    "History is unavailable while loading or using a dialog"
                });
            if forward {
                self.input_buttons.history_forward = Some(response.id);
            } else {
                self.input_buttons.history_back = Some(response.id);
            }
        }
    }

    pub(super) fn toolbar_history(&mut self, context: &egui::Context, forward: bool) {
        if !self.history_controls_ready(context) {
            return;
        }
        // Reading the clicked toolbar response may have released editor focus
        // before its final text events have reached the draft.
        self.comments.focus |= self.pending_comment_input(context);
        self.nav.clear_prefix();
        self.mark_action = None;
        self.hints.cancel();
        self.search_focus = false;
        self.outline.cancel_input();
        self.history(forward);
    }
}

#[cfg(test)]
mod tests;
