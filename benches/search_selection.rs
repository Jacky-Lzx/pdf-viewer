#[path = "support/allocations.rs"]
mod allocation_counter;
#[allow(dead_code, unused_imports)]
#[path = "../src/selection_index.rs"]
mod selection_index;
#[allow(dead_code, unused_imports)]
#[path = "../src/text.rs"]
mod text;

use std::{hint::black_box, time::Instant};

fn previous(text: &str) -> String {
    // Preserve b26d37d's complete selection-to-query conversion.
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn main() {
    let passes: usize = std::env::args()
        .nth(1)
        .and_then(|value| value.parse().ok())
        .unwrap_or(9);
    assert!((1..=1000).contains(&passes));
    println!(
        "{passes} alternating passes; complete selection-to-query conversion including result allocation and release; no PDF extraction, query matching, UI or GPU."
    );
    for (kind, value, iterations) in [
        ("single word", "  中文😀word  ".to_owned(), 10000),
        (
            "sentence",
            "A native PDF reader, written in Rust.".to_owned(),
            10000,
        ),
        (
            "multiline",
            "  Straße\r\n\t中文\u{a0}reader \n".repeat(20),
            2000,
        ),
        (
            "large selection",
            "Rust 中文 cafe\u{301} reader\r\n".repeat(10000),
            20,
        ),
        (
            "sparse whitespace",
            format!("word{}tail", " \t\r\n".repeat(10000)),
            100,
        ),
        ("blank selection", " \t\r\n\u{a0}".repeat(100), 10000),
    ] {
        let expected = previous(&value);
        let current = text::selection_query(&value);
        assert_eq!(current, expected);
        assert!(current.capacity() <= current.len().saturating_mul(2));
        let word_buffer_bytes =
            value.split_whitespace().collect::<Vec<_>>().capacity() * std::mem::size_of::<&str>();
        let retained_bytes = current.capacity();
        let allocations_before = allocation_counter::allocations(|| {
            black_box(previous(&value));
        });
        let allocations_after = allocation_counter::allocations(|| {
            black_box(text::selection_query(&value));
        });
        assert!(allocations_after.0 <= allocations_before.0);
        let mut times = [Vec::new(), Vec::new()];
        for pass in 0..passes {
            for order in 0..2 {
                let mode = (pass + order) % 2;
                let start = Instant::now();
                for _ in 0..iterations {
                    let query = if mode == 0 {
                        previous(black_box(&value))
                    } else {
                        text::selection_query(black_box(&value))
                    };
                    assert_eq!(query, expected);
                    black_box(query);
                }
                times[mode].push(start.elapsed().as_secs_f64() * 1e6 / f64::from(iterations));
            }
        }
        let medians = times.map(|mut values| {
            values.sort_by(f64::total_cmp);
            values[passes / 2]
        });
        println!(
            "{kind}, {} source bytes -> {} query bytes: median µs/conversion {:.4} -> {:.4}; (alloc,realloc) {allocations_before:?} -> {allocations_after:?}; previous temporary word buffer {word_buffer_bytes} bytes, current retained {retained_bytes} bytes",
            value.len(),
            expected.len(),
            medians[0],
            medians[1]
        );
    }
}
