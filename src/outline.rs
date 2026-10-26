use hayro::hayro_syntax::{
    Pdf,
    object::{Array, Dict, MaybeRef, Object, ObjectIdentifier, String as PdfString},
};
use std::collections::{HashMap, HashSet};

#[derive(Clone, Debug)]
pub struct OutlineEntry {
    pub title: String,
    pub page: Option<usize>,
    pub depth: usize,
}

pub struct DestinationIndex<'a> {
    pages: HashMap<ObjectIdentifier, usize>,
    named: HashMap<Vec<u8>, Object<'a>>,
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
        Self { pages, named }
    }

    pub fn resolve(&self, object: Object<'a>) -> Option<usize> {
        destination(object, &self.pages, &self.named, 0)
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
            .map(|s| decode_string(s.as_bytes()))
            .unwrap_or_default();
        let dest = item.get::<Object<'_>>(b"Dest").or_else(|| {
            let action = item.get::<Dict<'_>>(b"A")?;
            let kind = action.get::<hayro::hayro_syntax::object::Name<'_>>(b"S")?;
            (kind.as_ref() == b"GoTo")
                .then(|| action.get::<Object<'_>>(b"D"))
                .flatten()
        });
        let page = dest.and_then(|dest| destinations.resolve(dest));
        entries.push(OutlineEntry { title, page, depth });
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

fn destination<'a>(
    object: Object<'a>,
    pages: &HashMap<ObjectIdentifier, usize>,
    named: &HashMap<Vec<u8>, Object<'a>>,
    depth: usize,
) -> Option<usize> {
    if depth > 8 {
        return None;
    }
    match object {
        Object::Array(array) => match array.raw_iter().next()? {
            MaybeRef::Ref(reference) => pages.get(&reference.into()).copied(),
            MaybeRef::NotRef(Object::Number(number)) => {
                let page = Object::Number(number).into_i32()?;
                (page >= 0 && (page as usize) < pages.len()).then_some(page as usize)
            }
            _ => None,
        },
        Object::Dict(dict) => destination(dict.get::<Object<'_>>(b"D")?, pages, named, depth + 1),
        Object::Name(name) => {
            destination(named.get(name.as_ref())?.clone(), pages, named, depth + 1)
        }
        Object::String(name) => {
            destination(named.get(name.as_bytes())?.clone(), pages, named, depth + 1)
        }
        _ => None,
    }
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

pub fn decode_string(bytes: &[u8]) -> String {
    if bytes.starts_with(&[0xfe, 0xff]) {
        let units: Vec<_> = bytes[2..]
            .as_chunks::<2>()
            .0
            .iter()
            .map(|pair| u16::from_be_bytes([pair[0], pair[1]]))
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

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn outline_resolves_direct_action_and_named_destinations() {
        let pdf = Pdf::new(include_bytes!("../assets/folio-guide.pdf").to_vec()).unwrap();
        let outline = read(&pdf);
        assert_eq!(outline.len(), 6);
        assert_eq!(
            outline
                .iter()
                .map(|entry| entry.page.unwrap())
                .collect::<Vec<_>>(),
            vec![0, 1, 2, 3, 4, 5]
        );
        assert_eq!(outline[0].title, "A little room to read.");
    }

    #[test]
    fn pdf_titles_decode_utf16_and_pdf_doc_encoding() {
        assert_eq!(decode_string(&[0xfe, 0xff, 0x4e, 0x2d, 0x65, 0x87]), "中文");
        assert_eq!(decode_string(b"Hello\x84world\xa0"), "Hello—world€");
        assert_eq!(decode_string(b"\xef\xbb\xbfUTF8"), "UTF8");
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
