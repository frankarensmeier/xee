# Design: xsl:number level="any" Prefix-Sum Cache

**Date:** 2026-04-30 17:16 CEST  
**Status:** Implementation plan  
**Branch:** `feature/xslt-match-rooted-patterns`

## Problem

`xsl:number level="any"` is O(n²) — for every numbered element, it walks
backward through all preceding nodes in document order, testing each against
the count/from pattern. With the DocBook stylesheet on `input-large.xml`
(~50K nodes, ~15 distinct numbering patterns), this consumes **90.6% of total
runtime** (confirmed via hierarchical profiling).

Disabling the function entirely reduces `input-large.xml` from ~40s to ~20s.

## Root Cause

```
count_any_level_pattern(node):
    for n in reverse_document_order(node):   ← walks ALL preceding nodes
        if matches(n, count_pattern): count++
        if matches(n, from_pattern): break
```

Called once per numbered element. Each call does O(n) pattern matches.
Total work: O(n × k) where k is average distance to `from` boundary.
Worst case (no `from`): O(n²).

## Solution: Lazy Prefix-Sum Arrays

### Core Idea

On the **first** call for a given `(count_pattern, from_pattern)` pair,
walk the **entire document** forward once and build:

1. **`prefix_sum: Vec<i64>`** — at document-order position `i`, the
   cumulative count of nodes matching `count_pattern` from document start.
2. **`from_positions: Vec<usize>`** — sorted list of positions where
   `from_pattern` matched.
3. **`node_to_pos: HashMap<Node, usize>`** — maps each node to its
   document-order position.

All subsequent calls are **O(1)** lookups:

```
lookup(node):
    pos = node_to_pos[node]
    from_pos = binary_search(from_positions, pos)  // largest f ≤ pos
    if from_pos exists:
        return prefix_sum[pos] - prefix_sum[from_pos]
    else:
        return prefix_sum[pos]
```

### Complexity

| Operation | Before | After |
|-----------|--------|-------|
| First call | O(n) | O(n) — builds cache |
| Each subsequent call | O(n) | O(1) + O(log k) |
| Total for m calls | O(m × n) | O(n + m × log k) |

Where n = total nodes, m = numbered elements, k = from-boundary count.

### Memory

Per `(count, from)` pair:
- `prefix_sum`: 8 bytes × n
- `from_positions`: 8 bytes × k (typically < 50)
- `node_to_pos`: ~40 bytes × n (HashMap overhead)

For `input-large.xml` (~50K nodes, ~15 patterns): **~35 MB** total.
Acceptable — this is a time/space trade-off that eliminates a 90% bottleneck.

## Data Structures

```rust
/// Cache for xsl:number level="any" precomputed counts.
/// Keyed by (count_pattern_index, from_pattern_index).
struct NumberCountCache {
    entries: HashMap<(i64, i64), NumberCountEntry>,
}

struct NumberCountEntry {
    /// Maps each xot::Node to its document-order position index.
    node_to_pos: HashMap<xot::Node, usize>,
    /// prefix_sum[i] = count of nodes matching count_pattern
    /// from document start through position i (inclusive).
    /// Length = total number of nodes in the document.
    prefix_sum: Vec<i64>,
    /// Sorted positions where from_pattern matched.
    /// Empty if no from pattern specified.
    from_positions: Vec<usize>,
}
```

## Placement in Codebase

### Where the cache lives

On the **Interpreter** struct (`xee-interpreter/src/interpreter/interpret.rs`).
The Interpreter already holds mutable state and is passed to all XSLT
runtime functions via `&mut Interpreter`.

```rust
pub struct Interpreter {
    // ... existing fields ...
    number_count_cache: NumberCountCache,
}
```

### Where it's populated

In `count_any_level_pattern()` (`xee-interpreter/src/library/hidden_xslt.rs`).
Replace the backward-walking loop with:

```rust
fn count_any_level_pattern(
    interpreter: &mut Interpreter,
    node: xot::Node,
    count_index: i64,
    from_index: i64,
) -> i64 {
    interpreter.number_count_lookup(node, count_index, from_index)
}
```

The `number_count_lookup` method on Interpreter:

1. Checks if `(count_index, from_index)` is already cached
2. If not, builds the cache entry by forward-walking the document
3. Returns the O(1) answer

### Document root discovery

To walk the document forward, we need the root. Given any `node`, walk
`xot.parent()` to the root. This is O(depth) — negligible.

### Forward traversal

xot provides `xot.descendants(root)` which yields all nodes in document
order. We can also use `xot.traverse(root)` and filter for `Open` events.

## Handling the Default Count Pattern

When `count_index < 0` (no explicit `count=`), the default pattern matches
nodes with the same node kind and expanded-QName as the context node.
This is different from compiled patterns because the match depends on the
**calling node**, not a fixed pattern.

For this case, the cache key includes the `(value_type, name_id)` of the
reference node. The entry stores counts only for that specific element name.

Cache key for default count:
```rust
enum CountKey {
    Pattern(i64),                           // compiled pattern index
    Default(xot::ValueType, Option<xot::NameId>),  // node kind + name
}
```

## Edge Cases

### Multiple documents
Each cache entry is per-document. The `node_to_pos` map only contains
nodes from one document. If nodes from different documents hit the same
pattern, they get separate entries. We key by document root node.

### Document mutation
XSLT result trees are built separately — the source document is never
mutated during transformation. The cache remains valid for the lifetime
of the transformation.

### Patterns with predicates
Patterns like `db:section[not(parent::db:section)]` involve predicates
that check the tree context. These are handled correctly because we
evaluate them during the single forward pass using the same
`interpreter.matches()` call. The result is identical to evaluating
during the backward walk.

### From pattern interaction
The `from` boundary is **exclusive** — when we hit a `from` match, we
reset the count. In prefix-sum terms:

```
answer = prefix_sum[pos] - prefix_sum[from_boundary_pos]
```

where `from_boundary_pos` is the largest position ≤ `pos` that matches
the `from` pattern. If the from-matching node itself also matches count,
it is NOT included (the spec says counting starts **after** the from
boundary).

### No matching from boundary
If there's no from boundary before `pos`, use `prefix_sum[pos]` directly
(count from document start).

## Implementation Steps

1. **Add `NumberCountCache` struct** in a new file
   `xee-interpreter/src/library/number_count_cache.rs`

2. **Add cache field to Interpreter** in `interpret.rs`

3. **Add `number_count_lookup` method** on Interpreter that:
   - Finds document root from node
   - Checks cache for `(doc_root, count_key, from_index)`
   - On miss: forward-walks document, builds prefix_sum + from_positions
   - Returns answer via prefix_sum lookup

4. **Replace `count_any_level_pattern` body** with single cache call

5. **Handle default count** as special case with `(ValueType, NameId)` key

6. **Tests**: run conformance suite, benchmark

## Expected Impact

| Input | Before | Expected After | Speedup |
|-------|--------|----------------|---------|
| input-small.xml | 2.38s | ~2.2s | ~8% |
| input-large.xml | ~40s | ~20s | ~50% |

The improvement on large inputs is dramatic because the O(n²) → O(n)
change eliminates the superlinear scaling. The remaining time is template
execution, XPath evaluation, and serialization.

## Risks

- **Memory**: 35 MB for ~15 pattern pairs on a 50K-node document.
  Acceptable for a command-line tool. Could add LRU eviction if needed.
- **Correctness**: Prefix-sum arithmetic must handle from-boundary
  exactly right. Conformance tests will catch errors.
- **Pattern evaluation order**: Evaluating patterns in forward order
  vs backward should produce identical results since patterns don't
  have side effects in XSLT.
