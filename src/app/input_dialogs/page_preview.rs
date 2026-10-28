use std::time::{Duration, Instant};

const DELAY: Duration = Duration::from_millis(150);

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) struct Source {
    pub document: u64,
    pub page: usize,
    pub count: usize,
}

pub(super) struct Target {
    pub page: usize,
    pub caption: String,
    pub tooltip: String,
}

pub(super) enum State<'a> {
    Empty,
    Pending(Duration),
    Missing,
    Target(&'a Target),
}

/// One destination per open dialog; typing defers lookup, confirmation resolves
/// immediately. Repaints reuse both the result and its display strings.
#[derive(Default)]
pub(super) struct Preview {
    value: String,
    source: Option<Source>,
    due: Option<Instant>,
    target: Option<Target>,
}

impl Preview {
    pub fn invalidate(&mut self) {
        self.source = None;
    }

    pub fn update(
        &mut self,
        value: &str,
        source: Source,
        now: Instant,
        confirm: bool,
        resolve: impl FnOnce(&str) -> Option<Target>,
    ) -> State<'_> {
        if self.source != Some(source) || self.value != value {
            self.value.clear();
            self.value.push_str(value);
            self.source = Some(source);
            self.target = None;
            self.due = (!value.trim().is_empty()).then_some(now + DELAY);
        }
        if value.trim().is_empty() {
            return State::Empty;
        }
        if let Some(due) = self.due {
            if !confirm && now < due {
                return State::Pending(due - now);
            }
            self.target = resolve(value);
            self.due = None;
        }
        self.target.as_ref().map_or(State::Missing, State::Target)
    }
}

#[cfg(test)]
mod tests;
