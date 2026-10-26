use eframe::egui::{Event, Id, InputState, Key, Modifiers, Ui};

pub(super) fn focus_text_input(
    ui: &Ui,
    id: Id,
    requested: &mut bool,
    deferred: &mut Vec<Event>,
) -> bool {
    if !*requested {
        return false;
    }
    if ui.is_sizing_pass() {
        // A new window first lays out an invisible, disabled area. Its text
        // widget cannot accept focus or input until the next pass.
        ui.input_mut(|input| {
            deferred.extend(input.events.extract_if(.., |event| {
                matches!(
                    event,
                    Event::Key { .. }
                        | Event::Text(_)
                        | Event::Copy
                        | Event::Cut
                        | Event::Paste(_)
                        | Event::Ime(_)
                )
            }));
        });
        ui.ctx().request_repaint();
        return false;
    }
    *requested = false;
    ui.memory_mut(|memory| {
        memory.request_focus(id);
        // egui scheduled arrow/Tab focus movement before shortcut dispatch.
        // Explicitly opening an input must also cancel that pending movement.
        memory.move_focus(eframe::egui::FocusDirection::None);
    });
    true
}

pub(super) fn consume_text_shortcut(input: &mut InputState, key: Key, modifiers: Modifiers) {
    // Option shortcuts can also generate a text event on macOS. Preserve text
    // typed before/after this occurrence, including the same key typed again
    // after it opens an input, even when a busy frame batches everything.
    let mut consumed = false;
    let mut shortcut_text = false;
    input.events.retain(|event| match event {
        Event::Key {
            key: event_key,
            modifiers: event_modifiers,
            pressed,
            ..
        } => {
            shortcut_text =
                !consumed && *pressed && *event_key == key && *event_modifiers == modifiers;
            consumed |= shortcut_text;
            !shortcut_text
        }
        Event::Text(_) => !shortcut_text,
        _ => true,
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(key: Key, modifiers: Modifiers, pressed: bool) -> Event {
        Event::Key {
            key,
            physical_key: Some(key),
            modifiers,
            pressed,
            repeat: false,
        }
    }

    #[test]
    fn opening_shortcut_preserves_the_same_character_typed_in_the_query() {
        let plain = Modifiers::NONE;
        let mut input = InputState::default();
        input.events = vec![
            key(Key::Slash, plain, true),
            Event::Text("/".into()),
            key(Key::Slash, plain, false),
            key(Key::Slash, plain, true),
            Event::Text("/".into()),
            key(Key::Slash, plain, false),
            key(Key::U, plain, true),
            Event::Text("u".into()),
        ];
        consume_text_shortcut(&mut input, Key::Slash, plain);
        let text: String = input
            .events
            .iter()
            .filter_map(|event| match event {
                Event::Text(text) => Some(text.as_str()),
                _ => None,
            })
            .collect();
        assert_eq!(text, "/u");
        assert_eq!(input.num_presses(Key::Slash), 1);
    }

    #[test]
    fn opening_input_cancels_previously_scheduled_arrow_focus_movement() {
        use eframe::egui::{CentralPanel, Context, RawInput, TextEdit};

        let context = Context::default();
        let id = Id::new("opening-input");
        let mut requested = false;
        let mut text = String::new();
        let mut deferred = Vec::new();
        // Establish the geometry of a focusable button below the text field.
        for events in [Vec::new(), vec![key(Key::ArrowDown, Modifiers::NONE, true)]] {
            let _ = context.run(
                RawInput {
                    events,
                    ..Default::default()
                },
                |context| {
                    CentralPanel::default().show(context, |ui| {
                        focus_text_input(ui, id, &mut requested, &mut deferred);
                        ui.add(TextEdit::singleline(&mut text).id(id));
                        let _ = ui.button("Focusable result below the input");
                    });
                },
            );
            // The second pass opens the input while ArrowDown has already
            // scheduled egui focus movement at the beginning of that pass.
            requested = true;
        }
        assert!(context.memory(|memory| memory.has_focus(id)));
    }

    #[test]
    fn initial_window_sizing_defers_text_and_enter_until_the_input_is_interactive() {
        use eframe::egui::{Context, RawInput, TextEdit, Window};

        let context = Context::default();
        let id = Id::new("test-input");
        let mut requested = true;
        let mut deferred = Vec::new();
        let mut text = String::new();
        let mut focused_now = false;
        let input = RawInput {
            events: vec![
                key(Key::Num2, Modifiers::NONE, true),
                Event::Text("2".into()),
                key(Key::Enter, Modifiers::NONE, true),
            ],
            ..Default::default()
        };
        let _ = context.run(input, |context| {
            Window::new("Input test").show(context, |ui| {
                assert!(ui.is_sizing_pass());
                focused_now = focus_text_input(ui, id, &mut requested, &mut deferred);
                ui.add(TextEdit::singleline(&mut text).id(id));
            });
        });
        assert!(requested && !focused_now && text.is_empty());
        assert_eq!(deferred.len(), 3);
        let input = RawInput {
            events: std::mem::take(&mut deferred),
            ..Default::default()
        };
        let _ = context.run(input, |context| {
            Window::new("Input test").show(context, |ui| {
                assert!(!ui.is_sizing_pass());
                focused_now = focus_text_input(ui, id, &mut requested, &mut deferred);
                ui.add(TextEdit::singleline(&mut text).id(id));
                assert!(ui.input(|input| input.key_pressed(Key::Enter)));
            });
        });
        assert!(focused_now && !requested && deferred.is_empty());
        assert_eq!(text, "2");
        assert!(!context.memory(|memory| memory.has_focus(id)));
    }

    #[test]
    fn batched_typing_and_option_generated_text_are_handled_independently() {
        let alt = Modifiers::ALT;
        let plain = Modifiers::NONE;
        let mut input = InputState::default();
        input.events = vec![
            key(Key::T, plain, true),
            Event::Text("t".into()),
            key(Key::T, plain, false),
            key(Key::C, alt, true),
            Event::Text("ç".into()),
            key(Key::C, alt, false),
            key(Key::S, plain, true),
            Event::Text("s".into()),
            key(Key::S, plain, false),
            key(Key::W, alt, true),
            Event::Text("∑".into()),
            key(Key::W, alt, false),
            key(Key::C, plain, true),
            Event::Text("c".into()),
        ];
        consume_text_shortcut(&mut input, Key::C, alt);
        consume_text_shortcut(&mut input, Key::W, alt);
        let text: String = input
            .events
            .iter()
            .filter_map(|event| match event {
                Event::Text(text) => Some(text.as_str()),
                _ => None,
            })
            .collect();
        assert_eq!(text, "tsc");
        assert!(!input.events.iter().any(
            |event| matches!(event, Event::Key { modifiers, pressed: true, .. } if modifiers.alt)
        ));
        assert!(input.events.iter().any(|event| matches!(event, Event::Key { key: Key::C, pressed: true, modifiers, .. } if !modifiers.alt)));
    }
}
