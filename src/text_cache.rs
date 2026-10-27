use crate::text::PageText;
use std::{collections::HashMap, sync::Arc};

const BUDGET: usize = 32 * 1024 * 1024;
const MAX_ENTRIES: usize = 512;

struct Entry {
    text: Arc<PageText>,
    used: u64,
    bytes: usize,
}

pub struct TextCache {
    entries: HashMap<usize, Entry>,
    clock: u64,
    bytes: usize,
    budget: usize,
    limit: usize,
}

fn memory_bytes(text: &PageText) -> usize {
    text.memory_bytes()
        + std::mem::size_of::<PageText>()
        + std::mem::size_of::<Entry>()
        + 3 * std::mem::size_of::<usize>()
}

impl Default for TextCache {
    fn default() -> Self {
        Self {
            entries: HashMap::new(),
            clock: 0,
            bytes: 0,
            budget: BUDGET,
            limit: MAX_ENTRIES,
        }
    }
}

impl TextCache {
    pub fn get(&mut self, page: usize) -> Option<Arc<PageText>> {
        let entry = self.entries.get_mut(&page)?;
        self.clock += 1;
        entry.used = self.clock;
        Some(Arc::clone(&entry.text))
    }

    fn remove(&mut self, page: usize) {
        if let Some(entry) = self.entries.remove(&page) {
            self.bytes -= entry.bytes;
        }
    }

    fn evict_oldest(&mut self, protected: Option<usize>) {
        let page = self
            .entries
            .iter()
            .filter(|(page, _)| Some(**page) != protected)
            .min_by_key(|(_, entry)| entry.used)
            .map(|(&page, _)| page);
        if let Some(page) = page {
            self.remove(page);
        }
    }

    fn make_room(&mut self, bytes: usize, entries: usize, protected: Option<usize>) {
        let remaining = self.budget.saturating_sub(bytes);
        let fits = |cache: &Self| cache.bytes <= remaining && cache.entries.len() <= entries;
        while !fits(self) {
            self.evict_oldest(protected);
            if fits(self) {
                return;
            }
            let pinned = protected.and_then(|page| self.entries.get(&page));
            let eligible = self.entries.len() - usize::from(pinned.is_some());
            let eligible_bytes = self.bytes - pinned.map_or(0, |entry| entry.bytes);
            // A few removals need no allocation. For a large byte deficit,
            // sort remaining candidates once instead of repeatedly scanning.
            if eligible < 32
                || self.bytes.saturating_sub(remaining)
                    <= (eligible_bytes / eligible).saturating_mul(8)
            {
                continue;
            }
            let mut oldest: Vec<_> = self
                .entries
                .iter()
                .filter(|(page, _)| Some(**page) != protected)
                .map(|(&page, entry)| (entry.used, page))
                .collect();
            oldest.sort_unstable_by_key(|(used, _)| *used);
            for (_, page) in oldest {
                self.remove(page);
                if fits(self) {
                    return;
                }
            }
        }
    }

    pub fn insert(&mut self, page: usize, text: Arc<PageText>) -> bool {
        let bytes = memory_bytes(&text);
        // An uncached large page must not flush other useful entries.
        if bytes > self.budget || self.limit == 0 {
            return false;
        }
        self.remove(page);
        self.make_room(bytes, self.limit - 1, None);
        self.clock += 1;
        self.bytes += bytes;
        self.entries.insert(
            page,
            Entry {
                text,
                used: self.clock,
                bytes,
            },
        );
        true
    }

    /// Replace a lazy reservation; previously oversized text may now fit too.
    pub fn refresh(&mut self, page: usize, text: &Arc<PageText>) {
        let Some(entry) = self.entries.get_mut(&page) else {
            self.insert(page, Arc::clone(text));
            return;
        };
        let bytes = memory_bytes(&entry.text);
        if bytes > self.budget {
            self.remove(page);
            return;
        }
        self.bytes = self.bytes - entry.bytes + bytes;
        entry.bytes = bytes;
        self.make_room(0, self.limit, Some(page));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn page(value: &str) -> Arc<PageText> {
        let mut text = PageText::default();
        text.text = value.into();
        Arc::new(text)
    }

    #[test]
    fn evicts_only_the_least_recently_used_page_and_keeps_shared_text_alive() {
        let first = page("first");
        let second = page("other");
        let bytes = memory_bytes(&first);
        let mut cache = TextCache {
            budget: bytes * 2,
            ..Default::default()
        };
        assert!(cache.insert(0, Arc::clone(&first)));
        assert!(cache.insert(1, second));
        let held = cache.get(1).unwrap();
        assert!(Arc::ptr_eq(&cache.get(0).unwrap(), &first));
        assert!(cache.insert(2, page("third")));
        assert!(cache.get(1).is_none());
        assert!(cache.get(0).is_some() && cache.get(2).is_some());
        assert_eq!(cache.bytes, bytes * 2);
        assert_eq!(first.text, "first");
        assert_eq!(
            held.text, "other",
            "eviction must not invalidate active selection references"
        );
    }

    #[test]
    fn a_large_insertion_evicts_exactly_the_oldest_pages_and_keeps_the_touched_page() {
        let small = page("small");
        let unit = memory_bytes(&small);
        let incoming = page(&"x".repeat(4000));
        let removed = memory_bytes(&incoming).div_ceil(unit);
        assert!((9..64).contains(&removed));
        let mut cache = TextCache {
            budget: unit * 64,
            ..Default::default()
        };
        for index in 0..64 {
            assert!(cache.insert(index, Arc::clone(&small)));
        }
        cache.get(0).unwrap();
        assert!(cache.insert(64, Arc::clone(&incoming)));
        assert!(cache.bytes <= cache.budget);
        assert!(Arc::ptr_eq(&cache.get(64).unwrap(), &incoming));
        for index in 0..64 {
            assert_eq!(
                cache.get(index).is_some(),
                index == 0 || index > removed,
                "page {index}"
            );
        }
        assert_eq!(cache.bytes, unit * (64 - removed) + memory_bytes(&incoming));
    }

    #[test]
    fn large_index_growth_keeps_the_protected_oldest_page_and_evicts_others_in_order() {
        let mut active = PageText::default();
        active.text = "x".repeat(1000);
        active.spans = (0..1000)
            .map(|index| crate::text::TextSpan {
                range: index..index + 1,
                rect: hayro::kurbo::Rect::new(
                    (index % 100) as f64 * 8.0,
                    (index / 100) as f64 * 16.0,
                    (index % 100) as f64 * 8.0 + 6.0,
                    (index / 100) as f64 * 16.0 + 10.0,
                ),
            })
            .collect();
        let active = Arc::new(active);
        let before = memory_bytes(&active);
        let small = page("small");
        let unit = memory_bytes(&small);
        let mut cache = TextCache {
            budget: before + unit * 128,
            ..Default::default()
        };
        assert!(cache.insert(0, Arc::clone(&active)));
        for index in 1..=128 {
            assert!(cache.insert(index, Arc::clone(&small)));
        }
        active.prepare_selection();
        let after = memory_bytes(&active);
        let removed = (after - before).div_ceil(unit);
        assert!((9..128).contains(&removed));
        cache.refresh(0, &active);
        assert!(cache.bytes <= cache.budget);
        assert!(Arc::ptr_eq(&cache.get(0).unwrap(), &active));
        for index in 1..=128 {
            assert_eq!(cache.get(index).is_some(), index > removed, "page {index}");
        }
        assert_eq!(cache.bytes, after + unit * (128 - removed));
    }

    #[test]
    fn oversized_pages_do_not_evict_useful_cached_text() {
        let first = page("first");
        let mut cache = TextCache {
            budget: memory_bytes(&first),
            ..Default::default()
        };
        cache.insert(0, Arc::clone(&first));
        assert!(!cache.insert(1, page(&"x".repeat(1000))));
        assert!(Arc::ptr_eq(&cache.get(0).unwrap(), &first));
    }

    #[test]
    fn lazy_index_refresh_releases_unused_reservation_and_uses_actual_capacity() {
        let text = page(&"Rust Σ".repeat(100));
        let reserved = memory_bytes(&text);
        let mut cache = TextCache {
            budget: reserved,
            ..Default::default()
        };
        cache.insert(0, Arc::clone(&text));
        text.search(0, "missing");
        cache.refresh(0, &text);
        assert_eq!(cache.bytes, memory_bytes(&text));
        assert!(cache.bytes <= reserved);
        let remaining = cache.budget - cache.bytes;
        assert!(memory_bytes(&page("a")) < remaining);
        assert!(cache.insert(1, page("a")));
        assert!(cache.get(0).is_some());
    }

    #[test]
    fn an_uncached_unicode_page_is_admitted_after_its_reservation_shrinks() {
        let value = "中文 Σσς École 👩‍💻 ".repeat(1000);
        let text = page(&value);
        let prepared = page(&value);
        prepared.search(1, "missing");
        let kept = page("kept");
        let mut cache = TextCache {
            budget: memory_bytes(&prepared) + memory_bytes(&kept),
            ..Default::default()
        };
        assert!(memory_bytes(&text) > cache.budget);
        assert!(cache.insert(0, Arc::clone(&kept)));
        assert!(!cache.insert(1, Arc::clone(&text)));
        assert!(cache.get(0).is_some());
        assert_eq!(text.search(1, "中文").len(), 1000);
        cache.refresh(1, &text);
        assert!(Arc::ptr_eq(&cache.get(1).unwrap(), &text));
        assert!(Arc::ptr_eq(&cache.get(0).unwrap(), &kept));
        assert_eq!(cache.bytes, cache.budget);
    }

    #[test]
    fn blank_pages_are_bounded_by_entry_count_too() {
        let mut cache = TextCache {
            limit: 3,
            ..Default::default()
        };
        for index in 0..100 {
            cache.insert(index, page(""));
        }
        assert_eq!(cache.entries.len(), 3);
        assert!(cache.get(96).is_none() && cache.get(97).is_some());
        assert_eq!(cache.bytes, memory_bytes(&PageText::default()) * 3);
    }

    #[test]
    fn whole_word_boundary_capacity_is_accounted_after_search() {
        let text = page(&"a ".repeat(1000));
        let query = crate::text::SearchQuery::new(
            "a".into(),
            crate::text::SearchOptions {
                match_case: true,
                whole_word: true,
            },
        );
        let before = memory_bytes(&text);
        let mut cache = TextCache {
            budget: before,
            ..Default::default()
        };
        assert!(cache.insert(0, Arc::clone(&text)));
        assert_eq!(text.search_with(0, &query).len(), 1000);
        assert!(memory_bytes(&text) > before);
        cache.refresh(0, &text);
        assert!(cache.get(0).is_none());
        assert_eq!(cache.bytes, 0);
        assert_eq!(text.search_with(0, &query).len(), 1000);
    }

    #[test]
    fn selection_index_growth_is_accounted_and_held_text_survives_eviction() {
        let mut dense = PageText::default();
        dense.text = "a".repeat(1000);
        dense.spans = (0..1000)
            .map(|index| crate::text::TextSpan {
                range: index..index + 1,
                rect: hayro::kurbo::Rect::new(
                    (index % 100) as f64 * 8.0,
                    (index / 100) as f64 * 16.0,
                    (index % 100) as f64 * 8.0 + 6.0,
                    (index / 100) as f64 * 16.0 + 10.0,
                ),
            })
            .collect();
        let dense = Arc::new(dense);
        let before = memory_bytes(&dense);
        let mut cache = TextCache {
            budget: before,
            ..Default::default()
        };
        assert!(cache.insert(0, Arc::clone(&dense)));
        dense.prepare_selection();
        assert!(memory_bytes(&dense) > before);
        cache.refresh(0, &dense);
        assert!(cache.get(0).is_none());
        assert_eq!(cache.bytes, 0);
        assert_eq!(
            dense.nearest_span(hayro::kurbo::Point::new(13.0, 20.0)),
            Some(101)
        );
    }
}
