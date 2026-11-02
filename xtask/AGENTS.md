# xtask agent guidance

`xtask` owns development automation, CI gates, authority-transfer closeout, and
writer preflight admission. Read the crate-local `CLAUDE.md` for commands and
module layout.

## Safety-bearing boolean evidence (#14304)

**Principle:** Every self-declared boolean in a safety-bearing document must
have either an independent evidence path or a documented exemption. See
[PLSP-ADR-0009](../docs/adr/PLSP-ADR-0009-safety-bearing-boolean-evidence.md).

**Review obligation:** Do not merge changes that add load-bearing `bool` fields
to closeout or preflight documents without binding them to independent evidence
or documenting why syntactic validation suffices.

**Pinned regressions** (truth value checked against evidence, not caller intent):

| Instance | Proof |
|----------|-------|
| Writer preflight empty worktree map | `writer_preflight::tests::resume_or_mutate_with_unregistered_candidate_refuses` |
| `WorkingTreeDisposition` without `Default` | `safety_bearing_boolean_contract` + type comment in `observation.rs` |
| Closeout control head alignment | `authority_transfer_closeout::tests::controls_executed_elsewhere_cannot_claim_subject_alignment` |
| Closeout digest binds | `evaluated_profile_must_be_the_digest_bound_artifact`, `bind_artifacts` tests |

Open closeout gaps: [#14294](https://github.com/EffortlessMetrics/perl-lsp-swarm/issues/14294).
