//! Recent explicit searches owned by one reader window, never persisted.
use std::collections::VecDeque;

const LIMIT: usize = 16;
const QUERY_BYTES: usize = 4096;

#[derive(Default)]
pub struct QueryHistory {
    entries: VecDeque<String>,
}

impl QueryHistory {
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn queries(&self) -> impl ExactSizeIterator<Item = &str> {
        self.entries.iter().map(String::as_str)
    }

    pub fn remember(&mut self, query: &str) {
        if query.trim().is_empty() || query.len() > QUERY_BYTES {
            return;
        }
        if self.entries.front().is_some_and(|entry| entry == query) {
            return;
        }
        let entry = if let Some(index) = self.entries.iter().position(|entry| entry == query) {
            self.entries.remove(index).unwrap()
        } else {
            if self.entries.len() == LIMIT {
                self.entries.pop_back();
            }
            query.to_owned()
        };
        self.entries.push_front(entry);
    }

    pub fn clear(&mut self) {
        *self = Self::default();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recent_queries_keep_exact_unicode_text_and_promote_duplicates() {
        let mut history = QueryHistory::default();
        assert!(history.is_empty());
        for query in [
            "",
            " \n\t",
            "Straße 📝",
            "STRASSE",
            "two words",
            "Straße 📝",
        ] {
            history.remember(query);
        }
        assert_eq!(
            history.queries().collect::<Vec<_>>(),
            ["Straße 📝", "two words", "STRASSE"]
        );
        history.remember("Straße 📝");
        assert_eq!(history.queries().len(), 3);
        history.clear();
        assert!(history.is_empty());
        history.remember("new document");
        assert_eq!(history.queries().collect::<Vec<_>>(), ["new document"]);
    }

    #[test]
    fn history_bounds_count_and_bytes_without_truncating_a_query() {
        let mut history = QueryHistory::default();
        for index in 0..LIMIT + 5 {
            history.remember(&format!("query {index}"));
        }
        assert_eq!(history.queries().len(), LIMIT);
        assert_eq!(history.queries().last(), Some("query 5"));
        let exact = "😀".repeat(QUERY_BYTES / 4);
        history.remember(&exact);
        assert_eq!(history.queries().next(), Some(exact.as_str()));
        history.remember(&(exact.clone() + "a"));
        assert_eq!(history.queries().next(), Some(exact.as_str()));
        assert_eq!(history.queries().len(), LIMIT);
        assert!(history.queries().all(|query| query.len() <= QUERY_BYTES));
    }
}
