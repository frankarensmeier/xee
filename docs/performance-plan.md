# Xee Performance Plan

**Date:** 2026-05-19  
**Context:** Xee processes `input-xlarge.xml` (7.9 MB DocBook) with `print.xsl` in ~100 s.
Saxon processes the same workload in 10–15 s. This document records what the profile
showed and what we plan to do about it.

## What the profile says

Profiled with samply on `input-xlarge.xml` (124 k samples). Top self-time categories,
grouped semantically:

| % of runtime | Category |
|---|---|
| 16 % | Interpreter dispatch loop (`run_actual`) — Vec push/pop/deref/slice for every instruction |
| 5 % | Dropping `Atomic` values — heap objects created and freed on almost every operation |
| 3.3 % | `rmp_serde` — MessagePack deserialisation of the precompiled stylesheet at startup |
| 3 % | Allocator churn (`mi_malloc` / `mi_free`) |
| 2.8 % | Atomisation (`AtomizedItemIter`) — extracting string values from nodes |
| 2.4 % | HashMap hashing — variable lookup by `OwnedName`, pattern `NameCache` |
| 2.4 % | indextree axis traversal (xot) |
| 2.1 % | `Sequence::clone` |
| 1.8 % | Unicode grapheme segmentation (`fn:string-length`, `fn:substring`) |
| 1.2 % | `Sequence` drop |
| 1.2 % | `Atomic::clone` |
| 1.1 % | `xml::step::node_test` |

The 16 % in `run_actual` appears as `Vec::as_slice` / `Vec::deref` in the browser
profiler because those calls are inlined into the hot loop. The root cause is that
every XPath step materialises a `Vec<Item>`, iterates it as a slice, and drops it —
even for the common cases of zero or one result.

## What we decided not to do

**FastDoc** — a prototype dense array-backed document representation — was developed
on this branch and then removed. It addressed indextree traversal (2.4 %) and
`node_test` (1.1 %), a ceiling of ~3.5 % of runtime. The implementation also had a
fundamental flaw: `try_resolve_step` iterated FastDoc `NodeId`s but called back into
xot for every node test, making the fast path strictly slower than the baseline. The
complexity (two document representations, cache invalidation, fallback paths) was not
worth the gain when 7–8 % wins are available elsewhere. The design document is kept at
`docs/DESIGN-fast-source-tree.md` for future reference; if the other improvements
materialise and tree traversal becomes a larger fraction of remaining time, it is worth
revisiting.

## The plan

### Phase 1 — Quick wins

These are contained, testable changes with a clear and measurable payoff.

#### ~~1. ASCII fast-path in `fn:string-length` and `fn:substring` (~1.8 %)~~

**Invalidated by measurement (2026-05-19).** The 1.8 % attributed to
`unicode_segmentation::grapheme` in the initial profile analysis was a
misidentification: atos resolved those addresses against the xee binary, but they
actually belong to `libsystem_platform.dylib`. Adding a call counter to `string_length`
confirmed it is called fewer than 1,000 times across the full xlarge.xml transform —
not a hot function at all. Dropped.

---

#### ~~2. Collation lookup de-allocation (was "variable lookup by numeric index")~~

**Done (2026-05-19).** The 2.4 % attributed to HashMap hashing in the initial profile
was actually `Collations::load`, not variable lookup. All compiled variables already use
numeric indices (`Var`, `GlobalVar`, `ClosureVar`), so there was nothing to change there.

`Collations::load` was called 253 million times on the xlarge.xml transform, each time
allocating a `String` from the URI via `uri.to_string()` even on cache hits. The fix:

1. `Collations::load` — use `HashMap::get(uri.as_str())` on the fast path; only
   `uri.to_string()` when inserting a new (rare) entry.
2. `StaticContext::default_collation()` — added `default_collation_rc:
   RefCell<Option<Rc<Collation>>>` so the cache returns the `Rc` directly on all but
   the first call, skipping the `borrow_mut()` + HashMap probe entirely.

**Measured gain:** 1.79 s → 1.00 s on the small-xml benchmark (−44 %).

---

### Phase 2 — Medium effort, high impact

#### 3. Avoid `Atomic` allocation for string comparisons (~4.8 % drop + 2.8 % atomisation = ~7.6 %)

The dominant atomic churn is text-node and attribute string values being extracted as
`Rc<str>`, used for one comparison, then dropped. The allocation is pointless when the
comparison is `string-value(node) = "literal"` — you could compare directly against
xot's `&str`.

Target: wherever a comparison instruction atomises a node only to compare it against a
string literal, thread a `&str` fast path that skips the `Rc<str>` allocation entirely.
The deeper generalisation is giving `Atomic::String` a borrowed or interned variant for
compile-time literals, but even a targeted fix in the comparison instruction would
recover most of the combined 7.6 %.

**Where:** `atomic/atomic_core.rs`; the comparison instructions in `interpret.rs`;
`AtomizedItemIter` in `sequence/item.rs`.

---

#### 4. Reduce `Sequence::clone` (~2.1 % + 1.2 % drop = ~3.3 %)

`Sequence::Many` already uses `Rc<[Item]>`, so cloning a multi-item sequence is O(1).
The 2.1 % therefore comes from cloning `One` sequences, which clones the `Item` inside
and in turn the `Atomic`. Find the call sites in `run_actual` that clone single-item
sequences where a move or borrow would do.

**Where:** `interpret.rs`, call sites around stack push/pop and variable assignment.

---

### Phase 3 — Architectural changes

#### 5. Lazy / streaming sequences (large fraction of the 16 % in `run_actual`)

Every XPath step produces a `Vec<Item>` even when the result is empty or has one
element. Most sequences fall into the `Empty` or `One` variants already; the problem
is the `Many` path and the Vec-backed iteration machinery that the hot loop exercises
via `as_slice` / `deref`.

A lazy `Sequence` representation that defers allocation until items are actually needed
would eliminate most of the Vec construction, the `memmove` during collection, and most
of the drop overhead. This is the largest single opportunity in the profile but also the
most invasive change — every consumer of `Sequence` would need to handle an iterator
rather than a slice.

A practical first step is to audit which instructions force eagerly materialised sequences
when a count or a single-item check would suffice, and replace those with specialised
paths before committing to a full lazy redesign.

---

#### 6. Startup deserialization (~3.3 %)

`rmp_serde` accounts for ~3 s of the 100 s run: the precompiled stylesheet is fully
deserialised before execution begins. This is a fixed per-run cost rather than a
scaling problem, but it matters for the benchmark.

Investigate:
- Confirm it is truly one-shot startup and not something that recurs during execution
  (`time xee xslt --precompiled ...` vs `time xee xslt ...`).
- Consider a faster binary format (e.g. `bincode` with a stable encoding, or a
  hand-rolled format with memory-mapped pages).
- Consider lazy loading: parse only the templates invoked by the current transform
  rather than deserialising the full stylesheet upfront.

---

## Expected gains

None of these numbers are guarantees — they are ceilings derived from self-time in a
single profiling run. Real gains depend on how tightly the bottlenecks are coupled.

| Work item | Self-time addressed |
|---|---|
| ASCII fast-path | ~1.8 % |
| Variable numeric IDs | ~2.4 % |
| Avoid Atomic alloc for string compare | ~7.6 % |
| Reduce Sequence clone | ~3.3 % |
| Lazy sequences | large fraction of 16 % |
| Startup deserialization | ~3.3 % (fixed cost) |

Phases 1 and 2 together address roughly 15–20 % of current runtime without any
architectural redesign. Phase 3 is where the remaining structural gap with Saxon lies.
