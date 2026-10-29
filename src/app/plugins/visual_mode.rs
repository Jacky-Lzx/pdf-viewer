use super::*;

#[derive(Clone, PartialEq)]
struct View {
    page: usize,
    rotation: u8,
    zoom: f32,
    scroll: [f32; 2],
    viewport: [f32; 2],
    selection: String,
}

pub(super) struct Visual {
    pub name: String,
    document: u64,
    session: session::Session,
    revision: u64,
    page: Option<usize>,
    lines: Option<(Arc<Vec<visual::Line>>, bool)>,
    inspected: Option<Instant>,
    view: Option<View>,
    sent: Instant,
    next_id: u64,
    activated: bool,
    in_flight: bool,
    keys: Vec<String>,
    pending_keys: VecDeque<String>,
    scene: Option<visual::Scene>,
}

impl Pagewing {
    pub(super) fn start_visual_plugin(&mut self, index: usize, context: &egui::Context) {
        let plugin = &self.plugins.registry.plugins[index];
        match session::Session::start(plugin, context.clone()) {
            Ok(session) => {
                self.plugins.visual = Some(Visual {
                    name: plugin.name.clone(),
                    document: self.id,
                    session,
                    revision: 0,
                    page: None,
                    lines: None,
                    inspected: None,
                    view: None,
                    sent: Instant::now(),
                    next_id: 0,
                    activated: false,
                    in_flight: false,
                    keys: Vec::new(),
                    pending_keys: VecDeque::new(),
                    scene: None,
                });
                self.notify("Visual plugin active · Esc to exit");
                context.request_repaint();
            }
            Err(error) => self.error = Some(error.into()),
        }
    }

    pub(in crate::app) fn plugin_page_text(&mut self, page: usize, text: &crate::text::PageText) {
        if let Some(active) = &mut self.plugins.visual
            && active.page == Some(page)
            && active.lines.is_none()
        {
            let (lines, truncated) = visual::lines(text);
            active.lines = Some((Arc::new(lines), truncated));
        }
    }

    pub(in crate::app) fn plugin_key(
        &mut self,
        context: &egui::Context,
        key: egui::Key,
        modifiers: egui::Modifiers,
    ) -> bool {
        if self.plugins.visual.is_none() || modifiers.any() {
            return false;
        }
        if key == egui::Key::Escape {
            self.cancel_plugins();
            self.notify("Visual plugin stopped");
            return true;
        }
        let name = match key {
            egui::Key::ArrowUp => "ArrowUp".into(),
            egui::Key::ArrowDown => "ArrowDown".into(),
            egui::Key::ArrowLeft => "ArrowLeft".into(),
            egui::Key::ArrowRight => "ArrowRight".into(),
            egui::Key::Space => "Space".into(),
            _ => format!("{key:?}").to_ascii_lowercase(),
        };
        let active = self.plugins.visual.as_mut().unwrap();
        if active.page != Some(self.nav.page) || !active.keys.contains(&name) {
            return false;
        }
        if active.pending_keys.len() >= 16 {
            self.cancel_plugins();
            self.error = Some("Visual plugin cannot keep up with keyboard input".into());
            return true;
        }
        active.pending_keys.push_back(name);
        self.nav.clear_prefix();
        context.request_repaint();
        true
    }

    pub(super) fn visual_plugin_events(&mut self, context: &egui::Context) {
        let Some(mut active) = self.plugins.visual.take() else {
            return;
        };
        if active.document != self.id || self.loading || self.reloading {
            return;
        }
        // One RPC is in flight. Drain before sending its successor in plugin_sync.
        if let Some(result) = active.session.poll() {
            active.in_flight = false;
            match result {
                Ok((revision, reply))
                    if revision == active.revision && active.page == Some(self.nav.page) =>
                {
                    if let Err(error) = reply.validate(self.nav.count) {
                        self.error = Some(error.into());
                        return;
                    }
                    if reply.deactivate {
                        self.notify("Visual plugin stopped");
                        return;
                    }
                    if let Some(scene) = reply.scene {
                        active.scene = Some(scene);
                    }
                    if let Some(keys) = reply.capture_keys {
                        active.keys = keys;
                    }
                    if let Some(message) = reply.message {
                        self.plugins.output = Some(message);
                    }
                    if let Some(text) = reply.copy_text {
                        context.copy_text(text);
                    }
                    if let Some(page) = reply.goto_page {
                        self.jump(page - 1);
                    }
                    if let Some([x0, y0, x1, y1]) = reply.reveal {
                        self.pending_focus = Some(hayro::kurbo::Rect::new(x0, y0, x1, y1));
                    }
                    context.request_repaint();
                }
                Ok(_) => {} // Page change made this response obsolete.
                Err(error) => {
                    self.error = Some(error.into());
                    return;
                }
            }
        }
        self.plugins.visual = Some(active);
    }

    /// Called after input and canvas layout, so events contain the resolved view.
    pub(in crate::app) fn plugin_sync(&mut self, context: &egui::Context) {
        let Some(mut active) = self.plugins.visual.take() else {
            return;
        };
        if self.id != active.document || self.loading || self.reloading || self.nav.count == 0 {
            return;
        }
        if active.page != Some(self.nav.page) {
            active.page = Some(self.nav.page);
            active.revision += 1;
            active.lines = None;
            active.view = None;
            active.scene = None;
            active.inspected = None;
            active.pending_keys.clear();
        }
        if active.lines.is_none() {
            if active
                .inspected
                .is_none_or(|time| time.elapsed() >= Duration::from_millis(250))
            {
                self.worker.inspect_all(self.nav.page);
                active.inspected = Some(Instant::now());
            }
            context.request_repaint_after(Duration::from_millis(50));
            self.plugins.visual = Some(active);
            return;
        }
        let view = View {
            page: self.nav.page,
            rotation: self.reading.rotation,
            zoom: self.scale,
            scroll: [self.pan.x, self.pan.y],
            viewport: [self.viewport.x, self.viewport.y],
            selection: self
                .selection
                .selected_text()
                .unwrap_or_default()
                .chars()
                .take(4096)
                .collect(),
        };
        if !active.in_flight {
            let (event, key) = if active.view.is_none() {
                (if active.activated { "page" } else { "activate" }, None)
            } else if let Some(key) = active.pending_keys.pop_front() {
                ("key", Some(key))
            } else if active.view.as_ref() != Some(&view) {
                let elapsed = active.sent.elapsed();
                if elapsed < Duration::from_millis(50) {
                    context.request_repaint_after(Duration::from_millis(50) - elapsed);
                    self.plugins.visual = Some(active);
                    return;
                }
                (
                    if active
                        .view
                        .as_ref()
                        .is_some_and(|v| v.selection != view.selection)
                    {
                        "selection"
                    } else {
                        "view"
                    },
                    None,
                )
            } else {
                self.plugins.visual = Some(active);
                return;
            };
            active.next_id += 1;
            let (lines, truncated) = active.lines.as_ref().unwrap();
            let request = session::Request {
                api_version: 2,
                id: active.next_id,
                event,
                key,
                revision: active.revision,
                context: session::Context {
                    document: self.reading.path.clone(),
                    page: self.nav.page + 1,
                    page_count: self.nav.count,
                    page_size: self.dimensions[self.nav.page],
                    rotation: view.rotation,
                    zoom: view.zoom,
                    scroll: view.scroll,
                    viewport: view.viewport,
                    selected_text: view.selection.clone(),
                    lines: matches!(event, "activate" | "page").then(|| Arc::clone(lines)),
                    lines_truncated: *truncated,
                },
            };
            if let Err(error) = active.session.send(request) {
                self.error = Some(error.into());
                return;
            }
            active.in_flight = true;
            active.view = Some(view);
            active.activated = true;
            active.sent = Instant::now();
        }
        self.plugins.visual = Some(active);
    }

    pub(in crate::app) fn paint_plugin(
        &self,
        painter: &egui::Painter,
        page: usize,
        page_rect: egui::Rect,
        dimensions: [f32; 2],
    ) {
        let Some(active) = &self.plugins.visual else {
            return;
        };
        let Some(scene) = &active.scene else { return };
        if active.document != self.id || active.page != Some(self.nav.page) {
            return;
        }
        let painter = painter.with_clip_rect(painter.clip_rect().intersect(page_rect));
        let overlay = scene.pages.iter().find(|overlay| overlay.page == page + 1);
        let map = |[x0, y0, x1, y1]: visual::Rect| {
            self.link_rect(
                hayro::kurbo::Rect::new(x0, y0, x1, y1),
                page_rect,
                dimensions,
            )
            .intersect(page_rect)
        };
        if let Some([r, g, b, a]) = scene.dim {
            let holes = overlay
                .map(|p| p.holes.iter().copied().map(map).collect::<Vec<_>>())
                .unwrap_or_default();
            for piece in visual::mask(page_rect.intersect(painter.clip_rect()), &holes) {
                painter.rect_filled(
                    piece,
                    0.0,
                    egui::Color32::from_rgba_unmultiplied(r, g, b, a),
                );
            }
        }
        if let Some(overlay) = overlay {
            for rectangle in &overlay.rectangles {
                let [r, g, b, a] = rectangle.color;
                painter.rect_filled(
                    map(rectangle.rect),
                    0.0,
                    egui::Color32::from_rgba_unmultiplied(r, g, b, a),
                );
            }
        }
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;

    fn reader(root: &std::path::Path) -> (Pagewing, egui::Context) {
        let directory = root.join("plugins/visual");
        std::fs::create_dir_all(&directory).unwrap();
        std::fs::write(
            directory.join("plugin.toml"),
            "api_version=2\nname='Visual test'\nenabled=true\ncommand=['/bin/sh','run.sh']",
        )
        .unwrap();
        std::fs::write(directory.join("run.sh"),
            "n=0\nwhile IFS= read -r line; do n=$((n+1)); printf '{\"id\":%s,\"scene\":{\"dim\":[0,0,0,180],\"pages\":[{\"page\":1,\"holes\":[[0,%s,600,%s]]}]},\"capture_keys\":[\"j\",\"k\"]}\\n' \"$n\" \"$((n*20))\" \"$((n*20+60))\"; done").unwrap();
        let context = egui::Context::default();
        let mut app = Pagewing::with_context(&context, None, None, root.join("library.json"));
        app.nav = crate::navigation::Navigation::new(6, 0);
        app.dimensions = vec![[600., 800.]; 6];
        app.reading.path = root.join("book.pdf");
        app.start_visual_plugin(0, &context);
        app.plugin_sync(&context);
        app.plugin_page_text(0, &crate::text::PageText::default());
        app.plugin_sync(&context);
        settle(&mut app, &context);
        (app, context)
    }

    fn settle(app: &mut Pagewing, context: &egui::Context) {
        let deadline = Instant::now() + Duration::from_secs(3);
        loop {
            app.visual_plugin_events(context);
            if app.plugins.visual.as_ref().is_none_or(|v| !v.in_flight) {
                return;
            }
            assert!(Instant::now() < deadline);
            std::thread::sleep(Duration::from_millis(5));
        }
    }

    fn key(
        app: &mut Pagewing,
        context: &egui::Context,
        key: egui::Key,
        modifiers: egui::Modifiers,
    ) {
        let _ = context.run(
            egui::RawInput {
                events: vec![egui::Event::Key {
                    key,
                    physical_key: None,
                    pressed: true,
                    repeat: false,
                    modifiers,
                }],
                ..Default::default()
            },
            |context| {
                app.keys(context);
                app.plugin_sync(context);
            },
        );
    }

    #[test]
    fn captured_reader_keys_move_overlay_and_escape_restores_reader() {
        let directory = tempfile::tempdir().unwrap();
        let (mut app, context) = reader(directory.path());
        let initial = app
            .plugins
            .visual
            .as_ref()
            .unwrap()
            .scene
            .as_ref()
            .unwrap()
            .pages[0]
            .holes[0];
        let pan = app.pan;
        key(&mut app, &context, egui::Key::J, egui::Modifiers::NONE);
        settle(&mut app, &context);
        let moved = app
            .plugins
            .visual
            .as_ref()
            .unwrap()
            .scene
            .as_ref()
            .unwrap()
            .pages[0]
            .holes[0];
        assert!(moved[1] > initial[1]);
        assert_eq!(app.pan, pan, "captured j must not also scroll the reader");
        key(&mut app, &context, egui::Key::Escape, egui::Modifiers::NONE);
        assert!(app.plugins.visual.is_none());
        assert!(!app.plugin_key(&context, egui::Key::J, egui::Modifiers::NONE));
    }

    #[test]
    fn typing_and_modified_keys_are_not_captured_and_reload_clears_visuals() {
        let directory = tempfile::tempdir().unwrap();
        let (mut app, context) = reader(directory.path());
        assert!(!app.plugin_key(&context, egui::Key::J, egui::Modifiers::SHIFT));
        app.search_open = true;
        app.search_focus = true;
        key(&mut app, &context, egui::Key::J, egui::Modifiers::NONE);
        assert!(app.plugins.visual.as_ref().unwrap().pending_keys.is_empty());
        assert!(!app.plugins.visual.as_ref().unwrap().in_flight);
        app.reload(&context);
        assert!(app.plugins.visual.is_none());
    }

    #[test]
    fn page_change_discards_inflight_scene_and_requests_fresh_text() {
        let directory = tempfile::tempdir().unwrap();
        let (mut app, context) = reader(directory.path());
        app.plugin_key(&context, egui::Key::J, egui::Modifiers::NONE);
        app.plugin_sync(&context);
        app.jump(1);
        app.plugin_sync(&context);
        assert!(app.plugins.visual.as_ref().unwrap().scene.is_none());
        assert!(app.plugins.visual.as_ref().unwrap().lines.is_none());
        settle(&mut app, &context);
        assert!(app.plugins.visual.as_ref().unwrap().scene.is_none());
        app.plugin_page_text(1, &crate::text::PageText::default());
        app.plugin_sync(&context);
        settle(&mut app, &context);
        assert!(app.plugins.visual.as_ref().unwrap().scene.is_some());
    }
}
