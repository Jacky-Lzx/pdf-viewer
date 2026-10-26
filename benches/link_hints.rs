#[path = "../src/link_hints.rs"]
mod link_hints;

use link_hints::{LinkHints, label};
use std::{hint::black_box, time::Instant};

// Original per-key label scan, retained only for comparative measurement.
#[derive(Default)]
struct Baseline {
    prefix: String,
}

impl Baseline {
    fn press(&mut self, letter: char, count: usize) -> Option<usize> {
        self.prefix.push(letter);
        let mut matching = false;
        for index in 0..count {
            let label = label(index, count);
            if label == self.prefix {
                self.prefix.clear();
                return Some(index);
            }
            matching |= label.starts_with(&self.prefix);
        }
        if !matching {
            self.prefix.clear();
        }
        None
    }
}

fn main() {
    let passes: usize = std::env::args()
        .nth(1)
        .and_then(|value| value.parse().ok())
        .unwrap_or(9);
    assert!((1..=1000).contains(&passes));
    println!(
        "Alternating {passes} passes; each sample types every link label; medians exclude input-label generation and UI drawing."
    );
    for count in [26, 100, 676, 1000] {
        let labels: Vec<_> = (0..count).map(|index| label(index, count)).collect();
        let mut times = [Vec::with_capacity(passes), Vec::with_capacity(passes)];
        for pass in 0..passes {
            for order in 0..2 {
                let improved = (pass + order) % 2;
                let mut current = LinkHints::default();
                let mut baseline = Baseline::default();
                let start = Instant::now();
                let mut checksum = 0;
                for (index, label) in labels.iter().enumerate() {
                    if improved == 1 {
                        current.start(black_box(count));
                    }
                    let mut selected = None;
                    for letter in label.chars() {
                        selected = if improved == 1 {
                            current.press(black_box(letter))
                        } else {
                            baseline.press(black_box(letter), black_box(count))
                        };
                    }
                    assert_eq!(
                        selected,
                        Some(index),
                        "both implementations must resolve every link identically"
                    );
                    checksum += selected.unwrap();
                }
                black_box(checksum);
                times[improved].push(start.elapsed().as_secs_f64() * 1000.0);
            }
        }
        times
            .iter_mut()
            .for_each(|samples| samples.sort_by(f64::total_cmp));
        let old = times[0][passes / 2];
        let new = times[1][passes / 2];
        println!(
            "{count} labels · baseline {old:.4} ms · current {new:.4} ms · {:.2}×",
            old / new
        );
    }
}
