#[path = "../src/pdf_string.rs"]
mod pdf_string;

use std::{hint::black_box, time::Instant};

fn baseline(bytes: &[u8], limit: usize) -> (String, bool) {
    let mut text = pdf_string::decode(bytes);
    let end = text.char_indices().nth(limit).map(|(end, _)| end);
    if let Some(end) = end {
        text.truncate(end);
    }
    (text, end.is_some())
}

fn utf16(text: &str) -> Vec<u8> {
    let mut bytes = vec![0xfe, 0xff];
    bytes.extend(text.encode_utf16().flat_map(u16::to_be_bytes));
    bytes
}

fn utf8(text: &str) -> Vec<u8> {
    let mut bytes = vec![0xef, 0xbb, 0xbf];
    bytes.extend_from_slice(text.as_bytes());
    bytes
}

fn main() {
    let passes = std::env::args()
        .nth(1)
        .and_then(|value| value.parse::<usize>().ok())
        .unwrap_or(9);
    assert!((1..=1000).contains(&passes));
    let cases = [
        (
            "ASCII title",
            b"Chapter 4. Performance and concurrency".to_vec(),
            2048,
        ),
        ("UTF-16 title", utf16("第四章：跨平台阅读器 😀"), 2048),
        ("UTF-8 title", utf8("第四章：跨平台阅读器 😀"), 2048),
        ("UTF-16 within limit", utf16(&"中".repeat(1500)), 2048),
        ("UTF-8 within limit", utf8(&"中".repeat(1500)), 2048),
        ("long ASCII", b"Chapter 4. Performance ".repeat(1500), 2048),
        (
            "long PDFDocEncoding",
            b"Chapter\x84\xa0 ".repeat(3000),
            2048,
        ),
        (
            "long UTF-16 Latin",
            utf16(&"Chapter 4. ".repeat(2000)),
            2048,
        ),
        (
            "long UTF-16 mixed",
            utf16(&"第四章 😀 𝄞 ".repeat(2000)),
            2048,
        ),
        ("long UTF-8 Latin", utf8(&"Chapter 4. ".repeat(2000)), 2048),
        ("long UTF-8 mixed", utf8(&"第四章 😀 𝄞 ".repeat(2000)), 2048),
        (
            "long invalid UTF-8",
            [
                b"\xef\xbb\xbf".as_slice(),
                &b"a\xff\xe2\x82\xe4\xb8\xad".repeat(2000),
            ]
            .concat(),
            2048,
        ),
        ("UTF-16 label", utf16(&"第😀章".repeat(300)), 256),
    ];
    println!(
        "Alternating full decode/truncate and bounded decode, {passes} passes, 1000 decodes per sample. Allocation and drop included; ellipsis/trim/shrink, PDF parsing and GUI excluded."
    );
    for (name, bytes, limit) in cases {
        let bytes = &bytes[..bytes.len().min(if limit == 256 { 1024 } else { 16 * 1024 })];
        let old = baseline(bytes, limit);
        let new = pdf_string::decode_prefix(bytes, limit);
        assert_eq!(old, new, "{name}");
        let capacities = [old.0.capacity(), new.0.capacity()];
        drop((old, new));
        for _ in 0..32 {
            black_box(baseline(black_box(bytes), black_box(limit)));
            black_box(pdf_string::decode_prefix(
                black_box(bytes),
                black_box(limit),
            ));
        }
        let mut samples = [Vec::new(), Vec::new()];
        for pass in 0..passes {
            for order in 0..2 {
                let implementation = (pass + order) % 2;
                let decode = if implementation == 0 {
                    baseline
                } else {
                    pdf_string::decode_prefix
                };
                let start = Instant::now();
                for _ in 0..1000 {
                    black_box(decode(black_box(bytes), black_box(limit)));
                }
                samples[implementation].push(start.elapsed().as_secs_f64() * 1000.0);
            }
        }
        samples
            .iter_mut()
            .for_each(|values| values.sort_by(f64::total_cmp));
        println!(
            "{name}: {} input bytes, limit {limit}, output capacities {}/{}, baseline {:.4} ms, current {:.4} ms",
            bytes.len(),
            capacities[0],
            capacities[1],
            samples[0][passes / 2],
            samples[1][passes / 2]
        );
    }
}
