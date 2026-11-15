# perl-tidy-cli

## Owns

The `perl-tidy` executable's argument parsing, bounded input, explicit native
configuration projection, source/JSON channels, check/exit semantics, and local
read-only source snapshots. `perl-tidy` owns the native formatter, configuration
validation, typed outcomes, and exact source/edit-plan validation.

## Boundaries

- No LSP, editor, workspace runtime, Perl, Perl::Tidy, or subprocess dependency.
- No private formatter, style registry, or implicit profile/default authority.
- Every formatted output consumes `TypedFormatResult::apply_to` and verifies the
  authoritative rendered bytes before crossing the I/O boundary.
- A native `Refused`/`FailedOrNotProven` cannot become a successful check.
- Source files are opened for reading only. `--write` is unsupported; replacement
  and metadata preservation remain separately owned by #9312.
- Explicit native configuration and explanation do not establish the production
  profile or per-root effective configuration contract in #9308.

## Focused proof

`test -p perl-tidy-cli --locked` through `scripts/cargo-admitted` exercises the
real binary, rejected write requests, read-only snapshot currentness, source/JSON
separation, and native execution with working external-tool PATH traps.
