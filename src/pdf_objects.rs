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
        .filter_map(move |object| dictionary(pdf, object))
}

fn dictionary<'a>(pdf: &'a Pdf, object: MaybeRef<Object<'a>>) -> Option<Dict<'a>> {
    match object {
        MaybeRef::Ref(reference) => pdf.xref().get::<Dict<'_>>(reference.into()),
        MaybeRef::NotRef(Object::Dict(dict)) => Some(dict),
        _ => None,
    }
}

fn resolve<'a>(pdf: &'a Pdf, object: MaybeRef<Object<'a>>) -> Option<Object<'a>> {
    match object {
        MaybeRef::Ref(reference) => pdf.xref().get::<Object<'_>>(reference.into()),
        MaybeRef::NotRef(object) => Some(object),
    }
}

/// One tree-wide raw array budget, independent of its per-node/entry limits.
pub struct ScanBudget {
    remaining: usize,
}

impl Default for ScanBudget {
    fn default() -> Self {
        Self { remaining: 100_000 }
    }
}

impl ScanBudget {
    pub fn is_empty(&self) -> bool {
        self.remaining == 0
    }

    fn items<'a: 'b, 'b>(
        &'b mut self,
        array: &Array<'a>,
        limit: usize,
    ) -> impl Iterator<Item = MaybeRef<Object<'a>>> + 'b {
        let mut items = array.raw_iter().take(limit);
        std::iter::from_fn(move || {
            if self.is_empty() {
                return None;
            }
            let item = items.next()?;
            self.remaining -= 1;
            Some(item)
        })
    }

    pub fn dictionaries<'a: 'b, 'b>(
        &'b mut self,
        pdf: &'a Pdf,
        array: &Array<'a>,
        limit: usize,
    ) -> impl Iterator<Item = Dict<'a>> + 'b {
        self.items(array, limit)
            .filter_map(move |object| dictionary(pdf, object))
    }

    /// Resolve complete pairs while preserving slot alignment. Invalid and
    /// repeated pairs, plus a dangling final key, still spend the raw budget.
    pub fn pairs<'a: 'b, 'b>(
        &'b mut self,
        pdf: &'a Pdf,
        array: &Array<'a>,
        limit: usize,
    ) -> impl Iterator<Item = (Object<'a>, Object<'a>)> + 'b {
        let mut items = self.items(array, limit.saturating_mul(2));
        std::iter::from_fn(move || Some((items.next()?, items.next()?)))
            .filter_map(move |(key, value)| Some((resolve(pdf, key)?, resolve(pdf, value)?)))
    }
}

#[cfg(test)]
pub(crate) fn repeated_tree(items: usize, leaves: usize, kids: bool) -> Pdf {
    let shared = leaves + 6;
    let children: String = (5..shared).map(|id| format!("{id} 0 R ")).collect();
    let mut objects = vec![
        "<< /Type /Catalog /Pages 2 0 R /Names << /Dests 4 0 R >> /PageLabels 4 0 R >>".to_owned(),
        "<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_owned(),
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 600 800] >>".to_owned(),
        format!(
            "<< /Kids [{children}] /Names [(early) [3 0 R /Fit]] /Nums [0 << /S /D /P (early) >>] >>"
        ),
        "<< /Names [(late) [3 0 R /Fit]] /Nums [0 << /S /D /P (late) >>] >>".to_owned(),
    ];
    objects.extend((0..leaves).map(|_| {
        if kids {
            format!("<< /Kids {shared} 0 R >>")
        } else {
            format!("<< /Names {shared} 0 R /Nums {shared} 0 R >>")
        }
    }));
    objects.push(format!(
        "[{}]",
        if kids { "null " } else { "0 0 " }.repeat(items)
    ));
    let mut data = b"%PDF-1.4\n".to_vec();
    let mut offsets = vec![0];
    for (index, object) in objects.into_iter().enumerate() {
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
            let indices: Vec<_> = ScanBudget::default()
                .pairs(&pdf, &nums, limit)
                .map(|(key, _)| key.into_i32().unwrap())
                .collect();
            assert_eq!(indices.len(), expected);
            assert!(indices.iter().all(|index| *index == 4));
        }
        for remaining in 0..=6 {
            let mut budget = ScanBudget { remaining };
            let indices: Vec<_> = budget
                .pairs(&pdf, &nums, 10_000)
                .map(|(key, _)| key.into_i32().unwrap())
                .collect();
            assert_eq!(indices, if remaining >= 4 { vec![4] } else { vec![] });
            assert_eq!(budget.remaining, remaining.saturating_sub(5));
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
        let mut budget = ScanBudget { remaining: 5 };
        assert_eq!(budget.dictionaries(&pdf, &annotations, 3).count(), 1);
        assert_eq!(budget.remaining, 2);
        let catalog = pdf.xref().get::<Dict<'_>>(pdf.xref().root_id()).unwrap();
        let labels = catalog.get::<Dict<'_>>(b"PageLabels").unwrap();
        let label_kids = labels.get::<Array<'_>>(b"Kids").unwrap();
        let first = dictionaries(&pdf, &label_kids, 2).next().unwrap();
        let nums = first.get::<Array<'_>>(b"Nums").unwrap();
        assert_eq!(budget.pairs(&pdf, &nums, 10_000).count(), 1);
        assert!(budget.is_empty());
        assert_eq!(budget.dictionaries(&pdf, &annotations, 1000).count(), 0);
        assert_eq!(budget.pairs(&pdf, &nums, 10_000).count(), 0);
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
