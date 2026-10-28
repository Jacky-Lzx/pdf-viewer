#[path = "../src/bitmap.rs"]
mod bitmap;
#[allow(dead_code)]
#[path = "../src/document.rs"]
mod document;
#[allow(dead_code, unused_imports)]
#[path = "../src/document_bytes.rs"]
mod document_bytes;
#[allow(dead_code)]
#[path = "../src/page_colors.rs"]
mod page_colors;
#[path = "support/texture_pixels_previous.rs"]
mod previous;
#[path = "../src/texture_pixels.rs"]
mod texture_pixels;

use eframe::egui::ColorImage;
use hayro::{RenderCache, hayro_syntax::Pdf};
use std::{hint::black_box, time::Instant};

fn main() {
    let passes = std::env::args()
        .nth(1)
        .and_then(|value| value.parse().ok())
        .unwrap_or(9);
    assert!((1..=1000).contains(&passes));
    let pdf = Pdf::new(include_bytes!("../assets/pagewing-guide.pdf").to_vec()).unwrap();
    let caches = [RenderCache::new(), RenderCache::new()];
    let render = |current: usize, width, turns, night| -> ColorImage {
        if current == 0 {
            previous::prepare(
                document::rasterize_untransformed(&pdf, &caches[0], 0, width).unwrap(),
                turns,
                night,
            )
        } else {
            texture_pixels::prepare_with_palette(
                document::rasterize_pixels(&pdf, &caches[1], 0, width).unwrap(),
                turns,
                page_colors::PageColors::default().palette(night),
            )
        }
    };
    println!(
        "Alternating bitmap/final-texture raster targets, {passes} passes each. Real PDF first page, warmed separate render caches; medians include rasterization, target allocation and pixel transforms; exclude PDF parsing, final image release, upload and UI drawing."
    );
    for width in [1200, 3200] {
        for night in [false, true] {
            for turns in 0..4 {
                assert!(render(0, width, turns, night) == render(1, width, turns, night));
                let mut times = [Vec::with_capacity(passes), Vec::with_capacity(passes)];
                for pass in 0..passes {
                    for order in 0..2 {
                        let current = (pass + order) % 2;
                        let start = Instant::now();
                        let image = render(current, black_box(width), turns, night);
                        times[current].push(start.elapsed().as_secs_f64() * 1000.0);
                        black_box(image);
                    }
                }
                times
                    .iter_mut()
                    .for_each(|samples| samples.sort_by(f64::total_cmp));
                let old = times[0][passes / 2];
                let new = times[1][passes / 2];
                println!(
                    "width={width} · {:3}° · night={night:<5} · bitmap {old:.3} ms · texture {new:.3} ms · {:.2}×",
                    u16::from(turns) * 90,
                    old / new
                );
            }
        }
    }
}
