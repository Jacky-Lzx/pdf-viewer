use super::*;

fn previous(text: &str, limit: usize) -> String {
    let mut characters = text.chars();
    let mut output: String = characters
        .by_ref()
        .take(limit)
        .map(|ch| if ch.is_whitespace() { ' ' } else { ch })
        .collect();
    if characters.next().is_some() {
        output.push_str("… [truncated]");
    }
    output
}

#[test]
fn direct_fields_match_character_model_for_controls_unicode_and_limit_boundaries() {
    let whitespace: String = (0..=0x10ffff)
        .filter_map(char::from_u32)
        .filter(|ch| ch.is_whitespace())
        .collect();
    let mut seed = 17_u64;
    let alphabet = [
        'A', ' ', '\n', '\r', '\t', '\u{b}', '\u{85}', '\u{a0}', '\u{2028}', '中', '😀', 'ß',
        '\u{301}', '\0',
    ];
    let random: String = (0..4096)
        .map(|_| {
            seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1);
            alphabet[(seed >> 32) as usize % alphabet.len()]
        })
        .collect();
    for text in [
        String::new(),
        (0..128).map(char::from).collect(),
        whitespace,
        "ASCII text with ordinary spaces".into(),
        format!("{}😀\n", "a".repeat(1024)),
        format!("{}😀\n", "a".repeat(1023)),
        "中文 Straße İ cafe\u{301} 👩‍💻\r\n".repeat(80),
        random,
    ] {
        for limit in [0, 1, 2, 7, 31, 32, 255, 256, 1023, 1024, usize::MAX] {
            let mut actual = String::from("existing prefix: ");
            push_single_line(&mut actual, &text, limit);
            assert_eq!(
                actual,
                format!("existing prefix: {}", previous(&text, limit)),
                "limit={limit}"
            );
        }
        for count in [
            text.chars().count().saturating_sub(1),
            text.chars().count(),
            text.chars().count() + 1,
        ] {
            let mut actual = String::new();
            push_single_line(&mut actual, &text, count);
            assert_eq!(actual, previous(&text, count), "count={count}");
        }
    }
}

#[test]
fn label_lines_reuse_adjacent_pages_and_distinguish_absent_and_empty_labels() {
    let mut calls = Vec::new();
    let mut resolve = |page| {
        calls.push(page);
        match page {
            0 => Some("iv\n附录".into()),
            1 => None,
            _ => Some(String::new()),
        }
    };
    let mut line = PageLabelLine::new(usize::MAX);
    let mut output = String::new();
    for page in [0, 0, 1, 1, 2, 2, 0] {
        line.push(&mut output, page, &mut resolve);
    }
    assert_eq!(calls, [0, 1, 2, 0]);
    assert_eq!(
        output,
        "PDF page label: iv 附录\nPDF page label: iv 附录\nPDF page label: \nPDF page label: \nPDF page label: iv 附录\n"
    );
}

#[test]
fn label_buffers_keep_each_exports_character_policy_and_release_snapshot_state() {
    let label = "😀\n".repeat(600);
    for limit in [1024, usize::MAX] {
        let mut line = PageLabelLine::new(limit);
        let mut output = String::new();
        line.push(&mut output, 3, &mut |_| Some(label.clone()));
        assert_eq!(
            output,
            format!("PDF page label: {}\n", previous(&label, limit))
        );
    }
    let mut output = String::new();
    PageLabelLine::new(1024).push(&mut output, 3, &mut |_| Some("new snapshot".into()));
    assert_eq!(output, "PDF page label: new snapshot\n");
}
