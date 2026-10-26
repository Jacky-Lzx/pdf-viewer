use crate::outline::OutlineEntry;
use std::collections::HashSet;

/// Cached preorder tree: only rebuild visible rows when its filter or folds change.
#[derive(Default)]
pub struct OutlineView {
    entries: Vec<OutlineEntry>,
    folded_titles: Vec<String>,
    parents: Vec<Option<usize>>,
    subtree_end: Vec<usize>,
    collapsed: HashSet<usize>,
    visible: Vec<usize>,
    selected: Option<usize>,
    pub query: String,
    pub focus: bool,
    pub motions: Vec<isize>,
    pub confirm: bool,
    pub reveal: bool,
}

impl OutlineView {
    pub fn set_entries(&mut self, entries: Vec<OutlineEntry>) {
        *self = Self::default();
        self.subtree_end = vec![entries.len(); entries.len()];
        let mut ancestors: Vec<usize> = Vec::new();
        for (index, entry) in entries.iter().enumerate() {
            while ancestors
                .last()
                .is_some_and(|&parent| entries[parent].depth >= entry.depth)
            {
                self.subtree_end[ancestors.pop().unwrap()] = index;
            }
            self.parents.push(ancestors.last().copied());
            ancestors.push(index);
            self.folded_titles
                .push(caseless::default_case_fold_str(&entry.title));
        }
        self.entries = entries;
        self.refilter();
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

    pub fn apply_motions(&mut self) {
        // Reuse the input buffer while preserving each boundary clamp.
        let mut motions = std::mem::take(&mut self.motions);
        for delta in motions.drain(..) {
            self.step(delta);
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
            let query = caseless::default_case_fold_str(self.query.trim());
            let mut keep: Vec<_> = self
                .folded_titles
                .iter()
                .map(|title| title.contains(&query))
                .collect();
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
        tree.motions.push(5);
        tree.confirm = true;
        tree.cancel_input();
        assert!(!tree.focus && !tree.confirm);
        assert!(tree.motions.is_empty());
        assert_eq!(tree.selected(), Some(0));
        tree.set_entries(Vec::new());
        assert_eq!(tree.selected(), None);
    }
}
