struct Entry {
    keys: String,
    action: String,
    folded: String,
}

/// Searchable shortcut rows prepared when the help catalog changes.
#[derive(Default)]
pub struct HelpCatalog {
    entries: Vec<Entry>,
    visible: Vec<usize>,
}

impl HelpCatalog {
    pub fn replace(&mut self, rows: impl Iterator<Item = (String, String)>) {
        self.entries = rows
            .map(|(keys, action)| {
                let mut folded = String::with_capacity(keys.len() + 1 + action.len());
                folded.push_str(&keys);
                folded.push(' ');
                folded.push_str(&action);
                folded.make_ascii_lowercase();
                Entry {
                    keys,
                    action,
                    folded,
                }
            })
            .collect();
        self.visible.clear();
    }

    pub fn total(&self) -> usize {
        self.entries.len()
    }

    pub fn filter(&mut self, query: &str) {
        let query = query.to_ascii_lowercase();
        let words: Vec<_> = query.split_whitespace().collect();
        self.visible.clear();
        self.visible.extend(
            self.entries
                .iter()
                .enumerate()
                .filter_map(|(index, entry)| {
                    words
                        .iter()
                        .all(|word| entry.folded.contains(word))
                        .then_some(index)
                }),
        );
    }

    pub fn len(&self) -> usize {
        self.visible.len()
    }

    pub fn is_empty(&self) -> bool {
        self.visible.is_empty()
    }

    pub fn rows(&self) -> impl ExactSizeIterator<Item = (&str, &str)> + '_ {
        self.visible.iter().map(|&index| {
            let entry = &self.entries[index];
            (entry.keys.as_str(), entry.action.as_str())
        })
    }
}
