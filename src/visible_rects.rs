//! Reusable screen geometry for visible page regions in an immutable snapshot.
use eframe::egui::{Rect, Vec2};

#[derive(Clone, Copy, PartialEq)]
pub struct PageView {
    pub document: u64,
    pub page: usize,
    pub page_rect: Rect,
    pub clip: Rect,
    pub dimensions: [f32; 2],
    pub rotation: u8,
    pub scale: f32,
}

pub fn screen_rect(
    rect: hayro::kurbo::Rect,
    page: Rect,
    dimensions: [f32; 2],
    rotation: u8,
    scale: f32,
) -> Rect {
    let rect = crate::geometry::rotate_rect(rect, dimensions, rotation);
    let scale = f64::from(scale);
    Rect::from_min_max(
        page.min + Vec2::new((rect.x0 * scale) as f32, (rect.y0 * scale) as f32),
        page.min + Vec2::new((rect.x1 * scale) as f32, (rect.y1 * scale) as f32),
    )
}

impl PageView {
    pub fn screen_rect(self, rect: hayro::kurbo::Rect) -> Rect {
        screen_rect(
            rect,
            self.page_rect,
            self.dimensions,
            self.rotation,
            self.scale,
        )
    }
}

/// Paint paths can retain page-clipped bounds so viewport clipping does not
/// move an icon anchor or add an antialiased edge to a filled highlight.
#[derive(Default)]
pub struct VisibleRects<Key = usize, const CLIP_TO_VIEW: bool = true> {
    view: Option<PageView>,
    pub indices: Vec<Key>,
    pub rects: Vec<Rect>,
}

impl<Key: Copy + PartialEq, const CLIP_TO_VIEW: bool> VisibleRects<Key, CLIP_TO_VIEW> {
    pub fn clear(&mut self) {
        self.invalidate();
        self.indices.clear();
        self.rects.clear();
    }

    /// New metadata can change geometry without changing the view or count.
    pub fn invalidate(&mut self) {
        self.view = None;
    }

    /// Return whether visible identities changed. Moving the same regions keeps their order.
    pub fn update_keyed(
        &mut self,
        view: PageView,
        rectangles: impl Iterator<Item = (Key, hayro::kurbo::Rect)>,
    ) -> bool {
        if self.view == Some(view) {
            return false;
        }
        let mut count = 0;
        let mut changed = false;
        for (index, rect) in rectangles {
            let rect = view.screen_rect(rect);
            if !rect.intersects(view.clip) {
                continue;
            }
            let clipped = rect.intersect(view.page_rect);
            let clipped = if CLIP_TO_VIEW {
                clipped.intersect(view.clip)
            } else {
                clipped
            };
            if let Some(previous) = self.indices.get_mut(count) {
                changed |= *previous != index;
                *previous = index;
                self.rects[count] = clipped;
            } else {
                self.indices.push(index);
                self.rects.push(clipped);
                changed = true;
            }
            count += 1;
        }
        changed |= self.indices.len() != count;
        self.indices.truncate(count);
        self.rects.truncate(count);
        self.view = Some(view);
        changed
    }
}

impl VisibleRects {
    /// Links use their ordinal position as identity for hint numbering.
    pub fn update(
        &mut self,
        view: PageView,
        rectangles: impl Iterator<Item = hayro::kurbo::Rect>,
    ) -> bool {
        self.update_keyed(view, rectangles.enumerate())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use eframe::egui::pos2;
    use hayro::kurbo::Rect as PdfRect;
    use std::cell::Cell;

    fn view() -> PageView {
        PageView {
            document: 1,
            page: 0,
            page_rect: Rect::from_min_size(pos2(20.0, 30.0), Vec2::new(600.0, 800.0)),
            clip: Rect::from_min_max(pos2(40.0, 50.0), pos2(500.0, 700.0)),
            dimensions: [600.0, 800.0],
            rotation: 0,
            scale: 1.0,
        }
    }

    #[test]
    fn unchanged_views_skip_geometry_and_metadata_invalidation_rebuilds_equal_counts() {
        let mut cache = VisibleRects::default();
        let first = PdfRect::new(30.0, 40.0, 100.0, 50.0);
        assert!(cache.update(view(), [first].into_iter()));
        let geometry = cache.rects.clone();
        let reads = Cell::new(0);
        assert!(!cache.update(
            view(),
            [first].into_iter().inspect(|_| reads.set(reads.get() + 1))
        ));
        assert_eq!(reads.get(), 0);
        let changed = PdfRect::new(100.0, 140.0, 170.0, 150.0);
        cache.invalidate();
        assert!(!cache.update(view(), [changed].into_iter()));
        assert_eq!(cache.indices, [0]);
        assert_ne!(cache.rects, geometry);
        assert_eq!(cache.rects[0], view().screen_rect(changed));
        cache.clear();
        assert!(cache.indices.is_empty() && cache.rects.is_empty());
        assert!(cache.update(view(), [first].into_iter()));
    }

    #[test]
    fn every_geometry_and_document_change_rebuilds_and_only_membership_changes_labels() {
        let original = view();
        let changes = [
            PageView {
                document: 2,
                ..original
            },
            PageView {
                page: 1,
                ..original
            },
            PageView {
                page_rect: original.page_rect.translate(Vec2::splat(1.0)),
                ..original
            },
            PageView {
                clip: original.clip.shrink(1.0),
                ..original
            },
            PageView {
                dimensions: [610.0, 810.0],
                ..original
            },
            PageView {
                rotation: 1,
                ..original
            },
            PageView {
                scale: 1.1,
                ..original
            },
        ];
        let rectangles = [
            PdfRect::new(30.0, 40.0, 100.0, 50.0),
            PdfRect::new(550.0, 740.0, 580.0, 750.0),
            PdfRect::new(-10.0, 10.0, 60.0, 60.0),
        ];
        for changed in changes {
            let mut cache = VisibleRects::default();
            cache.update(original, rectangles.into_iter());
            let previous = cache.indices.clone();
            let reads = Cell::new(0);
            let renumbered = cache.update(
                changed,
                rectangles
                    .into_iter()
                    .inspect(|_| reads.set(reads.get() + 1)),
            );
            assert_eq!(reads.get(), rectangles.len());
            let expected: Vec<_> = rectangles
                .into_iter()
                .enumerate()
                .filter_map(|(index, rect)| {
                    let rect = changed.screen_rect(rect);
                    rect.intersects(changed.clip).then_some((
                        index,
                        rect.intersect(changed.page_rect).intersect(changed.clip),
                    ))
                })
                .collect();
            assert_eq!(
                cache
                    .indices
                    .iter()
                    .copied()
                    .zip(cache.rects.iter().copied())
                    .collect::<Vec<_>>(),
                expected
            );
            assert_eq!(renumbered, previous != cache.indices);
        }
    }
}
