use eframe::egui::{Event, Id, InputState, Key, Modifiers, Ui};

pub(super) struct SideButtonFocusGuard {
    context: eframe::egui::Context,
    previous: eframe::egui::SurrenderFocusOn,
}

impl Drop for SideButtonFocusGuard {
    fn drop(&mut self) {
        self.context
            .options_mut(|options| options.input_options.surrender_focus_on = self.previous);
    }
}

pub(super) fn preserve_side_button_focus(
    context: &eframe::egui::Context,
) -> Option<SideButtonFocusGuard> {
    let side_buttons_only = context.input(|input| {
        let mut found = false;
        for event in &input.events {
            if let Event::PointerButton { button, .. } = event {
                if !matches!(
                    button,
                    eframe::egui::PointerButton::Extra1 | eframe::egui::PointerButton::Extra2
                ) {
                    return false;
                }
                found = true;
            }
        }
        found
    });
    if !side_buttons_only {
        return None;
    }
    // egui treats every mouse button as a click outside a focused input.
    // Side buttons belong to history, so preserve focus for this draw only.
    let previous = context.options_mut(|options| {
        std::mem::replace(
            &mut options.input_options.surrender_focus_on,
            eframe::egui::SurrenderFocusOn::Never,
        )
    });
    Some(SideButtonFocusGuard {
        context: context.clone(),
        previous,
    })
}

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

pub(super) fn focus_clicked_input(context: &eframe::egui::Context, id: Id) -> bool {
    // egui has already hit-tested pointer events using the previous pass's
    // clipped widget geometry. Route keys to the clicked input before drawing it.
    let clicked = context.read_response(id).is_some_and(|response| {
        response.enabled()
            && (response.clicked_by(eframe::egui::PointerButton::Primary)
                || response.is_pointer_button_down_on())
    });
    if clicked {
        context.memory_mut(|memory| {
            memory.request_focus(id);
            memory.move_focus(eframe::egui::FocusDirection::None);
        });
    }
    clicked
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
    fn side_button_clicks_preserve_text_focus_but_primary_and_mixed_clicks_release_it() {
        use eframe::egui::{
            CentralPanel, Context, PointerButton, Pos2, RawInput, SurrenderFocusOn, TextEdit,
        };

        for (buttons, keep_focus) in [
            (vec![PointerButton::Extra1], true),
            (vec![PointerButton::Extra2], true),
            (vec![PointerButton::Primary], false),
            (vec![PointerButton::Extra1, PointerButton::Primary], false),
        ] {
            let context = Context::default();
            let id = Id::new("side-button-input");
            let mut text = String::new();
            let _ = context.run(RawInput::default(), |context| {
                CentralPanel::default().show(context, |ui| {
                    ui.add(TextEdit::singleline(&mut text).id(id))
                        .request_focus();
                });
            });
            let pos = Pos2::new(400.0, 400.0);
            let mut events = vec![Event::PointerMoved(pos)];
            for button in buttons {
                for pressed in [true, false] {
                    events.push(Event::PointerButton {
                        pos,
                        button,
                        pressed,
                        modifiers: Modifiers::NONE,
                    });
                }
            }
            events.extend([key(Key::J, Modifiers::NONE, true), Event::Text("j".into())]);
            let _ = context.run(
                RawInput {
                    events,
                    ..Default::default()
                },
                |context| {
                    let guard = preserve_side_button_focus(context);
                    CentralPanel::default().show(context, |ui| {
                        ui.add(TextEdit::singleline(&mut text).id(id));
                    });
                    drop(guard);
                    assert_eq!(
                        context.options(|options| options.input_options.surrender_focus_on),
                        SurrenderFocusOn::Clicks
                    );
                },
            );
            assert_eq!(context.memory(|memory| memory.has_focus(id)), keep_focus);
            assert_eq!(text, if keep_focus { "j" } else { "" });
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
    fn clicked_input_gains_focus_before_same_frame_key_dispatch() {
        use eframe::egui::{CentralPanel, Context, PointerButton, RawInput, Rect, TextEdit};

        for release in [false, true] {
            let context = Context::default();
            let id = Id::new("mouse-input");
            let mut text = String::new();
            let mut rect = Rect::NOTHING;
            let _ = context.run(RawInput::default(), |context| {
                CentralPanel::default().show(context, |ui| {
                    rect = ui.add(TextEdit::singleline(&mut text).id(id)).rect;
                });
            });
            let pos = rect.center();
            let mut events = vec![
                Event::PointerMoved(pos),
                Event::PointerButton {
                    pos,
                    button: PointerButton::Primary,
                    pressed: true,
                    modifiers: Modifiers::NONE,
                },
            ];
            if release {
                events.push(Event::PointerButton {
                    pos,
                    button: PointerButton::Primary,
                    pressed: false,
                    modifiers: Modifiers::NONE,
                });
            }
            events.extend([key(Key::J, Modifiers::NONE, true), Event::Text("j".into())]);
            let _ = context.run(
                RawInput {
                    events,
                    ..Default::default()
                },
                |context| {
                    assert!(!context.memory(|memory| memory.has_focus(id)));
                    assert!(!focus_clicked_input(context, Id::new("unrelated-input")));
                    assert!(focus_clicked_input(context, id));
                    assert!(
                        context.memory(|memory| memory.has_focus(id)),
                        "route J to text input now"
                    );
                    CentralPanel::default().show(context, |ui| {
                        ui.add(TextEdit::singleline(&mut text).id(id));
                    });
                },
            );
            assert_eq!(text, "j");
        }
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
