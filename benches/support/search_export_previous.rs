// Frozen export implementation from e7a4ff5; unchanged formatting and limits.
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
