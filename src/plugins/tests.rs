use super::*;

fn manifest(root: &Path, name: &str, text: &str) {
    let directory = root.join(name);
    fs::create_dir_all(&directory).unwrap();
    fs::write(directory.join("plugin.toml"), text).unwrap();
}

fn request() -> Request {
    Request {
        api_version: 1,
        document: PathBuf::from("/tmp/中文 ' ; $(touch nope).pdf"),
        page: 2,
        page_count: 6,
        selected_text: "line one\n中文".into(),
    }
}

#[test]
fn discovers_sorted_plugins_and_isolates_bad_manifests() {
    let root = tempfile::tempdir().unwrap();
    assert!(
        Registry::discover(&root.path().join("missing"))
            .errors
            .is_empty()
    );
    manifest(
        root.path(),
        "b",
        "api_version = 1\nname = 'B'\ncommand = ['echo']",
    );
    manifest(
        root.path(),
        "a",
        "api_version = 1\nname = 'A'\ncommand = ['echo']\nenabled = true",
    );
    manifest(
        root.path(),
        "bad",
        "api_version = 99\nname = 'Bad'\ncommand = ['echo']",
    );
    manifest(
        root.path(),
        "unknown",
        "api_version = 1\nname = 'Bad'\ncommand = ['echo']\nnetwork = true",
    );
    manifest(
        root.path(),
        "timeout",
        "api_version = 1\nname = 'Bad'\ncommand = ['echo']\ntimeout_seconds = 0",
    );
    manifest(root.path(), "huge", &" ".repeat(MAX_MANIFEST as usize + 1));
    let registry = Registry::discover(root.path());
    assert_eq!(
        registry
            .plugins
            .iter()
            .map(|p| p.name.as_str())
            .collect::<Vec<_>>(),
        ["A", "B"]
    );
    assert_eq!(registry.errors.len(), 4);
    assert!(registry.plugins[0].enabled);
    assert!(!registry.plugins[1].enabled);
    assert!(registry.plugins[1].start(request()).is_err());
}

#[test]
fn response_rejects_unknown_fields_and_out_of_range_pages() {
    for json in [r#"{"goto_page":0}"#, r#"{"goto_page":7}"#] {
        assert!(
            serde_json::from_str::<Response>(json)
                .unwrap()
                .validate(6)
                .is_err()
        );
    }
    assert!(serde_json::from_str::<Response>(r#"{"run":"sh"}"#).is_err());
    assert!(serde_json::from_str::<Response>(r#"{"goto_page":-1}"#).is_err());
    serde_json::from_str::<Response>(r#"{"goto_page":6,"message":"ok"}"#)
        .unwrap()
        .validate(6)
        .unwrap();
}

#[cfg(unix)]
fn shell(root: &Path, script: &str) -> Plugin {
    fs::write(root.join("run.sh"), script).unwrap();
    Plugin {
        id: "test".into(),
        description: String::new(),
        version: String::new(),
        author: String::new(),
        settings_schema: Vec::new(),
        settings: Default::default(),
        api_version: 1,
        name: "Test".into(),
        enabled: true,
        directory: root.to_path_buf(),
        command: vec!["/bin/sh".into(), "run.sh".into()],
        timeout: Duration::from_secs(1),
    }
}

#[cfg(unix)]
fn finish(job: &Job) -> Result<Response, String> {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        if let Some(result) = job.poll() {
            return result;
        }
        assert!(Instant::now() < deadline, "plugin runner did not finish");
        std::thread::sleep(Duration::from_millis(10));
    }
}

#[cfg(unix)]
#[test]
fn process_roundtrip_passes_json_and_literal_arguments() {
    let root = tempfile::tempdir().unwrap();
    let mut plugin = shell(
        root.path(),
        "cat > request.json\nprintf '%s' \"$PAGEWING_PLUGIN_SETTINGS\" > settings.json\nprintf '%s' \"$1\" > argument.txt\nprintf '%s' '{\"message\":\"done\",\"copy_text\":\"中文\",\"goto_page\":3}'",
    );
    let literal = "space ' ; $(touch nope)";
    plugin.command.push(literal.into());
    plugin
        .settings
        .insert("prefix".into(), "中文 ' quoted".into());
    let response = finish(&plugin.start(request()).unwrap()).unwrap();
    assert_eq!(response.goto_page, Some(3));
    assert_eq!(response.copy_text.as_deref(), Some("中文"));
    let sent: serde_json::Value =
        serde_json::from_slice(&fs::read(root.path().join("request.json")).unwrap()).unwrap();
    assert_eq!(sent["api_version"], 1);
    assert_eq!(sent["page"], 2);
    assert_eq!(sent["selected_text"], "line one\n中文");
    assert_eq!(sent["document"], request().document.to_str().unwrap());
    assert_eq!(
        fs::read_to_string(root.path().join("argument.txt")).unwrap(),
        literal
    );
    assert!(!root.path().join("nope").exists());
    let settings: serde_json::Value =
        serde_json::from_slice(&fs::read(root.path().join("settings.json")).unwrap()).unwrap();
    assert_eq!(settings["prefix"], "中文 ' quoted");
}

#[cfg(unix)]
#[test]
fn process_errors_are_bounded_and_actionable() {
    let root = tempfile::tempdir().unwrap();
    for (script, expected) in [
        ("echo broken", "expected value"),
        ("echo diagnostic >&2; exit 7", "diagnostic"),
        ("printf '%s' '{\"goto_page\":100}'", "outside the document"),
        ("sleep 10", "timed out"),
        (
            "while :; do printf '0123456789012345678901234567890123456789012345678901234567890123456789'; done",
            "exceeds 64 KiB",
        ),
        (
            "while :; do printf '0123456789012345678901234567890123456789012345678901234567890123456789' >&2; done",
            "exceeds 64 KiB",
        ),
    ] {
        let result = finish(&shell(root.path(), script).start(request()).unwrap()).unwrap_err();
        assert!(result.contains(expected), "{result}");
    }
    let mut plugin = shell(root.path(), "echo '{}'");
    plugin.command[0] = root.path().join("missing-executable").display().to_string();
    assert!(finish(&plugin.start(request()).unwrap()).is_err());
}

#[cfg(unix)]
#[test]
fn cancellation_stops_descendants() {
    let root = tempfile::tempdir().unwrap();
    let plugin = shell(
        root.path(),
        "(sleep 1; touch leaked) &\necho ready > ready\nwait",
    );
    let job = plugin.start(request()).unwrap();
    let deadline = Instant::now() + Duration::from_secs(3);
    while !root.path().join("ready").exists() {
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(10));
    }
    job.cancelled.store(true, Ordering::Relaxed);
    assert!(finish(&job).unwrap_err().contains("cancelled"));
    std::thread::sleep(Duration::from_millis(1100));
    assert!(!root.path().join("leaked").exists());
}
