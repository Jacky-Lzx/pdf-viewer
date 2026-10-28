#[allow(dead_code, unused_imports)]
#[path = "../src/annotations.rs"]
mod annotations;
#[allow(dead_code, unused_imports)]
#[path = "../src/bitmap.rs"]
mod bitmap;
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
#[path = "../src/pdf_string.rs"]
mod pdf_string;

#[allow(dead_code, unused_imports)]
#[path = "../src/comment_cards.rs"]
mod comment_cards;
#[allow(dead_code, unused_imports)]
#[path = "../src/fifo_cache.rs"]
mod fifo_cache;

use annotations::{Comment, CommentId, Comments, ReviewStatus};
use comment_cards::{CardTextCache, card_lines};
use comment_view::{CommentFilter, CommentView};
use std::{hint::black_box, time::Instant};
#[path = "support/allocations.rs"]
mod allocation_counter;
use allocation_counter::allocations;

fn measure(passes: usize, mut run: impl FnMut() -> usize) -> f64 {
    let mut samples: Vec<_> = (0..passes)
        .map(|_| {
            let start = Instant::now();
            for _ in 0..100 {
                black_box(run());
            }
            start.elapsed().as_secs_f64() * 10_000.0
        })
        .collect();
    samples.sort_by(f64::total_cmp);
    samples[samples.len() / 2]
}

fn main() {
    let passes: usize = std::env::args()
        .nth(1)
        .and_then(|s| s.parse().ok())
        .unwrap_or(9);
    assert!((1..=1000).contains(&passes));
    match std::env::args().nth(2).as_deref() {
        Some("filters") => benchmark_filters(passes),
        Some("threads") => benchmark_threads(passes),
        _ => {
            benchmark_threads(passes);
            benchmark_filters(passes);
            benchmark(passes);
        }
    }
}

fn apply_filter(view: &mut CommentView, comments: &Comments, filter: &CommentFilter) -> usize {
    // Model edits of an existing input field without charging each operation
    // for constructing the fixture's query string.
    view.filter.query.clone_from(&filter.query);
    view.filter.review = filter.review;
    view.filter.marked = filter.marked;
    view.filter.current_page = filter.current_page;
    assert!(view.update(comments, comments.items.len() / 10 - 1));
    black_box(view.rows()).len()
}

fn benchmark_filters(passes: usize) {
    println!(
        "Changed filters: {passes} passes, median µs/update, 100 updates/sample; warm Unicode search cache, real matching, ancestor retention, counts and row/selection updates. Includes query folding and allocations; excludes PDF parsing, widgets and GPU."
    );
    for count in [80, 10_000] {
        let comments = Comments {
            items: (0..count)
                .map(|index| Comment {
                    page: index / 10,
                    slot: Some(index % 10),
                    parent: (index % 10 != 0).then(|| CommentId {
                        page: index / 10,
                        slot: index % 10 - 1,
                    }),
                    review: match index % 10 {
                        0 => ReviewStatus::Completed,
                        9 => ReviewStatus::Accepted,
                        _ => ReviewStatus::None,
                    },
                    marked: index % 10 == 9,
                    states: Vec::new(),
                    kind: "Text".into(),
                    contents: format!(
                        "Review {index}: Straße 中文 {}",
                        if index % 10 == 9 {
                            "needle"
                        } else {
                            "ancestor"
                        }
                    ),
                    author: "Pagewing".into(),
                    rects: Vec::new(),
                    has_appearance: false,
                    truncated: false,
                    locked: false,
                })
                .collect(),
            limited: false,
        };
        let all = CommentFilter::default();
        let groups = [
            (
                "text",
                [
                    (
                        CommentFilter {
                            query: "STRASSE".into(),
                            ..all.clone()
                        },
                        count,
                        count,
                    ),
                    (
                        CommentFilter {
                            query: "pagewing needle".into(),
                            ..all.clone()
                        },
                        count / 10,
                        count,
                    ),
                    (
                        CommentFilter {
                            query: "missing".into(),
                            ..all.clone()
                        },
                        0,
                        0,
                    ),
                    (all.clone(), count, count),
                ],
            ),
            (
                "review",
                [
                    (
                        CommentFilter {
                            review: Some(ReviewStatus::Accepted),
                            ..all.clone()
                        },
                        count / 10,
                        count,
                    ),
                    (
                        CommentFilter {
                            review: Some(ReviewStatus::Completed),
                            ..all.clone()
                        },
                        count / 10,
                        count / 10,
                    ),
                    (
                        CommentFilter {
                            review: Some(ReviewStatus::Rejected),
                            ..all.clone()
                        },
                        0,
                        0,
                    ),
                    (all.clone(), count, count),
                ],
            ),
            (
                "page/marked",
                [
                    (
                        CommentFilter {
                            current_page: true,
                            ..all.clone()
                        },
                        10,
                        10,
                    ),
                    (
                        CommentFilter {
                            marked: true,
                            ..all.clone()
                        },
                        count / 10,
                        count,
                    ),
                    (
                        CommentFilter {
                            current_page: true,
                            marked: true,
                            ..all.clone()
                        },
                        1,
                        10,
                    ),
                    (all, count, count),
                ],
            ),
        ];
        for (name, cases) in groups {
            let mut view = CommentView::default();
            view.set_comments(&comments);
            for (filter, matches, shown) in &cases {
                assert_eq!(apply_filter(&mut view, &comments, filter), *shown);
                assert_eq!(view.matches(), *matches);
                assert_eq!(
                    view.rows().iter().filter(|row| row.matched).count(),
                    *matches
                );
            }
            let allocations = allocations(|| {
                for (filter, _, _) in &cases {
                    black_box(apply_filter(&mut view, &comments, filter));
                }
            });
            if cases.iter().all(|(filter, _, _)| filter.query.is_empty()) {
                assert_eq!(
                    allocations,
                    (0, 0),
                    "warm non-text filters must not allocate"
                );
            }
            let mut next = 0;
            let time = measure(passes, || {
                let filter = &cases[next % cases.len()].0;
                next += 1;
                apply_filter(&mut view, &comments, filter)
            });
            println!(
                "  {count} {name}: {time:.3} µs/update; (alloc,realloc) per four changed filters {allocations:?}"
            );
        }
    }
}

fn benchmark(passes: usize) {
    println!(
        "{passes} passes; median µs/operation, 100 operations/sample; real thread ordering and page overlay lookup. Prepared comments; no PDF parsing, widgets, text layout or GPU."
    );
    for count in [80, 10_000] {
        let comments = Comments {
            items: (0..count)
                .map(|index| Comment {
                    page: index / 10,
                    slot: Some(index % 10),
                    parent: (index % 10 != 0).then(|| CommentId {
                        page: index / 10,
                        slot: index % 10 - 1,
                    }),
                    review: Default::default(),
                    marked: false,
                    states: Vec::new(),
                    kind: "Highlight".into(),
                    contents: format!("Review {index}: Straße 中文"),
                    author: "Pagewing".into(),
                    rects: vec![hayro::kurbo::Rect::new(0., 0., 10., 10.)],
                    has_appearance: true,
                    truncated: false,
                    locked: false,
                })
                .collect(),
            limited: false,
        };
        let mut view = CommentView::default();
        let start = Instant::now();
        view.set_comments(&comments);
        view.update(&comments, 0);
        println!(
            "{count} comments: prepare {:.1} µs",
            start.elapsed().as_secs_f64() * 1e6
        );
        let expected = comments.thread_rows();
        assert_eq!(
            view.rows()
                .iter()
                .map(|row| (row.index, row.depth))
                .collect::<Vec<_>>(),
            expected
        );
        let before = measure(passes, || black_box(&comments).thread_rows().len());
        let after = measure(passes, || {
            assert!(!black_box(&mut view).update(black_box(&comments), 0));
            black_box(view.rows()).len()
        });
        println!("  redraw navigation: rebuild {before:.3}, cached {after:.3}");
        let page = count / 10 - 1;
        let scan = || {
            comments
                .items
                .iter()
                .enumerate()
                .filter_map(|(index, c)| (c.page == page && c.parent.is_none()).then_some(index))
                .collect::<Vec<_>>()
        };
        assert_eq!(view.overlays(page), scan());
        let before = measure(passes, || {
            let page = black_box(page);
            black_box(&comments)
                .items
                .iter()
                .filter(|c| c.page == page && c.parent.is_none())
                .count()
        });
        let after = measure(passes, || black_box(&view).overlays(black_box(page)).len());
        println!("  page overlays: scan {before:.3}, indexed {after:.3}");
    }
    println!(
        "Card text: 12 visible cards/frame, 3000 frames/sample, alternating rebuild/cache order. Includes preview, author, page label and status formatting/allocation; excludes widgets, text layout, PDF parsing and GPU."
    );
    for long in [false, true] {
        let comments: Vec<_> = (0..12)
            .map(|index| Comment {
                page: index,
                slot: Some(0),
                parent: None,
                review: annotations::ReviewStatus::Completed,
                marked: true,
                states: Vec::new(),
                kind: "Highlight".into(),
                contents: if long {
                    "中文📝 Review\n".repeat(5000)
                } else {
                    format!("Review {index}: Straße 中文")
                },
                author: if long {
                    "审阅者📝 ".repeat(1000)
                } else {
                    "Pagewing".into()
                },
                rects: Vec::new(),
                has_appearance: true,
                truncated: false,
                locked: false,
            })
            .collect();
        benchmark_cards(
            passes,
            if long {
                "long Unicode"
            } else {
                "short mixed text"
            },
            &comments,
        );
    }
}

fn rebuilt_frame(comments: &[Comment], context: bool) -> usize {
    comments
        .iter()
        .map(|comment| {
            let lines = card_lines(
                black_box(comment),
                format!("iv · {}", comment.page + 1),
                !context,
            );
            black_box(&lines).iter().map(String::len).sum::<usize>()
        })
        .sum()
}

fn cached_frame(
    comments: &[Comment],
    cache: &mut CardTextCache,
    context: bool,
    first: usize,
) -> usize {
    comments
        .iter()
        .enumerate()
        .map(|(index, comment)| {
            let lines = cache.get_or_insert_with(first + index, !context, || {
                card_lines(comment, format!("iv · {}", comment.page + 1), !context)
            });
            black_box(lines).iter().map(String::len).sum::<usize>()
        })
        .sum()
}

fn benchmark_cards(passes: usize, name: &str, comments: &[Comment]) {
    let mut cache = CardTextCache::default();
    // Model a sidebar that has already been scrolled: warm-hit timings must
    // include the cache's full-capacity lookup path, not only a fresh table.
    for index in 0..128 {
        let comment = &comments[index % comments.len()];
        cache.get_or_insert_with(1000 + index, true, || {
            card_lines(comment, "iv · 1".into(), true)
        });
    }
    for context in [false, true] {
        let expected = rebuilt_frame(comments, context);
        assert_eq!(cached_frame(comments, &mut cache, context, 0), expected);
    }
    let before = allocations(|| {
        black_box(rebuilt_frame(comments, false));
    });
    let after = allocations(|| {
        black_box(cached_frame(comments, &mut cache, false, 0));
    });
    assert_eq!(after, (0, 0), "warm card text must not allocate");
    let mut samples = [Vec::new(), Vec::new()];
    for pass in 0..passes {
        for mode in if pass % 2 == 0 { [0, 1] } else { [1, 0] } {
            let start = Instant::now();
            for frame in 0..3000 {
                let checksum = if mode == 0 {
                    rebuilt_frame(comments, frame % 2 == 0)
                } else {
                    cached_frame(comments, &mut cache, frame % 2 == 0, 0)
                };
                black_box(checksum);
            }
            samples[mode].push(start.elapsed().as_secs_f64() * 1e6 / 3000.0);
        }
    }
    for times in &mut samples {
        times.sort_by(f64::total_cmp);
    }
    let rebuilt = samples[0][passes / 2];
    let cached = samples[1][passes / 2];
    let mut first = 0;
    let churn = measure(passes, || {
        first += comments.len();
        cached_frame(comments, &mut cache, false, first)
    });
    println!(
        "  {name}: rebuild {rebuilt:.3} µs/frame, cached {cached:.3}, scrolling misses {churn:.3}; allocation/reallocation calls per warm frame {before:?} -> {after:?}"
    );
}

// Captured allocation-heavy implementation from 3fc3fb1.
fn previous_thread_rows(comments: &Comments) -> Vec<(usize, usize)> {
    use std::collections::HashMap;
    let indices: HashMap<_, _> = comments
        .items
        .iter()
        .enumerate()
        .filter_map(|(i, c)| Some((c.id()?, i)))
        .collect();
    let mut children = vec![Vec::new(); comments.items.len()];
    let mut roots = Vec::new();
    for (index, comment) in comments.items.iter().enumerate() {
        if let Some(parent) = comment
            .parent
            .and_then(|p| indices.get(&p))
            .copied()
            .filter(|p| *p != index)
        {
            children[parent].push(index);
        } else {
            roots.push(index);
        }
    }
    let mut rows = Vec::with_capacity(comments.items.len());
    let mut visited = vec![false; comments.items.len()];
    for root in roots.into_iter().chain(0..comments.items.len()) {
        let mut stack = vec![(root, 0)];
        while let Some((index, depth)) = stack.pop() {
            if std::mem::replace(&mut visited[index], true) {
                continue;
            }
            rows.push((index, depth));
            stack.extend(
                children[index]
                    .iter()
                    .rev()
                    .map(|&child| (child, depth + 1)),
            );
        }
    }
    rows
}

fn benchmark_threads(passes: usize) {
    println!(
        "Thread preparation: {passes} alternating passes, 100 operations/sample; µs/preorder including ID lookup, adjacency construction, traversal and output release. Prepared comments; excludes PDF parsing, text, filters, UI and GPU."
    );
    for count in [0, 1, 80, 10_000] {
        for shape in ["flat", "chains", "wide", "cycle"] {
            let comments = Comments {
                items: (0..count)
                    .map(|index| Comment {
                        page: 0,
                        slot: Some(index),
                        parent: match shape {
                            "chains" if index % 10 != 0 => Some(index - 1),
                            "wide" if index > 0 => Some(0),
                            "cycle" => Some(if index == 0 { count - 1 } else { index - 1 }),
                            _ => None,
                        }
                        .map(|slot| CommentId { page: 0, slot }),
                        review: Default::default(),
                        marked: false,
                        states: Vec::new(),
                        kind: "Text".into(),
                        contents: String::new(),
                        author: String::new(),
                        rects: Vec::new(),
                        has_appearance: false,
                        truncated: false,
                        locked: false,
                    })
                    .collect(),
                limited: false,
            };
            let expected = previous_thread_rows(&comments);
            assert_eq!(comments.thread_rows(), expected);
            assert_eq!(expected.len(), count);
            let old_allocations = allocations(|| {
                black_box(previous_thread_rows(&comments));
            });
            let new_allocations = allocations(|| {
                black_box(comments.thread_rows());
            });
            let mut times = [Vec::with_capacity(passes), Vec::with_capacity(passes)];
            for pass in 0..passes {
                for order in 0..2 {
                    let mode = (pass + order) % 2;
                    let start = Instant::now();
                    for _ in 0..100 {
                        black_box(if mode == 0 {
                            previous_thread_rows(black_box(&comments))
                        } else {
                            black_box(&comments).thread_rows()
                        });
                    }
                    times[mode].push(start.elapsed().as_secs_f64() * 1e6 / 100.0);
                }
            }
            let [old, new] = times.map(|mut values| {
                values.sort_by(f64::total_cmp);
                values[values.len() / 2]
            });
            assert_eq!(comments.thread_rows(), expected);
            println!(
                "  {count} {shape}: {old:.4} -> {new:.4} µs; (alloc,realloc) {old_allocations:?} -> {new_allocations:?}"
            );
        }
    }
}
