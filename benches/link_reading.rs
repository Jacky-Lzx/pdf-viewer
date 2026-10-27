#[allow(dead_code, unused_imports)]
#[path = "../src/destination.rs"]
mod destination;
#[allow(dead_code, unused_imports)]
#[path = "../src/geometry.rs"]
mod geometry;
#[allow(dead_code, unused_imports)]
#[path = "../src/link_hints.rs"]
mod link_hints;
#[allow(dead_code, unused_imports)]
#[path = "../src/links.rs"]
mod links;
#[allow(dead_code, unused_imports)]
#[path = "../src/navigation.rs"]
mod navigation;
#[allow(dead_code, unused_imports)]
#[path = "../src/outline.rs"]
mod outline;
#[allow(dead_code, unused_imports)]
#[path = "../src/pdf_objects.rs"]
mod pdf_objects;
#[allow(dead_code, unused_imports)]
#[path = "../src/pdf_string.rs"]
mod pdf_string;

use hayro::hayro_syntax::Pdf;
use links::LinkTarget;
use std::{hint::black_box, time::Instant};

fn fixture(count: usize, kind: &str) -> Pdf {
    let names = if kind == "direct-rich-names" {
        format!(
            "/Names << /Dests << /Names [{}] >> >>",
            (0..9999)
                .map(|index| format!("(unused{index:05}) [3 0 R /Fit] "))
                .collect::<String>()
        )
    } else if kind == "internal-wide-names" {
        format!("/Names << /Dests {} 0 R >>", count + 3)
    } else {
        String::new()
    };
    let mut objects = vec![
        format!(
            "<< /Type /Catalog /Pages 2 0 R /Dests << /target [{} 0 R /Fit] >> {names} >>",
            count + 2
        ),
        format!(
            "<< /Type /Pages /Kids [{}] /Count {count} >>",
            (3..count + 3)
                .map(|id| format!("{id} 0 R "))
                .collect::<String>()
        ),
    ];
    for page in 0..count {
        let direct = format!(
            "/Annots [<< /Subtype /Link /Rect [10 10 30 30] /Dest [{} 0 R /Fit] >>]",
            count + 2
        );
        let annotation = if page == 0 {
            match kind {
                "web" => {
                    "/Annots [<< /Subtype /Link /Rect [10 10 30 30] /A << /S /URI /URI (https://example.com/) >> >>]"
                }
                "internal" | "internal-wide-names" => {
                    "/Annots [<< /Subtype /Link /Rect [10 10 30 30] /Dest /target >>]"
                }
                "direct" | "direct-rich-names" => &direct,
                _ => "",
            }
        } else {
            ""
        };
        objects.push(format!(
            "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 600 800] {annotation} >>"
        ));
    }
    if kind == "internal-wide-names" {
        let root = count + 3;
        let empty = root + 32;
        let repeated = format!("{empty} 0 R ").repeat(9999);
        for depth in 0..32 {
            objects.push(if depth == 31 {
                "<< /Names [(unused) [3 0 R /Fit]] >>".into()
            } else {
                format!("<< /Kids [{repeated}{} 0 R] >>", root + depth + 1)
            });
        }
        objects.push("<< >>".into());
    }
    let mut data = b"%PDF-1.4\n".to_vec();
    let mut offsets = vec![0];
    for (index, object) in objects.iter().enumerate() {
        offsets.push(data.len());
        data.extend(format!("{} 0 obj\n{object}\nendobj\n", index + 1).bytes());
    }
    let xref = data.len();
    data.extend(format!("xref\n0 {}\n0000000000 65535 f \n", offsets.len()).bytes());
    for offset in &offsets[1..] {
        data.extend(format!("{offset:010} 00000 n \n").bytes());
    }
    data.extend(
        format!(
            "trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n",
            offsets.len()
        )
        .bytes(),
    );
    let pdf = Pdf::new(data).unwrap();
    assert_eq!(pdf.pages().len(), count);
    pdf
}

fn first_links(pdf: &Pdf) -> Vec<links::PageLink> {
    links::LinkReader::new(pdf).read(&pdf.pages()[0])
}

fn main() {
    let passes: usize = std::env::args()
        .nth(1)
        .and_then(|value| value.parse().ok())
        .unwrap_or(9);
    assert!((1..=1000).contains(&passes));
    println!(
        "{passes} passes; 100 fresh link sessions/sample, index construction, first-page links, checksum and drop; warmed parsed PDF, no parsing, rendering, widgets or GPU."
    );
    for pages in [6, 10_000] {
        for kind in [
            "none",
            "web",
            "internal",
            "direct",
            "direct-rich-names",
            "internal-wide-names",
        ] {
            let pdf = fixture(pages, kind);
            let expected = first_links(&pdf);
            assert_eq!(expected.len(), usize::from(kind != "none"));
            if let Some(link) = expected.first() {
                assert_eq!(link.rect, hayro::kurbo::Rect::new(10.0, 770.0, 30.0, 790.0));
                match &link.target {
                    LinkTarget::Url(url) => assert_eq!(url, "https://example.com/"),
                    LinkTarget::Destination(target) => assert_eq!(target.page, pages - 1),
                }
            }
            let mut samples = Vec::with_capacity(passes);
            let mut checksum = 0;
            for _ in 0..passes {
                let start = Instant::now();
                for _ in 0..100 {
                    let links = first_links(black_box(&pdf));
                    checksum = links
                        .iter()
                        .map(|link| match &link.target {
                            LinkTarget::Url(url) => url.len(),
                            LinkTarget::Destination(target) => target.page,
                        })
                        .sum::<usize>();
                    black_box((&links, checksum));
                }
                samples.push(start.elapsed().as_secs_f64() * 1000.0);
            }
            samples.sort_by(f64::total_cmp);
            println!(
                "{pages} pages, {kind}: {:.4} ms, {} links, checksum {checksum}",
                samples[passes / 2],
                expected.len()
            );
        }
    }
}
