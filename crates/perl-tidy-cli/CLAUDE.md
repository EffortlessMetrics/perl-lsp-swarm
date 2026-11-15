# perl-tidy-cli

## Owns

The `perl-tidy` executable's argument parsing, bounded input, explicit native
configuration projection, source/JSON channels, check/exit semantics, and local
filesystem write policy. `perl-tidy` owns the native formatter, configuration
validation, typed outcomes, and exact source/edit-plan validation.

## Boundaries

- No LSP, editor, workspace runtime, Perl, Perl::Tidy, or subprocess dependency.
- No private formatter, style registry, or implicit profile/default authority.
- Every output/write consumes `TypedFormatResult::apply_to` and verifies the
  authoritative rendered bytes before crossing the I/O boundary.
- A native `Refused`/`FailedOrNotProven` cannot become a successful check.
- Plan-first writes and individual atomic replacements do not establish a
  crash-atomic multi-file transaction. Preserve recovery evidence on rollback
  conflicts; do not overwrite a detected later writer.

## Focused proof

`test -p perl-tidy-cli --locked` through `scripts/cargo-admitted` exercises the
real binary and deterministic injected filesystem failures. Cross-platform
read-only behavior and Linux-only write admission have separate claims.
