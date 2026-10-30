use super::*;
use crate::plugins::management::{Kind, Removed};
use std::{
    collections::BTreeMap,
    path::Path,
    sync::mpsc::{self, Receiver, TryRecvError},
};

#[derive(Default)]
pub(super) struct Manager {
    pub open: bool,
    pub runtime: BTreeMap<String, Result<PathBuf, String>>,
    editor: Option<Editor>,
    operation: Option<Receiver<Result<Outcome, String>>>,
    removed: Option<Removed>,
    message: Option<(String, bool)>,
}

struct Editor {
    id: String,
    executable: String,
    values: BTreeMap<String, serde_json::Value>,
}
enum Outcome {
    Installed(String),
    Removed(Removed),
    Restored,
    Program(Option<String>),
    Cancelled,
}
enum Action {
    Run(usize),
    Enable(String, bool),
    StopVisual,
    StopTool,
    Edit(usize),
    Remove(String),
    Folder(PathBuf),
}

impl Pagewing {
    pub(in crate::app) fn plugin_manager_open(&self) -> bool {
        self.plugins.manager.open
    }
    pub(in crate::app) fn close_plugin_manager(&mut self) {
        if self.plugins.manager.editor.take().is_none() {
            self.plugins.manager.open = false;
        }
    }

    pub(super) fn restore_visual_plugin(&mut self, context: &egui::Context) {
        if self.plugins.visual.is_some()
            || self.plugins.restore_attempted
            || self.plugins.preferences_error
            || !self.plugins.preferences.remember_visual
            || self.nav.count == 0
            || self.loading
            || self.reloading
        {
            return;
        }
        let Some(id) = self.plugins.preferences.last_visual.clone() else {
            return;
        };
        self.plugins.restore_attempted = true;
        if let Some(index) = self
            .plugins
            .registry
            .plugins
            .iter()
            .position(|p| p.id == id && p.api_version == 2 && self.can_run_plugin(p))
        {
            self.start_visual_plugin(index, context);
        }
    }

    fn plugin_feedback(&mut self, result: Result<String, String>) {
        self.plugins.manager.message = Some(match result {
            Ok(message) => (message, false),
            Err(error) => (error, true),
        });
    }

    fn plugin_operation(
        &mut self,
        context: &egui::Context,
        operation: impl FnOnce() -> Result<Outcome, String> + Send + 'static,
    ) {
        if self.plugins.manager.operation.is_some() {
            return;
        }
        let (send, receive) = mpsc::sync_channel(1);
        let wake = context.clone();
        self.plugins.manager.operation = Some(receive);
        std::thread::spawn(move || {
            let _ = send.send(operation());
            wake.request_repaint();
        });
    }

    pub(super) fn plugin_manager_events(&mut self, _: &egui::Context) {
        let Some(receive) = &self.plugins.manager.operation else {
            return;
        };
        let result = match receive.try_recv() {
            Ok(result) => result,
            Err(TryRecvError::Empty) => return,
            Err(TryRecvError::Disconnected) => Err("Plugin operation stopped unexpectedly".into()),
        };
        self.plugins.manager.operation = None;
        match result {
            Ok(Outcome::Installed(id)) => {
                let result = self.plugins.change_preferences(|prefs| {
                    prefs.plugins.entry(id).or_default().enabled = Some(false);
                });
                self.plugins.reload();
                self.plugin_feedback(
                    result.map(|()| {
                        "Installed. Enable the plugin when you are ready to use it.".into()
                    }),
                );
            }
            Ok(Outcome::Removed(removed)) => {
                self.plugins.manager.removed = Some(removed);
                self.plugins.reload();
                self.plugin_feedback(Ok("Plugin removed. Undo is available below.".into()));
            }
            Ok(Outcome::Restored) => {
                self.plugins.manager.removed = None;
                self.plugins.reload();
                self.plugin_feedback(Ok("Plugin restored. Enable it to use it again.".into()));
            }
            Ok(Outcome::Program(path)) => {
                if let (Some(editor), Some(path)) = (&mut self.plugins.manager.editor, path) {
                    editor.executable = path;
                }
            }
            Ok(Outcome::Cancelled) => {}
            Err(error) => self.plugin_feedback(Err(error)),
        }
    }

    fn plugin_action(&mut self, action: Action, context: &egui::Context) {
        match action {
            Action::Run(index) => {
                self.start_plugin_with_context(index, context);
                // Run returns to the reader so its plugin keys immediately work.
                self.plugins.manager.open = false;
                self.plugins.manager.editor = None;
            }
            Action::Enable(id, enabled) => {
                let result = self.plugins.change_preferences(|prefs| {
                    prefs.plugins.entry(id.clone()).or_default().enabled = Some(enabled);
                    if !enabled && prefs.last_visual.as_deref() == Some(&id) {
                        prefs.last_visual = None;
                    }
                });
                if result.is_ok() && !enabled {
                    if self.plugins.visual.as_ref().is_some_and(|p| p.id == id) {
                        self.plugins.visual = None;
                    }
                    if self.plugins.tool_id.as_deref() == Some(&id) {
                        self.plugins.running = None;
                        self.plugins.tool_id = None;
                    }
                }
                if let Err(error) = result {
                    self.plugin_feedback(Err(error));
                }
            }
            Action::StopVisual => self.stop_visual_plugin(),
            Action::StopTool => {
                self.plugins.running = None;
                self.plugins.tool_id = None;
            }
            Action::Edit(index) => {
                let plugin = &self.plugins.registry.plugins[index];
                let executable = self
                    .plugins
                    .preferences
                    .plugins
                    .get(&plugin.id)
                    .and_then(|p| p.executable.clone())
                    .unwrap_or_default();
                self.plugins.manager.editor = Some(Editor {
                    id: plugin.id.clone(),
                    executable,
                    values: plugin.settings.clone(),
                });
            }
            Action::Remove(id) => {
                // Stop only the plugin being removed; another plugin can continue.
                let result = self.plugins.change_preferences(|prefs| {
                    prefs.plugins.entry(id.clone()).or_default().enabled = Some(false);
                    if prefs.last_visual.as_deref() == Some(&id) {
                        prefs.last_visual = None;
                    }
                });
                if let Err(error) = result {
                    self.plugin_feedback(Err(error));
                    return;
                }
                if self.plugins.visual.as_ref().is_some_and(|p| p.id == id) {
                    self.plugins.visual = None;
                }
                if self.plugins.tool_id.as_deref() == Some(&id) {
                    self.plugins.running = None;
                    self.plugins.tool_id = None;
                }
                let root = self.plugins.root.clone();
                self.plugin_operation(context, move || {
                    management::remove(&root, &id).map(Outcome::Removed)
                });
            }
            Action::Folder(path) => {
                self.plugin_feedback(open_folder(&path).map(|()| "Folder opened".into()))
            }
        }
    }

    pub(super) fn plugin_manager_window(&mut self, context: &egui::Context) {
        if !self.plugins.manager.open {
            return;
        }
        let mut open = true;
        let mut actions = Vec::new();
        let mut install = false;
        let mut example = None;
        let mut undo = false;
        let mut refresh = false;
        let operation_busy = self.plugins.manager.operation.is_some();
        let size = context.content_rect().size();
        egui::Window::new("Manage plugins").id(egui::Id::new("pagewing-plugin-manager"))
            .open(&mut open).default_width(620.0).vscroll(true).max_height((size.y-24.0).max(120.0)).max_width((size.x-24.0).max(180.0)).show(context,|ui| {
            ui.label("Install, enable and configure your reading tools.");
            ui.weak("Only enable plugins you trust. They run with your user permissions.");
            ui.add_enabled_ui(!operation_busy,|ui| ui.horizontal_wrapped(|ui| {
                install = ui.button("Install from folder…").clicked();
                if ui.button("Add focus reader").clicked() { example = Some(true); }
                if ui.button("Add reference tool").clicked() { example = Some(false); }
                refresh = ui.button("Refresh").clicked();
                if ui.button("Open plugins folder").clicked() { actions.push(Action::Folder(self.plugins.root.clone())); }
            }));
            if operation_busy { ui.horizontal(|ui| { ui.spinner(); ui.label("Working…"); }); }
            let mut remember = self.plugins.preferences.remember_visual;
            if ui.checkbox(&mut remember,"Remember visual mode across documents and reloads").changed() {
                let active = self.plugins.visual.as_ref().map(|p|p.id.clone());
                let result = self.plugins.change_preferences(|prefs| { prefs.remember_visual = remember; prefs.last_visual = if remember { active } else { None }; });
                if let Err(error) = result { self.plugin_feedback(Err(error)); }
            }
            ui.separator();
            egui::ScrollArea::vertical().id_salt("plugin-manager-list").max_height((size.y-270.0).max(100.0)).show(ui,|ui| {
                if self.plugins.registry.plugins.is_empty() { ui.label("No plugins installed. Add an example above or choose a plugin folder."); }
                for (index,plugin) in self.plugins.registry.plugins.iter().enumerate() {
                    let visual_running = self.plugins.visual.as_ref().is_some_and(|v|v.id == plugin.id);
                    let tool_running = self.plugins.tool_id.as_deref() == Some(&plugin.id) && self.plugins.running.is_some();
                    let runtime = self.plugins.manager.runtime.get(&plugin.id);
                    ui.push_id(&plugin.id,|ui| ui.group(|ui| {
                        ui.set_min_width(ui.available_width());
                        ui.horizontal_wrapped(|ui| {
                            ui.strong(&plugin.name);
                            if !plugin.version.is_empty() { ui.weak(&plugin.version); }
                            ui.weak(if plugin.api_version == 2 { "Visual mode" } else { "Tool" });
                            if visual_running || tool_running { ui.label(egui::RichText::new("Running").color(ui.visuals().selection.stroke.color)); }
                            let mut enabled = plugin.enabled;
                            if ui.add_enabled(!operation_busy,egui::Checkbox::new(&mut enabled,"Enabled")).changed() { actions.push(Action::Enable(plugin.id.clone(),enabled)); }
                        });
                        if !plugin.description.is_empty() { ui.label(&plugin.description); }
                        if !plugin.author.is_empty() { ui.weak(format!("By {}",plugin.author)); }
                        if let Some(Err(error)) = runtime { ui.colored_label(ui.visuals().error_fg_color,error); }
                        else { ui.weak(if plugin.enabled { "Ready" } else { "Disabled" }); }
                        ui.add_enabled_ui(!operation_busy,|ui| ui.horizontal_wrapped(|ui| {
                            if visual_running {
                                if ui.button("Stop").clicked() { actions.push(Action::StopVisual); }
                            } else if tool_running {
                                if ui.button("Cancel").clicked() { actions.push(Action::StopTool); }
                            } else {
                                let ready = self.can_run_plugin(plugin);
                                if ui.add_enabled(ready,egui::Button::new("Run")).clicked() { actions.push(Action::Run(index)); }
                                if self.nav.count == 0 { ui.weak("Open a PDF to run"); }
                                else if plugin.enabled && runtime.is_some_and(Result::is_ok) && !ready { ui.weak("Another plugin of this type is running"); }
                            }
                            if ui.button("Settings…").clicked() { actions.push(Action::Edit(index)); }
                            if ui.button("Open folder").clicked() { actions.push(Action::Folder(plugin.directory().to_path_buf())); }
                            if ui.button("Remove").clicked() { actions.push(Action::Remove(plugin.id.clone())); }
                        }));
                    }));
                    ui.add_space(6.0);
                }
                for error in &self.plugins.registry.errors { ui.colored_label(ui.visuals().error_fg_color,error); }
            });
            if let Some((message,error)) = &self.plugins.manager.message {
                ui.separator();
                ui.colored_label(if *error { ui.visuals().error_fg_color } else { ui.visuals().text_color() },message);
            }
            if let Some(removed) = &self.plugins.manager.removed {
                ui.horizontal_wrapped(|ui| {
                    ui.label(format!("Removed {}",removed.id));
                    undo = ui.add_enabled(!operation_busy,egui::Button::new("Undo removal")).clicked();
                    if ui.button("Open removed folder").clicked() { actions.push(Action::Folder(removed.path.clone())); }
                });
            }
        });
        if !open {
            self.plugins.manager.open = false;
            self.plugins.manager.editor = None;
        }
        for action in actions {
            self.plugin_action(action, context);
        }
        if refresh {
            let active = self.plugins.visual.as_ref().map(|v| v.id.clone());
            self.suspend_plugins();
            self.plugins.reload();
            self.plugins.restore_attempted = true;
            if let Some(index) = self
                .plugins
                .registry
                .plugins
                .iter()
                .position(|p| Some(&p.id) == active.as_ref() && self.can_run_plugin(p))
            {
                self.start_visual_plugin(index, context);
            }
        }
        if install {
            let root = self.plugins.root.clone();
            self.plugin_operation(context, move || {
                match rfd::FileDialog::new()
                    .set_title("Choose a folder containing plugin.toml")
                    .pick_folder()
                {
                    Some(source) => management::install(&root, &source).map(Outcome::Installed),
                    None => Ok(Outcome::Cancelled),
                }
            });
        }
        if let Some(focus) = example {
            let root = self.plugins.root.clone();
            self.plugin_operation(context, move || {
                management::install_example(&root, focus).map(Outcome::Installed)
            });
        }
        if undo && let Some(removed) = self.plugins.manager.removed.clone() {
            let root = self.plugins.root.clone();
            self.plugin_operation(context, move || {
                management::restore(&root, &removed).map(|()| Outcome::Restored)
            });
        }
        self.plugin_settings_window(context);
    }

    fn save_plugin_settings(
        &mut self,
        editor: &Editor,
        context: &egui::Context,
    ) -> Result<(), String> {
        let plugin = self
            .plugins
            .registry
            .plugins
            .iter()
            .find(|p| p.id == editor.id)
            .ok_or("Plugin is no longer installed")?;
        if editor.executable.contains('\0')
            || editor.executable.len() > 4096
            || plugin
                .settings_schema
                .iter()
                .any(|s| editor.values.get(&s.key).is_none_or(|v| !s.valid(v)))
        {
            return Err("Please enter valid plugin settings".into());
        }
        let active = self
            .plugins
            .visual
            .as_ref()
            .is_some_and(|v| v.id == editor.id);
        let id = editor.id.clone();
        self.plugins.change_preferences(|prefs| {
            let options = prefs.plugins.entry(id.clone()).or_default();
            options.executable = (!editor.executable.trim().is_empty())
                .then(|| editor.executable.trim().to_string());
            options.settings = editor.values.clone();
        })?;
        if active {
            self.plugins.visual = None;
            if let Some(index) = self
                .plugins
                .registry
                .plugins
                .iter()
                .position(|p| p.id == id && self.can_run_plugin(p))
            {
                self.start_visual_plugin(index, context);
            }
        }
        Ok(())
    }

    fn plugin_settings_window(&mut self, context: &egui::Context) {
        let Some(mut editor) = self.plugins.manager.editor.take() else {
            return;
        };
        let Some(plugin) = self
            .plugins
            .registry
            .plugins
            .iter()
            .find(|p| p.id == editor.id)
        else {
            return;
        };
        let name = plugin.name.clone();
        let mut open = true;
        let mut save = false;
        let mut browse = false;
        let mut reset = false;
        egui::Window::new(format!("Settings · {name}")).id(egui::Id::new("plugin-settings"))
            .open(&mut open).default_width(440.0).vscroll(true).max_height((context.content_rect().height()-24.0).max(120.0)).max_width((context.content_rect().width()-24.0).max(180.0)).show(context,|ui| {
            egui::ScrollArea::vertical().max_height((context.content_rect().height()-160.0).max(80.0)).show(ui,|ui| {
                for setting in &plugin.settings_schema {
                    let value = editor.values.entry(setting.key.clone()).or_insert_with(||setting.default.clone());
                    match setting.kind {
                        Kind::Boolean => { let mut v = value.as_bool().unwrap_or(false); if ui.checkbox(&mut v,&setting.label).changed() { *value = v.into(); } }
                        Kind::Text => { let mut v = value.as_str().unwrap_or_default().to_string(); ui.label(&setting.label); if ui.add(egui::TextEdit::singleline(&mut v).char_limit(1024)).changed() { *value = v.into(); } }
                        Kind::Integer => {
                            let mut v = value.as_i64().unwrap_or_default();
                            ui.horizontal_wrapped(|ui| { ui.label(&setting.label); if ui.add(egui::DragValue::new(&mut v).range(setting.min.unwrap_or(i64::MIN as f64) as i64..=setting.max.unwrap_or(i64::MAX as f64) as i64)).changed() { *value = v.into(); } });
                        }
                        Kind::Number => {
                            let mut v = value.as_f64().unwrap_or_default();
                            ui.horizontal_wrapped(|ui| { ui.label(&setting.label); if ui.add(egui::DragValue::new(&mut v).speed(0.01).range(setting.min.unwrap_or(f64::MIN)..=setting.max.unwrap_or(f64::MAX))).changed() { *value = v.into(); } });
                        }
                    }
                    ui.add_space(6.0);
                }
                if plugin.settings_schema.is_empty() { ui.weak("This plugin has no adjustable options."); }
                ui.separator();
                ui.label("Program location (optional)");
                ui.weak(format!("Plugin command: {}",plugin.program()));
                ui.add(egui::TextEdit::singleline(&mut editor.executable).hint_text("Automatic detection").char_limit(4096));
                ui.horizontal_wrapped(|ui| { browse = ui.add_enabled(self.plugins.manager.operation.is_none(),egui::Button::new("Choose program…")).clicked(); reset = ui.button("Reset to defaults").clicked(); });
                ui.weak("Choose Python or the executable this plugin needs. No dependencies are downloaded automatically.");
                if let Some(Ok(path)) = self.plugins.manager.runtime.get(&plugin.id) { ui.weak(format!("Detected: {}",path.display())); }
            });
            ui.separator();
            save = ui.add_enabled(self.plugins.manager.operation.is_none(),egui::Button::new("Save settings")).clicked();
            ui.weak("Settings are saved separately. An active visual mode restarts to apply changes.");
        });
        if reset {
            editor.executable.clear();
            editor.values = plugin
                .settings_schema
                .iter()
                .map(|s| (s.key.clone(), s.default.clone()))
                .collect();
        }
        if save {
            let result = self.save_plugin_settings(&editor, context);
            if result.is_ok() {
                open = false;
            }
            self.plugin_feedback(result.map(|()| "Plugin settings saved".into()));
        }
        if open {
            self.plugins.manager.editor = Some(editor);
        }
        if browse {
            self.plugin_operation(context, || {
                Ok(Outcome::Program(
                    rfd::FileDialog::new()
                        .set_title("Choose the plugin's program or interpreter")
                        .pick_file()
                        .map(|p| p.to_string_lossy().into_owned()),
                ))
            });
        }
    }
}

fn open_folder(path: &Path) -> Result<(), String> {
    std::fs::create_dir_all(path).map_err(|e| e.to_string())?;
    let program = if cfg!(target_os = "macos") {
        "open"
    } else {
        "xdg-open"
    };
    let mut child = std::process::Command::new(program)
        .arg(path)
        .spawn()
        .map_err(|e| e.to_string())?;
    std::thread::spawn(move || {
        let _ = child.wait();
    });
    Ok(())
}

#[cfg(all(test, unix))]
mod tests;
