//! The current and preceding comment-overlay buffer policies, without widgets.
use super::*;

type Cache = VisibleRects<(usize, usize), false>;

fn regions<'a, const REUSE: bool>(
    pages: &[Vec<Vec<PdfRect>>],
    view: PageView,
    buffers: &'a mut [Cache; 2],
) -> impl Iterator<Item = ((usize, usize), Rect)> + 'a {
    let neighbor = view.page != 1;
    let cache = &mut buffers[usize::from(neighbor)];
    // Baseline: comment_overlays at 8cf83e1 invalidates every neighbor draw.
    if neighbor && !REUSE {
        cache.invalidate();
    }
    let comments = &pages[view.page];
    cache.update_keyed(
        view,
        comments.iter().enumerate().flat_map(|(index, rects)| {
            rects
                .iter()
                .enumerate()
                .map(move |(part, &rect)| ((view.page * comments.len() + index, part), rect))
        }),
    );
    cache
        .indices
        .iter()
        .copied()
        .zip(cache.rects.iter().copied())
        .filter(move |&(_, rect)| rect.is_positive() && view.clip.intersects(rect))
}

fn summarize(regions: impl Iterator<Item = ((usize, usize), Rect)>) -> (usize, u64) {
    regions.fold((0, 0), |(count, sum), ((index, part), rect)| {
        (
            count + 1,
            sum + checksum(rect) + index as u64 * 4099 + part as u64 * 31,
        )
    })
}

pub(super) fn run(passes: usize) {
    const FRAMES: usize = 200;
    println!(
        "{passes} alternating passes, {FRAMES} frames/sample. Baseline: comment_overlays buffer policy at 8cf83e1. Both paths call production VisibleRects::update_keyed with prepared per-page regions. Includes buffer selection, transforms/clipping and visible identity/coordinate checksums; excludes fixture setup, PDF parsing, metadata lookup, widgets, paint and GPU. Counts are per page, three regions per comment."
    );
    for count in [0, 1, 80, 3000] {
        let comments: Vec<Vec<Vec<_>>> = (0..3)
            .map(|page| {
                (0..count)
                    .map(|index| {
                        let x = ((index * 17 + page * 13) % 600) as f64;
                        let y = ((index * 29 + page * 11) % 800) as f64;
                        (0..3)
                            .map(|part| {
                                let y = y + part as f64 * 7.0;
                                PdfRect::new(x, y, x + 20.0, y + 5.0)
                            })
                            .collect()
                    })
                    .collect()
            })
            .collect();
        for rotation in [0, 1] {
            for pages in [&[1, 2][..], &[0, 1, 2][..]] {
                for period in [0, 1, 10] {
                    let view_at = |page: usize, frame: usize| {
                        let shifted = period > 0 && (frame / period) % 2 == 1;
                        PageView {
                            document: 1,
                            page,
                            page_rect: Rect::from_min_size(
                                pos2(20.0 + page as f32 * 410.0, 30.0)
                                    - if shifted {
                                        Vec2::new(50.0, 60.0)
                                    } else {
                                        Vec2::ZERO
                                    },
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
                        }
                    };
                    let mut baseline = Default::default();
                    let mut current = Default::default();
                    for frame in 0..32 {
                        for &page in pages {
                            let view = view_at(page, frame);
                            let expected: Vec<_> = comments[page]
                                .iter()
                                .enumerate()
                                .flat_map(|(index, rects)| {
                                    rects.iter().enumerate().filter_map(move |(part, &rect)| {
                                        let rect = view.screen_rect(rect).intersect(view.page_rect);
                                        (rect.is_positive() && view.clip.intersects(rect))
                                            .then_some(((page * count + index, part), rect))
                                    })
                                })
                                .collect();
                            assert_eq!(
                                regions::<false>(&comments, view, &mut baseline)
                                    .collect::<Vec<_>>(),
                                expected
                            );
                            assert_eq!(
                                regions::<true>(&comments, view, &mut current).collect::<Vec<_>>(),
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
                                for &page in pages {
                                    let view = black_box(view_at(page, frame));
                                    black_box(if path == 0 {
                                        summarize(regions::<false>(
                                            black_box(&comments),
                                            view,
                                            &mut baseline,
                                        ))
                                    } else {
                                        summarize(regions::<true>(
                                            black_box(&comments),
                                            view,
                                            &mut current,
                                        ))
                                    });
                                }
                            }
                            times[path]
                                .push(start.elapsed().as_secs_f64() * 1_000_000.0 / FRAMES as f64);
                        }
                    }
                    for samples in &mut times {
                        samples.sort_by(f64::total_cmp);
                    }
                    println!(
                        "{count} comments/page x 3 regions, {} pages, rotation {rotation}, {}: baseline {:.4} us/frame, reuse {:.4} us/frame",
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
