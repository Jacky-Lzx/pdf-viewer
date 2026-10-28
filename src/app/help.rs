use super::{ACCENT, Pagewing, appearance, dialog};
use eframe::egui::{self, RichText};
use std::fmt::Write;

mod catalog;
use catalog::{FIXED_SHORTCUTS, SHORTCUTS};

#[derive(Default)]
pub(super) struct HelpFilter {
    query: String,
    catalog: crate::help_catalog::HelpCatalog,
    pub(super) focus: bool,
    scroll_to_top: bool,
}

impl HelpFilter {
    fn reset(&mut self) {
        self.catalog.replace(
            SHORTCUTS
                .iter()
                .map(|(keys, action)| ((*keys).into(), (*action).into())),
        );
        self.query.clear();
        self.focus = true;
        self.filter();
    }

    fn filter(&mut self) {
        if self.catalog.total() == 0 {
            self.catalog.replace(
                SHORTCUTS
                    .iter()
                    .map(|(keys, action)| ((*keys).into(), (*action).into())),
            );
        }
        self.catalog.filter(&self.query);
        self.scroll_to_top = true;
    }

    fn configure(&mut self, shortcuts: &crate::settings::Shortcuts) {
        if shortcuts.overrides.is_empty() {
            return;
        }
        self.catalog.replace(
            crate::settings::commands()
                .iter()
                .map(|command| (shortcuts.label(command.id), command.label.into()))
                .chain(
                    FIXED_SHORTCUTS
                        .iter()
                        .map(|(keys, action)| ((*keys).into(), (*action).into())),
                ),
        );
        self.filter();
    }

    fn rows(&self) -> impl ExactSizeIterator<Item = (&str, &str)> + '_ {
        self.catalog.rows()
    }

    fn copy_text(&self) -> String {
        let mut text = String::from("Pagewing keyboard shortcuts\n\n");
        for (keys, action) in self.rows() {
            let _ = writeln!(text, "{keys}\t{action}");
        }
        text
    }
}

impl Pagewing {
    pub(super) fn open_help(&mut self, context: &egui::Context) {
        self.close_zoom(context);
        self.outline.cancel_input();
        self.nav.clear_prefix();
        self.hints.cancel();
        self.selection.cancel();
        self.mark_action = None;
        self.recent_picker = None;
        self.properties_open = false;
        self.page_input = None;
        self.password_input = None;
        self.search_open = false;
        self.search_focus = false;
        self.search_navigation.clear();
        self.help = true;
        self.help_filter.reset();
        self.help_filter.configure(&self.library.shortcuts);
        context.memory_mut(|memory| {
            if let Some(id) = memory.focused() {
                memory.surrender_focus(id);
            }
            memory.move_focus(egui::FocusDirection::None);
        });
    }

    pub(super) fn close_help(&mut self, context: &egui::Context) {
        self.help = false;
        self.help_filter.focus = false;
        context.memory_mut(|memory| memory.surrender_focus(egui::Id::new("pagewing-help-input")));
    }

    pub(super) fn help_window(&mut self, context: &egui::Context) {
        if !self.help {
            return;
        }
        let mut open = true;
        let mut close = false;
        let mut copied = false;
        let size = context.content_rect().size();
        let short = size.y < 420.0;
        dialog::window(context, "Keyboard shortcuts", "Shortcuts", 620.0)
            .open(&mut open)
            .show(context, |ui| {
                if !short {
                    ui.label(
                        RichText::new("Stay in the flow.")
                            .size(23.0)
                            .strong()
                            .color(ACCENT),
                    );
                    ui.add_space(8.0);
                }
                let (input, _) = super::shortcuts::text_input(
                    ui,
                    egui::Id::new("pagewing-help-input"),
                    &mut self.help_filter.focus,
                    &mut self.deferred_input,
                    egui::TextEdit::singleline(&mut self.help_filter.query)
                        .desired_width(f32::INFINITY)
                        .char_limit(256)
                        .hint_text("Filter by a key or action…"),
                );
                if input.changed() {
                    self.help_filter.filter();
                }
                ui.weak(format!("{} shortcuts", self.help_filter.catalog.len()));
                let stacked = ui.available_width() < 420.0;
                let column_width = (ui.available_width() - 24.0) * 0.5;
                let mut results = egui::ScrollArea::vertical()
                    .id_salt("pagewing-help-results")
                    // Reserve room for the title, filter and actions outside
                    // scrolling, including the larger frame on desktop.
                    .max_height((size.y - if short { 180.0 } else { 300.0 }).max(32.0));
                if self.help_filter.scroll_to_top {
                    results = results.vertical_scroll_offset(0.0);
                }
                results.show(ui, |ui| {
                    if self.help_filter.catalog.is_empty() {
                        ui.label("No matching shortcuts.");
                    }
                    if stacked {
                        for (keys, action) in self.help_filter.rows() {
                            ui.add(egui::Label::new(RichText::new(keys).monospace()).wrap());
                            ui.add(egui::Label::new(action).wrap());
                            ui.add_space(8.0);
                        }
                    } else {
                        egui::Grid::new("keys")
                            .num_columns(2)
                            .spacing([24.0, 8.0])
                            .max_col_width(column_width)
                            .show(ui, |ui| {
                                for (keys, action) in self.help_filter.rows() {
                                    ui.monospace(keys);
                                    ui.label(action);
                                    ui.end_row();
                                }
                            });
                    }
                });
                if !ui.is_sizing_pass() {
                    self.help_filter.scroll_to_top = false;
                }
                ui.separator();
                ui.horizontal(|ui| {
                    if ui
                        .add_enabled_ui(!self.help_filter.catalog.is_empty(), |ui| {
                            appearance::action_button(
                                ui,
                                "help-copy",
                                egui::Button::new("Copy shortcuts"),
                            )
                        })
                        .inner
                        .clicked()
                    {
                        context.copy_text(self.help_filter.copy_text());
                        copied = true;
                        input.request_focus();
                    }
                    close = appearance::action_button(ui, "help-close", egui::Button::new("Close"))
                        .on_hover_text("Escape")
                        .clicked();
                });
                if !short {
                    ui.weak("Counts work with movement: 5j, 3J, 12gg. Drag to pan.");
                }
            });
        if copied {
            self.notify("Copied keyboard shortcuts");
        }
        if !open || close {
            self.close_help(context);
        }
    }
}

#[cfg(test)]
mod tests;
