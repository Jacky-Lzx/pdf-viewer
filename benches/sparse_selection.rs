#[allow(dead_code, unused_imports)]
#[path = "../src/selection_index.rs"]
mod selection_index;
#[allow(dead_code, unused_imports)]
#[path = "../src/text.rs"]
mod text;

use hayro::kurbo::Rect;
use std::{hint::black_box, time::Instant};
use text::{PageText, TextSpan};

fn median(samples: &mut [f64]) -> f64 {
    samples.sort_by(f64::total_cmp);
    samples[samples.len() / 2]
}

fn main() {
    let passes = std::env::args()
        .nth(1)
        .and_then(|value| value.parse::<usize>().ok())
        .unwrap_or(9);
    assert!((1..=1000).contains(&passes));
    println!(
        "{passes} alternating passes, 300 word+line queries/sample through PageText; one mapped glyph per page. Build excludes source cloning and measures prepare_selection; queries exclude extraction, build, GPU and GUI."
    );
    for (repeats, lines) in [(10, 1), (200, 1), (2000, 1), (2000, 100)] {
        let row = "Rust cafe\u{301} 中文 👩‍💻 word ".repeat(repeats / lines);
        let value = std::iter::repeat_n(row.as_str(), lines)
            .collect::<Vec<_>>()
            .join("\r\n");
        let mut original = PageText::default();
        original.spans = vec![TextSpan {
            range: 0..value.len(),
            rect: Rect::new(50.0, 80.0, 66.0, 104.0),
        }];
        original.text = value;
        let prepared = original.clone();
        prepared.prepare_selection();
        assert!(prepared.selection_prepared());
        assert_eq!(original.selection_prepared(), original.text.len() < 4096);
        let positions: Vec<_> = original
            .text
            .char_indices()
            .map(|(offset, _)| offset)
            .collect();
        let offsets: Vec<_> = (0..300)
            .map(|index| positions[(index * 7919) % positions.len()])
            .collect();
        let mut expected = 0;
        for &offset in &offsets {
            let old = (original.word_range(offset), original.line_range(offset));
            let new = (prepared.word_range(offset), prepared.line_range(offset));
            assert_eq!(old, new);
            expected += old.0.start + old.0.end + old.1.start + old.1.end;
        }
        let mut samples = [Vec::new(), Vec::new()];
        let mut builds = Vec::new();
        for pass in 0..passes {
            let fresh = original.clone();
            let start = Instant::now();
            fresh.prepare_selection();
            builds.push(start.elapsed().as_secs_f64() * 1000.0);
            assert_eq!(fresh.memory_bytes(), prepared.memory_bytes());
            black_box(fresh);
            for order in 0..2 {
                let implementation = (pass + order) % 2;
                let page = if implementation == 0 {
                    &original
                } else {
                    &prepared
                };
                let start = Instant::now();
                let mut checksum = 0;
                for &offset in &offsets {
                    let word = page.word_range(black_box(offset));
                    let line = page.line_range(black_box(offset));
                    checksum += word.start + word.end + line.start + line.end;
                }
                samples[implementation].push(start.elapsed().as_secs_f64() * 1000.0);
                assert_eq!(black_box(checksum), expected);
            }
        }
        println!(
            "{} bytes, {lines} lines: scan {:.4} ms, prepared {:.4} ms, build {:.4} ms, {} additional index bytes",
            original.text.len(),
            median(&mut samples[0]),
            median(&mut samples[1]),
            median(&mut builds),
            prepared.memory_bytes() - original.memory_bytes()
        );
    }
}
