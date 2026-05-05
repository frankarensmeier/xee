// Accumulator value cache for xsl:accumulator / fn:accumulator-before() / fn:accumulator-after().
//
// On the first accumulator-before() or accumulator-after() call for a given
// (document, accumulator-name) pair, we walk the entire document tree once in
// document order, evaluate all matching accumulator rules, and store the
// before/after values for every node. Subsequent calls for the same
// (document, accumulator-name) become O(1) hash lookups.

use ahash::HashMap;
use xot::xmlname::OwnedName;

use crate::sequence;

/// Accumulated values for a single node.
#[derive(Clone)]
pub(crate) struct AccumulatorNodeValues {
    /// The accumulator value just before visiting this node (before start-phase rules).
    pub(crate) before: sequence::Sequence,
    /// The accumulator value just after visiting this node (after end-phase rules).
    pub(crate) after: sequence::Sequence,
}

/// Index for a single (document, accumulator-name) combination.
pub(crate) struct AccumulatorIndex {
    /// Map from node to its before/after values.
    node_values: HashMap<xot::Node, AccumulatorNodeValues>,
}

impl AccumulatorIndex {
    pub(crate) fn new() -> Self {
        Self {
            node_values: HashMap::default(),
        }
    }

    pub(crate) fn insert(&mut self, node: xot::Node, values: AccumulatorNodeValues) {
        self.node_values.insert(node, values);
    }

    pub(crate) fn get_before(&self, node: xot::Node) -> Option<&sequence::Sequence> {
        self.node_values.get(&node).map(|v| &v.before)
    }

    pub(crate) fn get_after(&self, node: xot::Node) -> Option<&sequence::Sequence> {
        self.node_values.get(&node).map(|v| &v.after)
    }
}

/// Cache key: identifies an accumulator index by document root and accumulator name.
#[derive(Clone, Eq, PartialEq, Hash)]
pub(crate) struct AccumulatorCacheKey {
    pub(crate) doc_root: xot::Node,
    pub(crate) accumulator_name: OwnedName,
}

/// The top-level accumulator cache stored on the Interpreter.
pub(crate) struct AccumulatorCache {
    indices: HashMap<AccumulatorCacheKey, AccumulatorIndex>,
}

impl AccumulatorCache {
    pub(crate) fn new() -> Self {
        Self {
            indices: HashMap::default(),
        }
    }

    pub(crate) fn get(&self, key: &AccumulatorCacheKey) -> Option<&AccumulatorIndex> {
        self.indices.get(key)
    }

    pub(crate) fn insert(&mut self, key: AccumulatorCacheKey, index: AccumulatorIndex) {
        self.indices.insert(key, index);
    }
}
