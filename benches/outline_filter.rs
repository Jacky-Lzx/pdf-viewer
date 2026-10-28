#[path = "support/allocations.rs"]
mod allocation_counter;
#[allow(dead_code, unused_imports)]
#[path = "../src/destination.rs"]
mod destination;
#[allow(dead_code, unused_imports)]
#[path = "../src/geometry.rs"]
mod geometry;
#[allow(dead_code)]
#[path = "../src/list_navigation.rs"]
mod list_navigation;
#[allow(dead_code, unused_imports)]
#[path = "../src/navigation.rs"]
mod navigation;
#[allow(dead_code, unused_imports)]
#[path = "../src/outline.rs"]
mod outline;
#[allow(dead_code, unused_imports)]
#[path = "../src/outline_view.rs"]
mod outline_view;
#[allow(dead_code, unused_imports)]
#[path = "../src/pdf_objects.rs"]
mod pdf_objects;
#[allow(dead_code, unused_imports)]
#[path = "../src/pdf_string.rs"]
mod pdf_string;
#[allow(dead_code, unused_imports)]
#[path = "support/outline_view_previous.rs"]
mod previous;

use outline::OutlineEntry;
use outline_view::OutlineView;
use std::{hint::black_box, time::Instant};

fn main() {
    let passes: usize = std::env::args()
        .nth(1)
        .and_then(|value| value.parse().ok())
        .unwrap_or(9);
    assert!((1..=1000).contains(&passes));
    println!(
        "{passes} alternating passes; median ms/100 complete refilters, query folding, matching, ancestor retention, selection, visible rows and checksum reads. Exact d900810 baseline versus reused filter markers; prepared titles, no PDF parsing, widgets or GPU. Allocation counts cover one warm refilter."
    );
    for count in [80, 10_000] {
        let entries: Vec<_> = (0..count)
            .map(|index| OutlineEntry {
                title: format!(
                    "Chapter {index:05} — Rust PDF rendering and long document navigation — Straße ΟΣ 中文阅读器"
                ),
                destination: None,
                depth: usize::from(index % 8 != 0),
            })
            .collect();
        let folded: Vec<_> = entries
            .iter()
            .map(|entry| caseless::default_case_fold_str(&entry.title))
            .collect();
        let mut view = OutlineView::default();
        let mut previous = previous::OutlineView::default();
        previous.set_entries(entries.clone());
        previous.fold_all(true);
        view.set_entries(entries);
        view.fold_all(true);
        for (kind, query) in [
            ("one hit", format!("{:05} chapter", count - 1)),
            ("absent", "selection snapshots".into()),
            ("many hits", "rendering document".into()),
            ("Unicode", "STRASSE σ 阅读器".into()),
            ("one byte", "r".into()),
        ] {
            let query_folded = caseless::default_case_fold_str(&query);
            let words: Vec<_> = query_folded.split_whitespace().collect();
            let mut keep: Vec<_> = folded
                .iter()
                .map(|title| words.iter().all(|word| title.contains(word)))
                .collect();
            for index in 0..count {
                if keep[index] {
                    keep[index - index % 8] = true;
                }
            }
            let expected: Vec<_> = (0..count).filter(|&index| keep[index]).collect();
            view.query.clone_from(&query);
            previous.query.clone_from(&query);
            view.refilter();
            previous.refilter();
            assert_eq!(view.visible(), expected);
            assert_eq!(previous.visible(), view.visible());
            assert_eq!(previous.selected(), view.selected());
            let mut samples: [Vec<f64>; 2] = std::array::from_fn(|_| Vec::with_capacity(passes));
            let mut checksum = 0;
            for pass in 0..passes {
                for order in 0..2 {
                    let mode = (pass + order) % 2;
                    let start = Instant::now();
                    for _ in 0..100 {
                        checksum = if mode == 0 {
                            black_box(&mut previous).refilter();
                            black_box(previous.visible()).iter().sum::<usize>()
                                + previous.selected().unwrap_or(0)
                        } else {
                            black_box(&mut view).refilter();
                            black_box(view.visible()).iter().sum::<usize>()
                                + view.selected().unwrap_or(0)
                        };
                        black_box(checksum);
                    }
                    samples[mode].push(start.elapsed().as_secs_f64() * 1000.0);
                }
            }
            let medians = samples.map(|mut samples| {
                samples.sort_by(f64::total_cmp);
                samples[passes / 2]
            });
            let before = allocation_counter::allocations(|| {
                previous.refilter();
            });
            let after = allocation_counter::allocations(|| {
                view.refilter();
            });
            assert_eq!(
                before.0,
                after.0 + 1,
                "warm refilters must reuse the marker allocation"
            );
            assert_eq!(before.1, after.1);
            println!(
                "{count} titles, {kind}: previous {:.4}, reused {:.4} ms; (alloc,realloc) {before:?} -> {after:?}; {} visible, checksum {checksum}",
                medians[0],
                medians[1],
                view.visible().len()
            );
        }
    }
}
