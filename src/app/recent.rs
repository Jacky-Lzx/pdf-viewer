use super::{Pagewing, dialog};
use eframe::egui;

impl Pagewing {
    pub(super) fn toggle_recent(&mut self, context: &egui::Context) {
        self.outline.cancel_input();
        self.close_zoom(context);
        if self.recent_picker.take().is_some() {
            context.memory_mut(|memory| {
                memory.surrender_focus(egui::Id::new("pagewing-recent-input"))
            });
            return;
        }
        self.nav.clear_prefix();
        self.hints.cancel();
        self.selection.cancel();
        self.mark_action = None;
        self.help = false;
        self.properties_open = false;
        self.page_input = None;
        self.password_input = None;
        self.search_open = false;
        self.search_navigation.clear();
        self.recent_picker = Some(crate::recent::RecentPicker::new(
            self.library.recent.iter().map(|book| book.path.clone()),
        ));
    }

    pub(super) fn recent_dialog(&mut self, context: &egui::Context) {
        let Some(mut picker) = self.recent_picker.take() else {
            return;
        };
        let mut open = true;
        let mut cancelled = false;
        let mut chosen = None;
        let mut removed = None;
        let mut undo = false;
        let size = context.content_rect().size();
        let compact = size.x < 480.0 || size.y < 420.0;
        dialog::window(context, "Recent documents", "Recent", 580.0)
            .open(&mut open)
            .show(context, |ui| {
                let viewport_id = ui.make_persistent_id("recent-viewport");
                let resized = ui.data_mut(|data| {
                    let previous = data.get_temp::<egui::Vec2>(viewport_id);
                    if previous != Some(size) {
                        data.insert_temp(viewport_id, size);
                        true
                    } else {
                        false
                    }
                });
                // The result viewport shrinks with the screen budget. Reveal
                // the selection again without overriding ordinary wheel scroll.
                picker.reveal |= resized;
                let (input, _) = super::shortcuts::text_input(
                    ui,
                    egui::Id::new("pagewing-recent-input"),
                    &mut picker.focus,
                    &mut self.deferred_input,
                    egui::TextEdit::singleline(&mut picker.query)
                        .desired_width(f32::INFINITY)
                        .hint_text("Filter by filename or path…"),
                );
                if input.changed() {
                    picker.filter();
                }
                let list_height =
                    (size.y - if compact { 180.0 } else { 230.0 }).clamp(32.0, 280.0);
                let row_height = 32.0_f32
                    .max(ui.spacing().interact_size.y)
                    .max(ui.text_style_height(&egui::TextStyle::Button)
                        + ui.spacing().button_padding.y * 2.0);
                if picker.undo.is_some() {
                    ui.horizontal(|ui| {
                        ui.weak(format!("{} documents", picker.len()));
                        undo = ui
                            .small_button(if compact { "Undo" } else { "Undo remove" })
                            .on_hover_text("Restore the last removed document and its saved reading state")
                            .clicked();
                    });
                } else if compact {
                    ui.weak(format!("{} documents", picker.len()));
                } else {
                    ui.weak(format!(
                        "{} documents · ↑/↓ · PgUp/PgDn · Enter to open",
                        picker.len()
                    ));
                }
                let input = input.on_hover_text(
                    "↑/↓ or Ctrl+J/K: select · PageUp/PageDown: move by visible rows · Ctrl/Cmd+Home/End: first/last · Enter: open",
                );
                egui::ScrollArea::vertical()
                    .id_salt("pagewing-recent-results")
                    // A pending offset must not override the next selection
                    // when key batches and window resizing happen together.
                    .animated(false)
                    .max_height(list_height)
                    .show_viewport(ui, |ui, viewport| {
                        if !ui.is_sizing_pass() {
                            let spacing = ui.spacing().item_spacing.y;
                            let page_rows = crate::list_navigation::page_rows(
                                viewport.height(), row_height, spacing,
                            );
                            picker.apply_motions(page_rows);
                            if std::mem::take(&mut picker.confirm) {
                                chosen = picker.selected_path().map(ToOwned::to_owned);
                            }
                        }
                        let reveal = !ui.is_sizing_pass() && std::mem::take(&mut picker.reveal);
                        if picker.len() == 0 {
                            ui.label(if self.library.recent.is_empty() {
                                "No recent documents yet. Open a PDF to get started."
                            } else {
                                "No matching documents."
                            });
                        }
                        for row in 0..picker.len() {
                            let active = (self.nav.count > 0 || self.loading)
                                && picker.path(row) == self.reading.path;
                            let (response, remove) = ui
                                .push_id(picker.path(row), |ui| {
                                    let (rect, _) = ui.allocate_exact_size(
                                        egui::vec2(ui.available_width(), row_height),
                                        egui::Sense::hover(),
                                    );
                                    let remove_rect = egui::Rect::from_min_max(
                                        egui::pos2((rect.right() - 24.0).max(rect.left()), rect.top()),
                                        rect.max,
                                    );
                                    let open_rect = egui::Rect::from_min_max(
                                        rect.min,
                                        egui::pos2((remove_rect.left() - ui.spacing().item_spacing.x).max(rect.left()), rect.bottom()),
                                    );
                                    let response = ui.place(
                                        open_rect,
                                        egui::Button::selectable(picker.selected == row, picker.label(row)).truncate(),
                                    ).on_hover_text(picker.display(row));
                                    // This child stays inside the allocated row. An
                                    // enabled scope at the next cursor would add a
                                    // second spacing gap after every result.
                                    let mut remove_ui = ui.new_child(
                                        egui::UiBuilder::new()
                                            .max_rect(remove_rect)
                                            .layout(egui::Layout::centered_and_justified(egui::Direction::TopDown)),
                                    );
                                    if active {
                                        remove_ui.disable();
                                    }
                                    let remove = remove_ui.add(egui::Button::new("×"))
                                        .on_hover_text("Remove from recent, including saved position, bookmarks and marks. The PDF is kept. Delete also works when this row is focused.")
                                        .on_disabled_hover_text("Close this document before removing its saved history.");
                                    remove.widget_info(|| egui::WidgetInfo::labeled(
                                        egui::WidgetType::Button,
                                        !active,
                                        format!("Remove {} from recent", picker.display(row)),
                                    ));
                                    (response, remove)
                                })
                                .inner;
                            if reveal && picker.selected == row {
                                // Keep visible neighbors still; only reveal the
                                // portion that lies outside the result viewport.
                                response.scroll_to_me_animation(
                                    None,
                                    egui::style::ScrollAnimation::none(),
                                );
                            }
                            if super::list_focus::row_focus(&response)
                                .or_else(|| super::list_focus::row_focus(&remove))
                                .is_some()
                            {
                                picker.selected = row;
                            }
                            let delete = response.has_focus()
                                && ui.input_mut(|input| input.consume_key(egui::Modifiers::NONE, egui::Key::Delete));
                            if !active && (remove.clicked() || delete) {
                                removed = Some(picker.path(row).to_owned());
                            }
                            if response.clicked() {
                                picker.selected = row;
                                input.request_focus();
                            }
                            if response.double_clicked()
                                || (response.clicked()
                                    && !response.clicked_by(egui::PointerButton::Primary))
                            {
                                chosen = picker.selected_path().map(ToOwned::to_owned);
                            }
                        }
                    });
                ui.separator();
                ui.horizontal(|ui| {
                    if ui
                        .add_enabled(picker.selected_path().is_some(), egui::Button::new("Open"))
                        .on_hover_text("Enter")
                        .clicked()
                    {
                        chosen = picker.selected_path().map(ToOwned::to_owned);
                    }
                    let path = picker.selected_path().and_then(|path| path.to_str());
                    if ui
                        .add_enabled(
                            path.is_some(),
                            egui::Button::new(if compact { "Copy" } else { "Copy path" }),
                        )
                        .on_hover_text("Copy the selected document's full path")
                        .on_disabled_hover_text(if picker.selected_path().is_some() {
                            "This filename cannot be represented exactly as text."
                        } else {
                            "No document selected."
                        })
                        .clicked()
                        && let Some(path) = path
                    {
                        context.copy_text(path.to_owned());
                        self.notify("Copied document path");
                    }
                    if ui.button("Cancel").on_hover_text("Escape").clicked() {
                        cancelled = true;
                    }
                    if !compact {
                        ui.weak("Ctrl/Cmd+P · Escape to close");
                    }
                });
            });
        if undo {
            if let Some(record) = picker.undo.take() {
                let path = record.path().to_owned();
                self.library.restore_recent(record);
                picker.refresh(
                    self.library.recent.iter().map(|book| book.path.clone()),
                    Some(&path),
                );
                self.dirty = true;
                chosen = None;
            }
        } else if let Some(path) = removed
            && let Some(record) = self.library.remove_recent(&path)
        {
            picker.undo = Some(record);
            picker.refresh(
                self.library.recent.iter().map(|book| book.path.clone()),
                None,
            );
            self.dirty = true;
            chosen = None;
        }
        if let Some(path) = chosen {
            self.open(path, context);
        } else if open && !cancelled {
            self.recent_picker = Some(picker);
        } else {
            context.memory_mut(|memory| {
                memory.surrender_focus(egui::Id::new("pagewing-recent-input"))
            });
        }
    }
}

#[cfg(test)]
mod tests {
    mod removal;
    use super::*;
    use crate::recent::RecentPicker;

    fn key(key: egui::Key) -> Vec<egui::Event> {
        modified_key(key, egui::Modifiers::NONE)
    }

    fn modified_key(key: egui::Key, modifiers: egui::Modifiers) -> Vec<egui::Event> {
        [true, false]
            .into_iter()
            .map(|pressed| egui::Event::Key {
                key,
                physical_key: None,
                pressed,
                repeat: false,
                modifiers,
            })
            .collect()
    }

    fn frame(app: &mut Pagewing, context: &egui::Context, events: Vec<egui::Event>) {
        let _ = sized_frame(app, context, egui::vec2(1180.0, 820.0), events);
    }

    fn sized_frame(
        app: &mut Pagewing,
        context: &egui::Context,
        size: egui::Vec2,
        mut events: Vec<egui::Event>,
    ) -> egui::FullOutput {
        let mut deferred = std::mem::take(&mut app.deferred_input);
        deferred.append(&mut events);
        context.run(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, size)),
                time: Some(context.input(|input| input.time) + 0.25),
                events: deferred,
                ..Default::default()
            },
            |context| {
                app.keys(context);
                app.recent_dialog(context);
                egui::CentralPanel::default().show(context, |_| {});
            },
        )
    }

    fn selected_row(context: &egui::Context, output: &egui::FullOutput) -> Option<egui::Rect> {
        let selected = context.style().visuals.selection.bg_fill;
        output.shapes.iter().find_map(|shape| match &shape.shape {
            egui::Shape::Rect(rect)
                if rect.fill == selected
                    && rect.rect.width() > 60.0
                    && shape.clip_rect.contains_rect(rect.rect) =>
            {
                Some(rect.rect)
            }
            _ => None,
        })
    }

    fn copied(output: &egui::FullOutput) -> Option<&str> {
        output
            .platform_output
            .commands
            .iter()
            .find_map(|command| match command {
                egui::OutputCommand::CopyText(text) => Some(text.as_str()),
                _ => None,
            })
    }

    fn click_action(
        app: &mut Pagewing,
        context: &egui::Context,
        size: egui::Vec2,
        label: &str,
        mut pending: Vec<egui::Event>,
    ) -> egui::FullOutput {
        let output = sized_frame(app, context, size, Vec::new());
        let pos = output
            .shapes
            .iter()
            .find_map(|shape| match &shape.shape {
                egui::Shape::Text(text) if text.galley.text() == label => {
                    Some(text.pos + text.galley.size() * 0.5)
                }
                _ => None,
            })
            .unwrap_or_else(|| panic!("missing {label} at {size:?}"));
        let mut output = egui::FullOutput::default();
        for pressed in [true, false] {
            let mut events = std::mem::take(&mut pending);
            events.extend([
                egui::Event::PointerMoved(pos),
                egui::Event::PointerButton {
                    pos,
                    button: egui::PointerButton::Primary,
                    pressed,
                    modifiers: egui::Modifiers::NONE,
                },
            ]);
            output = sized_frame(app, context, size, events);
        }
        output
    }

    #[test]
    fn keyboard_copy_path_preserves_the_picker_reader_and_unsaved_comment() {
        for size in [egui::vec2(1180.0, 820.0), egui::vec2(240.0, 240.0)] {
            let directory = tempfile::tempdir().unwrap();
            let context = egui::Context::default();
            let mut app =
                Pagewing::with_context(&context, None, None, directory.path().join("library.json"));
            app.nav = crate::navigation::Navigation::new(6, 2);
            let current = directory.path().join("current.pdf");
            app.reading.path = current.clone();
            app.comments.editor = Some(crate::annotations::Comment {
                page: 2,
                slot: None,
                parent: None,
                review: Default::default(),
                marked: false,
                states: Vec::new(),
                kind: "Highlight".into(),
                contents: "Unsaved draft 📝".into(),
                author: "Pagewing".into(),
                rects: Vec::new(),
                has_appearance: false,
                truncated: false,
                locked: false,
            });
            let path = directory.path().join("审阅 Straße 📝 notes.pdf");
            app.recent_picker = Some(RecentPicker::new(
                [current.clone(), path.clone()].into_iter(),
            ));
            for _ in 0..4 {
                sized_frame(&mut app, &context, size, Vec::new());
            }
            sized_frame(
                &mut app,
                &context,
                size,
                vec![egui::Event::Paste("strasse".into())],
            );
            // The result, remove, Open and Copy are distinct focus stops.
            for _ in 0..4 {
                sized_frame(&mut app, &context, size, key(egui::Key::Tab));
            }
            for activation in [egui::Key::Enter, egui::Key::Space] {
                let output = sized_frame(&mut app, &context, size, key(activation));
                assert_eq!(copied(&output), path.to_str(), "{size:?}: {activation:?}");
                assert_eq!(app.recent_picker.as_ref().unwrap().query, "strasse");
                assert_eq!(
                    app.recent_picker.as_ref().unwrap().selected_path(),
                    Some(path.as_path())
                );
                assert_eq!(app.reading.path, current);
                assert_eq!(app.nav.page, 2);
                assert!(!app.loading);
                assert!(app.comments.unsaved());
                assert_eq!(
                    app.comments.editor.as_ref().unwrap().contents,
                    "Unsaved draft 📝"
                );
            }
            let input = egui::Id::new("pagewing-recent-input");
            for _ in 0..6 {
                sized_frame(
                    &mut app,
                    &context,
                    size,
                    modified_key(egui::Key::Tab, egui::Modifiers::SHIFT),
                );
                sized_frame(&mut app, &context, size, Vec::new());
                if context.memory(|memory| memory.has_focus(input)) {
                    break;
                }
            }
            assert!(context.memory(|memory| memory.has_focus(input)));
            sized_frame(
                &mut app,
                &context,
                size,
                modified_key(egui::Key::A, egui::Modifiers::COMMAND),
            );
            let mut copy = modified_key(egui::Key::C, egui::Modifiers::COMMAND);
            copy.push(egui::Event::Copy);
            let output = sized_frame(&mut app, &context, size, copy);
            assert_eq!(
                copied(&output),
                Some("strasse"),
                "Ctrl/Cmd+C still copies selected filter text"
            );
        }
    }

    #[test]
    fn mouse_copy_path_applies_pending_filter_text_before_copying() {
        let directory = tempfile::tempdir().unwrap();
        let context = egui::Context::default();
        let mut app =
            Pagewing::with_context(&context, None, None, directory.path().join("library.json"));
        let path = directory.path().join("审阅 Straße 📝 notes.pdf");
        app.recent_picker = Some(RecentPicker::new(
            [directory.path().join("other.pdf"), path.clone()].into_iter(),
        ));
        let size = egui::vec2(1180.0, 820.0);
        for _ in 0..4 {
            sized_frame(&mut app, &context, size, Vec::new());
        }
        let output = click_action(
            &mut app,
            &context,
            size,
            "Copy path",
            vec![egui::Event::Paste("strasse".into())],
        );
        assert_eq!(copied(&output), path.to_str());
        assert_eq!(app.recent_picker.as_ref().unwrap().query, "strasse");
        assert!(!app.loading);
        app.recent_picker.as_mut().unwrap().query = "missing".into();
        app.recent_picker.as_mut().unwrap().filter();
        let output = click_action(&mut app, &context, size, "Copy path", Vec::new());
        assert!(
            copied(&output).is_none(),
            "empty results must not overwrite the clipboard"
        );
        assert!(app.recent_picker.is_some());
    }

    #[cfg(unix)]
    #[test]
    fn copy_path_never_substitutes_invalid_filename_bytes() {
        use std::os::unix::ffi::OsStringExt;
        let directory = tempfile::tempdir().unwrap();
        let context = egui::Context::default();
        let mut app =
            Pagewing::with_context(&context, None, None, directory.path().join("library.json"));
        let path = directory
            .path()
            .join(std::ffi::OsString::from_vec(b"review-\xff.pdf".to_vec()));
        app.recent_picker = Some(RecentPicker::new(std::iter::once(path.clone())));
        let size = egui::vec2(240.0, 240.0);
        for _ in 0..4 {
            sized_frame(&mut app, &context, size, Vec::new());
        }
        let output = click_action(&mut app, &context, size, "Copy", Vec::new());
        assert!(copied(&output).is_none());
        assert_eq!(
            app.recent_picker.as_ref().unwrap().selected_path(),
            Some(path.as_path())
        );
        click_action(&mut app, &context, size, "Open", Vec::new());
        assert_eq!(
            app.reading.path, path,
            "Open must preserve the original filename bytes"
        );
    }

    #[test]
    fn selecting_a_visible_recent_document_keeps_neighboring_rows_still() {
        let directory = tempfile::tempdir().unwrap();
        let context = egui::Context::default();
        let mut app =
            Pagewing::with_context(&context, None, None, directory.path().join("library.json"));
        app.recent_picker =
            Some(RecentPicker::new((0..24).map(|index| {
                directory.path().join(format!("book-{index}.pdf"))
            })));
        for _ in 0..4 {
            frame(&mut app, &context, Vec::new());
        }
        app.recent_picker.as_mut().unwrap().step(12);
        let mut output = egui::FullOutput::default();
        for _ in 0..4 {
            output = sized_frame(&mut app, &context, egui::vec2(1180.0, 820.0), Vec::new());
        }
        let row_rect = |output: &egui::FullOutput, label: &str| {
            output.shapes.iter().find_map(|shape| match &shape.shape {
                egui::Shape::Text(text) if text.galley.text() == label => {
                    let rect = egui::Rect::from_min_size(text.pos, text.galley.size());
                    shape.clip_rect.contains_rect(rect).then_some(rect)
                }
                _ => None,
            })
        };
        let label = app.recent_picker.as_ref().unwrap().label(11).to_owned();
        let before = row_rect(&output, &label).expect("the previous result is fully visible");
        for (motion, selected) in [(egui::Key::ArrowUp, 11), (egui::Key::ArrowDown, 12)] {
            frame(&mut app, &context, key(motion));
            for _ in 0..4 {
                output = sized_frame(&mut app, &context, egui::vec2(1180.0, 820.0), Vec::new());
            }
            assert_eq!(app.recent_picker.as_ref().unwrap().selected, selected);
            assert!(selected_row(&context, &output).is_some());
            assert_eq!(
                row_rect(&output, &label),
                Some(before),
                "keyboard navigation must preserve the position of an already visible result"
            );
        }
    }

    #[test]
    fn recent_page_navigation_uses_the_current_viewport_and_preserves_event_order() {
        let directory = tempfile::tempdir().unwrap();
        let context = egui::Context::default();
        let mut app =
            Pagewing::with_context(&context, None, None, directory.path().join("library.json"));
        app.recent_picker =
            Some(RecentPicker::new((0..24).map(|index| {
                directory.path().join(format!("book-{index}.pdf"))
            })));
        for (size, page_rows) in [
            (egui::vec2(1180.0, 820.0), 7),
            (egui::vec2(240.0, 240.0), 1),
            (egui::vec2(480.0, 420.0), 4),
            (egui::vec2(240.0, 820.0), 7),
        ] {
            let mut events = modified_key(egui::Key::Home, egui::Modifiers::COMMAND);
            events.extend(key(egui::Key::PageDown));
            sized_frame(&mut app, &context, size, events);
            let mut output = egui::FullOutput::default();
            for _ in 0..4 {
                output = sized_frame(&mut app, &context, size, Vec::new());
            }
            assert_eq!(app.recent_picker.as_ref().unwrap().selected, page_rows);
            assert!(
                selected_row(&context, &output).is_some(),
                "selection must be visible at {size:?}"
            );
            sized_frame(&mut app, &context, size, key(egui::Key::PageUp));
            assert_eq!(app.recent_picker.as_ref().unwrap().selected, 0);
        }
        let style = context.style();
        context.style_mut(|style| {
            style
                .text_styles
                .get_mut(&egui::TextStyle::Button)
                .unwrap()
                .size = 28.0;
        });
        let mut events = modified_key(egui::Key::Home, egui::Modifiers::COMMAND);
        events.extend(key(egui::Key::PageDown));
        frame(&mut app, &context, events);
        let mut output = egui::FullOutput::default();
        for _ in 0..4 {
            output = sized_frame(&mut app, &context, egui::vec2(1180.0, 820.0), Vec::new());
        }
        assert!((1..7).contains(&app.recent_picker.as_ref().unwrap().selected));
        assert!(selected_row(&context, &output).is_some());
        context.set_style(style);
        for (boundary, page, arrow, expected) in [
            (egui::Key::End, egui::Key::PageDown, egui::Key::ArrowUp, 22),
            (egui::Key::Home, egui::Key::PageUp, egui::Key::ArrowDown, 1),
        ] {
            let mut events = modified_key(boundary, egui::Modifiers::COMMAND);
            events.extend(key(page));
            events.extend(key(arrow));
            frame(&mut app, &context, events);
            assert_eq!(app.recent_picker.as_ref().unwrap().selected, expected);
        }
        let mut events = vec![egui::Event::Paste("book-1".into())];
        events.extend(modified_key(egui::Key::End, egui::Modifiers::COMMAND));
        events.extend(key(egui::Key::PageUp));
        events.extend(key(egui::Key::Enter));
        frame(&mut app, &context, events);
        assert_eq!(app.reading.path, directory.path().join("book-12.pdf"));
        assert!(app.recent_picker.is_none());
    }

    #[test]
    fn recent_navigation_retains_text_editing_focus_and_handles_empty_results() {
        let directory = tempfile::tempdir().unwrap();
        let context = egui::Context::default();
        let mut app =
            Pagewing::with_context(&context, None, None, directory.path().join("library.json"));
        app.recent_picker =
            Some(RecentPicker::new((0..24).map(|index| {
                directory.path().join(format!("book-{index}.pdf"))
            })));
        for _ in 0..4 {
            frame(&mut app, &context, Vec::new());
        }
        let mut events = vec![egui::Event::Paste("book".into())];
        events.extend(key(egui::Key::Home));
        events.push(egui::Event::Paste("first-".into()));
        events.extend(key(egui::Key::End));
        events.push(egui::Event::Paste("-last".into()));
        frame(&mut app, &context, events);
        assert_eq!(app.recent_picker.as_ref().unwrap().query, "first-book-last");
        for motion in [egui::Key::PageUp, egui::Key::PageDown] {
            let mut events = modified_key(egui::Key::End, egui::Modifiers::COMMAND);
            events.extend(key(motion));
            events.extend(key(egui::Key::Enter));
            frame(&mut app, &context, events);
            assert!(
                app.recent_picker
                    .as_ref()
                    .unwrap()
                    .selected_path()
                    .is_none()
            );
            assert!(!app.loading);
        }
        frame(
            &mut app,
            &context,
            modified_key(egui::Key::A, egui::Modifiers::COMMAND),
        );
        frame(&mut app, &context, vec![egui::Event::Paste("book".into())]);
        frame(&mut app, &context, key(egui::Key::Tab));
        frame(&mut app, &context, key(egui::Key::PageDown));
        assert_eq!(app.recent_picker.as_ref().unwrap().selected, 0);
        assert!(!context.memory(|memory| memory.has_focus(egui::Id::new("pagewing-recent-input"))));
    }

    #[test]
    fn resizing_keeps_the_selected_recent_document_visible() {
        let directory = tempfile::tempdir().unwrap();
        let context = egui::Context::default();
        let mut app =
            Pagewing::with_context(&context, None, None, directory.path().join("library.json"));
        app.recent_picker =
            Some(RecentPicker::new((0..24).map(|index| {
                directory.path().join(format!("book-{index}.pdf"))
            })));
        for _ in 0..4 {
            frame(&mut app, &context, Vec::new());
        }
        frame(&mut app, &context, vec![egui::Event::Paste("book".into())]);
        app.recent_picker.as_mut().unwrap().step(23);
        let mut output = egui::FullOutput::default();
        for size in [
            egui::vec2(1180.0, 820.0),
            egui::vec2(860.0, 240.0),
            egui::vec2(240.0, 240.0),
        ] {
            for _ in 0..4 {
                output = sized_frame(&mut app, &context, size, Vec::new());
            }
            assert!(
                selected_row(&context, &output).is_some(),
                "selected recent row must be visible at {size:?}"
            );
            assert_eq!(app.recent_picker.as_ref().unwrap().selected, 23);
            assert_eq!(app.recent_picker.as_ref().unwrap().query, "book");
            assert!(
                context.memory(|memory| memory.has_focus(egui::Id::new("pagewing-recent-input")))
            );
        }
        let pos = selected_row(&context, &output).unwrap().center();
        let size = egui::vec2(240.0, 240.0);
        sized_frame(
            &mut app,
            &context,
            size,
            vec![
                egui::Event::PointerMoved(pos),
                egui::Event::MouseWheel {
                    unit: egui::MouseWheelUnit::Point,
                    delta: egui::vec2(0.0, 4000.0),
                    modifiers: egui::Modifiers::NONE,
                },
            ],
        );
        for _ in 0..4 {
            output = sized_frame(&mut app, &context, size, Vec::new());
        }
        assert!(
            selected_row(&context, &output).is_none(),
            "do not undo manual scrolling"
        );
        assert_eq!(app.recent_picker.as_ref().unwrap().selected, 23);
        for _ in 0..4 {
            output = sized_frame(&mut app, &context, egui::vec2(1180.0, 820.0), Vec::new());
        }
        assert!(selected_row(&context, &output).is_some());
    }

    #[test]
    fn tab_and_enter_activate_recent_rows_open_and_cancel_without_confirming_the_filter() {
        let directory = tempfile::tempdir().unwrap();
        let context = egui::Context::default();
        let mut app =
            Pagewing::with_context(&context, None, None, directory.path().join("library.json"));
        let path = directory.path().join("selected.pdf");
        for tabs in [5, 3, 1] {
            app.clear_document(&context);
            app.recent_picker = Some(RecentPicker::new(std::iter::once(path.clone())));
            for _ in 0..4 {
                frame(&mut app, &context, Vec::new());
            }
            for _ in 0..tabs {
                frame(&mut app, &context, key(egui::Key::Tab));
            }
            frame(&mut app, &context, key(egui::Key::Enter));
            assert!(app.recent_picker.is_none());
            if tabs == 5 {
                assert!(!app.loading, "Enter on Cancel must not load a document");
                assert!(app.reading.path.as_os_str().is_empty());
            } else {
                assert!(
                    app.loading,
                    "Enter on a row or Open must load the selected document"
                );
                assert_eq!(app.reading.path, path);
            }
        }
        app.clear_document(&context);
        let second = directory.path().join("second.pdf");
        app.recent_picker = Some(RecentPicker::new([path, second.clone()].into_iter()));
        for _ in 0..4 {
            frame(&mut app, &context, Vec::new());
        }
        for _ in 0..3 {
            frame(&mut app, &context, key(egui::Key::Tab));
        }
        frame(&mut app, &context, key(egui::Key::Enter));
        assert_eq!(
            app.reading.path, second,
            "activate the focused result, not the first one"
        );
    }

    #[test]
    fn recent_dialog_and_actions_fit_when_resizing() {
        let directory = tempfile::tempdir().unwrap();
        let context = egui::Context::default();
        let mut app =
            Pagewing::with_context(&context, None, None, directory.path().join("library.json"));
        for size in dialog::TEST_SIZES {
            for count in [0, 1, 24] {
                app.recent_picker = Some(RecentPicker::new((0..count).map(|index| {
                    directory
                        .path()
                        .join(format!("{index}-a-very-long-filename-without-spaces.pdf"))
                })));
                let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, size);
                let mut output = egui::FullOutput::default();
                for _ in 0..4 {
                    output = context.run(
                        egui::RawInput {
                            screen_rect: Some(screen),
                            ..Default::default()
                        },
                        |context| app.recent_dialog(context),
                    );
                }
                dialog::assert_visible_actions(
                    &context,
                    &output,
                    screen,
                    "Recent documents",
                    &[
                        "Open",
                        if size.x < 480.0 || size.y < 420.0 {
                            "Copy"
                        } else {
                            "Copy path"
                        },
                        "Cancel",
                    ],
                );
            }
        }
    }
}
