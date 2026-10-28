#[path = "support/allocations.rs"]
mod allocation_counter;
#[allow(dead_code, unused_imports)]
#[path = "../src/page_labels.rs"]
mod page_labels;
#[allow(dead_code, unused_imports)]
#[path = "../src/pdf_objects.rs"]
mod pdf_objects;
#[allow(dead_code, unused_imports)]
#[path = "../src/pdf_string.rs"]
mod pdf_string;

#[path = "support/page_labels.rs"]
mod label_fixture;

use label_fixture::fixture;
use page_labels::PageLabels;
use std::{fmt::Write, hint::black_box, time::Instant};

fn baseline(labels: &PageLabels, page: usize, preview: bool) -> String {
    if preview {
        let mut caption = format!("Page {}", page + 1);
        if let Some(label) = labels.label(page)
            && !label.is_empty()
            && label != caption["Page ".len()..]
        {
            let end = label
                .char_indices()
                .nth(12)
                .map_or(label.len(), |(end, _)| end);
            caption.reserve(" · ".len() + end + '…'.len_utf8());
            caption.push_str(" · ");
            caption.extend(label[..end].chars().map(|character| {
                if character.is_whitespace() {
                    ' '
                } else {
                    character
                }
            }));
            if end < label.len() {
                caption.push('…');
            }
        }
        caption
    } else {
        let physical = (page + 1).to_string();
        match labels.label(page) {
            Some(mut label) if !label.is_empty() && label != physical => {
                if let Some((end, _)) = label.char_indices().nth(12) {
                    label.truncate(end);
                    label.push('…');
                }
                let _ = write!(label, " · {}", page + 1);
                label
            }
            _ => physical,
        }
    }
}

fn batch(labels: &PageLabels, preview: bool, old: bool) {
    for page in 0..12 {
        black_box(if old {
            baseline(labels, page, preview)
        } else if preview {
            labels.sidebar_caption(page)
        } else {
            labels.display_name(page)
        });
    }
}

fn main() {
    let passes: usize = std::env::args()
        .nth(1)
        .and_then(|value| value.parse().ok())
        .unwrap_or(9);
    assert!((1..=1000).contains(&passes));
    println!(
        "{passes} alternating passes, 5000 batches/sample, 12 page names/batch. Includes formatting, allocations and drop; excludes PDF loading, extraction, layout, frame setup and GPU."
    );
    for (name, labels) in [
        ("no labels", PageLabels::default()),
        ("physical decimal", fixture("", "D")),
        ("short prefix", fixture("A-", "D")),
        ("Roman", fixture("", "r")),
        ("12-character prefix", fixture("Appendix A: ", "D")),
        ("long ASCII", fixture(&"Appendix ".repeat(32), "D")),
        ("long Unicode", fixture(&"中文😀".repeat(80), "D")),
    ] {
        for preview in [false, true] {
            for page in 0..12 {
                assert_eq!(
                    baseline(&labels, page, preview),
                    if preview {
                        labels.sidebar_caption(page)
                    } else {
                        labels.display_name(page)
                    }
                );
            }
            let mut times: [Vec<f64>; 2] = std::array::from_fn(|_| Vec::new());
            for pass in 0..passes {
                for order in 0..2 {
                    let mode = (pass + order) % 2;
                    let start = Instant::now();
                    for _ in 0..5000 {
                        batch(black_box(&labels), preview, mode == 0);
                    }
                    times[mode].push(start.elapsed().as_secs_f64() * 200.0);
                }
            }
            let medians = times.map(|mut samples| {
                samples.sort_by(f64::total_cmp);
                samples[samples.len() / 2]
            });
            let allocations: Vec<_> = [true, false]
                .into_iter()
                .map(|old| allocation_counter::allocations(|| batch(&labels, preview, old)))
                .collect();
            println!(
                "{name}, {}: median µs/batch {:.3} -> {:.3}; (alloc,realloc) {allocations:?}",
                if preview { "preview" } else { "display" },
                medians[0],
                medians[1]
            );
        }
    }
}
