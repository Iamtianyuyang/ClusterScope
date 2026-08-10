use parking_lot::RwLock;

/// Monotonically increasing sequence generator per node.
pub struct SequenceGenerator {
    sequences: RwLock<rustc_hash::FxHashMap<String, u64>>,
}

impl SequenceGenerator {
    pub fn new() -> Self {
        Self {
            sequences: RwLock::new(rustc_hash::FxHashMap::default()),
        }
    }
    
    pub fn next(&self, node_id: &str) -> u64 {
        let mut maps = self.sequences.write();
        let seq = maps.entry(node_id.to_string()).or_insert(0);
        *seq += 1;
        *seq
    }
}

/// Atomic counter for global unique IDs.
pub struct AtomicCounter {
    value: RwLock<u64>,
}

impl AtomicCounter {
    pub fn new() -> Self {
        Self {
            value: RwLock::new(0),
        }
    }
    
    pub fn next(&self) -> u64 {
        let mut v = self.value.write();
        *v += 1;
        *v
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_sequence_generator() {
        let generator = SequenceGenerator::new();
        
        assert_eq!(generator.next("node-1"), 1);
        assert_eq!(generator.next("node-1"), 2);
        assert_eq!(generator.next("node-1"), 3);
        
        assert_eq!(generator.next("node-2"), 1);
        assert_eq!(generator.next("node-2"), 2);
    }
    
    #[test]
    fn test_atomic_counter() {
        let counter = AtomicCounter::new();
        assert_eq!(counter.next(), 1);
        assert_eq!(counter.next(), 2);
        assert_eq!(counter.next(), 3);
    }
}
