use hayro::{
    PixmapSettings, RenderCache, RenderSettings, hayro_interpret::InterpreterSettings,
    hayro_syntax::Pdf, vello_cpu::color::palette::css::WHITE,
};
use std::path::Path;

pub const MAX_PIXELS: f64 = 16_000_000.0;

pub struct RasterSize {
    pub scale: f32,
    pub size: [usize; 2],
}

impl RasterSize {
    pub fn new([w, h]: [f32; 2], width: u32) -> Result<Self, String> {
        if !w.is_finite() || !h.is_finite() || w <= 0.0 || h <= 0.0 {
            return Err("Invalid PDF page dimensions".into());
        }
        let scale = (width.max(1) as f32 / w)
            .min((MAX_PIXELS / f64::from(w) / f64::from(h)).sqrt() as f32)
            .min(16384.0 / w.max(h));
        // Match hayro's truncation when allocating the raster, including its size limits.
        let size = [(w * scale) as usize, (h * scale) as usize];
        if size.contains(&0) {
            return Err("Page aspect ratio exceeds supported rendering limits".into());
        }
        Ok(Self { scale, size })
    }

    pub fn bytes(&self) -> usize {
        self.size[0] * self.size[1] * 4
    }
}

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

pub use crate::bitmap::Bitmap;

pub fn rasterize<'a>(
    pdf: &'a Pdf,
    cache: &RenderCache<'a>,
    key: RenderKey,
) -> Result<Bitmap, String> {
    let bitmap = rasterize_untransformed(pdf, cache, key.page, key.width)?;
    Ok(if key.night {
        crate::bitmap::rotate_inverted(bitmap, key.rotation)
    } else {
        crate::bitmap::rotate(bitmap, key.rotation)
    })
}

pub(crate) fn rasterize_untransformed<'a>(
    pdf: &'a Pdf,
    cache: &RenderCache<'a>,
    page: usize,
    width: u32,
) -> Result<Bitmap, String> {
    let pages = pdf.pages();
    let page = pages.get(page).ok_or("Page is out of range")?;
    let (w, h) = page.render_dimensions();
    let raster = RasterSize::new([w, h], width)?;
    let pixmap = hayro::render(
        page,
        cache,
        &InterpreterSettings::default(),
        &RenderSettings::default(),
        &PixmapSettings {
            x_scale: raster.scale,
            y_scale: raster.scale,
            bg_color: WHITE,
        },
    );
    Ok(Bitmap {
        size: [pixmap.width() as usize, pixmap.height() as usize],
        rgba: pixmap.take_rgba8(hayro::vello_cpu::peniko::ImageAlphaType::AlphaPremultiplied),
    })
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
        let raster = RasterSize::new([600.0, 800.0], 300).unwrap();
        assert_eq!(raster.size, bitmap.size);
        assert_eq!(raster.bytes(), bitmap.rgba.len());
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
    fn encrypted_pdf_rejects_bad_passwords_and_decodes_real_text() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("encrypted.pdf");
        std::fs::write(&path, include_bytes!("../tests/fixtures/encrypted.pdf")).unwrap();
        for password in ["", "wrong"] {
            assert!(load(&path, password).err().unwrap().contains("Decryption"));
        }
        let pdf = load(&path, "jkgdi").unwrap();
        assert_eq!(pdf.pages().len(), 3);
        let interpreter = hayro::hayro_interpret::InterpreterCache::new();
        for (page, data) in pdf.pages().iter().enumerate() {
            let text = crate::text::extract(data, &interpreter);
            assert!(
                text.text
                    .contains(&format!("Encrypted document page {}.", page + 1)),
                "{}",
                text.text
            );
            assert_eq!(text.search(page, "Rust").len(), 1);
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
    fn raster_limits_handle_large_and_invalid_page_dimensions() {
        for dimensions in [
            [600.0, 800.0],
            [800.0, 600.0],
            [10000.0, 100.0],
            [f32::MAX, f32::MAX],
        ] {
            let raster = RasterSize::new(dimensions, 16384).unwrap();
            assert!(raster.size.iter().all(|size| (1..=16384).contains(size)));
            assert!(raster.size[0] * raster.size[1] <= MAX_PIXELS as usize);
            assert_eq!(raster.bytes(), raster.size[0] * raster.size[1] * 4);
        }
        for dimensions in [
            [0.0, 100.0],
            [f32::NAN, 1.0],
            [f32::INFINITY, 1.0],
            [1.0, 1e30],
        ] {
            assert!(RasterSize::new(dimensions, 1200).is_err());
        }
    }
}
