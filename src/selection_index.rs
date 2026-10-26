use crate::text::TextSpan;
use hayro::kurbo::Point;

#[derive(Clone, Debug)]
pub struct SelectionIndex {
    by_top: Vec<usize>,
    max_bottom: Vec<f64>,
    by_bottom: Vec<usize>,
    inverted: Vec<usize>,
}

impl SelectionIndex {
    pub fn new(spans: &[TextSpan]) -> Self {
        let valid = |span: &TextSpan| span.rect.is_finite() && !span.range.is_empty();
        let mut by_top: Vec<_> = spans
            .iter()
            .enumerate()
            .filter(|(_, span)| valid(span) && span.rect.y0 <= span.rect.y1)
            .map(|(index, _)| index)
            .collect();
        let inverted = spans
            .iter()
            .enumerate()
            .filter(|(_, span)| valid(span) && span.rect.y0 > span.rect.y1)
            .map(|(index, _)| index)
            .collect();
        by_top.sort_unstable_by(|&a, &b| {
            spans[a]
                .rect
                .y0
                .total_cmp(&spans[b].rect.y0)
                .then(a.cmp(&b))
        });
        let mut by_bottom = by_top.clone();
        by_bottom.sort_unstable_by(|&a, &b| {
            spans[a]
                .rect
                .y1
                .total_cmp(&spans[b].rect.y1)
                .then(a.cmp(&b))
        });
        let mut maximum = f64::NEG_INFINITY;
        let max_bottom = by_top
            .iter()
            .map(|&index| {
                maximum = maximum.max(spans[index].rect.y1);
                maximum
            })
            .collect();
        Self {
            by_top,
            max_bottom,
            by_bottom,
            inverted,
        }
    }

    pub fn memory_bytes(&self) -> usize {
        (self.by_top.capacity() + self.by_bottom.capacity() + self.inverted.capacity())
            * std::mem::size_of::<usize>()
            + self.max_bottom.capacity() * std::mem::size_of::<f64>()
    }

    pub fn nearest(&self, spans: &[TextSpan], point: Point) -> Option<usize> {
        if !point.x.is_finite() || !point.y.is_finite() {
            return None;
        }
        let mut best: Option<(usize, f64, f64)> = None;
        let mut consider = |index: usize| {
            let rect = spans[index].rect;
            let dx = (rect.x0 - point.x).max(0.0).max(point.x - rect.x1);
            let dy = (rect.y0 - point.y).max(0.0).max(point.y - rect.y1);
            if best.is_none_or(|(previous, by, bx)| {
                dy.total_cmp(&by)
                    .then_with(|| dx.total_cmp(&bx))
                    .then(index.cmp(&previous))
                    .is_lt()
            }) {
                best = Some((index, dy, dx));
            }
        };
        let upper = self
            .by_top
            .partition_point(|&index| spans[index].rect.y0 <= point.y);
        if upper > 0 && self.max_bottom[upper - 1] >= point.y {
            // The prefix maximum lets us stop before rows that cannot overlap y.
            let mut position = upper;
            while position > 0 && self.max_bottom[position - 1] >= point.y {
                position -= 1;
                let index = self.by_top[position];
                if spans[index].rect.y1 >= point.y {
                    consider(index);
                }
            }
        } else {
            if let Some(&first) = self.by_top.get(upper) {
                let distance = spans[first].rect.y0 - point.y;
                for &index in self.by_top[upper..]
                    .iter()
                    .take_while(|&&index| spans[index].rect.y0 - point.y == distance)
                {
                    consider(index);
                }
            }
            let lower = self
                .by_bottom
                .partition_point(|&index| spans[index].rect.y1 < point.y);
            if lower > 0 {
                let distance = point.y - spans[self.by_bottom[lower - 1]].rect.y1;
                for &index in self.by_bottom[..lower]
                    .iter()
                    .rev()
                    .take_while(|&&index| point.y - spans[index].rect.y1 == distance)
                {
                    consider(index);
                }
            }
        }
        // Preserve the old distance rule for malformed, vertically reversed bounds.
        for &index in &self.inverted {
            consider(index);
        }
        best.map(|(index, _, _)| index)
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn indexed_queries_match_linear_selection_for_overlaps_gaps_ties_and_invalid_bounds() {
        use super::*;
        use hayro::kurbo::Rect;
        let mut spans = Vec::new();
        let mut random = 17u64;
        let mut next = || {
            random = random.wrapping_mul(6364136223846793005).wrapping_add(1);
            ((random >> 32) % 400) as f64 - 100.0
        };
        for index in 0..300 {
            let x = next();
            let y = next();
            let rect = Rect::new(x, y, x + next() * 0.2, y + next() * 0.3);
            spans.push(TextSpan {
                range: index..index + 1,
                rect,
            });
        }
        spans.extend([
            TextSpan {
                range: 0..0,
                rect: Rect::ZERO,
            },
            TextSpan {
                range: 0..1,
                rect: Rect::new(f64::NAN, 0.0, 1.0, 1.0),
            },
            TextSpan {
                range: 0..1,
                rect: Rect::new(-0.0, -0.0, 0.0, 0.0),
            },
            TextSpan {
                range: 0..1,
                rect: Rect::ZERO,
            },
        ]);
        let index = SelectionIndex::new(&spans);
        let linear = |point: Point| {
            spans
                .iter()
                .enumerate()
                .filter(|(_, span)| span.rect.is_finite() && !span.range.is_empty())
                .min_by(|(_, a), (_, b)| {
                    let distance = |rect: Rect| {
                        (
                            (rect.y0 - point.y).max(0.0).max(point.y - rect.y1),
                            (rect.x0 - point.x).max(0.0).max(point.x - rect.x1),
                        )
                    };
                    let (ay, ax) = distance(a.rect);
                    let (by, bx) = distance(b.rect);
                    ay.total_cmp(&by).then_with(|| ax.total_cmp(&bx))
                })
                .map(|(index, _)| index)
        };
        for _ in 0..5000 {
            let point = Point::new(next(), next());
            assert_eq!(index.nearest(&spans, point), linear(point), "{point:?}");
        }
        for point in [
            Point::ORIGIN,
            Point::new(-1e300, 1e300),
            Point::new(1e300, -1e300),
        ] {
            assert_eq!(index.nearest(&spans, point), linear(point));
        }
        assert_eq!(index.nearest(&spans, Point::new(f64::NAN, 0.0)), None);
        assert_eq!(SelectionIndex::new(&[]).nearest(&[], Point::ORIGIN), None);
    }
}
