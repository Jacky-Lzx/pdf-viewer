use serde::{Deserialize, Serialize};

pub const MIN_SCALE: f32 = 0.05;
pub const MAX_SCALE: f32 = 8.0;
// Single-precision UI coordinates retain pixel precision up to this extent.
pub const MAX_PAGE_EXTENT: f32 = 16_000_000.0;

/// Zero-based page at a percentage of the physical page count, rounded up.
pub fn percentage_page(pages: usize, percentage: usize) -> usize {
    let percentage = percentage.min(100);
    // Split the multiplication so even usize::MAX pages cannot overflow.
    (pages / 100 * percentage + (pages % 100 * percentage).div_ceil(100)).saturating_sub(1)
}

/// Resolve an explicitly signed physical page offset, clamped to the document.
/// Other inputs remain available for absolute page numbers and PDF labels.
pub fn relative_page(input: &str, current: usize, pages: usize) -> Option<usize> {
    let input = input.trim();
    let reverse = match input.as_bytes().first()? {
        b'+' => false,
        b'-' => true,
        _ => return None,
    };
    let amount = &input[1..];
    if pages == 0 || amount.is_empty() || !amount.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    let amount = amount.parse::<usize>().ok()?;
    let last = pages - 1;
    let current = current.min(last);
    Some(if reverse {
        current.saturating_sub(amount)
    } else {
        current.saturating_add(amount).min(last)
    })
}

/// Find a counted next/previous bookmark in a sorted, unique page list.
/// Skip the current page, wrap at either end, and avoid intermediate jumps.
pub fn bookmark_target(
    pages: &[usize],
    current: usize,
    reverse: bool,
    count: usize,
) -> Option<usize> {
    let total = pages.len();
    if total == 0 {
        return None;
    }
    let delta = count.saturating_sub(1) % total;
    let row = if reverse {
        let first = pages
            .partition_point(|&page| page < current)
            .checked_sub(1)
            .unwrap_or(total - 1);
        if delta <= first {
            first - delta
        } else {
            total - (delta - first)
        }
    } else {
        let first = pages.partition_point(|&page| page <= current) % total;
        if delta < total - first {
            first + delta
        } else {
            delta - (total - first)
        }
    };
    Some(pages[row])
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub enum Fit {
    #[default]
    Page,
    Width,
    Height,
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
            Self::Height => height,
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
    pub pending_z: bool,
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
        self.pending_z = false;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn relative_jumps_use_physical_offsets_preserve_zero_and_clamp_extreme_counts() {
        for (input, expected) in [
            ("+1", 3),
            ("-1", 1),
            ("+3", 5),
            ("-3", 0),
            ("+0", 2),
            ("-0", 2),
            (" +02 ", 4),
            ("-9999999", 0),
            ("+9999999", 5),
        ] {
            assert_eq!(relative_page(input, 2, 6), Some(expected), "{input}");
        }
        assert_eq!(relative_page("+1", 100, 6), Some(5));
        assert_eq!(relative_page("-1", 100, 6), Some(4));
        let maximum = usize::MAX.to_string();
        assert_eq!(
            relative_page(&format!("+{maximum}"), usize::MAX - 2, usize::MAX),
            Some(usize::MAX - 1)
        );
        assert_eq!(
            relative_page(&format!("-{maximum}"), usize::MAX - 2, usize::MAX),
            Some(0)
        );
        for input in [
            "",
            "1",
            "=+2",
            "=-2",
            "A-2",
            "+",
            "-",
            "++2",
            "--2",
            "+-2",
            "-+2",
            "+ 2",
            "-2.0",
            "+２",
            "+١",
            "+9999999999999999999999999999999999999999",
        ] {
            assert_eq!(relative_page(input, 2, 6), None, "{input}");
        }
        assert_eq!(relative_page("+1", 0, 0), None);
        assert_eq!(relative_page("-1", 0, 0), None);
    }

    #[test]
    fn percentage_navigation_rounds_up_clamps_and_handles_extreme_page_counts() {
        for pages in [0, 1, 2, 3, 6, 99, 100, 101, usize::MAX] {
            for percentage in [0, 1, 10, 25, 50, 75, 99, 100, 101, usize::MAX] {
                let expected = ((pages as u128 * percentage.min(100) as u128).div_ceil(100)
                    as usize)
                    .saturating_sub(1);
                assert_eq!(percentage_page(pages, percentage), expected);
            }
        }
        assert_eq!(percentage_page(6, 50), 2);
        assert_eq!(percentage_page(6, 51), 3);
        assert_eq!(percentage_page(1000, 0), 0);
        assert_eq!(percentage_page(1000, 1), 9);
        assert_eq!(percentage_page(1000, 100), 999);
    }

    #[test]
    fn bookmark_jumps_skip_current_wrap_and_handle_large_counts() {
        let pages = [2, 5, 10];
        for (current, next, previous) in [
            (0, 2, 10),
            (2, 5, 10),
            (4, 5, 2),
            (5, 10, 2),
            (10, 2, 5),
            (20, 2, 10),
        ] {
            assert_eq!(bookmark_target(&pages, current, false, 1), Some(next));
            assert_eq!(bookmark_target(&pages, current, true, 1), Some(previous));
        }
        for reverse in [false, true] {
            for current in 0..=12 {
                for count in [1, 2, 3, 4, 7, 100] {
                    let mut expected = current;
                    for _ in 0..count {
                        expected = if reverse {
                            pages
                                .iter()
                                .copied()
                                .rev()
                                .find(|&page| page < expected)
                                .unwrap_or(10)
                        } else {
                            pages
                                .iter()
                                .copied()
                                .find(|&page| page > expected)
                                .unwrap_or(2)
                        };
                    }
                    assert_eq!(
                        bookmark_target(&pages, current, reverse, count),
                        Some(expected)
                    );
                }
                let equivalent = (usize::MAX - 1) % pages.len() + 1;
                assert_eq!(
                    bookmark_target(&pages, current, reverse, usize::MAX),
                    bookmark_target(&pages, current, reverse, equivalent)
                );
            }
            assert_eq!(bookmark_target(&[], 0, reverse, 1), None);
            assert_eq!(bookmark_target(&[5], 5, reverse, usize::MAX), Some(5));
            assert_eq!(bookmark_target(&[5], 0, reverse, 0), Some(5));
        }
    }

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
        assert_eq!(Fit::Height.scale([1200.0, 400.0], [600.0, 300.0]), 0.75);
        assert_eq!(Fit::Height.scale([1200.0, 400.0], [600.0, 200.0]), 0.5);
        assert_eq!(Fit::Height.scale([400.0, 1200.0], [600.0, 300.0]), 0.25);
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
                Fit::Height,
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
