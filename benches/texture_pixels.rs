#[path = "../src/bitmap.rs"]
mod bitmap;
#[path = "../src/texture_pixels.rs"]
mod texture_pixels;

use bitmap::{Bitmap, rotate, rotate_inverted};
use eframe::egui::ColorImage;
use std::{hint::black_box, time::Instant};

fn baseline(bitmap: Bitmap, turns: u8, night: bool) -> ColorImage {
    let bitmap = if night {
        rotate_inverted(bitmap, turns)
    } else {
        rotate(bitmap, turns)
    };
    ColorImage::from_rgba_premultiplied(bitmap.size, &bitmap.rgba)
}

fn main() {
    let passes = std::env::args()
        .nth(1)
        .and_then(|value| value.parse().ok())
        .unwrap_or(9);
    assert!((1..=1000).contains(&passes));
    println!(
        "Alternating transform-then-copy/direct texture output, {passes} passes each; medians include output allocations and input release, exclude input cloning."
    );
    for size @ [w, h] in [[1200, 1600], [3200, 4000]] {
        let rgba: Vec<_> = (0..w * h * 4)
            .map(|index| (index * 17 + index / 257) as u8)
            .collect();
        let input = || Bitmap {
            size,
            rgba: rgba.clone(),
        };
        for night in [false, true] {
            for turns in 0..4 {
                assert!(
                    baseline(input(), turns, night)
                        == texture_pixels::prepare(input(), turns, night)
                );
                let mut times = [Vec::with_capacity(passes), Vec::with_capacity(passes)];
                for pass in 0..passes {
                    for order in 0..2 {
                        let current = (pass + order) % 2;
                        let bitmap = input();
                        let start = Instant::now();
                        let image = if current == 1 {
                            texture_pixels::prepare(bitmap, turns, night)
                        } else {
                            baseline(bitmap, turns, night)
                        };
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
                    "{w}×{h} · {:3}° · night={night:<5} · previous {old:.3} ms · direct {new:.3} ms · {:.2}×",
                    u16::from(turns) * 90,
                    old / new
                );
            }
        }
    }
}
