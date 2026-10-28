#[path = "support/allocations.rs"]
mod allocation_counter;
#[allow(dead_code)]
#[path = "../src/app/help/catalog.rs"]
mod catalog;
#[allow(dead_code)]
#[path = "../src/help_catalog.rs"]
mod help_catalog;

use catalog::SHORTCUTS;
use help_catalog::HelpCatalog;
use std::{hint::black_box, time::Instant};

// Preserve 2b26e79's query-time formatting and matching over the same rows.
struct Baseline {
    entries: Vec<(String, String)>,
    visible: Vec<usize>,
}

impl Baseline {
    fn filter(&mut self, query: &str) {
        self.visible.clear();
        let query = query.to_ascii_lowercase();
        let words: Vec<_> = query.split_whitespace().collect();
        self.visible
            .extend(
                self.entries
                    .iter()
                    .enumerate()
                    .filter_map(|(index, (keys, action))| {
                        let text = format!("{keys} {action}").to_ascii_lowercase();
                        words
                            .iter()
                            .all(|word| text.contains(word))
                            .then_some(index)
                    }),
            );
    }

    fn rows(&self) -> impl Iterator<Item = (&str, &str)> + '_ {
        self.visible.iter().map(|&index| {
            let (keys, action) = &self.entries[index];
            (keys.as_str(), action.as_str())
        })
    }
}

fn checksum<'a>(rows: impl Iterator<Item = (&'a str, &'a str)>) -> usize {
    rows.fold(0, |sum: usize, (keys, action)| {
        sum.wrapping_add(keys.len()).wrapping_add(action.len())
    })
}

fn compare_rows(baseline: &Baseline, current: &HelpCatalog) {
    assert_eq!(
        baseline.rows().collect::<Vec<_>>(),
        current.rows().collect::<Vec<_>>()
    );
}

fn main() {
    let passes: usize = std::env::args()
        .nth(1)
        .and_then(|value| value.parse().ok())
        .unwrap_or(9);
    assert!((1..=1000).contains(&passes));
    let rows: Vec<_> = SHORTCUTS
        .iter()
        .map(|(keys, action)| ((*keys).to_owned(), (*action).to_owned()))
        .collect();
    let mut baseline = Baseline {
        entries: rows.clone(),
        visible: Vec::new(),
    };
    let mut current = HelpCatalog::default();
    current.replace(rows.into_iter());
    baseline.filter("");
    current.filter("");
    let text_bytes: usize = SHORTCUTS
        .iter()
        .map(|(keys, action)| keys.len() + 1 + action.len())
        .sum();
    println!(
        "{passes} alternating passes, 1000 refilters/sample, {} actual default help rows; {text_bytes} bytes of cached matching text (contents only). Includes query folding, matching, reused result indices and checksum reads; excludes catalog preparation, copied help text, widgets and GPU.",
        current.total()
    );
    for query in [
        "",
        "COPY LINK",
        "zoom 0",
        "jkgmissing",
        "  document ctrl \t",
        "percentage",
        "50%",
    ] {
        baseline.filter(query);
        current.filter(query);
        compare_rows(&baseline, &current);
        let mut times: [Vec<f64>; 2] = std::array::from_fn(|_| Vec::new());
        for pass in 0..passes {
            for order in 0..2 {
                let mode = (pass + order) % 2;
                let start = Instant::now();
                for _ in 0..1000 {
                    if mode == 0 {
                        baseline.filter(black_box(query));
                        black_box(checksum(baseline.rows()));
                    } else {
                        current.filter(black_box(query));
                        black_box(checksum(current.rows()));
                    }
                }
                times[mode].push(start.elapsed().as_secs_f64() * 1000.0);
            }
        }
        let medians = times.map(|mut samples| {
            samples.sort_by(f64::total_cmp);
            samples[samples.len() / 2]
        });
        let before = allocation_counter::allocations(|| {
            baseline.filter(query);
            black_box(checksum(baseline.rows()));
        });
        let after = allocation_counter::allocations(|| {
            current.filter(query);
            black_box(checksum(current.rows()));
        });
        compare_rows(&baseline, &current);
        println!(
            "{query:?}, {} matches: median µs/filter {:.3} -> {:.3}; (alloc,realloc) {before:?} -> {after:?}",
            current.len(),
            medians[0],
            medians[1]
        );
    }

    // A replacement snapshot must discard old matching text and row indices,
    // while retaining the original ASCII-only case matching for Unicode text.
    for rows in [
        vec![("Ctrl+X".into(), "Straße ΩΣ 中文".into())],
        vec![("Alt+Y".into(), "Different action".into())],
        Vec::new(),
    ] {
        baseline.entries = rows.clone();
        current.replace(rows.into_iter());
        for query in [
            "",
            "ctrl straße",
            "strasse",
            "ωσ",
            "ΩΣ",
            "中文",
            "alt action",
        ] {
            baseline.filter(query);
            current.filter(query);
            compare_rows(&baseline, &current);
        }
    }
}
