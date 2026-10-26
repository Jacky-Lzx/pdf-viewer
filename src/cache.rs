use crate::document::RenderKey;
use eframe::egui::TextureHandle;
use std::collections::HashMap;

const BUDGET: usize = 96 * 1024 * 1024;

struct Entry {
    texture: TextureHandle,
    used: u64,
    bytes: usize,
}

#[derive(Default)]
pub struct TextureCache {
    entries: HashMap<RenderKey, Entry>,
    clock: u64,
    pub bytes: usize,
    pub hits: u64,
}

impl TextureCache {
    pub fn contains(&self, key: &RenderKey) -> bool {
        self.entries.contains_key(key)
    }
    pub fn clear(&mut self) {
        self.entries.clear();
        self.bytes = 0;
        self.hits = 0;
    }

    pub fn get(&mut self, key: &RenderKey) -> Option<TextureHandle> {
        self.clock += 1;
        let entry = self.entries.get_mut(key)?;
        entry.used = self.clock;
        self.hits += 1;
        Some(entry.texture.clone())
    }

    pub fn fallback(&mut self, key: RenderKey) -> Option<TextureHandle> {
        let best = self
            .entries
            .keys()
            .filter(|candidate| {
                candidate.page == key.page
                    && candidate.rotation == key.rotation
                    && candidate.night == key.night
            })
            .min_by_key(|candidate| candidate.width.abs_diff(key.width))
            .copied()?;
        self.get(&best)
    }

    pub fn insert(&mut self, key: RenderKey, texture: TextureHandle) {
        let bytes = texture.size()[0] * texture.size()[1] * 4;
        if let Some(old) = self.entries.remove(&key) {
            self.bytes -= old.bytes;
        }
        while self.bytes + bytes > BUDGET && !self.entries.is_empty() {
            let oldest = *self
                .entries
                .iter()
                .min_by_key(|(_, entry)| entry.used)
                .unwrap()
                .0;
            self.bytes -= self.entries.remove(&oldest).unwrap().bytes;
        }
        self.clock += 1;
        self.bytes += bytes;
        self.entries.insert(
            key,
            Entry {
                texture,
                used: self.clock,
                bytes,
            },
        );
    }
}
