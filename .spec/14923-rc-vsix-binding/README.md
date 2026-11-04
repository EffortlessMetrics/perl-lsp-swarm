# #14923: explicit RC / VSIX binding, offline contract slice

Status: root-verified implementation packet; offline implementation authorized.
Base: `4991ed9a28956581397de818f0fd57bd0d7e9a58`.

Authorities: [issue #14923](https://github.com/EffortlessMetrics/perl-lsp-swarm/issues/14923),
[accepted mapping decision](https://github.com/EffortlessMetrics/perl-lsp-swarm/issues/14923#issuecomment-5750012264),
[preparation reconciliation](https://github.com/EffortlessMetrics/perl-lsp-swarm/issues/4347#issuecomment-5750016802),
and [bounded claim](https://github.com/EffortlessMetrics/perl-lsp-swarm/issues/14923#issuecomment-5750033626).

## Outcome and authority boundary

An explicitly supplied RC identity and numeric extension identity can be represented,
generated, and checked offline against one prepared source, topology and actual VSIX.
No suffix stripping, shared-prefix inference, version allocation or occupancy decision
is performed. Inputs are synthetic fixtures or values already admitted by the existing
preparation owner. Structural validity is not permission to use an occupied version.

This is a partial implementation of #14923. It does not produce a release, alter any publication workflow, admit a terminal
candidate, or prove installed server/DAP selection. Existing proof workflows install
the pinned release-schema requirements used by the opted-in v4 checks.

The authorized cross-version transition is frozen v3 -> prepared v4. Existing
same-schema v1/2/3 routes remain valid; v1/v2 are not implicitly upgraded because
they lack v3 subject-projection authority. Both v3 and v4 schema files must already
exist with unchanged bytes in both checkouts. Only the selected schema inventory
entry may change. Compare all product membership, native target/member identities,
channels and claim-bearing projection first; separately admit exact mapped identity
fields and deterministic version labels. Never strip arbitrary candidate/RC fields
to manufacture equality. FF01's `0.18.0` series identity is not the full RC identity
and its contract is unchanged here.

## Versioned contracts

Add opt-in `release_topology.v4.schema.json` and support explicit
`--schema-version 4`. Keep schemas 1/2/3 and the current default 1 unchanged.
The v4 shape extends v3; it does not reinterpret a legacy receipt as mapped RC proof.
Older consumers must continue to refuse unknown schema versions unless explicitly
updated in this packet. Do not globally broaden the JSON loader's default versions.

For v4, retain root `release`, `frozen_product_sha`, `prepared_swarm_sha` and the
existing topology source/digest machinery. A mapped RC requires a non-null prepared
subject and the existing checked frozen-to-prepared transition. Root `release` is the
complete `X.Y.Z-rc.N` identity (positive N, canonical decimal components), while
Rust/workspace versions retain their exact release equality rule.

Retain existing `vsix.version`, `asset_name`, `package_path`, `managed_targets` and
`bundled_targets`. Add required `vsix.candidate_id`, `publisher`, `name`, and
`pre_release` (constant true for this RC contract). `vsix.version` is an independently
supplied canonical numeric `X.Y.Z`; never compute it from the RC components.
Publisher/name must equal the existing extension manifest. The asset name must be a
safe basename deterministically including extension name, numeric version and full
RC identity; the accepted formula is `<name>-<numeric-version>-<full-RC>.vsix`.
Changing the filename alone cannot establish the binding.

The generator's explicit `--vsix-mapping <path>` input is the existing payload identity
projection, not a second identity database: exactly `extension: {id, version,
sourceSha}`, `candidate: {id, release, sourceSha}`, and `preRelease: true`. Reject
unknown/missing fields. `extension.id` must equal `<publisher>.<name>` from the package;
both source SHAs equal the selected prepared subject; candidate.release equals root
release. Project these inputs into the v4 fields above and reject disagreement on
validation. Do not record the local mapping-file path as durable identity.

Add `vsix_candidate_payload.v2` by retaining all v1 fields and their existing server,
DAP, package-inventory and topology bindings, plus required `preRelease: true`.
Its existing `extension` and `candidate` fields carry the identities; do not introduce
parallel fields for the same facts. v1 remains readable under its existing rules but
cannot satisfy the v4 mapped-RC verification route. v2 package, source, candidate,
release and topology digest must equal the selected v4 projection exactly.

## Digest direction and actual bytes

Preparation input -> topology bytes/digest -> embedded candidate payload -> VSIX
bytes/digest. Neither topology nor embedded payload contains the final VSIX digest.
This avoids both a topology/artifact cycle and a self-hash inside the ZIP.

The offline verifier consumes the selected topology, VSIX path and explicit expected
VSIX SHA-256 from its caller. It recomputes the digest and checks actual package JSON,
VSIX identity metadata and the VSIX prerelease property, as well as the embedded v2
payload and existing selected native-member digests. Missing, duplicate, contradictory
or malformed identity/prerelease metadata refuses. A synthetic expected hash is only
fixture proof; its authority in a real transaction remains the later terminal owner.
Use the existing verifier result/error path, not a new release-status ledger.

## Implementation seams

- `scripts/generate_release_topology.py`: explicit v4/input admission; generation and
  validation; source-path/schema digest inclusion; frozen/prepared comparison.
  Existing v1/2/3 VSIX equality checks stay intact. Normalization may erase only the
  authorized preparation fields; a changed candidate or RC mapping must not disappear
  merely because both values look like versions.
- `scripts/release_topology_json.py`: preserve exact top-level schema admission;
  opt in at selected callers only, with old-version controls.
- `schemas/release_topology.v4.schema.json` and
  `schemas/vsix_candidate_payload.v2.schema.json`: closed required shapes.
- `vscode-extension/src/vsixPackageProjection.ts`: reuse current extension/candidate
  identities and package/server/DAP projections; add explicit v2 selection.
- `scripts/prepare_vsix_prebuilt_payload.py` and
  `vscode-extension/scripts/build_vsix_candidate_manifest.js`: forward only validated
  mapping inputs through the existing payload producer; do not duplicate packaging.
- `vscode-extension/scripts/check-vsix-prebuilt-payload.js`: verify actual selected
  archive metadata/bytes and mapping through the existing ZIP/member checks.
  The actual XML Identity.TargetPlatform must equal the target-specific payload's
  derived VS Code target; universal-managed payloads require that attribute absent.
- `scripts/release_build_identity.py`: inspect affected topology admission and update
  only the explicit offline v4 consumer path required by payload preparation.
- Existing Python topology/JSON/payload tests, TS projection tests and JS verifier
  tests own the paired controls; nearby synthetic fixtures carry no live identities.
- Exact non-Rust policy entries/inventory follow repository rules at implementation.

Before editing a newly implicated consumer, trace its actual schema gate and return
to root if enabling it would change a workflow, terminal admission or installed path.

## Acceptance and paired falsifiers

1. A valid synthetic mapped RC generates deterministically and validates with numeric
   package metadata, explicit prerelease metadata and the selected payload/native bytes.
2. Another RC iteration sharing the same numeric prefix fails the exact join.
3. Another numeric extension version, publisher/name or candidate ID fails independently.
4. Absent/false/wrong/duplicate prerelease metadata fails; the valid marker passes.
5. Wrong prepared source or topology digest fails independently; unchanged subjects pass.
6. Changed VSIX bytes fail the previously supplied digest, even with unchanged filenames.
7. v1 payload/legacy topology cannot pass the mapped route; legacy tests remain unchanged.
8. Deterministic preparation rejects mapping changes outside the accepted transition;
   it cannot normalize away changed RC identity or product changes.
9. Registry deferral is not an input permitting VSIX omission. This proves only the
   offline selected-artifact join; mandatory terminal/GitHub asset completeness remains
   the composition slice. Preserve any existing legitimate non-extension contract.

Proof uses the existing focused Python unittest suites (`test_generate_release_topology`,
`test_release_topology_json`, `test_prepare_vsix_prebuilt_payload`) and existing TS/Node
projection and ZIP-verifier test commands discovered from package.json. Add direct
schema accept/reject checks, deterministic output comparison, formatting and diff checks.
No Cargo, registry credentials, live dispatch or package publication is needed.

## Remaining work and rollback

Full #14923 still requires artifact-only build workflow composition, the versioned
terminal VSIX role and mandatory asset denominator, checksum/attestation integration,
same-artifact publisher consumption, and installed Windows/Linux server/DAP proof.
#14406/#14479 retain graph authority; #4347/#6068/#6052 retain preparation identity;
#6056/#4346/#14412 retain installed consumers. This packet does not close those claims.
In particular, `vscode-extension/scripts/package-vsix.js` still rejects payload v2;
production packaging admission and prerelease invocation belong to that composition
slice. Offline synthetic ZIP proof here is not proof that this publisher/package
entry point can already produce a mapped RC. The new mapping helper and v4/v2 schemas
are private contract machinery, not a second preparation or release authority.

Rollback removes the opt-in contracts and callers together before any live consumer
adopts them. Preserve all legacy schemas/defaults and do not mutate previously admitted
or invalidated release transactions. No actual version/tag/channel is allocated here.


## Review repair: raw identity and verifier snapshot

Mapped v4 topology admission rejects duplicate JSON properties at every depth;
legacy loader defaults remain 1/2 and keep their previous admission semantics.
The Python adapter parses and hashes the same captured raw topology bytes.
Mapping-file malformed UTF-8 and duplicate keys produce structured NOT_PROVEN.
The VSIX verifier uses the packager's existing pinned jsonc-parser visitor after
strict JSON syntax validation to reject duplicate topology/package/payload fields.
No new dependency is introduced.

A mapped VSIX is captured once into a private buffer. Its expected SHA256,
metadata, native members and semantic inventory all use that buffer, with no
path reopening between checks. This establishes one checked byte identity; it
does not promise that the source path remains unchanged afterward. Both Python
and Node offline schema checks bind the actual schema bytes to the topology's
recorded schema path/digest. This remains structural offline binding, not full
canonical topology generation or installed qualification.

V4 inventories include scripts/release_vsix_mapping.py. The v3-to-v4 transition
requires that helper to be committed and byte-identical in both checkouts, checks
the prepared inventory hash, then permits only that explicit inventory addition.
V1/v2/v3 inventories and default selection are unchanged. The existing two CI
jobs install scripts/requirements-release.txt before their newly affected proof;
Python subprocess tests use the current interpreter rather than ambient python.
