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
        mpsc::{self, Receiver, Sender},
    },
    time::Instant,
};

pub enum Event {
    Opened {
        id: u64,
        dimensions: Vec<[f32; 2]>,
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
    },
    Text {
        id: u64,
        page: usize,
        text: String,
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
    search: Option<SearchRequest>,
    copy: Option<usize>,
    stop: bool,
}

type Shared = Arc<(Mutex<Pending>, Condvar)>;

pub struct Worker {
    shared: Shared,
    pub events: Receiver<Event>,
}

impl Worker {
    pub fn new(context: egui::Context) -> Self {
        let shared = Arc::new((Mutex::new(Pending::default()), Condvar::new()));
        let (tx, events) = mpsc::channel();
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
                        let dimensions = pdf
                            .pages()
                            .iter()
                            .map(|page| {
                                let (w, h) = page.render_dimensions();
                                [w, h]
                            })
                            .collect();
                        emit(&tx, &context, Event::Opened { id, dimensions });
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
        });
    }

    pub fn render(&self, keys: Vec<RenderKey>) {
        self.update(|pending| pending.render = keys.into());
    }
    pub fn search(&self, revision: u64, query: String) {
        self.update(|pending| pending.search = Some(SearchRequest { revision, query }));
    }
    pub fn copy(&self, page: usize) {
        self.update(|pending| pending.copy = Some(page));
    }
}

impl Drop for Worker {
    fn drop(&mut self) {
        self.update(|pending| pending.stop = true);
    }
}

fn emit(tx: &Sender<Event>, context: &egui::Context, event: Event) {
    if tx.send(event).is_ok() {
        context.request_repaint();
    }
}

enum Job {
    Render(RenderKey),
    Search(usize),
    Copy(usize),
}

fn session(pdf: &Pdf, id: u64, shared: &Shared, tx: &Sender<Event>, context: &egui::Context) {
    let render_cache = RenderCache::new();
    let interpreter_cache = InterpreterCache::new();
    let mut texts: HashMap<usize, PageText> = HashMap::new();
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
                if let Some(key) = pending.render.pop_front() {
                    break Job::Render(key);
                }
                if let Some(page) = pending.copy.take() {
                    break Job::Copy(page);
                }
                if let Some((_, page, _)) = &search {
                    break Job::Search(*page);
                }
                pending = wake.wait(pending).unwrap();
            }
        };
        match job {
            Job::Render(key) => {
                let start = Instant::now();
                match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    document::rasterize(pdf, &render_cache, key)
                })) {
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
            Job::Search(page) | Job::Copy(page) => {
                if page >= pdf.pages().len() {
                    search = None;
                    continue;
                }
                if !texts.contains_key(&page) {
                    let text = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                        text::extract(&pdf.pages()[page], &interpreter_cache)
                    }))
                    .unwrap_or_default();
                    let bytes =
                        text.text.len() + text.spans.len() * std::mem::size_of::<text::TextSpan>();
                    if text_bytes + bytes > 32 * 1024 * 1024 {
                        texts.clear();
                        text_bytes = 0;
                    }
                    text_bytes += bytes;
                    texts.insert(page, text);
                }
                if matches!(job, Job::Copy(_)) {
                    emit(
                        tx,
                        context,
                        Event::Text {
                            id,
                            page,
                            text: texts[&page].text.clone(),
                        },
                    );
                } else if let Some((request, next, hit_count)) = &mut search {
                    let mut hits = texts[&page].search(page, &request.query);
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
