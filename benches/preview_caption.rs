#[path = "support/allocations.rs"]
mod allocation_counter;
#[path = "../src/app/sidebar/pages/caption.rs"]
mod caption;

use eframe::egui::{self, Color32};
use std::{hint::black_box, time::Instant};

fn frame(context: &egui::Context, mode: usize, count: bool) -> (f64, (usize, usize)) {
    let mut elapsed = 0.0;
    let mut allocations = (0, 0);
    let output = context.run(
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(400.0, 820.0),
            )),
            ..Default::default()
        },
        |context| {
            let painter = context.layer_painter(egui::LayerId::background());
            let draw = || {
                for row in 0..12 {
                    let card = egui::Rect::from_min_size(
                        egui::pos2(20.0, row as f32 * 36.0),
                        egui::vec2(228.0, 32.0),
                    );
                    let text = format!("Page {} · Appendix", row + 1);
                    if mode == 0 {
                        painter.text(
                            egui::pos2(card.center().x, card.bottom() - 8.0),
                            egui::Align2::CENTER_BOTTOM,
                            text,
                            egui::FontId::proportional(13.0),
                            Color32::BLACK,
                        );
                    } else {
                        caption::paint(&painter, card, text, Color32::BLACK);
                    }
                }
            };
            let start = Instant::now();
            if count {
                allocations = allocation_counter::allocations(draw);
            } else {
                draw();
            }
            elapsed = start.elapsed().as_secs_f64() * 1_000_000.0;
        },
    );
    black_box(output);
    (elapsed, allocations)
}

fn main() {
    let passes: usize = std::env::args()
        .nth(1)
        .and_then(|value| value.parse().ok())
        .unwrap_or(9);
    assert!((1..=1000).contains(&passes));
    let context = egui::Context::default();
    let mut times: [Vec<f64>; 2] = std::array::from_fn(|_| Vec::new());
    for pass in 0..passes {
        for order in 0..2 {
            let mode = (pass + order) % 2;
            for _ in 0..4 {
                frame(&context, mode, false);
            }
            let sample: f64 = (0..500).map(|_| frame(&context, mode, false).0).sum();
            times[mode].push(sample / 500.0);
        }
    }
    let medians = times.map(|mut samples| {
        samples.sort_by(f64::total_cmp);
        samples[samples.len() / 2]
    });
    let allocations: Vec<_> = (0..2)
        .map(|mode| {
            for _ in 0..4 {
                frame(&context, mode, false);
            }
            frame(&context, mode, true).1
        })
        .collect();
    println!(
        "{passes} alternating passes, 500 frames/sample, 12 captions/frame; caption formatting, warm egui layout lookup and shape insertion only. Excludes PDF labels, frame setup, font cold starts and GPU. Identical short captions fit both layouts."
    );
    println!(
        "median µs/frame: Painter::text {:.3}, bounded owned layout {:.3}; (alloc,realloc) per frame {allocations:?}",
        medians[0], medians[1]
    );
}
