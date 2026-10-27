use eframe::egui::{Pos2, Rect, Vec2};

/// Scroll in screen points while a text drag approaches a viewport edge.
pub(super) fn delta(viewport: Rect, pointer: Pos2, seconds: f32) -> Vec2 {
    if !viewport.is_positive()
        || !viewport.is_finite()
        || !pointer.is_finite()
        || !seconds.is_finite()
        || seconds <= 0.0
    {
        return Vec2::ZERO;
    }
    // Bound the first step after an idle frame, and cap speed outside the window.
    let distance = 720.0 * seconds.min(1.0 / 30.0);
    let axis = |position: f32, min: f32, max: f32| {
        let band = 48.0_f32.min((max - min) * 0.5);
        let before = ((min + band - position) / band).clamp(0.0, 1.0);
        let after = ((position - max + band) / band).clamp(0.0, 1.0);
        (after - before) * distance
    };
    Vec2::new(
        axis(pointer.x, viewport.left(), viewport.right()),
        axis(pointer.y, viewport.top(), viewport.bottom()),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use eframe::egui::{pos2, vec2};

    fn close(actual: Vec2, expected: Vec2) {
        assert!(
            (actual - expected).abs().max_elem() < 0.0001,
            "{actual:?} != {expected:?}"
        );
    }

    #[test]
    fn edge_speed_ramps_on_each_axis_and_remains_bounded_outside_the_window() {
        let rect = Rect::from_min_max(pos2(200.0, 50.0), pos2(1000.0, 750.0));
        let frame = 1.0 / 60.0;
        close(delta(rect, rect.center(), frame), Vec2::ZERO);
        close(delta(rect, pos2(248.0, 702.0), frame), Vec2::ZERO);
        close(delta(rect, pos2(224.0, 726.0), frame), vec2(-6.0, 6.0));
        close(delta(rect, rect.left_top(), frame), vec2(-12.0, -12.0));
        close(delta(rect, rect.right_bottom(), frame), vec2(12.0, 12.0));
        close(delta(rect, pos2(-500.0, 3000.0), frame), vec2(-12.0, 12.0));
        close(
            delta(rect, rect.right_bottom(), frame * 0.5),
            vec2(6.0, 6.0),
        );
        close(delta(rect, rect.right_bottom(), 60.0), vec2(24.0, 24.0));
    }

    #[test]
    fn small_viewports_do_not_overlap_edge_bands_and_invalid_input_does_not_pan() {
        let rect = Rect::from_min_size(pos2(200.0, 50.0), vec2(30.0, 20.0));
        close(delta(rect, rect.center(), 1.0), Vec2::ZERO);
        close(delta(rect, rect.left_top(), 1.0), vec2(-24.0, -24.0));
        for seconds in [0.0, -1.0, f32::NAN, f32::INFINITY] {
            close(delta(rect, rect.left_top(), seconds), Vec2::ZERO);
        }
        for rect in [Rect::NOTHING, Rect::ZERO, Rect::EVERYTHING] {
            close(delta(rect, Pos2::ZERO, 1.0), Vec2::ZERO);
        }
        close(delta(rect, pos2(f32::NAN, 50.0), 1.0), Vec2::ZERO);
    }
}
