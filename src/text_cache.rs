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

    pub fn insert(&mut self, page: usize, text: Arc<PageText>) -> bool {
        let bytes = memory_bytes(&text);
        // An uncached large page must not flush other useful entries.
        if bytes > self.budget || self.limit == 0 {
            return false;
        }
        self.remove(page);
        while self.bytes + bytes > self.budget || self.entries.len() >= self.limit {
            self.evict_oldest(None);
        }
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

    /// Replace a lazy index reservation with its actual string/vector capacities.
    pub fn refresh(&mut self, page: usize) {
        let Some(entry) = self.entries.get_mut(&page) else {
            return;
        };
        let bytes = memory_bytes(&entry.text);
        if bytes > self.budget {
            self.remove(page);
            return;
        }
        self.bytes = self.bytes - entry.bytes + bytes;
        entry.bytes = bytes;
        while self.bytes > self.budget {
            self.evict_oldest(Some(page));
        }
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
        cache.refresh(0);
        assert_eq!(cache.bytes, memory_bytes(&text));
        assert!(cache.bytes <= reserved);
        let remaining = cache.budget - cache.bytes;
        assert!(memory_bytes(&page("a")) < remaining);
        assert!(cache.insert(1, page("a")));
        assert!(cache.get(0).is_some());
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
}
