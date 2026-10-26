use hayro::kurbo::Rect;
use std::ops::Range;

#[derive(Clone, Debug)]
pub struct TextSpan {
    pub range: Range<usize>,
    pub rect: Rect,
}
