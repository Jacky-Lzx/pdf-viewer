use super::{Pagewing, appearance, tooltips::LazyTooltip};
use eframe::egui::{self, RichText, Vec2};

mod bookmarks;
mod contents;
mod focus;
mod marks;
mod pages;
mod results;

enum RowAction {
    Jump,
    Remove,
}

struct RowResponse {
    action: Option<RowAction>,
    focused: Option<egui::Id>,
}

fn removable_row(
    ui: &mut egui::Ui,
    selected: bool,
    label: impl Into<egui::WidgetText>,
    delete_tooltip: &str,
    tooltip: impl FnOnce(&mut egui::Ui),
) -> RowResponse {
    let mut action = None;
    let mut focused = None;
    let row_height = Pagewing::sidebar_row_height(ui);
    ui.horizontal(|ui| {
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let remove = ui.small_button("×").on_hover_text(delete_tooltip);
            let jump = ui
                .add_sized(
                    egui::vec2(ui.available_width(), row_height),
                    egui::Button::selectable(selected, label).truncate(),
                )
                .on_hover_ui(|ui| {
                    ui.set_max_width(ui.spacing().tooltip_width);
                    tooltip(ui);
                });
            focused = focus::row_focus(&jump).or_else(|| focus::row_focus(&remove));
            if jump.clicked() {
                action = Some(RowAction::Jump);
            } else if remove.clicked() {
                action = Some(RowAction::Remove);
            }
        });
    });
    RowResponse { action, focused }
}

/// Reveal only the part of a row outside the current list viewport.
fn reveal_offset(offset: f32, top: f32, row_height: f32, height: f32) -> f32 {
    offset.max((top + row_height - height).max(0.0)).min(top)
}

impl Pagewing {
    pub(super) fn sidebar_control_focused(&self, context: &egui::Context) -> bool {
        self.library.sidebar
            && self.nav.count > 0
            && super::shortcuts::focused_in_region(context, self.sidebar_region)
    }

    fn sidebar_row_height(ui: &egui::Ui) -> f32 {
        (ui.text_style_height(&egui::TextStyle::Button) + 2.0 * ui.spacing().button_padding.y)
            .max(ui.spacing().interact_size.y)
            .max(20.0)
    }

    pub(super) fn sidebar(&mut self, context: &egui::Context) {
        self.sidebar_region = None;
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
        let panel = egui::SidePanel::left("sidebar")
            .frame(appearance::panel(context, 14))
            .default_width(256.0_f32.min(maximum))
            .width_range(180.0_f32.min(maximum)..=maximum)
            .show(context, |ui| {
                // Reveal focused chapters when resizing changes how many rows fit.
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
                        .on_hover_text_lazy(|| self.reading.path.display().to_string());
                        ui.weak(format!("PDF document  ·  {} pages", self.nav.count));
                        ui.add_space(10.0);
                        ui.separator();
                        ui.add_space(10.0);
                    }
                    // Responsive headers and preceding lists must not change
                    // the IDs of focused controls in the following sections.
                    let contents_navigation = if self.outline.is_empty() {
                        false
                    } else {
                        ui.scope_builder(
                            egui::UiBuilder::new().id(("sidebar-contents", self.id)),
                            |ui| self.contents(ui),
                        )
                        .inner
                    };
                    let mut other_focus = chapter_focus || contents_navigation;
                    appearance::section(ui, "BOOKMARKS");
                    if self.reading.bookmarks.is_empty() {
                        ui.weak(format!(
                            "Press {} to bookmark a page.",
                            self.library.shortcuts.label("bookmark")
                        ));
                    }
                    if !self.bookmark_pages.is_empty() {
                        other_focus |= ui
                            .scope_builder(
                                egui::UiBuilder::new().id(("sidebar-bookmarks", self.id)),
                                |ui| self.bookmark_list(ui),
                            )
                            .inner;
                    }
                    if !self.reading.marks.is_empty() {
                        other_focus |= ui
                            .scope_builder(
                                egui::UiBuilder::new().id(("sidebar-marks", self.id)),
                                |ui| self.mark_list(ui),
                            )
                            .inner;
                    }
                    let reveal_search = self.reveal_hit.is_some() && !self.hits.is_empty();
                    if !self.hits.is_empty() {
                        ui.scope_builder(
                            egui::UiBuilder::new().id(("sidebar-results", self.id)),
                            |ui| self.search_results(ui, viewport, chapter_focus),
                        );
                    }
                    ui.scope_builder(
                        egui::UiBuilder::new().id(("sidebar-pages", self.id)),
                        |ui| self.pages(ui, viewport, other_focus || reveal_search),
                    );
                    super::shortcuts::reveal_focused_control(ui, viewport);
                });
            });
        self.sidebar_region = Some((panel.response.layer_id, panel.response.rect));
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
        fn paint(
            app: &mut Pagewing,
            context: &egui::Context,
            screen: egui::Rect,
            events: Vec<egui::Event>,
        ) -> egui::FullOutput {
            context.run(
                egui::RawInput {
                    screen_rect: Some(screen),
                    events,
                    ..Default::default()
                },
                |context| {
                    app.keys(context);
                    app.toolbar(context);
                    app.status(context);
                    app.sidebar(context);
                    egui::CentralPanel::default().show(context, |_| {});
                },
            )
        }
        let bounds = |output: &egui::FullOutput, page: usize| {
            output.shapes.iter().find_map(|shape| match &shape.shape {
                egui::Shape::Text(text) if text.galley.text() == format!("Chapter {page:03}") => {
                    Some((
                        shape.clip_rect,
                        egui::Rect::from_min_size(text.pos, text.galley.size()),
                    ))
                }
                _ => None,
            })
        };
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
                    output = paint(&mut app, &context, screen, Vec::new());
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
                let (clip, selected) =
                    bounds(&output, page).expect("the selected row should be drawn");
                assert!(screen.contains_rect(selected), "{size:?}: {selected:?}");
                assert!(
                    clip.contains_rect(selected),
                    "{size:?}: {selected:?} clipped by {clip:?}"
                );
                if page == 40 && size == egui::vec2(1180.0, 820.0) {
                    let (clip, previous) = bounds(&output, 39).unwrap();
                    assert!(clip.contains_rect(previous));
                    for (key, selected) in [(egui::Key::ArrowUp, 39), (egui::Key::ArrowDown, 40)] {
                        let events = [true, false]
                            .map(|pressed| egui::Event::Key {
                                key,
                                physical_key: None,
                                pressed,
                                repeat: false,
                                modifiers: egui::Modifiers::NONE,
                            })
                            .into();
                        paint(&mut app, &context, screen, events);
                        for _ in 0..6 {
                            output = paint(&mut app, &context, screen, Vec::new());
                        }
                        assert_eq!(app.outline.selected(), Some(selected));
                        let (clip, current) = bounds(&output, 39).unwrap();
                        assert!(clip.contains_rect(current));
                        assert_eq!(current, previous, "keep visible neighboring chapters still");
                    }
                }
            }
        }
    }
}
