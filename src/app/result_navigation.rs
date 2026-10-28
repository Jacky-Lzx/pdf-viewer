use super::{Pagewing, shortcuts};
use crate::list_navigation::Motion;
use eframe::egui::{self, Event, Id, Key, Modifiers, Response};
use std::collections::VecDeque;

#[derive(Clone, Copy)]
struct Row {
    scope: (u64, u64),
    index: usize,
}

#[derive(Default)]
pub(super) struct ResultNavigation {
    pending: Option<(Id, Row, Motion)>,
    registered: Vec<Id>,
}

fn motion(key: Key, modifiers: Modifiers) -> Option<Motion> {
    if !modifiers.is_none() {
        return None;
    }
    match key {
        Key::ArrowUp => Some(Motion::Step(-1)),
        Key::ArrowDown => Some(Motion::Step(1)),
        Key::PageUp => Some(Motion::Page(-1)),
        Key::PageDown => Some(Motion::Page(1)),
        Key::Home => Some(Motion::First),
        Key::End => Some(Motion::Last),
        _ => None,
    }
}

fn focused_row(context: &egui::Context, scope: (u64, u64)) -> Option<(Id, Row)> {
    let id = context.memory(|memory| memory.focused())?;
    let row = context.data(|data| data.get_temp::<Row>(id.with("search-result-navigation")))?;
    if row.scope != scope {
        return None;
    }
    let _focus = shortcuts::preserve_pointer_focus(context);
    context
        .read_response(id)
        .filter(|response| response.enabled() && response.has_focus())
        .map(|_| (id, row))
}

impl ResultNavigation {
    /// Previous-frame rows serve native input routing, then expire before
    /// layout. Retain capacity, rather than accumulating metadata as we scroll.
    pub fn begin_frame(&mut self, context: &egui::Context) {
        context.data_mut(|data| {
            for id in self.registered.drain(..) {
                data.remove::<Row>(id);
            }
        });
    }

    pub fn clear(&mut self) {
        self.pending = None;
    }

    pub fn register(&mut self, response: &Response, scope: (u64, u64), index: usize) {
        if !response.enabled() {
            return;
        }
        let id = response.id.with("search-result-navigation");
        response
            .ctx
            .data_mut(|data| data.insert_temp(id, Row { scope, index }));
        self.registered.push(id);
        if response.has_focus() {
            response.ctx.memory_mut(|memory| {
                memory.set_focus_lock_filter(
                    response.id,
                    egui::EventFilter {
                        vertical_arrows: true,
                        ..Default::default()
                    },
                );
            });
        }
    }

    /// Resolve page movement against this frame's actual result viewport.
    pub fn target(
        &mut self,
        context: &egui::Context,
        scope: (u64, u64),
        count: usize,
        page_rows: usize,
    ) -> Option<usize> {
        let (origin, row, motion) = self.pending?;
        if row.scope != scope || count == 0 || !context.memory(|memory| memory.has_focus(origin)) {
            self.clear();
            return None;
        }
        Some(
            row.index
                .saturating_add_signed(motion.delta(page_rows))
                .min(count - 1),
        )
    }
}

impl Pagewing {
    fn result_navigation_available(&self, context: &egui::Context) -> bool {
        // A virtual row can own focus while its old interaction rectangle is
        // clipped during scrolling/resizing. Its registered role, checked by
        // focused_row, determines ownership rather than that stale geometry.
        self.library.sidebar
            && self.nav.count > 0
            && self.sidebar_region.is_some()
            && !self.hits.is_empty()
            && !self.help
            && !self.properties_open
            && self.recent_picker.is_none()
            && self.page_input.is_none()
            && self.zoom_input.is_none()
            && self.password_input.is_none()
            && !(self.comments.open && self.comments.editor.is_some())
            && !egui::Popup::is_any_open(context)
    }

    pub(super) fn result_navigation_key(
        &mut self,
        context: &egui::Context,
        key: Key,
        modifiers: Modifiers,
    ) -> bool {
        let Some(motion) = motion(key, modifiers) else {
            return false;
        };
        if !self.result_navigation_available(context) {
            return false;
        }
        let Some((id, row)) = focused_row(context, (self.id, self.revision)) else {
            return false;
        };
        self.result_navigation.pending = Some((id, row, motion));
        self.nav.clear_prefix();
        self.hints.cancel();
        self.mark_action = None;
        context.memory_mut(|memory| memory.move_focus(egui::FocusDirection::None));
        context.input_mut(|input| input.consume_key(modifiers, key));
        true
    }

    /// Each motion must establish the new virtual row before a later copy,
    /// menu, activation or motion uses focus from the same native event batch.
    pub(super) fn result_navigation_input_split(
        &self,
        context: &egui::Context,
        events: &VecDeque<Event>,
        limit: usize,
    ) -> Option<usize> {
        let opener = events.iter().take(limit).position(|event| {
            matches!(event, Event::Key { key, modifiers, pressed: true, .. }
                if motion(*key, *modifiers).is_some())
        })?;
        if !self.result_navigation_available(context)
            || focused_row(context, (self.id, self.revision)).is_none()
        {
            return None;
        }
        if events
            .iter()
            .take(opener)
            .any(super::focus_input::changes_input)
        {
            // A preceding copy/click/text action still belongs to its current
            // owner. Let it finish before this motion changes virtual focus.
            return Some(opener);
        }
        events
            .iter()
            .take(limit)
            .enumerate()
            .skip(opener + 1)
            .find_map(|(index, event)| super::focus_input::changes_input(event).then_some(index))
    }
}
