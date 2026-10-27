use crate::document::RenderKey;
use eframe::egui::{TextureHandle, TextureId};
use std::{cmp::Reverse, collections::HashMap};

const BUDGET: usize = 96 * 1024 * 1024;
const MAX_ENTRIES: usize = 512;

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
    protected: Vec<RenderKey>,
}

impl Default for TextureCache {
    fn default() -> Self {
        Self {
            entries: HashMap::new(),
            clock: 0,
            bytes: 0,
            hits: 0,
            budget: BUDGET,
            protected: Vec::new(),
        }
    }
}

impl TextureCache {
    pub fn with_budget(budget: usize) -> Self {
        Self {
            budget,
            ..Self::default()
        }
    }

    pub fn budget(&self) -> usize {
        self.budget
    }

    pub fn contains(&self, key: &RenderKey) -> bool {
        self.entries.contains_key(key)
    }

    pub fn can_insert(&self, key: RenderKey, bytes: usize) -> bool {
        bytes <= self.available_for_insert(&self.pinned_for_insert(key))
    }

    fn available_for_insert(&self, pinned: &[RenderKey]) -> usize {
        let bytes: usize = pinned
            .iter()
            .filter_map(|key| self.entries.get(key))
            .map(|entry| entry.bytes)
            .sum();
        self.budget.saturating_sub(bytes)
    }
    pub fn clear(&mut self) {
        self.entries.clear();
        self.bytes = 0;
        self.hits = 0;
        self.protected.clear();
    }

    pub fn protect(&mut self, key: RenderKey) {
        self.protect_many(&[key]);
    }

    pub fn protect_many(&mut self, keys: &[RenderKey]) {
        self.protected.clear();
        self.protected.extend_from_slice(keys);
    }

    fn pinned_for_insert(&self, key: RenderKey) -> Vec<RenderKey> {
        self.protected
            .iter()
            .filter_map(|&target| {
                // A target may replace its own displayed zoom fallback.
                if target == key {
                    return None;
                }
                let pinned = if self.entries.contains_key(&target) {
                    target
                } else {
                    self.fallback_key(target)?
                };
                (pinned != key).then_some(pinned)
            })
            .collect()
    }

    /// Drawing needs only the ID; the cache retains ownership until eviction.
    pub fn get(&mut self, key: &RenderKey) -> Option<TextureId> {
        self.clock += 1;
        let entry = self.entries.get_mut(key)?;
        entry.used = self.clock;
        self.hits += 1;
        Some(entry.texture.id())
    }

    pub fn fallback(&mut self, key: RenderKey) -> Option<TextureId> {
        let best = self.fallback_key(key)?;
        self.get(&best)
    }

    fn fallback_key(&self, key: RenderKey) -> Option<RenderKey> {
        self.entries
            .keys()
            .filter(|candidate| {
                candidate.page == key.page
                    && candidate.rotation == key.rotation
                    && candidate.night == key.night
            })
            .min_by_key(|candidate| {
                (
                    candidate.width.abs_diff(key.width),
                    Reverse(candidate.width),
                )
            })
            .copied()
    }

    fn make_room(&mut self, bytes: usize, pinned: &[RenderKey]) -> bool {
        let remaining = self.budget.saturating_sub(bytes);
        let fits = |cache: &Self| cache.bytes <= remaining && cache.entries.len() < MAX_ENTRIES;
        while !fits(self) {
            let Some(oldest) = self
                .entries
                .iter()
                .filter(|(key, _)| !pinned.contains(key))
                .min_by_key(|(_, entry)| entry.used)
                .map(|(key, _)| *key)
            else {
                return false;
            };
            self.bytes -= self.entries.remove(&oldest).unwrap().bytes;
            if fits(self) {
                return true;
            }
            let protected_bytes: usize = pinned
                .iter()
                .filter_map(|key| self.entries.get(key))
                .map(|entry| entry.bytes)
                .sum();
            let eligible = self.entries.len() - pinned.len();
            let eligible_bytes = self.bytes - protected_bytes;
            // Keep ordinary single/few evictions allocation-free. A large
            // raster can instead release many entries using one sorted scan.
            if eligible < 32
                || self.bytes.saturating_sub(remaining)
                    <= (eligible_bytes / eligible).saturating_mul(8)
            {
                continue;
            }
            let mut oldest: Vec<_> = self
                .entries
                .iter()
                .filter(|(key, _)| !pinned.contains(key))
                .map(|(&key, entry)| (entry.used, key))
                .collect();
            oldest.sort_unstable_by_key(|(used, _)| *used);
            for (_, key) in oldest {
                self.bytes -= self.entries.remove(&key).unwrap().bytes;
                if fits(self) {
                    return true;
                }
            }
        }
        true
    }

    pub fn insert(&mut self, key: RenderKey, texture: TextureHandle) -> bool {
        let bytes = texture.size()[0] * texture.size()[1] * 4;
        let pinned = self.pinned_for_insert(key);
        if bytes > self.available_for_insert(&pinned) {
            return false;
        }
        if let Some(old) = self.entries.remove(&key) {
            self.bytes -= old.bytes;
        }
        if !self.make_room(bytes, &pinned) {
            return false;
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
    fn continuous_view_protects_all_visible_pages_and_their_zoom_fallbacks() {
        let ctx = Context::default();
        let mut cache = TextureCache::with_budget(192);
        for page in 0..3 {
            assert!(cache.insert(key(page), texture(&ctx)));
        }
        let targets = [
            RenderKey {
                width: 64,
                ..key(0)
            },
            RenderKey {
                width: 64,
                ..key(1)
            },
        ];
        cache.protect_many(&targets);
        // Only the invisible page may be replaced by a prefetch.
        assert!(cache.insert(key(3), texture(&ctx)));
        assert!(!cache.contains(&key(2)));
        assert!(cache.fallback(targets[0]).is_some());
        assert!(cache.fallback(targets[1]).is_some());
        assert!(!cache.can_insert(key(4), 128));
        // Each visible target can replace its own fallback, preserving the other.
        assert!(cache.insert(targets[0], texture(&ctx)));
        assert!(cache.insert(targets[1], texture(&ctx)));
        assert!(cache.contains(&targets[0]) && cache.contains(&targets[1]));
        cache.protect(key(4));
        assert!(cache.insert(key(4), texture(&ctx)));
    }

    #[test]
    fn prefetch_preserves_the_displayed_zoom_fallback_until_the_target_replaces_it() {
        let ctx = Context::default();
        let mut cache = TextureCache::with_budget(128);
        let old = key(0);
        let target = RenderKey { width: 64, ..old };
        let old_texture = ctx.load_texture(
            "old zoom",
            ColorImage::filled([8, 4], Color32::WHITE),
            TextureOptions::LINEAR,
        );
        let old_id = old_texture.id();
        cache.protect(old);
        assert!(cache.insert(old, old_texture));
        cache.protect(target);
        assert_eq!(cache.fallback(target), Some(old_id));
        assert!(
            !cache.can_insert(key(1), 64),
            "prefetch must leave the displayed fallback usable"
        );
        assert!(!cache.insert(key(1), texture(&ctx)));
        assert_eq!(cache.get(&old), Some(old_id));
        assert_eq!(
            ctx.tex_manager().read().meta(old_id).unwrap().retain_count,
            1
        );

        // The desired raster can replace that fallback even if both cannot fit.
        let new_texture = ctx.load_texture(
            "new zoom",
            ColorImage::filled([8, 4], Color32::WHITE),
            TextureOptions::LINEAR,
        );
        let new_id = new_texture.id();
        assert!(cache.can_insert(target, 128));
        assert!(cache.insert(target, new_texture));
        assert_eq!(cache.get(&target), Some(new_id));
        assert!(ctx.tex_manager().read().meta(old_id).is_none());
        assert_eq!(cache.bytes, 128);
        assert!(!cache.can_insert(key(1), 64));
    }

    #[test]
    fn prefetch_can_retry_after_the_smaller_target_is_available() {
        let ctx = Context::default();
        let mut cache = TextureCache::with_budget(192);
        let old = key(0);
        let target = RenderKey { width: 64, ..old };
        let large = || {
            ctx.load_texture(
                "large",
                ColorImage::filled([8, 4], Color32::WHITE),
                TextureOptions::LINEAR,
            )
        };
        assert!(cache.insert(old, large()));
        cache.protect(target);
        assert!(!cache.can_insert(key(1), 128));
        assert!(!cache.insert(key(1), large()));
        assert!(cache.insert(target, texture(&ctx)));
        assert!(cache.can_insert(key(1), 128));
        assert!(cache.insert(key(1), large()));
        assert!(cache.contains(&target) && cache.contains(&key(1)));
        assert!(!cache.contains(&old));
        assert_eq!(cache.bytes, 192);
    }

    #[test]
    fn equidistant_fallbacks_prefer_the_sharper_texture_and_protect_that_same_entry() {
        let ctx = Context::default();
        for reverse in [false, true] {
            let mut cache = TextureCache::with_budget(192);
            let small = RenderKey {
                width: 64,
                ..key(0)
            };
            let large = RenderKey {
                width: 128,
                ..key(0)
            };
            for candidate in if reverse {
                [large, small]
            } else {
                [small, large]
            } {
                assert!(cache.insert(candidate, texture(&ctx)));
            }
            let sharper = cache.get(&large).unwrap();
            let target = RenderKey {
                width: 96,
                ..key(0)
            };
            cache.protect(target);
            assert_eq!(cache.fallback(target), Some(sharper));
            cache.get(&small); // The displayed fallback is now older than this entry.
            let prefetch = ctx.load_texture(
                "prefetch",
                ColorImage::filled([8, 4], Color32::WHITE),
                TextureOptions::LINEAR,
            );
            assert!(cache.insert(key(1), prefetch));
            assert!(!cache.contains(&small));
            assert_eq!(cache.fallback(target), Some(sharper));
            assert_eq!(cache.bytes, 192);
        }
    }

    #[test]
    fn drawing_ids_keep_cache_ownership_and_release_on_eviction() {
        let ctx = Context::default();
        let mut cache = TextureCache::with_budget(64);
        let first = texture(&ctx);
        let id = first.id();
        assert!(cache.insert(key(0), first));
        let drawn = cache.get(&key(0)).unwrap();
        assert_eq!(drawn, id);
        assert_eq!(
            cache.fallback(RenderKey {
                width: 64,
                ..key(0)
            }),
            Some(id)
        );
        assert_eq!(ctx.tex_manager().read().meta(id).unwrap().retain_count, 1);
        assert!(cache.insert(key(1), texture(&ctx)));
        assert!(ctx.tex_manager().read().meta(id).is_none());
        let second = cache.get(&key(1)).unwrap();
        cache.clear();
        assert!(ctx.tex_manager().read().meta(second).is_none());
    }

    #[test]
    fn tiny_texture_count_is_bounded_without_losing_zoom_fallbacks_or_replacements() {
        let ctx = Context::default();
        let mut cache = TextureCache::default();
        let mut ids = Vec::new();
        for page in 0..512 {
            let image = texture(&ctx);
            ids.push(image.id());
            assert!(cache.insert(key(page), image));
        }
        assert!(cache.bytes < cache.budget);
        let target = RenderKey {
            width: 64,
            ..key(0)
        };
        cache.protect(target);
        assert_eq!(cache.fallback_key(target), Some(key(0)));
        assert!(cache.can_insert(key(512), 64));
        assert!(cache.insert(key(512), texture(&ctx)));
        assert_eq!(cache.entries.len(), 512);
        assert_eq!(cache.bytes, 512 * 64);
        assert!(cache.contains(&key(0)) && cache.contains(&key(512)));
        assert!(!cache.contains(&key(1)));
        assert!(ctx.tex_manager().read().meta(ids[0]).is_some());
        assert!(ctx.tex_manager().read().meta(ids[1]).is_none());

        // Replacing an existing key at the count limit needs no extra eviction.
        let old = cache.get(&key(512)).unwrap();
        assert!(cache.insert(key(512), texture(&ctx)));
        assert_eq!(cache.entries.len(), 512);
        assert!(cache.contains(&key(2)));
        assert!(ctx.tex_manager().read().meta(old).is_none());

        // The desired texture may now replace the old, pinned zoom fallback.
        assert!(cache.insert(target, texture(&ctx)));
        assert_eq!(cache.entries.len(), 512);
        assert_eq!(cache.bytes, 512 * 64);
        assert!(!cache.contains(&key(0)));
        assert!(cache.contains(&target) && cache.contains(&key(2)));
        assert!(ctx.tex_manager().read().meta(ids[0]).is_none());
    }

    #[test]
    fn batch_eviction_keeps_the_oldest_visible_texture_or_fallback_and_the_newest_page() {
        for fallback in [false, true] {
            let ctx = Context::default();
            let mut cache = TextureCache::with_budget(64 * 64);
            let mut ids = Vec::new();
            for page in 0..64 {
                let image = texture(&ctx);
                ids.push(image.id());
                assert!(cache.insert(key(page), image));
            }
            cache.protect(RenderKey {
                width: if fallback { 64 } else { 32 },
                ..key(0)
            });
            cache.get(&key(1));
            let incoming = ctx.load_texture(
                "large page",
                ColorImage::filled([8, 64], Color32::WHITE),
                TextureOptions::LINEAR,
            );
            let id = incoming.id();
            assert!(cache.insert(key(64), incoming));
            assert_eq!(cache.bytes, cache.budget);
            assert_eq!(cache.entries.len(), 33);
            assert_eq!(cache.get(&key(64)), Some(id));
            for (page, id) in ids.into_iter().enumerate() {
                let retained = !(2..=33).contains(&page);
                assert_eq!(cache.contains(&key(page)), retained, "page {page}");
                assert_eq!(ctx.tex_manager().read().meta(id).is_some(), retained);
            }

            // The desired raster can release the entire old set, including
            // its displayed fallback, to occupy the full byte budget itself.
            let target = RenderKey {
                width: 64,
                ..key(0)
            };
            cache.protect(target);
            let incoming = ctx.load_texture(
                "full budget target",
                ColorImage::filled([16, 64], Color32::WHITE),
                TextureOptions::LINEAR,
            );
            let target_id = incoming.id();
            assert!(cache.insert(target, incoming));
            assert_eq!(cache.entries.len(), 1);
            assert_eq!(cache.bytes, cache.budget);
            assert_eq!(cache.get(&target), Some(target_id));
            assert!(ctx.tex_manager().read().meta(id).is_none());
        }
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
    fn impossible_insert_preserves_other_useful_cached_pages() {
        let ctx = Context::default();
        let mut cache = TextureCache {
            budget: 192,
            ..Default::default()
        };
        let large = || {
            ctx.load_texture(
                "large",
                ColorImage::filled([8, 4], Color32::WHITE),
                TextureOptions::LINEAR,
            )
        };
        cache.protect(key(0));
        assert!(cache.insert(key(0), large()));
        assert!(cache.insert(key(1), texture(&ctx)));
        assert!(!cache.can_insert(key(2), 128));
        assert!(!cache.insert(key(2), large()));
        assert!(cache.contains(&key(0)));
        assert!(cache.contains(&key(1)));
        assert_eq!(cache.bytes, 192);
        assert!(cache.can_insert(key(0), 128));
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
