#[path = "support/allocations.rs"]
mod allocation_counter;
#[path = "../src/app/tooltips.rs"]
mod tooltips;

use eframe::egui;
use std::{hint::black_box, path::PathBuf, time::Instant};
use tooltips::LazyTooltip;

fn frame(context: &egui::Context, paths: &[PathBuf; 4], lazy: bool) -> usize {
    let output = context.run(
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(800.0, 600.0),
            )),
            time: Some(context.input(|input| input.time) + 1.0 / 60.0),
            ..Default::default()
        },
        |context| {
            egui::CentralPanel::default().show(context, |ui| {
                for path in paths {
                    let response = ui.button("Recent document");
                    if lazy {
                        response.on_hover_text_lazy(|| path.display().to_string());
                    } else {
                        // Preserve db190df's eagerly formatted welcome paths.
                        response.on_hover_text(path.display().to_string());
                    }
                }
            });
        },
    );
    black_box(output.shapes.len())
}

fn main() {
    let passes: usize = std::env::args()
        .nth(1)
        .and_then(|value| value.parse().ok())
        .unwrap_or(9);
    assert!((1..=1000).contains(&passes));
    println!(
        "{passes} alternating passes, 1000 complete egui frames/sample; four path tooltips on identical buttons with no pointer hover. Includes input, widgets, CPU layout/paint output and output release; warmed fonts, no filesystem access, PDF, GPU or application-level card layout."
    );
    for (kind, prefix) in [
        ("short ASCII", "/books/".to_owned()),
        (
            "long Unicode",
            format!("/books/{}", "文档😀segment/".repeat(70)),
        ),
    ] {
        let paths = std::array::from_fn(|index| PathBuf::from(format!("{prefix}book-{index}.pdf")));
        let before = egui::Context::default();
        let after = egui::Context::default();
        for _ in 0..64 {
            assert_eq!(frame(&before, &paths, false), frame(&after, &paths, true));
        }
        let allocations_before = allocation_counter::allocations(|| {
            frame(&before, &paths, false);
        });
        let allocations_after = allocation_counter::allocations(|| {
            frame(&after, &paths, true);
        });
        assert!(allocations_after.0 < allocations_before.0);
        let mut times = [Vec::new(), Vec::new()];
        for pass in 0..passes {
            for order in 0..2 {
                let mode = (pass + order) % 2;
                let context = if mode == 0 { &before } else { &after };
                let start = Instant::now();
                for _ in 0..1000 {
                    frame(context, black_box(&paths), mode == 1);
                }
                times[mode].push(start.elapsed().as_secs_f64() * 1000.0);
            }
        }
        let medians = times.map(|mut samples| {
            samples.sort_by(f64::total_cmp);
            samples[samples.len() / 2]
        });
        println!(
            "{kind}, {} path bytes/tooltip: median µs/frame {:.3} -> {:.3}; (alloc,realloc) {allocations_before:?} -> {allocations_after:?}",
            paths[0].as_os_str().len(),
            medians[0],
            medians[1]
        );
    }
}
