use super::{ACCENT, Pagewing};
use eframe::egui::{self, RichText, Vec2};
use std::fmt::Write;

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
    ("Alt + wheel / pinch", "Zoom at the pointer"),
    (
        "Ctrl/Cmd+Alt+Z / click zoom percentage",
        "Set an exact zoom percentage",
    ),
    ("p / w / e", "Fit page / width / height"),
    ("Shift+C", "Toggle continuous scrolling"),
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
    (
        "Ctrl/Cmd+Shift+t",
        "Reopen the most recently closed document in this session",
    ),
    ("Ctrl/Cmd+q", "Quit and save the latest reading position"),
    ("d / i", "Toggle dark interface / invert page"),
    (
        "Ctrl/Cmd+Shift+h",
        "Add a highlight comment to selected text",
    ),
    (
        "Ctrl/Cmd+Enter",
        "Save the current comment directly in the PDF",
    ),
    (
        "Ctrl/Cmd+Shift+m",
        "Read PDF comments; select text and use Add comment to annotate",
    ),
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
    (
        "Ctrl/Cmd+click PDF",
        "SyncTeX reverse search in the configured editor",
    ),
];

#[derive(Default)]
pub(super) struct HelpFilter {
    query: String,
    visible: Vec<usize>,
    catalog: Vec<(String, String)>,
    pub(super) focus: bool,
}

impl HelpFilter {
    fn reset(&mut self) {
        self.catalog = SHORTCUTS
            .iter()
            .map(|(keys, action)| ((*keys).into(), (*action).into()))
            .collect();
        self.query.clear();
        self.focus = true;
        self.filter();
    }

    fn filter(&mut self) {
        self.visible.clear();
        let query = self.query.to_ascii_lowercase();
        let words: Vec<_> = query.split_whitespace().collect();
        if self.catalog.is_empty() {
            self.catalog = SHORTCUTS
                .iter()
                .map(|(keys, action)| ((*keys).into(), (*action).into()))
                .collect();
        }
        self.visible
            .extend(
                self.catalog
                    .iter()
                    .enumerate()
                    .filter_map(|(index, (keys, action))| {
                        let text = format!("{keys} {action}").to_ascii_lowercase();
                        words
                            .iter()
                            .all(|word| text.contains(word))
                            .then_some(index)
                    }),
            );
    }

    fn configure(&mut self, shortcuts: &crate::settings::Shortcuts) {
        if shortcuts.overrides.is_empty() {
            return;
        }
        self.catalog = crate::settings::commands()
            .iter()
            .map(|command| (shortcuts.label(command.id), command.label.into()))
            .collect();
        // These interactions have their own grammar and remain fixed.
        for &(keys, action) in &[
            ("gg / 42gg / 42 Enter", "First page / counted page"),
            ("Backspace", "Edit count and cancel unfinished g/z command"),
            ("50%", "Jump to a percentage of the document's pages"),
            ("zt / zz / zb", "Align current page top / center / bottom"),
            (
                "zH / zL / z0 / z$",
                "Scroll half a screen / align horizontal edges",
            ),
            ("Alt + wheel / pinch", "Zoom at the pointer"),
            ("Click zoom percentage", "Set an exact zoom percentage"),
            (
                "Mouse back / forward",
                "Restore previous / next reading view",
            ),
            ("ma / 'a / `a", "Save / restore named position a–z"),
            ("'' / ``", "Toggle the last two jump positions"),
            ("Ctrl/Cmd+a / Ctrl/Cmd+c", "Select all / copy selected text"),
            (
                "Double / triple click in selection mode",
                "Select word / line",
            ),
            ("Ctrl/Cmd+Enter", "Save current comment"),
            ("Alt+c / Alt+w", "Match case / whole words (search open)"),
            ("Escape", "Close dialog / cancel current mode"),
            ("Right-click web link", "Copy link address"),
            ("Ctrl/Cmd+click PDF", "SyncTeX reverse search"),
        ] {
            self.catalog.push((keys.into(), action.into()));
        }
        self.filter();
    }

    fn rows(&self) -> impl ExactSizeIterator<Item = (&str, &str)> + '_ {
        self.visible.iter().map(|&index| {
            let (keys, action) = &self.catalog[index];
            (keys.as_str(), action.as_str())
        })
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
        self.goto_open = false;
        self.password_open = false;
        self.password_focus = false;
        self.password.clear();
        self.search_open = false;
        self.search_focus = false;
        self.pending_search_jump = None;
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
        let narrow = size.x < 480.0;
        let mut frame = egui::Frame::window(&context.style());
        if short || narrow {
            frame.inner_margin = 10.into();
        }
        egui::Window::new(if narrow {
            "Shortcuts"
        } else {
            "Keyboard shortcuts"
        })
        .id(egui::Id::new("Keyboard shortcuts"))
        .frame(frame)
        .open(&mut open)
        .collapsible(false)
        // Refresh the layout budget on every resize, rather than retaining
        // egui's previous narrow width or gradually growing its scroll area.
        .fixed_size(Vec2::new(
            (size.x - 64.0).clamp(100.0, 620.0),
            (size.y - 100.0).max(100.0),
        ))
        .anchor(egui::Align2::CENTER_CENTER, Vec2::ZERO)
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
            ui.weak(format!("{} shortcuts", self.help_filter.visible.len()));
            let stacked = ui.available_width() < 420.0;
            let column_width = (ui.available_width() - 24.0) * 0.5;
            egui::ScrollArea::vertical()
                .id_salt("pagewing-help-results")
                // Reserve room for the title, filter and actions outside
                // scrolling, including the larger frame on desktop.
                .max_height((size.y - if short { 180.0 } else { 300.0 }).max(32.0))
                .show(ui, |ui| {
                    if self.help_filter.visible.is_empty() {
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
                        !self.help_filter.visible.is_empty(),
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
        for size in [
            Vec2::new(1180.0, 820.0),
            Vec2::new(240.0, 240.0),
            Vec2::new(240.0, 400.0),
            Vec2::new(240.0, 420.0),
            Vec2::new(240.0, 820.0),
            Vec2::new(360.0, 540.0),
            Vec2::new(480.0, 420.0),
            Vec2::new(1180.0, 240.0),
            Vec2::new(1180.0, 420.0),
            Vec2::new(1180.0, 820.0),
        ] {
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
                for label in ["Copy shortcuts", "Close"] {
                    let (clip, text) = output
                        .shapes
                        .iter()
                        .find_map(|shape| match &shape.shape {
                            egui::Shape::Text(text) if text.galley.text() == label => {
                                Some((shape.clip_rect, text))
                            }
                            _ => None,
                        })
                        .unwrap_or_else(|| panic!("missing {label} at {size:?}"));
                    let bounds = egui::Rect::from_min_size(text.pos, text.galley.size());
                    assert!(
                        screen.contains_rect(bounds),
                        "{label}: {bounds:?} outside {screen:?}"
                    );
                    assert!(
                        clip.contains_rect(bounds),
                        "{label}: {bounds:?} clipped by {clip:?}"
                    );
                }
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
        shortcuts.overrides.insert(
            "next_page".into(),
            vec![crate::settings::Binding::from_event(
                egui::Key::X,
                egui::Modifiers::NONE,
                None,
            )],
        );
        let mut help = HelpFilter::default();
        help.reset();
        help.configure(&shortcuts);
        help.query = "next page".into();
        help.filter();
        assert_eq!(help.rows().collect::<Vec<_>>(), vec![("X", "Next page")]);
        assert!(help.copy_text().contains("X\tNext page"));
        help.query = "zt".into();
        help.filter();
        assert_eq!(help.rows().len(), 1, "fixed Vim sequences stay documented");
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
