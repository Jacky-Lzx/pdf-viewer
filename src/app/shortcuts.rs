use eframe::egui::{Event, InputState, Key, Modifiers};

pub(super) fn consume_text_shortcut(input: &mut InputState, key: Key, modifiers: Modifiers) {
    // Option shortcuts can also generate a text event on macOS. Preserve text
    // typed before/after the shortcut, even when a busy frame batches both.
    let mut shortcut_text = false;
    input.events.retain(|event| match event {
        Event::Key {
            key: event_key,
            modifiers: event_modifiers,
            pressed,
            ..
        } => {
            shortcut_text = *pressed && *event_key == key && *event_modifiers == modifiers;
            true
        }
        Event::Text(_) => !shortcut_text,
        _ => true,
    });
    input.consume_key(modifiers, key);
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
