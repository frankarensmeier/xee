// Key index cache for xsl:key / fn:key().
//
// On the first key() call for a given (document, key-name) pair, we walk the
// entire document tree once, evaluate the match pattern and use-expression for
// every node, and store the results in a hash index.  Subsequent key() calls
// for the same (document, key-name) pair become O(1) hash lookups instead of
// O(n) full-document walks.
//
// The index uses *typed* comparison matching XPath `eq` semantics.  For the
// common case where all key values are strings or xs:untypedAtomic, a fast
// string hash index is used.  For numeric and other typed key values, we fall
// back to linear-scan comparison (still only over the indexed entries, not the
// full document).

use ahash::HashMap;
use xot::xmlname::OwnedName;

use crate::atomic;

/// A single entry in the key index: a node paired with its key values.
struct KeyIndexEntry {
    node: xot::Node,
    /// The atomic values produced by the use-expression for this node.
    values: Vec<atomic::Atomic>,
}

/// Index for a single (document, key-name) combination.
///
/// For **non-composite** keys the primary index maps each string key value
/// to a list of matching nodes (in document order).  This fast path is only
/// used when both the stored value and the search value are string/untyped.
///
/// For **composite** keys and cross-type lookups we scan the entries list.
pub(crate) struct KeyIndex {
    /// Primary string-based index (non-composite only).
    /// Key = the string content of String/Untyped atomic values.
    /// Value = nodes (in document order) whose use-expression produced that string.
    string_index: HashMap<String, Vec<xot::Node>>,

    /// All entries — used for composite keys and non-string lookups.
    entries: Vec<KeyIndexEntry>,

    /// Whether this is a composite key (xsl:key composite="yes").
    pub(crate) composite: bool,
}

impl KeyIndex {
    pub(crate) fn new(composite: bool) -> Self {
        Self {
            string_index: HashMap::default(),
            entries: Vec::new(),
            composite,
        }
    }

    /// Add a node with its use-expression values to the index.
    pub(crate) fn add(&mut self, node: xot::Node, values: Vec<atomic::Atomic>) {
        if !self.composite {
            // Non-composite: index each string/untyped value for fast lookup.
            for v in &values {
                if let Some(s) = string_key(v) {
                    self.string_index.entry(s).or_default().push(node);
                }
            }
        }
        self.entries.push(KeyIndexEntry { node, values });
    }

    /// Look up nodes matching the given search values.
    pub(crate) fn lookup(
        &self,
        search_values: &[atomic::Atomic],
        collation: &crate::string::Collation,
        default_offset: chrono::FixedOffset,
    ) -> Vec<xot::Node> {
        if self.composite {
            return self.lookup_composite(search_values, collation, default_offset);
        }

        let mut result = Vec::new();
        let mut seen = ahash::HashSet::default();

        // Partition search values into string-indexable and non-string.
        let (string_svs, typed_svs): (Vec<_>, Vec<_>) = search_values
            .iter()
            .partition(|sv| is_untyped_or_string(sv));

        // Fast path: look up string/untyped search values via hash.
        for sv in &string_svs {
            if let Some(key) = string_key(sv) {
                if let Some(nodes) = self.string_index.get(&key) {
                    for &node in nodes {
                        if seen.insert(node) {
                            result.push(node);
                        }
                    }
                }
            }
        }

        // Slow path: for non-string search values (integer, double, dateTime, etc.)
        // we must do type-aware comparison against all entries.
        if !typed_svs.is_empty() {
            for entry in &self.entries {
                for v in &entry.values {
                    for sv in &typed_svs {
                        if v.equal(sv, collation, default_offset) && seen.insert(entry.node) {
                            result.push(entry.node);
                        }
                    }
                }
            }
        }

        result
    }

    /// Composite key lookup: element-wise comparison of the full value sequence.
    fn lookup_composite(
        &self,
        search_values: &[atomic::Atomic],
        collation: &crate::string::Collation,
        default_offset: chrono::FixedOffset,
    ) -> Vec<xot::Node> {
        let mut result = Vec::new();
        for entry in &self.entries {
            if entry.values.len() == search_values.len()
                && entry
                    .values
                    .iter()
                    .zip(search_values.iter())
                    .all(|(ka, sv)| ka.equal(sv, collation, default_offset))
            {
                result.push(entry.node);
            }
        }
        result
    }
}

/// Cache key: identifies a key index by document root and key name.
#[derive(Clone, Eq, PartialEq, Hash)]
pub(crate) struct KeyCacheKey {
    pub(crate) doc_root: xot::Node,
    pub(crate) key_name: OwnedName,
}

/// The top-level key cache stored on the Interpreter.
pub(crate) struct KeyCache {
    indices: HashMap<KeyCacheKey, KeyIndex>,
}

impl KeyCache {
    pub(crate) fn new() -> Self {
        Self {
            indices: HashMap::default(),
        }
    }

    pub(crate) fn get(&self, key: &KeyCacheKey) -> Option<&KeyIndex> {
        self.indices.get(key)
    }

    pub(crate) fn insert(&mut self, key: KeyCacheKey, index: KeyIndex) {
        self.indices.insert(key, index);
    }
}

/// Extract the string content from a String or Untyped atomic, for use as
/// a hash index key.  Returns None for all other types — those require
/// type-aware comparison via `Atomic::equal`.
fn string_key(atom: &atomic::Atomic) -> Option<String> {
    match atom {
        atomic::Atomic::Untyped(s) => Some(s.to_string()),
        atomic::Atomic::String(_, s) => Some(s.to_string()),
        _ => None,
    }
}

/// Check whether an atomic value is xs:string or xs:untypedAtomic.
fn is_untyped_or_string(atom: &atomic::Atomic) -> bool {
    matches!(atom, atomic::Atomic::Untyped(_) | atomic::Atomic::String(..))
}
