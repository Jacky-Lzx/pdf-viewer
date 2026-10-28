//! Cached comment navigation, independent of widget layout and PDF writing.
use crate::annotations::{Comments, ReviewStatus};
use std::collections::HashMap;

const SEARCH_BYTES: usize = 8 * 1024 * 1024;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct CommentFilter {
    pub query: String,
    pub current_page: bool,
    pub review: Option<ReviewStatus>,
    pub marked: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CommentRow {
    pub index: usize,
    pub depth: usize,
    /// Ancestors retained for context need not match the filters themselves.
    pub matched: bool,
}

#[derive(Default)]
pub struct CommentView {
    pub filter: CommentFilter,
    threads: Vec<(usize, usize)>,
    parents: Vec<Option<usize>>,
    // Scratch tables are sized to the snapshot and reused across filters.
    matched: Vec<bool>,
    retained: Vec<bool>,
    visible: Vec<CommentRow>,
    overlays: HashMap<usize, Vec<usize>>,
    folded: Vec<Option<[String; 2]>>,
    folded_bytes: usize,
    applied: Option<(CommentFilter, Option<usize>)>,
    matches: usize,
    selected: Option<usize>,
    pub reveal: bool,
}

impl CommentView {
    pub fn set_comments(&mut self, comments: &Comments) {
        self.threads = comments.thread_rows();
        self.parents.clear();
        let mut ancestors = Vec::new();
        for &(index, depth) in &self.threads {
            ancestors.truncate(depth);
            self.parents.push(ancestors.last().copied());
            ancestors.push(index);
        }
        self.overlays.clear();
        for (index, comment) in comments.items.iter().enumerate() {
            if comment.parent.is_none() && !comment.rects.is_empty() {
                self.overlays.entry(comment.page).or_default().push(index);
            }
        }
        self.folded = (0..comments.items.len()).map(|_| None).collect();
        self.folded_bytes = 0;
        self.matched = vec![false; comments.items.len()];
        self.retained = vec![false; comments.items.len()];
        self.applied = None;
        self.visible.clear();
        self.matches = 0;
        self.selected = None;
        self.reveal = false;
    }

    /// Refilter only when its inputs change, selecting the first direct match.
    pub fn update(&mut self, comments: &Comments, page: usize) -> bool {
        let page = self.filter.current_page.then_some(page);
        if self
            .applied
            .as_ref()
            .is_some_and(|(filter, previous)| filter == &self.filter && *previous == page)
        {
            return false;
        }
        let query = caseless::default_case_fold_str(&self.filter.query);
        // Prepare each keyword once for the whole snapshot, including both
        // author and contents fields, instead of rebuilding substring search.
        let words: Vec<_> = query
            .split_whitespace()
            .map(memchr::memmem::Finder::new)
            .collect();
        self.matched.fill(false);
        let matched = &mut self.matched;
        for (index, comment) in comments.items.iter().enumerate() {
            if page.is_some_and(|p| p != comment.page)
                || self.filter.review.is_some_and(|r| r != comment.review)
                || (self.filter.marked && !comment.marked)
            {
                continue;
            }
            matched[index] = if words.is_empty() {
                true
            } else if let Some(fields) = &self.folded[index] {
                words
                    .iter()
                    .all(|word| fields.iter().any(|s| word.find(s.as_bytes()).is_some()))
            } else {
                let fields = [
                    caseless::default_case_fold_str(&comment.author),
                    caseless::default_case_fold_str(&comment.contents),
                ];
                let found = words
                    .iter()
                    .all(|word| fields.iter().any(|s| word.find(s.as_bytes()).is_some()));
                let bytes = fields.iter().map(String::capacity).sum::<usize>();
                if bytes <= SEARCH_BYTES - self.folded_bytes {
                    self.folded_bytes += bytes;
                    self.folded[index] = Some(fields);
                }
                found
            };
        }
        self.matches = matched.iter().filter(|&&m| m).count();
        self.retained.copy_from_slice(matched);
        let keep = &mut self.retained;
        // The preorder is already cycle-safe; one reverse pass retains every
        // ancestor even for very deep threads, without walking each chain.
        for (row, &(index, _)) in self.threads.iter().enumerate().rev() {
            if keep[index]
                && let Some(parent) = self.parents[row]
            {
                keep[parent] = true;
            }
        }
        self.visible.clear();
        self.visible
            .extend(self.threads.iter().filter_map(|&(index, depth)| {
                keep[index].then_some(CommentRow {
                    index,
                    depth,
                    matched: matched[index],
                })
            }));
        self.applied = Some((self.filter.clone(), page));
        self.selected = self.visible.iter().position(|row| row.matched);
        self.reveal = true;
        true
    }

    pub fn rows(&self) -> &[CommentRow] {
        &self.visible
    }

    pub fn matches(&self) -> usize {
        self.matches
    }

    pub fn selected(&self) -> Option<usize> {
        self.visible.get(self.selected?).map(|row| row.index)
    }

    pub fn selected_row(&self) -> Option<usize> {
        self.selected
    }

    pub fn select(&mut self, index: usize) {
        if let Some(row) = self.visible.iter().position(|row| row.index == index) {
            self.selected = Some(row);
            self.reveal = true;
        }
    }

    pub fn step(&mut self, delta: isize) {
        if self.visible.is_empty() {
            return;
        }
        self.selected = Some(
            self.selected
                .unwrap_or(0)
                .saturating_add_signed(delta)
                .min(self.visible.len() - 1),
        );
        self.reveal = true;
    }

    /// Page painting is independent of the sidebar's filters.
    pub fn overlays(&self, page: usize) -> &[usize] {
        self.overlays.get(&page).map_or(&[], Vec::as_slice)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::annotations::{Comment, CommentId};

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
                    assert!(view.folded.iter().all(Option::is_none));
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
        let contents = format!("{}needle", "a".repeat(SEARCH_BYTES + 1));
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
            assert!(view.folded[0].is_none());
        }
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
}
