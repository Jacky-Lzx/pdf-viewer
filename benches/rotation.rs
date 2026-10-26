#[path = "../src/bitmap.rs"]
mod bitmap;

use bitmap::{Bitmap, rotate, rotate_inverted};
use std::{hint::black_box, time::Instant};

// Original pixel-by-pixel implementation, retained only as a measurement baseline.
fn baseline(bitmap: Bitmap, turns: u8) -> Bitmap {
    let [w, h] = bitmap.size;
    let size = if turns % 2 == 1 { [h, w] } else { [w, h] };
    let mut rgba = vec![0; bitmap.rgba.len()];
    for y in 0..h {
        for x in 0..w {
            let (nx, ny) = match turns % 4 {
                1 => (h - 1 - y, x),
                2 => (w - 1 - x, h - 1 - y),
                _ => (y, w - 1 - x),
            };
            rgba[(ny * size[0] + nx) * 4..(ny * size[0] + nx + 1) * 4]
                .copy_from_slice(&bitmap.rgba[(y * w + x) * 4..(y * w + x + 1) * 4]);
        }
    }
    Bitmap { size, rgba }
}

// Previous night-mode path: scan RGB first, then perform the optimized rotation.
fn two_pass(mut bitmap: Bitmap, turns: u8) -> Bitmap {
    for pixel in bitmap.rgba.as_chunks_mut::<4>().0 {
        for channel in &mut pixel[..3] {
            *channel = 255 - *channel;
        }
    }
    rotate(bitmap, turns)
}

fn main() {
    let passes: usize = std::env::args()
        .nth(1)
        .and_then(|value| value.parse().ok())
        .unwrap_or(9);
    assert!(
        (1..=1000).contains(&passes),
        "Passes must be between 1 and 1000"
    );
    let night = std::env::args().nth(2).as_deref() == Some("night");
    let reference = if night { two_pass } else { baseline };
    let current = if night { rotate_inverted } else { rotate };
    if night {
        println!("Night mode: baseline inverts then rotates; current fuses both operations.");
    }
    println!(
        "Alternating baseline/current, {passes} passes each; medians exclude input cloning, include output allocation."
    );
    for size @ [w, h] in [[1200, 1600], [3200, 4000], [4096, 3072]] {
        let pixels: Vec<u8> = (0..w * h * 4)
            .map(|value| (value * 17 + value / 257) as u8)
            .collect();
        let input = || Bitmap {
            size,
            rgba: pixels.clone(),
        };
        for turns in [1, 2, 3] {
            let expected = reference(input(), turns);
            let actual = current(input(), turns);
            assert_eq!(actual.size, expected.size);
            assert_eq!(
                actual.rgba, expected.rgba,
                "rotation must be pixel-identical"
            );
            drop((expected, actual));
            let mut times = [Vec::with_capacity(passes), Vec::with_capacity(passes)];
            for pass in 0..passes {
                for order in 0..2 {
                    let improved = (pass + order) % 2;
                    let bitmap = input();
                    let start = Instant::now();
                    let output = if improved == 1 {
                        current(bitmap, turns)
                    } else {
                        reference(bitmap, turns)
                    };
                    times[improved].push(start.elapsed().as_secs_f64() * 1000.0);
                    black_box(output);
                }
            }
            times
                .iter_mut()
                .for_each(|samples| samples.sort_by(f64::total_cmp));
            let old = times[0][passes / 2];
            let new = times[1][passes / 2];
            println!(
                "{w}×{h} · {:3}° · baseline {old:.3} ms · current {new:.3} ms · {:.2}×",
                u16::from(turns) * 90,
                old / new
            );
        }
    }
}
