#[allow(dead_code, unused_imports)]
#[path = "../src/bitmap.rs"]
mod bitmap;
#[allow(dead_code, unused_imports)]
#[path = "../src/cache.rs"]
mod cache;
#[allow(dead_code, unused_imports)]
#[path = "../src/document.rs"]
mod document;
#[allow(dead_code, unused_imports)]
#[path = "../src/document_bytes.rs"]
mod document_bytes;

use cache::TextureCache;
use document::{RenderKey, Tile};
use eframe::egui::{Color32, ColorImage, Context, ImageData, TextureOptions};
use std::{hint::black_box, sync::Arc, time::Instant};

fn key(page: usize) -> RenderKey {
    RenderKey {
        page,
        width: 128,
        rotation: 0,
        night: false,
        tile: None,
    }
}

fn measure(passes: usize, iterations: usize, mut run: impl FnMut()) -> f64 {
    let mut times: Vec<_> = (0..passes)
        .map(|_| {
            let start = Instant::now();
            for _ in 0..iterations {
                run();
            }
            start.elapsed().as_secs_f64() * 1e6 / iterations as f64
        })
        .collect();
    times.sort_by(f64::total_cmp);
    times[passes / 2]
}

fn main() {
    let passes: usize = std::env::args()
        .nth(1)
        .and_then(|arg| arg.parse().ok())
        .unwrap_or(9);
    assert!((1..=1000).contains(&passes));
    println!(
        "{passes} passes, median µs/operation; 1000 admission checks or 10000 protection updates/sample; 512 real TextureHandles with shared 4×4 pixels. Filling, allocation, rasterization, UI and GPU excluded."
    );
    for (name, count, tiles) in [
        ("single page", 1, false),
        ("16 zoom fallbacks", 16, false),
        ("64 zoom fallbacks", 64, false),
        ("16 detail keys", 16, true),
        ("64 detail keys", 64, true),
        ("256 detail keys", 256, true),
        ("512 detail keys", 512, true),
    ] {
        let context = Context::default();
        let image = Arc::new(ColorImage::filled([4, 4], Color32::WHITE));
        let mut cache = TextureCache::with_budget(512 * 64);
        let targets: Vec<_> = (0..count)
            .map(|index| {
                if tiles && index != 0 {
                    RenderKey {
                        tile: Some(Tile {
                            scale_bits: 2.0_f32.to_bits(),
                            x: (index % 32) as u32 * 16,
                            y: (index / 32) as u32 * 16,
                            size: [16, 16],
                        }),
                        ..key(0)
                    }
                } else {
                    RenderKey {
                        width: 256,
                        ..key(index)
                    }
                }
            })
            .collect();
        let resident: Vec<_> = if tiles {
            std::iter::once(key(0))
                .chain(targets.iter().copied().skip(1).take((count - 1) / 2))
                .chain((count..).map(key))
                .take(512)
                .collect()
        } else {
            (0..512).map(key).collect()
        };
        for &key in &resident {
            let texture = context.load_texture(
                "benchmark",
                ImageData::Color(Arc::clone(&image)),
                TextureOptions::LINEAR,
            );
            assert!(cache.insert(key, texture));
        }
        cache.protect_many(&targets);
        let incoming = *targets.last().unwrap();
        assert!(!cache.contains(&incoming));
        assert!(cache.can_insert(incoming, 64));
        assert!(!cache.can_insert(key(1000), cache.budget()));
        let admission = measure(passes, 1000, || {
            black_box(black_box(&cache).can_insert(black_box(incoming), black_box(64)));
        });
        let protect = measure(passes, 10000, || {
            black_box(&mut cache).protect_many(black_box(&targets));
        });
        let texture = context.load_texture(
            "incoming",
            ImageData::Color(Arc::clone(&image)),
            TextureOptions::LINEAR,
        );
        assert!(cache.insert(incoming, texture));
        assert!(cache.contains(&incoming));
        for &key in resident.iter().filter(|key| targets.contains(key)) {
            assert!(cache.contains(&key), "visible resident {key:?}");
        }
        for &target in targets.iter().filter(|key| key.tile.is_none()) {
            assert!(cache.contains(&target) || cache.fallback(target).is_some());
        }
        assert!(cache.bytes <= cache.budget());
        println!("{name}: admission {admission:.4}, protect {protect:.4}");
    }
}
