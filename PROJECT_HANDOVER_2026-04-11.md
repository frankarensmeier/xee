# Project Handover — 2026-04-11 00:45 CEST

## What is this project?

Rust XSLT 3.0 processor (`xee`) — a multi-crate workspace at `/Users/brillo/Repositories/xee`.
Fork: `frankarensmeier/xee`. Branch: `feature/xslt-match-rooted-patterns`.

The driving goal is making `xee` capable of running a real **DocBook NG XSLT stylesheet** end-to-end. Each session identifies the next blocker in the live DocBook path and implements just enough to move past it.

## Current state

**Clean working tree.** Latest commit: `494642a8 — Checkpoint key() function support`.

### Recent commit chain (newest first)

```
494642a8 Checkpoint key() function support
8e92d977 Checkpoint xsl:number value-form support
12b184db Checkpoint xsl:evaluate runtime support
97eb64d0 Checkpoint xsl:text expand-text regression fix
6548e2b7 Checkpoint xsl:map lowering and DocBook frontier
36bf6100 Checkpoint DocBook pattern and static-scope fixes
```

### Regression baseline

- **XSLT conformance sweep**: 3318 passed / 0 failed / 0 error / 5475 filtered / 5802 unsupported
- Filter baseline updated via `update.py`
- `cargo test` (full workspace unit tests) was green at last manual run

### DocBook frontier

The live DocBook stylesheet error is now:

```
[Unsupported] Error: xsl:map currently only supports xsl:map-entry children
```

Command to reproduce:
```bash
cargo run -q -p xee -- xslt \
  /Users/brillo/Repositories/fargate/microservice-contentoutput/docbook-xslt/docbook/xslt/main.xsl \
  /tmp/xee-docbook-min.xml
```

The minimal DocBook input is at `/tmp/xee-docbook-min.xml`.

## What was done this session

Three checkpoint commits, each moving the DocBook frontier forward:

1. **`xsl:evaluate`** (`12b184db`) — runtime dynamic XPath compilation + execution via `DynamicXPathEvaluator` trait. Hidden `xslt-evaluate(...)` helper. Three focused tests.

2. **`xsl:number` value-form** (`8e92d977`) — hidden `xslt-number-value` helper supporting `1/a/A/i/I` picture characters + dynamic format AVTs. Two focused tests.

3. **`key()` function** (`494642a8`) — full pipeline: `xsl:key` declarations compile through AST → IR `KeyDefinition` → runtime `KeyDeclaration` with compiled pattern + use-expression function. `fn:key()` implemented as a standard 3-arg XPath function (`context_last`). Two focused tests.

## What to do next

### Immediate: `xsl:map` with non-entry children

The current blocker. `xsl:map` already works with `xsl:map-entry` children, but DocBook uses `xsl:map-entry` mixed with other XSLT instructions (likely `xsl:for-each` producing entries). The lowering in `ast_ir.rs` only handles the simple case.

### After that: likely more DocBook frontier chasing

Run the live DocBook command above after each fix to discover the next blocker. The pattern is:

1. Run DocBook → see error
2. Trace the error to a specific XSLT instruction or XPath function
3. Implement the minimum needed
4. Write focused test(s)
5. Run `cargo test -p xee-xslt-compiler --test test_xslt <test_name> -- --nocapture`
6. Run the filtered regression sweep: `cargo run -q -p xee-testrunner -- check vendor/xslt-tests/`
7. Update `xslt-progress.md` with a timestamped entry
8. Checkpoint commit

### Known gaps that may surface soon

- **`xsl:number`** node-based forms (`level="single"`, `count=`, `from=`) — only `value=` is implemented
- **`key()` performance** — currently re-walks the subtree on every call (no index caching)
- **`key()` namespace resolution** — key names are resolved as no-namespace local names; namespaced key names would need QName parsing against static context
- **`xsl:function`** — already works (compiled as global variables holding function definitions), but `xsl:function` with overloading across imports may have edge cases

## Architecture cheat sheet

### Crate dependency flow

```
xee-xslt-ast (XSLT parsing)
    ↓
xee-xslt-compiler (AST → IR lowering in ast_ir.rs)
    ↓
xee-ir (ANF intermediate representation + DeclarationCompiler → bytecode)
    ↓
xee-interpreter (runtime: Interpreter, Declarations, pattern matching, library functions)
```

### XSLT instruction lowering pattern

XSLT instructions lower to **hidden `fn:xslt-*` static function calls** in IR:

1. `ast_ir.rs` — match on AST instruction, compile arguments, emit `static_function_call_expr("xslt-foo", FN_NAMESPACE, arity, args)`
2. `hidden_xslt.rs` — implement the `#[xpath_fn("fn:xslt-foo(...)")] fn xslt_foo(...)` body
3. Register in `hidden_xslt::static_function_descriptions()`

### Key files

| Purpose | File |
|---------|------|
| XSLT AST → IR lowering | `xee-xslt-compiler/src/ast_ir.rs` |
| IR declaration compiler | `xee-ir/src/declaration_compiler.rs` |
| IR data structures | `xee-ir/src/ir.rs` |
| Runtime declarations | `xee-interpreter/src/declaration/decl.rs` |
| Hidden XSLT helpers | `xee-interpreter/src/library/hidden_xslt.rs` |
| key() + id() + generate-id() | `xee-interpreter/src/library/id.rs` |
| Pattern matching core | `xee-interpreter/src/pattern/pattern_core.rs` |
| XSLT end-to-end tests | `xee-xslt-compiler/tests/test_xslt.rs` |
| Progress log | `xslt-progress.md` |

### Test commands

```bash
# Focused XSLT test
cargo test -p xee-xslt-compiler --test test_xslt <test_name> -- --nocapture

# Full workspace unit tests
cargo test

# Filtered XSLT conformance sweep (must be zero failures)
cargo run -q -p xee-testrunner -- check vendor/xslt-tests/

# Update filter baseline after new passes
python3 update.py

# Live DocBook frontier
cargo run -q -p xee -- xslt <stylesheet> <input>
```

## Workflow preferences (from user memory)

- Update `xslt-progress.md` with timestamped entries at each checkpoint
- Run focused tests while iterating; run filtered regression sweep before checkpoint
- Use `update.py` when refreshing the filter baseline
- Commit message pattern: `Checkpoint <feature> support`
- Values readability and elegance — challenge on code smells
