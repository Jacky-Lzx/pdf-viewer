use super::*;
use crate::{
    destination::Destination,
    navigation::{Fit, Navigation},
    outline::OutlineEntry,
};

fn frame(
    app: &mut Pagewing,
    context: &egui::Context,
    size: Vec2,
    events: Vec<egui::Event>,
) -> egui::FullOutput {
    context.run(
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, size)),
            events,
            ..Default::default()
        },
        |context| eframe::App::update(app, context, &mut eframe::Frame::_new_kittest()),
    )
}

fn settle(app: &mut Pagewing, context: &egui::Context, size: Vec2) -> egui::FullOutput {
    let mut output = egui::FullOutput::default();
    for _ in 0..6 {
        output = frame(app, context, size, Vec::new());
    }
    output
}

fn fixture() -> (tempfile::TempDir, egui::Context, Pagewing) {
    let root = tempfile::tempdir().unwrap();
    let context = egui::Context::default();
    let mut app = Pagewing::with_context(&context, None, None, root.path().join("library.json"));
    app.nav = Navigation::new(80, 45);
    app.dimensions = vec![[600.0, 800.0]; 80];
    app.reading.fit = Fit::Custom(2.0);
    app.library.sidebar = true;
    app.outline.set_entries(
        std::iter::once(OutlineEntry {
            title: "Part".into(),
            depth: 0,
            destination: None,
        })
        .chain((0..40).map(|index| OutlineEntry {
            title: format!("Section {index:02}"),
            depth: 1,
            destination: Some(Destination::page(index * 2)),
        }))
        .collect(),
    );
    (root, context, app)
}

fn text_rect(output: &egui::FullOutput, label: &str) -> Option<egui::Rect> {
    output.shapes.iter().find_map(|shape| match &shape.shape {
        egui::Shape::Text(text) if text.galley.text() == label => {
            let rect = egui::Rect::from_min_size(text.pos, text.galley.size());
            shape.clip_rect.contains_rect(rect).then_some(rect)
        }
        _ => None,
    })
}

fn click(
    app: &mut Pagewing,
    context: &egui::Context,
    size: Vec2,
    pos: egui::Pos2,
    tail: Vec<egui::Event>,
) {
    for pressed in [true, false] {
        let mut events = vec![
            egui::Event::PointerMoved(pos),
            egui::Event::PointerButton {
                pos,
                button: egui::PointerButton::Primary,
                pressed,
                modifiers: egui::Modifiers::NONE,
            },
        ];
        if !pressed {
            events.extend(tail.clone());
        }
        frame(app, context, size, events);
    }
}

#[test]
fn here_clears_filter_reveals_a_folded_chapter_and_keeps_the_pdf_view() {
    for size in [Vec2::splat(240.0), Vec2::new(1180.0, 820.0)] {
        let (_root, context, mut app) = fixture();
        app.outline.fold_all(true);
        app.outline.query = "absent".into();
        app.outline.refilter();
        let output = settle(&mut app, &context, size);
        app.pan = Vec2::new(37.0, 60.0).min(app.overflow);
        let position = app.current_position();
        let button =
            text_rect(&output, "Here").expect("recovery visible even without chapter matches");
        click(&mut app, &context, size, button.center(), Vec::new());
        let output = settle(&mut app, &context, size);
        assert_eq!(app.outline.selected(), Some(23));
        assert!(!app.outline.is_collapsed(0));
        assert!(app.outline.query.is_empty());
        assert!(context.memory(|memory| memory.has_focus(egui::Id::new("pagewing-outline-input"))));
        assert!(text_rect(&output, "Section 22").is_some(), "{size:?}");
        let after = app.current_position();
        assert_eq!(after.page, position.page);
        assert_eq!(after.fit, position.fit);
        assert_eq!(after.rotation, position.rotation);
        assert_eq!(after.offset, position.offset);
    }
}

#[test]
fn here_click_and_text_in_one_frame_leave_other_input_drafts_intact() {
    for comment_open in [false, true] {
        let (_root, context, mut app) = fixture();
        let size = Vec2::new(1180.0, 820.0);
        app.find();
        app.query = "Search draft".into();
        if comment_open {
            app.comments.open = true;
            app.comments.loaded = true;
            app.comments.editor = Some(crate::annotations::Comment {
                page: 45,
                slot: None,
                parent: None,
                review: Default::default(),
                marked: false,
                states: Vec::new(),
                kind: "Text".into(),
                contents: "Unsaved comment".into(),
                author: "Reader".into(),
                rects: Vec::new(),
                has_appearance: false,
                truncated: false,
                locked: false,
            });
            app.comments.focus = true;
        }
        let output = settle(&mut app, &context, size);
        let button = text_rect(&output, "Here").unwrap();
        click(
            &mut app,
            &context,
            size,
            button.center(),
            vec![egui::Event::Text("Section 22".into())],
        );
        settle(&mut app, &context, size);
        assert_eq!(app.outline.query, "Section 22");
        assert_eq!(app.query, "Search draft");
        assert!(app.search_open);
        assert_eq!(app.nav.page, 45);
        if comment_open {
            assert_eq!(
                app.comments.editor.as_ref().unwrap().contents,
                "Unsaved comment"
            );
            assert!(app.comments.open && app.comments.unsaved());
        }
    }
}

#[test]
fn keyboard_here_activation_focuses_the_filter_without_jumping_to_the_chapter_start() {
    for key in [egui::Key::Enter, egui::Key::Space] {
        let (_root, context, mut app) = fixture();
        let size = Vec2::new(1180.0, 820.0);
        settle(&mut app, &context, size);
        context
            .memory_mut(|memory| memory.request_focus(app.input_buttons.current_chapter.unwrap()));
        settle(&mut app, &context, size);
        let events = [true, false]
            .map(|pressed| egui::Event::Key {
                key,
                physical_key: None,
                pressed,
                repeat: false,
                modifiers: egui::Modifiers::NONE,
            })
            .into();
        frame(&mut app, &context, size, events);
        settle(&mut app, &context, size);
        assert_eq!(
            app.nav.page, 45,
            "{key:?} must not follow the page-44 destination"
        );
        assert_eq!(app.outline.selected(), Some(23));
        assert!(context.memory(|memory| memory.has_focus(egui::Id::new("pagewing-outline-input"))));
    }
}

#[test]
fn current_chapter_stays_highlighted_between_start_pages_without_moving_selection() {
    let (_root, context, mut app) = fixture();
    let size = Vec2::new(1180.0, 820.0);
    app.nav.page = 1;
    let output = settle(&mut app, &context, size);
    let rect = text_rect(&output, "Section 00").unwrap();
    let fill = context.style().visuals.selection.bg_fill;
    assert!(output.shapes.iter().any(|shape| matches!(&shape.shape, egui::Shape::Rect(shape) if shape.fill == fill && shape.rect.contains_rect(rect))));
    assert_eq!(
        app.outline.selected(),
        Some(0),
        "reading does not replace filter selection"
    );
    app.outline.fold_all(true);
    let output = settle(&mut app, &context, size);
    let rect = text_rect(&output, "Part").unwrap();
    assert!(output.shapes.iter().any(|shape| matches!(&shape.shape, egui::Shape::Rect(shape) if shape.fill == fill && shape.rect.contains_rect(rect))));
    assert!(
        app.outline.is_collapsed(0),
        "reading must not automatically expand chapters"
    );
}
