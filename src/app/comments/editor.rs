//! Comment fields and the pinned save/discard/thread actions.
use super::{CommentActions, Pagewing};
use crate::annotations::ReviewStatus;
use eframe::egui;

impl Pagewing {
    pub(super) fn comment_editor_fields(
        &mut self,
        ui: &mut egui::Ui,
        available_height: f32,
    ) -> bool {
        let mut changed = false;
        let busy = self.comments.busy();
        let saving = self.comments.saving;
        let Some(comment) = &mut self.comments.editor else {
            return false;
        };
        let top = ui.cursor().top();
        ui.separator();
        ui.label(format!(
            "{} · page {}",
            if comment.slot.is_none() {
                if comment.parent.is_some() {
                    "New reply"
                } else {
                    "New highlight"
                }
            } else {
                "Edit comment"
            },
            comment.page + 1
        ));
        let read_only = !comment.editable();
        if read_only {
            ui.label(if comment.locked {
                "This annotation is read-only."
            } else {
                "Comment text is read-only."
            });
        }
        // Long bodies scroll independently so their caret cannot push the
        // filter, result cards and status controls out of a regular window.
        // The outer panel can still scroll when the window is very small.
        let spacing = ui.spacing().item_spacing.y;
        let line = ui.text_style_height(&egui::TextStyle::Button);
        let button = ui
            .spacing()
            .interact_size
            .y
            .max(line + 2.0 * ui.spacing().button_padding.y);
        let checkbox = ui
            .spacing()
            .interact_size
            .y
            .max(ui.spacing().icon_width)
            .max(line);
        let controls = button
            + checkbox
            + 2.0 * spacing
            + if comment.states.is_empty() {
                0.0
            } else {
                button + spacing
            };
        let height = (available_height - (ui.cursor().top() - top) - controls).clamp(80.0, 280.0);
        // A queued save must still receive this batch's text;
        // other editor actions already stay disabled via busy.
        changed |= super::body::show(
            ui,
            &mut comment.contents,
            &mut self.comments.focus,
            &mut self.deferred_input,
            height,
            !saving && !read_only,
        );
        ui.add_enabled_ui(!busy && !comment.locked, |ui| {
            let status = format!("Status: {}", comment.review.label());
            let response = super::menus::button(ui, "comment-status", &status, 220.0);
            self.input_buttons.comment_status_menu = Some(response.id);
            changed |= super::menus::choices(
                ui,
                &response,
                &mut comment.review,
                ReviewStatus::ALL.map(|review| (review, review.label())),
            );
            changed |= ui
                .checkbox(&mut comment.marked, "Marked (Acrobat checkmark)")
                .changed();
        });
        if !comment.states.is_empty() {
            egui::CollapsingHeader::new("Status history").show(ui, |ui| {
                for state in &comment.states {
                    ui.label(format!(
                        "{} · {}: {}",
                        state.author, state.model, state.value
                    ));
                }
            });
        }
        changed
    }

    pub(super) fn comment_editor_actions(
        &mut self,
        ui: &mut egui::Ui,
        compact: bool,
    ) -> CommentActions {
        let mut actions = CommentActions::default();
        let Some(comment) = &self.comments.editor else {
            return actions;
        };
        let busy = self.comments.busy();
        let thread_actions = |ui: &mut egui::Ui| {
            let reply = ui
                .add_enabled(!busy && !comment.locked, egui::Button::new("Reply"))
                .clicked();
            let delete = ui
                .add_enabled(
                    !busy && !comment.locked,
                    egui::Button::new(if comment.parent.is_some() {
                        "Delete reply & replies"
                    } else {
                        "Delete comment & highlight"
                    }),
                )
                .on_hover_text("Delete this annotation and all its replies from the current PDF.")
                .clicked();
            (reply, delete)
        };
        ui.scope(|ui| {
            if compact {
                // Keep all pinned actions available without consuming the whole
                // content viewport in the minimum-height window.
                ui.spacing_mut().item_spacing.y = 2.0;
                ui.spacing_mut().button_padding.y = 2.0;
                ui.spacing_mut().button_padding.x = 1.0;
                ui.spacing_mut().item_spacing.x = 2.0;
            }
            let unsaved = self.comments.unsaved();
            if compact {
                ui.add_space(1.0);
                ui.horizontal(|ui| {
                    if !busy {
                        ui.weak(
                            egui::RichText::new(if unsaved { "Unsaved" } else { "Saved" }).small(),
                        );
                    }
                    if comment.slot.is_some() {
                        let (id, more) = super::menus::more(ui, comment, !busy);
                        self.input_buttons.comment_actions_menu = Some(id);
                        actions.reply = more.reply;
                        actions.delete = more.delete;
                    }
                });
            } else {
                ui.separator();
                if !busy {
                    ui.weak(if unsaved {
                        "Unsaved changes"
                    } else {
                        "Saved in PDF"
                    });
                }
            }
            // Bind Save and Discard to their actions when the compact footer
            // changes the structure of the preceding status and menu row.
            ui.scope_builder(
                egui::UiBuilder::new().id(("comment-editor-primary", self.id)),
                |ui| {
                    ui.horizontal_wrapped(|ui| {
                        actions.save = ui
                            .add_enabled(
                                !busy
                                    && !comment.locked
                                    && (comment.editable()
                                        || comment.contents == self.comments.original),
                                egui::Button::new(if compact { "Save" } else { "Save comment" }),
                            )
                            .on_hover_text(
                                "Save this comment directly to the current PDF · Ctrl/Cmd+Enter",
                            )
                            .clicked();
                        actions.discard = ui
                            .add_enabled(
                                !busy,
                                egui::Button::new(if compact {
                                    "Discard"
                                } else {
                                    "Discard / close editor"
                                }),
                            )
                            .on_hover_text("Discard changes and close this editor")
                            .clicked();
                    });
                },
            );
            if comment.slot.is_some() && !compact {
                (actions.reply, actions.delete) = ui.horizontal_wrapped(thread_actions).inner;
            }
            if busy {
                ui.horizontal(|ui| {
                    ui.spinner();
                    ui.label("Saving…");
                });
            }
        });
        actions
    }
}
