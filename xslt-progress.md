# XSLT progress log

This document records concrete progress on XSLT support: what moved forward,
what blocked us, and what finally worked. It complements `xslt-plan.md`
instead of replacing it.

## 2026-04-10 00:49 CEST

### Status snapshot

- Checkpoint focus: finish the `format-number` exponent-separator tranche and close the vendor-side `069b` placeholder mismatch.
- Newly unfiltered and passing vendor cases: `format-number-069a`, `format-number-069b`.
- Checked suite after validation: `3378 passed / 0 failed / 0 error / 0 wrongE / 11217 filtered`.
- Remaining filtered `format-number` cases after this checkpoint:
  - `format-number-031`
  - `format-number-040`
  - `format-number-041`
  - `format-number-050`
  - `format-number-051`
  - `format-number-070`

### Progress made

- Added processor XPath version to the shared static context and static-context builder so XSLT compilation can gate version-sensitive behavior on processor capabilities instead of stylesheet `@version` alone.
- Changed decimal-format validation so `exponent-separator` is accepted only in XSLT 3.0 processor mode with XPath 3.1 enabled, while the existing XSLT 2.0 rejection path still yields `XTSE0090`.
- Implemented exponent-picture parsing and formatting in `fn:format-number`, including exponent digit padding and post-rounding exponent carry handling for patterns such as `0.0000E0`.
- Taught the XSLT test runner to read XSLT vendor dependency metadata from `<spec>` and `<feature>` elements and use it to override per-test processor XSLT/XPath versions.
- Treated vendor `<error code="XXX"/>` expectations as an any-error placeholder in the XSLT assertion path, which resolves the `format-number-069b` suite convention cleanly.
- Added focused regressions for exponent-separator acceptance/rejection and for XSLT testcase loader extraction of processor versions from vendor dependency metadata.

### Validation used for the checkpoint

- `cargo test -p xee-xslt-compiler format_number -- --nocapture`
- `cargo test -p xee-testrunner processor_versions_from_dependencies -- --nocapture`
- `cargo run -p xee-testrunner -- -v all vendor/xslt-tests/tests/fn/format-number/_format-number-test-set.xml format-number-069`
- `cargo run -p xee-testrunner -- check vendor/xslt-tests`

### Obstacles seen

#### Exponent-separator support needed both gating and runtime formatting

- Symptoms:
  - `format-number-069a` initially failed with `XTSE0090`, then later compiled but raised `FODF1310` because the formatter still rejected exponent pictures.
- Root cause:
  - acceptance was tied to stylesheet version instead of processor capability, and the runtime picture parser/formatter had no scientific-notation branch.
- Resolution:
  - add explicit processor XPath version state, gate decimal-format exponent support on processor XSLT plus XPath versions, and implement exponent-picture parsing/formatting in the numeric library.

#### XSLT vendor dependency metadata uses a different shape than the generic dependency loader

- Symptoms:
  - live testrunner runs ignored the intended processor-version overrides even after focused loader experiments worked on synthetic XML.
- Root cause:
  - the XSLT vendor catalog encodes dependencies as `<spec>` and `<feature>` children under `<dependencies>`, not as generic `<dependency type=...>` elements.
- Resolution:
  - parse the XSLT-specific `<spec>` and `<feature>` elements directly in the XSLT testcase loader instead of reusing the generic dependency element shape.

## 2026-04-10 00:26 CEST

### Status snapshot

- Checkpoint focus: push `format-number` and `xsl:decimal-format` support forward far enough to unfilter the next real conformance tranche.
- Newly unfiltered and passing vendor cases: `format-number-057n`, `format-number-060n`, `format-number-063`.
- Checked suite after validation: `3374 passed / 0 failed / 0 error / 2 wrongE / 11219 filtered`.
- Remaining filtered `format-number` cases after this checkpoint:
  - `format-number-031`
  - `format-number-040`
  - `format-number-041`
  - `format-number-050`
  - `format-number-051`
  - `format-number-069a`
  - `format-number-069b`
  - `format-number-070`

### Progress made

- Implemented `fn:format-number` runtime support with picture parsing, grouping, digit substitution, infinity/NaN handling, and decimal-format lookup.
- Added shared static-context storage for default and named decimal formats, including import-precedence merge behavior and XSLT decimal-format validation.
- Split stylesheet `@version` from processor XSLT version so XSLT 3.0 conformance cases no longer inherit 2.0-only `format-number` error remapping from a `version="2.0"` stylesheet.
- Added a narrow XSLT-only lexical fallback for oversized decimal literals used as the first `format-number(...)` argument.
- Preserved in-scope namespaces on parsed XSLT expressions and used that context to normalize static third-argument decimal-format QNames before runtime lookup.
- Added focused regressions for default decimal-format symbols, import-precedence decimal-format merging, high-precision decimal literals, processor-version-sensitive invalid-picture errors, and prefixed decimal-format QName resolution.
- Reduced the checked `format-number` filter block by three more cases: `057n`, `060n`, and `063`.

### Obstacles seen

#### Processor behavior was tied too tightly to stylesheet `@version`

- Symptoms:
  - `format-number-057n` and `format-number-060n` returned `XTDE1310` instead of `FODF1310`.
- Root cause:
  - the runtime remap for invalid-picture errors consulted the stylesheet version stored in static context, which conflated the stylesheet's compatibility declaration with the processor mode expected by the conformance case.
- Resolution:
  - store stylesheet XSLT version separately from processor XSLT version and base the remap on processor mode only.

#### Static decimal-format QNames lost local namespace context

- Symptoms:
  - `format-number-063` failed with `FODF1280` when the third `format-number()` argument used a prefix declared on the containing XSLT instruction.
- Root cause:
  - runtime QName parsing only saw the global static-context namespaces, while the relevant prefix binding existed only in the expression's local XSLT namespace context.
- Resolution:
  - preserve literal namespaces on parsed expressions and rewrite static third-argument QName string literals into expanded `Q{...}local` form during XSLT compilation.

### Working practices that helped

- Unfilter only after both a focused vendor case rerun and a checked-suite rerun agree on the result.
- Keep the QName fix in the XSLT AST/compiler layers rather than pushing local XSLT namespace semantics into generic XPath runtime code.

### Current open edges

- `format-number-069a` and `format-number-069b` still need proper exponent-separator feature gating.
- `format-number-069b` still has a vendor placeholder expected code, so it likely needs a policy decision as well as an implementation change.
- The remaining filtered `format-number` cases are now narrow conformance gaps rather than missing baseline support.

## 2026-04-09 22:32:18 CEST

### Status snapshot

- Checkpoint focus: close the last import-local runtime failure in `decl/import`.
- Import bucket after fix: `31 passed / 0 failed / 0 error / 0 wrongE / 11 filtered`.
- Validation used for the checkpoint:
  - `cargo test -p xee-xslt-compiler test_imported_predicate_match_patterns_restore_state_after_swallowed_error -- --nocapture`
  - `cargo run --release --bin xee-testrunner -- check vendor/xslt-tests/tests/decl/import/_import-test-set.xml`

### Progress made

- Fixed the remaining import-local runtime failure behind `import-1201`.
- Added interpreter state checkpoints so swallowed pattern-predicate errors no longer leak stack, frame, or build-stack mutations into later predicate or template evaluation.
- Added a focused regression that reproduces the imported predicate interaction and proves later matching still succeeds after an earlier swallowed predicate failure.
- Confirmed the remaining non-pass import cases are no longer import-mechanics work: `import-0001` and `import-0002` still point at missing `format-number`, while `import-1301` still points at incomplete `key()` support.

### Obstacles seen

#### Swallowed pattern-predicate errors corrupted later evaluation state

- Symptoms:
  - `import-1201` raised `FORG0001` only when multiple predicate-based template rules were present together.
  - Each predicate rule could pass in isolation, but the combined imported rule set failed at runtime.
- Root cause:
  - pattern predicate evaluation intentionally swallowed dynamic errors, but the inline function call path left behind mutated interpreter state after the failed predicate call.
  - later predicate or template evaluation then ran against a corrupted stack/frame state.
- Resolution:
  - add explicit interpreter state checkpoints and restore them whenever a swallowed predicate evaluation fails.

### Working practices that helped

- Reduce a failing conformance case to minimal imported rule subsets until the failure changes shape.
- Keep one focused regression that isolates the runtime invariant, then re-check the real bucket command before checkpointing.

### Current open edges

- The remaining import non-pass cases are broader feature gaps, not import-specific behavior:
  - `import-0001`
  - `import-0002`
  - `import-1301`
- The next highest-yield follow-up is `format-number`, because it blocks two of the three remaining import cases.

## 2026-04-09

### Status snapshot

- Checkpoint focus: finish `decl/attribute-set` after the earlier import and instruction-path work.
- Attribute-set bucket after fixes: `50 passed / 0 failed / 0 error / 0 wrongE`.
- Filtered suite after validation: `3311 passed / 0 failed / 0 error / 0 wrongE / 11284 filtered`.

### Progress made

- Fixed instruction-level `use-attribute-sets` for `xsl:element` and `xsl:copy`, which removed the broad execution-path regressions in the attribute-set bucket.
- Isolated attribute-set compilation from local variable scopes so attribute-set declarations now see top-level variables without leaking template-local bindings.
- Added static validation for referenced attribute sets, which correctly raises `XTSE0710` for missing nested sets.
- Corrected `xsl:attribute` default separator behavior so sequence-constructor content defaults to the empty string while `select` still defaults to a space separator.
- Added `xml:base` support on `xsl:attribute-set` declarations and folded exact `static-base-uri()` calls under that overridden base URI so declaration-local base URIs are preserved.

### Obstacles seen

#### Attribute-set declarations saw the wrong variable scope

- Symptoms:
  - `attribute-set-1802` used a template-local variable where the spec requires only top-level variables and params to be visible.
- Root cause:
  - attribute-set bodies were compiled in the caller's live variable scope.
- Resolution:
  - compile attribute-set bodies with only the global variable scope retained, while preserving the current focus.

#### Missing referenced attribute sets were not rejected statically

- Symptoms:
  - `attribute-set-1003` reached an unrelated runtime error instead of failing with `XTSE0710`.
- Root cause:
  - nested `use-attribute-sets` references were only resolved lazily when an attribute set happened to be executed.
- Resolution:
  - validate all collected attribute-set references during compilation.

#### `xsl:attribute` used the wrong default separator for content

- Symptoms:
  - `attribute-set-1811` produced `"1 2 3"` where the expected default was `"123"`.
- Root cause:
  - attribute construction reused the same default separator path for both `select` and sequence-constructor content.
- Resolution:
  - use the empty-string default for content and keep the space default for `select`.

#### Declaration-local `xml:base` was parsed but not honored semantically

- Symptoms:
  - `attribute-set-1814` initially failed with `XTSE0090`, and after parser acceptance still returned the stylesheet file URI instead of the declaration-local base URIs.
- Root cause:
  - `xml:base` was not accepted on `xsl:attribute-set`, and `static-base-uri()` continued to observe only the program-wide static context.
- Resolution:
  - add AST/parser support for `xml:base` on attribute sets, temporarily override static base URI while compiling those declarations, and fold exact `static-base-uri()` calls under the override.

### Working practices that helped

- Finish the shared mechanism once the bucket shows the same failure shape across many tests; the instruction-path fix cleared most of the bucket at once.
- Re-run the filtered `xee-testrunner -- check vendor/xslt-tests/` sweep before checkpointing bucket-level work.

### Current open edges

- `decl/import` still has four error cases: `import-0001`, `import-0002`, `import-1201`, and `import-1301`.
- The standalone prefixed `xsl:element` namespace unit expectation still disagrees with current serialization behavior and predates these checkpoints.

## 2026-04-09

### Status snapshot

- Checkpoint focus: `xsl:apply-imports`, import precedence/import ancestry, import ambiguity handling, and instruction-level `use-attribute-sets`.
- Filtered suite before checkpoint: `3311 passed / 0 failed / 0 error / 0 wrongE / 11284 filtered`.
- Import bucket after fixes: `38 passed / 0 failed / 4 error / 0 wrongE`.
- Attribute-set bucket after shared instruction fix: `46 passed / 2 failed / 1 error / 1 wrongE`.

### Progress made

- Fixed `xsl:apply-imports` so continuation lookup respects stylesheet module ancestry instead of only raw import precedence.
- Added runtime handling for `on-multiple-match="error"` in the XSLT test runner and interpreter, which fixed the `XTRE0540` ambiguity cases in the import bucket.
- Mapped imported and included stylesheet load failures to `XTSE0165`, and preserved more specific parse-derived static errors via parse-error mapping.
- Fixed instruction-level `use-attribute-sets` for `xsl:element` and `xsl:copy`, which cleared the remaining plain import failure and a broad set of attribute-set regressions.
- Added focused regression coverage for import-chain `apply-imports`, multiple-match error handling, and instruction-level attribute sets.

### Obstacles seen

#### `apply-imports` chose templates from the wrong import branch

- Symptoms:
  - the dedicated `apply-imports` bucket failed even though matching templates existed.
- Root cause:
  - continuation lookup compared only numeric import precedence and could continue into sibling import branches.
- Resolution:
  - thread stylesheet `module_path` ancestry through preprocessing, IR, declarations, and interpreter lookup.

#### Import ambiguity cases expected `XTRE0540`

- Symptoms:
  - `import-0502b` and `import-0902b` matched multiple top-ranked templates but did not raise the expected error.
- Root cause:
  - runtime lookup only returned a chosen template id and discarded ambiguity information.
- Resolution:
  - store `TemplateRule` metadata in mode lookup, detect same-rank ambiguity at runtime, and let the XSLT runner enable fail-on-multiple-match from catalog metadata.

#### Imported stylesheet failures surfaced as generic unsupported errors

- Symptoms:
  - `import-2103`, `import-2402`, and `import-2404` reported generic unsupported compilation failures.
- Root cause:
  - imported stylesheet load and parse failures were converted too early into generic unsupported errors.
- Resolution:
  - map unreadable imports/includes to `XTSE0165` and feed parse failures through `map_parse_error(...)`.

#### Instruction-level attribute sets were silently ignored

- Symptoms:
  - `import-0701` and many `decl/attribute-set` cases constructed the right element but dropped attributes from `use-attribute-sets`.
- Root cause:
  - `xsl:element` and `xsl:copy` parsed `use-attribute-sets` but never appended those resolved attribute nodes in their compiler paths.
- Resolution:
  - append resolved attribute-set bindings for both instructions and cover both paths with focused tests.

### Working practices that helped

- Use focused bucket runs first to distinguish a local feature bug from broader unsupported surface area.
- Re-run the filtered `xee-testrunner -- check vendor/xslt-tests/` sweep before treating a change as checkpoint-ready.
- Keep catalog-behavior wiring localized when possible. The broad dependency-loader approach created avoidable filtered-suite fallout; the XSLT-specific runner hook was the safer seam.

### Current open edges

- The import bucket still has four error cases: `import-0001`, `import-0002`, `import-1201`, and `import-1301`.
- The attribute-set bucket is much healthier, but `attribute-set-1003`, `attribute-set-1802`, `attribute-set-1811`, and `attribute-set-1814` remain open.
- A standalone unit expectation around prefixed `xsl:element` namespace serialization still fails, but it predates this checkpoint and was not introduced by the current fixes.

## How to use this log

- Add new entries at the top.
- Record before/after numbers when they are known.
- Note the obstacle, the root cause, and the fix.
- Mention the validating command or workflow when it matters.

### Entry template

```md
## YYYY-MM-DD

### Status snapshot

- Checkpoint commit:
- Baseline:

### Progress made

-

### Obstacles seen

#### Topic

- Symptoms:
- Root cause:
- Resolution:

### Working practices that helped

-

### Current open edges

-
```

## 2026-04-09

### Status snapshot

- Current checkpoint commit: `d68ab2c8` (`Fix XSLT namespace and assertion regressions`).
- Recent checkpoint chain:
  - `e610db02` `Support attribute-set expansion and unfilter 33 tests`
  - `a978e85a` `Implement result-document serialization checkpoint`
  - `c9728e36` `Extend result-document serialization support`
  - `d68ab2c8` `Fix XSLT namespace and assertion regressions`
- Filtered suite after refresh: `3311 passed / 0 failed / 0 error / 0 wrongE / 11284 filtered`.

### Progress made

- `xsl:result-document` behavior improved and several regressions were fixed.
- Namespace handling for literal result elements is more correct.
- XPath assertion handling is more accurate for singleton element results.
- Computed XSLT names now resolve with stylesheet namespace context instead of relying on weak string-based fallback behavior.
- The filter file was refreshed to reflect newly passing groups.

### Obstacles seen

#### Result-document regressions

- Symptoms:
  - `result-document-0201`, `result-document-0209`, and `result-document-0210` regressed.
- Root causes:
  - literal namespace handling was incomplete
  - assertion serialization inherited XML defaults for HTML/XHTML checks
- Resolution:
  - fixed namespace capture for literal result elements
  - normalized HTML/XHTML assertion media type handling

#### Context-sensitive namespace failures

- Symptoms:
  - `namespace-3401` passed in isolation but failed in the full bucket
- Root cause:
  - default namespace and literal namespace retention were not being preserved correctly through compilation and serialization
- Resolution:
  - collect in-scope namespaces for literal result elements
  - respect `exclude-result-prefixes`
  - emit explicit `xmlns=""` when an unnamespaced literal element needs to clear an inherited default namespace

#### Wrong XPath assertion context

- Symptoms:
  - several filtered failures looked like evaluator bugs but were actually harness mismatches
- Root cause:
  - XPath assertions over singleton element results were not using a document-rooted context when the suite expected `/out`-style navigation
- Resolution:
  - normalize singleton non-document node results into a document for XPath assertions

#### Dynamic XSLT QName handling

- Symptoms:
  - computed `xsl:element` and `xsl:attribute` names produced incorrect namespace behavior in edge cases
- Root cause:
  - dynamic names lacked reliable resolution against stylesheet namespaces and default element namespace rules
- Resolution:
  - added a hidden runtime helper to resolve XSLT QNames with encoded stylesheet namespace context

### Working practices that helped

- Compare commits directly when a regression appears. The relevant first bad commit in this cycle was `c9728e36`, not `a978e85a`.
- Use targeted bucket runs while debugging, then refresh the global picture afterwards.
- Prefer `python3 update.py` for filter maintenance. It updates per test group and skips `misc/unicode-90`, which keeps refreshes practical.
- Create checkpoint commits after each meaningful stabilizing step.

### Current open edges

- Many tests remain filtered, so there is still substantial unsupported surface area.
- `xsl:use-package` is still unsupported and blocks some otherwise nearby test cases.
- `xsl:apply-imports` remains a good short-term target because its dedicated bucket currently has one behavioral failure instead of a broad unsupported feature wall.