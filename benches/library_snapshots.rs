#[path = "support/allocations.rs"]
mod allocation_counter;
#[allow(dead_code, unused_imports)]
#[path = "../src/navigation.rs"]
mod navigation;
#[allow(dead_code, unused_imports)]
#[path = "../src/selection_index.rs"]
mod selection_index;
#[allow(dead_code, unused_imports)]
#[path = "../src/settings.rs"]
mod settings;
#[allow(dead_code, unused_imports)]
#[path = "../src/state.rs"]
mod state;
#[allow(dead_code, unused_imports)]
#[path = "../src/text.rs"]
mod text;

use state::{Library, ReadingMark, ReadingState};
use std::{
    hint::black_box,
    time::{Duration, Instant},
};

// Preserve the previous owned representation, including all preference fields.
#[derive(Clone, serde::Serialize)]
struct PreviousLibrary {
    recent: Vec<ReadingState>,
    dark: bool,
    night: bool,
    sidebar: bool,
    thumbnails: bool,
    search_options: text::SearchOptions,
    display: settings::DisplaySettings,
    shortcuts: settings::Shortcuts,
}

impl PreviousLibrary {
    fn new(library: &Library) -> Self {
        Self {
            recent: library
                .recent
                .iter()
                .map(|record| record.as_ref().clone())
                .collect(),
            dark: library.dark,
            night: library.night,
            sidebar: library.sidebar,
            thumbnails: library.thumbnails,
            search_options: library.search_options,
            display: library.display.clone(),
            shortcuts: library.shortcuts.clone(),
        }
    }

    fn remember(&mut self, state: ReadingState) {
        self.recent.retain(|item| item.path != state.path);
        self.recent.insert(0, state);
        self.recent.truncate(24);
    }
}

fn measure<T>(iterations: usize, mut snapshot: impl FnMut() -> T) -> f64 {
    let mut elapsed = Duration::ZERO;
    for _ in 0..iterations {
        let start = Instant::now();
        let value = black_box(snapshot());
        elapsed += start.elapsed();
        // A completed snapshot normally belongs to the background writer.
        // Exclude its eventual release from UI preparation timings.
        drop(value);
    }
    elapsed.as_secs_f64() * 1_000_000.0 / iterations as f64
}

fn median(mut values: Vec<f64>) -> f64 {
    values.sort_by(f64::total_cmp);
    values[values.len() / 2]
}

fn main() {
    let passes: usize = std::env::args()
        .nth(1)
        .and_then(|value| value.parse().ok())
        .unwrap_or(9);
    assert!((1..=1000).contains(&passes));
    println!(
        "{passes} alternating passes; µs/snapshot. Clone-only and full save preparation (clone active record, remember it, clone library), using production records/preferences. Per-operation timing excludes snapshot drop, setup, correctness checks, JSON, IPC, filesystem, PDF, GPU and UI rendering."
    );
    for (documents, bookmarks, iterations) in [
        (1, 0, 2000),
        (24, 24, 1000),
        (24, 600, 200),
        (24, 10_000, 25),
        (4, 100_000, 10),
    ] {
        let mut library = Library::default();
        for index in 0..documents {
            library.remember(ReadingState {
                path: format!("/books/文档-{index}.pdf").into(),
                page: 7,
                bookmarks: (0..bookmarks).collect(),
                marks: [(
                    'a',
                    ReadingMark {
                        page: 3,
                        fit: navigation::Fit::Custom(1.75),
                        offset: [0.4, 0.6],
                        ..Default::default()
                    },
                )]
                .into(),
                ..Default::default()
            });
        }
        let mut previous = PreviousLibrary::new(&library);
        let current = library.recent[0].as_ref().clone();
        let original = serde_json::to_value(&library).unwrap();
        assert_eq!(serde_json::to_value(&previous).unwrap(), original);
        for preparation in [false, true] {
            let old_allocations = allocation_counter::allocations(|| {
                if preparation {
                    previous.remember(current.clone());
                }
                black_box(previous.clone());
            });
            let new_allocations = allocation_counter::allocations(|| {
                if preparation {
                    library.remember(current.clone());
                }
                black_box(library.clone());
            });
            let mut times = [Vec::with_capacity(passes), Vec::with_capacity(passes)];
            for pass in 0..passes {
                for order in 0..2 {
                    let mode = (pass + order) % 2;
                    let elapsed = if mode == 0 {
                        measure(iterations, || {
                            if preparation {
                                previous.remember(black_box(&current).clone());
                            }
                            previous.clone()
                        })
                    } else {
                        measure(iterations, || {
                            if preparation {
                                library.remember(black_box(&current).clone());
                            }
                            library.clone()
                        })
                    };
                    times[mode].push(elapsed);
                }
            }
            assert_eq!(serde_json::to_value(&previous).unwrap(), original);
            assert_eq!(serde_json::to_value(&library).unwrap(), original);
            let [old, new] = times.map(median);
            println!(
                "{documents} documents × {bookmarks} bookmarks, {}: {old:.4} -> {new:.4} µs; (alloc,realloc) {old_allocations:?} -> {new_allocations:?}",
                if preparation {
                    "save preparation"
                } else {
                    "clone only"
                }
            );
        }
    }
}
