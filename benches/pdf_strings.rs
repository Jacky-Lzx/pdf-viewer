#[path = "../src/pdf_string.rs"]
mod pdf_string;

use std::{hint::black_box, time::Instant};

// Previous PDF text-string decoder, before direct UTF-16 and ASCII paths.
fn baseline(bytes: &[u8]) -> String {
    if let Some(bytes) = bytes.strip_prefix(&[0xfe, 0xff]) {
        let units: Vec<_> = bytes
            .as_chunks::<2>()
            .0
            .iter()
            .map(|pair| u16::from_be_bytes(*pair))
            .collect();
        return String::from_utf16_lossy(&units);
    }
    if let Some(bytes) = bytes.strip_prefix(&[0xef, 0xbb, 0xbf]) {
        return String::from_utf8_lossy(bytes).into_owned();
    }
    // PDFDocEncoding differs from Latin-1 in these two ranges and at 0xA0.
    const DIACRITICS: [char; 8] = ['˘', 'ˇ', 'ˆ', '˙', '˝', '˛', '˚', '˜'];
    const SPECIAL: [char; 32] = [
        '•', '†', '‡', '…', '—', '–', 'ƒ', '⁄', '‹', '›', '−', '‰', '„', '“', '”', '‘', '’', '‚',
        '™', 'ﬁ', 'ﬂ', 'Ł', 'Œ', 'Š', 'Ÿ', 'Ž', 'ı', 'ł', 'œ', 'š', 'ž', '�',
    ];
    bytes
        .iter()
        .map(|byte| match byte {
            0x18..=0x1f => DIACRITICS[(byte - 0x18) as usize],
            0x80..=0x9f => SPECIAL[(byte - 0x80) as usize],
            0xa0 => '€',
            0x7f | 0xad => '�',
            _ => char::from(*byte),
        })
        .collect()
}

fn main() {
    let passes: usize = std::env::args()
        .nth(1)
        .and_then(|value| value.parse().ok())
        .unwrap_or(9);
    assert!(
        (1..=1000).contains(&passes),
        "Passes must be between 1 and 1000"
    );
    println!(
        "Alternating baseline/current, {passes} passes, 10,000 decodes per pass; medians include output allocation and drop."
    );
    let mut cases: Vec<_> = [
        (
            "Latin title",
            "Chapter 4. Performance and concurrency".to_owned(),
        ),
        ("Chinese title", "第四章：设计高效的跨平台阅读器".to_owned()),
        ("Greek and accents", "Ελληνικά Straße Résumé".to_owned()),
        ("Mixed and surrogates", "Chapter 4 中文 😀 𝄞".to_owned()),
        (
            "Long Chinese title",
            "第四章：设计高效的跨平台阅读器".repeat(32),
        ),
    ]
    .into_iter()
    .map(|(name, text)| {
        let mut bytes = vec![0xfe, 0xff];
        bytes.extend(text.encode_utf16().flat_map(u16::to_be_bytes));
        (name, bytes)
    })
    .collect();
    cases.extend([
        (
            "ASCII title",
            b"Chapter 4. Performance and concurrency".to_vec(),
        ),
        (
            "Long ASCII title",
            b"Chapter 4. Performance and concurrency".repeat(32),
        ),
        (
            "PDF control accents",
            b"title \x18\x19\x1a\x1b\x1c\x1d\x1e\x1f \x7f\x84\xa0".to_vec(),
        ),
    ]);
    for (name, bytes) in cases {
        let expected = baseline(&bytes);
        let actual = pdf_string::decode(&bytes);
        assert_eq!(actual, expected);
        let capacities = [expected.capacity(), actual.capacity()];
        drop((actual, expected));
        let mut times = [Vec::with_capacity(passes), Vec::with_capacity(passes)];
        for pass in 0..passes {
            for order in 0..2 {
                let improved = (pass + order) % 2;
                let function = if improved == 1 {
                    pdf_string::decode
                } else {
                    baseline
                };
                let start = Instant::now();
                for _ in 0..10_000 {
                    black_box(function(black_box(&bytes)));
                }
                times[improved].push(start.elapsed().as_secs_f64() * 1000.0);
            }
        }
        times
            .iter_mut()
            .for_each(|samples| samples.sort_by(f64::total_cmp));
        println!(
            "{name}: {} input bytes, output capacities {}/{}, temporary UTF-16 bytes {}/0, baseline {:.4} ms, current {:.4} ms",
            bytes.len(),
            capacities[0],
            capacities[1],
            if bytes.starts_with(&[0xfe, 0xff]) {
                bytes.len() - 2
            } else {
                0
            },
            times[0][passes / 2],
            times[1][passes / 2]
        );
    }
}
