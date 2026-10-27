#[allow(dead_code, unused_imports)]
#[path = "../src/bitmap.rs"]
mod bitmap;
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
#[path = "../src/render_plan.rs"]
mod render_plan;

use eframe::egui::{Rect, Vec2, pos2};
use navigation::Fit;
use render_plan::{PlanMode, RenderPlanner, RenderView};
use std::{hint::black_box, time::Instant};

fn measure_pair(passes: usize, mut raw: impl FnMut(), mut cached: impl FnMut()) -> [f64; 2] {
    let mut samples = [Vec::new(), Vec::new()];
    for pass in 0..passes {
        for index in if pass % 2 == 0 { [0, 1] } else { [1, 0] } {
            let start = Instant::now();
            for _ in 0..10_000 {
                if index == 0 {
                    raw();
                } else {
                    cached();
                }
            }
            samples[index].push(start.elapsed().as_secs_f64() * 100.0);
        }
    }
    samples.map(|mut values| {
        values.sort_by(f64::total_cmp);
        values[passes / 2]
    })
}

fn main() {
    let passes = std::env::args()
        .nth(1)
        .and_then(|arg| arg.parse().ok())
        .unwrap_or(9);
    assert!((1..=1000).contains(&passes));
    println!(
        "{passes} passes, median µs/plan, 10000 operations/sample; no PDF parsing, rasterization, UI or GPU"
    );
    let dimensions = vec![[600.0, 800.0]; 100_000];
    for (name, fit, viewport, density, count, budget) in [
        ("single fit", Fit::Page, [1000.0, 700.0], 2.0, 1, 96 << 20),
        (
            "single zoom",
            Fit::Custom(8.0),
            [1000.0, 700.0],
            2.0,
            1,
            96 << 20,
        ),
        (
            "HiDPI zoom",
            Fit::Custom(8.0),
            [7680.0, 4320.0],
            3.0,
            1,
            96 << 20,
        ),
        (
            "HiDPI tight budget",
            Fit::Custom(8.0),
            [7680.0, 4320.0],
            3.0,
            1,
            8 << 20,
        ),
        (
            "two-page HiDPI",
            Fit::Custom(8.0),
            [7680.0, 9000.0],
            4.0,
            2,
            16 << 20,
        ),
        (
            "dense large viewport",
            Fit::Custom(8.0),
            [30000.0, 30000.0],
            8.0,
            1,
            96 << 20,
        ),
        (
            "continuous overview",
            Fit::Custom(0.05),
            [1000.0, 700.0],
            2.0,
            16,
            96 << 20,
        ),
    ] {
        let view = RenderView {
            fit,
            viewport,
            pixels_per_point: density,
            rotation: 0,
            night: false,
            backwards: false,
        };
        let size = Vec2::new(600.0, 800.0) * view.scale(dimensions[0]);
        let pages: Vec<_> = (50..50 + count)
            .map(|page| {
                (
                    page,
                    Rect::from_min_size(pos2(-10.0, (page - 50) as f32 * size.y - 20.0), size),
                )
            })
            .collect();
        let clip = Rect::from_min_size(pos2(0.0, 0.0), Vec2::from(viewport));
        let build = |pages: &[(usize, Rect)]| {
            let detail = black_box(view).detail_plan(
                black_box(pages),
                black_box(&dimensions),
                black_box(clip),
                black_box(budget),
            );
            detail.unwrap_or_else(|| {
                if count == 1 {
                    view.plan(50, &dimensions, budget)
                } else {
                    view.continuous_plan(50..50 + count, &dimensions, budget)
                }
            })
        };
        let mode = if count == 1 {
            PlanMode::Single
        } else {
            PlanMode::Continuous
        };
        let expected = build(&pages);
        let checksum: u64 = expected.iter().fold(0, |hash, key| {
            let tile = key.tile;
            [
                key.page as u64,
                key.width as u64,
                tile.map_or(0, |t| t.scale_bits as u64),
                tile.map_or(0, |t| t.x as u64),
                tile.map_or(0, |t| t.y as u64),
                tile.map_or(0, |t| t.bytes() as u64),
            ]
            .into_iter()
            .fold(hash, |hash, value| {
                hash.wrapping_mul(31).wrapping_add(value)
            })
        });
        let mut planner = RenderPlanner::default();
        let result = planner.plan(mode, view, &pages, &dimensions, clip, budget);
        assert_eq!(*result.plan, expected);
        let [uncached, hit] = measure_pair(
            passes,
            || {
                black_box(build(&pages));
            },
            || {
                black_box(planner.plan(
                    black_box(mode),
                    black_box(view),
                    black_box(&pages),
                    black_box(&dimensions),
                    black_box(clip),
                    black_box(budget),
                ));
            },
        );
        let shifted: Vec<_> = pages
            .iter()
            .map(|&(page, rect)| (page, rect.translate(Vec2::new(0.0, -37.0))))
            .collect();
        let geometries = [&pages, &shifted];
        for pages in geometries {
            let result = planner.plan(mode, view, pages, &dimensions, clip, budget);
            assert_eq!(*result.plan, build(pages));
        }
        let mut raw_index = 0;
        // Verification leaves the planner on the shifted geometry.
        let mut cached_index = 1;
        let [moving_raw, miss] = measure_pair(
            passes,
            || {
                raw_index ^= 1;
                black_box(build(geometries[raw_index]));
            },
            || {
                cached_index ^= 1;
                black_box(planner.plan(
                    black_box(mode),
                    black_box(view),
                    black_box(geometries[cached_index]),
                    black_box(&dimensions),
                    black_box(clip),
                    black_box(budget),
                ));
            },
        );
        println!(
            "{name}: {} keys, checksum {checksum:016x}; stable rebuild {uncached:.4}, hit {hit:.4}; moving rebuild {moving_raw:.4}, miss {miss:.4}",
            expected.len()
        );
    }
}
