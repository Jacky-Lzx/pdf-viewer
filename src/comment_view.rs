//! Cached comment navigation, independent of widget layout and PDF writing.
use crate::annotations::{Comments, ReviewStatus};
use caseless::Caseless;
use std::{
    collections::HashMap,
    time::{Duration, Instant},
};

const SEARCH_BYTES: usize = 8 * 1024 * 1024;

struct Filtering {
    filter: CommentFilter,
    page: Option<usize>,
    words: Vec<memchr::memmem::Finder<'static>>,
    next: usize,
    matches: usize,
}

impl Filtering {
    fn matches(&self, filter: &CommentFilter, page: Option<usize>) -> bool {
        self.filter == *filter && self.page == page
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct CommentFilter {
    pub query: String,
    pub current_page: bool,
    pub review: Option<ReviewStatus>,
    pub marked: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CommentRow {
    pub index: usize,
    pub depth: usize,
    /// Ancestors retained for context need not match the filters themselves.
    pub matched: bool,
}

#[derive(Default)]
pub struct CommentView {
    pub filter: CommentFilter,
    threads: Vec<(usize, usize)>,
    parents: Vec<Option<usize>>,
    // Scratch tables are sized to the snapshot and reused across filters.
    matched: Vec<bool>,
    retained: Vec<bool>,
    visible: Vec<CommentRow>,
    overlays: HashMap<usize, Vec<usize>>,
    // Fields are prepared separately so an author-only match skips the body.
    folded: Vec<[Option<String>; 2]>,
    folded_bytes: usize,
    // Reuse temporary storage for fields that exceed the search cache budget.
    scratch: [String; 2],
    applied: Option<(CommentFilter, Option<usize>)>,
    filtering: Option<Filtering>,
    matches: usize,
    selected: Option<usize>,
    pub reveal: bool,
}

impl CommentView {
    pub fn set_comments(&mut self, comments: &Comments) {
        self.threads = comments.thread_rows();
        self.parents.clear();
        let mut ancestors = Vec::new();
        for &(index, depth) in &self.threads {
            ancestors.truncate(depth);
            self.parents.push(ancestors.last().copied());
            ancestors.push(index);
        }
        self.overlays.clear();
        for (index, comment) in comments.items.iter().enumerate() {
            if comment.parent.is_none() && !comment.rects.is_empty() {
                self.overlays.entry(comment.page).or_default().push(index);
            }
        }
        self.folded = (0..comments.items.len()).map(|_| [None, None]).collect();
        self.folded_bytes = 0;
        self.scratch = Default::default();
        self.matched = vec![false; comments.items.len()];
        self.retained = vec![false; comments.items.len()];
        self.applied = None;
        self.filtering = None;
        self.visible.clear();
        self.matches = 0;
        self.selected = None;
        self.reveal = false;
    }

    /// Refilter only when its inputs change, selecting the first direct match.
    pub fn update(&mut self, comments: &Comments, page: usize) -> bool {
        self.advance(comments, page, None)
    }

    /// Yield between comments after approximately `budget` of filtering work.
    /// A field fold and the final tree pass are indivisible, so this is a soft
    /// budget rather than a maximum frame time. Empty queries stay immediate.
    pub fn update_for(&mut self, comments: &Comments, page: usize, budget: Duration) -> bool {
        if self.filter.query.trim().is_empty() {
            self.update(comments, page)
        } else {
            self.advance(comments, page, Instant::now().checked_add(budget))
        }
    }

    pub fn progress(&self) -> Option<(usize, usize)> {
        self.filtering
            .as_ref()
            .map(|work| (work.next, self.matched.len()))
    }

    fn advance(&mut self, comments: &Comments, page: usize, deadline: Option<Instant>) -> bool {
        let page = self.filter.current_page.then_some(page);
        if self
            .applied
            .as_ref()
            .is_some_and(|(filter, previous)| filter == &self.filter && *previous == page)
        {
            return false;
        }
        if !self
            .filtering
            .as_ref()
            .is_some_and(|work| work.matches(&self.filter, page))
        {
            let query = caseless::default_case_fold_str(&self.filter.query);
            // Own the prepared keywords so later UI frames reuse them.
            let words = query
                .split_whitespace()
                .map(|word| memchr::memmem::Finder::new(word).into_owned())
                .collect();
            self.filtering = Some(Filtering {
                filter: self.filter.clone(),
                page,
                words,
                next: 0,
                matches: 0,
            });
            self.matched.fill(false);
            // Never expose rows or selection from a different query while a
            // replacement is incomplete. The editor is independent of rows.
            self.visible.clear();
            self.selected = None;
            self.matches = 0;
            self.applied = None;
            self.reveal = false;
        }
        let mut work = self.filtering.take().unwrap();
        let words = &work.words;
        let matched = &mut self.matched;
        let mut checked = 0;
        let mut bytes = 0;
        if words.is_empty() {
            // Status/page-only filters do not need resumable text scanning.
            // Keep their small loop independent of cursor and clock bookkeeping.
            for (matched, comment) in matched.iter_mut().zip(&comments.items) {
                *matched = page.is_none_or(|p| p == comment.page)
                    && self.filter.review.is_none_or(|r| r == comment.review)
                    && (!self.filter.marked || comment.marked);
            }
            work.next = comments.items.len();
            work.matches = matched.iter().filter(|&&m| m).count();
        }
        while work.next < comments.items.len() {
            let index = work.next;
            let comment = &comments.items[index];
            matched[index] = if page.is_some_and(|p| p != comment.page)
                || self.filter.review.is_some_and(|r| r != comment.review)
                || (self.filter.marked && !comment.marked)
            {
                false
            } else if let [Some(author), Some(contents)] = &self.folded[index] {
                words.iter().all(|word| {
                    word.find(author.as_bytes()).is_some()
                        || word.find(contents.as_bytes()).is_some()
                })
            } else {
                let [author_cache, contents_cache] = &mut self.folded[index];
                let [author_scratch, contents_scratch] = &mut self.scratch;
                let author = folded_field(
                    &comment.author,
                    author_cache,
                    author_scratch,
                    &mut self.folded_bytes,
                );
                if words
                    .iter()
                    .all(|word| word.find(author.as_bytes()).is_some())
                {
                    true
                } else {
                    let contents = folded_field(
                        &comment.contents,
                        contents_cache,
                        contents_scratch,
                        &mut self.folded_bytes,
                    );
                    words.iter().all(|word| {
                        word.find(author.as_bytes()).is_some()
                            || word.find(contents.as_bytes()).is_some()
                    })
                }
            };
            work.matches += usize::from(matched[index]);
            work.next += 1;
            if let Some(deadline) = deadline {
                checked += 1;
                bytes += comment.author.len() + comment.contents.len();
                // Avoid a clock call per short, cached comment while checking
                // after each large field, including cache misses.
                if checked >= 64 || bytes >= 32 * 1024 {
                    checked = 0;
                    bytes = 0;
                    if work.next < comments.items.len() && Instant::now() >= deadline {
                        self.filtering = Some(work);
                        return false;
                    }
                }
            }
        }
        self.matches = work.matches;
        self.retained.copy_from_slice(matched);
        let keep = &mut self.retained;
        // The preorder is already cycle-safe; one reverse pass retains every
        // ancestor even for very deep threads, without walking each chain.
        for (row, &(index, _)) in self.threads.iter().enumerate().rev() {
            if keep[index]
                && let Some(parent) = self.parents[row]
            {
                keep[parent] = true;
            }
        }
        self.visible.clear();
        self.visible
            .extend(self.threads.iter().filter_map(|&(index, depth)| {
                keep[index].then_some(CommentRow {
                    index,
                    depth,
                    matched: matched[index],
                })
            }));
        self.applied = Some((work.filter, page));
        self.selected = self.visible.iter().position(|row| row.matched);
        self.reveal = true;
        true
    }

    pub fn rows(&self) -> &[CommentRow] {
        &self.visible
    }

    pub fn matches(&self) -> usize {
        self.matches
    }

    pub fn selected(&self) -> Option<usize> {
        self.visible.get(self.selected?).map(|row| row.index)
    }

    pub fn selected_row(&self) -> Option<usize> {
        self.selected
    }

    pub fn select(&mut self, index: usize) {
        if let Some(row) = self.visible.iter().position(|row| row.index == index) {
            self.selected = Some(row);
            self.reveal = true;
        }
    }

    pub fn step(&mut self, delta: isize) {
        if self.visible.is_empty() {
            return;
        }
        self.selected = Some(
            self.selected
                .unwrap_or(0)
                .saturating_add_signed(delta)
                .min(self.visible.len() - 1),
        );
        self.reveal = true;
    }

    /// Page painting is independent of the sidebar's filters.
    pub fn overlays(&self, page: usize) -> &[usize] {
        self.overlays.get(&page).map_or(&[], Vec::as_slice)
    }
}

fn folded_field<'a>(
    text: &str,
    cached: &'a mut Option<String>,
    scratch: &'a mut String,
    bytes: &mut usize,
) -> &'a str {
    if cached.is_none() {
        scratch.clear();
        scratch.reserve(text.len());
        if text.is_ascii() {
            scratch.push_str(text);
            scratch.make_ascii_lowercase();
        } else {
            for ch in text.chars() {
                if ch.is_ascii() {
                    scratch.push(ch.to_ascii_lowercase());
                } else {
                    scratch.extend(std::iter::once(ch).default_case_fold());
                }
            }
        }
        let remaining = SEARCH_BYTES - *bytes;
        if scratch.capacity() <= remaining {
            *bytes += scratch.capacity();
            *cached = Some(std::mem::take(scratch));
        } else if scratch.len() <= remaining {
            // A previous oversized field may have left a large scratch buffer.
            // Keep that buffer for reuse without denying a smaller field cache.
            let field = scratch.clone();
            if field.capacity() <= remaining {
                *bytes += field.capacity();
                *cached = Some(field);
            }
        }
    }
    cached.as_deref().unwrap_or(scratch)
}

#[cfg(test)]
#[path = "comment_view/tests.rs"]
mod tests;
