use crate::outline::DestinationIndex;
use hayro::{
    hayro_interpret::util::TransformExt,
    hayro_syntax::{
        object::{Array, Dict, Name, Object, Rect as PdfRect, String as PdfString},
        page::Page,
    },
    kurbo::Rect,
};

#[derive(Clone, Debug, PartialEq)]
pub enum LinkTarget {
    Page(usize),
    Url(String),
}

#[derive(Clone, Debug)]
pub struct PageLink {
    pub rect: Rect,
    pub target: LinkTarget,
}

pub fn read<'a>(page: &Page<'a>, destinations: &DestinationIndex<'a>) -> Vec<PageLink> {
    let Some(annotations) = page.raw().get::<Array<'_>>(b"Annots") else {
        return Vec::new();
    };
    let transform = page.initial_transform(true).to_kurbo();
    annotations
        .iter::<Dict<'_>>()
        .take(1000)
        .filter_map(|annotation| {
            if annotation.get::<Name<'_>>(b"Subtype")?.as_ref() != b"Link" {
                return None;
            }
            if annotation.get::<u32>(b"F").unwrap_or(0) & (2 | 32) != 0 {
                return None;
            }
            let bounds = annotation.get::<PdfRect>(b"Rect")?;
            let (width, height) = page.render_dimensions();
            let rect = transform
                .transform_rect_bbox(Rect::new(bounds.x0, bounds.y0, bounds.x1, bounds.y1))
                .intersect(Rect::new(0.0, 0.0, width as f64, height as f64));
            if !rect.is_finite() || rect.width() <= 0.0 || rect.height() <= 0.0 {
                return None;
            }
            let target = if let Some(dest) = annotation.get::<Object<'_>>(b"Dest") {
                LinkTarget::Page(destinations.resolve(dest)?)
            } else {
                let action = annotation.get::<Dict<'_>>(b"A")?;
                match action.get::<Name<'_>>(b"S")?.as_ref() {
                    b"GoTo" => {
                        LinkTarget::Page(destinations.resolve(action.get::<Object<'_>>(b"D")?)?)
                    }
                    b"URI" => {
                        let uri = action.get::<PdfString<'_>>(b"URI")?;
                        let url = String::from_utf8_lossy(uri.as_bytes()).into_owned();
                        let scheme = url.split_once(':')?.0;
                        if !scheme.eq_ignore_ascii_case("https")
                            && !scheme.eq_ignore_ascii_case("http")
                        {
                            return None;
                        }
                        LinkTarget::Url(url)
                    }
                    _ => return None,
                }
            };
            Some(PageLink { rect, target })
        })
        .collect()
}

pub fn hint_label(mut index: usize, count: usize) -> String {
    let digits = if count > 676 {
        3
    } else if count > 26 {
        2
    } else {
        1
    };
    let mut label = vec![b'a'; digits];
    for position in (0..digits).rev() {
        label[position] += (index % 26) as u8;
        index /= 26;
    }
    String::from_utf8(label).expect("ASCII link hint")
}

#[derive(Default)]
pub struct LinkHints {
    pub active: bool,
    pub prefix: String,
    count: usize,
}

impl LinkHints {
    pub fn start(&mut self, count: usize) {
        self.active = count > 0;
        self.prefix.clear();
        self.count = count;
    }
    pub fn cancel(&mut self) {
        self.active = false;
        self.prefix.clear();
    }
    pub fn press(&mut self, letter: char) -> Option<usize> {
        if !self.active || !letter.is_ascii_lowercase() {
            return None;
        }
        self.prefix.push(letter);
        let mut matching = false;
        for index in 0..self.count {
            let label = hint_label(index, self.count);
            if label == self.prefix {
                self.cancel();
                return Some(index);
            }
            matching |= label.starts_with(&self.prefix);
        }
        if !matching {
            self.cancel();
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use hayro::hayro_syntax::Pdf;

    #[test]
    fn hint_labels_are_unique_and_never_prefix_one_another() {
        for count in [1, 26, 27, 676, 1000] {
            let labels: std::collections::HashSet<_> =
                (0..count).map(|i| hint_label(i, count)).collect();
            assert_eq!(labels.len(), count);
            assert!(
                labels
                    .iter()
                    .all(|label| label.len() == hint_label(0, count).len())
            );
        }
        let mut hints = LinkHints::default();
        hints.start(27);
        assert_eq!(hints.press('b'), None);
        assert_eq!(hints.press('a'), Some(26));
        assert!(!hints.active);
        hints.start(3);
        assert_eq!(hints.press('z'), None);
        assert!(!hints.active);
    }

    #[test]
    fn pdf_links_resolve_targets_and_flip_y_coordinates() {
        let pdf = Pdf::new(include_bytes!("../assets/folio-guide.pdf").to_vec()).unwrap();
        let index = DestinationIndex::new(&pdf);
        let links = read(&pdf.pages()[0], &index);
        assert_eq!(links.len(), 2);
        assert_eq!(links[0].target, LinkTarget::Page(1));
        assert_eq!(
            links[1].target,
            LinkTarget::Url("https://www.rust-lang.org/".into())
        );
        assert_eq!(links[0].rect, Rect::new(48.0, 622.0, 275.0, 650.0));
    }

    #[test]
    fn non_web_uri_actions_are_not_exposed_as_links() {
        let mut bytes = include_bytes!("../assets/folio-guide.pdf").to_vec();
        let offset = bytes
            .windows(8)
            .position(|value| value == b"https://")
            .unwrap();
        bytes[offset..offset + 8].copy_from_slice(b"file:///");
        let pdf = Pdf::new(bytes).unwrap();
        let links = read(&pdf.pages()[0], &DestinationIndex::new(&pdf));
        assert_eq!(links.len(), 1);
        assert_eq!(links[0].target, LinkTarget::Page(1));
    }
}
