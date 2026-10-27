use crate::{
    document::{self, RenderKey},
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

pub enum Event {
    FileChanged {
        id: u64,
    },
    Opened {
        id: u64,
        dimensions: Vec<[f32; 2]>,
        info: crate::document_info::DocumentInfo,
        outline: Vec<crate::outline::OutlineEntry>,
        labels: crate::page_labels::PageLabels,
    },
    Rendered {
        id: u64,
        key: RenderKey,
        image: egui::ColorImage,
        milliseconds: f64,
    },
    Thumbnail {
        id: u64,
        key: RenderKey,
        image: Option<egui::ColorImage>,
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
        message: String,
        opening: bool,
    },
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
        }
    }
}

#[derive(Default)]
struct Pending {
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
    stop: bool,
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

pub struct Worker {
    shared: Shared,
    pub events: Receiver<Event>,
}

impl Worker {
    pub fn new(context: egui::Context) -> Self {
        Self::with_loader(context, |path, password, cancelled| {
            document::load_cancellable(path, password, cancelled)
        })
    }

    fn with_loader(
        context: egui::Context,
        mut load: impl FnMut(&std::path::Path, &str, &dyn Fn() -> bool) -> Result<Option<Pdf>, String>
        + Send
        + 'static,
    ) -> Self {
        let shared = Arc::new((Mutex::new(Pending::default()), Condvar::new()));
        let (tx, events) = mpsc::sync_channel(4);
        let state = shared.clone();
        std::thread::Builder::new()
            .name("folio-render".into())
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
                        if cancelled() {
                            return Ok(None);
                        }
                        let labels = crate::page_labels::PageLabels::read(&pdf);
                        if cancelled() {
                            return Ok(None);
                        }
                        let info = crate::document_info::DocumentInfo::read(&pdf);
                        Ok::<_, String>(Some((pdf, dimensions, outline, labels, info, watch)))
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
                            active = Some((pdf, id, watch));
                            emit(
                                &tx,
                                &context,
                                Event::Opened {
                                    id,
                                    dimensions,
                                    info,
                                    outline,
                                    labels,
                                },
                            );
                        }
                        Ok(None) => continue,
                        Err(message) => emit(
                            &tx,
                            &context,
                            Event::Error {
                                id,
                                message,
                                opening: true,
                            },
                        ),
                    }
                    if let Some((pdf, active_id, watch)) = &mut active
                        && std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                            session(pdf, *active_id, watch, &state, &tx, &context);
                        }))
                        .is_err()
                    {
                        emit(
                            &tx,
                            &context,
                            Event::Error {
                                id: *active_id,
                                message: "The PDF engine could not process this document.".into(),
                                opening: false,
                            },
                        );
                    }
                }
            })
            .expect("Cannot start PDF worker");
        Self { shared, events }
    }

    fn update(&self, change: impl FnOnce(&mut Pending)) {
        let (lock, wake) = &*self.shared;
        change(&mut lock.lock().unwrap());
        wake.notify_one();
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
) {
    let render_cache = RenderCache::new();
    let interpreter_cache = InterpreterCache::new();
    let destinations = crate::outline::DestinationIndex::new(pdf);
    let mut texts = crate::text_cache::TextCache::default();
    let mut search: Option<SearchProgress> = None;
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
                if let Some(progress) = &search {
                    break Job::Search(progress.page);
                }
                if let Some(key) = pending.thumbnails.pop_front() {
                    break Job::Thumbnail(key, pending.thumbnail_revision);
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
            Job::Links(page) => {
                if let Some(data) = pdf.pages().get(page) {
                    let links = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                        crate::links::read(data, &destinations)
                    }))
                    .unwrap_or_default();
                    emit(tx, context, Event::Links { id, page, links });
                }
            }
            Job::Render(key, revision) | Job::Thumbnail(key, revision) => {
                let thumbnail = matches!(job, Job::Thumbnail(..));
                let start = Instant::now();
                let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    document::rasterize_pixels(pdf, &render_cache, key.page, key.width)
                }));
                let relevant = {
                    let pending = shared.0.lock().unwrap();
                    if thumbnail {
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
                    result
                        .map(|image| crate::texture_pixels::prepare(image, key.rotation, key.night))
                });
                // Requests can change during pixel preparation too. Consume any
                // duplicate still-needed job only when its final image is ready.
                let relevant = {
                    let mut pending = shared.0.lock().unwrap();
                    if thumbnail {
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
                                message,
                                opening: false,
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
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("assets/folio-guide.pdf")
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
    fn page_rendering_and_search_finish_before_queued_thumbnails() {
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
                _ => panic!("Thumbnails must follow document search"),
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
                        } if !thumbnail && actual == key => {
                            assert!(milliseconds.is_finite() && milliseconds >= 0.0);
                            image
                        }
                        Event::Thumbnail {
                            id: 1,
                            key: actual,
                            image: Some(image),
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
            Event::Error { opening: false, .. }
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
        for fail_first in [false, true] {
            for close_first in [false, true] {
                let (entered_tx, entered_rx) = mpsc::channel();
                let (release_tx, release_rx) = mpsc::channel();
                let mut first = true;
                let worker =
                    Worker::with_loader(egui::Context::default(), move |path, password, _| {
                        let result = document::load(path, password).map(Some);
                        if std::mem::take(&mut first) {
                            entered_tx.send(()).unwrap();
                            release_rx.recv_timeout(Duration::from_secs(5)).unwrap();
                        }
                        result
                    });
                worker.open(
                    1,
                    if fail_first {
                        sample().with_extension("missing")
                    } else {
                        sample()
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
            let result = document::load(
                match calls {
                    1 => original.path(),
                    2 => replacement.path(),
                    _ => path,
                },
                password,
            )
            .map(Some);
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
                opening: true,
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
                opening: true,
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
                opening: true,
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
                outline,
                ..
            } => {
                assert_eq!(id, 2);
                assert_eq!(dimensions.len(), 6);
                assert_eq!(outline.len(), 6);
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
                opening: false,
                ..
            }
        ));
    }

    #[test]
    fn all_text_inspection_defers_interactive_indexes_and_reuses_the_cached_page() {
        let worker = Worker::new(egui::Context::default());
        worker.open(1, sample(), String::new());
        assert!(matches!(receive(&worker), Event::Opened { .. }));
        worker.inspect_all(0);
        let all = match receive(&worker) {
            Event::PageText { page: 0, text, .. } => text,
            _ => panic!("Expected whole-page text"),
        };
        assert!(
            all.spans.len() >= 256,
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
        worker.reload(12, path.clone(), String::new());
        assert!(matches!(
            receive(&worker),
            Event::Error {
                id: 12,
                opening: true,
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
