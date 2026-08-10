# CLAUDE.md (perl-test-facts)

## Role

Pure TAP-text parser producing stable, execution-independent result facts.
Takes TAP output as a `&str` and returns a structured report. Zero
dependencies, `publish = false`.

## Owns

- `parse_tap(source: &str) -> TapReport` -- the entire public entry point.
- `TapReport` -- `version`, `plan`, `assertions`, `bail_out`, `diagnostics`,
  `raw_lines`, plus the counting helpers `count`, `passed_count`,
  `failed_count`, `skipped_count`, `todo_count`, `unknown_count`, and
  `is_success`.
- `TapAssertion` -- per-assertion facts: line, nesting `depth`, `number`,
  `status`, raw `outcome`, `name`, `directive`, YAML and non-YAML diagnostic
  lines, and the parsed `source_file` / `source_line` / `got` / `expected`
  values when a runner emitted them.
- `TapPlan`, `TapAssertionStatus` (`Pass`/`Fail`/`Skip`/`Todo`/`Unknown`), and
  `TapAssertionOutcome` (the raw `ok` / `not ok` reading, kept independent of
  directive classification).

## Does not own

- Test execution, subprocess management, and source-file discovery -- these
  belong to runtime adapters and workspace consumers. The crate-level docs say
  this explicitly and the parser holds to it.
- Reading or inspecting any file. `source_file` and `source_line` are
  *retained* when a runner emits an `at FILE line N.` diagnostic; retaining
  them does not open or validate the path.
- Interpreting YAML. Diagnostic blocks are kept as raw lines.
- Deciding structural validity. `is_success` reports the absence of hard
  assertion failures and bailout only -- plan mismatches and structural
  problems surface separately in `diagnostics`, and a plan-less or empty
  report can still be a hard success.

## Neighbors

- Upstream: none -- zero-dependency leaf crate.
- Downstream: none in-workspace yet. The crate entered the workspace in
  `ca4c987` (#5351) and currently has no consumers, so its API has not been
  exercised by a real caller.

## Read first

`src/lib.rs` -- the whole crate, including its 25 inline tests.

## Focused validation

`cargo test -p perl-test-facts`.

## Review hotspots

- **Directive vs outcome separation.** `status` is the classified result
  while `outcome` is the raw `ok` / `not ok` reading. A TODO that fails is
  `outcome: Fail` but `status: Todo`; collapsing the two would silently change
  what counts as a failure.
- **`is_success` boundary.** It deliberately ignores plan mismatches. Any
  caller treating it as "the run was valid" rather than "nothing hard-failed"
  is misusing it -- the doc comment on the method spells out the required
  additional checks.
- **Unknown retention.** Unsupported or malformed constructs are retained as
  `Unknown` / `diagnostics` / `raw_lines` rather than dropped. Changing that to
  silently discard input would make the facts less honest, not cleaner.

## Claim boundary

Describes the parsing API as authored and as exercised by the crate's own
inline tests. Does not assert conformance to any particular TAP specification
version, and does not claim the API is stable under real consumer pressure --
there are no downstream consumers yet.
