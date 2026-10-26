#[allow(unused_imports)] // Test-only imports are unused in a harness-free bench.
#[path = "../src/text/snippet.rs"]
mod snippet;

use std::{hint::black_box, time::Instant};

fn original(text: &str, start: usize) -> String {
    let before: String = text[..start]
        .chars()
        .rev()
        .take(32)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect();
    let after: String = text[start..].chars().take(80).collect();
    format!("{before}{after}").replace('\n', " ")
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
        "{passes} alternating passes; 10000 snippets/sample including allocation and drop; no extraction, index building, matching, geometry, GPU or UI."
    );
    for (kind, row, repeats) in [
        ("Short Unicode", "İ 中文 👩‍💻\n", 1),
        ("ASCII", "Rust PDF reader, 2026.\n", 1000),
        ("Unicode", "Rust Straße Σσς ﬃ 中文 İ 👩‍💻 cafe\u{301}\n", 1000),
    ] {
        let text = row.repeat(repeats);
        let boundaries: Vec<_> = text
            .char_indices()
            .map(|(offset, _)| offset)
            .chain(std::iter::once(text.len()))
            .collect();
        let positions: Vec<_> = (0..10000)
            .map(|index| boundaries[(index * 7919) % boundaries.len()])
            .collect();
        for &start in &positions {
            assert_eq!(snippet::snippet(&text, start), original(&text, start));
        }
        let mut times = [Vec::with_capacity(passes), Vec::with_capacity(passes)];
        for pass in 0..passes {
            for order in 0..2 {
                let improved = (pass + order) % 2;
                let start = Instant::now();
                for &position in &positions {
                    black_box(if improved == 1 {
                        snippet::snippet(black_box(&text), black_box(position))
                    } else {
                        original(black_box(&text), black_box(position))
                    });
                }
                times[improved].push(start.elapsed().as_secs_f64() * 1000.0);
            }
        }
        println!(
            "{kind}, {} bytes: original {:.4} ms, slice {:.4} ms per 10000 snippets",
            text.len(),
            median(&mut times[0]),
            median(&mut times[1])
        );
    }
}
