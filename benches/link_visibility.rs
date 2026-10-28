#[path = "link_visibility/comments.rs"]
mod comments;

#[allow(dead_code, unused_imports)] // Test imports are unused in this harness-free bench.
#[path = "../src/geometry.rs"]
mod geometry;
#[allow(dead_code, unused_imports)]
#[path = "../src/visible_rects.rs"]
mod visible_rects;

use eframe::egui::{Rect, Vec2, pos2};
use hayro::kurbo::Rect as PdfRect;
use std::{hint::black_box, time::Instant};
use visible_rects::{PageView, VisibleRects};

fn checksum(rect: Rect) -> u64 {
    [rect.min.x, rect.min.y, rect.max.x, rect.max.y]
        .into_iter()
        .map(|value| u64::from(value.to_bits()))
        .sum()
}

fn original(rectangles: &[PdfRect], view: PageView, indices: &mut Vec<usize>) -> (usize, u64) {
    indices.clear();
    indices.extend(rectangles.iter().enumerate().filter_map(|(index, &rect)| {
        view.screen_rect(rect)
            .intersects(view.clip)
            .then_some(index)
    }));
    let sum = indices
        .iter()
        .map(|&index| {
            checksum(
                view.screen_rect(rectangles[index])
                    .intersect(view.page_rect)
                    .intersect(view.clip),
            )
        })
        .sum();
    (indices.len(), sum)
}

fn cached(rectangles: &[PdfRect], view: PageView, cache: &mut VisibleRects) -> (usize, u64) {
    cache.update(view, rectangles.iter().copied());
    (
        cache.indices.len(),
        cache.rects.iter().copied().map(checksum).sum(),
    )
}

fn main() {
    let passes: usize = std::env::args()
        .nth(1)
        .and_then(|value| value.parse().ok())
        .unwrap_or(9);
    assert!((1..=1000).contains(&passes));
    if std::env::args().nth(2).as_deref() == Some("comments") {
        comments::run(passes);
        return;
    }
    println!(
        "{passes} alternating passes, 3000 frames/sample; visibility and clipped hit regions including cache reads, no widgets, hints, parsing, GPU or UI rendering."
    );
    for count in [2, 26, 1000] {
        let rectangles: Vec<_> = (0..count)
            .map(|index| {
                let x = (index % 25) as f64 * 24.0;
                let y = (index / 25) as f64 * 20.0;
                PdfRect::new(x, y, x + 20.0, y + 14.0)
            })
            .collect();
        for rotation in [0, 1] {
            let original_view = PageView {
                document: 1,
                page: 0,
                page_rect: Rect::from_min_size(
                    pos2(20.0, 30.0),
                    if rotation == 1 {
                        Vec2::new(1600.0, 1200.0)
                    } else {
                        Vec2::new(1200.0, 1600.0)
                    },
                ),
                clip: Rect::from_min_max(pos2(0.0, 0.0), pos2(700.0, 700.0)),
                dimensions: [600.0, 800.0],
                rotation,
                scale: 2.0,
            };
            let shifted = PageView {
                page_rect: original_view.page_rect.translate(Vec2::new(-100.0, -120.0)),
                ..original_view
            };
            for changing in [false, true] {
                let mut cache = VisibleRects::default();
                let mut indices = Vec::new();
                for view in [original_view, shifted] {
                    assert_eq!(
                        cached(&rectangles, view, &mut cache),
                        original(&rectangles, view, &mut indices)
                    );
                }
                let mut times = [Vec::with_capacity(passes), Vec::with_capacity(passes)];
                for pass in 0..passes {
                    for order in 0..2 {
                        let current = (pass + order) % 2;
                        let start = Instant::now();
                        for frame in 0..3000 {
                            let view = if changing && frame % 2 == 1 {
                                shifted
                            } else {
                                original_view
                            };
                            black_box(if current == 1 {
                                cached(black_box(&rectangles), black_box(view), &mut cache)
                            } else {
                                original(black_box(&rectangles), black_box(view), &mut indices)
                            });
                        }
                        times[current].push(start.elapsed().as_secs_f64() * 1000.0);
                    }
                }
                for samples in &mut times {
                    samples.sort_by(f64::total_cmp);
                }
                println!(
                    "{count} links, rotation {rotation}, {}: original {:.4} ms, cached {:.4} ms / 3000 frames",
                    if changing { "moving" } else { "fixed" },
                    times[0][passes / 2],
                    times[1][passes / 2]
                );
            }
        }
    }
}
