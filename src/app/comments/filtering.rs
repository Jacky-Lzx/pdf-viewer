use super::{CommentFilter, CommentUi};
use crate::list_navigation::Motion;
use std::time::Duration;

pub(super) struct FilterNavigation {
    filter: CommentFilter,
    page: Option<usize>,
    motions: Vec<Motion>,
    confirm: bool,
}

impl CommentUi {
    pub(super) fn advance_filter(&mut self, page: usize, focused: bool, budget: Duration) {
        if self.loaded {
            self.view.update_for(&self.comments, page, budget);
        }
        let page = self.view.filter.current_page.then_some(page);
        if self.filter_navigation.as_ref().is_some_and(|navigation| {
            navigation.filter != self.view.filter || navigation.page != page
        }) {
            self.filter_navigation = None;
        }
        if !focused {
            self.clear_filter_navigation();
        } else if !self.loaded || self.view.progress().is_some() {
            if !self.filter_motions.is_empty() || self.filter_confirm {
                let navigation = self
                    .filter_navigation
                    .get_or_insert_with(|| FilterNavigation {
                        filter: self.view.filter.clone(),
                        page,
                        motions: Vec::new(),
                        confirm: false,
                    });
                navigation.motions.append(&mut self.filter_motions);
                navigation.confirm |= std::mem::take(&mut self.filter_confirm);
            }
        } else if let Some(mut navigation) = self.filter_navigation.take() {
            // Earlier motions precede the keys received in the completion frame.
            navigation.motions.append(&mut self.filter_motions);
            self.filter_motions = navigation.motions;
            self.filter_confirm |= navigation.confirm;
        }
    }

    pub(super) fn clear_filter_navigation(&mut self) {
        self.filter_navigation = None;
        self.filter_motions.clear();
        self.filter_confirm = false;
    }
}
