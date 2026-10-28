use super::*;
use crate::annotations::{Comment, CommentId};

#[test]
fn sliced_unicode_results_match_complete_threaded_filters_with_and_without_cache() {
    let comments = Comments {
        items: (0..256)
            .map(|index| {
                let mut item = comment(
                    index / 16,
                    index % 16,
                    (index % 16 != 0).then(|| CommentId {
                        page: index / 16,
                        slot: index % 16 - 1,
                    }),
                    if index % 16 == 15 {
                        "Straße 中文 Σ ﬁnish"
                    } else {
                        "ancestor"
                    },
                );
                item.marked = index % 16 == 15;
                item.review = if item.marked {
                    ReviewStatus::Accepted
                } else {
                    ReviewStatus::None
                };
                item
            })
            .collect(),
        limited: false,
    };
    for uncached in [false, true] {
        let mut complete = CommentView::default();
        let mut sliced = CommentView::default();
        complete.set_comments(&comments);
        sliced.set_comments(&comments);
        if uncached {
            complete.folded_bytes = SEARCH_BYTES;
            sliced.folded_bytes = SEARCH_BYTES;
        }
        for query in [
            "STRASSE 中文",
            "Σ FINISH reviewer",
            "missing",
            "ancestor",
            "reviewer",
        ] {
            for view in [&mut complete, &mut sliced] {
                view.filter.query = query.into();
            }
            complete.update(&comments, 0);
            let mut slices = 0;
            while !sliced.update_for(&comments, 0, Duration::ZERO) {
                slices += 1;
                assert!(slices < 5);
                assert!(sliced.rows().is_empty());
                assert_eq!(sliced.matches(), 0);
                assert_eq!(sliced.selected(), None);
                assert_eq!(sliced.progress(), Some((slices * 64, 256)));
            }
            assert_eq!(slices, 3);
            assert_eq!(sliced.rows(), complete.rows());
            assert_eq!(sliced.matches(), complete.matches());
            assert_eq!(sliced.selected(), complete.selected());
            assert!(sliced.progress().is_none());
            assert!(!sliced.update_for(&comments, 0, Duration::ZERO));
            assert!(sliced.folded_bytes <= SEARCH_BYTES);
        }
    }
}

#[test]
fn changing_query_or_page_restarts_progress_and_reverting_never_exposes_stale_rows() {
    let comments = Comments {
        items: (0..256)
            .map(|index| comment(index % 2, index, None, "needle 中文"))
            .collect(),
        limited: false,
    };
    let mut view = CommentView::default();
    view.set_comments(&comments);
    view.update(&comments, 0);
    view.step(128);
    view.filter.query = "needle".into();
    assert!(!view.update_for(&comments, 0, Duration::ZERO));
    assert!(view.rows().is_empty());
    assert_eq!(view.selected(), None);
    view.filter.query = "中文".into();
    assert!(!view.update_for(&comments, 0, Duration::ZERO));
    assert_eq!(view.progress(), Some((64, 256)));
    assert!(!view.update_for(&comments, 1, Duration::ZERO));
    assert_eq!(
        view.progress(),
        Some((128, 256)),
        "all-page filtering survives navigation"
    );
    view.filter.current_page = true;
    assert!(!view.update_for(&comments, 0, Duration::ZERO));
    assert_eq!(view.progress(), Some((64, 256)));
    assert!(!view.update_for(&comments, 1, Duration::ZERO));
    assert_eq!(
        view.progress(),
        Some((64, 256)),
        "current-page changes cancel old progress"
    );
    while view.progress().is_some() {
        view.update_for(&comments, 1, Duration::ZERO);
    }
    assert_eq!(view.matches(), 128);
    assert!(
        view.rows()
            .iter()
            .all(|row| comments.items[row.index].page == 1)
    );
    assert_eq!(view.selected(), Some(1));
    view.filter.query = "missing".into();
    assert!(!view.update_for(&comments, 1, Duration::ZERO));
    view.filter = CommentFilter::default();
    assert!(view.update_for(&comments, 0, Duration::ZERO));
    assert!(view.progress().is_none());
    assert_eq!(view.rows().len(), 256);
    assert_eq!(view.selected(), Some(0));
}

#[test]
fn long_fields_yield_between_comments_and_replacement_cancels_the_old_scan() {
    let comments = Comments {
        items: (0..4)
            .map(|index| {
                comment(
                    0,
                    index,
                    None,
                    &format!("{} needle", "İΣﬁ中文".repeat(8192)),
                )
            })
            .collect(),
        limited: false,
    };
    let mut view = CommentView::default();
    view.set_comments(&comments);
    view.filter.query = "i\u{307}σfi中文 needle".into();
    for next in 1..4 {
        assert!(!view.update_for(&comments, 0, Duration::ZERO));
        assert_eq!(view.progress(), Some((next, 4)));
        assert!(view.rows().is_empty());
    }
    let replacement = Comments {
        items: vec![comment(2, 0, None, "different snapshot")],
        limited: false,
    };
    view.set_comments(&replacement);
    assert!(view.progress().is_none());
    assert_eq!(view.filter.query, "i\u{307}σfi中文 needle");
    assert!(view.update_for(&replacement, 0, Duration::ZERO));
    assert_eq!(view.matches(), 0);
    assert!(view.rows().is_empty());
    assert!(view.overlays(0).is_empty());
    assert_eq!(view.overlays(2), [0]);
}

fn comment(page: usize, slot: usize, parent: Option<CommentId>, text: &str) -> Comment {
    Comment {
        page,
        slot: Some(slot),
        parent,
        review: ReviewStatus::None,
        marked: false,
        states: Vec::new(),
        kind: "Highlight".into(),
        contents: text.into(),
        author: "Reviewer".into(),
        rects: vec![hayro::kurbo::Rect::new(10., 10., 30., 20.)],
        has_appearance: false,
        truncated: false,
        locked: false,
    }
}

#[test]
fn keyword_search_matches_the_string_model_across_fields_and_cache_states() {
    let fields = [
        ("Straße reviewer", "中文 annotation Σ ﬁnish"),
        ("İstanbul", "Combining e\u{301} and 👩‍💻 notes"),
        ("split", "field"),
        ("", "punctuation: [needle] aaaaab"),
        ("Reviewer", ""),
    ];
    let comments = Comments {
        items: fields
            .iter()
            .enumerate()
            .map(|(index, &(author, contents))| {
                let mut item = comment(0, index, None, contents);
                item.author = author.into();
                item
            })
            .collect(),
        limited: false,
    };
    let queries = [
        "",
        "\u{2003}\t",
        "STRASSE 中文",
        "Σ FINISH",
        "i\u{307}stanbul 👩‍💻",
        "e\u{301}",
        "é",
        "split field",
        "splitfield",
        "field split split",
        "[needle]",
        "aaaaab",
        "aaaaabc",
        "needle missing",
        "reviewer",
        "中文\u{2003}reviewer",
    ];
    for uncached in [false, true] {
        let mut view = CommentView::default();
        view.set_comments(&comments);
        if uncached {
            view.folded_bytes = SEARCH_BYTES;
        }
        // Repeat after warm-up to cover both prepared and uncached fields.
        for query in queries.into_iter().cycle().take(queries.len() * 2) {
            view.filter.query = query.into();
            assert!(view.update(&comments, 0));
            let query = caseless::default_case_fold_str(query);
            let expected: Vec<_> = fields
                .iter()
                .enumerate()
                .filter_map(|(index, &(author, contents))| {
                    let fields = [
                        caseless::default_case_fold_str(author),
                        caseless::default_case_fold_str(contents),
                    ];
                    query
                        .split_whitespace()
                        .all(|word| fields.iter().any(|field| field.contains(word)))
                        .then_some(index)
                })
                .collect();
            assert_eq!(
                view.rows().iter().map(|row| row.index).collect::<Vec<_>>(),
                expected,
                "{query:?}, uncached={uncached}"
            );
            assert_eq!(view.matches(), expected.len());
            assert_eq!(view.selected(), expected.first().copied());
            if uncached {
                // Empty fields can be cached without consuming the budget.
                assert!(
                    view.folded
                        .iter()
                        .flatten()
                        .all(|field| field.as_ref().is_none_or(String::is_empty))
                );
            }
        }
    }
}

#[test]
fn combined_unicode_filters_retain_ancestors_but_not_unmatched_siblings() {
    // Replies arrive out of thread order and may live on another page.
    let root = comment(0, 0, None, "Original question");
    let reply = comment(1, 0, root.id(), "Straße Σ 中文");
    let mut nested = comment(1, 1, reply.id(), "STRASSE ς 中文");
    nested.author = "审阅者".into();
    nested.review = ReviewStatus::Completed;
    nested.marked = true;
    let sibling = comment(0, 1, root.id(), "Unrelated reply");
    let comments = Comments {
        items: vec![nested, root, sibling, reply],
        limited: false,
    };
    let mut view = CommentView::default();
    view.set_comments(&comments);
    view.filter = CommentFilter {
        query: " strasse σ 审阅者 中文 ".into(),
        current_page: true,
        review: Some(ReviewStatus::Completed),
        marked: true,
    };
    assert!(view.update(&comments, 1));
    assert_eq!(view.matches(), 1);
    assert_eq!(
        view.selected(),
        Some(0),
        "select the matching reply, not its context"
    );
    assert_eq!(view.selected_row(), Some(2));
    assert_eq!(
        view.rows(),
        [
            CommentRow {
                index: 1,
                depth: 0,
                matched: false
            },
            CommentRow {
                index: 3,
                depth: 1,
                matched: false
            },
            CommentRow {
                index: 0,
                depth: 2,
                matched: true
            },
        ]
    );
    assert!(
        !view.update(&comments, 1),
        "unchanged draws reuse their rows"
    );
    assert_eq!(view.overlays(0), [1], "filters do not remove PDF overlays");
    assert!(
        view.overlays(1).is_empty(),
        "replies do not duplicate overlays"
    );
    assert!(view.update(&comments, 0));
    assert!(view.rows().is_empty());
    view.filter = CommentFilter::default();
    assert!(view.update(&comments, 0));
    assert_eq!(view.matches(), 4);
    assert!(
        !view.update(&comments, 99),
        "all-pages filter ignores navigation"
    );
}

#[test]
fn deep_threads_cycles_and_missing_parents_remain_stack_safe_and_unique() {
    let mut items = Vec::new();
    for slot in 0..10_000 {
        let parent = (slot > 0).then(|| CommentId {
            page: 0,
            slot: slot - 1,
        });
        items.push(comment(
            0,
            slot,
            parent,
            if slot == 9_999 { "needle" } else { "ancestor" },
        ));
    }
    let mut comments = Comments {
        items,
        limited: false,
    };
    let mut view = CommentView::default();
    view.set_comments(&comments);
    view.filter.query = "needle".into();
    view.update(&comments, 0);
    assert_eq!(view.matches(), 1);
    assert_eq!(view.rows().len(), 10_000);
    assert_eq!(view.rows().last().unwrap().depth, 9_999);
    assert_eq!(
        view.selected_row(),
        Some(9_999),
        "reveal the match at the end of a deep thread"
    );
    comments.items[0].parent = comments.items[9_999].id();
    view.set_comments(&comments);
    view.update(&comments, 0);
    assert_eq!(view.rows().len(), 10_000);
    comments.items[0].parent = Some(CommentId { page: 99, slot: 0 });
    view.set_comments(&comments);
    view.update(&comments, 0);
    assert_eq!(view.rows().len(), 10_000);
}

#[test]
fn replacing_comments_invalidates_search_and_page_indices_and_keeps_filter() {
    let mut comments = Comments {
        items: vec![comment(3, 0, None, "needle")],
        limited: false,
    };
    let mut view = CommentView::default();
    view.set_comments(&comments);
    view.filter.query = "needle".into();
    view.update(&comments, 0);
    assert_eq!(view.matches(), 1);
    comments.items[0].contents = "Changed externally".into();
    comments.items[0].page = 4;
    view.set_comments(&comments);
    view.update(&comments, 0);
    assert_eq!(view.matches(), 0);
    assert!(view.overlays(3).is_empty());
    assert_eq!(view.overlays(4), [0]);
    comments.items.clear();
    view.set_comments(&comments);
    view.update(&comments, 0);
    assert!(view.rows().is_empty());
    assert!(view.overlays(4).is_empty());
    let root = comment(5, 0, None, "parent");
    let reply = comment(5, 1, root.id(), "new needle");
    comments.items = vec![reply, root];
    view.set_comments(&comments);
    assert!(view.update(&comments, 0));
    assert_eq!(view.matches(), 1);
    assert_eq!(
        view.rows()
            .iter()
            .map(|row| (row.index, row.matched))
            .collect::<Vec<_>>(),
        [(1, false), (0, true)],
        "growing a replaced snapshot must rebuild context and direct matches"
    );
    assert_eq!(view.selected(), Some(0));
}

#[test]
fn successive_filters_reset_context_without_downgrading_direct_parent_matches() {
    let root = comment(0, 0, None, "shared root");
    let middle = comment(0, 1, root.id(), "shared middle");
    let leaf = comment(1, 0, middle.id(), "needle");
    let comments = Comments {
        items: vec![leaf, middle, root, comment(2, 0, None, "elsewhere")],
        limited: false,
    };
    let mut view = CommentView::default();
    view.set_comments(&comments);
    for (query, expected) in [
        ("needle", vec![(2, false), (1, false), (0, true)]),
        ("shared", vec![(2, true), (1, true)]),
        ("missing", vec![]),
        ("middle", vec![(2, false), (1, true)]),
        ("", vec![(2, true), (1, true), (0, true), (3, true)]),
        ("elsewhere", vec![(3, true)]),
    ] {
        view.filter.query = query.into();
        assert!(view.update(&comments, 0));
        assert_eq!(
            view.rows()
                .iter()
                .map(|row| (row.index, row.matched))
                .collect::<Vec<_>>(),
            expected,
            "{query:?}"
        );
        assert_eq!(
            view.matches(),
            expected.iter().filter(|(_, matched)| *matched).count()
        );
        assert_eq!(
            view.selected(),
            expected
                .iter()
                .find(|(_, matched)| *matched)
                .map(|(index, _)| *index)
        );
    }
}

#[test]
fn search_cache_budget_never_truncates_matching() {
    let contents = format!("{}NEEDLE", "A".repeat(SEARCH_BYTES + 1));
    let comments = Comments {
        items: vec![comment(0, 0, None, &contents)],
        limited: false,
    };
    let mut view = CommentView::default();
    view.set_comments(&comments);
    for query in ["needle", "reviewer needle", "missing"] {
        view.filter.query = query.into();
        view.update(&comments, 0);
        assert_eq!(view.matches(), usize::from(query != "missing"));
        assert!(view.folded_bytes <= SEARCH_BYTES);
        assert!(view.folded[0][1].is_none());
    }
}

#[test]
fn author_matches_leave_contents_unprepared_until_needed() {
    let comments = Comments {
        items: vec![comment(0, 0, None, "Straße 中文 ﬁnish")],
        limited: false,
    };
    let mut view = CommentView::default();
    view.set_comments(&comments);
    for (query, body_prepared) in [
        ("REVIEWER", false),
        ("reviewer reviewer", false),
        ("reviewer STRASSE", true),
        ("FINISH 中文", true),
    ] {
        view.filter.query = query.into();
        assert!(view.update(&comments, 0));
        assert_eq!(view.matches(), 1);
        assert_eq!(view.folded[0][1].is_some(), body_prepared);
    }
    let cached_bytes: usize = view
        .folded
        .iter()
        .flatten()
        .flatten()
        .map(String::capacity)
        .sum();
    assert_eq!(view.folded_bytes, cached_bytes);
    assert!(view.scratch.iter().all(String::is_empty));
    view.set_comments(&comments);
    assert_eq!(view.folded_bytes, 0);
    assert!(view.folded.iter().flatten().all(Option::is_none));
}

#[test]
fn oversized_scratch_does_not_block_smaller_fields_from_the_cache() {
    let comments = Comments {
        items: vec![
            comment(0, 0, None, &format!("{} needle", "İΣﬁa".repeat(1024))),
            comment(0, 1, None, "needle"),
        ],
        limited: false,
    };
    let mut view = CommentView::default();
    view.set_comments(&comments);
    let occupied = SEARCH_BYTES - 64;
    view.folded_bytes = occupied;
    for query in ["NEEDLE", "reviewer needle", "missing", "i\u{307}σfia"] {
        view.filter.query = query.into();
        view.update(&comments, 0);
        assert_eq!(
            view.matches(),
            match query {
                "missing" => 0,
                "i\u{307}σfia" => 1,
                _ => 2,
            }
        );
        assert!(view.folded[0][1].is_none());
        assert_eq!(view.folded[1][1].as_deref(), Some("needle"));
        let cached_bytes: usize = view
            .folded
            .iter()
            .flatten()
            .flatten()
            .map(String::capacity)
            .sum();
        assert_eq!(view.folded_bytes, occupied + cached_bytes);
        assert!(view.folded_bytes <= SEARCH_BYTES);
    }
    let capacity = view.scratch[1].capacity();
    assert!(capacity > 64);
    view.set_comments(&comments);
    assert!(view.scratch.iter().all(|field| field.capacity() == 0));
}

#[test]
fn keyboard_selection_clamps_each_motion_and_refilter_resets_it() {
    let comments = Comments {
        items: (0..3)
            .map(|slot| comment(0, slot, None, &format!("Comment {slot}")))
            .collect(),
        limited: false,
    };
    let mut view = CommentView::default();
    view.set_comments(&comments);
    view.update(&comments, 0);
    view.step(isize::MIN);
    view.step(1);
    assert_eq!(
        view.selected(),
        Some(1),
        "up at the start must not cancel the following down"
    );
    view.step(isize::MAX);
    assert_eq!(view.selected(), Some(2));
    view.select(0);
    assert_eq!(view.selected_row(), Some(0));
    view.filter.query = "missing".into();
    view.update(&comments, 0);
    view.step(1);
    assert_eq!(view.selected(), None);
    view.filter.query = "2".into();
    view.update(&comments, 0);
    assert_eq!(view.selected(), Some(2));
    assert_eq!(view.selected_row(), Some(0));
}
