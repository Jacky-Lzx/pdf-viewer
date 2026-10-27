use crate::destination::{Destination, DestinationFit};
use hayro::hayro_interpret::util::TransformExt;
use hayro::hayro_syntax::{
    Pdf,
    object::{Array, Dict, MaybeRef, Name, Object, ObjectIdentifier, String as PdfString},
    page::Page,
};
use std::collections::{HashMap, HashSet};

#[derive(Clone, Debug)]
pub struct OutlineEntry {
    pub title: String,
    pub destination: Option<Destination>,
    pub depth: usize,
}

fn decode_title(bytes: &[u8]) -> String {
    const MAX_BYTES: usize = 16 * 1024;
    const MAX_CHARS: usize = 2048;
    let mut title = crate::pdf_string::decode(&bytes[..bytes.len().min(MAX_BYTES)]);
    let end = title.char_indices().nth(MAX_CHARS).map(|(end, _)| end);
    if let Some(end) = end {
        title.truncate(end);
    }
    if end.is_some() || bytes.len() > MAX_BYTES {
        title.push('…');
        // Dropping the decoded suffix must also release its reserved storage.
        title.shrink_to_fit();
    }
    title
}

pub struct DestinationIndex<'a> {
    pages: HashMap<ObjectIdentifier, usize>,
    named: HashMap<Vec<u8>, Object<'a>>,
    page_data: &'a [Page<'a>],
}

impl<'a> DestinationIndex<'a> {
    pub fn new(pdf: &'a Pdf) -> Self {
        let pages = pdf
            .pages()
            .iter()
            .enumerate()
            .filter_map(|(page, data)| data.raw().obj_id().map(|id| (id, page)))
            .collect();
        let named = pdf
            .xref()
            .get::<Dict<'a>>(pdf.xref().root_id())
            .map(|catalog| named_destinations(&catalog))
            .unwrap_or_default();
        Self {
            pages,
            named,
            page_data: pdf.pages(),
        }
    }

    pub fn resolve(&self, object: Object<'a>) -> Option<Destination> {
        self.destination(object, 0)
    }

    fn destination(&self, object: Object<'a>, depth: usize) -> Option<Destination> {
        if depth > 8 {
            return None;
        }
        match object {
            Object::Array(array) => {
                let page = match array.raw_iter().next()? {
                    MaybeRef::Ref(reference) => self.pages.get(&reference.into()).copied()?,
                    MaybeRef::NotRef(Object::Number(number)) => {
                        usize::try_from(Object::Number(number).into_i32()?).ok()?
                    }
                    _ => return None,
                };
                let data = self.page_data.get(page)?;
                let mut destination = Destination::page(page);
                let mut values = array.iter::<Object<'_>>();
                values.next();
                let kind = values.next().and_then(Object::into_name);
                let number = |value: Option<Object<'a>>| {
                    value
                        .and_then(Object::into_f32)
                        .filter(|value| value.is_finite())
                };
                let mut position = [None; 2];
                match kind.as_deref() {
                    Some(b"XYZ") => {
                        position = [number(values.next()), number(values.next())];
                        destination.zoom = number(values.next()).filter(|&zoom| zoom > 0.0);
                    }
                    Some(b"Fit" | b"FitB") => destination.fit = Some(DestinationFit::Page),
                    Some(b"FitH" | b"FitBH") => {
                        destination.fit = Some(DestinationFit::Width);
                        position[1] = number(values.next());
                    }
                    Some(b"FitV" | b"FitBV") => {
                        destination.fit = Some(DestinationFit::Height);
                        position[0] = number(values.next());
                    }
                    Some(b"FitR") => {
                        if let [Some(left), Some(bottom), Some(right), Some(top)] =
                            std::array::from_fn::<_, 4, _>(|_| number(values.next()))
                        {
                            let rect = data.initial_transform(true).to_kurbo().transform_rect_bbox(
                                hayro::kurbo::Rect::new(
                                    f64::from(left),
                                    f64::from(bottom),
                                    f64::from(right),
                                    f64::from(top),
                                ),
                            );
                            if rect.width() > 0.0 && rect.height() > 0.0 {
                                destination.fit = Some(DestinationFit::Rectangle(rect));
                            }
                        }
                    }
                    _ => {}
                }
                let [a, b, c, d, e, f] = data.initial_transform(true).to_kurbo().as_coeffs();
                let transformed = |a: f64, b: f64, offset: f64| {
                    let first = if a == 0.0 {
                        0.0
                    } else {
                        a * f64::from(position[0]?)
                    };
                    let second = if b == 0.0 {
                        0.0
                    } else {
                        b * f64::from(position[1]?)
                    };
                    Some(first + second + offset)
                };
                destination.position = [transformed(a, c, e), transformed(b, d, f)];
                Some(destination)
            }
            Object::Dict(dict) => self.destination(dict.get::<Object<'_>>(b"D")?, depth + 1),
            Object::Name(name) => {
                self.destination(self.named.get(name.as_ref())?.clone(), depth + 1)
            }
            Object::String(name) => {
                self.destination(self.named.get(name.as_bytes())?.clone(), depth + 1)
            }
            _ => None,
        }
    }
}

pub fn read(pdf: &Pdf) -> Vec<OutlineEntry> {
    let Some(catalog) = pdf.xref().get::<Dict<'_>>(pdf.xref().root_id()) else {
        return Vec::new();
    };
    let Some(first) = catalog
        .get::<Dict<'_>>(b"Outlines")
        .and_then(|outline| outline.get::<Dict<'_>>(b"First"))
    else {
        return Vec::new();
    };
    let destinations = DestinationIndex::new(pdf);
    let mut stack = vec![(first, 0)];
    let mut seen = HashSet::new();
    let mut entries = Vec::new();
    while let Some((item, depth)) = stack.pop() {
        if entries.len() >= 10_000 || !seen.insert(item.data().as_ptr() as usize) {
            continue;
        }
        let title = item
            .get::<PdfString<'_>>(b"Title")
            .map(|s| decode_title(s.as_bytes()))
            .unwrap_or_default();
        let dest = item.get::<Object<'_>>(b"Dest").or_else(|| {
            let action = item.get::<Dict<'_>>(b"A")?;
            let kind = action.get::<Name<'_>>(b"S")?;
            (kind.as_ref() == b"GoTo")
                .then(|| action.get::<Object<'_>>(b"D"))
                .flatten()
        });
        let destination = dest.and_then(|dest| destinations.resolve(dest));
        entries.push(OutlineEntry {
            title,
            destination,
            depth,
        });
        if let Some(next) = item.get::<Dict<'_>>(b"Next") {
            stack.push((next, depth));
        }
        if depth < 32
            && let Some(child) = item.get::<Dict<'_>>(b"First")
        {
            stack.push((child, depth + 1));
        }
    }
    entries
}

fn named_destinations<'a>(catalog: &Dict<'a>) -> HashMap<Vec<u8>, Object<'a>> {
    let mut named = HashMap::new();
    if let Some(dict) = catalog.get::<Dict<'_>>(b"Dests") {
        for key in dict.keys().take(10_000) {
            if let Some(value) = dict.get::<Object<'_>>(key.as_ref()) {
                named.insert(key.as_ref().to_vec(), value);
            }
        }
    }
    let root = catalog
        .get::<Dict<'_>>(b"Names")
        .and_then(|names| names.get::<Dict<'_>>(b"Dests"));
    let mut stack: Vec<_> = root.into_iter().map(|node| (node, 0)).collect();
    let mut seen = HashSet::new();
    while let Some((node, depth)) = stack.pop() {
        if depth > 32
            || seen.len() >= 10_000
            || named.len() >= 10_000
            || !seen.insert(node.data().as_ptr() as usize)
        {
            continue;
        }
        if let Some(array) = node.get::<Array<'_>>(b"Names") {
            let mut iter = array.iter::<Object<'_>>();
            while let (Some(Object::String(key)), Some(value)) = (iter.next(), iter.next()) {
                named.insert(key.as_bytes().to_vec(), value);
                if named.len() >= 10_000 {
                    break;
                }
            }
        }
        if let Some(kids) = node.get::<Array<'_>>(b"Kids") {
            stack.extend(
                kids.iter::<Dict<'_>>()
                    .take(10_000)
                    .map(|kid| (kid, depth + 1)),
            );
        }
    }
    named
}

#[cfg(test)]
mod tests {
    use super::*;

    fn destination_pdf(view: &str, page_settings: &str) -> Pdf {
        let objects = [
            format!(
                "<< /Type /Catalog /Pages 2 0 R /Dests << /test {view} /alias /test /loop /loop >> /Target /alias >>"
            ),
            "<< /Type /Pages /Kids [3 0 R] /Count 1 >>".into(),
            format!("<< /Type /Page /Parent 2 0 R /MediaBox [0 0 600 800] {page_settings} >>"),
        ];
        let mut bytes = b"%PDF-1.4\n".to_vec();
        let mut offsets = vec![0];
        for (index, object) in objects.iter().enumerate() {
            offsets.push(bytes.len());
            bytes.extend_from_slice(format!("{} 0 obj\n{object}\nendobj\n", index + 1).as_bytes());
        }
        let xref = bytes.len();
        bytes.extend_from_slice(
            format!("xref\n0 {}\n0000000000 65535 f \n", offsets.len()).as_bytes(),
        );
        for offset in &offsets[1..] {
            bytes.extend_from_slice(format!("{offset:010} 00000 n \n").as_bytes());
        }
        bytes.extend_from_slice(
            format!(
                "trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n",
                offsets.len()
            )
            .as_bytes(),
        );
        Pdf::new(bytes).unwrap()
    }

    fn resolve_test(pdf: &Pdf) -> Destination {
        let catalog = pdf.xref().get::<Dict<'_>>(pdf.xref().root_id()).unwrap();
        DestinationIndex::new(pdf)
            .resolve(catalog.get::<Object<'_>>(b"Target").unwrap())
            .unwrap()
    }

    #[test]
    fn named_destinations_preserve_xyz_zoom_nulls_crop_and_intrinsic_rotation() {
        let plain = destination_pdf("[3 0 R /XYZ 48 500 2]", "");
        let target = resolve_test(&plain);
        assert_eq!(target.page, 0);
        assert_eq!(target.position, [Some(48.0), Some(300.0)]);
        assert_eq!(target.zoom, Some(2.0));
        let cropped = destination_pdf(
            "[3 0 R /XYZ 48 500 2]",
            "/CropBox [10 20 590 780] /Rotate 90",
        );
        assert_eq!(resolve_test(&cropped).position, [Some(480.0), Some(38.0)]);
        let nullable = destination_pdf(
            "[3 0 R /XYZ 48 null 0]",
            "/CropBox [10 20 590 780] /Rotate 90",
        );
        assert_eq!(resolve_test(&nullable).position, [None, Some(38.0)]);
        assert_eq!(resolve_test(&nullable).zoom, None);
        let catalog = plain
            .xref()
            .get::<Dict<'_>>(plain.xref().root_id())
            .unwrap();
        let named = catalog.get::<Dict<'_>>(b"Dests").unwrap();
        assert!(
            DestinationIndex::new(&plain)
                .resolve(named.get::<Object<'_>>(b"loop").unwrap())
                .is_none()
        );
    }

    #[test]
    fn fit_destinations_retain_axes_and_transform_bounded_rectangles() {
        for (view, position, fit) in [
            ("/FitH 600", [None, Some(200.0)], DestinationFit::Width),
            ("/FitBH 600", [None, Some(200.0)], DestinationFit::Width),
            ("/FitV 250", [Some(250.0), None], DestinationFit::Height),
            ("/FitBV 250", [Some(250.0), None], DestinationFit::Height),
            ("/Fit", [None; 2], DestinationFit::Page),
            ("/FitB", [None; 2], DestinationFit::Page),
            (
                "/FitR 100 100 300 300",
                [None; 2],
                DestinationFit::Rectangle(hayro::kurbo::Rect::new(100.0, 500.0, 300.0, 700.0)),
            ),
        ] {
            let pdf = destination_pdf(&format!("[3 0 R {view}]"), "");
            let destination = resolve_test(&pdf);
            assert_eq!(destination.position, position);
            assert_eq!(destination.fit, Some(fit));
        }
    }
    #[test]
    fn outline_resolves_direct_action_and_named_destinations() {
        let pdf = Pdf::new(include_bytes!("../assets/folio-guide.pdf").to_vec()).unwrap();
        let outline = read(&pdf);
        assert_eq!(outline.len(), 6);
        assert_eq!(
            outline
                .iter()
                .map(|entry| entry.destination.unwrap().page)
                .collect::<Vec<_>>(),
            vec![0, 1, 2, 3, 4, 5]
        );
        assert_eq!(outline[0].title, "A little room to read.");
    }

    #[test]
    fn long_outline_titles_are_bounded_before_the_ui_prepares_them() {
        let mut objects = vec![
            b"<< /Type /Catalog /Pages 2 0 R /Outlines << /First 4 0 R >> >>".to_vec(),
            b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_vec(),
            b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 600 800] >>".to_vec(),
        ];
        for index in 0..32 {
            let next = if index < 31 {
                format!("/Next {} 0 R", index + 5)
            } else {
                String::new()
            };
            objects.push(format!("<< /Title 36 0 R /Dest [3 0 R /Fit] {next} >>").into_bytes());
        }
        objects.push([b"(".as_slice(), &vec![b'x'; 128 * 1024], b")"].concat());
        let mut data = b"%PDF-1.4\n".to_vec();
        let mut offsets = vec![0];
        for (index, object) in objects.iter().enumerate() {
            offsets.push(data.len());
            data.extend(format!("{} 0 obj\n", index + 1).bytes());
            data.extend(object);
            data.extend(b"\nendobj\n");
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
        let entries = read(&pdf);
        assert_eq!(entries.len(), 32);
        for entry in &entries {
            assert!(
                entry.title == format!("{}…", "x".repeat(2048)),
                "retained title bytes: {}",
                entry.title.len()
            );
            assert_eq!(entry.destination.unwrap().page, 0);
        }
        let bytes: usize = entries.iter().map(|entry| entry.title.capacity()).sum();
        assert!(bytes < 128 * 1024, "bounded title capacities: {bytes}");
    }

    #[test]
    fn title_limits_preserve_complete_unicode_prefixes_and_ordinary_text() {
        for text in ["", " Chapter \t ", &"a".repeat(2048), "中😀𝄞"] {
            let source = [b"\xef\xbb\xbf".as_slice(), text.as_bytes()].concat();
            assert_eq!(decode_title(&source), text);
        }
        for text in ["a".repeat(2049), "中文😀𝄞".repeat(5000)] {
            let expected = text.chars().take(2048).collect::<String>() + "…";
            for source in [
                [b"\xef\xbb\xbf".as_slice(), text.as_bytes()].concat(),
                [
                    b"\xfe\xff".as_slice(),
                    &text
                        .encode_utf16()
                        .flat_map(u16::to_be_bytes)
                        .collect::<Vec<_>>(),
                ]
                .concat(),
            ] {
                let title = decode_title(&source);
                assert_eq!(title, expected);
                assert!(title.capacity() <= 2048 * 4 + 3);
            }
        }
        assert_eq!(decode_title(b"author\x84name"), "author—name");
        assert_eq!(decode_title(b"\xfe\xff\xd8\x00"), "�");
    }

    #[test]
    fn malformed_cyclic_outline_terminates() {
        let bytes = include_bytes!("../assets/folio-guide.pdf");
        let mut modified = bytes.to_vec();
        let offset = modified
            .windows(b"/Next 19 0 R".len())
            .position(|value| value == b"/Next 19 0 R")
            .unwrap();
        modified[offset..offset + b"/Next 18 0 R".len()].copy_from_slice(b"/Next 18 0 R");
        let pdf = Pdf::new(modified).unwrap();
        assert_eq!(read(&pdf).len(), 1);
    }
}
