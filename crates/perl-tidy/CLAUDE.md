# CLAUDE.md (perl-tidy)

## Role and ownership

The sole native formatter owner: pure native style policy, parse gates, layout,
literal preservation, typed outcomes, work counters and source-bound byte edit plans.
`src/native/implementation.rs` owns rendering; `outcome.rs` owns classification;
`bytes.rs` owns checked requests and plan application. Do not add another renderer
or application-specific process, workspace, filesystem or protocol policy here.

## Boundaries

- Dependencies on parser core and canonical source identity are deliberate.
- Engine selection, URI provenance, UTF-16 conversion and live document currentness
  belong to callers. Native configuration must not acquire mode/executable fields.
- External Perl::Tidy execution belongs to `perl-tidy-perltidy`. Legacy DTOs and
  compatibility translation belong to `perl-lsp-perltidy`.
- Deserialization transports data; revalidate source, target, edits and terminal
  evidence before execution or application. Refusal must preserve the input.

## Focused proof

Use the repository's admitted Cargo route for `test -p perl-tidy --lib --tests`.
`tests/byte_plan_tests.rs` falsifies source identity, range, edit and terminal
claims; layout and parse-gate tests protect admitted rendered bytes. Package tests
must work from an archive without repository-only fixture or workflow paths.
Benchmark counters are deterministic evidence; timing alone proves no complexity
or allocation bound. Repository benchmark wiring is tested in `xtask`.
