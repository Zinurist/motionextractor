use std::collections::VecDeque;

/// A tiny least-recently-used map for a handful of entries.
pub struct Lru<K, V> {
    capacity: usize,
    items: VecDeque<(K, V)>,
}

impl<K: PartialEq, V: Clone> Lru<K, V> {
    pub fn new(capacity: usize) -> Self {
        Self {
            capacity,
            items: VecDeque::with_capacity(capacity),
        }
    }

    pub fn get(&mut self, key: &K) -> Option<V> {
        let pos = self.items.iter().position(|(k, _)| k == key)?;
        let item = self.items.remove(pos)?;
        let value = item.1.clone();
        self.items.push_back(item);
        Some(value)
    }

    pub fn insert(&mut self, key: K, value: V) {
        if let Some(pos) = self.items.iter().position(|(k, _)| *k == key) {
            self.items.remove(pos);
        }
        if self.items.len() == self.capacity {
            self.items.pop_front();
        }
        self.items.push_back((key, value));
    }

    pub fn clear(&mut self) {
        self.items.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn evicts_least_recently_used() {
        let mut l = Lru::new(2);
        l.insert(1, "a");
        l.insert(2, "b");
        assert_eq!(l.get(&1), Some("a"));
        l.insert(3, "c");
        assert_eq!(l.get(&2), None);
        assert_eq!(l.get(&1), Some("a"));
        assert_eq!(l.get(&3), Some("c"));
        l.insert(3, "d");
        assert_eq!(l.get(&3), Some("d"));
        l.clear();
        assert_eq!(l.get(&1), None);
    }
}
