use eframe::egui::{self, Event, Id, Key, Modifiers, Response, Ui};

/// Logical menu order keeps clipped items reachable and excludes reader bars
/// behind the popup from directional focus. Reuse the previous pass's IDs.
pub(super) struct MenuFocus {
    id: Id,
    items: Vec<Id>,
    opening_motion: Option<isize>,
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
        let mut opener_seen = false;
        let mut opening_motion = 0isize;
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
                if matches!(key, Key::Enter | Key::Space) && modifiers.is_none() {
                    if keyboard_open && !opener_seen {
                        opener_seen = true;
                    } else {
                        activated = true;
                    }
                }
                let next = match (*key, *modifiers) {
                    (Key::ArrowDown | Key::Tab, Modifiers::NONE) => Some(false),
                    (Key::ArrowUp, Modifiers::NONE) | (Key::Tab, Modifiers::SHIFT) => Some(true),
                    _ => None,
                };
                let Some(reverse) = next else { return true };
                if keyboard_open && !activated {
                    // A new popup has no cached IDs yet. Apply these motions
                    // after all items exist instead of losing the first keys.
                    opening_motion = opening_motion.saturating_add(if reverse { -1 } else { 1 });
                } else if !items.is_empty() && !activated {
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
            opening_motion: keyboard_open.then_some(opening_motion),
        }
    }

    pub fn observe(&mut self, response: &Response) {
        self.items.push(response.id);
    }

    pub fn finish(self, ui: &Ui) {
        if let Some(motion) = self.opening_motion
            && !self.items.is_empty()
        {
            // Delay initial focus until construction finishes so the opener
            // cannot also activate a choice. Reveal it on the following pass.
            let row = motion.rem_euclid(self.items.len() as isize) as usize;
            ui.memory_mut(|memory| memory.request_focus(self.items[row]));
        }
        ui.data_mut(|data| data.insert_temp(self.id, self.items));
    }
}
