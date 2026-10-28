use super::{MarkAction, Pagewing, appearance, shortcuts, spread, toolbar};
use crate::state::ReadingMark;
use eframe::egui::{self, RichText};
use std::time::{Duration, Instant};

const NOTICE_DURATION: Duration = Duration::from_secs(5);

pub(super) struct Notice {
    pub(super) text: String,
    at: Instant,
    pub(super) recovery: Option<Recovery>,
}

pub(super) enum Recovery {
    Bookmark(usize),
    Mark { name: char, position: ReadingMark },
}

impl Recovery {
    fn caption(&self) -> &'static str {
        match self {
            Self::Bookmark(_) => "Undo bookmark",
            Self::Mark { .. } => "Undo mark",
        }
    }
}

impl Notice {
    pub(super) fn new(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            at: Instant::now(),
            recovery: None,
        }
    }
}

impl Pagewing {
    pub(super) fn status_control_focused(&self, context: &egui::Context) -> bool {
        shortcuts::focused_in_region(context, self.status_region)
    }

    fn restorable_deletion(&self) -> Option<&Recovery> {
        let notice = self.message.as_ref()?;
        if notice.at.elapsed() >= NOTICE_DURATION || self.loading {
            return None;
        }
        let recovery = notice.recovery.as_ref()?;
        let valid = match recovery {
            Recovery::Bookmark(page) => {
                *page < self.nav.count && !self.reading.bookmarks.contains(page)
            }
            Recovery::Mark { name, position } => {
                position.page < self.nav.count && !self.reading.marks.contains_key(name)
            }
        };
        valid.then_some(recovery)
    }

    pub(super) fn restore_deleted(&mut self) {
        if self.restorable_deletion().is_none() {
            return;
        }
        let recovery = self.message.as_mut().unwrap().recovery.take().unwrap();
        match recovery {
            Recovery::Bookmark(page) => {
                self.set_bookmark(page, true);
                self.notify("Bookmark restored");
            }
            Recovery::Mark { name, position } => {
                self.reading.marks.insert(name, position);
                self.dirty = true;
                self.notify(format!("Mark {name} restored"));
            }
        }
    }

    pub(super) fn status(&mut self, context: &egui::Context) {
        self.input_buttons.restore_deleted = None;
        self.input_buttons.page = None;
        self.input_buttons.zoom = None;
        self.input_buttons.view_menu = None;
        self.status_region = None;
        if self
            .message
            .as_ref()
            .is_some_and(|notice| notice.at.elapsed() >= NOTICE_DURATION)
        {
            self.message = None;
        }
        // Keep transient feedback and recovery visible in the reading view,
        // including unfinished Vim commands and selection/link modes.
        if self.controls_hidden
            && self.message.is_none()
            && !self.loading
            && self.mark_action.is_none()
            && !self.hints.active
            && !self.selection.active
            && self.nav.prefix.is_empty()
            && !self.nav.pending_g
            && !self.nav.pending_z
        {
            return;
        }
        let panel = egui::TopBottomPanel::bottom("status")
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
                    // Keep recovery reachable before long page labels, even in
                    // a narrow window. The action also describes the notice.
                    let recovery_caption = self.restorable_deletion().map(Recovery::caption);
                    if let Some(caption) = recovery_caption {
                        self.input_buttons.restore_deleted = Some(
                            appearance::action_button(
                                ui,
                                "undo-deletion",
                                egui::Button::new(caption),
                            )
                            .on_hover_ui(|ui| {
                                if let Some(notice) = &self.message {
                                    ui.label(&notice.text);
                                }
                                ui.label("Restore without leaving this page");
                            })
                            .id,
                        );
                        ui.separator();
                    }
                    if !self.controls_hidden && self.nav.count > 0 {
                        let (first, second) = if self.reading.facing {
                            let pages = spread::page_range(self.nav.page, self.nav.count);
                            (pages.start, (pages.len() == 2).then_some(pages.end - 1))
                        } else {
                            (self.nav.page, None)
                        };
                        self.input_buttons.page = Some(
                            appearance::action_button(
                                ui,
                                "page",
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
                    } else if let Some(notice) = &self.message {
                        if recovery_caption.is_none() {
                            ui.label(&notice.text);
                        }
                        context.request_repaint_after(
                            NOTICE_DURATION.saturating_sub(notice.at.elapsed()),
                        );
                    } else if self.nav.count == 0 {
                        ui.weak("j k  scroll    J K  pages    /  find    ?  help");
                    }
                    if !self.controls_hidden {
                        toolbar::right_group(ui, 290.0, |ui| {
                            ui.add_space(12.0);
                            if self.nav.count > 0 {
                                self.view_controls(ui);
                            }
                        });
                    }
                });
            });
        self.status_region = Some((panel.response.layer_id, panel.response.rect));
    }
}

#[cfg(test)]
mod tests;
