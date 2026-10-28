#[path = "support/allocations.rs"]
mod allocations;

#[allow(dead_code, unused_imports)]
mod boundaries {
    // Frozen constructor from 351011e; both paths return the same bitset type.
    pub fn previous(text: &str) -> WordBoundaries {
        let mut bits = vec![0; (text.len() + 1).div_ceil(64)];
        for offset in text
            .split_word_bound_indices()
            .map(|(offset, _)| offset)
            .chain(std::iter::once(text.len()))
        {
            bits[offset / 64] |= 1 << (offset % 64);
        }
        WordBoundaries {
            bits,
            line_breaks: OnceLock::new(),
        }
    }

    include!("../src/text/boundaries.rs");
}

use boundaries::WordBoundaries;
use std::{hint::black_box, time::Instant};

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
        "{passes} alternating passes, 16 builds/sample; allocation, construction and release of source word bits; no PDF extraction, case folding, matching, line table, geometry, UI or GPU."
    );
    for (name, text) in [
        ("Empty", String::new()),
        ("Short ASCII", "Can't a_b v1.2 a:b 123,456;789!\r\n".into()),
        (
            "ASCII page",
            "Rust PDF reader, built for long documents.\n".repeat(50),
        ),
        (
            "Large ASCII page",
            "Rust PDF reader, built for long documents.\n".repeat(5000),
        ),
        (
            "Identifiers and bridges",
            "can't _word_ a:b a.b 1.2 1,2 1;2 a.1 1.a\r\n".repeat(5000),
        ),
        ("Long ASCII word", "a".repeat(215000)),
        (
            "ASCII controls",
            String::from_utf8((0..128).collect()).unwrap().repeat(1000),
        ),
        (
            "Mixed Unicode",
            "中文阅读器 Straße Σσς 👩‍💻 café\n".repeat(5000),
        ),
        (
            "Late Unicode",
            format!(
                "{}ß",
                "Rust PDF reader, built for long documents.\n".repeat(5000)
            ),
        ),
    ] {
        let reference = boundaries::previous(&text);
        let current = WordBoundaries::new(&text);
        for offset in 0..=text.len() + 65 {
            assert_eq!(
                current.contains(offset),
                reference.contains(offset),
                "{name} at {offset}"
            );
        }
        assert_eq!(current.memory_bytes(), reference.memory_bytes());
        let mut counts = [(0, 0); 2];
        for (index, constructor) in [boundaries::previous, WordBoundaries::new]
            .into_iter()
            .enumerate()
        {
            counts[index] = allocations::allocations(|| {
                black_box(constructor(black_box(&text)));
            });
        }
        let mut samples = [Vec::with_capacity(passes), Vec::with_capacity(passes)];
        for pass in 0..passes {
            for order in 0..2 {
                let index = (pass + order) % 2;
                let start = Instant::now();
                for _ in 0..16 {
                    black_box(if index == 0 {
                        boundaries::previous(black_box(&text))
                    } else {
                        WordBoundaries::new(black_box(&text))
                    });
                }
                samples[index].push(start.elapsed().as_secs_f64() * 1_000_000.0 / 16.0);
            }
        }
        println!(
            "{name}, {} bytes: unicode {:.4} us, current {:.4} us; alloc/realloc {:?} -> {:?}; {} bytes",
            text.len(),
            median(&mut samples[0]),
            median(&mut samples[1]),
            counts[0],
            counts[1],
            current.memory_bytes()
        );
    }
}
