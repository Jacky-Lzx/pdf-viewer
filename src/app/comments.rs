use super::{Pagewing, shortcuts, tooltips::LazyTooltip};
use crate::annotations::{Comment, Comments, ReviewStatus};
use crate::comment_cards::CardTextCache;
use crate::comment_view::{CommentFilter, CommentView};
use eframe::egui::{self, Color32, Vec2};

mod editor;
mod export;
mod list;

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
    cards: CardTextCache,
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
    pub filter_motions: Vec<crate::list_navigation::Motion>,
    pub filter_confirm: bool,
    region: Option<(egui::LayerId, egui::Rect)>,
}

impl CommentUi {
    pub fn set_comments(&mut self, comments: Comments) {
        self.view.set_comments(&comments);
        self.cards.clear();
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
    pub(super) fn comment_control_focused(&self, context: &egui::Context) -> bool {
        self.comments.open && shortcuts::focused_in_region(context, self.comments.region)
    }

    fn copy_shown_comments(&mut self, context: &egui::Context) {
        let report = export::render(&self.comments.comments, self.comments.view.rows(), |page| {
            self.labels.label(page)
        });
        if report.copied == 0 {
            self.notify("No comments copied.");
            return;
        }
        context.copy_text(report.text);
        self.notify(if report.copied == report.total {
            format!("Copied {} shown comments", report.copied)
        } else {
            format!(
                "Copied {} of {} shown comments (clipboard size limit)",
                report.copied, report.total
            )
        });
    }

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
        self.comments.region = None;
        let save_requested = self.comments.save_requested;
        if (!self.comments.open && !save_requested && !self.pending_comment_input(context))
            || self.nav.count == 0
        {
            return;
        }
        let mut selected = None;
        let mut copy = false;
        let mut actions = CommentActions::default();
        let maximum = (context.available_rect().width() - 120.0).clamp(100.0, 480.0);
        let panel = egui::SidePanel::right("pagewing-comments-sidebar")
            .frame(super::appearance::panel(context, 14))
            .default_width(320.0_f32.min(maximum))
            .width_range(220.0_f32.min(maximum)..=maximum)
            .resizable(true)
            .show(context, |ui| {
                ui.horizontal_wrapped(|ui| {
                    if ui.available_width() < 220.0 {
                        ui.spacing_mut().item_spacing.x = 4.0;
                        if ui.available_width() < 120.0 {
                            // Keep the title and both header actions on one
                            // line so the editor footer retains its height.
                            ui.spacing_mut().button_padding.x = 2.0;
                            ui.label(egui::RichText::new("Notes").small().strong());
                        } else {
                            ui.strong("Comments");
                        }
                    } else {
                        ui.heading("Comments");
                        ui.weak(format!("{} total", self.comments.comments.items.len()));
                    }
                    self.input_buttons.close_comments = Some(
                        ui.small_button("×")
                            .on_hover_text_lazy(|| {
                                format!(
                                    "Hide comments · {}",
                                    self.library.shortcuts.label("comments")
                                )
                            })
                            .id,
                    );
                    copy = ui
                        .add_enabled(
                            self.comments.loaded
                                && !self.loading
                                && !self.comments.busy()
                                && !self.comments.view.rows().is_empty(),
                            egui::Button::new("Copy").small(),
                        )
                        .on_hover_text("Copy saved comments shown in this list, including thread context. Unsaved editor changes are not included.")
                        .clicked();
                });
                if self.comments.editor.is_some() {
                    let compact = ui.available_width() < 220.0 || ui.available_height() < 220.0;
                    egui::TopBottomPanel::bottom("pagewing-comment-actions")
                        .frame(egui::Frame::NONE)
                        .resizable(false)
                        .default_height(100.0)
                        .show_separator_line(false)
                        .show_inside(ui, |ui| {
                            let allocated = ui.available_height();
                            actions = self.comment_editor_actions(ui, compact);
                            if (ui.min_size().y - allocated).abs() > 1.0 {
                                // Reflow wrapped actions in the same frame so
                                // opening an editor or resizing cannot expose
                                // buttons at obsolete interaction coordinates.
                                context.request_discard("comment action footer reflowed");
                            }
                        });
                }
                let panel_height = ui.available_height();
                egui::ScrollArea::vertical()
                    .id_salt("pagewing-comments-panel")
                    .min_scrolled_height(0.0)
                    .max_height(panel_height.max(0.0))
                    .show(ui, |ui| {
                        let content_top = ui.cursor().top();
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
                            // Leave room for the editor; its actions are pinned
                            // below this scroll area even for a very long draft.
                            (remaining * 0.3).clamp(40.0, 160.0)
                        } else {
                            remaining
                        };
                        selected = self.comment_list(ui, list_height);
                        let confirm = !ui.is_sizing_pass()
                            && std::mem::take(&mut self.comments.filter_confirm)
                            && self.comment_filter_focused(context);
                        if confirm && !self.loading && !self.comments.busy() {
                            selected = self.comments.view.selected();
                        }
                        if self.comment_editor_fields(ui) {
                            context.request_repaint();
                        }
                        // InputState retains text events after TextEdit handles
                        // them. Later actions must inspect the actual draft.
                        self.comments.input_pass = None;
                    });
            });
        self.comments.region = Some((panel.response.layer_id, panel.response.rect));
        let save_requested = std::mem::take(&mut self.comments.save_requested);
        if copy {
            self.copy_shown_comments(context);
        }
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
mod tests;
