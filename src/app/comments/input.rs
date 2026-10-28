//! Keep final editor input ahead of a filter click in native event batches.
use super::{Pagewing, edits_comment};
use eframe::egui;

impl Pagewing {
    pub(in crate::app) fn comment_input_split(
        &self,
        context: &egui::Context,
        events: &std::collections::VecDeque<egui::Event>,
        limit: usize,
    ) -> Option<usize> {
        if !self.comments.open
            || !context.memory(|memory| memory.has_focus(egui::Id::new("pagewing-comment-input")))
        {
            return None;
        }
        let (index, position) =
            events
                .iter()
                .take(limit)
                .enumerate()
                .find_map(|(index, event)| {
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
                })?;
        let filter = context.read_response(egui::Id::new("pagewing-comment-filter"))?;
        if !filter.enabled()
            || !filter.interact_rect.contains(position)
            || context
                .layer_id_at(position)
                .is_some_and(|layer| layer != filter.layer_id)
            || !events.iter().take(index).any(edits_comment)
        {
            return None;
        }
        // Split before egui processes pointer focus. Drawing fields in their
        // layout order would otherwise give earlier editor text to the filter.
        // Replay the click and its following keys before next frame's input.
        Some(index)
    }
}
