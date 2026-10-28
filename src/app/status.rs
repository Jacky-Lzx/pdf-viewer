use super::{MarkAction, Pagewing, appearance, spread, toolbar};
use eframe::egui::{self, RichText};
use std::time::{Duration, Instant};

const NOTICE_DURATION: Duration = Duration::from_secs(5);

pub(super) struct Notice {
    pub(super) text: String,
    at: Instant,
    pub(super) restore_bookmark: Option<usize>,
}

impl Notice {
    pub(super) fn new(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            at: Instant::now(),
            restore_bookmark: None,
        }
    }
}

impl Pagewing {
    fn restorable_bookmark(&self) -> Option<usize> {
        let notice = self.message.as_ref()?;
        let page = notice.restore_bookmark?;
        (notice.at.elapsed() < NOTICE_DURATION
            && !self.loading
            && page < self.nav.count
            && !self.reading.bookmarks.contains(&page))
        .then_some(page)
    }

    pub(super) fn restore_bookmark(&mut self) {
        if let Some(page) = self.restorable_bookmark() {
            self.set_bookmark(page, true);
            self.notify("Bookmark restored");
        }
    }

    pub(super) fn status(&mut self, context: &egui::Context) {
        self.input_buttons.restore_bookmark = None;
        if self
            .message
            .as_ref()
            .is_some_and(|notice| notice.at.elapsed() >= NOTICE_DURATION)
        {
            self.message = None;
        }
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
                    // Keep recovery reachable before long page labels, even in
                    // a narrow window. The action also describes the notice.
                    let can_restore = self.restorable_bookmark().is_some();
                    if can_restore {
                        self.input_buttons.restore_bookmark = Some(
                            ui.push_id("undo-bookmark", |ui| {
                                ui.button("Undo bookmark").on_hover_text(
                                    "Restore the deleted bookmark without leaving this page",
                                )
                            })
                            .inner
                            .id,
                        );
                        ui.separator();
                    }
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
                    } else if let Some(notice) = &self.message {
                        if !can_restore {
                            ui.label(&notice.text);
                        }
                        context.request_repaint_after(
                            NOTICE_DURATION.saturating_sub(notice.at.elapsed()),
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::navigation::{Fit, Navigation};

    fn app() -> (tempfile::TempDir, egui::Context, Pagewing) {
        let directory = tempfile::tempdir().unwrap();
        let context = egui::Context::default();
        let mut app =
            Pagewing::with_context(&context, None, None, directory.path().join("library.json"));
        app.nav = Navigation::new(80, 2);
        app.reading.bookmarks.extend([2, 4, 79]);
        app.refresh_bookmarks();
        (directory, context, app)
    }

    fn frame(
        app: &mut Pagewing,
        context: &egui::Context,
        width: f32,
        events: Vec<egui::Event>,
    ) -> egui::FullOutput {
        context.run(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(width, 400.0),
                )),
                events,
                ..Default::default()
            },
            |context| {
                app.keys(context);
                app.status(context);
                egui::CentralPanel::default().show(context, |_| {});
            },
        )
    }

    fn undo_button(output: &egui::FullOutput) -> Option<egui::Rect> {
        output.shapes.iter().find_map(|shape| match &shape.shape {
            egui::Shape::Text(text) if text.galley.text() == "Undo bookmark" => {
                let rect = egui::Rect::from_min_size(text.pos, text.galley.size());
                assert!(shape.clip_rect.contains_rect(rect));
                Some(rect)
            }
            _ => None,
        })
    }

    #[test]
    fn undo_button_restores_another_page_without_moving_the_view() {
        for width in [240.0, 360.0, 1180.0] {
            let (_directory, context, mut app) = app();
            app.reading.fit = Fit::Custom(1.75);
            app.reading.rotation = 1;
            app.pan = egui::vec2(37.0, 53.0);
            app.pending_offset = Some([0.4, 0.6]);
            app.remove_bookmark(79);
            for _ in 0..3 {
                frame(&mut app, &context, width, Vec::new());
            }
            let output = frame(&mut app, &context, width, Vec::new());
            let button = undo_button(&output).expect("recovery must be visible");
            assert!(button.left() >= 0.0 && button.right() <= width);
            app.dirty = false;
            for pressed in [true, false] {
                frame(
                    &mut app,
                    &context,
                    width,
                    vec![
                        egui::Event::PointerMoved(button.center()),
                        egui::Event::PointerButton {
                            pos: button.center(),
                            button: egui::PointerButton::Primary,
                            pressed,
                            modifiers: egui::Modifiers::NONE,
                        },
                    ],
                );
            }
            assert_eq!(app.bookmark_pages, [2, 4, 79]);
            assert_eq!(app.reading.bookmarks, [2, 4, 79].into_iter().collect());
            assert!(app.dirty, "restored bookmark must be saved");
            assert_eq!(app.nav.page, 2);
            assert_eq!(app.pan, egui::vec2(37.0, 53.0));
            assert_eq!(app.pending_offset, Some([0.4, 0.6]));
            assert_eq!(app.reading.fit, Fit::Custom(1.75));
            assert_eq!(app.reading.rotation, 1);
            assert!(app.reveal_page.is_none());
            assert!(app.restorable_bookmark().is_none());
            let output = frame(&mut app, &context, width, Vec::new());
            assert!(undo_button(&output).is_none());
        }
    }

    #[test]
    fn keyboard_toggle_can_be_undone_and_later_edits_replace_recovery() {
        let (_directory, _context, mut app) = app();
        app.bookmark();
        assert_eq!(app.restorable_bookmark(), Some(2));
        app.restore_bookmark();
        assert_eq!(app.bookmark_pages, [2, 4, 79]);
        app.remove_bookmark(4);
        app.remove_bookmark(79);
        assert_eq!(app.restorable_bookmark(), Some(79));
        app.restore_bookmark();
        assert_eq!(app.bookmark_pages, [2, 79]);
        app.remove_bookmark(79);
        app.set_bookmark(79, true);
        app.set_bookmark(79, false);
        app.restore_bookmark();
        assert_eq!(
            app.bookmark_pages,
            [2],
            "an old action must not undo later edits"
        );
        app.remove_bookmark(2);
        app.notify("A newer notice");
        app.restore_bookmark();
        assert!(app.bookmark_pages.is_empty());
    }

    #[test]
    fn focused_recovery_consumes_activation_keys_before_reader_navigation() {
        for key in [egui::Key::Enter, egui::Key::Space] {
            let (_directory, context, mut app) = app();
            app.remove_bookmark(79);
            frame(&mut app, &context, 1180.0, Vec::new());
            let id = app.input_buttons.restore_bookmark.unwrap();
            context.memory_mut(|memory| memory.request_focus(id));
            // A pending page count makes an accidental Enter observable.
            app.nav.prefix = "40".into();
            frame(
                &mut app,
                &context,
                1180.0,
                vec![egui::Event::Key {
                    key,
                    physical_key: None,
                    pressed: true,
                    repeat: false,
                    modifiers: egui::Modifiers::NONE,
                }],
            );
            assert_eq!(app.bookmark_pages, [2, 4, 79]);
            assert_eq!(app.nav.page, 2);
            assert_eq!(app.nav.prefix, "40");
        }
    }

    #[test]
    fn expired_or_invalid_recovery_never_mutates_bookmarks() {
        let (_directory, context, mut app) = app();
        app.remove_bookmark(79);
        app.message.as_mut().unwrap().at = Instant::now() - NOTICE_DURATION;
        app.restore_bookmark();
        assert_eq!(app.bookmark_pages, [2, 4]);
        assert!(undo_button(&frame(&mut app, &context, 1180.0, Vec::new())).is_none());
        assert!(
            app.message.is_none(),
            "expiry releases the notice and action"
        );
        app.remove_bookmark(4);
        app.loading = true;
        app.restore_bookmark();
        assert_eq!(app.bookmark_pages, [2]);
        app.loading = false;
        app.nav.resize(3);
        app.restore_bookmark();
        assert_eq!(app.bookmark_pages, [2]);
        app.loading = true;
        app.remove_bookmark(2);
        app.loading = false;
        app.restore_bookmark();
        assert!(
            app.bookmark_pages.is_empty(),
            "reload-time edits cannot offer stale recovery"
        );
    }

    #[test]
    fn reload_and_document_switch_discard_recovery() {
        let (directory, context, mut app) = app();
        app.reading.path = directory.path().join("current.pdf");
        app.remove_bookmark(4);
        app.reload(&context);
        assert!(app.loading);
        app.loading = false;
        app.restore_bookmark();
        assert_eq!(app.bookmark_pages, [2, 79]);
        app.remove_bookmark(79);
        app.clear_document(&context);
        app.nav = Navigation::new(80, 0);
        app.restore_bookmark();
        assert!(app.bookmark_pages.is_empty());
        assert!(app.reading.bookmarks.is_empty());
        assert!(app.message.is_none());
    }
}
