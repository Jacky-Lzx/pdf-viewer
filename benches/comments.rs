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

use annotations::{Comment, CommentId, Comments};
use comment_view::CommentView;
use std::{hint::black_box, time::Instant};

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
}
