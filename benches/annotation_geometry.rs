#[path = "support/allocations.rs"]
mod allocation_counter;
#[allow(dead_code, unused_imports)]
#[path = "../src/annotations/geometry.rs"]
mod geometry;

use hayro::{
    hayro_interpret::util::TransformExt,
    hayro_syntax::{
        Pdf,
        object::{Array, Dict},
    },
    kurbo::{Affine, Point, Rect},
};
use lopdf::{Object, dictionary};
use std::{hint::black_box, time::Instant};

fn previous(numbers: impl Iterator<Item = f32>, transform: Affine) -> Vec<Rect> {
    let numbers: Vec<_> = numbers.take(8192).collect();
    let mut rects = Vec::new();
    for quad in numbers.as_chunks::<8>().0 {
        let points: Vec<_> = quad
            .as_chunks::<2>()
            .0
            .iter()
            .map(|p| transform * Point::new(p[0] as f64, p[1] as f64))
            .collect();
        let rect = points.iter().fold(
            Rect::new(
                f64::INFINITY,
                f64::INFINITY,
                f64::NEG_INFINITY,
                f64::NEG_INFINITY,
            ),
            |r, p| Rect::new(r.x0.min(p.x), r.y0.min(p.y), r.x1.max(p.x), r.y1.max(p.y)),
        );
        if rect.is_finite() && rect.width() > 0.0 && rect.height() > 0.0 {
            rects.push(rect);
        }
    }
    rects
}

fn fixture(regions: usize, rotation: i64) -> Pdf {
    let mut document =
        lopdf::Document::load_mem(include_bytes!("../assets/pagewing-guide.pdf")).unwrap();
    let id = document.get_pages()[&1];
    let numbers: Vec<Object> = (0..regions)
        .flat_map(|index| {
            let y = 700 - (index % 50) as i64 * 10;
            [50, y, 450, y, 50, y + 8, 450, y + 8].map(Object::Integer)
        })
        .collect();
    let annotation = dictionary! { "Subtype" => "Highlight", "Rect" => vec![0.into(), 0.into(), 600.into(), 800.into()], "QuadPoints" => numbers };
    let page = document.get_dictionary_mut(id).unwrap();
    page.set(
        "CropBox",
        vec![20.into(), 30.into(), 550.into(), 730.into()],
    );
    page.set("Rotate", rotation);
    page.set("Annots", vec![Object::Dictionary(annotation)]);
    let mut bytes = Vec::new();
    document.save_to(&mut bytes).unwrap();
    Pdf::new(bytes).unwrap()
}

fn main() {
    let passes: usize = std::env::args()
        .nth(1)
        .and_then(|value| value.parse().ok())
        .unwrap_or(9);
    assert!((1..=1000).contains(&passes));
    println!(
        "{passes} alternating passes, 200 conversions/sample; µs/conversion. Typed QuadPoints array reading, transforms, bounds, validation, output allocation and release; loaded cropped PDF fixtures. Excludes PDF loading, annotation traversal, text, widgets, rendering, IPC, filesystem and GPU."
    );
    for count in [0, 1, 2, 80, 1024, 1100] {
        for rotation in [0, 90] {
            let pdf = fixture(count, rotation);
            let page = &pdf.pages()[0];
            let annotations = page.raw().get::<Array<'_>>(b"Annots").unwrap();
            let annotation = annotations.iter::<Dict<'_>>().next().unwrap();
            let quads = annotation.get::<Array<'_>>(b"QuadPoints").unwrap();
            let transform = page.initial_transform(true).to_kurbo();
            let before = previous(quads.iter::<f32>(), transform);
            let after = geometry::quad_rects(quads.iter::<f32>(), transform);
            assert_eq!(before, after);
            assert_eq!(after.len(), count.min(1024));
            let old_allocations = allocation_counter::allocations(|| {
                black_box(previous(quads.iter::<f32>(), transform));
            });
            let new_allocations = allocation_counter::allocations(|| {
                black_box(geometry::quad_rects(quads.iter::<f32>(), transform));
            });
            let mut times = [Vec::with_capacity(passes), Vec::with_capacity(passes)];
            for pass in 0..passes {
                for order in 0..2 {
                    let mode = (pass + order) % 2;
                    let start = Instant::now();
                    for _ in 0..200 {
                        let numbers = black_box(&quads).iter::<f32>();
                        black_box(if mode == 0 {
                            previous(numbers, black_box(transform))
                        } else {
                            geometry::quad_rects(numbers, black_box(transform))
                        });
                    }
                    times[mode].push(start.elapsed().as_secs_f64() * 1_000_000.0 / 200.0);
                }
            }
            let [old, new] = times.map(|mut values| {
                values.sort_by(f64::total_cmp);
                values[values.len() / 2]
            });
            assert_eq!(geometry::quad_rects(quads.iter::<f32>(), transform), before);
            println!(
                "{count} regions, rotation {rotation}: {old:.4} -> {new:.4} µs; (alloc,realloc) {old_allocations:?} -> {new_allocations:?}"
            );
        }
    }
}
