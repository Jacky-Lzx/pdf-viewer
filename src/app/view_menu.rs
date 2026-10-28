use super::{Pagewing, shortcuts};
use crate::navigation::Fit;
use eframe::egui;

impl Pagewing {
    pub(super) fn view_menu(&mut self, ui: &mut egui::Ui) {
        let mode = if self.reading.facing {
            "Two pages"
        } else if self.reading.continuous {
            "Continuous"
        } else {
            "Single page"
        };
        let response = ui
            .button(format!("View: {mode}"))
            .on_hover_text("Choose page layout and size");
        self.input_buttons.view_menu = Some(response.id);
        shortcuts::show_menu(ui, &response, |ui, keyboard_open| {
            let mut focus = super::menu_focus::MenuFocus::new(ui, keyboard_open);
            let size = ui.ctx().content_rect().size();
            ui.set_max_width((size.x - 32.0).clamp(120.0, 420.0));
            egui::ScrollArea::vertical()
                .id_salt("view-menu-options")
                .max_height((size.y - 32.0).max(64.0))
                .show(ui, |ui| {
                    ui.label("Page layout");
                    for (label, continuous, facing) in [
                        ("Single page", false, false),
                        ("Continuous", true, false),
                        ("Two pages side by side", false, true),
                    ] {
                        let response = ui.add(
                            egui::Button::selectable(
                                self.reading.continuous == continuous
                                    && self.reading.facing == facing,
                                label,
                            )
                            .truncate(),
                        );
                        focus.observe(&response);
                        if response.clicked() {
                            self.set_display_mode(continuous, facing);
                            ui.close();
                        }
                    }
                    ui.separator();
                    ui.label("Page size");
                    for (label, binding, fit) in [
                        ("Fit page", "fit_page", Fit::Page),
                        ("Fit width", "fit_width", Fit::Width),
                        ("Fit height", "fit_height", Fit::Height),
                        ("Actual size", "actual_size", Fit::Custom(1.0)),
                    ] {
                        let caption =
                            format!("{label} · {}", self.library.shortcuts.label(binding));
                        let response = ui
                            .add(
                                egui::Button::selectable(self.reading.fit == fit, &caption)
                                    .truncate(),
                            )
                            .on_hover_text(caption);
                        focus.observe(&response);
                        if response.clicked() {
                            if fit == Fit::Custom(1.0) {
                                self.actual_size();
                            } else {
                                self.resolve_view();
                                self.reading.fit = fit;
                                self.pan = egui::Vec2::ZERO;
                                self.dirty = true;
                            }
                            ui.close();
                        }
                    }
                    shortcuts::reveal_focused_control(ui, ui.clip_rect().size());
                });
            focus.finish(ui);
        });
    }
}

#[cfg(test)]
mod tests;
