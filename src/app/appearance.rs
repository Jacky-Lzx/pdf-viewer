use super::ACCENT;
use eframe::egui::{self, Color32, RichText, Stroke, Vec2};

impl super::Pagewing {
    pub(super) fn adjust_text_embolden(&mut self, increase: bool, count: usize) {
        let delta = self.text_embolden_step * count as f64;
        let value = (f64::from(self.text_embolden) + if increase { delta } else { -delta })
            .clamp(0.0, 1.0) as f32;
        let value = if value < f32::EPSILON {
            0.0
        } else if (1.0 - value).abs() < f32::EPSILON {
            1.0
        } else {
            value
        };
        if value != self.text_embolden {
            self.text_embolden = value;
            self.appearance_revision = self.worker.set_text_embolden(value);
            self.cache.clear();
            self.thumbnails.clear();
            self.requested = None;
        }
        self.notify(format!("PDF text weight: {value:.3}"));
    }
}

pub(super) fn accent(ui: &egui::Ui) -> Color32 {
    if ui.visuals().dark_mode {
        Color32::from_rgb(133, 205, 174)
    } else {
        ACCENT
    }
}

pub(super) fn canvas(dark: bool) -> Color32 {
    if dark {
        Color32::from_rgb(23, 29, 28)
    } else {
        Color32::from_rgb(235, 239, 235)
    }
}

pub(super) fn panel(context: &egui::Context, margin: i8) -> egui::Frame {
    egui::Frame::NONE
        .fill(context.style().visuals.panel_fill)
        .inner_margin(margin)
}

pub(super) fn section(ui: &mut egui::Ui, text: &str) {
    ui.label(RichText::new(text).size(10.0).strong().color(accent(ui)));
}

/// Paint the same book-and-wing mark used by the desktop app icon.
pub(super) fn logo(ui: &mut egui::Ui, size: f32) {
    let (rect, _) = ui.allocate_exact_size(Vec2::splat(size), egui::Sense::hover());
    let painter = ui.painter();
    painter.rect_filled(rect, size * 0.22, ACCENT);
    let point = |x: f32, y: f32| rect.min + Vec2::new(x, y) * (size / 64.0);
    for points in [
        vec![
            (16., 20.),
            (22., 21.),
            (27., 24.),
            (30., 27.),
            (30., 47.),
            (24., 43.),
            (16., 40.),
        ],
        vec![
            (34., 27.),
            (41., 21.),
            (50., 16.),
            (46., 30.),
            (40., 33.),
            (34., 37.),
        ],
        vec![
            (34., 41.),
            (40., 37.),
            (46., 34.),
            (44., 42.),
            (39., 44.),
            (34., 48.),
        ],
    ] {
        painter.add(egui::Shape::convex_polygon(
            points.into_iter().map(|(x, y)| point(x, y)).collect(),
            Color32::from_rgb(250, 249, 246),
            Stroke::NONE,
        ));
    }
}

#[derive(Clone, Copy, Hash)]
pub(super) enum Icon {
    Sidebar,
    Comments,
    Close,
    Previous,
    Next,
    Rotate,
    Search,
    Bookmark,
    Theme,
    Help,
}

/// Toolbar controls keep their action identity when responsive labels or
/// selection controls appear, disappear, or change their place in the row.
pub(super) fn toolbar_button(
    ui: &mut egui::Ui,
    action: impl std::hash::Hash,
    button: egui::Button<'_>,
) -> egui::Response {
    ui.scope_builder(
        egui::UiBuilder::new().id(("reader-toolbar", action)),
        |ui| ui.add(button),
    )
    .inner
}

pub(super) fn toolbar_tool(
    ui: &mut egui::Ui,
    icon: Icon,
    label: &str,
    selected: bool,
) -> egui::Response {
    let response = toolbar_button(
        ui,
        icon,
        egui::Button::new("")
            .min_size(Vec2::new(32.0, 32.0))
            .selected(selected),
    );
    response.widget_info(|| {
        egui::WidgetInfo::labeled(egui::WidgetType::Button, ui.is_enabled(), label)
    });
    let color = if selected {
        accent(ui)
    } else {
        ui.visuals().text_color()
    };
    let center = response.rect.center();
    let p = |x: f32, y: f32| center + Vec2::new(x, y);
    let painter = ui.painter();
    let stroke = Stroke::new(1.5_f32, color);
    let line = |points: &[(f32, f32)]| {
        painter.add(egui::Shape::line(
            points.iter().map(|&(x, y)| p(x, y)).collect(),
            stroke,
        ));
    };
    match icon {
        Icon::Sidebar => {
            painter.rect_stroke(
                egui::Rect::from_center_size(center, Vec2::new(17., 15.)),
                3.,
                stroke,
                egui::StrokeKind::Inside,
            );
            line(&[(-2., -7.), (-2., 7.)]);
        }
        Icon::Comments => line(&[
            (-8., -7.),
            (8., -7.),
            (8., 4.),
            (-2., 4.),
            (-6., 8.),
            (-6., 4.),
            (-8., 4.),
            (-8., -7.),
        ]),
        Icon::Close => {
            line(&[(-5., -5.), (5., 5.)]);
            line(&[(5., -5.), (-5., 5.)]);
        }
        Icon::Previous => line(&[(3., -6.), (-3., 0.), (3., 6.)]),
        Icon::Next => line(&[(-3., -6.), (3., 0.), (-3., 6.)]),
        Icon::Rotate => {
            line(&[
                (5., -4.),
                (1., -7.),
                (-5., -5.),
                (-7., 0.),
                (-4., 6.),
                (2., 7.),
                (6., 3.),
            ]);
            line(&[(5., -9.), (5., -4.), (0., -4.)]);
        }
        Icon::Search => {
            painter.circle_stroke(p(-2., -2.), 5.5, stroke);
            line(&[(2., 2.), (7., 7.)]);
        }
        Icon::Bookmark => line(&[
            (-6., 8.),
            (-6., -8.),
            (6., -8.),
            (6., 8.),
            (0., 4.),
            (-6., 8.),
        ]),
        Icon::Theme => {
            painter.circle_stroke(center, 5., stroke);
            for i in 0..8 {
                let angle = i as f32 * std::f32::consts::TAU / 8.;
                let direction = Vec2::angled(angle);
                painter.line_segment([center + direction * 7.5, center + direction * 9.], stroke);
            }
        }
        Icon::Help => {
            painter.circle_stroke(center, 8., stroke);
            painter.text(
                center,
                egui::Align2::CENTER_CENTER,
                "?",
                egui::FontId::proportional(14.),
                color,
            );
        }
    }
    response.on_hover_text(label)
}

pub(super) fn apply(context: &egui::Context, dark: bool) {
    let mut visuals = if dark {
        egui::Visuals::dark()
    } else {
        egui::Visuals::light()
    };
    let (panel, surface, hover, border, text, muted, tint, accent) = if dark {
        (
            Color32::from_rgb(30, 38, 35),
            Color32::from_rgb(37, 46, 42),
            Color32::from_rgb(47, 60, 53),
            Color32::from_rgb(58, 70, 63),
            Color32::from_rgb(229, 236, 230),
            Color32::from_rgb(159, 177, 164),
            Color32::from_rgb(43, 68, 55),
            Color32::from_rgb(133, 205, 174),
        )
    } else {
        (
            Color32::from_rgb(250, 251, 248),
            Color32::from_rgb(242, 245, 240),
            Color32::from_rgb(231, 238, 229),
            Color32::from_rgb(218, 226, 215),
            Color32::from_rgb(40, 55, 46),
            Color32::from_rgb(112, 128, 115),
            Color32::from_rgb(222, 237, 225),
            ACCENT,
        )
    };
    visuals.panel_fill = panel;
    visuals.window_fill = panel;
    visuals.extreme_bg_color = surface;
    visuals.faint_bg_color = surface;
    visuals.window_stroke = Stroke::new(1.0_f32, border);
    visuals.window_corner_radius = 14.into();
    visuals.menu_corner_radius = 10.into();
    visuals.weak_text_color = Some(muted);
    visuals.hyperlink_color = accent;
    visuals.error_fg_color = if dark {
        Color32::from_rgb(255, 160, 140)
    } else {
        Color32::from_rgb(165, 45, 35)
    };
    visuals.selection.bg_fill = tint;
    visuals.selection.stroke = Stroke::new(1.0_f32, accent);
    for widget in [
        &mut visuals.widgets.noninteractive,
        &mut visuals.widgets.inactive,
        &mut visuals.widgets.hovered,
        &mut visuals.widgets.active,
        &mut visuals.widgets.open,
    ] {
        widget.corner_radius = 7.into();
        widget.fg_stroke = Stroke::new(1.0_f32, text);
        widget.bg_stroke = Stroke::new(1.0_f32, border);
        widget.expansion = 0.;
    }
    visuals.widgets.noninteractive.bg_fill = surface;
    visuals.widgets.inactive.bg_fill = surface;
    visuals.widgets.inactive.weak_bg_fill = surface;
    visuals.widgets.inactive.bg_stroke = Stroke::NONE;
    visuals.widgets.hovered.bg_fill = hover;
    visuals.widgets.hovered.weak_bg_fill = hover;
    visuals.widgets.hovered.bg_stroke = Stroke::new(1.0_f32, border);
    visuals.widgets.active.bg_fill = tint;
    visuals.widgets.active.weak_bg_fill = tint;
    visuals.widgets.active.fg_stroke.color = accent;
    visuals.widgets.open.bg_fill = tint;
    visuals.widgets.open.weak_bg_fill = tint;
    context.set_visuals(visuals);
    context.style_mut(|style| {
        style.spacing.button_padding = Vec2::new(11., 6.);
        style.spacing.item_spacing = Vec2::new(8., 8.);
        style.spacing.window_margin = 20.into();
        style
            .text_styles
            .insert(egui::TextStyle::Heading, egui::FontId::proportional(23.));
    });
}
