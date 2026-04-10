# Deferred Follow-Up: use-when DTD and External Entity Support

Date: 2026-04-10

## Scope

This note tracks the deferred `use-when-0136` conformance case in the XSLT `use-when` bucket.

Relevant vendor files:

- `vendor/xslt-tests/tests/attr/use-when/use-when-0136.xsl`
- `vendor/xslt-tests/tests/attr/use-when/dir/use-when-0136.ent`

## Current Failure

The current failure happens before XSLT static evaluation logic runs:

- current result: `Unsupported("Failed parsing XSLT: Unsupported(\"Parse error: DTD is not supported\")")`

This is not a `use-when` semantic bug in the existing AST/static-evaluation layer. The stylesheet contains:

- a DTD declaration
- an external entity
- entity-expanded content that participates in `use-when`

The current XML parser path rejects DTDs, so the stylesheet never reaches normal XSLT processing.

## Why This Matters

The test is checking more than entity expansion. It also checks that the static base URI is correct for content originating from the external entity. That means any future fix needs to preserve:

- entity expansion
- correct base URI on the expanded content
- compatibility with existing static `use-when` evaluation

## Likely Implementation Paths

1. Add DTD and external-entity support to the stylesheet XML parser path used by XSLT parsing.
2. Introduce a dedicated stylesheet-loading path that performs entity expansion before handing the document to the existing AST/static-evaluation pipeline.

## Acceptance Criteria

- `use-when-0136` passes in the vendor suite.
- The external entity content is visible to static evaluation.
- `static-base-uri()` inside that entity-expanded content resolves to the entity location as required by the test.
- Existing non-DTD `use-when` cases continue to pass unchanged.

## Recommendation

Treat this as a parser-capability tranche, not as another incremental `use-when` expression fix. It should be addressed separately from the remaining non-DTD `use-when` tail.