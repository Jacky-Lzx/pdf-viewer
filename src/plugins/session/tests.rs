use super::*;
use std::fs;

pub(super) fn request(id: u64) -> Request {
    Request {
        api_version: 2,
        id,
        event: if id == 1 { "activate" } else { "key" },
        key: (id > 1).then(|| "j".into()),
        revision: 7,
        context: Context {
            document: "/tmp/中文.pdf".into(),
            page: 3,
            page_count: 6,
            page_size: [600., 800.],
            rotation: 0,
            zoom: 1.0,
            viewport: [600., 800.],
            scroll: [0., 0.],
            selected_text: String::new(),
            settings: Default::default(),
            lines: None,
            lines_truncated: false,
        },
    }
}

fn plugin(root: &std::path::Path, script: &str) -> Plugin {
    fs::write(root.join("run.sh"), script).unwrap();
    Plugin {
        id: "test".into(),
        description: String::new(),
        version: String::new(),
        author: String::new(),
        settings_schema: Vec::new(),
        settings: Default::default(),
        api_version: 2,
        name: "Session test".into(),
        enabled: true,
        directory: root.to_path_buf(),
        command: vec!["/bin/sh".into(), "run.sh".into()],
        timeout: Duration::from_secs(1),
    }
}

fn receive(session: &Session) -> Result<(u64, Reply), String> {
    let deadline = Instant::now() + Duration::from_secs(4);
    loop {
        if let Some(result) = session.poll() {
            return result;
        }
        assert!(Instant::now() < deadline, "session failed to respond");
        std::thread::sleep(Duration::from_millis(5));
    }
}

#[test]
fn session_keeps_one_process_and_state_across_events_and_idle_time() {
    let root = tempfile::tempdir().unwrap();
    let plugin = plugin(
        root.path(),
        "n=0\nwhile IFS= read -r line; do n=$((n+1)); printf '{\"id\":%s,\"message\":\"step %s\",\"capture_keys\":[\"j\",\"k\"]}\\n' \"$n\" \"$n\"; done",
    );
    let session = Session::start(&plugin, eframe::egui::Context::default()).unwrap();
    session.send(request(1)).unwrap();
    let (revision, first) = receive(&session).unwrap();
    assert_eq!(revision, 7);
    assert_eq!(first.message.as_deref(), Some("step 1"));
    std::thread::sleep(Duration::from_millis(1100));
    session.send(request(2)).unwrap();
    let (_, second) = receive(&session).unwrap();
    assert_eq!(second.message.as_deref(), Some("step 2"));
}

#[test]
fn bad_protocol_and_hung_io_end_the_session() {
    let root = tempfile::tempdir().unwrap();
    for (script, expected) in [
        ("read line; printf 'garbage\\n'; sleep 10", "expected value"),
        (
            "read line; printf '{\"id\":99}\\n'; sleep 10",
            "does not match",
        ),
        (
            "read line; printf '{\"id\":1,\"capture_keys\":[\"Cmd+Q\"]}\\n'; sleep 10",
            "capture_keys",
        ),
        (
            "read line; printf '{\"id\":1}\\n{\"id\":2}\\n'; sleep 10",
            "exactly one",
        ),
        ("sleep 10", "timed out"),
        ("echo problem >&2; exit 7", "problem"),
        (
            "while :; do printf '01234567890123456789012345678901234567890123456789'; done",
            "exceeds 64 KiB",
        ),
    ] {
        let session = Session::start(
            &plugin(root.path(), script),
            eframe::egui::Context::default(),
        )
        .unwrap();
        let mut input = request(1);
        input.context.selected_text = "a".repeat(100_000); // exceeds pipe capacity
        session.send(input).unwrap();
        let error = receive(&session).unwrap_err();
        assert!(error.contains(expected), "{error}");
    }
}

#[test]
fn dropping_session_terminates_its_process_group() {
    let root = tempfile::tempdir().unwrap();
    let plugin = plugin(
        root.path(),
        "(sleep 1; touch leaked) &\necho ready > ready\nwait",
    );
    let session = Session::start(&plugin, eframe::egui::Context::default()).unwrap();
    let deadline = Instant::now() + Duration::from_secs(3);
    while !root.path().join("ready").exists() {
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(5));
    }
    drop(session);
    std::thread::sleep(Duration::from_millis(1100));
    assert!(!root.path().join("leaked").exists());
}
