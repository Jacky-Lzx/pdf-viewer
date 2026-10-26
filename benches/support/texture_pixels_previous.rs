// Previous bitmap-to-texture implementation, for measurement only.
use crate::bitmap::{Bitmap, write_quarter_turn};
use eframe::egui::{Color32, ColorImage};

/// Transform directly into the final texture allocation, without a rotated byte image.
pub fn prepare(bitmap: Bitmap, turns: u8, night: bool) -> ColorImage {
    if night {
        transform::<true>(bitmap, turns)
    } else {
        transform::<false>(bitmap, turns)
    }
}

fn transform<const NIGHT: bool>(bitmap: Bitmap, turns: u8) -> ColorImage {
    let [w, h] = bitmap.size;
    let source = bitmap.rgba.as_chunks::<4>().0;
    let convert = |[r, g, b, a]: [u8; 4]| {
        if NIGHT {
            Color32::from_rgba_premultiplied(255 - r, 255 - g, 255 - b, a)
        } else {
            Color32::from_rgba_premultiplied(r, g, b, a)
        }
    };
    let turns = turns % 4;
    let size = if turns % 2 == 1 { [h, w] } else { [w, h] };
    let pixels = match turns {
        0 => source.iter().copied().map(convert).collect(),
        2 => source.iter().rev().copied().map(convert).collect(),
        _ => {
            let mut pixels = vec![Color32::TRANSPARENT; source.len()];
            write_quarter_turn(source, &mut pixels, w, h, turns, convert);
            pixels
        }
    };
    ColorImage::new(size, pixels)
}
