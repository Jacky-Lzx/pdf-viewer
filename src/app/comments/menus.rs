//! Reader-owned popup choices share opening, logical focus and compact layout.
use super::{Comment, CommentActions, Pagewing, shortcuts};
use crate::app::menu_focus::MenuFocus;
use eframe::egui;

fn arrow(ui: &egui::Ui, response: &egui::Response) {
    let center = egui::pos2(
        response.rect.right() - ui.spacing().button_padding.x - 4.0,
        response.rect.center().y,
    );
    let stroke = egui::Stroke::new(1.2_f32, ui.style().interact(response).fg_stroke.color);
    let low = center + egui::vec2(0.0, 1.5);
    ui.painter()
        .line_segment([center + egui::vec2(-3.0, -1.5), low], stroke);
    ui.painter()
        .line_segment([low, center + egui::vec2(3.0, -1.5)], stroke);
}

pub(super) fn button(ui: &mut egui::Ui, id: &str, text: &str, width: f32) -> egui::Response {
    ui.scope_builder(egui::UiBuilder::new().id(("comment-menu", id)), |ui| {
        if ui.available_width() < 120.0 {
            ui.spacing_mut().button_padding.x = ui.spacing().button_padding.x.min(6.0);
        }
        let response = ui.add_sized(
            egui::vec2(
                width.min(ui.available_width()),
                ui.spacing().interact_size.y,
            ),
            egui::Button::new(text).right_text("  ").truncate(),
        );
        arrow(ui, &response);
        response
    })
    .inner
    .on_hover_text(text)
}

pub(super) fn choices<T: Copy + PartialEq>(
    ui: &mut egui::Ui,
    response: &egui::Response,
    value: &mut T,
    choices: impl IntoIterator<Item = (T, &'static str)>,
) -> bool {
    let mut changed = false;
    shortcuts::show_menu(ui, response, |ui, keyboard_open| {
        let mut focus = MenuFocus::new(ui, keyboard_open);
        let size = ui.ctx().content_rect().size();
        ui.set_max_width((size.x - 32.0).clamp(120.0, 360.0));
        egui::ScrollArea::vertical()
            .id_salt("comment-menu-choices")
            .max_height((size.y - 32.0).max(64.0))
            .show(ui, |ui| {
                for (choice, label) in choices {
                    let response = ui.add(egui::Button::selectable(*value == choice, label).wrap());
                    focus.observe(&response);
                    if response.clicked() {
                        changed |= *value != choice;
                        *value = choice;
                        ui.close();
                    }
                }
                shortcuts::reveal_focused_control(ui, ui.clip_rect().size());
            });
        focus.finish(ui);
    });
    changed
}

pub(super) fn more(
    ui: &mut egui::Ui,
    comment: &Comment,
    enabled: bool,
) -> (egui::Id, CommentActions) {
    let response = ui
        .add_enabled_ui(enabled && !comment.locked, |ui| {
            let response = crate::app::appearance::action_button(
                ui,
                "comment-more",
                egui::Button::new(egui::RichText::new("More…").small()).right_text("  "),
            );
            arrow(ui, &response);
            response
        })
        .inner;
    let mut actions = CommentActions::default();
    shortcuts::show_menu(ui, &response, |ui, keyboard_open| {
        let mut focus = MenuFocus::new(ui, keyboard_open);
        ui.set_max_width((ui.ctx().content_rect().width() - 32.0).clamp(120.0, 360.0));
        let reply = ui.button("Reply");
        focus.observe(&reply);
        let delete = ui
            .add(
                egui::Button::new(if comment.parent.is_some() {
                    "Delete reply & replies"
                } else {
                    "Delete comment & highlight"
                })
                .wrap(),
            )
            .on_hover_text("Delete this annotation and all its replies from the current PDF.");
        focus.observe(&delete);
        actions.reply = reply.clicked();
        actions.delete = delete.clicked();
        if actions.reply || actions.delete {
            ui.close();
        }
        focus.finish(ui);
    });
    (response.id, actions)
}

impl Pagewing {
    pub(super) fn comment_review_filter(&mut self, ui: &mut egui::Ui, width: f32) {
        use crate::annotations::ReviewStatus;
        let status = self
            .comments
            .view
            .filter
            .review
            .map_or("All statuses", ReviewStatus::label);
        let response = button(ui, "comment-review-filter", status, width.min(180.0));
        self.input_buttons.comment_filter_menu = Some(response.id);
        choices(
            ui,
            &response,
            &mut self.comments.view.filter.review,
            std::iter::once((None, "All statuses"))
                .chain(ReviewStatus::ALL.map(|review| (Some(review), review.label()))),
        );
    }
}
