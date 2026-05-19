# Design: Fast Source Tree for XPath/XSLT Execution

**Date:** 2026-05-11 22:58 CEST  
**Status:** Proposal  
**Related:** `docs/profiling.md`, `docs/ideas.md`

## Problem

Xee's current performance gap against Saxon is too large to plausibly close
with tactical caches alone. Recent work has already removed some dominant
bottlenecks such as dynamic XPath recompilation overhead and the worst
`xsl:number level="any"` scaling behavior. The remaining cost is more likely
to be diffuse and structural.

One likely structural cost is the source XML tree model itself. XPath and XSLT
execute large numbers of small structural queries:

- parent / ancestor navigation
- child / descendant navigation
- node-kind tests
- expanded-QName tests
- document-order comparisons
- subtree membership checks

If those operations require pointer chasing, branch-heavy navigation, repeated
string/name resolution, or generic wrapper objects, then every higher-level
optimization sits on top of an expensive substrate.

## Hypothesis

For source documents, Xee should use a read-only, dense, query-optimized tree
representation instead of relying only on a flexible general-purpose XML object
model.

The expected effect is not a narrow win in one feature, but a reduction in the
constant cost of many hot operations at once:

- XPath step execution
- template dispatch and pattern matching
- `xsl:number`, `key()`, and accumulator scans
- node-set comparisons and document-order tests

## Decision

Introduce a second internal representation for parsed source documents:
`FastDocument`.

`FastDocument` is an immutable, array-backed source-tree model optimized for
read-heavy XPath/XSLT traversal. It does **not** replace the existing tree
everywhere. It is scoped to parsed source documents only.

Result trees and temporary trees remain on the existing representation unless a
later performance investigation justifies a separate output-oriented model.

## Core Design

### 1. Dense node identities

Each source node gets a compact integer identity (`NodeId`). Node names are
interned as dense `NameId`s. Strings referenced by nodes are interned in a
string pool.

This makes common tests integer-based instead of string- or pointer-based.

### 2. Structure-of-arrays storage

Instead of storing a node as an object graph, store per-node fields in flat
arrays:

- node kind
- expanded name id
- parent id
- first child id
- next sibling id
- previous sibling id
- preorder position
- subtree end position
- depth

Elements also store contiguous slices for attributes and namespace bindings.

### 3. Preorder and subtree ranges

Each node stores:

- `preorder[node]`
- `subtree_end[node]`

This allows subtree reasoning using integer ranges instead of repeated tree
walking.

Examples:

- `descendant::*` becomes a contiguous preorder scan.
- `is descendant of` becomes a range containment test.
- document order becomes integer comparison.
- subtree exclusion becomes a range-boundary check.

### 4. Read-only navigation seam

Hot evaluator paths should not depend directly on the physical tree layout.
Instead they should target a narrow read-only navigation interface that can be
implemented by either:

- the current Xot-backed model
- `FastDocument`

This keeps the first prototype narrow and allows gradual adoption.

## Scope

### In scope

- Parsed source XML documents
- The hottest structural XPath axes and tests
- Template dispatch inputs (node kind, name, ancestry, document order)
- A sidecar or alternate source representation built from existing parse output

### Out of scope for the first prototype

- Replacing the entire XML stack
- Result-tree construction
- Temporary tree mutation
- Full namespace-node physical storage
- Schema-typed storage or typed-value specialization

## Why a second model instead of replacing the current one?

Source trees and result trees have different performance needs.

Source trees are mostly immutable and read-heavy. They benefit from compact,
cache-friendly layouts and precomputed indexes.

Result trees are append-heavy and construction-oriented. They may require a
different builder-friendly or serializer-friendly representation.

Trying to optimize one structure for both use cases is likely to make it good
at neither.

## Expected Benefits

If this hypothesis is correct, a fast source tree reduces cost in multiple hot
subsystems simultaneously:

1. XPath step execution becomes tighter and more cache-friendly.
2. Name tests become integer equality.
3. Template dispatch can key off kind + name id immediately.
4. Ancestor and descendant predicates become much cheaper.
5. Document-order and subtree checks become trivial arithmetic.
6. Secondary indexes such as `nodes_by_name` become natural to build.

The likely win is not asymptotic novelty for most axes, but lower constant
factors on operations executed millions of times.

## Risks

### Engineering complexity

This introduces another internal representation and a new abstraction seam.
That creates maintenance cost and raises the risk of semantic drift between the
two models.

### Namespace-node semantics

XPath namespace nodes do not map naturally to most physical XML structures.
The first version should treat them as virtual or deferred views over namespace
slices rather than trying to make them first-class stored nodes immediately.

### Partial adoption trap

If only a tiny part of execution uses `FastDocument`, the benefit may be too
small to justify the added complexity. The first prototype must therefore route
one genuinely hot slice through the new seam.

### Wrong bottleneck risk

It is possible that the current tree model is not a dominant cost, and that VM
dispatch, sequence materialization, or serializer overhead dominate instead.
That must be tested before committing to a full migration.

## Prototype Plan

The smallest serious prototype should answer one question only:

> Does a dense, array-backed source tree materially reduce real workload time
> on the current XSLT benchmark?

### Phase 1: Build `FastDocument`

Create a source-tree sidecar with:

- `NodeId`
- `NameId`
- node kind array
- parent / child / sibling arrays
- preorder / subtree_end arrays
- optional `by_preorder` array
- string and name interning

Build it from the existing parsed source tree. Do not build a new parser.

### Phase 2: Implement the narrow hot slice

Support only:

- `self`
- `parent`
- `child`
- `descendant`
- `ancestor`
- `following-sibling`
- node-kind test
- expanded-name test
- document-order comparison

### Phase 3: Route one hot execution path

Use the new seam for one or both of:

- common XPath path evaluation
- template dispatch / pattern matching

### Phase 4: Benchmark

Measure the precompiled large-input XSLT workload before and after.

If the delta is small, stop.

If the delta is meaningful, continue with deeper integration.

## Success Criteria

This proposal is worth pursuing only if the first prototype shows a meaningful
end-to-end win on the real workload, not just a microbenchmark.

Suggested threshold:

- **Continue** if the prototype gives a clear and repeatable improvement on the
  large precompiled XSLT benchmark.
- **Stop** if the improvement is negligible or isolated to synthetic tests.

The goal is not to prove that `FastDocument` is elegant. The goal is to prove
that the current source-tree representation is taxing the hot path enough to
justify a second model.

## Alternatives Considered

### 1. Keep the current tree and optimize only IR/bytecode

This is still attractive and may produce meaningful wins, but it does not help
if the underlying source-tree navigation is itself expensive.

### 2. Replace the entire tree model at once

Too much risk and too much surface area. It would make it hard to know whether
performance gains came from the model itself or from collateral rewrites.

### 3. Save or optimize final bytecode only

This can reduce interpreter overhead but does not address structural XML access
costs beneath the VM.

## Follow-up Questions

If the prototype succeeds, the next decisions are:

1. Which additional axes should move first?
2. Should template dispatch use dedicated indexes built from `FastDocument`?
3. Should source node sets be represented as spans or bitsets over preorder?
4. Should result trees get a separate output-optimized model later?

## Summary

This proposal does **not** claim that a fast source tree alone will close the
full performance gap to Saxon.

It does claim that a read-only, dense, array-backed source-tree model is one of
the few architectural changes that could reduce costs across XPath, pattern
matching, template dispatch, and document-order logic simultaneously.

That makes it a good candidate for a focused prototype.