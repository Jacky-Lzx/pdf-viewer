use super::{Job, Pending};

const SEARCH_BATCHES_PER_PREVIEW: u8 = 8;

/// Called only after foreground work. Search keeps most of the worker's time,
/// while a long query cannot leave visible page previews empty until it ends.
#[derive(Default)]
pub(super) struct Background {
    search_batches: u8,
}

impl Background {
    pub(super) fn next(
        &mut self,
        pending: &mut Pending,
        search_page: Option<usize>,
    ) -> Option<Job> {
        if (search_page.is_none() || self.search_batches >= SEARCH_BATCHES_PER_PREVIEW)
            && let Some(key) = pending.thumbnails.pop_front()
        {
            self.search_batches = 0;
            return Some(Job::Thumbnail(key, pending.thumbnail_revision));
        }
        if let Some(page) = search_page {
            return Some(Job::Search(page));
        }
        self.search_batches = 0;
        None
    }

    pub(super) fn searched(&mut self) {
        // Count completed batches, rather than selections that normalization or
        // newly arrived foreground work may interrupt. Idle periods reset it.
        self.search_batches = self.search_batches.saturating_add(1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::document::RenderKey;

    fn preview(page: usize) -> RenderKey {
        RenderKey {
            page,
            width: 120,
            rotation: 0,
            night: false,
            tile: None,
        }
    }

    #[test]
    fn pending_previews_yield_back_to_search_and_follow_the_latest_view() {
        let mut background = Background::default();
        let mut pending = Pending::default();
        pending.replace_thumbnails(vec![preview(1), preview(2)]);
        for _ in 0..8 {
            assert!(matches!(
                background.next(&mut pending, Some(5)),
                Some(Job::Search(5))
            ));
            background.searched();
        }
        // Scrolling can replace visible previews while search batches run.
        pending.replace_thumbnails(vec![preview(7), preview(8)]);
        assert!(matches!(
            background.next(&mut pending, Some(5)),
            Some(Job::Thumbnail(key, revision)) if key == preview(7) && revision == pending.thumbnail_revision
        ));
        assert!(matches!(
            background.next(&mut pending, Some(5)),
            Some(Job::Search(5))
        ));
        // Stopping/completing a search lets remaining previews run immediately.
        assert!(matches!(
            background.next(&mut pending, None),
            Some(Job::Thumbnail(key, _)) if key == preview(8)
        ));
        assert!(background.next(&mut pending, None).is_none());
    }

    #[test]
    fn no_previews_never_throttle_search_and_idle_discards_the_old_budget() {
        let mut background = Background::default();
        let mut pending = Pending::default();
        for _ in 0..1024 {
            assert!(matches!(
                background.next(&mut pending, Some(0)),
                Some(Job::Search(0))
            ));
            background.searched();
        }
        pending.replace_thumbnails(vec![preview(3)]);
        assert!(matches!(
            background.next(&mut pending, Some(0)),
            Some(Job::Thumbnail(..))
        ));
        for _ in 0..8 {
            background.searched();
        }
        assert!(background.next(&mut pending, None).is_none());
        pending.replace_thumbnails(vec![preview(4)]);
        assert!(matches!(
            background.next(&mut pending, Some(0)),
            Some(Job::Search(0))
        ));
        // Removing previews never leaves the scheduler waiting for a job.
        pending.replace_thumbnails(Vec::new());
        for _ in 0..8 {
            background.searched();
        }
        assert!(matches!(
            background.next(&mut pending, Some(0)),
            Some(Job::Search(0))
        ));
    }
}
