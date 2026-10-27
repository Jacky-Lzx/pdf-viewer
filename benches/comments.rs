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

use annotations::{Comment, CommentId, Comments};
use comment_cards::{CardTextCache, card_lines};
use comment_view::CommentView;
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
    benchmark();
}

fn benchmark() {
    let passes: usize = std::env::args()
        .nth(1)
        .and_then(|s| s.parse().ok())
        .unwrap_or(9);
    assert!((1..=1000).contains(&passes));
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
