#[path = "../src/selection_index.rs"]
mod selection_index;
#[path = "../src/text/span.rs"]
mod text;

use hayro::kurbo::{Point, Rect};
use selection_index::SelectionIndex;
use std::{hint::black_box, time::Instant};
use text::TextSpan;

fn baseline(spans: &[TextSpan], point: Point) -> Option<usize> {
    if !point.x.is_finite() || !point.y.is_finite() {
        return None;
    }
    spans
        .iter()
        .enumerate()
        .filter(|(_, span)| span.rect.is_finite() && !span.range.is_empty())
        .min_by(|(_, a), (_, b)| {
            let distance = |rect: Rect| {
                (
                    (rect.y0 - point.y).max(0.0).max(point.y - rect.y1),
                    (rect.x0 - point.x).max(0.0).max(point.x - rect.x1),
                )
            };
            let (ay, ax) = distance(a.rect);
            let (by, bx) = distance(b.rect);
            ay.total_cmp(&by).then_with(|| ax.total_cmp(&bx))
        })
        .map(|(index, _)| index)
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
        "{passes} alternating passes; 300 point queries/sample; build measured separately; no text extraction, GPU or UI drawing."
    );
    for rows in [30, 300, 1000] {
        let spans: Vec<_> = (0..rows * 100)
            .map(|index| {
                let x = (index % 100) as f64 * 8.0;
                let y = (index / 100) as f64 * 16.0 + (index % 7) as f64 * 0.15;
                TextSpan {
                    range: index..index + 1,
                    rect: Rect::new(x, y, x + 6.5, y + 10.0),
                }
            })
            .collect();
        let points: Vec<_> = (0..300)
            .map(|index| {
                Point::new(
                    (index * 37 % 130) as f64 * 8.0,
                    (index * 79 % rows) as f64 * 16.0 + if index % 3 == 0 { 13.5 } else { 5.0 },
                )
            })
            .collect();
        let index = SelectionIndex::new(&spans);
        for &point in &points {
            assert_eq!(index.nearest(&spans, point), baseline(&spans, point));
        }
        let mut builds = Vec::with_capacity(passes);
        let mut times = [Vec::with_capacity(passes), Vec::with_capacity(passes)];
        for pass in 0..passes {
            let start = Instant::now();
            let rebuilt = SelectionIndex::new(black_box(&spans));
            builds.push(start.elapsed().as_secs_f64() * 1000.0);
            black_box(rebuilt);
            for order in 0..2 {
                let improved = (pass + order) % 2;
                let start = Instant::now();
                let mut checksum = 0;
                for &point in &points {
                    checksum += if improved == 1 {
                        index.nearest(black_box(&spans), black_box(point))
                    } else {
                        baseline(black_box(&spans), black_box(point))
                    }
                    .unwrap_or(0);
                }
                times[improved].push(start.elapsed().as_secs_f64() * 1000.0);
                black_box(checksum);
            }
        }
        let old = median(&mut times[0]);
        let new = median(&mut times[1]);
        let build = median(&mut builds);
        println!(
            "{} glyphs · baseline {old:.3} ms · indexed {new:.3} ms · build {build:.3} ms · index {:.3} MiB · {:.2}× query speed",
            spans.len(),
            index.memory_bytes() as f64 / 1_048_576.0,
            old / new
        );
    }
}
