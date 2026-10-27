//! Comment fields and the pinned save/discard/thread actions.
use super::{CommentActions, Pagewing, shortcuts};
use crate::annotations::ReviewStatus;
use eframe::egui;

impl Pagewing {
    pub(super) fn comment_editor_fields(&mut self, ui: &mut egui::Ui) -> bool {
        let mut changed = false;
        let busy = self.comments.busy();
        let saving = self.comments.saving;
        let Some(comment) = &mut self.comments.editor else {
            return false;
        };
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
        // A queued save must still receive this batch's text;
        // other editor actions already stay disabled via busy.
        ui.add_enabled_ui(!saving && !read_only, |ui| {
            changed |= shortcuts::text_input(
                ui,
                egui::Id::new("pagewing-comment-input"),
                &mut self.comments.focus,
                &mut self.deferred_input,
                egui::TextEdit::multiline(&mut comment.contents)
                    .desired_rows(5)
                    .desired_width(f32::INFINITY)
                    .hint_text("Write a comment…"),
            )
            .0
            .changed();
        });
        ui.add_enabled_ui(!busy && !comment.locked, |ui| {
            egui::ComboBox::from_id_salt("pagewing-comment-status")
                .selected_text(format!("Status: {}", comment.review.label()))
                .show_ui(ui, |ui| {
                    for status in ReviewStatus::ALL {
                        changed |= ui
                            .selectable_value(&mut comment.review, status, status.label())
                            .changed();
                    }
                });
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
        &self,
        ui: &mut egui::Ui,
        compact: bool,
    ) -> CommentActions {
        let mut actions = CommentActions::default();
        let Some(comment) = &self.comments.editor else {
            return actions;
        };
        let busy = self.comments.busy();
        ui.separator();
        if !busy {
            let unsaved = self.comments.unsaved();
            ui.weak(match (unsaved, compact) {
                (true, true) => "Unsaved",
                (true, false) => "Unsaved changes",
                (false, true) => "Saved",
                (false, false) => "Saved in PDF",
            });
        }
        ui.scope(|ui| {
            if compact {
                ui.spacing_mut().button_padding.x = 1.0;
                ui.spacing_mut().item_spacing.x = 2.0;
            }
            ui.horizontal_wrapped(|ui| {
                actions.save = ui
                    .add_enabled(
                        !busy
                            && !comment.locked
                            && (comment.editable() || comment.contents == self.comments.original),
                        egui::Button::new(if compact { "Save" } else { "Save comment" }),
                    )
                    .on_hover_text("Save this comment directly to the current PDF · Ctrl/Cmd+Enter")
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
        });
        if comment.slot.is_some() {
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
                    .on_hover_text(
                        "Delete this annotation and all its replies from the current PDF.",
                    )
                    .clicked();
                (reply, delete)
            };
            (actions.reply, actions.delete) = if compact {
                ui.add_enabled_ui(!busy, |ui| {
                    ui.menu_button("More…", |ui| {
                        let result = thread_actions(ui);
                        if result.0 || result.1 {
                            ui.close();
                        }
                        result
                    })
                    .inner
                    .unwrap_or_default()
                })
                .inner
            } else {
                ui.horizontal_wrapped(thread_actions).inner
            };
        }
        if busy {
            ui.horizontal(|ui| {
                ui.spinner();
                ui.label("Saving…");
            });
        }
        actions
    }
}
