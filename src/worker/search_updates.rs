use std::time::{Duration, Instant};

const PROGRESS_INTERVAL: Duration = Duration::from_millis(50);

/// Empty page batches only advance the progress counter. Avoid filling the
/// bounded UI queue with them, while delivering every hit and final count.
pub(super) struct Updates {
    due: Instant,
}

impl Updates {
    pub(super) fn new(now: Instant) -> Self {
        Self { due: now }
    }

    pub(super) fn publish(&mut self, has_hits: bool, complete: bool, now: Instant) -> bool {
        if has_hits || complete || now >= self.due {
            self.due = now + PROGRESS_INTERVAL;
            true
        } else {
            false
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::{Event, SearchOptions, Worker};
    use super::Updates;
    use eframe::egui;
    use std::time::{Duration, Instant};

    #[test]
    fn empty_progress_is_bounded_but_hits_and_completion_are_immediate() {
        let now = Instant::now();
        let mut updates = Updates::new(now);
        assert!(updates.publish(false, false, now));
        for millis in 1..50 {
            assert!(!updates.publish(false, false, now + Duration::from_millis(millis)));
        }
        assert!(updates.publish(false, false, now + Duration::from_millis(50)));
        // A hit must never wait for an empty-progress deadline, including dense
        // batches within one page. Its event also carries the current progress.
        for millis in 51..60 {
            assert!(updates.publish(true, false, now + Duration::from_millis(millis)));
        }
        assert!(!updates.publish(false, false, now + Duration::from_millis(60)));
        assert!(updates.publish(false, true, now + Duration::from_millis(61)));
    }

    #[test]
    fn replacement_queries_have_their_own_initial_progress_and_long_pauses_do_not_replay_ticks() {
        let now = Instant::now();
        let mut updates = Updates::new(now);
        assert!(updates.publish(false, false, now));
        let later = now + Duration::from_secs(30);
        assert!(updates.publish(false, false, later));
        assert!(!updates.publish(false, false, later));
        let mut replacement = Updates::new(later);
        assert!(replacement.publish(false, false, later));
        assert!(replacement.publish(true, true, later));
    }

    fn empty_pages(count: usize) -> tempfile::NamedTempFile {
        use lopdf::{Document, Object, Stream, dictionary};
        let mut pdf = Document::with_version("1.4");
        let pages = pdf.new_object_id();
        let contents = pdf.add_object(Stream::new(dictionary! {}, Vec::new()));
        let kids: Vec<Object> = (0..count)
            .map(|_| {
                pdf.add_object(dictionary! {
                    "Type" => "Page",
                    "Parent" => pages,
                    "MediaBox" => vec![0.into(), 0.into(), 600.into(), 800.into()],
                    "Resources" => dictionary! {},
                    "Contents" => contents,
                })
                .into()
            })
            .collect();
        pdf.objects.insert(
            pages,
            dictionary! { "Type" => "Pages", "Kids" => kids, "Count" => count as i64 }.into(),
        );
        let root = pdf.add_object(dictionary! { "Type" => "Catalog", "Pages" => pages });
        pdf.trailer.set("Root", root);
        let file = tempfile::NamedTempFile::new().unwrap();
        pdf.save(file.path()).unwrap();
        file
    }

    #[test]
    fn empty_search_reports_exact_monotonic_progress_and_final_counts_after_replacement() {
        let file = empty_pages(128);
        let worker = Worker::new(egui::Context::default());
        worker.open(7, file.path().into(), String::new());
        assert!(matches!(
            worker.events.recv_timeout(Duration::from_secs(5)).unwrap(),
            Event::Opened { .. }
        ));
        for revision in [1, 2] {
            worker.search(revision, "absent".into(), SearchOptions::default());
            let mut scanned = 0;
            loop {
                let Event::Search {
                    id,
                    revision: actual,
                    hits,
                    scanned: current,
                    total,
                    complete,
                    limited,
                } = worker.events.recv_timeout(Duration::from_secs(5)).unwrap()
                else {
                    panic!("Expected a search update")
                };
                assert_eq!((id, actual, total), (7, revision, 128));
                assert!(hits.is_empty() && !limited);
                assert!(current > scanned && current <= total);
                scanned = current;
                if complete {
                    assert_eq!(scanned, total);
                    break;
                }
            }
        }
        worker.copy(0);
        assert!(
            matches!(worker.events.recv_timeout(Duration::from_secs(5)).unwrap(), Event::Text { page: 0, text, .. } if text.is_empty())
        );
    }

    #[test]
    #[ignore = "manual worker throughput measurement with a 16ms event consumer"]
    fn measure_empty_search_with_frame_paced_consumer() {
        const PAGES: usize = 4000;
        let file = empty_pages(PAGES);
        let worker = Worker::new(egui::Context::default());
        worker.open(1, file.path().into(), String::new());
        assert!(matches!(
            worker.events.recv_timeout(Duration::from_secs(10)).unwrap(),
            Event::Opened { dimensions, .. } if dimensions.len() == PAGES
        ));
        let started = Instant::now();
        worker.search(1, "absent".into(), SearchOptions::default());
        let (mut updates, mut scanned) = (0, 0);
        loop {
            assert!(
                started.elapsed() < Duration::from_secs(60),
                "search did not finish"
            );
            // The real bounded four-event queue is drained once per frame.
            // PDF extraction/cache work is included; loading and GUI painting
            // are excluded. Do not treat this as overall reader frame timing.
            std::thread::sleep(Duration::from_millis(16));
            while let Ok(event) = worker.events.try_recv() {
                let Event::Search {
                    hits,
                    scanned: current,
                    total,
                    complete,
                    limited,
                    ..
                } = event
                else {
                    panic!("Unexpected worker event")
                };
                assert!(hits.is_empty() && !limited);
                assert_eq!(total, PAGES);
                assert!(current >= scanned);
                scanned = current;
                updates += 1;
                if complete {
                    assert_eq!(scanned, PAGES);
                    println!(
                        "{PAGES} empty pages, {updates} search events, {:.1} ms elapsed, 16ms consumer; real PDF/worker, loading and painting excluded",
                        started.elapsed().as_secs_f64() * 1000.0
                    );
                    return;
                }
            }
        }
    }
}
