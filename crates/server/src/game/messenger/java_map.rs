//! Java's `HashMap<Integer, V>` iteration order, for the messenger's friends and requests.
//!
//! An `Integer` key hashes to its value, spread as `h ^ (h >>> 16)`, and lands in bucket
//! `hash & (capacity - 1)`; iteration goes bucket by bucket, and within a bucket in insertion
//! order (a resize keeps that order). The table starts at 16 and doubles once the map holds more
//! than three quarters of it; it never shrinks, not on `remove` and not on `clear`.

#[derive(Debug, Clone)]
pub struct JavaIntMap<V> {
    /// Entries in insertion order (a replaced value keeps its place).
    entries: Vec<(u32, V)>,
    capacity: usize,
}

impl<V> Default for JavaIntMap<V> {
    fn default() -> Self {
        Self {
            entries: Vec::new(),
            capacity: 16,
        }
    }
}

impl<V> JavaIntMap<V> {
    fn bucket(&self, key: u32) -> usize {
        ((key ^ (key >> 16)) as usize) & (self.capacity - 1)
    }

    /// Java: `put`.
    pub fn insert(&mut self, key: u32, value: V) -> Option<V> {
        if let Some(slot) = self.entries.iter_mut().find(|(k, _)| *k == key) {
            return Some(std::mem::replace(&mut slot.1, value));
        }
        self.entries.push((key, value));
        while self.entries.len() as f64 > self.capacity as f64 * 0.75 {
            self.capacity *= 2;
        }
        None
    }

    pub fn remove(&mut self, key: &u32) -> Option<V> {
        let i = self.entries.iter().position(|(k, _)| k == key)?;
        Some(self.entries.remove(i).1)
    }

    pub fn get(&self, key: &u32) -> Option<&V> {
        self.entries.iter().find(|(k, _)| k == key).map(|(_, v)| v)
    }

    pub fn get_mut(&mut self, key: &u32) -> Option<&mut V> {
        self.entries
            .iter_mut()
            .find(|(k, _)| k == key)
            .map(|(_, v)| v)
    }

    pub fn contains_key(&self, key: &u32) -> bool {
        self.entries.iter().any(|(k, _)| k == key)
    }

    #[allow(clippy::len_without_is_empty)]
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn clear(&mut self) {
        self.entries.clear();
    }

    /// The entries in Java's iteration order.
    pub fn iter(&self) -> impl ExactSizeIterator<Item = (&u32, &V)> {
        let mut order: Vec<usize> = (0..self.entries.len()).collect();
        order.sort_by_key(|&i| self.bucket(self.entries[i].0));
        order
            .into_iter()
            .map(|i| (&self.entries[i].0, &self.entries[i].1))
    }

    pub fn keys(&self) -> impl ExactSizeIterator<Item = &u32> {
        self.iter().map(|(k, _)| k)
    }

    pub fn values(&self) -> impl ExactSizeIterator<Item = &V> {
        self.iter().map(|(_, v)| v)
    }
}

impl<V> FromIterator<(u32, V)> for JavaIntMap<V> {
    fn from_iter<I: IntoIterator<Item = (u32, V)>>(iter: I) -> Self {
        let mut map = Self::default();
        for (k, v) in iter {
            map.insert(k, v);
        }
        map
    }
}

#[cfg(test)]
mod tests {
    use super::JavaIntMap;

    #[test]
    fn iterates_like_a_java_hash_map() {
        // 17 lands in bucket 1, before 3; 33 shares bucket 1 and comes after 17.
        let map: JavaIntMap<()> = [(3, ()), (17, ()), (33, ()), (2, ())].into_iter().collect();
        assert_eq!(map.keys().copied().collect::<Vec<_>>(), [17, 33, 2, 3]);
        // Thirteen entries grow the table to 32: 17 and 33 part ways.
        let map: JavaIntMap<()> = (1..=13).chain([17, 33]).map(|k| (k, ())).collect();
        let keys: Vec<u32> = map.keys().copied().collect();
        assert_eq!(&keys[..3], &[1, 33, 2]);
        assert_eq!(keys[keys.len() - 1], 17);
    }

    #[test]
    fn the_table_never_shrinks() {
        let mut map: JavaIntMap<()> = (1..=13).map(|k| (k, ())).collect();
        map.clear();
        map.insert(17, ());
        map.insert(1, ());
        // Still 32 buckets: 1 before 17.
        assert_eq!(map.keys().copied().collect::<Vec<_>>(), [1, 17]);
    }
}
