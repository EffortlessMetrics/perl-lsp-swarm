# Release integrity closeout: pre-freeze phase contract

Status: source contract for [#6055](https://github.com/EffortlessMetrics/perl-lsp-swarm/issues/6055).
The named `release_integrity_closeout.v1` aggregate has no admitted executable
implementation or schema in this contract. This document defines its prerequisite
evidence laws for [FF02 #13856](https://github.com/EffortlessMetrics/perl-lsp-swarm/issues/13856);
it establishes no executed integrity result, final freeze, or publication eligibility.

## Governing decisions

- [Accepted pre-freeze versus publication phase decision](https://github.com/EffortlessMetrics/perl-lsp-swarm/issues/6055#issuecomment-5750389870).
- [FF02 producer and phase prerequisites](https://github.com/EffortlessMetrics/perl-lsp-swarm/issues/13856#issuecomment-5750171875).
- [Current source-to-integrity composition](https://github.com/EffortlessMetrics/perl-lsp-swarm/issues/6055#issuecomment-5882087539).
- [Bounded documentation plan](https://github.com/EffortlessMetrics/perl-lsp-swarm/issues/6055#issuecomment-5885966767)
  and [independent BUILD admission](https://github.com/EffortlessMetrics/perl-lsp-swarm/issues/6055#issuecomment-5886096230).

[#13768](https://github.com/EffortlessMetrics/perl-lsp-swarm/issues/13768) owns the
bounded first-RC train. This contract refines the evidence consumed by that train;
it neither expands its channel authorization nor introduces a second human approval
requirement. The aggregate remains named-only in the
[release trust registry](../../policy/release-trust-invariants.v1.json).

## Phase and identity laws

The initial consumer phase is `pre_freeze_product`. Its exact source is the proposed
`frozen_product_sha` in the declared development repository, joined to the accepted
topology and policy identities. It must not fabricate a prepared SHA, a publication
repository candidate, or future artifact bytes. Null preparation remains valid for
the FF02 pre-preparation case. Non-null preparation and its product-inert transition
remain governed by [#4347](https://github.com/EffortlessMetrics/perl-lsp-swarm/issues/4347)
and [#6068](https://github.com/EffortlessMetrics/perl-lsp-swarm/issues/6068).

Repository identity, complete source SHA, topology digest, selected workflow and
policy bytes, producer contract identity, and evidence byte digest are distinct
inputs. A digest establishes byte identity, not successful execution, authenticity,
or policy compliance. Local paths transport bytes; they are not durable subject
identity. Evidence must resolve under an explicitly declared repository context.

A reviewed verifier revision may differ from the evaluated source revision. Its
output must still bind the evaluated source and selected topology. Merely finding
a commit object, sharing ancestry, or pointing to a durable URL does not establish
the required subject or proposition.

The accepted phase policy fixes the evidence denominator independently of the
candidate packet. The packet cannot select a weaker policy, omit an obligation,
change its owner, or make a missing producer optional. Every required obligation
appears exactly once; duplicate and contradictory bindings refuse admission.

## Closed pre-freeze obligations

Every row below is mandatory. The two graph propositions may share evidence only
when the existing producer explicitly proves both. The consumer must reuse the
producer's validator rather than infer success from workflow text or a summary.

| Obligation | Required proposition | Existing owner and admission gap |
| --- | --- | --- |
| `publication_ordering` | Required predecessor success precedes publisher eligibility across the complete selected workflow graph, including dispatched and reusable edges. | [#16762](https://github.com/EffortlessMetrics/perl-lsp-swarm/issues/16762) reconciles [#14479](https://github.com/EffortlessMetrics/perl-lsp-swarm/pull/14479) / [#6231](https://github.com/EffortlessMetrics/perl-lsp-swarm/issues/6231). [#9422](https://github.com/EffortlessMetrics/perl-lsp-swarm/issues/9422) supplies actual failed-predecessor execution. An accepted reusable aggregate adapter is `NOT_PROVEN`; a source graph alone cannot prove zero publisher execution. |
| `credential_isolation` | Publisher credentials and effective mutation authority are absent during checkout, dependency installation, candidate code, build, test, and rehearsal. | #6055 and [#8576](https://github.com/EffortlessMetrics/perl-lsp-swarm/issues/8576) retain the existing security leaves. Exact job/step exposure, effective permissions, and relied-upon settings need a concrete source/execution adapter. A named environment or source declaration alone is insufficient; that aggregate interface is `NOT_PROVEN`. |
| `immutable_inputs` | Selected source, actions, tools, policies, and evidence inputs obey their accepted immutable-input contracts. | #6055 consumes existing leaf owners and validators. The exact selected producer/schema/digest mapping is `NOT_PROVEN`; passing an unrelated pinning or recurrence checker is not complete release composition. |
| `exact_subject_channels` | Source, candidate role, topology, and required/deferred channel membership agree at each selected stage; invalid identities refuse. | [#14406](https://github.com/EffortlessMetrics/perl-lsp-swarm/issues/14406), #16762, and [#6052](https://github.com/EffortlessMetrics/perl-lsp-swarm/issues/6052) own eligibility/topology. Existing [terminal manifest mechanics](../../scripts/release_terminal_manifest.py) are narrower artifact-stage checks. The pre-freeze composition adapter is `NOT_PROVEN`; no version or channel set is chosen here. |
| `nonrecursive_publication` | The selected event/retry/dispatch graph has one publication path and rejects duplicate or recursive publication authority. | #6231/#16762 share the selected graph owner with `publication_ordering`. A concrete adapter must expose this proposition separately; source pattern presence cannot satisfy it. The aggregate join is `NOT_PROVEN`. |
| `no_publish_rejection` | Deliberate rejected mandatory stages have no effective public mutation authority and no topology-required public state change. | #8576 consumes [#9414](https://github.com/EffortlessMetrics/perl-lsp-swarm/issues/9414), [#9416](https://github.com/EffortlessMetrics/perl-lsp-swarm/issues/9416), [#9418](https://github.com/EffortlessMetrics/perl-lsp-swarm/issues/9418), [#9420](https://github.com/EffortlessMetrics/perl-lsp-swarm/issues/9420), #9422, and [#9423](https://github.com/EffortlessMetrics/perl-lsp-swarm/issues/9423). Schema/inventory fixtures are not an executed graph or before/after observation. The executable aggregate interface and exact execution evidence remain `NOT_PROVEN`. |

For each row, implementation admission must identify the concrete existing producer,
its exact schema and validator entry point, evidence bytes/digest, evaluated subject,
terminal success/refusal semantics, and invalidators. A schema name or arbitrary
`{subject, status, digest}` wrapper cannot replace these contracts. Missing adapters
return to their existing owner; FF02 must not implement a competing producer.

Failed, malformed, absent, skipped, cancelled, timed-out, stale, instrument-failed,
cross-subject, or `not_proven` mandatory evidence cannot satisfy a row. A later
successful row cannot compensate. A bounded limitation must narrow an accepted
claim explicitly; it cannot waive a mandatory safety obligation.

## Conditional live controls

A live control is freeze-mandatory exactly when the accepted safety argument relies
on it. The reviewed phase policy must enumerate that dependence, the control's
repository and branch/tag/environment scope, the owning producer, and the compliance
rule. The packet cannot declare an empty dependency list sufficient by itself.

Examples are environment restrictions used for credential isolation, effective token
or fork policy used for mutation refusal, protected branch/tag rules and bypass
posture used for source authority, or immutable releases used for irreversibility.
These are conditional examples, not a universal new freeze checklist. Source
CODEOWNERS coverage and enforced review/bypass behavior are separate propositions.

Calling a depended-upon control publication-only does not waive it. Conversely,
an applicable publication control on which pre-freeze proof does not rely remains
an explicit downstream prerequisite; its absence must not be misrepresented as a
pre-freeze success or silently expand the freeze denominator.

The existing [live-controls module](../../xtask/src/release_live_controls/mod.rs)
provides `observe`, `load_snapshot`, `receipt_verdict`, and
`required_contexts_union`. Its observer records repository identities, protection,
rulesets, environments, release posture, limitations, and instrument outcomes.
These APIs provide observations and consistency checks, not the missing aggregate's
accepted-policy compliance or collector-provenance contract.

`load_snapshot` validates the registered schema identity and structural consistency,
and forces `Currency::Snapshot` regardless of serialized currency. `observe` accepts
injected commands and observation time; serialized `LIVE`, a supplied timestamp,
or a caller-written collection record cannot prove real collection. `OBSERVED`
means values were observed, not that they comply with a release policy. Inaccessible
or instrument-failed controls remain `NOT_PROVEN`.

Offline FF02 may verify a recorded observation and its exact binding only through a
concrete accepted collection-provenance adapter. That adapter is `NOT_PROVEN` here.
It cannot label a replay current at publication. The publication owner must recollect
and evaluate applicable controls immediately before mutation. No TTL, scheduled
refresh, new observer, or settings mutation is introduced by this contract.

Reuse the [existing settings closeout](../security/release-settings-closeout.md)
and its [checker](../../scripts/ci/check_release_settings_closeout.py) under their
existing meanings. Historical subjects, null topology, reviewer expectations, and
channel choices cannot silently become current RC evidence or restore the rejected
second-approval policy. Reconciling that ledger is a separate owner action.

## Downstream obligations and exact joins

Keep the following obligations explicit without treating them as satisfied by a
pre-freeze result:

| Obligation | Existing owner | Boundary |
| --- | --- | --- |
| Applicable publication controls | #6055 | Current candidate and relied-upon settings are evaluated again immediately before mutation. |
| Complete candidate subject material | [#4350](https://github.com/EffortlessMetrics/perl-lsp-swarm/issues/4350), topology and artifact owners | Actual archives, VSIX, checksums, non-empty SBOM/evidence, and applicable attestations bind the complete selected subject set. Pre-freeze proof establishes enforcing mechanics; it cannot fabricate later bytes or attestation execution. |
| Selected-channel public currentness | [#7831](https://github.com/EffortlessMetrics/perl-lsp-swarm/issues/7831) and its channel owners | Required/deferred membership comes from topology. Dispatch success or source workflow existence is not observed public currentness. |
| Public installed verification | [#4351](https://github.com/EffortlessMetrics/perl-lsp-swarm/issues/4351) / #13768 | Independently downloaded public subjects receive the required identity and installed verification. |

Canonical joins must reject duplicates even when their payloads are identical.
Compare validated keys exactly; do not select a winner through input order, case
folding, or lossy coercion. Source obligations use the six fixed keys above. A live
dependency is identified by obligation, repository identity, control, and scope.
Its policy and observation references are load-bearing values, not extra key parts
that permit contradictory duplicates.

Every dependency resolves to exactly one recorded observation whose validated
contents cover the exact repository/control/scope. Several dependencies may share
one observation only when that content proves each proposition. Referential
existence is insufficient. Unused observations cannot pad the denominator with
unrelated green evidence. Missing provenance or content is `NOT_PROVEN`.

Each downstream obligation has a fixed owning role. Channel currentness has one
row per topology-selected public channel, including explicit deferments. Unknown,
duplicate, or omitted selected channels refuse. Non-channel obligations occur once
with their declared phase scope. Limitations are individually nonempty, unique,
and canonically ordered; their list may be empty and changes no mandatory row.

## Derived result and invalidation

A future admitted evaluator must preserve each obligation's evaluated subject and
bounded result. Aggregate precedence is deterministic:

1. Invalid shape, denominator, reference, or contradictory identity produces an
   invalid result before any success verdict.
2. A concrete validated mandatory safety violation produces blocked, while keeping
   other missing evidence visible.
3. Otherwise any unresolved mandatory evidence or adapter produces `NOT_PROVEN`.
4. Only complete validated mandatory and conditional evidence satisfies the
   pre-freeze phase. Publication eligibility remains unevaluated and downstream
   obligations remain explicit.

Input summary status cannot override these derived results. Missing evidence is not
a demonstrated safety violation; a demonstrated violation is not an instrument
failure. Synthetic fixture success tests a validator's law and earns no real-release
integrity or installed-execution claim.

Source/product movement invalidates source-bound integrity and dependent freeze
proof. Changes in topology, channel membership, workflow graph, artifact denominator,
actions/tools, permissions, credentials/environment settings, retry/event routes,
or relied-upon policy invalidate the corresponding evidence. Unrelated settings are
not automatically product-freeze invalidators. Preparation metadata follows its
separate transition law and refreshes affected subjects; ancestry does not preserve
all prior proof automatically.

## Implementation admission and proof

Before an aggregate/schema or FF02 success path is implemented, settle the missing
producer interfaces, collection-provenance contract, reviewed conditional-dependency
projection, and phase-specific artifact/channel joins on their existing owners.
Do not turn this prose inventory into an accepted runtime schema or generic adapter.

The [installed acceptance v2 validator](../../xtask/src/pre_freeze_public_beta_acceptance.rs)
illustrates the same claim boundary: `validate_v2` derives bundle consistency and
external `EvidenceRequirement` obligations, while `InstalledQualification` permits
only `NotProven`. A bundle recommendation of ready must not be promoted to executed
installed acceptance by FF02. Its real evidence adapters remain independently owed.

The future implementation packet must pair positive phase-correct synthetic controls
with semantic refusals for:

- wrong repository/source/topology, missing or duplicate obligations and observations;
- every absent, malformed, unsupported, cross-subject, stale, or non-success mandatory leaf;
- source-only settings impersonating recorded collection, serialized LIVE replay,
  injected collector/time, instrument failure, and observed-but-noncompliant values;
- a relied-upon live control omitted or incorrectly moved downstream;
- publication-only evidence used to compensate for missing pre-freeze rejection proof;
- omitted required channels, arbitrary pass wrappers, and dispatch relabeled public currentness;
- changed source/policy/control with stale dependent proof and nondeterministic results
  for identical immutable inputs.

Also prove that a genuinely independent publication-only gap stays downstream while
publication eligibility remains unevaluated. Exercise each mutation through semantic
validation, not merely incidental JSON rejection. Exact executable commands and
schema paths belong to that separately admitted implementation; none is claimed to
validate the named-only aggregate today.

This source-contract slice adds no runtime/schema/policy implementation, collector,
publisher, credentials access, workflow dispatch, artifact build, version selection,
preparation, sync, freeze, tag, or public mutation. #6055 remains open for executable
composition and actual evidence; #13856 remains accountable for the bounded semantic
consumer once its prerequisites are concrete.
