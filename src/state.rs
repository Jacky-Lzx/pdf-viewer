use crate::navigation::Fit;
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::{Path, PathBuf},
};

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct ReadingState {
    pub path: PathBuf,
    pub page: usize,
    pub fit: Fit,
    pub rotation: u8,
    pub bookmarks: BTreeSet<usize>,
    pub offset: [f32; 2],
    pub marks: BTreeMap<char, ReadingMark>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct ReadingMark {
    pub page: usize,
    pub fit: Fit,
    pub rotation: u8,
    pub offset: [f32; 2],
}

pub fn normalized_offset(pan: [f32; 2], overflow: [f32; 2]) -> [f32; 2] {
    std::array::from_fn(|axis| {
        if overflow[axis] > 0.0 {
            (pan[axis] / overflow[axis]).clamp(0.0, 1.0)
        } else {
            0.0
        }
    })
}

#[derive(Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Library {
    pub recent: Vec<ReadingState>,
    pub dark: bool,
    pub night: bool,
    pub sidebar: bool,
}

impl Library {
    pub fn load(path: &Path) -> Self {
        std::fs::read(path)
            .ok()
            .and_then(|bytes| serde_json::from_slice(&bytes).ok())
            .unwrap_or_default()
    }

    pub fn save(&self, path: &Path) -> std::io::Result<()> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let tmp = path.with_extension(format!("{}.tmp", std::process::id()));
        let bytes = serde_json::to_vec_pretty(self).map_err(std::io::Error::other)?;
        std::fs::write(&tmp, bytes)?;
        std::fs::rename(tmp, path)
    }

    pub fn remember(&mut self, state: ReadingState) {
        self.recent.retain(|item| item.path != state.path);
        self.recent.insert(0, state);
        self.recent.truncate(24);
    }
}

pub fn config_path() -> PathBuf {
    directories::ProjectDirs::from("dev", "folio", "Folio")
        .map(|dirs| dirs.config_dir().join("library.json"))
        .unwrap_or_else(|| PathBuf::from(".folio-library.json"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn library_roundtrip_preserves_unicode_paths_and_bookmarks() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config/library.json");
        let mut library = Library::default();
        library.remember(ReadingState {
            path: "文档.pdf".into(),
            page: 7,
            bookmarks: [1, 7].into(),
            offset: [0.25, 0.7],
            marks: [(
                'a',
                ReadingMark {
                    page: 3,
                    fit: Fit::Custom(2.0),
                    rotation: 1,
                    offset: [0.4, 0.8],
                },
            )]
            .into(),
            ..Default::default()
        });
        library.save(&path).unwrap();
        let restored = Library::load(&path);
        assert_eq!(restored.recent[0].page, 7);
        assert_eq!(restored.recent[0].bookmarks.len(), 2);
        assert_eq!(restored.recent[0].path, PathBuf::from("文档.pdf"));
        assert_eq!(restored.recent[0].offset, [0.25, 0.7]);
        let mark = &restored.recent[0].marks[&'a'];
        assert_eq!(mark.page, 3);
        assert_eq!(mark.fit, Fit::Custom(2.0));
        assert_eq!(mark.rotation, 1);
        assert_eq!(mark.offset, [0.4, 0.8]);
    }

    #[test]
    fn older_settings_without_marks_or_offsets_still_load() {
        let library: Library = serde_json::from_str(
            r#"{"recent":[{"path":"a.pdf","page":4,"fit":"Width","bookmarks":[1]}]}"#,
        )
        .unwrap();
        let book = &library.recent[0];
        assert_eq!(book.page, 4);
        assert_eq!(book.fit, Fit::Width);
        assert_eq!(book.offset, [0.0; 2]);
        assert!(book.marks.is_empty());
    }

    #[test]
    fn corrupted_settings_recover_and_recent_paths_are_unique() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("library.json");
        std::fs::write(&path, "broken").unwrap();
        assert!(Library::load(&path).recent.is_empty());
        let mut library = Library::default();
        for page in 0..30 {
            library.remember(ReadingState {
                path: "a.pdf".into(),
                page,
                ..Default::default()
            });
        }
        assert_eq!(library.recent.len(), 1);
        assert_eq!(library.recent[0].page, 29);
    }
}
