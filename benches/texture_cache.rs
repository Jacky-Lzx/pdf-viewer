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
use document::RenderKey;
use eframe::egui::{Color32, ColorImage, Context, ImageData, TextureHandle, TextureOptions};
use std::{hint::black_box, sync::Arc, time::Instant};

fn key(page: usize) -> RenderKey {
    RenderKey {
        page,
        width: 128,
        rotation: 0,
        night: false,
    }
}

fn texture(context: &Context, image: &Arc<ColorImage>) -> TextureHandle {
    context.load_texture(
        "benchmark",
        ImageData::Color(Arc::clone(image)),
        TextureOptions::LINEAR,
    )
}

fn filled(context: &Context, image: &Arc<ColorImage>, budget: usize, pinned: &str) -> TextureCache {
    let mut cache = TextureCache::with_budget(budget);
    for page in 0..512 {
        assert!(cache.insert(key(page), texture(context, image)));
    }
    assert_eq!(cache.bytes, budget);
    assert!(cache.get(&key(0)).is_some());
    match pinned {
        "visible" => cache.protect(key(0)),
        "fallback" => cache.protect(RenderKey {
            width: 256,
            ..key(0)
        }),
        "none" => {}
        _ => unreachable!(),
    }
    cache
}

fn main() {
    let passes: usize = std::env::args()
        .nth(1)
        .and_then(|value| value.parse().ok())
        .unwrap_or(9);
    assert!((1..=1000).contains(&passes));
    println!(
        "{passes} passes; 64 insertions/sample or 32768 hits; real TextureHandles and egui manager release; shared image buffers, cache filling, texture allocation, queued upload draining, survivor checks and final drop excluded; no GPU backend, PDF rendering or UI."
    );
    for (name, resident_size, large_size, budget) in [
        ("page", [384, 128], [4000, 4000], 96 * 1024 * 1024),
        ("thumbnail", [64, 64], [288, 384], 8 * 1024 * 1024),
    ] {
        let resident = Arc::new(ColorImage::filled(resident_size, Color32::WHITE));
        for kind in ["hit", "single", "few", "batch"] {
            let incoming_size = match kind {
                "few" => [resident_size[0], resident_size[1] * 3],
                "batch" => large_size,
                _ => resident_size,
            };
            let incoming = Arc::new(ColorImage::filled(incoming_size, Color32::WHITE));
            let expected_removed =
                (incoming_size[0] * incoming_size[1]).div_ceil(resident_size[0] * resident_size[1]);
            for pinned in ["none", "visible", "fallback"] {
                if pinned != "none" && (kind == "hit" || name == "thumbnail") {
                    continue;
                }
                let mut samples = Vec::with_capacity(passes);
                let mut checksum = 0;
                let mut removed = 0;
                for _ in 0..passes {
                    let context = Context::default();
                    let initial_textures = context.tex_manager().read().num_allocated();
                    if kind == "hit" {
                        let mut cache = filled(&context, &resident, budget, pinned);
                        drop(context.tex_manager().write().take_delta());
                        for index in 0..32768 {
                            black_box(cache.get(black_box(&key(index % 512))).unwrap());
                        }
                        let start = Instant::now();
                        for index in 0..32768 {
                            black_box(cache.get(black_box(&key(index % 512))).unwrap());
                        }
                        samples.push(start.elapsed().as_secs_f64() * 1000.0);
                        checksum = cache.bytes;
                        continue;
                    }
                    let mut caches: Vec<_> = (0..64)
                        .map(|_| filled(&context, &resident, budget, pinned))
                        .collect();
                    let textures: Vec<_> = (0..64).map(|_| texture(&context, &incoming)).collect();
                    let ids: Vec<_> = textures.iter().map(TextureHandle::id).collect();
                    drop(context.tex_manager().write().take_delta());
                    let start = Instant::now();
                    for (cache, texture) in caches.iter_mut().zip(textures) {
                        assert!(black_box(cache).insert(key(512), black_box(texture)));
                    }
                    samples.push(start.elapsed().as_secs_f64() * 1000.0);
                    for (cache, id) in caches.iter_mut().zip(ids) {
                        assert_eq!(cache.get(&key(512)), Some(id));
                        let survivors: Vec<_> = (0..512)
                            .filter(|&page| cache.contains(&key(page)))
                            .collect();
                        removed = 512 - survivors.len();
                        assert_eq!(removed, expected_removed);
                        assert_eq!(survivors[0], 0, "the touched/protected page survives");
                        assert!(survivors[1..].iter().copied().eq(removed + 1..512));
                        assert_eq!(
                            cache.bytes,
                            resident_size[0] * resident_size[1] * 4 * survivors.len()
                                + incoming_size[0] * incoming_size[1] * 4
                        );
                        assert!(cache.bytes <= budget);
                        checksum = survivors.iter().sum::<usize>() + cache.bytes;
                    }
                    let manager = context.tex_manager();
                    let mut manager = manager.write();
                    assert_eq!(manager.take_delta().free.len(), removed * 64);
                    assert_eq!(
                        manager.num_allocated(),
                        initial_textures + (513 - removed) * 64
                    );
                }
                samples.sort_by(f64::total_cmp);
                println!(
                    "{name}/{kind}/{pinned}: {:.4} ms, removed {removed}, checksum {checksum}",
                    samples[passes / 2]
                );
            }
        }
    }
}
