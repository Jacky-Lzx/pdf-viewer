use crate::{
    document::{self, LoadError, RenderKey},
    file_watch::FileWatch,
    text::{self, PageText, SearchHit, SearchOptions, SearchQuery},
};
use eframe::egui;
use hayro::{RenderCache, hayro_interpret::InterpreterCache, hayro_syntax::Pdf};
use std::{
    collections::VecDeque,
    path::PathBuf,
    sync::{
        Arc, Condvar, Mutex,
        mpsc::{self, Receiver, SyncSender},
    },
    time::Instant,
};

mod scheduling;
mod search_updates;

/// Recovery depends on whether one render failed or the whole session stopped.
pub enum Failure {
    Open(LoadError),
    Render(String),
    Session,
}

pub enum Event {
    Comments {
        id: u64,
        comments: crate::annotations::Comments,
    },
    CommentChanged {
        id: u64,
        operation: crate::annotations::Operation,
        result: Result<(), String>,
    },
    FileChanged {
        id: u64,
    },
    Opened {
        id: u64,
        dimensions: Vec<[f32; 2]>,
        sync_geometry: Vec<crate::synctex::PageGeometry>,
        info: crate::document_info::DocumentInfo,
        // Titles and tree indexes are ready for display and filtering. Boxing
        // keeps this large model out of every bounded-queue event slot.
        outline: Box<crate::outline_view::OutlineView>,
        labels: crate::page_labels::PageLabels,
    },
    Rendered {
        id: u64,
        key: RenderKey,
        image: egui::ColorImage,
        milliseconds: f64,
        appearance_revision: u64,
    },
    Thumbnail {
        id: u64,
        key: RenderKey,
        image: Option<egui::ColorImage>,
        appearance_revision: u64,
    },
    Search {
        id: u64,
        revision: u64,
        hits: Vec<SearchHit>,
        scanned: usize,
        total: usize,
        complete: bool,
        limited: bool,
    },
    Text {
        id: u64,
        page: usize,
        text: String,
    },
    PageText {
        id: u64,
        page: usize,
        text: Arc<PageText>,
    },
    Links {
        id: u64,
        page: usize,
        links: Vec<crate::links::PageLink>,
    },
    Error {
        id: u64,
        error: Failure,
    },
}

struct EditComment {
    id: u64,
    operation: crate::annotations::Operation,
    source: PathBuf,
    comment: crate::annotations::Comment,
}

struct OpenRequest {
    id: u64,
    path: PathBuf,
    password: String,
    keep_on_error: bool,
}
struct SearchRequest {
    revision: u64,
    query: String,
    options: SearchOptions,
}

struct SearchProgress {
    request: SearchRequest,
    query: Option<SearchQuery>,
    page: usize,
    hits: usize,
    cursor: text::SearchCursor,
    text: Option<Arc<PageText>>,
    updates: search_updates::Updates,
}

impl SearchProgress {
    fn new(request: SearchRequest) -> Self {
        Self {
            request,
            query: None,
            page: 0,
            hits: 0,
            cursor: text::SearchCursor::default(),
            text: None,
            updates: search_updates::Updates::new(Instant::now()),
        }
    }
}

#[derive(Default)]
struct Pending {
    text_style: TextStyle,
    open: Option<OpenRequest>,
    close: bool,
    render: VecDeque<RenderKey>,
    render_revision: u64,
    thumbnails: VecDeque<RenderKey>,
    thumbnail_revision: u64,
    search: Option<SearchRequest>,
    copy: Option<usize>,
    links: Option<usize>,
    inspect: Option<Inspection>,
    comments: bool,
    edit_comment: Option<EditComment>,
    stop: bool,
}

#[derive(Clone, Copy, Default)]
struct TextStyle {
    weight: f32,
    revision: u64,
}

struct Inspection {
    page: usize,
    interactive: bool,
}

impl Pending {
    fn clear_jobs(&mut self) {
        self.render.clear();
        self.thumbnails.clear();
        self.search = None;
        self.copy = None;
        self.links = None;
        self.inspect = None;
        self.comments = false;
        self.edit_comment = None;
    }

    fn replace_renders(&mut self, keys: Vec<RenderKey>) {
        self.render_revision = self.render_revision.wrapping_add(1);
        self.render = keys.into();
    }

    fn wants_render(&self, key: RenderKey, revision: u64) -> bool {
        !self.stop
            && !self.close
            && self.open.is_none()
            && (revision == self.render_revision || self.render.contains(&key))
    }

    fn finish_render(&mut self, key: RenderKey, revision: u64) -> bool {
        if !self.wants_render(key, revision) {
            return false;
        }
        // A view update can request the same page while it is still rendering.
        self.render.retain(|pending| *pending != key);
        true
    }

    fn replace_thumbnails(&mut self, keys: Vec<RenderKey>) {
        self.thumbnail_revision = self.thumbnail_revision.wrapping_add(1);
        self.thumbnails = keys.into_iter().take(16).collect();
    }

    fn wants_thumbnail(&self, key: RenderKey, revision: u64) -> bool {
        !self.stop
            && !self.close
            && self.open.is_none()
            && (revision == self.thumbnail_revision || self.thumbnails.contains(&key))
    }

    fn finish_thumbnail(&mut self, key: RenderKey, revision: u64) -> bool {
        if !self.wants_thumbnail(key, revision) {
            return false;
        }
        self.thumbnails.retain(|pending| *pending != key);
        true
    }
}

type Shared = Arc<(Mutex<Pending>, Condvar)>;

// Palettes are fixed for a worker's lifetime. Text weight can change between
// renders; its revision follows every result through the bounded event queue.
#[derive(Clone, Copy, Default)]
struct RenderAppearance {
    colors: crate::page_colors::PageColors,
    settings: hayro::RenderSettings,
}

pub struct Worker {
    shared: Shared,
    pub events: Receiver<Event>,
}

impl Worker {
    #[cfg(test)]
    pub fn new(context: egui::Context) -> Self {
        Self::with_colors(context, crate::page_colors::PageColors::default())
    }

    #[cfg(test)]
    pub fn with_colors(context: egui::Context, colors: crate::page_colors::PageColors) -> Self {
        Self::with_appearance(context, colors, hayro::RenderSettings::default())
    }

    pub fn with_appearance(
        context: egui::Context,
        colors: crate::page_colors::PageColors,
        settings: hayro::RenderSettings,
    ) -> Self {
        Self::with_loader_and_appearance(
            context,
            RenderAppearance { colors, settings },
            |path, password, cancelled| document::load_cancellable(path, password, cancelled),
        )
    }

    #[cfg(test)]
    fn with_loader(
        context: egui::Context,
        load: impl FnMut(&std::path::Path, &str, &dyn Fn() -> bool) -> Result<Option<Pdf>, LoadError>
        + Send
        + 'static,
    ) -> Self {
        Self::with_loader_and_appearance(context, RenderAppearance::default(), load)
    }

    fn with_loader_and_appearance(
        context: egui::Context,
        appearance: RenderAppearance,
        mut load: impl FnMut(
            &std::path::Path,
            &str,
            &dyn Fn() -> bool,
        ) -> Result<Option<Pdf>, LoadError>
        + Send
        + 'static,
    ) -> Self {
        let pending = Pending {
            text_style: TextStyle {
                weight: appearance.settings.text_embolden,
                revision: 0,
            },
            ..Default::default()
        };
        let shared = Arc::new((Mutex::new(pending), Condvar::new()));
        let (tx, events) = mpsc::sync_channel(4);
        let state = shared.clone();
        std::thread::Builder::new()
            .name("pagewing-render".into())
            .spawn(move || {
                let mut active: Option<(Pdf, u64, FileWatch)> = None;
                loop {
                    let (close, request) = {
                        let (lock, wake) = &*state;
                        let mut pending = lock.lock().unwrap();
                        while pending.open.is_none() && !pending.close && !pending.stop {
                            pending = wake.wait(pending).unwrap();
                        }
                        if pending.stop {
                            return;
                        }
                        (std::mem::take(&mut pending.close), pending.open.take())
                    };
                    if close {
                        // Drop the PDF outside the task lock, after session caches
                        // have been released. The same worker can open another file.
                        active = None;
                    }
                    let Some(request) = request else {
                        continue;
                    };
                    let id = request.id;
                    if !request.keep_on_error {
                        active = None;
                    }
                    let cancelled = || {
                        let pending = state.0.lock().unwrap();
                        pending.stop || pending.close || pending.open.is_some()
                    };
                    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                        if cancelled() {
                            return Ok(None);
                        }
                        let watch = FileWatch::new(request.path.clone(), Instant::now());
                        let Some(pdf) = load(&request.path, &request.password, &cancelled)? else {
                            return Ok(None);
                        };
                        if cancelled() {
                            return Ok(None);
                        }
                        let mut dimensions = Vec::with_capacity(pdf.pages().len());
                        for (index, page) in pdf.pages().iter().enumerate() {
                            if index % 256 == 0 && cancelled() {
                                return Ok(None);
                            }
                            let (w, h) = page.render_dimensions();
                            if !w.is_finite() || !h.is_finite() || w <= 0.0 || h <= 0.0 {
                                return Err("This PDF contains invalid page dimensions.".into());
                            }
                            dimensions.push([w, h]);
                        }
                        if cancelled() {
                            return Ok(None);
                        }
                        let outline = crate::outline::read(&pdf);
                        let Some(outline) =
                            crate::outline_view::OutlineView::prepare(outline, &cancelled)
                        else {
                            return Ok(None);
                        };
                        let outline = Box::new(outline);
                        let labels = crate::page_labels::PageLabels::read(&pdf);
                        if cancelled() {
                            return Ok(None);
                        }
                        let info = crate::document_info::DocumentInfo::read(&pdf);
                        Ok::<_, LoadError>(Some((pdf, dimensions, outline, labels, info, watch)))
                    }))
                    .unwrap_or_else(|_| {
                        Err("The PDF engine could not process this document.".into())
                    });
                    // Obsolete errors must not reopen a password prompt, and
                    // old completions must not occupy the bounded event queue.
                    if cancelled() {
                        continue;
                    }
                    match result {
                        Ok(Some((pdf, dimensions, outline, labels, info, watch))) => {
                            let sync_geometry = pdf
                                .pages()
                                .iter()
                                .map(crate::synctex::PageGeometry::new)
                                .collect();
                            active = Some((pdf, id, watch));
                            emit(
                                &tx,
                                &context,
                                Event::Opened {
                                    id,
                                    dimensions,
                                    sync_geometry,
                                    info,
                                    outline,
                                    labels,
                                },
                            );
                        }
                        Ok(None) => continue,
                        Err(error) => emit(
                            &tx,
                            &context,
                            Event::Error {
                                id,
                                error: Failure::Open(error),
                            },
                        ),
                    }
                    if let Some((pdf, active_id, watch)) = &mut active
                        && std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                            session(pdf, *active_id, watch, &state, &tx, &context, appearance);
                        }))
                        .is_err()
                    {
                        emit(
                            &tx,
                            &context,
                            Event::Error {
                                id: *active_id,
                                error: Failure::Session,
                            },
                        );
                    }
                }
            })
            .expect("Cannot start PDF worker");
        Self { shared, events }
    }

    fn update<T>(&self, change: impl FnOnce(&mut Pending) -> T) -> T {
        let (lock, wake) = &*self.shared;
        let result = change(&mut lock.lock().unwrap());
        wake.notify_one();
        result
    }

    pub fn set_text_embolden(&self, weight: f32) -> u64 {
        self.update(|pending| {
            pending.text_style.weight = weight;
            pending.text_style.revision = pending.text_style.revision.wrapping_add(1);
            pending.render.clear();
            pending.thumbnails.clear();
            pending.text_style.revision
        })
    }

    pub fn open(&self, id: u64, path: PathBuf, password: String) {
        self.request_open(id, path, password, false);
    }

    pub fn reload(&self, id: u64, path: PathBuf, password: String) {
        self.request_open(id, path, password, true);
    }

    fn request_open(&self, id: u64, path: PathBuf, password: String, keep_on_error: bool) {
        self.update(|pending| {
            pending.open = Some(OpenRequest {
                id,
                path,
                password,
                keep_on_error,
            });
            pending.clear_jobs();
        });
    }

    pub fn close(&self) {
        self.update(|pending| {
            pending.open = None;
            pending.close = true;
            pending.clear_jobs();
        });
    }

    pub fn render(&self, keys: Vec<RenderKey>) {
        self.update(|pending| pending.replace_renders(keys));
    }
    pub fn thumbnails(&self, keys: Vec<RenderKey>) {
        self.update(|pending| pending.replace_thumbnails(keys));
    }
    pub fn search(&self, revision: u64, query: String, options: SearchOptions) {
        self.update(|pending| {
            pending.search = Some(SearchRequest {
                revision,
                query,
                options,
            })
        });
    }
    pub fn comments(&self) {
        self.update(|pending| pending.comments = true);
    }

    pub fn save_comment(&self, id: u64, source: PathBuf, comment: crate::annotations::Comment) {
        self.edit_comment(id, source, comment, crate::annotations::Operation::Save);
    }

    pub fn delete_comment(&self, id: u64, source: PathBuf, comment: crate::annotations::Comment) {
        self.edit_comment(id, source, comment, crate::annotations::Operation::Delete);
    }

    fn edit_comment(
        &self,
        id: u64,
        source: PathBuf,
        comment: crate::annotations::Comment,
        operation: crate::annotations::Operation,
    ) {
        self.update(|pending| {
            pending.edit_comment = Some(EditComment {
                id,
                source,
                comment,
                operation,
            });
        });
    }

    pub fn copy(&self, page: usize) {
        self.update(|pending| pending.copy = Some(page));
    }
    pub fn links(&self, page: usize) {
        self.update(|pending| pending.links = Some(page));
    }
    pub fn inspect(&self, page: usize) {
        self.request_inspection(page, true);
    }

    pub fn inspect_all(&self, page: usize) {
        self.request_inspection(page, false);
    }

    fn request_inspection(&self, page: usize, interactive: bool) {
        self.update(|pending| pending.inspect = Some(Inspection { page, interactive }));
    }
}

impl Drop for Worker {
    fn drop(&mut self) {
        self.update(|pending| pending.stop = true);
    }
}

fn emit(tx: &SyncSender<Event>, context: &egui::Context, event: Event) {
    if tx.send(event).is_ok() {
        context.request_repaint();
    }
}

enum Job {
    Comments,
    EditComment(EditComment),
    Render(RenderKey, u64),
    Thumbnail(RenderKey, u64),
    Search(usize),
    Copy(usize),
    Links(usize),
    Inspect(Inspection),
}

fn session(
    pdf: &Pdf,
    id: u64,
    watch: &mut FileWatch,
    shared: &Shared,
    tx: &SyncSender<Event>,
    context: &egui::Context,
    appearance: RenderAppearance,
) {
    let render_cache = RenderCache::new();
    let mut rendered_weight = appearance.settings.text_embolden;
    let interpreter_cache = InterpreterCache::new();
    let link_reader = crate::links::LinkReader::new(pdf);
    let mut texts = crate::text_cache::TextCache::default();
    let mut search: Option<SearchProgress> = None;
    let mut background = scheduling::Background::default();
    loop {
        let job = {
            let (lock, wake) = &**shared;
            let mut pending = lock.lock().unwrap();
            loop {
                if pending.stop || pending.close || pending.open.is_some() {
                    return;
                }
                if watch.remaining(Instant::now()).is_zero() {
                    // Filesystem I/O and bounded event sends must not hold the
                    // task lock. Check between jobs too, even during searches.
                    drop(pending);
                    if watch.poll(Instant::now()) {
                        emit(tx, context, Event::FileChanged { id });
                    }
                    pending = lock.lock().unwrap();
                    continue;
                }
                if let Some(request) = pending.search.take() {
                    search = if request.query.is_empty() {
                        None
                    } else {
                        Some(SearchProgress::new(request))
                    };
                }
                if let Some(request) = pending.edit_comment.take() {
                    break Job::EditComment(request);
                }
                if let Some(request) = pending.inspect.take() {
                    break Job::Inspect(request);
                }
                if let Some(page) = pending.copy.take() {
                    break Job::Copy(page);
                }
                if let Some(page) = pending.links.take() {
                    break Job::Links(page);
                }
                if let Some(key) = pending.render.pop_front() {
                    break Job::Render(key, pending.render_revision);
                }
                if std::mem::take(&mut pending.comments) {
                    break Job::Comments;
                }
                if let Some(job) =
                    background.next(&mut pending, search.as_ref().map(|progress| progress.page))
                {
                    break job;
                }
                pending = wake
                    .wait_timeout(pending, watch.remaining(Instant::now()))
                    .unwrap()
                    .0;
            }
        };
        if matches!(job, Job::Search(_)) {
            let progress = search.as_mut().expect("Search jobs have active progress");
            if progress.query.is_none() {
                // Prepare once, after higher-priority jobs, without the task lock.
                let query = SearchQuery::new(
                    std::mem::take(&mut progress.request.query),
                    progress.request.options,
                );
                if query.is_empty() {
                    search = None;
                    continue;
                }
                progress.query = Some(query);
                // Loading or a newer query may have arrived during normalization.
                let pending = shared.0.lock().unwrap();
                if pending.stop
                    || pending.close
                    || pending.open.is_some()
                    || pending.search.is_some()
                    || pending.copy.is_some()
                    || pending.inspect.is_some()
                    || pending.links.is_some()
                    || !pending.render.is_empty()
                {
                    continue;
                }
            }
        }
        match job {
            Job::Comments => {
                let cancelled = || {
                    let pending = shared.0.lock().unwrap();
                    pending.stop || pending.close || pending.open.is_some()
                };
                let comments = crate::annotations::read(pdf, cancelled);
                if !cancelled() {
                    emit(tx, context, Event::Comments { id, comments });
                }
            }
            Job::EditComment(request) => {
                if request.id != id {
                    continue;
                }
                let result =
                    std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                        match request.operation {
                            crate::annotations::Operation::Save => {
                                crate::annotations::save(pdf, &request.comment, &request.source)
                            }
                            crate::annotations::Operation::Delete => {
                                crate::annotations::delete(pdf, &request.comment, &request.source)
                            }
                        }
                    }))
                    .unwrap_or_else(|_| Err("The PDF engine could not save this comment.".into()));
                emit(
                    tx,
                    context,
                    Event::CommentChanged {
                        id,
                        operation: request.operation,
                        result,
                    },
                );
            }
            Job::Links(page) => {
                if let Some(data) = pdf.pages().get(page) {
                    let links = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                        link_reader.read(data)
                    }))
                    .unwrap_or_default();
                    emit(tx, context, Event::Links { id, page, links });
                }
            }
            Job::Render(key, revision) | Job::Thumbnail(key, revision) => {
                let thumbnail = matches!(job, Job::Thumbnail(..));
                let style = shared.0.lock().unwrap().text_style;
                if style.weight != rendered_weight {
                    render_cache.clear_text_weight_cache();
                    rendered_weight = style.weight;
                }
                let settings = hayro::RenderSettings {
                    text_embolden: style.weight,
                    ..appearance.settings
                };
                let start = Instant::now();
                let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    if let Some(tile) = key.tile {
                        document::rasterize_tile_with_settings(
                            pdf,
                            &render_cache,
                            key,
                            tile,
                            &settings,
                        )
                    } else {
                        document::rasterize_pixels_with_settings(
                            pdf,
                            &render_cache,
                            key.page,
                            key.width,
                            &settings,
                        )
                    }
                }));
                let relevant = {
                    let pending = shared.0.lock().unwrap();
                    if pending.text_style.revision != style.revision {
                        false
                    } else if thumbnail {
                        pending.wants_thumbnail(key, revision)
                    } else {
                        pending.wants_render(key, revision)
                    }
                };
                if !relevant {
                    continue;
                }
                // Discard obsolete work before transforming the raster target.
                // The receiver only uploads finished texture images.
                let result = result.map(|result| {
                    result.map(|image| {
                        crate::texture_pixels::prepare_with_palette(
                            image,
                            if key.tile.is_some() { 0 } else { key.rotation },
                            appearance.colors.palette(key.night),
                        )
                    })
                });
                // Requests can change during pixel preparation too. Consume any
                // duplicate still-needed job only when its final image is ready.
                let relevant = {
                    let mut pending = shared.0.lock().unwrap();
                    if pending.text_style.revision != style.revision {
                        false
                    } else if thumbnail {
                        pending.finish_thumbnail(key, revision)
                    } else {
                        pending.finish_render(key, revision)
                    }
                };
                if !relevant {
                    continue;
                }
                let milliseconds = start.elapsed().as_secs_f64() * 1000.0;
                if thumbnail {
                    emit(
                        tx,
                        context,
                        Event::Thumbnail {
                            id,
                            key,
                            image: result.ok().and_then(Result::ok),
                            appearance_revision: style.revision,
                        },
                    );
                    continue;
                }
                match result {
                    Ok(Ok(image)) => emit(
                        tx,
                        context,
                        Event::Rendered {
                            id,
                            key,
                            image,
                            milliseconds,
                            appearance_revision: style.revision,
                        },
                    ),
                    result => {
                        let message = match result {
                            Ok(Err(message)) => message,
                            _ => "This page could not be rendered.".into(),
                        };
                        emit(
                            tx,
                            context,
                            Event::Error {
                                id,
                                error: Failure::Render(message),
                            },
                        );
                    }
                }
            }
            Job::Search(page) | Job::Copy(page) | Job::Inspect(Inspection { page, .. }) => {
                if page >= pdf.pages().len() {
                    search = None;
                    continue;
                }
                let retained = search
                    .as_ref()
                    .filter(|progress| progress.page == page)
                    .and_then(|progress| progress.text.clone());
                let text = if let Some(text) = retained.or_else(|| texts.get(page)) {
                    text
                } else {
                    let text = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                        text::extract(&pdf.pages()[page], &interpreter_cache)
                    }))
                    .unwrap_or_default();
                    let text = Arc::new(text);
                    texts.insert(page, Arc::clone(&text));
                    text
                };
                if let Job::Inspect(Inspection { interactive, .. }) = job {
                    if interactive {
                        text.prepare_selection();
                        texts.refresh(page, &text);
                    }
                    emit(
                        tx,
                        context,
                        Event::PageText {
                            id,
                            page,
                            text: Arc::clone(&text),
                        },
                    );
                } else if matches!(job, Job::Copy(_)) {
                    emit(
                        tx,
                        context,
                        Event::Text {
                            id,
                            page,
                            text: text.text.clone(),
                        },
                    );
                } else if let Some(progress) = &mut search {
                    let limit = (10_000_usize - progress.hits).min(256);
                    let batch = text.search_batch(
                        page,
                        progress
                            .query
                            .as_ref()
                            .expect("Search jobs prepare their query"),
                        &mut progress.cursor,
                        limit,
                    );
                    background.searched();
                    texts.refresh(page, &text);
                    progress.hits += batch.hits.len();
                    if batch.complete {
                        progress.page += 1;
                        progress.cursor = text::SearchCursor::default();
                        progress.text = None;
                    } else {
                        // Retain even uncached oversized pages across batches.
                        progress.text = Some(text);
                    }
                    let complete = progress.page >= pdf.pages().len() || progress.hits >= 10_000;
                    if progress
                        .updates
                        .publish(!batch.hits.is_empty(), complete, Instant::now())
                    {
                        emit(
                            tx,
                            context,
                            Event::Search {
                                id,
                                revision: progress.request.revision,
                                hits: batch.hits,
                                scanned: progress.page,
                                total: pdf.pages().len(),
                                complete,
                                limited: progress.hits >= 10_000,
                            },
                        );
                    }
                    if complete {
                        search = None;
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    fn receive(worker: &Worker) -> Event {
        worker
            .events
            .recv_timeout(Duration::from_secs(5))
            .expect("Worker did not finish in time")
    }

    fn sample() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("assets/pagewing-guide.pdf")
    }

    fn dense_sample(count: usize) -> tempfile::NamedTempFile {
        let stream = format!("BT /F1 6 Tf 10 750 Td ({}) Tj ET", "dense ".repeat(count));
        let second = "BT /F1 6 Tf 10 750 Td (dense) Tj ET";
        let objects = [
            "<< /Type /Catalog /Pages 2 0 R >>".to_owned(),
            "<< /Type /Pages /Kids [3 0 R 5 0 R] /Count 2 >>".into(),
            "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 600 800] /Resources << /Font << /F1 7 0 R >> >> /Contents 4 0 R >>".into(),
            format!("<< /Length {} >>\nstream\n{stream}\nendstream", stream.len()),
            "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 600 800] /Resources << /Font << /F1 7 0 R >> >> /Contents 6 0 R >>".into(),
            format!("<< /Length {} >>\nstream\n{second}\nendstream", second.len()),
            "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>".into(),
        ];
        let mut data = "%PDF-1.4\n".to_owned();
        let mut offsets = Vec::new();
        for (index, object) in objects.iter().enumerate() {
            offsets.push(data.len());
            data.push_str(&format!("{} 0 obj\n{object}\nendobj\n", index + 1));
        }
        let xref = data.len();
        data.push_str("xref\n0 8\n0000000000 65535 f \n");
        for offset in offsets {
            data.push_str(&format!("{offset:010} 00000 n \n"));
        }
        data.push_str(&format!(
            "trailer\n<< /Size 8 /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n"
        ));
        let file = tempfile::NamedTempFile::new().unwrap();
        std::fs::write(file.path(), data).unwrap();
        file
    }

    #[test]
    fn dense_page_search_continues_past_one_thousand_and_advances_pages() {
        let file = dense_sample(1500);
        let worker = Worker::new(egui::Context::default());
        worker.open(1, file.path().into(), String::new());
        assert!(matches!(receive(&worker), Event::Opened { .. }));
        worker.search(1, "dense".into(), SearchOptions::default());
        let mut count = 0;
        let mut last_page = 0;
        loop {
            let Event::Search {
                hits,
                complete,
                limited,
                scanned,
                ..
            } = receive(&worker)
            else {
                panic!("Expected search batch")
            };
            assert!(hits.len() <= 256 && !limited);
            for hit in hits {
                assert!(hit.page >= last_page);
                last_page = hit.page;
                count += 1;
            }
            if complete {
                assert_eq!(scanned, 2);
                break;
            }
        }
        assert_eq!(count, 1501);
        assert_eq!(last_page, 1);
    }

    #[test]
    fn dense_page_batches_yield_to_rendering_and_stop_at_the_global_cap() {
        let file = dense_sample(12000);
        let worker = Worker::new(egui::Context::default());
        worker.open(1, file.path().into(), String::new());
        assert!(matches!(receive(&worker), Event::Opened { .. }));
        worker.search(1, "dense".into(), SearchOptions::default());
        let Event::Search {
            hits,
            complete: false,
            scanned: 0,
            ..
        } = receive(&worker)
        else {
            panic!("Expected partial first page")
        };
        let mut count = hits.len();
        worker.render(vec![render_key(1)]);
        let mut rendered = false;
        let mut before_render = 0;
        loop {
            match receive(&worker) {
                Event::Search {
                    hits,
                    complete,
                    limited,
                    scanned,
                    ..
                } => {
                    count += hits.len();
                    assert!(hits.len() <= 256 && hits.iter().all(|hit| hit.page == 0));
                    if !rendered {
                        before_render += 1;
                    }
                    if complete {
                        assert!(limited && rendered);
                        assert_eq!(scanned, 0);
                        break;
                    }
                }
                Event::Rendered {
                    key: RenderKey { page: 1, .. },
                    ..
                } => rendered = true,
                _ => panic!("Unexpected worker event"),
            }
        }
        assert!(
            before_render <= 5,
            "Rendering waited for unnecessary search batches"
        );
        assert_eq!(count, 10000);
        worker.copy(1);
        assert!(matches!(receive(&worker), Event::Text { page: 1, .. }));
    }

    #[test]
    fn replacing_search_cancels_remaining_batches_of_a_dense_page() {
        let file = dense_sample(12000);
        let worker = Worker::new(egui::Context::default());
        worker.open(1, file.path().into(), String::new());
        assert!(matches!(receive(&worker), Event::Opened { .. }));
        worker.search(1, "dense".into(), SearchOptions::default());
        assert!(matches!(
            receive(&worker),
            Event::Search {
                revision: 1,
                complete: false,
                ..
            }
        ));
        worker.search(2, "absent".into(), SearchOptions::default());
        let mut old_batches = 0;
        loop {
            match receive(&worker) {
                Event::Search {
                    revision: 1,
                    complete: false,
                    ..
                } => old_batches += 1,
                Event::Search {
                    revision: 2,
                    hits,
                    complete,
                    ..
                } => {
                    assert!(hits.is_empty());
                    if complete {
                        break;
                    }
                }
                _ => panic!("Unexpected result after replacing a dense search"),
            }
        }
        assert!(old_batches <= 5);
    }

    #[test]
    fn whitespace_query_cancels_dense_search_and_leaves_the_worker_idle() {
        let file = dense_sample(12000);
        let worker = Worker::new(egui::Context::default());
        worker.open(1, file.path().into(), String::new());
        assert!(matches!(receive(&worker), Event::Opened { .. }));
        worker.search(1, "dense".into(), SearchOptions::default());
        assert!(matches!(
            receive(&worker),
            Event::Search {
                revision: 1,
                complete: false,
                ..
            }
        ));
        worker.search(2, " \n\t".into(), SearchOptions::default());
        worker.copy(1);
        loop {
            match receive(&worker) {
                Event::Search {
                    revision: 1,
                    complete: false,
                    ..
                } => {}
                Event::Text { page: 1, .. } => break,
                _ => panic!("Whitespace cancellation must not publish a new scan"),
            }
        }
        assert!(matches!(
            worker.events.recv_timeout(Duration::from_millis(100)),
            Err(mpsc::RecvTimeoutError::Timeout)
        ));
    }

    fn render_key(page: usize) -> RenderKey {
        RenderKey {
            page,
            width: 300,
            rotation: 0,
            night: false,
            tile: None,
        }
    }

    #[test]
    fn superseded_renders_are_discarded_but_still_requested_results_are_reused() {
        let mut pending = Pending::default();
        pending.replace_renders(vec![render_key(0)]);
        let revision = pending.render_revision;
        let running = pending.render.pop_front().unwrap();
        pending.replace_renders(vec![render_key(1)]);
        assert!(!pending.finish_render(running, revision));
        assert_eq!(pending.render.front(), Some(&render_key(1)));

        let running = pending.render.pop_front().unwrap();
        let revision = pending.render_revision;
        pending.replace_renders(vec![running, render_key(2)]);
        assert!(pending.finish_render(running, revision));
        assert_eq!(
            pending.render.into_iter().collect::<Vec<_>>(),
            [render_key(2)]
        );
    }

    #[test]
    fn texture_preparation_rechecks_cancellation_and_coalesces_new_requests() {
        let mut pending = Pending::default();
        let running = render_key(0);
        pending.replace_renders(vec![running]);
        pending.replace_thumbnails(vec![running]);
        let (render_revision, thumbnail_revision) =
            (pending.render_revision, pending.thumbnail_revision);
        pending.render.pop_front();
        pending.thumbnails.pop_front();
        // The raster is wanted before starting its texture preparation.
        assert!(pending.wants_render(running, render_revision));
        assert!(pending.wants_thumbnail(running, thumbnail_revision));
        // A new view arrives while the final pixels are being prepared.
        pending.replace_renders(vec![render_key(1)]);
        pending.replace_thumbnails(vec![render_key(1)]);
        assert!(!pending.finish_render(running, render_revision));
        assert!(!pending.finish_thumbnail(running, thumbnail_revision));
        assert_eq!(pending.render.front(), Some(&render_key(1)));
        assert_eq!(pending.thumbnails.front(), Some(&render_key(1)));

        pending.replace_renders(vec![running, render_key(2)]);
        pending.replace_thumbnails(vec![running, render_key(2)]);
        assert!(pending.wants_render(running, render_revision));
        assert!(pending.wants_thumbnail(running, thumbnail_revision));
        assert_eq!(pending.render.front(), Some(&running));
        assert_eq!(pending.thumbnails.front(), Some(&running));
        // Further same-key requests during preparation must reuse the result.
        pending.replace_renders(vec![running, render_key(3)]);
        pending.replace_thumbnails(vec![running, render_key(3)]);
        assert!(pending.finish_render(running, render_revision));
        assert!(pending.finish_thumbnail(running, thumbnail_revision));
        assert_eq!(
            pending.render.into_iter().collect::<Vec<_>>(),
            [render_key(3)]
        );
        assert_eq!(
            pending.thumbnails.into_iter().collect::<Vec<_>>(),
            [render_key(3)]
        );
    }

    #[test]
    fn thumbnail_requests_are_bounded_cancelable_and_reuse_still_needed_work() {
        let mut pending = Pending::default();
        pending.replace_thumbnails((0..100).map(render_key).collect());
        assert_eq!(pending.thumbnails.len(), 16);
        let revision = pending.thumbnail_revision;
        let running = pending.thumbnails.pop_front().unwrap();
        pending.replace_thumbnails(vec![render_key(20)]);
        assert!(!pending.finish_thumbnail(running, revision));
        let revision = pending.thumbnail_revision;
        let running = pending.thumbnails.pop_front().unwrap();
        pending.replace_thumbnails(vec![running, render_key(21)]);
        assert!(pending.finish_thumbnail(running, revision));
        assert_eq!(pending.thumbnails.front(), Some(&render_key(21)));
        let revision = pending.thumbnail_revision;
        pending.replace_thumbnails(Vec::new());
        assert!(!pending.finish_thumbnail(render_key(21), revision));
    }

    #[test]
    fn page_rendering_and_short_search_finish_before_queued_thumbnails() {
        let worker = Worker::new(egui::Context::default());
        worker.open(1, sample(), String::new());
        assert!(matches!(receive(&worker), Event::Opened { .. }));
        worker.update(|pending| {
            pending.replace_thumbnails(vec![render_key(1)]);
            pending.replace_renders(vec![render_key(0)]);
            pending.search = Some(SearchRequest {
                revision: 1,
                query: "Rust".into(),
                options: SearchOptions::default(),
            });
        });
        assert!(matches!(
            receive(&worker),
            Event::Rendered {
                key: RenderKey { page: 0, .. },
                ..
            }
        ));
        loop {
            match receive(&worker) {
                Event::Search { complete: true, .. } => break,
                Event::Search { .. } => {}
                _ => panic!("Thumbnails must follow a short document search"),
            }
        }
        assert!(matches!(
            receive(&worker),
            Event::Thumbnail {
                key: RenderKey { page: 1, .. },
                image: Some(_),
                ..
            }
        ));
    }

    #[test]
    fn live_text_weight_changes_reuse_the_session_and_tag_every_image() {
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/text-weight.pdf");
        let worker = Worker::new(egui::Context::default());
        worker.open(1, path.clone(), String::new());
        assert!(matches!(receive(&worker), Event::Opened { .. }));
        let pdf = document::load(&path, "").unwrap();
        let cache = RenderCache::new();
        let key = RenderKey {
            page: 0,
            width: 800,
            rotation: 0,
            night: false,
            tile: None,
        };
        for weight in [0.2, 0.4, 0.0, 0.2] {
            let revision = worker.set_text_embolden(weight);
            worker.render(vec![key]);
            worker.thumbnails(vec![key]);
            let settings = hayro::RenderSettings {
                text_embolden: weight,
                ..Default::default()
            };
            let expected =
                document::rasterize_pixels_with_settings(&pdf, &cache, 0, 800, &settings).unwrap();
            for thumbnail in [false, true] {
                let (image, actual_revision) = match receive(&worker) {
                    Event::Rendered {
                        image,
                        appearance_revision,
                        ..
                    } if !thumbnail => (image, appearance_revision),
                    Event::Thumbnail {
                        image: Some(image),
                        appearance_revision,
                        ..
                    } if thumbnail => (image, appearance_revision),
                    _ => panic!("expected updated weight image"),
                };
                assert_eq!(actual_revision, revision);
                assert_eq!(image.pixels, expected.pixels);
            }
        }
        worker.copy(0);
        assert!(matches!(receive(&worker), Event::Text { text, .. } if text.contains("Times")));
    }

    #[test]
    fn text_embolden_reaches_pages_thumbnails_and_rotated_tiles_in_both_modes() {
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/text-weight.pdf");
        let settings = hayro::RenderSettings {
            text_embolden: 0.3,
            ..Default::default()
        };
        let colors = crate::page_colors::PageColors::default();
        let worker = Worker::with_appearance(egui::Context::default(), colors, settings);
        worker.open(1, path.clone(), String::new());
        assert!(matches!(receive(&worker), Event::Opened { .. }));
        let pdf = document::load(&path, "").unwrap();
        let cache = RenderCache::new();
        for rotation in 0..4 {
            for night in [false, true] {
                for tiled in [false, true] {
                    let key = RenderKey {
                        page: 0,
                        width: 800,
                        rotation,
                        night,
                        tile: tiled.then_some(document::Tile {
                            scale_bits: 2.0_f32.to_bits(),
                            x: 0,
                            y: 0,
                            size: if rotation % 2 == 0 {
                                [800, 480]
                            } else {
                                [480, 800]
                            },
                        }),
                    };
                    let source = if let Some(tile) = key.tile {
                        document::rasterize_tile_with_settings(&pdf, &cache, key, tile, &settings)
                            .unwrap()
                    } else {
                        document::rasterize_pixels_with_settings(&pdf, &cache, 0, 800, &settings)
                            .unwrap()
                    };
                    let expected = crate::texture_pixels::prepare_with_palette(
                        source,
                        if tiled { 0 } else { rotation },
                        colors.palette(night),
                    );
                    worker.update(|pending| {
                        pending.replace_renders(vec![key]);
                        pending.replace_thumbnails(vec![key]);
                    });
                    for thumbnail in [false, true] {
                        let image = match receive(&worker) {
                            Event::Rendered {
                                key: actual, image, ..
                            } if !thumbnail && actual == key => image,
                            Event::Thumbnail {
                                key: actual,
                                image: Some(image),
                                ..
                            } if thumbnail && actual == key => image,
                            _ => panic!("Expected a prepared image for {key:?}"),
                        };
                        assert_eq!(image.pixels, expected.pixels);
                    }
                }
            }
        }
    }

    #[test]
    fn custom_palettes_reach_pages_thumbnails_and_tiles_in_both_modes() {
        use crate::page_colors::{InvertedPalette, PageColors, PagePalette, Rgb};
        let colors = PageColors {
            white: PagePalette {
                background: Rgb([255, 244, 220]),
                ..Default::default()
            },
            inverted: InvertedPalette {
                foreground: Rgb([205, 214, 244]),
                background: Rgb([30, 30, 46]),
                canvas: None,
            },
        };
        let worker = Worker::with_colors(egui::Context::default(), colors);
        worker.open(1, sample(), String::new());
        assert!(matches!(receive(&worker), Event::Opened { .. }));
        let pdf = document::load(&sample(), "").unwrap();
        let cache = RenderCache::new();
        for rotation in 0..4 {
            // Revisit white mode after inverted mode, as with repeated I presses.
            for night in [false, true, false] {
                for tiled in [false, true] {
                    let key = RenderKey {
                        page: 0,
                        width: 96,
                        rotation,
                        night,
                        tile: tiled.then_some(document::Tile {
                            scale_bits: 0.5_f32.to_bits(),
                            x: 0,
                            y: 0,
                            size: [32, 32],
                        }),
                    };
                    let source = if let Some(tile) = key.tile {
                        document::rasterize_tile(&pdf, &cache, key, tile).unwrap()
                    } else {
                        let bitmap = document::rasterize(
                            &pdf,
                            &cache,
                            RenderKey {
                                night: false,
                                ..key
                            },
                        )
                        .unwrap();
                        egui::ColorImage::from_rgba_premultiplied(bitmap.size, &bitmap.rgba)
                    };
                    worker.update(|pending| {
                        pending.replace_renders(vec![key]);
                        pending.replace_thumbnails(vec![key]);
                    });
                    for thumbnail in [false, true] {
                        let image = match receive(&worker) {
                            Event::Rendered {
                                key: actual, image, ..
                            } if !thumbnail && actual == key => image,
                            Event::Thumbnail {
                                key: actual,
                                image: Some(image),
                                ..
                            } if thumbnail && actual == key => image,
                            _ => panic!("Expected a prepared image for {key:?}"),
                        };
                        assert_eq!(image.size, source.size);
                        let palette = colors.palette(night);
                        for (actual, original) in image.pixels.iter().zip(&source.pixels) {
                            let mut expected = [0; 4];
                            for channel in 0..3 {
                                let t = f64::from(original[channel]) / 255.0;
                                expected[channel] =
                                    (f64::from(palette.foreground.0[channel]) * (1.0 - t)
                                        + f64::from(palette.background.0[channel]) * t)
                                        .round() as u8;
                            }
                            expected[3] = 255;
                            assert_eq!(actual.to_array(), expected, "{key:?}");
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn long_search_and_previews_both_advance_without_delaying_interactive_jobs() {
        let file = dense_sample(12000);
        let worker = Worker::new(egui::Context::default());
        worker.open(1, file.path().into(), String::new());
        assert!(matches!(receive(&worker), Event::Opened { .. }));
        let first = render_key(1);
        let second = RenderKey {
            rotation: 1,
            ..first
        };
        worker.update(|pending| {
            pending.replace_thumbnails(vec![first, second]);
            pending.replace_renders(vec![first]);
            pending.copy = Some(1);
            pending.search = Some(SearchRequest {
                revision: 9,
                query: "dense".into(),
                options: SearchOptions::default(),
            });
        });
        assert!(matches!(receive(&worker), Event::Text { page: 1, .. }));
        assert!(matches!(receive(&worker), Event::Rendered { key, .. } if key == first));
        let (mut complete, mut previews, mut hits, mut batches) = (false, 0, 0, 0);
        while !complete || previews < 2 {
            match receive(&worker) {
                Event::Search {
                    revision: 9,
                    hits: batch,
                    complete: done,
                    limited,
                    ..
                } => {
                    hits += batch.len();
                    batches += 1;
                    if done {
                        assert!(limited);
                        complete = true;
                    }
                }
                Event::Thumbnail {
                    key,
                    image: Some(_),
                    ..
                } => {
                    assert!(!complete, "previews must not wait for the whole search");
                    assert!(
                        (1..=8).contains(&batches),
                        "both tasks must advance: {batches} search batches"
                    );
                    assert_eq!(key, if previews == 0 { first } else { second });
                    previews += 1;
                    batches = 0;
                }
                _ => panic!("Unexpected worker event"),
            }
        }
        assert_eq!(
            hits, 10000,
            "yielding must preserve every accepted search hit"
        );
    }

    #[test]
    fn prepared_page_and_thumbnail_images_preserve_raster_pixels() {
        let worker = Worker::new(egui::Context::default());
        worker.open(1, sample(), String::new());
        assert!(matches!(receive(&worker), Event::Opened { .. }));
        let pdf = document::load(&sample(), "").unwrap();
        let cache = RenderCache::new();
        for rotation in 0..4 {
            for night in [false, true] {
                let key = RenderKey {
                    page: 0,
                    width: 96,
                    rotation,
                    night,
                    tile: None,
                };
                let expected = document::rasterize(&pdf, &cache, key).unwrap();
                worker.update(|pending| {
                    pending.replace_renders(vec![key]);
                    pending.replace_thumbnails(vec![key]);
                });
                for thumbnail in [false, true] {
                    let image = match receive(&worker) {
                        Event::Rendered {
                            id: 1,
                            key: actual,
                            image,
                            milliseconds,
                            appearance_revision: 0,
                        } if !thumbnail && actual == key => {
                            assert!(milliseconds.is_finite() && milliseconds >= 0.0);
                            image
                        }
                        Event::Thumbnail {
                            id: 1,
                            key: actual,
                            image: Some(image),
                            appearance_revision: 0,
                        } if thumbnail && actual == key => image,
                        _ => panic!("Expected a prepared image for {key:?}"),
                    };
                    assert_eq!(image.size, expected.size);
                    assert_eq!(image.pixels.len() * 4, expected.rgba.len());
                    for (pixel, expected) in
                        image.pixels.iter().zip(expected.rgba.as_chunks::<4>().0)
                    {
                        assert_eq!(pixel.to_array(), *expected);
                    }
                }
            }
        }
        let invalid = RenderKey {
            page: 99,
            ..render_key(0)
        };
        worker.update(|pending| {
            pending.replace_renders(vec![invalid]);
            pending.replace_thumbnails(vec![invalid]);
        });
        assert!(matches!(
            receive(&worker),
            Event::Error {
                error: Failure::Render(_),
                ..
            }
        ));
        assert!(matches!(
            receive(&worker),
            Event::Thumbnail { image: None, .. }
        ));
        worker.render(vec![render_key(0)]);
        assert!(matches!(receive(&worker), Event::Rendered { .. }));
    }

    #[test]
    fn document_switches_and_shutdown_discard_inflight_results() {
        let mut pending = Pending::default();
        pending.replace_renders(vec![render_key(0)]);
        let revision = pending.render_revision;
        pending.open = Some(OpenRequest {
            id: 2,
            path: sample(),
            password: String::new(),
            keep_on_error: false,
        });
        assert!(!pending.finish_render(render_key(0), revision));
        pending.open = None;
        pending.stop = true;
        assert!(!pending.finish_render(render_key(0), revision));
    }

    #[test]
    fn superseded_loads_do_not_publish_old_documents_or_errors() {
        for first_document in 0..3 {
            for close_first in [false, true] {
                let (entered_tx, entered_rx) = mpsc::channel();
                let (release_tx, release_rx) = mpsc::channel();
                let mut first = true;
                let worker =
                    Worker::with_loader(egui::Context::default(), move |path, password, _| {
                        let result = document::load_cancellable(path, password, || false);
                        if std::mem::take(&mut first) {
                            entered_tx.send(()).unwrap();
                            release_rx.recv_timeout(Duration::from_secs(5)).unwrap();
                        }
                        result
                    });
                worker.open(
                    1,
                    match first_document {
                        0 => sample(),
                        1 => sample().with_extension("missing"),
                        _ => PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                            .join("tests/fixtures/encrypted.pdf"),
                    },
                    String::new(),
                );
                entered_rx.recv_timeout(Duration::from_secs(5)).unwrap();
                if close_first {
                    worker.close();
                }
                worker.open(2, sample(), String::new());
                release_tx.send(()).unwrap();
                assert!(
                    matches!(receive(&worker), Event::Opened { id: 2, .. }),
                    "an obsolete load must not consume output capacity or request an old view"
                );
                worker.copy(0);
                assert!(
                    matches!(receive(&worker), Event::Text { id: 2, text, .. } if text.contains("little"))
                );
            }
        }
    }

    #[test]
    fn cancelled_reload_does_not_replace_the_snapshot_kept_after_a_later_failure() {
        let original = dense_sample(1);
        let replacement = dense_sample(1);
        let data = std::fs::read_to_string(replacement.path())
            .unwrap()
            .replace("dense", "fresh");
        std::fs::write(replacement.path(), data).unwrap();
        let (entered_tx, entered_rx) = mpsc::channel();
        let (release_tx, release_rx) = mpsc::channel();
        let mut calls = 0;
        let worker = Worker::with_loader(egui::Context::default(), move |path, password, _| {
            calls += 1;
            let result = document::load_cancellable(
                match calls {
                    1 => original.path(),
                    2 => replacement.path(),
                    _ => path,
                },
                password,
                || false,
            );
            if calls == 2 {
                entered_tx.send(()).unwrap();
                release_rx.recv_timeout(Duration::from_secs(5)).unwrap();
            }
            result
        });
        worker.open(7, sample(), String::new());
        assert!(matches!(receive(&worker), Event::Opened { id: 7, .. }));
        worker.reload(7, sample(), String::new());
        entered_rx.recv_timeout(Duration::from_secs(5)).unwrap();
        worker.reload(7, sample().with_extension("missing"), String::new());
        release_tx.send(()).unwrap();
        assert!(matches!(
            receive(&worker),
            Event::Error {
                id: 7,
                error: Failure::Open(_),
                ..
            }
        ));
        worker.copy(0);
        assert!(
            matches!(receive(&worker), Event::Text { id: 7, text, .. } if text.contains("dense") && !text.contains("fresh"))
        );
    }

    #[test]
    fn shutdown_cancels_an_inflight_load_and_exits_without_another_request() {
        let (entered_tx, entered_rx) = mpsc::channel();
        let (release_tx, release_rx) = mpsc::channel();
        let (done_tx, done_rx) = mpsc::channel();
        let worker = Worker::with_loader(egui::Context::default(), move |_, _, cancelled| {
            entered_tx.send(()).unwrap();
            release_rx.recv_timeout(Duration::from_secs(5)).unwrap();
            done_tx.send(cancelled()).unwrap();
            Ok(None)
        });
        worker.open(1, sample(), String::new());
        entered_rx.recv_timeout(Duration::from_secs(5)).unwrap();
        drop(worker);
        release_tx.send(()).unwrap();
        assert!(done_rx.recv_timeout(Duration::from_secs(5)).unwrap());
        assert_eq!(
            done_rx.recv_timeout(Duration::from_secs(5)),
            Err(mpsc::RecvTimeoutError::Disconnected)
        );
    }

    #[test]
    fn closing_discards_jobs_and_a_failed_reopen_cannot_restore_the_old_pdf() {
        let worker = Worker::new(egui::Context::default());
        worker.open(1, sample(), String::new());
        assert!(matches!(receive(&worker), Event::Opened { id: 1, .. }));
        // Coalesce close and reload exactly as rapidly arriving commands can do.
        worker.update(|pending| {
            pending.close = true;
            assert!(!pending.finish_render(render_key(0), pending.render_revision));
            assert!(!pending.finish_thumbnail(render_key(0), pending.thumbnail_revision));
            pending.open = Some(OpenRequest {
                id: 2,
                path: sample().with_extension("missing"),
                password: String::new(),
                keep_on_error: true,
            });
        });
        assert!(matches!(
            receive(&worker),
            Event::Error {
                id: 2,
                error: Failure::Open(_),
                ..
            }
        ));
        worker.copy(0);
        assert!(
            worker
                .events
                .recv_timeout(Duration::from_millis(100))
                .is_err()
        );

        worker.open(3, sample(), String::new());
        assert!(matches!(receive(&worker), Event::Opened { id: 3, .. }));
        worker.close();
        worker.copy(0);
        assert!(
            worker
                .events
                .recv_timeout(Duration::from_millis(100))
                .is_err()
        );
        worker.open(4, sample(), String::new());
        assert!(matches!(receive(&worker), Event::Opened { id: 4, .. }));
        worker.copy(0);
        assert!(matches!(receive(&worker), Event::Text { id: 4, .. }));
    }

    #[test]
    fn file_changes_notify_without_replacing_the_snapshot_and_survive_failed_reload() {
        let file = dense_sample(1);
        let worker = Worker::new(egui::Context::default());
        worker.open(31, file.path().into(), String::new());
        assert!(matches!(receive(&worker), Event::Opened { id: 31, .. }));
        let old = std::fs::read(file.path()).unwrap();
        let new = String::from_utf8(old.clone())
            .unwrap()
            .replace("dense", "fresh");
        std::fs::write(file.path(), new).unwrap();
        assert!(matches!(receive(&worker), Event::FileChanged { id: 31 }));
        worker.copy(0);
        assert!(matches!(receive(&worker), Event::Text { text, .. } if text.contains("dense")));
        worker.reload(31, file.path().into(), String::new());
        assert!(matches!(receive(&worker), Event::Opened { id: 31, .. }));
        worker.copy(0);
        assert!(matches!(receive(&worker), Event::Text { text, .. } if text.contains("fresh")));

        std::fs::remove_file(file.path()).unwrap();
        assert!(matches!(receive(&worker), Event::FileChanged { id: 31 }));
        worker.reload(31, file.path().into(), String::new());
        assert!(matches!(
            receive(&worker),
            Event::Error {
                id: 31,
                error: Failure::Open(_),
                ..
            }
        ));
        worker.copy(0);
        assert!(matches!(receive(&worker), Event::Text { text, .. } if text.contains("fresh")));
        std::fs::write(file.path(), old).unwrap();
        assert!(matches!(receive(&worker), Event::FileChanged { id: 31 }));
        worker.reload(31, file.path().into(), String::new());
        assert!(matches!(receive(&worker), Event::Opened { id: 31, .. }));
        worker.copy(0);
        assert!(matches!(receive(&worker), Event::Text { text, .. } if text.contains("dense")));

        worker.open(32, sample(), String::new());
        assert!(matches!(receive(&worker), Event::Opened { id: 32, .. }));
        std::fs::write(file.path(), b"changed after switching").unwrap();
        assert!(
            worker
                .events
                .recv_timeout(Duration::from_millis(1200))
                .is_err()
        );
        worker.close();
        worker.copy(0);
        assert!(
            worker
                .events
                .recv_timeout(Duration::from_millis(100))
                .is_err()
        );
    }

    #[test]
    fn worker_recovers_from_load_failure_and_publishes_outline() {
        let worker = Worker::new(egui::Context::default());
        worker.open(1, sample().with_extension("missing"), String::new());
        assert!(matches!(receive(&worker), Event::Error { id: 1, .. }));
        worker.open(2, sample(), String::new());
        match receive(&worker) {
            Event::Opened {
                id,
                dimensions,
                mut outline,
                ..
            } => {
                assert_eq!(id, 2);
                assert_eq!(dimensions.len(), 6);
                assert_eq!(outline.visible(), [0, 1, 2, 3, 4, 5]);
                assert_eq!(outline.selected(), Some(0));
                // Filtering is immediately usable without any UI preparation.
                outline.query = outline.entry(1).title.to_uppercase();
                outline.refilter();
                assert!(outline.visible().contains(&1));
                assert_eq!(outline.selected(), Some(1));
            }
            _ => panic!("Expected the second document to open"),
        }
    }

    #[test]
    fn rendering_copy_and_incremental_search_complete_together() {
        let worker = Worker::new(egui::Context::default());
        worker.open(7, sample(), String::new());
        assert!(matches!(receive(&worker), Event::Opened { id: 7, .. }));
        worker.search(12, "rust".into(), SearchOptions::default());
        worker.copy(0);
        worker.render(vec![RenderKey {
            page: 2,
            width: 300,
            rotation: 1,
            night: false,
            tile: None,
        }]);
        let (mut rendered, mut copied, mut complete, mut count) = (false, false, false, 0);
        while !(rendered && copied && complete) {
            match receive(&worker) {
                Event::Rendered { id, key, image, .. } => {
                    assert_eq!(id, 7);
                    assert_eq!(key.page, 2);
                    assert_eq!(image.size, [400, 300]);
                    rendered = true;
                }
                Event::Text { id, page, text } => {
                    assert_eq!((id, page), (7, 0));
                    assert!(text.contains("Rust"));
                    copied = true;
                }
                Event::Search {
                    id,
                    revision,
                    hits,
                    scanned,
                    total,
                    complete: done,
                    limited,
                } => {
                    assert_eq!((id, revision, total), (7, 12, 6));
                    assert!(!limited);
                    count += hits.len();
                    if done {
                        assert_eq!(scanned, total);
                        complete = true;
                    }
                }
                _ => panic!("Unexpected worker event"),
            }
        }
        assert_eq!(count, 4);
        worker.render(vec![render_key(99)]);
        assert!(matches!(
            receive(&worker),
            Event::Error {
                id: 7,
                error: Failure::Render(_),
                ..
            }
        ));
    }

    #[test]
    fn all_text_inspection_defers_interactive_indexes_and_reuses_the_cached_page() {
        let directory = tempfile::tempdir().unwrap();
        let mapping = directory.path().join("long-mapping.pdf");
        std::fs::write(
            &mapping,
            include_bytes!("../assets/selection-long-mapping.pdf"),
        )
        .unwrap();
        for (path, sparse) in [(sample(), false), (mapping, true)] {
            let worker = Worker::new(egui::Context::default());
            worker.open(1, path, String::new());
            assert!(matches!(receive(&worker), Event::Opened { .. }));
            worker.inspect_all(0);
            let all = match receive(&worker) {
                Event::PageText { page: 0, text, .. } => text,
                _ => panic!("Expected whole-page text"),
            };
            assert!(
                if sparse {
                    all.spans.len() == 1 && all.text.len() >= 4096
                } else {
                    all.spans.len() >= 256
                },
                "fixture must exercise indexed selection"
            );
            let bytes = all.memory_bytes();
            let rects = all.rects_for_range(0..all.text.len());
            let mut selection = crate::selection::TextSelection::default();
            selection.select_all();
            selection.set_text(Arc::clone(&all));
            assert!(selection.needs_preparation());
            selection.clear_range();
            assert!(
                selection.needs_preparation(),
                "a margin click must not lose the deferred inspection"
            );
            worker.inspect(0);
            let interactive = match receive(&worker) {
                Event::PageText { page: 0, text, .. } => text,
                _ => panic!("Expected interactive text"),
            };
            assert!(Arc::ptr_eq(&all, &interactive));
            assert!(
                !selection.needs_preparation(),
                "shared text sees the prepared index without another inspection"
            );
            assert!(interactive.memory_bytes() > bytes);
            assert_eq!(
                interactive.rects_for_range(0..interactive.text.len()),
                rects
            );
            let indexed_bytes = interactive.memory_bytes();
            worker.inspect_all(0);
            match receive(&worker) {
                Event::PageText { page: 0, text, .. } => {
                    assert!(Arc::ptr_eq(&all, &text));
                    assert_eq!(text.memory_bytes(), indexed_bytes);
                }
                _ => panic!("Expected the same indexed cached page"),
            }
        }
    }

    #[test]
    fn text_inspection_reuses_the_cached_page_and_survives_document_switches() {
        let worker = Worker::new(egui::Context::default());
        worker.open(9, sample(), String::new());
        assert!(matches!(receive(&worker), Event::Opened { id: 9, .. }));
        worker.inspect(0);
        let first = match receive(&worker) {
            Event::PageText {
                id: 9,
                page: 0,
                text,
            } => text,
            _ => panic!("Expected selectable text"),
        };
        assert!(first.text.contains("native"));
        worker.inspect(0);
        match receive(&worker) {
            Event::PageText {
                id: 9,
                page: 0,
                text,
            } => assert!(Arc::ptr_eq(&first, &text)),
            _ => panic!("Expected cached selectable text"),
        }
        worker.open(10, sample(), String::new());
        assert!(matches!(receive(&worker), Event::Opened { id: 10, .. }));
        worker.inspect(1);
        match receive(&worker) {
            Event::PageText {
                id: 10,
                page: 1,
                text,
            } => assert!(text.text.contains("NAVIGATION")),
            _ => panic!("Expected the new document's text"),
        }
        assert!(first.text.contains("native"));
    }

    #[test]
    fn failed_reload_preserves_the_active_pdf_and_a_later_reload_recovers() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("live.pdf");
        std::fs::copy(sample(), &path).unwrap();
        let worker = Worker::new(egui::Context::default());
        worker.open(12, path.clone(), String::new());
        assert!(matches!(receive(&worker), Event::Opened { id: 12, .. }));
        std::fs::write(&path, b"incomplete update").unwrap();
        // File notifications are independent of explicit reloads. Consume each
        // write's notification so a slow runner cannot leave it queued ahead
        // of the reload result. This also exercises the watcher-triggered path.
        assert!(matches!(receive(&worker), Event::FileChanged { id: 12 }));
        worker.reload(12, path.clone(), String::new());
        assert!(matches!(
            receive(&worker),
            Event::Error {
                id: 12,
                error: Failure::Open(_),
                ..
            }
        ));
        worker.copy(1);
        match receive(&worker) {
            Event::Text {
                id: 12,
                page: 1,
                text,
            } => assert!(text.contains("NAVIGATION")),
            _ => panic!("Expected the previous PDF to remain readable"),
        }
        // Replace bytes in place so binary PDF data and cross-reference offsets remain intact.
        let mut bytes = std::fs::read(sample()).unwrap();
        let mut replacements = 0;
        for offset in 0..bytes.len().saturating_sub(5) {
            if &bytes[offset..offset + 6] == b"little" {
                bytes[offset..offset + 6].copy_from_slice(b"gentle");
                replacements += 1;
            }
        }
        assert!(replacements > 0);
        std::fs::write(&path, bytes).unwrap();
        assert!(matches!(receive(&worker), Event::FileChanged { id: 12 }));
        worker.reload(12, path, String::new());
        assert!(matches!(receive(&worker), Event::Opened { id: 12, .. }));
        worker.copy(0);
        match receive(&worker) {
            Event::Text {
                id: 12,
                page: 0,
                text,
            } => assert!(text.contains("gentle")),
            _ => panic!("Expected the updated PDF"),
        }
    }
}
