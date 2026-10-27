use crate::{navigation::Fit, text::SearchOptions};
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::{Path, PathBuf},
};

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct ReadingState {
    #[serde(with = "path_serde")]
    pub path: PathBuf,
    pub page: usize,
    pub fit: Fit,
    pub rotation: u8,
    pub continuous: bool,
    pub bookmarks: BTreeSet<usize>,
    pub offset: [f32; 2],
    pub marks: BTreeMap<char, ReadingMark>,
}

// Keep existing UTF-8 settings readable while preserving every Unix path byte.
// Linux and macOS both use Unix OsStrings; lossy display text is never a path.
mod path_serde {
    use serde::{Deserialize, Deserializer, Serialize, Serializer};
    use std::path::{Path, PathBuf};

    #[derive(Serialize, Deserialize)]
    #[serde(untagged)]
    enum StoredPath {
        Utf8(String),
        Unix { unix_bytes: Vec<u8> },
    }

    pub fn serialize<S: Serializer>(path: &Path, serializer: S) -> Result<S::Ok, S::Error> {
        if let Some(path) = path.to_str() {
            return serializer.serialize_str(path);
        }
        #[cfg(unix)]
        {
            use std::os::unix::ffi::OsStrExt;
            StoredPath::Unix {
                unix_bytes: path.as_os_str().as_bytes().to_vec(),
            }
            .serialize(serializer)
        }
        #[cfg(not(unix))]
        Err(serde::ser::Error::custom(
            "path contains invalid UTF-8 characters",
        ))
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(deserializer: D) -> Result<PathBuf, D::Error> {
        match StoredPath::deserialize(deserializer)? {
            StoredPath::Utf8(path) => Ok(path.into()),
            StoredPath::Unix { unix_bytes } => {
                #[cfg(unix)]
                {
                    use std::{ffi::OsString, os::unix::ffi::OsStringExt};
                    Ok(OsString::from_vec(unix_bytes).into())
                }
                #[cfg(not(unix))]
                {
                    let _ = unix_bytes;
                    Err(serde::de::Error::custom(
                        "Unix path bytes require a Unix platform",
                    ))
                }
            }
        }
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct ReadingMark {
    pub page: usize,
    pub fit: Fit,
    pub rotation: u8,
    pub continuous: bool,
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

#[derive(Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Library {
    pub recent: Vec<ReadingState>,
    pub dark: bool,
    pub night: bool,
    pub sidebar: bool,
    pub thumbnails: bool,
    pub search_options: SearchOptions,
}

impl Library {
    pub fn load(path: &Path) -> Self {
        let legacy = (path == config_path()).then(|| {
            directories::ProjectDirs::from("dev", "folio", "Folio")
                .map(|dirs| dirs.config_dir().join("library.json"))
                .unwrap_or_else(|| PathBuf::from(".folio-library.json"))
        });
        Self::load_with_fallback(path, legacy.as_deref())
    }

    fn load_with_fallback(path: &Path, legacy: Option<&Path>) -> Self {
        std::fs::read(path)
            .or_else(|error| {
                // Import the old library only until Pagewing has saved its own state.
                if error.kind() == std::io::ErrorKind::NotFound
                    && let Some(legacy) = legacy
                {
                    std::fs::read(legacy)
                } else {
                    Err(error)
                }
            })
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
    directories::ProjectDirs::from("dev", "pagewing", "Pagewing")
        .map(|dirs| dirs.config_dir().join("library.json"))
        .unwrap_or_else(|| PathBuf::from(".pagewing-library.json"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn renamed_app_imports_legacy_library_without_modifying_it() {
        let dir = tempfile::tempdir().unwrap();
        let legacy = dir.path().join("folio/library.json");
        let current = dir.path().join("pagewing/library.json");
        let mut library = Library::default();
        library.remember(ReadingState {
            path: "paper.pdf".into(),
            page: 7,
            bookmarks: [2, 7].into(),
            ..Default::default()
        });
        library.save(&legacy).unwrap();
        let original = std::fs::read(&legacy).unwrap();

        let restored = Library::load_with_fallback(&current, Some(&legacy));
        assert_eq!(restored.recent[0].page, 7);
        assert_eq!(restored.recent[0].bookmarks, [2, 7].into());
        restored.save(&current).unwrap();
        assert_eq!(Library::load(&current).recent[0].page, 7);
        assert_eq!(std::fs::read(&legacy).unwrap(), original);
    }

    #[test]
    fn current_library_takes_precedence_over_legacy_state() {
        let dir = tempfile::tempdir().unwrap();
        let legacy = dir.path().join("folio/library.json");
        let current = dir.path().join("pagewing/library.json");
        let mut library = Library::default();
        library.remember(ReadingState {
            path: "old.pdf".into(),
            ..Default::default()
        });
        library.save(&legacy).unwrap();
        Library::default().save(&current).unwrap();

        assert!(
            Library::load_with_fallback(&current, Some(&legacy))
                .recent
                .is_empty()
        );
        std::fs::write(&current, b"invalid json").unwrap();
        assert!(
            Library::load_with_fallback(&current, Some(&legacy))
                .recent
                .is_empty()
        );
    }

    #[test]
    fn library_roundtrip_preserves_unicode_paths_and_bookmarks() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config/library.json");
        let mut library = Library {
            search_options: SearchOptions {
                match_case: true,
                whole_word: true,
            },
            ..Default::default()
        };
        library.remember(ReadingState {
            path: "文档.pdf".into(),
            page: 7,
            fit: Fit::Height,
            continuous: true,
            bookmarks: [1, 7].into(),
            offset: [0.25, 0.7],
            marks: [(
                'a',
                ReadingMark {
                    page: 3,
                    fit: Fit::Custom(2.0),
                    rotation: 1,
                    continuous: true,
                    offset: [0.4, 0.8],
                },
            )]
            .into(),
            ..Default::default()
        });
        library.save(&path).unwrap();
        let restored = Library::load(&path);
        assert_eq!(restored.search_options, library.search_options);
        assert_eq!(restored.recent[0].page, 7);
        assert_eq!(restored.recent[0].fit, Fit::Height);
        assert!(restored.recent[0].continuous);
        assert_eq!(restored.recent[0].bookmarks.len(), 2);
        assert_eq!(restored.recent[0].path, PathBuf::from("文档.pdf"));
        assert_eq!(restored.recent[0].offset, [0.25, 0.7]);
        let mark = &restored.recent[0].marks[&'a'];
        assert_eq!(mark.page, 3);
        assert_eq!(mark.fit, Fit::Custom(2.0));
        assert_eq!(mark.rotation, 1);
        assert!(mark.continuous);
        assert_eq!(mark.offset, [0.4, 0.8]);
    }

    #[test]
    fn legacy_reading_positions_default_to_single_page_mode() {
        let state: ReadingState = serde_json::from_str(
            r#"{"path":"book.pdf","page":2,"offset":[0.2,0.4],"marks":{"a":{"page":1}}}"#,
        )
        .unwrap();
        assert!(!state.continuous);
        assert!(!state.marks[&'a'].continuous);
        assert_eq!(state.offset, [0.2, 0.4]);
    }

    #[cfg(unix)]
    #[test]
    fn library_roundtrip_preserves_non_utf8_paths_without_losing_other_documents() {
        use std::{ffi::OsString, os::unix::ffi::OsStringExt};

        let dir = tempfile::tempdir().unwrap();
        let settings = dir.path().join("library.json");
        let raw_path = PathBuf::from(OsString::from_vec(b"/docs/\xff\xfe.pdf".to_vec()));
        let mut library = Library::default();
        library.remember(ReadingState {
            path: "ordinary.pdf".into(),
            page: 2,
            ..Default::default()
        });
        library.remember(ReadingState {
            path: raw_path.clone(),
            page: 7,
            rotation: 3,
            bookmarks: [1, 7].into(),
            marks: [(
                'a',
                ReadingMark {
                    page: 4,
                    ..Default::default()
                },
            )]
            .into(),
            ..Default::default()
        });
        library.save(&settings).unwrap();
        let restored = Library::load(&settings);
        assert_eq!(restored.recent.len(), 2);
        assert_eq!(restored.recent[0].path, raw_path);
        assert_eq!(restored.recent[0].page, 7);
        assert_eq!(restored.recent[0].rotation, 3);
        assert_eq!(restored.recent[0].bookmarks, [1, 7].into());
        assert_eq!(restored.recent[0].marks[&'a'].page, 4);
        assert_eq!(restored.recent[1].path, PathBuf::from("ordinary.pdf"));
        assert_eq!(restored.recent[1].page, 2);
        let json: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&settings).unwrap()).unwrap();
        assert_eq!(json["recent"][1]["path"], "ordinary.pdf");
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
        assert_eq!(library.search_options, SearchOptions::default());
    }

    #[test]
    fn partial_search_preferences_preserve_existing_library_fields() {
        let library: Library = serde_json::from_str(
            r#"{"recent":[{"path":"a.pdf","page":4}],"dark":true,"search_options":{"whole_word":true}}"#,
        ).unwrap();
        assert_eq!(library.recent[0].page, 4);
        assert!(library.dark);
        assert_eq!(
            library.search_options,
            SearchOptions {
                match_case: false,
                whole_word: true
            }
        );
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
