//! Compare neighboring-page reuse with the preceding production component.
use super::*;

#[allow(dead_code, unused_imports)]
#[path = "../support/search_highlights_previous.rs"]
mod previous;

fn summarize(regions: impl Iterator<Item = (usize, Rect)>, selected: usize) -> (usize, u64) {
    regions.fold((0, 0), |(count, sum), (index, rect)| {
        (count + 1, sum + checksum(index, rect, selected))
    })
}

pub fn run(passes: usize) {
    const FRAMES: usize = 200;
    println!(
        "{passes} alternating passes, {FRAMES} frames/sample. Baseline: SearchHighlights at 4670ef1; current: production component. Counts are per page. Includes page lookup, transforms/clipping and visible identity/coordinate/selected-hit checksums. Excludes fixture setup, PDF parsing/search, batch acceptance, shapes, UI and GPU."
    );
    for count in [0, 1, 80, 3_000] {
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
                for pages in [&[1, 2][..], &[0, 1, 2][..]] {
                    let views: Vec<_> = pages
                        .iter()
                        .map(|&page| PageView {
                            document: 1,
                            page,
                            page_rect: Rect::from_min_size(
                                pos2(20.0 + page as f32 * 410.0, 30.0),
                                if rotation == 1 {
                                    Vec2::new(400.0, 300.0)
                                } else {
                                    Vec2::new(300.0, 400.0)
                                },
                            ),
                            clip: Rect::from_min_max(pos2(0.0, 0.0), pos2(1180.0, 380.0)),
                            dimensions: [600.0, 800.0],
                            rotation,
                            scale: 0.5,
                        })
                        .collect();
                    for period in [0, 1, 10] {
                        let view_at = |view: PageView, frame: usize| {
                            if period > 0 && (frame / period) % 2 == 1 {
                                PageView {
                                    page_rect: view.page_rect.translate(Vec2::new(-50.0, -60.0)),
                                    ..view
                                }
                            } else {
                                view
                            }
                        };
                        let mut current = SearchHighlights::default();
                        let mut baseline = previous::SearchHighlights::default();
                        // Compare every identity and rectangle with the original
                        // drawing path, beyond the checksums used in timing.
                        for frame in 0..32 {
                            for &view in &views {
                                let view = view_at(view, frame);
                                let expected: Vec<_> = text::page_hits(&hits, view.page)
                                    .flat_map(|index| {
                                        hits[index].rects.iter().filter_map(move |&rect| {
                                            let rect =
                                                view.screen_rect(rect).intersect(view.page_rect);
                                            (rect.is_positive() && view.clip.intersects(rect))
                                                .then_some((index, rect))
                                        })
                                    })
                                    .collect();
                                assert_eq!(
                                    current.visible(&hits, view, 1).collect::<Vec<_>>(),
                                    expected
                                );
                                assert_eq!(
                                    baseline.visible(&hits, view, 1).collect::<Vec<_>>(),
                                    expected
                                );
                            }
                        }
                        let mut times = [Vec::with_capacity(passes), Vec::with_capacity(passes)];
                        for pass in 0..passes {
                            for order in 0..2 {
                                let path = (pass + order) % 2;
                                let start = Instant::now();
                                for frame in 0..FRAMES {
                                    let selected = black_box(count + frame % count.max(1));
                                    for &view in &views {
                                        let view = black_box(view_at(view, frame));
                                        black_box(if path == 0 {
                                            summarize(
                                                baseline.visible(black_box(&hits), view, 1),
                                                selected,
                                            )
                                        } else {
                                            summarize(
                                                current.visible(black_box(&hits), view, 1),
                                                selected,
                                            )
                                        });
                                    }
                                }
                                times[path].push(
                                    start.elapsed().as_secs_f64() * 1_000_000.0 / FRAMES as f64,
                                );
                            }
                        }
                        for values in &mut times {
                            values.sort_by(f64::total_cmp);
                        }
                        println!(
                            "{count} hits/page x {parts} regions, {} pages, rotation {rotation}, {}: baseline {:.4} us/frame, reuse {:.4} us/frame",
                            pages.len(),
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
}
