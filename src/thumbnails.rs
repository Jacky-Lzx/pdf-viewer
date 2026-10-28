use crate::{cache::TextureCache, document::RenderKey};
use eframe::egui::{self, TextureId};
use std::collections::HashSet;

pub const IMAGE_SIZE: [f32; 2] = [144.0, 192.0];
pub const ROW_HEIGHT: f32 = 224.0;

#[derive(Clone, Copy)]
pub struct PreviewLayout {
    pub image_size: [f32; 2],
    pub row_height: f32,
}

impl PreviewLayout {
    pub fn new(width: f32, height: f32) -> Self {
        let caption = ROW_HEIGHT - IMAGE_SIZE[1];
        let scale = ((width - 8.0).max(1.0) / IMAGE_SIZE[0])
            .min((height - caption).max(1.0) / IMAGE_SIZE[1])
            .min(1.0);
        Self {
            image_size: IMAGE_SIZE.map(|dimension| dimension * scale),
            row_height: IMAGE_SIZE[1] * scale + caption,
        }
    }
}

pub fn key(
    page: usize,
    dimensions: [f32; 2],
    rotation: u8,
    night: bool,
    dpi: f32,
    image_size: [f32; 2],
) -> RenderKey {
    let scale = crate::render_plan::RenderView {
        fit: crate::navigation::Fit::Page,
        viewport: image_size,
        pixels_per_point: dpi,
        rotation,
        night,
        backwards: false,
    }
    .scale(dimensions);
    // Avoid main-view width bucketing: rounding a very narrow preview up to 32
    // pixels would needlessly produce a huge image for tall sheets.
    let width = (dimensions[0] * scale * dpi.clamp(1.0, 2.0))
        .floor()
        .max(1.0) as u32;
    RenderKey {
        page,
        width,
        rotation,
        night,
        tile: None,
    }
}

pub struct Thumbnails {
    cache: TextureCache,
    wanted: Vec<RenderKey>,
    requested: Option<Vec<RenderKey>>,
    failed: HashSet<RenderKey>,
}

impl Default for Thumbnails {
    fn default() -> Self {
        Self {
            cache: TextureCache::exact_with_budget(8 * 1024 * 1024),
            wanted: Vec::new(),
            requested: None,
            failed: HashSet::new(),
        }
    }
}

impl Thumbnails {
    pub fn clear(&mut self) {
        *self = Self::default();
    }

    pub fn begin_frame(&mut self) {
        self.wanted.clear();
    }

    pub fn want(&mut self, key: RenderKey) -> Option<TextureId> {
        self.wanted.push(key);
        self.cache.get(&key)
    }

    pub fn failed(&self, key: RenderKey) -> bool {
        self.failed.contains(&key)
    }

    pub fn invalidate_requests(&mut self) {
        self.requested = None;
    }

    pub fn requests(&mut self) -> Option<Vec<RenderKey>> {
        self.failed.retain(|key| self.wanted.contains(key));
        let missing = self
            .wanted
            .iter()
            .filter(|key| !self.cache.contains(key) && !self.failed.contains(key))
            .copied();
        if self
            .requested
            .as_ref()
            .is_some_and(|requested| requested.iter().copied().eq(missing.clone()))
        {
            return None;
        }
        let missing: Vec<_> = missing.collect();
        self.requested = Some(missing.clone());
        Some(missing)
    }

    pub fn accept(
        &mut self,
        key: RenderKey,
        image: Option<egui::ColorImage>,
        context: &egui::Context,
    ) {
        if !self.wanted.contains(&key) {
            return;
        }
        let Some(image) = image else {
            self.failed.insert(key);
            return;
        };
        if !self.cache.can_insert(key, image.pixels.len() * 4) {
            self.failed.insert(key);
            return;
        }
        self.failed.remove(&key);
        let texture = context.load_texture(
            format!("preview-{}-{}", key.page, key.width),
            image,
            egui::TextureOptions::LINEAR,
        );
        self.cache.insert(key, texture);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn responsive_previews_fit_their_cards_and_request_smaller_rasters() {
        for width in [64.0, 92.0, 228.0, 352.0] {
            for height in [48.0, 80.0, 140.0, 400.0] {
                let layout = PreviewLayout::new(width, height);
                assert!(layout.image_size[0] + 8.0 <= width);
                assert!(layout.row_height <= height);
                assert!(layout.image_size[1] + 32.0 <= layout.row_height);
                assert!(layout.image_size[0] <= IMAGE_SIZE[0]);
                for dimensions in [[600.0, 800.0], [1200.0, 200.0], [2.0, 10000.0]] {
                    for rotation in 0..4 {
                        for dpi in [1.0, 2.0, 4.0] {
                            let compact =
                                key(0, dimensions, rotation, false, dpi, layout.image_size);
                            let normal = key(0, dimensions, rotation, false, dpi, IMAGE_SIZE);
                            let compact =
                                crate::document::RasterSize::new(dimensions, compact.width)
                                    .unwrap();
                            let normal =
                                crate::document::RasterSize::new(dimensions, normal.width).unwrap();
                            assert!(compact.bytes() <= normal.bytes());
                        }
                    }
                }
            }
        }
        let layout = PreviewLayout::new(72.0, 80.0);
        let compact = key(0, [600.0, 800.0], 0, false, 1.0, layout.image_size);
        let compact = crate::document::RasterSize::new([600.0, 800.0], compact.width).unwrap();
        let normal = key(0, [600.0, 800.0], 0, false, 1.0, IMAGE_SIZE);
        let normal = crate::document::RasterSize::new([600.0, 800.0], normal.width).unwrap();
        assert!(compact.bytes() <= normal.bytes() / 4);
        let normal = PreviewLayout::new(228.0, 400.0);
        assert_eq!(normal.image_size, IMAGE_SIZE);
        assert_eq!(normal.row_height, ROW_HEIGHT);
    }

    #[test]
    fn preview_keys_fit_mixed_pages_rotations_and_high_dpi_without_large_allocations() {
        for dimensions in [
            [600.0, 800.0],
            [1200.0, 200.0],
            [2.0, 10000.0],
            [600000.0, 800000.0],
            [f32::MAX, f32::MAX],
        ] {
            for rotation in 0..4 {
                for dpi in [1.0, 2.0, 4.0] {
                    let key = key(0, dimensions, rotation, false, dpi, IMAGE_SIZE);
                    let raster = crate::document::RasterSize::new(dimensions, key.width).unwrap();
                    assert!(raster.bytes() <= 512 * 512 * 4);
                    assert_eq!(key.rotation, rotation);
                }
            }
        }
    }

    #[test]
    fn scrolling_replaces_requests_and_ignores_late_images() {
        let context = egui::Context::default();
        let mut previews = Thumbnails::default();
        let first = key(0, [600.0, 800.0], 0, false, 1.0, IMAGE_SIZE);
        let second = key(1, [600.0, 800.0], 0, false, 1.0, IMAGE_SIZE);
        previews.begin_frame();
        previews.want(first);
        assert_eq!(previews.requests(), Some(vec![first]));
        assert_eq!(previews.requests(), None);
        previews.begin_frame();
        previews.want(second);
        assert_eq!(previews.requests(), Some(vec![second]));
        previews.accept(
            first,
            Some(egui::ColorImage::filled([2, 2], egui::Color32::WHITE)),
            &context,
        );
        assert!(previews.cache.get(&first).is_none());
        previews.accept(second, None, &context);
        assert_eq!(previews.requests(), Some(vec![]));
        assert!(previews.failed(second));
        previews.begin_frame();
        assert_eq!(previews.requests(), None);
        previews.want(second);
        assert_eq!(
            previews.requests(),
            Some(vec![second]),
            "offscreen failures can retry when revisited"
        );
    }

    #[test]
    fn cached_previews_are_reused_and_oversized_results_do_not_evict_them() {
        let context = egui::Context::default();
        let mut previews = Thumbnails {
            cache: TextureCache::with_budget(32),
            ..Default::default()
        };
        let keys: Vec<_> = (0..3)
            .map(|page| key(page, [600.0, 800.0], 0, false, 1.0, IMAGE_SIZE))
            .collect();
        let image = || egui::ColorImage::filled([2, 2], egui::Color32::WHITE);
        for &key in &keys[..2] {
            previews.want(key);
            previews.accept(key, Some(image()), &context);
        }
        assert_eq!(previews.requests(), Some(vec![]));
        assert_eq!(previews.cache.bytes, 32);
        previews.begin_frame();
        assert!(previews.want(keys[0]).is_some());
        previews.want(keys[2]);
        previews.accept(
            keys[2],
            Some(egui::ColorImage::filled([3, 3], egui::Color32::WHITE)),
            &context,
        );
        assert!(previews.failed(keys[2]));
        assert!(previews.cache.contains(&keys[0]));
        assert!(previews.cache.contains(&keys[1]));
        previews.accept(keys[2], Some(image()), &context);
        assert!(!previews.failed(keys[2]));
        assert!(previews.cache.contains(&keys[0]));
        assert!(!previews.cache.contains(&keys[1]));
        assert_eq!(previews.cache.bytes, 32);
        previews.clear();
        assert_eq!(previews.cache.bytes, 0);
        assert!(previews.requested.is_none());
    }
}
