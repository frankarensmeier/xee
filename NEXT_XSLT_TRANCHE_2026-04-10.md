# Next XSLT Tranche Notes: 2026-04-10

## Purpose

This note summarizes the discussion after checkpoint commit `9da441b5`
(`Checkpoint XSLT filter refresh tranche`). It is intended as a compact
restart document for a future chat that needs to choose the next XSLT work
item.

## Update since `9da441b5`

A follow-up reduction against a real DocBook NG stylesheet found and fixed a
generic namespace bug:

- Xee was dropping built-in static prefixes such as `xml` and `xs` from the
  XSLT parser context unless the stylesheet redeclared them explicitly.
- This caused generic `XPST0081` failures for expressions like `@xml:id`.
- The fix now preserves the default static namespace set before overlaying
  stylesheet prefixes.
- Focused regressions were added for implicit `xml` and implicit `xs` usage in
  XSLT expressions.

Most importantly, the minimal real DocBook NG transformation no longer fails
with `XPST0081`; it now fails later with `XTSE0090`. That means the DocBook
frontier advanced, and the next reduction target should be the specific
`XTSE0090` cause rather than any further namespace-prefix work.

## Current checkpoint state

- Repository: `/Users/brillo/Repositories/xee`
- Branch: `feature/xslt-match-rooted-patterns`
- Checkpoint commit: `9da441b5`
- Recent checkpoint scope:
  - refreshed `vendor/xslt-tests/filters`
  - fixed boolean XSLT dependency parsing in the testrunner
  - fixed circular include/import handling to report `XTSE0180`
  - added focused regressions for those fixes
- Validation at the checkpoint:
  - `python3 update.py` completed successfully
  - `cargo run --release --bin xee-testrunner -- -v check vendor/xslt-tests`
    finished clean with `Failed: 0 Error: 0 WrongE: 0`

## Important interpretation of the vendor summary

Current filtered sweep summary at the checkpoint:

```text
Total: 14595 Supported: 8793 Passed: 3318 Failed: 0 Error: 0 WrongE: 0 Filtered: 5475 Unsupported: 5802
```

The important testrunner semantics are:

- `Unsupported` means the test was skipped because its declared dependencies
  are not currently advertised by Xee.
- `Filtered` means the test is in-scope and considered supported by the
  testrunner, but it is still excluded by the current `vendor/xslt-tests/filters`
  baseline.
- `Supported` is not the same as `Passed`; in this runner, it is effectively
  `executed outcomes + filtered`.

So the large unsupported count is mostly a capability-declaration signal, not a
fresh regression signal.

## Decision principle for future work

The user explicitly discussed whether DocBook NG should influence prioritization.
The agreed conclusion was:

- Yes, DocBook NG is a legitimate prioritization signal.
- No, DocBook NG must not dictate the shape of the fixes.

Practical rule:

1. Use DocBook to identify what matters next.
2. Reduce the DocBook failure to the smallest generic reproducer.
3. Confirm the spec behavior.
4. Add or use focused vendor/compiler tests.
5. Implement the generic fix, not a DocBook-specific workaround.

In short: DocBook should steer priority, but not semantics.

## Current candidate next tranches

After the filter refresh, several small filtered buckets remain. The most
plausible near-term candidates that were examined are:

### `use-when`

- Remaining filtered case: `use-when-0136`
- Current direct result:
  - `COMPILATION ERROR Unsupported("Parse error: DTD is not supported")`
- Assessment:
  - This is a single-case follow-up.
  - It is generic, but it is fundamentally a DTD/parser-capability tranche.
  - It only makes sense next if DTD support is believed to matter for the real
    stylesheet path, including DocBook-related execution.

### `current`

- Remaining filtered case: `current-001`
- Current direct result:
  - `COMPILATION ERROR XPST0017 XPST0017`
- Assessment:
  - Very small and likely generic.
  - Potentially relevant to real stylesheet behavior because `current()` often
    matters in nontrivial match/pattern logic.
  - This is a good candidate if the goal is a tight semantic tranche with low
    blast radius.

### `include`

- Remaining filtered cases: `include-0102`, `include-0103`
- Current direct result:
  - both report `XTSE0165`
- Assessment:
  - This bucket is already very small.
  - The remaining cases probably concern fine-grained include semantics rather
    than basic loading.
  - Worth considering if the actual fixtures show generic module-processing
    gaps that large stylesheets may hit.

### `import`

- Remaining filtered case: `import-1301`
- Current direct result:
  - `COMPILATION ERROR XPST0017 XPST0017`
- Assessment:
  - Also very small.
  - Potentially valuable if it points to a real module or import-precedence
    semantic gap.

### Buckets not recommended as the immediate next tranche

- `docbook`
  - Too narrow as a named bucket; it risks steering fixes around one stylesheet
    family rather than the underlying generic features.
- very large buckets such as `error`, `number`, `key`, `match`, `package`,
  `override`, `accumulator`
  - too broad for the next checkpoint-sized step unless one specific subcluster
    is first isolated.

## Honest recommendation

If the goal is to stay disciplined while still moving toward DocBook NG, the
best next tranche should satisfy all three of these:

1. likely to matter for real stylesheet execution
2. generic enough to justify on spec grounds
3. small enough to close cleanly in one checkpoint

Based on the discussion so far, the best current order is:

1. `current-001`
   - best balance of small scope and likely semantic relevance
2. `import-1301`
   - small and probably tied to real stylesheet modularity semantics
3. `include-0102` / `include-0103`
   - also small, but slightly less clearly impactful without reading the exact fixtures
4. `use-when-0136`
   - worth doing only if parser/DTD support is intentionally the next tranche

## Suggested restart prompt

```text
Please read NEXT_XSLT_TRANCHE_2026-04-10.md and xslt-progress.md first.
We are in /Users/brillo/Repositories/xee on branch feature/xslt-match-rooted-patterns.
Checkpoint commit is 9da441b5 (Checkpoint XSLT filter refresh tranche).
Use DocBook NG as a prioritization signal, but keep fixes spec-generic.
Start by evaluating whether current-001, import-1301, or include-0102/include-0103
is the best next checkpoint-sized tranche.
```