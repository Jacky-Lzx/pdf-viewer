use std::{
    path::{Path, PathBuf},
    time::{Duration, Instant, SystemTime},
};

const INTERVAL: Duration = Duration::from_millis(500);

#[derive(PartialEq, Eq)]
struct Stamp {
    modified: Option<SystemTime>,
    bytes: u64,
    #[cfg(unix)]
    identity: [u64; 2],
    #[cfg(unix)]
    changed: [i64; 2],
}

impl Stamp {
    fn read(path: &Path) -> Option<Self> {
        let metadata = std::fs::metadata(path).ok()?;
        #[cfg(unix)]
        use std::os::unix::fs::MetadataExt;
        Some(Self {
            modified: metadata.modified().ok(),
            bytes: metadata.len(),
            #[cfg(unix)]
            identity: [metadata.dev(), metadata.ino()],
            #[cfg(unix)]
            changed: [metadata.ctime(), metadata.ctime_nsec()],
        })
    }
}

pub struct FileWatch {
    path: PathBuf,
    observed: Option<Stamp>,
    pending: Option<Option<Stamp>>,
    deadline: Instant,
}

impl FileWatch {
    pub fn new(path: PathBuf, now: Instant) -> Self {
        Self {
            observed: Stamp::read(&path),
            path,
            pending: None,
            deadline: now + INTERVAL,
        }
    }

    pub fn remaining(&self, now: Instant) -> Duration {
        self.deadline.saturating_duration_since(now)
    }

    /// Check only when due; publish a change after two identical observations.
    pub fn poll(&mut self, now: Instant) -> bool {
        if now < self.deadline {
            return false;
        }
        self.deadline = now + INTERVAL;
        let current = Stamp::read(&self.path);
        if current == self.observed {
            self.pending = None;
            return false;
        }
        if self.pending.as_ref() == Some(&current) {
            self.observed = current;
            self.pending = None;
            return true;
        }
        self.pending = Some(current);
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reports_stable_changes_once_and_waits_for_each_new_write_to_settle() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("book.pdf");
        std::fs::write(&path, b"first PDF").unwrap();
        let start = Instant::now();
        let mut watch = FileWatch::new(path.clone(), start);
        assert_eq!(watch.remaining(start), INTERVAL);
        std::fs::write(&path, b"second PDF").unwrap();
        assert!(!watch.poll(start));
        assert!(!watch.poll(start + INTERVAL));
        std::fs::write(&path, b"third PDF, still being written").unwrap();
        assert!(!watch.poll(start + INTERVAL * 2));
        assert!(watch.poll(start + INTERVAL * 3));
        assert!(!watch.poll(start + INTERVAL * 4));
        assert!(!watch.poll(start + INTERVAL * 5));
    }

    #[test]
    fn transient_missing_files_are_debounced_and_stable_removal_and_recreation_are_reported() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("book.pdf");
        let temporary = root.path().join("book.old");
        std::fs::write(&path, b"PDF").unwrap();
        let start = Instant::now();
        let mut watch = FileWatch::new(path.clone(), start);
        std::fs::rename(&path, &temporary).unwrap();
        assert!(!watch.poll(start + INTERVAL));
        std::fs::rename(&temporary, &path).unwrap();
        // Restoring the name can update ctime on Unix, so let any metadata
        // change settle before checking the actual removal transition.
        watch.poll(start + INTERVAL * 2);
        watch.poll(start + INTERVAL * 3);
        assert!(!watch.poll(start + INTERVAL * 4));
        std::fs::remove_file(&path).unwrap();
        assert!(!watch.poll(start + INTERVAL * 5));
        assert!(watch.poll(start + INTERVAL * 6));
        assert!(!watch.poll(start + INTERVAL * 7));
        std::fs::write(&path, b"PDF restored").unwrap();
        assert!(!watch.poll(start + INTERVAL * 8));
        assert!(watch.poll(start + INTERVAL * 9));
    }

    #[cfg(unix)]
    #[test]
    fn atomic_replacement_with_equal_size_and_mtime_preserves_native_paths() {
        use std::os::unix::ffi::OsStringExt;
        let root = tempfile::tempdir().unwrap();
        let name = if cfg!(target_os = "linux") {
            std::ffi::OsString::from_vec(b"book-\xff.pdf".to_vec())
        } else {
            "文档 notes.pdf".into()
        };
        let path = root.path().join(name);
        std::fs::write(&path, b"old PDF").unwrap();
        let modified = std::fs::metadata(&path).unwrap().modified().unwrap();
        let start = Instant::now();
        let mut watch = FileWatch::new(path.clone(), start);
        let replacement = root.path().join("replacement.pdf");
        std::fs::write(&replacement, b"new PDF").unwrap();
        std::fs::File::options()
            .write(true)
            .open(&replacement)
            .unwrap()
            .set_times(std::fs::FileTimes::new().set_modified(modified))
            .unwrap();
        std::fs::rename(replacement, &path).unwrap();
        assert!(!watch.poll(start + INTERVAL));
        assert!(watch.poll(start + INTERVAL * 2));
        assert!(!watch.poll(start + INTERVAL * 3));
    }
}
