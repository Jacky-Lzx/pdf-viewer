use super::*;
use crate::{app::Pagewing, navigation::Navigation, text::PageText};
use std::sync::Arc;

const SNIPPET: &str = "Straße 中文 😀 preview";
const EDITOR: &str = "pagewing-search-input";

struct Harness {
    _directory: tempfile::TempDir,
    context: egui::Context,
    app: Pagewing,
    owner: Id,
    draft: String,
    clicks: usize,
    size: egui::Vec2,
}

impl Harness {
    fn new() -> Self {
        let directory = tempfile::tempdir().unwrap();
        let context = egui::Context::default();
        let mut app =
            Pagewing::with_context(&context, None, None, directory.path().join("library.json"));
        app.nav = Navigation::new(4, 1);
        app.dimensions = vec![[600.0, 800.0]; 4];
        let mut harness = Self {
            _directory: directory,
            context,
            app,
            owner: Id::NULL,
            draft: "query 草稿".into(),
            clicks: 0,
            size: egui::vec2(860.0, 540.0),
        };
        harness.frame(Vec::new());
        harness
    }

    fn frame(&mut self, events: Vec<Event>) -> egui::FullOutput {
        let mut input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, self.size)),
            time: Some(self.context.input(|input| input.time) + 0.25),
            events,
            ..Default::default()
        };
        self.app.prepare_native_input(&self.context, &mut input);
        let context = self.context.clone();
        context.run(input, |context| {
            self.app.keys(context);
            egui::CentralPanel::default().show(context, |ui| {
                ui.add(egui::TextEdit::singleline(&mut self.draft).id(Id::new(EDITOR)));
                let response = ui.button("Search result");
                self.owner = response.id;
                self.clicks += usize::from(response.clicked());
                show(
                    &response,
                    response.id.with("snippet"),
                    "Copy snippet",
                    SNIPPET,
                    Target::Control,
                );
            });
        })
    }

    fn focus(&mut self, id: Id) {
        self.context.memory_mut(|memory| memory.request_focus(id));
        self.frame(Vec::new());
    }
}

fn key(key: Key, modifiers: Modifiers) -> Vec<Event> {
    [true, false]
        .map(|pressed| Event::Key {
            key,
            physical_key: None,
            pressed,
            repeat: false,
            modifiers,
        })
        .into()
}

fn copied(output: &egui::FullOutput) -> Vec<&str> {
    output
        .platform_output
        .commands
        .iter()
        .filter_map(|command| match command {
            egui::OutputCommand::CopyText(text) => Some(text.as_str()),
            _ => None,
        })
        .collect()
}

#[test]
fn focused_copy_respects_text_editing_and_preserves_the_pdf_selection() {
    let mut harness = Harness::new();
    let mut text = PageText::default();
    text.text = "PDF selection".into();
    harness.app.selection.set_text(Arc::new(text));
    harness.app.selection.select_all();
    harness.app.pan = egui::vec2(37.0, 53.0);
    harness.focus(harness.owner);
    let output = harness.frame(vec![Event::Copy]);
    assert_eq!(copied(&output), [SNIPPET]);
    assert_eq!(harness.app.nav.page, 1);
    assert_eq!(harness.app.pan, egui::vec2(37.0, 53.0));
    assert_eq!(harness.app.selection.selected_text(), Some("PDF selection"));
    assert_eq!(harness.clicks, 0);

    harness.focus(Id::new(EDITOR));
    harness.frame(key(Key::A, Modifiers::COMMAND));
    let output = harness.frame(vec![Event::Copy]);
    assert_eq!(copied(&output), [harness.draft.as_str()]);
    assert_eq!(harness.app.selection.selected_text(), Some("PDF selection"));

    harness
        .context
        .memory_mut(|memory| memory.surrender_focus(Id::new(EDITOR)));
    harness.frame(Vec::new());
    let output = harness.frame(vec![Event::Copy]);
    assert_eq!(copied(&output), ["PDF selection"]);
}

#[test]
fn keyboard_menu_sequences_copy_cancel_restore_focus_and_survive_resize() {
    let mut harness = Harness::new();
    harness.focus(harness.owner);
    let mut events = key(Key::F10, Modifiers::SHIFT);
    events.extend(key(Key::Enter, Modifiers::NONE));
    let output = harness.frame(events);
    assert!(copied(&output).is_empty());
    assert!(control_menu_open(&harness.context));
    assert_eq!(harness.clicks, 0);
    let output = harness.frame(Vec::new());
    assert_eq!(copied(&output), [SNIPPET]);
    assert!(!egui::Popup::is_any_open(&harness.context));
    assert_eq!(
        harness.context.memory(|memory| memory.focused()),
        Some(harness.owner)
    );
    assert_eq!(harness.clicks, 0);

    harness.frame(key(Key::F10, Modifiers::SHIFT));
    for size in [egui::vec2(240.0, 240.0), egui::vec2(860.0, 540.0)] {
        harness.size = size;
        harness.frame(Vec::new());
        let output = harness.frame(Vec::new());
        let action = output
            .shapes
            .iter()
            .find_map(|shape| match &shape.shape {
                egui::Shape::Text(text) if text.galley.text() == "Copy snippet" => {
                    let rect = egui::Rect::from_min_size(text.pos, text.galley.size());
                    shape.clip_rect.contains_rect(rect).then_some(rect)
                }
                _ => None,
            })
            .expect("the keyboard action stays visible without a pointer position");
        assert!(egui::Rect::from_min_size(egui::Pos2::ZERO, size).contains_rect(action));
    }
    let mut events = key(Key::Escape, Modifiers::NONE);
    events.push(Event::Copy);
    let output = harness.frame(events);
    assert!(copied(&output).is_empty());
    assert!(!egui::Popup::is_any_open(&harness.context));
    let output = harness.frame(Vec::new());
    assert_eq!(copied(&output), [SNIPPET]);
    assert_eq!(harness.clicks, 0);
    assert_eq!(harness.app.nav.page, 1);
}

#[test]
fn context_menu_key_only_overrides_reader_bindings_on_its_focused_owner() {
    let mut harness = Harness::new();
    harness.app.library.shortcuts.overrides.insert(
        "next_page".into(),
        vec![crate::settings::Binding::parse_config("Shift+F10").unwrap()],
    );
    harness.app.library.shortcuts.validate().unwrap();
    harness.focus(harness.owner);
    harness.frame(key(Key::F10, Modifiers::SHIFT));
    assert!(control_menu_open(&harness.context));
    assert_eq!(harness.app.nav.page, 1);
    harness.frame(key(Key::Escape, Modifiers::NONE));
    harness
        .context
        .memory_mut(|memory| memory.surrender_focus(harness.owner));
    harness.frame(Vec::new());
    harness.frame(key(Key::F10, Modifiers::SHIFT));
    assert!(!egui::Popup::is_any_open(&harness.context));
    assert_eq!(harness.app.nav.page, 2);
}

#[test]
fn clicking_an_editor_dismisses_the_keyboard_menu_without_reclaiming_focus() {
    let mut harness = Harness::new();
    harness.focus(harness.owner);
    harness.frame(key(Key::F10, Modifiers::SHIFT));
    harness.frame(Vec::new());
    let position = harness
        .context
        .read_response(Id::new(EDITOR))
        .unwrap()
        .rect
        .center();
    harness.frame(vec![Event::PointerMoved(position)]);
    for pressed in [true, false] {
        harness.frame(vec![Event::PointerButton {
            pos: position,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: Modifiers::NONE,
        }]);
    }
    assert!(!egui::Popup::is_any_open(&harness.context));
    assert_eq!(
        harness.context.memory(|memory| memory.focused()),
        Some(Id::new(EDITOR))
    );
    let previous = harness.draft.clone();
    harness.frame(vec![Event::Text("x".into())]);
    assert_ne!(harness.draft, previous);
    assert_eq!(harness.clicks, 0);
}
