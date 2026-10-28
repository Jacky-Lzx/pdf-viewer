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
