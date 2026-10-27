//! SyncTeX queries use unrotated, top-left page coordinates in PDF points.
use hayro::hayro_interpret::TransformExt;
use hayro::kurbo::{Affine, Point, Rect};
use serde::{Deserialize, Serialize};
use std::{
    ffi::OsString,
    path::{Path, PathBuf},
    process::Command,
};

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Options {
    /// Already tokenized: source paths are substituted only after tokenization.
    pub editor: Option<Vec<String>>,
    pub directory: Option<PathBuf>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Source {
    pub path: PathBuf,
    pub line: usize,
    pub column: usize,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Target {
    pub page: usize,
    pub rect: [f64; 4],
}

impl Target {
    pub fn valid(&self) -> bool {
        self.page < 1_000_000
            && self.rect.iter().all(|v| v.is_finite() && v.abs() < 1e9)
            && self.rect[0] <= self.rect[2]
            && self.rect[1] <= self.rect[3]
    }
}

#[derive(Clone, Copy, Debug)]
pub struct PageGeometry {
    to_render: Affine,
}

impl PageGeometry {
    pub fn new(page: &hayro::hayro_syntax::page::Page<'_>) -> Self {
        let media = page.media_box();
        Self {
            to_render: page.initial_transform(true).to_kurbo()
                * Affine::new([1.0, 0.0, 0.0, -1.0, media.x0, media.y1]),
        }
    }

    pub fn render_rect(self, rect: [f64; 4]) -> Rect {
        self.to_render
            .transform_rect_bbox(Rect::new(rect[0], rect[1], rect[2], rect[3]))
    }

    pub fn source_point(self, point: Point) -> Point {
        self.to_render.inverse() * point
    }
}

pub fn editor_command(value: &str) -> Result<Vec<String>, String> {
    let parts = shlex::split(value).ok_or("Unclosed quote in inverse-search command")?;
    if parts.first().is_none_or(|part| part.is_empty()) || !parts.iter().any(|p| p.contains("%f")) {
        return Err("Inverse-search command needs a program and %f (source file); use %l and %c for line and column".into());
    }
    Ok(parts)
}

fn absolute(path: &Path) -> Result<PathBuf, String> {
    if path.is_absolute() {
        Ok(path.into())
    } else {
        std::env::current_dir()
            .map(|cwd| cwd.join(path))
            .map_err(|e| e.to_string())
    }
}

impl Options {
    pub fn resolve_paths(&mut self) -> Result<(), String> {
        if let Some(path) = &mut self.directory {
            *path = absolute(path)?;
        }
        Ok(())
    }
}

fn query(mode: &str, input: OsString, pdf: &Path, options: &Options) -> Result<String, String> {
    let executable = std::env::var_os("FOLIO_SYNCTEX").unwrap_or_else(|| {
        // Finder-launched macOS applications often have no TeX installation in PATH.
        if cfg!(target_os = "macos") && Path::new("/Library/TeX/texbin/synctex").is_file() {
            "/Library/TeX/texbin/synctex".into()
        } else {
            "synctex".into()
        }
    });
    let mut command = Command::new(executable);
    command
        .arg(mode)
        .arg(if mode == "view" { "-i" } else { "-o" })
        .arg(input);
    if mode == "view" {
        command.arg("-o").arg(pdf);
    }
    if let Some(dir) = &options.directory {
        command.arg("-d").arg(dir);
    }
    // SyncTeX otherwise executes these environment variables through a shell.
    let result = command
        .env_remove("SYNCTEX_EDITOR")
        .env_remove("SYNCTEX_VIEWER")
        .output()
        .map_err(|e| {
            format!("Cannot run synctex: {e}. Install TeX Live/MacTeX or set FOLIO_SYNCTEX.")
        })?;
    if !result.status.success() {
        return Err(format!(
            "SyncTeX query failed: {}",
            String::from_utf8_lossy(&result.stderr).trim()
        ));
    }
    String::from_utf8(result.stdout).map_err(|_| "SyncTeX returned non-UTF-8 output".into())
}

/// Only consume protocol records, never diagnostic messages or partial records.
fn records(output: &str) -> Vec<Vec<(&str, &str)>> {
    let mut active = false;
    let mut result = Vec::new();
    for line in output.lines() {
        if line == "SyncTeX result begin" {
            active = true;
        } else if line == "SyncTeX result end" {
            break;
        } else if active && let Some((key, value)) = line.split_once(':') {
            if key == "Output" {
                result.push(Vec::new());
            }
            if let Some(record) = result.last_mut() {
                record.push((key, value));
            }
        }
    }
    result
}

fn field<'a>(record: &[(&str, &'a str)], key: &str) -> Option<&'a str> {
    record.iter().find_map(|(k, v)| (*k == key).then_some(*v))
}

fn parse_target(output: &str) -> Option<Target> {
    records(output).into_iter().find_map(|record| {
        let page = field(&record, "Page")?
            .parse::<usize>()
            .ok()?
            .checked_sub(1)?;
        let number = |key| field(&record, key)?.parse::<f64>().ok();
        let x = number("h").or_else(|| number("x"))?;
        let baseline = number("v").or_else(|| number("y"))?;
        let width = number("W").unwrap_or(8.0).max(1.0);
        let height = number("H").unwrap_or(12.0).max(1.0);
        let target = Target {
            page,
            rect: [x, baseline - height, x + width, baseline],
        };
        target.valid().then_some(target)
    })
}

pub fn forward(pdf: &Path, source: &Source, options: &Options) -> Result<Target, String> {
    let mut input = OsString::from(format!("{}:{}:", source.line, source.column));
    input.push(absolute(&source.path)?);
    parse_target(&query("view", input, &absolute(pdf)?, options)?)
        .ok_or_else(|| "No SyncTeX forward match. Compile with -synctex=1 and keep the .synctex.gz beside the PDF (or use --synctex-directory).".into())
}

pub fn reverse(pdf: &Path, page: usize, point: Point, options: &Options) -> Result<Source, String> {
    if !point.x.is_finite() || !point.y.is_finite() {
        return Err("Invalid SyncTeX coordinates".into());
    }
    let pdf = absolute(pdf)?;
    let mut input = OsString::from(format!("{}:{}:{}:", page + 1, point.x, point.y));
    input.push(&pdf);
    let output = query("edit", input, &pdf, options)?;
    for record in records(&output) {
        let Some(path) = field(&record, "Input").filter(|s| !s.is_empty()) else {
            continue;
        };
        let Some(line) = field(&record, "Line")
            .and_then(|s| s.parse::<usize>().ok())
            .filter(|l| *l > 0)
        else {
            continue;
        };
        let column = field(&record, "Column")
            .and_then(|s| s.parse::<usize>().ok())
            .unwrap_or(1)
            .max(1);
        let path = PathBuf::from(path);
        let path = if path.is_absolute() {
            path
        } else {
            pdf.parent().unwrap_or(Path::new(".")).join(path)
        };
        return Ok(Source { path, line, column });
    }
    Err("No SyncTeX reverse match. Compile with -synctex=1 and keep the .synctex.gz beside the PDF (or use --synctex-directory).".into())
}

pub fn launch_editor(parts: &[String], source: &Source) -> Result<(), String> {
    // Substitute in one pass: a percent sign in the source path is literal data.
    let expand = |part: &str| {
        let mut output = OsString::new();
        let mut chars = part.chars();
        while let Some(c) = chars.next() {
            if c != '%' {
                output.push(c.to_string());
                continue;
            }
            match chars.next() {
                Some('f') => output.push(&source.path),
                Some('l') => output.push(source.line.to_string()),
                Some('c') => output.push(source.column.to_string()),
                Some('%') => output.push("%"),
                Some(c) => {
                    output.push("%");
                    output.push(c.to_string());
                }
                None => output.push("%"),
            }
        }
        output
    };
    let (program, args) = parts
        .split_first()
        .ok_or("No inverse-search editor configured")?;
    let mut child = Command::new(expand(program))
        .args(args.iter().map(|a| expand(a)))
        .spawn()
        .map_err(|e| format!("Cannot start inverse-search editor: {e}"))?;
    std::thread::spawn(move || {
        let _ = child.wait();
    });
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn protocol_uses_first_match_box_baseline_and_one_based_pages() {
        let result = "banner\nSyncTeX result begin\nOutput:/tmp/文档 file.pdf\nPage:2\nx:70\ny:210\nh:60\nv:220\nW:200\nH:14\nOutput:other\nPage:3\nx:0\ny:0\nSyncTeX result end\n";
        let target = parse_target(result).unwrap();
        assert_eq!(target.page, 1);
        assert_eq!(target.rect, [60.0, 206.0, 260.0, 220.0]);
        assert!(parse_target("Page:1\nx:10\ny:10").is_none());
        assert!(
            parse_target("SyncTeX result begin\nOutput:x\nPage:0\nx:1\ny:1\nSyncTeX result end")
                .is_none()
        );
        assert!(
            parse_target("SyncTeX result begin\nOutput:x\nPage:1\nx:NaN\ny:1\nSyncTeX result end")
                .is_none()
        );
    }

    #[test]
    fn page_coordinates_roundtrip_crop_and_intrinsic_rotation() {
        let pdf = hayro::hayro_syntax::Pdf::new(
            include_bytes!("../tests/fixtures/render-crop.pdf").to_vec(),
        )
        .unwrap();
        for page in pdf.pages().iter() {
            let geometry = PageGeometry::new(page);
            for point in [Point::new(30.0, 60.0), Point::new(200.0, 300.0)] {
                let back = geometry.source_point(geometry.to_render * point);
                assert!((back - point).hypot() < 1e-6);
            }
        }
    }

    #[test]
    fn editor_is_tokenized_before_source_substitution() {
        assert_eq!(
            editor_command("code --goto '%f:%l:%c'").unwrap(),
            ["code", "--goto", "%f:%l:%c"]
        );
        assert!(editor_command("code --goto '%f").is_err());
        assert!(editor_command("code --goto").is_err());
    }
}
