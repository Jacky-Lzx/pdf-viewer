use crate::{
    document::{self, Bitmap, RenderKey},
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
    Opened {
        id: u64,
        dimensions: Vec<[f32; 2]>,
        outline: Vec<crate::outline::OutlineEntry>,
        labels: crate::page_labels::PageLabels,
    },
    Rendered {
        id: u64,
        key: RenderKey,
        bitmap: Bitmap,
        milliseconds: f64,
    },
    Thumbnail {
        id: u64,
        key: RenderKey,
        bitmap: Option<Bitmap>,
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
    query: SearchQuery,
}

struct SearchProgress {
    request: SearchRequest,
    page: usize,
    hits: usize,
    cursor: text::SearchCursor,
    text: Option<Arc<PageText>>,
}

impl SearchProgress {
    fn new(request: SearchRequest) -> Self {
        Self {
            request,
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
    render: VecDeque<RenderKey>,
    render_revision: u64,
    thumbnails: VecDeque<RenderKey>,
    thumbnail_revision: u64,
    search: Option<SearchRequest>,
    copy: Option<usize>,
    links: Option<usize>,
    inspect: Option<usize>,
    stop: bool,
}

impl Pending {
    fn replace_renders(&mut self, keys: Vec<RenderKey>) {
        self.render_revision = self.render_revision.wrapping_add(1);
        self.render = keys.into();
    }

    fn finish_render(&mut self, key: RenderKey, revision: u64) -> bool {
        if self.stop || self.open.is_some() {
            return false;
        }
        if revision != self.render_revision && !self.render.contains(&key) {
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

    fn finish_thumbnail(&mut self, key: RenderKey, revision: u64) -> bool {
        if self.stop
            || self.open.is_some()
            || (revision != self.thumbnail_revision && !self.thumbnails.contains(&key))
        {
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
        let shared = Arc::new((Mutex::new(Pending::default()), Condvar::new()));
        let (tx, events) = mpsc::sync_channel(4);
        let state = shared.clone();
        std::thread::Builder::new()
            .name("folio-render".into())
            .spawn(move || {
                let mut active: Option<(Pdf, u64)> = None;
                loop {
                    let request = {
                        let (lock, wake) = &*state;
                        let mut pending = lock.lock().unwrap();
                        while pending.open.is_none() && !pending.stop {
                            pending = wake.wait(pending).unwrap();
                        }
                        if pending.stop {
                            return;
                        }
                        pending.open.take().unwrap()
                    };
                    let id = request.id;
                    if !request.keep_on_error {
                        active = None;
                    }
                    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                        let pdf = document::load(&request.path, &request.password)?;
                        let dimensions: Vec<_> = pdf
                            .pages()
                            .iter()
                            .map(|page| {
                                let (w, h) = page.render_dimensions();
                                [w, h]
                            })
                            .collect();
                        if dimensions.iter().any(|size| {
                            size.iter()
                                .any(|dimension| !dimension.is_finite() || *dimension <= 0.0)
                        }) {
                            return Err("This PDF contains invalid page dimensions.".into());
                        }
                        let outline = crate::outline::read(&pdf);
                        let labels = crate::page_labels::PageLabels::read(&pdf);
                        Ok::<_, String>((pdf, dimensions, outline, labels))
                    }))
                    .unwrap_or_else(|_| {
                        Err("The PDF engine could not process this document.".into())
                    });
                    match result {
                        Ok((pdf, dimensions, outline, labels)) => {
                            active = Some((pdf, id));
                            emit(
                                &tx,
                                &context,
                                Event::Opened {
                                    id,
                                    dimensions,
                                    outline,
                                    labels,
                                },
                            );
                        }
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
                    if let Some((pdf, active_id)) = &active
                        && std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                            session(pdf, *active_id, &state, &tx, &context);
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
            pending.render.clear();
            pending.thumbnails.clear();
            pending.search = None;
            pending.copy = None;
            pending.links = None;
            pending.inspect = None;
        });
    }

    pub fn render(&self, keys: Vec<RenderKey>) {
        self.update(|pending| pending.replace_renders(keys));
    }
    pub fn thumbnails(&self, keys: Vec<RenderKey>) {
        self.update(|pending| pending.replace_thumbnails(keys));
    }
    pub fn search(&self, revision: u64, query: String, options: SearchOptions) {
        let query = SearchQuery::new(query, options);
        self.update(|pending| pending.search = Some(SearchRequest { revision, query }));
    }
    pub fn copy(&self, page: usize) {
        self.update(|pending| pending.copy = Some(page));
    }
    pub fn links(&self, page: usize) {
        self.update(|pending| pending.links = Some(page));
    }
    pub fn inspect(&self, page: usize) {
        self.update(|pending| pending.inspect = Some(page));
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
    Inspect(usize),
}

fn session(pdf: &Pdf, id: u64, shared: &Shared, tx: &SyncSender<Event>, context: &egui::Context) {
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
                if pending.stop || pending.open.is_some() {
                    return;
                }
                if let Some(request) = pending.search.take() {
                    search = if request.query.is_empty() {
                        None
                    } else {
                        Some(SearchProgress::new(request))
                    };
                }
                if let Some(page) = pending.inspect.take() {
                    break Job::Inspect(page);
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
                pending = wake.wait(pending).unwrap();
            }
        };
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
                    document::rasterize(pdf, &render_cache, key)
                }));
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
                if thumbnail {
                    emit(
                        tx,
                        context,
                        Event::Thumbnail {
                            id,
                            key,
                            bitmap: result.ok().and_then(Result::ok),
                        },
                    );
                    continue;
                }
                match result {
                    Ok(Ok(bitmap)) => emit(
                        tx,
                        context,
                        Event::Rendered {
                            id,
                            key,
                            bitmap,
                            milliseconds: start.elapsed().as_secs_f64() * 1000.0,
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
            Job::Search(page) | Job::Copy(page) | Job::Inspect(page) => {
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
                if matches!(job, Job::Inspect(_)) {
                    text.prepare_selection();
                    texts.refresh(page);
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
                        &progress.request.query,
                        &mut progress.cursor,
                        limit,
                    );
                    texts.refresh(page);
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
                query: SearchQuery::new("Rust".into(), SearchOptions::default()),
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
                bitmap: Some(_),
                ..
            }
        ));
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
                Event::Rendered {
                    id, key, bitmap, ..
                } => {
                    assert_eq!(id, 7);
                    assert_eq!(key.page, 2);
                    assert_eq!(bitmap.size, [400, 300]);
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
