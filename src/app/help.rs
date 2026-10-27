use super::{ACCENT, Folio};
use eframe::egui::{self, RichText, Vec2};
use std::{fmt::Write, sync::OnceLock};

const SHORTCUTS: &[(&str, &str)] = &[
    ("j / k / h / l", "Scroll down / up / left / right"),
    ("J / K   or   ] / [", "Next / previous page"),
    ("gg / G", "First / last page"),
    ("42G   or   42 Enter", "Go to page 42"),
    (
        "Backspace",
        "Remove the last count digit and cancel an unfinished g/z command",
    ),
    ("Ctrl+d / Ctrl+u", "Half screen down / up"),
    ("Space / Shift+Space", "Screen down / up"),
    ("50%", "Jump to a percentage of the document's pages"),
    (
        "zt / zz / zb",
        "Current page top / vertical center / bottom",
    ),
    (
        "zH / zL",
        "Scroll left / right half a screen; counts supported",
    ),
    (
        "z0 / z$",
        "Current page left / right edge; keep vertical position",
    ),
    (
        "3r / 3R",
        "Rotate clockwise / counterclockwise three quarter turns",
    ),
    (
        "Ctrl+e / Ctrl+y",
        "Scroll down / up without turning pages, optional count",
    ),
    ("Alt+Left / Alt+Right", "Jump history back / forward"),
    ("+ / −  ·  Ctrl/Cmd++ / −", "Zoom in / out"),
    (
        "3+ / 3−   or   4=",
        "Zoom by a count, preserving the viewport center",
    ),
    ("p / w / e", "Fit page / width / height"),
    ("r / Shift+r", "Rotate clockwise / counterclockwise"),
    (
        "Mouse back / forward",
        "Restore the previous / next reading view",
    ),
    ("Alt+j / Alt+k", "Next / previous bookmark, optional count"),
    ("a / Ctrl/Cmd+0", "100% zoom, keeping the viewport center"),
    ("Ctrl/Cmd+p", "Find a recent document"),
    ("/   or   Ctrl/Cmd+f", "Search document"),
    (
        "n / N  ·  3n / 3N",
        "Next / previous search match, optional count",
    ),
    ("Alt+c / Alt+w", "Match case / whole words (search open)"),
    ("b", "Toggle bookmark"),
    ("Tab", "Toggle sidebar"),
    ("ma   /   'a or `a", "Save / restore named position a–z"),
    (
        "''   or   ``",
        "Toggle the last two jump positions, including the full view",
    ),
    ("t", "Toggle page previews and open sidebar"),
    (
        "c",
        "Find chapter by keywords · ↑/↓ or Ctrl+j/k · Enter to follow",
    ),
    (
        "v, drag, then y",
        "Select and copy text (Ctrl/Cmd+C also works)",
    ),
    ("Ctrl/Cmd+a", "Select all text on the current page"),
    (
        "Double / triple click in v",
        "Select word / line; drag in those units, auto-scroll at edges",
    ),
    ("Ctrl/Cmd+r", "Reload PDF and keep reading position"),
    ("s, then y", "Document information, copy details"),
    ("Ctrl/Cmd+w", "Close PDF and save reading position"),
    ("Ctrl/Cmd+q", "Quit and save the latest reading position"),
    ("d / i", "Toggle dark interface / invert page"),
    ("y", "Copy page text"),
    ("o   or   Ctrl/Cmd+o", "Open PDF"),
    (":", "Go to page dialog"),
    (
        ":+5 Enter / :-5 Enter",
        "Jump forward / back five physical pages; clamp at document edges",
    ),
    ("f, then a label", "Follow a visible PDF link"),
    ("F, then a label", "Copy a visible web-link address"),
    ("F11", "Toggle fullscreen"),
    ("? / F1 / Escape", "Find keyboard shortcuts / close dialog"),
    ("Right-click web link", "Copy link address"),
];

#[derive(Default)]
pub(super) struct HelpFilter {
    query: String,
    visible: Vec<usize>,
    pub(super) focus: bool,
}

impl HelpFilter {
    fn reset(&mut self) {
        self.query.clear();
        self.focus = true;
        self.filter();
    }

    fn filter(&mut self) {
        self.visible.clear();
        let query = self.query.to_ascii_lowercase();
        let words: Vec<_> = query.split_whitespace().collect();
        if words.is_empty() {
            self.visible.extend(0..SHORTCUTS.len());
            return;
        }
        // The shortcut catalog is static. Normalize it once, then only rebuild
        // the visible indices when the query changes.
        static FOLDED: OnceLock<Vec<String>> = OnceLock::new();
        let folded = FOLDED.get_or_init(|| {
            SHORTCUTS
                .iter()
                .map(|(keys, action)| format!("{keys} {action}").to_ascii_lowercase())
                .collect()
        });
        self.visible
            .extend(folded.iter().enumerate().filter_map(|(index, text)| {
                words
                    .iter()
                    .all(|word| text.contains(word))
                    .then_some(index)
            }));
    }

    fn rows(&self) -> impl ExactSizeIterator<Item = (&'static str, &'static str)> + '_ {
        self.visible.iter().map(|&index| SHORTCUTS[index])
    }

    fn copy_text(&self) -> String {
        let mut text = String::from("Folio keyboard shortcuts\n\n");
        for (keys, action) in self.rows() {
            let _ = writeln!(text, "{keys}\t{action}");
        }
        text
    }
}

impl Folio {
    pub(super) fn open_help(&mut self, context: &egui::Context) {
        self.outline.cancel_input();
        self.nav.clear_prefix();
        self.hints.cancel();
        self.selection.cancel();
        self.mark_action = None;
        self.recent_picker = None;
        self.properties_open = false;
        self.goto_open = false;
        self.password_open = false;
        self.password_focus = false;
        self.password.clear();
        self.search_open = false;
        self.search_focus = false;
        self.pending_search_jump = None;
        self.help = true;
        self.help_filter.reset();
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
        context.memory_mut(|memory| memory.surrender_focus(egui::Id::new("folio-help-input")));
    }

    pub(super) fn help_window(&mut self, context: &egui::Context) {
        if !self.help {
            return;
        }
        let mut open = true;
        let mut close = false;
        let mut copied = false;
        egui::Window::new("Keyboard shortcuts")
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .default_width(620.0)
            .max_width((context.content_rect().width() - 60.0).max(280.0))
            .anchor(egui::Align2::CENTER_CENTER, Vec2::ZERO)
            .show(context, |ui| {
                ui.label(
                    RichText::new("Stay in the flow.")
                        .size(23.0)
                        .strong()
                        .color(ACCENT),
                );
                ui.add_space(8.0);
                let (input, _) = super::shortcuts::text_input(
                    ui,
                    egui::Id::new("folio-help-input"),
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
                ui.weak(format!(
                    "{} shortcuts · Escape to close",
                    self.help_filter.visible.len()
                ));
                let column_width = ((ui.available_width() - 24.0) * 0.5).max(100.0);
                egui::ScrollArea::vertical()
                    .id_salt("folio-help-results")
                    .max_height((context.content_rect().height() - 245.0).max(100.0))
                    .show(ui, |ui| {
                        if self.help_filter.visible.is_empty() {
                            ui.label("No matching shortcuts.");
                        }
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
                    });
                ui.separator();
                ui.horizontal(|ui| {
                    if ui
                        .add_enabled(
                            !self.help_filter.visible.is_empty(),
                            egui::Button::new("Copy shortcuts"),
                        )
                        .clicked()
                    {
                        context.copy_text(self.help_filter.copy_text());
                        copied = true;
                        input.request_focus();
                    }
                    close = ui.button("Close").clicked();
                });
                ui.weak(
                    "Counts work with movement: 5j, 3J, 12gg. Drag to pan. Double-click to fit.",
                );
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
    fn filters_keys_and_descriptions_with_all_case_insensitive_tokens() {
        let mut help = HelpFilter::default();
        help.reset();
        assert_eq!(help.rows().len(), SHORTCUTS.len());
        help.query = "  COPY  LINK  ".into();
        help.filter();
        let rows: Vec<_> = help.rows().collect();
        assert_eq!(rows.len(), 2);
        assert!(rows.iter().any(|(keys, _)| *keys == "F, then a label"));
        assert!(rows.iter().any(|(keys, _)| *keys == "Right-click web link"));
        help.query = "zoom 0".into();
        help.filter();
        assert_eq!(help.rows().next().unwrap().0, "a / Ctrl/Cmd+0");
        assert_eq!(help.rows().len(), 1);
    }

    #[test]
    fn copy_uses_only_visible_rows_and_reopening_clears_the_filter() {
        let mut help = HelpFilter {
            query: "bookmark".into(),
            ..Default::default()
        };
        help.filter();
        let text = help.copy_text();
        assert!(text.starts_with("Folio keyboard shortcuts\n\n"));
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
