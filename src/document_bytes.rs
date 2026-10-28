use std::{
    fs::OpenOptions,
    io::{self, Read},
    path::Path,
};

const BLOCK: u64 = 1024 * 1024;

pub fn read_file(path: &Path, cancelled: impl Fn() -> bool) -> io::Result<Option<Vec<u8>>> {
    if cancelled() {
        return Ok(None);
    }
    let mut options = OpenOptions::new();
    options.read(true);
    // A FIFO must not block in open while waiting for its first writer.
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(libc::O_NONBLOCK);
    }
    let mut file = options.open(path)?;
    let metadata = file.metadata()?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::FileTypeExt;
        if metadata.file_type().is_fifo() {
            return read(
                &mut PipeReader {
                    file,
                    cancelled: &cancelled,
                },
                0,
                &cancelled,
            );
        }
    }
    read(&mut file, metadata.len(), cancelled)
}

#[cfg(unix)]
struct PipeReader<'a, F> {
    file: std::fs::File,
    cancelled: &'a F,
}

#[cfg(unix)]
impl<F: Fn() -> bool> Read for PipeReader<'_, F> {
    fn read(&mut self, output: &mut [u8]) -> io::Result<usize> {
        use std::os::fd::AsRawFd;
        if output.is_empty() {
            return Ok(0);
        }
        #[cfg(not(target_os = "macos"))]
        let mut descriptor = libc::pollfd {
            fd: self.file.as_raw_fd(),
            events: libc::POLLIN,
            revents: 0,
        };
        loop {
            if (self.cancelled)() {
                // The outer block reader discards all bytes on cancellation.
                return Ok(0);
            }
            // macOS poll/kqueue only reports a FIFO's buffered byte count;
            // select also reports its socket's EOF when the last writer closes.
            #[cfg(target_os = "macos")]
            let ready = select_pipe(self.file.as_raw_fd())?;
            // SAFETY: descriptor is live and writable; the owned file remains
            // open for the entire poll/read pair. Only one descriptor is passed.
            #[cfg(not(target_os = "macos"))]
            let ready = unsafe { libc::poll(&mut descriptor, 1, 50) };
            if ready < 0 {
                let error = io::Error::last_os_error();
                if error.kind() == io::ErrorKind::Interrupted {
                    continue;
                }
                return Err(error);
            }
            if ready == 0 {
                continue;
            }
            // Read on hangup too: drain any final bytes before EOF. Waiting
            // first distinguishes no writer yet from a closed writer.
            match self.file.read(output) {
                Err(error) if error.kind() == io::ErrorKind::WouldBlock => continue,
                result => return result,
            }
        }
    }
}

#[cfg(all(unix, any(target_os = "macos", test)))]
fn select_pipe(fd: std::os::fd::RawFd) -> io::Result<libc::c_int> {
    if fd < 0 || fd as usize >= libc::FD_SETSIZE {
        return Err(io::Error::other("FIFO descriptor exceeds select capacity"));
    }
    // SAFETY: fd is within fd_set's bounds, the set and timeout are writable,
    // and select borrows the descriptor without taking ownership.
    let ready = unsafe {
        let mut readers = std::mem::zeroed::<libc::fd_set>();
        libc::FD_ZERO(&mut readers);
        libc::FD_SET(fd, &mut readers);
        let mut timeout = libc::timeval {
            tv_sec: 0,
            tv_usec: 50_000,
        };
        libc::select(
            fd + 1,
            &mut readers,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            &mut timeout,
        )
    };
    Ok(ready)
}

/// Keep filesystem reads bounded without copying through an intermediate buffer.
pub fn read(
    reader: &mut impl Read,
    expected: u64,
    cancelled: impl Fn() -> bool,
) -> io::Result<Option<Vec<u8>>> {
    if cancelled() {
        return Ok(None);
    }
    let length = usize::try_from(expected)
        .map_err(|_| io::Error::other("File is too large for this address space"))?;
    let mut bytes = Vec::new();
    bytes.try_reserve_exact(length).map_err(io::Error::other)?;
    loop {
        let count = reader.by_ref().take(BLOCK).read_to_end(&mut bytes)?;
        if cancelled() {
            return Ok(None);
        }
        if count < BLOCK as usize {
            return Ok(Some(bytes));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{cell::Cell, io::Cursor};

    #[cfg(unix)]
    mod pipes {
        use super::*;
        use std::{
            ffi::CString,
            io::Write,
            os::unix::{ffi::OsStrExt, fs::OpenOptionsExt},
            sync::{
                Arc,
                atomic::{AtomicBool, Ordering},
                mpsc,
            },
            thread,
            time::Duration,
        };

        fn fifo() -> (tempfile::TempDir, std::path::PathBuf) {
            let root = tempfile::tempdir().unwrap();
            let path = root.path().join("input.pdf");
            let name = CString::new(path.as_os_str().as_bytes()).unwrap();
            // SAFETY: name is a valid, NUL-terminated path for the duration of the call.
            assert_eq!(unsafe { libc::mkfifo(name.as_ptr(), 0o600) }, 0);
            (root, path)
        }

        struct PendingRead {
            cancelled: Arc<AtomicBool>,
            result: mpsc::Receiver<io::Result<Option<Vec<u8>>>>,
        }

        fn reader(path: std::path::PathBuf) -> PendingRead {
            let cancelled = Arc::new(AtomicBool::new(false));
            let pending = Arc::clone(&cancelled);
            let (started, ready) = mpsc::channel();
            let (finished, result) = mpsc::channel();
            thread::spawn(move || {
                let checks = Cell::new(0);
                let bytes = read_file(&path, || {
                    checks.set(checks.get() + 1);
                    // Second check runs after open and metadata, before reading.
                    if checks.get() == 2 {
                        started.send(()).unwrap();
                    }
                    pending.load(Ordering::Relaxed)
                });
                let _ = finished.send(bytes);
            });
            ready.recv_timeout(Duration::from_secs(2)).unwrap();
            PendingRead { cancelled, result }
        }

        #[test]
        fn cancel_before_a_writer_connects_or_while_it_stalls() {
            for connected in [false, true] {
                let (_root, path) = fifo();
                let PendingRead { cancelled, result } = reader(path.clone());
                let _writer = connected.then(|| {
                    let mut writer = OpenOptions::new()
                        .write(true)
                        .custom_flags(libc::O_NONBLOCK)
                        .open(path)
                        .unwrap();
                    writer.write_all(b"%").unwrap();
                    writer
                });
                assert!(matches!(
                    result.recv_timeout(Duration::from_millis(100)),
                    Err(mpsc::RecvTimeoutError::Timeout)
                ));
                cancelled.store(true, Ordering::Relaxed);
                assert!(
                    result
                        .recv_timeout(Duration::from_secs(2))
                        .unwrap()
                        .unwrap()
                        .is_none()
                );
            }
        }

        #[test]
        fn select_distinguishes_a_pending_writer_from_an_empty_closed_writer() {
            use std::os::fd::AsRawFd;
            let (_root, path) = fifo();
            let file = OpenOptions::new()
                .read(true)
                .custom_flags(libc::O_NONBLOCK)
                .open(&path)
                .unwrap();
            assert_eq!(select_pipe(file.as_raw_fd()).unwrap(), 0);
            let writer = OpenOptions::new().write(true).open(path).unwrap();
            assert_eq!(select_pipe(file.as_raw_fd()).unwrap(), 0);
            drop(writer);
            assert_eq!(select_pipe(file.as_raw_fd()).unwrap(), 1);
            assert!(select_pipe(libc::FD_SETSIZE as i32).is_err());
            assert!(select_pipe(-1).is_err());
        }

        #[test]
        fn delayed_writers_preserve_fragmented_bytes_and_real_eof() {
            for length in [0, 17, BLOCK as usize * 2 + 13] {
                let (_root, path) = fifo();
                let PendingRead { cancelled, result } = reader(path.clone());
                assert!(matches!(
                    result.recv_timeout(Duration::from_millis(30)),
                    Err(mpsc::RecvTimeoutError::Timeout)
                ));
                let data: Vec<_> = (0..length).map(|i| (i % 251) as u8).collect();
                let mut writer = OpenOptions::new().write(true).open(path).unwrap();
                if length > 0 {
                    writer.write_all(&data[..1]).unwrap();
                    thread::sleep(Duration::from_millis(20));
                    writer.write_all(&data[1..]).unwrap();
                }
                drop(writer);
                let bytes = result.recv_timeout(Duration::from_secs(2));
                cancelled.store(true, Ordering::Relaxed);
                assert_eq!(
                    bytes
                        .expect("FIFO must finish after its writer closes")
                        .unwrap()
                        .unwrap(),
                    data,
                    "FIFO length {length}"
                );
            }
        }
    }

    #[test]
    fn cancellation_before_read_and_between_blocks_leaves_the_rest_unread() {
        let source = vec![37; BLOCK as usize * 3 + 19];
        let mut reader = Cursor::new(&source);
        assert!(
            read(&mut reader, source.len() as u64, || true)
                .unwrap()
                .is_none()
        );
        assert_eq!(reader.position(), 0);
        let checks = Cell::new(0);
        assert!(
            read(&mut reader, source.len() as u64, || {
                checks.set(checks.get() + 1);
                checks.get() == 2
            })
            .unwrap()
            .is_none()
        );
        assert_eq!(reader.position(), BLOCK);
        let remaining = read(&mut reader, 0, || false).unwrap().unwrap();
        assert_eq!(remaining, source[BLOCK as usize..]);
    }

    #[test]
    fn complete_reads_match_the_source_for_empty_short_exact_and_multiple_blocks() {
        for length in [0, 13, BLOCK as usize, BLOCK as usize * 2 + 11] {
            let source: Vec<_> = (0..length).map(|index| (index % 251) as u8).collect();
            for expected in [0, length as u64 / 2, length as u64, length as u64 + 16] {
                assert_eq!(
                    read(&mut Cursor::new(&source), expected, || false)
                        .unwrap()
                        .unwrap(),
                    source
                );
            }
        }
    }

    #[test]
    fn interrupted_reads_retry_and_real_io_errors_propagate() {
        struct Interrupted {
            calls: usize,
        }
        impl Read for Interrupted {
            fn read(&mut self, output: &mut [u8]) -> io::Result<usize> {
                self.calls += 1;
                match self.calls {
                    1 => Err(io::ErrorKind::Interrupted.into()),
                    2 => {
                        output[..3].copy_from_slice(b"PDF");
                        Ok(3)
                    }
                    _ => Ok(0),
                }
            }
        }
        assert_eq!(
            read(&mut Interrupted { calls: 0 }, 0, || false)
                .unwrap()
                .unwrap(),
            b"PDF"
        );
        struct Failed;
        impl Read for Failed {
            fn read(&mut self, _: &mut [u8]) -> io::Result<usize> {
                Err(io::ErrorKind::PermissionDenied.into())
            }
        }
        assert_eq!(
            read(&mut Failed, 0, || false).unwrap_err().kind(),
            io::ErrorKind::PermissionDenied
        );
    }
}
