mod appearance;
mod chapters;
mod comments;
#[cfg(test)]
mod config;
mod continuous;
mod copy_menu;
mod dialog;
mod errors;
mod focus_input;
mod help;
mod history_controls;
mod input_dialogs;
mod keyboard;
mod lifecycle;
mod list_focus;
mod menu_focus;
mod properties;
mod reading_controls;
mod recent;
mod reload;
mod search;
mod selection_scroll;
mod shortcuts;
mod sidebar;
mod spread;
mod status;
mod sync;
mod tiles;
mod toolbar;
mod tooltips;
mod view_menu;
mod viewport;
mod welcome;
mod zoom;

use crate::{
    cache::TextureCache,
    link_hints::LinkHints,
    links::LinkTarget,
    navigation::{Fit, Navigation},
    render_plan::{PlanMode, RenderPlan, RenderPlanner, RenderView},
    selection::{SelectionUnit, TextSelection},
    state::{self, Library, ReadingMark, ReadingState},
    text::SearchHit,
    worker::{Event, Worker},
};
use eframe::egui::{self, Color32, Vec2};
use std::{
    path::PathBuf,
    sync::{
        Arc,
        mpsc::{self, Receiver},
    },
    time::{Duration, Instant},
};

const ACCENT: Color32 = Color32::from_rgb(41, 114, 92);
const SELECTION_COLOR: Color32 = Color32::from_rgba_unmultiplied_const(74, 145, 230, 95);
const SEARCH_COLOR: Color32 = Color32::from_rgba_unmultiplied_const(255, 215, 40, 85);
const CURRENT_SEARCH_COLOR: Color32 = Color32::from_rgba_unmultiplied_const(255, 143, 36, 120);

#[derive(Clone, Copy)]
enum MarkAction {
    Set,
    Jump,
}

pub struct Pagewing {
    #[cfg(target_os = "macos")]
    open_documents: Option<crate::macos_open::OpenDocuments>,
    worker: Worker,
    state_writer: crate::state_writer::StateWriter,
    library: Library,
    page_colors: crate::page_colors::PageColors,
    text_embolden: f32,
    text_embolden_step: f64,
    appearance_revision: u64,
    reading: ReadingState,
    last_closed: Option<PathBuf>,
    bookmark_pages: Vec<usize>,
    nav: Navigation,
    history: crate::history::ViewHistory,
    dimensions: Vec<[f32; 2]>,
    info: crate::document_info::DocumentInfo,
    properties_open: bool,
    comments: comments::CommentUi,
    synctex: sync::SyncUi,
    outline: crate::outline_view::OutlineView,
    sidebar_region: Option<(egui::LayerId, egui::Rect)>,
    toolbar_region: Option<(egui::LayerId, egui::Rect)>,
    search_region: Option<(egui::LayerId, egui::Rect)>,
    status_region: Option<(egui::LayerId, egui::Rect)>,
    labels: crate::page_labels::PageLabels,
    links: crate::link_cache::LinkCache,
    visible_links: crate::visible_rects::VisibleRects,
    other_visible_links: crate::visible_rects::VisibleRects,
    hints: LinkHints,
    copy_link_hints: bool,
    mark_action: Option<MarkAction>,
    pending_offset: Option<[f32; 2]>,
    pending_focus: Option<hayro::kurbo::Rect>,
    pending_destination: Option<(crate::destination::Destination, [f64; 2])>,
    selection: TextSelection,
    selection_page: usize,
    continuous_layout: crate::continuous::Layout,
    continuous_geometry: crate::page_geometry::PageGeometry,
    continuous_link_request: Option<usize>,
    cache: TextureCache,
    thumbnails: crate::thumbnails::Thumbnails,
    reveal_page: Option<usize>,
    reveal_page_section: bool,
    id: u64,
    loading: bool,
    reloading: bool,
    file_changed: bool,
    auto_reload_pending: bool,
    automatic_reload: bool,
    error: Option<errors::Notice>,
    configuration_error: Option<String>,
    message: Option<status::Notice>,
    pan: Vec2,
    overflow: Vec2,
    viewport: Vec2,
    scale: f32,
    render_planner: RenderPlanner,
    requested: Option<Arc<RenderPlan>>,
    render_ms: f64,
    dialog: Option<Receiver<Option<PathBuf>>>,
    deferred_input: Vec<egui::Event>,
    focus_input: std::collections::VecDeque<egui::Event>,
    reader_actions: Vec<crate::input_actions::ReaderAction>,
    search_open: bool,
    search_focus: bool,
    copy_search_requested: bool,
    search_run_requested: Option<search::RunAction>,
    query: String,
    query_history: crate::query_history::QueryHistory,
    search_due: Option<Instant>,
    revision: u64,
    hits: Vec<SearchHit>,
    search_highlights: crate::search_highlights::SearchHighlights,
    result_text: crate::search_result_text::ResultTextCache,
    hit_index: Option<usize>,
    reveal_hit: Option<usize>,
    search_navigation: crate::search_navigation::SearchNavigation,
    scanned: usize,
    scan_total: usize,
    search_complete: bool,
    search_limited: bool,
    search_stopped: bool,
    page_input: Option<input_dialogs::PageInput>,
    zoom_input: Option<zoom::ZoomInput>,
    help: bool,
    help_filter: help::HelpFilter,
    input_buttons: shortcuts::InputButtons,
    controls_hidden: bool,
    recent_picker: Option<crate::recent::RecentPicker>,
    password_input: Option<input_dialogs::PasswordInput>,
    saved: Instant,
    dirty: bool,
}

impl Pagewing {
    #[cfg(target_os = "macos")]
    pub fn set_open_documents(
        &mut self,
        documents: crate::macos_open::OpenDocuments,
        context: &egui::Context,
    ) {
        documents.attach(context);
        self.open_documents = Some(documents);
    }

    #[cfg(target_os = "macos")]
    fn macos_open_documents(&mut self, context: &egui::Context) {
        let paths = self
            .open_documents
            .as_ref()
            .map(|documents| documents.take_paths())
            .unwrap_or_default();
        let mut paths = paths.into_iter();
        if let Some(path) = paths.next() {
            self.open(path, context);
            context.send_viewport_cmd(egui::ViewportCommand::Focus);
        }
        // Each reader holds one document. Preserve every additional selection
        // from a Finder batch in its own reader process/window.
        for path in paths {
            let result = std::env::current_exe()
                .and_then(|exe| std::process::Command::new(exe).arg("--").arg(&path).spawn());
            match result {
                Ok(mut child) => {
                    std::thread::spawn(move || {
                        let _ = child.wait();
                    });
                }
                Err(error) => self.notify(format!("Cannot open {}: {error}", path.display())),
            }
        }
    }

    pub fn new(
        cc: &eframe::CreationContext<'_>,
        path: Option<PathBuf>,
        page: Option<usize>,
    ) -> Self {
        Self::with_configuration(
            &cc.egui_ctx,
            path,
            page,
            state::config_path(),
            crate::config::path(),
        )
    }

    #[cfg(test)]
    fn with_context(
        context: &egui::Context,
        path: Option<PathBuf>,
        page: Option<usize>,
        config: PathBuf,
    ) -> Self {
        let preferences = config.with_file_name("config.toml");
        Self::with_configuration(context, path, page, config, preferences)
    }

    fn with_configuration(
        context: &egui::Context,
        path: Option<PathBuf>,
        page: Option<usize>,
        state_path: PathBuf,
        preferences: PathBuf,
    ) -> Self {
        let mut library = Library::load(&state_path);
        let config = crate::config::load_or_create(&preferences, &library);
        let config_error = config.as_ref().err().cloned();
        let config = config.unwrap_or_default();
        config.apply(&mut library);
        let page_colors = config.page_colors();
        set_theme(context, library.dark);
        shortcuts::configure_input(context);
        crate::fonts::install(context.clone());
        let mut app = Self {
            #[cfg(target_os = "macos")]
            open_documents: None,
            worker: Worker::with_appearance(context.clone(), page_colors, config.render_settings()),
            page_colors,
            text_embolden: config.render_settings().text_embolden,
            text_embolden_step: config.text_embolden_step(),
            appearance_revision: 0,
            state_writer: crate::state_writer::StateWriter::new(state_path, context.clone()),
            library,
            reading: ReadingState::default(),
            last_closed: None,
            bookmark_pages: Vec::new(),
            nav: Navigation::default(),
            history: crate::history::ViewHistory::default(),
            dimensions: Vec::new(),
            info: crate::document_info::DocumentInfo::default(),
            properties_open: false,
            comments: comments::CommentUi::default(),
            synctex: sync::SyncUi::default(),
            outline: crate::outline_view::OutlineView::default(),
            sidebar_region: None,
            toolbar_region: None,
            search_region: None,
            status_region: None,
            labels: crate::page_labels::PageLabels::default(),
            links: crate::link_cache::LinkCache::default(),
            visible_links: crate::visible_rects::VisibleRects::default(),
            other_visible_links: crate::visible_rects::VisibleRects::default(),
            hints: LinkHints::default(),
            copy_link_hints: false,
            mark_action: None,
            pending_offset: None,
            pending_focus: None,
            pending_destination: None,
            selection: TextSelection::default(),
            selection_page: 0,
            continuous_layout: crate::continuous::Layout::default(),
            continuous_geometry: crate::page_geometry::PageGeometry::default(),
            continuous_link_request: None,
            cache: TextureCache::default(),
            thumbnails: crate::thumbnails::Thumbnails::default(),
            reveal_page: None,
            reveal_page_section: false,
            id: 0,
            loading: false,
            reloading: false,
            file_changed: false,
            auto_reload_pending: false,
            automatic_reload: false,
            error: None,
            configuration_error: config_error.clone(),
            message: None,
            pan: Vec2::ZERO,
            overflow: Vec2::ZERO,
            viewport: Vec2::splat(600.0),
            scale: 1.0,
            render_planner: RenderPlanner::default(),
            requested: None,
            render_ms: 0.0,
            dialog: None,
            deferred_input: Vec::new(),
            focus_input: Default::default(),
            reader_actions: Vec::new(),
            search_open: false,
            search_focus: false,
            copy_search_requested: false,
            search_run_requested: None,
            query: String::new(),
            query_history: crate::query_history::QueryHistory::default(),
            search_due: None,
            revision: 0,
            hits: Vec::new(),
            search_highlights: crate::search_highlights::SearchHighlights::default(),
            result_text: crate::search_result_text::ResultTextCache::default(),
            hit_index: None,
            reveal_hit: None,
            search_navigation: Default::default(),
            scanned: 0,
            scan_total: 0,
            search_complete: true,
            search_limited: false,
            search_stopped: false,
            page_input: None,
            zoom_input: None,
            help: false,
            help_filter: help::HelpFilter::default(),
            input_buttons: shortcuts::InputButtons::default(),
            controls_hidden: false,
            recent_picker: None,
            password_input: None,
            saved: Instant::now(),
            dirty: false,
        };
        if let Some(path) = path {
            app.open(path, context);
            if let Some(page) = page {
                app.reading.page = page;
                app.reading.offset = [0.0; 2];
                app.pending_offset = Some([0.0; 2]);
            }
        }
        if let Some(error) = config_error {
            eprintln!("{error}");
        }
        app
    }

    fn filename(&self) -> String {
        self.reading
            .path
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_else(|| "Pagewing".into())
    }

    fn page_name(&self, page: usize) -> String {
        self.labels.display_name(page)
    }

    fn remember(&mut self) {
        if self.nav.count > 0 {
            let position = self.current_position();
            self.reading.page = position.page;
            self.reading.offset = position.offset;
            self.library.remember(self.reading.clone());
        }
    }

    fn current_offset(&self) -> [f32; 2] {
        state::normalized_offset([self.pan.x, self.pan.y], [self.overflow.x, self.overflow.y])
    }

    fn current_position(&mut self) -> ReadingMark {
        self.resolve_view();
        ReadingMark {
            page: self.nav.page,
            fit: self.reading.fit,
            rotation: self.reading.rotation,
            continuous: self.reading.continuous,
            facing: self.reading.facing,
            offset: self.current_offset(),
        }
    }

    fn restore_position(&mut self, position: ReadingMark, record: bool) {
        if record {
            let current = self.current_position();
            self.history.record(current);
        }
        self.nav.jump(position.page);
        self.page_changed();
        self.reading.fit = position.fit;
        self.reading.rotation = position.rotation % 4;
        self.reading.continuous = position.continuous;
        self.reading.facing = position.facing;
        if !position.continuous && !position.facing {
            self.clear_multi_page_buffers();
        }
        self.pending_offset = Some(position.offset);
    }

    fn history(&mut self, forward: bool) -> bool {
        let current = self.current_position();
        if let Some(position) = self.history.go(forward, current, self.nav.count) {
            self.restore_position(position, false);
            true
        } else {
            false
        }
    }

    fn mark(&mut self, name: char, action: MarkAction) {
        if self.nav.count == 0 {
            return;
        }
        match action {
            MarkAction::Set => {
                let position = self.current_position();
                self.reading.marks.insert(name, position);
                self.dirty = true;
                self.notify(format!("Mark {name} saved · page {}", self.nav.page + 1));
            }
            MarkAction::Jump => {
                if let Some(mark) = self.reading.marks.get(&name).cloned() {
                    self.restore_position(mark, true);
                } else {
                    self.notify(format!("Mark {name} is not set · use m{name} to save it"));
                }
            }
        }
    }

    fn save(&mut self) {
        self.remember();
        self.state_writer.save(self.library.clone());
        self.saved = Instant::now();
        self.dirty = false;
    }

    fn notify(&mut self, text: impl Into<String>) {
        self.message = Some(status::Notice::new(text));
    }

    fn choose_file(&mut self) {
        self.recent_picker = None;
        self.nav.clear_prefix();
        if self.dialog.is_some() {
            return;
        }
        let (tx, rx) = mpsc::channel();
        self.dialog = Some(rx);
        std::thread::spawn(move || {
            let _ = tx.send(
                rfd::FileDialog::new()
                    .add_filter("PDF documents", &["pdf"])
                    .pick_file(),
            );
        });
    }

    fn jump(&mut self, page: usize) {
        let current = self.current_position();
        self.pending_destination = None;
        if self.nav.jump(page) || (self.reading.continuous && self.pan.y > 0.0) {
            self.history.record(current);
            self.page_changed();
        }
        // Explicit navigation can reveal the current page even when the PDF
        // page did not change, e.g. gg after scrolling only the sidebar.
        self.reveal_page = Some(self.nav.page);
    }
    fn step(&mut self, delta: isize) {
        let current = self.current_position();
        self.pending_destination = None;
        let delta = if self.reading.facing {
            let start = self.nav.page / 2 * 2;
            let target = start
                .saturating_add_signed(delta.saturating_mul(2))
                .min(self.nav.count.saturating_sub(1) / 2 * 2);
            if target == start {
                0
            } else {
                target as isize - self.nav.page as isize
            }
        } else {
            delta
        };
        if self.nav.step(delta) {
            self.history.record(current);
            self.page_changed();
        }
    }
    fn page_changed(&mut self) {
        self.links.set_page(self.nav.page);
        self.reveal_page = Some(self.nav.page);
        self.pending_destination = None;
        self.nav.clear_prefix();
        self.hints.cancel();
        self.selection.cancel();
        self.visible_links.clear();
        self.pan = Vec2::ZERO;
        self.pending_offset = None;
        self.pending_focus = None;
        self.reading.offset = [0.0; 2];
        self.search_navigation.clear();
        self.requested = None;
        self.dirty = true;
    }

    fn scroll(&mut self, delta: Vec2, turn_at_edge: bool) {
        if delta == Vec2::ZERO || self.resolve_view().is_none() {
            return;
        }
        self.hints.cancel();
        if self.reading.continuous {
            self.scroll_continuous(delta);
            return;
        }
        let old = self.pan;
        self.pan = (self.pan + delta).clamp(Vec2::ZERO, self.overflow);
        if turn_at_edge && (self.pan.y - old.y).abs() < 0.1 && delta.y.abs() > 0.0 {
            let previous_page = self.nav.page;
            self.step(if delta.y > 0.0 { 1 } else { -1 });
            if delta.y < 0.0 && self.nav.page != previous_page {
                self.pan.y = f32::MAX;
            }
        }
    }

    fn rotate_page(&mut self, backwards: bool, count: usize) {
        // Consume saved/search/PDF positions before the explicit pan reset.
        self.resolve_view();
        self.nav.clear_prefix();
        let turns = (count % 4) as u8;
        self.reading.rotation =
            (self.reading.rotation + if backwards { 4 - turns } else { turns }) % 4;
        self.pan = Vec2::ZERO;
        self.dirty = true;
    }

    fn zoom(&mut self, factor: f32) {
        self.zoom_at(factor, self.viewport * 0.5);
    }

    fn zoom_steps(&mut self, count: usize, out: bool) {
        let Some(dimensions) = self.resolve_view() else {
            return;
        };
        let target = crate::navigation::zoom_target(self.scale, count, out);
        self.apply_zoom(target, dimensions, self.viewport * 0.5);
    }

    fn align_axis(&mut self, axis: usize, fraction: f32) {
        // Apply any saved/search/PDF position before changing only one axis.
        if self.resolve_view().is_none() {
            return;
        }
        self.hints.cancel();
        self.search_navigation.clear();
        let mut offset = self.current_offset();
        offset[axis] = fraction;
        if self.reading.continuous && axis == 1 && self.overflow.y > 0.0 {
            let height = self.continuous_layout.pages[self.nav.page].size[1];
            offset[1] = (height - self.viewport.y).max(0.0) / self.overflow.y * fraction;
        }
        // Apply after this frame's zoom/rotation has established its overflow.
        self.pending_offset = Some(offset);
        self.dirty = true;
    }

    fn zoom_at(&mut self, factor: f32, pivot: Vec2) {
        if !factor.is_finite() || factor <= 0.0 {
            return;
        }
        let Some(dimensions) = self.resolve_view() else {
            return;
        };
        self.apply_zoom(self.scale * factor, dimensions, pivot);
    }

    fn actual_size(&mut self) {
        self.zoom_to(1.0);
    }

    fn zoom_to(&mut self, scale: f32) {
        let Some(dimensions) = self.resolve_view() else {
            return;
        };
        self.apply_zoom(scale, dimensions, self.viewport * 0.5);
    }

    fn apply_zoom(&mut self, target: f32, dimensions: [f32; 2], pivot: Vec2) {
        self.hints.cancel();
        let viewport = [self.viewport.x, self.viewport.y];
        let after = Fit::Custom(target).scale(dimensions, viewport);
        if self.reading.continuous {
            self.zoom_continuous(after, pivot);
            return;
        }
        if self.reading.facing {
            self.zoom_spread(target, pivot);
            return;
        }
        let before = self.scale;
        let pan = crate::geometry::zoom_pan(
            [self.pan.x, self.pan.y],
            dimensions,
            viewport,
            before,
            after,
            [pivot.x, pivot.y],
        );
        self.pan = Vec2::new(pan[0], pan[1]);
        self.reading.fit = Fit::Custom(after);
        self.dirty = true;
    }

    /// Resolve this command's current geometry and pending position before the
    /// next command uses pan/overflow, including commands queued in one frame.
    fn resolve_view(&mut self) -> Option<[f32; 2]> {
        let &dimensions = self.dimensions.get(self.nav.page)?;
        let rotated = if self.reading.rotation % 2 == 1 {
            [dimensions[1], dimensions[0]]
        } else {
            dimensions
        };
        self.scale = self
            .reading
            .fit
            .scale(rotated, [self.viewport.x, self.viewport.y]);
        self.overflow = (Vec2::from(rotated) * self.scale - self.viewport).max(Vec2::ZERO);
        if self.reading.continuous {
            self.update_continuous_layout();
            self.overflow.y = self.continuous_layout.limit(self.nav.page);
        }
        if self.reading.facing {
            let spread = self.spread();
            self.scale = spread.scale;
            self.overflow = (spread.size - self.viewport).max(Vec2::ZERO);
        }
        if let Some(offset) = self.pending_offset.take() {
            self.pan =
                Vec2::new(offset[0].clamp(0.0, 1.0), offset[1].clamp(0.0, 1.0)) * self.overflow;
        }
        if let Some(focus) = self.pending_focus.take() {
            let (focus, rotation) = if self.reading.facing {
                let offset = self.spread().offset(self.nav.page) / self.scale;
                (
                    crate::geometry::rotate_rect(focus, dimensions, self.reading.rotation)
                        + hayro::kurbo::Vec2::new(f64::from(offset.x), f64::from(offset.y)),
                    0,
                )
            } else {
                (focus, self.reading.rotation)
            };
            self.pan = Vec2::from(crate::geometry::focus_offset(
                focus,
                dimensions,
                rotation,
                self.scale,
                [self.viewport.x, self.viewport.y],
                [self.overflow.x, self.overflow.y],
            ));
        }
        if let Some((mut destination, origin)) = self.pending_destination.take() {
            let rotation = if self.reading.facing {
                let offset = self.spread().offset(self.nav.page) / self.scale;
                destination.position = crate::destination::rotate_position(
                    destination.position,
                    dimensions,
                    self.reading.rotation,
                );
                for (coordinate, offset) in
                    destination.position.iter_mut().zip([offset.x, offset.y])
                {
                    *coordinate = coordinate.map(|value| value + f64::from(offset));
                }
                if let Some(crate::destination::DestinationFit::Rectangle(rect)) = destination.fit {
                    destination.fit = Some(crate::destination::DestinationFit::Rectangle(
                        crate::geometry::rotate_rect(rect, dimensions, self.reading.rotation)
                            + hayro::kurbo::Vec2::new(f64::from(offset.x), f64::from(offset.y)),
                    ));
                }
                0
            } else {
                self.reading.rotation
            };
            self.pan = Vec2::from(destination.pan(
                dimensions,
                rotation,
                self.scale,
                [self.viewport.x, self.viewport.y],
                origin,
                [self.overflow.x, self.overflow.y],
            ));
        }
        self.pan = self.pan.clamp(Vec2::ZERO, self.overflow);
        Some(rotated)
    }

    fn bookmark(&mut self) {
        if self.nav.count == 0 {
            return;
        }
        let present = !self.reading.bookmarks.contains(&self.nav.page);
        if present {
            self.set_bookmark(self.nav.page, true);
            self.notify("Bookmark added");
        } else {
            self.remove_bookmark(self.nav.page);
        }
        self.reveal_page = Some(self.nav.page);
    }

    fn set_bookmark(&mut self, page: usize, present: bool) -> bool {
        let changed = crate::bookmark_index::set(
            &mut self.reading.bookmarks,
            &mut self.bookmark_pages,
            page,
            present,
        );
        if changed {
            if let Some(message) = &mut self.message {
                message.cancel_recovery();
            }
            self.dirty = true;
        }
        changed
    }

    fn remove_bookmark(&mut self, page: usize) {
        if !self.set_bookmark(page, false) {
            return;
        }
        let mut notice = status::Notice::new("Bookmark removed");
        notice.recovery = (!self.loading).then_some(status::Recovery::Bookmark(page));
        self.message = Some(notice);
    }

    fn remove_mark(&mut self, name: char) {
        let Some(position) = self.reading.marks.remove(&name) else {
            return;
        };
        self.dirty = true;
        let mut notice = status::Notice::new(format!("Mark {name} removed"));
        notice.recovery = (!self.loading).then_some(status::Recovery::Mark { name, position });
        self.message = Some(notice);
    }

    fn refresh_bookmarks(&mut self) {
        self.bookmark_pages.clear();
        self.bookmark_pages
            .extend(self.reading.bookmarks.iter().copied());
    }

    fn events(&mut self, context: &egui::Context) {
        self.begin_comment_input(context);
        if let Some(Err(error)) = self.state_writer.result() {
            self.notify(format!("Could not save reading progress: {error}"));
            self.dirty = true;
        }
        while let Ok(event) = self.worker.events.try_recv() {
            match event {
                Event::Comments { id, comments } if id == self.id => {
                    self.comments.set_comments(comments);
                }
                Event::CommentChanged {
                    id,
                    operation,
                    result,
                } if id == self.id => {
                    self.comments.saving = false;
                    match result {
                        Ok(()) => {
                            self.comments.editor = None;
                            self.reload(context);
                            self.notify(match operation {
                                crate::annotations::Operation::Save => "Comment saved in PDF.",
                                crate::annotations::Operation::Delete => {
                                    "Comment and highlight deleted from PDF."
                                }
                            });
                        }
                        Err(error) => self.error = Some(error.into()),
                    }
                }
                Event::FileChanged { id } if id == self.id => {
                    self.file_changed = true;
                    self.auto_reload_pending = true;
                }
                Event::PageText { id, page, text }
                    if id == self.id && page == self.selection_page && self.selection.active =>
                {
                    if text.spans.is_empty() {
                        self.notify("This page has no selectable text");
                    }
                    self.selection.set_text(text);
                }
                Event::Links { id, page, links } if id == self.id => {
                    if self.continuous_link_request == Some(page) {
                        self.continuous_link_request = None;
                    }
                    self.links.insert(page, links, self.nav.page);
                    if page == self.nav.page {
                        self.visible_links.invalidate();
                    }
                }
                Event::Opened {
                    id,
                    dimensions,
                    sync_geometry,
                    info,
                    outline,
                    labels,
                } if id == self.id => {
                    let reloaded = self.reloading;
                    if reloaded {
                        self.reading.page = self.nav.page;
                        self.pending_offset = Some(self.current_offset());
                    }
                    self.reloading = false;
                    self.file_changed = false;
                    self.auto_reload_pending = false;
                    self.automatic_reload = false;
                    self.loading = false;
                    self.error = None;
                    self.close_zoom(context);
                    self.cache.clear();
                    self.thumbnails.clear();
                    self.requested = None;
                    self.render_planner = RenderPlanner::default();
                    self.links.clear();
                    self.visible_links = crate::visible_rects::VisibleRects::default();
                    self.hints.cancel();
                    self.selection.cancel();
                    self.pending_focus = None;
                    if reloaded {
                        self.nav.resize(dimensions.len());
                        self.history.retain_pages(dimensions.len());
                    } else {
                        self.nav = Navigation::new(dimensions.len(), self.reading.page);
                    }
                    self.reading
                        .bookmarks
                        .retain(|page| *page < dimensions.len());
                    self.refresh_bookmarks();
                    self.reading.marks.retain(|name, mark| {
                        name.is_ascii_lowercase() && mark.page < dimensions.len()
                    });
                    self.dimensions = dimensions;
                    self.synctex.geometry = sync_geometry;
                    self.continuous_layout = crate::continuous::Layout::default();
                    self.clear_multi_page_buffers();
                    self.continuous_link_request = None;
                    self.info = info;
                    self.comments.reset(reloaded);
                    self.worker.comments();
                    self.reveal_page = Some(self.nav.page);
                    self.reveal_page_section = self.library.thumbnails;
                    self.outline = *outline;
                    self.labels = labels;
                    if let Some(input) = &mut self.page_input {
                        input.invalidate_preview();
                    }
                    if reloaded {
                        self.restart_search();
                    }
                    self.dirty = true;
                }
                Event::Rendered {
                    id,
                    key,
                    image,
                    milliseconds,
                    appearance_revision,
                } if id == self.id && appearance_revision == self.appearance_revision => {
                    let wanted = self
                        .requested
                        .as_ref()
                        .is_some_and(|keys| keys.contains(&key));
                    if (!wanted && (key.tile.is_some() || key.page != self.nav.page))
                        || key.rotation != self.reading.rotation
                        || key.night != self.library.night
                    {
                        continue;
                    }
                    if !self.cache.can_insert(key, image.pixels.len() * 4) {
                        if wanted {
                            // A larger displayed fallback can temporarily leave
                            // too little room for prefetch. Retry missing pages
                            // once the desired visible raster has replaced it.
                            self.requested = None;
                        }
                        continue;
                    }
                    if !wanted
                        && self
                            .requested
                            .as_ref()
                            .and_then(|keys| keys.first())
                            .is_some_and(|key| self.cache.contains(key))
                    {
                        continue;
                    }
                    let texture = context.load_texture(
                        format!("page-{}-{}", key.page, key.width),
                        image,
                        egui::TextureOptions::LINEAR,
                    );
                    self.cache.insert(key, texture);
                    if key.page == self.nav.page {
                        self.render_ms = milliseconds;
                    }
                }
                Event::Thumbnail {
                    id,
                    key,
                    image,
                    appearance_revision,
                } if id == self.id && appearance_revision == self.appearance_revision => {
                    if key.rotation == self.reading.rotation && key.night == self.library.night {
                        self.thumbnails.accept(key, image, context);
                    }
                }
                Event::Search {
                    id,
                    revision,
                    hits,
                    scanned,
                    total,
                    complete,
                    limited,
                } if id == self.id && revision == self.revision => {
                    if !hits.is_empty() {
                        self.search_highlights.invalidate();
                    }
                    self.hits.extend(hits);
                    self.scanned = scanned;
                    self.scan_total = total;
                    self.search_complete = complete;
                    self.search_limited = limited;
                    self.resume_search_navigation();
                }
                Event::Text { id, page, text } if id == self.id => {
                    if text.trim().is_empty() {
                        self.notify("No selectable text on this page");
                    } else {
                        context.copy_text(text);
                        self.notify(format!("Copied text from page {}", page + 1));
                    }
                }
                Event::Error { id, error } if id == self.id => {
                    self.worker_error(context, error);
                }
                _ => {}
            }
        }
        if let Some(dialog) = &self.dialog {
            match dialog.try_recv() {
                Ok(path) => {
                    self.dialog = None;
                    if let Some(path) = path {
                        self.open(path, context);
                    }
                }
                Err(mpsc::TryRecvError::Disconnected) => {
                    self.dialog = None;
                }
                Err(mpsc::TryRecvError::Empty) => {
                    context.request_repaint_after(Duration::from_millis(100));
                }
            }
        }
        if let Some(path) = context.input(|input| {
            input
                .raw
                .dropped_files
                .iter()
                .find_map(|file| file.path.clone())
        }) {
            self.open(path, context);
        }
    }

    fn canvas(&mut self, context: &egui::Context) {
        let background = self
            .page_colors
            .palette(self.library.night)
            .canvas
            .map_or_else(
                || appearance::canvas(self.library.dark),
                |color| color.color(),
            );
        let canvas = egui::CentralPanel::default()
            .frame(egui::Frame::NONE.fill(background))
            .show(context, |ui| {
                if self.nav.count == 0 {
                    self.welcome(ui);
                    return;
                }
                if self.reading.continuous || self.reading.facing {
                    self.continuous_canvas(ui);
                    return;
                }
                let rect = ui.max_rect().shrink(24.0);
                self.resize_viewport(rect.size());
                let dimensions = self.dimensions[self.nav.page];
                let Some(rotated) = self.resolve_view() else {
                    return;
                };
                let mut view = RenderView {
                    fit: self.reading.fit,
                    viewport: [rect.width(), rect.height()],
                    pixels_per_point: context.pixels_per_point(),
                    rotation: self.reading.rotation,
                    night: self.library.night,
                    backwards: self.nav.direction < 0,
                };
                let mut size = Vec2::new(rotated[0], rotated[1]) * self.scale;
                let response = ui.interact(
                    ui.max_rect(),
                    ui.id().with("canvas"),
                    egui::Sense::click_and_drag(),
                );
                if response.dragged() && !self.selection.active {
                    self.pan = (self.pan - response.drag_delta()).clamp(Vec2::ZERO, self.overflow);
                }
                if response.hovered() {
                    let (scroll, zoom) =
                        ui.input(|input| (input.smooth_scroll_delta, input.zoom_delta()));
                    if (zoom - 1.0).abs() > 0.001 {
                        let pivot = ui
                            .input(|input| input.pointer.hover_pos())
                            .map(|pos| pos - rect.min)
                            .unwrap_or(rect.size() * 0.5)
                            .clamp(Vec2::ZERO, rect.size());
                        self.zoom_at(zoom, pivot);
                    } else {
                        self.scroll(-scroll, false);
                    }
                }
                if view.fit != self.reading.fit {
                    view.fit = self.reading.fit;
                    self.scale = view.scale(dimensions);
                    size = Vec2::new(rotated[0], rotated[1]) * self.scale;
                    self.overflow = (size - rect.size()).max(Vec2::ZERO);
                    self.pan = self.pan.clamp(Vec2::ZERO, self.overflow);
                }
                let origin = rect.min + (rect.size() - size).max(Vec2::ZERO) * 0.5 - self.pan;
                let mut page_rect = egui::Rect::from_min_size(origin, size);
                self.synctex_click(ui, self.nav.page, page_rect);
                if self.selection.active {
                    if response.hovered() || response.dragged() {
                        context.set_cursor_icon(if self.selection.ready() {
                            egui::CursorIcon::Text
                        } else {
                            egui::CursorIcon::Progress
                        });
                    }
                    let point = |position: egui::Pos2, page_rect: egui::Rect| {
                        let relative = position - page_rect.min;
                        crate::geometry::unrotate_point(
                            hayro::kurbo::Point::new(
                                f64::from(relative.x) / f64::from(self.scale),
                                f64::from(relative.y) / f64::from(self.scale),
                            ),
                            dimensions,
                            self.reading.rotation,
                        )
                    };
                    let clicked = response.clicked_by(egui::PointerButton::Primary);
                    if response.drag_started_by(egui::PointerButton::Primary) || clicked {
                        let pressed = ui
                            .input(|input| input.pointer.press_origin())
                            .or(response.interact_pointer_pos());
                        if let Some(position) = pressed {
                            if !page_rect.contains(position) {
                                self.selection.clear_range();
                            } else {
                                if self.selection.needs_preparation() {
                                    self.worker.inspect(self.nav.page);
                                }
                                if clicked {
                                    let unit = if response.triple_clicked() {
                                        SelectionUnit::Line
                                    } else if response.double_clicked() {
                                        SelectionUnit::Word
                                    } else {
                                        SelectionUnit::Glyph
                                    };
                                    self.selection.select(point(position, page_rect), unit);
                                } else {
                                    self.selection.begin(point(position, page_rect));
                                }
                            }
                        }
                    }
                    if response.dragged_by(egui::PointerButton::Primary)
                        && self.selection.has_anchor()
                        && let Some(position) = response.interact_pointer_pos()
                    {
                        let delta = selection_scroll::delta(
                            ui.max_rect(),
                            position,
                            ui.input(|input| input.unstable_dt),
                        );
                        let next = (self.pan + delta).clamp(Vec2::ZERO, self.overflow);
                        if next != self.pan {
                            page_rect = page_rect.translate(self.pan - next);
                            self.pan = next;
                            context.request_repaint_after(Duration::from_millis(16));
                        }
                    }
                    if (response.dragged_by(egui::PointerButton::Primary)
                        || response.drag_stopped_by(egui::PointerButton::Primary))
                        && let Some(position) = response.interact_pointer_pos()
                    {
                        self.selection.extend(point(position, page_rect));
                    }
                }
                let offset = self.current_offset();
                if self.reading.offset != offset {
                    self.reading.offset = offset;
                    self.dirty = true;
                }
                let planned = self.render_planner.plan(
                    PlanMode::Single,
                    view,
                    &[(self.nav.page, page_rect)],
                    &self.dimensions,
                    ui.max_rect(),
                    self.cache.budget(),
                );
                let plan = planned.plan;
                let key = plan[0];
                if plan.iter().any(|key| key.tile.is_some()) {
                    self.cache.protect_many(&plan);
                } else {
                    self.cache.protect(key);
                }
                if !self.loading && self.requested.as_ref() != Some(&plan) {
                    if !self.links.contains_key(&self.nav.page) {
                        self.worker.links(self.nav.page);
                    }
                    let keys = plan
                        .iter()
                        .copied()
                        .filter(|key| !self.cache.contains(key))
                        .collect();
                    self.requested = Some(plan.clone());
                    self.worker.render(keys);
                }
                let texture = self.cache.get(&key).or_else(|| self.cache.fallback(key));
                let painter = ui.painter_at(ui.max_rect());
                painter.add(
                    egui::epaint::Shadow {
                        offset: [0, 3],
                        blur: 16,
                        spread: 0,
                        color: Color32::from_black_alpha(28),
                    }
                    .as_shape(page_rect, 0),
                );
                painter.rect_filled(
                    page_rect,
                    0.0,
                    self.page_colors
                        .palette(self.library.night)
                        .background
                        .color(),
                );
                if let Some(texture) = texture {
                    painter.image(
                        texture,
                        page_rect,
                        egui::Rect::from_min_max(egui::Pos2::ZERO, egui::pos2(1.0, 1.0)),
                        Color32::WHITE,
                    );
                    self.paint_tiles(&painter, &plan, self.nav.page, page_rect, self.scale);
                    self.highlights(&painter, self.nav.page, page_rect, dimensions);
                    let clip = painter.clip_rect().intersect(page_rect);
                    painter.extend(self.selection.rects.iter().filter_map(|rect| {
                        let rect = self
                            .link_rect(*rect, page_rect, dimensions)
                            .intersect(page_rect);
                        (rect.is_positive() && clip.intersects(rect))
                            .then(|| egui::Shape::rect_filled(rect, 1.0, SELECTION_COLOR))
                    }));
                    self.paint_synctex(&painter, self.nav.page, page_rect, dimensions);
                    if !ui.input(|i| i.modifiers.command) {
                        self.link_overlays(ui, self.nav.page, page_rect, dimensions);
                        self.comment_overlays(ui, self.nav.page, page_rect, dimensions);
                    }
                } else {
                    painter.text(
                        page_rect.center(),
                        egui::Align2::CENTER_CENTER,
                        "Rendering…",
                        egui::FontId::proportional(15.0),
                        Color32::GRAY,
                    );
                }
            });
        self.controls_restore_button(context, canvas.response.rect);
        if self.file_changed && self.nav.count > 0 {
            self.file_change_notice(context, canvas.response.rect);
        }
        // Honor this frame's keys and buttons before deciding whether the
        // document is safe to replace (e.g. starting a comment or selection).
        self.reload_if_changed(context);
    }

    fn highlights(
        &mut self,
        painter: &egui::Painter,
        page: usize,
        page_rect: egui::Rect,
        dimensions: [f32; 2],
    ) {
        let clip = painter.clip_rect().intersect(page_rect);
        if !clip.is_positive() {
            return;
        }
        let view = crate::visible_rects::PageView {
            document: self.id,
            page,
            page_rect,
            clip,
            dimensions,
            rotation: self.reading.rotation,
            scale: self.scale,
        };
        let selected = self.hit_index;
        painter.extend(
            self.search_highlights
                .visible(&self.hits, view, self.nav.page)
                .map(|(index, rect)| {
                    let color = if selected == Some(index) {
                        CURRENT_SEARCH_COLOR
                    } else {
                        SEARCH_COLOR
                    };
                    egui::Shape::rect_filled(rect, 1.0, color)
                }),
        );
    }

    fn follow_destination(&mut self, destination: crate::destination::Destination) {
        let Some(&dimensions) = self.dimensions.get(destination.page) else {
            return;
        };
        let current = self.current_position();
        let origin = [
            f64::from(self.pan.x) / f64::from(self.scale),
            f64::from(self.pan.y) / f64::from(self.scale),
        ];
        self.jump(destination.page);
        self.history.record(current);
        self.nav.clear_prefix();
        self.hints.cancel();
        self.selection.cancel();
        self.search_navigation.clear();
        self.pending_focus = None;
        self.pending_offset = None;
        self.reading.fit = destination.fit(
            Fit::Custom(self.scale),
            dimensions,
            self.reading.rotation,
            [self.viewport.x, self.viewport.y],
        );
        self.pending_destination = Some((destination, origin));
        self.dirty = true;
    }

    fn follow_link(&mut self, target: LinkTarget, context: &egui::Context) {
        self.hints.cancel();
        match target {
            LinkTarget::Destination(destination) => self.follow_destination(destination),
            LinkTarget::Url(url) => context.open_url(egui::OpenUrl::new_tab(url)),
        }
        context.request_repaint();
    }

    fn copy_selection(&mut self, context: &egui::Context) {
        if self.selection.is_all() && !self.selection.ready() {
            self.worker.copy(self.selection_page);
            return;
        }
        if let Some(text) = self
            .selection
            .selected_text()
            .filter(|text| !text.is_empty())
        {
            context.copy_text(text.to_owned());
            self.notify("Selected text copied");
        } else {
            self.notify("Drag over text to select it first");
        }
    }

    fn link_rect(
        &self,
        rect: hayro::kurbo::Rect,
        page_rect: egui::Rect,
        dimensions: [f32; 2],
    ) -> egui::Rect {
        crate::visible_rects::screen_rect(
            rect,
            page_rect,
            dimensions,
            self.reading.rotation,
            self.scale,
        )
    }

    fn link_overlays(
        &mut self,
        ui: &mut egui::Ui,
        page: usize,
        page_rect: egui::Rect,
        dimensions: [f32; 2],
    ) {
        if self.selection.active {
            self.visible_links.clear();
            return;
        }
        let links = self.links.get(&page).map(Vec::as_slice).unwrap_or(&[]);
        let visible = if page == self.nav.page {
            &mut self.visible_links
        } else {
            // Reuse one scratch buffer across neighboring pages. Recompute
            // every time so replacement or eviction of metadata stays visible.
            self.other_visible_links.invalidate();
            &mut self.other_visible_links
        };
        let changed = visible.update(
            crate::visible_rects::PageView {
                document: self.id,
                page,
                page_rect,
                clip: ui.clip_rect(),
                dimensions,
                rotation: self.reading.rotation,
                scale: self.scale,
            },
            links.iter().map(|link| link.rect),
        );
        if changed && page == self.nav.page {
            self.hints.cancel();
        }
        let visible = if page == self.nav.page {
            &self.visible_links
        } else {
            &self.other_visible_links
        };
        let matching = self.hints.matching_range();
        let mut follow = None;
        let mut copied = false;
        let mut hint_index = 0;
        for (&link_index, &rect) in visible.indices.iter().zip(&visible.rects) {
            let link = &links[link_index];
            let response = ui
                .interact(
                    rect,
                    ui.id().with(("link", page, link_index)),
                    egui::Sense::click(),
                )
                .on_hover_cursor(egui::CursorIcon::PointingHand)
                .on_hover_ui(|ui| match &link.target {
                    LinkTarget::Destination(destination) => {
                        ui.label(format!("Go to page {}", self.page_name(destination.page)));
                    }
                    LinkTarget::Url(url) => {
                        ui.label(url);
                    }
                });
            if let LinkTarget::Url(url) = &link.target {
                if response.secondary_clicked() {
                    self.nav.clear_prefix();
                    self.hints.cancel();
                    self.mark_action = None;
                }
                copied |= copy_menu::show(
                    &response,
                    egui::Popup::default_response_id(&response),
                    "Copy link address",
                    url,
                    copy_menu::Target::Document,
                );
            }
            if response.clicked() {
                follow = Some(link.target.clone());
            }
            let hint = !self.copy_link_hints || matches!(&link.target, LinkTarget::Url(_));
            let index = hint_index;
            hint_index += usize::from(hint);
            if page == self.nav.page && hint && matching.contains(&index) {
                let label = crate::link_hints::label(index, self.hints.count);
                let painter = ui.painter();
                let text =
                    painter.layout_no_wrap(label, egui::FontId::monospace(14.0), Color32::WHITE);
                let badge = egui::Rect::from_min_size(rect.min, text.size() + Vec2::new(8.0, 4.0));
                painter.rect_stroke(
                    rect,
                    1.0,
                    egui::Stroke::new(1.0_f32, ACCENT),
                    egui::StrokeKind::Inside,
                );
                painter.rect_filled(badge, 3.0, ACCENT);
                painter.galley(badge.min + Vec2::new(4.0, 2.0), text, Color32::WHITE);
            }
        }
        if let Some(target) = follow {
            self.follow_link(target, ui.ctx());
        }
        if copied {
            self.notify("Copied link address");
        }
    }

    fn dialogs(&mut self, context: &egui::Context) {
        self.recent_dialog(context);
        self.zoom_dialog(context);
        self.page_dialog(context);
        self.password_dialog(context);
        self.help_window(context);
        self.error_panels(context);
    }
}

impl eframe::App for Pagewing {
    fn raw_input_hook(&mut self, context: &egui::Context, input: &mut egui::RawInput) {
        self.prepare_native_input(context, input);
    }

    fn update(&mut self, context: &egui::Context, _: &mut eframe::Frame) {
        let popup_view = (
            self.id,
            self.nav.page,
            self.reading.rotation,
            self.reading.fit,
            self.pan,
            self.viewport,
        );
        if !self.deferred_input.is_empty() {
            let mut events = std::mem::take(&mut self.deferred_input);
            context.input_mut(|input| {
                events.append(&mut input.events);
                input.events = events;
            });
        }
        let _side_button_focus = shortcuts::preserve_side_button_focus(context);
        self.events(context);
        #[cfg(target_os = "macos")]
        self.macos_open_documents(context);
        self.synctex_events(context);
        if context.input(|input| input.viewport().close_requested())
            && self.comment_change_blocked(context)
        {
            context.send_viewport_cmd(egui::ViewportCommand::CancelClose);
        }
        self.keys(context);
        self.toolbar(context);
        self.search_bar(context);
        self.status(context);
        self.dialogs(context);
        self.properties_window(context);
        self.comments_sidebar(context);
        self.sidebar(context);
        self.canvas(context);
        if popup_view
            != (
                self.id,
                self.nav.page,
                self.reading.rotation,
                self.reading.fit,
                self.pan,
                self.viewport,
            )
            && !self.input_buttons.control_menu_open(context)
            && !copy_menu::control_menu_open(context)
        {
            // Document context menus depend on page geometry. Menus anchored
            // to reader controls retain their focus while the viewport resizes.
            egui::Popup::close_all(context);
        }
        if let Some(due) = self.search_due {
            if Instant::now() >= due {
                self.worker.search(
                    self.revision,
                    self.query.clone(),
                    self.library.search_options,
                );
                self.search_due = None;
            } else {
                context.request_repaint_after(due.saturating_duration_since(Instant::now()));
            }
        }
        if self.dirty {
            let elapsed = self.saved.elapsed();
            if elapsed >= Duration::from_secs(2) {
                self.save();
            } else {
                context.request_repaint_after(Duration::from_secs(2) - elapsed);
            }
        }
        if self.loading {
            context.request_repaint_after(Duration::from_millis(100));
        }
    }
    fn on_exit(&mut self, _: Option<&eframe::glow::Context>) {
        self.save();
    }
}

fn set_theme(context: &egui::Context, dark: bool) {
    appearance::apply(context, dark);
}
