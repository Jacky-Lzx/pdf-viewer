// Frozen export implementation from e7a4ff5; unchanged formatting and limits.
//! Plain text for the saved comments in the currently filtered thread order.
use crate::annotations::{Comments, ReviewStatus};
use crate::comment_view::CommentRow;
use std::fmt::Write;

const MAX_BYTES: usize = 8 * 1024 * 1024;
// Reserve space for the explicit partial-copy notice without splitting a record.
const NOTICE_BYTES: usize = 160;

pub(super) struct Report {
    pub text: String,
    pub copied: usize,
    pub total: usize,
}

pub(super) fn render(
    comments: &Comments,
    rows: &[CommentRow],
    label: impl FnMut(usize) -> Option<String>,
) -> Report {
    render_with_limit(comments, rows, label, MAX_BYTES)
}

fn render_with_limit(
    comments: &Comments,
    rows: &[CommentRow],
    mut label: impl FnMut(usize) -> Option<String>,
    limit: usize,
) -> Report {
    let mut report = Report {
        text: String::new(),
        copied: 0,
        total: rows.len(),
    };
    if comments.limited {
        report
            .text
            .push_str("[The loaded annotation list is limited.]\n\n");
    }
    let mut entry = String::new();
    for row in rows {
        let comment = &comments.items[row.index];
        entry.clear();
        let _ = writeln!(
            entry,
            "Page {} · {}",
            comment.page + 1,
            if comment.parent.is_some() {
                "Reply"
            } else {
                &comment.kind
            }
        );
        if let Some(label) = label(comment.page) {
            let _ = writeln!(entry, "PDF page label: {}", single_line(&label));
        }
        if !comment.author.is_empty() {
            let _ = writeln!(entry, "Author: {}", single_line(&comment.author));
        }
        if comment.parent.is_some() {
            let _ = writeln!(entry, "Reply depth: {}", row.depth);
        }
        if comment.review != ReviewStatus::None {
            let _ = writeln!(entry, "Status: {}", comment.review.label());
        }
        if comment.marked {
            entry.push_str("Marked: yes\n");
        }
        if !row.matched {
            entry.push_str("[Thread context]\n");
        }
        entry.push('\n');
        entry.push_str(&comment.contents);
        if comment.truncated {
            entry.push_str("\n[Comment text was truncated while reading the PDF.]");
        }
        entry.push_str("\n\n");
        if report.text.len().saturating_add(entry.len()) > limit.saturating_sub(NOTICE_BYTES) {
            break;
        }
        report.text.push_str(&entry);
        report.copied += 1;
    }
    if report.copied < report.total {
        let _ = writeln!(
            report.text,
            "[Copied {} of {} shown comments: clipboard size limit reached.]",
            report.copied, report.total
        );
    }
    report
}

fn single_line(text: &str) -> String {
    text.chars()
        .map(|c| if c.is_whitespace() { ' ' } else { c })
        .collect()
}
