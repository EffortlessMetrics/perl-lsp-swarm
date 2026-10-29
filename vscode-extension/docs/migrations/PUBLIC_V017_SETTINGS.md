# Public v0.17 settings inventory and conformance boundary (#7837)

The authentic public artifact is `EffortlessMetrics.perl-lsp-rs` 0.17.0, from the
[official GitHub release](https://github.com/EffortlessMetrics/perl-lsp/releases/tag/v0.17.0).
The downloaded VSIX has 7,325,907 bytes and SHA256
`b67d84a149efd99ae26310742bc3773c6a4644258391796489b7a9476d2e47d4`, matching
the official release asset digest. Its exact `extension/package.json` bytes are
checked in under `test/fixtures/configuration-migration/`, SHA256
`3186e23ef455e4111df92711b9b926cfbe68a949db9bc84f536ea8b2908c0e6a`.
The fixture was extracted from the downloaded VSIX, not reconstructed from git.
The `.json.fixture` suffix preserves the published bytes as an opaque fixture,
without changing the formatter configuration shipped in the VSIX.
Marketplace package retrieval returned HTTP 403 at the environment proxy;
Marketplace byte equality is not established.

## Observed source inventory

Compared with `vscode-extension/package.json` at source base
`1c8dd49399921e75643d2309cfb3f58f270ba0a5`:

| Inventory                | Count / subject                                                                                                         |
| ------------------------ | ----------------------------------------------------------------------------------------------------------------------- |
| Published definitions    | 25                                                                                                                      |
| Candidate definitions    | 35                                                                                                                      |
| Added                    | 11; no fabricated history required                                                                                      |
| Removed                  | 1: `perl-lsp.autoPopulateNewFiles`                                                                                      |
| Changed                  | 13; retained defaults are unchanged                                                                                     |
| Resource → machine scope | 5: `aiCompletion.enabled`, `aiCompletion.streaming.enabled`, `perlcritic.profile`, `perlcritic.theme`, `perltidyConfig` |

Cross-scope migration reading for all five scope changes depends on #14968's
legacy-reader/trust ownership. The other changes affect nested feature
enums, descriptions, deprecation messages, or feature-profile enum descriptions.
The existing MCP `removed_inert` row is valid even though its deprecated setting
remains contributed: contribution does not establish execution authority.

Concrete defect: the MCP-only registry did not detect the real published removal
of `autoPopulateNewFiles` or the twelve other changed definitions. The new normal
Jest guard returns thirteen missing dispositions when run without exclusions.

The current source manifest still declares **0.17.0**. This comparison is an exact
source candidate inventory; it is not an installed or released 0.18.0 subject.

## Guard and unresolved exclusions

`configurationManifestConformance.ts` consumes the canonical registry validator and
era selector. It compares every per-setting definition field recursively, including
text and presentation order, conservatively; group titles/order are not setting
definitions. Arrays retain order. Omitted manifest scope means VS Code `window`;
runtime `MigrationScope` is unchanged. Valid nonapplicable historical eras are skipped.

The published `featureProfile` and `disabledFeatures` settings explicitly use
`window`, which the current runtime registry vocabulary cannot represent. Their
exact #7837 exclusions remain required. A row declaring `workspace`, `resource`,
or another existing scope is rejected rather than mapped to `window`. Retiring
those exclusions requires a separately governed registry representation and live
configuration-reader evidence; cross-scope and trust policy stays with #14968.
This guard does not claim those dispositions are currently expressible.

Every changed/removed published definition needs exactly one applicable disposition
or an exact before/after, positive-issue, reason-bearing exclusion. Duplicate coverage,
invalid rows/exclusions, applicable rows absent from the published inventory, stale
exclusions, definition drift, wrong scopes, missing rename targets, changed compatible
rename value constraints/defaults (including a retained old-key alias), and changed
`unchanged` rows fail with named subjects.
`removed_inert` rows use canonical no-authority/no-read/no-write validation.

The thirteen exclusions in `v017-manifest-exclusions.json` deliberately retain
unresolved review boundaries. Five refer to #14968; the others to #7837. They are
not compatibility dispositions, alias implementations, runtime proof, or permission
to move configured choices. Any further drift invalidates their exact definitions.
Only unchanged, inert retirement and contributed rename relationships have static
conformance support. Other dispositions fail pending reviewed runtime or external
owner evidence; a made-up standard/project authority cannot be certified by a target
string. Remove/replace an exclusion only when that subject has a reviewed disposition
and its required proof. This guard does not close #7837 or #7841.

## Repeatable artifact verification and offline proof

Retrieve and verify in a temporary directory, without installing or changing profiles:

```sh
curl --fail --location --output /tmp/perl-lsp-rs-0.17.0.vsix \
  https://github.com/EffortlessMetrics/perl-lsp/releases/download/v0.17.0/perl-lsp-rs-0.17.0.vsix
printf '%s  %s\n' b67d84a149efd99ae26310742bc3773c6a4644258391796489b7a9476d2e47d4 /tmp/perl-lsp-rs-0.17.0.vsix | sha256sum --check
unzip -p /tmp/perl-lsp-rs-0.17.0.vsix extension/package.json > /tmp/perl-v017-package.json
cmp /tmp/perl-v017-package.json test/fixtures/configuration-migration/v0.17.0.package.json.fixture
npm test -- --runInBand --runTestsByPath src/test/configurationManifestConformance.test.ts
```

Run from `vscode-extension` with its admitted Node 26 / npm 11.18 toolchain.
Normal `npm test` / `test:ci` discovers the test. Tests verify the exact fixture hash
and identities offline; no network, installation or settings writes occur. Fixture,
exclusions and documentation are already excluded from VSIX packaging; the pure
checker is not imported by the extension runtime bundle.

## #7841 observed execution boundary and executor requirement

Real upgrade/rollback UX remains **NOT_PROVEN**. This Linux environment had no VS Code,
display server, or PowerShell. Official host download/catalog requests returned HTTP
403; an actual repository-native `@vscode/test-electron` download attempt failed
`EAI_AGAIN`. No install, activation, upgrade, downgrade, rollback, or GUI observation
ran. Existing mocks and receipt fixtures do not establish those behaviors.

A parent-arranged disposable GUI executor needs the authentic prior VSIX above
and an exact candidate VSIX plus matching platform-native server/DAP hashes and
artifact-set identity. A private Linux artifact handoff is available from its owner,
but transfer into this executor failed before any candidate bytes were verified.
Neither the source version string nor this manifest fixture supplies that candidate.
The execution matrix requires Linux minimum-supported host 1.125.0 and current
stable, and Windows x64 current stable, each in separate temporary
user-data/extensions/workspace/cache roots. This Linux executor cannot own native
Windows cells; the existing Windows source-review context is not an upgrade/rollback
execution claim. #16408 remains unclaimed for execution and #6056 names the
accountable owner. Use supported
CLI local VSIX installation with both isolated roots and `--force`, verify actual
activation/process identities and per-scope configured choices in the GUI, install
the exact candidate into the same roots, reload/reopen and observe idempotence.
Then close candidate processes and use the supported pinned prior installation or
explicit uninstall/reinstall route; record refusal if downgrade is unsupported.
Observe prior process selection/readiness and settings preservation before declaring
rollback. Capture artifact/host/configuration identities, before/after settings bytes,
warnings, trust and process evidence for each journey. Preserve #14968 ownership and
existing release holds: no Marketplace release, actual-user settings migration,
deployment, credentials, global machine changes or scope/trust broadening.
