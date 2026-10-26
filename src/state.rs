use crate::navigation::Fit;
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeSet,
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
        let tmp = path.with_extension("tmp");
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
            ..Default::default()
        });
        library.save(&path).unwrap();
        let restored = Library::load(&path);
        assert_eq!(restored.recent[0].page, 7);
        assert_eq!(restored.recent[0].bookmarks.len(), 2);
        assert_eq!(restored.recent[0].path, PathBuf::from("文档.pdf"));
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
