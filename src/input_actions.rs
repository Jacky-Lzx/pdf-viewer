//! Compact, ordered snapshots for reader dispatch while egui events stay live.
use eframe::egui::{Event, InputState, Key, Modifiers, PointerButton};

pub const MAX_RETAINED_ACTIONS: usize = 256;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum KeyText {
    None,
    Character(char),
    Other,
}

impl KeyText {
    pub fn character(self) -> Option<char> {
        match self {
            Self::Character(character) => Some(character),
            _ => None,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReaderAction {
    Key {
        key: Key,
        modifiers: Modifiers,
        text: KeyText,
    },
    Copy,
    History {
        forward: bool,
    },
}

pub fn reader_actions(input: &InputState) -> impl Iterator<Item = ReaderAction> + '_ {
    input
        .events
        .iter()
        .enumerate()
        .filter_map(|(index, event)| match event {
            Event::Key {
                key,
                modifiers,
                pressed: true,
                ..
            } => {
                // Literal text distinguishes layout-dependent punctuation from its
                // physical digit/letter fallback. Paste and later text are not keys.
                let text = match input.events.get(index + 1) {
                    Some(Event::Text(text)) => {
                        let mut characters = text.chars();
                        match (characters.next(), characters.next()) {
                            (Some(character), None) => KeyText::Character(character),
                            _ => KeyText::Other,
                        }
                    }
                    _ => KeyText::None,
                };
                Some(ReaderAction::Key {
                    key: *key,
                    modifiers: *modifiers,
                    text,
                })
            }
            Event::Copy => Some(ReaderAction::Copy),
            Event::PointerButton {
                button: button @ (PointerButton::Extra1 | PointerButton::Extra2),
                pressed: true,
                ..
            } => Some(ReaderAction::History {
                forward: *button == PointerButton::Extra2,
            }),
            _ => None,
        })
}

/// Keep ordinary input allocation-free after warmup without retaining a large
/// buffer for the lifetime of a window after an exceptional event burst.
pub fn recycle(actions: &mut Vec<ReaderAction>) {
    actions.clear();
    if actions.capacity() > MAX_RETAINED_ACTIONS {
        *actions = Vec::new();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mixed_actions_keep_order_and_modifiers_without_consuming_text_or_pointer_events() {
        let modifiers = Modifiers::CTRL | Modifiers::ALT;
        let pointer = |button, pressed| Event::PointerButton {
            pos: eframe::egui::pos2(20.0, 30.0),
            button,
            pressed,
            modifiers,
        };
        let mut input = InputState::default();
        input.events = vec![
            Event::Key {
                key: Key::J,
                physical_key: Some(Key::J),
                pressed: true,
                repeat: true,
                modifiers,
            },
            Event::Text("σ".into()),
            Event::Paste("G42".into()),
            pointer(PointerButton::Primary, true),
            Event::Copy,
            pointer(PointerButton::Extra1, true),
            pointer(PointerButton::Extra1, false),
            Event::Ime(eframe::egui::ImeEvent::Commit("草稿".into())),
            pointer(PointerButton::Extra2, true),
        ];
        let original = input.events.clone();
        assert_eq!(
            reader_actions(&input).collect::<Vec<_>>(),
            [
                ReaderAction::Key {
                    key: Key::J,
                    modifiers,
                    text: KeyText::Character('σ')
                },
                ReaderAction::Copy,
                ReaderAction::History { forward: false },
                ReaderAction::History { forward: true },
            ]
        );
        assert_eq!(
            input.events, original,
            "egui must retain its original editing and pointer events"
        );
    }
}
