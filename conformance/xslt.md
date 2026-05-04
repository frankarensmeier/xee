# XSLT 3.0 conformance

Status as of 2026-05-04. Per element.

**Overall:** 14,595 total vendor tests. 5,692 passed, 0 failed, 0 error,
3,886 filtered, 5,017 unsupported (streaming/schema/packages).
Of ~9,578 in-scope tests, 5,692 pass (59%).

## xsl:accept

Not planned — part of the xsl:package system (see xsl:package).
50 tests filtered.

## xsl:accumulator

Done. Accumulators are compiled and evaluated at runtime (non-streaming
mode). The accumulator rules are matched against document nodes and values
are accumulated during tree traversal.

64 tests filtered — many require streaming behavior or edge cases not
yet handled.

## xsl:accumulator-rule

See xsl:accumulator.

## xsl:analyze-string

Done. Uses regexml for regex matching. Supports matching-substring,
non-matching-substring, and regex-group().

6 tests filtered — edge cases in regex handling.

## xsl:apply-imports

Done. Compiled via import precedence tracking.

Not yet:
- Some edge cases in import precedence interaction with modes

## xsl:apply-templates

Done. Full mode dispatch, sort support, tunnel parameters.

Not yet:
- Variables in patterns (partial)
- Rooted patterns (in progress on feature branch)
- Some edge cases in pattern matching (28 tests filtered under "match")

## xsl:assert

Not yet implemented.

## xsl:attribute

Done. Basic attribute creation, namespace handling, separator, AVTs.

Not yet:
- type
- validation

## xsl:attribute-set

Done. Collection, validation, use-attribute-sets references.

## xsl:break

Done — part of xsl:iterate support.

## xsl:call-template

Done. Named template invocation with parameters.

## xsl:catch

Done — part of xsl:try/xsl:catch support.

## xsl:character-map

Done. Character maps are compiled and applied during serialization.

22 tests filtered — edge cases.

## xsl:choose

Done.

## xsl:comment

Done.

## xsl:context-item

Not yet fully implemented. The AST parses it but it is not compiled.

14 tests filtered.

## xsl:copy

Done. Basic copy with namespace fixup.

Not yet:
- copy-namespaces attribute (partial)
- inherit-namespaces
- type, validation

37 tests filtered — namespace and type edge cases.

## xsl:copy-of

Done. Deep copy with `clone_with_prefixes` preserving namespace
declarations from ancestors.

Not yet:
- copy-accumulators
- copy-namespaces attribute control
- type, validation

## xsl:decimal-format

Done. Decimal format symbols parsed and used by format-number().

## xsl:document

Done. Creates a new document root node wrapping a sequence constructor.

14 tests filtered — edge cases.

## xsl:element

Done. Element creation with namespace fixup (XSLT 3.0 §5.7.3).

Not yet:
- inherit-namespaces
- use-attribute-sets (on xsl:element specifically)
- type, validation

## xsl:evaluate

Done. Dynamic XPath evaluation with two-level caching (fast path via
namespace context node identity, full key fallback). Supports
xpath-default-namespace, namespace bindings, with-param, schema-aware=no.

## xsl:expose

Not planned — part of the xsl:package system (see xsl:package).

## xsl:fallback

Not yet implemented.

## xsl:for-each

Done. Including xsl:sort support.

## xsl:for-each-group

Done. Supports group-by, group-adjacent, group-starting-with,
group-ending-with.

Not yet:
- composite attribute
- collation attribute

39 tests filtered — composite keys, collation, edge cases.

## xsl:fork

Not yet implemented. Low priority — primarily a streaming/parallelism
instruction.

## xsl:function

Done. User-defined XSLT functions with parameter handling, arity
validation, recursion.

28 tests filtered — edge cases.

## xsl:global-context-item

Not yet implemented. AST parsed but not compiled.

14 tests filtered.

## xsl:if

Done.

## xsl:import

Done. Import resolution with precedence tracking, recursive imports.

## xsl:import-schema

Not planned — requires full XML Schema processing.

## xsl:include

Done. Include resolution with circular reference detection.

## xsl:iterate

Done. xsl:iterate, xsl:break, xsl:next-iteration, xsl:on-completion
all compiled and functional.

19 tests filtered — edge cases.

## xsl:key

Done. Per-document per-key-name index cache with O(1) string lookups.
Composite keys supported. XTSE1222 validation.

19 tests filtered — mostly key() in match patterns (XTSE0340,
architectural pattern compiler limitation) and xml:id in temporary
trees.

## xsl:map

Done. xsl:map and xsl:map-entry compile to map:entry() + map:merge().

32 tests filtered — edge cases and package-scoped tests.

## xsl:map-entry

See xsl:map.

## xsl:matching-substring

Done — part of xsl:analyze-string.

## xsl:merge

Stub. AST parsed and partially compiled, but calls
`xslt-unsupported-merge` at runtime — not functional.

61 tests filtered.

## xsl:merge-action

See xsl:merge (stub).

## xsl:merge-key

See xsl:merge (stub).

## xsl:merge-source

See xsl:merge (stub).

## xsl:message

Done. Supports select, sequence constructor, terminate, error-code.

31 tests filtered — edge cases.

## xsl:mode

Done. Named modes, default mode, on-no-match behaviors (text-only-copy,
deep-copy, shallow-copy, fail). Mode declarations parsed in AST.

64 tests filtered — declared-modes validation (14), mode edge cases.

## xsl:namespace

Done. Namespace node creation.

Not yet:
- Validation that namespace cannot be added after a normal child

43 tests filtered — namespace edge cases.

## xsl:namespace-alias

Done. Applied during element and attribute generation.

## xsl:next-iteration

Done — part of xsl:iterate support.

## xsl:next-match

Done. Compiled with ContinueBehavior::NextMatch.

3 tests filtered — edge cases.

## xsl:non-matching-substring

Done — part of xsl:analyze-string.

## xsl:number

Done. level="single", level="any", level="multiple". Prefix-sum cache
for level="any" (O(1) lookups). Format patterns including alphabetic
and Roman numeral numbering.

202 tests filtered — many are format-pattern edge cases, ordinal
numbering, and language-specific formatting.

## xsl:on-completion

Done — part of xsl:iterate support.

## xsl:on-empty

Done. Conditional output suppression.

## xsl:on-non-empty

Done. Conditional output inclusion.

2 tests filtered.

## xsl:otherwise

Done.

## xsl:output

Done. Output methods: xml, html, xhtml, text, json, adaptive.
Supports encoding, doctype, indent, cdata-section-elements,
omit-xml-declaration, standalone, media-type, byte-order-mark.

124 tests filtered — many are serialization edge cases (HTML5
serialization, encoding variants, suppression of indentation).

## xsl:output-character

Done — part of xsl:character-map support.

## xsl:override

Not planned — part of the xsl:package system (see xsl:package).

## xsl:package

Not planned. xsl:package (and the related xsl:use-package, xsl:override,
xsl:accept, xsl:expose) define a modular packaging system for XSLT 3.0.
However:

- Even Saxon gates xsl:package behind its paid Enterprise Edition (EE) license.
  The free Home Edition (HE) and open-source Community Edition do not support it.
- Real-world adoption is minimal — virtually no stylesheets in the wild use it.
- Implementation cost is enormous: package versioning, component visibility
  (public/private/final/abstract), cross-package linking, and the full
  accept/expose/override machinery.
- The W3C vendor test suite contains ~308 package-related tests across 5 test
  sets (expose, override, package, package-version, use-package), all excluded
  via wildcard filter.

xsl:import and xsl:include (the traditional module system) are supported.

## xsl:param

Done. Local params, global params (including CLI --param support), tunnel
params, as-type coercion via function conversion rules.

## xsl:perform-sort

Not yet implemented.

## xsl:preserve-space

Done — handled alongside xsl:strip-space.

## xsl:processing-instruction

Done.

## xsl:result-document

Done. Secondary result document support.

Not yet:
- validation, type
- Some serialization attributes (allow-duplicate-names, parameter-document,
  suppress-indentation)
- Dynamic @format resolution

25 tests filtered.

## xsl:sequence

Done.

## xsl:sort

Done. Used in xsl:for-each, xsl:apply-templates, xsl:for-each-group,
xsl:perform-sort contexts.

15 tests filtered — edge cases.

## xsl:source-document

Not yet implemented.

## xsl:strip-space

Done. Applied to source documents. strip_space_all flag.

19 tests filtered — element-level strip-space control edge cases.

## xsl:stylesheet

Done. Version, default-mode, default-collation, default-validation,
exclude-result-prefixes, extension-element-prefixes,
xpath-default-namespace, expand-text.

Not yet:
- input-type-annotations
- declared-modes attribute enforcement

## xsl:template

Done. Named templates, match patterns, priority, modes, as-type,
import precedence.

Not yet:
- Variables in patterns (partial)
- Rooted patterns (in progress)
- visibility attribute

## xsl:text

Done.

Not yet:
- disable-output-escaping (deprecated; 1 test filtered)

## xsl:transform

See xsl:stylesheet.

## xsl:try

Done. xsl:try with xsl:catch, error code matching, rollback-output.

1 test filtered.

## xsl:use-package

Not planned — part of the xsl:package system (see xsl:package).

## xsl:value-of

Done.

Not yet:
- disable-output-escaping (deprecated backward compatibility)

## xsl:variable

Done. Local and global variables, as-type, select and sequence
constructor forms, static variables.

## xsl:when

Done.

## xsl:where-populated

Done. Conditional output based on whether sequence constructor
produces non-empty content.

26 tests filtered — edge cases, some may require deeper content
analysis.

## xsl:with-param

Done. Named parameters, tunnel parameters, as-type coercion.

---

## Filtered test categories (not per-element)

These filter categories in the vendor test suite cut across multiple
XSLT elements:

| Category | Filtered | Notes |
|----------|----------|-------|
| unicode-90 | 1460 | Unicode 9.0 character property tests for regex. Requires updated Unicode property tables in regexml. |
| error | 340 | Error code validation tests (XTSE/XTDE/XPDY). Many edge cases in error detection/reporting. |
| number | 202 | xsl:number format patterns: ordinal, language-specific, letter-value. |
| output | 124 | Serialization edge cases: HTML5, encoding, indentation. |
| regex-classes | 120 | Unicode property classes in regex (\p{L}, \p{N}, block names). |
| mode | 64 | Mode declaration enforcement, edge cases. |
| accumulator | 64 | Accumulator edge cases, streaming-dependent tests. |
| merge | 61 | xsl:merge not yet functional (stub). |
| accept | 50 | Package system — not planned. |
| date | 49 | format-date/time locale and timezone edge cases. |
| namespace | 43 | Namespace handling edge cases. |
| for-each-group | 39 | Composite keys, collation in grouping. |
| static | 37 | Static variables, use-when on declarations. |
| copy | 37 | copy-namespaces, type preservation. |
| initial-function | 35 | Stylesheet invocation via initial function — not yet implemented. |
| maps | 32 | xsl:map edge cases, package-scoped tests. |
| base-uri | 32 | Base URI tracking through transformations. |
| message | 31 | xsl:message edge cases. |
| match | 28 | Pattern matching edge cases. |
| function | 28 | xsl:function edge cases. |
| format-date-en | 27 | English locale format-date patterns. |
| where-populated | 26 | xsl:where-populated edge cases. |
| collations | 26 | Unicode Collation Algorithm support. |
| result-document | 25 | xsl:result-document edge cases. |
| forwards | 23 | Forward compatibility mode behavior. |
| character-map | 22 | Character map edge cases. |
| axes | 22 | XPath axis edge cases in XSLT context. |
| math | 20 | Math function edge cases. |
| strip-space | 19 | Element-level strip-space control. |
| key | 19 | key() in patterns, xml:id in temp trees. |
| iterate | 19 | xsl:iterate edge cases. |
| normalize-unicode | 18 | Unicode normalization forms. |
| json-to-xml | 18 | json-to-xml() function edge cases. |
| version | 17 | Version handling/forward compatibility. |
| format-date | 17 | format-date() edge cases. |
| sort | 15 | Sorting edge cases. |
| global-context-item | 14 | xsl:global-context-item not compiled. |
| document | 14 | xsl:document edge cases. |
| declared-modes | 14 | xsl:mode declaration enforcement. |
| context-item | 14 | Context item edge cases. |
| misc (≤13 each) | ~130 | Various: select, catalog, construct-node, expand-text, regex, unparsed-text, type-available, system-property, etc. |
