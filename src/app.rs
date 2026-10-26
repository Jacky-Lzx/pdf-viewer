use crate::{
    cache::TextureCache,
    document::RenderKey,
    links::{LinkHints, LinkTarget, PageLink},
    navigation::{Fit, Navigation},
    render_plan::RenderView,
    state::{self, Library, ReadingState},
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

pub struct Folio {
    worker: Worker,
    library: Library,
    config: PathBuf,
    reading: ReadingState,
    nav: Navigation,
    dimensions: Vec<[f32; 2]>,
    outline: Vec<crate::outline::OutlineEntry>,
    links: HashMap<usize, Vec<PageLink>>,
    visible_links: Vec<PageLink>,
    hints: LinkHints,
    cache: TextureCache,
    id: u64,
    loading: bool,
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
            outline: Vec::new(),
            links: HashMap::new(),
            visible_links: Vec::new(),
            hints: LinkHints::default(),
            cache: TextureCache::default(),
            id: 0,
            loading: false,
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
        self.outline.clear();
        self.links.clear();
        self.visible_links.clear();
        self.hints.cancel();
        self.cache.clear();
        self.requested = None;
        self.pan = Vec2::ZERO;
        self.hits.clear();
        self.hit_index = None;
        self.query.clear();
        self.search_due = None;
        self.search_open = false;
        self.scanned = 0;
        self.scan_total = 0;
        self.search_complete = true;
        self.search_limited = false;
        self.revision += 1;
        self.loading = true;
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

    fn remember(&mut self) {
        if self.nav.count > 0 {
            self.reading.page = self.nav.page;
            self.library.remember(self.reading.clone());
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
        if self.nav.jump(page) {
            self.page_changed();
        }
    }
    fn step(&mut self, delta: isize) {
        if self.nav.step(delta) {
            self.page_changed();
        }
    }
    fn page_changed(&mut self) {
        self.hints.cancel();
        self.visible_links.clear();
        self.pan = Vec2::ZERO;
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
        self.hints.cancel();
        self.reading.fit = Fit::Custom((self.scale * factor).clamp(0.1, 8.0));
        self.pan *= factor;
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
        self.hints.cancel();
        self.search_open = true;
        self.search_focus = true;
    }

    fn next_hit(&mut self, reverse: bool) {
        if self.hits.is_empty() {
            return;
        }
        let index = match self.hit_index {
            Some(index) if reverse => (index + self.hits.len() - 1) % self.hits.len(),
            Some(index) => (index + 1) % self.hits.len(),
            None if reverse => self.hits.len() - 1,
            None => self
                .hits
                .iter()
                .position(|hit| hit.page >= self.nav.page)
                .unwrap_or(0),
        };
        self.select_hit(index);
    }

    fn select_hit(&mut self, index: usize) {
        self.hit_index = Some(index);
        let page = self.hits[index].page;
        let top = self.hits[index].rects.first().map(|rect| rect.y0 as f32);
        self.jump(page);
        if self.reading.rotation == 0
            && let Some(top) = top
        {
            self.pan.y = (top * self.scale - self.viewport.y * 0.3).max(0.0);
        }
    }

    fn events(&mut self, context: &egui::Context) {
        while let Ok(event) = self.worker.events.try_recv() {
            match event {
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
                } if id == self.id => {
                    self.loading = false;
                    self.nav = Navigation::new(dimensions.len(), self.reading.page);
                    self.reading
                        .bookmarks
                        .retain(|page| *page < dimensions.len());
                    self.dimensions = dimensions;
                    self.outline = outline;
                    self.dirty = true;
                }
                Event::Rendered {
                    id,
                    key,
                    bitmap,
                    milliseconds,
                } if id == self.id => {
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
                }
                Event::Text { id, page, text } if id == self.id => {
                    if text.trim().is_empty() {
                        self.notify("No selectable text on this page");
                    } else {
                        context.copy_text(text);
                        self.notify(format!("Copied text from page {}", page + 1));
                    }
                }
                Event::Error { id, message } if id == self.id => {
                    self.loading = false;
                    if message.contains("Decryption") {
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
            ]
            .iter()
            .any(|name| memory.has_focus(egui::Id::new(name)))
        });
        for event in context.input(|input| input.events.clone()) {
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
                self.help = false;
                self.goto_open = false;
                self.search_open = false;
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
            if typing || self.help || self.password_open || self.goto_open {
                continue;
            }
            context.input_mut(|input| {
                input.consume_key(modifiers, key);
            });
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
                match key {
                    Key::D => self.scroll(Vec2::new(0.0, self.viewport.y * 0.5), true),
                    Key::U => self.scroll(Vec2::new(0.0, -self.viewport.y * 0.5), true),
                    Key::F => self.scroll(Vec2::new(0.0, self.viewport.y * 0.9), true),
                    Key::B => self.scroll(Vec2::new(0.0, -self.viewport.y * 0.9), true),
                    Key::I if self.nav.history(true) => self.page_changed(),
                    _ => {}
                }
                continue;
            }
            if modifiers.alt {
                if matches!(key, Key::ArrowLeft | Key::ArrowRight)
                    && self.nav.history(key == Key::ArrowRight)
                {
                    self.page_changed();
                }
                continue;
            }
            if modifiers.command {
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
                Key::H if modifiers.shift => self.step(-1),
                Key::L if modifiers.shift => self.step(1),
                Key::H | Key::ArrowLeft => self.scroll(Vec2::new(-60.0, 0.0), false),
                Key::L | Key::ArrowRight => self.scroll(Vec2::new(60.0, 0.0), false),
                Key::PageDown | Key::CloseBracket => self.step(1),
                Key::PageUp | Key::OpenBracket => self.step(-1),
                Key::Space => self.scroll(
                    Vec2::new(
                        0.0,
                        self.viewport.y * if modifiers.shift { -0.9 } else { 0.9 },
                    ),
                    true,
                ),
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
                            .button(format!("{} / {}", self.nav.page + 1, self.nav.count))
                            .on_hover_text("Go to page · :")
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
                        ui.label(format!("{:.0}%", self.scale * 100.0));
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
                    }
                });
            });
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
                    if !self.outline.is_empty() {
                        ui.label(RichText::new("CONTENTS").small().strong().color(ACCENT));
                        let mut target = None;
                        egui::ScrollArea::vertical()
                            .id_salt("contents")
                            .max_height(260.0)
                            .show_rows(ui, 25.0, self.outline.len(), |ui, range| {
                                for index in range {
                                    let entry = &self.outline[index];
                                    ui.push_id(index, |ui| {
                                        ui.horizontal(|ui| {
                                            ui.add_space(entry.depth.min(8) as f32 * 12.0);
                                            if let Some(page) = entry.page {
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
                                                        page + 1
                                                    ))
                                                    .clicked()
                                                {
                                                    target = Some(page);
                                                }
                                            } else {
                                                ui.add(
                                                    egui::Label::new(
                                                        RichText::new(&entry.title).weak(),
                                                    )
                                                    .truncate(),
                                                );
                                            }
                                        });
                                    });
                                }
                            });
                        if let Some(page) = target {
                            self.jump(page);
                        }
                        ui.add_space(20.0);
                    }
                    ui.label(RichText::new("BOOKMARKS").small().strong().color(ACCENT));
                    if self.reading.bookmarks.is_empty() {
                        ui.weak("Press b to bookmark a page.");
                    }
                    for page in self.reading.bookmarks.clone() {
                        if ui
                            .selectable_label(page == self.nav.page, format!("Page {}", page + 1))
                            .clicked()
                        {
                            self.jump(page);
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
                        for (index, hit) in self.hits.iter().enumerate().take(300) {
                            if ui
                                .selectable_label(
                                    self.hit_index == Some(index),
                                    format!("{}   {}", hit.page + 1, hit.snippet),
                                )
                                .clicked()
                            {
                                selected = Some(index);
                            }
                        }
                        if let Some(index) = selected {
                            self.select_hit(index);
                        }
                        if self.hits.len() > 300 {
                            ui.weak("Use n / N to navigate all results.");
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
                                        format!("Page {}", page + 1),
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
                let view = RenderView {
                    fit: self.reading.fit,
                    viewport: [rect.width(), rect.height()],
                    pixels_per_point: context.pixels_per_point(),
                    rotation: self.reading.rotation,
                    night: self.library.night,
                };
                self.scale = view.scale(dimensions);
                let size = Vec2::new(rotated[0], rotated[1]) * self.scale;
                self.overflow = (size - rect.size()).max(Vec2::ZERO);
                self.pan = self.pan.clamp(Vec2::ZERO, self.overflow);
                let response = ui.interact(
                    ui.max_rect(),
                    ui.id().with("canvas"),
                    egui::Sense::click_and_drag(),
                );
                if response.dragged() {
                    self.pan = (self.pan - response.drag_delta()).clamp(Vec2::ZERO, self.overflow);
                }
                if response.hovered() {
                    let (scroll, zoom) =
                        ui.input(|input| (input.smooth_scroll_delta, input.zoom_delta()));
                    if (zoom - 1.0).abs() > 0.001 {
                        self.zoom(zoom);
                    } else {
                        self.scroll(-scroll, false);
                    }
                    if response.double_clicked() {
                        self.reading.fit = Fit::Page;
                        self.pan = Vec2::ZERO;
                        self.dirty = true;
                    }
                }
                let origin = rect.min + (rect.size() - size).max(Vec2::ZERO) * 0.5 - self.pan;
                let page_rect = egui::Rect::from_min_size(origin, size);
                let key = view.key(self.nav.page, dimensions);
                self.cache.protect(key);
                let plan = view.plan(self.nav.page, &self.dimensions, self.cache.budget());
                if self.requested.as_ref() != Some(&plan) {
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
        for (index, hit) in self
            .hits
            .iter()
            .enumerate()
            .filter(|(_, hit)| hit.page == self.nav.page)
        {
            let color = if self.hit_index == Some(index) {
                Color32::from_rgba_unmultiplied(255, 143, 36, 120)
            } else {
                Color32::from_rgba_unmultiplied(255, 215, 40, 85)
            };
            for rect in &hit.rects {
                let points = [(rect.x0, rect.y0), (rect.x1, rect.y1)].map(|(x, y)| {
                    let (x, y) = (x as f32, y as f32);
                    let (x, y) = match self.reading.rotation {
                        1 => (dimensions[1] - y, x),
                        2 => (dimensions[0] - x, dimensions[1] - y),
                        3 => (y, dimensions[0] - x),
                        _ => (x, y),
                    };
                    page_rect.min + Vec2::new(x, y) * self.scale
                });
                painter.rect_filled(
                    egui::Rect::from_two_pos(points[0], points[1]).intersect(page_rect),
                    1.0,
                    color,
                );
            }
        }
    }

    fn follow_link(&mut self, target: LinkTarget, context: &egui::Context) {
        self.hints.cancel();
        match target {
            LinkTarget::Page(page) => self.jump(page),
            LinkTarget::Url(url) => context.open_url(egui::OpenUrl::new_tab(url)),
        }
        context.request_repaint();
    }

    fn link_rect(
        &self,
        rect: hayro::kurbo::Rect,
        page_rect: egui::Rect,
        dimensions: [f32; 2],
    ) -> egui::Rect {
        let points = [(rect.x0, rect.y0), (rect.x1, rect.y1)].map(|(x, y)| {
            let (x, y) = (x as f32, y as f32);
            let (x, y) = match self.reading.rotation {
                1 => (dimensions[1] - y, x),
                2 => (dimensions[0] - x, dimensions[1] - y),
                3 => (y, dimensions[0] - x),
                _ => (x, y),
            };
            page_rect.min + Vec2::new(x, y) * self.scale
        });
        egui::Rect::from_two_pos(points[0], points[1])
    }

    fn link_overlays(&mut self, ui: &mut egui::Ui, page_rect: egui::Rect, dimensions: [f32; 2]) {
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
                LinkTarget::Page(page) => format!("Go to page {}", page + 1),
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
                        RichText::new(if self.hints.active { "LINKS" } else { "NORMAL" })
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
                    if let Some((message, at)) = &self.message
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
                    ui.label(format!("Page 1–{}", self.nav.count));
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
                        if let Ok(page) = self.goto.trim().parse::<usize>() {
                            self.jump(page.saturating_sub(1));
                            self.goto_open = false;
                        } else {
                            self.notify("Enter a page number");
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
                        self.worker.open(
                            self.id,
                            self.reading.path.clone(),
                            std::mem::take(&mut self.password),
                        );
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
                    if self.nav.count > 0 && ui.button("Retry page").clicked() {
                        self.requested = None;
                        self.error = None;
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
