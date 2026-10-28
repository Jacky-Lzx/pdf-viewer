use hayro::kurbo::{Affine, Point, Rect};

const MAX_COORDINATES: usize = 8192;

/// PDF QuadPoints contain four points per region. Keep only one complete
/// quadrilateral on the stack; incomplete trailing coordinates are ignored.
pub fn quad_rects(numbers: impl Iterator<Item = f32>, transform: Affine) -> Vec<Rect> {
    let mut rects = Vec::new();
    let mut quad = [0.0; 8];
    let mut used = 0;
    for number in numbers.take(MAX_COORDINATES) {
        quad[used] = number;
        used += 1;
        if used != quad.len() {
            continue;
        }
        used = 0;
        let rect = quad
            .as_chunks::<2>()
            .0
            .iter()
            .map(|p| transform * Point::new(f64::from(p[0]), f64::from(p[1])))
            .fold(
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

#[cfg(test)]
mod tests {
    use super::*;

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

    #[test]
    fn streaming_matches_the_previous_bounds_for_transformed_and_incomplete_quads() {
        let mut seed = 31_u64;
        let numbers: Vec<_> = (0..8300)
            .map(|_| {
                seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1);
                ((seed >> 32) as i32 % 10000) as f32 / 8.0
            })
            .collect();
        for transform in [
            Affine::IDENTITY,
            Affine::translate((-20.0, 730.0)) * Affine::scale_non_uniform(1.0, -1.0),
            Affine::rotate(std::f64::consts::FRAC_PI_2),
            Affine::new([1.25, 0.3, -0.2, 0.75, 15.0, -30.0]),
        ] {
            for count in [
                0, 1, 7, 8, 9, 15, 16, 24, 639, 640, 641, 8191, 8192, 8193, 8300,
            ] {
                assert_eq!(
                    quad_rects(numbers[..count].iter().copied(), transform),
                    previous(numbers[..count].iter().copied(), transform),
                    "{count}, {transform:?}"
                );
            }
        }
    }

    #[test]
    fn invalid_and_degenerate_coordinates_keep_the_previous_filtering_rules() {
        for transform in [Affine::IDENTITY, Affine::rotate(0.7)] {
            for special in [
                0.0,
                f32::INFINITY,
                f32::NEG_INFINITY,
                f32::NAN,
                f32::MAX,
                f32::MIN,
            ] {
                for index in 0..8 {
                    let mut numbers = [1.0, 2.0, 6.0, 2.0, 1.0, 9.0, 6.0, 9.0];
                    numbers[index] = special;
                    assert_eq!(
                        quad_rects(numbers.into_iter(), transform),
                        previous(numbers.into_iter(), transform)
                    );
                }
            }
            assert_eq!(quad_rects([0.0; 16].into_iter(), transform), Vec::new());
        }
    }

    #[test]
    fn coordinate_limit_stops_consuming_the_source_even_when_every_quad_is_invalid() {
        let reads = std::cell::Cell::new(0);
        let numbers = std::iter::repeat_with(|| {
            reads.set(reads.get() + 1);
            0.0
        });
        assert!(quad_rects(numbers, Affine::IDENTITY).is_empty());
        assert_eq!(reads.get(), 8192);
    }
}
