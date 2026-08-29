# PLSP-ADR-0004: lsp-stack extraction boundary

Status: accepted
Date: 2026-05-26
Amended: 2026-08-29
Owner: perl-lsp maintainers
Implementation order: delegated to [#7384](https://github.com/EffortlessMetrics/perl-lsp-swarm/issues/7384)
Linked proposal: n/a
Linked specs:
- [PLSP-SPEC-0028](../specs/PLSP-SPEC-0028-lsp-stack-extraction.md)
Canonical implementation controller: [#7384](https://github.com/EffortlessMetrics/perl-lsp-swarm/issues/7384)
Historical plan: [lsp-stack extraction implementation plan](../../plans/lsp-stack-extraction/implementation-plan.md)

## Status Ruling

The boundary decision in this ADR remains accepted. Its former implementation-
sequence obligation is superseded.

[#7384](https://github.com/EffortlessMetrics/perl-lsp-swarm/issues/7384) is the
single implementation controller for extracting, proving, packaging, dogfooding,
and—only under separately authorized leaves—externalizing the reusable LSP
runtime. PLSP-SPEC-0028 remains the zero-Perl/product-boundary contract. The
linked implementation plan is retained as historical routing evidence and a
current pointer to #7384; it does not own a competing PR sequence.

A concrete implementation PR must therefore be owned by one leaf from #7384's
current train. This ADR alone does not authorize a crate, move, scaffold,
rewrite, package, external repository, publication, or release.

## Context

`perl-lsp` established current-app protocol and runtime proof sufficient to
define a reusable boundary:

- inline completion selects one registration mode for static and dynamic
  clients;
- LSP 3.18 inline-completion parameter handling has runtime proof;
- runtime watcher registration honors lean and e2e tuning;
- semantic-token delta is advertised only with result-id state and parity
  proof;
- raw-RPC and lean-editor receipt paths exist for the current app;
- editor docs distinguish current standard and custom integration behavior.

Those facts define a regression baseline. They do not create a reusable runtime,
prove independent package use, establish release readiness, or authorize code
movement.

The later #7384 programme broadened the implementation object from a narrow
crate-first move to a state-coherent runtime contract covering messages, codec,
application ports, scheduling/currentness, request terminality, delivery,
lifecycle, deterministic testkit, package proof, non-Perl dogfood, Perl product
cutover, and authorization-gated externalization. That programme supersedes the
former numbered sequence without weakening this ADR's boundary.

## Decision

Reusable runtime work must start from owned behavior and dependency direction,
not from moving files or renaming current crates.

The reusable seam is language-neutral LSP infrastructure that can be shared
without knowing Perl. It may eventually include:

- strict JSON-RPC request, notification, response, error, and request-ID
  contracts;
- bounded LSP framing, codec, and connection helpers;
- server-originated request and response-correlation primitives;
- language-neutral application, route, admission, scheduling, cancellation,
  currentness, terminality, lifecycle, and delivery contracts;
- capability-shape or dynamic-registration helpers that do not encode Perl
  feature/provider policy;
- bounded observations and deterministic protocol/runtime testkit utilities.

Perl-specific behavior stays in the Perl application unless a later accepted
authority proves a different owner. That includes parser, lexer, semantic and
source facts, project/workspace state, provider behavior, feature catalog,
capability and trust policy, application workers, inline-completion behavior,
editor receipts, DAP, CLI/product composition, packaging, and release
automation.

Current directories and omnibus crates are not move units. A file or module
that contains one neutral type beside parser/provider/product policy is mixed
until the owning leaf separates and proves the boundary.

## Preconditions

An affected implementation leaf must preserve the current-app contracts whose
semantic subjects it changes. Depending on scope, that includes:

- static inline-completion clients receive only the static provider;
- dynamic inline-completion clients receive only dynamic registration;
- disabled inline completion disables both registration paths;
- lean/e2e runtime modes do not register file watchers;
- file-watcher tuning does not suppress inline-completion registration;
- semantic-token delta is not advertised without result-ID-backed support;
- raw-RPC and lean-editor receipts remain current;
- docs distinguish standard inline completion from the custom
  `perlInlineCompletionStream` extension.

Use current unaffected evidence when its semantic subject remains unchanged.
Missing, stale, partial, or unavailable evidence is `NOT_PROVEN`, not pass.

## Dependency Boundary

Generic runtime code must not depend on Perl crates, Perl providers, Perl
runtime tooling, Perl application state, or Perl product/release surfaces.

Forbidden dependencies include:

- `perl-lsp-rs`;
- `perl-lsp-rs-core`;
- `perllsp`;
- parser, lexer, semantic, workspace, project, module-resolution, provider,
  perltidy, subprocess-runtime, or feature-catalog crates whose public contract
  requires Perl;
- DAP/debugger crates;
- editor, installer, package-release, signing, marketplace, or product-identity
  metadata.

Allowed dependencies must be language-neutral protocol, serialization, error,
runtime, or test infrastructure. Any new dependency requires review by the
owning concrete leaf. Compilation of the current omnibus crate does not prove a
selected candidate is Perl-free; package and downstream claims require their
own exact proof.

## Non-goals

This ADR does not authorize:

- implementing #7384 or another controller directly;
- creating a generic runtime crate before real neutral ownership is ready;
- moving whole protocol, transport, router, lifecycle, scheduler, provider, or
  omnibus directories by name;
- rewriting routing, scheduling, lifecycle, currentness, cancellation, or
  delivery outside their owning leaf;
- introducing generic handler/provider traits merely to make a move compile;
- extracting inline-completion/provider policy, Perl capabilities, DAP, editor,
  CLI, package, or release behavior;
- creating an external repository or publishing/tagging/releasing without the
  explicit authorization required by [#7397](https://github.com/EffortlessMetrics/perl-lsp-swarm/issues/7397)'s externalization train;
- claiming general reuse, stability, publication, or release readiness from an
  empty crate, one primitive, a workspace compile, or a compatibility re-export.

## Consequences

Positive consequences:

- one current controller owns implementation order;
- the zero-Perl/product boundary remains durable across train changes;
- current-app behavior remains the regression baseline;
- package/downstream and non-Perl dogfood proof stay distinct from mechanical
  movement;
- tempting duplicate audit, scaffold, and first-primitive trains fail closed.

Tradeoffs:

- extraction follows dependency-ordered causal leaves rather than a short
  numbered move list;
- some neutral-looking infrastructure remains embedded until its state,
  terminality, delivery, or policy ownership is proven;
- implementation names and package shape remain provisional until the
  corresponding #7384 decision and proof leaves land.

## Alternatives Considered

### Keep the former numbered implementation plan as a second route

Rejected. It produced competing audit and first-source candidates and omitted
parts of the state-coherent runtime, package, dogfood, and externalization proof
owned by #7384.

### Create a generic crate immediately

Rejected. An empty shell or one isolated helper does not prove useful ownership,
state coherence, package correctness, or external reuse.

### Extract all protocol and runtime modules at once

Rejected. Current modules mix neutral mechanisms with Perl error taxonomy,
capability/provider policy, `LspServer` state, application workers, currentness,
editor receipts, and compatibility behavior.

### Extract through generic handler traits first

Rejected as a default. Traits are permitted only when one concrete leaf proves
they remove a real dependency or authority rather than adding scaffolding.

## Follow-up Obligations

- Keep [PLSP-SPEC-0028](../specs/PLSP-SPEC-0028-lsp-stack-extraction.md) as the
  zero-Perl/product-boundary acceptance contract.
- Keep [#7384](https://github.com/EffortlessMetrics/perl-lsp-swarm/issues/7384)
  as the implementation controller until an accepted successor explicitly
  replaces it.
- If #7384 is replaced, update the ADR, spec, and historical-plan pointer to the
  successor; do not restore the former numbered crate-first sequence.
- Require each implementation PR to name one concrete leaf, the authority moved,
  dependency direction, affected current-app proof, compatibility exit,
  rollback, and any package/reuse claim boundary.
- Keep release, publication, external-repository, and stability claims outside
  ordinary extraction PRs unless their separately authorized leaves prove them.

## Status Links

- [Canonical runtime controller #7384](https://github.com/EffortlessMetrics/perl-lsp-swarm/issues/7384)
- [PLSP-SPEC-0028](../specs/PLSP-SPEC-0028-lsp-stack-extraction.md)
- [Historical implementation plan](../../plans/lsp-stack-extraction/implementation-plan.md)
- [LSP interactive latency rollout](../development/LSP_INTERACTIVE_LATENCY_ROLLOUT.md)
- [Editor setup](../how-to/EDITOR_SETUP.md)
- [IntelliJ IDEA setup](../EDITORS/INTELLIJ_IDEA_SETUP.md)
- [LSP capability contract ADR](0021-lsp-capability-contract.md)
- [Custom LSP runtime ADR](0034-custom-lsp-runtime.md)

## Why ADR-worthy

This decision defines the durable knowledge and dependency boundary for a
reusable runtime while allowing the implementation train, package shape, and
externalization path to evolve under one current controller.
