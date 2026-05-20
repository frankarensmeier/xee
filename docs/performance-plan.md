# Xee Performance Plan

**Date:** 2026-05-19  
**Context:** Xee processes `input-xlarge.xml` (7.9 MB DocBook) with `print.xsl` in ~110 s.
Saxon processes the same workload in 10–15 s. This document records what the profiles
showed and what we plan to do about it.

## Profile: 2026-05-19 (second run, after collation + cast_to_string fixes)

Profiled with samply on `input-small.xml` (~1.7 s run, 110 K samples).

| % of runtime | Category | Detail |
|---|---|---|
| ~8 % | Interpreter dispatch loop (`run_actual`) | Instruction fetch (interpret.rs:155) and Var instruction (interpret.rs:237) — each fetch does ~6 indirect ops: frame lookup → function lookup → bytecode deref → ip read/write |
| ~6 % | `State::pop` | Called for every value-consuming instruction; frame lookup + Vec pop + `try_into` at massive call volume |
| ~5 % | `drop_in_place<Atomic>` | Short-lived atomics created for intermediate string/number operations and freed immediately |
| ~2.7 % | `OwnedName::maybe_to_ref` in `node_test` | **NEW.** For every named step (e.g. `child::para`, `attribute::id`), xot's name table is queried by string 3× at runtime (prefix, namespace, local name) to convert OwnedName→NameId. Should be a one-time compile-time cost. |
| ~2.3 % | `Atomic::clone` + `Sequence::drop` | Sequences and atomics copied in the hot path |
| ~2 % | `AtomizedItemIter` / `atomized_one` | Extracting string values from nodes for comparisons |
| ~1.8 % | `TakeWhile::next` (xot axis iterator) | indextree axis traversal; unchanged from first profile |
| ~1.7 % | `aho_corasick` DFA construction | Startup only — XSLT stylesheet compilation (`xee_xslt_ast::Context::clone`); not a runtime bottleneck |

## Profile: 2026-05-19 (first run, before session fixes)

Profiled with samply on `input-xlarge.xml` (124 k samples). Top self-time categories,
grouped semantically:

| % of runtime | Category |
|---|---|
| 16 % | Interpreter dispatch loop (`run_actual`) — Vec push/pop/deref/slice for every instruction |
| 5 % | Dropping `Atomic` values — heap objects created and freed on almost every operation |
| 3.3 % | `rmp_serde` — MessagePack deserialisation of the precompiled stylesheet at startup |
| 3 % | Allocator churn (`mi_malloc` / `mi_free`) |
| 2.8 % | Atomisation (`AtomizedItemIter`) — extracting string values from nodes |
| 2.4 % | HashMap hashing — `Collations::load` allocating String keys on every call (253 M calls total) |
| 2.4 % | indextree axis traversal (xot) |
| 2.1 % | `Sequence::clone` |
| 1.8 % | Unicode grapheme segmentation — **invalidated**: addresses belonged to `libsystem_platform`, not xee |
| 1.2 % | `Sequence` drop |
| 1.2 % | `Atomic::clone` |
| 1.1 % | `xml::step::node_test` |

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

## Completed work

### ✅ Collation lookup de-allocation (~2.4 % of original runtime)

`Collations::load` was called 253 million times on the xlarge.xml transform, each time
allocating a `String` from the URI via `uri.to_string()` even on cache hits.

Fixes applied:
1. `Collations::load` — use `HashMap::get(uri.as_str())` on the fast path; only
   `uri.to_string()` when inserting a new (rare) entry.
2. `StaticContext::default_collation()` — added `default_collation_rc:
   RefCell<Option<Rc<Collation>>>` so the cache returns the `Rc` directly on all but
   the first call, skipping the `borrow_mut()` + HashMap probe entirely.

**Measured gain:** ~6 s on xlarge (116 s → 110 s).

---

### ✅ `cast_to_string` / `cast_to_untyped_atomic` allocation (~combined ~7.6 % in original)

`cast_to_string` was called 202,360 times on small.xml, each time going through
`into_canonical()` (new String + Rc allocation) even for `Untyped` and `String`
variants whose canonical form is identical to the stored value.

Fix: match on variant first; for `Untyped` and `String` move the `Rc<str>` directly
with 0 allocations.

**Measured gain:** Small on small.xml (absorbed by other variance); meaningful on
xlarge in conjunction with collation fix.

---

### ✅ `resolve_step` Vec allocation for 0/1 results

Counter measurement confirmed: 34 % of steps return empty, 61 % return one node, 5 %
return multiple nodes. The old code always allocated a `Vec`.

Fix: two-pass loop — returns `Sequence::default()` (no alloc) for empty, `One(item)`
for single-item, only builds a Vec for the 5 % many-item case.

**Measured gain:** ~90 ms on xlarge — below noise floor (mimalloc makes small allocs
fast at ~30 ns each). Kept because it removes unnecessary allocation pressure.

---

### ✅ `TemplateRule::clone` on every template dispatch

`lookup_template_rule` returned `Option<(TemplateRule, bool)>`, cloning the full
`TemplateRule` struct (which includes `Vec<usize> module_path`) on every
`apply-templates` call. Both callers only needed `function_id`.

Fix: changed return type to `Option<(InlineFunctionId, bool)>` and extract `function_id`
from the borrowed reference, avoiding the Vec clone entirely.

**Measured gain:** Below noise floor on small.xml benchmark but removes a heap
allocation per template dispatch.

---

## The plan

### Phase 1 — Named node test caching (~2.7 %)

**Hypothesis:** `node_test` in `step.rs` calls `OwnedName::maybe_to_ref(xot)` on every
evaluation of a named step (e.g. `child::para`, `attribute::id`). This does 3 HashMap
string lookups per call (prefix, namespace, local name → NameId). On xlarge.xml, this
adds up to ~2.7 % of runtime.

The XSLT compiler already has access to the Xot name table when building Steps. The fix
is to resolve OwnedNames to NameIds at compile time and store them in the Step, so
`node_test` can compare NameIds directly (a u32 comparison).

**Required steps:**
1. Add a counter to `OwnedName::maybe_to_ref` at the `node_test` call site and verify
   call frequency on xlarge.
2. Add a resolved `Option<xot::NameId>` alongside the name test in the compiled `Step`
   (or as a parallel cache structure keyed by step_id).
3. Benchmark before committing.

**Where:** `xee-interpreter/src/xml/step.rs`, compiler in `xee-xslt-compiler` /
`xee-xpath-compiler`.

---

### Phase 2 — Reduce interpreter fetch overhead (~14 % combined `run_actual` + `State::pop`)

Each instruction fetch calls `read_instruction()` which does ~6 indirect operations:
`frame().function()` → `inline_function(id)` → `&function.chunk` → read byte → write
ip back. For instructions with operands, this repeats for each operand (`read_u16`,
`read_u8`).

The target: cache the current chunk pointer and IP as local variables in `run_actual`,
and only sync back to the frame when doing call/return/jump. This is the classic
"register IP" optimisation for bytecode interpreters.

`State::pop` is similarly hot because every value-consuming instruction calls it through
a frame lookup + Vec pop + try_into chain.

**Difficulty:** Requires unsafe raw pointers for the chunk reference (borrow checker
prevents holding `&[u8]` and `&mut self` simultaneously), or a significant refactor of
how programs are stored. Validate with a prototype before committing.

---

### Phase 3 — Reduce `Atomic` allocation churn (~5 %)

Short-lived atomics (integers, strings) are heap-allocated on creation and freed
immediately after use. The allocation is pointless when the value is used for one
comparison and discarded.

Target: wherever a comparison instruction atomises a node only to compare it against a
string literal, thread a `&str` fast path that skips the `Rc<str>` allocation entirely.
The deeper generalisation is giving `Atomic::String` a borrowed or interned variant for
compile-time literals, but even a targeted fix in the comparison instruction would
recover most of the 5 %.

**Where:** `atomic/atomic_core.rs`; comparison instructions in `interpret.rs`;
`AtomizedItemIter` in `sequence/item.rs`.

---

### Phase 4 — Startup deserialization (~3.3 % of original profile, fixed cost)

`rmp_serde` accounts for ~3 s of the 100 s run: the precompiled stylesheet is fully
deserialised before execution begins. This is a fixed per-run cost rather than a scaling
problem, but it matters for the benchmark.

Investigate:
- Confirm it is truly one-shot startup and not something that recurs during execution.
- Consider a faster binary format (e.g. `bincode` with a stable encoding, or a
  hand-rolled format with memory-mapped pages).
- Consider lazy loading: parse only the templates invoked by the current transform
  rather than deserialising the full stylesheet upfront.

---

### Phase 5 — Lazy / streaming sequences (large fraction of the ~14 % in `run_actual`)

Every XPath step produces a `Vec<Item>` even when the result is empty or has one
element. Most sequences fall into the `Empty` or `One` variants already (the
`resolve_step` fix addressed allocations for 0/1-item steps). The problem is the `Many`
path and the Vec-backed iteration machinery that the hot loop exercises via
`as_slice` / `deref`.

A lazy `Sequence` representation that defers allocation until items are actually needed
would eliminate most of the Vec construction, the `memmove` during collection, and most
of the drop overhead. This is the largest single opportunity but also the most invasive
change — every consumer of `Sequence` would need to handle an iterator rather than a
slice.

A practical first step is to audit which instructions force eagerly materialised sequences
when a count or a single-item check would suffice, and replace those with specialised
paths before committing to a full lazy redesign.

---

## Expected gains

None of these numbers are guarantees — they are ceilings derived from self-time in a
profiling run. Real gains depend on how tightly the bottlenecks are coupled.

| Work item | Self-time addressed | Status |
|---|---|---|
| Collation allocation | ~2.4 % (original) | ✅ Done — ~6 s on xlarge |
| `cast_to_string` allocation | ~7.6 % (original) | ✅ Done |
| `resolve_step` Vec alloc | ~1.1 % (original) | ✅ Done — below noise floor |
| `TemplateRule::clone` | small | ✅ Done |
| Named node test caching (`OwnedName::maybe_to_ref`) | ~2.7 % | Phase 1 |
| Interpreter fetch overhead (`run_actual` + `pop`) | ~14 % | Phase 2 |
| Atomic allocation churn | ~5 % | Phase 3 |
| Startup deserialization | ~3.3 % (fixed cost) | Phase 4 |
| Lazy sequences | large fraction of 14 % | Phase 5 |
