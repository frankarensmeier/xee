# Performance Analysis — Xee XSLT Processor

**Date:** 2026-04-19
**Branch:** `feature/xslt-match-rooted-patterns`
**Workload:** DocBook xslTNG stylesheet (`xslt/print.xsl`, 46 includes)

## Benchmarks

Measured with `hyperfine --warmup 3` on release build (Apple Silicon Mac):

| Input document | Lines | Mean | Min | Max |
|:---|---:|---:|---:|---:|
| `input.xml` (minimal) | 8 | **2.415s** ± 0.028s | 2.387s | 2.461s |
| `big-input.xml` (book) | ~65 | **5.806s** ± 0.192s | 5.572s | 6.258s |

The big document is **2.40x slower** than the small one. Since the small
document produces almost no output, the 2.4s baseline is **pure
compilation/startup overhead**.

| Phase | Time estimate |
|:---|---:|
| Stylesheet compilation (fixed cost) | ~2.4s |
| Transformation of big-input.xml | ~3.4s |

## Profile (macOS `sample`, 5s window on big-input.xml)

3821 samples total, single-threaded.

### Top-level split

From `xee::xslt::Xslt::run`:

| Phase | Samples | % | Source |
|:---|---:|---:|:---|
| Parse/compile stylesheet | 1054 | 27.6% | `xslt.rs:57` |
| Evaluate/run transformation | 2767 | 72.4% | `xslt.rs:73` |

Note: the profile under-counts compilation because the 5s sample window
starts slightly after process launch. The hyperfine benchmark confirms the
true fixed cost is ~2.4s.

### Compilation breakdown (1054 samples)

| Component | Samples | What |
|:---|---:|:---|
| `preprocess_stylesheet` | 750 | Recursive `xsl:include`/`xsl:import` processing (46 modules) |
| `process_stylesheet_module` | 499 | Per-module AST construction |
| `parse_transform` (XSLT AST) | ~200 | Parsing XSLT elements into AST nodes |
| IR compilation | ~209 | AST → IR conversion (`IrConverter`) |
| XPath parsing (chumsky) | significant | Parsing XPath expressions in `select`, `match`, `test` attributes |
| `BinaryExpr::clone` | visible | Deeply recursive pattern AST cloning during compilation |

### Execution breakdown (2767 samples)

The runtime is dominated by a deep recursive call chain:

```
run_actual
  → apply_templates_sequence
    → call_template_with_params
      → call_function_with_optional_arguments
        → run_actual (recurse)
```

Key functions by inclusive sample count:

| Function | Samples | Role |
|:---|---:|:---|
| `apply_templates_sequence` | 2767 | Template rule matching + dispatch |
| `call_template_with_params` | 2767 | Parameter binding + template invocation |
| `call_function_with_optional_arguments` | 2767 | Function call wrapper |
| `wrapper_xslt_with_temporary_output_state` | 1217 | Output state push/pop for result trees |
| `call_function` (XPath functions) | 1217 | XPath function invocation |
| `Rc::drop_slow` | visible | Reference counting cleanup |

## Optimization Targets (ordered by expected impact)

### 1. Stylesheet caching / precompilation

**Impact: eliminates ~2.4s startup per invocation**

The single biggest win. Every invocation re-parses and re-compiles the entire
stylesheet tree (46 files for DocBook xslTNG). A serialized compiled form
(similar to Saxon's `.sef` files) would make repeated invocations near-instant
for the same stylesheet.

Approach: serialize the compiled `Declarations` + bytecode to disk after first
compilation, reload on subsequent runs when stylesheet files haven't changed.

### 2. XPath parser performance

**Impact: reduces compilation time**

The `chumsky` parser combinator library shows up prominently in the
compilation profile (`MapWith`, `Foldl`, `Or` combinators). Options:

- Memoize parsed XPath expressions (many templates repeat similar patterns)
- Consider a hand-rolled recursive-descent parser for the XPath grammar
- Profile whether `chumsky`'s backtracking is causing redundant work

### 3. Reduce pattern AST cloning

**Impact: reduces compilation time and memory pressure**

`BinaryExpr::clone` appears in deeply recursive chains during compilation.
Pattern ASTs are being cloned rather than shared. Consider:

- Arena allocation for AST nodes (eliminates clone/drop overhead)
- `Rc`/`Arc` sharing of common sub-expressions
- Copy-on-write patterns

### 4. Template dispatch optimization

**Impact: reduces execution time**

`apply_templates_sequence` is the runtime hot path. Every element in the
input document goes through template matching. Possible improvements:

- Pre-compute a dispatch table indexed by element name
- Cache recent match results
- Reduce the overhead of `call_function_with_optional_arguments` (parameter
  passing, scope setup)

### 5. Reference counting pressure

**Impact: reduces both compile and execution time**

`Rc::drop_slow` appears with significant inclusive samples. This indicates
many small heap-allocated objects being individually reference-counted.
Arena allocation would batch both allocation and deallocation.

## Reproducing

```bash
# Build with debug symbols for profiling
CARGO_PROFILE_RELEASE_DEBUG=2 cargo build --release

# Benchmark
hyperfine --warmup 3 \
  'target/release/xee xslt xslt/print.xsl xslt/input.xml' \
  'target/release/xee xslt xslt/print.xsl target/tmp/big-input.xml'

# Profile (macOS)
target/release/xee xslt xslt/print.xsl target/tmp/big-input.xml > /dev/null &
PID=$!; sample $PID 5 -f target/tmp/sample.txt; wait $PID

# Profile with samply (cross-platform, opens Firefox Profiler)
samply record -- target/release/xee xslt xslt/print.xsl target/tmp/big-input.xml

# Tools: brew install hyperfine; cargo install samply
```

The test document `target/tmp/big-input.xml` is a DocBook 5 book with 2
chapters, 4 sections, ordered/unordered lists, and admonitions.
