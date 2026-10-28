#[path = "support/allocations.rs"]
mod allocation_counter;
#[allow(dead_code, unused_imports)]
#[path = "../src/input_actions.rs"]
mod input_actions;

use eframe::egui::{self, Event, InputState, Key, Modifiers};
use input_actions::{KeyText, ReaderAction, reader_actions, recycle};
use std::{hint::black_box, mem::size_of, time::Instant};

fn key(key: Key, pressed: bool) -> Event {
    Event::Key {
        key,
        physical_key: Some(key),
        pressed,
        repeat: false,
        modifiers: Modifiers::NONE,
    }
}

fn keys(count: usize) -> Vec<Event> {
    (0..count)
        .flat_map(|_| {
            [
                key(Key::J, true),
                Event::Text("j".into()),
                key(Key::J, false),
            ]
        })
        .collect()
}

fn normalized(input: &InputState) -> Vec<ReaderAction> {
    previous_actions(input)
        .map(|(event, text)| match event {
            Event::Key { key, modifiers, .. } => ReaderAction::Key {
                key,
                modifiers,
                text,
            },
            Event::Copy => ReaderAction::Copy,
            Event::PointerButton { button, .. } => ReaderAction::History {
                forward: button == egui::PointerButton::Extra2,
            },
            _ => unreachable!(),
        })
        .collect()
}

fn rebuilt(input: &InputState) -> usize {
    let actions: Vec<_> = previous_actions(black_box(input)).collect();
    black_box(&actions).len()
}

fn buffered(input: &InputState, actions: &mut Vec<ReaderAction>) -> usize {
    actions.extend(reader_actions(black_box(input)));
    let count = black_box(&*actions).len();
    recycle(actions);
    count
}

fn main() {
    let passes = std::env::args()
        .nth(1)
        .and_then(|value| value.parse::<usize>().ok())
        .unwrap_or(9);
    assert!((1..=1000).contains(&passes));
    println!(
        "{passes} alternating passes, 2000 snapshots/sample; µs/snapshot including parsing, literal text classification, buffer allocation/recycling and output release. Excludes egui frame setup, command execution, PDF and GPU."
    );
    println!(
        "Record size: {} bytes for (Event, KeyText), {} bytes for ReaderAction; at most {} retained actions.",
        size_of::<(Event, KeyText)>(),
        size_of::<ReaderAction>(),
        input_actions::MAX_RETAINED_ACTIONS
    );
    for (name, events) in [
        ("idle", Vec::new()),
        (
            "pointer only",
            (0..64)
                .map(|x| Event::PointerMoved(egui::pos2(x as f32, 20.0)))
                .collect(),
        ),
        ("single key", keys(1)),
        (
            "mixed input",
            vec![
                key(Key::Num5, true),
                Event::Text("%".into()),
                key(Key::Num5, false),
                Event::Copy,
                Event::PointerButton {
                    pos: egui::pos2(50.0, 60.0),
                    button: egui::PointerButton::Extra1,
                    pressed: true,
                    modifiers: Modifiers::NONE,
                },
                key(Key::Slash, true),
                Event::Text("/".into()),
                Event::Paste("草稿📝".repeat(10000)),
                Event::Ime(egui::ImeEvent::Commit("输入".into())),
            ],
        ),
        ("32 keys", keys(32)),
        (
            "oversized burst",
            keys(input_actions::MAX_RETAINED_ACTIONS + 37),
        ),
    ] {
        let mut input = InputState::default();
        input.events = events;
        let expected = normalized(&input);
        assert_eq!(reader_actions(&input).collect::<Vec<_>>(), expected);
        let mut buffer = Vec::new();
        assert_eq!(buffered(&input, &mut buffer), expected.len());
        let old_allocations = allocation_counter::allocations(|| {
            black_box(rebuilt(&input));
        });
        let new_allocations = allocation_counter::allocations(|| {
            black_box(buffered(&input, &mut buffer));
        });
        if expected.len() <= input_actions::MAX_RETAINED_ACTIONS {
            assert_eq!(
                new_allocations,
                (0, 0),
                "warm ordinary snapshots must not allocate"
            );
        } else {
            assert_eq!(
                buffer.capacity(),
                0,
                "a large burst must not leave its storage resident"
            );
        }
        let mut times = [Vec::new(), Vec::new()];
        for pass in 0..passes {
            for order in 0..2 {
                let mode = (pass + order) % 2;
                let start = Instant::now();
                for _ in 0..2000 {
                    let count = if mode == 0 {
                        rebuilt(&input)
                    } else {
                        buffered(&input, &mut buffer)
                    };
                    black_box(count);
                }
                times[mode].push(start.elapsed().as_secs_f64() * 1e6 / 2000.0);
            }
        }
        let [old, new] = times.map(|mut samples| {
            samples.sort_by(f64::total_cmp);
            samples[passes / 2]
        });
        assert!(buffer.capacity() <= input_actions::MAX_RETAINED_ACTIONS);
        println!(
            "  {name}, {} actions: {old:.4} -> {new:.4} µs; (alloc,realloc) {old_allocations:?} -> {new_allocations:?}; {} retained bytes",
            expected.len(),
            buffer.capacity() * size_of::<ReaderAction>()
        );
    }
}

// Captured Event-snapshot implementation from 36f5400.
fn previous_actions(input: &InputState) -> impl Iterator<Item = (Event, KeyText)> + '_ {
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
