//! Preserve search commands while ordered result batches are still arriving.
use crate::text::{SearchHit, advance_match, initial_match};
use std::collections::VecDeque;

const MAX_RETAINED_JUMPS: usize = 256;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Jump {
    reverse: bool,
    count: usize,
}

impl Jump {
    pub fn new(reverse: bool, count: usize) -> Self {
        Self {
            reverse,
            count: count.max(1),
        }
    }
}

#[derive(Clone, Copy)]
pub struct SearchState<'a> {
    pub hits: &'a [SearchHit],
    pub selected: Option<usize>,
    pub page: usize,
    pub scanned: usize,
    pub complete: bool,
}

impl SearchState<'_> {
    fn resolve(self, jump: Jump) -> Option<usize> {
        let (start, steps) = match self.selected {
            Some(index) => (index, jump.count),
            None => (
                initial_match(
                    self.hits,
                    self.page,
                    jump.reverse,
                    self.scanned,
                    self.complete,
                )?,
                jump.count - 1,
            ),
        };
        advance_match(start, self.hits.len(), jump.reverse, steps, self.complete)
    }
}

#[derive(Default)]
pub struct SearchNavigation {
    pending: VecDeque<Jump>,
}

impl SearchNavigation {
    /// Ready results need no queue allocation. A later command cannot overtake
    /// an unresolved request, even if it could be answered by a partial batch.
    pub fn request(&mut self, jump: Jump, state: SearchState<'_>) -> Option<usize> {
        if self.pending.is_empty() {
            if let Some(index) = state.resolve(jump) {
                return Some(index);
            }
            if state.complete {
                return None;
            }
        }
        self.pending.push_back(jump);
        None
    }

    pub fn next(&mut self, state: SearchState<'_>) -> Option<usize> {
        let jump = *self.pending.front()?;
        if let Some(index) = state.resolve(jump) {
            self.pending.pop_front();
            return Some(index);
        }
        if state.complete {
            self.clear();
        }
        None
    }

    pub fn clear(&mut self) {
        *self = Self::default();
    }

    pub fn recycle(&mut self) {
        if self.pending.is_empty() && self.pending.capacity() > MAX_RETAINED_JUMPS {
            self.clear();
        }
    }

    #[cfg(test)]
    pub fn first(&self) -> Option<(bool, usize)> {
        self.pending.front().map(|jump| (jump.reverse, jump.count))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hits() -> Vec<SearchHit> {
        [0, 0, 2, 3]
            .into_iter()
            .map(|page| SearchHit {
                page,
                rects: Vec::new(),
                snippet: String::new(),
                snippet_match: 0..0,
            })
            .collect()
    }

    fn state(hits: &[SearchHit]) -> SearchState<'_> {
        SearchState {
            hits,
            selected: None,
            page: 1,
            scanned: 0,
            complete: false,
        }
    }

    fn drain(navigation: &mut SearchNavigation, state: &mut SearchState<'_>) -> Vec<usize> {
        let mut selected = Vec::new();
        while let Some(index) = navigation.next(*state) {
            selected.push(index);
            state.selected = Some(index);
            state.page = state.hits[index].page;
        }
        selected
    }

    #[test]
    fn opposite_commands_wait_in_order_across_incomplete_batches() {
        let hits = hits();
        let mut state = state(&hits[..2]);
        let mut navigation = SearchNavigation::default();
        for reverse in [true, false, true] {
            assert_eq!(navigation.request(Jump::new(reverse, 1), state), None);
        }
        state.scanned = 2;
        assert_eq!(drain(&mut navigation, &mut state), [1]);
        assert_eq!(navigation.first(), Some((false, 1)));
        state.hits = &hits;
        state.scanned = 4;
        state.complete = true;
        assert_eq!(drain(&mut navigation, &mut state), [2, 1]);
        assert!(navigation.first().is_none());
    }

    #[test]
    fn completed_searches_handle_zero_and_large_counts_without_allocating_a_queue() {
        let hits = hits();
        let mut state = state(&hits);
        state.complete = true;
        let mut navigation = SearchNavigation::default();
        for (jump, expected) in [
            (Jump::new(false, 0), 2),
            (Jump::new(true, usize::MAX), 3),
            (Jump::new(false, usize::MAX), 2),
        ] {
            assert_eq!(navigation.request(jump, state), Some(expected));
            state.selected = Some(expected);
            state.page = hits[expected].page;
        }
        assert_eq!(navigation.pending.capacity(), 0);
    }

    #[test]
    fn completion_without_matches_and_explicit_cancellation_release_pending_commands() {
        let hits = hits();
        let mut state = state(&[]);
        let mut navigation = SearchNavigation::default();
        navigation.request(Jump::new(false, 8), state);
        navigation.request(Jump::new(true, 2), state);
        state.complete = true;
        assert_eq!(navigation.next(state), None);
        assert!(navigation.pending.is_empty());
        assert_eq!(navigation.pending.capacity(), 0);
        state.complete = false;
        navigation.request(Jump::new(true, 1), state);
        navigation.clear();
        state.hits = &hits;
        state.complete = true;
        assert_eq!(navigation.next(state), None);
        assert_eq!(navigation.pending.capacity(), 0);
    }

    #[test]
    fn exceptional_bursts_execute_every_command_and_release_excess_storage() {
        let hits = hits();
        let mut state = state(&[]);
        state.page = 0;
        let mut navigation = SearchNavigation::default();
        let count = MAX_RETAINED_JUMPS + 37;
        for _ in 0..count {
            navigation.request(Jump::new(false, 1), state);
        }
        assert!(navigation.pending.capacity() > MAX_RETAINED_JUMPS);
        state.hits = &hits;
        state.complete = true;
        assert_eq!(
            drain(&mut navigation, &mut state),
            (0..count)
                .map(|index| index % hits.len())
                .collect::<Vec<_>>()
        );
        navigation.recycle();
        assert_eq!(navigation.pending.capacity(), 0);
    }
}
