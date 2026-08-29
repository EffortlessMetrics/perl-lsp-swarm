# Context: #11709 — compile the durable Zed integration architecture

This is a checked, declarative programme-architecture contract. It changes no
product behavior, extension source, workflow, receipt, support claim, release,
registry subject, GitHub state, or external repository. It compiles settled
decisions so later nodes consume them instead of re-deriving the programme from
dozens of issues, review threads, and transient candidate state.

## Problem

The Zed programme has a strong product and evidence graph, but its settled
architecture was distributed across controller prose (#7759), child issues,
review threads, staged fixtures, and train-candidate state. Before another
stable graph or agent packet becomes authoritative, the lasting decisions must
be compiled into checked leaf contracts that do not depend on current PR state,
workflow colour, transient GitHub observations, or a model reconstructing the
programme from broad archaeology.

This bundle is **Z00 — durable architecture/spec compilation**. It creates the
semantic input consumed by #10338 (the stable DAG). It does not replace
#10338's DAG, the #10872/#10881 packet schemas, #9483 CI currentness, #10479
live observation, or any product/evidence owner.

## Why this approach

A spec-only bundle lands the stable decisions before any tooling exists,
exactly as the sibling precedents `.spec/10894-editor-host-reliability/`
(PR #11811), `.spec/11763-issue-controller-architecture/` (PR #12006), and
`.spec/11716-emacs-support-architecture/` (PR #12008) did for their rails. A
tooling-first compilation would freeze accidental mechanics as authority; an
issue-prose-only state would leave every leaf re-reading controller
archaeology and re-deriving boundaries differently. The method authority is
#3983 and the current `.spec` conventions (`docs/reference/SPEC_TEMPLATE.md`);
no Zed-specific spec schema is invented, and the shared authority represents
every decision this bundle records.

## Current state (honest, past tense, at this bundle's compilation base)

Nothing in this section claims future state; it records what existed on the
exact tree this bundle lands on:

- The repository carried **checked `.spec` bundles** under the three-file
  `docs/reference/SPEC_TEMPLATE.md` convention (a compilation-time
  observation, not a durable invariant); none covered the Zed integration
  architecture.
- The repository carried **Zed validator tooling**: `scripts/check-zed-*.sh`
  contract runners, `xtask/tests/zed_*.rs` contract tests, and
  `xtask/src/bin/validate-zed-*.rs` receipt validators, exercised by the
  Zed-integration CI workflow. These validate staged JSON contracts and host
  receipts; they do not validate `.spec` bundles (the absent executable
  `.spec` graph validator remains an honest tooling gap, recorded by the
  sibling precedents and not papered over here).
- A closed draft train candidate had served as the implementation seed and
  review corpus; this bundle is deliberately independent of it. Its
  load-bearing findings survive only as the decisions recorded below with
  their owning issues, never as branch or check state.
- The upstream Zed extension fixture staged on tree (`extension.toml`) still
  declared the existing upstream server `perl-lsp` alongside the existing
  default `perlnavigator-server` and the EffortlessMetrics entry then keyed
  `perllsp`. Migrating that key to `perl-lsp-rs` is decision
  `zed.server.identity` (#10842), not a completed tree observation.
- The executable `.spec` structural checks in the sibling bundles proved that
  a deterministic, twice-run byte-identical checker is the working proof layer
  until a repository-wide spec validator lands.

## Authority and ownership

### Programme controller: #7759

Owns the Zed programme meaning and its closeout denominator. It is a
controller: no ordinary builder leaf closes it, and no single leaf's bounded
authority or evidence constitutes programme completion.

### This node: Z00 #11709

Owns durable compilation only: stable identities, decision ownership, evidence
and publication stages, truth planes, claim ceilings, agentic execution law,
and explicit non-claims. It implements nothing.

### Downstream consumers

| Consumer | Owns | Consumes from this bundle |
| --- | --- | --- |
| #10338 (Z01) | Stable DAG, roles, typed edges, conflicts, manual stops | Decision IDs, identity contract, stage ladder, dependency classes |
| #11710 (Z02) | Exact-tree implementation observations | Proposition boundaries per leaf |
| #11711 (Z03) | Offline frontier | Claim ceilings and dependency classes |
| #10479 | Optional live overlay | Stable-vs-mutable boundary |
| #9483 | CI currentness | Evidence-stage semantics |
| #11712–#11714 | Packets, projections, closeout consumers | Leaf contracts and falsifier slices |

## Durable laws

### Identity contract (load-bearing invariant)

These independent identities are durable. None aliases another:

```text
Zed server ID       = perl-lsp-rs
Zed display name    = Perl LSP (EffortlessMetrics)
launched executable = perllsp
product/package     = perl-lsp
existing upstream   = perl-lsp
existing default    = perlnavigator-server
DAP adapter/binary  = perl-dap
extension ID        = perl
```

`perl-lsp-rs` must launch exact `perllsp --stdio`. It cannot alias, wrap, or
fall through to `perl-lsp`. The product/package name `perl-lsp` names a real
separate crates.io project and the existing upstream server; it is never an
executable name in a Zed launch. DAP cannot enter the LSP support row or block
#7759.

### Four truth planes (load-bearing invariant)

1. stable semantic architecture and PR topology;
2. exact current-tree implementation state;
3. optional live GitHub candidate/review/check state;
4. product behavior, public artifact, installed-host, support, and release
   evidence.

No plane substitutes for another:

```text
issue or PR state            != implementation on tree
implementation on tree       != real Zed behavior
public asset bytes/process   != Zed host behavior
exact-source host behavior   != official-registry distribution
upstream submission          != merged subject
merged subject               != released ordinary-install subject
host receipt                 != support projection
LSP evidence                 != DAP evidence
```

### Evidence and publication stage ladder

Each stage is a distinct authority with an exact handoff; promotion requires
the owning stage's evidence, never prose:

```text
S01 static source/package authority          (#10395)
S02 public asset bytes/archive/process       (#8661 / #8678)
S03 exact-source development-extension behav.#11041 + host-execution owners
S04 settings behavior                        (#10392)
S05 default/provider-order behavior          (#10393 / #11043)
S06 managed route/cache recovery             (#8753 / #8772 / #10396)
S07 exact-source fan-in                      fan-in role, not builder work
S08 upstream packet freeze                   packet_freeze role, frozen corpus
S09 manual external submission               external action, explicit stop
S10 merged upstream acceptance               read-only acceptance of merge fact
S11 official registry packet freeze          packet_freeze role, frozen corpus
S12 manual registry submission/released def. external gate, explicit stop
S13 clean official-registry public host      (#9467 / #7912)
S14 support-registry/generated-doc projection(#10168 close authority)
S15 programme closeout                       #7759 denominator complete
```

Stage-promotion laws: a public asset receipt is not real Zed behavior; an
exact-source receipt is not official-registry distribution; support docs are
never promoted from a packet, merge, or host receipt without the #10168
projection owner; a manual checkpoint is a stop, not a leaf.

### Agentic execution law

Every concrete leaf retains: one proposition; one semantic writer/conflict
key; hard/evidence/external/optional dependencies; a claim ceiling and
explicit non-claims; canonical authorities consumed and forbidden substitutes;
a first realistic false-green control; required code/spec/test/fixture/schema/
docs/generated/receipt artifacts; resolved focused and canonical proof
commands; the correct Advances/Closes relation; rollback/compatibility/
predecessor exit; next handoff; and stop/return conditions. Controllers,
fan-ins, manual checkpoints, external actions, and already-landed leaves
cannot become ordinary builder work.

### Stable versus mutable information

This bundle may contain stable issue IDs and authority names. It must not
encode as stable truth: the current main SHA; an open PR number or branch;
review/check colour; the assigned model or worker; wall-clock readiness;
current candidate uniqueness; the current workflow run; or a mutable release
or registry subject. Those belong to exact-tree observations (#11710),
optional live overlay (#10479), or evidence receipts.

## Compatibility with the repository operating contract (`AGENTS.md`)

- **GitHub owns durable live transaction state.** The four-truth-plane law
  keeps candidate, check, readiness, and release state out of these durable
  bytes; only stable reviewed decisions are recorded here.
- **No parallel lifecycle.** Nothing here creates a scheduler, lease table,
  agent roster, or task database; writer admission stays with the generic
  authorities (#4177 / #3982 / #3957), and route selection stays with the
  named `$deliver-*` skills.
- **Proof and currentness discipline.** Material change to a consumed
  decision invalidates affected downstream artifacts through their owning
  nodes; missing or stale evidence stays `not_proven` rather than pass.
- **Shared authority over duplication.** The #10858 operation/evidence
  profiles, #10872/#10881 packet schemas, and #10168 semantic-close authority
  are consumed, never cloned into a Zed-local ontology.

## Open decisions surfaced for owning nodes

Recorded, not decided here:

1. The final selected extension execution source and route is owned by
   #11041; this bundle fixes only that the decision and its evidence stage
   exist and are distinct from materialization and from registry
   distribution.
2. Implementation leaves under #10395's reproducible-materialization
   authority are sequenced by their own issues; this bundle fixes the
   authority boundary, not their order.
3. The exact released-subject checkpoint cadence between S11 and S13 is owned
   by #9467/#7912; this bundle fixes that the stages are distinct.
4. Support-projection mechanics beyond the #10168 ownership rule remain with
   the support-registry owners; no Zed-local support schema is created.
5. Optional platform/activation/remote/DAP breadth becomes blocking only when
   a selected bounded claim requires it (#11046/#10991 own the breadth
   matrix); the default remains non-blocking sidecar.

## Adoption, rollback, transfer and stop

**Adoption.** #10338 encodes the stable DAG directly from this bundle's
decision IDs, identity contract, stage ladder, and falsifiers; #11710/#11711/
#10479/#9483 consume their respective planes. No consumer rebuilds a compiled
decision.

**Rollback.** Revert the single commit or remove the bundle directory; no
runtime, product, CI, support, or GitHub state depends on it. Programme issue
authority is unchanged by rollback.

**Transfer.** If the architecture is superseded, a successor bundle supersedes
this one by explicit link, and downstream consumers re-derive affected nodes.

**Stop.** This bundle stops before: stable-DAG implementation (#10338), the
current-tree successor (#11710), readiness/frontier solving, live observation,
packet generation, product or extension implementation, host execution,
packet freeze, support promotion, external submission, release, publication,
or any external repository mutation.

## Links

- Controlling issue: #11709; parent programme: #7759.
- Stable train authority (consumer): #10338.
- Shared issue/spec authorities: #3983 / #3586 (historical context only).
- Shared operation/evidence profiles: #10858.
- Shared builder/reviewer packets: #10872 / #10881.
- Semantic close authority: #10168; extraction gate: #10554; lifecycle: #3949.
- Product authority issues: #10842, #10395, #11041, #10340, #10530, #11304,
  #10392, #10393, #11043, #8647, #11046, #10991.
- Managed-artifact authority issues: #8661, #8678, #10396, #8753, #8772,
  #11316, #11308, #9467, #7912.
- Method/spec convention: `docs/reference/SPEC_TEMPLATE.md`.
- Bundle precedents: `.spec/10894-editor-host-reliability/` (PR #11811),
  `.spec/11763-issue-controller-architecture/` (PR #12006),
  `.spec/11716-emacs-support-architecture/` (PR #12008).
