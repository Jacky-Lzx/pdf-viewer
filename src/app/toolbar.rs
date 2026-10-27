use super::{Pagewing, appearance, set_theme, shortcuts};
use crate::navigation::Fit;
use eframe::egui::{self, RichText, scroll_area::ScrollAreaOutput};

// Match egui's text tooltip layout while deferring shortcut formatting until
// the tooltip is actually shown. Ordinary toolbar frames only create a closure.
trait LazyTooltip {
    fn on_hover_text_lazy(self, text: impl FnOnce() -> String) -> Self;
}

impl LazyTooltip for egui::Response {
    fn on_hover_text_lazy(self, text: impl FnOnce() -> String) -> Self {
        self.on_hover_ui(|ui| {
            ui.set_max_width(ui.spacing().tooltip_width);
            ui.add(egui::Label::new(text()));
        })
    }
}

impl Pagewing {
    pub(super) fn toolbar(&mut self, context: &egui::Context) {
        self.input_buttons = shortcuts::InputButtons::default();
        egui::TopBottomPanel::top("toolbar")
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
                    if ui
                        .button("Open")
                        .on_hover_text_lazy(|| self.library.shortcuts.label("open"))
                        .clicked()
                    {
                        self.choose_file();
                    }
                    if !self.reading.path.as_os_str().is_empty()
                        && appearance::tool(ui, appearance::Icon::Close, "Close document", false)
                            .on_hover_text_lazy(|| self.library.shortcuts.label("close"))
                            .clicked()
                    {
                        self.close_document(context);
                    }
                    if self.nav.count > 0 {
                        if appearance::tool(
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
                            appearance::tool(
                                ui,
                                appearance::Icon::Comments,
                                "Comments",
                                self.comments.open,
                            )
                            .on_hover_text_lazy(|| self.library.shortcuts.label("comments"))
                            .id,
                        );
                        if !self.selection.rects.is_empty() && ui.button("Add comment").clicked() {
                            self.add_comment();
                        }
                        ui.separator();
                        if appearance::tool(ui, appearance::Icon::Previous, "Previous page", false)
                            .on_hover_text_lazy(|| self.library.shortcuts.label("previous_page"))
                            .clicked()
                        {
                            self.step(-1);
                        }
                        if appearance::tool(ui, appearance::Icon::Next, "Next page", false)
                            .on_hover_text_lazy(|| self.library.shortcuts.label("next_page"))
                            .clicked()
                        {
                            self.step(1);
                        }
                        ui.separator();
                        if appearance::tool(ui, appearance::Icon::Rotate, "Rotate page", false)
                            .on_hover_text_lazy(|| self.library.shortcuts.label("rotate"))
                            .clicked()
                        {
                            self.rotate_page(false, 1);
                        }
                    }
                    right_group(ui, if self.nav.count > 0 { 170.0 } else { 90.0 }, |ui| {
                        ui.add_space(12.0);
                        self.input_buttons.help = Some(
                            appearance::tool(
                                ui,
                                appearance::Icon::Help,
                                "Keyboard shortcuts",
                                false,
                            )
                            .id,
                        );
                        if appearance::tool(
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
                                appearance::tool(
                                    ui,
                                    appearance::Icon::Search,
                                    "Find in document",
                                    self.search_open,
                                )
                                .on_hover_text_lazy(|| self.library.shortcuts.label("find"))
                                .id,
                            );
                            if appearance::tool(
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
    }

    pub(super) fn view_controls(&mut self, ui: &mut egui::Ui) {
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
        let mode = if self.reading.facing {
            "Two pages"
        } else if self.reading.continuous {
            "Continuous"
        } else {
            "Single page"
        };
        ui.menu_button(format!("View: {mode}"), |ui| {
            ui.label("Page layout");
            for (label, continuous, facing) in [
                ("Single page", false, false),
                ("Continuous", true, false),
                ("Two pages side by side", false, true),
            ] {
                if ui
                    .selectable_label(
                        self.reading.continuous == continuous && self.reading.facing == facing,
                        label,
                    )
                    .clicked()
                {
                    self.set_display_mode(continuous, facing);
                    ui.close();
                }
            }
            ui.separator();
            ui.label("Page size");
            for (label, fit) in [
                (
                    format!("Fit page · {}", self.library.shortcuts.label("fit_page")),
                    Fit::Page,
                ),
                (
                    format!("Fit width · {}", self.library.shortcuts.label("fit_width")),
                    Fit::Width,
                ),
                (
                    format!(
                        "Fit height · {}",
                        self.library.shortcuts.label("fit_height")
                    ),
                    Fit::Height,
                ),
                (
                    format!(
                        "Actual size · {}",
                        self.library.shortcuts.label("actual_size")
                    ),
                    Fit::Custom(1.0),
                ),
            ] {
                if ui
                    .selectable_label(self.reading.fit == fit, label)
                    .clicked()
                {
                    if fit == Fit::Custom(1.0) {
                        self.actual_size();
                    } else {
                        self.resolve_view();
                        self.reading.fit = fit;
                        self.pan = egui::Vec2::ZERO;
                        self.dirty = true;
                    }
                    ui.close();
                }
            }
        })
        .response
        .on_hover_text("Choose page layout and size");
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
                let state_id = ui.id().with("focused-control");
                let state = (ui.memory(|memory| memory.focused()), ui.clip_rect().width());
                let previous = ui.data_mut(|data| {
                    let previous = data.get_temp::<(Option<egui::Id>, f32)>(state_id);
                    data.insert_temp(state_id, state);
                    previous
                });
                // Reveal a newly focused control or preserve it across a
                // resize. Do not undo a user's subsequent wheel/drag scroll.
                if previous != Some(state)
                    && let Some(id) = state.0
                {
                    let response = {
                        // read_response also processes outside clicks. Inspect
                        // geometry without undoing an input's explicit focus.
                        let _focus = super::shortcuts::preserve_pointer_focus(ui.ctx());
                        ui.ctx().read_response(id)
                    };
                    if let Some(response) = response
                        && response.layer_id == ui.layer_id()
                        && ui.min_rect().contains_rect(response.rect)
                    {
                        response.scroll_to_me_animation(None, egui::style::ScrollAnimation::none());
                    }
                }
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
