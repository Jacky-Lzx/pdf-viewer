use crate::document::RenderKey;
use eframe::egui::{TextureHandle, TextureId};
use std::{
    cmp::Reverse,
    collections::{HashMap, hash_map::Entry as MapEntry},
};

const BUDGET: usize = 96 * 1024 * 1024;
const MAX_ENTRIES: usize = 512;

struct Entry {
    texture: TextureHandle,
    used: u64,
    bytes: usize,
}

struct Pinned {
    keys: Vec<RenderKey>,
    bytes: usize,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct FullPageWidths {
    first: u32,
    others: Vec<u32>,
}

impl FullPageWidths {
    fn iter(&self) -> impl Iterator<Item = &u32> {
        std::iter::once(&self.first).chain(&self.others)
    }

    /// Return true when removing the last resident width for this page/style.
    fn remove(&mut self, width: u32) -> bool {
        if self.first == width {
            let Some(next) = self.others.pop() else {
                return true;
            };
            self.first = next;
        } else {
            let index = self
                .others
                .iter()
                .position(|&candidate| candidate == width)
                .unwrap();
            self.others.swap_remove(index);
        }
        if self.others.is_empty() {
            self.others = Vec::new();
        }
        false
    }
}

pub struct TextureCache {
    entries: HashMap<RenderKey, Entry>,
    // Only full-page rasters participate in zoom fallback selection. Most
    // pages have one or two widths, independent of the cache's tile count.
    full_pages: Option<HashMap<(usize, u8, bool), FullPageWidths>>,
    clock: u64,
    pub bytes: usize,
    pub hits: u64,
    budget: usize,
    protected: Vec<RenderKey>,
}

impl Default for TextureCache {
    fn default() -> Self {
        Self::with_budget(BUDGET)
    }
}

impl TextureCache {
    pub fn with_budget(budget: usize) -> Self {
        Self {
            entries: HashMap::new(),
            full_pages: Some(HashMap::new()),
            clock: 0,
            bytes: 0,
            hits: 0,
            budget,
            protected: Vec::new(),
        }
    }

    /// Previews use exact keys and never display another zoom's raster.
    pub fn exact_with_budget(budget: usize) -> Self {
        Self {
            budget,
            full_pages: None,
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
        if bytes > self.budget {
            return false;
        }
        // Admission only needs the protected byte total. Build the eviction
        // key list later, when an actual insertion can use it.
        let pinned_bytes = self.pinned_entries(key).map(|(_, entry)| entry.bytes).sum();
        bytes <= self.budget.saturating_sub(pinned_bytes)
    }

    fn available_for_insert(&self, pinned: &Pinned) -> usize {
        self.budget.saturating_sub(pinned.bytes)
    }
    pub fn clear(&mut self) {
        self.entries.clear();
        if let Some(full_pages) = &mut self.full_pages {
            full_pages.clear();
        }
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

    fn pinned_entries(&self, key: RenderKey) -> impl Iterator<Item = (RenderKey, &Entry)> {
        self.protected.iter().filter_map(move |&target| {
            // A target may replace its own displayed zoom fallback.
            if target == key {
                return None;
            }
            let resident = self.entries.get(&target);
            let pinned = if resident.is_some() {
                target
            } else {
                self.fallback_key(target)?
            };
            if pinned == key {
                return None;
            }
            Some((pinned, resident.or_else(|| self.entries.get(&pinned))?))
        })
    }

    fn pinned_for_insert(&self, key: RenderKey) -> Pinned {
        let mut bytes = 0;
        let keys = self
            .pinned_entries(key)
            .map(|(key, entry)| {
                bytes += entry.bytes;
                key
            })
            .collect();
        Pinned { keys, bytes }
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
        if key.tile.is_some() {
            return None;
        }
        let &width = self
            .full_pages
            .as_ref()?
            .get(&(key.page, key.rotation, key.night))?
            .iter()
            .min_by_key(|&&width| (width.abs_diff(key.width), Reverse(width)))?;
        Some(RenderKey { width, ..key })
    }

    /// Every replacement and eviction must update bytes and fallback widths
    /// together so indexed candidates always have a resident texture.
    fn remove(&mut self, key: RenderKey) {
        let Some(entry) = self.entries.remove(&key) else {
            return;
        };
        self.bytes -= entry.bytes;
        if key.tile.is_none()
            && let Some(full_pages) = &mut self.full_pages
        {
            let group = (key.page, key.rotation, key.night);
            let MapEntry::Occupied(mut widths) = full_pages.entry(group) else {
                unreachable!("full-page texture must have indexed widths");
            };
            if widths.get_mut().remove(key.width) {
                widths.remove();
            }
        }
    }

    fn make_room(&mut self, bytes: usize, pinned: &Pinned) -> bool {
        let remaining = self.budget.saturating_sub(bytes);
        let fits = |cache: &Self| cache.bytes <= remaining && cache.entries.len() < MAX_ENTRIES;
        while !fits(self) {
            let Some(oldest) = self
                .entries
                .iter()
                .filter(|(key, _)| !pinned.keys.contains(key))
                .min_by_key(|(_, entry)| entry.used)
                .map(|(key, _)| *key)
            else {
                return false;
            };
            self.remove(oldest);
            if fits(self) {
                return true;
            }
            let eligible = self.entries.len() - pinned.keys.len();
            let eligible_bytes = self.bytes - pinned.bytes;
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
                .filter(|(key, _)| !pinned.keys.contains(key))
                .map(|(&key, entry)| (entry.used, key))
                .collect();
            oldest.sort_unstable_by_key(|(used, _)| *used);
            for (_, key) in oldest {
                self.remove(key);
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
        self.remove(key);
        if !self.make_room(bytes, &pinned) {
            return false;
        }
        self.clock += 1;
        self.bytes += bytes;
        if key.tile.is_none()
            && let Some(full_pages) = &mut self.full_pages
        {
            full_pages
                .entry((key.page, key.rotation, key.night))
                .and_modify(|widths| widths.others.push(key.width))
                .or_insert_with(|| FullPageWidths {
                    first: key.width,
                    others: Vec::new(),
                });
        }
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
            tile: None,
        }
    }
    fn texture(ctx: &Context) -> TextureHandle {
        ctx.load_texture(
            "test",
            ColorImage::filled([4, 4], Color32::WHITE),
            TextureOptions::LINEAR,
        )
    }

    fn assert_fallback_index(cache: &TextureCache) {
        let mut expected: HashMap<_, Vec<_>> = HashMap::new();
        for key in cache.entries.keys().filter(|key| key.tile.is_none()) {
            expected
                .entry((key.page, key.rotation, key.night))
                .or_default()
                .push(key.width);
        }
        let mut actual: HashMap<_, Vec<_>> = cache
            .full_pages
            .as_ref()
            .unwrap()
            .iter()
            .map(|(&group, widths)| (group, widths.iter().copied().collect()))
            .collect();
        for widths in expected.values_mut().chain(actual.values_mut()) {
            widths.sort_unstable();
        }
        assert_eq!(actual, expected);
        for page in 0..6 {
            for rotation in 0..4 {
                for night in [false, true] {
                    for width in [1, 32, 48, 64, 96, 128, 256, 1024] {
                        let target = RenderKey {
                            page,
                            rotation,
                            night,
                            width,
                            tile: None,
                        };
                        let expected = cache
                            .entries
                            .keys()
                            .filter(|candidate| {
                                candidate.tile.is_none()
                                    && candidate.page == page
                                    && candidate.rotation == rotation
                                    && candidate.night == night
                            })
                            .min_by_key(|candidate| {
                                (candidate.width.abs_diff(width), Reverse(candidate.width))
                            })
                            .copied();
                        assert_eq!(cache.fallback_key(target), expected, "{target:?}");
                    }
                }
            }
        }
    }

    #[test]
    fn fallback_index_tracks_mixed_tiles_replacements_evictions_and_clear() {
        let context = Context::default();
        let mut cache = TextureCache::with_budget(64 * 64);
        for index in 0..120 {
            let key = RenderKey {
                page: index % 6,
                width: [32, 64, 96, 128][index / 6 % 4],
                rotation: (index / 24 % 4) as u8,
                night: index % 2 == 1,
                tile: (index % 4 == 0).then_some(crate::document::Tile {
                    scale_bits: 2.0_f32.to_bits(),
                    x: index as u32,
                    y: 0,
                    size: [4, 4],
                }),
            };
            assert!(cache.insert(key, texture(&context)));
            if index % 7 == 0 {
                assert!(cache.insert(key, texture(&context)));
            }
            if index % 10 == 0 {
                assert_fallback_index(&cache);
            }
        }
        assert_fallback_index(&cache);
        let target = RenderKey {
            width: 80,
            ..*cache.entries.keys().find(|key| key.tile.is_none()).unwrap()
        };
        let displayed = cache.fallback_key(target).unwrap();
        cache.protect(target);
        let image = context.load_texture(
            "batch eviction",
            ColorImage::filled([8, 64], Color32::WHITE),
            TextureOptions::LINEAR,
        );
        assert!(cache.insert(key(1000), image));
        assert!(cache.contains(&displayed));
        assert_eq!(cache.fallback_key(target), Some(displayed));
        assert_fallback_index(&cache);
        let before = cache.full_pages.clone();
        let bytes = cache.bytes;
        let image = context.load_texture(
            "over budget",
            ColorImage::filled([64, 64], Color32::WHITE),
            TextureOptions::LINEAR,
        );
        assert!(!cache.insert(key(1001), image));
        assert_eq!(cache.full_pages, before);
        assert_eq!(cache.bytes, bytes);
        assert_fallback_index(&cache);
        cache.clear();
        assert_fallback_index(&cache);
        assert!(cache.full_pages.as_ref().unwrap().is_empty());
        assert!(cache.fallback_key(target).is_none());
    }

    #[test]
    fn admission_matches_insertion_with_exact_fallback_and_tile_protection() {
        let context = Context::default();
        let tile = RenderKey {
            tile: Some(crate::document::Tile {
                scale_bits: 2.0_f32.to_bits(),
                x: 0,
                y: 0,
                size: [4, 4],
            }),
            ..key(2)
        };
        let target = RenderKey {
            width: 64,
            ..key(0)
        };
        for exact in [false, true] {
            for budget in [192, 256] {
                for incoming in [
                    target,
                    key(0),
                    key(1),
                    tile,
                    RenderKey {
                        tile: Some(crate::document::Tile {
                            x: 4,
                            ..tile.tile.unwrap()
                        }),
                        ..tile
                    },
                    key(3),
                    RenderKey {
                        rotation: 1,
                        ..key(0)
                    },
                ] {
                    for size in [[1, 1], [4, 4], [8, 4], [16, 16]] {
                        let mut cache = if exact {
                            TextureCache::exact_with_budget(budget)
                        } else {
                            TextureCache::with_budget(budget)
                        };
                        for resident in [key(0), key(1), tile] {
                            assert!(cache.insert(resident, texture(&context)));
                        }
                        cache.protect_many(&[target, key(1), tile, key(4)]);
                        assert!(cache.can_insert(incoming, 0));
                        let admitted = cache.can_insert(incoming, size[0] * size[1] * 4);
                        let image = context.load_texture(
                            "admission",
                            ColorImage::filled(size, Color32::WHITE),
                            TextureOptions::LINEAR,
                        );
                        assert_eq!(
                            cache.insert(incoming, image),
                            admitted,
                            "exact={exact}, budget={budget}, incoming={incoming:?}, size={size:?}"
                        );
                        assert!(cache.bytes <= budget);
                    }
                }
            }
        }
    }

    #[test]
    fn exact_cache_skips_fallback_index_but_still_protects_exact_textures() {
        let context = Context::default();
        let mut cache = TextureCache::exact_with_budget(128);
        assert!(cache.insert(key(0), texture(&context)));
        let target = RenderKey {
            width: 64,
            ..key(0)
        };
        assert!(cache.fallback(target).is_none());
        cache.protect(target);
        assert!(
            cache.can_insert(key(1), 128),
            "absent exact key needs no fallback reservation"
        );
        cache.protect(key(0));
        assert!(!cache.can_insert(key(1), 128));
        assert!(cache.insert(key(1), texture(&context)));
        assert!(cache.contains(&key(0)));
        cache.clear();
        assert!(cache.full_pages.is_none());
        assert!(cache.insert(key(0), texture(&context)));
        assert!(cache.fallback(target).is_none());
    }

    #[test]
    fn viewport_tiles_never_substitute_for_full_pages_or_other_tiles() {
        let ctx = Context::default();
        let mut cache = TextureCache::default();
        let tile = RenderKey {
            tile: Some(crate::document::Tile {
                scale_bits: 16.0_f32.to_bits(),
                x: 0,
                y: 0,
                size: [16, 16],
            }),
            ..key(0)
        };
        assert!(cache.insert(tile, texture(&ctx)));
        assert!(cache.fallback(key(0)).is_none());
        let adjacent = RenderKey {
            tile: Some(crate::document::Tile {
                x: 16,
                ..tile.tile.unwrap()
            }),
            ..tile
        };
        assert!(cache.fallback(adjacent).is_none());
        assert!(cache.insert(key(0), texture(&ctx)));
        assert!(cache.fallback(adjacent).is_none());
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
