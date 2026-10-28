use super::*;
use std::cell::Cell;

fn source() -> Source {
    Source {
        document: 7,
        page: 2,
        count: 6,
    }
}

fn target(page: usize) -> Target {
    Target {
        page,
        caption: format!("Target: Page {}", page + 1),
        tooltip: "Full PDF label".into(),
    }
}

#[test]
fn edits_debounce_and_idle_frames_reuse_the_result_and_text() {
    let mut preview = Preview::default();
    let start = Instant::now();
    let calls = Cell::new(0);
    let resolve = |_: &str| {
        calls.set(calls.get() + 1);
        Some(target(4))
    };
    for (value, time) in [("A", 0), ("A-", 100), ("A-1", 200), ("A-1", 349)] {
        assert!(matches!(
            preview.update(
                value,
                source(),
                start + Duration::from_millis(time),
                false,
                resolve
            ),
            State::Pending(_)
        ));
        assert_eq!(calls.get(), 0);
    }
    let State::Target(first) = preview.update(
        "A-1",
        source(),
        start + Duration::from_millis(350),
        false,
        resolve,
    ) else {
        panic!("resolve after the final edit settles")
    };
    let caption = first.caption.as_ptr();
    let tooltip = first.tooltip.as_ptr();
    for time in [351, 400, 1000] {
        let State::Target(result) = preview.update(
            "A-1",
            source(),
            start + Duration::from_millis(time),
            false,
            resolve,
        ) else {
            panic!("retain the result")
        };
        assert_eq!(result.page, 4);
        assert_eq!(result.caption.as_ptr(), caption);
        assert_eq!(result.tooltip.as_ptr(), tooltip);
    }
    assert_eq!(calls.get(), 1);
}

#[test]
fn confirmation_resolves_the_latest_edit_without_waiting_or_duplicating_lookup() {
    let mut preview = Preview::default();
    let now = Instant::now();
    let calls = Cell::new(0);
    let resolve = |value: &str| {
        calls.set(calls.get() + 1);
        Some(target(if value == "1" { 0 } else { 2 }))
    };
    assert!(matches!(
        preview.update("1", source(), now, false, resolve),
        State::Pending(_)
    ));
    let State::Target(result) = preview.update("=1", source(), now, true, resolve) else {
        panic!("Enter resolves the current text immediately")
    };
    assert_eq!(result.page, 2);
    assert_eq!(calls.get(), 1);
    assert!(matches!(
        preview.update("=1", source(), now, true, resolve),
        State::Target(_)
    ));
    assert_eq!(calls.get(), 1);
}

#[test]
fn missing_and_empty_values_do_not_repeat_failed_lookups() {
    let mut preview = Preview::default();
    let now = Instant::now();
    let calls = Cell::new(0);
    let resolve = |_: &str| {
        calls.set(calls.get() + 1);
        None
    };
    for value in ["", "  ", "\t"] {
        assert!(matches!(
            preview.update(value, source(), now, true, resolve),
            State::Empty
        ));
    }
    assert_eq!(calls.get(), 0);
    assert!(matches!(
        preview.update("missing", source(), now, true, resolve),
        State::Missing
    ));
    assert!(matches!(
        preview.update(
            "missing",
            source(),
            now + Duration::from_secs(1),
            false,
            resolve
        ),
        State::Missing
    ));
    assert_eq!(calls.get(), 1);
    assert!(matches!(
        preview.update("new", source(), now, false, resolve),
        State::Pending(_)
    ));
    assert_eq!(calls.get(), 1);
}

#[test]
fn page_count_document_and_reloaded_metadata_invalidate_the_cached_target() {
    let mut preview = Preview::default();
    let now = Instant::now();
    let calls = Cell::new(0);
    let resolve = |_: &str| {
        let page = calls.get();
        calls.set(page + 1);
        Some(target(page))
    };
    for (index, source) in [
        source(),
        Source {
            page: 3,
            ..source()
        },
        Source {
            count: 8,
            ..source()
        },
        Source {
            document: 8,
            ..source()
        },
    ]
    .into_iter()
    .enumerate()
    {
        let State::Target(result) = preview.update("+1", source, now, true, resolve) else {
            panic!("resolve the new source")
        };
        assert_eq!(result.page, index);
    }
    preview.invalidate();
    let State::Target(result) = preview.update(
        "+1",
        Source {
            document: 8,
            ..source()
        },
        now,
        true,
        resolve,
    ) else {
        panic!("same source with new metadata must resolve again")
    };
    assert_eq!(result.page, 4);
    assert_eq!(calls.get(), 5);
}
