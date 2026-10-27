#[allow(dead_code, unused_imports)]
#[path = "../src/selection_index.rs"]
mod selection_index;
#[allow(dead_code, unused_imports)]
#[path = "../src/text.rs"]
mod text;
#[allow(dead_code, unused_imports)]
#[path = "../src/text_cache.rs"]
mod text_cache;

use std::{hint::black_box, sync::Arc, time::Instant};
use text::{PageText, SearchCursor, SearchOptions, SearchQuery};
use text_cache::TextCache;

fn page(bytes: usize, prepare: bool) -> Arc<PageText> {
    let mut page = PageText::default();
    page.text = "x".repeat(bytes);
    if prepare {
        let query = SearchQuery::new("z".into(), SearchOptions::default());
        assert!(
            page.search_batch(0, &query, &mut SearchCursor::default(), 100)
                .complete
        );
    }
    Arc::new(page)
}

fn filled(pages: &[Arc<PageText>]) -> TextCache {
    let mut cache = TextCache::default();
    for (index, page) in pages.iter().enumerate() {
        assert!(cache.insert(index, Arc::clone(page)));
    }
    // Check the entry/byte budgets admitted all pages; then make page 0 newest.
    for index in 0..pages.len() {
        assert!(cache.get(index).is_some());
    }
    assert!(cache.get(0).is_some());
    cache
}

fn main() {
    let passes: usize = std::env::args()
        .nth(1)
        .and_then(|value| value.parse().ok())
        .unwrap_or(9);
    assert!((1..=1000).contains(&passes));
    println!(
        "{passes} passes; 64 insertions/sample or 32768 hits; cache population, payload preparation, survivor validation and cache drop excluded; real PageText accounting, held payload references, no PDF extraction, rendering or UI."
    );
    let pages: Vec<_> = (0..512).map(|_| page(32500, false)).collect();
    for kind in ["hit", "single", "few", "batch"] {
        let incoming = page(
            match kind {
                "few" => 128000,
                "batch" => 12 * 1024 * 1024,
                _ => 32500,
            },
            true,
        );
        let mut samples = Vec::with_capacity(passes);
        let mut checksum = 0;
        let mut removed = 0;
        for _ in 0..passes {
            if kind == "hit" {
                let mut cache = filled(&pages);
                let start = Instant::now();
                for index in 0..32768 {
                    let text = cache.get(black_box(index % 512)).unwrap();
                    checksum = black_box(text.text.len());
                }
                samples.push(start.elapsed().as_secs_f64() * 1000.0);
                continue;
            }
            let mut caches: Vec<_> = (0..64).map(|_| filled(&pages)).collect();
            let start = Instant::now();
            for cache in &mut caches {
                assert!(black_box(cache).insert(512, Arc::clone(black_box(&incoming))));
            }
            samples.push(start.elapsed().as_secs_f64() * 1000.0);
            for cache in &mut caches {
                assert!(Arc::ptr_eq(&cache.get(512).unwrap(), &incoming));
                let survivors: Vec<_> = (0..512)
                    .filter(|&index| cache.get(index).is_some())
                    .collect();
                assert_eq!(survivors[0], 0, "the touched page must survive");
                assert!(
                    survivors[1..].iter().copied().eq(survivors[1]..512),
                    "evict oldest first"
                );
                removed = 512 - survivors.len();
                match kind {
                    "single" => assert_eq!(removed, 1),
                    "few" => assert!((2..=8).contains(&removed)),
                    "batch" => assert!(removed >= 128),
                    _ => unreachable!(),
                }
                checksum = survivors.iter().sum::<usize>() + incoming.text.len();
            }
        }
        samples.sort_by(f64::total_cmp);
        println!(
            "{kind}: {:.4} ms, removed {removed}, checksum {checksum}",
            samples[passes / 2]
        );
    }
}
