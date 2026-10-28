use super::*;
use crate::{annotations::Comment, destination::Destination, outline::OutlineEntry};
use egui::{Context, Event, Modifiers};

#[derive(Clone, Copy, Debug)]
enum Filter {
    Recent,
    Chapters,
    Comments,
}

impl Filter {
    fn open(self, app: &mut Pagewing, context: &Context) {
        match self {
            Self::Recent => {
                app.recent_picker = Some(crate::recent::RecentPicker::new(
                    (0..2).map(|index| format!("book-{index}.pdf").into()),
                ));
            }
            Self::Chapters => {
                app.outline.set_entries(
                    (0..2)
                        .map(|page| OutlineEntry {
                            title: format!("book-{page}"),
                            destination: Some(Destination::page(page)),
                            depth: 0,
                        })
                        .collect(),
                );
                app.focus_contents();
            }
            Self::Comments => {
                app.comments.set_comments(crate::annotations::Comments {
                    items: (0..2)
                        .map(|index| Comment {
                            page: 0,
                            slot: Some(index),
                            parent: None,
                            review: Default::default(),
                            marked: false,
                            states: Vec::new(),
                            kind: "Text".into(),
                            contents: format!("book-{index}"),
                            author: "Reader".into(),
                            rects: Vec::new(),
                            has_appearance: false,
                            truncated: false,
                            locked: false,
                        })
                        .collect(),
                    limited: false,
                });
                app.comments.open = true;
                context.memory_mut(|memory| memory.request_focus(self.input_id()));
            }
        }
    }

    fn input_id(self) -> egui::Id {
        egui::Id::new(match self {
            Self::Recent => "pagewing-recent-input",
            Self::Chapters => "pagewing-outline-input",
            Self::Comments => "pagewing-comment-filter",
        })
    }

    fn query(self, app: &Pagewing) -> &str {
        match self {
            Self::Recent => &app.recent_picker.as_ref().unwrap().query,
            Self::Chapters => &app.outline.query,
            Self::Comments => &app.comments.view.filter.query,
        }
    }

    fn selected(self, app: &Pagewing) -> Option<usize> {
        match self {
            Self::Recent => Some(app.recent_picker.as_ref().unwrap().selected),
            Self::Chapters => app.outline.selected(),
            Self::Comments => app.comments.view.selected(),
        }
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

fn frame(app: &mut Pagewing, context: &Context, mut events: Vec<Event>) -> egui::FullOutput {
    let mut deferred = std::mem::take(&mut app.deferred_input);
    deferred.append(&mut events);
    context.run(
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(1180.0, 820.0),
            )),
            events: deferred,
            ..Default::default()
        },
        |context| {
            app.keys(context);
            app.toolbar(context);
            app.status(context);
            app.comments_sidebar(context);
            app.sidebar(context);
            app.recent_dialog(context);
            egui::CentralPanel::default().show(context, |_| {});
        },
    )
}

#[test]
fn filtered_lists_keep_shift_arrows_for_text_selection() {
    for filter in [Filter::Recent, Filter::Chapters, Filter::Comments] {
        let directory = tempfile::tempdir().unwrap();
        let context = Context::default();
        let mut app =
            Pagewing::with_context(&context, None, None, directory.path().join("library.json"));
        app.nav = crate::navigation::Navigation::new(2, 0);
        app.dimensions = vec![[600.0, 800.0]; 2];
        filter.open(&mut app, &context);
        for _ in 0..4 {
            frame(&mut app, &context, Vec::new());
        }
        frame(&mut app, &context, vec![Event::Paste("book".into())]);
        frame(&mut app, &context, key(Key::ArrowDown, Modifiers::NONE));
        assert_eq!(filter.selected(&app), Some(1));
        for (edge, arrow) in [(Key::End, Key::ArrowUp), (Key::Home, Key::ArrowDown)] {
            let mut events = key(edge, Modifiers::NONE);
            events.extend(key(arrow, Modifiers::SHIFT));
            events.push(Event::Copy);
            let output = frame(&mut app, &context, events);
            assert!(
                output.platform_output.commands.iter().any(|command| {
                    matches!(command, egui::OutputCommand::CopyText(text) if text == "book")
                }),
                "{filter:?}: Shift+{arrow:?} must select the filter text"
            );
            assert_eq!(filter.selected(&app), Some(1), "{filter:?}");
            assert_eq!(filter.query(&app), "book");
            assert!(context.memory(|memory| memory.has_focus(filter.input_id())));
            assert_eq!(app.nav.page, 0);
        }
    }
}

#[test]
fn filtered_lists_preserve_text_generated_by_ctrl_alt_j_and_k() {
    for filter in [Filter::Recent, Filter::Chapters, Filter::Comments] {
        let directory = tempfile::tempdir().unwrap();
        let context = Context::default();
        let mut app =
            Pagewing::with_context(&context, None, None, directory.path().join("library.json"));
        app.nav = crate::navigation::Navigation::new(2, 0);
        app.dimensions = vec![[600.0, 800.0]; 2];
        filter.open(&mut app, &context);
        for _ in 0..4 {
            frame(&mut app, &context, Vec::new());
        }
        frame(&mut app, &context, vec![Event::Paste("book".into())]);
        for (letter, text, expected) in [(Key::J, "中", "book中"), (Key::K, "文", "book中文")]
        {
            let mut events = key(letter, Modifiers::CTRL | Modifiers::ALT);
            events.insert(1, Event::Text(text.into()));
            frame(&mut app, &context, events);
            assert_eq!(
                filter.query(&app),
                expected,
                "{filter:?}: Ctrl+Alt+{letter:?}"
            );
            assert!(context.memory(|memory| memory.has_focus(filter.input_id())));
            assert_eq!(app.nav.page, 0);
        }
    }
}
