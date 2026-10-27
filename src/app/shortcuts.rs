use eframe::egui::{Event, Id, InputState, Key, Modifiers, Ui};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum InputButton {
    Help,
    Find,
    Page,
    ReloadChanged,
    DismissChange,
}

#[derive(Default)]
pub(super) struct InputButtons {
    pub help: Option<Id>,
    pub find: Option<Id>,
    pub page: Option<Id>,
    pub reload_changed: Option<Id>,
    pub dismiss_change: Option<Id>,
}

impl InputButtons {
    pub fn clicked(&self, context: &eframe::egui::Context) -> Option<InputButton> {
        for (id, action) in [
            (self.help, InputButton::Help),
            (self.find, InputButton::Find),
            (self.page, InputButton::Page),
            (self.reload_changed, InputButton::ReloadChanged),
            (self.dismiss_change, InputButton::DismissChange),
        ] {
            let Some(response) = id.and_then(|id| context.read_response(id)) else {
                continue;
            };
            if response.clicked() {
                if !response.clicked_by(eframe::egui::PointerButton::Primary) {
                    // Activation keys belong to the button, not reader commands
                    // or the newly opened input.
                    context.input_mut(|input| {
                        let activation = input.events.iter().find_map(|event| match event {
                            Event::Key {
                                key: key @ (Key::Enter | Key::Space),
                                pressed: true,
                                modifiers,
                                ..
                            } => Some((*key, *modifiers)),
                            _ => None,
                        });
                        if let Some((key, modifiers)) = activation {
                            consume_text_shortcut(input, key, modifiers);
                        }
                    });
                }
                return Some(action);
            }
        }
        None
    }
}

#[derive(Debug, PartialEq, Eq)]
pub(super) enum ZoomAction {
    In,
    Out,
    ActualSize,
}

pub(super) fn zoom_action(key: Key, modifiers: Modifiers) -> Option<ZoomAction> {
    if !modifiers.command || modifiers.alt {
        return None;
    }
    match key {
        Key::Plus | Key::Equals => Some(ZoomAction::In),
        Key::Minus => Some(ZoomAction::Out),
        Key::Num0 if !modifiers.shift => Some(ZoomAction::ActualSize),
        _ => None,
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum KeyText {
    None,
    Character(char),
    Other,
}

impl KeyText {
    pub(super) fn character(self) -> Option<char> {
        match self {
            Self::Character(character) => Some(character),
            _ => None,
        }
    }
}

pub(super) fn reader_zoom_action(key: Key, shift: bool, text: KeyText) -> Option<ZoomAction> {
    match text {
        KeyText::Character('+' | '=') => Some(ZoomAction::In),
        KeyText::Character('-' | '−') => Some(ZoomAction::Out),
        KeyText::None => match key {
            Key::Plus | Key::Equals => Some(ZoomAction::In),
            Key::Minus if !shift => Some(ZoomAction::Out),
            _ => None,
        },
        _ => None,
    }
}

pub(super) fn count_digit(key: Key, shift: bool, text: KeyText) -> Option<char> {
    match text {
        KeyText::Character(character) => character.is_ascii_digit().then_some(character),
        KeyText::None if !shift => match key {
            Key::Num0 => Some('0'),
            Key::Num1 => Some('1'),
            Key::Num2 => Some('2'),
            Key::Num3 => Some('3'),
            Key::Num4 => Some('4'),
            Key::Num5 => Some('5'),
            Key::Num6 => Some('6'),
            Key::Num7 => Some('7'),
            Key::Num8 => Some('8'),
            Key::Num9 => Some('9'),
            _ => None,
        },
        _ => None,
    }
}

pub(super) fn reader_actions(input: &InputState) -> impl Iterator<Item = (Event, KeyText)> + '_ {
    input
        .events
        .iter()
        .enumerate()
        .filter_map(|(index, event)| {
            let action = matches!(
                event,
                Event::Copy
                    | Event::Key { pressed: true, .. }
                    | Event::PointerButton {
                        button: eframe::egui::PointerButton::Extra1
                            | eframe::egui::PointerButton::Extra2,
                        pressed: true,
                        ..
                    }
            );
            if !action {
                return None;
            }
            // Some characters have no egui key and fall back to a physical
            // digit/letter. Keep this press's literal text to distinguish them.
            let text = if matches!(event, Event::Key { .. }) {
                match input.events.get(index + 1) {
                    Some(Event::Text(text)) => {
                        let mut characters = text.chars();
                        match (characters.next(), characters.next()) {
                            (Some(character), None) => KeyText::Character(character),
                            _ => KeyText::Other,
                        }
                    }
                    _ => KeyText::None,
                }
            } else {
                KeyText::None
            };
            Some((event.clone(), text))
        })
}

pub(super) struct PointerFocusGuard {
    context: eframe::egui::Context,
    previous: eframe::egui::SurrenderFocusOn,
}

impl Drop for PointerFocusGuard {
    fn drop(&mut self) {
        self.context
            .options_mut(|options| options.input_options.surrender_focus_on = self.previous);
    }
}

pub(super) fn preserve_side_button_focus(
    context: &eframe::egui::Context,
) -> Option<PointerFocusGuard> {
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
    Some(preserve_pointer_focus(context))
}

fn preserve_pointer_focus(context: &eframe::egui::Context) -> PointerFocusGuard {
    let previous = context.options_mut(|options| {
        std::mem::replace(
            &mut options.input_options.surrender_focus_on,
            eframe::egui::SurrenderFocusOn::Never,
        )
    });
    PointerFocusGuard {
        context: context.clone(),
        previous,
    }
}

fn focus_text_input(ui: &Ui, id: Id, requested: &mut bool, deferred: &mut Vec<Event>) -> bool {
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

pub(super) fn text_input(
    ui: &mut Ui,
    id: Id,
    requested: &mut bool,
    deferred: &mut Vec<Event>,
    field: eframe::egui::TextEdit<'_>,
) -> (eframe::egui::Response, bool) {
    let focused_now = focus_text_input(ui, id, requested, deferred);
    // Opening a field with a button must not let that outside click undo
    // the explicit focus request before this frame's text is processed.
    let _focus = focused_now.then(|| preserve_pointer_focus(ui.ctx()));
    (ui.add(field.id(id)), focused_now)
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

    #[test]
    fn button_opening_routes_text_and_consumes_keyboard_activation_without_leaking_focus_rules() {
        use eframe::egui::{
            CentralPanel, Context, PointerButton, RawInput, SurrenderFocusOn, TextEdit,
        };

        for (activation, submit) in [
            (None, false),
            (Some(Key::Enter), false),
            (Some(Key::Space), false),
            (None, true),
            (Some(Key::Enter), true),
            (Some(Key::Space), true),
        ] {
            let context = Context::default();
            let id = Id::new("button-input");
            let mut requested = false;
            let mut text = String::new();
            let mut deferred = Vec::new();
            let mut button = None;
            let mut pos = Default::default();
            let _ = context.run(RawInput::default(), |context| {
                CentralPanel::default().show(context, |ui| {
                    let response = ui.button("Find");
                    button = Some(response.id);
                    pos = response.rect.center();
                    text_input(
                        ui,
                        id,
                        &mut requested,
                        &mut deferred,
                        TextEdit::singleline(&mut text),
                    );
                });
            });
            let buttons = InputButtons {
                find: button,
                ..Default::default()
            };
            let click = || {
                [true, false].map(|pressed| Event::PointerButton {
                    pos,
                    button: PointerButton::Primary,
                    pressed,
                    modifiers: Modifiers::NONE,
                })
            };
            let mut events = if let Some(activation_key) = activation {
                context.memory_mut(|memory| memory.request_focus(button.unwrap()));
                vec![key(activation_key, Modifiers::NONE, true)]
            } else {
                let mut events = vec![Event::PointerMoved(pos)];
                events.extend(click());
                events
            };
            if activation == Some(Key::Space) {
                events.push(Event::Text(" ".into()));
            }
            events.extend([key(Key::J, Modifiers::NONE, true), Event::Text("j".into())]);
            if submit {
                events.push(key(Key::Enter, Modifiers::NONE, true));
            }
            let _ = context.run(
                RawInput {
                    events,
                    ..Default::default()
                },
                |context| {
                    assert_eq!(buttons.clicked(context), Some(InputButton::Find));
                    requested = true;
                    CentralPanel::default().show(context, |ui| {
                        let _ = ui.button("Find");
                        text_input(
                            ui,
                            id,
                            &mut requested,
                            &mut deferred,
                            TextEdit::singleline(&mut text),
                        );
                    });
                    assert_eq!(
                        context.options(|options| options.input_options.surrender_focus_on),
                        SurrenderFocusOn::Clicks
                    );
                },
            );
            assert_eq!(text, "j");
            assert_eq!(context.memory(|memory| memory.has_focus(id)), !submit);

            // An ordinary outside click on the next frame still releases focus.
            let mut events = vec![Event::PointerMoved(pos)];
            events.extend(click());
            events.push(Event::Text("k".into()));
            let _ = context.run(
                RawInput {
                    events,
                    ..Default::default()
                },
                |context| {
                    CentralPanel::default().show(context, |ui| {
                        let _ = ui.button("Find");
                        text_input(
                            ui,
                            id,
                            &mut requested,
                            &mut deferred,
                            TextEdit::singleline(&mut text),
                        );
                    });
                },
            );
            assert_eq!(text, "j");
            assert!(!context.memory(|memory| memory.has_focus(id)));
        }
    }

    #[test]
    fn zoom_shortcuts_use_platform_command_and_preserve_altgr_and_shifted_zero() {
        for platform in [
            Modifiers {
                ctrl: true,
                command: true,
                ..Modifiers::NONE
            },
            Modifiers {
                mac_cmd: true,
                command: true,
                ..Modifiers::NONE
            },
        ] {
            assert_eq!(zoom_action(Key::Equals, platform), Some(ZoomAction::In));
            assert_eq!(zoom_action(Key::Plus, platform), Some(ZoomAction::In));
            assert_eq!(zoom_action(Key::Minus, platform), Some(ZoomAction::Out));
            assert_eq!(
                zoom_action(Key::Num0, platform),
                Some(ZoomAction::ActualSize)
            );
            assert_eq!(
                zoom_action(
                    Key::Plus,
                    Modifiers {
                        shift: true,
                        ..platform
                    }
                ),
                Some(ZoomAction::In)
            );
            assert_eq!(
                zoom_action(
                    Key::Num0,
                    Modifiers {
                        shift: true,
                        ..platform
                    }
                ),
                None
            );
            assert_eq!(
                zoom_action(
                    Key::Equals,
                    Modifiers {
                        alt: true,
                        ..platform
                    }
                ),
                None
            );
        }
        assert_eq!(zoom_action(Key::Plus, Modifiers::NONE), None);
        assert_eq!(zoom_action(Key::Plus, Modifiers::CTRL), None);
    }

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
    fn punctuation_key_text_preserves_action_order_without_treating_paste_as_navigation() {
        for symbol in ['%', '$'] {
            for symbol_key in [Key::Num5, Key::Quote, Key::D] {
                let mut input = InputState::default();
                input.events = vec![
                    key(Key::Num5, Modifiers::NONE, true),
                    Event::Text("5".into()),
                    key(Key::Num5, Modifiers::NONE, false),
                    key(Key::Num0, Modifiers::NONE, true),
                    Event::Text("0".into()),
                    key(symbol_key, Modifiers::SHIFT, true),
                    Event::Text(symbol.to_string()),
                    key(symbol_key, Modifiers::SHIFT, false),
                    Event::Paste(format!("25{symbol}")),
                    Event::Text(symbol.to_string()),
                    key(Key::J, Modifiers::SHIFT, true),
                ];
                let actions: Vec<_> = reader_actions(&input).collect();
                assert_eq!(actions.len(), 4);
                assert_eq!(
                    actions
                        .iter()
                        .map(|(_, text)| text.character())
                        .collect::<Vec<_>>(),
                    [Some('5'), Some('0'), Some(symbol), None]
                );
                assert!(matches!(&actions[2].0, Event::Key { key, .. } if *key == symbol_key));
            }
        }
    }

    #[test]
    fn count_digits_use_literal_text_before_layout_fallbacks_and_ignore_other_text() {
        for (key, digit) in [
            (Key::Num0, '0'),
            (Key::Num1, '1'),
            (Key::Num2, '2'),
            (Key::Num3, '3'),
            (Key::Num4, '4'),
            (Key::Num5, '5'),
            (Key::Num6, '6'),
            (Key::Num7, '7'),
            (Key::Num8, '8'),
            (Key::Num9, '9'),
        ] {
            for shift in [false, true] {
                assert_eq!(
                    count_digit(key, shift, KeyText::Character(digit)),
                    Some(digit)
                );
                for character in "@#^&*()%$!５".chars() {
                    assert_eq!(count_digit(key, shift, KeyText::Character(character)), None);
                }
                assert_eq!(count_digit(key, shift, KeyText::Other), None);
                assert_eq!(
                    count_digit(key, shift, KeyText::None),
                    (!shift).then_some(digit)
                );
            }
        }
        let mut input = InputState::default();
        input.events = vec![
            key(Key::Num0, Modifiers::SHIFT, true),
            Event::Text("0".into()),
            key(Key::Num1, Modifiers::NONE, true),
            Event::Text("12".into()),
            key(Key::Num2, Modifiers::NONE, true),
            Event::Text(String::new()),
            key(Key::Num3, Modifiers::NONE, true),
        ];
        let digits: Vec<_> = reader_actions(&input)
            .map(|(event, text)| {
                let Event::Key { key, modifiers, .. } = event else {
                    unreachable!()
                };
                count_digit(key, modifiers.shift, text)
            })
            .collect();
        assert_eq!(digits, [Some('0'), None, None, Some('3')]);
    }

    #[test]
    fn reader_zoom_uses_literal_signs_and_does_not_treat_underscore_as_minus() {
        for key in [Key::Plus, Key::Equals, Key::Minus, Key::Num2, Key::J] {
            for shift in [false, true] {
                for sign in ['+', '='] {
                    assert_eq!(
                        reader_zoom_action(key, shift, KeyText::Character(sign)),
                        Some(ZoomAction::In)
                    );
                }
                for sign in ['-', '−'] {
                    assert_eq!(
                        reader_zoom_action(key, shift, KeyText::Character(sign)),
                        Some(ZoomAction::Out)
                    );
                }
                for character in "_@#^&*()3j＋".chars() {
                    assert_eq!(
                        reader_zoom_action(key, shift, KeyText::Character(character)),
                        None
                    );
                }
                assert_eq!(reader_zoom_action(key, shift, KeyText::Other), None);
                let fallback = match key {
                    Key::Plus | Key::Equals => Some(ZoomAction::In),
                    Key::Minus if !shift => Some(ZoomAction::Out),
                    _ => None,
                };
                assert_eq!(reader_zoom_action(key, shift, KeyText::None), fallback);
            }
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
