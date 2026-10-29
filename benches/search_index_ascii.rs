#[path = "support/allocations.rs"]
mod allocations;
#[allow(dead_code, unused_imports)]
#[path = "../src/text/search_index.rs"]
mod current;
#[allow(dead_code, unused_imports)]
#[path = "support/search_index_ascii_previous.rs"]
mod previous;

use std::{hint::black_box, time::Instant};

fn median(samples: &mut [f64]) -> f64 {
    samples.sort_by(f64::total_cmp);
    samples[samples.len() / 2]
}

fn main() {
    let passes: usize = std::env::args()
        .nth(1)
        .map_or(11, |value| value.parse().unwrap());
    assert!((1..=1000).contains(&passes));
    println!(
        "{passes} alternating passes; median microseconds/index build including drop, 16 builds/sample (128 for short text); frozen 25f1e76 versus current; buffers include folded text and source offsets, no extraction, matching, UI, GPU or RSS."
    );
    let ascii = "RUST PDF READER, BUILT FOR LONG DOCUMENTS.\n".repeat(5000);
    let unicode = "中文 ÉCOLE Σσς 👩‍💻\n".repeat(5000);
    for (kind, text) in [
        ("ASCII", ascii.clone()),
        ("Early expansion / ASCII suffix", format!("ß{ascii}")),
        ("Unicode at end", format!("{ascii}中")),
        (
            "Long ASCII runs",
            format!("{}中ß", "A".repeat(4096)).repeat(64),
        ),
        (
            "Short ASCII runs",
            "THE PDF READER Straße 2026 中文\n".repeat(5000),
        ),
        ("Short mixed field", "Document Straße résumé 2026".into()),
        ("Unchanged Unicode", unicode),
        ("CJK", "中文阅读器跨平台文件目录章節標題\n".repeat(5000)),
        (
            "Dense expansions",
            "Rust Straße Σσς ﬃ 中文 İ\n".repeat(5000),
        ),
    ] {
        let before = previous::SearchIndex::new(&text);
        let after = current::SearchIndex::new(&text);
        assert_eq!(before.folded, after.folded);
        assert!(after.memory_bytes() <= current::SearchIndex::reservation(&text));
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
        let counts = [
            allocations::allocations(|| {
                black_box(previous::SearchIndex::new(black_box(&text)));
            }),
            allocations::allocations(|| {
                black_box(current::SearchIndex::new(black_box(&text)));
            }),
        ];
        let builds = if text.len() < 256 { 128 } else { 16 };
        let mut times = [Vec::with_capacity(passes), Vec::with_capacity(passes)];
        for pass in 0..passes {
            for order in 0..2 {
                let mode = (pass + order) % 2;
                let start = Instant::now();
                for _ in 0..builds {
                    if mode == 0 {
                        black_box(previous::SearchIndex::new(black_box(&text)));
                    } else {
                        black_box(current::SearchIndex::new(black_box(&text)));
                    }
                }
                times[mode].push(start.elapsed().as_secs_f64() * 1_000_000.0 / builds as f64);
            }
        }
        println!(
            "{kind}, {} bytes: {:.3} -> {:.3} us; buffers {} -> {} B; alloc/realloc {:?} -> {:?}",
            text.len(),
            median(&mut times[0]),
            median(&mut times[1]),
            before.memory_bytes(),
            after.memory_bytes(),
            counts[0],
            counts[1]
        );
    }
}
