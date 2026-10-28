//! Keep the ordered navigation snapshot in sync with individual bookmark edits.
use std::collections::BTreeSet;

/// `pages` must initially contain the sorted contents of `bookmarks`.
/// Whole-document changes rebuild the snapshot; edits only move the affected tail.
pub fn set(
    bookmarks: &mut BTreeSet<usize>,
    pages: &mut Vec<usize>,
    page: usize,
    present: bool,
) -> bool {
    let changed = if present {
        bookmarks.insert(page)
    } else {
        bookmarks.remove(&page)
    };
    if changed {
        match (present, pages.binary_search(&page)) {
            (true, Err(index)) => pages.insert(index, page),
            (false, Ok(index)) => {
                pages.remove(index);
            }
            _ => debug_assert!(false, "bookmark snapshot was out of sync"),
        }
    }
    changed
}

#[cfg(test)]
mod tests {
    #[test]
    fn mixed_edits_match_an_independent_ordered_set_after_every_operation() {
        use super::*;
        let mut expected = BTreeSet::new();
        let mut bookmarks = BTreeSet::new();
        let mut pages = Vec::new();
        let mut seed = 17_u64;
        for index in 0..6000 {
            seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1);
            let page = match index % 17 {
                0 => 0,
                1 => usize::MAX,
                _ => (seed >> 32) as usize % 1000,
            };
            let present = (seed >> 16) & 1 != 0;
            let changed = if present {
                expected.insert(page)
            } else {
                expected.remove(&page)
            };
            assert_eq!(set(&mut bookmarks, &mut pages, page, present), changed);
            assert_eq!(bookmarks, expected);
            assert!(pages.iter().copied().eq(expected.iter().copied()));
            // Repeating the same action must neither duplicate nor remove a neighbor.
            assert!(!set(&mut bookmarks, &mut pages, page, present));
            assert!(pages.iter().copied().eq(expected.iter().copied()));
        }
    }

    #[test]
    fn deletions_and_reinsertions_preserve_storage_and_handle_empty_snapshots() {
        use super::*;
        let mut pages: Vec<_> = (0..600).collect();
        pages.reserve(1);
        let allocation = pages.as_ptr();
        let capacity = pages.capacity();
        let mut bookmarks = pages.iter().copied().collect();
        for page in (0..600).step_by(2).chain((1..600).rev().step_by(2)) {
            assert!(set(&mut bookmarks, &mut pages, page, false));
            assert_eq!(pages.as_ptr(), allocation);
            assert_eq!(pages.capacity(), capacity);
            assert!(pages.iter().copied().eq(bookmarks.iter().copied()));
        }
        assert!(pages.is_empty());
        assert!(bookmarks.is_empty());
        for page in (0..600).rev() {
            assert!(set(&mut bookmarks, &mut pages, page, true));
            assert_eq!(pages.as_ptr(), allocation);
            assert_eq!(pages.capacity(), capacity);
        }
        assert_eq!(pages, (0..600).collect::<Vec<_>>());
        assert!(set(&mut bookmarks, &mut pages, usize::MAX, true));
        assert_eq!(pages.last(), Some(&usize::MAX));
        assert!(set(&mut bookmarks, &mut pages, usize::MAX, false));
        assert_eq!(pages.last(), Some(&599));
    }
}
