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

fn frame(app: &mut Pagewing, context: &Context, events: Vec<Event>) -> egui::FullOutput {
    sized_frame(app, context, egui::vec2(1180.0, 820.0), events)
}

fn sized_frame(
    app: &mut Pagewing,
    context: &Context,
    size: egui::Vec2,
    mut events: Vec<Event>,
) -> egui::FullOutput {
    let mut deferred = std::mem::take(&mut app.deferred_input);
    deferred.append(&mut events);
    context.run(
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, size)),
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
fn chapter_page_navigation_uses_current_layout_and_visible_tree_rows() {
    let directory = tempfile::tempdir().unwrap();
    let context = Context::default();
    let mut app =
        Pagewing::with_context(&context, None, None, directory.path().join("library.json"));
    app.nav = crate::navigation::Navigation::new(80, 0);
    app.dimensions = vec![[600.0, 800.0]; 80];
    app.outline.set_entries(
        (0..80)
            .map(|page| OutlineEntry {
                title: format!("Chapter {page:03}"),
                destination: Some(Destination::page(page)),
                depth: 0,
            })
            .collect(),
    );
    app.focus_contents();
    for (size, rows) in [
        (egui::vec2(1180.0, 820.0), 7),
        (egui::vec2(240.0, 240.0), 2),
        (egui::vec2(1180.0, 820.0), 7),
    ] {
        let mut events = key(Key::Home, Modifiers::COMMAND);
        events.extend(key(Key::PageDown, Modifiers::NONE));
        sized_frame(&mut app, &context, size, events);
        let mut output = egui::FullOutput::default();
        for _ in 0..6 {
            output = sized_frame(&mut app, &context, size, Vec::new());
        }
        assert_eq!(app.outline.selected(), Some(rows), "{size:?}");
        let (clip, bounds) = output
            .shapes
            .iter()
            .find_map(|shape| match &shape.shape {
                egui::Shape::Text(text) if text.galley.text() == format!("Chapter {rows:03}") => {
                    Some((
                        shape.clip_rect,
                        egui::Rect::from_min_size(text.pos, text.galley.size()),
                    ))
                }
                _ => None,
            })
            .expect("the selected chapter must be painted");
        assert!(
            clip.contains_rect(bounds),
            "{size:?}: {bounds:?} clipped by {clip:?}"
        );
        assert!(context.memory(|memory| memory.has_focus(Filter::Chapters.input_id())));
        assert_eq!(app.nav.page, 0);
        sized_frame(&mut app, &context, size, key(Key::PageUp, Modifiers::NONE));
        assert_eq!(app.outline.selected(), Some(0));
    }
    let style = context.style();
    context.style_mut(|style| {
        style
            .text_styles
            .get_mut(&egui::TextStyle::Button)
            .unwrap()
            .size = 28.0;
    });
    let mut events = key(Key::Home, Modifiers::COMMAND);
    events.extend(key(Key::PageDown, Modifiers::NONE));
    frame(&mut app, &context, events);
    assert!((1..7).contains(&app.outline.selected().unwrap()));
    context.set_style(style);
    for (boundary, page, arrow, selected) in [
        (Key::End, Key::PageDown, Key::ArrowUp, 78),
        (Key::Home, Key::PageUp, Key::ArrowDown, 1),
    ] {
        let mut events = key(boundary, Modifiers::COMMAND);
        events.extend(key(page, Modifiers::NONE));
        events.extend(key(arrow, Modifiers::NONE));
        frame(&mut app, &context, events);
        assert_eq!(app.outline.selected(), Some(selected));
    }
    app.outline.set_entries(
        (0..80)
            .map(|page| OutlineEntry {
                title: if page % 10 == 0 {
                    format!("Part {page}")
                } else {
                    format!("Chapter child {page}")
                },
                destination: (page % 10 != 0).then_some(Destination::page(page)),
                depth: usize::from(page % 10 != 0),
            })
            .collect(),
    );
    app.outline.fold_all(true);
    app.focus_contents();
    let mut events = key(Key::Home, Modifiers::COMMAND);
    events.extend(key(Key::PageDown, Modifiers::NONE));
    events.extend(key(Key::Enter, Modifiers::NONE));
    frame(&mut app, &context, events);
    assert_eq!(app.outline.selected(), Some(70));
    assert!(
        !app.outline.is_collapsed(70),
        "Enter expands the selected heading without a destination"
    );
    assert!(app.outline.is_collapsed(0));
    assert_eq!(app.nav.page, 0);
    let mut events = vec![Event::Paste("child".into())];
    events.extend(key(Key::End, Modifiers::COMMAND));
    events.extend(key(Key::PageUp, Modifiers::NONE));
    events.extend(key(Key::Enter, Modifiers::NONE));
    frame(&mut app, &context, events);
    assert_eq!(
        app.nav.page, 72,
        "filter, boundary, page and confirm use the new tree in order"
    );
    assert_eq!(app.outline.query, "child");
    app.focus_contents();
    let mut events = key(Key::A, Modifiers::COMMAND);
    events.push(Event::Paste("missing".into()));
    events.extend(key(Key::End, Modifiers::COMMAND));
    events.extend(key(Key::PageDown, Modifiers::NONE));
    events.extend(key(Key::Enter, Modifiers::NONE));
    frame(&mut app, &context, events);
    assert!(app.outline.selected().is_none());
    assert_eq!(app.nav.page, 72);
    assert!(context.memory(|memory| memory.has_focus(Filter::Chapters.input_id())));
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
