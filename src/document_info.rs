use hayro::hayro_syntax::{Pdf, PdfVersion};

#[derive(Default)]
pub struct DocumentInfo {
    pub version: &'static str,
    pub bytes: usize,
    pub fields: Vec<(&'static str, String)>,
}

const MAX_FIELD_BYTES: usize = 16 * 1024;
const MAX_FIELD_CHARS: usize = 2048;

fn field(bytes: &[u8]) -> Option<String> {
    let mut value = crate::pdf_string::decode(&bytes[..bytes.len().min(MAX_FIELD_BYTES)]);
    let truncated = value
        .char_indices()
        .nth(MAX_FIELD_CHARS)
        .map(|(offset, _)| offset);
    if let Some(offset) = truncated {
        value.truncate(offset);
    }
    let mut value = value.trim().to_owned();
    if truncated.is_some() || bytes.len() > MAX_FIELD_BYTES {
        value.push('…');
    }
    (!value.is_empty()).then_some(value)
}

impl DocumentInfo {
    pub fn read(pdf: &Pdf) -> Self {
        let metadata = pdf.metadata();
        let fields = [
            ("Title", &metadata.title),
            ("Author", &metadata.author),
            ("Subject", &metadata.subject),
            ("Keywords", &metadata.keywords),
            ("Creator", &metadata.creator),
            ("Producer", &metadata.producer),
        ]
        .into_iter()
        .filter_map(|(label, bytes)| Some((label, field(bytes.as_deref()?)?)))
        .collect();
        Self {
            version: match pdf.version() {
                PdfVersion::Pdf10 => "1.0",
                PdfVersion::Pdf11 => "1.1",
                PdfVersion::Pdf12 => "1.2",
                PdfVersion::Pdf13 => "1.3",
                PdfVersion::Pdf14 => "1.4",
                PdfVersion::Pdf15 => "1.5",
                PdfVersion::Pdf16 => "1.6",
                PdfVersion::Pdf17 => "1.7",
                PdfVersion::Pdf20 => "2.0",
            },
            bytes: pdf.data().as_ref().len(),
            fields,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn real_document_reports_version_and_size_without_inventing_missing_fields() {
        let data = include_bytes!("../assets/folio-guide.pdf");
        let pdf = Pdf::new(data.to_vec()).unwrap();
        let info = DocumentInfo::read(&pdf);
        assert_eq!(info.version, "1.4");
        assert_eq!(info.bytes, data.len());
        assert!(info.fields.is_empty());
    }

    #[test]
    fn metadata_text_is_unicode_safe_bounded_and_marks_truncation() {
        assert!(field(b" \t\r\n").is_none());
        assert_eq!(field(b" author\x84name ").as_deref(), Some("author—name"));
        for text in ["中😀".repeat(3000), "a".repeat(20000)] {
            let mut bytes = vec![0xfe, 0xff];
            bytes.extend(text.encode_utf16().flat_map(u16::to_be_bytes));
            let value = field(&bytes).unwrap();
            assert!(value.ends_with('…'));
            assert!(value.chars().count() <= MAX_FIELD_CHARS + 1);
            assert!(value.len() <= MAX_FIELD_CHARS * 4 + 3);
        }
        let mut bytes = vec![0xfe, 0xff];
        bytes.extend("中文😀".encode_utf16().flat_map(u16::to_be_bytes));
        assert_eq!(field(&bytes).as_deref(), Some("中文😀"));
    }
}
