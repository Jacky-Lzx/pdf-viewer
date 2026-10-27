use super::{Pagewing, dialog, shortcuts};
use eframe::egui::{
    self,
    text::{CCursor, CCursorRange},
};

pub(super) fn input_id() -> egui::Id {
    egui::Id::new("pagewing-zoom-input")
}

pub(super) struct ZoomInput {
    value: String,
    pub focus: bool,
    pub confirm: bool,
    invalid: bool,
}

fn parse_percentage(value: &str) -> Option<f32> {
    let value = value.trim();
    let value = value.strip_suffix('%').unwrap_or(value).trim();
    let percent = value.parse::<f64>().ok()?;
    let scale = (percent / 100.0) as f32;
    (percent.is_finite() && scale.is_finite() && scale > 0.0).then_some(scale)
}

impl Pagewing {
    pub(super) fn open_zoom(&mut self, context: &egui::Context) {
        if self.loading || self.resolve_view().is_none() {
            return;
        }
        self.outline.cancel_input();
        self.close_password_input(context);
        self.nav.clear_prefix();
        self.hints.cancel();
        self.mark_action = None;
        self.help = false;
        self.properties_open = false;
        self.page_input = None;
        self.recent_picker = None;
        self.search_focus = false;
        self.pending_search_jump = None;
        self.comments.focus = false;
        let value = (self.scale * 100.0).to_string();
        let mut state = egui::text_edit::TextEditState::default();
        state.cursor.set_char_range(Some(CCursorRange::two(
            CCursor::new(0),
            CCursor::new(value.chars().count()),
        )));
        state.store(context, input_id());
        context.memory_mut(|memory| {
            if let Some(id) = memory.focused() {
                memory.surrender_focus(id);
            }
            memory.move_focus(egui::FocusDirection::None);
        });
        self.zoom_input = Some(ZoomInput {
            value,
            focus: true,
            confirm: false,
            invalid: false,
        });
    }

    pub(super) fn close_zoom(&mut self, context: &egui::Context) {
        self.zoom_input = None;
        context.memory_mut(|memory| memory.surrender_focus(input_id()));
    }

    pub(super) fn zoom_dialog(&mut self, context: &egui::Context) {
        let Some(mut input) = self.zoom_input.take() else {
            return;
        };
        if self.loading {
            self.close_zoom(context);
            return;
        }
        let mut open = true;
        let mut cancel = false;
        let mut chosen = None;
        let short = context.content_rect().height() < 420.0;
        dialog::window(context, "Zoom", "Zoom", 260.0)
            .open(&mut open)
            .show(context, |ui| {
                if short {
                    ui.spacing_mut().item_spacing.y = 6.0;
                } else {
                    ui.label("Zoom percentage");
                }
                let (response, _) = shortcuts::text_input(
                    ui,
                    input_id(),
                    &mut input.focus,
                    &mut self.deferred_input,
                    egui::TextEdit::singleline(&mut input.value)
                        .desired_width(f32::INFINITY)
                        .char_limit(64)
                        .hint_text("125 or 125%"),
                );
                if response.changed() {
                    input.invalid = false;
                }
                if input.invalid {
                    ui.colored_label(ui.visuals().error_fg_color, "Enter a percentage above 0.");
                }
                ui.horizontal_wrapped(|ui| {
                    for percent in [50, 100, 150, 200] {
                        if ui.button(format!("{percent}%")).clicked() {
                            chosen = Some(percent as f32 / 100.0);
                        }
                    }
                });
                ui.separator();
                ui.horizontal(|ui| {
                    let apply = ui.button("Apply").clicked();
                    cancel = ui.button("Cancel").clicked();
                    if !ui.is_sizing_pass() && (apply || std::mem::take(&mut input.confirm)) {
                        chosen = parse_percentage(&input.value);
                        if chosen.is_none() {
                            input.invalid = true;
                            input.focus = true;
                            context.request_repaint();
                        }
                    }
                });
            });
        if !open || cancel {
            chosen = None;
        }
        if let Some(scale) = chosen {
            self.zoom_to(scale);
        }
        if open && !cancel && chosen.is_none() {
            self.zoom_input = Some(input);
        } else {
            self.close_zoom(context);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::navigation::{Fit, Navigation};
    use egui::{Context, Event, Key, Modifiers, Pos2, RawInput, Rect, Vec2};

    fn frame(app: &mut Pagewing, context: &Context, mut events: Vec<Event>) {
        let mut deferred = std::mem::take(&mut app.deferred_input);
        deferred.append(&mut events);
        let _ = context.run(
            RawInput {
                screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(1100.0, 820.0))),
                events: deferred,
                ..Default::default()
            },
            |context| {
                app.keys(context);
                app.toolbar(context);
                app.status(context);
                app.zoom_dialog(context);
                app.canvas(context);
            },
        );
    }

    fn key(key: Key) -> Vec<Event> {
        modified_key(key, Modifiers::NONE)
    }

    fn modified_key(key: Key, modifiers: Modifiers) -> Vec<Event> {
        [true, false]
            .into_iter()
            .map(|pressed| Event::Key {
                key,
                physical_key: None,
                pressed,
                repeat: false,
                modifiers,
            })
            .collect()
    }

    fn settle(app: &mut Pagewing, context: &Context) {
        for _ in 0..3 {
            frame(app, context, Vec::new());
        }
    }

    fn reader(path: std::path::PathBuf) -> (Pagewing, Context) {
        let context = Context::default();
        let mut app = Pagewing::with_context(&context, None, None, path);
        app.dimensions = vec![[600.0, 800.0]; 4];
        app.nav = Navigation::new(4, 1);
        settle(&mut app, &context);
        (app, context)
    }

    fn enter_value(app: &mut Pagewing, context: &Context, text: &str) {
        let mut events = vec![Event::Paste(text.to_owned())];
        events.extend(key(Key::Enter));
        frame(app, context, events);
        settle(app, context);
    }

    #[test]
    fn percentages_reject_invalid_values_and_keep_sub_percent_precision() {
        for (text, expected) in [
            ("125", 1.25),
            (" 125 % ", 1.25),
            (".5%", 0.005),
            ("1e-29%", 1e-31),
            ("1000000%", 10000.0),
        ] {
            assert_eq!(parse_percentage(text), Some(expected), "{text}");
        }
        for text in [
            "",
            "%",
            "125%%",
            "125% extra",
            "-10",
            "0",
            "-0%",
            "NaN",
            "inf",
            "1e100",
            "1e-100",
            "j125",
            "1,5",
        ] {
            assert_eq!(parse_percentage(text), None, "{text}");
        }
    }

    #[test]
    fn clicking_the_percentage_routes_same_frame_text_and_enter_to_the_new_input() {
        let directory = tempfile::tempdir().unwrap();
        let (mut app, context) = reader(directory.path().join("library.json"));
        let pos = context
            .read_response(app.input_buttons.zoom.unwrap())
            .unwrap()
            .rect
            .center();
        let mut events = vec![Event::PointerMoved(pos)];
        events.extend([true, false].map(|pressed| Event::PointerButton {
            pos,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: Modifiers::NONE,
        }));
        for (key_code, text) in [(Key::Num1, "1"), (Key::Num2, "2"), (Key::Num5, "5")] {
            events.extend(key(key_code));
            events.push(Event::Text(text.into()));
        }
        events.extend(key(Key::Enter));
        frame(&mut app, &context, events);
        settle(&mut app, &context);
        assert!(app.zoom_input.is_none());
        assert_eq!(app.reading.fit, Fit::Custom(1.25));
        assert_eq!(app.nav.page, 1);
        assert!(app.nav.prefix.is_empty());
        assert!(!context.memory(|memory| memory.has_focus(input_id())));
    }

    #[test]
    fn keyboard_activation_opens_the_input_without_applying_it() {
        let directory = tempfile::tempdir().unwrap();
        let (mut app, context) = reader(directory.path().join("library.json"));
        let before = app.reading.fit;
        context.memory_mut(|memory| memory.request_focus(app.input_buttons.zoom.unwrap()));
        frame(&mut app, &context, key(Key::Enter));
        settle(&mut app, &context);
        assert!(app.zoom_input.is_some());
        assert_eq!(app.reading.fit, before);
        assert!(context.memory(|memory| memory.has_focus(input_id())));
    }

    fn zoom_shortcut() -> Vec<Event> {
        modified_key(Key::Z, Modifiers::COMMAND | Modifiers::ALT)
    }

    #[test]
    fn shortcut_routes_batched_text_and_enter_and_cancels_reader_prefixes() {
        let directory = tempfile::tempdir().unwrap();
        let (mut app, context) = reader(directory.path().join("library.json"));
        app.nav.prefix = "42".into();
        app.nav.pending_z = true;
        app.mark_action = Some(super::super::MarkAction::Set);
        let mut events = zoom_shortcut();
        // Option shortcuts can emit text on macOS; it belongs to the shortcut.
        events.insert(1, Event::Text("Ω".into()));
        for (key_code, text) in [(Key::Num1, "1"), (Key::Num2, "2"), (Key::Num5, "5")] {
            events.extend(key(key_code));
            events.push(Event::Text(text.into()));
        }
        events.extend(key(Key::Enter));
        frame(&mut app, &context, events);
        settle(&mut app, &context);
        assert!(app.zoom_input.is_none());
        assert_eq!(app.reading.fit, Fit::Custom(1.25));
        assert_eq!(app.nav.page, 1);
        assert!(app.nav.prefix.is_empty() && !app.nav.pending_z);
        assert!(app.mark_action.is_none());
    }

    #[test]
    fn shortcut_replaces_search_focus_but_keeps_an_open_zoom_draft() {
        let directory = tempfile::tempdir().unwrap();
        let (mut app, context) = reader(directory.path().join("library.json"));
        app.search_open = true;
        app.search_focus = true;
        context.memory_mut(|memory| {
            memory.request_focus(egui::Id::new("pagewing-search-input"));
        });
        let mut events = zoom_shortcut();
        events.push(Event::Paste("175".into()));
        frame(&mut app, &context, events);
        settle(&mut app, &context);
        assert_eq!(app.zoom_input.as_ref().unwrap().value, "175");
        assert!(!app.search_focus);
        assert!(context.memory(|memory| memory.has_focus(input_id())));
        frame(&mut app, &context, zoom_shortcut());
        settle(&mut app, &context);
        assert_eq!(app.zoom_input.as_ref().unwrap().value, "175");
        frame(&mut app, &context, key(Key::Enter));
        settle(&mut app, &context);
        assert_eq!(app.reading.fit, Fit::Custom(1.75));
    }

    #[test]
    fn configured_zoom_replaces_defaults_and_plain_binding_stays_out_of_text_fields() {
        let directory = tempfile::tempdir().unwrap();
        std::fs::write(
            directory.path().join("config.toml"),
            "[shortcuts]\nzoom = ['X', 'Cmd+Shift+Z']\n",
        )
        .unwrap();
        let (mut app, context) = reader(directory.path().join("library.json"));
        frame(&mut app, &context, zoom_shortcut());
        assert!(app.zoom_input.is_none(), "replaced default is inactive");
        frame(&mut app, &context, key(Key::X));
        settle(&mut app, &context);
        assert!(app.zoom_input.is_some());
        frame(&mut app, &context, key(Key::Escape));
        app.open_goto();
        frame(&mut app, &context, key(Key::X));
        assert!(
            app.zoom_input.is_none(),
            "plain binding cannot escape an input"
        );
        let mut events = modified_key(Key::Z, Modifiers::COMMAND | Modifiers::SHIFT);
        events.push(Event::Paste("150".into()));
        events.extend(key(Key::Enter));
        frame(&mut app, &context, events);
        settle(&mut app, &context);
        assert!(app.page_input.is_none());
        assert_eq!(app.reading.fit, Fit::Custom(1.5));
    }

    #[test]
    fn zoom_shortcut_can_be_disabled_and_requires_a_ready_document() {
        let directory = tempfile::tempdir().unwrap();
        std::fs::write(
            directory.path().join("config.toml"),
            "[shortcuts]\nzoom = []\n",
        )
        .unwrap();
        let (mut app, context) = reader(directory.path().join("library.json"));
        frame(&mut app, &context, zoom_shortcut());
        assert!(app.zoom_input.is_none());
        app.library.shortcuts.overrides.clear();
        app.loading = true;
        frame(&mut app, &context, zoom_shortcut());
        assert!(app.zoom_input.is_none());
        app.loading = false;
        app.clear_document(&context);
        frame(&mut app, &context, zoom_shortcut());
        assert!(app.zoom_input.is_none());
    }

    #[test]
    fn zoom_dialog_applies_in_every_layout_and_rotation_and_keeps_the_mode() {
        let directory = tempfile::tempdir().unwrap();
        let (mut app, context) = reader(directory.path().join("library.json"));
        for (continuous, facing) in [(false, false), (true, false), (false, true)] {
            app.set_display_mode(continuous, facing);
            for rotation in 0..4 {
                app.reading.rotation = rotation;
                app.reading.fit = Fit::Page;
                settle(&mut app, &context);
                app.open_zoom(&context);
                enter_value(&mut app, &context, "150%");
                assert!(app.zoom_input.is_none());
                assert_eq!(app.reading.fit, Fit::Custom(1.5));
                assert_eq!(app.reading.continuous, continuous);
                assert_eq!(app.reading.facing, facing);
                assert_eq!(app.reading.rotation, rotation);
                assert!(app.current_offset().iter().all(|v| v.is_finite()));
            }
        }
    }

    #[test]
    fn invalid_input_preserves_the_view_and_escape_releases_only_zoom_focus() {
        let directory = tempfile::tempdir().unwrap();
        let (mut app, context) = reader(directory.path().join("library.json"));
        app.comments.open = true;
        app.open_zoom(&context);
        settle(&mut app, &context);
        let before = (app.reading.fit, app.pan, app.nav.page);
        enter_value(&mut app, &context, "jkg0%");
        let input = app.zoom_input.as_ref().unwrap();
        assert_eq!(input.value, "jkg0%");
        assert!(input.invalid);
        assert_eq!((app.reading.fit, app.pan, app.nav.page), before);
        frame(&mut app, &context, key(Key::Escape));
        assert!(app.comments.open);
        assert!(app.zoom_input.is_none());
        assert!(!context.memory(|memory| memory.has_focus(input_id())));
        app.open_zoom(&context);
        settle(&mut app, &context);
        enter_value(&mut app, &context, "200");
        assert_eq!(app.reading.fit, Fit::Custom(2.0));
    }

    #[test]
    fn typed_zoom_obeys_page_limits_and_document_close_discards_the_input() {
        let directory = tempfile::tempdir().unwrap();
        let (mut app, context) = reader(directory.path().join("library.json"));
        for (value, expected) in [("10000%", 8.0), (".001%", 0.05)] {
            app.open_zoom(&context);
            enter_value(&mut app, &context, value);
            assert_eq!(app.reading.fit, Fit::Custom(expected));
        }
        app.dimensions = vec![[600_000.0, 800_000.0]; 4];
        app.open_zoom(&context);
        enter_value(&mut app, &context, ".1%");
        assert_eq!(app.reading.fit, Fit::Custom(0.001));
        app.open_zoom(&context);
        app.clear_document(&context);
        assert!(app.zoom_input.is_none());
        assert!(!context.memory(|memory| memory.has_focus(input_id())));
    }

    #[test]
    fn keyboard_presets_remain_visible_in_a_minimum_size_window() {
        let directory = tempfile::tempdir().unwrap();
        let (mut app, context) = reader(directory.path().join("library.json"));
        let screen = Rect::from_min_size(Pos2::ZERO, Vec2::splat(240.0));
        for (index, percent) in [50, 100, 150, 200].into_iter().enumerate() {
            app.open_zoom(&context);
            let input = app.zoom_input.as_mut().unwrap();
            input.value = "0".into();
            input.invalid = true;
            {
                let mut draw = |events| {
                    context.run(
                        RawInput {
                            screen_rect: Some(screen),
                            events,
                            ..Default::default()
                        },
                        |context| {
                            app.keys(context);
                            app.zoom_dialog(context);
                        },
                    )
                };
                for _ in 0..4 {
                    draw(Vec::new());
                }
                for _ in 0..=index {
                    draw(key(Key::Tab));
                }
                let output = draw(Vec::new());
                dialog::assert_visible_actions(
                    &context,
                    &output,
                    screen,
                    "Zoom",
                    &[&format!("{percent}%"), "Apply", "Cancel"],
                );
                draw(key(Key::Enter));
            }
            assert!(app.zoom_input.is_none());
            assert_eq!(app.reading.fit, Fit::Custom(percent as f32 / 100.0));
            assert_eq!(app.nav.page, 1);
        }
    }

    #[test]
    fn zoom_window_and_validation_fit_the_minimum_reader_size() {
        let directory = tempfile::tempdir().unwrap();
        let (mut app, context) = reader(directory.path().join("library.json"));
        for size in dialog::TEST_SIZES {
            app.open_zoom(&context);
            app.zoom_input.as_mut().unwrap().invalid = true;
            let screen = Rect::from_min_size(Pos2::ZERO, size);
            let mut output = egui::FullOutput::default();
            for _ in 0..4 {
                output = context.run(
                    RawInput {
                        screen_rect: Some(screen),
                        ..Default::default()
                    },
                    |context| app.zoom_dialog(context),
                );
            }
            dialog::assert_visible_actions(
                &context,
                &output,
                screen,
                "Zoom",
                &[
                    "Apply",
                    "Cancel",
                    "Enter a percentage above 0.",
                    "50%",
                    "100%",
                    "150%",
                    "200%",
                ],
            );
            app.close_zoom(&context);
        }
    }
}
