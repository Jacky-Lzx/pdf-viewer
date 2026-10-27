use super::{Pagewing, set_theme, shortcuts};
use crate::{
    navigation::Fit,
    settings::{Binding, DisplaySettings, Layout, Shortcuts, commands},
};
use eframe::egui::{self, Key};

#[derive(Clone, Copy, Default, PartialEq, Eq)]
enum Tab {
    #[default]
    Display,
    Keyboard,
}

pub(super) struct SettingsUi {
    display: DisplaySettings,
    shortcuts: Shortcuts,
    dark: bool,
    night: bool,
    sidebar: bool,
    thumbnails: bool,
    apply_current: bool,
    tab: Tab,
    filter: String,
    recording: Option<&'static str>,
    error: Option<String>,
}

impl SettingsUi {
    fn assign(&mut self, id: &str, binding: Binding) -> Result<(), String> {
        if binding.reserved() {
            return Err(format!(
                "{} is reserved for text editing or reader sequences.",
                binding.label()
            ));
        }
        if let Some(command) = commands()
            .iter()
            .find(|command| command.id != id && self.shortcuts.bindings(command).contains(&binding))
        {
            return Err(format!(
                "{} is already assigned to {}. Remove that binding first.",
                binding.label(),
                command.label
            ));
        }
        let command = commands()
            .iter()
            .find(|command| command.id == id)
            .expect("command");
        let mut bindings = self.shortcuts.bindings(command).to_vec();
        if !bindings.contains(&binding) {
            bindings.push(binding);
        }
        self.shortcuts.overrides.insert(id.into(), bindings);
        Ok(())
    }
}

impl Pagewing {
    pub(super) fn open_settings(&mut self, context: &egui::Context) {
        egui::Popup::close_all(context);
        self.outline.cancel_input();
        self.hints.cancel();
        self.mark_action = None;
        self.nav.clear_prefix();
        self.help = false;
        self.properties_open = false;
        self.recent_picker = None;
        self.goto_open = false;
        self.search_focus = false;
        self.password_focus = false;
        self.comments.focus = false;
        context.memory_mut(|memory| {
            if let Some(id) = memory.focused() {
                memory.surrender_focus(id);
            }
            memory.move_focus(egui::FocusDirection::None);
        });
        self.settings = Some(SettingsUi {
            display: self.library.display.clone(),
            shortcuts: self.library.shortcuts.clone(),
            dark: self.library.dark,
            night: self.library.night,
            sidebar: self.library.sidebar,
            thumbnails: self.library.thumbnails,
            apply_current: false,
            tab: Tab::Display,
            filter: String::new(),
            recording: None,
            error: None,
        });
    }

    pub(super) fn settings_keys(&mut self, context: &egui::Context) {
        for (event, text) in
            context.input(|input| shortcuts::reader_actions(input).collect::<Vec<_>>())
        {
            let egui::Event::Key {
                key,
                modifiers,
                repeat,
                ..
            } = event
            else {
                continue;
            };
            let Some(settings) = self.settings.as_mut() else {
                break;
            };
            if let Some(id) = settings.recording {
                shortcuts::consume_reader_shortcut(context, key, modifiers);
                if key == Key::Escape {
                    settings.recording = None;
                    settings.error = None;
                } else if !repeat {
                    match settings.assign(id, Binding::from_event(key, modifiers, text.character()))
                    {
                        Ok(()) => {
                            settings.recording = None;
                            settings.error = None;
                        }
                        Err(error) => settings.error = Some(error),
                    }
                }
            } else if key == Key::Escape {
                shortcuts::consume_reader_shortcut(context, key, modifiers);
                if egui::Popup::is_any_open(context) {
                    egui::Popup::close_all(context);
                } else {
                    self.settings = None;
                }
            }
        }
    }

    fn apply_settings(&mut self, settings: SettingsUi, context: &egui::Context) {
        self.library.display = settings.display;
        self.library.shortcuts = settings.shortcuts;
        self.library.dark = settings.dark;
        self.library.night = settings.night;
        self.library.sidebar = settings.sidebar;
        self.library.thumbnails = settings.thumbnails;
        set_theme(context, self.library.dark);
        if settings.apply_current && !self.reading.path.as_os_str().is_empty() {
            self.library.display.apply(&mut self.reading);
            self.set_display_mode(self.reading.continuous, self.reading.facing);
            self.pending_offset = Some([0.0; 2]);
            self.pending_destination = None;
            self.pending_focus = None;
            self.pan = egui::Vec2::ZERO;
        }
        self.reveal_page = Some(self.nav.page);
        self.save();
    }

    pub(super) fn settings_window(&mut self, context: &egui::Context) {
        let Some(mut settings) = self.settings.take() else {
            return;
        };
        let mut save = false;
        let mut cancel = false;
        egui::Modal::new(egui::Id::new("pagewing-settings")).show(context, |ui| {
            let width = (context.content_rect().width() - 48.0).clamp(160.0, 680.0);
            ui.set_width(width);
            ui.heading("Settings");
            ui.horizontal_wrapped(|ui| {
                if ui
                    .selectable_value(&mut settings.tab, Tab::Display, "Display")
                    .clicked()
                {
                    settings.recording = None;
                }
                ui.selectable_value(&mut settings.tab, Tab::Keyboard, "Keyboard shortcuts");
            });
            ui.separator();
            egui::ScrollArea::vertical()
                .id_salt("pagewing-settings-content")
                .max_height((context.content_rect().height() - 200.0).max(32.0))
                .show(ui, |ui| {
                    if let Some(error) = &settings.error {
                        ui.colored_label(ui.visuals().error_fg_color, error);
                    }
                    match settings.tab {
                        Tab::Display => display_settings(ui, &mut settings, self.nav.count > 0),
                        Tab::Keyboard => keyboard_settings(ui, &mut settings),
                    }
                });
            ui.separator();
            ui.horizontal(|ui| {
                save = ui
                    .add_enabled(
                        settings.recording.is_none(),
                        egui::Button::new("Save settings"),
                    )
                    .clicked();
                cancel = ui.button("Cancel").clicked();
            });
            ui.weak("Save to apply · Escape to cancel");
        });
        if save {
            match settings.shortcuts.validate() {
                Ok(()) => self.apply_settings(settings, context),
                Err(error) => {
                    settings.error = Some(error);
                    self.settings = Some(settings);
                }
            }
        } else if !cancel {
            self.settings = Some(settings);
        }
    }
}

fn display_settings(ui: &mut egui::Ui, settings: &mut SettingsUi, has_document: bool) {
    ui.heading("Appearance");
    ui.checkbox(&mut settings.dark, "Dark interface");
    ui.checkbox(&mut settings.night, "Invert PDF page colors");
    ui.checkbox(&mut settings.sidebar, "Show sidebar");
    ui.checkbox(&mut settings.thumbnails, "Show page previews in sidebar");
    ui.add_space(12.0);
    ui.heading("Default document view");
    ui.label("Page layout");
    egui::ComboBox::from_id_salt("settings-layout")
        .width(ui.available_width().min(280.0))
        .selected_text(settings.display.layout.label())
        .show_ui(ui, |ui| {
            for layout in [Layout::Single, Layout::Continuous, Layout::Facing] {
                ui.selectable_value(&mut settings.display.layout, layout, layout.label());
            }
        });
    let fit_label = match settings.display.fit {
        Fit::Page => "Fit page",
        Fit::Width => "Fit width",
        Fit::Height => "Fit height",
        Fit::Custom(_) => "Custom zoom",
    };
    ui.label("Page size");
    egui::ComboBox::from_id_salt("settings-fit")
        .width(ui.available_width().min(280.0))
        .selected_text(fit_label)
        .show_ui(ui, |ui| {
            for (label, fit) in [
                ("Fit page", Fit::Page),
                ("Fit width", Fit::Width),
                ("Fit height", Fit::Height),
                ("Custom zoom", Fit::Custom(1.0)),
            ] {
                let selected = if matches!(fit, Fit::Custom(_)) {
                    matches!(settings.display.fit, Fit::Custom(_))
                } else {
                    settings.display.fit == fit
                };
                if ui.selectable_label(selected, label).clicked() {
                    settings.display.fit = fit;
                }
            }
        });
    if let Fit::Custom(scale) = &mut settings.display.fit {
        let mut percent = *scale * 100.0;
        ui.horizontal(|ui| {
            ui.label("Zoom");
            ui.add(
                egui::DragValue::new(&mut percent)
                    .range(5.0..=800.0)
                    .speed(1.0)
                    .suffix("%"),
            );
        });
        *scale = percent / 100.0;
    }
    ui.add_space(8.0);
    ui.checkbox(
        &mut settings.display.remember_documents,
        "Restore each document's last position and view",
    );
    ui.weak("Defaults apply to new documents. Disable restore to use defaults and start at page 1 every time. Bookmarks and named positions are kept.");
    ui.add_enabled_ui(has_document, |ui| {
        ui.checkbox(
            &mut settings.apply_current,
            "Apply default view to the current document too",
        );
    });
    ui.add_space(8.0);
    if ui.button("Reset display defaults").clicked() {
        settings.display = DisplaySettings::default();
        settings.dark = false;
        settings.night = false;
        settings.sidebar = false;
        settings.thumbnails = false;
    }
}

fn keyboard_settings(ui: &mut egui::Ui, settings: &mut SettingsUi) {
    ui.label("Use Add to record a key or key combination. Remove existing keys with ×, or reset an action to its defaults.");
    ui.weak("Ctrl/Cmd+, always opens Settings. Escape, editing shortcuts, counts and Vim sequences (gg, z…, m…, '…) are reserved. Reader keys are inactive in text fields.");
    ui.add(
        egui::TextEdit::singleline(&mut settings.filter)
            .id_salt("settings-filter")
            .hint_text("Filter actions or keys…")
            .desired_width(f32::INFINITY),
    );
    if ui.button("Reset all shortcuts").clicked() {
        settings.shortcuts = Shortcuts::default();
        settings.recording = None;
        settings.error = None;
    }
    if let Some(id) = settings.recording {
        let label = commands()
            .iter()
            .find(|command| command.id == id)
            .unwrap()
            .label;
        ui.colored_label(
            ui.visuals().selection.stroke.color,
            format!("Press a shortcut for {label}… (Escape to cancel)"),
        );
        if ui.button("Stop recording").clicked() {
            settings.recording = None;
        }
    }
    let filter = settings.filter.to_lowercase();
    let mut visible = 0;
    for command in commands() {
        let label = settings.shortcuts.label(command.id);
        if !format!("{} {label}", command.label)
            .to_lowercase()
            .contains(&filter)
        {
            continue;
        }
        visible += 1;
        ui.push_id(command.id, |ui| {
            ui.separator();
            ui.label(egui::RichText::new(command.label).strong());
            let mut bindings = settings.shortcuts.bindings(command).to_vec();
            let mut remove = None;
            ui.horizontal_wrapped(|ui| {
                for (index, binding) in bindings.iter().enumerate() {
                    ui.monospace(binding.label());
                    if ui
                        .small_button("×")
                        .on_hover_text("Remove binding")
                        .clicked()
                    {
                        remove = Some(index);
                    }
                }
                if bindings.is_empty() {
                    ui.weak("Unassigned");
                }
                if ui
                    .add_enabled(settings.recording.is_none(), egui::Button::new("Add"))
                    .clicked()
                {
                    settings.recording = Some(command.id);
                    settings.error = None;
                    ui.memory_mut(|memory| {
                        if let Some(id) = memory.focused() {
                            memory.surrender_focus(id);
                        }
                        memory.move_focus(egui::FocusDirection::None);
                    });
                }
                if ui.small_button("Reset").clicked() {
                    settings.shortcuts.overrides.remove(command.id);
                    settings.error = None;
                }
            });
            if let Some(index) = remove {
                bindings.remove(index);
                settings
                    .shortcuts
                    .overrides
                    .insert(command.id.into(), bindings);
            }
        });
    }
    if visible == 0 {
        ui.weak("No matching actions.");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use egui::{Context, Event, FullOutput, Modifiers, PointerButton, Pos2, RawInput, Rect, Vec2};

    fn key(key: Key, modifiers: Modifiers) -> Vec<Event> {
        [true, false]
            .into_iter()
            .map(|pressed| Event::Key {
                key,
                physical_key: None,
                pressed,
                repeat: false,
                modifiers,
            })
            .collect()
    }

    fn frame(app: &mut Pagewing, context: &Context, events: Vec<Event>) -> FullOutput {
        context.run(
            RawInput {
                screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(1100.0, 820.0))),
                events,
                ..Default::default()
            },
            |context| {
                app.keys(context);
                app.toolbar(context);
                egui::CentralPanel::default().show(context, |_| {});
                app.settings_window(context);
            },
        )
    }

    fn text_position(output: &FullOutput, label: &str) -> Pos2 {
        output
            .shapes
            .iter()
            .find_map(|shape| {
                if let egui::Shape::Text(text) = &shape.shape
                    && text.galley.text() == label
                {
                    Some(text.pos + text.galley.size() * 0.5)
                } else {
                    None
                }
            })
            .unwrap_or_else(|| panic!("missing UI text: {label}"))
    }

    fn click(app: &mut Pagewing, context: &Context, label: &str) {
        let output = frame(app, context, Vec::new());
        let pos = text_position(&output, label);
        for pressed in [true, false] {
            frame(
                app,
                context,
                vec![
                    Event::PointerMoved(pos),
                    Event::PointerButton {
                        pos,
                        button: PointerButton::Primary,
                        pressed,
                        modifiers: Modifiers::NONE,
                    },
                ],
            );
        }
    }

    #[test]
    fn settings_toolbar_cancel_save_and_recording_work_through_egui() {
        let directory = tempfile::tempdir().unwrap();
        let config = directory.path().join("library.json");
        let context = Context::default();
        let mut app = Pagewing::with_context(&context, None, None, config.clone());
        frame(&mut app, &context, Vec::new());
        click(&mut app, &context, "Settings");
        assert!(app.settings.is_some());
        click(&mut app, &context, "Dark interface");
        assert!(app.settings.as_ref().unwrap().dark);
        assert!(!app.library.dark, "a draft must not change saved settings");
        click(&mut app, &context, "Cancel");
        assert!(app.settings.is_none());
        assert!(!app.library.dark);

        frame(&mut app, &context, key(Key::Comma, Modifiers::COMMAND));
        click(&mut app, &context, "Dark interface");
        click(&mut app, &context, "Keyboard shortcuts");
        app.settings.as_mut().unwrap().filter = "next page".into();
        click(&mut app, &context, "Add");
        assert_eq!(app.settings.as_ref().unwrap().recording, Some("next_page"));
        frame(&mut app, &context, key(Key::P, Modifiers::NONE));
        assert!(
            app.settings
                .as_ref()
                .unwrap()
                .error
                .as_ref()
                .unwrap()
                .contains("Fit page")
        );
        assert_eq!(app.settings.as_ref().unwrap().recording, Some("next_page"));
        frame(&mut app, &context, key(Key::X, Modifiers::NONE));
        assert!(app.settings.as_ref().unwrap().error.is_none());
        assert!(app.settings.as_ref().unwrap().recording.is_none());
        click(&mut app, &context, "Save settings");
        assert!(app.settings.is_none());
        assert!(app.library.dark);
        assert!(app.library.shortcuts.label("next_page").contains('X'));
        drop(app);
        let restored = crate::state::Library::load(&config);
        assert!(restored.dark);
        assert!(restored.shortcuts.label("next_page").contains('X'));
    }

    #[test]
    fn configured_reader_keys_keep_counts_and_do_not_escape_text_fields_or_sequences() {
        let directory = tempfile::tempdir().unwrap();
        let context = Context::default();
        let mut app =
            Pagewing::with_context(&context, None, None, directory.path().join("library.json"));
        app.nav = crate::navigation::Navigation::new(12, 0);
        app.dimensions = vec![[600.0, 800.0]; 12];
        app.library.shortcuts.overrides.insert(
            "next_page".into(),
            vec![Binding::from_event(Key::X, Modifiers::NONE, None)],
        );
        frame(&mut app, &context, key(Key::J, Modifiers::SHIFT));
        assert_eq!(app.nav.page, 0, "replaced defaults must be inactive");
        let mut events = key(Key::Num3, Modifiers::NONE);
        events.extend(key(Key::X, Modifiers::NONE));
        frame(&mut app, &context, events);
        assert_eq!(app.nav.page, 3);
        app.search_open = true;
        app.search_focus = true;
        frame(&mut app, &context, key(Key::X, Modifiers::NONE));
        assert_eq!(
            app.nav.page, 3,
            "plain custom keys must not navigate while typing"
        );
        app.search_open = false;
        app.search_focus = false;
        app.library
            .shortcuts
            .overrides
            .insert("thumbnails".into(), vec![]);
        let mut events = key(Key::Z, Modifiers::NONE);
        events.extend(key(Key::T, Modifiers::NONE));
        frame(&mut app, &context, events);
        assert!(
            !app.nav.pending_z,
            "zt must still work when t has been unassigned"
        );
        assert!(!app.library.thumbnails);
        app.mark_action = Some(super::super::MarkAction::Set);
        frame(&mut app, &context, key(Key::Num0, Modifiers::COMMAND));
        assert!(
            app.mark_action.is_none(),
            "Command zoom cancels the pending mark prompt"
        );
        assert_eq!(app.reading.fit, Fit::Custom(1.0));
        app.mark_action = Some(super::super::MarkAction::Set);
        frame(&mut app, &context, key(Key::Equals, Modifiers::COMMAND));
        assert!(app.mark_action.is_none());
        assert_eq!(app.reading.fit, Fit::Custom(1.2));
        app.open_settings(&context);
        app.settings.as_mut().unwrap().recording = Some("next_page");
        frame(&mut app, &context, key(Key::Escape, Modifiers::NONE));
        assert!(app.settings.is_some());
        assert!(app.settings.as_ref().unwrap().recording.is_none());
        frame(&mut app, &context, key(Key::Escape, Modifiers::NONE));
        assert!(app.settings.is_none());
    }

    #[test]
    fn settings_keep_save_and_cancel_visible_in_the_smallest_window() {
        let directory = tempfile::tempdir().unwrap();
        for size in [Vec2::new(240.0, 240.0), Vec2::new(360.0, 320.0)] {
            for tab in [Tab::Display, Tab::Keyboard] {
                let context = Context::default();
                let mut app = Pagewing::with_context(
                    &context,
                    None,
                    None,
                    directory.path().join("library.json"),
                );
                app.open_settings(&context);
                let draft = app.settings.as_mut().unwrap();
                draft.tab = tab;
                draft.display.layout = Layout::Facing;
                draft.error = Some("This shortcut conflicts with another action. Remove its existing binding first.".into());
                let mut output = None;
                for _ in 0..3 {
                    output = Some(context.run(
                        RawInput {
                            screen_rect: Some(Rect::from_min_size(Pos2::ZERO, size)),
                            ..Default::default()
                        },
                        |context| app.settings_window(context),
                    ));
                }
                let output = output.unwrap();
                for label in ["Save settings", "Cancel"] {
                    assert!(
                        Rect::from_min_size(Pos2::ZERO, size)
                            .contains(text_position(&output, label)),
                        "{label} outside {size:?}"
                    );
                }
            }
        }
    }

    #[test]
    fn display_defaults_respect_document_memory_and_keep_bookmarks() {
        let directory = tempfile::tempdir().unwrap();
        let config = directory.path().join("library.json");
        let first = directory.path().join("first.pdf");
        let second = directory.path().join("second.pdf");
        let mut library = crate::state::Library::default();
        library.display.layout = Layout::Facing;
        library.display.fit = Fit::Width;
        library.remember(crate::state::ReadingState {
            path: first.clone(),
            page: 7,
            fit: Fit::Height,
            continuous: true,
            bookmarks: [3].into(),
            ..Default::default()
        });
        library.save(&config).unwrap();
        let context = Context::default();
        let mut app = Pagewing::with_context(&context, Some(first.clone()), None, config);
        assert_eq!(app.reading.page, 7);
        assert_eq!(app.reading.fit, Fit::Height);
        assert!(app.reading.continuous);
        app.open(second, &context);
        assert!(app.reading.facing && !app.reading.continuous);
        assert_eq!(app.reading.fit, Fit::Width);
        app.library.display.remember_documents = false;
        app.open(first, &context);
        assert_eq!(app.reading.page, 0);
        assert!(app.reading.facing && !app.reading.continuous);
        assert_eq!(app.reading.fit, Fit::Width);
        assert_eq!(app.reading.bookmarks, [3].into());
        app.open_settings(&context);
        let mut draft = app.settings.take().unwrap();
        draft.display.layout = Layout::Continuous;
        draft.display.fit = Fit::Custom(1.5);
        draft.apply_current = true;
        app.apply_settings(draft, &context);
        assert!(app.reading.continuous && !app.reading.facing);
        assert_eq!(app.reading.fit, Fit::Custom(1.5));
        assert_eq!(app.reading.bookmarks, [3].into());
    }
}
