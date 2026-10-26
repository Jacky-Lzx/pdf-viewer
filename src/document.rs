use hayro::{
    PixmapSettings, RenderCache, RenderSettings, hayro_interpret::InterpreterSettings,
    hayro_syntax::Pdf, vello_cpu::color::palette::css::WHITE,
};
use std::path::Path;

pub const MAX_PIXELS: f32 = 16_000_000.0;

pub fn load(path: &Path, password: &str) -> Result<Pdf, String> {
    let data =
        std::fs::read(path).map_err(|error| format!("Cannot read {}: {error}", path.display()))?;
    let pdf = Pdf::new_with_password(data, password)
        .map_err(|error| format!("Cannot open PDF: {error:?}"))?;
    if pdf.pages().is_empty() {
        return Err("This PDF has no pages.".into());
    }
    Ok(pdf)
}

#[derive(Clone, Copy, Debug, Hash, PartialEq, Eq)]
pub struct RenderKey {
    pub page: usize,
    pub width: u32,
    pub rotation: u8,
    pub night: bool,
}

pub struct Bitmap {
    pub size: [usize; 2],
    pub rgba: Vec<u8>,
}

pub fn rasterize<'a>(
    pdf: &'a Pdf,
    cache: &RenderCache<'a>,
    key: RenderKey,
) -> Result<Bitmap, String> {
    let pages = pdf.pages();
    let page = pages.get(key.page).ok_or("Page is out of range")?;
    let (w, h) = page.render_dimensions();
    if !w.is_finite() || !h.is_finite() || w <= 0.0 || h <= 0.0 {
        return Err("Invalid PDF page dimensions".into());
    }
    let scale = (key.width.max(1) as f32 / w)
        .min((MAX_PIXELS / (w * h)).sqrt())
        .min(16384.0 / w.max(h));
    if (w * scale) < 1.0 || (h * scale) < 1.0 {
        return Err("Page aspect ratio exceeds supported rendering limits".into());
    }
    let pixmap = hayro::render(
        page,
        cache,
        &InterpreterSettings::default(),
        &RenderSettings::default(),
        &PixmapSettings {
            x_scale: scale,
            y_scale: scale,
            bg_color: WHITE,
        },
    );
    let mut bitmap = Bitmap {
        size: [pixmap.width() as usize, pixmap.height() as usize],
        rgba: pixmap.take_rgba8(hayro::vello_cpu::peniko::ImageAlphaType::AlphaPremultiplied),
    };
    if key.night {
        for pixel in bitmap.rgba.as_chunks_mut::<4>().0 {
            for channel in &mut pixel[..3] {
                *channel = 255 - *channel;
            }
        }
    }
    if !key.rotation.is_multiple_of(4) {
        bitmap = rotate(bitmap, key.rotation);
    }
    Ok(bitmap)
}

fn rotate(bitmap: Bitmap, turns: u8) -> Bitmap {
    let [w, h] = bitmap.size;
    let size = if turns % 2 == 1 { [h, w] } else { [w, h] };
    let mut rgba = vec![0; bitmap.rgba.len()];
    for y in 0..h {
        for x in 0..w {
            let (nx, ny) = match turns % 4 {
                1 => (h - 1 - y, x),
                2 => (w - 1 - x, h - 1 - y),
                _ => (y, w - 1 - x),
            };
            rgba[(ny * size[0] + nx) * 4..(ny * size[0] + nx + 1) * 4]
                .copy_from_slice(&bitmap.rgba[(y * w + x) * 4..(y * w + x + 1) * 4]);
        }
    }
    Bitmap { size, rgba }
}

pub fn write_png(bitmap: &Bitmap, path: &Path) -> Result<(), String> {
    let file = std::fs::File::create(path).map_err(|e| e.to_string())?;
    let mut encoder = png::Encoder::new(
        std::io::BufWriter::new(file),
        bitmap.size[0] as u32,
        bitmap.size[1] as u32,
    );
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    let mut writer = encoder.write_header().map_err(|e| e.to_string())?;
    writer
        .write_image_data(&bitmap.rgba)
        .map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> Pdf {
        Pdf::new(include_bytes!("../assets/folio-guide.pdf").to_vec()).unwrap()
    }

    #[test]
    fn real_pdf_renders_vectors_text_and_multiple_pages() {
        let pdf = sample();
        assert_eq!(pdf.pages().len(), 6);
        let cache = RenderCache::new();
        let bitmap = rasterize(
            &pdf,
            &cache,
            RenderKey {
                page: 0,
                width: 300,
                rotation: 0,
                night: false,
            },
        )
        .unwrap();
        assert_eq!(bitmap.size, [300, 400]);
        assert_eq!(bitmap.rgba.len(), 300 * 400 * 4);
        assert!(
            bitmap
                .rgba
                .as_chunks::<4>()
                .0
                .iter()
                .any(|pixel| pixel[0] < 100 && pixel[1] < 100)
        );
        assert!(
            rasterize(
                &pdf,
                &cache,
                RenderKey {
                    page: 99,
                    width: 300,
                    rotation: 0,
                    night: false
                }
            )
            .is_err()
        );
        let inverted = rasterize(
            &pdf,
            &cache,
            RenderKey {
                page: 0,
                width: 300,
                rotation: 1,
                night: true,
            },
        )
        .unwrap();
        assert_eq!(inverted.size, [400, 300]);
        assert!(
            inverted
                .rgba
                .as_chunks::<4>()
                .0
                .iter()
                .any(|pixel| pixel[0] < 10)
        );
    }

    #[test]
    fn real_pdf_text_extraction_retains_words_and_search_geometry() {
        let pdf = sample();
        let text = crate::text::extract(
            &pdf.pages()[0],
            &hayro::hayro_interpret::InterpreterCache::new(),
        );
        assert!(text.text.contains("written in Rust"), "{}", text.text);
        let hits = text.search(0, "rust");
        assert_eq!(hits.len(), 1);
        assert!(!hits[0].rects.is_empty());
        for rect in &hits[0].rects {
            assert!(rect.x0 >= 0.0 && rect.x1 <= 600.0);
            assert!(rect.y0 >= 0.0 && rect.y1 <= 800.0);
            assert!(rect.width() > 0.0 && rect.height() > 0.0);
        }
    }

    #[test]
    fn invalid_and_missing_documents_report_errors() {
        let dir = tempfile::tempdir().unwrap();
        assert!(load(&dir.path().join("missing.pdf"), "").is_err());
        let path = dir.path().join("invalid.pdf");
        std::fs::write(&path, b"not a PDF").unwrap();
        assert!(load(&path, "").is_err());
    }
    #[test]
    fn rotation_preserves_pixels_and_roundtrips() {
        let bitmap = Bitmap {
            size: [2, 3],
            rgba: (0..24).collect(),
        };
        let original = bitmap.rgba.clone();
        let once = rotate(bitmap, 1);
        assert_eq!(once.size, [3, 2]);
        let restored = rotate(once, 3);
        assert_eq!(restored.size, [2, 3]);
        assert_eq!(restored.rgba, original);
    }
}
