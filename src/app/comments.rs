use super::{Pagewing, shortcuts};
use crate::annotations::{Comment, Comments, ReviewStatus};
use crate::comment_view::{CommentFilter, CommentView};
use eframe::egui::{self, Color32, Vec2};

fn preview_text(text: &str, limit: usize) -> String {
    let mut chars = text.chars();
    let mut result: String = chars
        .by_ref()
        .take(limit)
        .map(|c| if c.is_whitespace() { ' ' } else { c })
        .collect();
    if chars.next().is_some() {
        result.push('…');
    }
    result
}

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

// Match the inputs accepted by the multiline editor. This is deliberately
// conservative for edits such as undo or Delete that may leave text unchanged:
// let the widget apply them before allowing a document change.
fn edits_comment(event: &egui::Event) -> bool {
    use egui::{Event, ImeEvent, Key};
    match event {
        Event::Cut => true,
        Event::Paste(text) => !text.is_empty(),
        Event::Text(text) => !text.is_empty() && text != "\n" && text != "\r",
        Event::Ime(ImeEvent::Preedit(_) | ImeEvent::Commit(_)) => true,
        Event::Key {
            key,
            pressed: true,
            modifiers,
            ..
        } => {
            matches!(key, Key::Backspace | Key::Delete)
                || (*key == Key::Enter && modifiers.is_none())
                || (modifiers.command && matches!(key, Key::Z | Key::Y))
                || (modifiers.ctrl && matches!(key, Key::H | Key::K | Key::U | Key::W))
        }
        _ => false,
    }
}

#[derive(Default)]
struct CommentActions {
    save: bool,
    delete: bool,
    reply: bool,
    discard: bool,
}

#[derive(Default)]
pub(super) struct CommentUi {
    pub open: bool,
    pub loaded: bool,
    pub comments: Comments,
    pub view: CommentView,
    pub editor: Option<Comment>,
    original: String,
    original_review: ReviewStatus,
    original_marked: bool,
    pub focus: bool,
    pub saving: bool,
    pub save_requested: bool,
    // Only the pass before TextEdit handles events can contain pending edits.
    // The pass number also expires this state outside Context::run.
    input_pass: Option<u64>,
    pub filter_motions: Vec<isize>,
    pub filter_confirm: bool,
}

impl CommentUi {
    pub fn set_comments(&mut self, comments: Comments) {
        self.view.set_comments(&comments);
        self.comments = comments;
        self.loaded = true;
    }

    pub fn reset(&mut self, reloaded: bool) {
        let open = reloaded && self.open;
        let filter = if reloaded {
            std::mem::take(&mut self.view.filter)
        } else {
            CommentFilter::default()
        };
        *self = Self::default();
        self.open = open;
        self.view.filter = filter;
    }

    pub fn unsaved(&self) -> bool {
        self.editor.as_ref().is_some_and(|c| {
            c.slot.is_none()
                || c.contents != self.original
                || c.review != self.original_review
                || c.marked != self.original_marked
        })
    }
    pub fn busy(&self) -> bool {
        // Keyboard save is queued before the editor consumes this frame's
        // text. It already owns the document, even before the worker starts.
        self.saving || self.save_requested
    }
}

impl Pagewing {
    pub(super) fn comment_filter_focused(&self, context: &egui::Context) -> bool {
        self.comments.open
            && self.zoom_input.is_none()
            && !self.search_focus
            && self.page_input.is_none()
            && self.password_input.is_none()
            && !self.help
            && !self.properties_open
            && self.recent_picker.is_none()
            && context.memory(|memory| memory.has_focus(egui::Id::new("pagewing-comment-filter")))
    }

    pub(super) fn set_comments_open(&mut self, context: &egui::Context, open: bool) {
        self.comments.open = open;
        if !open {
            self.comments.filter_motions.clear();
            self.comments.filter_confirm = false;
        }
        // Consume final text before hiding the panel, just as for a queued save.
        let pending = self.pending_comment_input(context);
        self.comments.focus |= !open && pending;
        if !open && !self.comments.save_requested && !pending {
            self.comments.focus = false;
            context.memory_mut(|memory| {
                for name in ["pagewing-comment-input", "pagewing-comment-filter"] {
                    memory.surrender_focus(egui::Id::new(name));
                }
            });
        }
    }

    pub(super) fn begin_comment_input(&mut self, context: &egui::Context) {
        self.comments.input_pass = (self.comments.open
            && !self.comments.saving
            && self.comments.editor.as_ref().is_some_and(Comment::editable)
            && (self.comments.focus
                || context
                    .memory(|memory| memory.has_focus(egui::Id::new("pagewing-comment-input")))))
        .then(|| context.cumulative_pass_nr());
    }

    fn pending_comment_input(&self, context: &egui::Context) -> bool {
        self.comments.input_pass == Some(context.cumulative_pass_nr())
            && context.input(|input| input.events.iter().any(edits_comment))
    }

    pub(super) fn comment_change_blocked(&mut self, context: &egui::Context) -> bool {
        let pending = self.pending_comment_input(context);
        if self.comments.busy() || self.comments.unsaved() || pending {
            // A pointer action may have surrendered focus before the editor
            // draws. Give its unprocessed text back to the same draft.
            self.comments.focus |= pending;
            self.comments.open = true;
            self.notify("Save or discard the comment before changing documents.");
            true
        } else {
            false
        }
    }

    pub(super) fn add_comment(&mut self, context: &egui::Context) {
        if self.loading || self.comment_change_blocked(context) {
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

    fn edit_comment(&mut self, index: usize, context: &egui::Context) {
        if self.loading || self.comment_change_blocked(context) {
            return;
        }
        let comment = self.comments.comments.items[index].clone();
        self.comments.view.select(index);
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

    fn reply_comment(&mut self, context: &egui::Context) {
        if self.loading || self.comment_change_blocked(context) {
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
        self.input_buttons.close_comments = None;
        let save_requested = self.comments.save_requested;
        if (!self.comments.open && !save_requested && !self.pending_comment_input(context))
            || self.nav.count == 0
        {
            return;
        }
        let mut selected = None;
        let mut actions = CommentActions::default();
        let maximum = (context.available_rect().width() - 120.0).clamp(100.0, 480.0);
        egui::SidePanel::right("pagewing-comments-sidebar")
            .frame(super::appearance::panel(context, 14))
            .default_width(320.0_f32.min(maximum))
            .width_range(220.0_f32.min(maximum)..=maximum)
            .resizable(true)
            .show(context, |ui| {
                let panel_height = ui.available_height();
                egui::ScrollArea::vertical()
                    .id_salt("pagewing-comments-panel")
                    .show(ui, |ui| {
                        let content_top = ui.cursor().top();
                        ui.horizontal_wrapped(|ui| {
                            ui.heading("Comments");
                            ui.weak(format!("{} total", self.comments.comments.items.len()));
                            self.input_buttons.close_comments = Some(
                                ui.small_button("×")
                                    .on_hover_text(format!(
                                        "Hide comments · {}",
                                        self.library.shortcuts.label("comments")
                                    ))
                                    .id,
                            );
                        });
                        self.comment_filters(ui);
                        if ui
                            .add_enabled(
                                !self.comments.busy() && !self.selection.rects.is_empty(),
                                egui::Button::new("Add comment to selection").wrap(),
                            )
                            .clicked()
                        {
                            self.add_comment(context);
                        }
                        ui.separator();
                        if !self.comments.loaded {
                            ui.spinner();
                            ui.label("Reading PDF comments…");
                        } else if self.comments.comments.items.is_empty() {
                            ui.label("No existing comments.");
                        }
                        if self.comments.comments.limited {
                            ui.colored_label(
                                Color32::YELLOW,
                                "Showing a limited number of annotations.",
                            );
                        }
                        self.comments
                            .view
                            .update(&self.comments.comments, self.nav.page);
                        let filter_focused = self.comment_filter_focused(context);
                        let mut motions = std::mem::take(&mut self.comments.filter_motions);
                        for delta in motions.drain(..) {
                            if filter_focused {
                                self.comments.view.step(delta);
                            }
                        }
                        self.comments.filter_motions = motions;
                        let confirm =
                            std::mem::take(&mut self.comments.filter_confirm) && filter_focused;
                        if self.comments.loaded
                            && self.comments.view.filter != CommentFilter::default()
                        {
                            ui.weak(format!(
                                "{} matches · {} shown with thread context",
                                self.comments.view.matches(),
                                self.comments.view.rows().len()
                            ));
                            if self.comments.view.matches() == 0
                                && !self.comments.comments.items.is_empty()
                            {
                                ui.label("No matching comments. Try clearing the filters.");
                            }
                        }
                        let remaining =
                            (panel_height - (ui.cursor().top() - content_top)).max(40.0);
                        let list_height = if self.comments.editor.is_some() {
                            // Keep the editor's save/discard actions reachable when
                            // filters or status controls take more vertical space.
                            (remaining - 360.0).clamp(40.0, 160.0)
                        } else {
                            remaining
                        };
                        selected = self.comment_list(ui, list_height);
                        if confirm && !self.loading && !self.comments.busy() {
                            selected = self.comments.view.selected();
                        }
                        actions = self.comment_editor(ui);
                        // InputState retains text events after TextEdit handles
                        // them. Later actions must inspect the actual draft.
                        self.comments.input_pass = None;
                    });
            });
        let save_requested = std::mem::take(&mut self.comments.save_requested);
        if actions.discard {
            self.comments.editor = None;
        }
        if let Some(index) = selected {
            self.edit_comment(index, context);
        }
        // Keyboard confirmation is queued before widgets run. Save only after
        // the editor has applied text events from this same input batch.
        if actions.save || save_requested {
            self.save_comment();
        } else if actions.delete {
            self.delete_comment();
        } else if actions.reply {
            self.reply_comment(context);
        }
        if !self.comments.open {
            self.set_comments_open(context, false);
            context.request_repaint();
        }
    }

    fn comment_filters(&mut self, ui: &mut egui::Ui) {
        ui.add(
            egui::TextEdit::singleline(&mut self.comments.view.filter.query)
                .id(egui::Id::new("pagewing-comment-filter"))
                .desired_width(f32::INFINITY)
                .hint_text("Filter text or author…"),
        )
        .on_hover_text("Filter comments · ↑/↓ or Ctrl+J/K to select · Enter to open");
        ui.horizontal_wrapped(|ui| {
            egui::ComboBox::from_id_salt("pagewing-comment-review-filter")
                .selected_text(
                    self.comments
                        .view
                        .filter
                        .review
                        .map_or("All statuses", ReviewStatus::label),
                )
                .width(100.0)
                .show_ui(ui, |ui| {
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
                });
            ui.checkbox(&mut self.comments.view.filter.marked, "Marked only");
            ui.checkbox(&mut self.comments.view.filter.current_page, "Current page");
            if self.comments.view.filter != CommentFilter::default()
                && ui.small_button("Clear filters").clicked()
            {
                self.comments.view.filter = CommentFilter::default();
            }
        });
    }

    fn comment_list(&mut self, ui: &mut egui::Ui, list_height: f32) -> Option<usize> {
        let mut selected = None;
        let row_height = ui.text_style_height(&egui::TextStyle::Button) * 3.0
            + ui.spacing().button_padding.y * 2.0;
        let mut list = egui::ScrollArea::vertical()
            .id_salt("pagewing-comment-list")
            .auto_shrink([false, false])
            .max_height(list_height);
        if std::mem::take(&mut self.comments.view.reveal) {
            let row = self.comments.view.selected_row().unwrap_or(0);
            list = list
                .vertical_scroll_offset(row as f32 * (row_height + ui.spacing().item_spacing.y));
        }
        let keyboard_focus = self.comment_filter_focused(ui.ctx());
        list.show_rows(
            ui,
            row_height,
            self.comments.view.rows().len(),
            |ui, range| {
                for row in range {
                    let crate::comment_view::CommentRow {
                        index,
                        depth,
                        matched,
                    } = self.comments.view.rows()[row];
                    let comment = &self.comments.comments.items[index];
                    // Bound layout work even for very long annotations. The
                    // complete text stays available in the tooltip and editor.
                    let preview = preview_text(&comment.contents, 160);
                    let author = preview_text(&comment.author, 60);
                    let lines = [
                        format!(
                            "Page {}{}",
                            self.page_name(comment.page),
                            if author.is_empty() {
                                String::new()
                            } else {
                                format!(" · {author}")
                            }
                        ),
                        if preview.is_empty() {
                            "(no comment)".into()
                        } else {
                            preview
                        },
                        format!(
                            "{}{}{}{}",
                            if comment.parent.is_some() {
                                "Reply"
                            } else {
                                &comment.kind
                            },
                            if comment.review == ReviewStatus::None {
                                String::new()
                            } else {
                                format!(" · {}", comment.review.label())
                            },
                            if comment.marked { " · ✓ Marked" } else { "" },
                            if matched { "" } else { " · Thread context" },
                        ),
                    ];
                    let active = if keyboard_focus {
                        self.comments.view.selected() == Some(index)
                    } else {
                        self.comments.editor.as_ref().is_some_and(|current| {
                            current.page == comment.page && current.slot == comment.slot
                        })
                    };
                    ui.horizontal(|ui| {
                        ui.add_space(depth.min(6) as f32 * 14.0);
                        if ui
                            .add_enabled_ui(!self.loading && !self.comments.busy(), |ui| {
                                comment_card(ui, &lines, active, matched, row_height)
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
        selected
    }

    fn comment_editor(&mut self, ui: &mut egui::Ui) -> CommentActions {
        let mut actions = CommentActions::default();
        if self.comments.editor.is_some() {
            egui::ScrollArea::vertical()
                .id_salt("pagewing-comment-editor")
                .max_height(360.0)
                .show(ui, |ui| {
                    ui.separator();
                    let busy = self.comments.busy();
                    let saving = self.comments.saving;
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
                        // A queued save must still receive this batch's text;
                        // other editor actions already stay disabled via busy.
                        ui.add_enabled_ui(!saving && !read_only, |ui| {
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
                            actions.save = ui
                                .add_enabled(
                                    !busy && !comment.locked && (comment.editable() || comment.contents == self.comments.original),
                                    egui::Button::new("Save comment"),
                                )
                                .clicked();
                            if existing {
                                actions.reply = ui.add_enabled(!busy && deletable, egui::Button::new("Reply")).clicked();
                                actions.delete = ui.add_enabled(!busy && deletable,
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
                                actions.discard = true;
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
        actions
    }

    pub(super) fn comment_overlays(
        &mut self,
        ui: &mut egui::Ui,
        page: usize,
        page_rect: egui::Rect,
        dimensions: [f32; 2],
    ) {
        let mut selected = None;
        for &index in self.comments.view.overlays(page) {
            let comment = &self.comments.comments.items[index];
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
            self.edit_comment(index, ui.ctx());
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
            .rev()
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

    fn saved_comment_editor() -> (tempfile::TempDir, Context, Pagewing) {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("comments.pdf");
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
                contents: "Original comment".into(),
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
            None,
            root.path().join("library.json"),
        );
        wait(&mut app, &context, |app| app.comments.loaded);
        app.edit_comment(0, &context);
        for _ in 0..3 {
            frame(&mut app, &context, Vec::new());
        }
        assert!(!app.comments.unsaved());
        (root, context, app)
    }

    #[test]
    fn pending_editor_text_survives_document_shortcuts() {
        for (action_key, modifiers) in [
            (Key::W, Modifiers::COMMAND),
            (Key::R, Modifiers::COMMAND),
            (Key::T, Modifiers::COMMAND | Modifiers::SHIFT),
        ] {
            for (edits, expected) in [
                (vec![Event::Text("Final draft 📝".into())], "Final draft 📝"),
                (vec![Event::Paste("Pasted draft".into())], "Pasted draft"),
                (vec![Event::Paste("\n".into())], "\n"),
                (vec![Event::Cut], ""),
                (key(Key::Backspace, Modifiers::NONE), ""),
                (key(Key::Delete, Modifiers::NONE), ""),
                (key(Key::Enter, Modifiers::NONE), "\n"),
                (
                    vec![
                        Event::Ime(egui::ImeEvent::Enabled),
                        Event::Ime(egui::ImeEvent::Preedit("草稿".into())),
                        Event::Ime(egui::ImeEvent::Commit("草稿".into())),
                    ],
                    "草稿",
                ),
            ] {
                let (root, context, mut app) = saved_comment_editor();
                let path = app.reading.path.clone();
                let target = root.path().join("closed.pdf");
                app.last_closed = Some(target.clone());
                let mut events = key(Key::A, Modifiers::COMMAND);
                events.extend(edits);
                let mut action = key(action_key, modifiers);
                // Shortcut-generated text must be consumed without swallowing
                // the user's earlier draft in this same event batch.
                action.insert(1, Event::Text(format!("{action_key:?}")));
                events.extend(action);
                frame(&mut app, &context, events);
                assert_eq!(app.reading.path, path, "{action_key:?}: {expected:?}");
                assert!(!app.loading, "{action_key:?}");
                assert!(
                    app.comments.unsaved(),
                    "{action_key:?}: {expected:?}, {:?}",
                    app.comments.editor
                );
                assert_eq!(
                    app.comments.editor.as_ref().unwrap().contents,
                    expected,
                    "{action_key:?}"
                );
                assert_eq!(app.last_closed.as_ref(), Some(&target));
                // A previous input batch must not block a later clean close.
                app.comments.editor.as_mut().unwrap().contents = "Original comment".into();
                let mut close = key(Key::W, Modifiers::COMMAND);
                close.insert(1, Event::Text("w".into()));
                frame(&mut app, &context, close);
                assert_eq!(app.nav.count, 0);
                assert!(app.comments.editor.is_none());
            }
        }
    }

    #[test]
    fn pointer_transitions_preserve_pending_editor_input() {
        for action in ["open", "reload", "close", "hide"] {
            let (root, context, mut app) = saved_comment_editor();
            let path = app.reading.path.clone();
            let mut button = egui::Rect::NOTHING;
            let mut draw = |events| {
                let _ = context.run(
                    RawInput {
                        screen_rect: Some(egui::Rect::from_min_size(
                            egui::Pos2::ZERO,
                            Vec2::new(1180.0, 820.0),
                        )),
                        events,
                        ..Default::default()
                    },
                    |context| {
                        app.events(context);
                        egui::TopBottomPanel::top("transition").show(context, |ui| {
                            let response = ui.button(action);
                            button = response.rect;
                            if response.clicked() {
                                match action {
                                    "open" => app.open(root.path().join("another.pdf"), context),
                                    "reload" => app.reload(context),
                                    "close" => app.close_document(context),
                                    "hide" => app.set_comments_open(context, false),
                                    _ => unreachable!(),
                                }
                            }
                        });
                        app.comments_sidebar(context);
                        app.canvas(context);
                    },
                );
                button.center()
            };
            let mut pos = egui::Pos2::ZERO;
            for _ in 0..3 {
                pos = draw(Vec::new());
            }
            let mut events = key(Key::A, Modifiers::COMMAND);
            events.push(Event::Text("Pointer draft 📝".into()));
            events.push(Event::PointerMoved(pos));
            events.extend([true, false].map(|pressed| Event::PointerButton {
                pos,
                pressed,
                button: egui::PointerButton::Primary,
                modifiers: Modifiers::NONE,
            }));
            draw(events);
            assert_eq!(app.reading.path, path, "{action}");
            assert!(!app.loading, "{action}");
            assert_eq!(
                app.comments.editor.as_ref().unwrap().contents,
                "Pointer draft 📝",
                "{action}"
            );
            assert_eq!(app.comments.open, action != "hide");
        }
    }

    #[test]
    fn queued_save_protects_the_editor_and_final_input_until_the_worker_starts() {
        for action in [
            Some((Key::W, Modifiers::COMMAND)),
            Some((Key::R, Modifiers::COMMAND)),
            Some((Key::T, Modifiers::COMMAND | Modifiers::SHIFT)),
            None,
        ] {
            let (root, context, mut app) = saved_comment_editor();
            let path = app.reading.path.clone();
            let output = frame(&mut app, &context, Vec::new());
            let target = root.path().join("closed.pdf");
            app.last_closed = Some(target.clone());
            let expected = if action.is_some() {
                "Final comment 📝"
            } else {
                "Original comment"
            };
            let mut events = Vec::new();
            if action.is_some() {
                events.extend(key(Key::A, Modifiers::COMMAND));
                events.push(Event::Text(expected.into()));
            }
            events.extend(key(Key::Enter, Modifiers::COMMAND));
            if let Some((key, modifiers)) = action {
                events.extend(self::key(key, modifiers));
            } else {
                let pos = output
                    .shapes
                    .iter()
                    .find_map(|shape| match &shape.shape {
                        egui::Shape::Text(text)
                            if text.galley.text() == "Discard / close editor" =>
                        {
                            Some(text.pos + text.galley.size() * 0.5)
                        }
                        _ => None,
                    })
                    .expect("visible discard action");
                events.push(Event::PointerMoved(pos));
                events.extend([true, false].map(|pressed| Event::PointerButton {
                    pos,
                    pressed,
                    button: egui::PointerButton::Primary,
                    modifiers: Modifiers::NONE,
                }));
            }
            frame(&mut app, &context, events);
            assert_eq!(app.reading.path, path, "{action:?}");
            assert!(!app.loading, "{action:?}");
            assert!(app.comments.saving, "{action:?}");
            assert_eq!(
                app.comments.editor.as_ref().unwrap().contents,
                expected,
                "{action:?}"
            );
            assert_eq!(app.last_closed.as_ref(), Some(&target));
            wait(&mut app, &context, |app| {
                !app.comments.saving && !app.loading && app.comments.loaded
            });
            assert_eq!(app.comments.comments.items[0].contents, expected);
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
        app.edit_comment(0, &context);
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
            app.edit_comment(depth, &context);
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
        app.reload(&context);
        assert!(!app.reloading, "status drafts must defer reloads");
        app.edit_comment(0, &context);
        assert_eq!(
            app.comments.editor.as_ref().unwrap().slot,
            app.comments.comments.items[2].slot,
            "changing selection must preserve the draft"
        );
        click_text(&mut app, &context, "Marked (Acrobat checkmark)");
        assert!(app.comments.editor.as_ref().unwrap().marked);
        app.comments.view.filter.query = "Nested".into();
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
        assert_eq!(
            app.comments.view.filter.query, "Nested",
            "saving must retain the active filters"
        );
        assert_eq!(app.comments.comments.items.len(), 3);
        app.edit_comment(1, &context);
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
        app.reload(&context);
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
        app.comments.focus = true;
        let mut events = vec![Event::Text(" last 📝".into())];
        events.extend(key(Key::Enter, Modifiers::COMMAND));
        events.extend(key(Key::M, Modifiers::COMMAND | Modifiers::SHIFT));
        events.extend(key(Key::J, Modifiers::SHIFT));
        frame(&mut app, &context, events);
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
        assert_eq!(
            app.nav.page, 2,
            "saving preserves navigation after the save request"
        );
        assert_eq!(app.comments.comments.items.len(), 1);
        assert!(
            !app.comments.open,
            "a later hide request must keep the comments sidebar closed"
        );
        app.edit_comment(0, &context);
        assert_eq!(
            app.comments.editor.as_ref().unwrap().contents,
            "中文 comment\nj k / ? should stay in the comment last 📝"
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
                .is_some_and(|error| error.message.contains("changed on disk"))
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
    fn right_sidebar_previews_comments_and_keeps_full_text_in_the_editor() {
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
        let first = texts
            .iter()
            .find(|text| text.galley.text().contains("A longer comment."))
            .unwrap();
        assert_eq!(
            first.galley.text(),
            preview_text(&long, 160),
            "the summary must be a separately painted line, not truncated with its header"
        );
        assert!(
            !first.galley.text().contains("END OF FULL COMMENT"),
            "cards keep long comments compact"
        );
        let first_point = first.pos + Vec2::new(10.0, 8.0);
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
        app.comments.editor = None;
        for pressed in [true, false] {
            frame(
                &mut app,
                &context,
                vec![
                    Event::PointerMoved(first_point),
                    Event::PointerButton {
                        pos: first_point,
                        button: egui::PointerButton::Primary,
                        pressed,
                        modifiers: Modifiers::NONE,
                    },
                ],
            );
        }
        assert_eq!(
            app.comments.editor.as_ref().unwrap().contents,
            long,
            "opening a compact card retains the entire annotation"
        );
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

    #[test]
    fn comment_filter_input_isolated_from_reader_keys_and_preserves_drafts() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("filters.pdf");
        std::fs::write(&path, include_bytes!("../../assets/pagewing-guide.pdf")).unwrap();
        let context = Context::default();
        let mut app = Pagewing::with_context(
            &context,
            Some(path),
            Some(0),
            root.path().join("library.json"),
        );
        wait(&mut app, &context, |app| app.comments.loaded);
        let comment = Comment {
            page: 0,
            slot: Some(0),
            parent: None,
            review: ReviewStatus::None,
            marked: false,
            states: Vec::new(),
            kind: "Highlight".into(),
            contents: "jkg draft".into(),
            author: "Straße".into(),
            rects: Vec::new(),
            has_appearance: false,
            truncated: false,
            locked: false,
        };
        app.comments.set_comments(Comments {
            items: vec![comment],
            limited: false,
        });
        app.comments.open = true;
        click_text(&mut app, &context, "Filter text or author…");
        let mut events = key(Key::J, Modifiers::NONE);
        events.extend(key(Key::K, Modifiers::NONE));
        events.extend(key(Key::G, Modifiers::NONE));
        events.push(Event::Text("jkg STRASSE".into()));
        frame(&mut app, &context, events);
        assert_eq!(app.nav.page, 0);
        assert_eq!(app.reading.rotation, 0);
        assert_eq!(app.comments.view.matches(), 1);
        assert_eq!(app.comments.view.filter.query, "jkg STRASSE");
        app.edit_comment(0, &context);
        app.comments.editor.as_mut().unwrap().contents = "unsaved draft".into();
        click_text(&mut app, &context, "Current page");
        click_text(&mut app, &context, "Marked only");
        assert_eq!(app.comments.view.matches(), 0);
        assert!(app.comments.unsaved());
        click_text(&mut app, &context, "Clear filters");
        assert_eq!(app.comments.view.matches(), 1);
        assert_eq!(
            app.comments.editor.as_ref().unwrap().contents,
            "unsaved draft"
        );
        frame(&mut app, &context, key(Key::Escape, Modifiers::NONE));
        assert!(!app.comments.open);
        assert!(
            app.comments.unsaved(),
            "hiding filtered comments preserves the editor"
        );
    }

    #[test]
    fn hiding_comments_releases_filter_focus_before_later_keys_in_the_same_frame() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("focus.pdf");
        std::fs::write(&path, include_bytes!("../../assets/pagewing-guide.pdf")).unwrap();
        let context = Context::default();
        let mut app = Pagewing::with_context(
            &context,
            Some(path),
            Some(0),
            root.path().join("library.json"),
        );
        wait(&mut app, &context, |app| app.comments.loaded);
        frame(
            &mut app,
            &context,
            key(Key::M, Modifiers::COMMAND | Modifiers::SHIFT),
        );
        click_text(&mut app, &context, "Filter text or author…");
        frame(&mut app, &context, vec![Event::Text("draft filter".into())]);
        let mut events = key(Key::M, Modifiers::COMMAND | Modifiers::SHIFT);
        events.extend(key(Key::J, Modifiers::SHIFT));
        frame(&mut app, &context, events);
        assert!(!app.comments.open);
        assert_eq!(
            app.nav.page, 1,
            "closing a filter must route the following key to the reader"
        );
        assert_eq!(app.comments.view.filter.query, "draft filter");
        for page in 2..=3 {
            frame(
                &mut app,
                &context,
                key(Key::M, Modifiers::COMMAND | Modifiers::SHIFT),
            );
            context.memory_mut(|memory| {
                memory.request_focus(egui::Id::new("pagewing-comment-filter"))
            });
            frame(&mut app, &context, Vec::new());
            let id = if page == 2 {
                app.input_buttons.comments
            } else {
                app.input_buttons.close_comments
            }
            .unwrap();
            let point = context.read_response(id).unwrap().rect.center();
            let mut events = vec![Event::PointerMoved(point)];
            events.extend([true, false].map(|pressed| Event::PointerButton {
                pos: point,
                button: egui::PointerButton::Primary,
                pressed,
                modifiers: Modifiers::NONE,
            }));
            events.extend(key(Key::J, Modifiers::SHIFT));
            frame(&mut app, &context, events);
            assert!(
                !app.comments.open,
                "both mouse buttons must close before routing keys"
            );
            assert_eq!(app.nav.page, page);
            assert_eq!(app.comments.view.filter.query, "draft filter");
        }
    }

    #[test]
    fn comment_filter_keyboard_confirms_new_query_and_protects_editor_drafts() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("keyboard.pdf");
        std::fs::write(&path, include_bytes!("../../assets/pagewing-guide.pdf")).unwrap();
        let context = Context::default();
        let mut app = Pagewing::with_context(
            &context,
            Some(path),
            Some(0),
            root.path().join("library.json"),
        );
        wait(&mut app, &context, |app| app.comments.loaded);
        let parent = Comment {
            page: 0,
            slot: Some(0),
            parent: None,
            review: ReviewStatus::None,
            marked: false,
            states: Vec::new(),
            kind: "Text".into(),
            contents: "Parent context".into(),
            author: "Reviewer".into(),
            rects: Vec::new(),
            has_appearance: false,
            truncated: false,
            locked: false,
        };
        let mut other = parent.clone();
        other.page = 2;
        other.contents = "Another needle".into();
        let mut reply = parent.reply().unwrap();
        reply.page = 4;
        reply.slot = Some(0);
        reply.contents = "Nested needle".into();
        app.comments.set_comments(Comments {
            items: vec![parent, other, reply],
            limited: false,
        });
        app.comments.open = true;
        click_text(&mut app, &context, "Filter text or author…");
        let mut events = vec![Event::Text("needle".into())];
        events.extend(key(Key::Enter, Modifiers::NONE));
        frame(&mut app, &context, events);
        assert_eq!(
            app.nav.page, 4,
            "Enter follows the new query's first match, not its ancestor"
        );
        assert_eq!(
            app.comments.editor.as_ref().unwrap().contents,
            "Nested needle"
        );
        frame(&mut app, &context, Vec::new());
        app.comments.editor.as_mut().unwrap().contents = "Unsaved draft".into();
        context.memory_mut(|memory| memory.request_focus(egui::Id::new("pagewing-comment-filter")));
        let mut events = key(Key::J, Modifiers::CTRL);
        events.extend(key(Key::Enter, Modifiers::NONE));
        frame(&mut app, &context, events);
        assert_eq!(app.comments.view.selected(), Some(1));
        assert_eq!(
            app.comments.editor.as_ref().unwrap().contents,
            "Unsaved draft"
        );
        assert_eq!(app.nav.page, 4, "keyboard selection cannot discard a draft");
        app.comments.editor = None;
        frame(&mut app, &context, key(Key::Enter, Modifiers::NONE));
        assert_eq!(app.nav.page, 2);
        assert_eq!(
            app.comments.editor.as_ref().unwrap().contents,
            "Another needle"
        );
        frame(&mut app, &context, Vec::new());
        context.memory_mut(|memory| memory.request_focus(egui::Id::new("pagewing-comment-filter")));
        app.comments.view.filter.query = "missing".into();
        let mut events = key(Key::ArrowDown, Modifiers::NONE);
        events.extend(key(Key::Enter, Modifiers::NONE));
        frame(&mut app, &context, events);
        assert_eq!(app.nav.page, 2);
        assert_eq!(app.comments.view.selected(), None);
        // A later global Find action must cancel a queued comment confirmation.
        app.comments.view.filter.query = "needle".into();
        let mut events = key(Key::Enter, Modifiers::NONE);
        events.extend(key(Key::F, Modifiers::COMMAND));
        frame(&mut app, &context, events);
        assert!(app.search_open);
        assert_eq!(app.nav.page, 2);
        // A match beneath many retained ancestors must actually be painted in
        // the virtual viewport, rather than merely selected outside it.
        app.search_open = false;
        app.search_focus = false;
        app.comments.editor = None;
        let template = app.comments.comments.items[0].clone();
        let items = (0..128)
            .map(|slot| {
                let mut comment = template.clone();
                comment.slot = Some(slot);
                comment.parent = (slot > 0).then(|| crate::annotations::CommentId {
                    page: 0,
                    slot: slot - 1,
                });
                comment.contents = if slot == 127 {
                    "Deepest needle"
                } else {
                    "Ancestor"
                }
                .into();
                comment
            })
            .collect();
        app.comments.set_comments(Comments {
            items,
            limited: false,
        });
        app.comments.view.filter.query = "deepest".into();
        context.memory_mut(|memory| memory.request_focus(egui::Id::new("pagewing-comment-filter")));
        frame(&mut app, &context, Vec::new());
        let output = frame(&mut app, &context, Vec::new());
        assert!(output.shapes.iter().any(|shape| matches!(&shape.shape, egui::Shape::Text(text) if text.galley.text() == "Deepest needle")));
        frame(&mut app, &context, key(Key::Enter, Modifiers::NONE));
        assert_eq!(app.comments.editor.as_ref().unwrap().slot, Some(127));
    }
}
