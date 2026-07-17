# Acceptance criteria

## Required behavior

1. With `use Mojo::Pg`, static completion for `Mojo::Pg->` offers only the
   first-slice `Mojo::Pg` catalog plus existing generic behavior.
2. With `use Mojo::mysql`, static completion for `Mojo::mysql->` offers only
   the first-slice `Mojo::mysql` catalog plus existing generic behavior.
3. Prefixes such as `Mojo::Pg->re` and `Mojo::mysql->st` narrow results to the
   matching adapter methods.
4. Without the corresponding import, adapter-specific methods are not offered
   solely because the receiver text names the adapter.
5. DBI receiver catalogs and generic method completion retain their current
   behavior.

## Proof

- Focused completion tests covering positive imports, both catalogs, prefixes,
  and no-import negative cases.
- Existing semantic framework tests covering imported framework symbols.
- `cargo fmt --all -- --check` or the narrowest successful equivalent.
- Focused `cargo test` for the changed completion/semantic crates.
- Relevant clippy/policy checks and cargo-allow review recorded in the PR.

## Non-claims

- No result-object or chained query inference.
- No promise, callback, pub/sub, or migration-flow inference.
- No dynamic module or runtime-reflection support.

