use eframe::egui::{self, Event, Id, Key, Modifiers, Response, Ui};

/// Logical menu order keeps clipped items reachable and excludes reader bars
/// behind the popup from directional focus. Reuse the previous pass's IDs.
pub(super) struct MenuFocus {
    id: Id,
    items: Vec<Id>,
    opening_target: Option<usize>,
}

impl MenuFocus {
    pub fn new(ui: &Ui, keyboard_open: bool) -> Self {
        let id = ui.make_persistent_id("menu-focus-items");
        let mut items = ui.data_mut(|data| data.remove_temp::<Vec<Id>>(id).unwrap_or_default());
        let focused = ui.memory(|memory| memory.focused());
        let mut row = if keyboard_open {
            Some(0)
        } else {
            items.iter().position(|id| Some(*id) == focused)
        };
        let mut moved = false;
        let mut activated = false;
        ui.input_mut(|input| {
            input.events.retain(|event| {
                let Event::Key {
                    key,
                    pressed: true,
                    modifiers,
                    ..
                } = event
                else {
                    return true;
                };
                if !keyboard_open && matches!(key, Key::Enter | Key::Space) && modifiers.is_none() {
                    activated = true;
                }
                let next = match (*key, *modifiers) {
                    (Key::ArrowDown | Key::Tab, Modifiers::NONE) => Some(false),
                    (Key::ArrowUp, Modifiers::NONE) | (Key::Tab, Modifiers::SHIFT) => Some(true),
                    _ => None,
                };
                let Some(reverse) = next else { return true };
                if !items.is_empty() && !activated {
                    row = Some(if reverse {
                        row.map_or(items.len() - 1, |row| (row + items.len() - 1) % items.len())
                    } else {
                        row.map_or(0, |row| (row + 1) % items.len())
                    });
                }
                moved = true;
                false
            });
        });
        if moved || keyboard_open {
            ui.memory_mut(|memory| {
                memory.move_focus(egui::FocusDirection::None);
                if moved
                    && !keyboard_open
                    && let Some(&id) = row.and_then(|row| items.get(row))
                {
                    memory.request_focus(id);
                }
            });
        }
        items.clear();
        Self {
            id,
            items,
            opening_target: keyboard_open.then_some(row.unwrap_or(0)),
        }
    }

    pub fn observe(&mut self, response: &Response) {
        if self.opening_target == Some(self.items.len()) {
            // Focus after constructing the item so the key that opened the
            // menu cannot also activate its first option.
            response.request_focus();
        }
        self.items.push(response.id);
    }

    pub fn finish(self, ui: &Ui) {
        ui.data_mut(|data| data.insert_temp(self.id, self.items));
    }
}
