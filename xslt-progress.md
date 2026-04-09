# XSLT progress log

This document records concrete progress on XSLT support: what moved forward,
what blocked us, and what finally worked. It complements `xslt-plan.md`
instead of replacing it.

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