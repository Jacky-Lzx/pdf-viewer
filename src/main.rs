mod annotations;
mod app;
mod bitmap;
mod cache;
mod cli;
mod continuous;
mod destination;
mod document;
mod document_bytes;
mod document_info;
mod file_watch;
mod fonts;
mod geometry;
mod history;
mod link_cache;
mod link_hints;
mod link_visibility;
mod links;
mod navigation;
mod outline;
mod outline_view;
mod output;
mod page_labels;
mod pdf_objects;
mod pdf_string;
mod recent;
mod render_plan;
mod selection;
mod selection_index;
mod settings;
mod state;
mod state_writer;
mod synctex;
mod synctex_ipc;
mod text;
mod text_cache;
mod texture_pixels;
mod thumbnails;
mod worker;

use std::{fmt::Write as _, time::Instant};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = std::env::args_os().skip(1).collect();
    let (path, page, sync_request) = match cli::parse(&args)? {
        cli::Command::Help => {
            output::line(format_args!(
                "Pagewing — a keyboard-driven PDF reader\n\nUsage:\n  pagewing [document.pdf]\n  pagewing -- [document.pdf]\n  pagewing --page page [--] document.pdf\n  pagewing [--inverse-search COMMAND] [--synctex-directory DIR]\n        --forward-search LINE COLUMN SOURCE [--] document.pdf\n  pagewing --inverse-search COMMAND [--] document.pdf\n  pagewing [--no-focus] [--inverse-search-position COMMAND]\n        --synctex-location JSON [--] document.pdf\n  pagewing --render document.pdf output.png [page] [width]\n  pagewing --benchmark document.pdf [iterations]\n  pagewing --text document.pdf [page]\n  pagewing --info document.pdf\n  pagewing --version\n\nPage numbers start at 1. Width: 1–16384 pixels. Iterations: 1–1000.\n--page selects a physical starting page; values beyond the document open its last page.\n--info reports document details without opening a window.\nSyncTeX: compile with -synctex=1. Lines start at 1; use column 0 if unknown.\nInverse command placeholders: %f file, %l line, %c column, %% literal percent.\nPosition callback placeholders: %f PDF, %p page, %x/%y coordinates.\n--synctex-location accepts an LSP location {{page,x,y,h?,v?,width?,height?}}; no SyncTeX query.\n--no-focus opens/reuses the window without requesting focus.\nCtrl/Cmd+click the PDF to jump to source (outside selection mode).\nUse -- before a GUI filename starting with '-'.\nPress ? inside the reader for Vim shortcuts."
            ))?;
            return Ok(());
        }
        cli::Command::Version => {
            output::line(format_args!("Pagewing {}", env!("CARGO_PKG_VERSION")))?;
            return Ok(());
        }
        cli::Command::Gui { path, page } => (path, page, None),
        cli::Command::SyncGui {
            path,
            source,
            target,
            mut options,
        } => {
            options.resolve_paths()?;
            let path = std::fs::canonicalize(&path)
                .map_err(|e| format!("Cannot read {}: {e}", path.display()))?;
            let target = target.or(source
                .as_ref()
                .map(|source| synctex::forward(&path, source, &options))
                .transpose()?);
            let request = synctex_ipc::Request { target, options };
            if synctex_ipc::send_existing(&path, &request)? {
                return Ok(());
            }
            (Some(path), None, Some(request))
        }
        command => return headless(command).map_err(Into::into),
    };
    let options = eframe::NativeOptions {
        viewport: eframe::egui::ViewportBuilder::default()
            .with_active(
                sync_request
                    .as_ref()
                    .is_none_or(|r| r.options.focus != Some(false)),
            )
            .with_inner_size([1180.0, 820.0])
            .with_min_inner_size([240.0, 240.0])
            .with_icon(eframe::icon_data::from_png_bytes(include_bytes!(
                "../assets/icon.png"
            ))?)
            .with_drag_and_drop(true),
        ..Default::default()
    };
    eframe::run_native(
        "Pagewing",
        options,
        Box::new(move |cc| {
            let mut app = app::Pagewing::new(cc, path, page);
            if let Some(request) = sync_request {
                app.configure_synctex(request, &cc.egui_ctx);
            }
            Ok(Box::new(app))
        }),
    )?;
    Ok(())
}

fn headless(command: cli::Command) -> Result<(), String> {
    let path = command.path().ok_or("Missing PDF path")?;
    if let cli::Command::Render { output, .. } = &command {
        document::check_output_path(path, output)?;
    }
    let start = Instant::now();
    let pdf = document::load(path, "")?;
    let load_ms = start.elapsed().as_secs_f64() * 1000.0;
    match command {
        cli::Command::Info { path } => {
            let info = document_info::DocumentInfo::read(&pdf);
            let mut report = format!(
                "File: {}\nFormat: PDF {}\nSize: {} bytes\nPages: {}",
                path.display(),
                info.version,
                info.bytes,
                pdf.pages().len()
            );
            for (label, value) in info.fields {
                let _ = write!(report, "\n{label}: {value}");
            }
            output::line(format_args!("{report}"))?;
        }
        cli::Command::Text { page, .. } => {
            if page >= pdf.pages().len() {
                return Err("Page out of range".into());
            }
            output::line(format_args!(
                "{}",
                text::extract(
                    &pdf.pages()[page],
                    &hayro::hayro_interpret::InterpreterCache::new()
                )
                .text
            ))?;
        }
        cli::Command::Render {
            output,
            page,
            width,
            ..
        } => {
            let cache = hayro::RenderCache::new();
            let start = Instant::now();
            let bitmap = document::rasterize(
                &pdf,
                &cache,
                document::RenderKey {
                    page,
                    width,
                    rotation: 0,
                    night: false,
                    tile: None,
                },
            )?;
            let elapsed = start.elapsed();
            document::write_png(&bitmap, &output)?;
            output::line(format_args!(
                "{} × {} · load {load_ms:.1} ms · render {:.1} ms",
                bitmap.size[0],
                bitmap.size[1],
                elapsed.as_secs_f64() * 1000.0
            ))?;
        }
        cli::Command::Benchmark { iterations, .. } => {
            let cache = hayro::RenderCache::new();
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
                            tile: None,
                        },
                    )?);
                    times.push(start.elapsed().as_secs_f64() * 1000.0);
                }
            }
            times.sort_by(f64::total_cmp);
            output::line(format_args!(
                "{} pages × {iterations} passes · load {load_ms:.1} ms · mean {:.1} ms · median {:.1} ms · p95 {:.1} ms",
                pdf.pages().len(),
                times.iter().sum::<f64>() / times.len() as f64,
                times[times.len() / 2],
                times[((times.len() - 1) as f64 * 0.95).ceil() as usize]
            ))?;
        }
        _ => return Err("Expected a document export command".into()),
    }
    Ok(())
}
