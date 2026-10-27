use super::{Pagewing, shortcuts};
use crate::annotations::{Comment, Comments, ReviewStatus};
use crate::comment_cards::{CardTextCache, card_lines};
use crate::comment_view::{CommentFilter, CommentView};
use eframe::egui::{self, Color32, Vec2};

mod editor;

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
    pub filter_motions: Vec<isize>,
    pub filter_confirm: bool,
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
                ui.horizontal_wrapped(|ui| {
                    if ui.available_width() < 220.0 {
                        ui.spacing_mut().item_spacing.x = 4.0;
                        if ui.available_width() < 120.0 {
                            ui.label(egui::RichText::new("Comments").small().strong());
                        } else {
                            ui.strong("Comments");
                        }
                    } else {
                        ui.heading("Comments");
                        ui.weak(format!("{} total", self.comments.comments.items.len()));
                    }
                    self.input_buttons.close_comments = Some(
                        ui.small_button("×")
                            .on_hover_text(format!(
                                "Hide comments · {}",
                                self.library.shortcuts.label("comments")
                            ))
                            .id,
                    );
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
                            // Leave room for the editor; its actions are pinned
                            // below this scroll area even for a very long draft.
                            (remaining * 0.3).clamp(40.0, 160.0)
                        } else {
                            remaining
                        };
                        selected = self.comment_list(ui, list_height);
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
        let layout_id = ui.make_persistent_id("pagewing-comment-list-layout");
        let layout = (ui.available_width(), list_height, row_height);
        let resized = ui.data_mut(|data| {
            let previous = data.get_temp::<(f32, f32, f32)>(layout_id);
            data.insert_temp(layout_id, layout);
            previous != Some(layout)
        });
        let reveal = std::mem::take(&mut self.comments.view.reveal);
        let keyboard_focus = self.comment_filter_focused(ui.ctx());
        let target = if reveal || (resized && keyboard_focus) {
            self.comments.view.selected_row()
        } else if resized {
            self.comments.editor.as_ref().and_then(|editor| {
                self.comments.view.rows().iter().position(|row| {
                    let comment = &self.comments.comments.items[row.index];
                    editor.page == comment.page && editor.slot == comment.slot
                })
            })
        } else {
            None
        };
        let list = egui::ScrollArea::vertical()
            .id_salt("pagewing-comment-list")
            .auto_shrink([false, false])
            .animated(false)
            .max_height(list_height);
        let busy = self.comments.busy();
        list.show_rows(
            ui,
            row_height,
            self.comments.view.rows().len(),
            |ui, range| {
                if let Some(row) = target {
                    // show_rows only builds visible cards. Compute the target
                    // from its virtual row so distant selections can be shown
                    // without laying out the intervening comments.
                    let top = ui.max_rect().top()
                        + (row as f32 - range.start as f32)
                            * (row_height + ui.spacing().item_spacing.y);
                    let rect = egui::Rect::from_min_size(
                        egui::pos2(ui.max_rect().left(), top),
                        egui::vec2(ui.available_width(), row_height),
                    );
                    ui.scroll_to_rect(rect, None);
                }
                for row in range {
                    let crate::comment_view::CommentRow {
                        index,
                        depth,
                        matched,
                    } = self.comments.view.rows()[row];
                    let comment = &self.comments.comments.items[index];
                    let active = if keyboard_focus {
                        self.comments.view.selected() == Some(index)
                    } else {
                        self.comments.editor.as_ref().is_some_and(|current| {
                            current.page == comment.page && current.slot == comment.slot
                        })
                    };
                    let lines = self.comments.cards.get_or_insert_with(index, matched, || {
                        card_lines(comment, self.labels.display_name(comment.page), matched)
                    });
                    ui.horizontal(|ui| {
                        ui.add_space(depth.min(6) as f32 * 14.0);
                        if ui
                            .add_enabled_ui(!self.loading && !busy, |ui| {
                                comment_card(ui, lines, active, matched, row_height)
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
                app.status(context);
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
        click_text_size(app, context, label, Vec2::new(1180.0, 820.0));
    }

    fn click_text_size(app: &mut Pagewing, context: &Context, label: &str, size: Vec2) {
        let output = frame_size(app, context, Vec::new(), size);
        // Prefer footer actions over matching card previews. An open menu
        // paints above both, so choose its last matching label instead.
        let mut points = output.shapes.iter().filter_map(|shape| {
            if let egui::Shape::Text(text) = &shape.shape
                && text.galley.text() == label
            {
                Some(text.pos + text.galley.size() * 0.5)
            } else {
                None
            }
        });
        let point = if egui::Popup::is_any_open(context) {
            points.next_back()
        } else {
            points.next()
        }
        .unwrap_or_else(|| panic!("Missing visible button: {label}"));
        for pressed in [true, false] {
            frame_size(
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
                size,
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

    fn many_comments() -> (tempfile::TempDir, Context, Pagewing) {
        let (root, context, mut app) = saved_comment_editor();
        let template = app.comments.comments.items[0].clone();
        app.comments.editor = None;
        app.comments.focus = false;
        app.comments.set_comments(Comments {
            items: (0..80)
                .map(|index| Comment {
                    slot: Some(index),
                    contents: format!("Row {index}"),
                    ..template.clone()
                })
                .collect(),
            limited: false,
        });
        (root, context, app)
    }

    fn visible_selected_card(
        context: &Context,
        output: &egui::FullOutput,
        body: &str,
    ) -> Option<egui::Rect> {
        let text_rect = output.shapes.iter().find_map(|shape| match &shape.shape {
            egui::Shape::Text(text) if text.galley.text() == body => {
                Some(egui::Rect::from_min_size(text.pos, text.galley.size()))
            }
            _ => None,
        })?;
        let fill = context.style().visuals.selection.bg_fill;
        output.shapes.iter().find_map(|shape| match &shape.shape {
            egui::Shape::Rect(rect)
                if rect.fill == fill
                    && rect.rect.height() > text_rect.height() * 2.0
                    && rect.rect.contains_rect(text_rect)
                    && shape.clip_rect.contains_rect(rect.rect) =>
            {
                Some(rect.rect)
            }
            _ => None,
        })
    }

    #[test]
    fn resizing_keeps_the_selected_comment_visible_without_undoing_manual_scroll() {
        let (_root, context, mut app) = many_comments();
        context.memory_mut(|memory| {
            memory.request_focus(egui::Id::new("pagewing-comment-filter"));
        });
        frame(&mut app, &context, vec![Event::Paste("Row".into())]);
        app.comments.view.step(79);
        let mut output = egui::FullOutput::default();
        let size = Vec2::new(500.0, 400.0);
        for size in [Vec2::new(1180.0, 820.0), size] {
            for _ in 0..4 {
                output = frame_size(&mut app, &context, Vec::new(), size);
            }
            assert_eq!(app.comments.view.selected(), Some(79));
            assert_eq!(app.comments.view.filter.query, "Row");
            assert!(app.comment_filter_focused(&context));
            assert!(
                visible_selected_card(&context, &output, "Row 79").is_some(),
                "selected comment must remain visible at {size:?}"
            );
        }
        let pos = visible_selected_card(&context, &output, "Row 79")
            .unwrap()
            .center();
        frame_size(
            &mut app,
            &context,
            vec![
                Event::PointerMoved(pos),
                Event::MouseWheel {
                    unit: egui::MouseWheelUnit::Point,
                    delta: Vec2::new(0.0, 4000.0),
                    modifiers: Modifiers::NONE,
                },
            ],
            size,
        );
        for _ in 0..30 {
            output = frame_size(&mut app, &context, Vec::new(), size);
        }
        assert!(
            visible_selected_card(&context, &output, "Row 79").is_none(),
            "manual scrolling must not be undone by the retained selection"
        );
        frame_size(&mut app, &context, key(Key::K, Modifiers::CTRL), size);
        for _ in 0..4 {
            output = frame_size(&mut app, &context, Vec::new(), size);
        }
        assert_eq!(app.comments.view.selected(), Some(78));
        let previous = visible_selected_card(&context, &output, "Row 78").unwrap();
        frame_size(&mut app, &context, key(Key::ArrowUp, Modifiers::NONE), size);
        for _ in 0..4 {
            output = frame_size(&mut app, &context, Vec::new(), size);
        }
        assert_eq!(app.comments.view.selected(), Some(77));
        let current = visible_selected_card(&context, &output, "Row 77").unwrap();
        assert!(
            current.top() < previous.top(),
            "a row already in view must not be moved to the top on each keypress"
        );
        assert_eq!(app.comments.view.filter.query, "Row");
        assert!(app.comment_filter_focused(&context));
    }

    #[test]
    fn opening_and_resizing_the_editor_reveals_its_comment_card() {
        let (_root, context, mut app) = many_comments();
        for _ in 0..3 {
            frame(&mut app, &context, Vec::new());
        }
        // Opening a page overlay may choose a card outside the virtual rows.
        app.edit_comment(79, &context);
        for size in [Vec2::new(1180.0, 820.0), Vec2::new(640.0, 480.0)] {
            let mut output = egui::FullOutput::default();
            for _ in 0..4 {
                output = frame_size(&mut app, &context, Vec::new(), size);
            }
            assert!(
                visible_selected_card(&context, &output, "Row 79").is_some(),
                "the editor's active card must be fully visible at {size:?}"
            );
            assert_eq!(app.comments.editor.as_ref().unwrap().contents, "Row 79");
            assert!(
                context
                    .memory(|memory| { memory.has_focus(egui::Id::new("pagewing-comment-input")) })
            );
        }
        context.memory_mut(|memory| {
            memory.request_focus(egui::Id::new("pagewing-comment-filter"));
        });
        frame_size(
            &mut app,
            &context,
            vec![Event::Paste("Row".into())],
            Vec2::new(640.0, 480.0),
        );
        assert_eq!(app.comments.view.selected(), Some(0));
        context.memory_mut(|memory| {
            memory.request_focus(egui::Id::new("pagewing-comment-input"));
        });
        let mut output = egui::FullOutput::default();
        for _ in 0..4 {
            output = frame(&mut app, &context, Vec::new());
        }
        assert!(
            visible_selected_card(&context, &output, "Row 79").is_some(),
            "with editor focus, resize reveals the active draft rather than the filter's first match"
        );
        assert_eq!(app.comments.view.selected(), Some(0));
        assert_eq!(app.comments.editor.as_ref().unwrap().contents, "Row 79");
    }

    #[test]
    fn editor_actions_stay_visible_with_long_drafts_in_short_windows() {
        let (_root, context, mut app) = saved_comment_editor();
        app.comments.editor.as_mut().unwrap().contents = "Long draft line\n".repeat(100);
        let expected = app.comments.editor.as_ref().unwrap().contents.clone();
        let mut save_pos = egui::Pos2::ZERO;
        for size in [
            Vec2::new(640.0, 480.0),
            Vec2::new(500.0, 360.0),
            Vec2::new(240.0, 240.0),
        ] {
            let mut output = egui::FullOutput::default();
            for _ in 0..3 {
                output = frame_size(&mut app, &context, Vec::new(), size);
            }
            assert!(
                app.viewport.x >= 60.0,
                "editor collapsed the PDF viewport at {size:?}: {:?}",
                app.viewport
            );
            let compact = output.shapes.iter().any(|shape| {
                matches!(&shape.shape,
                egui::Shape::Text(text) if text.galley.text() == "Save")
            });
            save_pos = output
                .shapes
                .iter()
                .find_map(|shape| match &shape.shape {
                    egui::Shape::Text(text)
                        if text.galley.text() == if compact { "Save" } else { "Save comment" } =>
                    {
                        Some(text.pos + text.galley.size() * 0.5)
                    }
                    _ => None,
                })
                .expect("visible save action");
            for label in if compact {
                ["Save", "Discard"]
            } else {
                ["Save comment", "Discard / close editor"]
            } {
                assert!(
                    output.shapes.iter().any(|shape| {
                        if let egui::Shape::Text(text) = &shape.shape {
                            let rect = egui::Rect::from_min_size(text.pos, text.galley.size());
                            text.galley.text() == label
                                && shape.clip_rect.contains_rect(rect)
                                && egui::Rect::from_min_size(egui::Pos2::ZERO, size)
                                    .contains_rect(rect)
                        } else {
                            false
                        }
                    }),
                    "{label} must remain visible at {size:?}"
                );
            }
        }
        for pressed in [true, false] {
            frame_size(
                &mut app,
                &context,
                vec![
                    Event::PointerMoved(save_pos),
                    Event::PointerButton {
                        pos: save_pos,
                        pressed,
                        button: egui::PointerButton::Primary,
                        modifiers: Modifiers::NONE,
                    },
                ],
                Vec2::new(240.0, 240.0),
            );
        }
        // A fast worker may already have saved and started reloading between
        // pointer frames. Verify the resulting PDF, not that transient state.
        wait(&mut app, &context, |app| {
            !app.comments.saving && !app.loading && app.comments.loaded
        });
        assert_eq!(app.comments.comments.items[0].contents, expected);
    }

    #[test]
    fn compact_editor_menu_can_reply_and_delete_after_resizing() {
        let (_root, context, mut app) = saved_comment_editor();
        let size = Vec2::new(240.0, 240.0);
        for _ in 0..3 {
            frame_size(&mut app, &context, Vec::new(), size);
        }
        click_text_size(&mut app, &context, "More…", size);
        click_text_size(&mut app, &context, "Reply", size);
        let editor = app.comments.editor.as_ref().unwrap();
        assert!(editor.slot.is_none() && editor.parent.is_some());
        frame_size(
            &mut app,
            &context,
            vec![Event::Text("Compact reply".into())],
            size,
        );
        click_text_size(&mut app, &context, "Save", size);
        wait(&mut app, &context, |app| {
            !app.comments.busy() && !app.loading && app.comments.loaded
        });
        assert_eq!(app.comments.comments.items[1].contents, "Compact reply");
        app.edit_comment(1, &context);
        for _ in 0..3 {
            frame_size(&mut app, &context, Vec::new(), size);
        }
        click_text_size(&mut app, &context, "More…", size);
        click_text_size(&mut app, &context, "Delete reply & replies", size);
        wait(&mut app, &context, |app| {
            !app.comments.busy() && !app.loading && app.comments.loaded
        });
        assert_eq!(app.comments.comments.items.len(), 1);
        assert_eq!(app.comments.comments.items[0].contents, "Original comment");
    }

    #[test]
    fn reloaded_comment_cards_replace_text_author_status_and_page_labels() {
        use lopdf::dictionary;
        let (_root, context, mut app) = saved_comment_editor();
        let path = app.reading.path.clone();
        app.comments.editor = None;
        let mut before = egui::FullOutput::default();
        for _ in 0..3 {
            before = frame(&mut app, &context, Vec::new());
        }
        let header = format!("Page {} · Pagewing", app.page_name(0));
        assert!(before.shapes.iter().any(|shape| matches!(&shape.shape,
            egui::Shape::Text(text) if text.galley.text() == header)));
        let mut changed = app.comments.comments.items[0].clone();
        changed.contents = "Replaced on disk".into();
        changed.review = ReviewStatus::Completed;
        changed.marked = true;
        crate::annotations::save(&crate::document::load(&path, "").unwrap(), &changed, &path)
            .unwrap();
        let mut pdf = lopdf::Document::load(&path).unwrap();
        for object in pdf.objects.values_mut() {
            if let Ok(dict) = object.as_dict_mut()
                && dict.get(b"Subtype").and_then(lopdf::Object::as_name).ok() == Some(b"Highlight")
            {
                dict.set("T", lopdf::Object::string_literal("Another reviewer"));
            }
        }
        pdf.catalog_mut().unwrap().set(
            "PageLabels",
            lopdf::dictionary! {
                "Nums" => vec![lopdf::Object::Integer(0), lopdf::dictionary! { "S" => "r" }.into()],
            },
        );
        pdf.save(&path).unwrap();
        app.comments.editor = None;
        app.reload(&context);
        wait(&mut app, &context, |app| {
            !app.loading && app.comments.loaded
        });
        let after = frame(&mut app, &context, Vec::new());
        for expected in [
            "Page i · 1 · Another reviewer",
            "Replaced on disk",
            "Highlight · Completed / resolved · ✓ Marked",
        ] {
            assert!(
                after.shapes.iter().any(|shape| matches!(&shape.shape,
                egui::Shape::Text(text) if text.galley.text() == expected)),
                "missing refreshed card line: {expected}"
            );
        }
        // The Comments event can replace metadata without a new document
        // identity. Reusing index zero must still rebuild its painted text.
        let mut replacement = app.comments.comments.items[0].clone();
        replacement.contents = "Another metadata snapshot".into();
        app.comments.set_comments(Comments {
            items: vec![replacement],
            limited: false,
        });
        let refreshed = frame(&mut app, &context, Vec::new());
        assert!(refreshed.shapes.iter().any(|shape| matches!(&shape.shape,
            egui::Shape::Text(text) if text.galley.text() == "Another metadata snapshot")));
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
            crate::comment_cards::preview_text(&long, 160),
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
