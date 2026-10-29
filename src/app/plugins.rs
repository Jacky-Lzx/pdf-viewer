use super::Pagewing;
use crate::plugins::{Job, Registry, Request, management, session, visual};
use eframe::egui;
use std::{
    collections::VecDeque,
    path::PathBuf,
    sync::Arc,
    time::{Duration, Instant},
};

mod manager;
mod visual_mode;

pub(super) struct PluginUi {
    root: PathBuf,
    registry: Registry,
    running: Option<(u64, Job)>,
    output: Option<String>,
    visual: Option<visual_mode::Visual>,
    preferences: management::Preferences,
    preferences_error: bool,
    restore_attempted: bool,
    tool_id: Option<String>,
    manager: manager::Manager,
}

impl PluginUi {
    fn reload(&mut self) {
        self.registry = Registry::discover(&self.root);
        match management::Preferences::load(&self.root) {
            Ok(preferences) => {
                self.preferences = preferences;
                self.preferences_error = false;
            }
            Err(error) => {
                self.registry
                    .errors
                    .push(format!("Cannot load plugin preferences: {error}"));
                self.preferences_error = true;
            }
        }
        self.preferences.apply(&mut self.registry);
        self.manager.runtime = self
            .registry
            .plugins
            .iter()
            .map(|p| (p.id.clone(), p.executable()))
            .collect();
    }

    fn change_preferences(
        &mut self,
        change: impl FnOnce(&mut management::Preferences),
    ) -> Result<(), String> {
        if self.preferences_error {
            return Err("Cannot save while plugins.json is invalid. Open the plugins folder and repair the preferences file in its parent folder.".into());
        }
        let mut next = management::Preferences::load(&self.root)?;
        change(&mut next);
        next.save(&self.root)?;
        self.preferences = next;
        self.reload();
        Ok(())
    }

    pub fn new(root: PathBuf) -> Self {
        let mut state = Self {
            root,
            registry: Registry::default(),
            running: None,
            output: None,
            visual: None,
            preferences: management::Preferences::default(),
            preferences_error: false,
            restore_attempted: false,
            tool_id: None,
            manager: manager::Manager::default(),
        };
        state.reload();
        state
    }
}

impl Pagewing {
    pub(super) fn suspend_plugins(&mut self) {
        self.plugins.running = None;
        self.plugins.tool_id = None;
        self.plugins.visual = None;
        self.plugins.restore_attempted = false;
        self.plugins.reload();
    }

    fn stop_visual_plugin(&mut self) {
        self.plugins.visual = None;
        self.plugins.restore_attempted = true;
        if let Err(error) = self
            .plugins
            .change_preferences(|prefs| prefs.last_visual = None)
        {
            self.error = Some(error.into());
        }
    }

    fn can_run_plugin(&self, plugin: &crate::plugins::Plugin) -> bool {
        plugin.enabled
            && self
                .plugins
                .manager
                .runtime
                .get(&plugin.id)
                .is_some_and(Result::is_ok)
            && self.nav.count > 0
            && !self.loading
            && !self.reloading
            && if plugin.api_version == 2 {
                self.plugins.visual.is_none()
            } else {
                self.plugins.running.is_none()
            }
    }

    pub(super) fn plugin_menu(&mut self, ui: &mut egui::Ui) {
        let mut selected = None;
        let mut stop_visual = false;
        let mut stop_tool = false;
        ui.menu_button("Plugins", |ui| {
            ui.set_max_width(340.0);
            if let Some(active) = &self.plugins.visual {
                ui.label(format!("Active: {}", active.name));
                stop_visual = ui.button("Stop visual mode · Esc").clicked();
                ui.separator();
            }
            if self.plugins.running.is_some() {
                ui.label("Tool running…");
                stop_tool = ui.button("Cancel tool").clicked();
                ui.separator();
            }
            if self.plugins.registry.plugins.is_empty() {
                ui.label("No plugins installed");
            }
            egui::ScrollArea::vertical()
                .max_height(240.0)
                .show(ui, |ui| {
                    for (index, plugin) in self.plugins.registry.plugins.iter().enumerate() {
                        if plugin.enabled
                            && ui
                                .add_enabled(
                                    self.can_run_plugin(plugin),
                                    egui::Button::new(&plugin.name),
                                )
                                .clicked()
                        {
                            selected = Some(index);
                            ui.close();
                        }
                    }
                });
            ui.separator();
            if ui.button("Manage plugins…").clicked() {
                self.plugins.manager.open = true;
                ui.close();
            }
        });
        if stop_visual {
            self.stop_visual_plugin();
        }
        if stop_tool {
            self.plugins.running = None;
            self.plugins.tool_id = None;
        }
        if let Some(index) = selected {
            self.start_plugin_with_context(index, ui.ctx());
            ui.ctx().request_repaint();
        }
        if let Some(active) = &self.plugins.visual {
            ui.weak(format!("{} · Esc to exit", active.name));
        }
        if self.plugins.running.is_some() {
            ui.spinner();
        }
    }

    #[cfg(test)]
    fn start_plugin(&mut self, index: usize) {
        self.start_plugin_with_context(index, &egui::Context::default());
    }

    fn start_plugin_with_context(&mut self, index: usize, context: &egui::Context) {
        let plugin = &self.plugins.registry.plugins[index];
        if !self.can_run_plugin(plugin) {
            return;
        }
        if plugin.api_version == 2 {
            let id = plugin.id.clone();
            self.start_visual_plugin(index, context);
            self.plugins.restore_attempted = true;
            if self.plugins.visual.is_some()
                && let Err(error) = self.plugins.change_preferences(|prefs| {
                    prefs.last_visual = prefs.remember_visual.then_some(id)
                })
            {
                self.error = Some(error.into());
            }
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
            Ok(job) => {
                self.plugins.tool_id = Some(self.plugins.registry.plugins[index].id.clone());
                self.plugins.running = Some((self.id, job));
            }
            Err(error) => self.error = Some(error.into()),
        }
    }

    pub(super) fn plugin_events(&mut self, context: &egui::Context) {
        self.plugin_manager_events(context);
        self.visual_plugin_events(context);
        self.restore_visual_plugin(context);
        let Some((document_id, job)) = &self.plugins.running else {
            return;
        };
        // A response from a closed/replaced/reloaded document must have no effects.
        if *document_id != self.id || self.loading || self.reloading {
            self.plugins.running = None;
            self.plugins.tool_id = None;
            return;
        }
        let Some(result) = job.poll() else {
            context.request_repaint_after(Duration::from_millis(50));
            return;
        };
        self.plugins.running = None;
        self.plugins.tool_id = None;
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
        self.plugin_manager_window(context);
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
