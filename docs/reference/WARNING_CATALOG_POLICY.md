# Supplied warning catalogs and policies

`perl_pragma::compile_environment::warnings` owns immutable builtin catalogs and
queries over explicitly supplied warning policies. This advances #8655; it does
not close support for every version in the repository's Perl matrix.

## Supported authority

Complete live catalogs are available only for exact Perl 5.36.3 (80 categories),
5.38.5 (80), 5.42.3 (80), and 5.44.0 (81). Every other patch or minor profile is
unavailable, without nearest-version fallback. Accepted retired no-op spellings
are separately represented (0, 8, 11, and 13 respectively); they are not live
categories, aliases, or unknown registrations.

The fixture records exact upstream tag URLs and raw SHA-256 hashes for
`regen/warnings.pl`, generated `lib/warnings.pm`, `lib/feature.pm`, and
`pod/perlsub.pod`. A bounded literal parser reads the explicit warning tree; it
never executes downloaded regeneration code. The tree is checked against the
complete generated bitmask closure. Native offsets are oracle metadata, not
category identifiers. Hierarchy is explicit: `pipe` belongs to `io` despite
having no `io::` prefix.

Documented feature associations and signature-syntax relationships carry source
locators and qualifiers. They do not infer feature admission or warning emission.
Historical signatures associations remain historical even when a warning name
is still registered. Empty relationship lists mean no association established
from these selected sources, not proof that none exists.

## Query and identity contract

A policy carries both its exact catalog identity and the full accepted consumer
Binding. The producer selects the catalog appropriate for that language profile;
opaque profile labels/digests are not parsed as Perl versions or equated with
catalog digests. Queries compare the expected Binding and catalog. The caller
selects the policy; this API performs no lexical-position lookup or timeline
resolution.

Enabled, disabled, and fatal are distinct. Later applicable effective ordinals
win; duplicate ordinals are rejected. Matching uses explicit ancestor closure.
`all_warnings_disposition` queries the root category's own bit, not uniformity of
all descendants. Catalog defaults can therefore disable the root while enabling
`glob` or `deprecated`.

Unknown registrations remain qualified unknowns. An exact override for an unknown
name is refused; qualified unknown effects conservatively prevent exact known
answers. Earlier applicable nonexact evidence remains visible even when a later
exact override supplies the winning value. Explanations retain contributing
origins, ordinals, and boundary references separately from the winning origin.
Answers are immutable; exact answers require a known category and nonempty exact
evidence. Accepted no-ops do not invent an enabled/fatal disposition.

Policy identity includes typed values, qualifications, effective order, origins,
Binding, catalog, and boundary references, but excludes freeform Facet reasons.
Those reasons remain on the wire. Producer identifiers are canonical inputs;
this module does not authenticate them or sanitize arbitrary path-bearing IDs.

Catalog-bound category references validate catalog digest, exact profile and
membership. WarningPolicy ports reuse the #8520 role and subject contract.
The #8561 category selector remains pending: these references do not promote
opaque selector strings into exact authority. Qualified selector/application
integration remains #8673 work.

## Bounds and exclusions

Operational limits are 256 categories, ancestor depth 16, 256 aliases, 1024
overrides and 256 distinct unknown names. Shared admission limits apply to
names, provenance and boundary references. Snapshot JSON is bounded to 1 MiB
and depth 32. Limit refusal returns no admitted policy; it never truncates into
an exact value. These are operational limits, not Perl language limits.

There is no source directive interpreter, source-order application, runtime
warning registration, provider or diagnostic migration, legacy cutoff, or
feature registry here. Catalog presence does not establish runtime emission.

## Reproduction

Explicit network fetch validates pinned hashes and bounded input sizes. Once
cached, omit `--fetch` for a fully offline generated-file check:

```sh
python scripts/ci/check-warning-catalog.py --sources target/warning-catalog-sources --output crates/perl-pragma/tests/fixtures/warning_catalogs.json --fetch --check
python scripts/ci/check-warning-catalog.py --sources target/warning-catalog-sources --output crates/perl-pragma/tests/fixtures/warning_catalogs.json --check
cargo test --locked -p perl-pragma --lib --test warning_policy
```

Optional `--oracle-root <directory>` expects exact runtimes at
`perl-<version>/runtime/bin/perl.exe`; `--receipt <path>` records interpreter and
module identities. The oracle snapshots warnings registries before loading
JSON::PP, whose dependencies otherwise contaminate the builtin denominator.
`--contaminate-oracle` is an explicit negative instrument requiring oracle-root;
it must fail the clean builtin comparison. This network/process work is manual
proof tooling, never request-time product behavior.