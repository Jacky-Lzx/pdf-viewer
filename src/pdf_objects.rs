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

#[cfg(test)]
mod tests {
    use super::*;

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
