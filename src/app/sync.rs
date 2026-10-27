use super::Pagewing;
use crate::{
    synctex::{self, Options, PageGeometry, Target},
    synctex_ipc::{Request, Server},
};
use eframe::egui::{self, Color32};
use hayro::kurbo::{Point, Rect};
use std::{
    sync::mpsc::{self, Receiver},
    time::{Duration, Instant},
};

#[derive(Default)]
pub(super) struct SyncUi {
    pub server: Option<Server>,
    pub geometry: Vec<PageGeometry>,
    options: Options,
    pending: Option<Target>,
    highlight: Option<(usize, Rect, Instant)>,
    reverse: Option<(u64, Receiver<Result<String, String>>)>,
}

impl SyncUi {
    pub fn cancel_pending(&mut self) {
        self.pending = None;
    }

    pub fn clear_document(&mut self) {
        self.server = None;
        self.geometry.clear();
        self.pending = None;
        self.highlight = None;
        self.reverse = None;
        self.options.directory = None;
        self.options.position_command = None;
    }
}

impl Pagewing {
    pub fn configure_synctex(&mut self, request: Request, context: &egui::Context) {
        if let Some(editor) = request.options.editor {
            self.synctex.options.editor = Some(editor);
            self.synctex.options.position_command = None;
        }
        if let Some(command) = request.options.position_command {
            self.synctex.options.position_command = Some(command);
            self.synctex.options.editor = None;
        }
        if let Some(directory) = request.options.directory {
            self.synctex.options.directory = Some(directory);
        }
        if let Some(target) = request.target {
            if !target.valid() {
                self.error = Some("Invalid SyncTeX target".into());
                return;
            }
            if self.comment_change_blocked(context) {
                return;
            }
            self.synctex.pending = Some(target);
            if !self.loading && self.nav.count > 0 {
                // A forward search commonly follows a rebuild. Load that PDF before
                // applying the new coordinates, even if the watcher hasn't fired yet.
                self.reload(context);
            }
        }
        if request.options.focus != Some(false) {
            context.send_viewport_cmd(egui::ViewportCommand::Minimized(false));
            context.send_viewport_cmd(egui::ViewportCommand::Focus);
        }
    }

    pub(super) fn synctex_events(&mut self, context: &egui::Context) {
        let requests: Vec<_> = self
            .synctex
            .server
            .as_ref()
            .map(|s| s.requests.try_iter().collect())
            .unwrap_or_default();
        for request in requests {
            self.configure_synctex(request, context);
        }
        if !self.loading
            && self.nav.count > 0
            && let Some(target) = self.synctex.pending.take()
        {
            if let Some(geometry) = self.synctex.geometry.get(target.page).copied() {
                let rect = geometry.render_rect(target.rect);
                self.jump(target.page);
                self.pending_offset = None;
                self.pending_destination = None;
                self.pending_focus = Some(rect);
                self.synctex.highlight = Some((target.page, rect, Instant::now()));
                self.notify(format!("SyncTeX · page {}", target.page + 1));
                self.dirty = true;
            } else {
                self.error =
                    Some("SyncTeX page is outside this PDF. Recompile with -synctex=1.".into());
            }
        }
        if let Some((id, receiver)) = &self.synctex.reverse
            && let Ok(result) = receiver.try_recv()
        {
            let id = *id;
            self.synctex.reverse = None;
            if id == self.id {
                match result {
                    Ok(message) => self.notify(message),
                    Err(error) => self.error = Some(error.into()),
                }
            }
        }
    }

    pub(super) fn synctex_click(&mut self, ui: &egui::Ui, page: usize, page_rect: egui::Rect) {
        if self.selection.active || self.loading || self.synctex.reverse.is_some() {
            return;
        }
        let click = ui.input(|i| {
            i.modifiers.command && i.pointer.button_clicked(egui::PointerButton::Primary)
        });
        if !click || !ui.rect_contains_pointer(page_rect.intersect(ui.clip_rect())) {
            return;
        }
        let Some(position) = ui.input(|i| i.pointer.interact_pos()) else {
            return;
        };
        let Some(geometry) = self.synctex.geometry.get(page).copied() else {
            return;
        };
        let options = self.synctex.options.clone();
        let editor = options.editor.clone().or_else(|| {
            std::env::var("PAGEWING_EDITOR")
                .ok()
                .and_then(|s| synctex::editor_command(&s).ok())
        });
        if editor.is_none() && options.position_command.is_none() {
            self.notify(
                "Set --inverse-search 'code --goto %f:%l:%c' or PAGEWING_EDITOR to jump to source",
            );
            return;
        }
        let local = position - page_rect.min;
        let point = crate::geometry::unrotate_point(
            Point::new(
                f64::from(local.x / self.scale),
                f64::from(local.y / self.scale),
            ),
            self.dimensions[page],
            self.reading.rotation,
        );
        let point = geometry.source_point(point);
        let pdf = self.reading.path.clone();
        let (tx, receiver) = mpsc::channel();
        self.synctex.reverse = Some((self.id, receiver));
        let context = ui.ctx().clone();
        std::thread::spawn(move || {
            let result = if let Some(command) = &options.position_command {
                synctex::launch_position(command, &pdf, page, point)
                    .map(|()| format!("SyncTeX · page {} sent to editor", page + 1))
            } else {
                synctex::reverse(&pdf, page, point, &options).and_then(|source| {
                    synctex::launch_editor(editor.as_ref().unwrap(), &source)?;
                    Ok(format!(
                        "SyncTeX · {}:{}:{}",
                        source.path.display(),
                        source.line,
                        source.column
                    ))
                })
            };
            let _ = tx.send(result);
            context.request_repaint();
        });
    }

    pub(super) fn paint_synctex(
        &self,
        painter: &egui::Painter,
        page: usize,
        page_rect: egui::Rect,
        dimensions: [f32; 2],
    ) {
        if let Some((target_page, rect, since)) = self.synctex.highlight
            && page == target_page
            && since.elapsed() < Duration::from_secs(3)
        {
            let rect = self
                .link_rect(rect, page_rect, dimensions)
                .intersect(page_rect);
            painter.rect_filled(
                rect.expand(2.0),
                2.0,
                Color32::from_rgba_unmultiplied(255, 143, 36, 95),
            );
            painter.ctx().request_repaint_after(
                Duration::from_secs(3) - since.elapsed().min(Duration::from_secs(3)),
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::synctex::Source;
    use egui::{Event, Modifiers, Pos2, RawInput, Vec2};
    use std::{fs, path::Path, process::Command};

    fn frame(app: &mut Pagewing, context: &egui::Context, events: Vec<Event>) {
        let _ = context.run(
            RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    Pos2::ZERO,
                    Vec2::new(1000.0, 800.0),
                )),
                modifiers: Modifiers::COMMAND,
                events,
                ..Default::default()
            },
            |context| {
                app.events(context);
                app.synctex_events(context);
                app.canvas(context);
            },
        );
    }

    fn wait(app: &mut Pagewing, context: &egui::Context, ready: impl Fn(&Pagewing) -> bool) {
        let until = Instant::now() + Duration::from_secs(8);
        while !ready(app) {
            assert!(
                Instant::now() < until,
                "SyncTeX UI timed out: {:?}",
                app.error
            );
            frame(app, context, Vec::new());
            std::thread::sleep(Duration::from_millis(10));
        }
    }

    #[test]
    fn forward_request_waits_for_load_and_reuses_window_via_ipc() {
        let directory = tempfile::tempdir().unwrap();
        let pdf = directory.path().join("book.pdf");
        fs::copy("assets/pagewing-guide.pdf", &pdf).unwrap();
        let context = egui::Context::default();
        let mut app = Pagewing::with_context(
            &context,
            Some(pdf.clone()),
            None,
            directory.path().join("library.json"),
        );
        let request = Request {
            target: Some(Target {
                page: 1,
                rect: [50.0, 400.0, 100.0, 415.0],
            }),
            ..Default::default()
        };
        app.configure_synctex(request.clone(), &context);
        assert!(app.loading);
        wait(&mut app, &context, |a| {
            !a.loading && a.synctex.highlight.is_some()
        });
        assert_eq!(app.nav.page, 1);
        let id = app.id;
        let mut request = request;
        request.target.as_mut().unwrap().page = 0;
        assert!(crate::synctex_ipc::send_existing(&pdf, &request).unwrap());
        wait(&mut app, &context, |a| a.nav.page == 0 && !a.loading);
        assert_eq!(app.id, id, "the existing document window must be reused");
        assert!(app.error.is_none(), "{:?}", app.error);
        app.close_document(&context);
        assert!(app.synctex.server.is_none());
        assert!(app.synctex.highlight.is_none());
    }

    #[test]
    fn background_requests_do_not_emit_focus_or_unminimize_commands() {
        let context = egui::Context::default();
        let directory = tempfile::tempdir().unwrap();
        let mut app =
            Pagewing::with_context(&context, None, None, directory.path().join("library.json"));
        let output = context.run(RawInput::default(), |context| {
            app.configure_synctex(
                Request {
                    options: Options {
                        focus: Some(false),
                        ..Default::default()
                    },
                    ..Default::default()
                },
                context,
            );
        });
        assert!(
            !output.viewport_output[&egui::ViewportId::ROOT]
                .commands
                .iter()
                .any(|c| matches!(
                    c,
                    egui::ViewportCommand::Focus | egui::ViewportCommand::Minimized(false)
                ))
        );
        let output = context.run(RawInput::default(), |context| {
            app.configure_synctex(Request::default(), context)
        });
        assert!(
            output.viewport_output[&egui::ViewportId::ROOT]
                .commands
                .iter()
                .any(|c| matches!(c, egui::ViewportCommand::Focus))
        );
    }

    #[test]
    #[ignore = "requires pdflatex, synctex and python3"]
    fn real_tex_forward_reverse_through_egui() {
        let directory = tempfile::Builder::new()
            .prefix("pagewing synctex 中文 ")
            .tempdir()
            .unwrap();
        let root = directory.path();
        fs::create_dir(root.join("build")).unwrap();
        fs::write(root.join("main.tex"), "\\documentclass{article}\n\\begin{document}\nFirst page target.\n\\newpage\nSecond page target.\n\\newpage\nThird page target.\n\\end{document}\n").unwrap();
        let compile = Command::new("pdflatex")
            .current_dir(root)
            .args([
                "-interaction=nonstopmode",
                "-halt-on-error",
                "-synctex=1",
                "-output-directory=build",
                "main.tex",
            ])
            .output()
            .unwrap();
        assert!(
            compile.status.success(),
            "{}",
            String::from_utf8_lossy(&compile.stdout)
        );
        let pdf = root.join("build/main.pdf");
        let source = Source {
            path: root.join("main.tex"),
            line: 5,
            column: 1,
        };
        let target = synctex::forward(&pdf, &source, &Options::default()).unwrap();
        assert_eq!(target.page, 1);
        let context = egui::Context::default();
        let mut app = Pagewing::with_context(&context, Some(pdf), None, root.join("library.json"));
        let capture = root.join("editor.json");
        let editor = vec!["python3".into(), "-c".into(), "import sys,json,pathlib; pathlib.Path(sys.argv[1]).write_text(json.dumps(sys.argv[2:]))".into(), capture.to_str().unwrap().into(), "%f".into(), "%l".into(), "%c".into()];
        for continuous in [false, true] {
            for rotation in 0..4 {
                let _ = fs::remove_file(&capture);
                app.reading.continuous = continuous;
                app.reading.rotation = rotation;
                app.reading.fit = crate::navigation::Fit::Custom(1.75);
                app.synctex.highlight = None;
                app.configure_synctex(
                    Request {
                        target: Some(target.clone()),
                        options: Options {
                            editor: Some(editor.clone()),
                            ..Default::default()
                        },
                    },
                    &context,
                );
                wait(&mut app, &context, |a| {
                    !a.loading && a.synctex.highlight.is_some()
                });
                frame(&mut app, &context, Vec::new());
                let dimensions = app.dimensions[1];
                let rect = app.synctex.geometry[1].render_rect(target.rect);
                let rotated = crate::geometry::rotate_rect(rect, dimensions, rotation);
                let size = if rotation % 2 == 1 {
                    Vec2::new(dimensions[1], dimensions[0])
                } else {
                    Vec2::from(dimensions)
                } * app.scale;
                let origin = Pos2::new(24.0, 24.0)
                    + Vec2::new(
                        (app.viewport.x - size.x).max(0.0) * 0.5,
                        if continuous {
                            0.0
                        } else {
                            (app.viewport.y - size.y).max(0.0) * 0.5
                        },
                    )
                    - app.pan;
                let position = origin
                    + Vec2::new(rotated.center().x as f32, rotated.center().y as f32) * app.scale;
                assert!(
                    egui::Rect::from_min_size(Pos2::ZERO, Vec2::new(1000.0, 800.0))
                        .contains(position),
                    "target outside viewport: {position:?}"
                );
                frame(&mut app, &context, vec![Event::PointerMoved(position)]);
                for pressed in [true, false] {
                    frame(
                        &mut app,
                        &context,
                        vec![Event::PointerButton {
                            pos: position,
                            button: egui::PointerButton::Primary,
                            pressed,
                            modifiers: Modifiers::COMMAND,
                        }],
                    );
                }
                wait(&mut app, &context, |a| {
                    a.synctex.reverse.is_none() && capture.exists()
                });
                let args: Vec<String> =
                    serde_json::from_slice(&fs::read(&capture).unwrap()).unwrap();
                assert_eq!(
                    fs::canonicalize(Path::new(&args[0])).unwrap(),
                    fs::canonicalize(&source.path).unwrap()
                );
                assert_eq!(
                    &args[1..],
                    ["5", "1"],
                    "continuous={continuous}, rotation={rotation}"
                );
                assert!(app.error.is_none(), "{:?}", app.error);
            }
        }

        // Multiple pages are visible: clicking page 2 must query page 2 even
        // though the active reading page is still page 1.
        fs::remove_file(&capture).unwrap();
        app.reading.continuous = true;
        app.reading.rotation = 0;
        app.reading.fit = crate::navigation::Fit::Custom(0.3);
        app.jump(0);
        frame(&mut app, &context, Vec::new());
        let layout = app.continuous_layout.pages[1];
        let rect = app.synctex.geometry[1].render_rect(target.rect);
        let position = Pos2::new(
            24.0 + (app.viewport.x - layout.size[0]).max(0.0) * 0.5
                + rect.center().x as f32 * layout.scale,
            24.0 + layout.top as f32 + rect.center().y as f32 * layout.scale,
        );
        frame(&mut app, &context, vec![Event::PointerMoved(position)]);
        for pressed in [true, false] {
            frame(
                &mut app,
                &context,
                vec![Event::PointerButton {
                    pos: position,
                    button: egui::PointerButton::Primary,
                    pressed,
                    modifiers: Modifiers::COMMAND,
                }],
            );
        }
        wait(&mut app, &context, |a| {
            a.synctex.reverse.is_none() && capture.exists()
        });
        let args: Vec<String> = serde_json::from_slice(&fs::read(&capture).unwrap()).unwrap();
        assert_eq!(app.nav.page, 0);
        assert_eq!(args[1], "5");

        let aux = root.join("separate aux");
        fs::create_dir(&aux).unwrap();
        fs::rename(
            root.join("build/main.synctex.gz"),
            aux.join("main.synctex.gz"),
        )
        .unwrap();
        let options = Options {
            directory: Some(aux.clone()),
            ..Default::default()
        };
        assert_eq!(
            synctex::forward(&root.join("build/main.pdf"), &source, &options)
                .unwrap()
                .page,
            1
        );
        assert!(
            Command::new("gzip")
                .arg("-d")
                .arg(aux.join("main.synctex.gz"))
                .status()
                .unwrap()
                .success()
        );
        assert_eq!(
            synctex::forward(&root.join("build/main.pdf"), &source, &options)
                .unwrap()
                .page,
            1
        );
        let result = synctex::reverse(
            &root.join("build/main.pdf"),
            1,
            Point::new(target.rect[0] + 10.0, target.rect[3] - 2.0),
            &options,
        )
        .unwrap();
        assert_eq!(result.line, 5);

        // The LSP integration must work even without any local SyncTeX mapping.
        fs::remove_file(aux.join("main.synctex")).unwrap();
        let callback = vec!["python3".into(), "-c".into(), "import sys,json,pathlib; pathlib.Path(sys.argv[1]).write_text(json.dumps(sys.argv[2:]))".into(), capture.to_str().unwrap().into(), "%f".into(), "%p".into(), "%x".into(), "%y".into()];
        for continuous in [false, true] {
            for rotation in 0..4 {
                fs::remove_file(&capture).unwrap();
                app.reading.continuous = continuous;
                app.reading.rotation = rotation;
                app.reading.fit = crate::navigation::Fit::Custom(1.75);
                app.synctex.highlight = None;
                app.configure_synctex(
                    Request {
                        target: Some(target.clone()),
                        options: Options {
                            position_command: Some(callback.clone()),
                            focus: Some(false),
                            ..Default::default()
                        },
                    },
                    &context,
                );
                wait(&mut app, &context, |a| {
                    !a.loading && a.synctex.highlight.is_some()
                });
                frame(&mut app, &context, Vec::new());
                let dimensions = app.dimensions[1];
                let rect = app.synctex.geometry[1].render_rect(target.rect);
                let rotated = crate::geometry::rotate_rect(rect, dimensions, rotation);
                let size = if rotation % 2 == 1 {
                    Vec2::new(dimensions[1], dimensions[0])
                } else {
                    Vec2::from(dimensions)
                } * app.scale;
                let origin = Pos2::new(24.0, 24.0)
                    + Vec2::new(
                        (app.viewport.x - size.x).max(0.0) * 0.5,
                        if continuous {
                            0.0
                        } else {
                            (app.viewport.y - size.y).max(0.0) * 0.5
                        },
                    )
                    - app.pan;
                let position = origin
                    + Vec2::new(rotated.center().x as f32, rotated.center().y as f32) * app.scale;
                frame(&mut app, &context, vec![Event::PointerMoved(position)]);
                for pressed in [true, false] {
                    frame(
                        &mut app,
                        &context,
                        vec![Event::PointerButton {
                            pos: position,
                            button: egui::PointerButton::Primary,
                            pressed,
                            modifiers: Modifiers::COMMAND,
                        }],
                    );
                }
                wait(&mut app, &context, |a| {
                    a.synctex.reverse.is_none() && capture.exists()
                });
                let args: Vec<String> =
                    serde_json::from_slice(&fs::read(&capture).unwrap()).unwrap();
                assert_eq!(
                    fs::canonicalize(&args[0]).unwrap(),
                    fs::canonicalize(root.join("build/main.pdf")).unwrap()
                );
                assert_eq!(args[1], "2");
                assert!(
                    (args[2].parse::<f64>().unwrap() - (target.rect[0] + target.rect[2]) * 0.5)
                        .abs()
                        < 0.01
                );
                assert!(
                    (args[3].parse::<f64>().unwrap() - (target.rect[1] + target.rect[3]) * 0.5)
                        .abs()
                        < 0.01
                );
                assert!(app.error.is_none(), "{:?}", app.error);
            }
        }
    }
}
