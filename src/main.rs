mod app;
mod cache;
mod document;
mod navigation;
mod state;
mod text;
mod worker;

use std::{path::PathBuf, time::Instant};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = std::env::args_os().skip(1).collect();
    if args
        .first()
        .is_some_and(|arg| arg == "--help" || arg == "-h")
    {
        println!(
            "Folio — a keyboard-driven PDF reader\n\nUsage:\n  folio [document.pdf]\n  folio --render document.pdf output.png [page] [width]\n  folio --benchmark document.pdf [iterations]\n  folio --text document.pdf [page]\n\nPress ? inside the reader for Vim shortcuts."
        );
        return Ok(());
    }
    if args
        .first()
        .is_some_and(|arg| arg == "--render" || arg == "--benchmark" || arg == "--text")
    {
        return headless(&args).map_err(Into::into);
    }
    if args
        .first()
        .is_some_and(|arg| arg.to_string_lossy().starts_with('-'))
    {
        return Err("Unknown option. Run folio --help.".into());
    }
    let path = args.first().map(PathBuf::from);
    let options = eframe::NativeOptions {
        viewport: eframe::egui::ViewportBuilder::default()
            .with_inner_size([1180.0, 820.0])
            .with_min_inner_size([860.0, 540.0])
            .with_drag_and_drop(true),
        ..Default::default()
    };
    eframe::run_native(
        "Folio",
        options,
        Box::new(move |cc| Ok(Box::new(app::Folio::new(cc, path)))),
    )?;
    Ok(())
}

fn headless(args: &[std::ffi::OsString]) -> Result<(), String> {
    let path = args.get(1).ok_or("Missing PDF path. Run folio --help.")?;
    let start = Instant::now();
    let pdf = document::load(std::path::Path::new(path), "")?;
    let load_ms = start.elapsed().as_secs_f64() * 1000.0;
    let cache = hayro::RenderCache::new();
    let number = |index: usize, default: usize| -> Result<usize, String> {
        args.get(index)
            .map(|arg| {
                arg.to_string_lossy()
                    .parse::<usize>()
                    .map_err(|_| "Expected a positive number".to_owned())
            })
            .unwrap_or(Ok(default))
    };
    if args[0] == "--text" {
        let page = number(2, 1)?
            .checked_sub(1)
            .ok_or("Page numbers start at 1")?;
        if page >= pdf.pages().len() {
            return Err("Page out of range".into());
        }
        println!(
            "{}",
            text::extract(
                &pdf.pages()[page],
                &hayro::hayro_interpret::InterpreterCache::new()
            )
            .text
        );
    } else if args[0] == "--render" {
        let output = args.get(2).ok_or("Missing output PNG path")?;
        let page = number(3, 1)?
            .checked_sub(1)
            .ok_or("Page numbers start at 1")?;
        let width = number(4, 1200)?;
        if width == 0 || width > 16384 {
            return Err("Width must be between 1 and 16384".into());
        }
        let start = Instant::now();
        let bitmap = document::rasterize(
            &pdf,
            &cache,
            document::RenderKey {
                page,
                width: width as u32,
                rotation: 0,
                night: false,
            },
        )?;
        let elapsed = start.elapsed();
        document::write_png(&bitmap, std::path::Path::new(output))?;
        println!(
            "{} × {} · load {load_ms:.1} ms · render {:.1} ms",
            bitmap.size[0],
            bitmap.size[1],
            elapsed.as_secs_f64() * 1000.0
        );
    } else {
        let iterations = number(2, 3)?;
        if !(1..=1000).contains(&iterations) {
            return Err("Iterations must be between 1 and 1000".into());
        }
        let mut times = Vec::new();
        for _ in 0..iterations {
            for page in 0..pdf.pages().len() {
                let start = Instant::now();
                std::hint::black_box(document::rasterize(
                    &pdf,
                    &cache,
                    document::RenderKey {
                        page,
                        width: 1200,
                        rotation: 0,
                        night: false,
                    },
                )?);
                times.push(start.elapsed().as_secs_f64() * 1000.0);
            }
        }
        times.sort_by(f64::total_cmp);
        println!(
            "{} pages × {iterations} passes · load {load_ms:.1} ms · mean {:.1} ms · median {:.1} ms · p95 {:.1} ms",
            pdf.pages().len(),
            times.iter().sum::<f64>() / times.len() as f64,
            times[times.len() / 2],
            times[((times.len() - 1) as f64 * 0.95).ceil() as usize]
        );
    }
    Ok(())
}
