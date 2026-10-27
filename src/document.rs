use hayro::{
    PixmapSettings, RenderCache, RenderSettings, hayro_interpret::InterpreterSettings,
    hayro_syntax::Pdf, vello_cpu::color::palette::css::WHITE,
};
use std::path::Path;

pub const MAX_PIXELS: f64 = 16_000_000.0;

pub fn check_output_path(source: &Path, output: &Path) -> Result<(), String> {
    #[cfg(unix)]
    let same = {
        use std::os::unix::fs::MetadataExt;
        match (std::fs::metadata(source), std::fs::metadata(output)) {
            (Ok(source), Ok(output)) => {
                source.dev() == output.dev() && source.ino() == output.ino()
            }
            _ => false,
        }
    };
    #[cfg(not(unix))]
    let same = source
        .canonicalize()
        .ok()
        .zip(output.canonicalize().ok())
        .is_some_and(|(source, output)| source == output);
    if same {
        return Err(format!(
            "Cannot export PNG over the source PDF: {}",
            output.display()
        ));
    }
    Ok(())
}

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
    parse(data, password)
}

pub fn load_cancellable(
    path: &Path,
    password: &str,
    cancelled: impl Fn() -> bool,
) -> Result<Option<Pdf>, String> {
    if cancelled() {
        return Ok(None);
    }
    let bytes = (|| {
        let mut file = std::fs::File::open(path)?;
        let expected = file.metadata()?.len();
        crate::document_bytes::read(&mut file, expected, &cancelled)
    })()
    .map_err(|error: std::io::Error| format!("Cannot read {}: {error}", path.display()))?;
    let Some(bytes) = bytes else { return Ok(None) };
    if cancelled() {
        return Ok(None);
    }
    let pdf = parse(bytes, password)?;
    Ok((!cancelled()).then_some(pdf))
}

fn parse(data: Vec<u8>, password: &str) -> Result<Pdf, String> {
    let pdf = Pdf::new_with_password(data, password)
        .map_err(|error| format!("Cannot open PDF: {error:?}"))?;
    if pdf.pages().is_empty() {
        return Err("This PDF has no pages.".into());
    }
    Ok(pdf)
}

/// A small target in the rotated page's physical pixel coordinates. One pixel
/// of overlap keeps linear texture filtering from introducing tile seams.
#[derive(Clone, Copy, Debug, Hash, PartialEq, Eq)]
pub struct Tile {
    pub scale_bits: u32,
    pub x: u32,
    pub y: u32,
    pub size: [u16; 2],
}

impl Tile {
    pub fn scale(self) -> f32 {
        f32::from_bits(self.scale_bits)
    }

    pub fn bytes(self) -> usize {
        usize::from(self.size[0]) * usize::from(self.size[1]) * 4
    }
}

#[derive(Clone, Copy, Debug, Hash, PartialEq, Eq)]
pub struct RenderKey {
    pub page: usize,
    pub width: u32,
    pub rotation: u8,
    pub night: bool,
    pub tile: Option<Tile>,
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

/// Render premultiplied RGBA directly into the aligned final texture buffer.
pub(crate) fn rasterize_pixels<'a>(
    pdf: &'a Pdf,
    cache: &RenderCache<'a>,
    page: usize,
    width: u32,
) -> Result<eframe::egui::ColorImage, String> {
    use hayro::{hayro_interpret::util::TransformExt, kurbo::Affine};

    let page = pdf.pages().get(page).ok_or("Page is out of range")?;
    let (w, h) = page.render_dimensions();
    let raster = RasterSize::new([w, h], width)?;
    let transform = Affine::scale_non_uniform(f64::from(raster.scale), f64::from(raster.scale))
        * page.initial_transform(true).to_kurbo();
    render_pixels(page, cache, raster.size, transform)
}

/// Render directly into a small viewport tile, without allocating a zoomed
/// full-page surface. Crop boxes and intrinsic PDF rotation are applied first.
pub(crate) fn rasterize_tile<'a>(
    pdf: &'a Pdf,
    cache: &RenderCache<'a>,
    key: RenderKey,
    tile: Tile,
) -> Result<eframe::egui::ColorImage, String> {
    use hayro::{hayro_interpret::util::TransformExt, kurbo::Affine};
    let page = pdf.pages().get(key.page).ok_or("Page is out of range")?;
    if !tile.scale().is_finite()
        || tile.scale() <= 0.0
        || tile.size.contains(&0)
        || tile.size.iter().any(|&side| side > 1026)
    {
        return Err("Invalid PDF render tile".into());
    }
    let (w, h) = page.render_dimensions();
    let rotation = Affine::new(match key.rotation % 4 {
        1 => [0.0, 1.0, -1.0, 0.0, f64::from(h), 0.0],
        2 => [-1.0, 0.0, 0.0, -1.0, f64::from(w), f64::from(h)],
        3 => [0.0, -1.0, 1.0, 0.0, 0.0, f64::from(w)],
        _ => [1.0, 0.0, 0.0, 1.0, 0.0, 0.0],
    });
    let transform = Affine::translate((-f64::from(tile.x), -f64::from(tile.y)))
        * Affine::scale(f64::from(tile.scale()))
        * rotation
        * page.initial_transform(true).to_kurbo();
    render_pixels(page, cache, tile.size.map(usize::from), transform)
}

fn render_pixels<'a>(
    page: &'a hayro::hayro_syntax::page::Page<'a>,
    cache: &RenderCache<'a>,
    size: [usize; 2],
    transform: hayro::kurbo::Affine,
) -> Result<eframe::egui::ColorImage, String> {
    use eframe::egui::{Color32, ColorImage};
    use hayro::vello_cpu::{PixmapMut, RasterizerSettings, RenderContext, Resources, TargetInit};
    let [w, h] = size.map(|side| side as u16);
    let mut context = RenderContext::new(w, h);
    hayro::render_into(
        page,
        cache,
        &InterpreterSettings::default(),
        &RenderSettings::default(),
        &mut context,
        transform,
    );
    context.flush();
    let mut image = ColorImage::filled(size, Color32::TRANSPARENT);
    let target = PixmapMut::new(w, h, bytemuck::cast_slice_mut(&mut image.pixels))
        .expect("texture buffer matches the validated raster size");
    context.render_with(
        target,
        &mut Resources::default(),
        RasterizerSettings {
            target_init: TargetInit::Clear(WHITE),
            ..Default::default()
        },
    );
    Ok(image)
}

pub fn write_png(bitmap: &Bitmap, path: &Path) -> Result<(), String> {
    let file = std::fs::File::create(path)
        .map_err(|error| format!("Cannot create {}: {error}", path.display()))?;
    encode_png(bitmap, std::io::BufWriter::new(file))
        .map_err(|error| format!("Cannot write {}: {error}", path.display()))
}

fn encode_png(bitmap: &Bitmap, output: impl std::io::Write) -> Result<(), String> {
    let mut encoder = png::Encoder::new(output, bitmap.size[0] as u32, bitmap.size[1] as u32);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    let mut writer = encoder.write_header().map_err(|e| e.to_string())?;
    writer
        .write_image_data(&bitmap.rgba)
        .map_err(|error| error.to_string())?;
    // Drop ignores IEND and buffered-write errors; report them before success.
    writer.finish().map_err(|error| error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn image_output_rejects_the_source_file_and_accepts_distinct_files() {
        let root = tempfile::tempdir().unwrap();
        let source = root.path().join("文档.pdf");
        std::fs::write(&source, b"source PDF").unwrap();
        assert!(check_output_path(&source, &source).is_err());
        assert!(check_output_path(&source, &root.path().join("./文档.pdf")).is_err());
        let output = root.path().join("image.png");
        assert!(check_output_path(&source, &output).is_ok());
        std::fs::write(&output, b"source PDF").unwrap();
        assert!(check_output_path(&source, &output).is_ok());
        let missing = root.path().join("missing.pdf");
        assert!(check_output_path(&missing, &missing).is_ok());
        #[cfg(unix)]
        {
            use std::os::unix::fs::symlink;
            let hard = root.path().join("hard.png");
            std::fs::hard_link(&source, &hard).unwrap();
            assert!(check_output_path(&source, &hard).is_err());
            let link = root.path().join("symlink.png");
            symlink(&source, &link).unwrap();
            assert!(check_output_path(&source, &link).is_err());
            assert!(check_output_path(&link, &source).is_err());
        }
        assert_eq!(std::fs::read(source).unwrap(), b"source PDF");
    }

    #[test]
    fn cancellable_loading_preserves_documents_passwords_and_read_errors() {
        let root = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        let path = root.join("assets/pagewing-guide.pdf");
        let ordinary = load(&path, "").unwrap();
        let interactive = load_cancellable(&path, "", || false).unwrap().unwrap();
        assert_eq!(ordinary.data().as_ref(), interactive.data().as_ref());
        assert_eq!(ordinary.pages().len(), interactive.pages().len());
        let encrypted = root.join("tests/fixtures/encrypted.pdf");
        assert!(load_cancellable(&encrypted, "wrong", || false).is_err());
        assert!(
            load_cancellable(&encrypted, "jkgdi", || false)
                .unwrap()
                .is_some()
        );
        let missing = path.with_extension("missing");
        assert!(load_cancellable(&missing, "", || true).unwrap().is_none());
        assert_eq!(
            load_cancellable(&missing, "", || false).err(),
            load(&missing, "").err()
        );
    }

    #[test]
    fn png_exports_report_final_flush_errors() {
        struct FailsOnFlush;
        impl std::io::Write for FailsOnFlush {
            fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
                Ok(bytes.len())
            }

            fn flush(&mut self) -> std::io::Result<()> {
                Err(std::io::Error::other("export flush failed"))
            }
        }

        let bitmap = Bitmap {
            size: [1, 1],
            rgba: vec![255; 4],
        };
        let error = encode_png(&bitmap, FailsOnFlush).unwrap_err();
        assert!(error.contains("export flush failed"), "{error}");
    }

    #[test]
    fn png_exports_finish_and_preserve_dimensions_and_pixels() {
        let bitmap = Bitmap {
            size: [2, 2],
            rgba: vec![
                255, 0, 0, 255, 0, 255, 0, 255, 0, 0, 255, 255, 12, 34, 56, 255,
            ],
        };
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("page.png");
        write_png(&bitmap, &path).unwrap();
        let bytes = std::fs::read(&path).unwrap();
        assert!(bytes.ends_with(b"\0\0\0\0IEND\xaeB`\x82"));
        let mut reader = png::Decoder::new(std::io::Cursor::new(bytes))
            .read_info()
            .unwrap();
        let mut pixels = vec![0; reader.output_buffer_size().unwrap()];
        let info = reader.next_frame(&mut pixels).unwrap();
        assert_eq!([info.width as usize, info.height as usize], bitmap.size);
        assert_eq!(&pixels[..info.buffer_size()], bitmap.rgba);
    }

    fn sample() -> Pdf {
        Pdf::new(include_bytes!("../assets/pagewing-guide.pdf").to_vec()).unwrap()
    }

    #[test]
    fn viewport_tiles_match_full_resolution_text_crops_and_rotations() {
        for pdf in [
            sample(),
            Pdf::new(include_bytes!("../tests/fixtures/render-crop.pdf").to_vec()).unwrap(),
        ] {
            let cache = RenderCache::new();
            for page in 0..pdf.pages().len() {
                let (w, _) = pdf.pages()[page].render_dimensions();
                for rotation in 0..4 {
                    let key = RenderKey {
                        page,
                        width: (w * 2.0) as u32,
                        rotation,
                        night: false,
                        tile: None,
                    };
                    let full = rasterize(&pdf, &cache, key).unwrap();
                    // Odd, nonzero offsets exercise translated glyphs, image filtering,
                    // PDF CropBox, UserUnit, and intrinsic/reader rotations together.
                    let tile = Tile {
                        scale_bits: 2.0_f32.to_bits(),
                        x: 17,
                        y: 23,
                        size: [
                            (full.size[0] - 34).min(301) as u16,
                            (full.size[1] - 46).min(257) as u16,
                        ],
                    };
                    let image = rasterize_tile(&pdf, &cache, key, tile).unwrap();
                    let mut max_diff = 0;
                    let mut differing = 0;
                    for y in 0..usize::from(tile.size[1]) {
                        for x in 0..usize::from(tile.size[0]) {
                            let expected = &full.rgba[((y + tile.y as usize) * full.size[0]
                                + x
                                + tile.x as usize)
                                * 4..][..4];
                            let actual = image.pixels[y * image.size[0] + x].to_array();
                            for (actual, expected) in actual.iter().zip(expected) {
                                let diff = actual.abs_diff(*expected);
                                max_diff = max_diff.max(diff);
                                differing += usize::from(diff > 3);
                            }
                        }
                    }
                    assert!(
                        max_diff <= 3 || differing * 1000 < image.pixels.len(),
                        "page={page}, rotation={rotation}, max difference={max_diff}, differing channels={differing}"
                    );
                }
            }
        }
    }

    #[test]
    fn direct_texture_targets_match_rasters_for_cropped_rotated_and_translucent_content() {
        for pdf in [
            sample(),
            Pdf::new(include_bytes!("../tests/fixtures/render-crop.pdf").to_vec()).unwrap(),
        ] {
            let cache = RenderCache::new();
            for page in 0..pdf.pages().len() {
                for width in [3, 96, 301] {
                    let expected = rasterize_untransformed(&pdf, &cache, page, width).unwrap();
                    let actual = rasterize_pixels(&pdf, &cache, page, width).unwrap();
                    assert_eq!(actual.size, expected.size);
                    assert_eq!(bytemuck::cast_slice::<_, u8>(&actual.pixels), expected.rgba);
                }
            }
            assert!(rasterize_pixels(&pdf, &cache, usize::MAX, 96).is_err());
        }
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
                tile: None,
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
                    night: false,
                    tile: None,
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
                tile: None,
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
