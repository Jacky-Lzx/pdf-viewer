//! Fixed-capacity FIFO storage for immutable, lazily prepared UI values.
use std::{collections::HashMap, hash::Hash};

pub struct FifoCache<K, V, const LIMIT: usize> {
    indices: HashMap<K, usize>,
    slots: Vec<(K, V)>,
    oldest: usize,
}

impl<K, V, const LIMIT: usize> Default for FifoCache<K, V, LIMIT> {
    fn default() -> Self {
        assert!(LIMIT > 0, "FIFO cache capacity must be positive");
        Self {
            indices: HashMap::new(),
            slots: Vec::new(),
            oldest: 0,
        }
    }
}

impl<K: Copy + Eq + Hash, V, const LIMIT: usize> FifoCache<K, V, LIMIT> {
    /// Drop values and both index/slot allocations at a snapshot boundary.
    pub fn clear(&mut self) {
        *self = Self::default();
    }

    /// Hits need one hash lookup and do not change FIFO order. Construction
    /// completes before eviction so a failed factory leaves residents intact.
    pub fn get_or_insert_with(&mut self, key: K, create: impl FnOnce() -> V) -> &V {
        if let Some(&index) = self.indices.get(&key) {
            return &self.slots[index].1;
        }
        let value = create();
        let index = if self.slots.len() < LIMIT {
            let index = self.slots.len();
            self.slots.push((key, value));
            index
        } else {
            let index = self.oldest;
            self.indices.remove(&self.slots[index].0);
            self.slots[index] = (key, value);
            self.oldest = (index + 1) % LIMIT;
            index
        };
        self.indices.insert(key, index);
        &self.slots[index].1
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{collections::VecDeque, rc::Rc};

    fn compare_with_fifo_model<const LIMIT: usize>() {
        let mut cache = FifoCache::<usize, String, LIMIT>::default();
        let mut model = VecDeque::<(usize, String)>::new();
        let (mut created, mut expected_created) = (0, 0);
        let mut random = 31_u64;
        for _ in 0..10_000 {
            random = random.wrapping_mul(6364136223846793005).wrapping_add(1);
            let key = (random >> 32) as usize % (LIMIT * 2 + 1);
            let expected = if let Some((_, value)) = model.iter().find(|(held, _)| *held == key) {
                value.clone()
            } else {
                expected_created += 1;
                let value = format!("{key}: {expected_created}");
                if model.len() == LIMIT {
                    model.pop_front();
                }
                model.push_back((key, value.clone()));
                value
            };
            assert_eq!(
                cache.get_or_insert_with(key, || {
                    created += 1;
                    format!("{key}: {created}")
                }),
                &expected
            );
            assert_eq!(created, expected_created);
            assert_eq!(cache.slots.len(), model.len());
            assert_eq!(cache.indices.len(), model.len());
            assert!(cache.oldest < LIMIT);
            for (key, value) in &model {
                assert_eq!(&cache.slots[cache.indices[key]], &(*key, value.clone()));
            }
        }
    }

    #[test]
    fn hits_and_wraparound_eviction_match_fifo_for_small_and_full_ui_capacities() {
        compare_with_fifo_model::<1>();
        compare_with_fifo_model::<3>();
        compare_with_fifo_model::<128>();
    }

    #[test]
    fn failed_construction_preserves_residents_and_order_before_and_after_capacity() {
        for count in 0..=3 {
            let mut cache = FifoCache::<usize, usize, 3>::default();
            for key in 0..count {
                cache.get_or_insert_with(key, || key);
            }
            let failure = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                cache.get_or_insert_with(99, || panic!("failed factory"));
            }));
            assert!(failure.is_err());
            assert_eq!(cache.slots.len(), count);
            assert_eq!(cache.indices.len(), count);
            assert_eq!(cache.oldest, 0);
            for key in 0..count {
                assert_eq!(
                    *cache.get_or_insert_with(key, || panic!("resident lost")),
                    key
                );
            }
            for key in count..6 {
                cache.get_or_insert_with(key, || key);
            }
            assert_eq!(cache.slots.len(), 3);
            assert_eq!(cache.indices.len(), 3);
            for key in 3..6 {
                assert_eq!(
                    *cache.get_or_insert_with(key, || panic!("wrong eviction")),
                    key
                );
            }
        }
    }

    #[test]
    fn eviction_drops_values_and_clear_releases_all_storage_before_reuse() {
        let mut cache = FifoCache::<usize, Rc<str>, 3>::default();
        let first = Rc::downgrade(cache.get_or_insert_with(0, || Rc::from("old snapshot")));
        for key in 1..4 {
            cache.get_or_insert_with(key, || Rc::from("resident"));
        }
        assert!(first.upgrade().is_none());
        let last = Rc::downgrade(cache.get_or_insert_with(3, || panic!("resident lost")));
        cache.clear();
        assert!(last.upgrade().is_none());
        assert_eq!(cache.indices.capacity(), 0);
        assert_eq!(cache.slots.capacity(), 0);
        assert_eq!(cache.oldest, 0);
        assert_eq!(
            &**cache.get_or_insert_with(0, || Rc::from("new snapshot")),
            "new snapshot"
        );
    }

    #[test]
    #[should_panic(expected = "FIFO cache capacity must be positive")]
    fn zero_capacity_is_rejected() {
        let _ = FifoCache::<usize, (), 0>::default();
    }
}
