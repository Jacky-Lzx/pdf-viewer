//! On-demand process plugins. No code is loaded into the reader process.
pub mod session;
#[cfg(test)]
mod tests;
pub mod visual;
use serde::{Deserialize, Serialize};
use std::{
    fs,
    io::{Read, Seek, SeekFrom, Write},
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc::{self, Receiver, TryRecvError},
    },
    time::{Duration, Instant},
};

const MAX_OUTPUT: u64 = 64 * 1024;
const MAX_MANIFEST: u64 = 16 * 1024;
const MAX_PLUGINS: usize = 128;

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Manifest {
    api_version: u32,
    name: String,
    command: Vec<String>,
    #[serde(default)]
    enabled: bool,
    #[serde(default = "default_timeout")]
    timeout_seconds: u64,
}

fn default_timeout() -> u64 {
    10
}

#[derive(Clone, Debug)]
pub struct Plugin {
    pub api_version: u32,
    pub name: String,
    pub enabled: bool,
    directory: PathBuf,
    command: Vec<String>,
    timeout: Duration,
}

#[derive(Default)]
pub struct Registry {
    pub plugins: Vec<Plugin>,
    pub errors: Vec<String>,
}

impl Registry {
    pub fn discover(root: &Path) -> Self {
        let mut registry = Self::default();
        let entries = match fs::read_dir(root) {
            Ok(entries) => entries,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return registry,
            Err(error) => {
                registry.errors.push(format!("{}: {error}", root.display()));
                return registry;
            }
        };
        // Bound discovery work, including unrelated files in the directory.
        let mut paths = Vec::new();
        for entry in entries.take(MAX_PLUGINS + 1) {
            match entry {
                Ok(entry) => paths.push(entry.path()),
                Err(error) => registry.errors.push(error.to_string()),
            }
        }
        if paths.len() > MAX_PLUGINS {
            registry
                .errors
                .push(format!("Plugin directory exceeds {MAX_PLUGINS} entries"));
            return registry;
        }
        paths.sort();
        for directory in paths.into_iter().filter(|path| path.is_dir()) {
            let path = directory.join("plugin.toml");
            let loaded = (|| -> Result<Plugin, String> {
                let mut text = String::new();
                fs::File::open(&path)
                    .map_err(|error| error.to_string())?
                    .take(MAX_MANIFEST + 1)
                    .read_to_string(&mut text)
                    .map_err(|error| error.to_string())?;
                if text.len() as u64 > MAX_MANIFEST {
                    return Err("Manifest exceeds 16 KiB".into());
                }
                let manifest: Manifest = toml::from_str(&text).map_err(|e| e.to_string())?;
                if !matches!(manifest.api_version, 1 | 2) {
                    return Err("Unsupported plugin api_version (expected 1 or 2)".into());
                }
                if manifest.name.trim().is_empty() || manifest.name.len() > 128 {
                    return Err("Plugin name must contain 1–128 bytes".into());
                }
                if manifest.command.is_empty() || manifest.command[0].trim().is_empty() {
                    return Err("Plugin command must contain an executable".into());
                }
                if manifest.command.iter().any(|arg| arg.contains('\0')) {
                    return Err("Plugin command cannot contain NUL bytes".into());
                }
                if !(1..=120).contains(&manifest.timeout_seconds) {
                    return Err("timeout_seconds must be between 1 and 120".into());
                }
                Ok(Plugin {
                    api_version: manifest.api_version,
                    name: manifest.name,
                    enabled: manifest.enabled,
                    directory,
                    command: manifest.command,
                    timeout: Duration::from_secs(manifest.timeout_seconds),
                })
            })();
            match loaded {
                Ok(plugin) => registry.plugins.push(plugin),
                Err(error) => registry.errors.push(format!("{}: {error}", path.display())),
            }
        }
        registry
    }
}

#[derive(Debug, Serialize)]
pub struct Request {
    pub api_version: u32,
    pub document: PathBuf,
    /// Physical page numbers are one-based, independent of PDF page labels.
    pub page: usize,
    pub page_count: usize,
    pub selected_text: String,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Response {
    pub message: Option<String>,
    pub copy_text: Option<String>,
    pub goto_page: Option<usize>,
}

impl Response {
    pub fn validate(&self, page_count: usize) -> Result<(), String> {
        if self
            .goto_page
            .is_some_and(|page| page == 0 || page > page_count)
        {
            return Err("Plugin returned a page outside the document".into());
        }
        Ok(())
    }
}

pub struct Job {
    result: Receiver<Result<Response, String>>,
    cancelled: Arc<AtomicBool>,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl Job {
    pub fn poll(&self) -> Option<Result<Response, String>> {
        match self.result.try_recv() {
            Ok(result) => Some(result),
            Err(TryRecvError::Empty) => None,
            Err(TryRecvError::Disconnected) => Some(Err("Plugin runner stopped".into())),
        }
    }
}

impl Drop for Job {
    fn drop(&mut self) {
        self.cancelled.store(true, Ordering::Relaxed);
        // Reap before the reader exits (or starts another job). The runner checks
        // cancellation every 10 ms and never waits on a plugin-owned pipe.
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

impl Plugin {
    fn command(&self) -> Command {
        let executable = Path::new(&self.command[0]);
        let executable = if executable.is_relative() && executable.components().count() > 1 {
            self.directory.join(executable)
        } else {
            executable.to_path_buf()
        };
        let mut command = Command::new(executable);
        command
            .args(&self.command[1..])
            .current_dir(&self.directory);
        #[cfg(unix)]
        {
            use std::os::unix::process::CommandExt;
            command.process_group(0);
        }
        command
    }

    pub fn start(&self, request: Request) -> Result<Job, String> {
        if !self.enabled {
            return Err("Plugin is disabled; set enabled = true in plugin.toml".into());
        }
        let plugin = self.clone();
        let cancelled = Arc::new(AtomicBool::new(false));
        let stop = Arc::clone(&cancelled);
        let (send, result) = mpsc::sync_channel(1);
        let thread = std::thread::Builder::new()
            .name("pagewing-plugin".into())
            .spawn(move || {
                let result = plugin.execute(&request, &stop);
                let _ = send.send(result);
            })
            .map_err(|error| error.to_string())?;
        Ok(Job {
            result,
            cancelled,
            thread: Some(thread),
        })
    }

    fn execute(&self, request: &Request, cancelled: &AtomicBool) -> Result<Response, String> {
        self.execute_io(request, cancelled)
            .map_err(|error| format!("{}: {error}", self.name))
    }

    fn execute_io(&self, request: &Request, cancelled: &AtomicBool) -> Result<Response, String> {
        let run = || -> Result<Response, Box<dyn std::error::Error>> {
            // Regular temporary files avoid pipe deadlocks and extra I/O threads.
            let mut input = tempfile::tempfile()?;
            serde_json::to_writer(&mut input, request)?;
            input.write_all(b"\n")?;
            input.seek(SeekFrom::Start(0))?;
            let mut output = tempfile::tempfile()?;
            let mut errors = tempfile::tempfile()?;
            let mut command = self.command();
            command
                .stdin(Stdio::from(input))
                .stdout(Stdio::from(output.try_clone()?))
                .stderr(Stdio::from(errors.try_clone()?));
            let mut child = command.spawn()?;
            let start = Instant::now();
            let status = loop {
                let failure = if cancelled.load(Ordering::Relaxed) {
                    Some("Plugin cancelled")
                } else if start.elapsed() >= self.timeout {
                    Some("Plugin timed out")
                } else if output.metadata().map_or(true, |m| m.len() > MAX_OUTPUT)
                    || errors.metadata().map_or(true, |m| m.len() > MAX_OUTPUT)
                {
                    Some("Plugin output exceeds 64 KiB")
                } else {
                    None
                };
                if let Some(failure) = failure {
                    terminate(&mut child);
                    return Err(failure.into());
                }
                match child.try_wait() {
                    Ok(Some(status)) => break status,
                    Ok(None) => std::thread::sleep(Duration::from_millis(10)),
                    Err(error) => {
                        terminate(&mut child);
                        return Err(error.into());
                    }
                }
            };
            terminate(&mut child);
            let stdout = read_output(&mut output)?;
            let stderr = read_output(&mut errors)?;
            if !status.success() {
                return Err(format!("Plugin exited with {status}: {}", stderr.trim()).into());
            }
            let response: Response = serde_json::from_str(&stdout)?;
            response.validate(request.page_count)?;
            Ok(response)
        };
        run().map_err(|error| error.to_string())
    }
}

fn read_output(file: &mut fs::File) -> Result<String, Box<dyn std::error::Error>> {
    file.seek(SeekFrom::Start(0))?;
    let mut text = String::new();
    file.take(MAX_OUTPUT + 1).read_to_string(&mut text)?;
    if text.len() as u64 > MAX_OUTPUT {
        return Err("Plugin output exceeds 64 KiB".into());
    }
    Ok(text)
}

fn terminate(child: &mut std::process::Child) {
    #[cfg(unix)]
    // SAFETY: process_group(0) made this child leader of a separate process group.
    // A negative PID addresses only that group; the reader is never in it.
    unsafe {
        libc::kill(-(child.id() as libc::pid_t), libc::SIGKILL);
    }
    let _ = child.kill();
    let _ = child.wait();
}
