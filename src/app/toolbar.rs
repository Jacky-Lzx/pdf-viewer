use super::{Pagewing, appearance, set_theme, shortcuts, tooltips::LazyTooltip};
use eframe::egui::{self, RichText, scroll_area::ScrollAreaOutput};

impl Pagewing {
    pub(super) fn toolbar_control_focused(&self, context: &egui::Context) -> bool {
        shortcuts::focused_in_region(context, self.toolbar_region)
    }

    pub(super) fn toolbar(&mut self, context: &egui::Context) {
        self.input_buttons = shortcuts::InputButtons::default();
        self.toolbar_region = None;
        if self.controls_hidden {
            return;
        }
        let panel = egui::TopBottomPanel::top("toolbar")
            .frame(appearance::panel(context, 4))
            .exact_height(56.0)
            .show(context, |ui| {
                let compact = ui.available_width() < 1000.0;
                horizontal_bar(ui, "reader-controls", |ui| {
                    ui.add_space(12.0);
                    appearance::logo(ui, 30.0);
                    if !compact {
                        ui.label(RichText::new("Pagewing").size(19.0).strong());
                    }
                    ui.add_space(16.0);
                    if appearance::action_button(ui, "open", egui::Button::new("Open"))
                        .on_hover_text_lazy(|| self.library.shortcuts.label("open"))
                        .clicked()
                    {
                        self.choose_file();
                    }
                    if !self.reading.path.as_os_str().is_empty()
                        && appearance::toolbar_tool(
                            ui,
                            appearance::Icon::Close,
                            "Close document",
                            false,
                        )
                        .on_hover_text_lazy(|| self.library.shortcuts.label("close"))
                        .clicked()
                    {
                        self.close_document(context);
                    }
                    if self.nav.count > 0 {
                        if appearance::toolbar_tool(
                            ui,
                            appearance::Icon::Sidebar,
                            "Sidebar",
                            self.library.sidebar,
                        )
                        .on_hover_text_lazy(|| self.library.shortcuts.label("sidebar"))
                        .clicked()
                        {
                            self.library.sidebar = !self.library.sidebar;
                            self.dirty = true;
                        }
                        self.input_buttons.comments = Some(
                            appearance::toolbar_tool(
                                ui,
                                appearance::Icon::Comments,
                                "Comments",
                                self.comments.open,
                            )
                            .on_hover_text_lazy(|| self.library.shortcuts.label("comments"))
                            .id,
                        );
                        if !self.selection.rects.is_empty()
                            && appearance::action_button(
                                ui,
                                "add-comment",
                                egui::Button::new("Add comment"),
                            )
                            .clicked()
                        {
                            self.add_comment(ui.ctx());
                        }
                        ui.separator();
                        if appearance::toolbar_tool(
                            ui,
                            appearance::Icon::Previous,
                            "Previous page",
                            false,
                        )
                        .on_hover_text_lazy(|| self.library.shortcuts.label("previous_page"))
                        .clicked()
                        {
                            self.step(-1);
                        }
                        if appearance::toolbar_tool(ui, appearance::Icon::Next, "Next page", false)
                            .on_hover_text_lazy(|| self.library.shortcuts.label("next_page"))
                            .clicked()
                        {
                            self.step(1);
                        }
                        ui.separator();
                        if appearance::toolbar_tool(
                            ui,
                            appearance::Icon::Rotate,
                            "Rotate page",
                            false,
                        )
                        .on_hover_text_lazy(|| self.library.shortcuts.label("rotate"))
                        .clicked()
                        {
                            self.rotate_page(false, 1);
                        }
                        ui.separator();
                        self.history_controls(ui);
                    }
                    self.input_buttons.reading_controls = Some(
                        appearance::action_button(
                            ui,
                            "hide-reading-controls",
                            egui::Button::new("Hide controls"),
                        )
                        .on_hover_text_lazy(|| {
                            format!(
                                "More room for reading · {}",
                                self.library.shortcuts.label("reading_controls")
                            )
                        })
                        .id,
                    );
                    self.plugin_menu(ui);
                    right_group(ui, if self.nav.count > 0 { 170.0 } else { 90.0 }, |ui| {
                        ui.add_space(12.0);
                        self.input_buttons.help = Some(
                            appearance::toolbar_tool(
                                ui,
                                appearance::Icon::Help,
                                "Keyboard shortcuts",
                                false,
                            )
                            .id,
                        );
                        if appearance::toolbar_tool(
                            ui,
                            appearance::Icon::Theme,
                            if self.library.dark {
                                "Switch to light theme"
                            } else {
                                "Switch to dark theme"
                            },
                            false,
                        )
                        .clicked()
                        {
                            self.library.dark = !self.library.dark;
                            set_theme(context, self.library.dark);
                            self.dirty = true;
                        }
                        if self.nav.count > 0 {
                            self.input_buttons.find = Some(
                                appearance::toolbar_tool(
                                    ui,
                                    appearance::Icon::Search,
                                    "Find in document",
                                    self.search_open,
                                )
                                .on_hover_text_lazy(|| self.library.shortcuts.label("find"))
                                .id,
                            );
                            if appearance::toolbar_tool(
                                ui,
                                appearance::Icon::Bookmark,
                                "Bookmark",
                                self.reading.bookmarks.contains(&self.nav.page),
                            )
                            .on_hover_text_lazy(|| self.library.shortcuts.label("bookmark"))
                            .clicked()
                            {
                                self.bookmark();
                            }
                        }
                    });
                });
            });
        self.toolbar_region = Some((panel.response.layer_id, panel.response.rect));
    }

    pub(super) fn view_controls(&mut self, ui: &mut egui::Ui) {
        // Notices, recovery actions and numeric prefixes precede this group.
        // Its permanent controls keep their IDs as that status content changes.
        ui.scope_builder(egui::UiBuilder::new().id("reader-view-controls"), |ui| {
            self.view_controls_row(ui)
        });
    }

    fn view_controls_row(&mut self, ui: &mut egui::Ui) {
        if ui
            .button("+")
            .on_hover_text_lazy(|| self.library.shortcuts.label("zoom_in"))
            .clicked()
        {
            self.zoom(1.2);
        }
        let percent = self.scale * 100.0;
        let label = if percent < 0.01 {
            "<0.01%".to_owned()
        } else if percent < 1.0 {
            format!("{percent:.2}%")
        } else {
            format!("{percent:.0}%")
        };
        self.input_buttons.zoom = Some(
            ui.add_enabled(!self.loading, egui::Button::new(label).frame(false))
                .on_hover_text_lazy(|| {
                    format!(
                        "Set zoom percentage · {}",
                        self.library.shortcuts.label("zoom")
                    )
                })
                .id,
        );
        if ui
            .button("−")
            .on_hover_text_lazy(|| self.library.shortcuts.label("zoom_out"))
            .clicked()
        {
            self.zoom(1.0 / 1.2);
        }
        ui.separator();
        self.view_menu(ui);
    }
}

/// A single row that keeps every control reachable without widening the window.
pub(super) fn horizontal_bar<R>(
    ui: &mut egui::Ui,
    id: &str,
    contents: impl FnOnce(&mut egui::Ui) -> R,
) -> ScrollAreaOutput<R> {
    ui.style_mut().always_scroll_the_only_direction = true;
    egui::ScrollArea::horizontal()
        .id_salt(id)
        .auto_shrink([false, false])
        .min_scrolled_width(0.0)
        .show(ui, |ui| {
            ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Extend);
            ui.horizontal_centered(|ui| {
                let inner = contents(ui);
                shortcuts::reveal_focused_control(ui, egui::vec2(ui.clip_rect().width(), 0.0));
                inner
            })
            .inner
        })
}

/// Extend the scrollable row before right-aligning its trailing controls.
pub(super) fn right_group<R>(
    ui: &mut egui::Ui,
    minimum_width: f32,
    contents: impl FnOnce(&mut egui::Ui) -> R,
) -> R {
    ui.allocate_ui_with_layout(
        egui::vec2(
            ui.available_width().max(minimum_width),
            ui.available_height(),
        ),
        egui::Layout::right_to_left(egui::Align::Center),
        contents,
    )
    .inner
}

#[cfg(test)]
mod tests {
    use super::*;
    use egui::{Context, Event, Modifiers, MouseWheelUnit, PointerButton, RawInput, Rect, Vec2};

    fn reader_frame(app: &mut Pagewing, context: &Context, width: f32, events: Vec<Event>) {
        let _ = context.run(
            RawInput {
                screen_rect: Some(Rect::from_min_size(
                    egui::Pos2::ZERO,
                    Vec2::new(width, 400.0),
                )),
                events,
                ..Default::default()
            },
            |context| {
                app.keys(context);
                app.toolbar(context);
                egui::CentralPanel::default().show(context, |_| {});
            },
        );
    }

    #[test]
    fn reader_toolbar_focus_stays_on_its_action_when_controls_reflow() {
        use super::shortcuts::InputButton;
        for action in [InputButton::Help, InputButton::Find, InputButton::Comments] {
            let root = tempfile::tempdir().unwrap();
            let context = Context::default();
            let mut app =
                Pagewing::with_context(&context, None, None, root.path().join("library.json"));
            app.nav = crate::navigation::Navigation::new(6, 2);
            app.dimensions = vec![[600.0, 800.0]; 6];
            reader_frame(&mut app, &context, 1180.0, Vec::new());
            let target = |app: &Pagewing| {
                match action {
                    InputButton::Help => app.input_buttons.help,
                    InputButton::Find => app.input_buttons.find,
                    InputButton::Comments => app.input_buttons.comments,
                    _ => unreachable!(),
                }
                .unwrap()
            };
            let focused = target(&app);
            context.memory_mut(|memory| memory.request_focus(focused));
            for (width, selected) in [
                (900.0, false),
                (240.0, false),
                (1180.0, false),
                (240.0, false),
                (240.0, true),
                (240.0, false),
            ] {
                app.selection.rects = if selected {
                    vec![hayro::kurbo::Rect::new(10.0, 20.0, 80.0, 30.0)]
                } else {
                    Vec::new()
                };
                for _ in 0..5 {
                    reader_frame(&mut app, &context, width, Vec::new());
                }
                assert_eq!(
                    target(&app),
                    focused,
                    "{action:?}, width {width}, selection {selected}: preserve the action identity"
                );
                assert_eq!(context.memory(|memory| memory.focused()), Some(focused));
                let response = context.read_response(focused).unwrap();
                assert!(
                    response.interact_rect.contains_rect(response.rect),
                    "{action:?}, width {width}, selection {selected}: reveal the actual focused button"
                );
                assert_eq!(app.nav.page, 2);
                assert!(!app.help && !app.search_open && !app.comments.open);
            }
            reader_frame(
                &mut app,
                &context,
                240.0,
                [true, false]
                    .map(|pressed| Event::Key {
                        key: egui::Key::Space,
                        physical_key: None,
                        pressed,
                        repeat: false,
                        modifiers: Modifiers::NONE,
                    })
                    .into(),
            );
            assert_eq!(app.help, action == InputButton::Help);
            assert_eq!(app.search_open, action == InputButton::Find);
            assert_eq!(app.comments.open, action == InputButton::Comments);
            assert_eq!(app.nav.page, 2);
        }
    }

    #[test]
    fn toolbar_tab_and_button_activation_leave_the_reading_view_unchanged() {
        let root = tempfile::tempdir().unwrap();
        let context = Context::default();
        let mut app =
            Pagewing::with_context(&context, None, None, root.path().join("library.json"));
        app.nav = crate::navigation::Navigation::new(6, 2);
        app.dimensions = vec![[600.0, 800.0]; 6];
        app.viewport = Vec2::new(600.0, 500.0);
        app.overflow = Vec2::new(800.0, 1000.0);
        app.pan = Vec2::new(37.0, 53.0);
        let sidebar = app.library.sidebar;
        reader_frame(&mut app, &context, 1180.0, Vec::new());
        let find = app.input_buttons.find.unwrap();
        context.memory_mut(|memory| memory.request_focus(find));
        reader_frame(&mut app, &context, 1180.0, Vec::new());
        let key = |key, modifiers| {
            [true, false]
                .map(|pressed| Event::Key {
                    key,
                    physical_key: None,
                    pressed,
                    repeat: false,
                    modifiers,
                })
                .to_vec()
        };
        reader_frame(
            &mut app,
            &context,
            1180.0,
            key(egui::Key::Tab, Modifiers::NONE),
        );
        assert_eq!(
            app.library.sidebar, sidebar,
            "Tab on the toolbar must traverse controls"
        );
        let bookmark = context.memory(|memory| memory.focused()).unwrap();
        assert_ne!(bookmark, find);
        for width in [240.0, 1180.0, 240.0] {
            for _ in 0..5 {
                reader_frame(&mut app, &context, width, Vec::new());
            }
            assert_eq!(context.memory(|memory| memory.focused()), Some(bookmark));
            let response = context.read_response(bookmark).unwrap();
            assert!(response.interact_rect.contains_rect(response.rect));
        }
        reader_frame(
            &mut app,
            &context,
            240.0,
            key(egui::Key::Space, Modifiers::NONE),
        );
        assert!(
            app.reading.bookmarks.contains(&2),
            "Space must activate the focused Bookmark button"
        );
        assert_eq!(
            app.pan,
            Vec2::new(37.0, 53.0),
            "button activation must not scroll the PDF"
        );
        assert_eq!(app.nav.page, 2);
        reader_frame(
            &mut app,
            &context,
            240.0,
            key(egui::Key::Tab, Modifiers::SHIFT),
        );
        // Reverse traversal publishes its target on the following egui frame.
        reader_frame(&mut app, &context, 240.0, Vec::new());
        assert_eq!(context.memory(|memory| memory.focused()), Some(find));
        assert_eq!(app.library.sidebar, sidebar);
        reader_frame(
            &mut app,
            &context,
            240.0,
            key(egui::Key::Escape, Modifiers::NONE),
        );
        assert_eq!(context.memory(|memory| memory.focused()), None);
        reader_frame(
            &mut app,
            &context,
            240.0,
            key(egui::Key::Tab, Modifiers::NONE),
        );
        assert_eq!(
            app.library.sidebar, !sidebar,
            "Tab keeps its reading shortcut after Escape"
        );
    }

    #[test]
    fn shortcut_tooltips_format_only_when_visible_and_follow_current_bindings() {
        let context = Context::default();
        context.style_mut(|style| style.interaction.tooltip_delay = 0.0);
        let mut shortcuts = crate::settings::Shortcuts::default();
        let formats = std::cell::Cell::new(0);
        let mut position = egui::Pos2::new(500.0, 400.0);
        let mut button = Rect::NOTHING;
        let mut time = 0.0;
        let mut draw = |shortcuts: &crate::settings::Shortcuts, position| {
            time += 0.25;
            let mut button = Rect::NOTHING;
            let output = context.run(
                RawInput {
                    screen_rect: Some(Rect::from_min_size(
                        egui::Pos2::ZERO,
                        Vec2::new(600.0, 500.0),
                    )),
                    time: Some(time),
                    events: vec![Event::PointerMoved(position)],
                    ..Default::default()
                },
                |context| {
                    egui::CentralPanel::default().show(context, |ui| {
                        let response = ui.button("Comments").on_hover_text_lazy(|| {
                            formats.set(formats.get() + 1);
                            shortcuts.label("comments")
                        });
                        button = response.rect;
                    });
                },
            );
            let text = output
                .shapes
                .iter()
                .filter_map(|shape| match &shape.shape {
                    egui::Shape::Text(text) => Some(text.galley.text()),
                    _ => None,
                })
                .collect::<Vec<_>>()
                .join("\n");
            (button, text)
        };
        for _ in 0..4 {
            button = draw(&shortcuts, position).0;
        }
        assert_eq!(formats.get(), 0, "hidden tooltips must not format labels");
        position = button.center();
        for bindings in [
            vec![crate::settings::Binding::parse_config("Cmd+Alt+M").unwrap()],
            Vec::new(),
        ] {
            shortcuts.overrides.insert("comments".into(), bindings);
            let mut text = String::new();
            for _ in 0..12 {
                text = draw(&shortcuts, position).1;
            }
            assert!(formats.get() > 0);
            assert!(text.contains(&shortcuts.label("comments")), "{text}");
        }
    }

    fn frame(
        context: &Context,
        width: f32,
        events: Vec<Event>,
    ) -> (ScrollAreaOutput<()>, egui::Response, bool) {
        let mut output = None;
        let mut last = None;
        let mut clicked = false;
        let _ = context.run(
            RawInput {
                screen_rect: Some(Rect::from_min_size(
                    egui::Pos2::ZERO,
                    Vec2::new(width, 240.0),
                )),
                events,
                ..Default::default()
            },
            |context| {
                egui::TopBottomPanel::top("toolbar")
                    .exact_height(56.0)
                    .show(context, |ui| {
                        output = Some(horizontal_bar(ui, "controls", |ui| {
                            let mut left_end = ui.cursor().left();
                            for label in [
                                "Open",
                                "Close",
                                "Sidebar",
                                "Previous",
                                "Next",
                                "Zoom out",
                                "Zoom in",
                                "Fit",
                                "Width",
                                "Rotate",
                                "Continuous",
                            ] {
                                left_end = ui.button(label).rect.right();
                            }
                            right_group(ui, 280.0, |ui| {
                                for label in ["Help", "Dark", "Find", "Bookmark"] {
                                    let response = ui.button(label);
                                    assert!(
                                        response.rect.left() >= left_end,
                                        "trailing controls must not overlap the left group"
                                    );
                                    if label == "Help" {
                                        clicked = response.clicked();
                                        last = Some(response);
                                    }
                                }
                            });
                        }));
                    });
                egui::TopBottomPanel::top("search")
                    .exact_height(42.0)
                    .show(context, |ui| {
                        horizontal_bar(ui, "search-controls", |ui| {
                            ui.label("Find results");
                        });
                    });
                egui::TopBottomPanel::bottom("status")
                    .exact_height(29.0)
                    .show(context, |ui| {
                        ui.label("NORMAL");
                    });
                egui::CentralPanel::default().show(context, |ui| {
                    assert!(
                        ui.max_rect().shrink(24.0).is_positive(),
                        "a short window must retain a usable page viewport"
                    );
                });
            },
        );
        let output = output.unwrap();
        assert!(output.inner_rect.width() <= width);
        assert!(output.content_size.x.is_finite());
        assert!(output.content_size.y < 56.0, "buttons must stay on one row");
        (output, last.unwrap(), clicked)
    }

    #[test]
    fn narrow_bars_scroll_to_and_activate_the_last_button_without_widening_the_viewport() {
        for width in [240.0, 360.0, 860.0, 1180.0] {
            let context = Context::default();
            frame(&context, width, Vec::new());
            let (output, _, _) = frame(
                &context,
                width,
                vec![
                    Event::PointerMoved(egui::pos2(width * 0.5, 24.0)),
                    Event::MouseWheel {
                        unit: MouseWheelUnit::Point,
                        delta: Vec2::new(0.0, -4000.0),
                        modifiers: Modifiers::NONE,
                    },
                ],
            );
            if output.content_size.x > output.inner_rect.width() {
                assert!(
                    output.state.offset.x > 0.0,
                    "vertical wheel must scroll the row horizontally"
                );
            }
            let mut state = output.state;
            state.offset.x = (output.content_size.x - output.inner_rect.width()).max(0.0);
            state.store(&context, output.id);
            // Reach the right edge, then click the previously hidden last control.
            let (output, last, _) = frame(&context, width, Vec::new());
            assert!(output.inner_rect.contains(last.rect.center()));
            frame(
                &context,
                width,
                vec![
                    Event::PointerMoved(last.rect.center()),
                    Event::PointerButton {
                        pos: last.rect.center(),
                        button: PointerButton::Primary,
                        pressed: true,
                        modifiers: Modifiers::NONE,
                    },
                ],
            );
            let (_, _, clicked) = frame(
                &context,
                width,
                vec![Event::PointerButton {
                    pos: last.rect.center(),
                    button: PointerButton::Primary,
                    pressed: false,
                    modifiers: Modifiers::NONE,
                }],
            );
            assert!(clicked, "last control must remain usable at width {width}");
        }
    }

    #[test]
    fn focused_control_is_revealed_on_resize_but_manual_scrolling_is_preserved() {
        let context = Context::default();
        let (_, target, _) = frame(&context, 1180.0, Vec::new());
        context.memory_mut(|memory| memory.request_focus(target.id));
        for width in [1180.0, 360.0, 240.0, 860.0, 240.0] {
            for _ in 0..3 {
                frame(&context, width, Vec::new());
            }
            let (output, target, _) = frame(&context, width, Vec::new());
            assert!(target.has_focus());
            assert!(
                output.inner_rect.contains_rect(target.rect),
                "focused control must remain visible at width {width}: {:?}",
                target.rect
            );
        }
        let (output, _, _) = frame(
            &context,
            240.0,
            vec![
                Event::PointerMoved(egui::pos2(120.0, 24.0)),
                Event::MouseWheel {
                    unit: MouseWheelUnit::Point,
                    delta: Vec2::new(0.0, 4000.0),
                    modifiers: Modifiers::NONE,
                },
            ],
        );
        assert_eq!(output.state.offset.x, 0.0);
        for _ in 0..3 {
            let (output, target, _) = frame(&context, 240.0, Vec::new());
            assert!(target.has_focus());
            assert_eq!(output.state.offset.x, 0.0, "do not fight manual scrolling");
        }
    }
}
