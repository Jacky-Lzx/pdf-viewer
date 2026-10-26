use crate::{
    cache::TextureCache,
    document::RenderKey,
    links::{LinkHints, LinkTarget, PageLink},
    navigation::{Fit, Navigation},
    render_plan::RenderView,
    selection::TextSelection,
    state::{self, Library, ReadingMark, ReadingState},
    text::SearchHit,
    worker::{Event, Worker},
};
use eframe::egui::{self, Color32, Key, RichText, Vec2};
use std::{
    collections::HashMap,
    path::PathBuf,
    sync::mpsc::{self, Receiver},
    time::{Duration, Instant},
};

const ACCENT: Color32 = Color32::from_rgb(41, 114, 92);

#[derive(Clone, Copy)]
enum MarkAction {
    Set,
    Jump,
}

pub struct Folio {
    worker: Worker,
    library: Library,
    config: PathBuf,
    reading: ReadingState,
    nav: Navigation,
    dimensions: Vec<[f32; 2]>,
    outline: crate::outline_view::OutlineView,
    labels: crate::page_labels::PageLabels,
    links: HashMap<usize, Vec<PageLink>>,
    visible_links: Vec<PageLink>,
    hints: LinkHints,
    mark_action: Option<MarkAction>,
    pending_offset: Option<[f32; 2]>,
    pending_focus: Option<hayro::kurbo::Rect>,
    pending_destination: Option<(crate::destination::Destination, [f64; 2])>,
    selection: TextSelection,
    cache: TextureCache,
    id: u64,
    loading: bool,
    reloading: bool,
    error: Option<String>,
    message: Option<(String, Instant)>,
    pan: Vec2,
    overflow: Vec2,
    viewport: Vec2,
    scale: f32,
    requested: Option<Vec<RenderKey>>,
    render_ms: f64,
    dialog: Option<Receiver<Option<PathBuf>>>,
    search_open: bool,
    search_focus: bool,
    query: String,
    search_due: Option<Instant>,
    revision: u64,
    hits: Vec<SearchHit>,
    hit_index: Option<usize>,
    reveal_hit: Option<usize>,
    pending_search_jump: Option<bool>,
    scanned: usize,
    scan_total: usize,
    search_complete: bool,
    search_limited: bool,
    goto: String,
    goto_open: bool,
    goto_focus: bool,
    help: bool,
    password: String,
    password_open: bool,
    saved: Instant,
    dirty: bool,
}

impl Folio {
    pub fn new(cc: &eframe::CreationContext<'_>, path: Option<PathBuf>) -> Self {
        let config = state::config_path();
        let library = Library::load(&config);
        set_theme(&cc.egui_ctx, library.dark);
        crate::fonts::install(cc.egui_ctx.clone());
        let mut app = Self {
            worker: Worker::new(cc.egui_ctx.clone()),
            library,
            config,
            reading: ReadingState::default(),
            nav: Navigation::default(),
            dimensions: Vec::new(),
            outline: crate::outline_view::OutlineView::default(),
            labels: crate::page_labels::PageLabels::default(),
            links: HashMap::new(),
            visible_links: Vec::new(),
            hints: LinkHints::default(),
            mark_action: None,
            pending_offset: None,
            pending_focus: None,
            pending_destination: None,
            selection: TextSelection::default(),
            cache: TextureCache::default(),
            id: 0,
            loading: false,
            reloading: false,
            error: None,
            message: None,
            pan: Vec2::ZERO,
            overflow: Vec2::ZERO,
            viewport: Vec2::splat(600.0),
            scale: 1.0,
            requested: None,
            render_ms: 0.0,
            dialog: None,
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
            goto: String::new(),
            goto_open: false,
            goto_focus: false,
            help: false,
            password: String::new(),
            password_open: false,
            saved: Instant::now(),
            dirty: false,
        };
        if let Some(path) = path {
            app.open(path, &cc.egui_ctx);
        }
        app
    }

    fn open(&mut self, path: PathBuf, context: &egui::Context) {
        self.remember();
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
        self.reading.rotation %= 4;
        self.id += 1;
        self.nav = Navigation::default();
        self.dimensions.clear();
        self.outline.set_entries(Vec::new());
        self.labels = crate::page_labels::PageLabels::default();
        self.links.clear();
        self.visible_links.clear();
        self.hints.cancel();
        self.selection.cancel();
        self.mark_action = None;
        self.cache.clear();
        self.requested = None;
        self.pan = Vec2::ZERO;
        self.pending_offset = Some(self.reading.offset);
        self.pending_focus = None;
        self.pending_destination = None;
        self.hits.clear();
        self.hit_index = None;
        self.reveal_hit = None;
        self.pending_search_jump = None;
        self.query.clear();
        self.search_due = None;
        self.search_open = false;
        self.scanned = 0;
        self.scan_total = 0;
        self.search_complete = true;
        self.search_limited = false;
        self.revision += 1;
        self.loading = true;
        self.reloading = false;
        self.error = None;
        self.password.clear();
        self.password_open = false;
        self.worker
            .open(self.id, self.reading.path.clone(), String::new());
        context.send_viewport_cmd(egui::ViewportCommand::Title(format!(
            "{} — Folio",
            self.filename()
        )));
    }

    fn filename(&self) -> String {
        self.reading
            .path
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_else(|| "Folio".into())
    }

    fn reload(&mut self) {
        if self.nav.count == 0 || self.loading {
            return;
        }
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

    fn page_name(&self, page: usize) -> String {
        let physical = (page + 1).to_string();
        match self.labels.label(page) {
            Some(label) if !label.is_empty() && label != physical => {
                let mut short: String = label.chars().take(12).collect();
                if label.chars().count() > 12 {
                    short.push('…');
                }
                format!("{short} · {}", page + 1)
            }
            _ => physical,
        }
    }

    fn remember(&mut self) {
        if self.nav.count > 0 {
            self.reading.page = self.nav.page;
            self.library.remember(self.reading.clone());
        }
    }

    fn current_offset(&self) -> [f32; 2] {
        state::normalized_offset([self.pan.x, self.pan.y], [self.overflow.x, self.overflow.y])
    }

    fn mark(&mut self, name: char, action: MarkAction) {
        if self.nav.count == 0 {
            return;
        }
        match action {
            MarkAction::Set => {
                self.reading.marks.insert(
                    name,
                    ReadingMark {
                        page: self.nav.page,
                        fit: self.reading.fit,
                        rotation: self.reading.rotation,
                        offset: self.current_offset(),
                    },
                );
                self.dirty = true;
                self.notify(format!("Mark {name} saved · page {}", self.nav.page + 1));
            }
            MarkAction::Jump => {
                if let Some(mark) = self.reading.marks.get(&name).cloned() {
                    self.pending_search_jump = None;
                    self.jump(mark.page);
                    self.hints.cancel();
                    self.reading.fit = mark.fit;
                    self.reading.rotation = mark.rotation % 4;
                    self.pending_offset = Some(mark.offset);
                    self.pending_focus = None;
                    self.dirty = true;
                } else {
                    self.notify(format!("Mark {name} is not set · use m{name} to save it"));
                }
            }
        }
    }

    fn save(&mut self) {
        self.remember();
        if let Err(error) = self.library.save(&self.config) {
            self.notify(format!("Could not save reading progress: {error}"));
        }
        self.saved = Instant::now();
        self.dirty = false;
    }

    fn notify(&mut self, text: impl Into<String>) {
        self.message = Some((text.into(), Instant::now()));
    }

    fn choose_file(&mut self) {
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
        self.pending_destination = None;
        if self.nav.jump(page) {
            self.page_changed();
        }
    }
    fn step(&mut self, delta: isize) {
        self.pending_destination = None;
        if self.nav.step(delta) {
            self.page_changed();
        }
    }
    fn page_changed(&mut self) {
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
        if delta != Vec2::ZERO {
            self.hints.cancel();
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

    fn zoom(&mut self, factor: f32) {
        self.zoom_at(factor, self.viewport * 0.5);
    }

    fn zoom_at(&mut self, factor: f32, pivot: Vec2) {
        let Some(&dimensions) = self.dimensions.get(self.nav.page) else {
            return;
        };
        if !factor.is_finite() || factor <= 0.0 {
            return;
        }
        self.hints.cancel();
        let dimensions = if self.reading.rotation % 2 == 1 {
            [dimensions[1], dimensions[0]]
        } else {
            dimensions
        };
        let viewport = [self.viewport.x, self.viewport.y];
        let before = self.reading.fit.scale(dimensions, viewport);
        let after = Fit::Custom(before * factor).scale(dimensions, viewport);
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
        self.dirty = true;
    }

    fn find(&mut self) {
        self.nav.clear_prefix();
        self.hints.cancel();
        self.selection.cancel();
        self.mark_action = None;
        self.search_open = true;
        self.search_focus = true;
    }

    fn next_hit(&mut self, reverse: bool) {
        if self.hits.is_empty() {
            self.pending_search_jump =
                (!self.search_complete || self.search_due.is_some()).then_some(reverse);
            return;
        }
        let index = match self.hit_index {
            Some(index) if reverse => (index + self.hits.len() - 1) % self.hits.len(),
            Some(index) => (index + 1) % self.hits.len(),
            None => match crate::text::initial_match(
                &self.hits,
                self.nav.page,
                reverse,
                self.scanned,
                self.search_complete,
            ) {
                Some(index) => index,
                None => {
                    self.pending_search_jump = (!self.search_complete).then_some(reverse);
                    return;
                }
            },
        };
        self.pending_search_jump = None;
        self.select_hit(index);
    }

    fn select_hit(&mut self, index: usize) {
        self.pending_search_jump = None;
        self.hit_index = Some(index);
        self.reveal_hit = Some(index);
        let page = self.hits[index].page;
        let rect = self.hits[index].rects.first().copied();
        self.jump(page);
        self.pending_focus = rect;
    }

    fn events(&mut self, context: &egui::Context) {
        while let Ok(event) = self.worker.events.try_recv() {
            match event {
                Event::PageText { id, page, text }
                    if id == self.id && page == self.nav.page && self.selection.active =>
                {
                    if text.spans.is_empty() {
                        self.notify("This page has no selectable text");
                    }
                    self.selection.set_text(text);
                }
                Event::Links { id, page, links } if id == self.id => {
                    if self.links.len() >= 64 {
                        self.links.retain(|page, _| *page == self.nav.page);
                    }
                    self.links.insert(page, links);
                }
                Event::Opened {
                    id,
                    dimensions,
                    outline,
                    labels,
                } if id == self.id => {
                    let reloaded = self.reloading;
                    if reloaded {
                        self.reading.page = self.nav.page;
                        self.pending_offset = Some(self.current_offset());
                    }
                    self.reloading = false;
                    self.loading = false;
                    self.error = None;
                    self.cache.clear();
                    self.requested = None;
                    self.links.clear();
                    self.visible_links.clear();
                    self.hints.cancel();
                    self.selection.cancel();
                    self.pending_focus = None;
                    if reloaded {
                        self.nav.resize(dimensions.len());
                    } else {
                        self.nav = Navigation::new(dimensions.len(), self.reading.page);
                    }
                    self.reading
                        .bookmarks
                        .retain(|page| *page < dimensions.len());
                    self.reading.marks.retain(|name, mark| {
                        name.is_ascii_lowercase() && mark.page < dimensions.len()
                    });
                    self.dimensions = dimensions;
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
                    bitmap,
                    milliseconds,
                } if id == self.id => {
                    let wanted = self
                        .requested
                        .as_ref()
                        .is_some_and(|keys| keys.contains(&key));
                    if (!wanted && key.page != self.nav.page)
                        || key.rotation != self.reading.rotation
                        || key.night != self.library.night
                        || !self.cache.can_insert(key, bitmap.rgba.len())
                    {
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
                    let image =
                        egui::ColorImage::from_rgba_premultiplied(bitmap.size, &bitmap.rgba);
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
                    if let Some(reverse) = self.pending_search_jump.take() {
                        self.next_hit(reverse);
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
                    if opening && self.loading && self.reloading {
                        self.restart_search();
                        self.requested = None;
                    }
                    if opening {
                        self.loading = false;
                    }
                    if opening && message.contains("Decryption") {
                        self.password_open = true;
                    }
                    self.error = Some(message);
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
        let typing = context.memory(|memory| {
            [
                "folio-search-input",
                "folio-page-input",
                "folio-password-input",
                "folio-outline-input",
            ]
            .iter()
            .any(|name| memory.has_focus(egui::Id::new(name)))
        });
        for event in context.input(|input| input.events.clone()) {
            if matches!(event, egui::Event::Copy) && !typing && self.selection.active {
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
            if key == Key::Escape {
                self.hints.cancel();
                self.selection.cancel();
                self.mark_action = None;
                self.help = false;
                self.goto_open = false;
                self.search_open = false;
                self.pending_search_jump = None;
                self.password_open = false;
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
                self.find();
                continue;
            }
            if modifiers.command && key == Key::R {
                self.reload();
                continue;
            }
            if typing || self.help || self.password_open || self.goto_open {
                continue;
            }
            if modifiers.command && key == Key::C && self.selection.active {
                self.copy_selection(context);
                context.input_mut(|input| {
                    input.consume_key(modifiers, key);
                });
                continue;
            }
            context.input_mut(|input| {
                input.consume_key(modifiers, key);
            });
            if let Some(action) = self.mark_action.take() {
                if !modifiers.ctrl && !modifiers.command && !modifiers.alt {
                    let name = key.name().to_ascii_lowercase();
                    if name.len() == 1 && name.as_bytes()[0].is_ascii_lowercase() {
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
                    && let Some(link) = self.visible_links.get(index)
                {
                    self.follow_link(link.target.clone(), context);
                }
                continue;
            }
            if modifiers.ctrl {
                let n = self.nav.take_count() as f32;
                match key {
                    Key::D => self.scroll(Vec2::new(0.0, self.viewport.y * 0.5 * n), true),
                    Key::U => self.scroll(Vec2::new(0.0, -self.viewport.y * 0.5 * n), true),
                    Key::F => self.scroll(Vec2::new(0.0, self.viewport.y * 0.9 * n), true),
                    Key::B => self.scroll(Vec2::new(0.0, -self.viewport.y * 0.9 * n), true),
                    Key::I if self.nav.history(true) => self.page_changed(),
                    _ => {}
                }
                continue;
            }
            if modifiers.alt {
                self.nav.clear_prefix();
                if matches!(key, Key::ArrowLeft | Key::ArrowRight)
                    && self.nav.history(key == Key::ArrowRight)
                {
                    self.page_changed();
                }
                continue;
            }
            if modifiers.command {
                self.nav.clear_prefix();
                continue;
            }
            if let Some(digit) = match key {
                Key::Num0 => Some('0'),
                Key::Num1 => Some('1'),
                Key::Num2 => Some('2'),
                Key::Num3 => Some('3'),
                Key::Num4 => Some('4'),
                Key::Num5 => Some('5'),
                Key::Num6 => Some('6'),
                Key::Num7 => Some('7'),
                Key::Num8 => Some('8'),
                Key::Num9 => Some('9'),
                _ => None,
            } {
                self.nav.digit(digit);
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
                Key::Home => self.jump(0),
                Key::End => self.jump(self.nav.count.saturating_sub(1)),
                Key::Enter if !self.nav.prefix.is_empty() => {
                    let page = self.nav.take_count() - 1;
                    self.jump(page);
                }
                Key::Plus | Key::Equals => self.zoom(1.2),
                Key::Minus => self.zoom(1.0 / 1.2),
                Key::W => {
                    self.reading.fit = Fit::Width;
                    self.dirty = true;
                }
                Key::P => {
                    self.reading.fit = Fit::Page;
                    self.pan = Vec2::ZERO;
                    self.dirty = true;
                }
                Key::R => {
                    self.reading.rotation =
                        (self.reading.rotation + if modifiers.shift { 3 } else { 1 }) % 4;
                    self.pan = Vec2::ZERO;
                    self.dirty = true;
                }
                Key::Slash => self.find(),
                Key::F => {
                    self.hints.start(self.visible_links.len());
                    if !self.hints.active {
                        self.notify("No links visible on this page");
                    }
                }
                Key::N | Key::F3 => self.next_hit(modifiers.shift),
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
                    self.library.sidebar = !self.library.sidebar;
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
                        self.worker.inspect(self.nav.page);
                        self.notify(
                            "Drag to select text · y or Ctrl/Cmd+C to copy · Escape to exit",
                        );
                    }
                }
                Key::Y if self.selection.active => self.copy_selection(context),
                Key::Y => self.worker.copy(self.nav.page),
                Key::O => self.choose_file(),
                Key::Colon => {
                    self.goto_open = true;
                    self.goto_focus = true;
                    self.goto.clear();
                }
                Key::Questionmark | Key::F1 => self.help = true,
                Key::F11 => {
                    let fullscreen =
                        context.input(|input| input.viewport().fullscreen.unwrap_or(false));
                    context.send_viewport_cmd(egui::ViewportCommand::Fullscreen(!fullscreen));
                }
                _ => self.nav.clear_prefix(),
            }
            if key != Key::G {
                self.nav.clear_prefix();
            }
        }
    }

    fn toolbar(&mut self, context: &egui::Context) {
        egui::TopBottomPanel::top("toolbar")
            .exact_height(56.0)
            .show(context, |ui| {
                let compact = ui.available_width() < 1100.0;
                ui.horizontal_centered(|ui| {
                    ui.add_space(12.0);
                    ui.label(RichText::new("F").size(28.0).strong().color(ACCENT));
                    if !compact {
                        ui.label(RichText::new("folio").size(21.0).strong());
                    }
                    ui.add_space(16.0);
                    if ui
                        .button("Open")
                        .on_hover_text("Open PDF · Ctrl/Cmd+O")
                        .clicked()
                    {
                        self.choose_file();
                    }
                    if self.nav.count > 0 {
                        if ui.button("Sidebar").on_hover_text("Tab").clicked() {
                            self.library.sidebar = !self.library.sidebar;
                            self.dirty = true;
                        }
                        ui.separator();
                        if ui
                            .button("‹")
                            .on_hover_text("Previous page · K / [")
                            .clicked()
                        {
                            self.step(-1);
                        }
                        if ui
                            .button(format!(
                                "{} / {}",
                                self.page_name(self.nav.page),
                                self.nav.count
                            ))
                            .on_hover_text(format!(
                                "Go to page · :\n{}",
                                self.labels.label(self.nav.page).unwrap_or_default()
                            ))
                            .clicked()
                        {
                            self.goto_open = true;
                            self.goto_focus = true;
                            self.goto.clear();
                        }
                        if ui.button("›").on_hover_text("Next page · J / ]").clicked() {
                            self.step(1);
                        }
                        ui.separator();
                        if ui.button("−").clicked() {
                            self.zoom(1.0 / 1.2);
                        }
                        let percent = self.scale * 100.0;
                        ui.label(if percent < 0.01 {
                            "<0.01%".to_owned()
                        } else if percent < 1.0 {
                            format!("{percent:.2}%")
                        } else {
                            format!("{percent:.0}%")
                        });
                        if ui.button("+").clicked() {
                            self.zoom(1.2);
                        }
                        if ui.button("Fit").on_hover_text("Fit page · p").clicked() {
                            self.reading.fit = Fit::Page;
                            self.pan = Vec2::ZERO;
                            self.dirty = true;
                        }
                        if !compact && ui.button("Width").on_hover_text("Fit width · w").clicked()
                        {
                            self.reading.fit = Fit::Width;
                            self.dirty = true;
                        }
                        if !compact
                            && ui
                                .button("Rotate")
                                .on_hover_text("Rotate · r / Shift+R")
                                .clicked()
                        {
                            self.reading.rotation = (self.reading.rotation + 1) % 4;
                            self.dirty = true;
                        }
                    }
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.add_space(12.0);
                        if ui.button("?").clicked() {
                            self.help = !self.help;
                        }
                        if ui
                            .button(if self.library.dark { "Light" } else { "Dark" })
                            .clicked()
                        {
                            self.library.dark = !self.library.dark;
                            set_theme(context, self.library.dark);
                            self.dirty = true;
                        }
                        if self.nav.count > 0 {
                            if ui
                                .button("Find")
                                .on_hover_text("Search · / or Ctrl/Cmd+F")
                                .clicked()
                            {
                                self.find();
                            }
                            if ui
                                .selectable_label(
                                    self.reading.bookmarks.contains(&self.nav.page),
                                    "Bookmark",
                                )
                                .on_hover_text("b")
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
            .exact_height(42.0)
            .show(context, |ui| {
                ui.horizontal_centered(|ui| {
                    ui.add_space(16.0);
                    ui.label("Find");
                    let response = ui.add(
                        egui::TextEdit::singleline(&mut self.query)
                            .id(egui::Id::new("folio-search-input"))
                            .desired_width(280.0)
                            .hint_text("Search this document…"),
                    );
                    if self.search_focus {
                        response.request_focus();
                        self.search_focus = false;
                    }
                    if response.changed() {
                        self.revision += 1;
                        self.hits.clear();
                        self.hit_index = None;
                        self.reveal_hit = None;
                        self.pending_search_jump = None;
                        self.scanned = 0;
                        self.scan_total = self.nav.count;
                        self.search_complete = self.query.trim().is_empty();
                        self.search_limited = false;
                        self.worker.search(self.revision, String::new());
                        self.search_due = Some(Instant::now() + Duration::from_millis(180));
                    }
                    if response.lost_focus() && ui.input(|input| input.key_pressed(Key::Enter)) {
                        self.next_hit(ui.input(|input| input.modifiers.shift));
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
                        ui.weak(format!("{} / {} pages", self.scanned, self.scan_total));
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

    fn contents(&mut self, ui: &mut egui::Ui) {
        if self.outline.is_empty() {
            return;
        }
        ui.horizontal(|ui| {
            ui.label(RichText::new("CONTENTS").small().strong().color(ACCENT));
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
        ui.horizontal(|ui| {
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let clear = ui
                    .small_button("×")
                    .on_hover_text("Clear chapter filter")
                    .clicked();
                let response = ui.add(
                    egui::TextEdit::singleline(&mut self.outline.query)
                        .id(egui::Id::new("folio-outline-input"))
                        .hint_text("Filter chapters…")
                        .desired_width(f32::INFINITY),
                );
                if clear {
                    self.outline.query.clear();
                }
                if response.changed() || clear {
                    self.outline.refilter();
                }
            });
        });
        if self.outline.visible().is_empty() {
            ui.weak("No matching chapters.");
        }
        let mut target = None;
        let mut toggle = None;
        let row_height = (ui.text_style_height(&egui::TextStyle::Button)
            + 2.0 * ui.spacing().button_padding.y)
            .max(ui.spacing().interact_size.y)
            .max(20.0);
        egui::ScrollArea::vertical()
            .id_salt("contents")
            .max_height(260.0)
            .show_rows(ui, row_height, self.outline.visible().len(), |ui, range| {
                for row in range {
                    let index = self.outline.visible()[row];
                    let entry = self.outline.entry(index);
                    ui.push_id(index, |ui| {
                        ui.horizontal(|ui| {
                            ui.add_space(entry.depth.min(8) as f32 * 12.0);
                            if self.outline.has_children(index) {
                                let collapsed = self.outline.is_collapsed(index);
                                let response = ui
                                    .add_enabled_ui(!self.outline.filtering(), |ui| {
                                        let response = ui.allocate_response(
                                            Vec2::splat(20.0),
                                            egui::Sense::click(),
                                        );
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
                                            self.nav.page == page,
                                            &entry.title,
                                        )
                                        .truncate(),
                                    )
                                    .on_hover_text(format!(
                                        "{}\nPage {}",
                                        entry.title,
                                        self.page_name(page)
                                    ))
                                    .clicked()
                                {
                                    target = Some(destination);
                                }
                            } else {
                                ui.add(
                                    egui::Label::new(RichText::new(&entry.title).weak()).truncate(),
                                );
                            }
                        });
                    });
                }
            });
        if let Some(index) = toggle {
            self.outline.toggle(index);
        }
        if let Some(destination) = target {
            self.follow_destination(destination);
        }
        ui.add_space(20.0);
    }

    fn sidebar(&mut self, context: &egui::Context) {
        if !self.library.sidebar || self.nav.count == 0 {
            return;
        }
        egui::SidePanel::left("sidebar")
            .default_width(230.0)
            .width_range(180.0..=380.0)
            .show(context, |ui| {
                ui.add_space(16.0);
                ui.label(RichText::new(self.filename()).strong());
                ui.weak(format!("{} pages", self.nav.count));
                ui.add_space(16.0);
                egui::ScrollArea::vertical().show(ui, |ui| {
                    self.contents(ui);
                    ui.label(RichText::new("BOOKMARKS").small().strong().color(ACCENT));
                    if self.reading.bookmarks.is_empty() {
                        ui.weak("Press b to bookmark a page.");
                    }
                    for page in self.reading.bookmarks.clone() {
                        if ui
                            .selectable_label(
                                page == self.nav.page,
                                format!("Page {}", self.page_name(page)),
                            )
                            .clicked()
                        {
                            self.jump(page);
                        }
                    }
                    if !self.reading.marks.is_empty() {
                        ui.add_space(20.0);
                        ui.label(RichText::new("MARKS").small().strong().color(ACCENT));
                        let mut target = None;
                        let mut remove = None;
                        for (&name, mark) in &self.reading.marks {
                            ui.horizontal(|ui| {
                                if ui
                                    .selectable_label(
                                        mark.page == self.nav.page,
                                        format!("'{name}   Page {}", mark.page + 1),
                                    )
                                    .on_hover_text(
                                        "Restore page, zoom, rotation and scroll position",
                                    )
                                    .clicked()
                                {
                                    target = Some(name);
                                }
                                if ui.small_button("×").on_hover_text("Delete mark").clicked() {
                                    remove = Some(name);
                                }
                            });
                        }
                        if let Some(name) = target {
                            self.mark(name, MarkAction::Jump);
                        }
                        if let Some(name) = remove {
                            self.reading.marks.remove(&name);
                            self.dirty = true;
                        }
                    }
                    if !self.hits.is_empty() {
                        ui.add_space(20.0);
                        ui.label(
                            RichText::new("SEARCH RESULTS")
                                .small()
                                .strong()
                                .color(ACCENT),
                        );
                        let mut selected = None;
                        let mut results = egui::ScrollArea::vertical()
                            .id_salt("search-results")
                            .max_height(280.0);
                        if let Some(index) = self.reveal_hit.take() {
                            results = results.vertical_scroll_offset(
                                index as f32 * (25.0 + ui.spacing().item_spacing.y),
                            );
                        }
                        results.show_rows(ui, 25.0, self.hits.len(), |ui, range| {
                            for index in range {
                                let hit = &self.hits[index];
                                if ui
                                    .add(
                                        egui::Button::selectable(
                                            self.hit_index == Some(index),
                                            format!(
                                                "{}   {}",
                                                self.page_name(hit.page),
                                                hit.snippet
                                            ),
                                        )
                                        .truncate(),
                                    )
                                    .on_hover_text(&hit.snippet)
                                    .clicked()
                                {
                                    selected = Some(index);
                                }
                            }
                        });
                        if let Some(index) = selected {
                            self.select_hit(index);
                        }
                    }
                    ui.add_space(20.0);
                    ui.label(RichText::new("PAGES").small().strong().color(ACCENT));
                    egui::ScrollArea::vertical()
                        .id_salt("page-list")
                        .max_height(300.0)
                        .show_rows(ui, 25.0, self.nav.count, |ui, range| {
                            for page in range {
                                if ui
                                    .selectable_label(
                                        page == self.nav.page,
                                        format!("Page {}", self.page_name(page)),
                                    )
                                    .clicked()
                                {
                                    self.jump(page);
                                }
                            }
                        });
                });
            });
    }

    fn canvas(&mut self, context: &egui::Context) {
        let background = if self.library.dark {
            Color32::from_rgb(27, 29, 31)
        } else {
            Color32::from_rgb(234, 233, 229)
        };
        egui::CentralPanel::default()
            .frame(egui::Frame::NONE.fill(background))
            .show(context, |ui| {
                if self.nav.count == 0 {
                    self.welcome(ui);
                    return;
                }
                let rect = ui.max_rect().shrink(24.0);
                self.viewport = rect.size();
                let dimensions = self.dimensions[self.nav.page];
                let rotated = if self.reading.rotation % 2 == 1 {
                    [dimensions[1], dimensions[0]]
                } else {
                    dimensions
                };
                let mut view = RenderView {
                    fit: self.reading.fit,
                    viewport: [rect.width(), rect.height()],
                    pixels_per_point: context.pixels_per_point(),
                    rotation: self.reading.rotation,
                    night: self.library.night,
                    backwards: self.nav.direction < 0,
                };
                self.scale = view.scale(dimensions);
                let mut size = Vec2::new(rotated[0], rotated[1]) * self.scale;
                self.overflow = (size - rect.size()).max(Vec2::ZERO);
                if let Some(offset) = self.pending_offset.take() {
                    self.pan = Vec2::new(offset[0].clamp(0.0, 1.0), offset[1].clamp(0.0, 1.0))
                        * self.overflow;
                }
                if let Some(focus) = self.pending_focus.take() {
                    let offset = crate::geometry::focus_offset(
                        focus,
                        dimensions,
                        self.reading.rotation,
                        self.scale,
                        [self.viewport.x, self.viewport.y],
                        [self.overflow.x, self.overflow.y],
                    );
                    self.pan = Vec2::new(offset[0], offset[1]);
                }
                if let Some((destination, origin)) = self.pending_destination.take() {
                    let offset = destination.pan(
                        dimensions,
                        self.reading.rotation,
                        self.scale,
                        [self.viewport.x, self.viewport.y],
                        origin,
                        [self.overflow.x, self.overflow.y],
                    );
                    self.pan = Vec2::new(offset[0], offset[1]);
                }
                self.pan = self.pan.clamp(Vec2::ZERO, self.overflow);
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
                    if response.double_clicked() {
                        self.reading.fit = Fit::Page;
                        self.pan = Vec2::ZERO;
                        self.dirty = true;
                    }
                }
                if view.fit != self.reading.fit {
                    view.fit = self.reading.fit;
                    self.scale = view.scale(dimensions);
                    size = Vec2::new(rotated[0], rotated[1]) * self.scale;
                    self.overflow = (size - rect.size()).max(Vec2::ZERO);
                    self.pan = self.pan.clamp(Vec2::ZERO, self.overflow);
                }
                let offset = self.current_offset();
                if self.reading.offset != offset {
                    self.reading.offset = offset;
                    self.dirty = true;
                }
                let origin = rect.min + (rect.size() - size).max(Vec2::ZERO) * 0.5 - self.pan;
                let page_rect = egui::Rect::from_min_size(origin, size);
                if self.selection.active {
                    if response.hovered() || response.dragged() {
                        context.set_cursor_icon(if self.selection.ready() {
                            egui::CursorIcon::Text
                        } else {
                            egui::CursorIcon::Progress
                        });
                    }
                    let point = |position: egui::Pos2| {
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
                    if response.drag_started_by(egui::PointerButton::Primary)
                        || response.clicked_by(egui::PointerButton::Primary)
                    {
                        let pressed = ui
                            .input(|input| input.pointer.press_origin())
                            .or(response.interact_pointer_pos());
                        if let Some(position) =
                            pressed.filter(|position| page_rect.contains(*position))
                        {
                            self.selection.begin(point(position));
                        }
                    }
                    if (response.dragged_by(egui::PointerButton::Primary)
                        || response.drag_stopped_by(egui::PointerButton::Primary))
                        && let Some(position) = response.interact_pointer_pos()
                    {
                        self.selection.extend(point(position));
                    }
                }
                let key = view.key(self.nav.page, dimensions);
                self.cache.protect(key);
                let plan = view.plan(self.nav.page, &self.dimensions, self.cache.budget());
                if !self.loading && self.requested.as_ref() != Some(&plan) {
                    if !self.links.contains_key(&self.nav.page) {
                        self.worker.links(self.nav.page);
                    }
                    let keys = plan
                        .iter()
                        .copied()
                        .filter(|key| !self.cache.contains(key))
                        .collect();
                    self.requested = Some(plan);
                    self.worker.render(keys);
                }
                let texture = self.cache.get(&key).or_else(|| self.cache.fallback(key));
                let painter = ui.painter_at(ui.max_rect());
                painter.rect_filled(
                    page_rect.translate(Vec2::new(2.0, 4.0)).expand(2.0),
                    1.0,
                    Color32::from_black_alpha(25),
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
                        texture.id(),
                        page_rect,
                        egui::Rect::from_min_max(egui::Pos2::ZERO, egui::pos2(1.0, 1.0)),
                        Color32::WHITE,
                    );
                    self.highlights(&painter, page_rect, dimensions);
                    for rect in &self.selection.rects {
                        painter.rect_filled(
                            self.link_rect(*rect, page_rect, dimensions)
                                .intersect(page_rect),
                            1.0,
                            Color32::from_rgba_unmultiplied(74, 145, 230, 95),
                        );
                    }
                    self.link_overlays(ui, page_rect, dimensions);
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
    }

    fn highlights(&self, painter: &egui::Painter, page_rect: egui::Rect, dimensions: [f32; 2]) {
        for index in crate::text::page_hits(&self.hits, self.nav.page) {
            let hit = &self.hits[index];
            let color = if self.hit_index == Some(index) {
                Color32::from_rgba_unmultiplied(255, 143, 36, 120)
            } else {
                Color32::from_rgba_unmultiplied(255, 215, 40, 85)
            };
            for rect in &hit.rects {
                painter.rect_filled(
                    self.link_rect(*rect, page_rect, dimensions)
                        .intersect(page_rect),
                    1.0,
                    color,
                );
            }
        }
    }

    fn follow_destination(&mut self, destination: crate::destination::Destination) {
        let Some(&dimensions) = self.dimensions.get(destination.page) else {
            return;
        };
        let origin = [
            f64::from(self.pan.x) / f64::from(self.scale),
            f64::from(self.pan.y) / f64::from(self.scale),
        ];
        self.jump(destination.page);
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
        let rect = crate::geometry::rotate_rect(rect, dimensions, self.reading.rotation);
        let scale = f64::from(self.scale);
        egui::Rect::from_min_max(
            page_rect.min + Vec2::new((rect.x0 * scale) as f32, (rect.y0 * scale) as f32),
            page_rect.min + Vec2::new((rect.x1 * scale) as f32, (rect.y1 * scale) as f32),
        )
    }

    fn link_overlays(&mut self, ui: &mut egui::Ui, page_rect: egui::Rect, dimensions: [f32; 2]) {
        if self.selection.active {
            self.visible_links.clear();
            return;
        }
        self.visible_links = self
            .links
            .get(&self.nav.page)
            .into_iter()
            .flatten()
            .filter(|link| {
                self.link_rect(link.rect, page_rect, dimensions)
                    .intersects(ui.clip_rect())
            })
            .cloned()
            .collect();
        let mut follow = None;
        for (index, link) in self.visible_links.iter().enumerate() {
            let rect = self
                .link_rect(link.rect, page_rect, dimensions)
                .intersect(page_rect)
                .intersect(ui.clip_rect());
            let description = match &link.target {
                LinkTarget::Destination(destination) => {
                    format!("Go to page {}", self.page_name(destination.page))
                }
                LinkTarget::Url(url) => url.clone(),
            };
            let response = ui
                .interact(
                    rect,
                    ui.id().with(("link", self.nav.page, index)),
                    egui::Sense::click(),
                )
                .on_hover_cursor(egui::CursorIcon::PointingHand)
                .on_hover_text(description);
            if response.clicked() {
                follow = Some(link.target.clone());
            }
            if self.hints.active {
                let label = crate::links::hint_label(index, self.visible_links.len());
                if !label.starts_with(&self.hints.prefix) {
                    continue;
                }
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
    }

    fn welcome(&mut self, ui: &mut egui::Ui) {
        ui.vertical_centered(|ui| {
            ui.add_space((ui.available_height() * 0.18).max(30.0));
            ui.label(RichText::new("F").size(72.0).strong().color(ACCENT));
            ui.add_space(8.0);
            ui.heading(RichText::new("A little room to read.").size(30.0));
            ui.add_space(12.0);
            ui.weak("Open a PDF. Keep your hands on the keyboard.");
            ui.add_space(24.0);
            if self.loading {
                ui.spinner();
                ui.label("Opening document…");
            } else if ui
                .add(
                    egui::Button::new(RichText::new("Open a document").color(Color32::WHITE))
                        .fill(ACCENT)
                        .min_size(Vec2::new(190.0, 42.0)),
                )
                .clicked()
            {
                self.choose_file();
            }
            ui.add_space(12.0);
            ui.weak("or drop a PDF anywhere · Ctrl / Cmd + O");
            if !self.library.recent.is_empty() {
                ui.add_space(32.0);
                ui.label(
                    RichText::new("PICK UP WHERE YOU LEFT OFF")
                        .small()
                        .color(ACCENT),
                );
                let mut open = None;
                for item in self.library.recent.iter().take(5) {
                    let name = item.path.file_name().unwrap_or_default().to_string_lossy();
                    if ui
                        .button(format!("{name}    ·    page {}", item.page + 1))
                        .on_hover_text(item.path.display().to_string())
                        .clicked()
                    {
                        open = Some(item.path.clone());
                    }
                }
                if let Some(path) = open {
                    self.open(path, ui.ctx());
                }
            }
        });
    }

    fn status(&mut self, context: &egui::Context) {
        egui::TopBottomPanel::bottom("status")
            .exact_height(29.0)
            .show(context, |ui| {
                ui.horizontal_centered(|ui| {
                    ui.add_space(12.0);
                    ui.label(
                        RichText::new(match self.mark_action {
                            Some(MarkAction::Set) => "SET MARK · a–z",
                            Some(MarkAction::Jump) => "GO TO MARK · a–z",
                            None if self.hints.active => "LINKS",
                            None if self.selection.active => "VISUAL",
                            None => "NORMAL",
                        })
                        .small()
                        .strong()
                        .color(ACCENT),
                    );
                    if self.hints.active {
                        ui.monospace(&self.hints.prefix);
                    }
                    if !self.nav.prefix.is_empty() || self.nav.pending_g {
                        ui.monospace(format!(
                            "{}{}",
                            self.nav.prefix,
                            if self.nav.pending_g { "g" } else { "" }
                        ));
                    }
                    ui.separator();
                    if self.loading && self.reloading {
                        ui.spinner();
                        ui.label("Reloading document…");
                    } else if let Some((message, at)) = &self.message
                        && at.elapsed() < Duration::from_secs(5)
                    {
                        ui.label(message);
                        context.request_repaint_after(Duration::from_secs(5) - at.elapsed());
                    } else if self.nav.count > 0 {
                        ui.label(self.filename());
                    } else {
                        ui.weak("j k  scroll    J K  pages    /  find    ?  help");
                    }
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.add_space(12.0);
                        if self.nav.count > 0 {
                            ui.weak(format!(
                                "{:.0} ms  ·  {:.1} MB cached",
                                self.render_ms,
                                self.cache.bytes as f64 / 1_048_576.0
                            ));
                            ui.separator();
                            ui.label(format!(
                                "{}%",
                                ((self.nav.page + 1) * 100)
                                    .checked_div(self.nav.count)
                                    .unwrap_or(0)
                            ));
                        }
                    });
                });
            });
    }

    fn dialogs(&mut self, context: &egui::Context) {
        if self.goto_open {
            egui::Window::new("Go to page")
                .collapsible(false)
                .resizable(false)
                .anchor(egui::Align2::CENTER_CENTER, Vec2::ZERO)
                .show(context, |ui| {
                    ui.label(format!("Physical page 1–{}", self.nav.count));
                    ui.weak("Or a PDF label: iv, A-3. Use =1 for label 1.");
                    let response = ui.add(
                        egui::TextEdit::singleline(&mut self.goto)
                            .id(egui::Id::new("folio-page-input")),
                    );
                    if self.goto_focus {
                        response.request_focus();
                        self.goto_focus = false;
                    }
                    if ui.button("Go").clicked()
                        || (response.lost_focus()
                            && ui.input(|input| input.key_pressed(Key::Enter)))
                    {
                        if let Some(page) = self.labels.resolve_input(&self.goto, self.nav.count) {
                            self.jump(page);
                            self.goto_open = false;
                        } else {
                            self.notify("Enter a valid page number or PDF page label");
                        }
                    }
                });
        }
        if self.password_open {
            egui::Window::new("Password required")
                .collapsible(false)
                .resizable(false)
                .anchor(egui::Align2::CENTER_CENTER, Vec2::ZERO)
                .show(context, |ui| {
                    ui.label("This document is encrypted.");
                    let response = ui.add(
                        egui::TextEdit::singleline(&mut self.password)
                            .id(egui::Id::new("folio-password-input"))
                            .password(true),
                    );
                    if ui.button("Unlock").clicked()
                        || (response.lost_focus()
                            && ui.input(|input| input.key_pressed(Key::Enter)))
                    {
                        self.error = None;
                        self.loading = true;
                        self.password_open = false;
                        let password = std::mem::take(&mut self.password);
                        if self.reloading {
                            self.worker
                                .reload(self.id, self.reading.path.clone(), password);
                        } else {
                            self.worker
                                .open(self.id, self.reading.path.clone(), password);
                        }
                    }
                });
        }
        if self.help {
            egui::Window::new("Keyboard shortcuts").open(&mut self.help).collapsible(false).resizable(false).default_width(480.0).anchor(egui::Align2::CENTER_CENTER, Vec2::ZERO).max_height((context.content_rect().height() - 60.0).max(200.0)).vscroll(true).show(context, |ui| {
                ui.label(RichText::new("Stay in the flow.").size(23.0).strong().color(ACCENT)); ui.add_space(12.0);
                egui::Grid::new("keys").num_columns(2).spacing([35.0, 8.0]).show(ui, |ui| {
                    for (keys, action) in [
                        ("j / k / h / l", "Scroll down / up / left / right"), ("J / K   or   ] / [", "Next / previous page"), ("gg / G", "First / last page"),
                        ("42G   or   42 Enter", "Go to page 42"), ("Ctrl+d / Ctrl+u", "Half screen down / up"), ("Space / Shift+Space", "Screen down / up"),
                        ("Alt+Left / Alt+Right", "Jump history back / forward"), ("+ / −", "Zoom in / out"), ("p / w", "Fit page / fit width"), ("r / Shift+r", "Rotate clockwise / counterclockwise"),
                        ("/   or   Ctrl/Cmd+f", "Search document"), ("n / N", "Next / previous search match"), ("b", "Toggle bookmark"), ("Tab", "Toggle sidebar"),
                        ("ma   /   'a or `a", "Save / restore named position a–z"),
                        ("v, drag, then y", "Select and copy text (Ctrl/Cmd+C also works)"),
                        ("Ctrl/Cmd+r", "Reload PDF and keep reading position"),
                        ("d / i", "Toggle dark interface / invert page"), ("y", "Copy page text"), ("o   or   Ctrl/Cmd+o", "Open PDF"), (":", "Go to page dialog"),
                        ("f, then a label", "Follow a visible PDF link"), ("F11", "Toggle fullscreen"), ("? / Escape", "Show help / close dialog"),
                    ] { ui.monospace(keys); ui.label(action); ui.end_row(); }
                });
                ui.add_space(12.0); ui.weak("Counts work with movement: 5j, 3J, 12gg. Drag to pan. Double-click to fit.");
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

impl eframe::App for Folio {
    fn update(&mut self, context: &egui::Context, _: &mut eframe::Frame) {
        self.events(context);
        self.keys(context);
        self.toolbar(context);
        self.search_bar(context);
        self.status(context);
        self.dialogs(context);
        self.sidebar(context);
        self.canvas(context);
        if let Some(due) = self.search_due {
            if Instant::now() >= due {
                self.worker.search(self.revision, self.query.clone());
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
    let mut visuals = if dark {
        egui::Visuals::dark()
    } else {
        egui::Visuals::light()
    };
    visuals.selection.bg_fill = ACCENT;
    visuals.selection.stroke.color = Color32::WHITE;
    visuals.widgets.active.bg_fill = ACCENT;
    visuals.panel_fill = if dark {
        Color32::from_rgb(35, 38, 40)
    } else {
        Color32::from_rgb(250, 249, 246)
    };
    context.set_visuals(visuals);
    context.style_mut(|style| {
        style.spacing.button_padding = Vec2::new(10.0, 6.0);
        style.spacing.item_spacing = Vec2::new(8.0, 8.0);
    });
}
