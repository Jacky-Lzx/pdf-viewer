use eframe::egui::{self, FontData, FontDefinitions, FontFamily};
use std::{path::Path, sync::Arc};

pub fn install(context: egui::Context) {
    // Read large system font collections without delaying the first frame.
    let _ = std::thread::Builder::new()
        .name("folio-fonts".into())
        .spawn(move || {
            let Some(bytes) = system_font() else {
                return;
            };
            let mut fonts = FontDefinitions::default();
            fonts.font_data.insert(
                "system-fallback".into(),
                Arc::new(FontData::from_owned(bytes)),
            );
            for family in [FontFamily::Proportional, FontFamily::Monospace] {
                fonts
                    .families
                    .entry(family)
                    .or_default()
                    .push("system-fallback".into());
            }
            context.set_fonts(fonts);
            context.request_repaint();
        });
}

fn read_font(path: &Path, require_cjk: bool) -> Option<Vec<u8>> {
    if std::fs::metadata(path).ok()?.len() > 128 * 1024 * 1024 {
        return None;
    }
    let bytes = std::fs::read(path).ok()?;
    let face = ttf_parser::Face::parse(&bytes, 0).ok()?;
    if require_cjk && face.glyph_index('中').is_none() {
        return None;
    }
    Some(bytes)
}

fn system_font() -> Option<Vec<u8>> {
    if let Some(path) = std::env::var_os("FOLIO_FONT")
        && let Some(bytes) = read_font(Path::new(&path), false)
    {
        return Some(bytes);
    }
    for path in [
        "/System/Library/Fonts/PingFang.ttc",
        "/System/Library/Fonts/STHeiti Light.ttc",
        "/usr/share/fonts/opentype/noto/NotoSansCJK-Regular.ttc",
        "/usr/share/fonts/noto-cjk/NotoSansCJK-Regular.ttc",
        "/usr/share/fonts/truetype/wqy/wqy-microhei.ttc",
    ] {
        if let Some(bytes) = read_font(Path::new(path), true) {
            return Some(bytes);
        }
    }
    #[cfg(target_os = "linux")]
    {
        let output = std::process::Command::new("fc-match")
            .args(["-s", "-f", "%{file}\n", "sans-serif:lang=zh"])
            .output()
            .ok()?;
        if output.status.success() {
            let paths = String::from_utf8(output.stdout).ok()?;
            let mut seen = std::collections::HashSet::new();
            for path in paths.lines().take(64) {
                if seen.insert(path)
                    && let Some(bytes) = read_font(Path::new(path), true)
                {
                    return Some(bytes);
                }
            }
        }
    }
    None
}
