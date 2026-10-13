# PLSP-SPEC-0028: lsp-stack extraction boundary

Status: accepted
Owner: perl-lsp maintainers
Linked proposal: n/a
Linked ADRs:
- [PLSP-ADR-0004](../adr/PLSP-ADR-0004-lsp-stack-extraction.md)
Canonical implementation controller: [#7384](https://github.com/EffortlessMetrics/perl-lsp-swarm/issues/7384)
Historical plan: [lsp-stack extraction implementation plan](../../plans/lsp-stack-extraction/implementation-plan.md)
Status impact: protocol/runtime hardening, editor integration docs, future
crate-boundary reviews

## Current Implementation Status

This is a boundary specification, not an implementation controller. The former
numbered implementation sequence has been superseded by #7384's state-coherent
runtime programme. Use one concrete leaf from that controller; do not create an
implementation PR against this spec or its historical plan alone.

No reusable LSP runtime package, external repository, publication, or stability
contract is established by this specification. The current app remains the
source of truth for LSP behavior until the relevant #7384 leaves prove and land
the replacement path.

The hardening baseline remains a regression boundary for any affected leaf:

- inline-completion registration mode is coherent for static, dynamic, and
  disabled clients;
- LSP 3.18 inline-completion request shape has runtime coverage;
- runtime watcher registration honors lean and e2e file-watcher tuning;
- semantic-token delta is advertised only with result-id state and parity proof;
- raw RPC and lean editor receipts exist for current-app behavior;
- editor docs distinguish LSP4IJ, Neovim lean/e2e mode, standard inline
  completion, and the custom `perlInlineCompletionStream` extension.

This spec does not claim release readiness.

## Implementation Routing

[#7384](https://github.com/EffortlessMetrics/perl-lsp-swarm/issues/7384) is the
single implementation controller. It owns the dependency-ordered trains for:

- messages, JSON-RPC structure, and neutral errors;
- codec and connection behavior;
- application ports and route/work policy;
- scheduling, pressure, terminality, and currentness;
- reverse requests and delivery;
- lifecycle, cancellation, validation, and application workers;
- observations and deterministic testkit;
- substrate selection, public package proof, non-Perl dogfood, Perl cutover,
  and authorized externalization.

The constraints below remain applicable to those concrete leaves. When this
spec conflicts with #7384 about implementation order or package shape, #7384
controls the route and this spec controls only the zero-Perl/product-boundary
constraint unless a newer accepted authority says otherwise.

## Contract

Extraction is allowed only as a staged migration from proven current-app
behavior to language-neutral infrastructure.

The reusable seam may include only infrastructure that does not need Perl
source, Perl facts, Perl runtime state, Perl debugging state, or Perl release
state:

- JSON-RPC request, notification, response, error, and request-id contracts;
- bounded LSP framing and transport helpers;
- server-originated request and response-correlation primitives;
- capability-shape helpers that do not encode Perl features;
- dynamic-registration helpers that do not encode Perl provider behavior;
- lifecycle, scheduling, admission, cancellation, delivery, and stale-read
  primitives that are language-neutral;
- protocol contract tests and deterministic testkit primitives.

The Perl application must retain ownership of:

- Perl parser, lexer, semantic analyzer, source, and project facts;
- workspace indexing and module resolution;
- provider implementations and provider receipts;
- `features.toml`, capability catalog, support policy, and project trust;
- inline-completion provider behavior and stream payloads;
- parser/indexer/watcher/diagnostic application workers;
- DAP and Perl debugger integration;
- editor-specific integration and support docs;
- CLI composition, release, publish, signing, marketplace, package, installer,
  and product identity surfaces.

## Dependency Boundary

Future generic runtime code must be dependency-inverted away from Perl.

It must not depend on:

- any crate named `perl-*`;
- `perl-lsp-rs`;
- `perl-lsp-rs-core`;
- `perllsp`;
- DAP crates from this workspace;
- parser, lexer, semantic, workspace-index, module-resolution, perltidy, or
  subprocess-runtime crates from this workspace;
- generated feature catalogs that encode Perl provider policy;
- editor, release, installer, or product metadata.

It may depend only on language-neutral protocol, serialization, error, test, or
runtime infrastructure after dependency review. Adding or widening dependencies
is a separate acceptance item and cannot be hidden inside a move or scaffold PR.
A source file that contains one neutral type and one Perl-coupled type is mixed;
its path or module name does not prove an extractable boundary.

## Valid PR Shapes

Valid concrete-leaf PRs under this boundary include:

- static ownership or dependency analysis that reports blockers without moving
  code;
- focused tests that preserve current-app behavior before changing ownership;
- dependency-boundary changes that remove Perl policy from a generic candidate;
- one bounded no-behavior-change ownership move after its dependency and parity
  proof is current;
- post-move integration and compatibility changes that preserve one product
  authority;
- deterministic testkit, package, unpacked-consumer, and non-Perl dogfood proof
  owned by the corresponding #7384 leaves.

Every PR must state whether it changes:

- code location or public paths;
- runtime or JSON-RPC behavior;
- capability shape or dynamic registration;
- scheduling, currentness, cancellation, terminality, or delivery;
- editor integration behavior;
- dependencies or package/public API;
- release, publication, or external-repository surfaces.

## Invalid PR Shapes

Invalid PRs include:

- implementing a controller issue instead of one concrete leaf;
- creating a generic crate before real neutral ownership is ready to move;
- moving a whole current directory or omnibus crate by name;
- importing Perl, provider, workspace, project, DAP, editor, or release policy
  into generic runtime code;
- bundling extraction with unrelated inline-completion, DAP, provider, editor,
  release, publish, signing, marketplace, installer, or package changes;
- adding generic handler traits merely to make a file move compile;
- rewriting routing, scheduling, or lifecycle outside their owning #7384 leaf;
- weakening current inline-completion, watcher, semantic-token, raw-RPC, lean
  editor, terminality, freshness, pressure, or delivery proof;
- claiming independent reuse from an empty crate, one primitive, workspace-only
  compilation, or compatibility re-export;
- creating an external repository, publishing, tagging, or releasing without
  the explicit authorization required by #7397's externalization train.

## Acceptance

A boundary-documentation PR satisfies this spec when it:

- states that no code or runtime behavior changed;
- defines the reusable and Perl-specific ownership boundary;
- bans Perl dependencies in generic runtime code;
- links #7384 as the canonical implementation controller;
- defines executable proof and rollback;
- avoids reuse, release, publication, and stability overclaims.

A future implementation PR satisfies this spec only when it:

- is owned by one concrete #7384 leaf and its prerequisites are current in
  source;
- moves or changes one bounded authority;
- proves affected current-app behavior still passes;
- proves the generic candidate has no Perl/application dependency;
- preserves typed source/cause and request/currentness/terminal/delivery
  identity rather than stringifying or flattening non-pass outcomes;
- states rollback and compatibility exit conditions;
- leaves unrelated providers, DAP, editor, package, release, and publication
  surfaces alone.

General reuse is not proven until the package and dogfood leaves show an
independent non-Perl mutable server consuming only the packaged public API.

## Proof Commands

Docs-only boundary changes must run:

```bash
git diff --check
just ci-docs-check
```

When support claims may change, also run:

```bash
cargo xtask check-support-claims
```

Implementation leaves run the exact proof named by their controlling issue.
When they touch current protocol/runtime behavior, the affected proof may
include:

```bash
./scripts/cargo-safe test -p perl-lsp-rs --test lsp_inline_completion_registration_tests --profile agent --locked
./scripts/cargo-safe test -p perl-lsp-rs --test lsp_registration_tests --profile agent --locked
./scripts/cargo-safe test -p perl-lsp-rs --test lsp_cap_snap --profile agent --locked
./scripts/cargo-safe check -p perl-lsp-rs --all-targets --profile agent --locked
./scripts/cargo-safe check -p perl-lsp-rs-core --all-targets --profile agent --locked
```

Raw-RPC and lean-editor receipts remain required when the semantic subject
touches scheduling, watcher registration, diagnostics timing, initialization,
request dispatch, cancellation, currentness, or delivery. Package and reuse
claims require the package/unpack/downstream and non-Perl dogfood proof owned by
#9301 and #7395; workspace compilation is not a substitute.

Report unrelated pre-existing warnings separately from candidate failures.
Missing, stale, partial, or unavailable proof is `NOT_PROVEN`.

## Rollback Rules

Boundary-doc rollback:

- revert the documentation PR;
- keep current app behavior and proof unchanged;
- never restore or reopen the former numbered implementation sequence;
- reconsider one historical candidate only when neither #7384 nor its accepted
  successor owns that exact claim.

Implementation rollback:

- revert the smallest leaf PR that introduced the regression;
- restore current-app imports, ownership, and compatibility paths;
- keep a real regression test even when the ownership move is reverted;
- do not repair extraction by weakening current-app, dependency, currentness,
  terminality, pressure, delivery, package, or dogfood proof.

Dependency rollback:

- remove the dependency from generic runtime code;
- move product-specific code back to the Perl application;
- record why the candidate was mixed and update the owning #7384 leaf.

## Non-goals

This spec does not authorize:

- creating a crate or external repository;
- moving production code;
- rewriting routing, scheduling, lifecycle, or delivery;
- introducing generic handler/provider traits;
- extracting inline-completion implementation;
- extracting DAP;
- changing editor, package, release, publish, tag, signing, or marketplace
  automation;
- claiming general reuse, stability, release readiness, or publication.

## Claim Boundaries

This spec may claim only that the extraction boundary is documented and routed
to #7384. It must not claim:

- an extracted reusable runtime exists;
- extraction has started or completed;
- runtime behavior changed;
- editor integrations are more ready than current receipts prove;
- a public package, non-Perl consumer, external repository, release,
  publication, or stability contract exists.
