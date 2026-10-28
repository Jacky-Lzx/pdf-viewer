use super::*;
use crate::{
    navigation::{Fit, Navigation},
    text::SearchHit,
};

fn fixture() -> (tempfile::TempDir, egui::Context, Pagewing) {
    let root = tempfile::tempdir().unwrap();
    let context = egui::Context::default();
    let mut app = Pagewing::with_context(&context, None, None, root.path().join("library.json"));
    app.nav = Navigation::new(100, 42);
    app.dimensions = vec![[600.0, 800.0]; 100];
    app.reading.fit = Fit::Custom(2.0);
    app.pan = egui::vec2(37.0, 53.0);
    app.query = "Needle".into();
    app.query_history.remember(&app.query);
    app.hits = (41..44)
        .map(|page| SearchHit {
            page,
            rects: Vec::new(),
            snippet: format!("Needle 中文 on {page}"),
            snippet_match: 0..6,
        })
        .collect();
    app.hit_index = Some(1);
    app.scanned = 50;
    app.scan_total = 100;
    app.search_open = true;
    app.search_complete = false;
    (root, context, app)
}

fn frame(app: &mut Pagewing, context: &egui::Context, size: egui::Vec2, events: Vec<egui::Event>) {
    let _ = context.run(
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, size)),
            time: Some(context.input(|input| input.time) + 0.25),
            events,
            ..Default::default()
        },
        |context| {
            app.keys(context);
            app.toolbar(context);
            app.search_bar(context);
            app.status(context);
            egui::CentralPanel::default().show(context, |_| {});
        },
    );
}

fn settle(app: &mut Pagewing, context: &egui::Context, size: egui::Vec2) {
    for _ in 0..5 {
        frame(app, context, size, Vec::new());
    }
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

#[test]
fn stop_retires_batches_and_pending_navigation_without_changing_accepted_results_or_view() {
    let (_root, _context, mut app) = fixture();
    let snippets: Vec<_> = app.hits.iter().map(|hit| hit.snippet.clone()).collect();
    app.next_hits(false, 1000);
    assert!(app.search_navigation.first().is_some());
    app.search_due = Some(Instant::now());
    let revision = app.revision;
    app.apply_search_run(RunAction::Stop);
    assert_eq!(app.revision, revision + 1);
    assert!(app.search_due.is_none() && app.search_navigation.first().is_none());
    assert!(app.search_complete && app.search_stopped && !app.search_limited);
    assert_eq!(
        (app.scanned, app.scan_total, app.hit_index),
        (50, 100, Some(1))
    );
    assert_eq!(
        app.hits.iter().map(|hit| &hit.snippet).collect::<Vec<_>>(),
        snippets.iter().collect::<Vec<_>>()
    );
    app.resume_search_navigation();
    assert_eq!(app.nav.page, 42);
    assert_eq!(app.reading.fit, Fit::Custom(2.0));
    assert_eq!(app.pan, egui::vec2(37.0, 53.0));
    assert!(
        !app.history(false),
        "stopping must not create a history entry"
    );
    // Accepted partial results now form a terminal list that can wrap normally.
    app.next_hit(false);
    assert_eq!(app.hit_index, Some(2));
    app.next_hit(false);
    assert_eq!(app.hit_index, Some(0));
    assert!(app.search_navigation.first().is_none());
}

#[test]
fn restart_and_query_changes_release_stopped_results_but_retain_query_options_and_view() {
    let (_root, context, mut app) = fixture();
    app.library.search_options.whole_word = true;
    app.apply_search_run(RunAction::Stop);
    let revision = app.revision;
    app.apply_search_run(RunAction::Restart);
    assert_eq!(app.revision, revision + 1);
    assert!(!app.search_stopped && !app.search_complete && !app.search_limited);
    assert!(app.hits.is_empty() && app.hit_index.is_none() && app.search_due.is_some());
    assert_eq!((app.scanned, app.scan_total), (0, 100));
    assert_eq!(app.query, "Needle");
    assert!(app.library.search_options.whole_word);
    assert_eq!(app.nav.page, 42);
    assert_eq!(app.pan, egui::vec2(37.0, 53.0));
    app.apply_search_run(RunAction::Stop);
    app.query = "changed".into();
    app.change_search();
    assert!(!app.search_stopped && app.search_run_requested.is_none());
    app.apply_search_run(RunAction::Stop);
    app.clear_document(&context);
    assert!(!app.search_stopped && app.search_run_requested.is_none() && app.hits.is_empty());
}

#[test]
fn stop_after_same_frame_text_edits_applies_to_the_updated_query() {
    let (_root, context, mut app) = fixture();
    let size = egui::vec2(1180.0, 820.0);
    app.search_focus = true;
    settle(&mut app, &context, size);
    app.search_button(crate::app::shortcuts::SearchButton::Stop);
    frame(
        &mut app,
        &context,
        size,
        vec![egui::Event::Text("中文".into())],
    );
    assert!(app.query.contains("中文"));
    assert!(app.search_stopped && app.search_complete && app.search_due.is_none());
    assert!(
        app.hits.is_empty(),
        "the old query's results must be discarded first"
    );
    assert_eq!(app.query_history.queries().next(), Some(app.query.as_str()));
    assert!(app.search_run_requested.is_none());
}

#[test]
fn stop_and_restart_keep_keyboard_focus_through_resizing_and_do_not_scroll_the_reader() {
    for activation in [egui::Key::Enter, egui::Key::Space] {
        let (_root, context, mut app) = fixture();
        let wide = egui::vec2(1180.0, 820.0);
        let narrow = egui::vec2(240.0, 240.0);
        settle(&mut app, &context, wide);
        let id = app.input_buttons.search_stop.unwrap();
        context.memory_mut(|memory| memory.request_focus(id));
        settle(&mut app, &context, narrow);
        app.nav.prefix = "40".into();
        frame(&mut app, &context, narrow, key(activation));
        assert!(app.search_stopped);
        settle(&mut app, &context, wide);
        assert_eq!(app.input_buttons.search_restart, Some(id));
        assert_eq!(context.memory(|memory| memory.focused()), Some(id));
        let response = context.read_response(id).unwrap();
        assert!(response.interact_rect.contains_rect(response.rect));
        frame(&mut app, &context, wide, key(activation));
        assert!(!app.search_stopped && !app.search_complete && app.hits.is_empty());
        assert_eq!(app.nav.page, 42);
        assert_eq!(app.nav.prefix, "40");
        assert_eq!(app.pan, egui::vec2(37.0, 53.0));
    }
}

#[test]
fn stale_stop_blank_queries_and_loading_do_not_restart_a_finished_search() {
    let (_root, context, mut app) = fixture();
    app.search_complete = true;
    let revision = app.revision;
    app.search_button(crate::app::shortcuts::SearchButton::Stop);
    frame(&mut app, &context, egui::vec2(1180.0, 820.0), Vec::new());
    assert_eq!(app.revision, revision);
    assert!(!app.search_stopped && app.hits.len() == 3);
    app.apply_search_run(RunAction::Restart);
    assert_eq!(app.revision, revision);
    for query in ["", "  \n", "Needle"] {
        app.query = query.into();
        app.loading = query == "Needle";
        app.search_complete = false;
        app.apply_search_run(RunAction::Stop);
        assert_eq!(app.revision, revision);
        assert!(!app.search_stopped);
    }
}

#[test]
fn progress_changes_and_resizing_keep_every_search_action_identity() {
    for control in [
        "copy", "history", "close", "case", "word", "previous", "next", "results",
    ] {
        let (_root, context, mut app) = fixture();
        let target = |app: &Pagewing| {
            match control {
                "copy" => app.input_buttons.search_copy,
                "history" => app.input_buttons.search_history,
                "close" => app.input_buttons.close_search,
                "case" => app.input_buttons.search_case,
                "word" => app.input_buttons.search_word,
                "previous" => app.input_buttons.search_previous,
                "next" => app.input_buttons.search_next,
                "results" => app.input_buttons.search_results,
                _ => unreachable!(),
            }
            .unwrap()
        };
        let wide = egui::vec2(1180.0, 820.0);
        let narrow = egui::vec2(240.0, 240.0);
        settle(&mut app, &context, wide);
        let id = target(&app);
        context.memory_mut(|memory| memory.request_focus(id));
        for (size, stopped) in [(narrow, false), (wide, true), (narrow, true)] {
            if stopped && !app.search_stopped {
                app.apply_search_run(RunAction::Stop);
            }
            settle(&mut app, &context, size);
            assert_eq!(target(&app), id, "{control}: {size:?}, stopped {stopped}");
            assert_eq!(
                context.memory(|memory| memory.focused()),
                Some(id),
                "{control}"
            );
        }
    }
}
