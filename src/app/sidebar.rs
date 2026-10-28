use super::{MarkAction, Pagewing, appearance};
use eframe::egui::{self, RichText, Vec2};

mod bookmarks;
mod pages;
mod results;

impl Pagewing {
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

    fn contents(&mut self, ui: &mut egui::Ui) -> bool {
        if self.outline.is_empty() {
            return false;
        }
        let mut target = None;
        let mut toggle = None;
        let mut keyboard_focus = false;
        ui.horizontal(|ui| {
            appearance::section(ui, "CONTENTS");
            if ui.available_width() < 70.0 {
                return;
            }
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
                    egui::Id::new("pagewing-outline-input"),
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
        let viewport_height =
            (ui.clip_rect().bottom() - ui.cursor().top()).clamp(row_height, 260.0);
        let mut scroll = egui::ScrollArea::vertical()
            .id_salt("contents")
            .max_height(viewport_height);
        if keyboard_focus
            && std::mem::take(&mut self.outline.reveal)
            && let Some(row) = self.outline.selected_row()
        {
            let row_top = row as f32 * (row_height + ui.spacing().item_spacing.y);
            scroll = scroll
                .vertical_scroll_offset((row_top - (viewport_height - row_height) * 0.5).max(0.0));
        }
        let mut selected = None;
        scroll.show_rows(ui, row_height, self.outline.visible().len(), |ui, range| {
            for row in range {
                let index = self.outline.visible()[row];
                let entry = self.outline.entry(index);
                ui.push_id(index, |ui| {
                    ui.horizontal(|ui| {
                        ui.add_space(
                            (entry.depth.min(8) as f32 * 12.0)
                                .min((ui.available_width() - 60.0).max(0.0)),
                        );
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
        let navigated = target.is_some();
        if let Some(destination) = target {
            self.follow_destination(destination);
        }
        ui.add_space(20.0);
        navigated
    }

    fn sidebar_row_height(ui: &egui::Ui) -> f32 {
        (ui.text_style_height(&egui::TextStyle::Button) + 2.0 * ui.spacing().button_padding.y)
            .max(ui.spacing().interact_size.y)
            .max(20.0)
    }

    pub(super) fn sidebar(&mut self, context: &egui::Context) {
        self.thumbnails.begin_frame();
        if !self.library.sidebar
            || self.nav.count == 0
            || (self.comments.open && context.available_rect().width() < 220.0)
        {
            if !self.loading
                && let Some(keys) = self.thumbnails.requests()
            {
                self.worker.thumbnails(keys);
            }
            return;
        }
        // Leave room for the page even when reopening a wide saved sidebar
        // in a narrow window.
        let maximum = (context.available_rect().width() - 120.0).clamp(100.0, 380.0);
        let short = context.available_rect().height() < 320.0;
        egui::SidePanel::left("sidebar")
            .frame(appearance::panel(context, 14))
            .default_width(256.0_f32.min(maximum))
            .width_range(180.0_f32.min(maximum)..=maximum)
            .show(context, |ui| {
                // Recenter focused chapters when resizing changes how many rows fit.
                let viewport_id = ui.make_persistent_id("chapter-viewport");
                let viewport = ui.available_size();
                let resized = ui.ctx().data_mut(|data| {
                    let previous = data.get_temp::<Vec2>(viewport_id);
                    if previous != Some(viewport) {
                        data.insert_temp(viewport_id, viewport);
                        true
                    } else {
                        false
                    }
                });
                let chapter_focus = self.outline.focus
                    || context
                        .memory(|memory| memory.has_focus(egui::Id::new("pagewing-outline-input")));
                let mut sidebar_scroll = egui::ScrollArea::vertical().animated(false);
                if self.outline.focus || (resized && chapter_focus) {
                    sidebar_scroll = sidebar_scroll.vertical_scroll_offset(0.0);
                    self.outline.reveal = true;
                }
                sidebar_scroll.show(ui, |ui| {
                    ui.add_space(6.0);
                    if !short {
                        appearance::section(ui, "YOUR DOCUMENT");
                        ui.add(
                            egui::Label::new(RichText::new(self.filename()).strong()).truncate(),
                        )
                        .on_hover_text(self.reading.path.display().to_string());
                        ui.weak(format!("PDF document  ·  {} pages", self.nav.count));
                        ui.add_space(10.0);
                        ui.separator();
                        ui.add_space(10.0);
                    }
                    let contents_navigation = self.contents(ui);
                    let mut other_focus = chapter_focus || contents_navigation;
                    appearance::section(ui, "BOOKMARKS");
                    if self.reading.bookmarks.is_empty() {
                        ui.weak(format!(
                            "Press {} to bookmark a page.",
                            self.library.shortcuts.label("bookmark")
                        ));
                    }
                    if !self.bookmark_pages.is_empty() {
                        other_focus |= self.bookmark_list(ui);
                    }
                    if !self.reading.marks.is_empty() {
                        ui.add_space(20.0);
                        appearance::section(ui, "MARKS");
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
                            other_focus = true;
                        }
                        if let Some(name) = remove {
                            self.reading.marks.remove(&name);
                            self.dirty = true;
                        }
                    }
                    let reveal_search = self.reveal_hit.is_some() && !self.hits.is_empty();
                    self.search_results(ui, viewport, chapter_focus);
                    self.pages(ui, viewport, other_focus || reveal_search);
                });
            });
        if !self.loading
            && let Some(keys) = self.thumbnails.requests()
        {
            self.worker.thumbnails(keys);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{destination::Destination, navigation::Navigation, outline::OutlineEntry};
    use eframe::egui::Color32;

    #[test]
    fn result_highlights_update_with_queries_themes_and_fonts() {
        let directory = tempfile::tempdir().unwrap();
        let context = egui::Context::default();
        let mut app =
            Pagewing::with_context(&context, None, None, directory.path().join("library.json"));
        app.nav = Navigation::new(2, 0);
        app.dimensions = vec![[600.0, 800.0]; 2];
        app.library.sidebar = true;
        let mut page = crate::text::PageText::default();
        page.text = "Straße 中文 tail".into();
        for (query, expected, dark, size) in [
            ("STRASSE", "Straße", false, 14.0),
            ("STRASSE", "Straße", true, 14.0),
            ("STRASSE", "Straße", true, 20.0),
            ("中文", "中文", false, 14.0),
        ] {
            if app.query != query {
                app.query = query.into();
                app.restart_search();
                app.hits = page.search(0, query);
            }
            context.set_visuals(if dark {
                egui::Visuals::dark()
            } else {
                egui::Visuals::light()
            });
            context.style_mut(|style| {
                style
                    .text_styles
                    .insert(egui::TextStyle::Button, egui::FontId::proportional(size));
            });
            let mut output = egui::FullOutput::default();
            for _ in 0..3 {
                output = context.run(
                    egui::RawInput {
                        screen_rect: Some(egui::Rect::from_min_size(
                            egui::Pos2::ZERO,
                            egui::vec2(1180.0, 820.0),
                        )),
                        ..Default::default()
                    },
                    |context| {
                        app.sidebar(context);
                        egui::CentralPanel::default().show(context, |_| {});
                    },
                );
            }
            let job = output
                .shapes
                .iter()
                .find_map(|shape| match &shape.shape {
                    egui::Shape::Text(text) if text.galley.job.text.contains("中文 tail") => {
                        Some(&text.galley.job)
                    }
                    _ => None,
                })
                .expect("the result row must be painted");
            let highlighted: Vec<_> = job
                .sections
                .iter()
                .filter(|section| section.format.background != Color32::TRANSPARENT)
                .collect();
            assert_eq!(highlighted.len(), 1);
            assert_eq!(&job.text[highlighted[0].byte_range.clone()], expected);
            assert_eq!(highlighted[0].format.font_id.size, size);
            assert_eq!(
                highlighted[0].format.background,
                if dark {
                    Color32::from_rgb(100, 80, 25)
                } else {
                    Color32::from_rgb(250, 220, 125)
                }
            );
        }
        app.clear_document(&context);
        let label = app.result_text.get_or_insert_with(0, || {
            egui::text::LayoutJob::simple_singleline(
                "new document".into(),
                egui::FontId::default(),
                Color32::PLACEHOLDER,
            )
        });
        assert_eq!(label.text, "new document");
    }

    #[test]
    fn chapter_focus_keeps_the_input_and_selected_row_visible_in_short_windows() {
        let directory = tempfile::tempdir().unwrap();
        let context = egui::Context::default();
        let mut app =
            Pagewing::with_context(&context, None, None, directory.path().join("library.json"));
        app.nav = Navigation::new(80, 0);
        app.dimensions = vec![[600.0, 800.0]; 80];
        app.outline.set_entries(
            (0..80)
                .map(|page| OutlineEntry {
                    title: format!("Chapter {page:03}"),
                    destination: Some(Destination::page(page)),
                    depth: 0,
                })
                .collect(),
        );
        app.focus_contents();
        for page in [0, 40, 79] {
            app.outline.select(page);
            app.outline.reveal = true;
            for size in super::super::dialog::TEST_SIZES {
                let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, size);
                let mut output = egui::FullOutput::default();
                for _ in 0..6 {
                    output = context.run(
                        egui::RawInput {
                            screen_rect: Some(screen),
                            ..Default::default()
                        },
                        |context| {
                            app.keys(context);
                            app.toolbar(context);
                            app.status(context);
                            app.sidebar(context);
                            egui::CentralPanel::default().show(context, |_| {});
                        },
                    );
                }
                let input = context
                    .read_response(egui::Id::new("pagewing-outline-input"))
                    .unwrap();
                assert!(input.has_focus());
                assert!(
                    input.interact_rect.contains_rect(input.rect.shrink(1.0)),
                    "{size:?}: input {:?} clipped by {:?}",
                    input.rect,
                    input.interact_rect
                );
                assert!(
                    screen.contains_rect(input.rect),
                    "{size:?}: {:?}",
                    input.rect
                );
                let (clip, text) = output
                    .shapes
                    .iter()
                    .find_map(|shape| match &shape.shape {
                        egui::Shape::Text(text)
                            if text.galley.text() == format!("Chapter {page:03}") =>
                        {
                            Some((shape.clip_rect, text))
                        }
                        _ => None,
                    })
                    .expect("the selected row should be drawn");
                let bounds = egui::Rect::from_min_size(text.pos, text.galley.size());
                assert!(screen.contains_rect(bounds), "{size:?}: {bounds:?}");
                assert!(
                    clip.contains_rect(bounds),
                    "{size:?}: {bounds:?} clipped by {clip:?}"
                );
            }
        }
    }
}
