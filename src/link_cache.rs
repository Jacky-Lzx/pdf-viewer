use crate::links::{LinkTarget, PageLink};
use std::{cell::Cell, collections::HashMap};

const BUDGET: usize = 4 * 1024 * 1024;
const MAX_ENTRIES: usize = 64;

struct Entry {
    links: Vec<PageLink>,
    used: Cell<u64>,
    bytes: usize,
}

pub struct LinkCache {
    entries: HashMap<usize, Entry>,
    clock: Cell<u64>,
    bytes: usize,
    budget: usize,
    limit: usize,
    // A large current page remains usable without evicting reusable entries.
    oversized: Option<(usize, Vec<PageLink>)>,
}

fn memory_bytes(links: &Vec<PageLink>) -> usize {
    let fixed = std::mem::size_of::<Entry>() + 3 * std::mem::size_of::<usize>();
    let vectors = links
        .capacity()
        .saturating_mul(std::mem::size_of::<PageLink>());
    links
        .iter()
        .fold(fixed.saturating_add(vectors), |bytes, link| {
            bytes.saturating_add(match &link.target {
                LinkTarget::Url(url) => url.capacity(),
                LinkTarget::Destination(_) => 0,
            })
        })
}

impl Default for LinkCache {
    fn default() -> Self {
        Self {
            entries: HashMap::new(),
            clock: Cell::new(0),
            bytes: 0,
            budget: BUDGET,
            limit: MAX_ENTRIES,
            oversized: None,
        }
    }
}

impl LinkCache {
    pub fn clear(&mut self) {
        self.entries.clear();
        self.bytes = 0;
        self.clock.set(0);
        self.oversized = None;
    }

    pub fn set_page(&mut self, page: usize) {
        if self
            .oversized
            .as_ref()
            .is_some_and(|(held, _)| *held != page)
        {
            self.oversized = None;
        }
    }

    pub fn contains_key(&self, page: &usize) -> bool {
        self.entries.contains_key(page)
            || self
                .oversized
                .as_ref()
                .is_some_and(|(held, _)| held == page)
    }

    pub fn get(&self, page: &usize) -> Option<&Vec<PageLink>> {
        if let Some((held, links)) = &self.oversized
            && held == page
        {
            return Some(links);
        }
        let entry = self.entries.get(page)?;
        self.clock.set(self.clock.get().wrapping_add(1));
        entry.used.set(self.clock.get());
        Some(&entry.links)
    }

    fn remove(&mut self, page: usize) {
        if let Some(entry) = self.entries.remove(&page) {
            self.bytes -= entry.bytes;
        }
    }

    pub fn insert(&mut self, page: usize, links: Vec<PageLink>, current: usize) -> bool {
        self.set_page(current);
        let bytes = memory_bytes(&links);
        if bytes > self.budget || self.limit == 0 {
            if page == current {
                self.remove(page);
                self.oversized = Some((page, links));
                return true;
            }
            return false;
        }
        let pinned = self
            .entries
            .get(&current)
            .filter(|_| page != current)
            .map_or(0, |entry| entry.bytes);
        if bytes > self.budget.saturating_sub(pinned) {
            return false;
        }
        self.remove(page);
        while self.bytes + bytes > self.budget || self.entries.len() >= self.limit {
            let oldest = self
                .entries
                .iter()
                .filter(|(page, _)| **page != current)
                .min_by_key(|(_, entry)| entry.used.get())
                .map(|(&page, _)| page);
            let Some(oldest) = oldest else {
                return false;
            };
            self.remove(oldest);
        }
        if self
            .oversized
            .as_ref()
            .is_some_and(|(held, _)| *held == page)
        {
            self.oversized = None;
        }
        self.clock.set(self.clock.get().wrapping_add(1));
        self.bytes += bytes;
        self.entries.insert(
            page,
            Entry {
                links,
                used: Cell::new(self.clock.get()),
                bytes,
            },
        );
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use hayro::kurbo::Rect;

    fn links(url: &str) -> Vec<PageLink> {
        vec![PageLink {
            rect: Rect::new(0.0, 0.0, 10.0, 10.0),
            target: LinkTarget::Url(url.to_owned()),
        }]
    }

    #[test]
    fn evicts_only_the_oldest_page_and_keeps_recent_targets() {
        let bytes = memory_bytes(&links("https://a"));
        let mut cache = LinkCache {
            budget: bytes * 2,
            ..Default::default()
        };
        assert!(cache.insert(0, links("https://a"), 0));
        assert!(cache.insert(1, links("https://b"), 1));
        assert!(cache.get(&0).is_some());
        assert!(cache.insert(2, links("https://c"), 2));
        assert!(cache.get(&0).is_some() && cache.get(&2).is_some());
        assert!(!cache.contains_key(&1));
        assert_eq!(cache.bytes, bytes * 2);
    }

    #[test]
    fn current_page_is_protected_from_late_offscreen_results() {
        let bytes = memory_bytes(&links("https://a"));
        let mut cache = LinkCache {
            budget: bytes * 2,
            ..Default::default()
        };
        cache.insert(0, links("https://a"), 0);
        cache.insert(1, links("https://b"), 0);
        cache.get(&1);
        assert!(cache.insert(2, links("https://c"), 0));
        assert!(cache.contains_key(&0) && cache.contains_key(&2));
        assert!(!cache.contains_key(&1));
        let mut cache = LinkCache {
            limit: 1,
            ..Default::default()
        };
        cache.insert(0, links("https://a"), 0);
        assert!(!cache.insert(1, links("https://b"), 0));
        assert!(cache.contains_key(&0));
    }

    #[test]
    fn oversized_current_links_stay_usable_without_flushing_cached_pages() {
        let bytes = memory_bytes(&links("https://a"));
        let mut cache = LinkCache {
            budget: bytes * 2,
            ..Default::default()
        };
        cache.insert(0, links("https://a"), 0);
        cache.insert(1, links("https://b"), 1);
        let large = "https://example.com/".repeat(100);
        assert!(cache.insert(2, links(&large), 2));
        assert_eq!(
            cache.get(&2).unwrap()[0].target,
            LinkTarget::Url(large.clone())
        );
        assert!(cache.contains_key(&0) && cache.contains_key(&1));
        assert_eq!(cache.bytes, bytes * 2);
        assert!(!cache.insert(3, links(&large), 2));
        assert!(
            cache.contains_key(&2),
            "late results must not replace the oversized current page"
        );
        cache.set_page(3);
        assert!(!cache.contains_key(&2));
        assert!(!cache.insert(2, links(&large), 3));
        assert!(cache.contains_key(&0) && cache.contains_key(&1));
    }

    #[test]
    fn empty_pages_are_bounded_and_replacing_an_entry_does_not_evict_other_pages() {
        let mut cache = LinkCache {
            limit: 3,
            ..Default::default()
        };
        for page in 0..100 {
            assert!(cache.insert(page, vec![], page));
        }
        assert_eq!(cache.entries.len(), 3);
        assert_eq!(cache.bytes, memory_bytes(&vec![]) * 3);
        assert!(!cache.contains_key(&96) && cache.contains_key(&97));
        assert!(cache.insert(99, links("https://a"), 99));
        assert!(cache.contains_key(&97) && cache.contains_key(&98));
        cache.clear();
        assert_eq!(cache.bytes, 0);
        assert!(!cache.contains_key(&99));
    }

    #[test]
    fn capacity_accounting_includes_unused_vector_and_url_reservations() {
        let budget = memory_bytes(&links("https://a"));
        let mut cache = LinkCache {
            budget,
            ..Default::default()
        };
        cache.insert(0, links("https://a"), 0);
        let mut reserved = Vec::with_capacity(128);
        let mut url = String::with_capacity(4096);
        url.push_str("https://b");
        reserved.push(PageLink {
            rect: Rect::new(0.0, 0.0, 10.0, 10.0),
            target: LinkTarget::Url(url),
        });
        assert!(memory_bytes(&reserved) >= 128 * std::mem::size_of::<PageLink>() + 4096);
        assert!(!cache.insert(1, reserved, 0));
        assert!(cache.contains_key(&0));
        assert_eq!(cache.bytes, budget);
    }
}
