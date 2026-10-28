use super::{Pagewing, appearance, tooltips::LazyTooltip};
use eframe::egui;

impl Pagewing {
    pub(super) fn reading_controls_input_split(
        &self,
        context: &egui::Context,
        events: &std::collections::VecDeque<egui::Event>,
        limit: usize,
    ) -> Option<usize> {
        if !self.toolbar_control_focused(context) && !self.status_control_focused(context) {
            return None;
        }
        let command = crate::settings::commands()
            .iter()
            .find(|command| command.id == "reading_controls")?;
        let bindings = self.library.shortcuts.bindings(command);
        let focused = context.memory(|memory| memory.focused());
        let end = events
            .iter()
            .take(limit)
            .enumerate()
            .find_map(|(index, event)| {
                let egui::Event::Key {
                    key,
                    modifiers,
                    pressed: true,
                    ..
                } = event
                else {
                    return None;
                };
                // Keep a key's literal text in the same frame, including a
                // layout-dependent punctuation binding that egui must consume.
                let text = match events.get(index + 1) {
                    Some(egui::Event::Text(text)) => Some(text),
                    _ => None,
                };
                let character = text.and_then(|text| {
                    let mut chars = text.chars();
                    let first = chars.next()?;
                    chars.next().is_none().then_some(first)
                });
                let activates = focused == self.input_buttons.reading_controls
                    && matches!(key, egui::Key::Enter | egui::Key::Space)
                    && modifiers.is_none();
                (activates
                    || bindings.contains(&crate::settings::Binding::from_event(
                        *key, *modifiers, character,
                    )))
                .then_some(index + 1 + usize::from(text.is_some()))
            })?;
        events
            .iter()
            .take(limit)
            .enumerate()
            .skip(end)
            .find_map(|(index, event)| super::focus_input::changes_input(event).then_some(index))
    }

    pub(super) fn toggle_reading_controls(&mut self, context: &egui::Context) {
        // Release only the controls that disappear. Search and comment drafts
        // retain their focus and contents when the bars are toggled by mouse.
        if (self.toolbar_control_focused(context) || self.status_control_focused(context))
            && let Some(id) = context.memory(|memory| memory.focused())
        {
            context.memory_mut(|memory| memory.surrender_focus(id));
        }
        self.controls_hidden = !self.controls_hidden;
    }

    pub(super) fn controls_restore_button(&mut self, context: &egui::Context, canvas: egui::Rect) {
        if !self.controls_hidden {
            return;
        }
        let bounds = canvas.shrink(8.0);
        let area = egui::Area::new(egui::Id::new("reading-controls-restore"))
            .order(egui::Order::Foreground)
            .fixed_pos(bounds.right_top())
            .pivot(egui::Align2::RIGHT_TOP)
            .constrain_to(bounds)
            .show(context, |ui| {
                self.input_buttons.reading_controls = Some(
                    appearance::action_button(
                        ui,
                        "show-reading-controls",
                        egui::Button::new("Show controls"),
                    )
                    .on_hover_text_lazy(|| self.library.shortcuts.label("reading_controls"))
                    .id,
                );
            });
        self.toolbar_region = Some((area.response.layer_id, area.response.rect));
    }
}

#[cfg(test)]
mod tests;
