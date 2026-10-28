use super::{Pagewing, appearance, reveal_offset};
use eframe::egui::{self, RichText, Vec2};

impl Pagewing {
    pub(in crate::app) fn focus_contents(&mut self) {
        if self.outline.is_empty() {
            self.notify("This document has no chapter outline");
            return;
        }
        self.nav.clear_prefix();
        self.hints.cancel();
        self.selection.cancel();
        self.mark_action = None;
        self.search_open = false;
        self.search_focus = false;
        self.pending_search_jump = None;
        self.library.sidebar = true;
        self.outline.focus = true;
        self.outline.reveal = true;
        self.dirty = true;
    }

    pub(super) fn contents(&mut self, ui: &mut egui::Ui) -> bool {
        if self.outline.is_empty() {
            return false;
        }
        let mut target = None;
        let mut toggle = None;
        ui.horizontal(|ui| {
            appearance::section(ui, "CONTENTS");
            if ui.available_width() < 70.0 {
                return;
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.add_enabled_ui(!self.outline.filtering(), |ui| {
                    if ui
                        .small_button("−")
                        .on_hover_text("Collapse all chapters")
                        .clicked()
                    {
                        self.outline.fold_all(true);
                    }
                    if ui
                        .small_button("+")
                        .on_hover_text("Expand all chapters")
                        .clicked()
                    {
                        self.outline.fold_all(false);
                    }
                });
            });
        });
        let response = ui.horizontal(|ui| {
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let clear = ui
                    .small_button("×")
                    .on_hover_text("Clear chapter filter")
                    .clicked();
                let (response, _) = crate::app::shortcuts::text_input(
                    ui,
                    egui::Id::new("pagewing-outline-input"),
                    &mut self.outline.focus,
                    &mut self.deferred_input,
                    egui::TextEdit::singleline(&mut self.outline.query)
                        .hint_text("Filter chapters · c")
                        .desired_width(f32::INFINITY),
                );
                let response = response.on_hover_text(
                    "Match all keywords in any order, ignoring letter case. ↑/↓ or Ctrl+J/K: select · PageUp/PageDown: move by visible rows · Ctrl/Cmd+Home/End: first/last · Enter: follow",
                );
                if clear {
                    self.outline.query.clear();
                }
                if response.changed() || clear {
                    self.outline.refilter();
                }
                response
            }).inner
        }).inner;
        let keyboard_focus = response.has_focus() || response.lost_focus();
        if self.outline.visible().is_empty() {
            ui.weak("No matching chapters.");
        }
        let row_height = Self::sidebar_row_height(ui);
        let viewport_height =
            (ui.clip_rect().bottom() - ui.cursor().top()).clamp(row_height, 260.0);
        if !ui.is_sizing_pass() {
            self.outline
                .apply_motions(crate::list_navigation::page_rows(
                    viewport_height,
                    row_height,
                    ui.spacing().item_spacing.y,
                ));
            if std::mem::take(&mut self.outline.confirm)
                && let Some(index) = self.outline.selected()
            {
                if let Some(destination) = self.outline.entry(index).destination {
                    target = Some(destination);
                    response.surrender_focus();
                } else {
                    toggle = Some(index);
                    response.request_focus();
                }
            }
        }
        let mut scroll = egui::ScrollArea::vertical()
            .id_salt("contents")
            .max_height(viewport_height);
        if keyboard_focus
            && !ui.is_sizing_pass()
            && std::mem::take(&mut self.outline.reveal)
            && let Some(row) = self.outline.selected_row()
        {
            let id = ui.make_persistent_id(egui::Id::new("contents"));
            let offset =
                egui::scroll_area::State::load(ui.ctx(), id).map_or(0.0, |state| state.offset.y);
            let top = row as f32 * (row_height + ui.spacing().item_spacing.y);
            scroll = scroll.vertical_scroll_offset(reveal_offset(
                offset,
                top,
                row_height,
                viewport_height,
            ));
        }
        let mut selected = None;
        scroll.show_rows(ui, row_height, self.outline.visible().len(), |ui, range| {
            for row in range {
                let index = self.outline.visible()[row];
                let entry = self.outline.entry(index);
                ui.push_id(index, |ui| {
                    ui.horizontal(|ui| {
                        ui.add_space(
                            (entry.depth.min(8) as f32 * 12.0)
                                .min((ui.available_width() - 60.0).max(0.0)),
                        );
                        if self.outline.has_children(index) {
                            let collapsed = self.outline.is_collapsed(index);
                            let response = ui
                                .add_enabled_ui(!self.outline.filtering(), |ui| {
                                    let response = ui
                                        .allocate_response(Vec2::splat(20.0), egui::Sense::click());
                                    let mut icon = response.clone();
                                    icon.rect = icon.rect.shrink(3.0);
                                    egui::collapsing_header::paint_default_icon(
                                        ui,
                                        if collapsed { 0.0 } else { 1.0 },
                                        &icon,
                                    );
                                    response
                                })
                                .inner;
                            if response
                                .on_hover_text(if collapsed {
                                    "Expand chapter"
                                } else {
                                    "Collapse chapter"
                                })
                                .clicked()
                            {
                                toggle = Some(index);
                            }
                        } else {
                            ui.allocate_space(Vec2::splat(20.0));
                        }
                        if let Some(destination) = entry.destination {
                            let page = destination.page;
                            if ui
                                .add(
                                    egui::Button::selectable(
                                        if keyboard_focus {
                                            self.outline.selected() == Some(index)
                                        } else {
                                            self.nav.page == page
                                        },
                                        &entry.title,
                                    )
                                    .truncate(),
                                )
                                .on_hover_ui(|ui| {
                                    ui.set_max_width(ui.spacing().tooltip_width);
                                    ui.label(format!(
                                        "{}\nPage {}",
                                        entry.title,
                                        self.page_name(page)
                                    ));
                                })
                                .clicked()
                            {
                                selected = Some(index);
                                target = Some(destination);
                            }
                        } else {
                            if ui
                                .add(
                                    egui::Button::selectable(
                                        keyboard_focus && self.outline.selected() == Some(index),
                                        RichText::new(&entry.title).weak(),
                                    )
                                    .truncate(),
                                )
                                .clicked()
                            {
                                selected = Some(index);
                                toggle = Some(index);
                            }
                        }
                    });
                });
            }
        });
        if let Some(index) = selected {
            self.outline.select(index);
        }
        if let Some(index) = toggle {
            self.outline.toggle(index);
        }
        let navigated = target.is_some();
        if let Some(destination) = target {
            self.follow_destination(destination);
        }
        ui.add_space(20.0);
        navigated
    }
}
