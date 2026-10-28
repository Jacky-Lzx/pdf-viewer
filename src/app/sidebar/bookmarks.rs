use super::Pagewing;
use eframe::egui::{self, Vec2};

impl Pagewing {
    pub(super) fn bookmark_list(&mut self, ui: &mut egui::Ui) -> bool {
        let row_height = Self::sidebar_row_height(ui);
        let height = 140.0;
        let mut scroll = egui::ScrollArea::vertical()
            .id_salt("bookmarks")
            .max_height(height);
        if self.reveal_page.is_some()
            && let Ok(row) = self.bookmark_pages.binary_search(&self.nav.page)
        {
            let id = ui.make_persistent_id(egui::Id::new("bookmarks"));
            let offset =
                egui::scroll_area::State::load(ui.ctx(), id).map_or(0.0, |state| state.offset.y);
            let top = row as f32 * (row_height + ui.spacing().item_spacing.y);
            // Reveal only the part outside the viewport. Navigation between
            // bookmarks already on screen should keep their neighbors still.
            scroll = scroll
                .vertical_scroll_offset(offset.max((top + row_height - height).max(0.0)).min(top));
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
                            self.labels.sidebar_caption(page),
                        )
                        .truncate(),
                    )
                    .on_hover_ui(|ui| {
                        ui.set_max_width(ui.spacing().tooltip_width);
                        ui.label(self.labels.sidebar_tooltip(page));
                    })
                    .clicked()
                {
                    target = Some(page);
                }
            }
        });
        if let Some(page) = target {
            self.jump(page);
        }
        target.is_some()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::navigation::Navigation;

    fn app() -> (tempfile::TempDir, egui::Context, Pagewing) {
        let directory = tempfile::tempdir().unwrap();
        let context = egui::Context::default();
        let mut app =
            Pagewing::with_context(&context, None, None, directory.path().join("library.json"));
        app.nav = Navigation::new(80, 0);
        app.dimensions = vec![[600.0, 800.0]; 80];
        app.reading.bookmarks.extend(0..80);
        app.refresh_bookmarks();
        (directory, context, app)
    }

    fn frame(
        app: &mut Pagewing,
        context: &egui::Context,
        events: Vec<egui::Event>,
    ) -> egui::FullOutput {
        context.run(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(240.0, 240.0),
                )),
                time: Some(context.input(|input| input.time) + 0.25),
                events,
                ..Default::default()
            },
            |context| {
                egui::CentralPanel::default().show(context, |ui| {
                    app.bookmark_list(ui);
                    // The page list normally consumes this request later in
                    // the sidebar. Keep the same lifetime in this focused view.
                    if !ui.is_sizing_pass() {
                        app.reveal_page = None;
                    }
                });
            },
        )
    }

    fn settle(app: &mut Pagewing, context: &egui::Context) -> egui::FullOutput {
        let mut output = egui::FullOutput::default();
        for _ in 0..4 {
            output = frame(app, context, Vec::new());
        }
        output
    }

    fn row(output: &egui::FullOutput, page: usize) -> Option<egui::Rect> {
        output.shapes.iter().find_map(|shape| match &shape.shape {
            egui::Shape::Text(text) if text.galley.text() == format!("Page {}", page + 1) => {
                let rect = egui::Rect::from_min_size(text.pos, text.galley.size());
                shape.clip_rect.contains_rect(rect).then_some(rect)
            }
            _ => None,
        })
    }

    #[test]
    fn clicking_a_visible_bookmark_keeps_neighboring_rows_in_place() {
        let (_directory, context, mut app) = app();
        let output = settle(&mut app, &context);
        let before = row(&output, 2).unwrap();
        let neighbor = row(&output, 1).unwrap();
        let position = before.center();
        for pressed in [true, false] {
            frame(
                &mut app,
                &context,
                vec![
                    egui::Event::PointerMoved(position),
                    egui::Event::PointerButton {
                        pos: position,
                        button: egui::PointerButton::Primary,
                        pressed,
                        modifiers: egui::Modifiers::NONE,
                    },
                ],
            );
        }
        assert_eq!(app.nav.page, 2);
        let output = settle(&mut app, &context);
        assert_eq!(row(&output, 2), Some(before));
        assert_eq!(row(&output, 1), Some(neighbor));
    }

    #[test]
    fn page_navigation_reveals_distant_bookmarks_and_keeps_visible_neighbors_still() {
        let (_directory, context, mut app) = app();
        settle(&mut app, &context);
        app.jump(40);
        let output = settle(&mut app, &context);
        let selected = row(&output, 40).expect("the distant bookmark must be fully visible");
        let previous = row(&output, 39).expect("the preceding bookmark should stay in view");
        app.jump(39);
        let output = settle(&mut app, &context);
        assert_eq!(row(&output, 40), Some(selected));
        assert_eq!(row(&output, 39), Some(previous));
        app.jump(0);
        let output = settle(&mut app, &context);
        assert!(row(&output, 0).is_some());
        assert!(row(&output, 1).is_some());
        app.jump(79);
        let output = settle(&mut app, &context);
        assert!(row(&output, 79).is_some());
        assert!(row(&output, 78).is_some());
        let position = row(&output, 79).unwrap().center();
        frame(
            &mut app,
            &context,
            vec![
                egui::Event::PointerMoved(position),
                egui::Event::MouseWheel {
                    unit: egui::MouseWheelUnit::Point,
                    delta: egui::vec2(0.0, 1000.0),
                    modifiers: egui::Modifiers::NONE,
                },
            ],
        );
        assert!(row(&settle(&mut app, &context), 79).is_none());
        assert!(row(&settle(&mut app, &context), 79).is_none());
        app.jump(79);
        assert!(row(&settle(&mut app, &context), 79).is_some());
    }
}
