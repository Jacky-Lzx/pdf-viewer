#[path = "support/allocations.rs"]
mod allocation_counter;
#[allow(dead_code, unused_imports)]
#[path = "../src/case_fold.rs"]
mod case_fold;

use std::{hint::black_box, time::Instant};

fn main() {
    let passes: usize = std::env::args()
        .nth(1)
        .and_then(|value| value.parse().ok())
        .unwrap_or(11);
    assert!((1..=1000).contains(&passes));
    println!(
        "{passes} alternating passes; median microseconds/fold (100 folds/sample). Standard default_case_fold_str versus shared allocating fold; allocation and release included, results observed through black_box, no PDF parsing, indexing, matching, UI or GPU."
    );
    for (kind, text) in [
        (
            "short ASCII",
            "CHAPTER 12345: PDF reading and navigation".into(),
        ),
        (
            "long ASCII",
            "RUST PDF READER AND DOCUMENT NAVIGATION ".repeat(128),
        ),
        ("short mixed", "Chapter 12345 — Straße ΟΣ 中文".into()),
        ("long mixed", "Straße ΟΣ 中文阅读器 ".repeat(128)),
        ("long CJK", "中文阅读器跨平台文件目录章節標題".repeat(128)),
        ("long Greek", "Σσς ΟΣ ος ΆΈΉΊΌΎΏ ".repeat(128)),
        ("expanding", "İßﬃẞﬁﬄ ſK ".repeat(128)),
    ] {
        let expected = caseless::default_case_fold_str(&text);
        assert_eq!(case_fold::fold(&text), expected);
        let mut scratch = String::new();
        case_fold::fold_into(&text, &mut scratch);
        assert_eq!(scratch, expected);
        let mut samples: [Vec<f64>; 2] = std::array::from_fn(|_| Vec::with_capacity(passes));
        for pass in 0..passes {
            for offset in 0..2 {
                let mode = (pass + offset) % 2;
                let start = Instant::now();
                for _ in 0..100 {
                    let result = if mode == 0 {
                        caseless::default_case_fold_str(black_box(&text))
                    } else {
                        case_fold::fold(black_box(&text))
                    };
                    black_box(&result);
                }
                samples[mode].push(start.elapsed().as_secs_f64() * 10_000.0);
            }
        }
        let times = samples.map(|mut values| {
            values.sort_by(f64::total_cmp);
            values[passes / 2]
        });
        let old_alloc = allocation_counter::allocations(|| {
            black_box(caseless::default_case_fold_str(&text));
        });
        let new_alloc = allocation_counter::allocations(|| {
            black_box(case_fold::fold(&text));
        });
        let reuse_alloc = allocation_counter::allocations(|| {
            case_fold::fold_into(&text, &mut scratch);
            black_box(&scratch);
        });
        assert_eq!(reuse_alloc, (0, 0));
        println!(
            "{kind}: {} bytes -> {} bytes; standard {:.4}, shared {:.4} us/fold; (alloc,realloc) {old_alloc:?} -> {new_alloc:?}; warm scratch {reuse_alloc:?}; capacity {} -> {}",
            text.len(),
            expected.len(),
            times[0],
            times[1],
            expected.capacity(),
            case_fold::fold(&text).capacity()
        );
    }
}
