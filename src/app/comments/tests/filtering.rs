use super::*;
use crate::list_navigation::Motion;

#[test]
fn confirmation_waits_for_the_initial_comment_snapshot() {
    let (_root, context, mut app) = many_comments();
    let items = app.comments.comments.items.clone();
    app.comments.set_comments(Comments::default());
    app.comments.loaded = false;
    app.comments.view.filter.query = "Row".into();
    context.memory_mut(|memory| memory.request_focus(egui::Id::new("pagewing-comment-filter")));
    app.comments.filter_confirm = true;
    app.comments
        .advance_filter(app.nav.page, true, Duration::ZERO);
    assert!(app.comments.filter_navigation.is_some());
    assert!(app.comments.editor.is_none());
    app.comments.set_comments(Comments {
        items,
        limited: false,
    });
    assert!(app.comments.filter_navigation.is_some());
    frame(&mut app, &context, Vec::new());
    assert_eq!(app.comments.editor.as_ref().unwrap().contents, "Row 0");
}

#[test]
fn deferred_filter_navigation_keeps_key_order_and_opens_only_complete_results() {
    let (_root, context, mut app) = many_comments();
    let original = std::fs::read(&app.reading.path).unwrap();
    context.memory_mut(|memory| memory.request_focus(egui::Id::new("pagewing-comment-filter")));
    app.comments.view.filter.query = "Row".into();
    app.comments.filter_motions.push(Motion::Step(79));
    app.comments.filter_confirm = true;
    app.comments
        .advance_filter(app.nav.page, true, Duration::ZERO);
    assert_eq!(app.comments.view.progress(), Some((64, 80)));
    assert!(app.comments.filter_motions.is_empty());
    assert!(!app.comments.filter_confirm);
    assert!(app.comments.editor.is_none());
    app.comments.filter_motions.push(Motion::Step(-1));
    frame(&mut app, &context, Vec::new());
    assert_eq!(app.comments.editor.as_ref().unwrap().contents, "Row 78");
    assert!(app.comments.view.progress().is_none());
    assert!(app.comments.filter_navigation.is_none());
    assert_eq!(std::fs::read(&app.reading.path).unwrap(), original);
}

#[test]
fn replacing_the_query_discards_its_deferred_navigation() {
    let (_root, context, mut app) = many_comments();
    context.memory_mut(|memory| memory.request_focus(egui::Id::new("pagewing-comment-filter")));
    app.comments.view.filter.query = "Row".into();
    app.comments.filter_motions.push(Motion::Step(79));
    app.comments.filter_confirm = true;
    app.comments
        .advance_filter(app.nav.page, true, Duration::ZERO);
    app.comments.view.filter.query = "Row 1".into();
    frame(&mut app, &context, Vec::new());
    assert!(
        app.comments.editor.is_none(),
        "Enter for an old query must not open a new result"
    );
    assert_eq!(app.comments.view.matches(), 17);
    assert_eq!(app.comments.view.selected(), Some(1));
    assert!(app.comments.filter_navigation.is_none());
    assert!(app.comments.filter_motions.is_empty());
    assert!(!app.comments.filter_confirm);
}

#[test]
fn changed_conditions_snapshot_and_focus_loss_clear_deferred_filter_actions() {
    for action in ["page", "review", "marked", "snapshot", "focus", "hide"] {
        let (_root, context, mut app) = many_comments();
        app.comments.view.filter.query = "Row".into();
        app.comments.view.filter.current_page = true;
        app.comments.filter_confirm = true;
        app.comments
            .advance_filter(app.nav.page, true, Duration::ZERO);
        assert!(app.comments.filter_navigation.is_some());
        match action {
            "page" => app
                .comments
                .advance_filter(app.nav.page + 1, true, Duration::ZERO),
            "review" | "marked" => {
                if action == "review" {
                    app.comments.view.filter.review = Some(ReviewStatus::Accepted);
                } else {
                    app.comments.view.filter.marked = true;
                }
                app.comments
                    .advance_filter(app.nav.page, true, Duration::ZERO);
                assert_eq!(app.comments.view.progress(), Some((64, 80)), "{action}");
            }
            "snapshot" => app.comments.set_comments(Comments {
                items: app.comments.comments.items.clone(),
                limited: false,
            }),
            "focus" => app
                .comments
                .advance_filter(app.nav.page, false, Duration::ZERO),
            "hide" => app.set_comments_open(&context, false),
            _ => unreachable!(),
        }
        assert!(app.comments.filter_navigation.is_none(), "{action}");
        assert!(!app.comments.filter_confirm, "{action}");
        assert!(app.comments.editor.is_none(), "{action}");
    }
}

#[test]
fn deferred_confirmation_preserves_an_unsaved_draft() {
    let (_root, context, mut app) = many_comments();
    let original = std::fs::read(&app.reading.path).unwrap();
    let mut draft = app.comments.comments.items[0].clone();
    draft.contents.push_str(" draft 中文");
    app.comments.editor = Some(draft.clone());
    app.comments.original = "Row 0".into();
    app.comments.view.filter.query = "Row".into();
    context.memory_mut(|memory| memory.request_focus(egui::Id::new("pagewing-comment-filter")));
    app.comments.filter_motions.push(Motion::Step(79));
    app.comments.filter_confirm = true;
    app.comments
        .advance_filter(app.nav.page, true, Duration::ZERO);
    frame(&mut app, &context, Vec::new());
    assert_eq!(app.comments.view.selected(), Some(79));
    assert_eq!(
        app.comments.editor.as_ref().unwrap().contents,
        draft.contents
    );
    assert!(app.comments.unsaved());
    assert_eq!(std::fs::read(&app.reading.path).unwrap(), original);
}
