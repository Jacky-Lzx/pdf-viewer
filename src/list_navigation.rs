/// A queued selection movement, resolved after the current viewport is known.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Motion {
    Step(isize),
    Page(isize),
    First,
    Last,
}

impl Motion {
    pub fn delta(self, page_rows: usize) -> isize {
        match self {
            Self::Step(delta) => delta,
            Self::Page(direction) => {
                direction.saturating_mul(page_rows.clamp(1, isize::MAX as usize) as isize)
            }
            Self::First => isize::MIN,
            Self::Last => isize::MAX,
        }
    }
}

pub fn page_rows(height: f32, row_height: f32, spacing: f32) -> usize {
    (((height + spacing) / (row_height + spacing)).floor() as usize).max(1)
}
