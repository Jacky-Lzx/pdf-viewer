use crate::{list_navigation::Motion, outline::OutlineEntry};
use std::collections::HashSet;

/// Cached preorder tree: only rebuild visible rows when its filter or folds change.
#[derive(Default)]
pub struct OutlineView {
    entries: Vec<OutlineEntry>,
    folded_titles: Vec<String>,
    // Reuse one byte per entry across filter edits; reset with the document.
    filter_matches: Vec<bool>,
    parents: Vec<Option<usize>>,
    subtree_end: Vec<usize>,
    chapter_starts: Vec<(usize, usize)>,
    collapsed: HashSet<usize>,
    visible: Vec<usize>,
    selected: Option<usize>,
    pub query: String,
    pub focus: bool,
    pub motions: Vec<Motion>,
    pub confirm: bool,
    pub reveal: bool,
}

impl OutlineView {
    pub fn set_entries(&mut self, entries: Vec<OutlineEntry>) {
        *self = Self::prepare(entries, || false).unwrap();
    }

    /// Prepare searchable titles and tree indexes before handing a loaded
    /// document to the UI. A superseded load can stop between bounded batches.
    pub fn prepare(
        entries: Vec<OutlineEntry>,
        mut cancelled: impl FnMut() -> bool,
    ) -> Option<Self> {
        if cancelled() {
            return None;
        }
        let mut prepared = Self {
            subtree_end: vec![entries.len(); entries.len()],
            parents: Vec::with_capacity(entries.len()),
            folded_titles: Vec::with_capacity(entries.len()),
            visible: Vec::with_capacity(entries.len()),
            ..Default::default()
        };
        let mut ancestors: Vec<usize> = Vec::new();
        for (index, entry) in entries.iter().enumerate() {
            if index != 0 && index % 64 == 0 && cancelled() {
                return None;
            }
            while ancestors
                .last()
                .is_some_and(|&parent| entries[parent].depth >= entry.depth)
            {
                prepared.subtree_end[ancestors.pop().unwrap()] = index;
            }
            prepared.parents.push(ancestors.last().copied());
            ancestors.push(index);
            if let Some(destination) = entry.destination {
                prepared.chapter_starts.push((destination.page, index));
            }
            prepared
                .folded_titles
                .push(crate::case_fold::fold(&entry.title));
        }
        if cancelled() {
            return None;
        }
        // Destinations need not follow tree order. On the same page, prefer
        // the later navigable entry, typically the more specific heading.
        prepared.chapter_starts.sort_unstable();
        prepared.entries = entries;
        prepared.refilter();
        (!cancelled()).then_some(prepared)
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn visible(&self) -> &[usize] {
        &self.visible
    }

    pub fn entry(&self, index: usize) -> &OutlineEntry {
        &self.entries[index]
    }

    pub fn selected(&self) -> Option<usize> {
        self.selected
    }

    pub fn chapter_at(&self, page: usize) -> Option<usize> {
        let end = self
            .chapter_starts
            .partition_point(|&(start, _)| start <= page);
        end.checked_sub(1).map(|index| self.chapter_starts[index].1)
    }

    /// Show the nearest visible ancestor if the active chapter is folded away
    /// or excluded by the filter. Reading never changes the user's folds.
    pub fn visible_chapter_at(&self, page: usize) -> Option<usize> {
        let mut chapter = self.chapter_at(page);
        while let Some(index) = chapter {
            if self.visible.binary_search(&index).is_ok() {
                return Some(index);
            }
            chapter = self.parents[index];
        }
        None
    }

    pub fn reveal_chapter_at(&mut self, page: usize) -> bool {
        let Some(index) = self.chapter_at(page) else {
            return false;
        };
        self.query.clear();
        let mut parent = self.parents[index];
        while let Some(index) = parent {
            self.collapsed.remove(&index);
            parent = self.parents[index];
        }
        self.selected = Some(index);
        self.refilter();
        true
    }

    pub fn selected_row(&self) -> Option<usize> {
        self.visible.binary_search(&self.selected?).ok()
    }

    pub fn select(&mut self, index: usize) {
        if self.visible.binary_search(&index).is_ok() {
            self.selected = Some(index);
        }
    }

    pub fn step(&mut self, motion: isize) {
        if self.visible.is_empty() {
            return;
        }
        let row = self
            .selected_row()
            .unwrap_or(0)
            .saturating_add_signed(motion)
            .min(self.visible.len() - 1);
        self.selected = Some(self.visible[row]);
        self.reveal = true;
    }

    pub fn cancel_input(&mut self) {
        self.focus = false;
        self.motions.clear();
        self.confirm = false;
    }

    pub fn apply_motions(&mut self, page_rows: usize) {
        // Reuse the input buffer while preserving each boundary clamp.
        let mut motions = std::mem::take(&mut self.motions);
        for motion in motions.drain(..) {
            self.step(motion.delta(page_rows));
        }
        self.motions = motions;
    }

    pub fn has_children(&self, index: usize) -> bool {
        self.subtree_end[index] > index + 1
    }

    pub fn is_collapsed(&self, index: usize) -> bool {
        !self.filtering() && self.collapsed.contains(&index)
    }

    pub fn filtering(&self) -> bool {
        !self.query.trim().is_empty()
    }

    pub fn toggle(&mut self, index: usize) {
        if self.filtering() || !self.has_children(index) {
            return;
        }
        if !self.collapsed.remove(&index) {
            self.collapsed.insert(index);
        }
        self.refilter();
    }

    pub fn fold_all(&mut self, collapsed: bool) {
        self.collapsed.clear();
        if collapsed {
            self.collapsed = (0..self.entries.len())
                .filter(|&index| self.has_children(index))
                .collect();
        }
        self.refilter();
    }

    pub fn refilter(&mut self) {
        self.visible.clear();
        if self.filtering() {
            let query = crate::case_fold::fold(self.query.trim());
            let words: Vec<_> = query
                .split_whitespace()
                .map(memchr::memmem::Finder::new)
                .collect();
            let mut keep = std::mem::take(&mut self.filter_matches);
            keep.clear();
            keep.extend(self.folded_titles.iter().map(|title| {
                words
                    .iter()
                    .all(|word| word.find(title.as_bytes()).is_some())
            }));
            // Ancestors provide context, but Enter should initially follow a
            // title match rather than the first retained parent.
            if !self.selected.is_some_and(|index| keep[index]) {
                self.selected = keep.iter().position(|&matches| matches);
            }
            // Keep ancestors as context, temporarily revealing matches in folded chapters.
            for index in (0..keep.len()).rev() {
                if keep[index]
                    && let Some(parent) = self.parents[index]
                {
                    keep[parent] = true;
                }
            }
            self.visible
                .extend((0..keep.len()).filter(|&index| keep[index]));
            self.filter_matches = keep;
        } else {
            let mut index = 0;
            while index < self.entries.len() {
                self.visible.push(index);
                index = if self.collapsed.contains(&index) {
                    self.subtree_end[index]
                } else {
                    index + 1
                };
            }
            // Folding a selected descendant moves selection to its visible
            // ancestor, preserving the reader's place in the tree.
            while let Some(index) = self.selected {
                if self.visible.binary_search(&index).is_ok() {
                    break;
                }
                self.selected = self.parents[index];
            }
            if self.selected.is_none() {
                self.selected = self.visible.first().copied();
            }
        }
        self.reveal = true;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn current_chapters_follow_page_starts_and_reveal_only_their_ancestors() {
        let mut tree = OutlineView::default();
        tree.set_entries(
            [
                (0, "Part", None),
                (1, "Later", Some(5)),
                (2, "Later child", Some(9)),
                (1, "Earlier", Some(2)),
                (2, "Earlier detail", Some(2)),
                (0, "Other root", Some(20)),
                (1, "Other child", Some(25)),
            ]
            .into_iter()
            .map(|(depth, title, page)| OutlineEntry {
                title: title.into(),
                depth,
                destination: page.map(crate::destination::Destination::page),
            })
            .collect(),
        );
        for (page, expected) in [
            (0, None),
            (2, Some(4)),
            (4, Some(4)),
            (5, Some(1)),
            (11, Some(2)),
            (24, Some(5)),
            (99, Some(6)),
        ] {
            assert_eq!(tree.chapter_at(page), expected);
        }
        tree.fold_all(true);
        assert_eq!(tree.visible_chapter_at(4), Some(0));
        assert_eq!(tree.visible_chapter_at(24), Some(5));
        tree.query = "Later".into();
        tree.refilter();
        assert_eq!(tree.visible_chapter_at(4), Some(0));
        assert_eq!(tree.visible_chapter_at(24), None);
        assert!(tree.reveal_chapter_at(4));
        assert!(tree.query.is_empty());
        assert_eq!(tree.selected(), Some(4));
        assert_eq!(tree.visible_chapter_at(4), Some(4));
        assert_eq!(tree.visible(), [0, 1, 3, 4, 5]);
        assert!(tree.is_collapsed(1) && tree.is_collapsed(5));
        assert!(!tree.is_collapsed(0) && !tree.is_collapsed(3));
        tree.query = "Other".into();
        tree.refilter();
        let selected = tree.selected();
        assert!(!tree.reveal_chapter_at(0));
        assert_eq!(tree.query, "Other");
        assert_eq!(tree.selected(), selected);
    }

    #[test]
    fn chapter_index_matches_page_lookup_across_unordered_snapshots_and_duplicates() {
        let mut tree = OutlineView::default();
        let mut random = 91_u64;
        for count in [0, 1, 17, 10_000, 0, 29] {
            tree.set_entries(
                (0..count)
                    .map(|index| {
                        random = random.wrapping_mul(6364136223846793005).wrapping_add(1);
                        OutlineEntry {
                            title: format!("Heading {index}"),
                            depth: (random >> 32) as usize % 9,
                            destination: (!random.is_multiple_of(3)).then(|| {
                                crate::destination::Destination::page((random >> 16) as usize % 200)
                            }),
                        }
                    })
                    .collect(),
            );
            for page in 0..220 {
                let expected = tree
                    .entries
                    .iter()
                    .enumerate()
                    .filter_map(|(index, entry)| {
                        entry
                            .destination
                            .filter(|destination| destination.page <= page)
                            .map(|destination| (destination.page, index))
                    })
                    .max()
                    .map(|(_, index)| index);
                assert_eq!(
                    tree.chapter_at(page),
                    expected,
                    "{count} entries, page {page}"
                );
            }
        }
        tree.set_entries(Vec::new());
        assert_eq!(tree.chapter_at(usize::MAX), None);
        assert_eq!(tree.chapter_starts.capacity(), 0);
    }

    fn tree() -> OutlineView {
        let mut tree = OutlineView::default();
        tree.set_entries(
            [
                (0, "Part one"),
                (1, "Chapter"),
                (2, "Straße"),
                (1, "Appendix"),
                (0, "Part two"),
                (1, "ΟΣ"),
            ]
            .into_iter()
            .enumerate()
            .map(|(page, (depth, title))| OutlineEntry {
                title: title.into(),
                destination: Some(crate::destination::Destination::page(page)),
                depth,
            })
            .collect(),
        );
        tree
    }

    #[test]
    fn prepared_outline_is_ready_for_unicode_filters_folds_and_navigation() {
        let mut prepared = OutlineView::prepare(tree().entries, || false).unwrap();
        assert_eq!(prepared.visible(), [0, 1, 2, 3, 4, 5]);
        assert_eq!(prepared.selected(), Some(0));
        assert!(prepared.query.is_empty() && prepared.motions.is_empty());
        assert!(!prepared.focus && !prepared.confirm);
        assert!(prepared.reveal);
        prepared.toggle(1);
        prepared.query = "STRASSE".into();
        prepared.refilter();
        assert_eq!(prepared.visible(), [0, 1, 2]);
        assert_eq!(prepared.selected(), Some(2));
        prepared.query.clear();
        prepared.refilter();
        assert_eq!(prepared.visible(), [0, 1, 3, 4, 5]);
        assert_eq!(prepared.selected(), Some(1));
        prepared.step(1);
        assert_eq!(prepared.selected(), Some(3));
    }

    #[test]
    fn outline_preparation_stops_before_start_during_titles_and_before_publication() {
        assert!(OutlineView::prepare(Vec::new(), || true).is_none());
        assert!(OutlineView::prepare(tree().entries, || true).is_none());
        let entries: Vec<_> = (0..10_000)
            .map(|index| OutlineEntry {
                title: format!("Chapter {index} — Straße 中文"),
                destination: None,
                depth: index % 8,
            })
            .collect();
        let mut checks = 0;
        assert!(
            OutlineView::prepare(entries, || {
                checks += 1;
                checks == 3
            })
            .is_none()
        );
        assert_eq!(checks, 3);
        // Even empty outlines check again before their ready model is returned.
        let mut checks = 0;
        assert!(
            OutlineView::prepare(Vec::new(), || {
                checks += 1;
                checks == 3
            })
            .is_none()
        );
        assert_eq!(checks, 3);
    }

    #[test]
    fn nested_folds_preserve_child_state_and_root_siblings() {
        let mut tree = tree();
        tree.toggle(1);
        assert_eq!(tree.visible(), [0, 1, 3, 4, 5]);
        tree.toggle(0);
        assert_eq!(tree.visible(), [0, 4, 5]);
        tree.toggle(0);
        assert_eq!(tree.visible(), [0, 1, 3, 4, 5]);
        tree.toggle(3);
        assert!(!tree.is_collapsed(3), "leaf toggles are ignored");
        tree.fold_all(true);
        assert_eq!(tree.visible(), [0, 4]);
        tree.fold_all(false);
        assert_eq!(tree.visible(), [0, 1, 2, 3, 4, 5]);
    }

    #[test]
    fn filtering_reveals_matches_and_ancestors_without_losing_folds() {
        let mut tree = tree();
        tree.fold_all(true);
        tree.query = " STRASSE ".into();
        tree.refilter();
        assert_eq!(tree.visible(), [0, 1, 2]);
        assert!(!tree.is_collapsed(0));
        tree.toggle(0);
        assert_eq!(tree.visible(), [0, 1, 2]);
        tree.query = "ος".into();
        tree.refilter();
        assert_eq!(tree.visible(), [4, 5]);
        tree.query = "missing".into();
        tree.refilter();
        assert!(tree.visible().is_empty());
        tree.query.clear();
        tree.refilter();
        assert_eq!(tree.visible(), [0, 4]);
        tree.set_entries(Vec::new());
        assert!(tree.is_empty());
        assert!(tree.query.is_empty());
    }

    #[test]
    fn filtered_rows_match_ancestor_walks_across_queries_depths_and_snapshots() {
        let mut tree = OutlineView::default();
        let mut random = 31_u64;
        for count in [1, 17, 130, 1000, 0, 29] {
            tree.set_entries(
                (0..count)
                    .map(|index| {
                        random = random.wrapping_mul(6364136223846793005).wrapping_add(1);
                        OutlineEntry {
                            title: format!(
                                "Node {index} {} {}",
                                if random & 1 == 0 {
                                    "Straße"
                                } else {
                                    "chapter"
                                },
                                if random & 2 == 0 { "中文" } else { "ΟΣ" }
                            ),
                            depth: (random >> 32) as usize % 9,
                            destination: None,
                        }
                    })
                    .collect(),
            );
            tree.fold_all(true);
            for query in [
                "STRASSE",
                "中文 chapter",
                "σ",
                "missing",
                "node",
                "chapter",
                "strasse 中文",
                "node 1",
                "STRASSE",
            ] {
                let folded_query = caseless::default_case_fold_str(query);
                let words: Vec<_> = folded_query.split_whitespace().collect();
                let matched: Vec<_> = tree
                    .entries
                    .iter()
                    .map(|entry| {
                        let title = caseless::default_case_fold_str(&entry.title);
                        words.iter().all(|word| title.contains(word))
                    })
                    .collect();
                let selected = tree
                    .selected
                    .filter(|&index| matched[index])
                    .or_else(|| matched.iter().position(|&matched| matched));
                let mut retained = matched.clone();
                for (index, &matched) in matched.iter().enumerate() {
                    if matched {
                        let mut parent = tree.parents[index];
                        while let Some(index) = parent {
                            retained[index] = true;
                            parent = tree.parents[index];
                        }
                    }
                }
                let expected: Vec<_> = (0..count).filter(|&index| retained[index]).collect();
                tree.query = query.into();
                tree.refilter();
                assert_eq!(tree.visible(), expected, "{count} nodes, {query}");
                assert_eq!(tree.selected(), selected, "{count} nodes, {query}");
                // Sometimes select context rather than a title match before
                // changing the query, as keyboard navigation in the UI can.
                if let Some(&index) = tree.visible().first() {
                    tree.select(index);
                }
            }
        }
    }

    #[test]
    fn multiword_filters_match_one_title_and_preserve_context_selection_and_folds() {
        let mut tree = OutlineView::default();
        tree.set_entries(
            [
                (0, "Part one"),
                (1, "Straße — ΟΣ chapter"),
                (1, "Straße appendix"),
                (0, "Part two"),
                (1, "chapter Straße ΟΣ"),
            ]
            .into_iter()
            .map(|(depth, title)| OutlineEntry {
                title: title.into(),
                depth,
                destination: None,
            })
            .collect(),
        );
        tree.fold_all(true);
        tree.query = "  CHAPTER\tstrasse\u{2003}ος  ".into();
        tree.refilter();
        assert_eq!(tree.visible(), [0, 1, 3, 4]);
        assert_eq!(tree.selected(), Some(1), "skip context-only parents");
        tree.select(4);
        tree.query = "STRASSE chapter σ".into();
        tree.refilter();
        assert_eq!(tree.visible(), [0, 1, 3, 4]);
        assert_eq!(tree.selected(), Some(4), "retain a still-matching title");
        tree.query = "part strasse".into();
        tree.refilter();
        assert!(
            tree.visible().is_empty(),
            "tokens cannot span parent and child"
        );
        assert_eq!(tree.selected(), None);
        tree.query = "chapter missing".into();
        tree.refilter();
        assert!(tree.visible().is_empty(), "every keyword must match");
        tree.query = "\t\u{2003} ".into();
        tree.refilter();
        assert_eq!(tree.visible(), [0, 3], "restore the original folds");
        assert_eq!(tree.selected(), Some(0));
    }

    #[test]
    fn large_trees_and_skipped_depths_are_bounded() {
        let mut tree = OutlineView::default();
        tree.set_entries(
            (0..10_000)
                .map(|index| OutlineEntry {
                    title: format!("Node {index}"),
                    destination: None,
                    depth: if index == 0 { 0 } else { 20 },
                })
                .collect(),
        );
        tree.toggle(0);
        assert_eq!(tree.visible(), [0]);
        tree.query = "Node 9999".into();
        tree.refilter();
        assert_eq!(tree.visible(), [0, 9999]);
    }

    #[test]
    fn keyboard_selection_uses_title_matches_and_visible_rows() {
        let mut tree = tree();
        tree.fold_all(true);
        tree.query = "STRASSE".into();
        tree.refilter();
        assert_eq!(tree.selected(), Some(2), "skip context-only ancestors");
        assert_eq!(tree.selected_row(), Some(2));
        tree.step(-1);
        assert_eq!(tree.selected(), Some(1), "ancestors can still be selected");
        tree.step(isize::MIN);
        assert_eq!(tree.selected(), Some(0));
        tree.step(isize::MAX);
        assert_eq!(tree.selected(), Some(2));
        tree.query = "ος".into();
        tree.refilter();
        assert_eq!(tree.selected(), Some(5));
        tree.query = "missing".into();
        tree.refilter();
        tree.step(1);
        assert_eq!(tree.selected(), None);
        tree.query.clear();
        tree.refilter();
        assert_eq!(tree.selected(), Some(0));
        tree.step(1);
        assert_eq!(tree.selected(), Some(4), "folded children are skipped");
    }

    #[test]
    fn folding_moves_selection_to_visible_parent_and_reset_cancels_commands() {
        let mut tree = tree();
        tree.select(2);
        tree.toggle(1);
        assert_eq!(tree.selected(), Some(1));
        tree.toggle(0);
        assert_eq!(tree.selected(), Some(0));
        tree.select(2);
        assert_eq!(tree.selected(), Some(0), "hidden rows cannot be selected");
        tree.focus = true;
        tree.motions.push(Motion::Step(5));
        tree.confirm = true;
        tree.cancel_input();
        assert!(!tree.focus && !tree.confirm);
        assert!(tree.motions.is_empty());
        assert_eq!(tree.selected(), Some(0));
        tree.set_entries(Vec::new());
        assert_eq!(tree.selected(), None);
    }
}
