use crate::state::ReadingMark;
use std::collections::VecDeque;

const LIMIT: usize = 256;

#[derive(Default)]
pub struct ViewHistory {
    back: VecDeque<ReadingMark>,
    forward: VecDeque<ReadingMark>,
    previous_jump: Option<ReadingMark>,
}

fn same_view(left: &ReadingMark, right: &ReadingMark) -> bool {
    left.page == right.page
        && left.fit == right.fit
        && left.rotation == right.rotation
        && left.continuous == right.continuous
        && left.facing == right.facing
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
    /// Read-only availability must agree with traversal, including entries
    /// removed by a reload and repeated positions at the current view.
    pub fn can_go(&self, forward: bool, current: &ReadingMark, count: usize) -> bool {
        let stack = if forward { &self.forward } else { &self.back };
        stack
            .iter()
            .rev()
            .any(|position| position.page < count && !same_view(position, current))
    }

    pub fn record(&mut self, position: ReadingMark) {
        self.previous_jump = Some(position.clone());
        push(&mut self.back, position);
        self.forward.clear();
    }

    pub fn previous_jump(&self, count: usize) -> Option<ReadingMark> {
        self.previous_jump
            .as_ref()
            .filter(|position| position.page < count)
            .cloned()
    }

    pub fn go(&mut self, forward: bool, current: ReadingMark, count: usize) -> Option<ReadingMark> {
        let (from, to) = if forward {
            (&mut self.forward, &mut self.back)
        } else {
            (&mut self.back, &mut self.forward)
        };
        while let Some(position) = from.pop_back() {
            if position.page < count && !same_view(&position, &current) {
                self.previous_jump = Some(current.clone());
                push(to, current);
                return Some(position);
            }
        }
        None
    }

    pub fn retain_pages(&mut self, count: usize) {
        self.back.retain(|position| position.page < count);
        self.forward.retain(|position| position.page < count);
        if self
            .previous_jump
            .as_ref()
            .is_some_and(|position| position.page >= count)
        {
            self.previous_jump = None;
        }
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
            continuous: false,
            facing: false,
            offset: [0.25, offset],
        }
    }

    #[test]
    fn history_distinguishes_reading_modes_at_the_same_page_and_offset() {
        let mut history = ViewHistory::default();
        let paged = position(2, 0.5);
        let continuous = ReadingMark {
            continuous: true,
            ..paged.clone()
        };
        history.record(paged);
        let restored = history.go(false, continuous.clone(), 5).unwrap();
        assert!(!restored.continuous);
        assert!(history.go(true, restored, 5).unwrap().continuous);
    }

    #[test]
    fn previous_jump_toggles_complete_views_including_same_page_positions() {
        let mut history = ViewHistory::default();
        assert!(history.previous_jump(10).is_none());
        let mut current = position(3, 0.8);
        current.fit = Fit::Height;
        current.rotation = 3;
        history.record(position(3, 0.2));
        for index in 0..6 {
            let target = history.previous_jump(10).unwrap();
            let expected = if index % 2 == 0 {
                position(3, 0.2)
            } else {
                ReadingMark {
                    fit: Fit::Height,
                    rotation: 3,
                    ..position(3, 0.8)
                }
            };
            assert!(same_view(&target, &expected));
            history.record(current);
            current = target;
        }
        // Toggling is also a regular jump: Back returns to its origin, and
        // a later new jump clears Forward without losing the last position.
        let target = history.go(false, current.clone(), 10).unwrap();
        assert!(same_view(&history.previous_jump(10).unwrap(), &current));
        let current = history.go(true, target, 10).unwrap();
        history.record(current);
        assert!(history.go(true, position(7, 0.0), 10).is_none());
    }

    #[test]
    fn previous_jump_tracks_history_traversal_and_prunes_removed_pages() {
        let mut history = ViewHistory::default();
        history.record(position(2, 0.2));
        let current = history.go(false, position(5, 0.8), 6).unwrap();
        assert_eq!(history.previous_jump(6).unwrap().page, 5);
        assert!(history.previous_jump(3).is_none());
        // A failed history move must not change the last jump.
        assert!(history.go(false, current, 6).is_none());
        assert_eq!(history.previous_jump(6).unwrap().page, 5);
        history.retain_pages(3);
        assert!(history.previous_jump(6).is_none());
        history.record(position(2, 0.2));
        history.retain_pages(3);
        assert_eq!(history.previous_jump(3).unwrap().page, 2);
        history.retain_pages(0);
        assert!(history.previous_jump(6).is_none());
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

    #[test]
    fn availability_agrees_with_traversal_for_repeated_invalid_and_same_page_views() {
        let mut history = ViewHistory::default();
        for page in [0, 2, 5, 2] {
            history.record(position(page, 0.2));
        }
        history.go(false, position(2, 0.8), 6);
        for forward in [false, true] {
            for count in [0, 1, 3, 6] {
                for current in [position(2, 0.2), position(2, 0.8), position(5, 0.2)] {
                    let mut copy = ViewHistory {
                        back: history.back.clone(),
                        forward: history.forward.clone(),
                        previous_jump: history.previous_jump.clone(),
                    };
                    assert_eq!(
                        history.can_go(forward, &current, count),
                        copy.go(forward, current, count).is_some()
                    );
                }
            }
        }
        let mut history = ViewHistory::default();
        history.record(position(0, 0.2));
        assert!(!history.can_go(false, &position(0, 0.200001), 1));
        assert!(history.can_go(false, &position(0, 0.8), 1));
    }
}
