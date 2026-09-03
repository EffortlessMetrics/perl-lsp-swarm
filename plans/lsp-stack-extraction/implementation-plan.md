# lsp-stack Extraction Implementation Plan

Status: superseded for implementation
Owner: perl-lsp maintainers
Canonical controller: [#7384](https://github.com/EffortlessMetrics/perl-lsp-swarm/issues/7384)
Linked ADR: [PLSP-ADR-0004](../../docs/adr/PLSP-ADR-0004-lsp-stack-extraction.md)
Linked boundary spec: [PLSP-SPEC-0028](../../docs/specs/PLSP-SPEC-0028-lsp-stack-extraction.md)
Convergence issue: [#13523](https://github.com/EffortlessMetrics/perl-lsp-swarm/issues/13523)

## Ruling

This document no longer owns implementation order.

[#7384](https://github.com/EffortlessMetrics/perl-lsp-swarm/issues/7384) is the
single implementation controller for extracting and externalizing the reusable
LSP runtime. Use one concrete leaf from its canonical train. Do not create a
crate, audit, scaffold, move, or cleanup PR from the former numbered sequence in
this file.

The previous sequence remains available in Git history as design history. It
was superseded because it described a narrow crate-first extraction while the
current programme owns a state-coherent runtime spanning messages, codec,
application ports, scheduling/currentness, request terminality, delivery,
lifecycle, testkit, package proof, non-Perl dogfood, and Perl-product cutover.

## Preserved Boundary Findings

The following findings remain valid inputs to #7384 and its leaves:

- generic runtime code must have zero dependency on Perl source, parser,
  semantic, provider, workspace, project, DAP, editor, package, release, or
  product policy;
- Perl capability policy, feature catalogs, provider behavior, project trust,
  parser/workspace facts, application workers, CLI, and editor packaging remain
  in the Perl application;
- current `protocol`, `transport`, and `runtime` directories are not whole move
  units merely because their names sound generic;
- `JsonRpcId` is language-neutral in behavior, while its current containing
  file is mixed because `JsonRpcError` imports the parser-owned operational
  error taxonomy;
- low-level Content-Length framing is separable, but current framing also owns
  parser classification, JSON-RPC decoding, logging/recovery policy, and the
  `$/perl-lsp/clientResponse` compatibility shim;
- current scheduling, cancellation, lifecycle, watcher, and reverse-request
  mechanisms contain reusable kernels but are mixed with `LspServer`, Perl
  currentness, capability policy, application state, and editor receipts;
- an empty crate or one moved primitive does not prove general reuse;
  independent package proof and a real non-Perl mutable-server consumer are
  separate gates;
- no extraction change earns release, publication, support, or stability claims
  without the separately governed evidence and authorization.

## Current Route Map

The references below are current GitHub **issues** unless explicitly labelled
as a PR. They are the subjects linked from #7384, not historical PR-number
references from older forensic documents.

| Concern from the former plan | Current #7384 owner |
| --- | --- |
| Strict incoming decode and message kinds | [#7596](https://github.com/EffortlessMetrics/perl-lsp-swarm/issues/7596), [#7626](https://github.com/EffortlessMetrics/perl-lsp-swarm/issues/7626), [#9636](https://github.com/EffortlessMetrics/perl-lsp-swarm/issues/9636) |
| Neutral generic errors and Perl classification | [#7611](https://github.com/EffortlessMetrics/perl-lsp-swarm/issues/7611), [#7612](https://github.com/EffortlessMetrics/perl-lsp-swarm/issues/7612), controller [#7599](https://github.com/EffortlessMetrics/perl-lsp-swarm/issues/7599) |
| Content-Length codec and DAP sidecar | [#9638](https://github.com/EffortlessMetrics/perl-lsp-swarm/issues/9638), [#7602](https://github.com/EffortlessMetrics/perl-lsp-swarm/issues/7602) |
| Application boundary and route authority | #9640, #9642, #7609, #7610, #9503, #9504 |
| Ordered scheduling, priority, pressure, and freshness | #9643, #9644, #9505, #7098, #9506, #9646 |
| Reverse requests and write-boundary delivery | #7007 / PR #7245, #7010, #8402, #9507 |
| Lifecycle, cancellation, workers, and validation | #9647, #9648, #9508, #9650, #9652, #9653 |
| `LspServer` state decomposition | #8384, #8385, #8386, #8388, #8389 |
| Observations and deterministic testkit | #9510, #9654, #9655, #9656 |
| Substrate bakeoff and terminal architecture decision | #9353, #9356, #9358, #9360, conditional #9512 |
| Zero-Perl package and public API proof | #9291, #9509, #9657, #9658, #9298, #9301 |
| Non-Perl and Perl product dogfood | #7395, #9659, #9660 |
| Public-facade and externalization closeout | #9511, #7216, #9661, #9663, #9666, #9668, #9670 |
| Late omnibus retirement | #7412 |

## Current Entry Rule

1. Read #7384 and the selected concrete leaf.
2. Confirm every named prerequisite is present in current source, not merely
   closed in GitHub.
3. Reuse the current candidate when one exists; do not open a competitor.
4. Keep one PR to one acceptance-and-rollback claim.
5. Keep generic code free of Perl/application policy and preserve message,
   request, source/currentness, terminal-cause, and delivery identities.
6. Run focused proof first, then affected package and repository-required proof.
7. Do not publish, create an external repository, tag, or release without the
   explicit authorization required by #7397's externalization train.

## Historical Candidate Disposition

The following audit candidates arose from the obsolete executable-looking
sequence and are superseded by #7384 after their useful facts were mapped above:

- #13054 / PR #13084
- #13059 / PR #13097
- #13080 / PR #13106
- #13093 / PR #13090

The in-place `JsonRpcId` preparation issue #13176 is not an independent current
frontier. Its useful dependency and proof requirements belong under the message
and error leaves named by #7384, especially #7611, #9636, and #7612.

## Proof for This Documentation Boundary

```bash
git diff --check
just ci-docs-check
cargo xtask check-support-claims
```

Missing or unavailable command evidence is `NOT_PROVEN`, not pass.

## Successor and Rollback

Supersession survives controller replacement. If #7384 is explicitly replaced,
update this document to name the accepted successor and its concrete leaves; do
not restore the obsolete numbered crate-first sequence.

Reopening one of the historical candidates requires evidence that its exact
claim is not covered by #7384 or the accepted successor controller.