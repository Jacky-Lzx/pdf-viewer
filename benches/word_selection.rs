#[allow(unused_imports)] // Test-only imports are unused in a harness-free bench.
#[path = "../src/text/boundaries.rs"]
mod boundaries;

use boundaries::{WordBoundaries, word_range};
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
        "{passes} alternating passes; 300 word-range queries/sample; build includes word bits and line table; no glyph lookup, extraction, GPU or UI."
    );
    for (repeats, lines) in [(100, 1), (1000, 1), (10000, 1), (10000, 100)] {
        let row = "Rust cafe\u{301} can't 中文 👩‍💻 ".repeat(repeats / lines);
        let text = std::iter::repeat_n(row.as_str(), lines)
            .collect::<Vec<_>>()
            .join("\n");
        let positions: Vec<_> = text.char_indices().map(|(offset, _)| offset).collect();
        let offsets: Vec<_> = (0..300)
            .map(|index| positions[(index * 7919) % positions.len()])
            .collect();
        let index = WordBoundaries::new(&text);
        index.prepare_lines(&text);
        for &offset in &offsets {
            assert_eq!(index.word_range(&text, offset), word_range(&text, offset));
            assert!(index.contains(index.word_range(&text, offset).start));
        }
        let mut builds = Vec::with_capacity(passes);
        let mut times = [Vec::with_capacity(passes), Vec::with_capacity(passes)];
        for pass in 0..passes {
            let start = Instant::now();
            let rebuilt = WordBoundaries::new(black_box(&text));
            rebuilt.prepare_lines(black_box(&text));
            builds.push(start.elapsed().as_secs_f64() * 1000.0);
            black_box(rebuilt);
            for order in 0..2 {
                let improved = (pass + order) % 2;
                let start = Instant::now();
                let mut checksum = 0;
                for &offset in &offsets {
                    let range = if improved == 1 {
                        index.word_range(black_box(&text), black_box(offset))
                    } else {
                        word_range(black_box(&text), black_box(offset))
                    };
                    checksum += range.start + range.end;
                }
                times[improved].push(start.elapsed().as_secs_f64() * 1000.0);
                black_box(checksum);
            }
        }
        println!(
            "{} bytes, {lines} lines: scan {:.4} ms, indexed {:.4} ms, build {:.4} ms, {} bytes index",
            text.len(),
            median(&mut times[0]),
            median(&mut times[1]),
            median(&mut builds),
            index.memory_bytes()
        );
    }
}
