use super::*;

fn reader() -> (tempfile::TempDir, Pagewing, egui::Context) {
    let directory = tempfile::tempdir().unwrap();
    let context = egui::Context::default();
    let mut app =
        Pagewing::with_context(&context, None, None, directory.path().join("library.json"));
    app.nav = crate::navigation::Navigation::new(6, 2);
    app.dimensions = vec![[600.0, 800.0]; 6];
    app.open_help(&context);
    (directory, app, context)
}

fn frame(
    app: &mut Pagewing,
    context: &egui::Context,
    size: egui::Vec2,
    events: Vec<egui::Event>,
) -> egui::FullOutput {
    context.run(
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, size)),
            time: Some(context.input(|input| input.time) + 0.1),
            events,
            ..Default::default()
        },
        |context| {
            app.keys(context);
            app.help_window(context);
            egui::CentralPanel::default().show(context, |_| {});
        },
    )
}

fn key(key: egui::Key) -> Vec<egui::Event> {
    [true, false]
        .into_iter()
        .map(|pressed| egui::Event::Key {
            key,
            physical_key: None,
            pressed,
            repeat: false,
            modifiers: egui::Modifiers::NONE,
        })
        .collect()
}

fn visible_text(output: &egui::FullOutput, value: &str) -> Option<egui::Rect> {
    output.shapes.iter().find_map(|shape| match &shape.shape {
        egui::Shape::Text(text) if text.galley.text() == value => {
            let rect = egui::Rect::from_min_size(text.pos, text.galley.size());
            shape.clip_rect.contains_rect(rect).then_some(rect)
        }
        _ => None,
    })
}

#[test]
fn help_actions_retain_focus_across_resizing_and_activate_the_original_action() {
    for activation in [egui::Key::Enter, egui::Key::Space] {
        for (query, tabs, copy) in [
            ("zoom 0", 1, true),
            ("zoom 0", 2, false),
            ("missing", 1, false),
        ] {
            let (_directory, mut app, context) = reader();
            let large = egui::vec2(1180.0, 820.0);
            for _ in 0..4 {
                frame(&mut app, &context, large, Vec::new());
            }
            frame(
                &mut app,
                &context,
                large,
                vec![egui::Event::Paste(query.into())],
            );
            for _ in 0..tabs {
                frame(&mut app, &context, large, key(egui::Key::Tab));
            }
            let focused = context.memory(|memory| memory.focused()).unwrap();
            for size in dialog::TEST_SIZES {
                for _ in 0..4 {
                    frame(&mut app, &context, size, Vec::new());
                }
                assert_eq!(
                    context.memory(|memory| memory.focused()),
                    Some(focused),
                    "{query}, {tabs}, {size:?}"
                );
            }
            let expected = app.help_filter.copy_text();
            let output = frame(&mut app, &context, large, key(activation));
            assert_eq!(app.help, copy);
            if copy {
                assert!(
                    output
                        .platform_output
                        .commands
                        .iter()
                        .any(|command| matches!(command,
                    egui::OutputCommand::CopyText(text) if text == &expected))
                );
                assert!(
                    context.memory(|memory| memory.has_focus(egui::Id::new("pagewing-help-input")))
                );
            }
            assert_eq!(app.nav.page, 2);
        }
    }
}

#[test]
fn changing_help_filter_shows_its_first_match_and_keeps_manual_scrolling() {
    for size in [egui::vec2(1180.0, 420.0), egui::vec2(240.0, 240.0)] {
        let (_directory, mut app, context) = reader();
        let mut output = egui::FullOutput::default();
        for _ in 0..4 {
            output = frame(&mut app, &context, size, Vec::new());
        }
        let first = SHORTCUTS[0].0;
        let pos = visible_text(&output, first).unwrap().center();
        let scroll = || {
            vec![
                egui::Event::PointerMoved(pos),
                egui::Event::MouseWheel {
                    unit: egui::MouseWheelUnit::Point,
                    delta: egui::vec2(0.0, -4000.0),
                    modifiers: egui::Modifiers::NONE,
                },
            ]
        };
        frame(&mut app, &context, size, scroll());
        for _ in 0..4 {
            output = frame(&mut app, &context, size, Vec::new());
        }
        assert!(visible_text(&output, first).is_none());
        frame(
            &mut app,
            &context,
            size,
            vec![egui::Event::Paste("page".into())],
        );
        for _ in 0..4 {
            output = frame(&mut app, &context, size, Vec::new());
        }
        let matched = app.help_filter.rows().next().unwrap().0.to_owned();
        assert!(
            visible_text(&output, &matched).is_some(),
            "first match {matched} hidden at {size:?}"
        );
        frame(&mut app, &context, size, scroll());
        for _ in 0..4 {
            output = frame(&mut app, &context, size, Vec::new());
        }
        assert!(
            visible_text(&output, &matched).is_none(),
            "manual scrolling must remain possible"
        );
        app.close_help(&context);
        frame(&mut app, &context, size, Vec::new());
        app.open_help(&context);
        for _ in 0..4 {
            output = frame(&mut app, &context, size, Vec::new());
        }
        assert!(
            visible_text(&output, first).is_some(),
            "reopening help starts at its first shortcut"
        );
    }
}

#[test]
fn help_and_its_actions_fit_the_minimum_reader_size() {
    let directory = tempfile::tempdir().unwrap();
    let context = egui::Context::default();
    let mut app =
        Pagewing::with_context(&context, None, None, directory.path().join("library.json"));
    for size in dialog::TEST_SIZES {
        for query in ["", "copy link", "jkgmissing"] {
            app.open_help(&context);
            app.help_filter.query = query.into();
            app.help_filter.filter();
            let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, size);
            let mut output = egui::FullOutput::default();
            for _ in 0..4 {
                output = context.run(
                    egui::RawInput {
                        screen_rect: Some(screen),
                        ..Default::default()
                    },
                    |context| app.help_window(context),
                );
            }
            let rect = context
                .memory(|memory| memory.area_rect(egui::Id::new("Keyboard shortcuts")))
                .unwrap();
            assert!(
                screen.contains_rect(rect),
                "screen {screen:?}, window {rect:?}"
            );
            if size.x > 1000.0 {
                assert!(
                    rect.width() > 600.0,
                    "desktop width must recover after resizing"
                );
            }
            dialog::assert_visible_actions(
                &context,
                &output,
                screen,
                "Keyboard shortcuts",
                &["Copy shortcuts", "Close"],
            );
            app.close_help(&context);
        }
    }
}

#[test]
fn filters_keys_and_descriptions_with_all_case_insensitive_tokens() {
    let mut help = HelpFilter::default();
    help.reset();
    assert_eq!(help.rows().len(), SHORTCUTS.len());
    help.query = "  COPY  LINK  ".into();
    help.filter();
    let rows: Vec<_> = help.rows().collect();
    assert_eq!(rows.len(), 2);
    assert!(rows.iter().any(|(keys, _)| keys == &"F, then a label"));
    assert!(rows.iter().any(|(keys, _)| keys == &"Right-click web link"));
    help.query = "zoom 0".into();
    help.filter();
    assert_eq!(help.rows().next().unwrap().0, "a / Ctrl/Cmd+0");
    assert_eq!(help.rows().len(), 1);
}

#[test]
fn custom_bindings_replace_the_default_help_and_copy_output() {
    let mut shortcuts = crate::settings::Shortcuts::default();
    let mut help = HelpFilter::default();
    for (key, label) in [
        (egui::Key::X, "X"),
        (egui::Key::Y, "Y"),
        (egui::Key::X, "X"),
    ] {
        shortcuts.overrides.insert(
            "next_page".into(),
            vec![crate::settings::Binding::from_event(
                key,
                egui::Modifiers::NONE,
                None,
            )],
        );
        help.reset();
        help.configure(&shortcuts);
        help.query = "next page".into();
        help.filter();
        assert_eq!(help.rows().collect::<Vec<_>>(), vec![(label, "Next page")]);
        assert!(help.copy_text().contains(&format!("{label}\tNext page")));
        help.query = "zt".into();
        help.filter();
        assert_eq!(help.rows().len(), 1, "fixed Vim sequences stay documented");
    }
    shortcuts.overrides.clear();
    help.reset();
    help.configure(&shortcuts);
    help.query = "next page".into();
    help.filter();
    assert!(help.copy_text().contains("Next / previous page"));
    assert!(!help.copy_text().contains("X\tNext page"));
}

#[test]
fn copy_uses_only_visible_rows_and_reopening_clears_the_filter() {
    let mut help = HelpFilter {
        query: "bookmark".into(),
        ..Default::default()
    };
    help.filter();
    let text = help.copy_text();
    assert!(text.starts_with("Pagewing keyboard shortcuts\n\n"));
    assert!(text.contains("Toggle bookmark"));
    assert!(!text.contains("Zoom in"));
    help.query = "jkgmissing".into();
    help.filter();
    assert_eq!(help.rows().len(), 0);
    help.reset();
    assert!(help.query.is_empty() && help.focus);
    assert_eq!(help.rows().len(), SHORTCUTS.len());
}
