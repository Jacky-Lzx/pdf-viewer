use super::{Pagewing, RowAction, removable_row, reveal_offset};
use crate::app::list_focus::ListFocus;
use eframe::egui;

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
            scroll = scroll.vertical_scroll_offset(reveal_offset(offset, top, row_height, height));
        }
        let mut target = None;
        let mut remove = None;
        let mut focus = ListFocus::new(ui, "bookmarks", row_height, height);
        focus.remap(|page| self.bookmark_pages.binary_search(&page).ok());
        let scroll = focus.scroll(ui, scroll);
        scroll.show_rows(ui, row_height, self.bookmark_pages.len(), |ui, range| {
            for row in range {
                let page = self.bookmark_pages[row];
                // A global scope keeps button IDs independent of row position.
                ui.scope_builder(
                    egui::UiBuilder::new().id(("sidebar-bookmark", self.id, page)),
                    |ui| {
                        let response = removable_row(
                            ui,
                            page == self.nav.page,
                            self.labels.sidebar_caption(page),
                            "Delete bookmark",
                            |ui| {
                                ui.label(self.labels.sidebar_tooltip(page));
                            },
                        );
                        focus.observe_id(response.focused, row, page);
                        match response.action {
                            Some(RowAction::Jump) => target = Some(page),
                            Some(RowAction::Remove) => remove = Some(page),
                            None => {}
                        }
                    },
                );
            }
        });
        focus.finish(ui);
        if let Some(page) = target {
            self.jump(page);
        }
        if let Some(page) = remove {
            self.remove_bookmark(page);
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

    fn sidebar_frame(
        app: &mut Pagewing,
        context: &egui::Context,
        size: egui::Vec2,
        events: Vec<egui::Event>,
    ) -> egui::FullOutput {
        context.run(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, size)),
                time: Some(context.input(|input| input.time) + 0.25),
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

    fn settle_sidebar(
        app: &mut Pagewing,
        context: &egui::Context,
        size: egui::Vec2,
    ) -> egui::FullOutput {
        let mut output = egui::FullOutput::default();
        for _ in 0..6 {
            output = sidebar_frame(app, context, size, Vec::new());
        }
        output
    }

    fn click_sidebar(
        app: &mut Pagewing,
        context: &egui::Context,
        size: egui::Vec2,
        pos: egui::Pos2,
    ) {
        for pressed in [true, false] {
            sidebar_frame(
                app,
                context,
                size,
                vec![
                    egui::Event::PointerMoved(pos),
                    egui::Event::PointerButton {
                        pos,
                        button: egui::PointerButton::Primary,
                        pressed,
                        modifiers: egui::Modifiers::NONE,
                    },
                ],
            );
        }
    }

    fn delete_button(output: &egui::FullOutput, page: usize) -> egui::Rect {
        let label = row(output, page).expect("bookmark caption must be visible");
        let (text, clip) = output
            .shapes
            .iter()
            .find_map(|shape| match &shape.shape {
                egui::Shape::Text(text) if text.galley.text() == "×" => {
                    let rect = egui::Rect::from_min_size(text.pos, text.galley.size());
                    (label.y_range().intersects(rect.y_range())).then_some((rect, shape.clip_rect))
                }
                _ => None,
            })
            .expect("each bookmark must have its own delete button");
        let button = output
            .shapes
            .iter()
            .filter_map(|shape| match &shape.shape {
                egui::Shape::Rect(rect) if rect.rect.contains_rect(text) => Some(rect.rect),
                _ => None,
            })
            .min_by(|a, b| a.area().total_cmp(&b.area()))
            .unwrap();
        assert!(
            clip.contains_rect(button.shrink(0.5)),
            "delete button clipped: {button:?}, {clip:?}"
        );
        assert!(label.right() < button.left());
        button
    }

    #[test]
    fn focused_sidebar_buttons_keep_tab_and_activation_out_of_reader_commands() {
        let (_directory, context, mut app) = app();
        let size = egui::vec2(1180.0, 820.0);
        app.library.sidebar = true;
        app.reading.bookmarks = [0, 2, 4].into_iter().collect();
        app.refresh_bookmarks();
        app.outline.set_entries(vec![crate::outline::OutlineEntry {
            title: "Keyboard chapter".into(),
            destination: Some(crate::destination::Destination::page(2)),
            depth: 0,
        }]);
        app.nav.jump(2);
        app.focus_contents();
        settle_sidebar(&mut app, &context, size);
        let key_events = |key, modifiers| {
            [true, false]
                .map(|pressed| egui::Event::Key {
                    key,
                    physical_key: None,
                    pressed,
                    repeat: false,
                    modifiers,
                })
                .into()
        };
        sidebar_frame(
            &mut app,
            &context,
            size,
            key_events(egui::Key::Tab, egui::Modifiers::NONE),
        );
        let focused = context.memory(|memory| memory.focused()).unwrap();
        assert_ne!(focused, egui::Id::new("pagewing-outline-input"));
        for (key, modifiers) in [
            (egui::Key::Tab, egui::Modifiers::NONE),
            (egui::Key::Tab, egui::Modifiers::SHIFT),
            (egui::Key::Space, egui::Modifiers::NONE),
            (egui::Key::Enter, egui::Modifiers::NONE),
        ] {
            context.memory_mut(|memory| memory.request_focus(focused));
            sidebar_frame(&mut app, &context, size, Vec::new());
            app.pan = egui::vec2(37.0, 53.0);
            app.pending_destination = None;
            sidebar_frame(&mut app, &context, size, key_events(key, modifiers));
            // egui applies Shift+Tab's previous widget on the following pass.
            sidebar_frame(&mut app, &context, size, Vec::new());
            assert!(app.library.sidebar, "{key:?} must stay inside the sidebar");
            assert_eq!(app.nav.page, 2, "{key:?} must not turn the PDF page");
            if key == egui::Key::Tab {
                assert_eq!(app.pan, egui::vec2(37.0, 53.0));
                assert_ne!(
                    context.memory(|memory| memory.focused()),
                    Some(focused),
                    "{modifiers:?}"
                );
            } else {
                assert_eq!(
                    app.pending_destination
                        .map(|(destination, _)| destination.page),
                    Some(2),
                    "{key:?} must activate the focused chapter"
                );
            }
        }
        for resized in [egui::vec2(240.0, 240.0), size] {
            settle_sidebar(&mut app, &context, resized);
            assert_eq!(
                context.memory(|memory| memory.focused()),
                Some(focused),
                "retain the same chapter button after resizing"
            );
            let response = context.read_response(focused).unwrap();
            assert!(
                response
                    .interact_rect
                    .contains_rect(response.rect.shrink(1.0)),
                "focused control must be fully visible after resizing"
            );
        }
        // Traverse actual virtual bookmark controls, then activate jump and
        // delete separately. A focused row must receive Space/Enter itself.
        context.memory_mut(|memory| memory.request_focus(focused));
        settle_sidebar(&mut app, &context, size);
        let mut bookmark = None;
        for _ in 0..10 {
            sidebar_frame(
                &mut app,
                &context,
                size,
                key_events(egui::Key::Tab, egui::Modifiers::NONE),
            );
            let output = settle_sidebar(&mut app, &context, size);
            if let Some(id) = context.memory(|memory| memory.focused())
                && context.read_response(id).is_some_and(|response| {
                    row(&output, 2).is_some_and(|label| response.rect.contains_rect(label))
                })
            {
                bookmark = Some(id);
                break;
            }
        }
        let bookmark = bookmark.expect("Tab must reach the bookmark jump button");
        app.remove_bookmark(0);
        settle_sidebar(&mut app, &context, size);
        assert_eq!(
            context.memory(|memory| memory.focused()),
            Some(bookmark),
            "removing an earlier bookmark must retain the focused target"
        );
        app.restore_deleted();
        settle_sidebar(&mut app, &context, size);
        assert_eq!(
            context.memory(|memory| memory.focused()),
            Some(bookmark),
            "restoring an earlier bookmark must retain the focused target"
        );
        app.jump(0);
        settle_sidebar(&mut app, &context, size);
        assert_eq!(context.memory(|memory| memory.focused()), Some(bookmark));
        sidebar_frame(
            &mut app,
            &context,
            size,
            key_events(egui::Key::Space, egui::Modifiers::NONE),
        );
        assert_eq!(app.nav.page, 2, "Space follows the focused bookmark");
        sidebar_frame(
            &mut app,
            &context,
            size,
            key_events(egui::Key::Tab, egui::Modifiers::SHIFT),
        );
        settle_sidebar(&mut app, &context, size);
        sidebar_frame(
            &mut app,
            &context,
            size,
            key_events(egui::Key::Enter, egui::Modifiers::NONE),
        );
        assert_eq!(
            app.bookmark_pages,
            [0, 4],
            "Enter activates the delete button"
        );
        assert_eq!(app.nav.page, 2, "deletion preserves the current page");
        sidebar_frame(
            &mut app,
            &context,
            size,
            key_events(egui::Key::Escape, egui::Modifiers::NONE),
        );
        sidebar_frame(
            &mut app,
            &context,
            size,
            key_events(egui::Key::Tab, egui::Modifiers::NONE),
        );
        assert!(!app.library.sidebar, "Escape returns Tab to the reader");
    }

    #[test]
    fn delete_actions_fit_narrow_sidebars_and_preserve_reading_position() {
        use crate::navigation::Fit;
        for size in [
            egui::vec2(1180.0, 820.0),
            egui::vec2(240.0, 400.0),
            egui::vec2(240.0, 240.0),
            egui::vec2(360.0, 540.0),
        ] {
            let (_directory, context, mut app) = app();
            app.library.sidebar = true;
            app.nav.jump(2);
            app.reading.bookmarks = [4, 79].into_iter().collect();
            app.refresh_bookmarks();
            app.reading.fit = Fit::Custom(1.75);
            app.reading.rotation = 1;
            app.reading.continuous = true;
            app.reading.facing = false;
            app.pan = egui::vec2(37.0, 53.0);
            app.pending_offset = Some([0.4, 0.6]);
            let output = settle_sidebar(&mut app, &context, size);
            let delete = delete_button(&output, 79);
            assert!(egui::Rect::from_min_size(egui::Pos2::ZERO, size).contains_rect(delete));
            app.dirty = false;
            click_sidebar(&mut app, &context, size, delete.center());
            assert_eq!(app.nav.page, 2, "deleting another page must not navigate");
            assert_eq!(app.pan, egui::vec2(37.0, 53.0));
            assert_eq!(app.pending_offset, Some([0.4, 0.6]));
            assert_eq!(app.reading.fit, Fit::Custom(1.75));
            assert_eq!(app.reading.rotation, 1);
            assert!(app.reading.continuous);
            assert!(!app.reading.facing);
            assert!(app.dirty, "deletion must be persisted");
            assert_eq!(app.bookmark_pages, [4]);
            assert_eq!(app.reading.bookmarks, [4].into_iter().collect());
            assert_eq!(app.message.as_ref().unwrap().text, "Bookmark removed");
            assert!(app.reveal_page.is_none());
            let output = settle_sidebar(&mut app, &context, size);
            click_sidebar(&mut app, &context, size, row(&output, 4).unwrap().center());
            assert_eq!(app.nav.page, 4, "remaining row must keep its destination");
            let output = settle_sidebar(&mut app, &context, size);
            let delete = delete_button(&output, 4);
            click_sidebar(&mut app, &context, size, delete.center());
            assert_eq!(
                app.nav.page, 4,
                "deleting current bookmark must not navigate"
            );
            assert!(app.reading.bookmarks.is_empty());
            assert!(app.bookmark_pages.is_empty());
        }
    }

    #[test]
    fn editing_earlier_bookmarks_keeps_a_virtual_boundary_target_focused() {
        let (_directory, context, mut app) = app();
        app.jump(40);
        settle(&mut app, &context);
        let key = |key| {
            [true, false]
                .map(|pressed| egui::Event::Key {
                    key,
                    physical_key: None,
                    pressed,
                    repeat: false,
                    modifiers: egui::Modifiers::NONE,
                })
                .into()
        };
        let mut focused = None;
        for _ in 0..30 {
            frame(&mut app, &context, key(egui::Key::Tab));
            let output = settle(&mut app, &context);
            if let Some(id) = context.memory(|memory| memory.focused())
                && context.read_response(id).is_some_and(|response| {
                    row(&output, 40).is_some_and(|label| {
                        response.rect.contains_rect(label)
                            && response.rect.height() < 2.0 * label.height()
                    })
                })
            {
                focused = Some(id);
                break;
            }
        }
        let focused = focused.expect("Tab must reach the distant bookmark jump button");
        // Place the target at the top of the inner viewport. Removing enough
        // predecessors moves it beyond show_rows' overscan without remapping.
        let _ = context.run(egui::RawInput::default(), |context| {
            egui::CentralPanel::default().show(context, |ui| {
                let id = ui.make_persistent_id(egui::Id::new("bookmarks"));
                let mut state = egui::scroll_area::State::load(context, id).unwrap();
                state.offset.y =
                    40.0 * (Pagewing::sidebar_row_height(ui) + ui.spacing().item_spacing.y);
                state.store(context, id);
                app.bookmark_list(ui);
            });
        });
        assert!(row(&settle(&mut app, &context), 40).is_some());
        assert_eq!(context.memory(|memory| memory.focused()), Some(focused));
        for page in 0..12 {
            app.remove_bookmark(page);
        }
        assert!(row(&settle(&mut app, &context), 40).is_some());
        assert_eq!(context.memory(|memory| memory.focused()), Some(focused));
        app.restore_deleted();
        assert!(row(&settle(&mut app, &context), 40).is_some());
        assert_eq!(context.memory(|memory| memory.focused()), Some(focused));
        app.nav.jump(0);
        frame(&mut app, &context, key(egui::Key::Space));
        assert_eq!(app.nav.page, 40, "Space follows the same logical bookmark");
        app.remove_bookmark(40);
        settle(&mut app, &context);
        assert_ne!(
            context.memory(|memory| memory.focused()),
            Some(focused),
            "deleting the focused target must not focus its replacement row"
        );
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
