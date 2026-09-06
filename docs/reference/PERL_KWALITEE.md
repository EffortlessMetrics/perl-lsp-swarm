# Historical Perl Kwalitee Compatibility Surface

`perl-release-readiness` owns the historical `perl_kwalitee.v1`
repository/product/release-readiness evaluator. `cargo xtask perl-kwalitee`
is the compatibility command wrapper. They are not the authority for
CPANTS-compatible Perl distribution Kwalitee and must not gain new indicators.

The frozen 17-row catalog, the pinned v1 receipt reader, and the disposition of
every historical proposition are documented in
[PERL_KWALITEE_MIGRATION.md](PERL_KWALITEE_MIGRATION.md). New work belongs in
the independent native-product, engineering-evidence, release-integrity,
release-governance, and installed-acceptance rails named there.

The native CPANTS-compatible catalog v1 and fixture-identity contract are
documented in [DISTRIBUTION_KWALITEE_CATALOG.md](DISTRIBUTION_KWALITEE_CATALOG.md).
That catalog is independent of this historical evaluator.

## Compatibility commands

These commands remain available only to reproduce or consume historical
evidence during migration:

```bash
cargo xtask perl-kwalitee check --profile pr
cargo xtask perl-kwalitee report --profile release --dist dist \
  --json target/receipts/kwalitee/perl-kwalitee.json \
  --markdown target/receipts/kwalitee/perl-kwalitee.md
cargo xtask perl-kwalitee explain release.no_external_tooling
```

The crate remains `publish = false`. Do not publish it as a distribution
Kwalitee analyzer or treat its weighted score as a release decision. Domain
validators and candidate-bound receipts remain authoritative.
