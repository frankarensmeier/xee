# Xee Project Guidelines

## Architecture

Rust workspace with ~16 crates implementing an XPath 3.1 / XSLT 3.0 processor.
Key crates: `xee-interpreter` (runtime), `xee-xslt-compiler`, `xee-xpath-compiler`,
`xee-xpath-ast`, `xee-xslt-ast`, `xee-ir`, `xee-xpath` (public API), `xee-testrunner`.

See `hacking.md` for architecture overview and development guide.

## Build and Test

```sh
# Quick focused test (crates most often touched)
cargo test -q -p xee-interpreter -p xee-ir -p xee-xslt-compiler -p xee-testrunner

# Full test suite
cargo test

# XSLT conformance check (must show 0 failed, 0 error)
cargo run --release -p xee-testrunner -- check vendor/xslt-tests/

# XPath conformance check
cargo run --release -p xee-testrunner -- check vendor/xpath-tests/

# Refresh XSLT test filters after improvements
python3 update.py
```

## Performance Benchmark

Run `./bench-xslt` before committing changes to key(), pattern matching,
template dispatch, or interpreter core. It transforms `xslt/input-small.xml`
(~3.5s) and compares against a stored baseline.

```sh
./bench-xslt              # compare to baseline, fail if >20% regression
./bench-xslt --save       # save current timing as new baseline
./bench-xslt --help       # full usage
```

## Code Style

- Prefer `bail!`/`anyhow` over `.unwrap()` — panics are bugs.
- Use the `#[xpath_fn(...)]` macro for XPath function implementations.
- Follow existing patterns in whatever module you're editing.

## Test Filter Rules

**NEVER add a failing test to `vendor/xslt-tests/filters`.** It masks real problems.

- If a test fails, investigate and fix the root cause.
- If filtering seems warranted (e.g., test is outside current scope), discuss
  with the maintainer first — never filter unilaterally.
- Always use `python3 update.py` to refresh filters (not raw testrunner update).
- Before committing filter changes, verify no new filters were added:
  `git diff vendor/xslt-tests/filters | grep '^+[a-z]'` must return zero lines.

## Pre-Commit Review

Before every commit, review the staged diff (`git diff --cached`) for:

1. Logic errors and off-by-one mistakes
2. Unwrap/panic risks
3. Regressions — does the change match its stated intent?
4. Missing test coverage for new code paths
5. Consistency with surrounding code style
6. Security issues (OWASP Top 10)

## Progress Tracking

Update `xslt-progress.md` with an entry when making meaningful changes.
Use `date '+%Y-%m-%d %H:%M %Z'` for timestamps — never guess.
