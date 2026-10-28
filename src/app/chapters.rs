use super::Pagewing;

impl Pagewing {
    pub(super) fn jump_chapter(&mut self, reverse: bool, count: usize) {
        if let Some(destination) = self.outline.chapter_target(self.nav.page, reverse, count) {
            self.follow_destination(destination);
        } else {
            self.notify(if self.outline.chapter_at(usize::MAX).is_none() {
                "This document has no chapter destinations"
            } else if reverse {
                "No earlier chapter start"
            } else {
                "No later chapter start"
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        destination::Destination,
        navigation::{Fit, Navigation},
        outline::OutlineEntry,
    };
    use eframe::egui::{self, Context, Event, Key, Modifiers, Vec2};

    fn frame(app: &mut Pagewing, context: &Context, events: Vec<Event>) {
        let _ = context.run(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    Vec2::new(1180.0, 820.0),
                )),
                events,
                ..Default::default()
            },
            |context| eframe::App::update(app, context, &mut eframe::Frame::_new_kittest()),
        );
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

    fn fixture() -> (tempfile::TempDir, Context, Pagewing) {
        let root = tempfile::tempdir().unwrap();
        let context = Context::default();
        let mut app =
            Pagewing::with_context(&context, None, None, root.path().join("library.json"));
        app.nav = Navigation::new(40, 5);
        app.dimensions = vec![[600.0, 800.0]; 40];
        app.outline.set_entries(
            [25, 0, 12, 4, 12]
                .into_iter()
                .map(|page| OutlineEntry {
                    title: format!("Chapter {page}"),
                    depth: 0,
                    destination: Some(Destination {
                        zoom: Some(2.5),
                        position: [Some(48.0), Some(150.0)],
                        ..Destination::page(page)
                    }),
                })
                .collect(),
        );
        (root, context, app)
    }

    #[test]
    fn chapter_shortcuts_preserve_exact_views_and_one_history_step_for_a_counted_jump() {
        for (continuous, facing) in [(false, false), (true, false), (false, true)] {
            let (_root, context, mut app) = fixture();
            app.set_display_mode(continuous, facing);
            app.reading.fit = Fit::Custom(2.0);
            app.reading.rotation = 1;
            frame(&mut app, &context, Vec::new());
            app.pending_offset = Some([0.2, 0.3]);
            frame(&mut app, &context, Vec::new());
            let before = app.current_position();
            app.outline.query = "absent".into();
            app.outline.refilter();
            let mut events = key(Key::Num2, Modifiers::NONE);
            events.extend(key(Key::ArrowDown, Modifiers::ALT));
            frame(&mut app, &context, events);
            assert_eq!(app.nav.page, 25);
            assert_eq!(app.reading.fit, Fit::Custom(2.5));
            assert_eq!(app.reading.rotation, 1);
            assert_eq!(
                (app.reading.continuous, app.reading.facing),
                (continuous, facing)
            );
            // Clockwise rotation maps (48, 150) to (800 - 150, 48).
            // An odd page belongs on the right of the facing-page gutter.
            let mut expected = Vec2::new(650.0, 48.0) * app.scale;
            if facing {
                expected += app.spread().offset(app.nav.page);
            }
            assert_eq!(app.pan, expected.min(app.overflow));
            assert_eq!(app.outline.query, "absent");
            assert!(app.nav.prefix.is_empty());
            assert!(app.history(false));
            frame(&mut app, &context, Vec::new());
            let after = app.current_position();
            assert_eq!(after.page, before.page);
            assert_eq!(after.fit, before.fit);
            for (actual, expected) in after.offset.into_iter().zip(before.offset) {
                assert!((actual - expected).abs() < 0.0001);
            }
            assert!(
                !app.history(false),
                "counted navigation must not record intermediate chapters"
            );
            assert!(app.history(true));
            frame(&mut app, &context, Vec::new());
            assert_eq!(app.nav.page, 25);
        }
    }

    #[test]
    fn chapter_bindings_support_ordered_batches_overrides_disabling_and_boundaries() {
        let (_root, context, mut app) = fixture();
        let mut events = key(Key::ArrowUp, Modifiers::ALT);
        events.extend(key(Key::ArrowUp, Modifiers::ALT));
        events.extend(key(Key::ArrowDown, Modifiers::ALT));
        frame(&mut app, &context, events);
        assert_eq!(app.nav.page, 4, "5 → 4 → 0 → 4 within one frame");
        app.library.shortcuts.overrides.insert(
            "next_chapter".into(),
            vec![crate::settings::Binding::parse_config("X").unwrap()],
        );
        app.library
            .shortcuts
            .overrides
            .insert("previous_chapter".into(), vec![]);
        app.library.shortcuts.validate().unwrap();
        for key_name in [Key::ArrowDown, Key::ArrowUp] {
            frame(
                &mut app,
                &context,
                key(
                    key_name,
                    Modifiers {
                        alt: true,
                        shift: true,
                        ..Modifiers::NONE
                    },
                ),
            );
            assert_eq!(
                app.nav.page, 4,
                "extra modifiers must not bypass disabled bindings"
            );
        }
        frame(&mut app, &context, key(Key::ArrowDown, Modifiers::ALT));
        assert_eq!(app.nav.page, 4, "replaced default stays disabled");
        frame(&mut app, &context, key(Key::X, Modifiers::NONE));
        assert_eq!(app.nav.page, 12, "one distinct chapter per press");
        frame(&mut app, &context, key(Key::ArrowUp, Modifiers::ALT));
        assert_eq!(app.nav.page, 12, "disabled previous stays disabled");
        app.nav.prefix = "9999999".into();
        frame(&mut app, &context, key(Key::X, Modifiers::NONE));
        assert_eq!(app.nav.page, 25);
        let before = app.current_position();
        frame(&mut app, &context, key(Key::X, Modifiers::NONE));
        assert_eq!(app.nav.page, before.page);
        assert_eq!(
            app.current_offset(),
            before.offset,
            "boundary is a view no-op"
        );
        app.outline.set_entries(Vec::new());
        frame(&mut app, &context, key(Key::X, Modifiers::NONE));
        assert_eq!(app.nav.page, 25);
    }

    #[test]
    fn chapter_shortcuts_do_not_escape_search_chapter_filters_or_comment_drafts() {
        for input in ["search", "chapters", "comment"] {
            let (_root, context, mut app) = fixture();
            match input {
                "search" => {
                    app.find();
                    app.query = "draft".into();
                }
                "chapters" => {
                    app.focus_contents();
                    app.outline.query = "draft".into();
                    app.outline.refilter();
                }
                _ => {
                    app.comments.open = true;
                    app.comments.loaded = true;
                    app.comments.editor = Some(crate::annotations::Comment {
                        page: 5,
                        slot: None,
                        parent: None,
                        review: Default::default(),
                        marked: false,
                        states: Vec::new(),
                        kind: "Text".into(),
                        contents: "draft".into(),
                        author: "Reader".into(),
                        rects: Vec::new(),
                        has_appearance: false,
                        truncated: false,
                        locked: false,
                    });
                    app.comments.focus = true;
                }
            }
            for _ in 0..5 {
                frame(&mut app, &context, Vec::new());
            }
            let mut events = key(Key::ArrowDown, Modifiers::ALT);
            events.extend(key(Key::ArrowUp, Modifiers::ALT));
            frame(&mut app, &context, events);
            assert_eq!(app.nav.page, 5, "{input}");
            match input {
                "search" => assert_eq!(app.query, "draft"),
                "chapters" => assert_eq!(app.outline.query, "draft"),
                _ => assert_eq!(app.comments.editor.as_ref().unwrap().contents, "draft"),
            }
        }
    }
}
