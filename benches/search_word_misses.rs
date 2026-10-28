#[path = "support/allocations.rs"]
mod allocations;
#[allow(dead_code, unused_imports)]
#[path = "../src/selection_index.rs"]
mod selection_index;
#[allow(dead_code, unused_imports)]
#[path = "../src/text.rs"]
mod text;

use std::{hint::black_box, time::Instant};
use text::{PageText, SearchCursor, SearchOptions, SearchQuery};

fn search(page: &PageText, query: &SearchQuery, expected: usize) {
    let batch = page.search_batch(0, query, &mut SearchCursor::default(), 256);
    assert!(batch.complete);
    assert_eq!(batch.hits.len(), expected);
    black_box(batch);
}

fn main() {
    let passes: usize = std::env::args()
        .nth(1)
        .and_then(|value| value.parse().ok())
        .unwrap_or(9);
    assert!((1..=1000).contains(&passes));
    println!(
        "{passes} passes; medians per cold whole-word search; case-fold index prepared, word index cold; includes matching, word indexing, snippets and batch release; excludes page cloning, extraction, geometry, GPU and UI."
    );
    for (kind, row, repeats, suffix, needle, expected, searches) in [
        ("Empty", "", 0, "", "missing", 0, 1000),
        (
            "ASCII miss",
            "Rust PDF reader, built for long documents.\n",
            50,
            "",
            "selection snapshots",
            0,
            200,
        ),
        (
            "Large ASCII miss",
            "Rust PDF reader, built for long documents.\n",
            5000,
            "",
            "selection snapshots",
            0,
            20,
        ),
        (
            "CJK miss",
            "中文阅读器，跨平台运行。\n",
            50,
            "",
            "不存在",
            0,
            200,
        ),
        (
            "Mixed Unicode miss",
            "中文阅读器 Straße Σσς 👩‍💻 café\n",
            50,
            "",
            "不存在 ss",
            0,
            200,
        ),
        (
            "ASCII hit at end",
            "Rust PDF reader, built for long documents.\n",
            5000,
            "selection snapshots",
            "selection snapshots",
            1,
            20,
        ),
        (
            "Unicode hit at end",
            "中文阅读器 Straße Σσς 👩‍💻 café\n",
            5000,
            "STRASSE FINALEWORD",
            "strasse finaleword",
            1,
            20,
        ),
    ] {
        let mut template = PageText::default();
        template.text = row.repeat(repeats) + suffix;
        // Warm only case folding, leaving the Unicode word index cold.
        search(
            &template,
            &SearchQuery::new("absent-preparation-query".into(), SearchOptions::default()),
            0,
        );
        let query = SearchQuery::new(
            needle.into(),
            SearchOptions {
                match_case: false,
                whole_word: true,
            },
        );
        let counted = template.clone();
        let before = counted.memory_bytes();
        let counts = allocations::allocations(|| search(&counted, &query, expected));
        let added = counted.memory_bytes() - before;
        let mut times = Vec::with_capacity(passes);
        for _ in 0..passes {
            let pages: Vec<_> = (0..searches).map(|_| template.clone()).collect();
            let start = Instant::now();
            for page in &pages {
                search(black_box(page), black_box(&query), expected);
            }
            times.push(start.elapsed().as_secs_f64() * 1_000_000.0 / searches as f64);
        }
        times.sort_by(f64::total_cmp);
        println!(
            "{kind}, {} bytes: {:.4} us/search; alloc/realloc {counts:?}; word index +{added} bytes",
            template.text.len(),
            times[passes / 2],
        );
    }
}
