#[allow(dead_code, unused_imports)]
#[path = "../src/selection_index.rs"]
mod selection_index;
#[allow(dead_code, unused_imports)]
#[path = "../src/text.rs"]
mod text;

use hayro::kurbo::Rect;
use std::{hint::black_box, time::Instant};
use text::{PageText, SearchCursor, SearchOptions, SearchQuery, TextSpan};

fn search(page: &PageText, query: &SearchQuery) -> (usize, usize) {
    let mut cursor = SearchCursor::default();
    let (mut count, mut checksum) = (0, 0);
    loop {
        let batch = page.search_batch(0, query, &mut cursor, 256);
        count += batch.hits.len();
        for hit in &batch.hits {
            checksum += hit.snippet.len() + hit.rects.len();
        }
        if batch.complete || count >= 10_000 {
            return (count, checksum);
        }
    }
}

fn main() {
    let passes: usize = std::env::args()
        .nth(1)
        .and_then(|value| value.parse().ok())
        .unwrap_or(9);
    assert!((1..=1000).contains(&passes));
    println!(
        "{passes} passes; full search batches include matching, ranges, snippets, allocation and release; warmed text/word indices, one query reused across pages; no extraction, cache eviction, GPU or UI."
    );
    for (kind, row, repeats, needle, whole_word, searches, expected) in [
        (
            "ASCII hit",
            "Rust PDF reader, built for long documents.\n",
            50,
            "documents",
            false,
            200,
            50,
        ),
        (
            "ASCII miss",
            "Rust PDF reader, built for long documents.\n",
            50,
            "selection snapshots",
            false,
            2000,
            0,
        ),
        (
            "Large miss",
            "Rust PDF reader, built for long documents.\n",
            5000,
            "selection snapshots",
            false,
            200,
            0,
        ),
        (
            "One byte",
            "Rust PDF reader, built for long documents.\n",
            50,
            "r",
            false,
            200,
            200,
        ),
        (
            "Unicode hit",
            "中文阅读器 Straße Σσς 👩‍💻 café\n",
            50,
            "阅读器",
            false,
            200,
            50,
        ),
        ("Rejected words", "ba a ", 5000, "a a", true, 200, 0),
    ] {
        let mut page = PageText::default();
        page.text = row.repeat(repeats);
        page.spans = page
            .text
            .char_indices()
            .enumerate()
            .map(|(index, (start, ch))| {
                let x = (index % 80) as f64 * 6.0;
                let y = (index / 80) as f64 * 12.0;
                TextSpan {
                    range: start..start + ch.len_utf8(),
                    rect: Rect::new(x, y, x + 6.0, y + 10.0),
                }
            })
            .collect();
        let query = SearchQuery::new(
            needle.into(),
            SearchOptions {
                match_case: false,
                whole_word,
            },
        );
        let reference = search(&page, &query);
        assert_eq!(reference.0, expected, "{kind}");
        let mut times = Vec::with_capacity(passes);
        for _ in 0..passes {
            let start = Instant::now();
            for _ in 0..searches {
                let actual = search(black_box(&page), black_box(&query));
                assert_eq!(actual, reference);
                black_box(actual);
            }
            times.push(start.elapsed().as_secs_f64() * 1000.0);
        }
        times.sort_by(f64::total_cmp);
        println!(
            "{kind}, {} bytes, {searches} searches: {:.4} ms; hits {} checksum {}",
            page.text.len(),
            times[passes / 2],
            reference.0,
            reference.1
        );
    }
}
