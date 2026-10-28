use eframe::egui::{self, Event, Id, Key, Modifiers, Response};
use std::collections::VecDeque;

#[derive(Clone, Copy, PartialEq)]
pub(super) enum Target {
    Document,
    Control,
}

pub(super) fn control_menu_open(context: &egui::Context) -> bool {
    context
        .data(|data| data.get_temp::<(Id, Target)>(Id::new("pagewing-open-copy-menu")))
        .is_some_and(|(id, target)| {
            target == Target::Control && egui::Popup::is_id_open(context, id)
        })
}

#[derive(Clone, Copy)]
struct Owner {
    widget: Id,
    menu: Id,
}

#[derive(Clone, Copy, Default, PartialEq)]
struct State {
    keyboard: bool,
    opening: bool,
    was_open: bool,
    copy_requested: bool,
}

fn owner_id() -> Id {
    Id::new("pagewing-focused-copy-menu")
}

fn focused_owner(context: &egui::Context) -> Option<Owner> {
    let owner = context.data(|data| data.get_temp::<Owner>(owner_id()))?;
    let _focus = super::shortcuts::preserve_pointer_focus(context);
    context
        .read_response(owner.widget)
        .filter(|response| response.enabled() && response.has_focus())
        .map(|_| owner)
}

pub(super) fn request_copy(context: &egui::Context) -> bool {
    let Some(owner) = focused_owner(context) else {
        return false;
    };
    context.data_mut(|data| {
        data.get_temp_mut_or_default::<State>(owner.menu)
            .copy_requested = true;
    });
    context.input_mut(|input| input.events.retain(|event| !matches!(event, Event::Copy)));
    true
}

pub(super) fn open(context: &egui::Context, key: Key, modifiers: Modifiers) -> bool {
    if key != Key::F10 || modifiers != Modifiers::SHIFT {
        return false;
    }
    let Some(owner) = focused_owner(context) else {
        return false;
    };
    context.data_mut(|data| {
        let state = data.get_temp_mut_or_default::<State>(owner.menu);
        state.keyboard = true;
        state.opening = true;
        state.was_open = true;
    });
    egui::Popup::open_id(context, owner.menu);
    true
}

/// Let a context menu establish its item focus before later native input.
pub(super) fn input_split(
    context: &egui::Context,
    events: &VecDeque<Event>,
    limit: usize,
) -> Option<usize> {
    let menu_open = context
        .data(|data| data.get_temp::<(Id, Target)>(Id::new("pagewing-open-copy-menu")))
        .is_some_and(|(id, _)| egui::Popup::is_id_open(context, id));
    let opener = events.iter().take(limit).position(|event| match event {
        Event::PointerButton {
            button: egui::PointerButton::Secondary,
            pressed: false,
            ..
        } => true,
        Event::Key {
            key: Key::F10,
            pressed: true,
            modifiers,
            ..
        } => *modifiers == Modifiers::SHIFT && focused_owner(context).is_some(),
        Event::Key {
            key: Key::Enter | Key::Space | Key::Escape,
            pressed: true,
            modifiers,
            ..
        } => menu_open && modifiers.is_none(),
        Event::Copy => menu_open,
        Event::PointerButton {
            button: egui::PointerButton::Primary,
            pressed: false,
            ..
        } => menu_open,
        _ => false,
    })?;
    events
        .iter()
        .take(limit)
        .enumerate()
        .skip(opener + 1)
        .find_map(|(index, event)| super::focus_input::changes_input(event).then_some(index))
}

/// Focus owns keyboard copying; popup IDs scope requests to the current source.
/// Only an actual copy allocates text. Keyboard menus follow their owner on resize.
pub(super) fn show(response: &Response, id: Id, label: &str, text: &str, target: Target) -> bool {
    if response.has_focus() && response.enabled() {
        response.ctx.data_mut(|data| {
            data.insert_temp(
                owner_id(),
                Owner {
                    widget: response.id,
                    menu: id,
                },
            );
        });
    }
    let mut state = response
        .ctx
        .data_mut(|data| data.remove_temp::<State>(id).unwrap_or_default());
    let mut copied =
        std::mem::take(&mut state.copy_requested) && response.has_focus() && response.enabled();
    if copied {
        response.ctx.copy_text(text.to_owned());
    }
    if response.secondary_clicked() {
        state = State::default();
    }
    let opening = std::mem::take(&mut state.opening);
    let popup = if state.keyboard {
        egui::Popup::menu(response).open_memory(None)
    } else {
        egui::Popup::context_menu(response)
    };
    popup.id(id).show(|ui| {
        let copy = ui.button(label);
        if response.secondary_clicked() || opening {
            copy.request_focus();
        }
        ui.memory_mut(|memory| {
            memory.set_focus_lock_filter(
                copy.id,
                egui::EventFilter {
                    tab: true,
                    horizontal_arrows: true,
                    vertical_arrows: true,
                    ..Default::default()
                },
            );
        });
        let copy_shortcut = copy.has_focus()
            && ui.input_mut(|input| {
                let mut requested = false;
                input.events.retain(|event| {
                    if matches!(event, Event::Copy) {
                        requested = true;
                        false
                    } else {
                        true
                    }
                });
                requested
            });
        if copy.clicked() || copy_shortcut {
            ui.ctx().copy_text(text.to_owned());
            copied = true;
            ui.close();
        }
    });
    let open = egui::Popup::is_id_open(&response.ctx, id);
    if open {
        response
            .ctx
            .data_mut(|data| data.insert_temp(Id::new("pagewing-open-copy-menu"), (id, target)));
    }
    if state.keyboard && state.was_open && !open {
        // A mouse dismissal can focus another control before this row is
        // drawn. Preserve that choice; keyboard closure returns to the row.
        if copied || !response.ctx.input(|input| input.pointer.any_click()) {
            response.request_focus();
        }
        response.ctx.request_repaint();
        state.keyboard = false;
    }
    state.was_open = open;
    if state != State::default() {
        response.ctx.data_mut(|data| data.insert_temp(id, state));
    }
    copied
}

#[cfg(test)]
mod tests;
