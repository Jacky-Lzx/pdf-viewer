#[path = "support/allocations.rs"]
mod allocations;
#[allow(dead_code, unused_imports)]
#[path = "../src/text/search_index.rs"]
mod current;
#[allow(dead_code, unused_imports)]
#[path = "support/search_index_suffix_previous.rs"]
mod previous;

use std::{hint::black_box, ops::Range, time::Instant};

fn median(samples: &mut [f64]) -> f64 {
    samples.sort_by(f64::total_cmp);
    samples[samples.len() / 2]
}

fn main() {
    let passes: usize = std::env::args()
        .nth(1)
        .map_or(9, |value| value.parse().unwrap());
    assert!((1..=1000).contains(&passes));
    println!(
        "{passes} alternating passes; build medians include build/drop, 16 builds/sample; map timings include source_range and partial_expansion for 1024 ranges, 32 iterations/sample; capacities include folded text and source map, not extraction, matching, geometry, UI or RSS."
    );
    let ascii = "Rust PDF reader, built for long documents.\n".repeat(5000);
    let unicode = "中文 ÉCOLE Σσς 👩‍💻\n".repeat(5000);
    for (kind, text) in [
        ("ASCII", ascii.clone()),
        ("Early ß / ASCII suffix", format!("ß{ascii}")),
        ("Early İ / Unicode suffix", format!("İ{unicode}")),
        ("Changes near start", format!("ß hello İ 中 ﬃ{ascii}")),
        ("Separated changes", format!("ß{ascii}İ{unicode}")),
        ("Dense changes", "Rust Straße Σσς ﬃ 中文 İ\n".repeat(5000)),
        ("Late change", format!("{unicode}ß")),
    ] {
        let before = previous::SearchIndex::new(&text);
        let after = current::SearchIndex::new(&text);
        assert_eq!(before.folded, after.folded);
        assert!(after.memory_bytes() <= current::SearchIndex::reservation(&text));
        let boundaries: Vec<_> = after
            .folded
            .char_indices()
            .map(|(offset, _)| offset)
            .collect();
        let ranges: Vec<Range<usize>> = (0..1024)
            .map(|sample| {
                let index = sample * boundaries.len() / 1024;
                let start = boundaries[index];
                let end = boundaries
                    .get(index + 13)
                    .copied()
                    .unwrap_or(after.folded.len());
                start..end
            })
            .collect();
        for (start, ch) in after.folded.char_indices() {
            for end in [start + ch.len_utf8(), after.folded.len()] {
                assert_eq!(
                    before.source_range(&text, start..end),
                    after.source_range(&text, start..end)
                );
                assert_eq!(
                    before.partial_expansion(start..end),
                    after.partial_expansion(start..end)
                );
            }
        }
        let mut counts = [(0, 0); 2];
        counts[0] = allocations::allocations(|| {
            black_box(previous::SearchIndex::new(black_box(&text)));
        });
        counts[1] = allocations::allocations(|| {
            black_box(current::SearchIndex::new(black_box(&text)));
        });
        let mut builds = [Vec::new(), Vec::new()];
        let mut lookups = [Vec::new(), Vec::new()];
        for pass in 0..passes {
            for order in 0..2 {
                let variant = (pass + order) % 2;
                let start = Instant::now();
                for _ in 0..16 {
                    if variant == 0 {
                        black_box(previous::SearchIndex::new(black_box(&text)));
                    } else {
                        black_box(current::SearchIndex::new(black_box(&text)));
                    }
                }
                builds[variant].push(start.elapsed().as_secs_f64() * 1_000_000.0 / 16.0);
                let start = Instant::now();
                for _ in 0..32 {
                    for range in &ranges {
                        let range = black_box(range.clone());
                        if variant == 0 {
                            black_box(before.source_range(black_box(&text), range.clone()));
                            black_box(before.partial_expansion(range));
                        } else {
                            black_box(after.source_range(black_box(&text), range.clone()));
                            black_box(after.partial_expansion(range));
                        }
                    }
                }
                lookups[variant].push(start.elapsed().as_secs_f64() * 1_000_000.0 / 32.0);
            }
        }
        println!(
            "{kind}, {} bytes: build {:.3} -> {:.3} us; map 1024 ranges {:.3} -> {:.3} us; buffers {} -> {} bytes; alloc/realloc {:?} -> {:?}",
            text.len(),
            median(&mut builds[0]),
            median(&mut builds[1]),
            median(&mut lookups[0]),
            median(&mut lookups[1]),
            before.memory_bytes(),
            after.memory_bytes(),
            counts[0],
            counts[1]
        );
    }
}
