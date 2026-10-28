#[path = "support/allocations.rs"]
mod allocation_counter;
#[path = "support/page_labels.rs"]
mod label_fixture;
#[allow(dead_code, unused_imports)]
#[path = "../src/page_labels.rs"]
mod page_labels;
#[allow(dead_code, unused_imports)]
#[path = "../src/pdf_objects.rs"]
mod pdf_objects;
#[allow(dead_code, unused_imports)]
#[path = "../src/pdf_string.rs"]
mod pdf_string;

use label_fixture::fixture;
use page_labels::PageLabels;
use std::{hint::black_box, time::Instant};

fn baseline(labels: &PageLabels, first: usize, second: Option<usize>, total: usize) -> String {
    format!(
        "{} / {total}",
        if let Some(second) = second {
            format!(
                "{}–{}",
                labels.display_name(first),
                labels.display_name(second)
            )
        } else {
            labels.display_name(first)
        }
    )
}

fn batch(labels: &PageLabels, facing: bool, old: bool) {
    for index in 0..12 {
        let page = if facing { (index % 6) * 2 } else { index };
        let second = facing.then_some(page + 1);
        black_box(if old {
            baseline(labels, page, second, 12)
        } else {
            labels.status_caption(page, second, 12)
        });
    }
}

fn main() {
    let passes: usize = std::env::args()
        .nth(1)
        .and_then(|value| value.parse().ok())
        .unwrap_or(9);
    assert!((1..=1000).contains(&passes));
    println!(
        "{passes} alternating passes, 5000 batches/sample, 12 status captions/batch. Includes current display-name formatting, composition, allocations and drop; excludes PDF loading, spread geometry, layout, frame setup and GPU."
    );
    for (name, labels) in [
        ("no labels", PageLabels::default()),
        ("physical decimal", fixture("", "D")),
        ("short prefix", fixture("A-", "D")),
        ("Roman", fixture("", "r")),
        ("long Unicode", fixture(&"中文😀".repeat(80), "D")),
    ] {
        for facing in [false, true] {
            for first in [0, 1, 10, 11, 12, usize::MAX - 2] {
                for total in [0, 1, 12, 1200, usize::MAX] {
                    let second = facing.then_some(first + 1);
                    assert_eq!(
                        baseline(&labels, first, second, total),
                        labels.status_caption(first, second, total)
                    );
                }
            }
            let mut times: [Vec<f64>; 2] = std::array::from_fn(|_| Vec::new());
            for pass in 0..passes {
                for order in 0..2 {
                    let mode = (pass + order) % 2;
                    let start = Instant::now();
                    for _ in 0..5000 {
                        batch(black_box(&labels), facing, mode == 0);
                    }
                    times[mode].push(start.elapsed().as_secs_f64() * 200.0);
                }
            }
            let medians = times.map(|mut samples| {
                samples.sort_by(f64::total_cmp);
                samples[samples.len() / 2]
            });
            let allocations: Vec<_> = [true, false]
                .into_iter()
                .map(|old| allocation_counter::allocations(|| batch(&labels, facing, old)))
                .collect();
            println!(
                "{name}, {}: median µs/batch {:.3} -> {:.3}; (alloc,realloc) {allocations:?}",
                if facing { "pair" } else { "single" },
                medians[0],
                medians[1]
            );
        }
    }
}
