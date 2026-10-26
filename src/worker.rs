use crate::{
    document::{self, Bitmap, RenderKey},
    text::{self, PageText, SearchHit},
};
use eframe::egui;
use hayro::{RenderCache, hayro_interpret::InterpreterCache, hayro_syntax::Pdf};
use std::{
    collections::{HashMap, VecDeque},
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
    },
}

struct OpenRequest {
    id: u64,
    path: PathBuf,
    password: String,
}
struct SearchRequest {
    revision: u64,
    query: String,
}

#[derive(Default)]
struct Pending {
    open: Option<OpenRequest>,
    render: VecDeque<RenderKey>,
    render_revision: u64,
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
                        session(&pdf, id, &state, &tx, &context);
                        Ok::<_, String>(())
                    }))
                    .unwrap_or_else(|_| {
                        Err("The PDF engine could not process this document.".into())
                    });
                    if let Err(message) = result {
                        emit(&tx, &context, Event::Error { id, message });
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
        self.update(|pending| {
            pending.open = Some(OpenRequest { id, path, password });
            pending.render.clear();
            pending.search = None;
            pending.copy = None;
            pending.links = None;
            pending.inspect = None;
        });
    }

    pub fn render(&self, keys: Vec<RenderKey>) {
        self.update(|pending| pending.replace_renders(keys));
    }
    pub fn search(&self, revision: u64, query: String) {
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
    Search(usize),
    Copy(usize),
    Links(usize),
    Inspect(usize),
}

fn session(pdf: &Pdf, id: u64, shared: &Shared, tx: &SyncSender<Event>, context: &egui::Context) {
    let render_cache = RenderCache::new();
    let interpreter_cache = InterpreterCache::new();
    let destinations = crate::outline::DestinationIndex::new(pdf);
    let mut texts: HashMap<usize, Arc<PageText>> = HashMap::new();
    let mut text_bytes = 0;
    let mut search: Option<(SearchRequest, usize, usize)> = None;
    loop {
        let job = {
            let (lock, wake) = &**shared;
            let mut pending = lock.lock().unwrap();
            loop {
                if pending.stop || pending.open.is_some() {
                    return;
                }
                if let Some(request) = pending.search.take() {
                    search = if request.query.trim().is_empty() {
                        None
                    } else {
                        Some((request, 0, 0))
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
                if let Some((_, page, _)) = &search {
                    break Job::Search(*page);
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
            Job::Render(key, revision) => {
                let start = Instant::now();
                let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    document::rasterize(pdf, &render_cache, key)
                }));
                if !shared.0.lock().unwrap().finish_render(key, revision) {
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
                        emit(tx, context, Event::Error { id, message });
                    }
                }
            }
            Job::Search(page) | Job::Copy(page) | Job::Inspect(page) => {
                if page >= pdf.pages().len() {
                    search = None;
                    continue;
                }
                let mut uncached = None;
                if !texts.contains_key(&page) {
                    let text = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                        text::extract(&pdf.pages()[page], &interpreter_cache)
                    }))
                    .unwrap_or_default();
                    let bytes = text.memory_bytes();
                    let text = Arc::new(text);
                    if bytes > 32 * 1024 * 1024 {
                        uncached = Some(text);
                    } else {
                        if text_bytes + bytes > 32 * 1024 * 1024 {
                            texts.clear();
                            text_bytes = 0;
                        }
                        text_bytes += bytes;
                        texts.insert(page, text);
                    }
                }
                let text = uncached.as_ref().unwrap_or_else(|| &texts[&page]);
                if matches!(job, Job::Inspect(_)) {
                    emit(
                        tx,
                        context,
                        Event::PageText {
                            id,
                            page,
                            text: Arc::clone(text),
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
                } else if let Some((request, next, hit_count)) = &mut search {
                    let mut hits = text.search(page, &request.query);
                    hits.truncate(10_000_usize.saturating_sub(*hit_count));
                    *hit_count += hits.len();
                    *next += 1;
                    emit(
                        tx,
                        context,
                        Event::Search {
                            id,
                            revision: request.revision,
                            hits,
                            scanned: *next,
                            total: pdf.pages().len(),
                            complete: *next >= pdf.pages().len() || *hit_count >= 10_000,
                            limited: *hit_count >= 10_000,
                        },
                    );
                    if *next >= pdf.pages().len() || *hit_count >= 10_000 {
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
    fn document_switches_and_shutdown_discard_inflight_results() {
        let mut pending = Pending::default();
        pending.replace_renders(vec![render_key(0)]);
        let revision = pending.render_revision;
        pending.open = Some(OpenRequest {
            id: 2,
            path: sample(),
            password: String::new(),
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
        worker.search(12, "rust".into());
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
}
