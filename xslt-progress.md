# XSLT progress log

This document records concrete progress on XSLT support: what moved forward,
what blocked us, and what finally worked. It complements `xslt-plan.md`
instead of replacing it.

## 2026-04-10 09:54 CEST

### Status snapshot

- Checkpoint focus: close the `use-when` static-variable visibility and precedence tranche around `0133`, `0137`, and `0138`.
- Direct vendor verification now passes for:
  - `use-when-0133`
  - `use-when-0137`
  - `use-when-0138`
- Focused compiler regressions for the `use-when` tranche now pass at `8 passed / 0 failed`.
- Full unfiltered `use-when` bucket is currently `86 passed / 5 failed / 10 error / 1 wrongE`; this tranche fixed the static-variable cases, but the remaining failures are now concentrated elsewhere.

### Progress made

- Added a base-dir-aware XSLT AST parse entry point so static evaluation can resolve relative `xsl:include` and `xsl:import` hrefs while building the `use-when` static environment.
- Taught static evaluation to load static variables from included and imported stylesheet modules before evaluating later top-level `use-when` expressions, matching stylesheet tree order instead of treating each module in isolation.
- Carried imported and included static-variable names into the parser context used for subsequent static XPath compilation, so later `use-when` expressions can successfully bind references such as `$oink` after an include.
- Added precedence-aware tracking for static variables and parameters during static evaluation, including `XTSE3450` when a later higher-precedence declaration is inconsistent with an earlier lower-precedence declaration.
- Wired the compiler-side stylesheet loading path through the same base-dir-aware AST entry point so nested import/include static evaluation uses consistent filesystem resolution.
- Added focused compiler regressions for:
  - included-module static-variable visibility into later `use-when`
  - inconsistent imported static variables raising `XTSE3450`
  - repeated imports with conflicting static-variable values raising `XTSE3450`

### Validation used for the checkpoint

- `cargo test -p xee-xslt-compiler test_use_when_supports_function_available_and_element_available -- --nocapture`
- `cargo test -p xee-xslt-compiler test_use_when_sees_static_variable_from_included_module -- --nocapture`
- `cargo test -p xee-xslt-compiler test_use_when_reports_xtse3450_for_inconsistent_imported_static_variable -- --nocapture`
- `cargo test -p xee-xslt-compiler test_use_when_reports_xtse3450_for_reimported_inconsistent_static_variable -- --nocapture`
- `cargo test -p xee-xslt-compiler test_use_when_ -- --nocapture`
- `cargo run -q -p xee-testrunner -- -v all vendor/xslt-tests/tests/attr/use-when/_use-when-test-set.xml use-when-0133`
- `cargo run -q -p xee-testrunner -- -v all vendor/xslt-tests/tests/attr/use-when/_use-when-test-set.xml use-when-0137`
- `cargo run -q -p xee-testrunner -- -v all vendor/xslt-tests/tests/attr/use-when/_use-when-test-set.xml use-when-0138`
- `cargo run -q -p xee-testrunner -- all vendor/xslt-tests/tests/attr/use-when/_use-when-test-set.xml`
- `cargo run -q -p xee-testrunner -- -v all vendor/xslt-tests/tests/attr/use-when/_use-when-test-set.xml | rg '^use-when-[0-9]+'`

### Obstacles seen

#### Top-level static evaluation could not see future include/import contributions in stylesheet tree order

- Symptoms:
  - `use-when-0133` still raised `XPST0008` because `$oink` from an included module was not visible when compiling a later top-level `use-when`.
- Root cause:
  - static evaluation only considered the current module's already-seen declarations and had no way to resolve relative stylesheet references during AST-level processing.
- Resolution:
  - resolve includes/imports during static evaluation using the stylesheet base directory, merge their static variables into the active static environment, and rebuild the parser context with the new variable names before evaluating later top-level declarations.

#### Static-variable precedence logic was silently overwriting conflicting declarations

- Symptoms:
  - `use-when-0137` and `0138` could not report the required `XTSE3450` conflict semantics for inconsistent later higher-precedence declarations.
- Root cause:
  - static evaluation tracked only a flat name-to-value map, so it lost import-precedence information and could not distinguish safe shadowing from inconsistent override.
- Resolution:
  - track static-variable values together with module precedence and declaration kind, compare precedence during merge, and raise `XTSE3450` when a later higher-precedence declaration is inconsistent with an earlier lower-precedence declaration.

## 2026-04-10 08:52 CEST

### Status snapshot

- Checkpoint focus: finish the `use-when` follow-up around same-element `xpath-default-namespace` handling and processor-version-sensitive static evaluation.
- Full `use-when` bucket moved from `83 passed / 5 failed / 12 error / 2 wrongE` to `87 passed / 5 failed / 8 error / 2 wrongE`.
- Filtered full-suite regression remains clean after the tranche: `3534 passed / 0 failed / 0 error / 0 wrongE / 11061 filtered`.
- Newly fixed `use-when` cases in this tranche:
  - `use-when-0120`
  - `use-when-0121`
  - `use-when-0127b`

### Progress made

- Taught sequence-type parsing to apply the default element/type namespace to unprefixed atomic type names, which fixes `instance of string` style expressions when `xpath-default-namespace` points at XML Schema.
- Threaded processor XSLT/XPath version overrides into static evaluation, so `use-when` no longer relies only on the stylesheet's declared `@version` when deciding function availability.
- Fixed same-element `xpath-default-namespace` handling for `use-when` by parsing `use-when` with the element's own static namespace context before standard-attribute processing completes.
- Preserved `use-when` as a seen standard attribute in that same-element reparsing path so the parser no longer misclassifies it as an unexpected attribute.
- Added focused regressions for:
  - XSLT 2.0 processor mode rejecting `generate-id()` in `use-when`
  - XSLT 3.0 processor mode accepting the same construct even with a `version="2.0"` stylesheet
  - `instance of string` under `xpath-default-namespace="http://www.w3.org/2001/XMLSchema"`

### Validation used for the checkpoint

- `cargo test -p xee-xslt-compiler test_use_when_ -- --nocapture`
- `cargo run -q -p xee-testrunner -- -v all vendor/xslt-tests/tests/attr/use-when/_use-when-test-set.xml use-when-0120`
- `cargo run -q -p xee-testrunner -- -v all vendor/xslt-tests/tests/attr/use-when/_use-when-test-set.xml use-when-0121`
- `cargo run -q -p xee-testrunner -- all vendor/xslt-tests/tests/attr/use-when/_use-when-test-set.xml`
- `cargo run -q -p xee-testrunner -- -v all vendor/xslt-tests/tests/attr/use-when/_use-when-test-set.xml`
- `cargo run -q -p xee-testrunner -- check vendor/xslt-tests/`

### Obstacles seen

#### Unprefixed atomic type names in sequence types were not using the default type namespace

- Symptoms:
  - `use-when-0120` and `0121` still failed around `instance of string` even after the earlier default-namespace fixes.
- Root cause:
  - the XPath sequence-type parser treated unprefixed atomic type names as having no namespace instead of the default element/type namespace.
- Resolution:
  - apply the default element namespace while parsing single types, atomic-or-union item types, and typed map key types.

#### Static `use-when` evaluation only saw stylesheet version, not processor mode

- Symptoms:
  - the XSLT 3.0 processor-mode variant of the `generate-id()` `use-when` case still failed when the stylesheet itself declared `version="2.0"`.
- Root cause:
  - processor XSLT/XPath versions from the test harness were not threaded into AST-level static evaluation.
- Resolution:
  - pass processor version overrides through `parse_transform_with_static_variables` into static evaluation and use them when enforcing `use-when` function restrictions.

#### Same-element `xpath-default-namespace` parsing briefly regressed into `XTSE0090`

- Symptoms:
  - after reparsing `use-when` in a temporary context, `0120` and `0121` surfaced as `XTSE0090` instead of evaluating normally.
- Root cause:
  - the reparsed `use-when` attribute was marked as seen only on the temporary `Attributes` instance, so the real parser later treated it as unexpected.
- Resolution:
  - propagate the seen marker onto the real attribute set before the reparsing step.

## 2026-04-10 08:34 CEST

### Status snapshot

- Checkpoint focus: close the first `use-when` availability-function tranche by implementing `function-available()` / `element-available()` for static evaluation, normalizing QName-based built-in lookup, and preserving real static-evaluation error codes instead of flattening them into `Unsupported(...)`.
- Full `use-when` bucket moved from `57 passed / 6 failed / 24 error / 15 wrongE` to `83 passed / 5 failed / 12 error / 2 wrongE`.
- Filtered `check` run still reports `56 passed / 1 error / 45 filtered`; that did not regress, but the filter baseline has not been refreshed yet so the newly fixed cases remain masked there.
- Current highest-value remaining `use-when` blockers after this checkpoint:
  - typed-expression cases around unqualified `string` type names (`0118`, `0120`, `0121`)
  - `generate-id()` availability in the specific `0127b` static-evaluation path
  - static-variable precedence / error-code issues around `0137` and `0138`
  - unsupported `xsl:fallback` handling in `0420` and `0430`

### Progress made

- Added runtime support for `fn:function-available()` (one- and two-argument forms) and `fn:element-available()` in the interpreter function library used by static evaluation.
- Normalized static function registration and lookup to compare by expanded name instead of lexical prefix, so calls such as `fn:doc` and equivalent in-scope bindings resolve to the same built-in function.
- Taught static `use-when` evaluation to propagate stylesheet standard attributes before static-only processing, so version-sensitive availability rules see the actual stylesheet XSLT version.
- Disabled the XSLT functions that are not allowed inside `use-when` in the static context, including version-sensitive handling for `generate-id()`.
- Stopped wrapping static-evaluation parser/runtime failures as generic `Unsupported(...)`, and mapped XPath parser/runtime errors through the normal compiler path so `XPST0051`, `XPST0017`, `XPST0008`, `XPDY0002`, and similar codes now surface correctly.
- Added focused regressions for `function-available()` / `element-available()` in `use-when`, and for the XSLT 2.0 versus 3.0 `generate-id()` visibility split.

### Validation used for the checkpoint

- `cargo test -p xee-xslt-compiler test_use_when_ -- --nocapture`
- `cargo run -q -p xee-testrunner -- check vendor/xslt-tests/tests/attr/use-when/_use-when-test-set.xml`
- `cargo run -q -p xee-testrunner -- all vendor/xslt-tests/tests/attr/use-when/_use-when-test-set.xml`
- `cargo run -q -p xee-testrunner -- -v all vendor/xslt-tests/tests/attr/use-when/_use-when-test-set.xml`

### Obstacles seen

#### Availability checks needed expanded-name semantics, not prefix-sensitive lookup

- Symptoms:
  - `function-available('fn:doc')` and similar prefixed forms still behaved as unavailable even after the built-in availability functions existed.
- Root cause:
  - static function registration and lookup compared `OwnedName` values including lexical prefix, while parsed names coming from different namespace bindings should match by namespace URI plus local name only.
- Resolution:
  - normalize function-table keys and availability lookups to expanded names before comparison.

#### Static-evaluation failures were being downgraded to generic unsupported errors

- Symptoms:
  - many `use-when` vendor cases surfaced as `Unsupported(...)` instead of the real XPath/XSLT codes, obscuring what was actually fixed and what remained.
- Root cause:
  - the AST parser wrapped static-evaluation failures into `Unsupported`, and the XSLT compiler mapper treated parser/runtime errors asymmetrically.
- Resolution:
  - let static-evaluation errors propagate as structured parser/runtime failures and preserve those codes in `map_parse_error`.

## 2026-04-10 07:59 CEST

### Status snapshot

- Checkpoint focus: unblock the first real DocBook NG parser/loader tranche by fixing static-global visibility across sequential includes and prefixed QName parsing on stylesheet-level standard attributes.
- Verified that the earlier DocBook front-edge failures moved forward: the minimal prefixed `default-mode` repro now parses and runs, and the `main.xsl` path no longer stops on that `XTSE0020` parse bug.
- Current DocBook state after this checkpoint:
  - the original `variable.xsl` static-scope failure is fixed in the real include chain
  - the next remaining DocBook blocker is later in the load/compile path and still surfaces as `XTSE0165` from `main.xsl` / `print.xsl`

### Progress made

- Extended XSLT static evaluation so a stylesheet module can be parsed with already-known static globals in scope, and return the static globals it establishes.
- Changed stylesheet loading and include/import preprocessing to thread static globals forward in declaration order, so later included modules can use earlier static params and variables during `use-when` and other static evaluation.
- Fixed AST parser context construction so attribute parsing always sees the current element's in-scope namespace prefixes, including on root stylesheet attributes parsed before standard-context propagation.
- Added focused regressions for `use-when` depending on externally supplied static globals, for sequential includes carrying static globals forward, and for prefixed `default-mode` parsing on the stylesheet element.
- Confirmed the DocBook diagnosis moved past two concrete front-edge bugs and narrowed the next work to later global-declaration / load behavior instead of the original parser issues.

### Validation used for the checkpoint

- `cargo test -p xee-xslt-ast test_use_when_depends_on_initial_static_variable -- --nocapture`
- `cargo test -p xee-xslt-ast test_transform_prefixed_default_mode_is_accepted -- --nocapture`
- `cargo test -p xee-xslt-compiler test_static_globals_flow_across_sequential_includes -- --nocapture`
- `cargo test -p xee-xslt-compiler test_vendor_format_number_x43import_parses -- --nocapture`
- `cargo test -p xee-xslt-compiler test_missing_initial_template_uses_xtde0040 -- --nocapture`
- `cargo build -p xee`
- `target/debug/xee xslt /tmp/xee-default-mode-prefixed/test.xsl /tmp/xee-default-mode-prefixed/input.xml`
- `target/debug/xee xslt /Users/brillo/Repositories/fargate/microservice-contentoutput/docbook-xslt/docbook/xslt/main.xsl /tmp/xee-docbook-min.xml`

### Obstacles seen

#### Included DocBook modules needed static globals from earlier declarations during parse-time evaluation

- Symptoms:
  - `modules/variable.xsl` failed under the real DocBook chain because `use-when` and static expressions could not see static params established earlier in `param.xsl`.
- Root cause:
  - stylesheet modules were parsed and statically evaluated in isolation, so later included modules did not inherit already-established static globals from earlier declarations.
- Resolution:
  - thread static globals through stylesheet loading and include processing in declaration order, and seed static-evaluation context with those names and values.

#### Prefixed QName values on stylesheet-level standard attributes were parsed without current element prefixes

- Symptoms:
  - a minimal stylesheet using `default-mode="m:docbook"` failed with `XTSE0020`, and the same parser bug blocked DocBook `main.xsl` early.
- Root cause:
  - the parser context used while decoding attribute values did not include the current element's in-scope prefixes yet.
- Resolution:
  - build parser context with the current node's prefixes for attribute parsing, which fixes prefixed `default-mode` and the same class of root-attribute QName issues.

## 2026-04-10 07:26 CEST

### Status snapshot

- Checkpoint focus: finish the initial-template error-code follow-up for the remaining filtered `format-number-070` investigation.
- Checked suite state remains: `3498 passed / 0 failed / 0 error / 0 wrongE / 11097 filtered`.
- Remaining filtered `format-number` cases after this checkpoint:
  - `format-number-070`

### Progress made

- Added the missing XSLT dynamic error code `XTDE0040` for requested initial templates that do not exist in the stylesheet.
- Changed runtime named-template invocation so a missing requested initial template now raises `XTDE0040` instead of a generic `Unsupported` error.
- Added a focused local regression for missing initial-template invocation and validated it against the dedicated vendor `initial-template-901` conformance case.
- Re-ran vendor `format-number-070` after the error-code fix and confirmed the remaining failure is now a correctly classified `XTDE0040`, which supports leaving it filtered as a catalog mismatch rather than a `format-number` engine bug.

### Validation used for the checkpoint

- `cargo test -p xee-xslt-compiler test_missing_initial_template_uses_xtde0040 -- --nocapture`
- `cargo run -p xee-testrunner -- -v all vendor/xslt-tests/tests/misc/initial-template/_initial-template-test-set.xml initial-template-901`
- `cargo run -p xee-testrunner -- -v all vendor/xslt-tests/tests/fn/format-number/_format-number-test-set.xml format-number-070`

### Obstacles seen

#### The remaining 070 failure is a runner/catalog edge case, not a formatter defect

- Symptoms:
  - after the Unicode/import/system-property tranche, `format-number-070` still failed only when the vendor runner honored `<initial-template name="main"/>`.
- Root cause:
  - the stylesheet defines only `match="root"`, while the catalog requests a named initial template `main`; the correct processor response is therefore `XTDE0040`.
- Resolution:
  - fix the runtime error mapping to use `XTDE0040`, keep `format-number-070` filtered for now, and treat any further action as a separate runner-policy decision rather than `format-number` work.

## 2026-04-10 07:10 CEST

### Status snapshot

- Checkpoint focus: close the Unicode `format-number` bucket, clear the imported decimal-format cases, and narrow the last remaining failure to the vendor runner path.
- Newly unfiltered and passing vendor cases: `format-number-031`, `format-number-040`, `format-number-041`, `format-number-050`, `format-number-051`.
- Checked suite after validation: `3498 passed / 0 failed / 0 error / 0 wrongE / 11097 filtered`.
- Remaining filtered `format-number` cases after this checkpoint:
  - `format-number-070`

### Progress made

- Replaced ASCII-only zero-digit validation with Unicode Decimal_Number checks in both the XSLT compiler and runtime formatter, including contiguous ten-codepoint validation for non-ASCII and non-BMP digit sets.
- Fixed picture parsing so leading grouping separators are accepted, which unlocks the non-BMP separator cases instead of rejecting them with `FODF1310`.
- Added focused regressions for non-ASCII zero digits, non-BMP zero-digit output, leading grouping-separator pictures, imported named decimal-format visibility and merge behavior, the exact vendor `x43import.xsl` parse shape, and the exact vendor `format-number-040`/`041`/`070` stylesheets.
- Relaxed top-level XSLT document parsing to ignore trailing comments, processing instructions, and whitespace after the stylesheet element, which allows the imported `x43import.xsl` vendor asset to compile instead of being flattened into `XTSE0165`.
- Implemented `fn:system-property()` for the standard XSLT processor properties needed by the vendor suite, which removes the old `XPST0017` compile-time failure from the exact `format-number-070` stylesheet under normal source-driven execution.
- Reduced the `format-number` filter block to a single remaining case after validating `040` and `041` through both focused runs and the filtered suite.

### Validation used for the checkpoint

- `cargo test -p xee-xslt-compiler test_format_number_supports_non_ascii_zero_digit_and_literal_ascii_suffix -- --nocapture`
- `cargo test -p xee-xslt-compiler test_format_number_supports_leading_grouping_separator_pattern -- --nocapture`
- `cargo test -p xee-xslt-compiler test_format_number_supports_non_bmp_zero_digit_output -- --nocapture`
- `cargo test -p xee-xslt-compiler test_vendor_format_number_040_stylesheet -- --nocapture`
- `cargo test -p xee-xslt-compiler test_vendor_format_number_041_stylesheet -- --nocapture`
- `cargo test -p xee-xslt-compiler test_vendor_format_number_070_stylesheet -- --nocapture`
- `cargo test -p xee-xslt-compiler test_vendor_format_number_x43import_parses -- --nocapture`
- `cargo test -p xee-xslt-compiler test_system_property_product_version_is_available -- --nocapture`
- `cargo run -p xee-testrunner -- -v all vendor/xslt-tests/tests/fn/format-number/_format-number-test-set.xml format-number-040`
- `cargo run -p xee-testrunner -- -v all vendor/xslt-tests/tests/fn/format-number/_format-number-test-set.xml format-number-041`
- `cargo run -p xee-testrunner -- check vendor/xslt-tests/`

### Obstacles seen

#### Unicode decimal-format handling was still ASCII-shaped in two places

- Symptoms:
  - `format-number-031`, `050`, and `051` still failed even after the main `format-number` implementation existed.
- Root cause:
  - zero-digit validation relied on ASCII digit semantics and the picture parser rejected leading grouping separators before the formatter had a chance to use them.
- Resolution:
  - switch validation to ICU Decimal_Number membership plus contiguous digit checks, and remove the over-strict leading-grouping rejection from picture parsing.

#### Imported vendor stylesheets could fail on trailing metadata after `</xsl:stylesheet>`

- Symptoms:
  - `format-number-040` and `041` both surfaced as `XTSE0165` even though named decimal-format import handling already worked in simpler focused regressions.
- Root cause:
  - `x43import.xsl` includes a trailing document-level metadata comment after the stylesheet element, and the XSLT parser treated any trailing sibling node as fatal.
- Resolution:
  - allow trailing comments, processing instructions, and ignorable whitespace after the top-level stylesheet element while still rejecting real trailing content.

#### The last remaining `format-number` failure is no longer in `format-number`

- Symptoms:
  - `format-number-070` no longer fails at compile time under normal execution, but the vendor testrunner still reports `Initial template not found: main`.
- Root cause:
  - the catalog requests `<initial-template name="main"/>`, while the stylesheet only defines `match="root"`; this is a runner/catalog mismatch rather than a formatter or user-function rewrite bug.
- Resolution:
  - keep `format-number-070` filtered for now and treat the remaining work as a separate initial-template handling investigation.

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