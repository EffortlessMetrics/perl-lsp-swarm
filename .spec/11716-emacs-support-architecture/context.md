# Context: #11716 — durable Emacs support architecture and evidence boundaries

## Problem

The Emacs support programme (#7979 / #8706) has converged on a strong
product/evidence architecture, but the settled decisions are still distributed
across programme controllers, client controllers, host-journey leaves,
project/root issues, public-artifact issues, registry/docs issues, review
comments, and newer shared evidence seams (#10894, #11360, #11361, #11766,
#11768, #11770). Before the stable train consumer #10918 encodes
`emacs_train.v1` as the machine-readable execution topology, those decisions
must become one checked repository contract so the train consumes them instead
of becoming their second authority. A builder must be able to compile any leaf
later without rereading controller archaeology.

This bundle is E00 — durable architecture/spec compilation. It changes no
Emacs/perllsp behavior, launches no host, computes no readiness, observes no
GitHub state, and creates no support claim.

## Why this approach

The architecture is projected as a spec-only bundle before the train manifest,
leaf specs, and implementation for three reasons. First, ordering: every
downstream control plane (#10918 `emacs_train.v1`, #11717 checked leaf specs,
#11718 exact-tree context, #11719 packets, #10936 dogfood, #11770 revision
governance) and every implementation lane (subjects, adapters, profiles,
actual-host journeys, roots, public replay, registry/docs) consumes these
boundaries, so the boundaries must be reviewable before any of them is
encoded. Second, honesty: the train graph and the architecture it preserves
must stay separate authorities — a graph byte cannot amend a durable decision,
and a durable decision must not silently mutate because a graph was edited.
Third, drift: without one authority split, each leaf re-derives whether #8734
or #10894 owns process cleanup, whether a profile proves a host, or whether a
manual root is stock discovery, and cheap agents get many paths to a plausible
green. Every projection derived from this bundle must remain deterministic and
second-run clean.

## Stable identity decisions

Exact subject identity is non-substitutable. A visible version string,
executable name, config snippet, capability object, or another Perl server
cannot satisfy it.

| Identity cell | Durable decision |
| --- | --- |
| Product / public repo | `perl-lsp` |
| Server executable | `perllsp` |
| Canonical launch | `perllsp --stdio` |
| Integration mode | `generic_lsp` |

Client families, in fixed order — each is a distinct exact subject generation
and no family fills another's evidence:

1. bundled Eglot
2. released standalone Eglot
3. pinned upstream-source Eglot
4. released lsp-mode
5. pinned upstream-source lsp-mode

Major-mode cells: `perl-mode`, `cperl-mode`, and optional `perl-ts-mode`.
Version strings are naming hints, never authority; exact identity binds
executable/file digests, loaded-client digests, and immutable source state per
the subject-materialization lane.

## Four truth planes

These planes are a durable invariant. No plane substitutes for another:

1. stable Emacs semantic architecture and implementation topology;
2. exact current-tree implementation state;
3. live branch/worktree/PR/check/review/writer state;
4. behavior/public/support/release evidence.

Plane non-substitution table (fixed order, ten laws):

| True statement in one plane | Never implies |
| --- | --- |
| issue/PR closed | implementation on tree |
| runner/profile present | actual Emacs behavior |
| actual local host pass | public artifact |
| manual registration pass | stock discovery |
| correct root URI | root-sensitive semantics |
| exact-source source head | released client |
| upstream accepted | released built-in client |
| Linux pass | macOS/Windows/TRAMP |
| schema-valid receipt | host observation |
| host observation | support projection |

## Authority and ownership

### Shared generic authorities (consumed, never cloned)

- #7777 / #10527 — generic durable editor-client receipt semantics and
  integrity. No Emacs-local receipt ontology may duplicate them.
- #10894 — generic editor-host deadline/freshness/process-ledger/cleanup/
  outcome authority, projected in
  `.spec/10894-editor-host-reliability/`. It owns the generic host-run
  contract for every editor consumer.
- #10872 / #10881 — shared builder and reviewer packet contracts.
- #10858 — shared dependency/evidence profile vocabulary.
- #10554 — generic checked-train extraction gate; #11770 reuses its concrete
  mechanical primitives where they exist.

### Emacs lane authorities

```text
#10894 generic editor-host reliability authority
   ↓ (adoption/conformance, not re-ownership)
#8734 Emacs-specific host-runner conformance + transcript/artifact/orphan falsifiers

#11744 core subject manifest/resolver/cache + bundled Eglot subjects
   ├→ #11745 released + pinned-source Eglot subjects
   └→ #11746 released + pinned-source lsp-mode subjects
        ↓
      #8755 subject-materialization fan-in (reference, not a giant coding leaf)

#11360 typed host-native observation vocabulary and conjunction validation
#11361 one observation → editor_client_compat.v1 receipt producer
#8776 / #8795 Eglot and lsp-mode client adapters (selected client/process facts)

#8819 / #8821 authentic synthetic capability profiles (Eglot, lsp-mode)
#8822 / #8823 / #8824 / #8825 / #8828 / #8830 actual local-host semantic verdicts

#11768 governed Emacs host journeys, fixture expectations, cohort membership,
        and receipt-cell catalog (actual-host leaves select checked
        journey/cell identities; they do not create free-form receipt truth)

#11366 root fixture/probe substrate (landed current-tree substrate)
#11747 Eglot stock-root observation → #11749 Eglot root semantics/override
#11748 lsp-mode stock-root observation → #11750 lsp-mode root semantics/override
#8834 / #8838 root-observation and root-semantics fan-ins

#8842 Linux install/fresh-process substrate
#8846 / #8849 / #8853 bundled/released-Eglot and released-lsp-mode public replays
#8858 / #8862 / #8865 exact registry → generated docs → current-main certification

#11717 / #11718 / #11719 checked-leaf-spec, exact-tree-context, and packet
        control planes over the shared #3983 compiler and #10872/#10881
        contracts
#10936 packet dogfood controller (#11759 deterministic, #11760 real cohorts)
#11770 semantic train revision and impact governance
```

No leaf may recreate another layer because its API is inconvenient. #8734 does
not own generic host process/cleanup/artifact policy after #11766/#10894; it
remains the Emacs proving consumer.

### Already-landed historical foundations

The durable architecture does not require new implementation work for
already-landed foundations: the exact `perllsp` process facade, #7054
configuration authority, #7007 request registry, and #11366 project
fixture/probe substrate. Preserving their stable authority relationships here
does not certify their current-tree state; exact current-tree truth remains
#10923's job.

## Diagnostic cohort contract

Exact client generations keep independent diagnostic evidence:

- bundled Eglot push cohorts — actual `publishDiagnostics` plus Flymake
  consumption;
- released/source standalone Eglot pull cohorts — actual
  `textDocument/diagnostic` polling plus
  `resultId`/`previousResultId`/`unchanged`/edit-invalidation/clear;
- lsp-mode — the diagnostic path is observed per exact subject and is never
  flattened to a generic `diagnostics-supported` bit.

Synthetic capability fixtures prove negotiation/result shapes only. They do
not prove Flymake or lsp-mode consumption.

## Project/root contract

Three root evidence states, kept distinct:

1. `stock_project_discovery` — observed unprebound stock selection;
2. `standard_user_project_override` — an ordinary user override, separately
   classified where stock discovery is insufficient;
3. `future custom_repository_helper` — not implemented or implied.

A manually bound fixture root cannot become stock discovery. A correct
`rootUri` cannot establish module/configuration isolation without #8838-class
behavior-bearing proof.

## Public and distribution stages

Local evidence stages: `exact_source_local`, `release_candidate`,
`public_artifact` — each requires exact direct evidence; no relabeling or
inheritance. Registration/upstream stages: `manual_client_registration`,
`upstream_source_registration/client`, `upstream_accepted_unreleased`,
`upstream_builtin_released` — external acceptance never becomes shipped
built-in discovery without its own released evidence.

## Platform and optional breadth

The initial support cut is Linux first-mile and platform-independent in
contract: no Linux-specific mechanism is claimed as another platform's
capability, and no macOS/Windows/TRAMP support is implied. Unsupported or
unobserved capability stays `not_proven`, never an inferred pass or
incompatibility. The post-Linux
platform/upstream/optional train (#9310) and the documented
formatting/code-action/inlay-hint feature-depth train (#9413) are
already-decomposed trains referenced here, not re-expanded and not merged into
the bounded initial Linux cut except where current documentation must be
qualified to avoid an unsupported claim. Optional perl-ts-mode, TRAMP, and
managed-install breadth (#7774/#7775/#7776) is never a hard prerequisite of
the initial Linux cut.

## Agentic execution law

Every concrete implementation leaf is representable as:

```text
one proposition / one reviewable PR result
hard vs evidence vs external/authorization dependencies
writer slot + conflict key + allowed/forbidden authority surfaces
claim/evidence-stage ceiling
canonical authorities consumed
first realistic false-green discriminator
positive/opposite/stale/wrong-subject/cleanup controls
spec/test/fixture/schema/docs/generated/receipt obligations
rollback + transfer + return-to-issue + stop conditions
successors/fan-in potentially unblocked
```

Controllers, fan-ins, certifications, external actions, and already-landed
history cannot become ordinary product leaves by title or order. Each leaf
carries a claim ceiling: subject materialization earns no journey, profile, or
support result; profiles earn no host evidence; host journeys earn no public
artifact; public replays earn no registry/docs completion; certification
repairs no product behavior.

## Stable implementation topology and dependency ordering

E00 is this bundle. It unblocks the stable train and every downstream lane.
The ordering below is the durable dependency structure #10918 must preserve
(topology only; it is not a schedule, queue, or live state):

```text
E00  #11716 (this bundle: durable architecture + evidence boundaries)
 ↓
E01  #10918 emacs_train.v1 stable checked topology
 ↓
E01R #11770 semantic revision/impact governance (after stable train,
     before current-tree/context/packet state treats one revision as current)

E02  #11717 checked leaf-spec dispositions fan-in
     ↓ #11751 disposition compiler engine
       ↓ #11752 substrate/adapter spec population
       ↓ #11753 profile/actual-host spec population
       ↓ #11754 root/public-replay/projection spec population
       ↓ #11755 controller/residual/external/maintenance dispositions

E04  #11718 exact-tree context fan-in
     ↓ #11756 emacs_node_context.v1 resolver engine
       ↓ #11757 substrate/adapter/profile/host context population
       ↓ #11758 root/public/projection context population

E06  #11719 builder/reviewer/reconciliation packet adapter
     (#10872/#10881 payloads; no Emacs packet schema)

#10936 dogfood controller
     ↓ #11759 deterministic routing scenarios
       ↓ #11760 real fresh-agent/reviewer cohorts

Subject lane (parallel to control planes after E01):
     #11744 manifest/resolver core + bundled Eglot
       ↓ #11745 external Eglot subjects
       ↓ #11746 external lsp-mode subjects
       ↓ #8755 fan-in (reference only)

Root lane (after subjects, adapters, #11366, #11360/#11361):
     #11747 Eglot stock-root matrix → #11749 Eglot root semantics
     #11748 lsp-mode stock-root matrix → #11750 lsp-mode root semantics
       ↓ #8834 / #8838 fan-ins

#11768 governed host journeys/cell catalog must exist before actual-host
journeys (#8822-#8830) and root-semantics leaves select journey/cell
identities; it creates no host driver or receipt result itself.
```

The control planes (E02/E04/E06) and the implementation lanes (subjects, root,
journeys, profiles, public replay) proceed on their own dependencies; only the
edges above are durable.

## Stable versus mutable information

Durable specs may contain stable issue IDs, semantic component IDs, authority
names, and evidence vocabulary. They must not encode as stable truth: current
main SHA, open PR or branch number, current CI/review colour, active
writer/model/provider, live candidate uniqueness, current upstream `main`,
current release/package subject unless pinned as an exact historical/selected
identity, or wall-clock readiness. Those belong to #10923 exact-tree state,
#10930 live overlay, or exact evidence receipts.

## Current-tree basis (navigation only, not durable authority)

At compilation time the exact tree contains only documentation, policy, and
test-scaffolding surfaces for Emacs: editor setup and client-status
documentation, the lsp-client-support claim-tier policy, and xtask plus
`scripts/test` Emacs run-plan/driver test scaffolding — including the hermetic
`emacs_host_driver.v1` elisp protocol, compat/receipt/docs contract tests, and
sample receipt fixtures. No governed subject manifest, client adapter,
stock-root matrix execution, host-journey catalog, train manifest, or revision
governance exists yet. This bundle therefore describes what the train WILL
build with explicit evidence boundaries; it does not certify any current
capability beyond those surfaces. Exact paths and SHAs are #10923/#11718
navigation evidence, never durable scope authority.

## Alternatives rejected

- **The train manifest as architecture authority:** rejected; `emacs_train.v1`
  would make graph bytes a second authority able to silently amend decisions.
  The graph preserves this bundle; it does not own it.
- **#8734 as the generic process/cleanup authority:** rejected and superseded;
  #11766/#10894 own generic host-run mechanics. #8734 remains the Emacs
  proving consumer with Emacs-specific falsifiers.
- **An Emacs-local receipt/spec/packet ontology:** rejected; #7777/#10527,
  #3983, #10872/#10881 are consumed, never cloned.
- **Giant coding leaves (#8755, #8834, #8838, #11717, #11718, #10936):**
  rejected as single-PR implementations; each is decomposed into one
  engine/manifest PR plus bounded population/execution leaves, with the parent
  retained as fan-in reference.
- **Free-form actual-host receipt truth:** rejected; #11768 governs journey,
  fixture-expectation, cohort-membership, and receipt-cell identities before
  actual-host leaves execute.
- **Silent graph replacement:** rejected; #11770 provides an explicit
  revision/impact plane so material node/authority/dependency movement
  invalidates exactly the affected specs/contexts/packets/proofs.
- **Copying live issue/PR state into durable bytes:** rejected; current
  SHA/PR/check/writer state stays in #10923/#10930 and exact receipts.
- **Optional breadth as initial-cut prerequisite:** rejected; perl-ts-mode,
  TRAMP, managed-install, and non-Linux platforms remain #9310-train options.

## Prior art / duplicates

The prior-art scan found the sibling shared-contract projection
`.spec/10894-editor-host-reliability/` (#11766, PR #11811) — referenced, not
duplicated; this bundle is its Emacs programme consumer. No Emacs
architecture `.spec` bundle exists on main. #3983/#3949 conventions and
`docs/reference/SPEC_TEMPLATE.md` govern this packet's shape. The already
decomposed trains #9310 and #9413 are referenced, not recreated.

## Links

- Issue: [#11716](https://github.com/EffortlessMetrics/perl-lsp-swarm/issues/11716)
- Parent programme: [#7979](https://github.com/EffortlessMetrics/perl-lsp-swarm/issues/7979) / [#8706](https://github.com/EffortlessMetrics/perl-lsp-swarm/issues/8706)
- Stable train consumer: [#10918](https://github.com/EffortlessMetrics/perl-lsp-swarm/issues/10918)
- Shared spec method: [#3983](https://github.com/EffortlessMetrics/perl-lsp-swarm/issues/3983) and [`docs/reference/SPEC_TEMPLATE.md`](../../docs/reference/SPEC_TEMPLATE.md)
- Shared host reliability projection: [#11766](https://github.com/EffortlessMetrics/perl-lsp-swarm/issues/11766), [#10894](https://github.com/EffortlessMetrics/perl-lsp-swarm/issues/10894), `.spec/10894-editor-host-reliability/`
- Generic receipts: [#7777](https://github.com/EffortlessMetrics/perl-lsp-swarm/issues/7777) / [#10527](https://github.com/EffortlessMetrics/perl-lsp-swarm/issues/10527)
- Observation/producer: [#11360](https://github.com/EffortlessMetrics/perl-lsp-swarm/issues/11360) / [#11361](https://github.com/EffortlessMetrics/perl-lsp-swarm/issues/11361)
- Governed journeys: [#11768](https://github.com/EffortlessMetrics/perl-lsp-swarm/issues/11768); revision governance: [#11770](https://github.com/EffortlessMetrics/perl-lsp-swarm/issues/11770)
- Builder/reviewer contracts: [#10872](https://github.com/EffortlessMetrics/perl-lsp-swarm/issues/10872) / [#10881](https://github.com/EffortlessMetrics/perl-lsp-swarm/issues/10881)
- Covered leaves: #11717, #11718, #11719, #11744-#11760, #11768, #11770 (per-leaf boundaries in `acceptance.md`)

## Scope boundary

In scope: this directory's `context.md`, `acceptance.md`, and `checklist.md`.

Out of scope: Emacs/product implementation, `emacs_train.v1` bytes, current-tree
probes, live GitHub observation, product tests, registry mutation, external
submission, CI changes, and any host execution.
