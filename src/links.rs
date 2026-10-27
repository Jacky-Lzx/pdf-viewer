use crate::outline::DestinationIndex;
use hayro::{
    hayro_interpret::util::TransformExt,
    hayro_syntax::{
        Pdf,
        object::{Array, Dict, Name, Object, Rect as PdfRect, String as PdfString},
        page::Page,
    },
    kurbo::Rect,
};
use std::cell::OnceCell;

#[derive(Clone, Debug, PartialEq)]
pub enum LinkTarget {
    Destination(crate::destination::Destination),
    Url(String),
}

#[derive(Clone, Debug)]
pub struct PageLink {
    pub rect: Rect,
    pub target: LinkTarget,
}

pub struct LinkReader<'a> {
    pdf: &'a Pdf,
    destinations: OnceCell<DestinationIndex<'a>>,
}

impl<'a> LinkReader<'a> {
    pub fn new(pdf: &'a Pdf) -> Self {
        Self {
            pdf,
            destinations: OnceCell::new(),
        }
    }

    fn resolve(&self, object: Object<'a>) -> Option<crate::destination::Destination> {
        self.destinations
            .get_or_init(|| DestinationIndex::new(self.pdf))
            .resolve(object)
    }

    pub fn read(&self, page: &Page<'a>) -> Vec<PageLink> {
        let Some(annotations) = page.raw().get::<Array<'_>>(b"Annots") else {
            return Vec::new();
        };
        let transform = page.initial_transform(true).to_kurbo();
        annotations
            .iter::<Dict<'_>>()
            .take(crate::link_hints::MAX_LINKS)
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
                    LinkTarget::Destination(self.resolve(dest)?)
                } else {
                    let action = annotation.get::<Dict<'_>>(b"A")?;
                    match action.get::<Name<'_>>(b"S")?.as_ref() {
                        b"GoTo" => {
                            LinkTarget::Destination(self.resolve(action.get::<Object<'_>>(b"D")?)?)
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
}

#[cfg(test)]
mod tests {
    use super::*;
    use hayro::hayro_syntax::Pdf;

    #[test]
    fn pdf_links_resolve_targets_and_flip_y_coordinates() {
        let pdf = Pdf::new(include_bytes!("../assets/folio-guide.pdf").to_vec()).unwrap();
        let reader = LinkReader::new(&pdf);
        assert!(reader.destinations.get().is_none());
        assert!(reader.read(&pdf.pages()[1]).is_empty());
        assert!(reader.destinations.get().is_none());
        let links = reader.read(&pdf.pages()[0]);
        let index = reader.destinations.get().unwrap() as *const DestinationIndex<'_>;
        let again = reader.read(&pdf.pages()[0]);
        assert_eq!(again.len(), links.len());
        assert!(std::ptr::eq(reader.destinations.get().unwrap(), index));
        assert_eq!(links.len(), 2);
        assert!(matches!(
            links[0].target,
            LinkTarget::Destination(crate::destination::Destination {
                page: 1,
                fit: Some(crate::destination::DestinationFit::Page),
                ..
            })
        ));
        assert_eq!(
            links[1].target,
            LinkTarget::Url("https://www.rust-lang.org/".into())
        );
        assert_eq!(links[0].rect, Rect::new(48.0, 622.0, 275.0, 650.0));
    }

    #[test]
    fn web_links_do_not_initialize_the_document_destination_index() {
        let mut bytes = include_bytes!("../assets/folio-guide.pdf").to_vec();
        let offset = bytes
            .windows(7)
            .position(|value| value == b"/Dest [")
            .unwrap();
        bytes[offset..offset + 5].copy_from_slice(b"/Skip");
        let pdf = Pdf::new(bytes).unwrap();
        let reader = LinkReader::new(&pdf);
        for _ in 0..2 {
            let links = reader.read(&pdf.pages()[0]);
            assert_eq!(links.len(), 1);
            assert_eq!(
                links[0].target,
                LinkTarget::Url("https://www.rust-lang.org/".into())
            );
            assert!(reader.destinations.get().is_none());
        }
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
        let links = LinkReader::new(&pdf).read(&pdf.pages()[0]);
        assert_eq!(links.len(), 1);
        assert!(matches!(
            links[0].target,
            LinkTarget::Destination(crate::destination::Destination {
                page: 1,
                fit: Some(crate::destination::DestinationFit::Page),
                ..
            })
        ));
    }
}
