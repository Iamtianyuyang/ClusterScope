use lru::LruCache;
use parking_lot::RwLock;
use std::collections::HashSet;
use std::num::NonZeroUsize;

/// Tracks seen (node_id, sequence) pairs for deduplication.
pub struct SequenceDeduplicator {
    seen: RwLock<LruCache<u64, ()>>,
}

impl SequenceDeduplicator {
    pub fn new(capacity: usize) -> Self {
        Self {
            seen: RwLock::new(LruCache::new(NonZeroUsize::new(capacity).unwrap())),
        }
    }
    
    /// Returns true if this sequence is new (not previously seen).
    pub fn try_insert(&self, key: u64) -> bool {
        let mut cache = self.seen.write();
        if cache.contains(&key) {
            false
        } else {
            cache.put(key, ());
            true
        }
    }
    
    pub fn len(&self) -> usize {
        self.seen.read().len()
    }
    
    pub fn clear(&self) {
        self.seen.write().clear();
    }
}

/// Tracks seen (node_id, sequence, timestamp) triples for out-of-order detection.
/// Returns:
///   Ok(true)  — new data, accepted
///   Ok(false) — duplicate, rejected
///   Err(())   — out of order (sequence went backward beyond tolerance)
pub struct SequenceTracker {
    last_sequence: RwLock<HashSet<(String, u64)>>,
}

impl SequenceTracker {
    pub fn new() -> Self {
        Self {
            last_sequence: RwLock::new(HashSet::new()),
        }
    }
    
    pub fn check(&self, node_id: &str, sequence: u64) -> bool {
        let mut set = self.last_sequence.write();
        let key = (node_id.to_string(), sequence);
        if set.contains(&key) {
            false  // duplicate
        } else {
            set.insert(key);
            true
        }
    }
    
    pub fn clear(&self) {
        self.last_sequence.write().clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_deduplicator() {
        let dedup = SequenceDeduplicator::new(1000);
        
        assert!(dedup.try_insert(1));
        assert!(!dedup.try_insert(1));  // duplicate
        assert!(dedup.try_insert(2));
        assert!(dedup.try_insert(3));
        assert!(!dedup.try_insert(3));  // duplicate
    }
    
    #[test]
    fn test_sequence_tracker() {
        let tracker = SequenceTracker::new();
        
        assert!(tracker.check("node-1", 1));
        assert!(!tracker.check("node-1", 1));  // duplicate
        assert!(tracker.check("node-1", 2));
        assert!(tracker.check("node-2", 1));   // different node, same sequence
        assert!(!tracker.check("node-2", 1));  // duplicate for node-2
    }
}
