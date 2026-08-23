# perl-kwalitee — legacy compatibility

> **Frozen historical meaning.** This crate currently contains the repository's
> original `perl_kwalitee.v1` evaluator. Despite the name, that evaluator is a
> weighted mixture of native-product posture, engineering evidence, release
> integrity, release governance, and documentation checks. It is **not** the
> native Rust CPANTS-compatible Perl distribution analyser being built under
> #4745.

The historical evaluator remains temporarily available so existing automation
can migrate without losing receipt compatibility. Its indicator catalog and
wire contract are closed to new work.

## Frozen contract

- receipt kind: `perl_kwalitee`;
- schema version: `1`;
- historical domain: `mixed_repository_product_release_readiness`;
- status: compatibility-read-only;
- replacement: independent evidence rails and scoreless candidate fan-in.

Every current indicator has exactly one destination in
[`legacy_indicator_migrations.toml`](legacy_indicator_migrations.toml). The
generated human view is
[`docs/reference/PERL_KWALITEE_MIGRATION.md`](../../docs/reference/PERL_KWALITEE_MIGRATION.md).

## Existing compatibility API

```rust
use perl_kwalitee::{evaluate, KwaliteeOptions, KwaliteeProfile};

let options = KwaliteeOptions::new("/path/to/repo", KwaliteeProfile::Pr);
let receipt = evaluate(&options);
println!("{}", receipt.to_markdown());
```

The explicit compatibility surfaces are:

- `legacy_migration_ledger()` — validated one-to-one disposition of every
  historical indicator;
- `legacy_indicator_records()` — catalog metadata joined to those dispositions;
- `read_legacy_receipt()` — fail-closed reader for kind `perl_kwalitee`, schema
  `1`;
- `render_legacy_migration_markdown()` — deterministic generated reference.

## Migration sequence

1. Freeze this contract and migration ledger (#7164).
2. Move the mixed implementation to `perl-release-readiness` without changing
   its observed results (#7166).
3. Replace the weighted readiness decision with independent, candidate-bound
   evidence rails (#7168, #7169, #7191).
4. Reclaim `perl-kwalitee` for the native Rust distribution analyser and remove
   the ambiguous compatibility alias once active callers reach zero (#7185,
   #7192).

New CPANTS metrics, staged-distribution facts, or release-readiness propositions
must not be added to this frozen catalog.

## License

MIT OR Apache-2.0 (workspace-inherited).
