use super::*;
use crate::{navigation::Navigation, text::SearchHit};

fn frame(
    app: &mut Pagewing,
    context: &egui::Context,
    size: egui::Vec2,
    events: Vec<egui::Event>,
) -> egui::FullOutput {
    context.run(
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, size)),
            events,
            ..Default::default()
        },
        |context| {
            eframe::App::update(app, context, &mut eframe::Frame::_new_kittest());
        },
    )
}

fn settle(app: &mut Pagewing, context: &egui::Context, size: egui::Vec2) -> egui::FullOutput {
    let mut output = egui::FullOutput::default();
    for _ in 0..4 {
        output = frame(app, context, size, Vec::new());
    }
    output
}

fn key(key: egui::Key) -> Vec<egui::Event> {
    [true, false]
        .map(|pressed| egui::Event::Key {
            key,
            physical_key: None,
            pressed,
            repeat: false,
            modifiers: egui::Modifiers::NONE,
        })
        .into()
}

fn fixture() -> (tempfile::TempDir, egui::Context, Pagewing) {
    let root = tempfile::tempdir().unwrap();
    let context = egui::Context::default();
    let mut app = Pagewing::with_context(&context, None, None, root.path().join("library.json"));
    app.nav = Navigation::new(6, 2);
    app.dimensions = vec![[600.0, 800.0]; 6];
    app.find();
    settle(&mut app, &context, egui::vec2(1180.0, 820.0));
    (root, context, app)
}

#[test]
fn native_history_open_navigation_and_confirmation_recall_without_following_a_result() {
    for size in [egui::vec2(240.0, 240.0), egui::vec2(1180.0, 820.0)] {
        let (_root, context, mut app) = fixture();
        app.query_history.remember("older Straße");
        app.query_history.remember("newer");
        app.query = "retained draft".into();
        let options = app.library.search_options;
        settle(&mut app, &context, size);
        context
            .memory_mut(|memory| memory.request_focus(app.input_buttons.search_history.unwrap()));
        settle(&mut app, &context, size);
        let mut events = [egui::Key::Enter, egui::Key::ArrowDown, egui::Key::Enter]
            .into_iter()
            .flat_map(key)
            .collect();
        for _ in 0..12 {
            let mut input = egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, size)),
                events: std::mem::take(&mut events),
                ..Default::default()
            };
            eframe::App::raw_input_hook(&mut app, &context, &mut input);
            let _ = context.run(input, |context| {
                eframe::App::update(&mut app, context, &mut eframe::Frame::_new_kittest());
            });
        }
        assert_eq!(app.query, "older Straße");
        assert_eq!(app.library.search_options, options);
        assert_eq!(app.nav.page, 2);
        assert!(!egui::Popup::is_any_open(&context));
        assert!(context.memory(|memory| memory.has_focus(egui::Id::new("pagewing-search-input"))));
    }
}

fn click(app: &mut Pagewing, context: &egui::Context, size: egui::Vec2, label: &str) {
    let output = settle(app, context, size);
    let pos = output
        .shapes
        .iter()
        .rev()
        .find_map(|shape| match &shape.shape {
            egui::Shape::Text(text) if text.galley.text() == label => {
                let rect = egui::Rect::from_min_size(text.pos, text.galley.size());
                assert!(
                    shape.clip_rect.contains_rect(rect),
                    "{label} is clipped at {size:?}"
                );
                Some(rect.center())
            }
            _ => None,
        })
        .unwrap_or_else(|| panic!("missing {label}"));
    for pressed in [true, false] {
        frame(
            app,
            context,
            size,
            vec![
                egui::Event::PointerMoved(pos),
                egui::Event::PointerButton {
                    pos,
                    button: egui::PointerButton::Primary,
                    pressed,
                    modifiers: egui::Modifiers::NONE,
                },
            ],
        );
    }
}

#[test]
fn confirming_records_final_text_and_recall_resets_results_without_moving_the_view() {
    let (_root, context, mut app) = fixture();
    let size = egui::vec2(1180.0, 820.0);
    let mut events = vec![egui::Event::Text("Straße 📝".into())];
    events.extend(key(egui::Key::Enter));
    frame(&mut app, &context, size, events);
    assert_eq!(
        app.query_history.queries().collect::<Vec<_>>(),
        ["Straße 📝"]
    );
    app.query_history.remember("second query");
    app.library.search_options = crate::text::SearchOptions {
        match_case: true,
        whole_word: true,
    };
    let options = app.library.search_options;
    app.hits = vec![SearchHit {
        page: 0,
        rects: Vec::new(),
        snippet: String::new(),
        snippet_match: 0..0,
    }];
    app.hit_index = Some(0);
    app.search_complete = true;
    app.search_due = None;
    let page = app.nav.page;
    let revision = app.revision;
    click(&mut app, &context, size, "History");
    click(&mut app, &context, size, "Straße 📝");
    settle(&mut app, &context, size);
    assert_eq!(app.query, "Straße 📝");
    assert_eq!(
        app.query_history.queries().collect::<Vec<_>>(),
        ["Straße 📝", "second query"]
    );
    assert_eq!(app.library.search_options, options);
    assert_eq!(app.nav.page, page);
    assert!(app.hits.is_empty() && app.hit_index.is_none());
    assert!(app.revision > revision && app.search_navigation.first().is_none());
    assert!(!egui::Popup::is_any_open(&context));
    assert!(context.memory(|memory| memory.has_focus(egui::Id::new("pagewing-search-input"))));
    let output = frame(&mut app, &context, size, vec![egui::Event::Copy]);
    assert!(
        output.platform_output.commands.iter().any(|command| {
            matches!(command, egui::OutputCommand::CopyText(text) if text == "Straße 📝")
        }),
        "the recalled Unicode query is fully selected"
    );
    frame(
        &mut app,
        &context,
        size,
        vec![egui::Event::Text("replacement".into())],
    );
    assert_eq!(app.query, "replacement");
    assert_eq!(
        app.query_history.queries().len(),
        2,
        "live typing is not saved as history"
    );
}

#[test]
fn keyboard_menu_activation_and_escape_do_not_follow_a_search_result() {
    let (_root, context, mut app) = fixture();
    let size = egui::vec2(1180.0, 820.0);
    app.query_history.remember("needle");
    app.query = "needle".into();
    app.hits = vec![SearchHit {
        page: 0,
        rects: Vec::new(),
        snippet: String::new(),
        snippet_match: 0..0,
    }];
    app.search_complete = true;
    settle(&mut app, &context, size);
    let id = app.input_buttons.search_history.unwrap();
    context.memory_mut(|memory| memory.request_focus(id));
    frame(&mut app, &context, size, key(egui::Key::Enter));
    assert!(egui::Popup::is_any_open(&context));
    assert_eq!(app.nav.page, 2);
    frame(&mut app, &context, size, key(egui::Key::Escape));
    assert!(!egui::Popup::is_any_open(&context));
    assert!(app.search_open);
    assert_eq!(app.nav.page, 2);
}

#[test]
fn keyboard_can_recall_a_query_from_the_minimum_size_menu() {
    let (_root, context, mut app) = fixture();
    let size = egui::vec2(240.0, 240.0);
    app.query_history.remember("older");
    app.query_history.remember("newer");
    app.query = "draft".into();
    settle(&mut app, &context, size);
    let id = app.input_buttons.search_history.unwrap();
    context.memory_mut(|memory| memory.request_focus(id));
    settle(&mut app, &context, size);
    frame(&mut app, &context, size, key(egui::Key::Enter));
    frame(&mut app, &context, size, key(egui::Key::ArrowDown));
    frame(&mut app, &context, size, key(egui::Key::Enter));
    settle(&mut app, &context, size);
    assert_eq!(app.query, "older");
    assert_eq!(app.nav.page, 2);
    assert!(!egui::Popup::is_any_open(&context));
}

#[test]
fn clicking_history_owns_later_reader_keys_in_the_same_frame() {
    let (_root, context, mut app) = fixture();
    let size = egui::vec2(1180.0, 820.0);
    app.query_history.remember("needle");
    settle(&mut app, &context, size);
    let id = app.input_buttons.search_history.unwrap();
    let pos = context.read_response(id).unwrap().rect.center();
    let mut events = vec![egui::Event::PointerMoved(pos)];
    for pressed in [true, false] {
        events.push(egui::Event::PointerButton {
            pos,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: egui::Modifiers::NONE,
        });
    }
    let mut turn_page = key(egui::Key::J);
    for event in &mut turn_page {
        if let egui::Event::Key { modifiers, .. } = event {
            *modifiers = egui::Modifiers::SHIFT;
        }
    }
    events.extend(turn_page);
    frame(&mut app, &context, size, events);
    assert_eq!(
        app.nav.page, 2,
        "a menu-opening click must own subsequent reader keys"
    );
    assert!(egui::Popup::is_any_open(&context));
}

#[test]
fn escape_in_the_menu_opening_click_batch_closes_only_the_menu() {
    let (_root, context, mut app) = fixture();
    let size = egui::vec2(1180.0, 820.0);
    app.query_history.remember("needle");
    app.query = "draft".into();
    settle(&mut app, &context, size);
    let id = app.input_buttons.search_history.unwrap();
    let pos = context.read_response(id).unwrap().rect.center();
    let mut events = vec![egui::Event::PointerMoved(pos)];
    for pressed in [true, false] {
        events.push(egui::Event::PointerButton {
            pos,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: egui::Modifiers::NONE,
        });
    }
    events.extend(key(egui::Key::Escape));
    frame(&mut app, &context, size, events);
    settle(&mut app, &context, size);
    assert!(!egui::Popup::is_any_open(&context));
    assert!(app.search_open, "Escape belongs to the newly opened menu");
    assert_eq!(app.query, "draft");
    assert_eq!(app.nav.page, 2);
    click(&mut app, &context, size, "History");
    assert!(egui::Popup::is_any_open(&context));
    click(&mut app, &context, size, "History");
    assert!(!egui::Popup::is_any_open(&context));
}

#[test]
fn cleared_history_stays_empty_when_a_pending_search_result_arrives() {
    let (_root, context, mut app) = fixture();
    let size = egui::vec2(1180.0, 820.0);
    app.query = "needle".into();
    app.search_complete = false;
    app.next_hit(false);
    assert_eq!(app.query_history.queries().collect::<Vec<_>>(), ["needle"]);
    assert!(app.search_navigation.first().is_some());
    click(&mut app, &context, size, "History");
    click(&mut app, &context, size, "Clear history");
    assert!(app.query_history.is_empty());
    app.hits = vec![SearchHit {
        page: 0,
        rects: Vec::new(),
        snippet: String::new(),
        snippet_match: 0..0,
    }];
    app.scanned = app.nav.count;
    app.search_complete = true;
    app.search_due = None;
    app.resume_search_navigation();
    assert_eq!(
        app.nav.page, 0,
        "clearing history preserves pending navigation"
    );
    assert!(
        app.query_history.is_empty(),
        "background results must not recreate cleared history"
    );
    app.select_hit(0);
    assert_eq!(
        app.query_history.queries().collect::<Vec<_>>(),
        ["needle"],
        "a new explicit result selection records the query again"
    );
}

#[test]
fn arrow_navigation_reveals_long_history_entries_instead_of_leaving_focus_clipped() {
    let (_root, context, mut app) = fixture();
    let size = egui::vec2(240.0, 240.0);
    for index in 0..16 {
        app.query_history.remember(&format!("query {index:02}"));
    }
    settle(&mut app, &context, size);
    let id = app.input_buttons.search_history.unwrap();
    context.memory_mut(|memory| memory.request_focus(id));
    settle(&mut app, &context, size);
    frame(&mut app, &context, size, key(egui::Key::Enter));
    for index in 0..16 {
        settle(&mut app, &context, size);
        let focused = context.memory(|memory| memory.focused()).unwrap();
        let response = context.read_response(focused).unwrap();
        assert!(
            response.interact_rect.contains_rect(response.rect),
            "entry {index}: {:?} clipped by {:?}",
            response.rect,
            response.interact_rect
        );
        if index < 15 {
            frame(&mut app, &context, size, key(egui::Key::ArrowDown));
        }
    }
    frame(&mut app, &context, size, key(egui::Key::Enter));
    settle(&mut app, &context, size);
    assert_eq!(
        app.query, "query 00",
        "all 16 entries must be reachable by direction keys"
    );
}

#[test]
fn history_tab_wrap_and_resizing_retain_the_same_query_instead_of_background_controls() {
    let (_root, context, mut app) = fixture();
    let desktop = egui::vec2(1180.0, 820.0);
    let small = egui::vec2(240.0, 240.0);
    app.query_history.remember("older");
    app.query_history.remember("newer");
    app.query = "draft".into();
    settle(&mut app, &context, desktop);
    context.memory_mut(|memory| memory.request_focus(app.input_buttons.search_history.unwrap()));
    frame(&mut app, &context, desktop, key(egui::Key::Enter));
    settle(&mut app, &context, desktop);
    let first = context.memory(|memory| memory.focused()).unwrap();
    frame(&mut app, &context, desktop, key(egui::Key::ArrowUp));
    settle(&mut app, &context, desktop);
    let clear = context.memory(|memory| memory.focused()).unwrap();
    assert_ne!(first, clear);
    frame(&mut app, &context, desktop, key(egui::Key::Tab));
    settle(&mut app, &context, desktop);
    assert_eq!(context.memory(|memory| memory.focused()), Some(first));
    let mut reverse = key(egui::Key::Tab);
    for event in &mut reverse {
        if let egui::Event::Key { modifiers, .. } = event {
            *modifiers = egui::Modifiers::SHIFT;
        }
    }
    frame(&mut app, &context, desktop, reverse);
    settle(&mut app, &context, desktop);
    assert_eq!(context.memory(|memory| memory.focused()), Some(clear));
    frame(&mut app, &context, desktop, key(egui::Key::ArrowUp));
    settle(&mut app, &context, desktop);
    let older = context.memory(|memory| memory.focused()).unwrap();
    for size in [small, desktop, small] {
        settle(&mut app, &context, size);
        assert_eq!(context.memory(|memory| memory.focused()), Some(older));
        let response = context.read_response(older).unwrap();
        assert!(response.interact_rect.contains_rect(response.rect));
        assert!(context.content_rect().contains_rect(response.rect));
    }
    frame(&mut app, &context, small, key(egui::Key::Enter));
    assert_eq!(app.query, "older");
    assert_eq!(app.nav.page, 2);
    assert!(!egui::Popup::is_any_open(&context));
}

#[test]
fn clear_action_stays_visible_at_minimum_size_and_history_survives_document_switches() {
    let (_root, context, mut app) = fixture();
    for index in 0..16 {
        app.query_history.remember(&format!("saved search {index}"));
    }
    app.clear_document(&context);
    assert_eq!(app.query_history.queries().len(), 16);
    app.nav = Navigation::new(6, 3);
    app.dimensions = vec![[600.0, 800.0]; 6];
    app.find();
    app.query = "unsaved draft".into();
    let wide = egui::vec2(1180.0, 820.0);
    settle(&mut app, &context, wide);
    let history = app.input_buttons.search_history.unwrap();
    context.memory_mut(|memory| memory.request_focus(history));
    let small = egui::vec2(240.0, 240.0);
    settle(&mut app, &context, small);
    let focused = context.read_response(history).unwrap();
    assert!(focused.interact_rect.contains_rect(focused.rect));
    frame(&mut app, &context, small, key(egui::Key::Enter));
    assert!(egui::Popup::is_any_open(&context));
    click(&mut app, &context, small, "Clear history");
    settle(&mut app, &context, small);
    assert!(app.query_history.is_empty());
    assert_eq!(app.query, "unsaved draft");
    assert_eq!(app.nav.page, 3);
    assert!(app.search_open && !egui::Popup::is_any_open(&context));
    assert!(context.memory(|memory| memory.has_focus(egui::Id::new("pagewing-search-input"))));
}
