// Prefix-sum cache for xsl:number level="any".
//
// On the first call for a given (count_pattern, from_pattern) pair and document,
// we walk the entire document forward in document order and build:
// - prefix_sum[i]: cumulative count of matching nodes from position 0..=i
// - from_positions: sorted list of positions where the from_pattern matched
// - node_to_pos: maps each Node to its position in the traversal
//
// All subsequent lookups are O(1) + O(log k) where k = from-boundary count.

use ahash::HashMap;

/// A single precomputed entry for one (count, from) pattern pair in one document.
pub(crate) struct NumberCountEntry {
    /// Maps each xot::Node to its position in document-order traversal.
    node_to_pos: HashMap<xot::Node, usize>,
    /// prefix_sum[i] = cumulative count of count-matching nodes from
    /// document start through position i (inclusive).
    prefix_sum: Vec<i64>,
    /// Sorted positions where from_pattern matched.
    from_positions: Vec<usize>,
}

impl NumberCountEntry {
    pub(crate) fn new(
        node_to_pos: HashMap<xot::Node, usize>,
        prefix_sum: Vec<i64>,
        from_positions: Vec<usize>,
    ) -> Self {
        Self {
            node_to_pos,
            prefix_sum,
            from_positions,
        }
    }

    /// Look up the count for a node. Returns the number of count-matching nodes
    /// preceding (or equal to) this node in document order, after the nearest
    /// from-boundary.
    pub(crate) fn lookup(&self, node: xot::Node) -> i64 {
        let Some(&pos) = self.node_to_pos.get(&node) else {
            return 0;
        };

        let count_at_pos = self.prefix_sum[pos];

        if self.from_positions.is_empty() {
            return count_at_pos;
        }

        // Find the largest from-position that is strictly less than pos.
        // (The from boundary itself is excluded from counting — counting
        // starts AFTER the from boundary.)
        let from_idx = self.from_positions.partition_point(|&fp| fp < pos);
        if from_idx == 0 {
            // No from boundary before this node
            count_at_pos
        } else {
            let from_pos = self.from_positions[from_idx - 1];
            count_at_pos - self.prefix_sum[from_pos]
        }
    }
}

/// Cache key combining document root, count pattern identity, and from pattern identity.
/// For compiled patterns, we use the pattern index.
/// For default count patterns (no explicit count=), we use node type + name.
#[derive(Clone, Eq, PartialEq, Hash)]
pub(crate) enum CountPatternKey {
    /// Compiled pattern at this index in declarations.number_patterns
    PatternIndex(i64),
    /// Default count: match by node value type and optional NameId
    Default(xot::ValueType, Option<xot::NameId>),
}

/// Full cache key: (document_root, count_key, from_index).
/// from_index: -1 means no from pattern.
#[derive(Clone, Eq, PartialEq, Hash)]
pub(crate) struct CacheKey {
    pub(crate) doc_root: xot::Node,
    pub(crate) count_key: CountPatternKey,
    pub(crate) from_index: i64,
}

/// The top-level cache stored on the Interpreter.
pub(crate) struct NumberCountCache {
    entries: HashMap<CacheKey, NumberCountEntry>,
}

impl NumberCountCache {
    pub(crate) fn new() -> Self {
        Self {
            entries: HashMap::default(),
        }
    }

    pub(crate) fn get(&self, key: &CacheKey) -> Option<&NumberCountEntry> {
        self.entries.get(key)
    }

    pub(crate) fn insert(&mut self, key: CacheKey, entry: NumberCountEntry) {
        self.entries.insert(key, entry);
    }
}
