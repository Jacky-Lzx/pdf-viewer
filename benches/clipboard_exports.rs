//! Complete clipboard report construction against the frozen e7a4ff5 exporters.
#[path = "support/allocations.rs"]
mod allocation_counter;
#[allow(dead_code, unused_imports)]
#[path = "../src/annotations.rs"]
mod annotations;
#[allow(dead_code, unused_imports)]
#[path = "../src/bitmap.rs"]
mod bitmap;
#[allow(dead_code, unused_imports)]
#[path = "../src/case_fold.rs"]
mod case_fold;
#[allow(dead_code, unused_imports)]
#[path = "../src/comment_view.rs"]
mod comment_view;
#[allow(dead_code, unused_imports)]
#[path = "../src/document.rs"]
mod document;
#[allow(dead_code, unused_imports)]
#[path = "../src/document_bytes.rs"]
mod document_bytes;
#[allow(dead_code, unused_imports)]
#[path = "../src/export_text.rs"]
mod export_text;
#[allow(dead_code, unused_imports)]
#[path = "../src/pdf_objects.rs"]
mod pdf_objects;
#[allow(dead_code, unused_imports)]
#[path = "../src/pdf_string.rs"]
mod pdf_string;
#[allow(dead_code, unused_imports)]
#[path = "../src/selection_index.rs"]
mod selection_index;
#[allow(dead_code, unused_imports)]
#[path = "../src/text.rs"]
mod text;

#[allow(dead_code, unused_imports)]
#[path = "../src/app/comments/export.rs"]
mod comments_export;
#[path = "support/comments_export_previous.rs"]
mod comments_previous;
#[allow(dead_code, unused_imports)]
#[path = "../src/app/search/export.rs"]
mod search_export;
#[path = "support/search_export_previous.rs"]
mod search_previous;

#[allow(dead_code, unused_imports)]
mod page_labels {
    pub fn fixture(kind: &str) -> PageLabels {
        if kind == "none" {
            return PageLabels {
                count: 10_000,
                ranges: Vec::new(),
            };
        }
        PageLabels {
            count: 10_000,
            ranges: (0..4)
                .map(|index| LabelRange {
                    page: index * 2500,
                    first: 1,
                    prefix: if kind == "Unicode" {
                        // 240 scalars / 480 bytes, within the reader's retention limits.
                        "附录\u{2028}Straße😀".repeat(24)
                    } else {
                        format!("Chapter-{index}-")
                    },
                    style: Style::Decimal,
                })
                .collect(),
        }
    }

    include!("../src/page_labels.rs");
}

use annotations::{Comment, CommentId, Comments, ReviewStatus};
use comment_view::CommentRow;
use page_labels::PageLabels;
use std::{hint::black_box, path::Path, time::Instant};
use text::{SearchHit, SearchOptions};

#[derive(Clone, Copy)]
struct Case {
    count: usize,
    order: &'static str,
    label: &'static str,
    large: bool,
}

struct Fixture {
    hits: Vec<SearchHit>,
    comments: Comments,
    rows: Vec<CommentRow>,
    labels: PageLabels,
}

impl Fixture {
    fn new(case: Case) -> Self {
        let body = if case.large {
            "Review 中文😀\n".repeat(256)
        } else {
            "Review 中文😀\nsecond line".into()
        };
        let author = if case.large {
            "Straße\u{2028}Reviewer ".repeat(32)
        } else {
            "Alice\nReviewer".into()
        };
        let hits: Vec<_> = (0..case.count)
            .map(|index| SearchHit {
                page: match case.order {
                    "same" => 0,
                    "grouped" => index / 80,
                    "sparse" => index,
                    "alternating" => index % 2,
                    _ => unreachable!(),
                },
                snippet: body.clone(),
                rects: Vec::new(),
                snippet_match: 0..0,
            })
            .collect();
        let comments = Comments {
            items: hits
                .iter()
                .enumerate()
                .map(|(index, hit)| Comment {
                    page: hit.page,
                    slot: Some(index),
                    parent: (index % 3 == 1).then_some(CommentId { page: 0, slot: 0 }),
                    review: if index % 5 == 0 {
                        ReviewStatus::Completed
                    } else {
                        ReviewStatus::None
                    },
                    marked: index % 7 == 0,
                    states: Vec::new(),
                    kind: "Text".into(),
                    contents: body.clone(),
                    author: author.clone(),
                    rects: Vec::new(),
                    has_appearance: false,
                    truncated: index % 11 == 0,
                    locked: false,
                })
                .collect(),
            limited: case.large,
        };
        let rows = (0..case.count)
            .map(|index| CommentRow {
                index,
                depth: index % 3,
                matched: index % 3 != 0,
            })
            .collect();
        Self {
            hits,
            comments,
            rows,
            labels: page_labels::fixture(case.label),
        }
    }

    fn run(&self, search: bool, previous: bool) -> (String, usize, usize) {
        let label = |page| black_box(&self.labels).label(page);
        if search {
            // Both snapshot types and the complete exporters are production sources.
            let path = Path::new("/books/Review\n中文.pdf");
            let query = "Straße\u{2028}中文";
            let options = SearchOptions {
                match_case: false,
                whole_word: true,
            };
            if previous {
                let report = search_previous::render(
                    search_previous::Snapshot {
                        path,
                        query,
                        options,
                        hits: black_box(&self.hits),
                        scanned: 10_000,
                        pages: 10_000,
                        complete: true,
                        limited: false,
                    },
                    label,
                );
                (report.text, report.copied, report.total)
            } else {
                let report = search_export::render(
                    search_export::Snapshot {
                        path,
                        query,
                        options,
                        hits: black_box(&self.hits),
                        scanned: 10_000,
                        pages: 10_000,
                        complete: true,
                        limited: false,
                        stopped: false,
                    },
                    label,
                );
                (report.text, report.copied, report.total)
            }
        } else if previous {
            let report =
                comments_previous::render(black_box(&self.comments), black_box(&self.rows), label);
            (report.text, report.copied, report.total)
        } else {
            let report =
                comments_export::render(black_box(&self.comments), black_box(&self.rows), label);
            (report.text, report.copied, report.total)
        }
    }
}

fn sample(fixture: &Fixture, search: bool, previous: bool, iterations: usize) -> f64 {
    let start = Instant::now();
    for _ in 0..iterations {
        // Include the full report and its destruction, not only a byte count.
        drop(black_box(fixture.run(search, previous)));
    }
    start.elapsed().as_secs_f64() * 1_000_000.0 / iterations as f64
}

fn main() {
    let rounds = std::env::args()
        .nth(1)
        .and_then(|arg| arg.parse().ok())
        .unwrap_or(17);
    assert!((1..=1000).contains(&rounds));
    println!(
        "{rounds} alternating rounds; median µs/export, allocations/reallocations. Full output equality checked before timing. Includes real PageLabels lookup, normalization, report construction and drop; excludes fixtures, PDF reading, OS clipboard, UI and GPU."
    );
    for (count, order, label, large) in [
        (1, "same", "ASCII", false),
        (1, "same", "Unicode", false),
        (1, "same", "none", false),
        (80, "grouped", "ASCII", false),
        (80, "sparse", "ASCII", false),
        (80, "alternating", "Unicode", false),
        (80, "grouped", "none", false),
        (10_000, "same", "ASCII", false),
        (10_000, "grouped", "ASCII", false),
        (10_000, "sparse", "ASCII", false),
        (10_000, "alternating", "Unicode", false),
        (10_000, "grouped", "Unicode", false),
        (10_000, "grouped", "none", false),
        (10_000, "sparse", "none", false),
        (10_000, "grouped", "Unicode", true),
    ] {
        let case = Case {
            count,
            order,
            label,
            large,
        };
        let fixture = Fixture::new(case);
        for search in [true, false] {
            let expected = fixture.run(search, true);
            assert_eq!(fixture.run(search, false), expected);
            if large {
                assert!(expected.1 > 0 && expected.1 < expected.2);
                assert!(expected.0.len() <= 8 * 1024 * 1024);
                assert!(expected.0.ends_with("clipboard size limit reached.]\n"));
            } else {
                assert_eq!(expected.1, count);
            }
            let copied = expected.1;
            drop(expected);
            let iterations = if count == 1 {
                2000
            } else if count == 80 {
                100
            } else {
                2
            };
            let mut times = [Vec::new(), Vec::new()];
            for round in 0..rounds {
                for index in if round % 2 == 0 { [0, 1] } else { [1, 0] } {
                    times[index].push(sample(&fixture, search, index == 0, iterations));
                }
            }
            for samples in &mut times {
                samples.sort_by(f64::total_cmp);
            }
            let old = times[0][rounds / 2];
            let new = times[1][rounds / 2];
            let old_alloc =
                allocation_counter::allocations(|| drop(black_box(fixture.run(search, true))));
            let new_alloc =
                allocation_counter::allocations(|| drop(black_box(fixture.run(search, false))));
            println!(
                "{} {count} {order} {label} {}: {old:.3} -> {new:.3} µs ({:.3}x); {old_alloc:?} -> {new_alloc:?}; copied {copied}",
                if search { "search" } else { "comments" },
                if large { "large" } else { "short" },
                old / new
            );
        }
    }
}
