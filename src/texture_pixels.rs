use crate::bitmap::{reverse_map, write_quarter_turn};
use eframe::egui::{Color32, ColorImage};

/// Reuse the raster target for even rotations; allocate once for quarter turns.
pub fn prepare(image: ColorImage, turns: u8, night: bool) -> ColorImage {
    if night {
        transform::<true>(image, turns)
    } else {
        transform::<false>(image, turns)
    }
}

fn transform<const NIGHT: bool>(mut image: ColorImage, turns: u8) -> ColorImage {
    let [w, h] = image.size;
    let convert = |[r, g, b, a]: [u8; 4]| {
        if NIGHT {
            Color32::from_rgba_premultiplied(255 - r, 255 - g, 255 - b, a)
        } else {
            Color32::from_rgba_premultiplied(r, g, b, a)
        }
    };
    let turns = turns % 4;
    match turns {
        0 => {
            if NIGHT {
                for pixel in &mut image.pixels {
                    *pixel = convert(pixel.to_array());
                }
            }
            image
        }
        2 => {
            if NIGHT {
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
