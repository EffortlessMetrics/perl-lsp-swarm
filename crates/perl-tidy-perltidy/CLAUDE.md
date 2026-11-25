# CLAUDE.md (perl-tidy-perltidy)

## Role and ownership

Explicit external Perl::Tidy configuration and one source-in/text-out executor.
`src/config.rs` owns the external options and argument projection. `src/lib.rs`
accepts an explicit runtime, executable and resolved arguments, and maps runtime,
exit and UTF-8 failures. It contains no native formatter or fallback selection.

## Boundaries

The existing legacy runtime is an execution seam, not a production process
supervisor. Do not claim live cancellation, document currentness or supervised
process cleanup from this adapter. Caller-owned argument precedence remains in
the caller: the old facade uses `-st`, while the LSP legacy path also uses `-se`.
Do not add discovery or implicit native/external fallback. File transactions and
legacy file-in-place helpers are outside this text API.

## Focused proof

Use the repository's admitted Cargo route for `test -p perl-tidy-perltidy
--lib --tests`. Contract tests use supplied runtimes and distinguish program/args,
input/output bytes and each error class. The normal dependency graph must exclude
`perl-tidy`, parser and LSP packages.
