//! Keep final editor input ahead of a filter click in native event batches.
use super::{Pagewing, edits_comment};
use eframe::egui;

impl Pagewing {
    pub(in crate::app) fn prepare_comment_input(
        &mut self,
        context: &egui::Context,
        input: &mut egui::RawInput,
    ) {
        if !self.comments.filter_input.is_empty() {
            let mut queued = std::mem::take(&mut self.comments.filter_input);
            queued.append(&mut input.events);
            input.events = queued;
        }
        if !self.comments.open
            || !context.memory(|memory| memory.has_focus(egui::Id::new("pagewing-comment-input")))
        {
            return;
        }
        let Some((index, position)) = input.events.iter().enumerate().find_map(|(index, event)| {
            if let egui::Event::PointerButton {
                pos,
                button: egui::PointerButton::Primary,
                pressed: true,
                ..
            } = event
            {
                Some((index, *pos))
            } else {
                None
            }
        }) else {
            return;
        };
        let Some(filter) = context.read_response(egui::Id::new("pagewing-comment-filter")) else {
            return;
        };
        if !filter.enabled()
            || !filter.interact_rect.contains(position)
            || context
                .layer_id_at(position)
                .is_some_and(|layer| layer != filter.layer_id)
            || !input.events[..index].iter().any(edits_comment)
        {
            return;
        }
        // Split before egui processes pointer focus. Drawing fields in their
        // layout order would otherwise give earlier editor text to the filter.
        // Replay the click and its following keys before next frame's input.
        self.comments.filter_input = input.events.split_off(index);
        context.request_repaint();
    }
}
