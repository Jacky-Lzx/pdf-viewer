use crate::state::Library;
use eframe::egui;
use std::{
    path::PathBuf,
    sync::{Arc, Condvar, Mutex},
    thread::{self, JoinHandle},
};

#[derive(Default)]
struct Pending {
    snapshot: Option<(u64, Library)>,
    completed: Option<(u64, Result<(), String>)>,
    closing: bool,
}

pub struct StateWriter {
    shared: Arc<(Mutex<Pending>, Condvar)>,
    revision: u64,
    thread: Option<JoinHandle<()>>,
}

impl StateWriter {
    pub fn new(path: PathBuf, context: egui::Context) -> Self {
        Self::spawn(context, move |library| {
            library.save(&path).map_err(|error| error.to_string())
        })
    }

    fn spawn(
        context: egui::Context,
        mut write: impl FnMut(&Library) -> Result<(), String> + Send + 'static,
    ) -> Self {
        let shared = Arc::new((Mutex::new(Pending::default()), Condvar::new()));
        let background = Arc::clone(&shared);
        let thread = thread::spawn(move || {
            loop {
                let (revision, library) = {
                    let (lock, wake) = &*background;
                    let mut pending = lock.lock().unwrap();
                    loop {
                        if let Some(snapshot) = pending.snapshot.take() {
                            break snapshot;
                        }
                        if pending.closing {
                            return;
                        }
                        pending = wake.wait(pending).unwrap();
                    }
                };
                let result = write(&library);
                background.0.lock().unwrap().completed = Some((revision, result));
                context.request_repaint();
            }
        });
        Self {
            shared,
            revision: 0,
            thread: Some(thread),
        }
    }

    pub fn save(&mut self, library: Library) {
        self.revision = self.revision.wrapping_add(1);
        self.shared.0.lock().unwrap().snapshot = Some((self.revision, library));
        self.shared.1.notify_one();
    }

    pub fn result(&mut self) -> Option<Result<(), String>> {
        let (revision, result) = self.shared.0.lock().unwrap().completed.take()?;
        // An older failed snapshot must not trigger a retry of a newer save.
        (revision == self.revision).then_some(result)
    }
}

impl Drop for StateWriter {
    fn drop(&mut self) {
        self.shared.0.lock().unwrap().closing = true;
        self.shared.1.notify_one();
        if let Some(thread) = self.thread.take() {
            // The loop drains the latest snapshot before acknowledging shutdown.
            let _ = thread.join();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::ReadingState;
    use std::{
        sync::mpsc,
        time::{Duration, Instant},
    };

    fn snapshot(page: usize) -> Library {
        let mut library = Library::default();
        library.remember(ReadingState {
            path: "reading.pdf".into(),
            page,
            ..Default::default()
        });
        library
    }

    fn result(writer: &mut StateWriter) -> Result<(), String> {
        let deadline = Instant::now() + Duration::from_secs(3);
        loop {
            if let Some(result) = writer.result() {
                return result;
            }
            assert!(Instant::now() < deadline, "Background save did not finish");
            thread::sleep(Duration::from_millis(1));
        }
    }

    #[test]
    fn slow_writes_coalesce_snapshots_ignore_stale_errors_and_flush_latest_on_drop() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("library.json");
        let saved_path = path.clone();
        let calls = Arc::new(Mutex::new(Vec::new()));
        let written = Arc::clone(&calls);
        let (entered_tx, entered_rx) = mpsc::channel();
        let (release_tx, release_rx) = mpsc::channel();
        let mut writer = StateWriter::spawn(egui::Context::default(), move |library| {
            let page = library.recent[0].page;
            written.lock().unwrap().push(page);
            entered_tx.send(page).unwrap();
            release_rx
                .recv_timeout(Duration::from_secs(3))
                .map_err(|error| error.to_string())?;
            if page == 1 {
                return Err("older write failed".into());
            }
            library.save(&saved_path).map_err(|error| error.to_string())
        });
        writer.save(snapshot(1));
        assert_eq!(entered_rx.recv_timeout(Duration::from_secs(3)).unwrap(), 1);
        writer.save(snapshot(2));
        writer.save(snapshot(3));
        assert_eq!(
            writer
                .shared
                .0
                .lock()
                .unwrap()
                .snapshot
                .as_ref()
                .unwrap()
                .1
                .recent[0]
                .page,
            3
        );
        release_tx.send(()).unwrap();
        assert_eq!(entered_rx.recv_timeout(Duration::from_secs(3)).unwrap(), 3);
        assert!(
            writer.result().is_none(),
            "ignore the older failed snapshot"
        );
        release_tx.send(()).unwrap();
        drop(writer);
        assert_eq!(*calls.lock().unwrap(), vec![1, 3]);
        assert_eq!(Library::load(&path).recent[0].page, 3);
    }

    #[test]
    fn reports_real_filesystem_errors_and_recovers_after_the_path_is_fixed() {
        let directory = tempfile::tempdir().unwrap();
        let parent = directory.path().join("settings");
        std::fs::write(&parent, "a file blocks the config directory").unwrap();
        let path = parent.join("library.json");
        let mut writer = StateWriter::new(path.clone(), egui::Context::default());
        writer.save(snapshot(1));
        assert!(result(&mut writer).is_err());
        std::fs::remove_file(&parent).unwrap();
        writer.save(snapshot(2));
        result(&mut writer).unwrap();
        assert_eq!(Library::load(&path).recent[0].page, 2);
    }

    #[test]
    fn shutdown_saves_the_latest_snapshot_without_polling_for_a_result() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("library.json");
        let mut writer = StateWriter::new(path.clone(), egui::Context::default());
        writer.save(snapshot(42));
        drop(writer);
        assert_eq!(Library::load(&path).recent[0].page, 42);
    }

    #[test]
    fn in_flight_shared_records_stay_immutable_while_the_next_snapshot_is_prepared() {
        let (entered_tx, entered_rx) = mpsc::channel();
        let (release_tx, release_rx) = mpsc::channel();
        let written = Arc::new(Mutex::new(Vec::new()));
        let completed = Arc::clone(&written);
        let mut writer = StateWriter::spawn(egui::Context::default(), move |library| {
            entered_tx.send(()).unwrap();
            release_rx.recv_timeout(Duration::from_secs(3)).unwrap();
            completed
                .lock()
                .unwrap()
                .push(serde_json::to_value(library).unwrap());
            Ok(())
        });
        let mut source = snapshot(7);
        let mut current = source.recent[0].as_ref().clone();
        current.bookmarks.extend(0..600);
        source.remember(current.clone());
        let first = serde_json::to_value(&source).unwrap();
        writer.save(source.clone());
        entered_rx.recv_timeout(Duration::from_secs(3)).unwrap();
        current.page = 8;
        current.bookmarks.remove(&7);
        source.remember(current);
        let latest = serde_json::to_value(&source).unwrap();
        writer.save(source);
        release_tx.send(()).unwrap();
        entered_rx.recv_timeout(Duration::from_secs(3)).unwrap();
        release_tx.send(()).unwrap();
        drop(writer);
        assert_eq!(*written.lock().unwrap(), [first, latest]);
    }
}
