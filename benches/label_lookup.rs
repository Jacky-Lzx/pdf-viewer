#[path = "support/allocations.rs"]
mod allocation_counter;
#[allow(dead_code, unused_imports)]
#[path = "../src/case_fold.rs"]
mod case_fold;
#[allow(dead_code, unused_imports)]
#[path = "../src/pdf_objects.rs"]
mod pdf_objects;
#[allow(dead_code, unused_imports)]
#[path = "../src/pdf_string.rs"]
mod pdf_string;

#[allow(dead_code, unused_imports)]
mod page_labels {
    include!("../src/page_labels.rs");

    pub mod previous {
        include!("support/label_lookup_previous.rs");
    }

    pub fn fixture(count: usize, kind: &str) -> PageLabels {
        PageLabels {
            count: count * 4,
            ranges: (0..count)
                .map(|index| LabelRange {
                    page: index * 4,
                    first: 1,
                    style: Style::Decimal,
                    prefix: match kind {
                        "ASCII" => format!("Chapter-{index}-"),
                        "Unicode" if index + 1 == count => "Straße-".into(),
                        "Unicode" => format!("附录-{index}-"),
                        "long unrelated" => format!("附录{}-{index}-", "章节😀".repeat(70)),
                        "long shared" => format!("{}-{index}-", "章节Straße-".repeat(24)),
                        _ => unreachable!(),
                    },
                })
                .collect(),
        }
    }
}

use page_labels::{PageLabels, previous::Previous};
use std::{hint::black_box, time::Instant};

fn batch(labels: &PageLabels, query: &str, count: usize, iterations: usize, old: bool) {
    for _ in 0..iterations {
        let result = if old {
            Previous(black_box(labels)).resolve_input(black_box(query), count)
        } else {
            black_box(labels).resolve_input(black_box(query), count)
        };
        black_box(result);
    }
}

fn main() {
    let rounds: usize = std::env::args()
        .nth(1)
        .and_then(|value| value.parse().ok())
        .unwrap_or(17);
    assert!((1..=1000).contains(&rounds));
    let filter = std::env::args().nth(2);
    println!(
        "{rounds} alternating rounds; µs/query. Includes lookup, folding, number validation and temporary allocations/drop. Excludes prepared fixtures, PDF parsing, UI and GPU."
    );
    for (ranges, kind, query_kind) in [
        (1, "ASCII", "exact late"),
        (1, "Unicode", "folded late"),
        (80, "ASCII", "folded late"),
        (10_000, "ASCII", "exact late"),
        (10_000, "ASCII", "folded late"),
        (10_000, "Unicode", "exact early"),
        (10_000, "Unicode", "folded late"),
        (10_000, "long unrelated", "missing"),
        (10_000, "long shared", "exact late"),
        (10_000, "long shared", "folded late"),
        (10_000, "long shared", "missing shared"),
        (10_000, "long shared", "physical"),
    ] {
        if filter
            .as_ref()
            .is_some_and(|filter| !format!("{kind} {query_kind}").contains(filter))
        {
            continue;
        }
        let labels = page_labels::fixture(ranges, kind);
        let count = ranges * 4;
        let last = count - 3;
        let query = match query_kind {
            "exact early" => labels.label(1).unwrap(),
            "exact late" => labels.label(last).unwrap(),
            "folded late" => case_fold::fold(&labels.label(last).unwrap()),
            "missing" => "no-such-label".into(),
            "missing shared" => format!("{}!", case_fold::fold(&labels.label(last).unwrap())),
            "physical" => "123".into(),
            _ => unreachable!(),
        };
        let expected = match query_kind {
            "exact early" => Some(1),
            "exact late" | "folded late" => Some(last),
            "physical" => Some(122),
            _ => None,
        };
        assert_eq!(labels.resolve_input(&query, count), expected);
        assert_eq!(Previous(&labels).resolve_input(&query, count), expected);
        let iterations = if ranges == 1 || query_kind == "physical" || query_kind == "exact early" {
            10_000
        } else if kind == "long shared" {
            2
        } else if ranges == 10_000 {
            20
        } else {
            1_000
        };
        for old in [true, false] {
            batch(&labels, &query, count, 2, old);
        }
        let mut times: [Vec<f64>; 2] = std::array::from_fn(|_| Vec::new());
        for round in 0..rounds {
            for order in 0..2 {
                let mode = (round + order) % 2;
                let start = Instant::now();
                batch(&labels, &query, count, iterations, mode == 0);
                times[mode].push(start.elapsed().as_secs_f64() * 1e6 / iterations as f64);
            }
        }
        let medians = times.map(|mut samples| {
            samples.sort_by(f64::total_cmp);
            samples[samples.len() / 2]
        });
        let allocations = [true, false]
            .map(|old| allocation_counter::allocations(|| batch(&labels, &query, count, 1, old)));
        println!(
            "{ranges:5} ranges, {kind}, {query_kind}: {:.4} -> {:.4}; (alloc,realloc) {allocations:?}",
            medians[0], medians[1]
        );
    }
}
