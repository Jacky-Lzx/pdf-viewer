#[path = "support/allocations.rs"]
mod allocation_counter;
#[allow(dead_code, unused_imports)]
#[path = "../src/search_result_text.rs"]
mod search_result_text;

use eframe::egui::{Color32, FontId};
use search_result_text::{ResultTextCache, result_label};
use std::{hint::black_box, sync::Arc, time::Instant};

fn frame(snippets: &[String], cache: &mut ResultTextCache, mode: usize, first: usize) {
    for (row, snippet) in snippets.iter().enumerate() {
        let create = || {
            result_label(
                format!("iv · {}", row + 1),
                snippet,
                4..11,
                FontId::default(),
                Color32::GOLD,
            )
        };
        match mode {
            0 => {
                black_box(format!("iv · {}   {snippet}", row + 1));
            }
            1 => {
                black_box(Arc::new(create()));
            }
            2 => {
                black_box(cache.get_or_insert_with(first + row, create));
            }
            // egui consumes an owned copy when it applies the live wrap width.
            3 => {
                black_box(Arc::unwrap_or_clone(
                    cache.get_or_insert_with(first + row, create),
                ));
            }
            _ => unreachable!(),
        }
    }
}

fn main() {
    let passes = std::env::args()
        .nth(1)
        .and_then(|s| s.parse().ok())
        .unwrap_or(9);
    assert!((1..=1000).contains(&passes));
    println!(
        "{passes} alternating passes; median µs/frame, 12 rows and 5000 frames/sample. Includes label text/section preparation and allocation/drop; excludes widgets, layout, PDF extraction and GPU. Owned-copy case includes egui's wrap-job clone."
    );
    for long in [false, true] {
        let snippets: Vec<_> = (0..12)
            .map(|row| {
                format!(
                    "前 Straße {} {row}",
                    if long {
                        "中文😀 ".repeat(20)
                    } else {
                        "PDF reader".into()
                    }
                )
            })
            .collect();
        let mut cache = ResultTextCache::default();
        cache.set_style(FontId::default(), Color32::GOLD);
        frame(&snippets, &mut cache, 2, 0);
        let mut times: [Vec<f64>; 5] = std::array::from_fn(|_| Vec::new());
        for pass in 0..passes {
            for order in 0..5 {
                let mode = (pass + order) % 5;
                let start = Instant::now();
                for iteration in 0..5000 {
                    // Run full-cache misses with fresh immutable result indices.
                    frame(
                        &snippets,
                        &mut cache,
                        mode.min(3),
                        if mode == 4 { (iteration + 1) * 12 } else { 0 },
                    );
                }
                times[mode].push(start.elapsed().as_secs_f64() * 200.0);
            }
        }
        let medians = times.map(|mut samples| {
            samples.sort_by(f64::total_cmp);
            samples[samples.len() / 2]
        });
        frame(&snippets, &mut cache, 2, 0);
        let allocations: Vec<_> = (0..4)
            .map(|mode| {
                allocation_counter::allocations(|| {
                    frame(&snippets, &mut cache, mode, 0);
                })
            })
            .collect();
        println!(
            "{}: plain {:.3}, highlighted rebuild {:.3}, cached {:.3}, cached owned copy {:.3}, full-cache misses + owned copy {:.3}; (alloc,realloc) per frame {allocations:?}",
            if long { "long Unicode" } else { "short mixed" },
            medians[0],
            medians[1],
            medians[2],
            medians[3],
            medians[4]
        );
    }
}
