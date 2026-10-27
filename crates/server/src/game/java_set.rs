//! The iteration order of a Java `HashSet<String>`, which the protocol exposes: `IGNORED_LIST`
//! sends `Player.getIgnoredList()` (a `HashSet`) in its iteration order.
//!
//! A `HashSet` iterates its buckets in index order, and a bucket in insertion order (a resize
//! splits buckets without reordering them), so the order is "insertion order, stably sorted by
//! bucket index". The index is `(h ^ (h >>> 16)) & (capacity - 1)` with `h = String.hashCode()`,
//! and the capacity starts at 16 and doubles when the size exceeds 3/4 of it (it never shrinks).
//! Buckets that turn into trees (8 colliding names) are not modelled.

/// `String.hashCode()` of a string whose characters are the given ISO-8859-1 bytes.
fn java_hash(s: &[u8]) -> i32 {
    s.iter()
        .fold(0i32, |h, &b| h.wrapping_mul(31).wrapping_add(i32::from(b)))
}

#[derive(Clone, Debug)]
pub struct JavaStringSet {
    items: Vec<Vec<u8>>,
    capacity: usize,
}

impl Default for JavaStringSet {
    fn default() -> Self {
        Self {
            items: Vec::new(),
            capacity: 16,
        }
    }
}

impl JavaStringSet {
    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    pub fn contains(&self, s: &[u8]) -> bool {
        self.items.iter().any(|i| i == s)
    }

    /// Returns whether the set changed.
    pub fn insert(&mut self, s: &[u8]) -> bool {
        if self.contains(s) {
            return false;
        }
        self.items.push(s.to_vec());
        while self.items.len() * 4 > self.capacity * 3 {
            self.capacity *= 2;
        }
        true
    }

    pub fn remove(&mut self, s: &[u8]) -> bool {
        match self.items.iter().position(|i| i == s) {
            Some(pos) => {
                self.items.remove(pos);
                true
            }
            None => false,
        }
    }

    /// The elements in the order Java iterates them.
    pub fn iter_java_order(&self) -> Vec<&[u8]> {
        let mask = self.capacity - 1;
        let mut order: Vec<&[u8]> = self.items.iter().map(Vec::as_slice).collect();
        order.sort_by_key(|s| {
            let h = java_hash(s) as u32;
            ((h ^ (h >> 16)) as usize) & mask
        });
        order
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hash_matches_java() {
        // "tracebot1".hashCode() and "tracebot2".hashCode() differ by one, so their buckets are
        // adjacent; the order in the user-ignore trace is tracebot2, tracebot1.
        assert_eq!(java_hash(b"a"), 97);
        assert_eq!(java_hash(b"ab"), 3105);
        assert_eq!(java_hash(b"hello"), 99_162_322);
    }

    #[test]
    fn order_follows_bucket_index() {
        let mut set = JavaStringSet::default();
        set.insert(b"tracebot1");
        set.insert(b"tracebot2");
        assert_eq!(
            set.iter_java_order(),
            vec![b"tracebot2".as_slice(), b"tracebot1".as_slice()]
        );
        assert!(!set.insert(b"tracebot1"));
        assert!(set.remove(b"tracebot2"));
        assert_eq!(set.iter_java_order().len(), 1);
    }

    #[test]
    fn grows_like_a_hash_map() {
        let mut set = JavaStringSet::default();
        for i in 0..13 {
            set.insert(format!("n{i}").as_bytes());
        }
        // 13 > 12 = 0.75 * 16: the table doubled.
        assert_eq!(set.capacity, 32);
    }
}
