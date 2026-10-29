//! Restore keyboard focus after editing a sidebar list, before replaying input.
use super::{Pagewing, shortcuts};
use eframe::egui::{self, Event, Id, Key, Response};
use std::collections::VecDeque;

#[derive(Clone, Copy, PartialEq)]
pub(super) enum List {
    Bookmarks,
    Marks,
}

struct Request {
    document: u64,
    list: List,
    key: usize,
    origin: Id,
}

#[derive(Default)]
pub(super) struct RemovalFocus {
    pending: Option<Request>,
    frame: Option<u64>,
    buttons: Vec<Id>,
}

impl RemovalFocus {
    pub fn register(&mut self, response: &Response) {
        let frame = response.ctx.cumulative_frame_nr();
        if self.frame != Some(frame) {
            self.buttons.clear();
            self.frame = Some(frame);
        }
        if response.enabled() {
            self.buttons.push(response.id);
        }
    }

    pub fn request(&mut self, document: u64, list: List, key: Option<usize>, origin: Id) {
        self.pending = key.map(|key| Request {
            document,
            list,
            key,
            origin,
        });
    }

    pub fn clear(&mut self) {
        self.pending = None;
    }

    fn waiting(&mut self, context: &egui::Context, document: u64) -> bool {
        if self.pending.as_ref().is_some_and(|request| {
            request.document != document
                || context.memory(|memory| memory.focused().is_some_and(|id| id != request.origin))
        }) {
            self.clear();
        }
        self.pending.is_some()
    }

    pub fn target(&mut self, context: &egui::Context, document: u64, list: List) -> Option<usize> {
        if !self.waiting(context, document) {
            return None;
        }
        self.pending
            .as_ref()
            .filter(|request| request.list == list)
            .map(|request| request.key)
    }

    pub fn apply(&mut self, ui: &egui::Ui, response: &Response) {
        if self.pending.is_some()
            && response.enabled()
            // egui cannot scroll an inner list against an offscreen viewport.
            // Once exposed, normal focus scrolling reveals the whole control.
            && response.interact_rect.is_positive()
            && !ui.is_sizing_pass()
            && !ui.ctx().will_discard()
        {
            response.request_focus();
            self.clear();
        }
    }

    fn focused_button(&self, context: &egui::Context) -> Option<Id> {
        let focused = context.memory(|memory| memory.focused())?;
        if !self.buttons.contains(&focused)
            || self
                .frame
                .is_none_or(|frame| frame < context.cumulative_frame_nr().saturating_sub(1))
        {
            return None;
        }
        let _focus = shortcuts::preserve_pointer_focus(context);
        context
            .read_response(focused)
            .filter(|response| response.enabled() && response.has_focus())
            .map(|_| focused)
    }

    pub fn control_focused(&self, context: &egui::Context) -> bool {
        // A resize may clip the last response before focus scrolling runs.
        // Its registered role still owns activation during that layout change.
        self.focused_button(context).is_some()
    }

    fn input_split(
        &self,
        context: &egui::Context,
        events: &VecDeque<Event>,
        limit: usize,
    ) -> Option<usize> {
        self.focused_button(context)?;
        let activation = events.iter().take(limit).position(|event| {
            matches!(event, Event::Key { key: Key::Enter | Key::Space, pressed: true, modifiers, .. } if modifiers.is_none())
        })?;
        if events
            .iter()
            .take(activation)
            .any(super::focus_input::changes_input)
        {
            return Some(activation);
        }
        events
            .iter()
            .take(limit)
            .enumerate()
            .skip(activation + 1)
            .find_map(|(index, event)| super::focus_input::changes_input(event).then_some(index))
    }
}

impl Pagewing {
    fn sidebar_removal_available(&self) -> bool {
        self.library.sidebar
            && self.nav.count > 0
            && !self.help
            && !self.properties_open
            && self.recent_picker.is_none()
            && self.page_input.is_none()
            && self.zoom_input.is_none()
            && self.password_input.is_none()
    }

    pub(super) fn removal_focus_waiting(&mut self, context: &egui::Context) -> bool {
        if !self.sidebar_removal_available()
            || egui::Popup::is_any_open(context)
            || self
                .removal_focus
                .pending
                .as_ref()
                .is_some_and(|request| match request.list {
                    List::Bookmarks => self.bookmark_pages.is_empty(),
                    List::Marks => self.reading.marks.is_empty(),
                })
        {
            self.removal_focus.clear();
            return false;
        }
        self.removal_focus.waiting(context, self.id)
    }

    pub(super) fn removal_input_split(
        &self,
        context: &egui::Context,
        events: &VecDeque<Event>,
        limit: usize,
    ) -> Option<usize> {
        if !self.sidebar_removal_available() || egui::Popup::is_any_open(context) {
            return None;
        }
        self.removal_focus.input_split(context, events, limit)
    }
}
