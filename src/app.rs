mod appearance;
mod comments;
#[cfg(test)]
mod config;
mod continuous;
mod dialog;
mod help;
mod input_dialogs;
mod properties;
mod recent;
mod reload;
mod selection_scroll;
mod shortcuts;
mod sidebar;
mod spread;
mod sync;
mod tiles;
mod toolbar;
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
use eframe::egui::{self, Color32, Key, RichText, Vec2};
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
    labels: crate::page_labels::PageLabels,
    links: crate::link_cache::LinkCache,
    visible_links: crate::link_visibility::LinkVisibility,
    hints: LinkHints,
    copy_link_hints: bool,
    mark_action: Option<MarkAction>,
    pending_offset: Option<[f32; 2]>,
    pending_focus: Option<hayro::kurbo::Rect>,
    pending_destination: Option<(crate::destination::Destination, [f64; 2])>,
    selection: TextSelection,
    selection_page: usize,
    continuous_layout: crate::continuous::Layout,
    continuous_link_request: Option<usize>,
    cache: TextureCache,
    thumbnails: crate::thumbnails::Thumbnails,
    reveal_page: Option<usize>,
    id: u64,
    loading: bool,
    reloading: bool,
    file_changed: bool,
    auto_reload_pending: bool,
    automatic_reload: bool,
    error: Option<String>,
    configuration_error: Option<String>,
    message: Option<(String, Instant)>,
    pan: Vec2,
    overflow: Vec2,
    viewport: Vec2,
    scale: f32,
    render_planner: RenderPlanner,
    requested: Option<Arc<RenderPlan>>,
    render_ms: f64,
    dialog: Option<Receiver<Option<PathBuf>>>,
    deferred_input: Vec<egui::Event>,
    search_open: bool,
    search_focus: bool,
    query: String,
    search_due: Option<Instant>,
    revision: u64,
    hits: Vec<SearchHit>,
    hit_index: Option<usize>,
    reveal_hit: Option<usize>,
    pending_search_jump: Option<(bool, usize)>,
    scanned: usize,
    scan_total: usize,
    search_complete: bool,
    search_limited: bool,
    page_input: Option<input_dialogs::PageInput>,
    zoom_input: Option<zoom::ZoomInput>,
    help: bool,
    help_filter: help::HelpFilter,
    input_buttons: shortcuts::InputButtons,
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
        config.unwrap_or_default().apply(&mut library);
        set_theme(context, library.dark);
        shortcuts::configure_input(context);
        crate::fonts::install(context.clone());
        let mut app = Self {
            #[cfg(target_os = "macos")]
            open_documents: None,
            worker: Worker::new(context.clone()),
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
            labels: crate::page_labels::PageLabels::default(),
            links: crate::link_cache::LinkCache::default(),
            visible_links: crate::link_visibility::LinkVisibility::default(),
            hints: LinkHints::default(),
            copy_link_hints: false,
            mark_action: None,
            pending_offset: None,
            pending_focus: None,
            pending_destination: None,
            selection: TextSelection::default(),
            selection_page: 0,
            continuous_layout: crate::continuous::Layout::default(),
            continuous_link_request: None,
            cache: TextureCache::default(),
            thumbnails: crate::thumbnails::Thumbnails::default(),
            reveal_page: None,
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
            search_open: false,
            search_focus: false,
            query: String::new(),
            search_due: None,
            revision: 0,
            hits: Vec::new(),
            hit_index: None,
            reveal_hit: None,
            pending_search_jump: None,
            scanned: 0,
            scan_total: 0,
            search_complete: true,
            search_limited: false,
            page_input: None,
            zoom_input: None,
            help: false,
            help_filter: help::HelpFilter::default(),
            input_buttons: shortcuts::InputButtons::default(),
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

    fn open(&mut self, path: PathBuf, context: &egui::Context) {
        if self.comment_change_blocked() {
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
            .cloned()
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

    fn clear_document(&mut self, context: &egui::Context) {
        self.synctex.clear_document();
        egui::Popup::close_all(context);
        self.id += 1;
        self.nav = Navigation::default();
        self.history = crate::history::ViewHistory::default();
        self.dimensions.clear();
        self.continuous_layout = crate::continuous::Layout::default();
        self.continuous_link_request = None;
        self.info = crate::document_info::DocumentInfo::default();
        self.properties_open = false;
        self.zoom_input = None;
        self.comments = comments::CommentUi::default();
        self.outline.set_entries(Vec::new());
        self.bookmark_pages.clear();
        self.labels = crate::page_labels::PageLabels::default();
        self.links.clear();
        self.visible_links.clear();
        self.hints.cancel();
        self.selection.cancel();
        self.mark_action = None;
        self.cache.clear();
        self.thumbnails.clear();
        self.reveal_page = None;
        self.requested = None;
        self.render_planner = RenderPlanner::default();
        self.pan = Vec2::ZERO;
        self.pending_offset = None;
        self.pending_focus = None;
        self.pending_destination = None;
        self.hits.clear();
        self.hit_index = None;
        self.reveal_hit = None;
        self.pending_search_jump = None;
        self.query.clear();
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
        self.deferred_input.clear();
        context.memory_mut(|memory| {
            if let Some(id) = memory.focused() {
                memory.surrender_focus(id);
            }
            memory.move_focus(egui::FocusDirection::None);
        });
    }

    fn close_document(&mut self, context: &egui::Context) {
        if self.comment_change_blocked() {
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

    fn reopen_closed(&mut self, context: &egui::Context) {
        self.nav.clear_prefix();
        self.mark_action = None;
        if let Some(path) = self.last_closed.take() {
            self.open(path, context);
        } else {
            self.notify("No recently closed document");
        }
    }

    fn filename(&self) -> String {
        self.reading
            .path
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_else(|| "Pagewing".into())
    }

    fn reload(&mut self) {
        if self.comment_change_blocked() {
            return;
        }
        if self.nav.count == 0 || self.loading {
            return;
        }
        self.auto_reload_pending = false;
        self.automatic_reload = false;
        self.remember();
        self.nav.clear_prefix();
        self.loading = true;
        self.reloading = true;
        self.error = None;
        self.hints.cancel();
        self.selection.cancel();
        self.pending_search_jump = None;
        self.worker
            .reload(self.id, self.reading.path.clone(), String::new());
    }

    fn restart_search(&mut self) {
        self.revision += 1;
        self.hits.clear();
        self.hit_index = None;
        self.reveal_hit = None;
        self.scanned = 0;
        self.scan_total = self.nav.count;
        self.search_complete = self.query.trim().is_empty();
        self.search_limited = false;
        self.search_due = (!self.search_complete).then_some(Instant::now());
    }

    fn change_search(&mut self) {
        self.pending_search_jump = None;
        self.restart_search();
        self.worker
            .search(self.revision, String::new(), self.library.search_options);
        self.search_due =
            (!self.search_complete).then_some(Instant::now() + Duration::from_millis(180));
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
        self.message = Some((text.into(), Instant::now()));
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
        self.pending_search_jump = None;
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
        self.pending_search_jump = None;
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
        if !self.reading.bookmarks.remove(&self.nav.page) {
            self.reading.bookmarks.insert(self.nav.page);
            self.notify("Bookmark added");
        } else {
            self.notify("Bookmark removed");
        }
        self.refresh_bookmarks();
        self.reveal_page = Some(self.nav.page);
        self.dirty = true;
    }

    fn refresh_bookmarks(&mut self) {
        self.bookmark_pages.clear();
        self.bookmark_pages
            .extend(self.reading.bookmarks.iter().copied());
    }

    fn find(&mut self) {
        self.zoom_input = None;
        self.outline.cancel_input();
        let selected = self
            .selection
            .selected_text()
            .map(str::trim)
            .filter(|text| !text.is_empty() && !text.contains(['\n', '\r']))
            .map(str::to_owned);
        self.recent_picker = None;
        self.nav.clear_prefix();
        self.hints.cancel();
        self.selection.cancel();
        self.mark_action = None;
        self.help = false;
        self.properties_open = false;
        self.page_input = None;
        self.password_input = None;
        self.search_open = true;
        self.search_focus = true;
        if let Some(query) = selected {
            self.query = query;
            self.change_search();
        }
    }

    fn next_hit(&mut self, reverse: bool) {
        self.next_hits(reverse, 1);
    }

    fn next_hits(&mut self, reverse: bool, count: usize) {
        let count = count.max(1);
        if self.hits.is_empty() {
            self.pending_search_jump =
                (!self.search_complete || self.search_due.is_some()).then_some((reverse, count));
            return;
        }
        let (start, steps) = match self.hit_index {
            Some(index) => (index, count),
            None => match crate::text::initial_match(
                &self.hits,
                self.nav.page,
                reverse,
                self.scanned,
                self.search_complete,
            ) {
                Some(index) => (index, count - 1),
                None => {
                    self.pending_search_jump = (!self.search_complete).then_some((reverse, count));
                    return;
                }
            },
        };
        let Some(index) = crate::text::advance_match(
            start,
            self.hits.len(),
            reverse,
            steps,
            self.search_complete,
        ) else {
            self.pending_search_jump = (!self.search_complete).then_some((reverse, count));
            return;
        };
        self.pending_search_jump = None;
        self.select_hit(index);
    }

    fn select_hit(&mut self, index: usize) {
        let current = self.current_position();
        self.pending_search_jump = None;
        self.hit_index = Some(index);
        self.reveal_hit = Some(index);
        let page = self.hits[index].page;
        let rect = self.hits[index].rects.first().copied();
        self.jump(page);
        self.history.record(current);
        self.pending_focus = rect;
    }

    fn events(&mut self, context: &egui::Context) {
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
                            self.reload();
                            self.notify(match operation {
                                crate::annotations::Operation::Save => "Comment saved in PDF.",
                                crate::annotations::Operation::Delete => {
                                    "Comment and highlight deleted from PDF."
                                }
                            });
                        }
                        Err(error) => self.error = Some(error),
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
                    self.visible_links.clear();
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
                    self.continuous_link_request = None;
                    self.info = info;
                    self.comments.reset(reloaded);
                    self.worker.comments();
                    self.reveal_page = Some(self.nav.page);
                    self.outline.set_entries(outline);
                    self.labels = labels;
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
                } if id == self.id => {
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
                Event::Thumbnail { id, key, image } if id == self.id => {
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
                    self.hits.extend(hits);
                    self.scanned = scanned;
                    self.scan_total = total;
                    self.search_complete = complete;
                    self.search_limited = limited;
                    if let Some((reverse, count)) = self.pending_search_jump.take() {
                        self.next_hits(reverse, count);
                    }
                }
                Event::Text { id, page, text } if id == self.id => {
                    if text.trim().is_empty() {
                        self.notify("No selectable text on this page");
                    } else {
                        context.copy_text(text);
                        self.notify(format!("Copied text from page {}", page + 1));
                    }
                }
                Event::Error {
                    id,
                    message,
                    opening,
                } if id == self.id => {
                    if self.loading && !opening {
                        continue;
                    }
                    let automatic = opening && self.automatic_reload;
                    if opening && self.loading && self.reloading {
                        self.restart_search();
                        self.requested = None;
                        self.thumbnails.invalidate_requests();
                    }
                    if opening {
                        self.loading = false;
                        self.automatic_reload = false;
                        if !message.contains("Decryption") {
                            self.synctex.cancel_pending();
                        }
                    }
                    if opening && message.contains("Decryption") && !automatic {
                        self.open_password(context);
                    }
                    self.error = Some(if automatic && message.contains("Decryption") {
                        "PDF changed but needs a password. Press Ctrl/Cmd+R to unlock the new version.".into()
                    } else {
                        message
                    });
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

    fn keys(&mut self, context: &egui::Context) {
        // Apply button actions before routing this frame's keyboard input.
        // Otherwise a click that releases search focus can send following text
        // to reader commands before the new input has opened.
        match self.input_buttons.clicked(context) {
            Some(shortcuts::InputButton::Help) => {
                if self.help {
                    self.close_help(context);
                } else {
                    self.open_help(context);
                }
            }
            Some(shortcuts::InputButton::Find) => self.find(),
            Some(shortcuts::InputButton::Comments) => {
                self.set_comments_open(context, !self.comments.open);
            }
            Some(shortcuts::InputButton::CloseComments) => self.set_comments_open(context, false),
            Some(shortcuts::InputButton::Page) => self.open_goto(),
            Some(shortcuts::InputButton::Zoom) => self.open_zoom(context),
            Some(shortcuts::InputButton::ReloadChanged) => {
                if self.file_changed {
                    self.reload();
                }
            }
            Some(shortcuts::InputButton::DismissChange) => {
                self.file_changed = false;
                self.auto_reload_pending = false;
            }
            None => {}
        }
        if context.input(|input| input.pointer.button_pressed(egui::PointerButton::Primary)) {
            for (visible, name) in [
                (self.search_open, "pagewing-search-input"),
                (self.recent_picker.is_some(), "pagewing-recent-input"),
                (self.page_input.is_some(), "pagewing-page-input"),
                (self.zoom_input.is_some(), "pagewing-zoom-input"),
                (self.password_input.is_some(), "pagewing-password-input"),
                (self.help, "pagewing-help-input"),
                (
                    self.comments.open && self.comments.editor.is_some(),
                    "pagewing-comment-input",
                ),
                (self.comments.open, "pagewing-comment-filter"),
                (
                    self.library.sidebar && self.nav.count > 0 && !self.outline.is_empty(),
                    "pagewing-outline-input",
                ),
            ] {
                if visible && shortcuts::focus_clicked_input(context, egui::Id::new(name)) {
                    // These dialogs also need to recognize Enter when focus
                    // was gained and released within this very same frame.
                    match name {
                        "pagewing-search-input" => self.search_focus = true,
                        "pagewing-page-input" => {
                            if let Some(input) = &mut self.page_input {
                                input.focus = true;
                            }
                        }
                        "pagewing-password-input" => {
                            if let Some(input) = &mut self.password_input {
                                input.focus = true;
                            }
                        }
                        "pagewing-zoom-input" => {
                            if let Some(input) = &mut self.zoom_input {
                                input.focus = true;
                            }
                        }
                        _ => {}
                    }
                    break;
                }
            }
        }
        // Preserve action order, including side-button history mixed with keys.
        // Text, paste, IME and primary pointer events stay in egui.
        for (event, key_text) in
            context.input(|input| shortcuts::reader_actions(input).collect::<Vec<_>>())
        {
            let symbol = key_text.character();
            // Opening/closing an input changes how later keys in this same
            // frame must be routed, before its widget has been drawn.
            let typing = (self.search_open && self.search_focus)
                || (self.help && self.help_filter.focus)
                || self.outline.focus
                || self.page_input.is_some()
                || self.zoom_input.is_some()
                || self.password_input.is_some()
                || self.recent_picker.is_some()
                || (self.comments.open && self.comments.editor.is_some())
                || (self.comments.open
                    && context.memory(|memory| {
                        ["pagewing-comment-input", "pagewing-comment-filter"]
                            .iter()
                            .any(|name| memory.has_focus(egui::Id::new(name)))
                    }))
                || context.memory(|memory| {
                    [
                        "pagewing-search-input",
                        "pagewing-recent-input",
                        "pagewing-page-input",
                        "pagewing-zoom-input",
                        "pagewing-password-input",
                        "pagewing-help-input",
                        "pagewing-outline-input",
                    ]
                    .iter()
                    .any(|name| memory.has_focus(egui::Id::new(name)))
                });
            let popup_open = egui::Popup::is_any_open(context);
            if let egui::Event::PointerButton { button, .. } = event {
                if !typing && !self.help && !self.properties_open && !popup_open {
                    self.nav.clear_prefix();
                    self.mark_action = None;
                    self.history(button == egui::PointerButton::Extra2);
                }
                continue;
            }
            if matches!(event, egui::Event::Copy) && !typing && !popup_open && self.selection.active
            {
                self.copy_selection(context);
                context.input_mut(|input| {
                    input
                        .events
                        .retain(|event| !matches!(event, egui::Event::Copy))
                });
                continue;
            }
            let egui::Event::Key {
                key,
                pressed: true,
                modifiers,
                ..
            } = event
            else {
                continue;
            };
            let raw_key = key;
            let raw_modifiers = modifiers;
            // Adapt configured bindings to the existing reader commands, but
            // consume the original physical press so text cannot leak to egui.
            // Sequences and text fields retain their own keyboard vocabulary.
            let reader_ready = !typing
                && !self.help
                && !self.properties_open
                && !popup_open
                && !self.hints.active
                && self.mark_action.is_none()
                && !self.nav.pending_z;
            let binding = crate::settings::Binding::from_event(key, modifiers, symbol);
            let (key, modifiers, key_text) =
                match self.library.shortcuts.resolve(&binding, !reader_ready) {
                    crate::settings::Resolution::Mapped(key, modifiers) => {
                        (key, modifiers, shortcuts::KeyText::None)
                    }
                    crate::settings::Resolution::Disabled => {
                        shortcuts::consume_reader_shortcut(context, raw_key, raw_modifiers);
                        continue;
                    }
                    crate::settings::Resolution::Unmatched => (key, modifiers, key_text),
                };
            if popup_open && key == Key::Escape {
                egui::Popup::close_all(context);
                context.input_mut(|input| input.consume_key(raw_modifiers, raw_key));
                continue;
            }
            if popup_open
                && modifiers.command
                && (matches!(key, Key::O | Key::F | Key::P | Key::R | Key::W | Key::Q)
                    || (key == Key::Z && modifiers.alt && !modifiers.shift)
                    || (key == Key::T && modifiers.shift))
            {
                egui::Popup::close_all(context);
            }
            if (typing || !(self.help || self.properties_open || popup_open))
                && matches!(
                    raw_key,
                    Key::ArrowUp | Key::ArrowDown | Key::ArrowLeft | Key::ArrowRight
                )
            {
                // egui scheduled directional focus before dispatch. Arrow
                // keys belong to text editing or reader navigation here.
                context.memory_mut(|memory| memory.move_focus(egui::FocusDirection::None));
            }
            if self.search_open
                && !popup_open
                && !self.help
                && !self.properties_open
                && self.password_input.is_none()
                && self.page_input.is_none()
                && modifiers.alt
                && !modifiers.command
                && !modifiers.shift
                && matches!(key, Key::C | Key::W)
            {
                if key == Key::C {
                    self.library.search_options.match_case =
                        !self.library.search_options.match_case;
                } else {
                    self.library.search_options.whole_word =
                        !self.library.search_options.whole_word;
                }
                self.dirty = true;
                context.input_mut(|input| {
                    shortcuts::consume_text_shortcut(input, raw_key, raw_modifiers)
                });
                self.change_search();
                continue;
            }
            if !popup_open
                && self.zoom_input.is_none()
                && key == Key::M
                && modifiers.command
                && modifiers.shift
            {
                self.set_comments_open(context, !self.comments.open);
                context.input_mut(|input| {
                    shortcuts::consume_text_shortcut(input, raw_key, raw_modifiers)
                });
                continue;
            }
            if !popup_open
                && self.zoom_input.is_none()
                && modifiers.command
                && modifiers.shift
                && key == Key::H
                && self.nav.count > 0
            {
                self.add_comment();
                context.input_mut(|input| {
                    shortcuts::consume_text_shortcut(input, raw_key, raw_modifiers)
                });
                continue;
            }
            if self.comments.open
                && self.zoom_input.is_none()
                && self.page_input.is_none()
                && self.password_input.is_none()
                && modifiers.command
                && key == Key::Enter
                && self.comments.editor.is_some()
            {
                self.comments.save_requested = true;
                context.input_mut(|input| input.consume_key(raw_modifiers, raw_key));
                continue;
            }
            if self.zoom_input.is_some() && key == Key::Escape {
                self.close_zoom(context);
                context.input_mut(|input| input.consume_key(raw_modifiers, raw_key));
                continue;
            }
            if let Some(input) = &mut self.zoom_input
                && key == Key::Enter
                && modifiers.is_none()
                && (input.focus || context.memory(|memory| memory.has_focus(zoom::input_id())))
            {
                input.confirm = true;
                context.input_mut(|input| input.consume_key(raw_modifiers, raw_key));
                continue;
            }
            if self.page_input.is_some() && key == Key::Escape {
                self.close_page_input(context);
                context.input_mut(|input| input.consume_key(raw_modifiers, raw_key));
                continue;
            }
            if self.password_input.is_some() && key == Key::Escape {
                self.close_password_input(context);
                context.input_mut(|input| input.consume_key(raw_modifiers, raw_key));
                continue;
            }
            if let Some(input) = &mut self.page_input
                && key == Key::Enter
                && modifiers.is_none()
                && (input.focus
                    || context.memory(|memory| memory.has_focus(input_dialogs::page_id())))
            {
                input.confirm = true;
                context.input_mut(|input| input.consume_key(raw_modifiers, raw_key));
                continue;
            }
            if let Some(input) = &mut self.password_input
                && key == Key::Enter
                && modifiers.is_none()
                && (input.focus
                    || context.memory(|memory| memory.has_focus(input_dialogs::password_id())))
            {
                input.confirm = true;
                context.input_mut(|input| input.consume_key(raw_modifiers, raw_key));
                continue;
            }
            if key == Key::Escape && self.comments.open {
                self.set_comments_open(context, false);
                context.input_mut(|input| input.consume_key(raw_modifiers, raw_key));
                continue;
            }
            if key == Key::Escape {
                self.outline.cancel_input();
                self.hints.cancel();
                self.selection.cancel();
                self.mark_action = None;
                self.help = false;
                self.properties_open = false;
                self.recent_picker = None;
                self.page_input = None;
                self.search_open = false;
                self.search_focus = false;
                self.pending_search_jump = None;
                self.password_input = None;
                self.nav.clear_prefix();
                context.memory_mut(|memory| {
                    if let Some(id) = memory.focused() {
                        memory.surrender_focus(id);
                    }
                });
                continue;
            }
            if modifiers.command && key == Key::O {
                self.choose_file();
                continue;
            }
            if modifiers.command && key == Key::F {
                context.input_mut(|input| {
                    shortcuts::consume_text_shortcut(input, raw_key, raw_modifiers)
                });
                self.find();
                continue;
            }
            if modifiers.command && modifiers.alt && !modifiers.shift && key == Key::Z {
                shortcuts::consume_reader_shortcut(context, raw_key, raw_modifiers);
                // Repeating the shortcut keeps any in-progress value intact.
                if self.zoom_input.is_none() {
                    self.open_zoom(context);
                }
                continue;
            }
            if modifiers.command && key == Key::P {
                context.input_mut(|input| {
                    shortcuts::consume_text_shortcut(input, raw_key, raw_modifiers)
                });
                self.toggle_recent(context);
                continue;
            }
            if modifiers.command && key == Key::R {
                self.reload();
                continue;
            }
            if modifiers.command && key == Key::W {
                context.input_mut(|input| {
                    shortcuts::consume_text_shortcut(input, raw_key, raw_modifiers)
                });
                self.close_document(context);
                continue;
            }
            if modifiers.command && modifiers.shift && key == Key::T {
                context.input_mut(|input| {
                    shortcuts::consume_text_shortcut(input, raw_key, raw_modifiers)
                });
                self.reopen_closed(context);
                continue;
            }
            if modifiers.command && key == Key::Q {
                context.input_mut(|input| {
                    input.consume_key(raw_modifiers, raw_key);
                });
                context.send_viewport_cmd(egui::ViewportCommand::Close);
                continue;
            }
            if popup_open {
                continue;
            }
            if let Some(picker) = &mut self.recent_picker {
                let filter_focused = picker.focus
                    || context
                        .memory(|memory| memory.has_focus(egui::Id::new("pagewing-recent-input")));
                // Tab can focus a result, Open or Cancel. Leave their activation
                // keys for egui instead of treating every Enter as filter confirm.
                if key == Key::Enter && filter_focused {
                    picker.confirm = true;
                    context.input_mut(|input| input.consume_key(raw_modifiers, raw_key));
                } else if filter_focused
                    && (matches!(key, Key::ArrowUp | Key::ArrowDown)
                        || (modifiers.ctrl && matches!(key, Key::J | Key::K)))
                {
                    let delta = if matches!(key, Key::ArrowUp | Key::K) {
                        -1
                    } else {
                        1
                    };
                    picker.motions.push(delta);
                    context.input_mut(|input| {
                        shortcuts::consume_text_shortcut(input, raw_key, raw_modifiers)
                    });
                }
                continue;
            }
            if self.comment_filter_focused(context) {
                if key == Key::Enter && modifiers.is_none() {
                    self.comments.filter_confirm = true;
                    context.input_mut(|input| input.consume_key(raw_modifiers, raw_key));
                } else if (matches!(key, Key::ArrowUp | Key::ArrowDown) && modifiers.is_none())
                    || (modifiers.ctrl
                        && !modifiers.alt
                        && !modifiers.shift
                        && matches!(key, Key::J | Key::K))
                {
                    self.comments
                        .filter_motions
                        .push(if matches!(key, Key::ArrowUp | Key::K) {
                            -1
                        } else {
                            1
                        });
                    context.input_mut(|input| {
                        shortcuts::consume_text_shortcut(input, raw_key, raw_modifiers)
                    });
                }
                continue;
            }
            if !self.search_focus
                && self.zoom_input.is_none()
                && self.page_input.is_none()
                && self.password_input.is_none()
                && !self.help
                && (self.outline.focus
                    || context
                        .memory(|memory| memory.has_focus(egui::Id::new("pagewing-outline-input"))))
            {
                if key == Key::Enter {
                    self.outline.confirm = true;
                    context.input_mut(|input| input.consume_key(raw_modifiers, raw_key));
                } else if matches!(key, Key::ArrowUp | Key::ArrowDown)
                    || (modifiers.ctrl && matches!(key, Key::J | Key::K))
                {
                    let delta = if matches!(key, Key::ArrowUp | Key::K) {
                        -1
                    } else {
                        1
                    };
                    self.outline.motions.push(delta);
                    context.input_mut(|input| {
                        shortcuts::consume_text_shortcut(input, raw_key, raw_modifiers)
                    });
                }
                continue;
            }
            if self.properties_open {
                if key == Key::Y && !modifiers.command && !modifiers.alt && !modifiers.ctrl {
                    self.copy_properties(context);
                }
                continue;
            }
            if typing || self.help || self.password_input.is_some() || self.page_input.is_some() {
                continue;
            }
            if modifiers.command && key == Key::A && self.nav.count > 0 {
                context.input_mut(|input| {
                    shortcuts::consume_text_shortcut(input, raw_key, raw_modifiers)
                });
                self.nav.clear_prefix();
                self.hints.cancel();
                self.mark_action = None;
                if self.selection_page != self.nav.page {
                    self.selection.start();
                }
                self.selection.select_all();
                self.selection_page = self.nav.page;
                if !self.selection.ready() {
                    self.worker.inspect_all(self.nav.page);
                }
                continue;
            }
            if modifiers.command && key == Key::C && self.selection.active {
                self.copy_selection(context);
                context.input_mut(|input| {
                    input.consume_key(raw_modifiers, raw_key);
                });
                continue;
            }
            shortcuts::consume_reader_shortcut(context, raw_key, raw_modifiers);
            if let Some(action) = shortcuts::zoom_action(key, modifiers) {
                self.nav.clear_prefix();
                self.mark_action = None;
                match action {
                    shortcuts::ZoomAction::In => self.zoom(1.2),
                    shortcuts::ZoomAction::Out => self.zoom(1.0 / 1.2),
                    shortcuts::ZoomAction::ActualSize => self.actual_size(),
                }
                continue;
            }
            if let Some(action) = self.mark_action.take() {
                if !modifiers.ctrl && !modifiers.command && !modifiers.alt {
                    let name = key.name().to_ascii_lowercase();
                    if matches!(action, MarkAction::Jump)
                        && !modifiers.shift
                        && matches!(key, Key::Quote | Key::Backtick)
                    {
                        if let Some(position) = self.history.previous_jump(self.nav.count) {
                            self.restore_position(position, true);
                        } else {
                            self.notify("No previous jump position");
                        }
                    } else if name.len() == 1 && name.as_bytes()[0].is_ascii_lowercase() {
                        self.mark(name.as_bytes()[0] as char, action);
                    } else {
                        self.notify("Use a letter a–z for a mark");
                    }
                }
                continue;
            }
            if self.hints.active {
                if modifiers.ctrl || modifiers.command || modifiers.alt {
                    continue;
                }
                if key == Key::Backspace {
                    self.hints.prefix.pop();
                    continue;
                }
                let name = key.name().to_ascii_lowercase();
                if name.len() == 1
                    && let Some(index) = self.hints.press(name.as_bytes()[0] as char)
                    && let Some(target) = self
                        .links
                        .get(&self.nav.page)
                        .and_then(|links| {
                            if self.copy_link_hints {
                                self.visible_links
                                    .indices
                                    .iter()
                                    .filter_map(|&index| links.get(index))
                                    .filter(|link| matches!(&link.target, LinkTarget::Url(_)))
                                    .nth(index)
                            } else {
                                self.visible_links
                                    .indices
                                    .get(index)
                                    .and_then(|&index| links.get(index))
                            }
                        })
                        .map(|link| link.target.clone())
                {
                    if self.copy_link_hints {
                        if let LinkTarget::Url(url) = target {
                            context.copy_text(url);
                            self.notify("Copied link address");
                        }
                    } else {
                        self.follow_link(target, context);
                    }
                }
                continue;
            }
            if modifiers.ctrl {
                let n = self.nav.take_count() as f32;
                match key {
                    Key::E => self.scroll(Vec2::new(0.0, 60.0 * n), false),
                    Key::Y => self.scroll(Vec2::new(0.0, -60.0 * n), false),
                    Key::D => self.scroll(Vec2::new(0.0, self.viewport.y * 0.5 * n), true),
                    Key::U => self.scroll(Vec2::new(0.0, -self.viewport.y * 0.5 * n), true),
                    Key::F => self.scroll(Vec2::new(0.0, self.viewport.y * 0.9 * n), true),
                    Key::B => self.scroll(Vec2::new(0.0, -self.viewport.y * 0.9 * n), true),
                    Key::I => {
                        for _ in 0..n as usize {
                            if !self.history(true) {
                                break;
                            }
                        }
                    }
                    _ => {}
                }
                continue;
            }
            if modifiers.alt {
                let count = self.nav.take_count();
                if matches!(key, Key::ArrowLeft | Key::ArrowRight) {
                    for _ in 0..count {
                        if !self.history(key == Key::ArrowRight) {
                            break;
                        }
                    }
                } else if matches!(key, Key::J | Key::K) {
                    self.pending_search_jump = None;
                    if let Some(page) = crate::navigation::bookmark_target(
                        &self.bookmark_pages,
                        self.nav.page,
                        key == Key::K,
                        count,
                    ) {
                        self.jump(page);
                    } else {
                        self.notify(format!(
                            "No bookmarks yet · {} to add one",
                            self.library.shortcuts.label("bookmark")
                        ));
                    }
                }
                continue;
            }
            if modifiers.command {
                self.nav.clear_prefix();
                continue;
            }
            if key == Key::Backspace {
                self.nav.backspace();
                continue;
            }
            let digit = shortcuts::count_digit(key, modifiers.shift, key_text);
            if self.nav.pending_z {
                let count = self.nav.take_count() as f32;
                if symbol == Some('$') {
                    self.align_axis(0, 1.0);
                } else if digit == Some('0') {
                    self.align_axis(0, 0.0);
                } else if modifiers.shift && matches!(key, Key::H | Key::L) {
                    let direction = if key == Key::H { -1.0 } else { 1.0 };
                    self.scroll(
                        Vec2::new(self.viewport.x * 0.5 * count * direction, 0.0),
                        false,
                    );
                } else if !modifiers.shift {
                    match key {
                        Key::T => self.align_axis(1, 0.0),
                        Key::Z => self.align_axis(1, 0.5),
                        Key::B => self.align_axis(1, 1.0),
                        _ => {}
                    }
                }
                continue;
            }
            if symbol == Some('%') {
                let percentage = self.nav.prefix.parse::<usize>().ok();
                self.nav.clear_prefix();
                if let Some(percentage) = percentage {
                    self.jump(crate::navigation::percentage_page(
                        self.nav.count,
                        percentage,
                    ));
                } else {
                    self.notify("Type a percentage first, e.g. 50%");
                }
                continue;
            }
            if symbol == Some('$') {
                self.nav.clear_prefix();
                continue;
            }
            if let Some(digit) = digit {
                self.nav.digit(digit);
                continue;
            }
            if let Some(action) = shortcuts::reader_zoom_action(key, modifiers.shift, key_text) {
                let count = self.nav.take_count();
                self.zoom_steps(count, matches!(action, shortcuts::ZoomAction::Out));
                continue;
            }
            if key != Key::G {
                self.nav.pending_g = false;
            }
            match key {
                Key::J if modifiers.shift => {
                    let n = self.nav.take_count();
                    self.step(n as isize);
                }
                Key::K if modifiers.shift => {
                    let n = self.nav.take_count();
                    self.step(-(n as isize));
                }
                Key::J | Key::ArrowDown => {
                    let n = self.nav.take_count();
                    self.scroll(Vec2::new(0.0, 60.0 * n as f32), true);
                }
                Key::K | Key::ArrowUp => {
                    let n = self.nav.take_count();
                    self.scroll(Vec2::new(0.0, -60.0 * n as f32), true);
                }
                Key::H if modifiers.shift => {
                    let n = self.nav.take_count();
                    self.step(-(n as isize));
                }
                Key::L if modifiers.shift => {
                    let n = self.nav.take_count();
                    self.step(n as isize);
                }
                Key::H | Key::ArrowLeft => {
                    let n = self.nav.take_count();
                    self.scroll(Vec2::new(-60.0 * n as f32, 0.0), false);
                }
                Key::L | Key::ArrowRight => {
                    let n = self.nav.take_count();
                    self.scroll(Vec2::new(60.0 * n as f32, 0.0), false);
                }
                Key::PageDown | Key::CloseBracket => {
                    let n = self.nav.take_count();
                    self.step(n as isize);
                }
                Key::PageUp | Key::OpenBracket => {
                    let n = self.nav.take_count();
                    self.step(-(n as isize));
                }
                Key::Space => {
                    let n = self.nav.take_count();
                    self.scroll(
                        Vec2::new(
                            0.0,
                            self.viewport.y * if modifiers.shift { -0.9 } else { 0.9 } * n as f32,
                        ),
                        true,
                    );
                }
                Key::G if modifiers.shift => {
                    let page = if self.nav.prefix.is_empty() {
                        self.nav.count.saturating_sub(1)
                    } else {
                        self.nav.take_count() - 1
                    };
                    self.nav.clear_prefix();
                    self.jump(page);
                }
                Key::G if self.nav.pending_g => {
                    let page = self.nav.take_count() - 1;
                    self.jump(page);
                }
                Key::G => self.nav.pending_g = true,
                Key::Z if !modifiers.shift => self.nav.pending_z = true,
                Key::Home => self.jump(0),
                Key::End => self.jump(self.nav.count.saturating_sub(1)),
                Key::Enter if !self.nav.prefix.is_empty() => {
                    let page = self.nav.take_count() - 1;
                    self.jump(page);
                }
                Key::A => self.actual_size(),
                Key::W => {
                    self.reading.fit = Fit::Width;
                    self.dirty = true;
                }
                Key::E => {
                    self.reading.fit = Fit::Height;
                    self.dirty = true;
                }
                Key::P => {
                    self.reading.fit = Fit::Page;
                    self.pan = Vec2::ZERO;
                    self.dirty = true;
                }
                Key::R => {
                    let count = self.nav.take_count();
                    self.rotate_page(modifiers.shift, count);
                }
                Key::Slash => self.find(),
                Key::C if modifiers.shift => self.toggle_continuous(),
                Key::C => self.focus_contents(),
                Key::S => self.show_properties(context),
                Key::F => {
                    self.copy_link_hints = modifiers.shift;
                    let count = if self.copy_link_hints {
                        self.links.get(&self.nav.page).map_or(0, |links| {
                            self.visible_links
                                .indices
                                .iter()
                                .filter_map(|&index| links.get(index))
                                .filter(|link| matches!(&link.target, LinkTarget::Url(_)))
                                .count()
                        })
                    } else {
                        self.visible_links.indices.len()
                    };
                    self.hints.start(count);
                    if !self.hints.active {
                        self.notify(if self.copy_link_hints {
                            "No web links visible on this page"
                        } else {
                            "No links visible on this page"
                        });
                    } else if self.copy_link_hints {
                        self.notify("Copy link address · type a label · Escape to cancel");
                    }
                }
                Key::N | Key::F3 => {
                    let count = self.nav.take_count();
                    self.next_hits(modifiers.shift, count);
                }
                Key::M => {
                    self.nav.clear_prefix();
                    self.mark_action = Some(MarkAction::Set);
                }
                Key::Quote | Key::Backtick => {
                    self.nav.clear_prefix();
                    self.mark_action = Some(MarkAction::Jump);
                }
                Key::B => self.bookmark(),
                Key::Tab => {
                    self.hints.cancel();
                    self.library.sidebar = !self.library.sidebar;
                    self.dirty = true;
                }
                Key::T => {
                    self.hints.cancel();
                    self.library.thumbnails = !self.library.thumbnails;
                    self.library.sidebar = true;
                    self.reveal_page = Some(self.nav.page);
                    self.dirty = true;
                }
                Key::D => {
                    self.library.dark = !self.library.dark;
                    set_theme(context, self.library.dark);
                    self.dirty = true;
                }
                Key::I => {
                    self.library.night = !self.library.night;
                    self.dirty = true;
                }
                Key::V if self.nav.count > 0 => {
                    self.hints.cancel();
                    if self.selection.active {
                        self.selection.cancel();
                    } else {
                        self.selection.start();
                        self.selection_page = self.nav.page;
                        self.worker.inspect(self.nav.page);
                        self.notify(
                            "Drag to select · double-click word / triple-click line · y to copy · Escape to exit",
                        );
                    }
                }
                Key::Y if self.selection.active => self.copy_selection(context),
                Key::Y => self.worker.copy(self.nav.page),
                Key::O => self.choose_file(),
                Key::Colon => self.open_goto(),
                Key::Questionmark | Key::F1 => self.open_help(context),
                Key::F11 => {
                    let fullscreen =
                        context.input(|input| input.viewport().fullscreen.unwrap_or(false));
                    context.send_viewport_cmd(egui::ViewportCommand::Fullscreen(!fullscreen));
                }
                _ => self.nav.clear_prefix(),
            }
            if !matches!(key, Key::G | Key::Z) {
                self.nav.clear_prefix();
            }
        }
    }

    fn toolbar(&mut self, context: &egui::Context) {
        self.input_buttons = shortcuts::InputButtons::default();
        egui::TopBottomPanel::top("toolbar")
            .frame(appearance::panel(context, 4))
            .exact_height(56.0)
            .show(context, |ui| {
                let compact = ui.available_width() < 1000.0;
                toolbar::horizontal_bar(ui, "reader-controls", |ui| {
                    ui.add_space(12.0);
                    appearance::logo(ui, 30.0);
                    if !compact {
                        ui.label(RichText::new("Pagewing").size(19.0).strong());
                    }
                    ui.add_space(16.0);
                    if ui
                        .button("Open")
                        .on_hover_text(self.library.shortcuts.label("open"))
                        .clicked()
                    {
                        self.choose_file();
                    }
                    if !self.reading.path.as_os_str().is_empty()
                        && appearance::tool(ui, appearance::Icon::Close, "Close document", false)
                            .on_hover_text(self.library.shortcuts.label("close"))
                            .clicked()
                    {
                        self.close_document(context);
                    }
                    if self.nav.count > 0 {
                        if appearance::tool(
                            ui,
                            appearance::Icon::Sidebar,
                            "Sidebar",
                            self.library.sidebar,
                        )
                        .on_hover_text(self.library.shortcuts.label("sidebar"))
                        .clicked()
                        {
                            self.library.sidebar = !self.library.sidebar;
                            self.dirty = true;
                        }
                        self.input_buttons.comments = Some(
                            appearance::tool(
                                ui,
                                appearance::Icon::Comments,
                                "Comments",
                                self.comments.open,
                            )
                            .on_hover_text(self.library.shortcuts.label("comments"))
                            .id,
                        );
                        if !self.selection.rects.is_empty() && ui.button("Add comment").clicked() {
                            self.add_comment();
                        }
                        ui.separator();
                        if appearance::tool(ui, appearance::Icon::Previous, "Previous page", false)
                            .on_hover_text(self.library.shortcuts.label("previous_page"))
                            .clicked()
                        {
                            self.step(-1);
                        }
                        if appearance::tool(ui, appearance::Icon::Next, "Next page", false)
                            .on_hover_text(self.library.shortcuts.label("next_page"))
                            .clicked()
                        {
                            self.step(1);
                        }
                        ui.separator();
                        if appearance::tool(ui, appearance::Icon::Rotate, "Rotate page", false)
                            .on_hover_text(self.library.shortcuts.label("rotate"))
                            .clicked()
                        {
                            self.rotate_page(false, 1);
                        }
                    }
                    toolbar::right_group(ui, if self.nav.count > 0 { 170.0 } else { 90.0 }, |ui| {
                        ui.add_space(12.0);
                        self.input_buttons.help = Some(
                            appearance::tool(
                                ui,
                                appearance::Icon::Help,
                                "Keyboard shortcuts",
                                false,
                            )
                            .id,
                        );
                        if appearance::tool(
                            ui,
                            appearance::Icon::Theme,
                            if self.library.dark {
                                "Switch to light theme"
                            } else {
                                "Switch to dark theme"
                            },
                            false,
                        )
                        .clicked()
                        {
                            self.library.dark = !self.library.dark;
                            set_theme(context, self.library.dark);
                            self.dirty = true;
                        }
                        if self.nav.count > 0 {
                            self.input_buttons.find = Some(
                                appearance::tool(
                                    ui,
                                    appearance::Icon::Search,
                                    "Find in document",
                                    self.search_open,
                                )
                                .on_hover_text(self.library.shortcuts.label("find"))
                                .id,
                            );
                            if appearance::tool(
                                ui,
                                appearance::Icon::Bookmark,
                                "Bookmark",
                                self.reading.bookmarks.contains(&self.nav.page),
                            )
                            .on_hover_text(self.library.shortcuts.label("bookmark"))
                            .clicked()
                            {
                                self.bookmark();
                            }
                        }
                    });
                });
            });
    }

    fn search_bar(&mut self, context: &egui::Context) {
        if !self.search_open {
            return;
        }
        egui::TopBottomPanel::top("search")
            .frame(appearance::panel(context, 4))
            .exact_height(42.0)
            .show(context, |ui| {
                toolbar::horizontal_bar(ui, "search-controls", |ui| {
                    ui.add_space(16.0);
                    ui.label("Find");
                    let (response, focused_now) = shortcuts::text_input(
                        ui,
                        egui::Id::new("pagewing-search-input"),
                        &mut self.search_focus,
                        &mut self.deferred_input,
                        egui::TextEdit::singleline(&mut self.query)
                            .desired_width((ui.available_width() - 600.0).clamp(100.0, 280.0))
                            .hint_text("Search this document…"),
                    );
                    let case_changed = ui
                        .selectable_label(self.library.search_options.match_case, "Aa")
                        .on_hover_text("Match case · Alt+C")
                        .clicked();
                    let word_changed = ui
                        .selectable_label(self.library.search_options.whole_word, "Word")
                        .on_hover_text("Whole words · Alt+W")
                        .clicked();
                    if case_changed {
                        self.library.search_options.match_case =
                            !self.library.search_options.match_case;
                    }
                    if word_changed {
                        self.library.search_options.whole_word =
                            !self.library.search_options.whole_word;
                    }
                    if case_changed || word_changed {
                        self.dirty = true;
                        response.request_focus();
                    }
                    if response.changed() || case_changed || word_changed {
                        self.change_search();
                    }
                    // egui compares focus with the previous frame. Acquiring
                    // and releasing it with Enter in one frame is not lost_focus.
                    if (response.lost_focus() || focused_now)
                        && let Some(reverse) = ui.input(|input| {
                            input.events.iter().find_map(|event| match event {
                                egui::Event::Key {
                                    key: Key::Enter,
                                    pressed: true,
                                    modifiers,
                                    ..
                                } => Some(modifiers.shift),
                                _ => None,
                            })
                        })
                    {
                        self.next_hit(reverse);
                    }
                    if ui.button("Prev").clicked() {
                        self.next_hit(true);
                    }
                    if ui.button("Next").clicked() {
                        self.next_hit(false);
                    }
                    let index = self.hit_index.map(|i| i + 1).unwrap_or(0);
                    ui.label(format!(
                        "{index} / {}{} matches",
                        self.hits.len(),
                        if self.search_limited { "+" } else { "" }
                    ));
                    if !self.search_complete && !self.query.trim().is_empty() {
                        ui.spinner();
                        if ui.available_width() > 180.0 {
                            ui.weak(format!("{} / {} pages", self.scanned, self.scan_total));
                        }
                    }
                    if ui.button("Results").clicked() {
                        self.library.sidebar = true;
                        self.dirty = true;
                    }
                    if ui.button("×").clicked() {
                        self.search_open = false;
                        self.pending_search_jump = None;
                    }
                });
            });
    }

    fn canvas(&mut self, context: &egui::Context) {
        let background = appearance::canvas(self.library.dark);
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
                self.viewport = rect.size();
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
                    if self.library.night {
                        Color32::BLACK
                    } else {
                        Color32::WHITE
                    },
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
        if self.file_changed && self.nav.count > 0 {
            self.file_change_notice(context, canvas.response.rect);
        }
        // Honor this frame's keys and buttons before deciding whether the
        // document is safe to replace (e.g. starting a comment or selection).
        self.reload_if_changed(context);
    }

    fn file_change_notice(&mut self, context: &egui::Context, canvas: egui::Rect) {
        egui::Area::new(egui::Id::new("file-change-notice"))
            .order(egui::Order::Foreground)
            .fixed_pos(canvas.center_top() + Vec2::new(0.0, 12.0))
            .pivot(egui::Align2::CENTER_TOP)
            .show(context, |ui| {
                egui::Frame::popup(ui.style()).show(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.label("PDF changed on disk.");
                        let reload = ui
                            .add_enabled(!self.loading, egui::Button::new("Reload"))
                            .on_hover_text(format!(
                                "{} — keep current reading position",
                                self.library.shortcuts.label("reload")
                            ));
                        self.input_buttons.reload_changed = Some(reload.id);
                        self.input_buttons.dismiss_change = Some(ui.button("Dismiss").id);
                    });
                });
            });
    }

    fn highlights(
        &self,
        painter: &egui::Painter,
        page: usize,
        page_rect: egui::Rect,
        dimensions: [f32; 2],
    ) {
        let clip = painter.clip_rect().intersect(page_rect);
        if !clip.is_positive() {
            return;
        }
        painter.extend(crate::text::page_hits(&self.hits, page).flat_map(|index| {
            let hit = &self.hits[index];
            let color = if self.hit_index == Some(index) {
                CURRENT_SEARCH_COLOR
            } else {
                SEARCH_COLOR
            };
            hit.rects.iter().filter_map(move |rect| {
                let rect = self
                    .link_rect(*rect, page_rect, dimensions)
                    .intersect(page_rect);
                (rect.is_positive() && clip.intersects(rect))
                    .then(|| egui::Shape::rect_filled(rect, 1.0, color))
            })
        }));
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
        self.pending_search_jump = None;
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
        crate::link_visibility::screen_rect(
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
        let mut other_links = crate::link_visibility::LinkVisibility::default();
        let visible = if page == self.nav.page {
            &mut self.visible_links
        } else {
            &mut other_links
        };
        let changed = visible.update(
            crate::link_visibility::LinkView {
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
            &other_links
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
                response.context_menu(|ui| {
                    let copy = ui.button("Copy link address");
                    if response.secondary_clicked() {
                        copy.request_focus();
                    }
                    ui.memory_mut(|memory| {
                        memory.set_focus_lock_filter(
                            copy.id,
                            egui::EventFilter {
                                tab: true,
                                horizontal_arrows: true,
                                vertical_arrows: true,
                                ..Default::default()
                            },
                        );
                    });
                    if copy.clicked() {
                        ui.ctx().copy_text(url.clone());
                        copied = true;
                        ui.close();
                    }
                });
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

    fn welcome(&mut self, ui: &mut egui::Ui) {
        egui::ScrollArea::vertical()
            .id_salt("welcome")
            .auto_shrink([false, false])
            .show(ui, |ui| {
                let width = (ui.available_width() - 40.0).clamp(160.0, 460.0);
                ui.vertical_centered(|ui| {
                    ui.add_space((ui.available_height() * 0.10).clamp(24.0, 80.0));
                    appearance::logo(ui, 68.0);
                    ui.add_space(18.0);
                    appearance::section(ui, "P A G E W I N G");
                    ui.add_space(10.0);
                    ui.label(RichText::new("A little room to read.").size(32.0).strong());
                    ui.add_space(8.0);
                    ui.weak("Your documents. A quieter place to focus.");
                    ui.add_space(24.0);
                    if self.loading {
                        ui.spinner();
                        ui.label("Opening document…");
                    } else if ui
                        .add(
                            egui::Button::new(
                                RichText::new("Open a document")
                                    .color(Color32::WHITE)
                                    .strong(),
                            )
                            .fill(ACCENT)
                            .corner_radius(10)
                            .min_size(Vec2::new(200.0, 44.0)),
                        )
                        .clicked()
                    {
                        self.choose_file();
                    }
                    ui.add_space(10.0);
                    ui.label(
                        RichText::new(format!(
                            "Drop a PDF here  ·  {}",
                            self.library.shortcuts.label("open")
                        ))
                        .small()
                        .weak(),
                    );
                    ui.add_space(36.0);
                    ui.allocate_ui_with_layout(
                        Vec2::new(width, 0.0),
                        egui::Layout::top_down(egui::Align::Min),
                        |ui| {
                            ui.set_width(width);
                            if self.library.recent.is_empty() {
                                ui.separator();
                                ui.add_space(12.0);
                                ui.horizontal_wrapped(|ui| {
                                    ui.weak("Made for your keyboard");
                                    ui.label(
                                        RichText::new("j / k")
                                            .monospace()
                                            .color(appearance::accent(ui)),
                                    );
                                    ui.weak("scroll");
                                    ui.label(
                                        RichText::new("/")
                                            .monospace()
                                            .color(appearance::accent(ui)),
                                    );
                                    ui.weak("find");
                                    ui.label(
                                        RichText::new("?")
                                            .monospace()
                                            .color(appearance::accent(ui)),
                                    );
                                    ui.weak("shortcuts");
                                });
                                return;
                            }
                            appearance::section(ui, "CONTINUE READING");
                            ui.add_space(6.0);
                            let mut open = None;
                            for (index, item) in self.library.recent.iter().take(4).enumerate() {
                                let name =
                                    item.path.file_name().unwrap_or_default().to_string_lossy();
                                let row = egui::Frame::new()
                                    .fill(ui.visuals().panel_fill)
                                    .corner_radius(10)
                                    .inner_margin(14)
                                    .show(ui, |ui| {
                                        ui.set_width((width - 28.0).max(0.0));
                                        ui.horizontal(|ui| {
                                            let (rect, _) = ui.allocate_exact_size(
                                                Vec2::new(28.0, 32.0),
                                                egui::Sense::hover(),
                                            );
                                            let color = appearance::accent(ui);
                                            ui.painter().rect_stroke(
                                                rect.shrink2(Vec2::new(5.0, 2.0)),
                                                3.0,
                                                egui::Stroke::new(1.3_f32, color),
                                                egui::StrokeKind::Inside,
                                            );
                                            for offset in [11.0, 16.0, 21.0] {
                                                ui.painter().line_segment(
                                                    [
                                                        rect.min + Vec2::new(10.0, offset),
                                                        rect.min + Vec2::new(18.0, offset),
                                                    ],
                                                    egui::Stroke::new(1.0_f32, color),
                                                );
                                            }
                                            ui.vertical(|ui| {
                                                ui.set_width((width - 80.0).max(0.0));
                                                ui.add(
                                                    egui::Label::new(RichText::new(name).strong())
                                                        .truncate(),
                                                );
                                                ui.label(
                                                    RichText::new(format!(
                                                        "Page {}  ·  Resume reading",
                                                        item.page + 1
                                                    ))
                                                    .small()
                                                    .weak(),
                                                );
                                            });
                                        });
                                    });
                                let response = ui
                                    .interact(
                                        row.response.rect,
                                        ui.id().with(("recent", index)),
                                        egui::Sense::click(),
                                    )
                                    .on_hover_cursor(egui::CursorIcon::PointingHand)
                                    .on_hover_text(item.path.display().to_string());
                                response.widget_info(|| {
                                    egui::WidgetInfo::labeled(
                                        egui::WidgetType::Button,
                                        true,
                                        item.path.display().to_string(),
                                    )
                                });
                                if response.hovered() || response.has_focus() {
                                    ui.painter().rect_stroke(
                                        row.response.rect,
                                        10.0,
                                        egui::Stroke::new(1.0_f32, appearance::accent(ui)),
                                        egui::StrokeKind::Inside,
                                    );
                                }
                                if response.clicked() {
                                    open = Some(item.path.clone());
                                }
                            }
                            if let Some(path) = open {
                                self.open(path, ui.ctx());
                            }
                            ui.add_space(6.0);
                            if ui
                                .add(
                                    egui::Button::new(format!(
                                        "All recent documents  ·  {}",
                                        self.library.shortcuts.label("recent")
                                    ))
                                    .frame(false),
                                )
                                .clicked()
                            {
                                self.toggle_recent(ui.ctx());
                            }
                        },
                    );
                    ui.add_space(30.0);
                });
            });
    }

    fn status(&mut self, context: &egui::Context) {
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
                        self.input_buttons.page = Some(
                            ui.add(
                                egui::Button::new(format!(
                                    "{} / {}",
                                    if self.reading.facing {
                                        let pages = self.spread().pages;
                                        if pages.len() == 2 {
                                            format!(
                                                "{}–{}",
                                                self.page_name(pages.start),
                                                self.page_name(pages.end - 1)
                                            )
                                        } else {
                                            self.page_name(self.nav.page)
                                        }
                                    } else {
                                        self.page_name(self.nav.page)
                                    },
                                    self.nav.count
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
                        context.request_repaint_after(Duration::from_secs(5) - at.elapsed());
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

    fn dialogs(&mut self, context: &egui::Context) {
        self.recent_dialog(context);
        self.zoom_dialog(context);
        self.page_dialog(context);
        self.password_dialog(context);
        self.help_window(context);
        if let Some(error) = self.configuration_error.clone() {
            egui::TopBottomPanel::bottom("configuration-error").show(context, |ui| {
                ui.horizontal_wrapped(|ui| {
                    ui.colored_label(Color32::from_rgb(200, 75, 55), error);
                    if ui.button("Dismiss").clicked() {
                        self.configuration_error = None;
                    }
                });
            });
        }
        if let Some(error) = self.error.clone() {
            egui::TopBottomPanel::bottom("error").show(context, |ui| {
                ui.horizontal_wrapped(|ui| {
                    ui.colored_label(Color32::from_rgb(200, 75, 55), error);
                    if ui.button("Dismiss").clicked() {
                        self.error = None;
                    }
                    if self.nav.count > 0
                        && ui
                            .button(if self.reloading {
                                "Retry reload"
                            } else {
                                "Retry page"
                            })
                            .clicked()
                    {
                        if self.reloading {
                            self.reload();
                        } else {
                            self.requested = None;
                            self.error = None;
                        }
                    }
                });
            });
        }
    }
}

impl eframe::App for Pagewing {
    fn update(&mut self, context: &egui::Context, _: &mut eframe::Frame) {
        #[cfg(target_os = "macos")]
        self.macos_open_documents(context);
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
        self.synctex_events(context);
        if context.input(|input| input.viewport().close_requested())
            && self.comment_change_blocked()
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
        {
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
            if self.saved.elapsed() >= Duration::from_secs(2) {
                self.save();
            } else {
                context.request_repaint_after(Duration::from_secs(2) - self.saved.elapsed());
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
