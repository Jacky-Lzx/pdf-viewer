#[path = "support/allocations.rs"]
mod allocation_counter;
#[allow(dead_code, unused_imports)]
#[path = "../src/bitmap.rs"]
mod bitmap;
#[allow(dead_code, unused_imports)]
#[path = "../src/continuous.rs"]
mod continuous;
#[allow(dead_code, unused_imports)]
#[path = "../src/document.rs"]
mod document;
#[allow(dead_code, unused_imports)]
#[path = "../src/document_bytes.rs"]
mod document_bytes;
#[allow(dead_code, unused_imports)]
#[path = "../src/navigation.rs"]
mod navigation;
#[allow(dead_code, unused_imports)]
#[path = "../src/page_geometry.rs"]
mod page_geometry;
#[allow(dead_code, unused_imports)]
#[path = "support/continuous_previous.rs"]
mod previous;
#[allow(dead_code, unused_imports)]
#[path = "../src/render_plan.rs"]
mod render_plan;

use navigation::Fit;
use render_plan::RenderView;
use std::{hint::black_box, time::Instant};

fn verify(before: &previous::Layout, after: &continuous::Layout) {
    assert_eq!(before.pages.len(), after.pages.len());
    for (before, after) in before.pages.iter().zip(&after.pages) {
        assert_eq!(before.top, after.top);
        assert_eq!(before.size, after.size);
        assert_eq!(before.scale, after.scale);
    }
    assert_eq!(before.maximum(), after.maximum());
    for page in [0, before.pages.len() / 2, before.pages.len() - 1] {
        assert_eq!(before.limit(page), after.limit(page));
        for offset in [0.0, 60.0, before.limit(page)] {
            assert_eq!(
                before.visible(page, offset, 24.0),
                after.visible(page, offset, 24.0)
            );
            for delta in [-f32::MAX, -70.0, 0.0, 120.0, f32::MAX] {
                assert_eq!(
                    before.position(page, offset, delta),
                    after.position(page, offset, delta)
                );
            }
        }
    }
}

fn main() {
    let passes: usize = std::env::args()
        .nth(1)
        .and_then(|arg| arg.parse().ok())
        .unwrap_or(17);
    assert!((1..=1000).contains(&passes));
    println!(
        "{passes} alternating passes; median µs/update. Exact 18e458f continuous layout vs geometry-dependent invalidation. Warm buffers, mixed page sizes; includes signature checks, height updates, any rebuild and sparse result reads. No PDF parsing, rendering, UI or GPU. Allocation counts cover a warm pair of updates."
    );
    for count in [80, 10_000, 100_000] {
        let dimensions: Vec<_> = (0..count)
            .map(|index| match index % 3 {
                0 => [600.0, 800.0],
                1 => [900.0, 400.0],
                _ => [800.0, 1200.0],
            })
            .collect();
        let base = RenderView {
            fit: Fit::Custom(2.0),
            viewport: [1000.0, 700.0],
            pixels_per_point: 1.0,
            rotation: 0,
            night: false,
            backwards: false,
        };
        for (name, fit, viewport, rotation) in [
            ("unchanged custom", Fit::Custom(2.0), base.viewport, 0),
            ("custom resize", Fit::Custom(2.0), [1200.0, 900.0], 0),
            ("width: height resize", Fit::Width, [1000.0, 900.0], 0),
            ("height: width resize", Fit::Height, [1200.0, 700.0], 0),
            ("page: height resize", Fit::Page, [1000.0, 900.0], 0),
            ("width: width resize", Fit::Width, [1200.0, 700.0], 0),
            (
                "below-min custom resize",
                Fit::Custom(0.01),
                [1200.0, 900.0],
                0,
            ),
            ("half-turn", Fit::Custom(2.0), base.viewport, 2),
            ("quarter-turn", Fit::Custom(2.0), base.viewport, 1),
        ] {
            let views = [
                RenderView { fit, ..base },
                RenderView {
                    fit,
                    viewport,
                    rotation,
                    ..base
                },
            ];
            let mut before = previous::Layout::default();
            let mut after = continuous::Layout::default();
            for index in 0..6 {
                before.update(&dimensions, views[index % 2]);
                after.update(&dimensions, views[index % 2]);
                verify(&before, &after);
            }
            let iterations = (2_000_000 / count).clamp(20, 2000);
            let iterations = iterations / 2 * 2;
            let mut samples: [Vec<f64>; 2] = std::array::from_fn(|_| Vec::with_capacity(passes));
            for pass in 0..passes {
                for order in 0..2 {
                    let mode = (pass + order) % 2;
                    let start = Instant::now();
                    for index in 0..iterations {
                        let view = black_box(views[index % 2]);
                        if mode == 0 {
                            black_box(&mut before).update(black_box(&dimensions), view);
                            black_box(before.pages.last());
                        } else {
                            black_box(&mut after).update(black_box(&dimensions), view);
                            black_box(after.pages.last());
                        }
                    }
                    samples[mode].push(start.elapsed().as_secs_f64() * 1e6 / iterations as f64);
                }
            }
            verify(&before, &after);
            let before_allocs = allocation_counter::allocations(|| {
                for view in views {
                    before.update(&dimensions, view);
                }
            });
            let after_allocs = allocation_counter::allocations(|| {
                for view in views {
                    after.update(&dimensions, view);
                }
            });
            assert_eq!(before_allocs, (0, 0));
            assert_eq!(after_allocs, (0, 0));
            let medians = samples.map(|mut samples| {
                samples.sort_by(f64::total_cmp);
                samples[passes / 2]
            });
            println!(
                "{count} pages, {name}: old {:.4}, current {:.4} µs/update; {iterations} updates/sample; allocations {before_allocs:?} -> {after_allocs:?}",
                medians[0], medians[1]
            );
        }
    }
}
