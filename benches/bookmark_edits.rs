#[path = "../src/bookmark_index.rs"]
mod bookmark_index;

use std::{collections::BTreeSet, hint::black_box, time::Instant};

fn previous(
    bookmarks: &mut BTreeSet<usize>,
    pages: &mut Vec<usize>,
    page: usize,
    present: bool,
) -> bool {
    let changed = if present {
        bookmarks.insert(page)
    } else {
        bookmarks.remove(&page)
    };
    // Preserve the previous complete rebuild after every edit.
    pages.clear();
    pages.extend(bookmarks.iter().copied());
    changed
}

fn main() {
    let passes: usize = std::env::args()
        .nth(1)
        .and_then(|value| value.parse().ok())
        .unwrap_or(9);
    assert!((1..=1000).contains(&passes));
    println!(
        "{passes} alternating passes, µs/edit. Each sample removes and restores one bookmark repeatedly, including BTreeSet edits and ordered-index maintenance. State construction and correctness checks are outside timing; no UI, state serialization, PDF or GPU work."
    );
    for (count, iterations) in [
        (1, 20_000),
        (24, 10_000),
        (600, 4000),
        (10_000, 250),
        (100_000, 25),
    ] {
        for (position, page) in [("first", 0), ("middle", count / 2), ("last", count - 1)] {
            if count == 1 && position != "last" {
                continue;
            }
            let original: BTreeSet<_> = (0..count).collect();
            let snapshot: Vec<_> = original.iter().copied().collect();
            let mut times = [Vec::new(), Vec::new()];
            for pass in 0..passes {
                for order in 0..2 {
                    let mode = (pass + order) % 2;
                    let mut bookmarks = original.clone();
                    let mut pages = snapshot.clone();
                    let edit = if mode == 0 {
                        previous
                    } else {
                        bookmark_index::set
                    };
                    // Validate both the intermediate removal and restoration.
                    assert!(edit(&mut bookmarks, &mut pages, page, false));
                    assert_eq!(pages.len(), count - 1);
                    assert!(!pages.contains(&page));
                    assert!(pages.iter().copied().eq(bookmarks.iter().copied()));
                    assert!(edit(&mut bookmarks, &mut pages, page, true));
                    assert_eq!(pages, snapshot);
                    let storage = pages.as_ptr();
                    let capacity = pages.capacity();
                    let start = Instant::now();
                    for _ in 0..iterations {
                        for present in [false, true] {
                            black_box(edit(
                                black_box(&mut bookmarks),
                                black_box(&mut pages),
                                black_box(page),
                                present,
                            ));
                        }
                    }
                    times[mode]
                        .push(start.elapsed().as_secs_f64() * 1e6 / f64::from(iterations * 2));
                    assert_eq!(bookmarks, original);
                    assert_eq!(pages, snapshot);
                    assert_eq!(pages.as_ptr(), storage);
                    assert_eq!(pages.capacity(), capacity);
                }
            }
            let medians = times.map(|mut values| {
                values.sort_by(f64::total_cmp);
                values[passes / 2]
            });
            println!(
                "{count} bookmarks, {position}: {:.4} -> {:.4} µs/edit",
                medians[0], medians[1],
            );
        }
    }
}
