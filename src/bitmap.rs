pub struct Bitmap {
    pub size: [usize; 2],
    pub rgba: Vec<u8>,
}

pub fn rotate(bitmap: Bitmap, turns: u8) -> Bitmap {
    transform::<false>(bitmap, turns)
}

pub fn rotate_inverted(bitmap: Bitmap, turns: u8) -> Bitmap {
    transform::<true>(bitmap, turns)
}

#[inline]
fn color<const INVERT: bool>(pixel: [u8; 4]) -> [u8; 4] {
    if INVERT {
        [255 - pixel[0], 255 - pixel[1], 255 - pixel[2], pixel[3]]
    } else {
        pixel
    }
}

fn transform<const INVERT: bool>(mut bitmap: Bitmap, turns: u8) -> Bitmap {
    let turns = turns % 4;
    if turns == 0 {
        if INVERT {
            for pixel in bitmap.rgba.as_chunks_mut::<4>().0 {
                *pixel = color::<true>(*pixel);
            }
        }
        return bitmap;
    }
    if turns == 2 {
        // Reverse pixels, preserving channel order and reusing the full allocation.
        let pixels = bitmap.rgba.as_chunks_mut::<4>().0;
        if INVERT {
            let middle = pixels.len() / 2;
            let (first, second) = pixels.split_at_mut(middle);
            for (left, right) in first.iter_mut().zip(second.iter_mut().rev()) {
                let original = *left;
                *left = color::<true>(*right);
                *right = color::<true>(original);
            }
            if pixels.len() % 2 == 1 {
                pixels[middle] = color::<true>(pixels[middle]);
            }
        } else {
            pixels.reverse();
        }
        return bitmap;
    }
    let [w, h] = bitmap.size;
    let size = [h, w];
    let mut rgba = vec![0; bitmap.rgba.len()];
    let source = bitmap.rgba.as_chunks::<4>().0;
    let target = rgba.as_chunks_mut::<4>().0;
    write_quarter_turn(source, target, w, h, turns, color::<INVERT>);
    Bitmap { size, rgba }
}

/// Shared traversal for byte exports and directly prepared texture pixels.
pub(crate) fn write_quarter_turn<T>(
    source: &[[u8; 4]],
    target: &mut [T],
    w: usize,
    h: usize,
    turns: u8,
    convert: impl Fn([u8; 4]) -> T,
) {
    if source.len() <= 4_000_000 {
        // Small pages benefit from the simpler sequential source walk.
        for y in 0..h {
            for x in 0..w {
                let (nx, ny) = if turns == 1 {
                    (h - 1 - y, x)
                } else {
                    (y, w - 1 - x)
                };
                target[ny * h + nx] = convert(source[y * w + x]);
            }
        }
    } else {
        // Large transposes otherwise write a full page stride between each pixel.
        // Keep source and destination tiles close in cache, writing output rows.
        const TILE: usize = 64;
        for y0 in (0..h).step_by(TILE) {
            for x0 in (0..w).step_by(TILE) {
                for x in x0..(x0 + TILE).min(w) {
                    let ny = if turns == 1 { x } else { w - 1 - x };
                    let row = &mut target[ny * h..(ny + 1) * h];
                    for y in y0..(y0 + TILE).min(h) {
                        let nx = if turns == 1 { h - 1 - y } else { y };
                        row[nx] = convert(source[y * w + x]);
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn fused_inversion_matches_two_passes_including_alpha_and_odd_center_pixels() {
        use super::{Bitmap, rotate, rotate_inverted};

        for size in [[1, 1], [1, 5], [2, 3], [31, 33], [2001, 2000]] {
            let pixels: Vec<_> = (0..size[0] * size[1] * 4)
                .map(|value| (value * 17 + value / 257) as u8)
                .collect();
            let mut inverted = pixels.clone();
            for pixel in inverted.as_chunks_mut::<4>().0 {
                for channel in &mut pixel[..3] {
                    *channel = 255 - *channel;
                }
            }
            for turns in 0..8 {
                let input = Bitmap {
                    size,
                    rgba: pixels.clone(),
                };
                let address = input.rgba.as_ptr();
                let actual = rotate_inverted(input, turns);
                let expected = rotate(
                    Bitmap {
                        size,
                        rgba: inverted.clone(),
                    },
                    turns,
                );
                assert_eq!(actual.size, expected.size);
                assert_eq!(actual.rgba, expected.rgba, "{size:?}, {turns} turns");
                if turns.is_multiple_of(2) {
                    assert_eq!(actual.rgba.as_ptr(), address);
                }
                let restored = rotate_inverted(actual, (4 - turns % 4) % 4);
                assert_eq!(restored.size, size);
                assert_eq!(restored.rgba, pixels);
            }
        }
    }

    #[test]
    fn quarter_and_half_turns_keep_pixel_channels_in_the_correct_order() {
        use super::{Bitmap, rotate};

        let pixels = (1..=6)
            .flat_map(|value| [value, value + 10, value + 20, 255])
            .collect::<Vec<_>>();
        for (turns, size, expected) in [
            (0, [2, 3], [1, 2, 3, 4, 5, 6]),
            (1, [3, 2], [5, 3, 1, 6, 4, 2]),
            (2, [2, 3], [6, 5, 4, 3, 2, 1]),
            (3, [3, 2], [2, 4, 6, 1, 3, 5]),
            (4, [2, 3], [1, 2, 3, 4, 5, 6]),
        ] {
            let rotated = rotate(
                Bitmap {
                    size: [2, 3],
                    rgba: pixels.clone(),
                },
                turns,
            );
            assert_eq!(rotated.size, size);
            for (pixel, value) in rotated.rgba.as_chunks::<4>().0.iter().zip(expected) {
                assert_eq!(*pixel, [value, value + 10, value + 20, 255]);
            }
        }
    }

    #[test]
    fn half_turn_reuses_the_original_allocation() {
        use super::{Bitmap, rotate};

        let bitmap = Bitmap {
            size: [31, 33],
            rgba: (0..31 * 33 * 4).map(|value| value as u8).collect(),
        };
        let address = bitmap.rgba.as_ptr();
        let original = bitmap.rgba.clone();
        let rotated = rotate(bitmap, 2);
        assert_eq!(rotated.rgba.as_ptr(), address);
        assert_eq!(rotate(rotated, 2).rgba, original);
    }

    #[test]
    fn tiled_rotation_roundtrips_portrait_landscape_and_partial_edge_tiles() {
        use super::{Bitmap, rotate};

        for size in [[2001, 2000], [2048, 2049], [4097, 977]] {
            let pixels: Vec<_> = (0..size[0] * size[1] * 4)
                .map(|value| (value * 17 + value / 257) as u8)
                .collect();
            let rotated = rotate(
                Bitmap {
                    size,
                    rgba: pixels.clone(),
                },
                1,
            );
            assert_eq!(rotated.size, [size[1], size[0]]);
            let restored = rotate(rotated, 3);
            assert_eq!(restored.size, size);
            assert_eq!(restored.rgba, pixels);
            let twice = rotate(
                rotate(
                    Bitmap {
                        size,
                        rgba: pixels.clone(),
                    },
                    1,
                ),
                1,
            );
            assert_eq!(twice.size, size);
            assert!(
                twice
                    .rgba
                    .as_chunks::<4>()
                    .0
                    .iter()
                    .eq(pixels.as_chunks::<4>().0.iter().rev())
            );
        }
    }
}
