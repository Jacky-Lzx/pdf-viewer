use super::reveal_offset;
use eframe::egui::{self, Id, Response, ScrollArea, Ui, Vec2};

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
struct Viewport {
    size: Vec2,
    height: f32,
    stride: f32,
    row_height: f32,
    focused: Option<(Id, usize)>,
}

/// Keep a focused virtual row alive before a resized list chooses its range.
pub(super) struct ListFocus {
    id: Id,
    previous: Option<Viewport>,
    current: Viewport,
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
        let reveal = previous.and_then(|previous| {
            previous.focused.and_then(|(id, row)| {
                (focused == Some(id)
                    // A chapter list's height also changes when its outer
                    // sidebar is wheel-scrolled; that must not pull focus back.
                    && (previous.size != current.size
                        || previous.stride != current.stride
                        || previous.row_height != row_height))
                    .then_some(row)
            })
        });
        Self {
            id,
            previous,
            current,
            reveal,
        }
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
        self.observe_id(row_focus(response), row);
    }

    pub fn observe_id(&mut self, id: Option<Id>, row: usize) {
        if let Some(id) = id {
            self.current.focused = Some((id, row));
        }
    }

    pub fn finish(mut self, ui: &Ui) {
        if ui.is_sizing_pass() {
            return;
        }
        self.current.focused = self
            .current
            .focused
            .filter(|&(id, _)| ui.memory(|memory| memory.has_focus(id)));
        if self.previous == Some(self.current) {
            return;
        }
        if let Some((id, _)) = self.current.focused
            && (self.reveal.is_some()
                || self.previous.and_then(|previous| previous.focused) != self.current.focused)
        {
            let response = {
                let _focus = crate::app::shortcuts::preserve_pointer_focus(ui.ctx());
                ui.ctx().read_response(id)
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
