use crate::{
    cache::TextureCache,
    document::{Bitmap, RenderKey},
};
use eframe::egui::{self, TextureHandle};
use std::collections::HashSet;

pub const IMAGE_SIZE: [f32; 2] = [144.0, 192.0];
pub const ROW_HEIGHT: f32 = 224.0;

pub fn key(page: usize, dimensions: [f32; 2], rotation: u8, night: bool, dpi: f32) -> RenderKey {
    let scale = crate::render_plan::RenderView {
        fit: crate::navigation::Fit::Page,
        viewport: IMAGE_SIZE,
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
    }
}

pub struct Thumbnails {
    cache: TextureCache,
    wanted: Vec<RenderKey>,
    requested: Option<Vec<RenderKey>>,
    failed: HashSet<RenderKey>,
    pub reveal: Option<usize>,
}

impl Default for Thumbnails {
    fn default() -> Self {
        Self {
            cache: TextureCache::with_budget(8 * 1024 * 1024),
            wanted: Vec::new(),
            requested: None,
            failed: HashSet::new(),
            reveal: None,
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

    pub fn want(&mut self, key: RenderKey) -> Option<TextureHandle> {
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
        let missing: Vec<_> = self
            .wanted
            .iter()
            .filter(|key| !self.cache.contains(key) && !self.failed.contains(key))
            .copied()
            .collect();
        if self.requested.as_ref() == Some(&missing) {
            return None;
        }
        self.requested = Some(missing.clone());
        Some(missing)
    }

    pub fn accept(&mut self, key: RenderKey, bitmap: Option<Bitmap>, context: &egui::Context) {
        if !self.wanted.contains(&key) {
            return;
        }
        let Some(bitmap) = bitmap else {
            self.failed.insert(key);
            return;
        };
        if !self.cache.can_insert(key, bitmap.rgba.len()) {
            self.failed.insert(key);
            return;
        }
        self.failed.remove(&key);
        let image = egui::ColorImage::from_rgba_premultiplied(bitmap.size, &bitmap.rgba);
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
                    let key = key(0, dimensions, rotation, false, dpi);
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
        let first = key(0, [600.0, 800.0], 0, false, 1.0);
        let second = key(1, [600.0, 800.0], 0, false, 1.0);
        previews.begin_frame();
        previews.want(first);
        assert_eq!(previews.requests(), Some(vec![first]));
        assert_eq!(previews.requests(), None);
        previews.begin_frame();
        previews.want(second);
        assert_eq!(previews.requests(), Some(vec![second]));
        previews.accept(
            first,
            Some(Bitmap {
                size: [2, 2],
                rgba: vec![255; 16],
            }),
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
            .map(|page| key(page, [600.0, 800.0], 0, false, 1.0))
            .collect();
        let bitmap = || Bitmap {
            size: [2, 2],
            rgba: vec![255; 16],
        };
        for &key in &keys[..2] {
            previews.want(key);
            previews.accept(key, Some(bitmap()), &context);
        }
        assert_eq!(previews.requests(), Some(vec![]));
        assert_eq!(previews.cache.bytes, 32);
        previews.begin_frame();
        assert!(previews.want(keys[0]).is_some());
        previews.want(keys[2]);
        previews.accept(
            keys[2],
            Some(Bitmap {
                size: [3, 3],
                rgba: vec![255; 36],
            }),
            &context,
        );
        assert!(previews.failed(keys[2]));
        assert!(previews.cache.contains(&keys[0]));
        assert!(previews.cache.contains(&keys[1]));
        previews.accept(keys[2], Some(bitmap()), &context);
        assert!(!previews.failed(keys[2]));
        assert!(previews.cache.contains(&keys[0]));
        assert!(!previews.cache.contains(&keys[1]));
        assert_eq!(previews.cache.bytes, 32);
        previews.clear();
        assert_eq!(previews.cache.bytes, 0);
        assert!(previews.requested.is_none());
    }
}
