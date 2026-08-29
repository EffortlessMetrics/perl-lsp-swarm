use std::collections::HashSet;

use crate::tasks::ci_scope::ScopeOutput;
use xtask::ci_route_plan::{
    ExpansionStatus, GateSelectorInput, LifecycleDisposition, LifecycleState, PolicyRole,
    Resolution, RouteDispositionInput, RouteExecutionIdentity, RouteProfileExpansionInput,
    RouteQuarantineEvidence, RouteScopeEvidence, RouteSelectionEvidence, ScopedIdentity,
    SelectorPlacement, SelectorProof, SelectorRole,
};

use super::disposition::{
    DispositionAuthority, DispositionResolution, GateLifecycle, GatePolicyRole,
};
use super::route_profile::{ExpansionResolution, ProfileExpansion};
use super::{GateDefinition, GatePlanningRole, GatePolicy, GateTier};

#[derive(Debug, Clone)]
pub(super) struct GatePlan {
    pub(super) tier: GateTier,
    pub(super) base: String,
    pub(super) scope: Option<ScopeOutput>,
    pub(super) scope_ok: bool,
    pub(super) fallback_used: bool,
    pub(super) fallback_reason: Option<String>,
    pub(super) package_args: Vec<String>,
    pub(super) selected: Vec<PlannedGate>,
    pub(super) skipped: Vec<SkippedGate>,
    /// `git write-tree` OID of the staged tree this plan was built against
    /// (issue #3786). `Some` only when the run was invoked with `--staged`;
    /// carried through to `AgentReceipt.staged_tree_oid` in the action
    /// packet.
    pub(super) staged_tree_oid: Option<String>,
}

// ---------------------------------------------------------------------------
// Route-plan adapters (#9148 domain seam)
//
// These adapters project resolved authority results (`ci_route_profile.v1`
// from #10178, `gate_disposition.v1` from #10176) and runner planning facts
// into the lib-side `ci_route_plan.v1` compile inputs. No derivation that
// belongs to an authority happens here: profile membership, lifecycle,
// quarantine evidence, and policy role are projected 1:1 from the authority
// results.
// ---------------------------------------------------------------------------

/// Project a `ci_route_profile.v1` expansion result 1:1. Non-complete
/// resolutions are projected faithfully (with their closed detail) so the
/// domain compile fails closed on them; membership is never recomputed.
#[allow(dead_code)] // consumer seam: #10179 CLI publication
pub(super) fn route_profile_expansion_input(
    expansion: &ProfileExpansion,
) -> RouteProfileExpansionInput {
    RouteProfileExpansionInput {
        requested_profile: expansion.requested_profile.as_str().to_string(),
        included_native_tiers: expansion.included_native_tiers.clone(),
        semantic_fingerprint: expansion.semantic_fingerprint.clone(),
        policy_digest: expansion.policy_digest.clone(),
        denominator: expansion.denominator.clone(),
        resolution: match expansion.resolution {
            ExpansionResolution::Complete => ExpansionStatus::Complete,
            ExpansionResolution::Unsupported => ExpansionStatus::Unsupported,
            ExpansionResolution::Invalid => ExpansionStatus::Invalid,
        },
        detail: expansion.detail.clone(),
    }
}

/// Project a resolved `gate_disposition.v1` authority into domain rows.
/// Refuses authorities with unattributable quarantine-source rows: they have
/// no per-gate representation and must not be silently dropped.
#[allow(dead_code)] // consumer seam: #10179 CLI publication
pub(super) fn route_disposition_inputs(
    authority: &DispositionAuthority,
) -> Result<(Vec<RouteDispositionInput>, String), String> {
    if !authority.unknown_ledger_entries.is_empty() {
        return Err(format!(
            "gate_disposition.v1 authority has unattributable quarantine sources: {}",
            authority.unknown_ledger_entries.join(", ")
        ));
    }
    let rows = authority
        .rows
        .iter()
        .map(|row| RouteDispositionInput {
            gate_id: row.gate_id.clone(),
            policy_role: row.policy_role.into(),
            lifecycle: LifecycleDisposition {
                state: row.lifecycle.into(),
                resolution: row.resolution.into(),
            },
            native_tier: row.intended_profile.clone(),
            quarantine: row.quarantine.as_ref().map(|evidence| RouteQuarantineEvidence {
                owner: evidence.owner.clone(),
                owner_issue: evidence.owner_issue.clone(),
                reason_token: evidence.reason_token.clone(),
                review_after: evidence.review_after.format("%Y-%m-%d").to_string(),
            }),
            detail: row.detail.clone(),
        })
        .collect();
    Ok((rows, authority.semantic_digest.clone()))
}

/// Project the runner gate rows into executable identities for the domain.
#[allow(dead_code)] // consumer seam: #10179 CLI publication
pub(super) fn route_execution_identities(policy: &GatePolicy) -> Vec<RouteExecutionIdentity> {
    policy
        .gates
        .iter()
        .map(|gate| RouteExecutionIdentity {
            gate_id: gate.name.clone(),
            command: gate.command.clone(),
            timeout_seconds: gate.timeout_seconds,
        })
        .collect()
}

/// Project the runner's gate populations into typed selector facts.
///
/// A positive proof is projected only where today's selector actually
/// decided: always-on/tier-static/fallback roles apply by their own
/// contract, and the scope selector proves (non-)applicability for scoped
/// roles when it ran to completion. A fallback decision carries no positive
/// proof — the selection evidence records the fallback honestly instead.
#[allow(dead_code)] // consumer seam: #10179 CLI publication / #9149 selector compiler
pub(super) fn route_selector_inputs(plan: &GatePlan) -> Vec<GateSelectorInput> {
    let mut inputs: Vec<GateSelectorInput> = plan
        .selected
        .iter()
        .map(|planned| GateSelectorInput {
            gate_id: planned.gate.name.clone(),
            placement: SelectorPlacement::Selected,
            role: Some(planned.role.into()),
            reason: planned.reason.clone(),
            proof: selected_proof(planned.role, plan.fallback_used),
        })
        .collect();
    inputs.extend(plan.skipped.iter().map(|skipped| GateSelectorInput {
        gate_id: skipped.name.clone(),
        placement: SelectorPlacement::Skipped,
        role: skipped.role.map(Into::into),
        reason: skipped.reason.clone(),
        proof: skipped_proof(skipped.role, plan.fallback_used),
    }));
    inputs
}

/// A selection is positively applicable when the planning role applies by
/// its own contract (always-on or tier-static — these apply to every
/// subject by policy) or when the scope selector selected the gate without
/// fallback. A fallback firing is a policy decision, not an exact-subject
/// selector decision: it projects no positive proof, so the domain
/// compiles the row as a typed `selector_evidence_missing` error rather
/// than a proof-backed run.
fn selected_proof(role: GatePlanningRole, fallback_used: bool) -> Option<SelectorProof> {
    match role {
        GatePlanningRole::AlwaysOn | GatePlanningRole::Static => Some(SelectorProof::Applicable),
        GatePlanningRole::RustFallback
        | GatePlanningRole::RustScoped
        | GatePlanningRole::RustPackageScoped
            if !fallback_used =>
        {
            Some(SelectorProof::Applicable)
        }
        GatePlanningRole::RustFallback
        | GatePlanningRole::RustScoped
        | GatePlanningRole::RustPackageScoped => None,
    }
}

/// A skip is positively non-applicable only when the scope selector (not a
/// fallback policy) decided it for a scoped role.
fn skipped_proof(role: Option<GatePlanningRole>, fallback_used: bool) -> Option<SelectorProof> {
    match role {
        Some(
            GatePlanningRole::RustScoped
            | GatePlanningRole::RustFallback
            | GatePlanningRole::RustPackageScoped,
        ) if !fallback_used => Some(SelectorProof::NotApplicableToSubject),
        _ => None,
    }
}

/// Project the runner's selection evidence. Scope identity lists are
/// canonicalized here (the adapter owns data replication; the domain
/// validator rejects non-canonical input instead of silently repairing it).
#[allow(dead_code)] // consumer seam: #10179 CLI publication
pub(super) fn route_selection_evidence(
    plan: &GatePlan,
    selector_digest: String,
) -> RouteSelectionEvidence {
    let scope = plan.scope.as_ref().map(|scope| {
        let mut evidence = RouteScopeEvidence {
            head_sha: scope.head_sha.clone(),
            diff_class: scope.diff_class.clone(),
            direct_crates: scope
                .direct_crates
                .iter()
                .map(|row| ScopedIdentity { name: row.name.clone(), reason: row.reason.clone() })
                .collect(),
            reverse_dependencies: scope
                .reverse_dep_closure
                .iter()
                .map(|row| ScopedIdentity { name: row.name.clone(), reason: row.reason.clone() })
                .collect(),
            architecture_wideners: scope
                .architecture_wideners
                .iter()
                .map(|row| ScopedIdentity { name: row.name.clone(), reason: row.rule.clone() })
                .collect(),
            risk_tags: scope.risk_tags.clone(),
        };
        evidence.direct_crates.sort();
        evidence.direct_crates.dedup();
        evidence.reverse_dependencies.sort();
        evidence.reverse_dependencies.dedup();
        evidence.architecture_wideners.sort();
        evidence.architecture_wideners.dedup();
        evidence.risk_tags.sort();
        evidence.risk_tags.dedup();
        evidence
    });
    RouteSelectionEvidence {
        base: plan.base.clone(),
        scope_ok: plan.scope_ok,
        fallback_used: plan.fallback_used,
        fallback_reason: plan.fallback_reason.clone(),
        package_args: plan.package_args.clone(),
        scope,
        selector_digest,
    }
}

impl From<GatePolicyRole> for PolicyRole {
    fn from(role: GatePolicyRole) -> Self {
        match role {
            GatePolicyRole::Required => PolicyRole::Required,
            GatePolicyRole::Advisory => PolicyRole::Advisory,
        }
    }
}

impl From<GateLifecycle> for LifecycleState {
    fn from(state: GateLifecycle) -> Self {
        match state {
            GateLifecycle::Active => LifecycleState::Active,
            GateLifecycle::Dormant => LifecycleState::Dormant,
            GateLifecycle::Quarantined => LifecycleState::Quarantined,
            GateLifecycle::Retired => LifecycleState::Retired,
            GateLifecycle::Blocked => LifecycleState::Blocked,
        }
    }
}

impl From<DispositionResolution> for Resolution {
    fn from(resolution: DispositionResolution) -> Self {
        match resolution {
            DispositionResolution::Current => Resolution::Current,
            DispositionResolution::Expired => Resolution::Expired,
            DispositionResolution::Invalid => Resolution::Invalid,
        }
    }
}

impl From<GatePlanningRole> for SelectorRole {
    fn from(role: GatePlanningRole) -> Self {
        match role {
            GatePlanningRole::AlwaysOn => Self::AlwaysOn,
            GatePlanningRole::RustScoped => Self::RustScoped,
            GatePlanningRole::RustFallback => Self::RustFallback,
            GatePlanningRole::RustPackageScoped => Self::RustPackageScoped,
            GatePlanningRole::Static => Self::Static,
        }
    }
}

#[derive(Debug, Clone)]
pub(super) struct PlannedGate {
    pub(super) gate: GateDefinition,
    pub(super) role: GatePlanningRole,
    pub(super) reason: String,
}

#[derive(Debug, Clone)]
pub(super) struct SkippedGate {
    pub(super) name: String,
    pub(super) role: Option<GatePlanningRole>,
    pub(super) reason: String,
}

#[derive(Debug, Clone, Default)]
pub(super) struct PackageTargetIndex {
    pub(super) lib_packages: HashSet<String>,
}

impl PackageTargetIndex {
    pub(super) fn has_lib(&self, package: &str) -> bool {
        self.lib_packages.contains(package)
    }
}

// ---------------------------------------------------------------------------
// Authority-consumption seam tests (#10176/#10178 -> #9148 domain)
// ---------------------------------------------------------------------------

#[cfg(test)]
mod route_plan_seam_tests {
    use super::*;
    use crate::tasks::gates::disposition::{LedgerQuarantineEntry, QuarantineLedger, resolve_from};
    use crate::tasks::gates::route_profile::{RequestedProfile, expand};
    use crate::tasks::gates::{GlobalSettings, TierDefinition};
    use chrono::NaiveDate;
    use std::collections::HashMap;
    use xtask::ci_route_plan::{
        Applicability, CiRoutePlanV1, CompileRoutePlanInput, PlannedOutcome, RouteSubjectRef,
    };

    pub(super) const TODAY: NaiveDate = NaiveDate::from_ymd_opt(2026, 8, 24).unwrap();
    pub(super) const SHA: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
    pub(super) const DIGEST: &str =
        "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

    pub(super) fn gate(name: &str, tier: &str, required: bool, quarantine: bool) -> GateDefinition {
        GateDefinition {
            name: name.to_string(),
            tier: tier.to_string(),
            description: name.to_string(),
            required,
            command: format!("run {name}"),
            timeout_seconds: 60,
            retry_count: 0,
            budgets: None,
            quarantine,
            tags: Vec::new(),
            artifacts: Vec::new(),
            matrix: None,
            planning: None,
        }
    }

    pub(super) fn policy(gates: Vec<GateDefinition>) -> GatePolicy {
        let tier = |name: &str| {
            (
                name.to_string(),
                TierDefinition {
                    description: name.to_string(),
                    target_duration_seconds: 120,
                    enforcement: "pr".to_string(),
                    trigger: Vec::new(),
                },
            )
        };
        GatePolicy {
            schema_version: 1,
            global: GlobalSettings {
                default_timeout_seconds: 60,
                artifact_retention_days: 0,
                default_retry_count: 0,
                environment: HashMap::new(),
                toolchain: None,
            },
            tiers: HashMap::from([tier("pr_fast"), tier("merge_gate")]),
            gates,
            flake_policy: None,
            audit: None,
        }
    }

    fn ledger_row(
        name: &str,
        owner: Option<&str>,
        issue: Option<&str>,
        expires: Option<&str>,
    ) -> LedgerQuarantineEntry {
        LedgerQuarantineEntry {
            name: Some(name.to_string()),
            added: Some("2026-01-01".to_string()),
            issue: issue.map(str::to_string),
            tier: Some("quarantine".to_string()),
            quarantine_days: None,
            expires: expires.map(str::to_string),
            owner: owner.map(str::to_string),
            notes: None,
            failure_pattern: Some("secondary_failure".to_string()),
        }
    }

    fn compile_input(
        policy: &GatePolicy,
        expansion: &ProfileExpansion,
        authority: &DispositionAuthority,
        selectors: Vec<GateSelectorInput>,
    ) -> CompileRoutePlanInput {
        let (dispositions, disposition_digest) =
            route_disposition_inputs(authority).unwrap_or_else(|error| panic!("{error}"));
        CompileRoutePlanInput {
            subject: RouteSubjectRef {
                kind: "pull_request".to_string(),
                head_sha: SHA.to_string(),
                base_sha: None,
                subject_digest: DIGEST.to_string(),
            },
            expansion: route_profile_expansion_input(expansion),
            dispositions,
            disposition_digest,
            workflow_digest: DIGEST.to_string(),
            selectors,
            selection: RouteSelectionEvidence {
                base: SHA.to_string(),
                scope_ok: true,
                fallback_used: false,
                fallback_reason: None,
                package_args: Vec::new(),
                scope: None,
                selector_digest: DIGEST.to_string(),
            },
            execution: route_execution_identities(policy),
        }
    }

    fn selected(name: &str, role: GatePlanningRole, reason: &str) -> GateSelectorInput {
        GateSelectorInput {
            gate_id: name.to_string(),
            placement: SelectorPlacement::Selected,
            role: Some(role.into()),
            reason: reason.to_string(),
            proof: Some(SelectorProof::Applicable),
        }
    }

    fn skipped(name: &str, reason: &str, proof: Option<SelectorProof>) -> GateSelectorInput {
        GateSelectorInput {
            gate_id: name.to_string(),
            placement: SelectorPlacement::Skipped,
            role: Some(SelectorRole::RustScoped),
            reason: reason.to_string(),
            proof,
        }
    }

    /// Real authorities (#10178 `expand` + #10176 `resolve_from`) projected
    /// through the adapters and compiled: a current quarantine that the
    /// runner placed in the skipped population stays `quarantined` with its
    /// #10176 owner/reason/review identity — never `scoped_noop`.
    #[test]
    fn current_quarantine_skipped_by_runner_stays_quarantined() {
        let policy = policy(vec![gate("sec_gate", "pr_fast", false, true)]);
        let ledger = QuarantineLedger {
            schema_version: 1,
            flaky_tests: vec![ledger_row(
                "sec_gate",
                Some("ci-owner"),
                Some("10176"),
                Some("2030-01-01"),
            )],
        };

        let expansion = expand(&policy, RequestedProfile::PrFast, None);
        let authority = resolve_from(&policy, Some(&ledger), TODAY);
        let input = compile_input(
            &policy,
            &expansion,
            &authority,
            vec![skipped(
                "sec_gate",
                "selector says not applicable",
                Some(SelectorProof::NotApplicableToSubject),
            )],
        );
        let plan = CiRoutePlanV1::compile(input).expect("compile");
        assert_eq!(plan.summary.quarantined, 1);
        assert_eq!(plan.summary.scoped_noop, 0);
        let row = &plan.rows[0];
        assert_eq!(row.applicability, Applicability::NotApplicable);
        match &row.outcome {
            PlannedOutcome::Quarantined { owner, owner_issue, review_after, .. } => {
                assert_eq!(owner, "ci-owner");
                assert_eq!(owner_issue.as_deref(), Some("10176"));
                assert_eq!(review_after, "2030-01-01");
            }
            other => panic!("expected quarantined outcome, got {other:?}"),
        }
    }

    /// Wake falsifier from #9148: an invalid lifecycle (ownerless quarantine
    /// evidence) plus a selector that reports non-applicability must still
    /// produce `error` — removing the lifecycle lookup would silently turn
    /// this row into `scoped_noop` and fail this test.
    #[test]
    fn invalid_lifecycle_with_selector_noop_is_error_not_scoped_noop() {
        let policy = policy(vec![gate("sec_gate", "pr_fast", false, true)]);
        // Ownerless + issueless ledger row: typed Invalid by #10176.
        let ledger = QuarantineLedger {
            schema_version: 1,
            flaky_tests: vec![ledger_row("sec_gate", None, None, Some("2027-01-01"))],
        };

        let expansion = expand(&policy, RequestedProfile::PrFast, None);
        let authority = resolve_from(&policy, Some(&ledger), TODAY);
        assert_eq!(
            authority.get("sec_gate").map(|row| row.resolution),
            Some(DispositionResolution::Invalid)
        );

        let input = compile_input(
            &policy,
            &expansion,
            &authority,
            vec![skipped(
                "sec_gate",
                "selector says not applicable",
                Some(SelectorProof::NotApplicableToSubject),
            )],
        );
        let plan = CiRoutePlanV1::compile(input).expect("compile");
        match &plan.rows[0].outcome {
            PlannedOutcome::Error { code, .. } => assert_eq!(code, "disposition_invalid"),
            other => panic!("expected error outcome, got {other:?}"),
        }
        assert_eq!(plan.summary.scoped_noop, 0);
        assert_eq!(plan.summary.error, 1);
    }

    /// Expired quarantine evidence stays action-required even when the
    /// selector proves non-applicability.
    #[test]
    fn expired_quarantine_with_selector_noop_is_error() {
        let policy = policy(vec![gate("sec_gate", "pr_fast", false, true)]);
        let ledger = QuarantineLedger {
            schema_version: 1,
            flaky_tests: vec![ledger_row(
                "sec_gate",
                Some("ci-owner"),
                Some("10176"),
                Some("2026-05-26"),
            )],
        };

        let expansion = expand(&policy, RequestedProfile::PrFast, None);
        let authority = resolve_from(&policy, Some(&ledger), TODAY);
        assert_eq!(
            authority.get("sec_gate").map(|row| row.resolution),
            Some(DispositionResolution::Expired)
        );

        let input = compile_input(
            &policy,
            &expansion,
            &authority,
            vec![skipped(
                "sec_gate",
                "selector says not applicable",
                Some(SelectorProof::NotApplicableToSubject),
            )],
        );
        let plan = CiRoutePlanV1::compile(input).expect("compile");
        match &plan.rows[0].outcome {
            PlannedOutcome::Error { code, .. } => assert_eq!(code, "disposition_expired"),
            other => panic!("expected error outcome, got {other:?}"),
        }
    }

    /// The authority with unattributable quarantine sources must not be
    /// projected at all — the adapter refuses instead of dropping rows.
    #[test]
    fn unattributable_quarantine_sources_refuse_projection() {
        let policy = policy(vec![gate("fmt_gate", "pr_fast", true, false)]);
        let ledger = QuarantineLedger {
            schema_version: 1,
            flaky_tests: vec![LedgerQuarantineEntry {
                name: None,
                added: None,
                issue: None,
                tier: None,
                quarantine_days: None,
                expires: None,
                owner: None,
                notes: None,
                failure_pattern: None,
            }],
        };
        let authority = resolve_from(&policy, Some(&ledger), TODAY);
        let error = route_disposition_inputs(&authority).expect_err("must refuse");
        assert!(error.contains("unattributable"));
    }

    /// Policy role is projected from the authority and is identical across
    /// requested profiles; an aggregate profile inherits the pr_fast gate.
    #[test]
    fn policy_role_is_stable_across_profiles_and_aggregates_inherit() {
        let policy = policy(vec![
            gate("fmt_gate", "pr_fast", true, false),
            gate("static_gate", "merge_gate", false, false),
        ]);
        let pr_fast = expand(&policy, RequestedProfile::PrFast, None);
        let merge_gate = expand(&policy, RequestedProfile::MergeGate, None);
        let authority = resolve_from(&policy, None, TODAY);

        let fmt_only = CiRoutePlanV1::compile(compile_input(
            &policy,
            &pr_fast,
            &authority,
            vec![selected("fmt_gate", GatePlanningRole::AlwaysOn, "always on")],
        ))
        .expect("pr_fast plan");
        let both = CiRoutePlanV1::compile(compile_input(
            &policy,
            &merge_gate,
            &authority,
            vec![
                selected("fmt_gate", GatePlanningRole::AlwaysOn, "always on"),
                selected("static_gate", GatePlanningRole::Static, "tier static"),
            ],
        ))
        .expect("merge_gate plan");

        let role_of = |plan: &CiRoutePlanV1, id: &str| {
            plan.rows.iter().find(|row| row.gate_id == id).map(|row| row.policy_role)
        };
        assert_eq!(role_of(&fmt_only, "fmt_gate"), Some(PolicyRole::Required));
        // The merge_gate expansion inherits the pr_fast gate (no tier-string
        // equality): both gates appear with stable roles.
        assert_eq!(both.denominator.len(), 2);
        assert_eq!(role_of(&both, "fmt_gate"), Some(PolicyRole::Required));
        assert_eq!(role_of(&both, "static_gate"), Some(PolicyRole::Advisory));
    }

    fn find<'a>(inputs: &'a [GateSelectorInput], id: &str) -> Option<&'a GateSelectorInput> {
        inputs.iter().find(|input| input.gate_id == id)
    }

    /// Runner adapter honesty: a fallback decision carries no positive
    /// selector proof; a scope-decided skip does.
    #[test]
    fn adapter_projects_positive_proof_only_where_the_selector_decided() {
        let mut plan = GatePlan {
            tier: GateTier::PrFast,
            base: SHA.to_string(),
            scope: None,
            scope_ok: true,
            fallback_used: false,
            fallback_reason: None,
            package_args: Vec::new(),
            selected: vec![PlannedGate {
                gate: gate("unit_scoped", "pr_fast", true, false),
                role: GatePlanningRole::RustScoped,
                reason: "code diff selected packages".to_string(),
            }],
            skipped: vec![SkippedGate {
                name: "lint_scoped".to_string(),
                role: Some(GatePlanningRole::RustScoped),
                reason: "no affected Rust package".to_string(),
            }],
            staged_tree_oid: None,
        };
        let inputs = route_selector_inputs(&plan);
        assert_eq!(
            find(&inputs, "unit_scoped").map(|input| input.proof),
            Some(Some(SelectorProof::Applicable))
        );
        assert_eq!(
            find(&inputs, "lint_scoped").map(|input| input.proof),
            Some(Some(SelectorProof::NotApplicableToSubject))
        );

        // Under fallback: a fallback-selected scoped gate, a
        // fallback-role gate, and a fallback skip all lose their positive
        // proof — a fallback firing is a policy decision, not an
        // exact-subject selector decision.
        plan.fallback_used = true;
        plan.fallback_reason = Some("scope unavailable".to_string());
        plan.selected.clear();
        plan.skipped.push(SkippedGate {
            name: "unit_scoped".to_string(),
            role: Some(GatePlanningRole::RustScoped),
            reason: "rust fallback selected".to_string(),
        });
        let inputs = route_selector_inputs(&plan);
        assert_eq!(
            find(&inputs, "unit_scoped").map(|input| input.proof),
            Some(None),
            "fallback decisions carry no positive selector proof"
        );

        // The fallback role itself, selected by the scope selector without
        // a fallback firing, keeps its positive proof.
        let mut no_fallback = plan.clone();
        no_fallback.fallback_used = false;
        no_fallback.fallback_reason = None;
        no_fallback.selected = vec![PlannedGate {
            gate: gate("rust_fallback_gate", "pr_fast", true, false),
            role: GatePlanningRole::RustFallback,
            reason: "scope selected the fallback gate".to_string(),
        }];
        no_fallback.skipped.clear();
        let inputs = route_selector_inputs(&no_fallback);
        assert_eq!(
            find(&inputs, "rust_fallback_gate").map(|input| input.proof),
            Some(Some(SelectorProof::Applicable))
        );

        // A fallback firing selects the fallback-role gate without proof.
        let mut fired = no_fallback.clone();
        fired.fallback_used = true;
        fired.fallback_reason = Some("scope unavailable".to_string());
        fired.selected[0].reason = "rust fallback selected".to_string();
        let inputs = route_selector_inputs(&fired);
        assert_eq!(
            find(&inputs, "rust_fallback_gate").map(|input| input.proof),
            Some(None),
            "a fallback firing carries no positive selector proof"
        );
    }

    /// A fallback-fired selection never becomes a proof-backed `run`: the
    /// row compiles as a typed `selector_evidence_missing` error, keeping
    /// exact-subject proof and policy fallback distinct.
    #[test]
    fn fallback_fired_selection_never_becomes_a_proof_backed_run() {
        let policy = policy(vec![gate("rust_fallback_gate", "pr_fast", true, false)]);
        let expansion = expand(&policy, RequestedProfile::PrFast, None);
        let authority = resolve_from(&policy, None, TODAY);

        // The adapter projects a fallback-fired RustFallback selection
        // with no positive proof.
        let plan = GatePlan {
            tier: GateTier::PrFast,
            base: SHA.to_string(),
            scope: None,
            scope_ok: false,
            fallback_used: true,
            fallback_reason: Some("scope unavailable".to_string()),
            package_args: Vec::new(),
            selected: vec![PlannedGate {
                gate: gate("rust_fallback_gate", "pr_fast", true, false),
                role: GatePlanningRole::RustFallback,
                reason: "rust fallback selected".to_string(),
            }],
            skipped: vec![],
            staged_tree_oid: None,
        };
        let selector = route_selector_inputs(&plan).into_iter().next().expect("one selector input");
        assert_eq!(selector.proof, None);

        let input = compile_input(&policy, &expansion, &authority, vec![selector]);
        let compiled = CiRoutePlanV1::compile(input).expect("compile");
        assert_eq!(compiled.summary.run, 0);
        assert_eq!(compiled.summary.error, 1);
        match &compiled.rows[0].outcome {
            PlannedOutcome::Error { code, .. } => {
                assert_eq!(code, "selector_evidence_missing");
            }
            other => panic!("expected typed error outcome, got {other:?}"),
        }

        // Contrast: the same gate selected by the scope selector (no
        // fallback) is a proof-backed run.
        let selected_proof_input = compile_input(
            &policy,
            &expansion,
            &authority,
            vec![selected("rust_fallback_gate", GatePlanningRole::RustFallback, "scope decided")],
        );
        let compiled = CiRoutePlanV1::compile(selected_proof_input).expect("compile");
        assert_eq!(compiled.summary.run, 1);
        assert!(compiled.rows[0].applicability == Applicability::Applicable);
    }
}

/// #10179 canonical-publication seam: adapter outputs must survive the
/// full publication pipeline — compile, canonical encode, JSON handoff,
/// parse, validate, and byte-identical re-encode — against the domain
/// validator's actual behavior. This is the seam the `ci-route-plan` CLI
/// consumes; it does not re-test adapter projection semantics (those are
/// `route_plan_seam_tests` above).
#[cfg(test)]
mod route_plan_canonical_seam_tests {
    use super::route_plan_seam_tests::{DIGEST, SHA, TODAY, gate, policy};
    use super::*;
    use crate::tasks::gates::disposition::resolve_from;
    use crate::tasks::gates::route_profile::{RequestedProfile, expand};
    use xtask::ci_route_plan::{
        CiRoutePlanV1, CompileRoutePlanInput, RouteSelectionEvidence, RouteSubjectRef,
    };

    /// One proof-backed run and one positive scoped noop, projected from
    /// the real #10178 expander and #10176 resolver through the adapters.
    fn canonical_pipeline_input() -> CompileRoutePlanInput {
        let policy = policy(vec![
            gate("fmt_gate", "pr_fast", true, false),
            gate("scope_gate", "pr_fast", false, false),
        ]);
        let expansion = expand(&policy, RequestedProfile::PrFast, None);
        let authority = resolve_from(&policy, None, TODAY);
        let (dispositions, disposition_digest) = route_disposition_inputs(&authority)
            .unwrap_or_else(|error| panic!("authority projects: {error}"));
        CompileRoutePlanInput {
            subject: RouteSubjectRef {
                kind: "pull_request".to_string(),
                head_sha: SHA.to_string(),
                base_sha: None,
                subject_digest: DIGEST.to_string(),
            },
            expansion: route_profile_expansion_input(&expansion),
            dispositions,
            disposition_digest,
            workflow_digest: DIGEST.to_string(),
            selectors: vec![
                GateSelectorInput {
                    gate_id: "fmt_gate".to_string(),
                    placement: SelectorPlacement::Selected,
                    role: Some(GatePlanningRole::AlwaysOn.into()),
                    reason: "always on".to_string(),
                    proof: Some(SelectorProof::Applicable),
                },
                GateSelectorInput {
                    gate_id: "scope_gate".to_string(),
                    placement: SelectorPlacement::Skipped,
                    role: Some(GatePlanningRole::RustScoped.into()),
                    reason: "scope selector decided".to_string(),
                    proof: Some(SelectorProof::NotApplicableToSubject),
                },
            ],
            selection: RouteSelectionEvidence {
                base: SHA.to_string(),
                scope_ok: true,
                fallback_used: false,
                fallback_reason: None,
                package_args: Vec::new(),
                scope: None,
                selector_digest: DIGEST.to_string(),
            },
            execution: route_execution_identities(&policy),
        }
    }

    /// Adapter outputs round-trip through the canonical publication
    /// pipeline without reinterpretation: the published bytes reparse,
    /// revalidate, and re-encode identically.
    #[test]
    fn adapter_output_round_trips_canonical_publication() {
        let input = canonical_pipeline_input();
        let compiled = CiRoutePlanV1::compile(input).expect("adapter output compiles");
        let bytes = compiled.canonical_json().expect("canonical bytes");
        let reparsed: CiRoutePlanV1 = serde_json::from_slice(&bytes).expect("bytes reparse");
        reparsed.validate().expect("reparsed plan validates against the domain validator");
        assert_eq!(reparsed.canonical_json().expect("re-encode"), bytes);
    }

    /// The CLI handoff shape: serializing the adapter-produced compile
    /// input to JSON and reparsing it (what `ci-route-plan compile` does
    /// with the input file) preserves the semantic fingerprint and bytes.
    #[test]
    fn adapter_input_survives_the_cli_json_handoff() {
        let input = canonical_pipeline_input();
        let direct = CiRoutePlanV1::compile(input.clone()).expect("direct compile");
        let json = serde_json::to_vec(&input).expect("serialize compile input");
        let handed_off: CompileRoutePlanInput =
            serde_json::from_slice(&json).expect("compile input reparses");
        let through_handoff = CiRoutePlanV1::compile(handed_off).expect("handoff compile");
        assert_eq!(through_handoff.semantic_fingerprint, direct.semantic_fingerprint);
        assert_eq!(
            through_handoff.canonical_json().expect("bytes"),
            direct.canonical_json().expect("bytes")
        );
    }
}
