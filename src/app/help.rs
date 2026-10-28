use super::{ACCENT, Pagewing, dialog};
use eframe::egui::{self, RichText};
use std::fmt::Write;

mod catalog;
use catalog::{FIXED_SHORTCUTS, SHORTCUTS};

#[derive(Default)]
pub(super) struct HelpFilter {
    query: String,
    catalog: crate::help_catalog::HelpCatalog,
    pub(super) focus: bool,
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
                egui::ScrollArea::vertical()
                    .id_salt("pagewing-help-results")
                    // Reserve room for the title, filter and actions outside
                    // scrolling, including the larger frame on desktop.
                    .max_height((size.y - if short { 180.0 } else { 300.0 }).max(32.0))
                    .show(ui, |ui| {
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
                ui.separator();
                ui.horizontal(|ui| {
                    if ui
                        .add_enabled(
                            !self.help_filter.catalog.is_empty(),
                            egui::Button::new("Copy shortcuts"),
                        )
                        .clicked()
                    {
                        context.copy_text(self.help_filter.copy_text());
                        copied = true;
                        input.request_focus();
                    }
                    close = ui.button("Close").on_hover_text("Escape").clicked();
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
mod tests {
    use super::*;

    #[test]
    fn help_and_its_actions_fit_the_minimum_reader_size() {
        let directory = tempfile::tempdir().unwrap();
        let context = egui::Context::default();
        let mut app =
            Pagewing::with_context(&context, None, None, directory.path().join("library.json"));
        for size in dialog::TEST_SIZES {
            for query in ["", "copy link", "jkgmissing"] {
                app.open_help(&context);
                app.help_filter.query = query.into();
                app.help_filter.filter();
                let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, size);
                let mut output = egui::FullOutput::default();
                for _ in 0..4 {
                    output = context.run(
                        egui::RawInput {
                            screen_rect: Some(screen),
                            ..Default::default()
                        },
                        |context| app.help_window(context),
                    );
                }
                let rect = context
                    .memory(|memory| memory.area_rect(egui::Id::new("Keyboard shortcuts")))
                    .unwrap();
                assert!(
                    screen.contains_rect(rect),
                    "screen {screen:?}, window {rect:?}"
                );
                if size.x > 1000.0 {
                    assert!(
                        rect.width() > 600.0,
                        "desktop width must recover after resizing"
                    );
                }
                dialog::assert_visible_actions(
                    &context,
                    &output,
                    screen,
                    "Keyboard shortcuts",
                    &["Copy shortcuts", "Close"],
                );
                app.close_help(&context);
            }
        }
    }

    #[test]
    fn filters_keys_and_descriptions_with_all_case_insensitive_tokens() {
        let mut help = HelpFilter::default();
        help.reset();
        assert_eq!(help.rows().len(), SHORTCUTS.len());
        help.query = "  COPY  LINK  ".into();
        help.filter();
        let rows: Vec<_> = help.rows().collect();
        assert_eq!(rows.len(), 2);
        assert!(rows.iter().any(|(keys, _)| keys == &"F, then a label"));
        assert!(rows.iter().any(|(keys, _)| keys == &"Right-click web link"));
        help.query = "zoom 0".into();
        help.filter();
        assert_eq!(help.rows().next().unwrap().0, "a / Ctrl/Cmd+0");
        assert_eq!(help.rows().len(), 1);
    }

    #[test]
    fn custom_bindings_replace_the_default_help_and_copy_output() {
        let mut shortcuts = crate::settings::Shortcuts::default();
        let mut help = HelpFilter::default();
        for (key, label) in [
            (egui::Key::X, "X"),
            (egui::Key::Y, "Y"),
            (egui::Key::X, "X"),
        ] {
            shortcuts.overrides.insert(
                "next_page".into(),
                vec![crate::settings::Binding::from_event(
                    key,
                    egui::Modifiers::NONE,
                    None,
                )],
            );
            help.reset();
            help.configure(&shortcuts);
            help.query = "next page".into();
            help.filter();
            assert_eq!(help.rows().collect::<Vec<_>>(), vec![(label, "Next page")]);
            assert!(help.copy_text().contains(&format!("{label}\tNext page")));
            help.query = "zt".into();
            help.filter();
            assert_eq!(help.rows().len(), 1, "fixed Vim sequences stay documented");
        }
        shortcuts.overrides.clear();
        help.reset();
        help.configure(&shortcuts);
        help.query = "next page".into();
        help.filter();
        assert!(help.copy_text().contains("Next / previous page"));
        assert!(!help.copy_text().contains("X\tNext page"));
    }

    #[test]
    fn copy_uses_only_visible_rows_and_reopening_clears_the_filter() {
        let mut help = HelpFilter {
            query: "bookmark".into(),
            ..Default::default()
        };
        help.filter();
        let text = help.copy_text();
        assert!(text.starts_with("Pagewing keyboard shortcuts\n\n"));
        assert!(text.contains("Toggle bookmark"));
        assert!(!text.contains("Zoom in"));
        help.query = "jkgmissing".into();
        help.filter();
        assert_eq!(help.rows().len(), 0);
        help.reset();
        assert!(help.query.is_empty() && help.focus);
        assert_eq!(help.rows().len(), SHORTCUTS.len());
    }
}
