#[allow(dead_code, unused_imports)]
#[path = "../src/case_fold.rs"]
mod case_fold;
#[allow(dead_code, unused_imports)]
#[path = "../src/destination.rs"]
mod destination;
#[allow(dead_code, unused_imports)]
#[path = "../src/geometry.rs"]
mod geometry;
#[allow(dead_code, unused_imports)]
#[path = "../src/navigation.rs"]
mod navigation;
#[allow(dead_code, unused_imports)]
#[path = "../src/outline.rs"]
mod outline;
#[allow(dead_code, unused_imports)]
#[path = "../src/page_labels.rs"]
mod page_labels;
#[allow(dead_code, unused_imports)]
#[path = "../src/pdf_objects.rs"]
mod pdf_objects;
#[allow(dead_code, unused_imports)]
#[path = "../src/pdf_string.rs"]
mod pdf_string;

use hayro::hayro_syntax::{
    Pdf,
    object::{Dict, Object},
};
use std::{hint::black_box, time::Instant};

fn fixture(labels: bool, kind: &str) -> Pdf {
    let leaves = if kind == "ordinary" { 0 } else { 1000 };
    let shared = leaves + 5;
    let children: String = (5..shared).map(|id| format!("{id} 0 R ")).collect();
    let mut objects = vec![
        "<< /Type /Catalog /Pages 2 0 R /Names << /Dests 4 0 R >> /PageLabels 4 0 R /Target (kept) >>".to_owned(),
        "<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_owned(),
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 600 800] >>".to_owned(),
        format!("<< /Kids [{children}] /Names [(kept) [3 0 R /Fit]] /Nums [0 << /S /D /P (kept) >>] >>"),
    ];
    let key = if kind == "shared-kids" {
        "Kids"
    } else if labels {
        "Nums"
    } else {
        "Names"
    };
    objects.extend((0..leaves).map(|_| format!("<< /{key} {shared} 0 R >>")));
    let item = match kind {
        "shared-invalid" => "0 0 ",
        "shared-duplicate" if labels => "0 << /S /D /P (kept) >> ",
        "shared-duplicate" => "(noise) [3 0 R /Fit] ",
        "shared-kids" => "null ",
        "ordinary" => "",
        _ => unreachable!(),
    };
    objects.push(format!("[{}]", item.repeat(1000)));
    let mut data = b"%PDF-1.4\n".to_vec();
    let mut offsets = vec![0];
    for (index, object) in objects.iter().enumerate() {
        offsets.push(data.len());
        data.extend_from_slice(format!("{} 0 obj\n{object}\nendobj\n", index + 1).as_bytes());
    }
    let xref = data.len();
    data.extend_from_slice(format!("xref\n0 {}\n0000000000 65535 f \n", offsets.len()).as_bytes());
    for offset in &offsets[1..] {
        data.extend_from_slice(format!("{offset:010} 00000 n \n").as_bytes());
    }
    data.extend_from_slice(
        format!(
            "trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n",
            offsets.len()
        )
        .as_bytes(),
    );
    Pdf::new(data).unwrap()
}

fn session(pdf: &Pdf, labels: bool) -> usize {
    if labels {
        let labels = page_labels::PageLabels::read(pdf);
        let label = labels.label(0).unwrap();
        assert_eq!(label, "kept1");
        label.bytes().map(usize::from).sum()
    } else {
        let catalog = pdf.xref().get::<Dict<'_>>(pdf.xref().root_id()).unwrap();
        let index = outline::DestinationIndex::new(pdf);
        let destination = index
            .resolve(catalog.get::<Object<'_>>(b"Target").unwrap())
            .unwrap();
        assert_eq!(destination.page, 0);
        destination.page + 1
    }
}

fn main() {
    let passes: usize = std::env::args()
        .nth(1)
        .and_then(|value| value.parse().ok())
        .unwrap_or(9);
    assert!((1..=1000).contains(&passes));
    println!(
        "{passes} passes; 10 fresh metadata sessions/sample; 1000 distinct leaves sharing a 1000-item/pair array, valid root target/label retained; traversal, result validation and metadata drop included; warmed parsed PDF, no parsing, rendering or UI."
    );
    for labels in [false, true] {
        for kind in [
            "ordinary",
            "shared-invalid",
            "shared-duplicate",
            "shared-kids",
        ] {
            let pdf = fixture(labels, kind);
            let expected = session(&pdf, labels);
            let mut samples = Vec::with_capacity(passes);
            let mut checksum = 0;
            for _ in 0..passes {
                let start = Instant::now();
                for _ in 0..10 {
                    checksum = black_box(session(black_box(&pdf), labels));
                }
                samples.push(start.elapsed().as_secs_f64() * 1000.0);
                assert_eq!(checksum, expected);
            }
            samples.sort_by(f64::total_cmp);
            println!(
                "{}/{kind}: {:.4} ms, checksum {checksum}",
                if labels { "labels" } else { "names" },
                samples[passes / 2]
            );
        }
    }
}
