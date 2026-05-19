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

## Performance Optimization Discipline

**Never jump from profiling data straight to implementation.** Caching and
algorithmic changes are expensive to build and review. Validate the hypothesis
first.

### Required steps before building any optimization

1. **Use a real profiler.** Use `xctrace record --template 'Time Profiler'`
   (Instruments) or `samply` with the Firefox Profiler UI. Do NOT use macOS
   `sample` — it only gives time percentages, not call counts or call-graph
   context, which leads to wrong conclusions.

2. **Verify the hypothesis cheaply first.** Before building anything,
   confirm the suspected hot path actually matters: bypass it (comment it
   out, short-circuit with a dummy return, or stub it) and re-run the
   benchmark. If skipping the code path doesn't improve performance,
   optimizing it won't either — move on. This takes minutes, not hours.

3. **Measure call frequency before caching.** A function at 6% self-time
   could be 3 expensive calls (cache won't help) or 10,000 cheap calls
   (cache will help enormously). Add a temporary counter (`eprintln!` or
   `AtomicUsize`) and run the benchmark once to find out. Remove the counter
   before committing.

4. **Distinguish "few expensive calls" from "many repeated calls."**
   Caching only helps the latter. For "few expensive calls," look for
   algorithmic improvements within the function itself.

5. **State the hypothesis explicitly** before writing code. Example:
   "key() is called N times with the same (doc, name) pair; caching the
   index will eliminate N−1 full document walks." If you can't state the
   hypothesis with concrete numbers, go back to step 2.

6. **Benchmark immediately after implementing.** If the optimization shows
   no measurable improvement on the target workload, consider reverting
   rather than keeping dead complexity.

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

Update `docs/xslt-progress.md` with an entry when making meaningful changes.
Use `date '+%Y-%m-%d %H:%M %Z'` for timestamps — never guess.
