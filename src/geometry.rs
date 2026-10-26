use hayro::kurbo::{Point, Rect};

pub fn unrotate_point(point: Point, dimensions: [f32; 2], rotation: u8) -> Point {
    let [w, h] = dimensions.map(f64::from);
    let Point { x, y } = point;
    match rotation % 4 {
        1 => Point::new(y, h - x),
        2 => Point::new(w - x, h - y),
        3 => Point::new(w - y, x),
        _ => point,
    }
}

pub fn zoom_pan(
    pan: [f32; 2],
    page: [f32; 2],
    viewport: [f32; 2],
    before: f32,
    after: f32,
    pivot: [f32; 2],
) -> [f32; 2] {
    std::array::from_fn(|axis| {
        let old_margin = (viewport[axis] - page[axis] * before).max(0.0) * 0.5;
        let new_margin = (viewport[axis] - page[axis] * after).max(0.0) * 0.5;
        let overflow = (page[axis] * after - viewport[axis]).max(0.0);
        ((pan[axis] + pivot[axis] - old_margin) * (after / before) + new_margin - pivot[axis])
            .clamp(0.0, overflow)
    })
}

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
    let origin = [rect.x0, rect.y0];
    std::array::from_fn(|axis| {
        (origin[axis] * f64::from(scale) - f64::from(viewport[axis]) * 0.3)
            .clamp(0.0, f64::from(overflow[axis])) as f32
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn zooming_from_fit_page_keeps_the_reading_center() {
        let pan = zoom_pan(
            [0.0; 2],
            [600.0, 800.0],
            [1000.0, 700.0],
            0.875,
            1.75,
            [500.0, 350.0],
        );
        assert_eq!(pan, [25.0, 350.0]);
        assert_eq!(
            zoom_pan(
                pan,
                [600.0, 800.0],
                [1000.0, 700.0],
                1.75,
                0.875,
                [500.0, 350.0]
            ),
            [0.0; 2]
        );
    }

    #[test]
    fn zoom_preserves_an_off_center_pointer_and_clamps_at_page_edges() {
        assert_eq!(
            zoom_pan(
                [320.0, 450.0],
                [600.0, 800.0],
                [700.0, 500.0],
                2.0,
                3.0,
                [250.0, 160.0]
            ),
            [605.0, 755.0]
        );
        let pan = zoom_pan(
            [0.0; 2],
            [600.0, 800.0],
            [700.0, 500.0],
            2.0,
            0.5,
            [250.0, 160.0],
        );
        assert_eq!(pan, [0.0; 2]);
    }

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
        let original = Point::new(120.0, 250.0);
        for (rotation, point) in [
            (0, original),
            (1, Point::new(550.0, 120.0)),
            (2, Point::new(480.0, 550.0)),
            (3, Point::new(250.0, 480.0)),
        ] {
            assert_eq!(unrotate_point(point, [600.0, 800.0], rotation), original);
        }
    }
}
