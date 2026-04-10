# Next XSLT Tranche Notes: 2026-04-10

## Purpose

This note summarizes the discussion after checkpoint commit `9da441b5`
(`Checkpoint XSLT filter refresh tranche`). It is intended as a compact
restart document for a future chat that needs to choose the next XSLT work
item.

## Update since `8339327c`

Three more DocBook-driven reductions after the built-in-namespace checkpoint
turned into generic parser fixes:

- `modules/index.xsl` exposed that `lang` was mapped internally as
  `language`; fixing that cleared the reduced `XTSE0090` on
  `xsl:sort lang="{$lang}"`.
- `modules/programming.xsl` exposed that `xsl:text` content was incorrectly
  routed through AVT parsing; literal `{` and `}` in `xsl:text` are now kept
  as text.
- `modules/footnotes.xsl` exposed that multiline AVTs on literal result
  element attributes failed when indentation appeared before the closing `}`;
  the AVT tokenizer now consumes trailing whitespace before that brace.

The practical outcome is that these direct DocBook module runs all moved
forward:

- `modules/index.xsl` now reaches `XPST0017` instead of `XTSE0090`.
- `modules/programming.xsl` now reaches `XPST0017` instead of a value-template
  parse failure.
- `modules/footnotes.xsl` now reaches an unsupported `xsl:number` path instead
  of `ValueTemplate(UnescapedCurly { c: '}' ... })`.

The new real frontier is now later in the top-level preprocessing layer:

- `docbook.xsl` and `print.xsl` now fail with `XPST0003`.
- That `XPST0003` should be the next reduction target.
- The working assumption is that it is still a generic XPath/XSLT parser gap,
  not a DocBook-specific requirement, but it has not yet been isolated to a
  smallest reproducer.

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
- Latest committed checkpoint before this note's current worktree: `8339327c`
- Recent checkpoint scope:
  - preserve built-in static namespaces in the XSLT parser context
  - fix `lang` name mapping for `xsl:sort`
  - treat `xsl:text` braces as literal text
  - fix multiline literal-result-element AVTs with indentation before `}`
  - add focused regressions for each of those generic parser fixes
- Validation in the current worktree:
  - `cargo test -p xee-xslt-compiler test_sort_lang_attribute_is_parsed_before_compile_support_check -- --nocapture`
  - `cargo test -p xee-xslt-compiler test_xsl_text_treats_curly_braces_as_literal_text -- --nocapture`
  - `cargo test -p xee-xslt-ast test_string_with_multiline_value_and_trailing_whitespace_before_closing_curly -- --nocapture`
  - `cargo test -p xee-xslt-compiler test_literal_result_attribute_value_template_allows_trailing_whitespace_before_closing_curly -- --nocapture`
  - `cargo run --bin xee -- xslt .../modules/index.xsl /tmp/xee-docbook-min.xml`
  - `cargo run --bin xee -- xslt .../modules/programming.xsl /tmp/xee-docbook-min.xml`
  - `cargo run --bin xee -- xslt .../modules/footnotes.xsl /tmp/xee-docbook-min.xml`
  - `cargo run --bin xee -- xslt .../docbook.xsl /tmp/xee-docbook-min.xml`
  - `cargo run --bin xee -- xslt .../print.xsl /tmp/xee-docbook-min.xml`

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

## Current candidate next tranche

The next checkpoint-sized tranche should not go back to the earlier vendor-only
ranking. The real DocBook frontier is now more informative:

1. Reduce the `docbook.xsl` / `print.xsl` `XPST0003` to a smallest generic
   reproducer.
2. Confirm whether the culprit is one specific XPath/XSLT syntax family in the
   preprocessing layer, such as map-related syntax or another parser boundary.
3. Add a focused regression and fix that generic gap.

The earlier small vendor candidates like `current-001`, `import-1301`, or
`include-0102` remain valid side quests, but they are no longer the best next
priority if the goal is to keep pushing real stylesheet execution forward with
checkpoint-sized generic fixes.

## Honest recommendation

The current recommendation is simpler now:

1. stay on the real DocBook frontier
2. reduce `docbook.xsl` `XPST0003` to a generic reproducer
3. fix only that generic parser/compiler gap

This keeps the prioritization signal honest: the current blockers being exposed
by real stylesheet execution are still yielding generic parser fixes with a
small enough blast radius for checkpoint work.

## Suggested restart prompt

```text
Please read NEXT_XSLT_TRANCHE_2026-04-10.md and xslt-progress.md first.
We are in /Users/brillo/Repositories/xee on branch feature/xslt-match-rooted-patterns.
The latest committed checkpoint before the current worktree is 8339327c
(Checkpoint built-in XSLT namespace fix).
Use DocBook NG as a prioritization signal, but keep fixes spec-generic.
Start by reducing the current docbook.xsl / print.xsl XPST0003 failure to a
smallest generic reproducer before choosing any broader tranche.
```