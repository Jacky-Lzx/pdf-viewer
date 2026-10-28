use super::{CommentFilter, Pagewing, ReviewStatus};
use crate::comment_cards::card_lines;
use eframe::egui::{self, Vec2};

fn comment_card(
    ui: &mut egui::Ui,
    lines: &[String; 3],
    active: bool,
    matched: bool,
    height: f32,
) -> egui::Response {
    let response = ui.add_sized(
        egui::vec2(ui.available_width(), height),
        egui::Button::selectable(active, ""),
    );
    response.widget_info(|| {
        egui::WidgetInfo::labeled(egui::WidgetType::Button, ui.is_enabled(), lines.join("\n"))
    });
    let inner = response.rect.shrink2(ui.spacing().button_padding);
    let line_height = ui.text_style_height(&egui::TextStyle::Button);
    for (line, text) in lines.iter().enumerate() {
        let color = if matched && line == 1 {
            ui.visuals().text_color()
        } else {
            ui.visuals().weak_text_color()
        };
        let galley = egui::WidgetText::from(text.as_str()).into_galley(
            ui,
            Some(egui::TextWrapMode::Truncate),
            inner.width().max(0.0),
            egui::TextStyle::Button,
        );
        ui.painter().galley(
            inner.min + Vec2::new(0.0, line as f32 * line_height),
            galley,
            color,
        );
    }
    response
}

impl Pagewing {
    pub(super) fn comment_filters(&mut self, ui: &mut egui::Ui) {
        ui.add(
            egui::TextEdit::singleline(&mut self.comments.view.filter.query)
                .id(egui::Id::new("pagewing-comment-filter"))
                .desired_width(f32::INFINITY)
                .hint_text("Filter text or author…"),
        )
        .on_hover_text("Filter comments · ↑/↓ or Ctrl+J/K: select · PageUp/PageDown: move by visible cards · Ctrl/Cmd+Home/End: first/last · Enter: open");
        let width = ui.available_width();
        let mut controls = |ui: &mut egui::Ui| {
            let status = self
                .comments
                .view
                .filter
                .review
                .map_or("All statuses", ReviewStatus::label);
            egui::ComboBox::from_id_salt("pagewing-comment-review-filter")
                .selected_text(status)
                .width(width.min(100.0))
                .truncate()
                .show_ui(ui, |ui| {
                    ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Wrap);
                    ui.selectable_value(
                        &mut self.comments.view.filter.review,
                        None,
                        "All statuses",
                    );
                    for status in ReviewStatus::ALL {
                        ui.selectable_value(
                            &mut self.comments.view.filter.review,
                            Some(status),
                            status.label(),
                        );
                    }
                })
                .response
                .on_hover_text(status);
            ui.checkbox(&mut self.comments.view.filter.marked, "Marked only");
            ui.checkbox(&mut self.comments.view.filter.current_page, "Current page");
            if self.comments.view.filter != CommentFilter::default()
                && ui.small_button("Clear filters").clicked()
            {
                self.comments.view.filter = CommentFilter::default();
            }
        };
        if width < 180.0 {
            ui.scope(|ui| {
                ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Wrap);
                controls(ui);
            });
        } else {
            ui.horizontal_wrapped(controls);
        }
    }

    pub(super) fn comment_list(&mut self, ui: &mut egui::Ui, list_height: f32) -> Option<usize> {
        // Keep egui's existing minimum explicit when resolving page motions.
        const MIN_HEIGHT: f32 = 64.0;
        let mut selected = None;
        let row_height = ui.text_style_height(&egui::TextStyle::Button) * 3.0
            + ui.spacing().button_padding.y * 2.0;
        let viewport_height = ui.available_height().min(list_height).max(MIN_HEIGHT);
        let keyboard_focus = self.comment_filter_focused(ui.ctx());
        if !ui.is_sizing_pass() {
            let page_rows = crate::list_navigation::page_rows(
                viewport_height,
                row_height,
                ui.spacing().item_spacing.y,
            );
            let mut motions = std::mem::take(&mut self.comments.filter_motions);
            for motion in motions.drain(..) {
                if keyboard_focus {
                    self.comments.view.step(motion.delta(page_rows));
                }
            }
            self.comments.filter_motions = motions;
        }
        let layout_id = ui.make_persistent_id("pagewing-comment-list-layout");
        let layout = (ui.available_width(), viewport_height, row_height);
        let resized = !ui.is_sizing_pass()
            && ui.data_mut(|data| {
                let previous = data.get_temp::<(f32, f32, f32)>(layout_id);
                data.insert_temp(layout_id, layout);
                previous != Some(layout)
            });
        let reveal = !ui.is_sizing_pass() && std::mem::take(&mut self.comments.view.reveal);
        let target = if reveal || (resized && keyboard_focus) {
            self.comments.view.selected_row()
        } else if resized {
            self.comments.editor.as_ref().and_then(|editor| {
                self.comments.view.rows().iter().position(|row| {
                    let comment = &self.comments.comments.items[row.index];
                    editor.page == comment.page && editor.slot == comment.slot
                })
            })
        } else {
            None
        };
        let list = egui::ScrollArea::vertical()
            .id_salt("pagewing-comment-list")
            .auto_shrink([false, false])
            .animated(false)
            .min_scrolled_height(MIN_HEIGHT)
            .max_height(list_height);
        let busy = self.comments.busy();
        let response = list.show_rows(
            ui,
            row_height,
            self.comments.view.rows().len(),
            |ui, range| {
                if let Some(row) = target {
                    // show_rows only builds visible cards. Compute the target
                    // from its virtual row so distant selections can be shown
                    // without laying out the intervening comments.
                    let top = ui.max_rect().top()
                        + (row as f32 - range.start as f32)
                            * (row_height + ui.spacing().item_spacing.y);
                    let rect = egui::Rect::from_min_size(
                        egui::pos2(ui.max_rect().left(), top),
                        egui::vec2(ui.available_width(), row_height),
                    );
                    ui.scroll_to_rect(rect, None);
                }
                for row in range {
                    let crate::comment_view::CommentRow {
                        index,
                        depth,
                        matched,
                    } = self.comments.view.rows()[row];
                    let comment = &self.comments.comments.items[index];
                    let active = if keyboard_focus {
                        self.comments.view.selected() == Some(index)
                    } else {
                        self.comments.editor.as_ref().is_some_and(|current| {
                            current.page == comment.page && current.slot == comment.slot
                        })
                    };
                    let lines = self.comments.cards.get_or_insert_with(index, matched, || {
                        card_lines(comment, self.labels.display_name(comment.page), matched)
                    });
                    ui.horizontal(|ui| {
                        ui.add_space(depth.min(6) as f32 * 14.0);
                        if ui
                            .add_enabled_ui(!self.loading && !busy, |ui| {
                                comment_card(ui, lines, active, matched, row_height)
                            })
                            .inner
                            .on_hover_ui(|ui| {
                                ui.set_max_width(ui.spacing().tooltip_width);
                                ui.label(&comment.author);
                                ui.label(if comment.contents.is_empty() {
                                    "(no comment)"
                                } else {
                                    &comment.contents
                                });
                            })
                            .clicked()
                        {
                            selected = Some(index);
                        }
                    });
                }
            },
        );
        if resized && target.is_some() {
            // The editor can scroll the outer panel in a small window.
            // Revealing a virtual card must also reveal its list viewport.
            ui.scroll_to_rect(response.inner_rect, None);
        }
        selected
    }
}
