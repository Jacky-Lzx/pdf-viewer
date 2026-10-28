use crate::page_labels::PageLabels;

pub fn fixture(prefix: &str, style: &str) -> PageLabels {
    let bytes: Vec<_> = [0xfe, 0xff]
        .into_iter()
        .chain(prefix.encode_utf16().flat_map(u16::to_be_bytes))
        .collect();
    let prefix: String = bytes.iter().map(|byte| format!("{byte:02x}")).collect();
    let kids: String = (4..16).map(|id| format!("{id} 0 R ")).collect();
    let mut objects = vec![
        "<< /Type /Catalog /Pages 2 0 R /PageLabels 3 0 R >>".to_owned(),
        format!("<< /Type /Pages /Kids [{kids}] /Count 12 >>"),
        format!("<< /Nums [0 << /S /{style} /P <{prefix}> >>] >>"),
    ];
    objects.extend(
        (0..12).map(|_| "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 600 800] >>".to_owned()),
    );
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
    PageLabels::read(&hayro::hayro_syntax::Pdf::new(data).unwrap())
}
