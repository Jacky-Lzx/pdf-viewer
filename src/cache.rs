use crate::document::RenderKey;
use eframe::egui::TextureHandle;
use std::collections::HashMap;

const BUDGET: usize = 96 * 1024 * 1024;

struct Entry {
    texture: TextureHandle,
    used: u64,
    bytes: usize,
}

pub struct TextureCache {
    entries: HashMap<RenderKey, Entry>,
    clock: u64,
    pub bytes: usize,
    pub hits: u64,
    budget: usize,
    protected: Option<RenderKey>,
}

impl Default for TextureCache {
    fn default() -> Self {
        Self {
            entries: HashMap::new(),
            clock: 0,
            bytes: 0,
            hits: 0,
            budget: BUDGET,
            protected: None,
        }
    }
}

impl TextureCache {
    pub fn contains(&self, key: &RenderKey) -> bool {
        self.entries.contains_key(key)
    }
    pub fn clear(&mut self) {
        self.entries.clear();
        self.bytes = 0;
        self.hits = 0;
        self.protected = None;
    }

    pub fn protect(&mut self, key: RenderKey) {
        self.protected = Some(key);
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

    pub fn insert(&mut self, key: RenderKey, texture: TextureHandle) -> bool {
        let bytes = texture.size()[0] * texture.size()[1] * 4;
        if let Some(old) = self.entries.remove(&key) {
            self.bytes -= old.bytes;
        }
        while self.bytes + bytes > self.budget && !self.entries.is_empty() {
            let Some(oldest) = self
                .entries
                .iter()
                .filter(|(key, _)| Some(**key) != self.protected)
                .min_by_key(|(_, entry)| entry.used)
                .map(|(key, _)| *key)
            else {
                return false;
            };
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
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use eframe::egui::{Color32, ColorImage, Context, TextureOptions};

    fn key(page: usize) -> RenderKey {
        RenderKey {
            page,
            width: 32,
            rotation: 0,
            night: false,
        }
    }
    fn texture(ctx: &Context) -> TextureHandle {
        ctx.load_texture(
            "test",
            ColorImage::filled([4, 4], Color32::WHITE),
            TextureOptions::LINEAR,
        )
    }

    #[test]
    fn prefetch_never_evicts_the_visible_page() {
        let ctx = Context::default();
        let mut cache = TextureCache {
            budget: 64,
            ..Default::default()
        };
        cache.protect(key(0));
        assert!(cache.insert(key(0), texture(&ctx)));
        assert!(!cache.insert(key(1), texture(&ctx)));
        assert!(cache.contains(&key(0)));
        assert_eq!(cache.bytes, 64);
        cache.protect(key(1));
        assert!(cache.insert(key(1), texture(&ctx)));
        assert!(!cache.contains(&key(0)));
    }

    #[test]
    fn eviction_respects_recency_and_fallback_requires_matching_orientation() {
        let ctx = Context::default();
        let mut cache = TextureCache {
            budget: 128,
            ..Default::default()
        };
        cache.insert(key(0), texture(&ctx));
        cache.insert(key(1), texture(&ctx));
        cache.get(&key(0));
        cache.insert(key(2), texture(&ctx));
        assert!(cache.contains(&key(0)));
        assert!(!cache.contains(&key(1)));
        assert!(
            cache
                .fallback(RenderKey {
                    width: 128,
                    ..key(0)
                })
                .is_some()
        );
        assert!(
            cache
                .fallback(RenderKey {
                    rotation: 1,
                    ..key(0)
                })
                .is_none()
        );
        cache.clear();
        assert_eq!(cache.bytes, 0);
    }
}
