//! Per-document IPC, shared by Linux and macOS; no network listener.
use crate::synctex::{Options, Target};
use serde::{Deserialize, Serialize};
use std::{
    collections::hash_map::DefaultHasher,
    fs,
    hash::{Hash, Hasher},
    io::{self, Read, Write},
    os::unix::{
        fs::{DirBuilderExt, MetadataExt, PermissionsExt},
        net::{UnixListener, UnixStream},
    },
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc::{self, Receiver},
    },
    thread::JoinHandle,
    time::Duration,
};

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Request {
    pub target: Option<Target>,
    pub options: Options,
}

fn socket_path(pdf: &Path) -> io::Result<PathBuf> {
    // A short, stable path also fits macOS's 104-byte sockaddr_un.sun_path.
    let uid = unsafe { libc::geteuid() };
    let directory = PathBuf::from(format!("/tmp/folio-synctex-{uid}"));
    match fs::DirBuilder::new().mode(0o700).create(&directory) {
        Ok(()) => (),
        Err(error) if error.kind() == io::ErrorKind::AlreadyExists => (),
        Err(error) => return Err(error),
    }
    let meta = fs::symlink_metadata(&directory)?;
    if !meta.is_dir() || meta.uid() != uid || meta.permissions().mode() & 0o077 != 0 {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "Unsafe SyncTeX socket directory",
        ));
    }
    let mut hash = DefaultHasher::new();
    fs::canonicalize(pdf)?.hash(&mut hash);
    Ok(directory.join(format!("{:016x}.sock", hash.finish())))
}

pub fn send_existing(pdf: &Path, request: &Request) -> Result<bool, String> {
    let path = socket_path(pdf).map_err(|e| format!("Cannot locate SyncTeX window: {e}"))?;
    let mut stream = match UnixStream::connect(&path) {
        Ok(stream) => stream,
        Err(e)
            if matches!(
                e.kind(),
                io::ErrorKind::NotFound | io::ErrorKind::ConnectionRefused
            ) =>
        {
            return Ok(false);
        }
        Err(e) => return Err(format!("Cannot contact SyncTeX window: {e}")),
    };
    let mut transfer = || -> io::Result<()> {
        stream.set_read_timeout(Some(Duration::from_secs(2)))?;
        stream.set_write_timeout(Some(Duration::from_secs(2)))?;
        stream.write_all(&serde_json::to_vec(request)?)?;
        stream.shutdown(std::net::Shutdown::Write)?;
        let mut ack = [0];
        stream.read_exact(&mut ack)?;
        if ack != [1] {
            return Err(io::Error::other("SyncTeX request was rejected"));
        }
        Ok(())
    };
    transfer().map_err(|e| format!("Cannot send SyncTeX location: {e}"))?;
    Ok(true)
}

pub struct Server {
    pub requests: Receiver<Request>,
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
    path: PathBuf,
}

impl Server {
    pub fn bind(pdf: &Path, context: eframe::egui::Context) -> io::Result<Self> {
        let path = socket_path(pdf)?;
        if path.exists() {
            match UnixStream::connect(&path) {
                Ok(_) => {
                    return Err(io::Error::new(
                        io::ErrorKind::AddrInUse,
                        "Document already has a SyncTeX window",
                    ));
                }
                Err(e)
                    if matches!(
                        e.kind(),
                        io::ErrorKind::ConnectionRefused | io::ErrorKind::NotFound
                    ) =>
                {
                    let _ = fs::remove_file(&path);
                }
                Err(e) => return Err(e),
            }
        }
        let listener = UnixListener::bind(&path)?;
        fs::set_permissions(&path, fs::Permissions::from_mode(0o600))?;
        listener.set_nonblocking(true)?;
        let stop = Arc::new(AtomicBool::new(false));
        let stopped = stop.clone();
        let (tx, requests) = mpsc::sync_channel(16);
        let thread = std::thread::spawn(move || {
            while !stopped.load(Ordering::Relaxed) {
                match listener.accept() {
                    Ok((mut stream, _)) => {
                        let _ = stream.set_nonblocking(false);
                        let _ = stream.set_read_timeout(Some(Duration::from_millis(500)));
                        let _ = stream.set_write_timeout(Some(Duration::from_millis(500)));
                        let mut bytes = Vec::new();
                        if (&mut stream).take(65_537).read_to_end(&mut bytes).is_ok()
                            && bytes.len() <= 65_536
                            && let Ok(request) = serde_json::from_slice::<Request>(&bytes)
                            && request.target.as_ref().is_none_or(Target::valid)
                            && tx.try_send(request).is_ok()
                        {
                            let _ = stream.write_all(&[1]);
                            context.request_repaint();
                        }
                    }
                    Err(e) if e.kind() == io::ErrorKind::WouldBlock => {
                        std::thread::sleep(Duration::from_millis(20))
                    }
                    Err(_) => break,
                }
            }
        });
        Ok(Self {
            requests,
            stop,
            thread: Some(thread),
            path,
        })
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
        let _ = fs::remove_file(&self.path);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reuses_canonical_document_and_removes_socket_on_close() {
        let directory = tempfile::tempdir().unwrap();
        let pdf = directory.path().join("中文 document.pdf");
        fs::write(&pdf, b"test").unwrap();
        assert!(!send_existing(&pdf, &Request::default()).unwrap());
        let server = Server::bind(&pdf, eframe::egui::Context::default()).unwrap();
        let alias = directory.path().join("alias.pdf");
        std::os::unix::fs::symlink(&pdf, &alias).unwrap();
        let request = Request {
            target: Some(Target {
                page: 4,
                rect: [10.0, 20.0, 30.0, 40.0],
            }),
            ..Default::default()
        };
        assert!(send_existing(&alias, &request).unwrap());
        assert_eq!(
            server
                .requests
                .recv_timeout(Duration::from_secs(2))
                .unwrap()
                .target
                .unwrap()
                .page,
            4
        );
        assert!(Server::bind(&pdf, eframe::egui::Context::default()).is_err());
        let socket = server.path.clone();
        drop(server);
        assert!(!socket.exists());
        assert!(!send_existing(&pdf, &request).unwrap());
    }
}
