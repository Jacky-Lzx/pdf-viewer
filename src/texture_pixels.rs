use crate::bitmap::{reverse_map, write_quarter_turn};
use eframe::egui::{Color32, ColorImage};

/// Reuse the raster target for even rotations; allocate once for quarter turns.
pub fn prepare(image: ColorImage, turns: u8, night: bool) -> ColorImage {
    if night {
        transform::<true>(image, turns, |[r, g, b, a]| {
            Color32::from_rgba_premultiplied(255 - r, 255 - g, 255 - b, a)
        })
    } else {
        transform::<false>(image, turns, |rgba| {
            Color32::from_rgba_premultiplied(rgba[0], rgba[1], rgba[2], rgba[3])
        })
    }
}

pub fn prepare_with_palette(
    image: ColorImage,
    turns: u8,
    palette: crate::page_colors::PagePalette,
) -> ColorImage {
    use crate::page_colors::PagePalette;
    // Preserve the allocation-free identity path and exact legacy inversion.
    if palette.foreground == PagePalette::default().foreground
        && palette.background == PagePalette::default().background
    {
        return prepare(image, turns, false);
    }
    if palette.foreground == PagePalette::inverted().foreground
        && palette.background == PagePalette::inverted().background
    {
        return prepare(image, turns, true);
    }
    let foreground = palette.foreground.0.map(u32::from);
    let background = palette.background.0.map(u32::from);
    // Map each RGB channel independently: grayscale follows the paper/ink
    // gradient, while colored figures retain their channel differences.
    let channels: [[u8; 256]; 3] = std::array::from_fn(|channel| {
        std::array::from_fn(|value| {
            ((foreground[channel] * (255 - value as u32)
                + background[channel] * value as u32
                + 127)
                / 255) as u8
        })
    });
    transform::<true>(image, turns, |[r, g, b, a]| {
        let input = [r, g, b];
        let rgb: [u8; 3] = std::array::from_fn(|channel| {
            if a == 255 {
                channels[channel][usize::from(input[channel])]
            } else {
                let value = u32::from(input[channel].min(a));
                ((foreground[channel] * (u32::from(a) - value) + background[channel] * value + 127)
                    / 255) as u8
            }
        });
        Color32::from_rgba_premultiplied(rgb[0], rgb[1], rgb[2], a)
    })
}

fn transform<const CONVERT: bool>(
    mut image: ColorImage,
    turns: u8,
    convert: impl Fn([u8; 4]) -> Color32,
) -> ColorImage {
    let [w, h] = image.size;
    let turns = turns % 4;
    match turns {
        0 => {
            if CONVERT {
                for pixel in &mut image.pixels {
                    *pixel = convert(pixel.to_array());
                }
            }
            image
        }
        2 => {
            if CONVERT {
                reverse_map(&mut image.pixels, |pixel| convert(pixel.to_array()));
            } else {
                image.pixels.reverse();
            }
            image
        }
        _ => {
            let source = bytemuck::cast_slice(&image.pixels);
            let mut pixels = vec![Color32::TRANSPARENT; source.len()];
            write_quarter_turn(source, &mut pixels, w, h, turns, convert);
            ColorImage::new([h, w], pixels)
        }
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn custom_palette_maps_ink_paper_antialiasing_color_and_alpha_through_rotations() {
        use super::*;
        use crate::page_colors::{PagePalette, Rgb};
        let palette = PagePalette {
            foreground: Rgb([205, 214, 244]),
            background: Rgb([30, 30, 46]),
            canvas: None,
        };
        let input = ColorImage::new(
            [2, 3],
            vec![
                Color32::BLACK,
                Color32::WHITE,
                Color32::from_gray(128),
                Color32::RED,
                Color32::TRANSPARENT,
                Color32::from_rgba_premultiplied(0, 0, 0, 128),
            ],
        );
        let expected = ColorImage::new(
            [2, 3],
            vec![
                palette.foreground.color(),
                palette.background.color(),
                Color32::from_rgb(117, 122, 145),
                Color32::from_rgb(30, 214, 244),
                Color32::TRANSPARENT,
                Color32::from_rgba_premultiplied(103, 107, 122, 128),
            ],
        );
        for turns in 0..8 {
            let image = input.clone();
            let original = image.pixels.as_ptr();
            let actual = prepare_with_palette(image, turns, palette);
            assert_eq!(actual, prepare(expected.clone(), turns, false));
            if turns % 2 == 0 {
                assert_eq!(actual.pixels.as_ptr(), original);
            }
        }
    }

    #[test]
    fn direct_texture_pixels_match_byte_transforms_including_alpha_and_tile_edges() {
        use super::*;
        use crate::bitmap::{Bitmap, rotate, rotate_inverted};

        for size in [[1, 1], [1, 5], [5, 1], [2, 3], [31, 33], [2001, 2000]] {
            let rgba: Vec<_> = (0..size[0] * size[1] * 4)
                .map(|index| (index * 17 + index / 257) as u8)
                .collect();
            for turns in 0..8 {
                for night in [false, true] {
                    let input = || Bitmap {
                        size,
                        rgba: rgba.clone(),
                    };
                    let expected = if night {
                        rotate_inverted(input(), turns)
                    } else {
                        rotate(input(), turns)
                    };
                    let expected =
                        ColorImage::from_rgba_premultiplied(expected.size, &expected.rgba);
                    let image = ColorImage::from_rgba_premultiplied(size, &rgba);
                    let original = image.pixels.as_ptr();
                    let actual = prepare(image, turns, night);
                    assert!(actual == expected, "{size:?}, turns={turns}, night={night}");
                    if turns % 2 == 0 {
                        assert_eq!(
                            actual.pixels.as_ptr(),
                            original,
                            "even rotations reuse the raster target"
                        );
                    }
                }
            }
        }
    }
}
