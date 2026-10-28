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
        "{passes} passes; 100 complete refilters/sample, query folding, matching, ancestor retention, selection, visible rows and checksum reads; prepared titles, no PDF parsing, widgets or GPU."
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
            view.refilter();
            assert_eq!(view.visible(), expected);
            let mut samples = Vec::with_capacity(passes);
            let mut checksum = 0;
            for _ in 0..passes {
                let start = Instant::now();
                for _ in 0..100 {
                    black_box(&mut view).refilter();
                    checksum = black_box(view.visible()).iter().sum::<usize>()
                        + view.selected().unwrap_or(0);
                    black_box(checksum);
                }
                samples.push(start.elapsed().as_secs_f64() * 1000.0);
            }
            samples.sort_by(f64::total_cmp);
            println!(
                "{count} titles, {kind}: {:.4} ms, {} visible, checksum {checksum}",
                samples[passes / 2],
                view.visible().len()
            );
        }
    }
}
