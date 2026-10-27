use super::{ACCENT, Folio, MarkAction};
use crate::navigation::Fit;
use eframe::egui::{self, Color32, RichText, Vec2};

impl Folio {
    pub(super) fn focus_contents(&mut self) {
        if self.outline.is_empty() {
            self.notify("This document has no chapter outline");
            return;
        }
        self.nav.clear_prefix();
        self.hints.cancel();
        self.selection.cancel();
        self.mark_action = None;
        self.search_open = false;
        self.search_focus = false;
        self.pending_search_jump = None;
        self.library.sidebar = true;
        self.outline.focus = true;
        self.outline.reveal = true;
        self.dirty = true;
    }

    fn contents(&mut self, ui: &mut egui::Ui) {
        if self.outline.is_empty() {
            return;
        }
        let mut target = None;
        let mut toggle = None;
        let mut keyboard_focus = false;
        ui.horizontal(|ui| {
            ui.label(RichText::new("CONTENTS").small().strong().color(ACCENT));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.add_enabled_ui(!self.outline.filtering(), |ui| {
                    if ui
                        .small_button("−")
                        .on_hover_text("Collapse all chapters")
                        .clicked()
                    {
                        self.outline.fold_all(true);
                    }
                    if ui
                        .small_button("+")
                        .on_hover_text("Expand all chapters")
                        .clicked()
                    {
                        self.outline.fold_all(false);
                    }
                });
            });
        });
        ui.horizontal(|ui| {
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let clear = ui
                    .small_button("×")
                    .on_hover_text("Clear chapter filter")
                    .clicked();
                let (response, _) = super::shortcuts::text_input(
                    ui,
                    egui::Id::new("folio-outline-input"),
                    &mut self.outline.focus,
                    &mut self.deferred_input,
                    egui::TextEdit::singleline(&mut self.outline.query)
                        .hint_text("Filter chapters · c")
                        .desired_width(f32::INFINITY),
                );
                let response =
                    response.on_hover_text("Match all keywords in any order, ignoring letter case");
                if clear {
                    self.outline.query.clear();
                }
                if response.changed() || clear {
                    self.outline.refilter();
                }
                keyboard_focus = response.has_focus() || response.lost_focus();
                if !ui.is_sizing_pass() {
                    self.outline.apply_motions();
                    if std::mem::take(&mut self.outline.confirm)
                        && let Some(index) = self.outline.selected()
                    {
                        if let Some(destination) = self.outline.entry(index).destination {
                            target = Some(destination);
                            response.surrender_focus();
                        } else {
                            toggle = Some(index);
                            response.request_focus();
                        }
                    }
                }
            });
        });
        if self.outline.visible().is_empty() {
            ui.weak("No matching chapters.");
        }
        let row_height = Self::sidebar_row_height(ui);
        let mut scroll = egui::ScrollArea::vertical()
            .id_salt("contents")
            .max_height(260.0);
        if keyboard_focus
            && std::mem::take(&mut self.outline.reveal)
            && let Some(row) = self.outline.selected_row()
        {
            scroll = scroll.vertical_scroll_offset(
                row.saturating_sub(3) as f32 * (row_height + ui.spacing().item_spacing.y),
            );
        }
        let mut selected = None;
        scroll.show_rows(ui, row_height, self.outline.visible().len(), |ui, range| {
            for row in range {
                let index = self.outline.visible()[row];
                let entry = self.outline.entry(index);
                ui.push_id(index, |ui| {
                    ui.horizontal(|ui| {
                        ui.add_space(entry.depth.min(8) as f32 * 12.0);
                        if self.outline.has_children(index) {
                            let collapsed = self.outline.is_collapsed(index);
                            let response = ui
                                .add_enabled_ui(!self.outline.filtering(), |ui| {
                                    let response = ui
                                        .allocate_response(Vec2::splat(20.0), egui::Sense::click());
                                    let mut icon = response.clone();
                                    icon.rect = icon.rect.shrink(3.0);
                                    egui::collapsing_header::paint_default_icon(
                                        ui,
                                        if collapsed { 0.0 } else { 1.0 },
                                        &icon,
                                    );
                                    response
                                })
                                .inner;
                            if response
                                .on_hover_text(if collapsed {
                                    "Expand chapter"
                                } else {
                                    "Collapse chapter"
                                })
                                .clicked()
                            {
                                toggle = Some(index);
                            }
                        } else {
                            ui.allocate_space(Vec2::splat(20.0));
                        }
                        if let Some(destination) = entry.destination {
                            let page = destination.page;
                            if ui
                                .add(
                                    egui::Button::selectable(
                                        if keyboard_focus {
                                            self.outline.selected() == Some(index)
                                        } else {
                                            self.nav.page == page
                                        },
                                        &entry.title,
                                    )
                                    .truncate(),
                                )
                                .on_hover_ui(|ui| {
                                    ui.set_max_width(ui.spacing().tooltip_width);
                                    ui.label(format!(
                                        "{}\nPage {}",
                                        entry.title,
                                        self.page_name(page)
                                    ));
                                })
                                .clicked()
                            {
                                selected = Some(index);
                                target = Some(destination);
                            }
                        } else {
                            if ui
                                .add(
                                    egui::Button::selectable(
                                        keyboard_focus && self.outline.selected() == Some(index),
                                        RichText::new(&entry.title).weak(),
                                    )
                                    .truncate(),
                                )
                                .clicked()
                            {
                                selected = Some(index);
                                toggle = Some(index);
                            }
                        }
                    });
                });
            }
        });
        if let Some(index) = selected {
            self.outline.select(index);
        }
        if let Some(index) = toggle {
            self.outline.toggle(index);
        }
        if let Some(destination) = target {
            self.follow_destination(destination);
        }
        ui.add_space(20.0);
    }

    fn sidebar_row_height(ui: &egui::Ui) -> f32 {
        (ui.text_style_height(&egui::TextStyle::Button) + 2.0 * ui.spacing().button_padding.y)
            .max(ui.spacing().interact_size.y)
            .max(20.0)
    }

    fn bookmark_list(&mut self, ui: &mut egui::Ui) {
        let row_height = Self::sidebar_row_height(ui);
        let mut scroll = egui::ScrollArea::vertical()
            .id_salt("bookmarks")
            .max_height(140.0);
        if self.reveal_page.is_some()
            && let Ok(row) = self.bookmark_pages.binary_search(&self.nav.page)
        {
            scroll = scroll
                .vertical_scroll_offset(row as f32 * (row_height + ui.spacing().item_spacing.y));
        }
        let mut target = None;
        scroll.show_rows(ui, row_height, self.bookmark_pages.len(), |ui, range| {
            for row in range {
                let page = self.bookmark_pages[row];
                if ui
                    .add_sized(
                        Vec2::new(ui.available_width(), row_height),
                        egui::Button::selectable(
                            page == self.nav.page,
                            format!("Page {}", self.page_name(page)),
                        )
                        .truncate(),
                    )
                    .clicked()
                {
                    target = Some(page);
                }
            }
        });
        if let Some(page) = target {
            self.jump(page);
        }
    }

    fn page_previews(&mut self, ui: &mut egui::Ui) {
        let mut scroll = egui::ScrollArea::vertical()
            .id_salt("page-previews")
            .max_height((ui.clip_rect().bottom() - ui.cursor().top()).clamp(120.0, 400.0));
        if let Some(page) = self.reveal_page.take() {
            scroll = scroll.vertical_scroll_offset(
                page as f32 * (crate::thumbnails::ROW_HEIGHT + ui.spacing().item_spacing.y),
            );
        }
        let mut target = None;
        scroll.show_rows(
            ui,
            crate::thumbnails::ROW_HEIGHT,
            self.nav.count,
            |ui, range| {
                for page in range {
                    let (rect, response) = ui.allocate_exact_size(
                        Vec2::new(ui.available_width(), crate::thumbnails::ROW_HEIGHT),
                        egui::Sense::click(),
                    );
                    if !ui.is_rect_visible(rect) {
                        continue;
                    }
                    let dimensions = self.dimensions[page];
                    let key = crate::thumbnails::key(
                        page,
                        dimensions,
                        self.reading.rotation,
                        self.library.night,
                        ui.ctx().pixels_per_point(),
                    );
                    let texture = self.thumbnails.want(key);
                    let painter = ui.painter();
                    if page == self.nav.page {
                        painter.rect_filled(rect, 4.0, ACCENT.gamma_multiply(0.15));
                        painter.rect_stroke(
                            rect,
                            4.0,
                            egui::Stroke::new(1.0_f32, ACCENT),
                            egui::StrokeKind::Inside,
                        );
                    } else if response.hovered() {
                        painter.rect_filled(rect, 4.0, ui.visuals().widgets.hovered.bg_fill);
                    }
                    let box_size = crate::thumbnails::IMAGE_SIZE;
                    let center = egui::pos2(rect.center().x, rect.top() + box_size[1] * 0.5 + 4.0);
                    let rotated = if key.rotation % 2 == 1 {
                        [dimensions[1], dimensions[0]]
                    } else {
                        dimensions
                    };
                    let scale = Fit::Page.scale(rotated, box_size);
                    let image_rect = egui::Rect::from_center_size(
                        center,
                        Vec2::new(rotated[0] * scale, rotated[1] * scale),
                    );
                    painter.rect_filled(
                        image_rect,
                        0.0,
                        if key.night {
                            Color32::BLACK
                        } else {
                            Color32::WHITE
                        },
                    );
                    if let Some(texture) = texture {
                        painter.image(
                            texture,
                            image_rect,
                            egui::Rect::from_min_max(egui::Pos2::ZERO, egui::pos2(1.0, 1.0)),
                            Color32::WHITE,
                        );
                    } else {
                        painter.text(
                            center,
                            egui::Align2::CENTER_CENTER,
                            if self.thumbnails.failed(key) {
                                "Unavailable"
                            } else {
                                "Loading…"
                            },
                            egui::FontId::proportional(12.0),
                            if key.night {
                                Color32::LIGHT_GRAY
                            } else {
                                Color32::DARK_GRAY
                            },
                        );
                    }
                    painter.text(
                        egui::pos2(rect.center().x, rect.bottom() - 8.0),
                        egui::Align2::CENTER_BOTTOM,
                        format!("Page {}", self.page_name(page)),
                        egui::FontId::proportional(13.0),
                        ui.visuals().text_color(),
                    );
                    if response
                        .on_hover_ui(|ui| {
                            ui.set_max_width(ui.spacing().tooltip_width);
                            ui.label(format!("Go to page {}", self.page_name(page)));
                        })
                        .clicked()
                    {
                        target = Some(page);
                    }
                }
            },
        );
        if let Some(page) = target {
            self.jump(page);
            // The clicked row is already visible. Keep the preview scroll stable
            // so adjacent pages remain under the pointer after navigation.
            self.reveal_page = None;
        }
    }

    pub(super) fn sidebar(&mut self, context: &egui::Context) {
        self.thumbnails.begin_frame();
        if !self.library.sidebar || self.nav.count == 0 {
            if !self.loading
                && let Some(keys) = self.thumbnails.requests()
            {
                self.worker.thumbnails(keys);
            }
            return;
        }
        egui::SidePanel::left("sidebar")
            .default_width(230.0)
            .width_range(180.0..=380.0)
            .show(context, |ui| {
                ui.add_space(16.0);
                ui.label(RichText::new(self.filename()).strong());
                ui.weak(format!("{} pages", self.nav.count));
                ui.add_space(16.0);
                let mut sidebar_scroll = egui::ScrollArea::vertical();
                if self.outline.focus {
                    sidebar_scroll = sidebar_scroll.vertical_scroll_offset(0.0);
                }
                sidebar_scroll.show(ui, |ui| {
                    self.contents(ui);
                    ui.label(RichText::new("BOOKMARKS").small().strong().color(ACCENT));
                    if self.reading.bookmarks.is_empty() {
                        ui.weak("Press b to bookmark a page.");
                    }
                    if !self.bookmark_pages.is_empty() {
                        self.bookmark_list(ui);
                    }
                    if !self.reading.marks.is_empty() {
                        ui.add_space(20.0);
                        ui.label(RichText::new("MARKS").small().strong().color(ACCENT));
                        let mut target = None;
                        let mut remove = None;
                        for (&name, mark) in &self.reading.marks {
                            ui.horizontal(|ui| {
                                if ui
                                    .selectable_label(
                                        mark.page == self.nav.page,
                                        format!("'{name}   Page {}", mark.page + 1),
                                    )
                                    .on_hover_text(
                                        "Restore page, zoom, rotation and scroll position",
                                    )
                                    .clicked()
                                {
                                    target = Some(name);
                                }
                                if ui.small_button("×").on_hover_text("Delete mark").clicked() {
                                    remove = Some(name);
                                }
                            });
                        }
                        if let Some(name) = target {
                            self.mark(name, MarkAction::Jump);
                        }
                        if let Some(name) = remove {
                            self.reading.marks.remove(&name);
                            self.dirty = true;
                        }
                    }
                    if !self.hits.is_empty() {
                        ui.add_space(20.0);
                        ui.label(
                            RichText::new("SEARCH RESULTS")
                                .small()
                                .strong()
                                .color(ACCENT),
                        );
                        let mut selected = None;
                        let row_height = Self::sidebar_row_height(ui);
                        let mut results = egui::ScrollArea::vertical()
                            .id_salt("search-results")
                            .max_height(
                                (ui.clip_rect().bottom() - ui.cursor().top()).clamp(120.0, 280.0),
                            );
                        if let Some(index) = self.reveal_hit.take() {
                            results = results.vertical_scroll_offset(
                                index as f32 * (row_height + ui.spacing().item_spacing.y),
                            );
                        }
                        results.show_rows(ui, row_height, self.hits.len(), |ui, range| {
                            let mut previous_page = None;
                            let mut page_name = String::new();
                            for index in range {
                                let hit = &self.hits[index];
                                if previous_page != Some(hit.page) {
                                    page_name = self.page_name(hit.page);
                                    previous_page = Some(hit.page);
                                }
                                if ui
                                    .add_sized(
                                        Vec2::new(ui.available_width(), row_height),
                                        egui::Button::selectable(
                                            self.hit_index == Some(index),
                                            format!("{page_name}   {}", hit.snippet),
                                        )
                                        .truncate(),
                                    )
                                    .on_hover_text(&hit.snippet)
                                    .clicked()
                                {
                                    selected = Some(index);
                                }
                            }
                        });
                        if let Some(index) = selected {
                            self.select_hit(index);
                        }
                    }
                    ui.add_space(20.0);
                    ui.horizontal(|ui| {
                        ui.label(RichText::new("PAGES").small().strong().color(ACCENT));
                        let changed = ui
                            .selectable_value(&mut self.library.thumbnails, false, "List")
                            .changed()
                            | ui.selectable_value(&mut self.library.thumbnails, true, "Previews")
                                .changed();
                        if changed {
                            self.dirty = true;
                            self.reveal_page = Some(self.nav.page);
                        }
                    });
                    if self.library.thumbnails {
                        self.page_previews(ui);
                    } else {
                        let row_height = Self::sidebar_row_height(ui);
                        let mut scroll = egui::ScrollArea::vertical()
                            .id_salt("page-list")
                            .max_height(
                                (ui.clip_rect().bottom() - ui.cursor().top()).clamp(120.0, 300.0),
                            );
                        if let Some(page) = self.reveal_page.take() {
                            scroll = scroll.vertical_scroll_offset(
                                page as f32 * (row_height + ui.spacing().item_spacing.y),
                            );
                        }
                        scroll.show_rows(ui, row_height, self.nav.count, |ui, range| {
                            for page in range {
                                if ui
                                    .add_sized(
                                        Vec2::new(ui.available_width(), row_height),
                                        egui::Button::selectable(
                                            page == self.nav.page,
                                            format!("Page {}", self.page_name(page)),
                                        )
                                        .truncate(),
                                    )
                                    .clicked()
                                {
                                    self.jump(page);
                                    self.reveal_page = None;
                                }
                            }
                        });
                    }
                });
            });
        if !self.loading
            && let Some(keys) = self.thumbnails.requests()
        {
            self.worker.thumbnails(keys);
        }
    }
}
