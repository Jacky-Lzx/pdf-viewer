use super::{Pagewing, appearance, reveal_offset};
use crate::app::list_focus::ListFocus;
use eframe::egui::{self, Color32, RichText, Vec2};

#[derive(Clone, Copy)]
struct ResultViewport {
    size: Vec2,
    row_height: f32,
    selected_visible: bool,
}

impl Pagewing {
    pub(super) fn search_results(
        &mut self,
        ui: &mut egui::Ui,
        viewport: Vec2,
        chapter_focus: bool,
    ) {
        let focus_requested = self.result_focus_pending(ui.ctx());
        if self.hits.is_empty() {
            return;
        }
        ui.add_space(20.0);
        let heading = ui.label(
            RichText::new("SEARCH RESULTS")
                .small()
                .strong()
                .color(appearance::accent(ui)),
        );
        let row_height = Self::sidebar_row_height(ui);
        let stride = row_height + ui.spacing().item_spacing.y;
        // Use the panel's viewport, not the current cursor position in the outer
        // scroll area: wheel scrolling must not change the list's height.
        let height = (viewport.y - heading.rect.height() - 3.0 * ui.spacing().item_spacing.y)
            .clamp(row_height, 280.0);
        let viewport_id = ui.make_persistent_id("result-viewport");
        let previous = ui
            .ctx()
            .data(|data| data.get_temp::<ResultViewport>(viewport_id));
        let viewport_changed = previous
            .is_some_and(|previous| previous.size != viewport || previous.row_height != row_height);
        let resized =
            viewport_changed && previous.is_some_and(|previous| previous.selected_visible);
        let mut reveal = if ui.is_sizing_pass() {
            self.reveal_hit
        } else {
            self.reveal_hit.take()
        };
        if reveal.is_none() && resized && !chapter_focus {
            reveal = self.hit_index.filter(|&index| {
                self.hits
                    .get(index)
                    .is_some_and(|hit| hit.page == self.nav.page)
            });
        }
        let font = egui::FontSelection::Default
            .resolve_with_fallback(ui.style(), egui::TextStyle::Button.into());
        let highlight = if ui.visuals().dark_mode {
            Color32::from_rgb(100, 80, 25)
        } else {
            Color32::from_rgb(250, 220, 125)
        };
        self.result_text.set_style(font.clone(), highlight);
        let mut results = egui::ScrollArea::vertical()
            .id_salt("search-results")
            .min_scrolled_height(height)
            .max_height(height)
            .animated(false);
        if let Some(index) = reveal {
            let id = ui.make_persistent_id(egui::Id::new("search-results"));
            let offset =
                egui::scroll_area::State::load(ui.ctx(), id).map_or(0.0, |state| state.offset.y);
            let top = index.min(self.hits.len() - 1) as f32 * stride;
            // Compute this before show_rows: an inner list outside the parent's
            // clip rect cannot use scroll_to_rect to measure its own viewport.
            let offset = reveal_offset(offset, top, row_height, height);
            results = results.vertical_scroll_offset(offset);
        }
        let mut selected = None;
        let mut copied = false;
        let focus_target =
            focus_requested.then(|| self.hit_index.unwrap_or(0).min(self.hits.len() - 1));
        let mut focus = ListFocus::new(ui, "search-results", row_height, height);
        let results = focus.scroll(ui, results);
        let output = results.show_rows(ui, row_height, self.hits.len(), |ui, range| {
            for index in range {
                let hit = &self.hits[index];
                let labels = &self.labels;
                let label = self.result_text.get_or_insert_with(index, || {
                    crate::search_result_text::result_label(
                        labels.display_name(hit.page),
                        &hit.snippet,
                        hit.snippet_match.clone(),
                        font.clone(),
                        highlight,
                    )
                });
                let response = ui
                    .add_sized(
                        Vec2::new(ui.available_width(), row_height),
                        egui::Button::selectable(self.hit_index == Some(index), label).truncate(),
                    )
                    .on_hover_ui(|ui| {
                        ui.set_max_width(ui.spacing().tooltip_width);
                        ui.label(&hit.snippet);
                        ui.weak("Right-click to copy this snippet");
                        ui.weak("When focused: Ctrl/Cmd+C to copy · Shift+F10 for menu");
                    });
                if focus_target == Some(index)
                    && !ui.is_sizing_pass()
                    && !ui.ctx().will_discard()
                    && response.enabled()
                {
                    response.request_focus();
                    self.result_focus_origin = None;
                }
                if response.secondary_clicked() {
                    self.nav.clear_prefix();
                    self.hints.cancel();
                    self.mark_action = None;
                }
                copied |= crate::app::copy_menu::show(
                    &response,
                    // A replacement query must not inherit this row's open menu.
                    response.id.with(("snippet", self.revision)),
                    "Copy snippet",
                    &hit.snippet,
                    crate::app::copy_menu::Target::Control,
                );
                focus.observe(&response, index);
                if response.clicked() {
                    selected = Some(index);
                }
            }
        });
        let bottom = ui.min_rect().bottom();
        if reveal.is_some() || (viewport_changed && focus.has_focus()) {
            // This UI belongs to the outer sidebar scroll area. Reveal the
            // heading and list after positioning a selected or focused row,
            // including the source of an open copy menu.
            ui.scroll_to_rect(
                egui::Rect::from_min_max(
                    heading.rect.min,
                    egui::pos2(heading.rect.right(), bottom),
                ),
                None,
            );
        }
        if !ui.is_sizing_pass() {
            let clip = ui.clip_rect().intersect(egui::Rect::from_min_max(
                output.inner_rect.min,
                egui::pos2(output.inner_rect.right(), bottom),
            ));
            let selected_visible = self.hit_index.is_some_and(|index| {
                let top = output.inner_rect.top() + index as f32 * stride - output.state.offset.y;
                top >= clip.top() && top + row_height <= clip.bottom()
            });
            ui.ctx().data_mut(|data| {
                data.insert_temp(
                    viewport_id,
                    ResultViewport {
                        size: viewport,
                        row_height,
                        selected_visible,
                    },
                );
            });
        }
        if let Some(index) = selected {
            self.select_hit(index);
        }
        if copied {
            self.notify("Copied search snippet");
        }
        focus.finish(ui);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::shortcuts::SearchButton;
    use crate::{destination::Destination, navigation::Navigation, outline::OutlineEntry};

    fn long_sidebar() -> (tempfile::TempDir, egui::Context, Pagewing) {
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
        app.reading.bookmarks.extend(0..60);
        app.refresh_bookmarks();
        app.hits = (0..80)
            .map(|page| crate::text::SearchHit {
                page,
                rects: Vec::new(),
                snippet: format!("Needle {page:03}"),
                snippet_match: 0..6,
            })
            .collect();
        (directory, context, app)
    }

    fn frame(
        app: &mut Pagewing,
        context: &egui::Context,
        size: Vec2,
        events: Vec<egui::Event>,
    ) -> egui::FullOutput {
        let mut input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, size)),
            time: Some(context.input(|input| input.time) + 0.25),
            events,
            ..Default::default()
        };
        app.prepare_native_input(context, &mut input);
        context.run(input, |context| {
            app.keys(context);
            app.toolbar(context);
            app.search_bar(context);
            app.status(context);
            app.sidebar(context);
            egui::CentralPanel::default().show(context, |_| {});
        })
    }

    fn settle(app: &mut Pagewing, context: &egui::Context, size: Vec2) -> egui::FullOutput {
        let mut output = egui::FullOutput::default();
        for _ in 0..6 {
            output = frame(app, context, size, Vec::new());
        }
        output
    }

    fn visible_result(output: &egui::FullOutput, index: usize) -> Option<egui::Rect> {
        output.shapes.iter().find_map(|shape| match &shape.shape {
            egui::Shape::Text(text)
                if text
                    .galley
                    .job
                    .text
                    .ends_with(&format!("Needle {index:03}")) =>
            {
                let rect = egui::Rect::from_min_size(text.pos, text.galley.size());
                shape.clip_rect.contains_rect(rect).then_some(rect)
            }
            _ => None,
        })
    }

    fn selected_result(
        context: &egui::Context,
        output: &egui::FullOutput,
        index: usize,
    ) -> Option<egui::Rect> {
        let text = visible_result(output, index)?;
        output.shapes.iter().find_map(|shape| match &shape.shape {
            egui::Shape::Rect(rect)
                if rect.fill == context.style().visuals.selection.bg_fill
                    && rect.rect.contains_rect(text)
                    && shape.clip_rect.contains_rect(rect.rect) =>
            {
                Some(rect.rect)
            }
            _ => None,
        })
    }

    fn wheel(
        app: &mut Pagewing,
        context: &egui::Context,
        size: Vec2,
        position: egui::Pos2,
        distance: f32,
    ) -> egui::FullOutput {
        frame(
            app,
            context,
            size,
            vec![
                egui::Event::PointerMoved(position),
                egui::Event::MouseWheel {
                    unit: egui::MouseWheelUnit::Point,
                    delta: Vec2::new(0.0, distance),
                    modifiers: egui::Modifiers::NONE,
                },
            ],
        );
        settle(app, context, size)
    }

    fn pointer_click(
        app: &mut Pagewing,
        context: &egui::Context,
        size: Vec2,
        position: egui::Pos2,
        button: egui::PointerButton,
    ) -> egui::FullOutput {
        // Move before pressing: combining a large pointer delta and a held
        // primary button in one frame starts ScrollArea's drag-to-scroll.
        frame(
            app,
            context,
            size,
            vec![egui::Event::PointerMoved(position)],
        );
        let mut output = egui::FullOutput::default();
        for pressed in [true, false] {
            output = frame(
                app,
                context,
                size,
                vec![egui::Event::PointerButton {
                    pos: position,
                    button,
                    pressed,
                    modifiers: egui::Modifiers::NONE,
                }],
            );
        }
        output
    }

    fn key(key: egui::Key) -> Vec<egui::Event> {
        [true, false]
            .map(|pressed| egui::Event::Key {
                key,
                physical_key: None,
                pressed,
                repeat: false,
                modifiers: egui::Modifiers::NONE,
            })
            .into()
    }

    fn menu_action(output: &egui::FullOutput) -> Option<egui::Rect> {
        output.shapes.iter().find_map(|shape| match &shape.shape {
            egui::Shape::Text(text) if text.galley.text() == "Copy snippet" => {
                let rect = egui::Rect::from_min_size(text.pos, text.galley.size());
                shape.clip_rect.contains_rect(rect).then_some(rect)
            }
            _ => None,
        })
    }

    #[test]
    fn replacing_search_results_closes_the_old_snippet_menu() {
        let (_directory, context, mut app) = long_sidebar();
        let size = Vec2::new(860.0, 540.0);
        app.select_hit(79);
        app.search_button(SearchButton::Results);
        let output = settle(&mut app, &context, size);
        pointer_click(
            &mut app,
            &context,
            size,
            visible_result(&output, 78).unwrap().center(),
            egui::PointerButton::Secondary,
        );
        assert!(egui::Popup::is_any_open(&context));
        let mut hits = std::mem::take(&mut app.hits);
        app.query = "replacement".into();
        app.restart_search();
        for hit in &mut hits {
            hit.snippet = "replacement text".into();
            hit.snippet_match = 0..11;
        }
        app.hits = hits;
        let output = settle(&mut app, &context, size);
        assert!(menu_action(&output).is_none());
        assert!(!egui::Popup::is_any_open(&context));
        assert_eq!(app.nav.page, 79);
        assert_eq!(app.hit_index, None);
    }

    #[test]
    fn snippet_menu_copies_unicode_without_selecting_or_navigating_and_survives_resize() {
        let (_directory, context, mut app) = long_sidebar();
        let size = Vec2::new(860.0, 540.0);
        let snippet = "Straße 中文 😀 Needle 078";
        app.hits[78].snippet = snippet.into();
        let start = snippet.find("Needle").unwrap();
        app.hits[78].snippet_match = start..start + 6;
        app.select_hit(79);
        app.search_button(SearchButton::Results);
        let output = settle(&mut app, &context, size);
        let position = visible_result(&output, 78).unwrap().center();
        app.pan = Vec2::new(37.0, 53.0);
        pointer_click(
            &mut app,
            &context,
            size,
            position,
            egui::PointerButton::Secondary,
        );
        assert!(egui::Popup::is_any_open(&context));
        assert_eq!(app.hit_index, Some(79));
        assert_eq!(app.nav.page, 79);
        for resized in [Vec2::new(240.0, 240.0), size] {
            let output = settle(&mut app, &context, resized);
            let action = menu_action(&output).expect("copy action must remain fully visible");
            assert!(egui::Rect::from_min_size(egui::Pos2::ZERO, resized).contains_rect(action));
        }
        let output = frame(&mut app, &context, size, key(egui::Key::Enter));
        assert!(output.platform_output.commands.iter().any(|command| {
            matches!(command, egui::OutputCommand::CopyText(text) if text == snippet)
        }));
        assert!(!egui::Popup::is_any_open(&context));
        assert_eq!(app.hit_index, Some(79));
        assert_eq!(app.nav.page, 79);
        assert_eq!(app.pan, Vec2::new(37.0, 53.0));

        let output = settle(&mut app, &context, size);
        pointer_click(
            &mut app,
            &context,
            size,
            visible_result(&output, 78).unwrap().center(),
            egui::PointerButton::Secondary,
        );
        settle(&mut app, &context, size);
        let output = frame(&mut app, &context, size, key(egui::Key::Escape));
        assert!(!egui::Popup::is_any_open(&context));
        assert!(
            !output
                .platform_output
                .commands
                .iter()
                .any(|command| matches!(command, egui::OutputCommand::CopyText(_)))
        );
        assert_eq!(app.hit_index, Some(79));
        assert_eq!(app.nav.page, 79);
        assert_eq!(app.pan, Vec2::new(37.0, 53.0));

        let output = settle(&mut app, &context, size);
        pointer_click(
            &mut app,
            &context,
            size,
            visible_result(&output, 78).unwrap().center(),
            egui::PointerButton::Secondary,
        );
        let output = settle(&mut app, &context, size);
        let output = pointer_click(
            &mut app,
            &context,
            size,
            menu_action(&output).unwrap().center(),
            egui::PointerButton::Primary,
        );
        assert!(output.platform_output.commands.iter().any(|command| {
            matches!(command, egui::OutputCommand::CopyText(text) if text == snippet)
        }));
        assert_eq!(app.nav.page, 79);
        assert_eq!(app.hit_index, Some(79));
    }

    #[test]
    fn results_button_reveals_the_current_match_below_long_contents_and_bookmarks() {
        let (_directory, context, mut app) = long_sidebar();
        app.select_hit(79);
        app.search_button(SearchButton::Results);
        for size in crate::app::dialog::TEST_SIZES {
            let output = settle(&mut app, &context, size);
            assert!(
                selected_result(&context, &output, 79).is_some(),
                "current result must remain visible at {size:?}"
            );
            assert!(
                output.shapes.iter().any(|shape| match &shape.shape {
                    egui::Shape::Text(text) if text.galley.text() == "SEARCH RESULTS" => shape
                        .clip_rect
                        .contains_rect(egui::Rect::from_min_size(text.pos, text.galley.size())),
                    _ => false,
                }),
                "result heading must be visible at {size:?}"
            );
        }
    }

    #[test]
    fn results_requested_before_hits_arrive_reveal_the_first_row_without_navigating() {
        let (_directory, context, mut app) = long_sidebar();
        let hits = std::mem::take(&mut app.hits);
        app.nav.page = 40;
        app.search_button(SearchButton::Results);
        let size = Vec2::new(860.0, 540.0);
        settle(&mut app, &context, size);
        assert_eq!(app.reveal_hit, Some(0));
        app.hits = hits;
        let output = settle(&mut app, &context, size);
        assert!(visible_result(&output, 0).is_some());
        assert_eq!(app.hit_index, None);
        assert_eq!(app.nav.page, 40);
    }

    #[test]
    fn adjacent_search_navigation_only_scrolls_when_necessary() {
        let (_directory, context, mut app) = long_sidebar();
        let size = Vec2::new(860.0, 540.0);
        app.select_hit(79);
        app.search_button(SearchButton::Results);
        let output = settle(&mut app, &context, size);
        let last = selected_result(&context, &output, 79).unwrap();
        frame(
            &mut app,
            &context,
            size,
            vec![egui::Event::Key {
                key: egui::Key::N,
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers: egui::Modifiers::SHIFT,
            }],
        );
        let output = settle(&mut app, &context, size);
        assert_eq!(app.hit_index, Some(78));
        let previous = selected_result(&context, &output, 78).unwrap();
        assert!(previous.top() < last.top());
        assert_eq!(
            visible_result(&output, 79).unwrap().center().y,
            last.center().y
        );
    }

    #[test]
    fn larger_fonts_keep_the_selected_result_visible() {
        let (_directory, context, mut app) = long_sidebar();
        let size = Vec2::new(860.0, 400.0);
        app.select_hit(79);
        app.search_button(SearchButton::Results);
        settle(&mut app, &context, size);
        context.style_mut(|style| {
            style
                .text_styles
                .insert(egui::TextStyle::Button, egui::FontId::proportional(24.0));
        });
        let output = settle(&mut app, &context, size);
        assert!(selected_result(&context, &output, 79).is_some());
    }

    #[test]
    fn inner_manual_scroll_survives_resize_and_results_restores_the_selection() {
        let (_directory, context, mut app) = long_sidebar();
        let size = Vec2::new(860.0, 540.0);
        app.select_hit(79);
        app.search_button(SearchButton::Results);
        let output = settle(&mut app, &context, size);
        let last = selected_result(&context, &output, 79).unwrap();
        let output = wheel(&mut app, &context, size, last.center(), 1500.0);
        assert!(selected_result(&context, &output, 79).is_none());
        assert!((0..60).any(|index| visible_result(&output, index).is_some()));
        let size = Vec2::new(860.0, 400.0);
        let output = settle(&mut app, &context, size);
        assert!(
            selected_result(&context, &output, 79).is_none(),
            "resizing must respect deliberate scrolling away"
        );
        app.search_button(SearchButton::Results);
        let output = settle(&mut app, &context, size);
        assert!(selected_result(&context, &output, 79).is_some());
        assert_eq!(app.nav.page, 79);
    }

    #[test]
    fn left_click_follows_without_copying_or_moving_visible_rows() {
        let (_directory, context, mut app) = long_sidebar();
        let size = Vec2::new(860.0, 540.0);
        app.select_hit(40);
        app.search_button(SearchButton::Results);
        let output = settle(&mut app, &context, size);
        let heading = output
            .shapes
            .iter()
            .find_map(|shape| match &shape.shape {
                egui::Shape::Text(text) if text.galley.text() == "SEARCH RESULTS" => Some(text.pos),
                _ => None,
            })
            .unwrap();
        let output = wheel(
            &mut app,
            &context,
            size,
            heading + egui::vec2(5.0, 5.0),
            -90.0,
        );
        let before = visible_result(&output, 39).expect("neighboring result remains visible");
        let position = before.center();
        let output = pointer_click(
            &mut app,
            &context,
            size,
            position,
            egui::PointerButton::Primary,
        );
        assert!(
            !output
                .platform_output
                .commands
                .iter()
                .any(|command| matches!(command, egui::OutputCommand::CopyText(_)))
        );
        let output = settle(&mut app, &context, size);
        assert_eq!(app.nav.page, 39);
        assert_eq!(app.hit_index, Some(39));
        assert_eq!(visible_result(&output, 39), Some(before));
    }

    #[test]
    fn outer_manual_scroll_survives_resize_and_results_restores_the_section() {
        let (_directory, context, mut app) = long_sidebar();
        let size = Vec2::new(860.0, 540.0);
        app.select_hit(79);
        app.search_button(SearchButton::Results);
        let output = settle(&mut app, &context, size);
        let heading = output
            .shapes
            .iter()
            .find_map(|shape| match &shape.shape {
                egui::Shape::Text(text) if text.galley.text() == "SEARCH RESULTS" => Some(text.pos),
                _ => None,
            })
            .unwrap();
        let output = wheel(
            &mut app,
            &context,
            size,
            heading + egui::vec2(5.0, 5.0),
            4000.0,
        );
        assert!(selected_result(&context, &output, 79).is_none());
        let size = Vec2::new(860.0, 400.0);
        let output = settle(&mut app, &context, size);
        assert!(selected_result(&context, &output, 79).is_none());
        app.search_button(SearchButton::Results);
        let output = settle(&mut app, &context, size);
        assert!(selected_result(&context, &output, 79).is_some());
    }

    fn focus_results_button(app: &mut Pagewing, context: &egui::Context, size: Vec2) -> egui::Id {
        app.search_open = true;
        app.query = "Needle".into();
        settle(app, context, size);
        let id = app.input_buttons.search_results.unwrap();
        context.memory_mut(|memory| memory.request_focus(id));
        settle(app, context, size);
        id
    }

    fn assert_focused_result(context: &egui::Context, output: &egui::FullOutput, index: usize) {
        let id = context.memory(|memory| memory.focused()).unwrap();
        let _focus = crate::app::shortcuts::preserve_pointer_focus(context);
        let response = context.read_response(id).unwrap();
        let text = visible_result(output, index).unwrap();
        assert!(
            response.rect.contains_rect(text),
            "focused {id:?} {:?}, text {text:?}",
            response.rect
        );
        assert!(response.has_focus());
    }

    fn copied(output: &egui::FullOutput) -> Vec<&str> {
        output
            .platform_output
            .commands
            .iter()
            .filter_map(|command| match command {
                egui::OutputCommand::CopyText(text) => Some(text.as_str()),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn results_activation_focuses_the_selected_row_before_queued_copy_and_enter_follows_it() {
        for activation in [egui::Key::Enter, egui::Key::Space] {
            let (_directory, context, mut app) = long_sidebar();
            let size = Vec2::new(860.0, 540.0);
            app.select_hit(79);
            app.nav.page = 40;
            app.pan = Vec2::new(37.0, 53.0);
            focus_results_button(&mut app, &context, size);
            let mut events = key(activation);
            events.push(egui::Event::Copy);
            let output = frame(&mut app, &context, size, events);
            assert!(copied(&output).is_empty());
            let output = frame(&mut app, &context, size, Vec::new());
            assert_eq!(copied(&output), ["Needle 079"]);
            assert_eq!(app.hit_index, Some(79));
            assert_eq!(app.nav.page, 40);
            assert_eq!(app.pan, Vec2::new(37.0, 53.0));
            frame(&mut app, &context, size, key(egui::Key::Enter));
            assert_eq!(app.nav.page, 79);
            let output = settle(&mut app, &context, size);
            assert_focused_result(&context, &output, 79);

            app.nav.page = 40;
            focus_results_button(&mut app, &context, size);
            let mut events = key(activation);
            events.extend(key(egui::Key::Enter));
            frame(&mut app, &context, size, events);
            assert_eq!(app.nav.page, 40);
            frame(&mut app, &context, size, Vec::new());
            assert_eq!(app.nav.page, 79, "queued Enter belongs to the result row");
        }
    }

    #[test]
    fn results_can_focus_the_first_arriving_hit_without_selecting_or_navigating() {
        let (_directory, context, mut app) = long_sidebar();
        let size = Vec2::new(860.0, 540.0);
        let hits = std::mem::take(&mut app.hits);
        app.nav.page = 40;
        let origin = focus_results_button(&mut app, &context, size);
        frame(&mut app, &context, size, key(egui::Key::Enter));
        assert_eq!(app.result_focus_origin, Some(origin));
        assert!(context.memory(|memory| memory.has_focus(origin)));
        app.hits = hits;
        let output = settle(&mut app, &context, size);
        assert_focused_result(&context, &output, 0);
        let output = frame(&mut app, &context, size, vec![egui::Event::Copy]);
        assert_eq!(copied(&output), ["Needle 000"]);
        assert_eq!(app.hit_index, None);
        assert_eq!(app.nav.page, 40);
    }

    #[test]
    fn pending_result_focus_is_abandoned_when_the_user_moves_on_or_replaces_the_query() {
        for replaced in [false, true] {
            let (_directory, context, mut app) = long_sidebar();
            let size = Vec2::new(860.0, 540.0);
            let hits = std::mem::take(&mut app.hits);
            app.nav.page = 40;
            let origin = focus_results_button(&mut app, &context, size);
            frame(&mut app, &context, size, key(egui::Key::Enter));
            let expected = if replaced {
                app.query = "replacement".into();
                app.restart_search();
                origin
            } else {
                let editor = egui::Id::new("pagewing-search-input");
                context.memory_mut(|memory| memory.request_focus(editor));
                editor
            };
            app.hits = hits;
            settle(&mut app, &context, size);
            assert_eq!(app.result_focus_origin, None);
            assert!(context.memory(|memory| memory.has_focus(expected)));
            assert_eq!(app.nav.page, 40);
            assert_eq!(app.hit_index, None);
        }
    }

    #[test]
    fn returning_to_results_does_not_revive_an_abandoned_focus_request() {
        let (_directory, context, mut app) = long_sidebar();
        let size = Vec2::new(860.0, 540.0);
        let hits = std::mem::take(&mut app.hits);
        let origin = focus_results_button(&mut app, &context, size);
        frame(&mut app, &context, size, key(egui::Key::Enter));
        context.memory_mut(|memory| {
            memory.request_focus(egui::Id::new("pagewing-search-input"));
        });
        settle(&mut app, &context, size);
        assert_eq!(app.result_focus_origin, None);
        context.memory_mut(|memory| memory.request_focus(origin));
        settle(&mut app, &context, size);
        app.hits = hits;
        settle(&mut app, &context, size);
        assert!(context.memory(|memory| memory.has_focus(origin)));
        assert_eq!(app.hit_index, None);
        assert_eq!(app.nav.page, 0);
    }

    #[test]
    fn resizing_keeps_a_snippet_menu_owner_alive_while_reading_another_page() {
        for keyboard in [true, false] {
            let (_directory, context, mut app) = long_sidebar();
            let size = Vec2::new(860.0, 540.0);
            app.select_hit(79);
            app.nav.page = 40;
            focus_results_button(&mut app, &context, size);
            frame(&mut app, &context, size, key(egui::Key::Enter));
            let output = settle(&mut app, &context, size);
            if keyboard {
                for code in [egui::Key::Tab, egui::Key::F10] {
                    let events = key(code)
                        .into_iter()
                        .map(|mut event| {
                            if let egui::Event::Key { modifiers, .. } = &mut event {
                                *modifiers = egui::Modifiers::SHIFT;
                            }
                            event
                        })
                        .collect();
                    frame(&mut app, &context, size, events);
                }
            } else {
                pointer_click(
                    &mut app,
                    &context,
                    size,
                    visible_result(&output, 78).unwrap().center(),
                    egui::PointerButton::Secondary,
                );
            }
            assert!(egui::Popup::is_any_open(&context));
            for resized in [Vec2::new(240.0, 240.0), size] {
                let output = settle(&mut app, &context, resized);
                let action = menu_action(&output).expect("the virtual source must survive resize");
                assert!(egui::Rect::from_min_size(egui::Pos2::ZERO, resized).contains_rect(action));
                assert!(egui::Popup::is_any_open(&context));
            }
            let output = frame(&mut app, &context, size, key(egui::Key::Enter));
            assert_eq!(copied(&output), ["Needle 078"]);
            assert_eq!(app.nav.page, 40);
            assert_eq!(app.hit_index, Some(79));
        }
    }

    #[test]
    fn clicking_results_transfers_focus_before_a_copy_from_the_same_native_batch() {
        let (_directory, context, mut app) = long_sidebar();
        let size = Vec2::new(860.0, 540.0);
        app.select_hit(79);
        app.nav.page = 40;
        let origin = focus_results_button(&mut app, &context, size);
        context.memory_mut(|memory| memory.request_focus(egui::Id::new("pagewing-search-input")));
        settle(&mut app, &context, size);
        let position = context.read_response(origin).unwrap().rect.center();
        frame(
            &mut app,
            &context,
            size,
            vec![egui::Event::PointerMoved(position)],
        );
        for pressed in [true, false] {
            let mut events = vec![egui::Event::PointerButton {
                pos: position,
                button: egui::PointerButton::Primary,
                pressed,
                modifiers: egui::Modifiers::NONE,
            }];
            if !pressed {
                events.push(egui::Event::Copy);
            }
            let output = frame(&mut app, &context, size, events);
            assert!(copied(&output).is_empty());
        }
        let output = frame(&mut app, &context, size, Vec::new());
        assert_eq!(copied(&output), ["Needle 079"]);
        let output = settle(&mut app, &context, size);
        assert_focused_result(&context, &output, 79);
        assert_eq!(app.nav.page, 40);
        assert_eq!(app.hit_index, Some(79));
    }
}
