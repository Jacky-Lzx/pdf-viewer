//! Plain text for the saved comments in the currently filtered thread order.
use crate::annotations::{Comments, ReviewStatus};
use crate::comment_view::CommentRow;
use crate::export_text::{PageLabelLine, push_single_line};
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
    let mut labels = PageLabelLine::new(usize::MAX);
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
        labels.push(&mut entry, comment.page, &mut label);
        if !comment.author.is_empty() {
            entry.push_str("Author: ");
            push_single_line(&mut entry, &comment.author, usize::MAX);
            entry.push('\n');
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::annotations::{Comment, CommentId};
    use crate::comment_view::CommentView;

    fn comment(slot: usize, contents: &str) -> Comment {
        Comment {
            page: 3,
            slot: Some(slot),
            parent: None,
            review: ReviewStatus::None,
            marked: false,
            states: Vec::new(),
            kind: "Text".into(),
            contents: contents.into(),
            author: "Alice".into(),
            rects: Vec::new(),
            has_appearance: false,
            truncated: false,
            locked: false,
        }
    }

    #[test]
    fn export_uses_filtered_thread_order_full_text_and_metadata() {
        let mut reply = comment(1, "needle 中文😀\nsecond line");
        reply.parent = Some(CommentId { page: 3, slot: 0 });
        reply.author = "Straße\nReviewer".into();
        reply.review = ReviewStatus::Completed;
        reply.marked = true;
        let comments = Comments {
            items: vec![
                reply,
                comment(2, "excluded comment"),
                comment(0, "root context"),
            ],
            limited: false,
        };
        let mut view = CommentView::default();
        view.set_comments(&comments);
        view.filter.query = "needle".into();
        view.update(&comments, 0);
        let report = render(&comments, view.rows(), |_| Some("iv\nappendix".into()));
        assert_eq!((report.copied, report.total), (2, 2));
        assert_eq!(
            report.text,
            "Page 4 · Text\nPDF page label: iv appendix\nAuthor: Alice\n[Thread context]\n\nroot context\n\nPage 4 · Reply\nPDF page label: iv appendix\nAuthor: Straße Reviewer\nReply depth: 1\nStatus: Completed / resolved\nMarked: yes\n\nneedle 中文😀\nsecond line\n\n"
        );
    }

    #[test]
    fn export_marks_incomplete_source_and_keeps_full_loaded_body() {
        let mut entry = comment(0, &"中文😀\n".repeat(100));
        entry.truncated = true;
        entry.author.clear();
        let comments = Comments {
            items: vec![entry],
            limited: true,
        };
        let rows = [CommentRow {
            index: 0,
            depth: 0,
            matched: true,
        }];
        let report = render(&comments, &rows, |_| None);
        assert!(
            report
                .text
                .starts_with("[The loaded annotation list is limited.]\n\nPage 4 · Text\n\n")
        );
        assert!(report.text.contains(&comments.items[0].contents));
        assert!(
            report
                .text
                .ends_with("[Comment text was truncated while reading the PDF.]\n\n")
        );
        assert!(!report.text.contains("PDF page label:"));
        assert_eq!((report.copied, report.total), (1, 1));
    }

    #[test]
    fn byte_limit_stops_at_complete_records_and_reports_exact_counts() {
        let comments = Comments {
            items: (0..10)
                .map(|index| comment(index, &format!("record-{index}: {}", "中文😀".repeat(18))))
                .collect(),
            limited: false,
        };
        let rows: Vec<_> = (0..10)
            .map(|index| CommentRow {
                index,
                depth: 0,
                matched: true,
            })
            .collect();
        let report = render_with_limit(&comments, &rows, |_| None, 1024);
        assert!(report.copied > 0 && report.copied < report.total);
        assert!(report.text.len() <= 1024);
        for (index, comment) in comments.items.iter().enumerate() {
            assert_eq!(
                report.text.contains(&format!("record-{index}:")),
                index < report.copied
            );
            if index < report.copied {
                assert!(report.text.contains(&comment.contents));
            }
        }
        assert!(report.text.ends_with(&format!(
            "[Copied {} of 10 shown comments: clipboard size limit reached.]\n",
            report.copied
        )));
        let none = render_with_limit(&comments, &rows, |_| None, 200);
        assert_eq!((none.copied, none.total), (0, 10));
        assert!(none.text.len() <= 200);
        let empty = render(&comments, &[], |_| panic!("no rows"));
        assert_eq!((empty.copied, empty.total), (0, 0));
        assert!(empty.text.is_empty());
    }

    #[test]
    fn nonmonotonic_rows_reuse_only_adjacent_labels_and_preserve_byte_boundaries() {
        let comments = Comments {
            items: [0, 0, 1, 1, 2, 2, 0]
                .into_iter()
                .enumerate()
                .map(|(index, page)| {
                    let mut entry = comment(index, &format!("record-{index}: 中文😀\nsecond line"));
                    entry.page = page;
                    entry.author = "Alice\u{2028}Reviewer".into();
                    entry
                })
                .collect(),
            limited: true,
        };
        let rows: Vec<_> = (0..comments.items.len())
            .map(|index| CommentRow {
                index,
                depth: 0,
                matched: true,
            })
            .collect();
        let label = |page| match page {
            0 => Some("iv\u{85}附录".into()),
            1 => None,
            2 => Some(String::new()),
            _ => unreachable!(),
        };
        let mut calls = Vec::new();
        let full = render(&comments, &rows, |page| {
            calls.push(page);
            label(page)
        });
        assert_eq!(calls, [0, 1, 2, 0]);
        let mut expected = String::from("[The loaded annotation list is limited.]\n\n");
        let start = expected.len();
        let ends: Vec<_> = comments
            .items
            .iter()
            .map(|comment| {
                let _ = writeln!(expected, "Page {} · Text", comment.page + 1);
                match comment.page {
                    0 => expected.push_str("PDF page label: iv 附录\n"),
                    2 => expected.push_str("PDF page label: \n"),
                    _ => {}
                }
                let _ = write!(
                    expected,
                    "Author: Alice Reviewer\n\n{}\n\n",
                    comment.contents
                );
                expected.len()
            })
            .collect();
        assert_eq!(full.text, expected);
        for limit in 0..=expected.len() + NOTICE_BYTES + 1 {
            let report = render_with_limit(&comments, &rows, label, limit);
            let count = ends.partition_point(|&end| end <= limit.saturating_sub(NOTICE_BYTES));
            let end = count.checked_sub(1).map_or(start, |index| ends[index]);
            let mut text = expected[..end].to_owned();
            if count < rows.len() {
                let _ = writeln!(
                    text,
                    "[Copied {count} of 7 shown comments: clipboard size limit reached.]"
                );
            }
            assert_eq!((report.copied, report.total), (count, 7), "limit {limit}");
            assert_eq!(report.text, text, "limit {limit}");
        }
        assert!(
            !render(&comments, &rows, |_| None)
                .text
                .contains("PDF page label:")
        );
    }
}
