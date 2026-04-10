# Project Handover: 2026-04-10

## Scope of this handover

This handover is for the current uncommitted `xsl:try` tranche in the `xee`
workspace. The chat got long, so this document is intended to let a fresh chat
resume without having to reconstruct the state from history.

## Repo state

- Repository: `/Users/brillo/Repositories/xee`
- Branch: `feature/xslt-match-rooted-patterns`
- HEAD: `531fd35f Checkpoint simplified stylesheet fixes`
- Current state: dirty working tree with uncommitted `xsl:try`-related changes

Modified files:

- `xee-interpreter/src/error.rs`
- `xee-interpreter/src/interpreter/interpret.rs`
- `xee-interpreter/src/interpreter/program.rs`
- `xee-interpreter/src/interpreter/runnable.rs`
- `xee-interpreter/src/interpreter/state.rs`
- `xee-interpreter/src/library/context.rs`
- `xee-interpreter/src/library/external.rs`
- `xee-interpreter/src/library/hidden_xslt.rs`
- `xee-testrunner/src/dependency.rs`
- `xee-xpath-compiler/src/ast_ir.rs`
- `xee-xslt-ast/src/ast_core.rs`
- `xee-xslt-ast/src/element.rs`
- `xee-xslt-ast/src/instruction.rs`
- `xee-xslt-ast/src/names.rs`
- `xee-xslt-compiler/src/ast_ir.rs`
- `xee-xslt-compiler/tests/test_xslt.rs`

## What was completed in this chat

The supported vendor `xsl:try` bucket was driven to green.

Implemented or fixed in the dirty tree:

- parsing and lowering support for `xsl:try` and `xsl:catch`
- catch error-pattern normalization and QName matching
- `err:*` catch variable plumbing, including module/line/column reporting
- forward-compatibility behavior around `element-available()` and `xsl:fallback`
- non-catchability of global-variable evaluation errors
- named-template context inheritance needed by `current()`-adjacent cases
- `xsl:result-document validation="strip"` acceptance for the supported slice
- duplicate secondary result-document URI mapping to `XTDE1490`
- instruction-span preservation for `xsl:result-document` helper calls
- XSLT-style dependency parsing in the testrunner so unsupported schema-aware
  cases are classified correctly
- minimal `xsl:source-document` lowering sufficient for the supported `try`
  cases
- rollback-output handling sufficient to satisfy the remaining supported
  `try-033` / `try-034` behavior

## Last verified results from this chat

Focused compiler regression suite:

```text
cargo test -p xee-xslt-compiler --test test_xslt test_try_ -- --nocapture
running 14 tests
test result: ok. 14 passed; 0 failed
```

Vendor `try` bucket:

```text
cargo run -p xee-testrunner -- -v all vendor/xslt-tests/tests/insn/try/_try-test-set.xml
Total: 42 Supported: 34 Passed: 34 Failed: 0 Error: 0 WrongE: 0 Unsupported: 8
```

Dependency loader unit tests were also green during the tranche:

```text
cargo test -p xee-testrunner dependency::tests:: -- --nocapture
2 passed; 0 failed
```

## Important context for the next chat

- The existing `xslt-progress.md` currently reflects older checkpoints and does
  not describe this `xsl:try` tranche yet.
- No checkpoint commit was created for this work yet.
- The user explicitly wants generic, spec-oriented fixes, not DocBook-specific
  hacks.
- For XSLT work in this repo, the user prefers:
  - focused tests while iterating
  - a filtered vendor regression sweep before checkpoint-quality changes
  - updating `xslt-progress.md` with local time when making a checkpoint entry

## Caution / open question

There is one thing worth re-checking before committing: the dirty tree still
contains a direct `fn:current()` special-case in
`xee-xpath-compiler/src/ast_ir.rs`, while the intended semantic fix for this
chat was primarily the XSLT-layer expression-entry capture in
`xee-xslt-compiler/src/ast_ir.rs`. A fresh chat should verify whether the XPath
compiler change is still desirable or whether it should be removed before the
checkpoint commit.

## Recommended next steps

1. Re-run the three key validations to confirm the dirty tree is still green:
   - `cargo test -p xee-xslt-compiler --test test_xslt test_try_ -- --nocapture`
   - `cargo test -p xee-testrunner dependency::tests:: -- --nocapture`
   - `cargo run -p xee-testrunner -- -v all vendor/xslt-tests/tests/insn/try/_try-test-set.xml`
2. Review the `fn:current()` change in `xee-xpath-compiler/src/ast_ir.rs` and
   decide whether it is redundant or semantically wrong given the XSLT-layer
   rewrite.
3. Update `xslt-progress.md` with a new dated entry including local time for
   this `xsl:try` checkpoint.
4. Create the checkpoint commit once the validations and the `current()` review
   are complete.

## Suggested restart prompt for the next chat

```text
Please read PROJECT_HANDOVER_2026-04-10.md and continue from there.
We are in /Users/brillo/Repositories/xee on branch feature/xslt-match-rooted-patterns.
The dirty worktree is the xsl:try tranche. First verify the current green state,
then inspect whether xee-xpath-compiler/src/ast_ir.rs still needs its fn:current()
special case before updating xslt-progress.md and making a checkpoint commit.
```