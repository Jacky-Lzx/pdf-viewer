use super::{MarkAction, Pagewing, appearance, spread, toolbar};
use eframe::egui::{self, RichText};
use std::time::Duration;

impl Pagewing {
    pub(super) fn status(&mut self, context: &egui::Context) {
        egui::TopBottomPanel::bottom("status")
            .frame(appearance::panel(context, 2))
            .exact_height(36.0)
            .show(context, |ui| {
                toolbar::horizontal_bar(ui, "status-controls", |ui| {
                    ui.add_space(12.0);
                    ui.label(
                        RichText::new(match self.mark_action {
                            Some(MarkAction::Set) => "SET MARK · a–z",
                            Some(MarkAction::Jump) => "GO TO MARK · a–z or ' / `",
                            None if self.hints.active => "LINKS",
                            None if self.selection.active => "VISUAL",
                            None if self.recent_picker.is_some() => "RECENT",
                            None => "NORMAL",
                        })
                        .small()
                        .strong()
                        .color(appearance::accent(ui)),
                    );
                    if self.hints.active {
                        ui.monospace(&self.hints.prefix);
                    }
                    if !self.nav.prefix.is_empty() || self.nav.pending_g || self.nav.pending_z {
                        ui.monospace(format!(
                            "{}{}",
                            self.nav.prefix,
                            if self.nav.pending_g {
                                "g"
                            } else if self.nav.pending_z {
                                "z"
                            } else {
                                ""
                            }
                        ));
                    }
                    ui.separator();
                    if self.nav.count > 0 {
                        let (first, second) = if self.reading.facing {
                            let pages = spread::page_range(self.nav.page, self.nav.count);
                            (pages.start, (pages.len() == 2).then_some(pages.end - 1))
                        } else {
                            (self.nav.page, None)
                        };
                        self.input_buttons.page = Some(
                            ui.add(
                                egui::Button::new(self.labels.status_caption(
                                    first,
                                    second,
                                    self.nav.count,
                                ))
                                .frame(false),
                            )
                            .on_hover_ui(|ui| {
                                ui.set_max_width(ui.spacing().tooltip_width);
                                ui.label(format!(
                                    "Go to page · :\n{}\n{:.0} ms · {:.1} MB cached",
                                    self.labels.label(self.nav.page).unwrap_or_default(),
                                    self.render_ms,
                                    self.cache.bytes as f64 / 1_048_576.0
                                ));
                            })
                            .id,
                        );
                        ui.separator();
                    }
                    if self.loading && self.reloading {
                        ui.spinner();
                        ui.label("Reloading document…");
                    } else if let Some((message, at)) = &self.message
                        && at.elapsed() < Duration::from_secs(5)
                    {
                        ui.label(message);
                        context.request_repaint_after(
                            Duration::from_secs(5).saturating_sub(at.elapsed()),
                        );
                    } else if self.nav.count == 0 {
                        ui.weak("j k  scroll    J K  pages    /  find    ?  help");
                    }
                    toolbar::right_group(ui, 290.0, |ui| {
                        ui.add_space(12.0);
                        if self.nav.count > 0 {
                            self.view_controls(ui);
                        }
                    });
                });
            });
    }
}
