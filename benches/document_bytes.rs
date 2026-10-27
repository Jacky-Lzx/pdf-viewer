#[allow(unused_imports)]
#[path = "../src/document_bytes.rs"]
mod document_bytes;

use std::{fs::File, hint::black_box, sync::Mutex, time::Instant};

fn main() {
    let passes: usize = std::env::args()
        .nth(1)
        .and_then(|value| value.parse().ok())
        .unwrap_or(9);
    assert!((1..=1000).contains(&passes));
    let file = tempfile::NamedTempFile::new().unwrap();
    let pending = Mutex::new(false);
    let current = || {
        let mut reader = File::open(file.path()).unwrap();
        let length = reader.metadata().unwrap().len();
        document_bytes::read(&mut reader, length, || *pending.lock().unwrap())
            .unwrap()
            .unwrap()
    };
    println!(
        "{passes} alternating passes; warm filesystem cache, metadata, allocation, reading and drop; cancellation checks use a mutex like the worker; no PDF parsing, rendering or UI."
    );
    for (length, repeats) in [(8192, 200), (1024 * 1024, 50), (32 * 1024 * 1024, 3)] {
        let data: Vec<_> = (0..length).map(|index| (index % 251) as u8).collect();
        std::fs::write(file.path(), &data).unwrap();
        assert_eq!(current(), data);
        assert_eq!(std::fs::read(file.path()).unwrap(), data);
        let mut times = [Vec::with_capacity(passes), Vec::with_capacity(passes)];
        for pass in 0..passes {
            for order in 0..2 {
                let implementation = (pass + order) % 2;
                let start = Instant::now();
                for _ in 0..repeats {
                    let bytes = if implementation == 0 {
                        std::fs::read(black_box(file.path())).unwrap()
                    } else {
                        current()
                    };
                    assert_eq!(bytes.len(), length);
                    black_box(bytes);
                }
                times[implementation].push(start.elapsed().as_secs_f64() * 1000.0 / repeats as f64);
            }
        }
        for samples in &mut times {
            samples.sort_by(f64::total_cmp);
        }
        println!(
            "{length} bytes: fs::read {:.4} ms, cancellable {:.4} ms per file",
            times[0][passes / 2],
            times[1][passes / 2]
        );
    }
}
