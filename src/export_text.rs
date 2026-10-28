//! Shared plain-text export fields. Each export owns at most one page label.

/// Append Unicode whitespace as spaces, retaining at most `limit` scalars.
/// Preserve ordinary spaces and copy unchanged spans without a temporary string.
pub fn push_single_line(output: &mut String, text: &str, limit: usize) {
    let mut start = 0;
    let mut end = text.len();
    // Bytes beyond the scalar cap cannot affect an all-ASCII retained prefix.
    if text.as_bytes()[..end.min(limit)].is_ascii() {
        end = end.min(limit);
        for (offset, &byte) in text.as_bytes()[..end].iter().enumerate() {
            // char::is_whitespace also includes vertical tab (0x0b).
            if (b'\t'..=b'\r').contains(&byte) {
                output.push_str(&text[start..offset]);
                output.push(' ');
                start = offset + 1;
            }
        }
    } else {
        for (count, (offset, ch)) in text.char_indices().enumerate() {
            if count == limit {
                end = offset;
                break;
            }
            if ch != ' ' && ch.is_whitespace() {
                output.push_str(&text[start..offset]);
                output.push(' ');
                start = offset + ch.len_utf8();
            }
        }
    }
    output.push_str(&text[start..end]);
    if end < text.len() {
        output.push_str("… [truncated]");
    }
}

/// Label metadata stays fixed for one export. Reuse adjacent records' complete
/// label line, including absence; changing page replaces this single buffer.
pub struct PageLabelLine {
    page: Option<usize>,
    line: String,
    limit: usize,
}

impl PageLabelLine {
    pub fn new(limit: usize) -> Self {
        Self {
            page: None,
            line: String::new(),
            limit,
        }
    }

    pub fn push(
        &mut self,
        output: &mut String,
        page: usize,
        resolve: &mut impl FnMut(usize) -> Option<String>,
    ) {
        if self.page != Some(page) {
            self.page = Some(page);
            self.line.clear();
            if let Some(label) = resolve(page) {
                self.line.push_str("PDF page label: ");
                push_single_line(&mut self.line, &label, self.limit);
                self.line.push('\n');
            }
        }
        // Absent labels have no line; avoid even an empty append per record.
        if !self.line.is_empty() {
            output.push_str(&self.line);
        }
    }
}

#[cfg(test)]
#[path = "export_text/tests.rs"]
mod tests;
