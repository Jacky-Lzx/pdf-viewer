use super::{Pagewing, shortcuts};
use crate::annotations::{Comment, Comments, ReviewStatus};
use eframe::egui::{self, Color32, Vec2};

#[derive(Default)]
pub(super) struct CommentUi {
    pub open: bool,
    pub loaded: bool,
    pub comments: Comments,
    pub editor: Option<Comment>,
    original: String,
    original_review: ReviewStatus,
    original_marked: bool,
    pub focus: bool,
    pub saving: bool,
}

impl CommentUi {
    pub fn unsaved(&self) -> bool {
        self.editor.as_ref().is_some_and(|c| {
            c.slot.is_none()
                || c.contents != self.original
                || c.review != self.original_review
                || c.marked != self.original_marked
        })
    }
    pub fn busy(&self) -> bool {
        self.saving
    }
}

impl Pagewing {
    pub(super) fn comment_change_blocked(&mut self) -> bool {
        if self.comments.busy() || self.comments.unsaved() {
            self.comments.open = true;
            self.notify("Save or discard the comment before changing documents.");
            true
        } else {
            false
        }
    }

    pub(super) fn add_comment(&mut self) {
        if self.loading || self.comment_change_blocked() {
            return;
        }
        let Some(text) = self
            .selection
            .selected_text()
            .filter(|s| !s.trim().is_empty())
        else {
            self.notify("Press v and select text before adding a comment.");
            return;
        };
        if self.selection.rects.is_empty() {
            return;
        }
        self.comments.editor = Some(Comment {
            page: self.selection_page,
            slot: None,
            parent: None,
            review: Default::default(),
            marked: false,
            states: Vec::new(),
            kind: "Highlight".into(),
            contents: String::new(),
            author: "Pagewing".into(),
            rects: self.selection.rects.clone(),
            has_appearance: false,
            truncated: false,
            locked: false,
        });
        self.comments.original.clear();
        self.comments.original_review = ReviewStatus::None;
        self.comments.original_marked = false;
        self.comments.open = true;
        self.comments.focus = true;
        self.notify(format!(
            "Highlight: {}",
            text.chars().take(80).collect::<String>()
        ));
    }

    fn edit_comment(&mut self, index: usize) {
        if self.loading || self.comment_change_blocked() {
            return;
        }
        let comment = self.comments.comments.items[index].clone();
        self.jump(comment.page);
        self.pending_focus = comment.rects.first().copied();
        self.comments.original = comment.contents.clone();
        self.comments.original_review = comment.review;
        self.comments.original_marked = comment.marked;
        self.comments.editor = Some(comment);
        self.comments.open = true;
        self.comments.focus = true;
    }

    pub(super) fn save_comment(&mut self) {
        if self.loading || self.comments.busy() {
            return;
        }
        let Some(comment) = self
            .comments
            .editor
            .as_ref()
            .filter(|c| !c.locked && (c.editable() || c.contents == self.comments.original))
            .cloned()
        else {
            return;
        };
        self.comments.saving = true;
        self.worker
            .save_comment(self.id, self.reading.path.clone(), comment);
    }

    fn reply_comment(&mut self) {
        if self.loading || self.comment_change_blocked() {
            return;
        }
        let Some(reply) = self
            .comments
            .editor
            .as_ref()
            .filter(|c| !c.locked)
            .and_then(Comment::reply)
        else {
            return;
        };
        self.comments.editor = Some(reply);
        self.comments.original.clear();
        self.comments.original_review = ReviewStatus::None;
        self.comments.original_marked = false;
        self.comments.focus = true;
    }

    fn delete_comment(&mut self) {
        if self.loading || self.comments.busy() {
            return;
        }
        let Some(comment) = self
            .comments
            .editor
            .as_ref()
            .filter(|c| c.slot.is_some() && !c.locked)
            .cloned()
        else {
            return;
        };
        self.comments.saving = true;
        self.worker
            .delete_comment(self.id, self.reading.path.clone(), comment);
    }

    pub(super) fn comments_sidebar(&mut self, context: &egui::Context) {
        if !self.comments.open || self.nav.count == 0 {
            return;
        }
        let mut open = true;
        let mut selected = None;
        let mut save = false;
        let mut delete = false;
        let mut reply = false;
        let mut discard = false;
        let maximum = (context.available_rect().width() - 120.0).clamp(100.0, 480.0);
        egui::SidePanel::right("pagewing-comments-sidebar")
            .frame(super::appearance::panel(context, 14))
            .default_width(320.0_f32.min(maximum))
            .width_range(220.0_f32.min(maximum)..=maximum)
            .resizable(true)
            .show(context, |ui| {
                ui.horizontal_wrapped(|ui| {
                    ui.heading("Comments");
                    ui.weak(format!("{} total", self.comments.comments.items.len()));
                    if ui
                        .small_button("×")
                        .on_hover_text("Hide comments · Ctrl/Cmd+Shift+M")
                        .clicked()
                    {
                        open = false;
                    }
                });
                ui.label("All pages");
                if ui
                    .add_enabled(
                        !self.comments.busy() && !self.selection.rects.is_empty(),
                        egui::Button::new("Add comment to selection").wrap(),
                    )
                    .clicked()
                {
                    self.add_comment();
                }
                ui.separator();
                if !self.comments.loaded {
                    ui.spinner();
                    ui.label("Reading PDF comments…");
                } else if self.comments.comments.items.is_empty() {
                    ui.label("No existing comments.");
                }
                if self.comments.comments.limited {
                    ui.colored_label(Color32::YELLOW, "Showing a limited number of annotations.");
                }
                let list_height = if self.comments.editor.is_some() {
                    (ui.available_height() * 0.5).max(40.0)
                } else {
                    ui.available_height()
                };
                egui::ScrollArea::vertical()
                    .id_salt("pagewing-comment-list")
                    .auto_shrink([false, false])
                    .max_height(list_height)
                    .show(ui, |ui| {
                        for (index, depth) in self.comments.comments.thread_rows() {
                            let comment = &self.comments.comments.items[index];
                            let label = format!(
                                "Page {} · {}{}{}{}\n{}",
                                comment.page + 1,
                                if comment.parent.is_some() { "Reply" } else { &comment.kind },
                                if comment.author.is_empty() {
                                    String::new()
                                } else {
                                    format!(" · {}", comment.author)
                                },
                                if comment.review == ReviewStatus::None { String::new() } else { format!(" · {}", comment.review.label()) },
                                if comment.marked { " · ✓ Marked" } else { "" },
                                if comment.contents.is_empty() {
                                    "(no comment)"
                                } else {
                                    &comment.contents
                                }
                            );
                            let active = self.comments.editor.as_ref().is_some_and(|current| {
                                current.page == comment.page && current.slot == comment.slot
                            });
                            ui.horizontal(|ui| {
                                ui.add_space(depth.min(6) as f32 * 14.0);
                                if ui
                                    .add_enabled(
                                        !self.loading && !self.comments.busy(),
                                        egui::Button::selectable(active, label)
                                            .wrap()
                                            .min_size(egui::vec2(ui.available_width(), 0.0)),
                                    )
                                    .clicked()
                                {
                                    selected = Some(index);
                                }
                            });
                        }
                    });
                if self.comments.editor.is_some() {
                    egui::ScrollArea::vertical()
                        .id_salt("pagewing-comment-editor")
                        .show(ui, |ui| {
                            ui.separator();
                            let busy = self.comments.busy();
                            if let Some(comment) = &mut self.comments.editor {
                                ui.label(format!(
                                    "{} · page {}",
                                    if comment.slot.is_none() {
                                        if comment.parent.is_some() { "New reply" } else { "New highlight" }
                                    } else {
                                        "Edit comment"
                                    },
                                    comment.page + 1
                                ));
                                let read_only = !comment.editable();
                                if read_only {
                                    ui.label(if comment.locked { "This annotation is read-only." } else { "Comment text is read-only." });
                                }
                                ui.add_enabled_ui(!busy && !read_only, |ui| {
                                    shortcuts::text_input(
                                        ui,
                                        egui::Id::new("pagewing-comment-input"),
                                        &mut self.comments.focus,
                                        &mut self.deferred_input,
                                        egui::TextEdit::multiline(&mut comment.contents)
                                            .desired_rows(5)
                                            .desired_width(f32::INFINITY)
                                            .hint_text("Write a comment…"),
                                    );
                                });
                                ui.add_enabled_ui(!busy && !comment.locked, |ui| {
                                    egui::ComboBox::from_id_salt("pagewing-comment-status")
                                        .selected_text(format!("Status: {}", comment.review.label()))
                                        .show_ui(ui, |ui| {
                                            for status in ReviewStatus::ALL {
                                                ui.selectable_value(&mut comment.review, status, status.label());
                                            }
                                        });
                                    ui.checkbox(&mut comment.marked, "Marked (Acrobat checkmark)");
                                });
                                if !comment.states.is_empty() {
                                    egui::CollapsingHeader::new("Status history").show(ui, |ui| {
                                        for state in &comment.states {
                                            ui.label(format!("{} · {}: {}", state.author, state.model, state.value));
                                        }
                                    });
                                }
                                ui.label("Save writes this comment directly to the current PDF.");
                                let existing = comment.slot.is_some();
                                let deletable = !comment.locked;
                                ui.horizontal_wrapped(|ui| {
                                    save = ui
                                        .add_enabled(
                                            !busy && !comment.locked && (comment.editable() || comment.contents == self.comments.original),
                                            egui::Button::new("Save comment"),
                                        )
                                        .clicked();
                                    if existing {
                                        reply = ui.add_enabled(!busy && deletable, egui::Button::new("Reply")).clicked();
                                        delete = ui.add_enabled(!busy && deletable,
                                            egui::Button::new(if comment.parent.is_some() { "Delete reply & replies" } else { "Delete comment & highlight" }))
                                            .on_hover_text("Delete this annotation and all its replies from the current PDF.")
                                            .clicked();
                                    }
                                    if ui
                                        .add_enabled(
                                            !busy,
                                            egui::Button::new("Discard / close editor"),
                                        )
                                        .clicked()
                                    {
                                        discard = true;
                                    }
                                    if busy {
                                        ui.spinner();
                                        ui.label("Saving…");
                                    }
                                });
                            } else {
                                ui.label("Choose an annotation to read or edit its comment.");
                            }
                        });
                }
            });
        self.comments.open = open;
        if discard {
            self.comments.editor = None;
        }
        if let Some(index) = selected {
            self.edit_comment(index);
        }
        if save {
            self.save_comment();
        } else if delete {
            self.delete_comment();
        } else if reply {
            self.reply_comment();
        }
    }

    pub(super) fn comment_overlays(
        &mut self,
        ui: &mut egui::Ui,
        page: usize,
        page_rect: egui::Rect,
        dimensions: [f32; 2],
    ) {
        let mut selected = None;
        for (index, comment) in self
            .comments
            .comments
            .items
            .iter()
            .enumerate()
            .filter(|(_, c)| c.page == page && c.parent.is_none())
        {
            for (part, rect) in comment.rects.iter().enumerate() {
                let rect = self
                    .link_rect(*rect, page_rect, dimensions)
                    .intersect(page_rect);
                if !rect.is_positive() || !ui.clip_rect().intersects(rect) {
                    continue;
                }
                if !comment.has_appearance {
                    if comment.kind == "Highlight" {
                        ui.painter().rect_filled(
                            rect,
                            0.0,
                            Color32::from_rgba_unmultiplied(255, 210, 0, 75),
                        );
                    } else {
                        ui.painter().circle_filled(
                            rect.left_top() + Vec2::splat(4.0),
                            4.0,
                            Color32::from_rgb(215, 145, 30),
                        );
                    }
                }
                if !self.selection.active {
                    let response = ui.interact(
                        rect,
                        ui.id().with(("pdf-comment", page, index, part)),
                        egui::Sense::click(),
                    );
                    let response = response.on_hover_text(if comment.contents.is_empty() {
                        "Click to add a comment"
                    } else {
                        &comment.contents
                    });
                    if response.clicked() {
                        selected = Some(index);
                    }
                }
            }
        }
        if let Some(index) = selected {
            self.edit_comment(index);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use egui::{Context, Event, Key, Modifiers, RawInput};
    use std::time::{Duration, Instant};

    fn frame(app: &mut Pagewing, context: &Context, events: Vec<Event>) -> egui::FullOutput {
        frame_size(app, context, events, Vec2::new(1180.0, 820.0))
    }

    fn frame_size(
        app: &mut Pagewing,
        context: &Context,
        events: Vec<Event>,
        size: Vec2,
    ) -> egui::FullOutput {
        context.run(
            RawInput {
                screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, size)),
                events,
                ..Default::default()
            },
            |context| {
                if !app.deferred_input.is_empty() {
                    let mut events = std::mem::take(&mut app.deferred_input);
                    context.input_mut(|input| {
                        events.append(&mut input.events);
                        input.events = events;
                    });
                }
                app.events(context);
                app.keys(context);
                app.toolbar(context);
                app.comments_sidebar(context);
                app.sidebar(context);
                app.canvas(context);
            },
        )
    }

    fn key(key: Key, modifiers: Modifiers) -> Vec<Event> {
        vec![
            Event::Key {
                key,
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers,
            },
            Event::Key {
                key,
                physical_key: None,
                pressed: false,
                repeat: false,
                modifiers,
            },
        ]
    }

    fn wait(app: &mut Pagewing, context: &Context, condition: impl Fn(&Pagewing) -> bool) {
        let deadline = Instant::now() + Duration::from_secs(5);
        while !condition(app) {
            assert!(Instant::now() < deadline, "UI worker did not finish");
            frame(app, context, Vec::new());
            std::thread::sleep(Duration::from_millis(5));
        }
    }

    fn click_text(app: &mut Pagewing, context: &Context, label: &str) {
        let output = frame(app, context, Vec::new());
        let point = output
            .shapes
            .iter()
            .find_map(|shape| {
                if let egui::Shape::Text(text) = &shape.shape
                    && text.galley.text() == label
                {
                    Some(text.pos + text.galley.size() * 0.5)
                } else {
                    None
                }
            })
            .unwrap_or_else(|| panic!("Missing visible button: {label}"));
        for pressed in [true, false] {
            frame(
                app,
                context,
                vec![
                    Event::PointerMoved(point),
                    Event::PointerButton {
                        pos: point,
                        button: egui::PointerButton::Primary,
                        pressed,
                        modifiers: Modifiers::NONE,
                    },
                ],
            );
        }
    }

    #[test]
    fn reply_buttons_nested_threads_status_controls_and_draft_protection() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("threads.pdf");
        std::fs::write(&path, include_bytes!("../../assets/pagewing-guide.pdf")).unwrap();
        let pdf = crate::document::load(&path, "").unwrap();
        crate::annotations::save(
            &pdf,
            &Comment {
                page: 0,
                slot: None,
                parent: None,
                review: ReviewStatus::None,
                marked: false,
                states: Vec::new(),
                kind: "Highlight".into(),
                contents: "Root comment".into(),
                author: "Pagewing".into(),
                rects: vec![hayro::kurbo::Rect::new(50.0, 80.0, 150.0, 100.0)],
                has_appearance: false,
                truncated: false,
                locked: false,
            },
            &path,
        )
        .unwrap();
        let context = Context::default();
        let mut app = Pagewing::with_context(
            &context,
            Some(path.clone()),
            Some(0),
            root.path().join("library.json"),
        );
        wait(&mut app, &context, |app| app.comments.loaded);
        app.edit_comment(0);
        for depth in 1..=2 {
            click_text(&mut app, &context, "Reply");
            let editor = app.comments.editor.as_ref().unwrap();
            assert!(editor.slot.is_none() && editor.parent.is_some());
            assert!(app.comments.unsaved());
            frame(
                &mut app,
                &context,
                vec![Event::Text(format!("Nested reply {depth}"))],
            );
            click_text(&mut app, &context, "Save comment");
            wait(&mut app, &context, |app| {
                !app.comments.busy()
                    && !app.loading
                    && app.comments.loaded
                    && app.comments.editor.is_none()
            });
            assert_eq!(app.comments.comments.items.len(), depth + 1);
            app.edit_comment(depth);
        }
        assert_eq!(
            app.comments.comments.thread_rows(),
            [(0, 0), (1, 1), (2, 2)]
        );
        click_text(&mut app, &context, "Status: None");
        click_text(&mut app, &context, "Completed / resolved");
        assert_eq!(
            app.comments.editor.as_ref().unwrap().review,
            ReviewStatus::Completed
        );
        assert!(app.comments.unsaved(), "status changes are drafts too");
        app.reload();
        assert!(!app.reloading, "status drafts must defer reloads");
        app.edit_comment(0);
        assert_eq!(
            app.comments.editor.as_ref().unwrap().slot,
            app.comments.comments.items[2].slot,
            "changing selection must preserve the draft"
        );
        click_text(&mut app, &context, "Marked (Acrobat checkmark)");
        assert!(app.comments.editor.as_ref().unwrap().marked);
        click_text(&mut app, &context, "Save comment");
        wait(&mut app, &context, |app| {
            !app.comments.busy()
                && !app.loading
                && app.comments.loaded
                && app.comments.editor.is_none()
        });
        assert_eq!(
            app.comments.comments.items[2].review,
            ReviewStatus::Completed
        );
        assert!(app.comments.comments.items[2].marked);
        assert_eq!(app.comments.comments.items.len(), 3);
        app.edit_comment(1);
        click_text(&mut app, &context, "Delete reply & replies");
        wait(&mut app, &context, |app| {
            !app.comments.busy()
                && !app.loading
                && app.comments.loaded
                && app.comments.editor.is_none()
        });
        assert_eq!(app.comments.comments.items.len(), 1);
        assert_eq!(app.comments.comments.items[0].contents, "Root comment");
    }

    #[test]
    fn select_add_type_save_and_reread_comments_through_the_ui_worker() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("source.pdf");
        std::fs::write(&path, include_bytes!("../../assets/pagewing-guide.pdf")).unwrap();
        // The reader canonicalizes paths on open (e.g. /var -> /private/var on macOS).
        let path = path.canonicalize().unwrap();
        let context = Context::default();
        let mut app = Pagewing::with_context(
            &context,
            Some(path.clone()),
            Some(1),
            root.path().join("library.json"),
        );
        wait(&mut app, &context, |app| {
            app.comments.loaded && app.nav.count > 0
        });
        assert!(!app.comments.open, "comments must stay closed on open");
        frame(&mut app, &context, key(Key::A, Modifiers::COMMAND));
        wait(&mut app, &context, |app| !app.selection.rects.is_empty());
        // Exercise the same visible toolbar button a mouse user activates.
        let output = frame(&mut app, &context, Vec::new());
        let button = output
            .shapes
            .iter()
            .find_map(|shape| {
                if let egui::Shape::Text(text) = &shape.shape
                    && text.galley.text() == "Add comment"
                {
                    Some(text.pos + text.galley.size() * 0.5)
                } else {
                    None
                }
            })
            .expect("Add comment toolbar button");
        for pressed in [true, false] {
            frame(
                &mut app,
                &context,
                vec![
                    Event::PointerMoved(button),
                    Event::PointerButton {
                        pos: button,
                        button: egui::PointerButton::Primary,
                        pressed,
                        modifiers: Modifiers::NONE,
                    },
                ],
            );
        }
        assert!(app.comments.editor.is_some());
        frame(&mut app, &context, Vec::new());
        frame(
            &mut app,
            &context,
            vec![Event::Paste(
                "中文 comment\nj k / ? should stay in the comment".into(),
            )],
        );
        assert_eq!(
            app.comments.editor.as_ref().unwrap().contents,
            "中文 comment\nj k / ? should stay in the comment"
        );
        assert_eq!(app.nav.page, 1);
        let sidebar = app.library.sidebar;
        frame(&mut app, &context, key(Key::Tab, Modifiers::NONE));
        assert_eq!(
            app.library.sidebar, sidebar,
            "Tab must belong to the comment editor"
        );
        app.close_document(&context);
        assert_eq!(app.nav.count, 6, "unsaved comment must block closing");
        app.reload();
        assert!(!app.reloading, "unsaved comment must block reloading");
        frame(&mut app, &context, key(Key::Escape, Modifiers::NONE));
        assert!(!app.comments.open);
        assert!(app.comments.unsaved());
        frame(
            &mut app,
            &context,
            key(Key::M, Modifiers::COMMAND | Modifiers::SHIFT),
        );
        assert!(app.comments.open);
        frame(&mut app, &context, key(Key::Enter, Modifiers::COMMAND));
        wait(&mut app, &context, |app| {
            !app.comments.saving
                && app.comments.loaded
                && !app.loading
                && app.comments.editor.is_none()
        });
        assert_eq!(
            app.reading.path, path,
            "saving must keep the same document path"
        );
        assert_eq!(app.nav.page, 1, "saving preserves the current page");
        assert_eq!(app.comments.comments.items.len(), 1);
        assert!(
            app.comments.open,
            "saving must keep the comments sidebar visible"
        );
        app.edit_comment(0);
        assert_eq!(
            app.comments.editor.as_ref().unwrap().contents,
            "中文 comment\nj k / ? should stay in the comment"
        );
        assert!(!app.comments.unsaved());
        assert_ne!(
            std::fs::read(&path).unwrap(),
            include_bytes!("../../assets/pagewing-guide.pdf")
        );
        assert!(!root.path().join("source-annotated.pdf").exists());
        // Failed deletion must leave the selected comment available for retry.
        let snapshot = std::fs::read(&path).unwrap();
        std::fs::write(&path, b"changed by another application").unwrap();
        app.delete_comment();
        wait(&mut app, &context, |app| !app.comments.busy());
        assert!(
            app.error
                .as_ref()
                .is_some_and(|error| error.contains("changed on disk"))
        );
        assert!(app.comments.editor.is_some());
        assert_eq!(
            std::fs::read(&path).unwrap(),
            b"changed by another application"
        );
        std::fs::write(&path, snapshot).unwrap();
        let output = frame(&mut app, &context, Vec::new());
        let delete_button = output
            .shapes
            .iter()
            .find_map(|shape| {
                if let egui::Shape::Text(text) = &shape.shape
                    && text.galley.text() == "Delete comment & highlight"
                {
                    Some(text.pos + text.galley.size() * 0.5)
                } else {
                    None
                }
            })
            .expect("Delete comment button");
        for pressed in [true, false] {
            frame(
                &mut app,
                &context,
                vec![
                    Event::PointerMoved(delete_button),
                    Event::PointerButton {
                        pos: delete_button,
                        button: egui::PointerButton::Primary,
                        pressed,
                        modifiers: Modifiers::NONE,
                    },
                ],
            );
        }
        assert!(
            app.comments.saving,
            "the visible delete button must submit deletion"
        );
        wait(&mut app, &context, |app| {
            !app.comments.busy() && !app.loading && app.comments.loaded
        });
        assert!(app.comments.editor.is_none());
        assert!(app.comments.comments.items.is_empty());
        assert!(app.comments.open);
        assert_eq!(app.nav.page, 1, "deletion must preserve the current page");
        let saved = crate::document::load(&path, "").unwrap();
        assert!(crate::annotations::read(&saved, || false).items.is_empty());
    }

    #[test]
    fn right_sidebar_shows_full_comments_from_all_pages_and_clicking_reveals_the_page() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("comments.pdf");
        std::fs::write(&path, include_bytes!("../../assets/pagewing-guide.pdf")).unwrap();
        let long = format!("{}END OF FULL COMMENT", "A longer comment. ".repeat(9));
        for (page, contents) in [(0, long.clone()), (4, "Comment on another page".to_owned())] {
            let pdf = crate::document::load(&path, "").unwrap();
            crate::annotations::save(
                &pdf,
                &Comment {
                    page,
                    slot: None,
                    parent: None,
                    review: Default::default(),
                    marked: false,
                    states: Vec::new(),
                    kind: "Highlight".into(),
                    contents,
                    author: "Pagewing".into(),
                    rects: vec![hayro::kurbo::Rect::new(50.0, 80.0, 150.0, 100.0)],
                    has_appearance: false,
                    truncated: false,
                    locked: false,
                },
                &path,
            )
            .unwrap();
        }
        let context = Context::default();
        let mut app = Pagewing::with_context(
            &context,
            Some(path),
            Some(0),
            root.path().join("library.json"),
        );
        wait(&mut app, &context, |app| app.comments.loaded);
        assert!(
            !app.comments.open,
            "the right sidebar stays closed even when the PDF contains comments"
        );
        frame(
            &mut app,
            &context,
            key(Key::M, Modifiers::COMMAND | Modifiers::SHIFT),
        );
        assert!(app.comments.open, "the shortcut opens the comments sidebar");
        let output = frame(&mut app, &context, Vec::new());
        let texts: Vec<_> = output
            .shapes
            .iter()
            .filter_map(|shape| {
                if let egui::Shape::Text(text) = &shape.shape {
                    Some(text)
                } else {
                    None
                }
            })
            .collect();
        assert!(texts.iter().any(|text| text.galley.text() == "2 total"));
        assert!(
            texts.iter().any(|text| text.galley.text().contains(&long)),
            "the list must retain content past the old 100-character preview"
        );
        let other_page = texts
            .iter()
            .find(|text| text.galley.text().contains("Comment on another page"))
            .unwrap();
        let point = other_page.pos + Vec2::new(10.0, 8.0);
        assert!(
            point.x > 800.0,
            "the list must be on the right side of the canvas"
        );
        assert_eq!(app.nav.page, 0);
        // A permanently visible list must not capture document navigation keys.
        frame(&mut app, &context, key(Key::J, Modifiers::SHIFT));
        assert_eq!(app.nav.page, 1);
        for pressed in [true, false] {
            frame(
                &mut app,
                &context,
                vec![
                    Event::PointerMoved(point),
                    Event::PointerButton {
                        pos: point,
                        button: egui::PointerButton::Primary,
                        pressed,
                        modifiers: Modifiers::NONE,
                    },
                ],
            );
        }
        assert_eq!(app.nav.page, 4);
        assert_eq!(
            app.comments.editor.as_ref().unwrap().contents,
            "Comment on another page"
        );
        assert!(app.comments.open);
        // Make sure both sidebars leave a usable PDF area in small windows.
        app.comments.editor = None;
        app.comments.focus = false;
        app.library.sidebar = true;
        for width in [500.0, 240.0] {
            frame_size(&mut app, &context, Vec::new(), Vec2::new(width, 500.0));
            assert!(app.viewport.is_finite());
            assert!(
                app.viewport.x >= 60.0,
                "PDF viewport collapsed at width {width}: {:?}",
                app.viewport
            );
        }
    }
}
