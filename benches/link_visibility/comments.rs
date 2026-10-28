use super::*;

type Cache = VisibleRects<(usize, usize), false>;

fn original(comments: &[Vec<PdfRect>], view: PageView) -> (usize, u64) {
    let mut count = 0;
    let mut sum = 0;
    for (index, regions) in comments.iter().enumerate() {
        for (part, &rect) in regions.iter().enumerate() {
            let rect = view.screen_rect(rect).intersect(view.page_rect);
            if rect.is_positive() && view.clip.intersects(rect) {
                count += 1;
                sum += checksum(rect) + index as u64 * 4099 + part as u64 * 31;
            }
        }
    }
    (count, sum)
}

fn cached(comments: &[Vec<PdfRect>], view: PageView, cache: &mut Cache) -> (usize, u64) {
    cache.update_keyed(
        view,
        comments.iter().enumerate().flat_map(|(index, regions)| {
            regions
                .iter()
                .enumerate()
                .map(move |(part, &rect)| ((index, part), rect))
        }),
    );
    let mut count = 0;
    let mut sum = 0;
    for (&(index, part), &rect) in cache.indices.iter().zip(&cache.rects) {
        if rect.is_positive() && view.clip.intersects(rect) {
            count += 1;
            sum += checksum(rect) + index as u64 * 4099 + part as u64 * 31;
        }
    }
    (count, sum)
}

pub(super) fn run(passes: usize) {
    const FRAMES: usize = 200;
    println!(
        "{passes} alternating passes, {FRAMES} frames/sample; warm comment visibility cache and visible-region reads. Includes transform/clipping/identity checksums, excludes PDF loading, widgets, paint, GPU and allocation of fixtures."
    );
    for count in [1, 80, 10_000] {
        for parts in [1, 3] {
            let comments: Vec<Vec<_>> = (0..count)
                .map(|index| {
                    let x = (index * 17 % 600) as f64;
                    let y = (index * 29 % 800) as f64;
                    (0..parts)
                        .map(|part| {
                            let y = y + part as f64 * 7.0;
                            PdfRect::new(x, y, x + 20.0, y + 5.0)
                        })
                        .collect()
                })
                .collect();
            for rotation in [0, 1] {
                let initial = PageView {
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
                    page_rect: initial.page_rect.translate(Vec2::new(-100.0, -120.0)),
                    ..initial
                };
                for period in [0, 1, 10] {
                    let mut cache = Cache::default();
                    for view in [initial, shifted, initial] {
                        assert_eq!(
                            original(&comments, view),
                            cached(&comments, view, &mut cache)
                        );
                        assert_eq!(
                            original(&comments, view),
                            cached(&comments, view, &mut cache)
                        );
                    }
                    let mut times = [Vec::with_capacity(passes), Vec::with_capacity(passes)];
                    for pass in 0..passes {
                        for order in 0..2 {
                            let path = (pass + order) % 2;
                            let start = Instant::now();
                            for frame in 0..FRAMES {
                                let view = if period > 0 && (frame / period) % 2 == 1 {
                                    shifted
                                } else {
                                    initial
                                };
                                black_box(if path == 0 {
                                    original(black_box(&comments), black_box(view))
                                } else {
                                    cached(black_box(&comments), black_box(view), &mut cache)
                                });
                            }
                            times[path]
                                .push(start.elapsed().as_secs_f64() * 1_000_000.0 / FRAMES as f64);
                        }
                    }
                    for samples in &mut times {
                        samples.sort_by(f64::total_cmp);
                    }
                    println!(
                        "{count} comments x {parts} regions, rotation {rotation}, {}: original {:.4} us/frame, cached {:.4} us/frame",
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
