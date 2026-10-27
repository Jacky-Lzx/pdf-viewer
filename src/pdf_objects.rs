use hayro::hayro_syntax::{
    Pdf,
    object::{Array, Dict, MaybeRef, Object},
};

/// Skip bad entries and dangling references without ending the array traversal.
/// Count raw entries before resolving so malformed items cannot evade the cap.
pub fn dictionaries<'a>(
    pdf: &'a Pdf,
    array: &Array<'a>,
    limit: usize,
) -> impl Iterator<Item = Dict<'a>> + 'a {
    array
        .raw_iter()
        .take(limit)
        .filter_map(move |object| match object {
            MaybeRef::Ref(reference) => pdf.xref().get::<Dict<'_>>(reference.into()),
            MaybeRef::NotRef(Object::Dict(dict)) => Some(dict),
            _ => None,
        })
}

/// Resolve complete key/value pairs while preserving raw slot alignment.
/// Unresolved pairs and repeated keys still consume the pair budget.
pub fn pairs<'a>(
    pdf: &'a Pdf,
    array: &Array<'a>,
    limit: usize,
) -> impl Iterator<Item = (Object<'a>, Object<'a>)> + 'a {
    let mut items = array.raw_iter();
    let resolve = move |object| match object {
        MaybeRef::Ref(reference) => pdf.xref().get::<Object<'_>>(reference.into()),
        MaybeRef::NotRef(object) => Some(object),
    };
    std::iter::from_fn(move || Some((items.next()?, items.next()?)))
        .take(limit)
        .filter_map(move |(key, value)| Some((resolve(key)?, resolve(value)?)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unresolved_pairs_consume_the_budget_and_trailing_keys_are_ignored() {
        let mut bytes = include_bytes!("../assets/folio-guide.pdf").to_vec();
        for (original, replacement) in [
            (
                b"/Nums [2 << /S /D >> 4 ".as_slice(),
                b"/Nums [2 999 0 R     4 ".as_slice(),
            ),
            (b"/P (A-) >>]", b"/P (A-)>>0]"),
        ] {
            assert_eq!(original.len(), replacement.len());
            let offset = bytes
                .windows(original.len())
                .position(|value| value == original)
                .unwrap();
            bytes[offset..offset + original.len()].copy_from_slice(replacement);
        }
        let pdf = Pdf::new(bytes).unwrap();
        let catalog = pdf.xref().get::<Dict<'_>>(pdf.xref().root_id()).unwrap();
        let root = catalog.get::<Dict<'_>>(b"PageLabels").unwrap();
        let kids = root.get::<Array<'_>>(b"Kids").unwrap();
        let leaf = dictionaries(&pdf, &kids, 2).nth(1).unwrap();
        let nums = leaf.get::<Array<'_>>(b"Nums").unwrap();
        for (limit, expected) in [(0, 0), (1, 0), (2, 1), (3, 1), (10000, 1)] {
            let indices: Vec<_> = pairs(&pdf, &nums, limit)
                .map(|(key, _)| key.into_i32().unwrap())
                .collect();
            assert_eq!(indices.len(), expected);
            assert!(indices.iter().all(|index| *index == 4));
        }
    }

    #[test]
    fn invalid_entries_consume_the_scan_budget_and_valid_references_still_resolve() {
        let mut bytes = include_bytes!("../assets/folio-guide.pdf").to_vec();
        let marker = b"/Annots [<< /Type /Annot ";
        let offset = bytes
            .windows(marker.len())
            .position(|value| value == marker)
            .unwrap();
        bytes[offset..offset + marker.len()].copy_from_slice(b"/Annots [null 999 0 R << ");
        let pdf = Pdf::new(bytes).unwrap();
        let annotations = pdf.pages()[0].raw().get::<Array<'_>>(b"Annots").unwrap();
        for (limit, expected) in [(0, 0), (1, 0), (2, 0), (3, 1), (4, 2), (1000, 2)] {
            assert_eq!(dictionaries(&pdf, &annotations, limit).count(), expected);
        }
        let catalog = pdf.xref().get::<Dict<'_>>(pdf.xref().root_id()).unwrap();
        let pages = catalog.get::<Dict<'_>>(b"Pages").unwrap();
        let kids = pages.get::<Array<'_>>(b"Kids").unwrap();
        let ids: Vec<_> = dictionaries(&pdf, &kids, 1000)
            .map(|page| page.obj_id())
            .collect();
        assert_eq!(
            ids,
            pdf.pages()
                .iter()
                .map(|page| page.raw().obj_id())
                .collect::<Vec<_>>()
        );
    }
}
