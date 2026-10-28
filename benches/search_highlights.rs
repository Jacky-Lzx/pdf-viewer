#[allow(dead_code, unused_imports)]
#[path = "../src/geometry.rs"]
mod geometry;
#[allow(dead_code, unused_imports)]
#[path = "../src/search_highlights.rs"]
mod search_highlights;
#[allow(dead_code, unused_imports)]
#[path = "../src/selection_index.rs"]
mod selection_index;
#[allow(dead_code, unused_imports)]
#[path = "../src/text.rs"]
mod text;
#[allow(dead_code, unused_imports)]
#[path = "../src/visible_rects.rs"]
mod visible_rects;

#[path = "search_highlights/neighbors.rs"]
mod neighbors;

use eframe::egui::{Rect, Vec2, pos2};
use hayro::kurbo::Rect as PdfRect;
use search_highlights::SearchHighlights;
use std::{hint::black_box, time::Instant};
use text::SearchHit;
use visible_rects::PageView;

fn checksum(index: usize, rect: Rect, selected: usize) -> u64 {
    [rect.min.x, rect.min.y, rect.max.x, rect.max.y]
        .into_iter()
        .map(|value| u64::from(value.to_bits()))
        .sum::<u64>()
        + index as u64 * 4099
        + u64::from(index == selected) * 7919
}

fn original(hits: &[SearchHit], view: PageView, selected: usize) -> (usize, u64) {
    let mut count = 0;
    let mut sum = 0;
    for index in text::page_hits(hits, view.page) {
        for &rect in &hits[index].rects {
            let rect = view.screen_rect(rect).intersect(view.page_rect);
            if rect.is_positive() && view.clip.intersects(rect) {
                count += 1;
                sum += checksum(index, rect, selected);
            }
        }
    }
    (count, sum)
}

fn cached(
    hits: &[SearchHit],
    view: PageView,
    selected: usize,
    cache: &mut SearchHighlights,
) -> (usize, u64) {
    let mut count = 0;
    let mut sum = 0;
    for (index, rect) in cache.visible(hits, view, 1) {
        count += 1;
        sum += checksum(index, rect, selected);
    }
    (count, sum)
}

fn main() {
    let passes: usize = std::env::args()
        .nth(1)
        .and_then(|value| value.parse().ok())
        .unwrap_or(9);
    assert!((1..=1000).contains(&passes));
    if std::env::args().nth(2).as_deref() == Some("neighbors") {
        neighbors::run(passes);
        return;
    }
    const FRAMES: usize = 200;
    println!(
        "{passes} alternating passes, {FRAMES} frames/sample; three ordered pages, counts below are per page. Includes page lookup, transform/clipping and visible identities/coordinates/selected-hit checksums. Excludes fixture allocation, PDF/text search, batch acceptance, shape construction, UI and GPU. Cached cases use the production SearchHighlights component."
    );
    for count in [1, 80, 3_000] {
        for parts in [1, 3] {
            let hits: Vec<_> = (0..count * 3)
                .map(|index| {
                    let local = index % count;
                    let x = (local * 17 % 600) as f64;
                    let y = (local * 29 % 800) as f64;
                    SearchHit {
                        page: index / count,
                        rects: (0..parts)
                            .map(|part| {
                                let y = y + part as f64 * 7.0;
                                PdfRect::new(x, y, x + 20.0, y + 5.0)
                            })
                            .collect(),
                        snippet: String::new(),
                        snippet_match: 0..0,
                    }
                })
                .collect();
            for rotation in [0, 1] {
                let initial = PageView {
                    document: 1,
                    page: 1,
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
                    page_rect: initial.page_rect.translate(Vec2::new(-100.0, -120.0)),
                    ..initial
                };
                for period in [0, 1, 10] {
                    let mut cache = SearchHighlights::default();
                    let view_at = |frame| {
                        if period > 0 && (frame / period) % 2 == 1 {
                            shifted
                        } else {
                            initial
                        }
                    };
                    for frame in 0..32 {
                        let view = view_at(frame);
                        let selected = count + frame % count;
                        assert_eq!(
                            original(&hits, view, selected),
                            cached(&hits, view, selected, &mut cache)
                        );
                    }
                    let mut times = [Vec::with_capacity(passes), Vec::with_capacity(passes)];
                    for pass in 0..passes {
                        for order in 0..2 {
                            let path = (pass + order) % 2;
                            let start = Instant::now();
                            for frame in 0..FRAMES {
                                let view = black_box(view_at(frame));
                                let selected = black_box(count + frame % count);
                                black_box(if path == 0 {
                                    original(black_box(&hits), view, selected)
                                } else {
                                    cached(black_box(&hits), view, selected, &mut cache)
                                });
                            }
                            times[path]
                                .push(start.elapsed().as_secs_f64() * 1_000_000.0 / FRAMES as f64);
                        }
                    }
                    for values in &mut times {
                        values.sort_by(f64::total_cmp);
                    }
                    println!(
                        "{count} hits/page x {parts} regions, rotation {rotation}, {}: original {:.4} us/frame, cached {:.4} us/frame",
                        match period {
                            0 => "fixed",
                            1 => "moving",
                            _ => "10% changes",
                        },
                        times[0][passes / 2],
                        times[1][passes / 2]
                    );
                }
            }
        }
    }
}
