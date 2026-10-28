use super::*;

#[test]
fn phrase_search_crosses_pdf_line_breaks_and_retains_source_geometry() {
    let value = "before Straße\r\n\t  中文\u{a0}reader after\nSTRASSE 中文 reader";
    let page = PageText {
        text: value.into(),
        spans: value
            .char_indices()
            .enumerate()
            .filter(|(_, (_, ch))| !ch.is_whitespace())
            .map(|(glyph, (start, ch))| TextSpan {
                range: start..start + ch.len_utf8(),
                rect: Rect::new(glyph as f64 * 8.0, 0.0, glyph as f64 * 8.0 + 6.0, 10.0),
            })
            .collect(),
        ..Default::default()
    };
    for whole_word in [false, true] {
        for match_case in [false, true] {
            let query = SearchQuery::new(
                "Straße 中文 reader".into(),
                SearchOptions {
                    match_case,
                    whole_word,
                },
            );
            let mut cursor = SearchCursor::default();
            let mut hits = Vec::new();
            loop {
                let batch = page.search_batch(3, &query, &mut cursor, 1);
                hits.extend(batch.hits);
                if batch.complete {
                    break;
                }
            }
            assert_eq!(hits.len(), if match_case { 1 } else { 2 });
            let start = value.find("Straße").unwrap();
            let end = value.find(" after").unwrap();
            assert_eq!(hits[0].rects, page.rects_for_range(start..end));
            assert_eq!(
                (hits[0].snippet.clone(), hits[0].snippet_match.clone()),
                snippet::snippet(value, start..end)
            );
            assert_eq!(hits[0].page, 3);
        }
    }
    assert_eq!(page.text, value);
}

#[test]
fn phrase_anchors_preserve_order_overlap_and_bounded_batch_progress() {
    for (value, needle, expected) in [
        ("a\r\nlongest\tend a longest end", "a longest end", 2),
        ("longest\nend longest end", "longest end", 2),
        ("a\nlongest a longest", "a longest", 2),
        ("a\n a\t a a", "a a", 2),
        ("aa\naa aa a", "a aa", 1),
        ("a longestX a longest", "a longest", 2),
        ("a X longest a longest", "a longest", 1),
        ("a-longest a\u{2003}longest", "a longest", 1),
        ("longest\r\n a longest\t", "longest ", 2),
        ("\tlongest a\nlongest", " longest", 2),
        ("alongest", "a longest", 0),
    ] {
        let page = PageText {
            text: value.into(),
            ..Default::default()
        };
        for match_case in [true, false] {
            let query = SearchQuery::new(
                needle.into(),
                SearchOptions {
                    match_case,
                    whole_word: false,
                },
            );
            let mut cursor = SearchCursor::default();
            let mut count = 0;
            loop {
                let batch = page.search_batch(0, &query, &mut cursor, 1);
                count += batch.hits.len();
                if batch.complete {
                    break;
                }
            }
            assert_eq!(count, expected, "{value:?} / {needle:?}");
        }
    }
    // Yield after rejected anchors too, then allow a prefix before the
    // resumed anchor offset without crossing an already accepted phrase.
    let page = PageText {
        text: format!(
            "{}a\nlongest a longest a longest",
            "longest x ".repeat(4096)
        ),
        ..Default::default()
    };
    let query = SearchQuery::new("a longest".into(), SearchOptions::default());
    let mut cursor = SearchCursor::default();
    let first = page.search_batch(0, &query, &mut cursor, 1);
    assert!(first.hits.is_empty() && !first.complete);
    for _ in 0..3 {
        assert_eq!(page.search_batch(0, &query, &mut cursor, 1).hits.len(), 1);
    }
    assert!(page.search_batch(0, &query, &mut cursor, 1).complete);
}

#[test]
fn phrase_ranges_match_an_independent_normalized_reference() {
    fn normalize(value: &str, fold: bool) -> (String, Vec<Range<usize>>) {
        let mut text = String::new();
        let mut source: Vec<Range<usize>> = Vec::new();
        for (start, ch) in value.char_indices() {
            let end = start + ch.len_utf8();
            if ch.is_whitespace() {
                if text.ends_with(' ') {
                    source.last_mut().unwrap().end = end;
                } else {
                    text.push(' ');
                    source.push(start..end);
                }
            } else {
                let character = ch.to_string();
                let character = if fold {
                    caseless::default_case_fold_str(&character)
                } else {
                    character
                };
                source.extend(std::iter::repeat_n(start..end, character.len()));
                text.push_str(&character);
            }
        }
        (text, source)
    }
    for separator in [" ", "\n", "\r\n\t  ", "\u{a0}", "\u{2003}\n"] {
        let value = format!(
            " a{separator}longest{separator}ß 中{separator}longest{separator}a a{separator}a a a STRASSE{separator}word\nword{separator}ß \0 "
        );
        let page = PageText {
            spans: value
                .char_indices()
                .enumerate()
                .map(|(glyph, (start, ch))| TextSpan {
                    range: start..start + ch.len_utf8(),
                    rect: Rect::new(glyph as f64 * 8.0, 0.0, glyph as f64 * 8.0 + 6.0, 10.0),
                })
                .collect(),
            text: value.clone(),
            ..Default::default()
        };
        for match_case in [false, true] {
            let (normalized, source) = normalize(&value, !match_case);
            for needle in [
                "a longest",
                "longest ß",
                "中 longest a",
                "a a",
                " STRASSE ",
                "word ss",
                "s word",
                "word s",
                "a \n a",
                "longest missing",
                " \0 ",
            ] {
                let (pattern, _) = normalize(needle, !match_case);
                let reference: Vec<_> = normalized
                    .match_indices(&pattern)
                    .map(|(start, matched)| {
                        let range = source[start].start..source[start + matched.len() - 1].end;
                        (
                            page.rects_for_range(range.clone()),
                            snippet::snippet(&value, range),
                        )
                    })
                    .collect();
                let query = SearchQuery::new(
                    needle.into(),
                    SearchOptions {
                        match_case,
                        whole_word: false,
                    },
                );
                for limit in [1, 3, 256] {
                    let mut cursor = SearchCursor::default();
                    let mut actual = Vec::new();
                    loop {
                        let batch = page.search_batch(0, &query, &mut cursor, limit);
                        actual.extend(
                            batch
                                .hits
                                .into_iter()
                                .map(|hit| (hit.rects, (hit.snippet, hit.snippet_match))),
                        );
                        if batch.complete {
                            break;
                        }
                    }
                    assert_eq!(
                        actual, reference,
                        "{separator:?} / {needle:?} / case={match_case} / batch={limit}"
                    );
                }
            }
        }
    }
    let page = PageText {
        text: "word\nß word\ns s\nword ß\nword".into(),
        ..Default::default()
    };
    for needle in ["word s", "s word"] {
        let query = SearchQuery::new(
            needle.into(),
            SearchOptions {
                match_case: false,
                whole_word: true,
            },
        );
        assert_eq!(page.search_with(0, &query).len(), 1, "{needle}");
    }
}
