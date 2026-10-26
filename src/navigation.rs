use serde::{Deserialize, Serialize};

pub const MIN_SCALE: f32 = 0.05;
pub const MAX_SCALE: f32 = 8.0;
// Single-precision UI coordinates retain pixel precision up to this extent.
pub const MAX_PAGE_EXTENT: f32 = 16_000_000.0;

#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub enum Fit {
    #[default]
    Page,
    Width,
    Custom(f32),
}

impl Fit {
    pub fn scale(self, page: [f32; 2], viewport: [f32; 2]) -> f32 {
        let page = page.map(|value| f64::from(value.max(1.0)));
        let width = f64::from(viewport[0].max(1.0)) / page[0];
        let height = f64::from(viewport[1].max(1.0)) / page[1];
        let maximum = f64::from(MAX_SCALE).min(f64::from(MAX_PAGE_EXTENT) / page[0].max(page[1]));
        let fit = width.min(height).min(maximum);
        // A large sheet must still fit the viewport below the usual 5% minimum.
        let minimum = f64::from(MIN_SCALE).min(fit);
        let scale = match self {
            Self::Page => fit,
            Self::Width => width,
            Self::Custom(scale) if scale.is_nan() => fit,
            Self::Custom(scale) => f64::from(scale),
        };
        scale.clamp(minimum, maximum) as f32
    }
}

#[derive(Default, Debug)]
pub struct Navigation {
    pub page: usize,
    pub count: usize,
    pub prefix: String,
    pub pending_g: bool,
    pub direction: isize,
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
        self.direction = if page > self.page { 1 } else { -1 };
        self.page = page;
        true
    }

    pub fn resize(&mut self, count: usize) {
        self.count = count;
        self.page = self.page.min(count.saturating_sub(1));
        self.clear_prefix();
    }

    pub fn step(&mut self, delta: isize) -> bool {
        self.jump(self.page.saturating_add_signed(delta))
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
    fn navigation_clamps_and_tracks_reading_direction() {
        let mut nav = Navigation::new(5, 20);
        assert_eq!(nav.page, 4);
        assert!(!nav.step(1));
        assert!(nav.step(-100));
        assert_eq!(nav.direction, -1);
        assert_eq!(nav.page, 0);
        assert!(nav.jump(4));
        assert_eq!(nav.direction, 1);
        assert_eq!(nav.page, 4);
        assert!(nav.jump(0));
        assert_eq!(nav.direction, -1);
        assert_eq!(nav.page, 0);
        nav.jump(2);
        assert!(!nav.jump(2));
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
    fn large_pages_fit_below_five_percent_and_zoom_remains_finite() {
        let viewport = [1000.0, 700.0];
        for page in [
            [600_000.0, 800_000.0],
            [f32::MAX, f32::MAX],
            [1.0, f32::MAX],
        ] {
            let fit = Fit::Page.scale(page, viewport);
            assert!(fit > 0.0 && fit < MIN_SCALE);
            assert!(page[0] * fit <= viewport[0] + 0.001);
            assert!(page[1] * fit <= viewport[1] + 0.001);
            assert_eq!(Fit::Custom(0.0).scale(page, viewport), fit);
            for mode in [
                Fit::Page,
                Fit::Width,
                Fit::Custom(8.0),
                Fit::Custom(f32::MAX),
                Fit::Custom(f32::INFINITY),
                Fit::Custom(f32::NAN),
            ] {
                let scale = mode.scale(page, viewport);
                assert!(scale.is_finite() && scale > 0.0);
                assert!(
                    page.into_iter()
                        .all(|dimension| dimension * scale <= MAX_PAGE_EXTENT + 1.0)
                );
            }
            let zoomed = Fit::Custom(fit * 1.2).scale(page, viewport);
            assert!(
                (zoomed / fit - 1.2).abs() < 0.001,
                "zooming must be gradual below five percent"
            );
        }
        assert_eq!(Fit::Custom(0.01).scale([600.0, 800.0], viewport), MIN_SCALE);
        assert_eq!(
            Fit::Custom(100.0).scale([600.0, 800.0], viewport),
            MAX_SCALE
        );
    }

    #[test]
    fn reload_clamps_changed_page_counts_and_clears_prefixes() {
        let mut nav = Navigation::new(6, 0);
        nav.jump(2);
        nav.jump(5);
        nav.resize(3);
        assert_eq!((nav.page, nav.count), (2, 3));
        nav.digit('5');
        nav.resize(0);
        assert_eq!(nav.page, 0);
        assert!(nav.prefix.is_empty());
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
