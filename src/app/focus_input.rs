//! Preserve native input order across focus movement and popup opening.
use super::Pagewing;
use eframe::egui::{self, Event, Key};
use std::collections::VecDeque;

pub(super) fn changes_input(event: &Event) -> bool {
    !matches!(
        event,
        Event::Key { pressed: false, .. } | Event::PointerMoved(_) | Event::MouseMoved(_)
    )
}

fn tab_split(events: &VecDeque<Event>) -> Option<usize> {
    let tab = events.iter().position(|event| {
        matches!(
            event,
            Event::Key {
                key: Key::Tab,
                pressed: true,
                ..
            }
        )
    })?;
    if events.iter().take(tab).any(changes_input) {
        // Earlier text or shortcuts must reach their input before Tab changes
        // focus, including inputs still being laid out in a sizing pass.
        Some(tab)
    } else {
        // Release and pointer motion can accompany this Tab. Keep later
        // actions, including Enter/Space, behind the resulting focus change.
        events
            .iter()
            .enumerate()
            .skip(tab + 1)
            .find_map(|(index, event)| changes_input(event).then_some(index))
    }
}

impl Pagewing {
    pub(super) fn prepare_native_input(
        &mut self,
        context: &egui::Context,
        input: &mut egui::RawInput,
    ) {
        // Only one split is chosen per frame, so these queues are exclusive.
        debug_assert!(self.focus_input.is_empty() || self.comments.filter_input.is_empty());
        let mut events = if self.focus_input.is_empty() && self.comments.filter_input.is_empty() {
            VecDeque::from(std::mem::take(&mut input.events))
        } else {
            let mut events = if self.comments.filter_input.is_empty() {
                std::mem::take(&mut self.focus_input)
            } else {
                std::mem::take(&mut self.comments.filter_input)
            };
            events.extend(std::mem::take(&mut input.events));
            events
        };
        // A disabled sizing pass may have held back text from the previous
        // prefix. It precedes the queued suffix as well as new native input.
        for event in std::mem::take(&mut self.deferred_input).into_iter().rev() {
            events.push_front(event);
        }
        // Each detector can shorten the current prefix. Later detectors never
        // inspect input already held behind an earlier focus transition.
        let mut split = tab_split(&events);
        split = self
            .input_buttons
            .menu_input_split(context, &events, split.unwrap_or(events.len()))
            .or(split);
        split = super::copy_menu::input_split(context, &events, split.unwrap_or(events.len()))
            .or(split);
        split = self
            .input_buttons
            .results_input_split(context, &events, split.unwrap_or(events.len()))
            .or(split);
        split = self
            .result_navigation_input_split(context, &events, split.unwrap_or(events.len()))
            .or(split);
        // A bar can disappear before a later activation in the same native
        // batch. Establish its new focus before reading old button responses.
        split = self
            .reading_controls_input_split(context, &events, split.unwrap_or(events.len()))
            .or(split);
        let comment = self.comment_input_split(context, &events, split.unwrap_or(events.len()));
        if let Some(split) = comment.or(split) {
            input.events.extend(events.drain(..split));
            if comment.is_some() {
                self.comments.filter_input = events;
            } else {
                self.focus_input = events;
            }
            context.request_repaint();
        } else {
            input.events = events.into();
        }
    }
}
