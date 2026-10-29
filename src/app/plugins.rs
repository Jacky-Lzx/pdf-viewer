use super::Pagewing;
use crate::plugins::{Job, Registry, Request, session, visual};
use eframe::egui;
use std::{
    collections::VecDeque,
    path::PathBuf,
    sync::Arc,
    time::{Duration, Instant},
};

mod visual_mode;

pub(super) struct PluginUi {
    root: PathBuf,
    registry: Registry,
    running: Option<(u64, Job)>,
    output: Option<String>,
    visual: Option<visual_mode::Visual>,
}

impl PluginUi {
    fn busy(&self) -> bool {
        self.running.is_some() || self.visual.is_some()
    }
    pub fn new(root: PathBuf) -> Self {
        let registry = Registry::discover(&root);
        Self {
            root,
            registry,
            running: None,
            output: None,
            visual: None,
        }
    }
}

impl Pagewing {
    pub(super) fn cancel_plugins(&mut self) {
        self.plugins.running = None;
        self.plugins.visual = None;
    }

    pub(super) fn plugin_menu(&mut self, ui: &mut egui::Ui) {
        let mut selected = None;
        ui.menu_button("Plugins", |ui| {
            ui.set_max_width(340.0);
            if self.plugins.busy() {
                ui.label(
                    self.plugins
                        .visual
                        .as_ref()
                        .map_or("Plugin running…", |v| v.name.as_str()),
                );
                if ui.button("Cancel plugin").clicked() {
                    self.cancel_plugins();
                    self.notify("Plugin cancelled");
                    ui.close();
                }
                ui.separator();
            }
            if self.plugins.registry.plugins.is_empty() {
                ui.label("No plugins installed");
            }
            egui::ScrollArea::vertical()
                .max_height(240.0)
                .show(ui, |ui| {
                    for (index, plugin) in self.plugins.registry.plugins.iter().enumerate() {
                        let enabled = plugin.enabled
                            && !self.plugins.busy()
                            && self.nav.count > 0
                            && !self.loading
                            && !self.reloading;
                        let label = if plugin.enabled {
                            plugin.name.clone()
                        } else {
                            format!("{} (disabled)", plugin.name)
                        };
                        if ui.add_enabled(enabled, egui::Button::new(label)).clicked() {
                            selected = Some(index);
                            ui.close();
                        }
                    }
                    for error in &self.plugins.registry.errors {
                        ui.colored_label(ui.visuals().error_fg_color, error);
                    }
                });
            ui.separator();
            if ui
                .add_enabled(!self.plugins.busy(), egui::Button::new("Reload plugins"))
                .clicked()
            {
                self.plugins.registry = Registry::discover(&self.plugins.root);
                ui.close();
            }
            if ui.button("Copy plugins folder").clicked() {
                ui.ctx().copy_text(self.plugins.root.display().to_string());
                ui.close();
            }
        });
        if let Some(index) = selected {
            self.start_plugin_with_context(index, ui.ctx());
            ui.ctx().request_repaint();
        }
    }

    #[cfg(test)]
    fn start_plugin(&mut self, index: usize) {
        self.start_plugin_with_context(index, &egui::Context::default());
    }

    fn start_plugin_with_context(&mut self, index: usize, context: &egui::Context) {
        if self.plugins.busy() {
            return;
        }
        if self.plugins.registry.plugins[index].api_version == 2 {
            self.start_visual_plugin(index, context);
            return;
        }
        let request = Request {
            api_version: 1,
            document: self.reading.path.clone(),
            page: self.nav.page + 1,
            page_count: self.nav.count,
            selected_text: self
                .selection
                .selected_text()
                .unwrap_or_default()
                .to_owned(),
        };
        match self.plugins.registry.plugins[index].start(request) {
            Ok(job) => self.plugins.running = Some((self.id, job)),
            Err(error) => self.error = Some(error.into()),
        }
    }

    pub(super) fn plugin_events(&mut self, context: &egui::Context) {
        self.visual_plugin_events(context);
        let Some((document_id, job)) = &self.plugins.running else {
            return;
        };
        // A response from a closed/replaced/reloaded document must have no effects.
        if *document_id != self.id || self.loading || self.reloading {
            self.plugins.running = None;
            return;
        }
        let Some(result) = job.poll() else {
            context.request_repaint_after(Duration::from_millis(50));
            return;
        };
        self.plugins.running = None;
        match result {
            Ok(response) => {
                // Validate all actions before applying any of them.
                if let Err(error) = response.validate(self.nav.count) {
                    self.error = Some(error.into());
                    return;
                }
                if let Some(page) = response.goto_page {
                    self.jump(page - 1);
                }
                if let Some(text) = response.copy_text {
                    context.copy_text(text);
                    self.notify("Plugin text copied");
                }
                if let Some(message) = response.message {
                    self.plugins.output = Some(message);
                }
            }
            Err(error) => self.error = Some(error.into()),
        }
    }

    pub(super) fn plugin_output(&mut self, context: &egui::Context) {
        let Some(text) = &self.plugins.output else {
            return;
        };
        let mut open = true;
        egui::Window::new("Plugin output")
            .open(&mut open)
            .default_width(440.0)
            .show(context, |ui| {
                egui::ScrollArea::vertical()
                    .max_height(360.0)
                    .show(ui, |ui| {
                        ui.add(egui::Label::new(text).wrap().selectable(true));
                    });
                if ui.button("Copy output").clicked() {
                    context.copy_text(text.clone());
                }
            });
        if !open {
            self.plugins.output = None;
        }
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::time::Instant;

    fn reader(root: &std::path::Path, response: &str) -> (Pagewing, egui::Context) {
        let directory = root.join("plugins/test");
        std::fs::create_dir_all(&directory).unwrap();
        std::fs::write(
            directory.join("plugin.toml"),
            "api_version = 1\nname = 'Test'\nenabled = true\ncommand = ['/bin/sh', 'run.sh']",
        )
        .unwrap();
        std::fs::write(
            directory.join("run.sh"),
            format!("cat > /dev/null\nprintf '%s' '{response}'"),
        )
        .unwrap();
        let context = egui::Context::default();
        let mut app = Pagewing::with_context(&context, None, None, root.join("library.json"));
        app.nav = crate::navigation::Navigation::new(6, 0);
        app.dimensions = vec![[600.0, 800.0]; 6];
        app.reading.path = root.join("book.pdf");
        (app, context)
    }

    fn finish(app: &mut Pagewing, context: &egui::Context) -> Vec<egui::OutputCommand> {
        let deadline = Instant::now() + Duration::from_secs(3);
        let mut commands = Vec::new();
        while app.plugins.running.is_some() {
            let output = context.run(egui::RawInput::default(), |context| {
                app.plugin_events(context)
            });
            commands.extend(output.platform_output.commands);
            assert!(Instant::now() < deadline);
            std::thread::sleep(Duration::from_millis(10));
        }
        commands
    }

    #[test]
    fn applies_plugin_actions_through_reader_navigation_and_clipboard() {
        let directory = tempfile::tempdir().unwrap();
        let (mut app, context) = reader(
            directory.path(),
            r#"{"goto_page":3,"copy_text":"reference","message":"Done"}"#,
        );
        app.start_plugin(0);
        let commands = finish(&mut app, &context);
        assert_eq!(app.nav.page, 2);
        assert_eq!(app.plugins.output.as_deref(), Some("Done"));
        assert!(commands.iter().any(
            |command| matches!(command, egui::OutputCommand::CopyText(text) if text == "reference")
        ));
    }

    #[test]
    fn invalid_response_has_no_partial_effects_and_closed_document_cancels_job() {
        let directory = tempfile::tempdir().unwrap();
        let (mut app, context) = reader(
            directory.path(),
            r#"{"goto_page":99,"copy_text":"must not copy","message":"must not show"}"#,
        );
        app.start_plugin(0);
        let commands = finish(&mut app, &context);
        assert_eq!(app.nav.page, 0);
        assert!(app.plugins.output.is_none());
        assert!(
            !commands
                .iter()
                .any(|command| matches!(command, egui::OutputCommand::CopyText(_)))
        );
        assert!(
            app.error
                .as_ref()
                .unwrap()
                .message
                .contains("outside the document")
        );
        app.start_plugin(0);
        app.clear_document(&context);
        assert!(app.plugins.running.is_none());
        assert!(finish(&mut app, &context).is_empty());
    }

    #[test]
    fn stale_document_response_is_discarded() {
        let directory = tempfile::tempdir().unwrap();
        let (mut app, context) =
            reader(directory.path(), r#"{"message":"obsolete","goto_page":5}"#);
        app.start_plugin(0);
        app.id += 1;
        finish(&mut app, &context);
        assert_eq!(app.nav.page, 0);
        assert!(app.plugins.output.is_none());
    }
}
