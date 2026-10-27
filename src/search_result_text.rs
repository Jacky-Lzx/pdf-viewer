//! Bounded presentation text for one append-only search/page-label snapshot.
use eframe::egui::{self, Color32, FontId, TextFormat, text::LayoutJob};
use std::{
    collections::{HashMap, VecDeque},
    ops::Range,
    sync::Arc,
};

const RESULT_LIMIT: usize = 128;

pub fn result_label(
    page: String,
    snippet: &str,
    matched: Range<usize>,
    font: FontId,
    highlight: Color32,
) -> LayoutJob {
    let matched = if snippet.get(matched.clone()).is_some() {
        matched
    } else {
        // Malformed presentation metadata must not split a UTF-8 scalar.
        0..0
    };
    // The full 32-character leading context belongs in the tooltip. Keep the
    // actual hit near the start of a narrow row instead of truncating it away.
    let first = snippet[..matched.start]
        .char_indices()
        .rev()
        .nth(5)
        .map_or(0, |(offset, _)| offset);
    let ellipsis = if first > 0 { "…" } else { "" };
    let prefix = page.len() + 3 + ellipsis.len();
    let text = format!("{page}   {ellipsis}{}", &snippet[first..]);
    let matched = prefix + matched.start - first..prefix + matched.end - first;
    let mut job = LayoutJob {
        text,
        sections: Vec::with_capacity(3),
        ..Default::default()
    };
    for (range, background) in [
        (0..matched.start, Color32::TRANSPARENT),
        (matched.clone(), highlight),
        (matched.end..job.text.len(), Color32::TRANSPARENT),
    ] {
        if !range.is_empty() {
            job.sections.push(egui::text::LayoutSection {
                leading_space: 0.0,
                byte_range: range,
                format: TextFormat {
                    font_id: font.clone(),
                    color: Color32::PLACEHOLDER,
                    background,
                    valign: egui::Align::Center,
                    ..Default::default()
                },
            });
        }
    }
    job
}

#[derive(Default)]
pub struct ResultTextCache {
    entries: HashMap<usize, Arc<LayoutJob>>,
    order: VecDeque<usize>,
    style: Option<(FontId, Color32)>,
}

impl ResultTextCache {
    /// Indices belong to one query/document. Release all storage at that boundary.
    pub fn clear(&mut self) {
        *self = Self::default();
    }

    pub fn set_style(&mut self, font: FontId, highlight: Color32) {
        if self.style.as_ref() != Some(&(font.clone(), highlight)) {
            self.clear();
            self.style = Some((font, highlight));
        }
    }

    /// Width, selection and text color remain live widget inputs. Only label
    /// assembly and section formatting are cached, never galleys or GPU data.
    pub fn get_or_insert_with(
        &mut self,
        index: usize,
        create: impl FnOnce() -> LayoutJob,
    ) -> Arc<LayoutJob> {
        if self.entries.len() == RESULT_LIMIT && !self.entries.contains_key(&index) {
            let oldest = self
                .order
                .pop_front()
                .expect("a full cache has an oldest result");
            self.entries.remove(&oldest);
        }
        Arc::clone(self.entries.entry(index).or_insert_with(|| {
            self.order.push_back(index);
            Arc::new(create())
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn label() -> LayoutJob {
        result_label(
            "iv · 4".into(),
            "前 Straße 后",
            4..11,
            FontId::default(),
            Color32::YELLOW,
        )
    }

    #[test]
    fn labels_highlight_the_source_range_and_keep_context_and_page_label_plain() {
        let job = label();
        assert_eq!(job.text, "iv · 4   前 Straße 后");
        assert_eq!(job.sections.len(), 3);
        let matched = &job.sections[1];
        assert_eq!(&job.text[matched.byte_range.clone()], "Straße");
        assert_eq!(matched.format.background, Color32::YELLOW);
        let context = format!("{}Straße tail", "前".repeat(32));
        let job = result_label(
            "1".into(),
            &context,
            96..103,
            FontId::default(),
            Color32::YELLOW,
        );
        assert_eq!(job.text, format!("1   …{}Straße tail", "前".repeat(6)));
        assert_eq!(&job.text[job.sections[1].byte_range.clone()], "Straße");
        for range in [0..0, 1..2, 20..30, Range { start: 8, end: 4 }] {
            let job = result_label(
                "1".into(),
                "中文",
                range,
                FontId::default(),
                Color32::YELLOW,
            );
            assert!(
                job.sections
                    .iter()
                    .all(|section| section.format.background == Color32::TRANSPARENT)
            );
        }
    }

    #[test]
    fn cache_is_bounded_reuses_jobs_and_releases_storage_on_query_or_style_change() {
        let mut cache = ResultTextCache::default();
        let font = FontId::default();
        cache.set_style(font.clone(), Color32::YELLOW);
        let first = cache.get_or_insert_with(0, label);
        cache.set_style(font.clone(), Color32::YELLOW);
        assert!(Arc::ptr_eq(
            &first,
            &cache.get_or_insert_with(0, || panic!("cache miss"))
        ));
        for index in 1..10_000 {
            cache.get_or_insert_with(index, label);
            assert!(cache.entries.len() <= RESULT_LIMIT);
            assert_eq!(cache.entries.len(), cache.order.len());
        }
        for index in 10_000 - RESULT_LIMIT..10_000 {
            cache.get_or_insert_with(index, || panic!("visible result evicted"));
        }
        cache.set_style(FontId::proportional(24.0), Color32::YELLOW);
        assert!(cache.entries.is_empty());
        cache.get_or_insert_with(0, label);
        cache.set_style(FontId::proportional(24.0), Color32::GOLD);
        assert!(cache.entries.is_empty());
        cache.get_or_insert_with(0, label);
        cache.clear();
        assert_eq!(cache.entries.capacity(), 0);
        assert_eq!(cache.order.capacity(), 0);
        assert!(cache.style.is_none());
        // Eviction must not invalidate a job still held by a rendered widget.
        assert_eq!(first.text, "iv · 4   前 Straße 后");
    }
}
