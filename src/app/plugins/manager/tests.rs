use super::*;

fn reader(root: &Path) -> (Pagewing, egui::Context) {
    let folder = root.join("plugins/focus");
    std::fs::create_dir_all(&folder).unwrap();
    std::fs::write(folder.join("plugin.toml"),
        "api_version=2\nname='Focus'\nenabled=true\ncommand=['/bin/sh','run.sh']\n[[settings]]\nkey='lines'\nlabel='Visible lines'\ntype='integer'\ndefault=3\nmin=1\nmax=20").unwrap();
    std::fs::write(folder.join("run.sh"),
        "n=0\nwhile IFS= read -r line; do n=$((n+1)); printf '%s\\n' \"$line\" >> events.jsonl; printf '{\"id\":%s,\"scene\":{},\"capture_keys\":[\"j\"],\"message\":\"visual ready\"}\\n' \"$n\"; done").unwrap();
    let tool = root.join("plugins/reference");
    std::fs::create_dir_all(&tool).unwrap();
    std::fs::write(
        tool.join("plugin.toml"),
        "api_version=1\nname='Reference'\nenabled=true\ncommand=['/bin/sh','run.sh']",
    )
    .unwrap();
    std::fs::write(
        tool.join("run.sh"),
        "cat > /dev/null; printf '%s' '{\"copy_text\":\"reference text\"}'",
    )
    .unwrap();
    let context = egui::Context::default();
    let mut app = Pagewing::with_context(&context, None, None, root.join("library.json"));
    app.nav = crate::navigation::Navigation::new(6, 0);
    app.dimensions = vec![[600., 800.]; 6];
    app.reading.path = root.join("book.pdf");
    (app, context)
}

fn activate(app: &mut Pagewing, context: &egui::Context) {
    app.plugins.output = None;
    app.plugin_sync(context);
    app.plugin_page_text(0, &crate::text::PageText::default());
    app.plugin_sync(context);
    let deadline = Instant::now() + Duration::from_secs(3);
    while app.plugins.output.is_none() {
        app.plugin_events(context);
        assert!(
            app.plugins.visual.is_some(),
            "visual plugin stopped: {:?}",
            app.error
        );
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(5));
    }
}

#[test]
fn tool_can_run_during_visual_mode_and_cancel_is_independent() {
    let directory = tempfile::tempdir().unwrap();
    let (mut app, context) = reader(directory.path());
    app.plugin_action(Action::Run(0), &context);
    activate(&mut app, &context);
    assert!(app.can_run_plugin(&app.plugins.registry.plugins[1]));
    app.plugin_action(Action::Run(1), &context);
    let deadline = Instant::now() + Duration::from_secs(3);
    let mut copied = false;
    while app.plugins.running.is_some() {
        let output = context.run(egui::RawInput::default(), |context| {
            app.plugin_events(context)
        });
        copied |= output
            .platform_output
            .commands
            .iter()
            .any(|c| matches!(c,egui::OutputCommand::CopyText(text) if text=="reference text"));
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(5));
    }
    assert!(copied);
    assert!(app.plugins.visual.is_some());
    app.plugin_action(Action::StopTool, &context);
    assert!(app.plugins.visual.is_some());
    app.plugin_action(Action::StopVisual, &context);
    assert!(app.plugins.visual.is_none());
}

#[test]
fn visual_mode_restores_on_document_transition_but_explicit_stop_is_remembered() {
    let directory = tempfile::tempdir().unwrap();
    let (mut app, context) = reader(directory.path());
    app.plugin_action(Action::Run(0), &context);
    assert_eq!(
        management::Preferences::load(&app.plugins.root)
            .unwrap()
            .last_visual
            .as_deref(),
        Some("focus")
    );
    app.suspend_plugins();
    app.id += 1;
    app.restore_visual_plugin(&context);
    assert_eq!(app.plugins.visual.as_ref().unwrap().id, "focus");
    app.stop_visual_plugin();
    app.suspend_plugins();
    app.restore_visual_plugin(&context);
    assert!(app.plugins.visual.is_none());
    let mut reopened = PluginUi::new(app.plugins.root.clone());
    assert!(reopened.preferences.last_visual.is_none());
    reopened
        .change_preferences(|prefs| prefs.remember_visual = false)
        .unwrap();
    app.plugins = reopened;
    app.plugin_action(Action::Run(0), &context);
    app.suspend_plugins();
    app.restore_visual_plugin(&context);
    assert!(app.plugins.visual.is_none());
}

#[test]
fn settings_apply_to_session_without_editing_package_and_disabling_stops_it() {
    let directory = tempfile::tempdir().unwrap();
    let (mut app, context) = reader(directory.path());
    let manifest = directory.path().join("plugins/focus/plugin.toml");
    let before = std::fs::read(&manifest).unwrap();
    app.plugin_action(Action::Run(0), &context);
    activate(&mut app, &context);
    let editor = Editor {
        id: "focus".into(),
        executable: String::new(),
        values: BTreeMap::from([("lines".into(), 5.into())]),
    };
    app.save_plugin_settings(&editor, &context).unwrap();
    activate(&mut app, &context);
    let log = std::fs::read_to_string(directory.path().join("plugins/focus/events.jsonl")).unwrap();
    let request: serde_json::Value = serde_json::from_str(log.lines().last().unwrap()).unwrap();
    assert_eq!(request["context"]["settings"]["lines"], 5);
    assert_eq!(std::fs::read(&manifest).unwrap(), before);
    app.plugin_action(Action::Enable("focus".into(), false), &context);
    assert!(app.plugins.visual.is_none());
    assert!(!PluginUi::new(app.plugins.root.clone()).registry.plugins[0].enabled);
}

#[test]
fn manager_text_entry_is_isolated_from_reader_and_visual_shortcuts() {
    let directory = tempfile::tempdir().unwrap();
    let (mut app, context) = reader(directory.path());
    app.plugin_action(Action::Run(0), &context);
    activate(&mut app, &context);
    app.plugins.manager.open = true;
    let _ = context.run(
        egui::RawInput {
            events: vec![egui::Event::Key {
                key: egui::Key::J,
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers: egui::Modifiers::NONE,
            }],
            ..Default::default()
        },
        |context| app.keys(context),
    );
    assert_eq!(app.nav.page, 0);
    // No queued key should be sent when we return to the reader.
    app.plugins.manager.open = false;
    app.plugin_sync(&context);
    std::thread::sleep(Duration::from_millis(30));
    let log = std::fs::read_to_string(directory.path().join("plugins/focus/events.jsonl")).unwrap();
    assert_eq!(log.lines().count(), 1);
    app.plugins.manager.open = true;
    let output = context.run(
        egui::RawInput {
            events: vec![egui::Event::Key {
                key: egui::Key::Q,
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers: egui::Modifiers::COMMAND,
            }],
            ..Default::default()
        },
        |context| app.keys(context),
    );
    assert!(
        output
            .viewport_output
            .values()
            .flat_map(|v| &v.commands)
            .any(|c| matches!(c, egui::ViewportCommand::Close)),
        "manager must preserve the application quit shortcut"
    );
}
