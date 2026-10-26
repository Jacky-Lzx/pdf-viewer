use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub enum Fit {
    #[default]
    Page,
    Width,
    Custom(f32),
}

impl Fit {
    pub fn scale(self, page: [f32; 2], viewport: [f32; 2]) -> f32 {
        let width = viewport[0].max(1.0) / page[0].max(1.0);
        let height = viewport[1].max(1.0) / page[1].max(1.0);
        match self {
            Self::Page => width.min(height),
            Self::Width => width,
            Self::Custom(scale) => scale,
        }
        .clamp(0.05, 8.0)
    }
}

#[derive(Default, Debug)]
pub struct Navigation {
    pub page: usize,
    pub count: usize,
    pub prefix: String,
    pub pending_g: bool,
    pub direction: isize,
    back: Vec<usize>,
    forward: Vec<usize>,
}

impl Navigation {
    pub fn new(count: usize, page: usize) -> Self {
        Self {
            count,
            page: page.min(count.saturating_sub(1)),
            ..Self::default()
        }
    }

    pub fn jump(&mut self, page: usize) -> bool {
        let page = page.min(self.count.saturating_sub(1));
        if page == self.page || self.count == 0 {
            return false;
        }
        self.back.push(self.page);
        if self.back.len() > 256 {
            self.back.remove(0);
        }
        self.forward.clear();
        self.direction = if page > self.page { 1 } else { -1 };
        self.page = page;
        true
    }

    pub fn step(&mut self, delta: isize) -> bool {
        self.jump(self.page.saturating_add_signed(delta))
    }

    pub fn history(&mut self, forward: bool) -> bool {
        let (from, to) = if forward {
            (&mut self.forward, &mut self.back)
        } else {
            (&mut self.back, &mut self.forward)
        };
        if let Some(page) = from.pop() {
            to.push(self.page);
            self.direction = if page > self.page { 1 } else { -1 };
            self.page = page;
            true
        } else {
            false
        }
    }

    pub fn digit(&mut self, digit: char) {
        if self.prefix.len() < 7 {
            self.prefix.push(digit);
        }
    }

    pub fn take_count(&mut self) -> usize {
        let count = self.prefix.parse().unwrap_or(1).max(1);
        self.clear_prefix();
        count
    }

    pub fn clear_prefix(&mut self) {
        self.prefix.clear();
        self.pending_g = false;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn navigation_clamps_and_restores_history() {
        let mut nav = Navigation::new(5, 20);
        assert_eq!(nav.page, 4);
        assert!(!nav.step(1));
        assert!(nav.step(-100));
        assert_eq!(nav.direction, -1);
        assert_eq!(nav.page, 0);
        assert!(nav.history(false));
        assert_eq!(nav.direction, 1);
        assert_eq!(nav.page, 4);
        assert!(nav.history(true));
        assert_eq!(nav.direction, -1);
        assert_eq!(nav.page, 0);
        nav.jump(2);
        assert!(!nav.history(true));
    }

    #[test]
    fn empty_documents_and_extreme_scales_are_safe() {
        let mut nav = Navigation::new(0, 0);
        assert!(!nav.step(1));
        assert_eq!(Fit::Page.scale([600.0, 800.0], [300.0, 800.0]), 0.5);
        assert_eq!(Fit::Width.scale([600.0, 800.0], [300.0, 100.0]), 0.5);
        assert!(Fit::Page.scale([0.0, 0.0], [0.0, 0.0]).is_finite());
    }

    #[test]
    fn count_prefix_is_bounded_and_consumed() {
        let mut nav = Navigation::default();
        for _ in 0..100 {
            nav.digit('9');
        }
        assert_eq!(nav.take_count(), 9_999_999);
        assert_eq!(nav.take_count(), 1);
    }
}
