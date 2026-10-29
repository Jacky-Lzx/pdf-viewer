//! One persistent child per active visual plugin; bounded, nonblocking NDJSON RPC.
use super::{
    Plugin, Response, terminate,
    visual::{Line, Rect, Scene, valid_rect},
};
use serde::{Deserialize, Serialize};
use std::{
    io::{self, Read, Write},
    path::PathBuf,
    process::{Child, Stdio},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc::{self, Receiver, RecvTimeoutError, SyncSender, TryRecvError},
    },
    time::{Duration, Instant},
};

#[derive(Debug, Serialize)]
pub struct Request {
    pub api_version: u32,
    pub id: u64,
    pub event: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub key: Option<String>,
    pub context: Context,
    #[serde(skip)]
    pub revision: u64,
}

#[derive(Debug, Serialize)]
pub struct Context {
    pub document: PathBuf,
    pub page: usize,
    pub page_count: usize,
    pub page_size: [f32; 2],
    pub rotation: u8,
    pub zoom: f32,
    pub viewport: [f32; 2],
    pub scroll: [f32; 2],
    pub selected_text: String,
    pub settings: std::collections::BTreeMap<String, serde_json::Value>,
    /// Included for activate/page events, omitted for small view/key updates.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub lines: Option<Arc<Vec<Line>>>,
    pub lines_truncated: bool,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Reply {
    pub id: u64,
    #[serde(default)]
    pub message: Option<String>,
    #[serde(default)]
    pub copy_text: Option<String>,
    #[serde(default)]
    pub goto_page: Option<usize>,
    #[serde(default)]
    pub scene: Option<Scene>,
    #[serde(default)]
    pub capture_keys: Option<Vec<String>>,
    #[serde(default)]
    pub reveal: Option<Rect>,
    #[serde(default)]
    pub deactivate: bool,
}

impl Reply {
    pub fn validate(&self, count: usize) -> Result<(), String> {
        Response {
            goto_page: self.goto_page,
            ..Default::default()
        }
        .validate(count)?;
        if let Some(scene) = &self.scene {
            scene.validate(count)?;
        }
        if let Some(keys) = &self.capture_keys
            && (keys.len() > 32 || keys.iter().any(|key| !supported_key(key)))
        {
            return Err("capture_keys supports at most 32 unmodified letters or arrow keys".into());
        }
        if self.reveal.is_some_and(|rect| !valid_rect(rect)) {
            return Err("Invalid reveal rectangle".into());
        }
        Ok(())
    }
}

pub fn supported_key(key: &str) -> bool {
    (key.len() == 1 && key.as_bytes()[0].is_ascii_lowercase())
        || matches!(
            key,
            "ArrowUp" | "ArrowDown" | "ArrowLeft" | "ArrowRight" | "Space"
        )
}

pub struct Session {
    requests: Option<SyncSender<Request>>,
    replies: Receiver<Result<(u64, Reply), String>>,
    stop: Arc<AtomicBool>,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl Session {
    pub fn start(plugin: &Plugin, context: eframe::egui::Context) -> Result<Self, String> {
        if !plugin.enabled || plugin.api_version != 2 {
            return Err("Visual sessions require an enabled API v2 plugin".into());
        }
        let plugin = plugin.clone();
        let stop = Arc::new(AtomicBool::new(false));
        let cancelled = Arc::clone(&stop);
        let (requests, receive) = mpsc::sync_channel(16);
        let (send, replies) = mpsc::sync_channel(32);
        let thread = std::thread::Builder::new()
            .name("pagewing-visual-plugin".into())
            .spawn(move || {
                let result = run(&plugin, &receive, &send, &cancelled, &context);
                if let Err(error) = result {
                    let _ = send.try_send(Err(format!("{}: {error}", plugin.name)));
                }
                context.request_repaint();
            })
            .map_err(|error| error.to_string())?;
        Ok(Self {
            requests: Some(requests),
            replies,
            stop,
            thread: Some(thread),
        })
    }

    pub fn send(&self, request: Request) -> Result<(), String> {
        self.requests
            .as_ref()
            .ok_or("Visual session stopped")?
            .try_send(request)
            .map_err(|_| "Plugin event queue is full or stopped".into())
    }

    pub fn poll(&self) -> Option<Result<(u64, Reply), String>> {
        match self.replies.try_recv() {
            Ok(reply) => Some(reply),
            Err(TryRecvError::Empty) => None,
            Err(TryRecvError::Disconnected) => Some(Err("Visual plugin stopped".into())),
        }
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        // Disconnect wakes an idle runner immediately, without polling or a
        // long join on the UI thread.
        self.requests.take();
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

#[cfg(unix)]
fn nonblocking(file: &impl std::os::fd::AsRawFd) -> io::Result<()> {
    let fd = file.as_raw_fd();
    // SAFETY: the pipe owns a live fd throughout these fcntl calls.
    let flags = unsafe { libc::fcntl(fd, libc::F_GETFL) };
    if flags < 0 || unsafe { libc::fcntl(fd, libc::F_SETFL, flags | libc::O_NONBLOCK) } < 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}

fn run(
    plugin: &Plugin,
    requests: &Receiver<Request>,
    replies: &SyncSender<Result<(u64, Reply), String>>,
    stop: &AtomicBool,
    context: &eframe::egui::Context,
) -> Result<(), String> {
    let mut child = plugin
        .command()
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| e.to_string())?;
    let result = pump(&mut child, plugin, requests, replies, stop, context);
    terminate(&mut child);
    result
}

fn drain(file: &mut impl Read, bytes: &mut Vec<u8>) -> Result<(), String> {
    let mut buffer = [0; 4096];
    loop {
        match file.read(&mut buffer) {
            Ok(0) => return Ok(()),
            Ok(count) => {
                bytes.extend_from_slice(&buffer[..count]);
                if bytes.len() > super::MAX_OUTPUT as usize {
                    return Err("Plugin output exceeds 64 KiB per event".into());
                }
            }
            Err(e) if e.kind() == io::ErrorKind::WouldBlock => return Ok(()),
            Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
            Err(e) => return Err(e.to_string()),
        }
    }
}

fn pump(
    child: &mut Child,
    plugin: &Plugin,
    requests: &Receiver<Request>,
    replies: &SyncSender<Result<(u64, Reply), String>>,
    stop: &AtomicBool,
    context: &eframe::egui::Context,
) -> Result<(), String> {
    let mut stdin = child.stdin.take().ok_or("Missing plugin stdin")?;
    let mut stdout = child.stdout.take().ok_or("Missing plugin stdout")?;
    let mut stderr = child.stderr.take().ok_or("Missing plugin stderr")?;
    nonblocking(&stdin).map_err(|e| e.to_string())?;
    nonblocking(&stdout).map_err(|e| e.to_string())?;
    nonblocking(&stderr).map_err(|e| e.to_string())?;
    let mut active: Option<Request> = None;
    let mut input = Vec::new();
    let mut written = 0;
    let mut output = Vec::new();
    let mut errors = Vec::new();
    let mut since = Instant::now();
    while !stop.load(Ordering::Relaxed) {
        if active.is_none() {
            match requests.recv_timeout(Duration::from_millis(100)) {
                Ok(request) => {
                    input = serde_json::to_vec(&request).map_err(|e| e.to_string())?;
                    if input.len() > 1024 * 1024 {
                        return Err("Plugin context exceeds 1 MiB".into());
                    }
                    input.push(b'\n');
                    written = 0;
                    errors.clear();
                    since = Instant::now();
                    active = Some(request);
                }
                Err(RecvTimeoutError::Timeout) => {}
                Err(RecvTimeoutError::Disconnected) => return Ok(()),
            }
        }
        if active.is_some() && written < input.len() {
            match stdin.write(&input[written..]) {
                Ok(0) => return Err("Plugin closed stdin".into()),
                Ok(count) => written += count,
                Err(e)
                    if matches!(
                        e.kind(),
                        io::ErrorKind::WouldBlock | io::ErrorKind::Interrupted
                    ) => {}
                Err(e) => {
                    // An early exit can close stdin before try_wait observes it.
                    // Preserve the child's diagnostic instead of only Broken pipe.
                    drain(&mut stderr, &mut errors)?;
                    return Err(format!("{e}: {}", String::from_utf8_lossy(&errors).trim()));
                }
            }
        }
        drain(&mut stdout, &mut output)?;
        drain(&mut stderr, &mut errors)?;
        if let Some(end) = output.iter().position(|byte| *byte == b'\n') {
            let request = active.take().ok_or("Unsolicited plugin response")?;
            if written < input.len() {
                return Err("Plugin replied before reading its context".into());
            }
            let reply: Reply = serde_json::from_slice(&output[..end]).map_err(|e| e.to_string())?;
            if reply.id != request.id {
                return Err("Plugin response id does not match event".into());
            }
            reply.validate(request.context.page_count)?;
            if output[end + 1..].iter().any(|b| !b.is_ascii_whitespace()) {
                return Err("Plugin must return exactly one JSON line per event".into());
            }
            output.clear();
            replies
                .try_send(Ok((request.revision, reply)))
                .map_err(|_| "Plugin response queue is full")?;
            context.request_repaint();
        }
        if let Some(status) = child.try_wait().map_err(|e| e.to_string())? {
            return Err(format!(
                "Visual plugin exited with {status}: {}",
                String::from_utf8_lossy(&errors).trim()
            ));
        }
        if active.is_some() && since.elapsed() >= plugin.timeout {
            return Err("Visual plugin event timed out".into());
        }
        if active.is_some() {
            std::thread::sleep(Duration::from_millis(2));
        }
    }
    Ok(())
}

#[cfg(all(test, unix))]
mod tests;
