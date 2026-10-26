#[allow(unused_imports)] // Test-only imports are unused in a harness-free bench.
#[path = "../src/text/search_index.rs"]
mod search_index;

use caseless::Caseless;
use search_index::SearchIndex;
use std::{hint::black_box, time::Instant};

// The previous index always allocated a usize for every folded UTF-8 byte.
fn mapped_index(text: &str) -> (String, Vec<usize>) {
    let mut folded = String::with_capacity(text.len());
    let mut source = Vec::with_capacity(text.len());
    for (offset, ch) in text.char_indices() {
        if ch.is_ascii() {
            folded.push(ch.to_ascii_lowercase());
            source.push(offset);
            continue;
        }
        for lower in std::iter::once(ch).default_case_fold() {
            folded.push(lower);
            source.extend(std::iter::repeat_n(offset, lower.len_utf8()));
        }
    }
    (folded, source)
}

fn median(samples: &mut [f64]) -> f64 {
    samples.sort_by(f64::total_cmp);
    samples[samples.len() / 2]
}

fn main() {
    let passes: usize = std::env::args()
        .nth(1)
        .and_then(|value| value.parse().ok())
        .unwrap_or(9);
    assert!((1..=1000).contains(&passes));
    println!(
        "{passes} alternating passes; medians per index build (20 builds/sample); buffer capacity only, no extraction, matching, GPU or UI."
    );
    for (kind, text) in [
        ("ASCII", "Rust PDF reader, 2026.\n", 100),
        ("ASCII", "Rust PDF reader, 2026.\n", 1000),
        ("ASCII", "Rust PDF reader, 2026.\n", 10000),
        ("CJK", "中文阅读器，跨平台运行。\n", 10000),
        ("Greek/Latin", "ÉCOLE école Σσς ΟΣ ος\n", 10000),
        ("Unicode", "Rust Straße Σσς ﬃ 中文 İ\n", 10000),
    ]
    .into_iter()
    .map(|(kind, row, repeats)| (kind, row.repeat(repeats)))
    .chain(std::iter::once((
        "Late expansion",
        format!("{}ß", "中文 ÉCOLE Σσς 👩‍💻\n".repeat(10000)),
    ))) {
        let reference = mapped_index(&text);
        let index = SearchIndex::new(&text);
        assert_eq!(index.folded, reference.0);
        assert!(index.memory_bytes() <= SearchIndex::reservation(&text));
        for (offset, ch) in index.folded.char_indices() {
            let end = offset + ch.len_utf8();
            let source_start = reference.1[offset];
            let source_last = reference.1[end - 1];
            let expected =
                source_start..source_last + text[source_last..].chars().next().unwrap().len_utf8();
            assert_eq!(index.source_range(&text, offset..end), expected);
            let partial = (offset > 0 && reference.1[offset - 1] == reference.1[offset])
                || (end < reference.1.len() && reference.1[end - 1] == reference.1[end]);
            assert_eq!(index.partial_expansion(offset..end), partial);
        }
        let mut times = [Vec::with_capacity(passes), Vec::with_capacity(passes)];
        for pass in 0..passes {
            for order in 0..2 {
                let improved = (pass + order) % 2;
                let mut elapsed = 0.0;
                for _ in 0..20 {
                    let start = Instant::now();
                    if improved == 1 {
                        let index = SearchIndex::new(black_box(&text));
                        elapsed += start.elapsed().as_secs_f64();
                        black_box(index);
                    } else {
                        let index = mapped_index(black_box(&text));
                        elapsed += start.elapsed().as_secs_f64();
                        black_box(index);
                    }
                }
                times[improved].push(elapsed * 1000.0 / 20.0);
            }
        }
        println!(
            "{kind}, {} bytes: mapped {:.4} ms, compact {:.4} ms; mapped {} bytes, compact {} bytes",
            text.len(),
            median(&mut times[0]),
            median(&mut times[1]),
            reference.0.capacity() + reference.1.capacity() * std::mem::size_of::<usize>(),
            index.memory_bytes()
        );
    }
}
