use crate::list_navigation::Motion;
use std::path::{Path, PathBuf};

struct Entry {
    path: PathBuf,
    folded: String,
    label: String,
    display: String,
}

pub struct RecentPicker {
    entries: Vec<Entry>,
    visible: Vec<usize>,
    pub query: String,
    pub selected: usize,
    pub focus: bool,
    pub reveal: bool,
    pub motions: Vec<Motion>,
    pub confirm: bool,
    pub undo: Option<crate::state::RemovedRecent>,
}

impl RecentPicker {
    pub fn new(paths: impl Iterator<Item = PathBuf>) -> Self {
        let entries: Vec<_> = paths
            .take(24)
            .map(|path| {
                let display = path.to_string_lossy().into_owned();
                let name = path.file_name().unwrap_or_default().to_string_lossy();
                let parent = path
                    .parent()
                    .unwrap_or_else(|| Path::new(""))
                    .to_string_lossy();
                let label = format!("{name}   ·   {parent}");
                Entry {
                    folded: caseless::default_case_fold_str(&display),
                    path,
                    label,
                    display,
                }
            })
            .collect();
        Self {
            visible: (0..entries.len()).collect(),
            entries,
            query: String::new(),
            selected: 0,
            focus: true,
            reveal: true,
            motions: Vec::new(),
            confirm: false,
            undo: None,
        }
    }

    pub fn filter(&mut self) {
        let query = caseless::default_case_fold_str(&self.query);
        let words: Vec<_> = query.split_whitespace().collect();
        self.visible.clear();
        self.visible.extend(
            self.entries
                .iter()
                .enumerate()
                .filter_map(|(index, entry)| {
                    words
                        .iter()
                        .all(|word| entry.folded.contains(word))
                        .then_some(index)
                }),
        );
        self.selected = 0;
        self.reveal = true;
    }

    /// Rebuild only after a library edit, keeping the query and nearest result.
    pub fn refresh(&mut self, paths: impl Iterator<Item = PathBuf>, preferred: Option<&Path>) {
        let selected = self.selected;
        let query = std::mem::take(&mut self.query);
        let undo = self.undo.take();
        *self = Self::new(paths);
        self.query = query;
        self.filter();
        self.selected = preferred
            .and_then(|path| {
                self.visible
                    .iter()
                    .position(|&index| self.entries[index].path == path)
            })
            .unwrap_or(selected)
            .min(self.len().saturating_sub(1));
        self.undo = undo;
    }

    pub fn len(&self) -> usize {
        self.visible.len()
    }

    pub fn step(&mut self, delta: isize) {
        self.selected = self
            .selected
            .saturating_add_signed(delta)
            .min(self.len().saturating_sub(1));
        self.reveal = true;
    }

    pub fn apply_motions(&mut self, page_rows: usize) {
        // Clamp each event in order. Summing up/down first loses movement at edges.
        let mut motions = std::mem::take(&mut self.motions);
        for motion in motions.drain(..) {
            self.step(motion.delta(page_rows));
        }
        self.motions = motions;
    }

    pub fn label(&self, row: usize) -> &str {
        &self.entries[self.visible[row]].label
    }

    pub fn path(&self, row: usize) -> &Path {
        &self.entries[self.visible[row]].path
    }

    pub fn display(&self, row: usize) -> &str {
        &self.entries[self.visible[row]].display
    }

    pub fn selected_path(&self) -> Option<&Path> {
        self.visible
            .get(self.selected)
            .map(|&index| self.entries[index].path.as_path())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn refreshing_after_removal_keeps_filter_and_nearest_selection_and_undo_reveals_the_path() {
        let paths: Vec<PathBuf> = [
            "one STRASSE.pdf",
            "other.pdf",
            "two Straße.pdf",
            "three Straße.pdf",
        ]
        .map(PathBuf::from)
        .into();
        let mut picker = RecentPicker::new(paths.iter().cloned());
        picker.query = "STRASSE".into();
        picker.filter();
        picker.step(1);
        assert_eq!(picker.selected_path(), Some(paths[2].as_path()));
        picker.refresh(
            paths.iter().filter(|path| **path != paths[2]).cloned(),
            None,
        );
        assert_eq!(picker.query, "STRASSE");
        assert_eq!(picker.selected_path(), Some(paths[3].as_path()));
        assert!(picker.focus && picker.reveal);
        picker.refresh(paths.iter().cloned(), Some(&paths[2]));
        assert_eq!(picker.selected_path(), Some(paths[2].as_path()));
        picker.query = "other".into();
        picker.filter();
        picker.refresh(paths.iter().cloned(), Some(&paths[2]));
        assert_eq!(picker.query, "other");
        assert_eq!(picker.selected_path(), Some(paths[1].as_path()));
        picker.refresh(std::iter::empty(), None);
        assert_eq!(picker.selected_path(), None);
        assert_eq!(picker.selected, 0);
    }

    #[test]
    fn unicode_filter_matches_all_path_tokens_and_retains_original_paths() {
        let mut picker = RecentPicker::new(
            [
                "/books/语言/ΣStraße.pdf",
                "/work/other.pdf",
                "/books/语言/STRASSE.pdf",
            ]
            .into_iter()
            .map(PathBuf::from),
        );
        picker.query = "语言 strasse".into();
        picker.filter();
        assert_eq!(picker.len(), 2);
        assert_eq!(
            picker.selected_path(),
            Some(Path::new("/books/语言/ΣStraße.pdf"))
        );
        picker.step(1);
        assert_eq!(
            picker.selected_path(),
            Some(Path::new("/books/语言/STRASSE.pdf"))
        );
        picker.query = "missing".into();
        picker.filter();
        picker.step(1);
        picker.step(-1);
        assert!(picker.selected_path().is_none());
        picker.query = "  ".into();
        picker.filter();
        assert_eq!(picker.len(), 3);
        assert_eq!(picker.selected, 0);
    }

    #[test]
    fn snapshot_is_bounded_and_keyboard_selection_clamps() {
        let mut picker =
            RecentPicker::new((0..100).map(|index| format!("folder/book-{index}.pdf").into()));
        assert_eq!(picker.len(), 24);
        picker.step(isize::MAX);
        assert_eq!(picker.selected, 23);
        picker.step(isize::MIN);
        assert_eq!(picker.selected, 0);
        assert!(picker.label(0).contains("book-0.pdf"));
        assert_eq!(picker.display(0), "folder/book-0.pdf");
    }
}
