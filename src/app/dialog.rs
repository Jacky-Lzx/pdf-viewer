//! Shared window constraints for reader dialogs. Contents budget their scrolling
//! separately so primary actions remain visible outside the scroll area.
use eframe::egui::{self, Vec2};

pub(super) fn window(
    context: &egui::Context,
    title: &'static str,
    compact_title: &'static str,
    preferred_width: f32,
) -> egui::Window<'static> {
    let size = context.content_rect().size();
    let narrow = size.x < 480.0;
    let mut frame = egui::Frame::window(&context.style());
    if narrow || size.y < 420.0 {
        frame.inner_margin = 10.into();
    }
    // Centering uses the previous window size on the first resize frame. Keep
    // that old width from placing short selectable fields left of the screen;
    // the measured new width restores normal centering on the next pass.
    let previous_width = context
        .memory(|memory| memory.area_rect(egui::Id::new(title)))
        .map_or(0.0, |rect| rect.width());
    let offset = ((previous_width - size.x) * 0.5).max(0.0);
    egui::Window::new(if narrow { compact_title } else { title })
        // A shorter title must retain focus, scroll and window state.
        .id(egui::Id::new(title))
        .frame(frame)
        .collapsible(false)
        // Refresh the layout budget on every resize rather than retaining
        // egui's previous narrow width or gradually growing its scroll area.
        .fixed_size(Vec2::new(
            (size.x - 64.0).clamp(100.0, preferred_width),
            (size.y - 100.0).max(100.0),
        ))
        .anchor(egui::Align2::CENTER_CENTER, Vec2::new(offset, 0.0))
}

#[cfg(test)]
pub(super) fn assert_visible_actions(
    context: &egui::Context,
    output: &egui::FullOutput,
    screen: egui::Rect,
    title: &str,
    actions: &[&str],
) {
    let rect = context
        .memory(|memory| memory.area_rect(egui::Id::new(title)))
        .unwrap();
    assert!(
        screen.contains_rect(rect),
        "{title}: {rect:?} outside {screen:?}"
    );
    for &label in actions {
        let (clip, text) = output
            .shapes
            .iter()
            .find_map(|shape| match &shape.shape {
                egui::Shape::Text(text) if text.galley.text() == label => {
                    Some((shape.clip_rect, text))
                }
                _ => None,
            })
            .unwrap_or_else(|| panic!("missing {label} at {screen:?}"));
        let bounds = egui::Rect::from_min_size(text.pos, text.galley.size());
        assert!(
            screen.contains_rect(bounds),
            "{label}: {bounds:?} outside {screen:?}"
        );
        assert!(
            clip.contains_rect(bounds),
            "{label}: {bounds:?} clipped by {clip:?}"
        );
    }
}

#[cfg(test)]
pub(super) const TEST_SIZES: [Vec2; 10] = [
    Vec2::new(1180.0, 820.0),
    Vec2::new(240.0, 240.0),
    Vec2::new(240.0, 400.0),
    Vec2::new(240.0, 420.0),
    Vec2::new(240.0, 820.0),
    Vec2::new(360.0, 540.0),
    Vec2::new(480.0, 420.0),
    Vec2::new(1180.0, 240.0),
    Vec2::new(1180.0, 420.0),
    Vec2::new(1180.0, 820.0),
];
