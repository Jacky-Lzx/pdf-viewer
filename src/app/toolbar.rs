use super::Pagewing;
use crate::navigation::Fit;
use eframe::egui::{self, scroll_area::ScrollAreaOutput};

impl Pagewing {
    pub(super) fn view_controls(&mut self, ui: &mut egui::Ui) {
        if ui
            .button("+")
            .on_hover_text(self.library.shortcuts.label("zoom_in"))
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
                .on_hover_text("Set zoom percentage")
                .id,
        );
        if ui
            .button("−")
            .on_hover_text(self.library.shortcuts.label("zoom_out"))
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
            ui.horizontal_centered(contents).inner
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

    fn frame(
        context: &Context,
        width: f32,
        events: Vec<Event>,
    ) -> (ScrollAreaOutput<()>, Rect, bool) {
        let mut output = None;
        let mut last = Rect::NOTHING;
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
                                        last = response.rect;
                                        clicked = response.clicked();
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
        (output, last, clicked)
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
            assert!(output.inner_rect.contains(last.center()));
            frame(
                &context,
                width,
                vec![
                    Event::PointerMoved(last.center()),
                    Event::PointerButton {
                        pos: last.center(),
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
                    pos: last.center(),
                    button: PointerButton::Primary,
                    pressed: false,
                    modifiers: Modifiers::NONE,
                }],
            );
            assert!(clicked, "last control must remain usable at width {width}");
        }
    }
}
