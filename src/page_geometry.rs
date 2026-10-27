use eframe::egui::Rect;
use std::ops::Range;

/// One frame's visible page rectangles, shared by planning and painting.
#[derive(Default)]
pub struct PageGeometry {
    pages: Vec<(usize, Rect)>,
}

impl PageGeometry {
    pub fn update(&mut self, pages: Range<usize>, mut rect: impl FnMut(usize) -> Rect) {
        if self.pages.len() == pages.len() {
            for (slot, page) in self.pages.iter_mut().zip(pages) {
                *slot = (page, rect(page));
            }
        } else {
            // Release the old capacity when the visible count changes.
            self.pages = pages.map(|page| (page, rect(page))).collect();
        }
    }

    pub fn pages(&self) -> &[(usize, Rect)] {
        &self.pages
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use eframe::egui::{Vec2, pos2};

    #[test]
    fn moving_ranges_reuse_equal_lengths_and_replace_all_page_rectangles_once() {
        let mut geometry = PageGeometry::default();
        for pages in [0..1, 5..6, 0..16, 1..17, 15..16, 0..0] {
            let before = (geometry.pages.as_ptr(), geometry.pages.len());
            let mut calls = Vec::new();
            let rect =
                |page: usize| Rect::from_min_size(pos2(page as f32, 20.0), Vec2::splat(50.0));
            geometry.update(pages.clone(), |page| {
                calls.push(page);
                rect(page)
            });
            assert_eq!(calls, pages.clone().collect::<Vec<_>>());
            assert_eq!(
                geometry.pages(),
                pages
                    .clone()
                    .map(|page| (page, rect(page)))
                    .collect::<Vec<_>>()
            );
            assert_eq!(geometry.pages.capacity(), pages.len());
            if before.1 == pages.len() {
                assert_eq!(geometry.pages.as_ptr(), before.0);
            }
        }
    }
}
