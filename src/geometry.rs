use hayro::kurbo::{Point, Rect};

pub fn rotate_rect(rect: Rect, dimensions: [f32; 2], rotation: u8) -> Rect {
    let rotate = |x: f64, y: f64| {
        let [w, h] = dimensions.map(f64::from);
        match rotation % 4 {
            1 => Point::new(h - y, x),
            2 => Point::new(w - x, h - y),
            3 => Point::new(y, w - x),
            _ => Point::new(x, y),
        }
    };
    Rect::from_points(rotate(rect.x0, rect.y0), rotate(rect.x1, rect.y1))
}

/// Put a search match near the upper left third of the viewport, clamped to the page.
pub fn focus_offset(
    rect: Rect,
    dimensions: [f32; 2],
    rotation: u8,
    scale: f32,
    viewport: [f32; 2],
    overflow: [f32; 2],
) -> [f32; 2] {
    let rect = rotate_rect(rect, dimensions, rotation);
    let origin = [rect.x0 as f32, rect.y0 as f32];
    std::array::from_fn(|axis| {
        (origin[axis] * scale - viewport[axis] * 0.3).clamp(0.0, overflow[axis])
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn search_focus_keeps_rotated_matches_visible_at_high_zoom() {
        let dimensions = [600.0, 800.0];
        let viewport = [500.0, 400.0];
        let scale = 2.0;
        for rotation in 0..4 {
            let size = if rotation % 2 == 1 {
                [800.0, 600.0]
            } else {
                dimensions
            };
            let overflow = [size[0] * scale - viewport[0], size[1] * scale - viewport[1]];
            for rect in [
                Rect::new(0.0, 0.0, 60.0, 12.0),
                Rect::new(250.0, 380.0, 310.0, 392.0),
                Rect::new(540.0, 788.0, 600.0, 800.0),
            ] {
                let offset = focus_offset(rect, dimensions, rotation, scale, viewport, overflow);
                let rotated = rotate_rect(rect, dimensions, rotation);
                assert!(rotated.x0 * f64::from(scale) >= f64::from(offset[0]) - 0.01);
                assert!(rotated.y0 * f64::from(scale) >= f64::from(offset[1]) - 0.01);
                assert!(rotated.x1 * f64::from(scale) <= f64::from(offset[0] + viewport[0]) + 0.01);
                assert!(rotated.y1 * f64::from(scale) <= f64::from(offset[1] + viewport[1]) + 0.01);
            }
        }
    }

    #[test]
    fn rotations_roundtrip_rectangles_and_preserve_bounds() {
        let rect = Rect::new(25.0, 70.0, 125.0, 90.0);
        let rotated = rotate_rect(rect, [600.0, 800.0], 1);
        assert_eq!(rotated, Rect::new(710.0, 25.0, 730.0, 125.0));
        assert_eq!(rotate_rect(rotated, [800.0, 600.0], 3), rect);
    }
}
