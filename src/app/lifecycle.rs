//! Document lifetime transitions and cleanup of document-owned UI buffers.
use super::{Pagewing, comments};
use crate::{navigation::Navigation, render_plan::RenderPlanner, state::ReadingState};
use eframe::egui::{self, Vec2};
use std::path::PathBuf;

impl Pagewing {
    pub(super) fn open(&mut self, path: PathBuf, context: &egui::Context) {
        if self.comment_change_blocked(context) {
            return;
        }
        self.remember();
        self.clear_document(context);
        let path = std::fs::canonicalize(&path).unwrap_or(path);
        self.reading = self
            .library
            .recent
            .iter()
            .find(|item| item.path == path)
            .map(|item| item.as_ref().clone())
            .unwrap_or(ReadingState {
                path,
                ..Default::default()
            });
        if !self.library.display.remember_documents {
            self.reading.page = 0;
            self.reading.rotation = 0;
            self.reading.offset = [0.0; 2];
            self.library.display.apply(&mut self.reading);
        } else if !self
            .library
            .recent
            .iter()
            .any(|item| item.path == self.reading.path)
        {
            self.library.display.apply(&mut self.reading);
        }
        self.reading.rotation %= 4;
        self.synctex.server =
            crate::synctex_ipc::Server::bind(&self.reading.path, context.clone()).ok();
        self.pending_offset = Some(self.reading.offset);
        self.loading = true;
        self.worker
            .open(self.id, self.reading.path.clone(), String::new());
        context.send_viewport_cmd(egui::ViewportCommand::Title(format!(
            "{} — Pagewing",
            self.filename()
        )));
    }

    pub(super) fn clear_document(&mut self, context: &egui::Context) {
        self.synctex.clear_document();
        egui::Popup::close_all(context);
        self.id += 1;
        self.nav = Navigation::default();
        self.history = crate::history::ViewHistory::default();
        // Document-sized buffers should release their allocation at this
        // boundary; clear() would retain a large closed document's capacity.
        self.dimensions = Vec::new();
        self.continuous_layout = crate::continuous::Layout::default();
        self.clear_multi_page_buffers();
        self.continuous_link_request = None;
        self.info = crate::document_info::DocumentInfo::default();
        self.properties_open = false;
        self.zoom_input = None;
        self.comments = comments::CommentUi::default();
        self.outline.set_entries(Vec::new());
        self.bookmark_pages = Vec::new();
        self.labels = crate::page_labels::PageLabels::default();
        self.links.clear();
        self.visible_links = crate::link_visibility::LinkVisibility::default();
        self.hints.cancel();
        self.selection.cancel();
        self.mark_action = None;
        self.cache.clear();
        self.thumbnails.clear();
        self.reveal_page = None;
        self.reveal_page_section = false;
        self.requested = None;
        self.render_planner = RenderPlanner::default();
        self.pan = Vec2::ZERO;
        self.pending_offset = None;
        self.pending_focus = None;
        self.pending_destination = None;
        self.hits = Vec::new();
        self.result_text.clear();
        self.hit_index = None;
        self.reveal_hit = None;
        self.search_navigation.clear();
        self.query = String::new();
        self.search_due = None;
        self.search_open = false;
        self.search_focus = false;
        self.scanned = 0;
        self.scan_total = 0;
        self.search_complete = true;
        self.search_limited = false;
        self.revision += 1;
        self.reading = ReadingState::default();
        self.overflow = Vec2::ZERO;
        self.scale = 1.0;
        self.render_ms = 0.0;
        self.loading = false;
        self.reloading = false;
        self.file_changed = false;
        self.auto_reload_pending = false;
        self.automatic_reload = false;
        self.error = None;
        self.message = None;
        self.page_input = None;
        self.password_input = None;
        self.help = false;
        self.recent_picker = None;
        self.deferred_input = Vec::new();
        context.memory_mut(|memory| {
            if let Some(id) = memory.focused() {
                memory.surrender_focus(id);
            }
            memory.move_focus(egui::FocusDirection::None);
        });
    }

    pub(super) fn close_document(&mut self, context: &egui::Context) {
        if self.comment_change_blocked(context) {
            return;
        }
        if !self.reading.path.as_os_str().is_empty() {
            self.last_closed = Some(self.reading.path.clone());
        }
        self.save();
        self.clear_document(context);
        self.dialog = None;
        self.worker.close();
        context.send_viewport_cmd(egui::ViewportCommand::Title("Pagewing".into()));
    }

    pub(super) fn reopen_closed(&mut self, context: &egui::Context) {
        self.nav.clear_prefix();
        self.mark_action = None;
        // A draft or in-flight save postpones the document change. Retain
        // the target until a later shortcut can actually attempt to open it.
        if self.last_closed.is_some() && self.comment_change_blocked(context) {
            return;
        }
        if let Some(path) = self.last_closed.take() {
            self.open(path, context);
        } else {
            self.notify("No recently closed document");
        }
    }

    pub(super) fn reload(&mut self, context: &egui::Context) {
        if self.comment_change_blocked(context) {
            return;
        }
        if self.reading.path.as_os_str().is_empty() || self.loading {
            return;
        }
        self.auto_reload_pending = false;
        self.automatic_reload = false;
        self.remember();
        if let Some(message) = &mut self.message {
            message.recovery = None;
        }
        self.nav.clear_prefix();
        self.loading = true;
        self.reloading = self.nav.count > 0;
        self.error = None;
        self.hints.cancel();
        self.selection.cancel();
        self.search_navigation.clear();
        if self.reloading {
            self.worker
                .reload(self.id, self.reading.path.clone(), String::new());
        } else {
            self.worker
                .open(self.id, self.reading.path.clone(), String::new());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn blocked_reopen_retains_the_target_and_editor_until_the_change_is_allowed() {
        for saving in [false, true] {
            let directory = tempfile::tempdir().unwrap();
            let context = egui::Context::default();
            let mut app =
                Pagewing::with_context(&context, None, None, directory.path().join("library.json"));
            let current = directory.path().join("current.pdf");
            let target = directory.path().join("closed.pdf");
            app.reading.path = current.clone();
            app.nav = Navigation::new(1, 0);
            app.dimensions = vec![[600.0, 800.0]];
            app.comments.saving = saving;
            if !saving {
                app.comments.editor = Some(crate::annotations::Comment {
                    page: 0,
                    slot: None,
                    parent: None,
                    review: Default::default(),
                    marked: false,
                    states: Vec::new(),
                    kind: "Highlight".into(),
                    contents: "keep this draft 📝".into(),
                    author: "Pagewing".into(),
                    rects: Vec::new(),
                    has_appearance: false,
                    truncated: false,
                    locked: false,
                });
            }
            app.query = "keep search".into();
            let focus = egui::Id::new("pagewing-comment-input");
            context.memory_mut(|memory| memory.request_focus(focus));
            let reopen = |app: &mut Pagewing| {
                let modifiers = egui::Modifiers::COMMAND | egui::Modifiers::SHIFT;
                let mut events: Vec<_> = [true, false]
                    .into_iter()
                    .map(|pressed| egui::Event::Key {
                        key: egui::Key::T,
                        physical_key: None,
                        pressed,
                        repeat: false,
                        modifiers,
                    })
                    .collect();
                events.insert(1, egui::Event::Text("T".into()));
                let _ = context.run(
                    egui::RawInput {
                        events,
                        ..Default::default()
                    },
                    |context| {
                        app.keys(context);
                        egui::CentralPanel::default().show(context, |ui| {
                            let text = app
                                .comments
                                .editor
                                .as_mut()
                                .map_or(&mut app.query, |comment| &mut comment.contents);
                            ui.add(egui::TextEdit::multiline(text).id(focus));
                        });
                    },
                );
            };
            reopen(&mut app);
            assert!(
                app.message
                    .as_ref()
                    .unwrap()
                    .text
                    .contains("No recently closed")
            );
            assert!(
                !app.comments.open,
                "an empty target should not open comments"
            );
            app.last_closed = Some(target.clone());
            reopen(&mut app);
            assert_eq!(app.last_closed.as_ref(), Some(&target));
            assert_eq!(app.reading.path, current);
            assert!(!app.loading);
            assert!(app.comments.open);
            assert_eq!(app.query, "keep search");
            assert!(context.memory(|memory| memory.has_focus(focus)));
            if !saving {
                assert_eq!(
                    app.comments.editor.as_ref().unwrap().contents,
                    "keep this draft 📝"
                );
            }
            app.comments.saving = false;
            app.comments.editor = None;
            reopen(&mut app);
            assert!(app.loading);
            assert_eq!(app.reading.path, target);
            assert!(app.last_closed.is_none());
        }
    }

    #[test]
    fn closing_releases_document_sized_ui_buffers_and_keeps_reopen_state() {
        let directory = tempfile::tempdir().unwrap();
        let context = egui::Context::default();
        let mut app =
            Pagewing::with_context(&context, None, None, directory.path().join("library.json"));
        let path = directory.path().join("large-document.pdf");
        app.reading.path = path.clone();
        app.reading.bookmarks.insert(7);
        app.nav = Navigation::new(4096, 7);
        app.dimensions = vec![[600.0, 800.0]; 4096];
        app.bookmark_pages = (0..4096).collect();
        app.hits = (0..4096)
            .map(|page| crate::text::SearchHit {
                page,
                rects: Vec::new(),
                snippet: String::new(),
                snippet_match: 0..0,
            })
            .collect();
        app.query = "previous query ".repeat(2048);
        app.deferred_input = vec![egui::Event::Text("old input".into()); 64];
        app.visible_links.indices = (0..4096).collect();
        app.visible_links.rects = vec![egui::Rect::EVERYTHING; 4096];
        let dark = app.library.dark;
        context.memory_mut(|memory| memory.request_focus(egui::Id::new("pagewing-search-input")));
        app.close_document(&context);
        for (name, capacity) in [
            ("dimensions", app.dimensions.capacity()),
            ("bookmarks", app.bookmark_pages.capacity()),
            ("search hits", app.hits.capacity()),
            ("query", app.query.capacity()),
            ("deferred input", app.deferred_input.capacity()),
            ("link indices", app.visible_links.indices.capacity()),
            ("link rectangles", app.visible_links.rects.capacity()),
        ] {
            assert_eq!(capacity, 0, "{name} must release its old allocation");
        }
        assert_eq!(app.nav.count, 0);
        assert!(app.reading.path.as_os_str().is_empty());
        assert_eq!(app.library.dark, dark);
        assert!(context.memory(|memory| memory.focused().is_none()));
        assert_eq!(app.last_closed.as_ref(), Some(&path));
        assert_eq!(app.library.recent[0].page, 7);
        app.reopen_closed(&context);
        assert!(app.loading);
        assert_eq!(app.reading.path, path);
        assert_eq!(app.reading.page, 7);
        assert!(app.reading.bookmarks.contains(&7));
    }

    #[test]
    fn accepting_a_new_snapshot_releases_the_previous_link_overlay_allocation() {
        let directory = tempfile::tempdir().unwrap();
        let context = egui::Context::default();
        let mut app =
            Pagewing::with_context(&context, None, None, directory.path().join("library.json"));
        app.dimensions = vec![[600.0, 800.0]];
        app.nav = Navigation::new(1, 0);
        app.visible_links.indices = (0..4096).collect();
        app.visible_links.rects = vec![egui::Rect::EVERYTHING; 4096];
        app.reloading = true;
        app.loading = true;
        let (sender, receiver) = std::sync::mpsc::channel();
        app.worker.events = receiver;
        sender
            .send(crate::worker::Event::Opened {
                id: app.id,
                dimensions: vec![[600.0, 800.0]; 2],
                sync_geometry: Vec::new(),
                info: Default::default(),
                outline: Vec::new(),
                labels: Default::default(),
            })
            .unwrap();
        let _ = context.run(egui::RawInput::default(), |context| app.events(context));
        assert!(!app.loading && !app.reloading);
        assert_eq!(app.nav.count, 2);
        assert_eq!(app.visible_links.indices.capacity(), 0);
        assert_eq!(app.visible_links.rects.capacity(), 0);
    }
}
