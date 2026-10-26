use hayro::kurbo::Rect;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum DestinationFit {
    Page,
    Width,
    Height,
    Rectangle(Rect),
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Destination {
    pub page: usize,
    /// Renderer coordinates, after the PDF's own crop and rotation transform.
    pub position: [Option<f64>; 2],
    pub zoom: Option<f32>,
    pub fit: Option<DestinationFit>,
}

impl Destination {
    pub fn page(page: usize) -> Self {
        Self {
            page,
            position: [None; 2],
            zoom: None,
            fit: None,
        }
    }

    pub fn fit(
        self,
        current: crate::navigation::Fit,
        dimensions: [f32; 2],
        rotation: u8,
        viewport: [f32; 2],
    ) -> crate::navigation::Fit {
        use crate::navigation::Fit;
        if let Some(zoom) = self.zoom {
            return Fit::Custom(zoom);
        }
        match self.fit {
            Some(DestinationFit::Page) => Fit::Page,
            Some(DestinationFit::Width) => Fit::Width,
            Some(DestinationFit::Height) => Fit::Height,
            Some(DestinationFit::Rectangle(rect)) => {
                let rect = crate::geometry::rotate_rect(rect, dimensions, rotation);
                Fit::Custom(
                    (f64::from(viewport[0]) / rect.width())
                        .min(f64::from(viewport[1]) / rect.height()) as f32,
                )
            }
            None => current,
        }
    }

    pub fn pan(
        self,
        dimensions: [f32; 2],
        rotation: u8,
        scale: f32,
        viewport: [f32; 2],
        current: [f64; 2],
        overflow: [f32; 2],
    ) -> [f32; 2] {
        let position = if let Some(DestinationFit::Rectangle(rect)) = self.fit {
            let rect = crate::geometry::rotate_rect(rect, dimensions, rotation);
            [Some(rect.center().x), Some(rect.center().y)]
        } else {
            rotate_position(self.position, dimensions, rotation)
        };
        let centered = matches!(self.fit, Some(DestinationFit::Rectangle(_)));
        std::array::from_fn(|axis| {
            let coordinate = position[axis].unwrap_or(current[axis]);
            (coordinate * f64::from(scale)
                - if centered {
                    f64::from(viewport[axis]) * 0.5
                } else {
                    0.0
                })
            .clamp(0.0, f64::from(overflow[axis])) as f32
        })
    }
}

/// Rotate nullable coordinates without inventing values for PDF `null` operands.
pub fn rotate_position(
    position: [Option<f64>; 2],
    dimensions: [f32; 2],
    rotation: u8,
) -> [Option<f64>; 2] {
    let [x, y] = position;
    let [w, h] = dimensions.map(f64::from);
    match rotation % 4 {
        1 => [y.map(|y| h - y), x],
        2 => [x.map(|x| w - x), y.map(|y| h - y)],
        3 => [y, x.map(|x| w - x)],
        _ => position,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::navigation::Fit;

    #[test]
    fn destinations_position_the_requested_point_in_every_reading_rotation() {
        let target = Destination {
            position: [Some(48.0), Some(300.0)],
            zoom: Some(2.0),
            ..Destination::page(0)
        };
        for (rotation, overflow, expected) in [
            (0, [700.0, 1200.0], [96.0, 600.0]),
            (1, [1100.0, 800.0], [1000.0, 96.0]),
            (2, [700.0, 1200.0], [700.0, 1000.0]),
            (3, [1100.0, 800.0], [600.0, 800.0]),
        ] {
            assert_eq!(
                target.fit(Fit::Width, [600.0, 800.0], rotation, [500.0, 400.0]),
                Fit::Custom(2.0)
            );
            assert_eq!(
                target.pan(
                    [600.0, 800.0],
                    rotation,
                    2.0,
                    [500.0, 400.0],
                    [0.0; 2],
                    overflow
                ),
                expected
            );
        }
    }

    #[test]
    fn nullable_axes_retain_document_position_when_zoom_changes() {
        let target = Destination {
            position: [None, Some(300.0)],
            ..Destination::page(0)
        };
        assert_eq!(
            target.pan(
                [600.0, 800.0],
                0,
                2.0,
                [500.0, 400.0],
                [80.0, 90.0],
                [700.0, 1200.0]
            ),
            [160.0, 600.0]
        );
        assert_eq!(
            target.pan(
                [600.0, 800.0],
                1,
                2.0,
                [500.0, 400.0],
                [80.0, 90.0],
                [1100.0, 800.0]
            ),
            [1000.0, 180.0]
        );
    }

    #[test]
    fn rectangular_destinations_fit_and_center_the_requested_region() {
        let target = Destination {
            fit: Some(DestinationFit::Rectangle(Rect::new(
                100.0, 500.0, 300.0, 700.0,
            ))),
            ..Destination::page(0)
        };
        assert_eq!(
            target.fit(Fit::Page, [600.0, 800.0], 0, [500.0, 400.0]),
            Fit::Custom(2.0)
        );
        assert_eq!(
            target.pan(
                [600.0, 800.0],
                0,
                2.0,
                [500.0, 400.0],
                [0.0; 2],
                [700.0, 1200.0]
            ),
            [150.0, 1000.0]
        );
        let height = Destination {
            fit: Some(DestinationFit::Height),
            ..Destination::page(0)
        };
        assert_eq!(
            height.fit(Fit::Page, [600.0, 800.0], 0, [500.0, 400.0]),
            Fit::Height
        );
    }
}
