//! Bounded text preparation for virtualized comment cards.
//! Entries belong to one immutable comments/page-label snapshot. The UI clears
//! the cache whenever that snapshot is replaced, before indices can be reused.
use crate::annotations::{Comment, ReviewStatus};
use crate::fifo_cache::FifoCache;

const CARD_LIMIT: usize = 128;
type Key = (usize, bool);

pub fn preview_text(text: &str, limit: usize) -> String {
    let mut chars = text.chars();
    let mut result: String = chars
        .by_ref()
        .take(limit)
        .map(|c| if c.is_whitespace() { ' ' } else { c })
        .collect();
    if chars.next().is_some() {
        result.push('…');
    }
    result
}

pub fn card_lines(comment: &Comment, page_name: String, matched: bool) -> [String; 3] {
    let preview = preview_text(&comment.contents, 160);
    let author = preview_text(&comment.author, 60);
    [
        format!(
            "Page {}{}",
            page_name,
            if author.is_empty() {
                String::new()
            } else {
                format!(" · {author}")
            }
        ),
        if preview.is_empty() {
            "(no comment)".into()
        } else {
            preview
        },
        format!(
            "{}{}{}{}",
            if comment.parent.is_some() {
                "Reply"
            } else {
                &comment.kind
            },
            if comment.review == ReviewStatus::None {
                String::new()
            } else {
                format!(" · {}", comment.review.label())
            },
            if comment.marked { " · ✓ Marked" } else { "" },
            if matched { "" } else { " · Thread context" },
        ),
    ]
}

#[derive(Default)]
pub struct CardTextCache {
    cache: FifoCache<Key, [String; 3], CARD_LIMIT>,
}

impl CardTextCache {
    /// Release strings and index storage when comments or page labels change.
    pub fn clear(&mut self) {
        self.cache.clear();
    }

    /// Cache only recently encountered cards, independent of document length.
    /// A filter's ancestor/context label is part of the key; colors, selection,
    /// width, scale and theme remain live widget/layout inputs.
    pub fn get_or_insert_with(
        &mut self,
        index: usize,
        matched: bool,
        create: impl FnOnce() -> [String; 3],
    ) -> &[String; 3] {
        self.cache.get_or_insert_with((index, matched), create)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn comment() -> Comment {
        Comment {
            page: 3,
            slot: Some(0),
            parent: None,
            review: ReviewStatus::Completed,
            marked: true,
            states: Vec::new(),
            kind: "Highlight".into(),
            contents: "中文\n草稿\t📝".into(),
            author: "Reviewer".into(),
            rects: Vec::new(),
            has_appearance: false,
            truncated: false,
            locked: false,
        }
    }

    #[test]
    fn cards_preserve_unicode_previews_labels_status_and_thread_context() {
        let mut comment = comment();
        let mut cache = CardTextCache::default();
        let lines =
            cache.get_or_insert_with(7, true, || card_lines(&comment, "iv · 4".into(), true));
        assert_eq!(
            lines,
            &[
                "Page iv · 4 · Reviewer",
                "中文 草稿 📝",
                "Highlight · Completed / resolved · ✓ Marked"
            ]
        );
        assert_eq!(
            cache.get_or_insert_with(7, true, || panic!("unchanged cards must reuse their text"))
                [1],
            "中文 草稿 📝"
        );
        let context =
            cache.get_or_insert_with(7, false, || card_lines(&comment, "iv · 4".into(), false));
        assert_eq!(
            context[2],
            "Highlight · Completed / resolved · ✓ Marked · Thread context"
        );
        comment.contents = "📝".repeat(161);
        comment.author = "中".repeat(61);
        let lines = card_lines(&comment, "4".into(), true);
        assert_eq!(lines[1], format!("{}…", "📝".repeat(160)));
        assert_eq!(lines[0], format!("Page 4 · {}…", "中".repeat(60)));
        comment.contents.clear();
        comment.author.clear();
        assert_eq!(
            card_lines(&comment, "4".into(), true)[..2],
            ["Page 4", "(no comment)"]
        );
    }

    #[test]
    fn scrolling_is_bounded_and_cache_hits_do_not_evict_other_visible_cards() {
        let mut cache = CardTextCache::default();
        for index in 0..10_000 {
            cache.get_or_insert_with(index, true, || {
                [index.to_string(), String::new(), String::new()]
            });
        }
        for _ in 0..3 {
            for index in 10_000 - CARD_LIMIT..10_000 {
                assert_eq!(
                    cache.get_or_insert_with(index, true, || panic!("hot card evicted"))[0],
                    index.to_string()
                );
            }
        }
        assert_eq!(
            cache.get_or_insert_with(0, true, || ["rebuilt".into(), String::new(), String::new()])
                [0],
            "rebuilt"
        );
    }

    #[test]
    fn failed_card_preparation_keeps_resident_cards_and_eviction_usable() {
        let mut cache = CardTextCache::default();
        for index in 0..CARD_LIMIT {
            cache.get_or_insert_with(index, true, || {
                [index.to_string(), String::new(), String::new()]
            });
        }
        let failure = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            cache.get_or_insert_with(CARD_LIMIT, true, || panic!("preparation failed"));
        }));
        assert!(failure.is_err());
        for index in 0..CARD_LIMIT {
            assert_eq!(
                cache.get_or_insert_with(index, true, || panic!("resident card evicted"))[0],
                index.to_string()
            );
        }
        for index in CARD_LIMIT..10_000 {
            cache.get_or_insert_with(index, true, || {
                [index.to_string(), String::new(), String::new()]
            });
        }
    }

    #[test]
    fn clearing_rebuilds_text_before_comment_indices_are_reused() {
        let mut cache = CardTextCache::default();
        cache.get_or_insert_with(0, true, || card_lines(&comment(), "iv · 4".into(), true));
        cache.clear();
        let mut replacement = comment();
        replacement.contents = "Replacement snapshot".into();
        assert_eq!(
            cache.get_or_insert_with(0, true, || card_lines(&replacement, "5".into(), true))[1],
            "Replacement snapshot"
        );
    }
}
