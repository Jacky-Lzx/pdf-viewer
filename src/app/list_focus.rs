use eframe::egui::{self, Id, Response, ScrollArea, Ui, Vec2};

/// Reveal only the part of a row outside the current list viewport.
pub(super) fn reveal_offset(offset: f32, top: f32, row_height: f32, height: f32) -> f32 {
    offset.max((top + row_height - height).max(0.0)).min(top)
}

pub(super) fn row_focus(response: &Response) -> Option<Id> {
    if !response.has_focus() {
        return None;
    }
    if response.gained_focus() {
        response.scroll_to_me_animation(None, egui::style::ScrollAnimation::none());
    }
    Some(response.id)
}

#[derive(Clone, Copy, PartialEq)]
struct FocusedRow {
    id: Id,
    row: usize,
    key: usize,
}

#[derive(Clone, Copy, PartialEq)]
struct Viewport {
    size: Vec2,
    height: f32,
    stride: f32,
    row_height: f32,
    focused: Option<FocusedRow>,
}

/// Keep a focused virtual row alive before a resized or edited list chooses its range.
pub(super) struct ListFocus {
    id: Id,
    previous: Option<Viewport>,
    current: Viewport,
    tracked: Option<FocusedRow>,
    reveal: Option<usize>,
}

impl ListFocus {
    pub fn new(ui: &Ui, salt: &str, row_height: f32, height: f32) -> Self {
        let id = ui.make_persistent_id(Id::new(salt));
        let previous = ui.data(|data| data.get_temp::<Viewport>(id.with("focused-row")));
        let current = Viewport {
            size: ui.clip_rect().size(),
            height,
            stride: row_height + ui.spacing().item_spacing.y,
            row_height,
            focused: None,
        };
        let focused = ui.memory(|memory| memory.focused());
        let tracked = previous
            .and_then(|previous| previous.focused)
            .filter(|row| focused == Some(row.id));
        let reveal = previous.and_then(|previous| {
            tracked.and_then(|row| {
                // A chapter list's height also changes when its outer
                // sidebar is wheel-scrolled; that must not pull focus back.
                (previous.size != current.size
                    || previous.stride != current.stride
                    || previous.row_height != row_height)
                    .then_some(row.row)
            })
        });
        Self {
            id,
            previous,
            current,
            tracked,
            reveal,
        }
    }

    /// Resolve a logical target before virtualizing a list that can be edited.
    pub fn remap(&mut self, row_for: impl FnOnce(usize) -> Option<usize>) {
        if let Some(tracked) = self.tracked {
            self.reveal = match row_for(tracked.key) {
                Some(row) if row != tracked.row => Some(row),
                Some(_) => self.reveal,
                None => None,
            };
        }
    }

    pub fn has_focus(&self) -> bool {
        self.tracked.is_some()
    }

    pub fn scroll(&self, ui: &Ui, scroll: ScrollArea) -> ScrollArea {
        let Some(row) = self.reveal else {
            return scroll;
        };
        let mut offset =
            egui::scroll_area::State::load(ui.ctx(), self.id).map_or(0.0, |state| state.offset.y);
        if let Some(previous) = self.previous {
            offset *= self.current.stride / previous.stride;
        }
        scroll.vertical_scroll_offset(reveal_offset(
            offset,
            row as f32 * self.current.stride,
            self.current.row_height,
            self.current.height,
        ))
    }

    pub fn observe(&mut self, response: &Response, row: usize) {
        self.observe_id(row_focus(response), row, row);
    }

    pub fn observe_id(&mut self, id: Option<Id>, row: usize, key: usize) {
        if let Some(id) = id {
            self.current.focused = Some(FocusedRow { id, row, key });
        }
    }

    pub fn finish(mut self, ui: &Ui) {
        // A responsive footer can discard this pass before its final height
        // is known. Revealing against both layouts would add the scroll twice.
        if ui.is_sizing_pass() || ui.ctx().will_discard() {
            return;
        }
        self.current.focused = self
            .current
            .focused
            .filter(|row| ui.memory(|memory| memory.has_focus(row.id)));
        if self.previous == Some(self.current) {
            return;
        }
        if let Some(row) = self.current.focused
            && (self.reveal.is_some()
                || self.previous.and_then(|previous| previous.focused) != self.current.focused)
        {
            let response = {
                let _focus = crate::app::shortcuts::preserve_pointer_focus(ui.ctx());
                ui.ctx().read_response(row.id)
            };
            // The inner list has finished; reveal the actual row in the outer
            // sidebar too, including when only fonts or card heights changed.
            if let Some(response) = response {
                response.scroll_to_me_animation(None, egui::style::ScrollAnimation::none());
            }
        }
        ui.data_mut(|data| data.insert_temp(self.id.with("focused-row"), self.current));
    }
}
