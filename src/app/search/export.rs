//! A bounded snapshot of available search previews, without page text extraction.
use crate::text::{SearchHit, SearchOptions};
use std::{fmt::Write, path::Path};

const MAX_BYTES: usize = 8 * 1024 * 1024;
const NOTICE_BYTES: usize = 160;
const HEADER_CHARACTERS: usize = 1024;

pub(super) struct Snapshot<'a> {
    pub path: &'a Path,
    pub query: &'a str,
    pub options: SearchOptions,
    pub hits: &'a [SearchHit],
    pub scanned: usize,
    pub pages: usize,
    pub complete: bool,
    pub limited: bool,
}

pub(super) struct Report {
    pub text: String,
    pub copied: usize,
    pub total: usize,
}

pub(super) fn render(snapshot: Snapshot<'_>, label: impl FnMut(usize) -> Option<String>) -> Report {
    render_with_limit(snapshot, label, MAX_BYTES)
}

fn render_with_limit(
    snapshot: Snapshot<'_>,
    mut label: impl FnMut(usize) -> Option<String>,
    limit: usize,
) -> Report {
    let mut text = String::from("Pagewing search snippets\n");
    let _ = writeln!(
        text,
        "File: {}",
        single_line(&snapshot.path.to_string_lossy())
    );
    let _ = writeln!(text, "Query: {}", single_line(snapshot.query));
    let _ = writeln!(
        text,
        "Match case: {}",
        if snapshot.options.match_case {
            "yes"
        } else {
            "no"
        }
    );
    let _ = writeln!(
        text,
        "Whole words: {}",
        if snapshot.options.whole_word {
            "yes"
        } else {
            "no"
        }
    );
    let _ = writeln!(text, "Available results: {}", snapshot.hits.len());
    let _ = writeln!(
        text,
        "Pages scanned: {} / {}",
        snapshot.scanned, snapshot.pages
    );
    text.push_str(if snapshot.limited {
        "[Search stopped at the result limit; more matches may exist.]\n"
    } else if !snapshot.complete {
        "[Search in progress; this snapshot contains the currently available matches.]\n"
    } else {
        "[Search complete.]\n"
    });
    text.push_str("[Snippets are previews, not full page text.]\n\n");
    let mut report = Report {
        text,
        copied: 0,
        total: snapshot.hits.len(),
    };
    let mut entry = String::new();
    for (index, hit) in snapshot.hits.iter().enumerate() {
        entry.clear();
        let _ = writeln!(entry, "Result {} · Page {}", index + 1, hit.page + 1);
        if let Some(label) = label(hit.page) {
            let _ = writeln!(entry, "PDF page label: {}", single_line(&label));
        }
        entry.push('\n');
        // Check before copying a potentially large snippet. Keep whole records
        // and reserve the notice so partial exports always explain their limit.
        let bytes = report
            .text
            .len()
            .saturating_add(entry.len())
            .saturating_add(hit.snippet.len())
            .saturating_add(2);
        if bytes > limit.saturating_sub(NOTICE_BYTES) {
            break;
        }
        report.text.push_str(&entry);
        report.text.push_str(&hit.snippet);
        report.text.push_str("\n\n");
        report.copied += 1;
    }
    if report.copied < report.total {
        let _ = writeln!(
            report.text,
            "[Copied {} of {} available search snippets: clipboard size limit reached.]",
            report.copied, report.total
        );
    }
    report
}

fn single_line(value: &str) -> String {
    let mut characters = value.chars();
    let mut text: String = characters
        .by_ref()
        .take(HEADER_CHARACTERS)
        .map(|c| if c.is_whitespace() { ' ' } else { c })
        .collect();
    if characters.next().is_some() {
        text.push_str("… [truncated]");
    }
    text
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hits() -> Vec<SearchHit> {
        ["前 Straße 后", "Second\nmatch 中文 📝", "Last match"]
            .into_iter()
            .enumerate()
            .map(|(index, snippet)| SearchHit {
                page: index * 2,
                rects: Vec::new(),
                snippet: snippet.into(),
                snippet_match: 0..0,
            })
            .collect()
    }

    fn snapshot(hits: &[SearchHit]) -> Snapshot<'_> {
        Snapshot {
            path: Path::new("/books/中文 notes.pdf"),
            query: "STRASSE\n中文",
            options: SearchOptions {
                match_case: true,
                whole_word: true,
            },
            hits,
            scanned: 6,
            pages: 6,
            complete: true,
            limited: false,
        }
    }

    #[test]
    fn snapshot_keeps_original_snippets_order_options_and_both_page_numbers() {
        let hits = hits();
        let report = render(snapshot(&hits), |page| {
            (page == 0).then(|| "iv\nappendix".into())
        });
        assert_eq!((report.copied, report.total), (3, 3));
        assert_eq!(
            report.text,
            "Pagewing search snippets\nFile: /books/中文 notes.pdf\nQuery: STRASSE 中文\nMatch case: yes\nWhole words: yes\nAvailable results: 3\nPages scanned: 6 / 6\n[Search complete.]\n[Snippets are previews, not full page text.]\n\nResult 1 · Page 1\nPDF page label: iv appendix\n\n前 Straße 后\n\nResult 2 · Page 3\n\nSecond\nmatch 中文 📝\n\nResult 3 · Page 5\n\nLast match\n\n"
        );
    }

    #[test]
    fn partial_searches_and_long_header_fields_are_explicit() {
        let hits = hits();
        let query = "中😀".repeat(5000);
        let mut source = snapshot(&hits);
        source.query = &query;
        source.complete = false;
        source.scanned = 2;
        let report = render(source, |_| None);
        assert!(report.text.contains("Pages scanned: 2 / 6"));
        assert!(report.text.contains("[Search in progress;"));
        assert!(
            report
                .text
                .contains(&format!("Query: {}… [truncated]\n", "中😀".repeat(512)))
        );
        assert!(report.text.len() < 5000);
        let mut source = snapshot(&hits);
        source.limited = true;
        let report = render(source, |_| None);
        assert!(report.text.contains("result limit; more matches may exist"));
        assert!(!report.text.contains("[Search complete.]"));
        assert!(report.text.contains("Result 3 · Page 5"));
    }

    #[test]
    fn clipboard_limit_keeps_whole_unicode_records_and_reports_exact_counts() {
        let mut hits = hits();
        for hit in &mut hits {
            hit.snippet = "中😀".repeat(2000);
        }
        let report = render_with_limit(snapshot(&hits), |_| None, 20_000);
        assert_eq!((report.copied, report.total), (1, 3));
        assert!(report.text.len() <= 20_000);
        assert!(report.text.contains(&hits[0].snippet));
        assert!(!report.text.contains("Result 2"));
        assert!(report.text.ends_with(
            "[Copied 1 of 3 available search snippets: clipboard size limit reached.]\n"
        ));
        let report = render_with_limit(snapshot(&hits), |_| None, 1024);
        assert_eq!((report.copied, report.total), (0, 3));
        assert!(report.text.len() <= 1024);
        assert!(!report.text.contains("Result 1"));
    }
}
