use crate::state::ReadingMark;
use std::collections::VecDeque;

const LIMIT: usize = 256;

#[derive(Default)]
pub struct ViewHistory {
    back: VecDeque<ReadingMark>,
    forward: VecDeque<ReadingMark>,
}

fn same_view(left: &ReadingMark, right: &ReadingMark) -> bool {
    left.page == right.page
        && left.fit == right.fit
        && left.rotation == right.rotation
        && left
            .offset
            .iter()
            .zip(right.offset)
            .all(|(left, right)| (left - right).abs() < 0.0001)
}

fn push(stack: &mut VecDeque<ReadingMark>, position: ReadingMark) {
    if stack.back().is_none_or(|last| !same_view(last, &position)) {
        stack.push_back(position);
        if stack.len() > LIMIT {
            stack.pop_front();
        }
    }
}

impl ViewHistory {
    pub fn record(&mut self, position: ReadingMark) {
        push(&mut self.back, position);
        self.forward.clear();
    }

    pub fn go(&mut self, forward: bool, current: ReadingMark, count: usize) -> Option<ReadingMark> {
        let (from, to) = if forward {
            (&mut self.forward, &mut self.back)
        } else {
            (&mut self.back, &mut self.forward)
        };
        while let Some(position) = from.pop_back() {
            if position.page < count && !same_view(&position, &current) {
                push(to, current);
                return Some(position);
            }
        }
        None
    }

    pub fn retain_pages(&mut self, count: usize) {
        self.back.retain(|position| position.page < count);
        self.forward.retain(|position| position.page < count);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::navigation::Fit;

    fn position(page: usize, offset: f32) -> ReadingMark {
        ReadingMark {
            page,
            fit: Fit::Custom(2.0),
            rotation: 1,
            offset: [0.25, offset],
        }
    }

    #[test]
    fn same_page_history_restores_view_and_forward_is_cleared_by_a_new_jump() {
        let mut history = ViewHistory::default();
        history.record(position(3, 0.2));
        let restored = history.go(false, position(3, 0.8), 10).unwrap();
        assert_eq!(restored.page, 3);
        assert_eq!(restored.offset, [0.25, 0.2]);
        assert_eq!(restored.fit, Fit::Custom(2.0));
        assert_eq!(restored.rotation, 1);
        assert_eq!(history.go(true, restored, 10).unwrap().offset, [0.25, 0.8]);
        let current = history.go(false, position(3, 0.8), 10).unwrap();
        history.record(current);
        assert!(history.go(true, position(5, 0.0), 10).is_none());
    }

    #[test]
    fn repeated_views_are_skipped_and_oldest_entries_are_bounded() {
        let mut history = ViewHistory::default();
        history.record(position(0, 0.2));
        history.record(position(0, 0.200001));
        assert_eq!(history.back.len(), 1);
        assert!(history.go(false, position(0, 0.2), 1).is_none());
        for page in 0..1000 {
            history.record(position(page, 0.0));
        }
        assert_eq!(history.back.len(), LIMIT);
        assert_eq!(history.back.front().unwrap().page, 1000 - LIMIT);
        assert_eq!(
            history.go(false, position(999, 0.0), 1000).unwrap().page,
            998
        );
    }

    #[test]
    fn reload_prunes_missing_pages_on_both_sides_of_history() {
        let mut history = ViewHistory::default();
        history.record(position(0, 0.2));
        history.record(position(2, 0.4));
        let current = history.go(false, position(5, 0.8), 6).unwrap();
        history.retain_pages(3);
        assert!(history.go(true, current.clone(), 3).is_none());
        assert_eq!(history.go(false, current, 3).unwrap().page, 0);
        history.retain_pages(0);
        assert!(history.back.is_empty() && history.forward.is_empty());
    }
}
