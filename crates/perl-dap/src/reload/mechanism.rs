//! Reload mechanism record: comparative limitation statements only.
//!
//! Live measurement belongs to #10098's harness. This record states, for
//! each candidate mechanism, what it can and cannot prove today. Two laws
//! are absolute here: compile success is never reload success, and no
//! external module becomes product authority merely by being available
//! (Class::Refresh is a measured compatibility subject, never a bundled
//! dependency).

/// The candidate mechanism families the contract compared.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ReloadMechanism {
    /// `delete $INC{...}; require ...` directly in the debuggee.
    IncDeletionAndRequire,
    /// A `do`/`require`-based helper with explicit package handling.
    DoOrRequireHelper,
    /// A small workspace-owned runtime helper/observer injected under its
    /// own reviewed authority.
    WorkspaceRuntimeHelperObserver,
    /// An established external module (Class::Refresh) — a measured
    /// compatibility subject only, never an automatic bundled dependency.
    ClassRefreshCompatibilitySubject,
}

impl ReloadMechanism {
    /// All compared mechanisms in frozen order.
    pub const ALL: [ReloadMechanism; 4] = [
        ReloadMechanism::IncDeletionAndRequire,
        ReloadMechanism::DoOrRequireHelper,
        ReloadMechanism::WorkspaceRuntimeHelperObserver,
        ReloadMechanism::ClassRefreshCompatibilitySubject,
    ];

    /// Stable closed-vocabulary code used by the `.spec` fixtures.
    pub const fn as_str(self) -> &'static str {
        match self {
            ReloadMechanism::IncDeletionAndRequire => "inc_deletion_and_require",
            ReloadMechanism::DoOrRequireHelper => "do_or_require_helper",
            ReloadMechanism::WorkspaceRuntimeHelperObserver => "workspace_runtime_helper_observer",
            ReloadMechanism::ClassRefreshCompatibilitySubject => {
                "class_refresh_compatibility_subject"
            }
        }
    }

    /// Parse the closed vocabulary; unknown spellings are refused.
    pub fn parse(code: &str) -> Option<ReloadMechanism> {
        ReloadMechanism::ALL.into_iter().find(|mechanism| mechanism.as_str() == code)
    }
}

/// Perl runtime truths that limit every mechanism. Stated as limits, not
/// claims: none of them is a proof of migration.
pub const PERL_RUNTIME_LIMITATIONS: &[&str] = &[
    "Re-executing `require` does not remove previously defined symbols; old subs remain defined under redefinition, and methods already resolved may keep old definitions until caches are explicitly changed.",
    "Existing blessed instances, closures, and captured lexical state keep running the code they were built with; no mechanism here migrates them.",
    "Inheritance and method resolution caches (`@ISA`, `mro`) require an explicit `mro::method_changed_in` after package replacement; without it old resolutions can persist.",
    "Frames active during the reload continue executing old code; a reload never rewrites a live frame's code mid-execution.",
    "Source filters and compile hooks re-enter only under their own conditions (for example explicit `filter_read`-scoped use); a re-`require` is not guaranteed to re-run them identically.",
    "Engine compile success is never reload success: a module can compile and still leave old symbols, caches, and instances in place.",
];

/// One compared mechanism's record: what it is and what it cannot prove
/// before #10098's live measurement.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReloadMechanismRecord {
    /// The mechanism family.
    pub mechanism: ReloadMechanism,
    /// Limitation statements specific to this mechanism, beyond the
    /// shared [`PERL_RUNTIME_LIMITATIONS`].
    pub limitation_statements: &'static [&'static str],
}

impl ReloadMechanismRecord {
    /// Every limitation that applies to this mechanism (specific plus
    /// shared Perl truths).
    pub fn all_limitations(&self) -> Vec<&'static str> {
        let mut all: Vec<&'static str> = self.limitation_statements.to_vec();
        all.extend_from_slice(PERL_RUNTIME_LIMITATIONS);
        all
    }
}

const INC_DELETION_LIMITS: &[&str] = &[
    "Deletes only the `%INC` bookkeeping; package state, symbol tables, and already-created subs are untouched.",
    "Fails closed for `require` returning false (module already in `%INC` if deletion raced) and for die during recompile, both surfacing as ordinary Perl failures, not transaction outcomes.",
    "No acknowledgement channel: prompt-return proves command completion, not runtime replacement.",
];

const DO_REQUIRE_LIMITS: &[&str] = &[
    "`do` re-executes and returns the file's last expression without `%INC` bookkeeping; using it for reload requires explicit package handling that is entirely the helper's burden.",
    "`do` returns `undef` on failure with no exception detail, which cannot distinguish compile failure from runtime failure without additional scaffolding.",
];

const WORKSPACE_HELPER_LIMITS: &[&str] = &[
    "A workspace-owned helper/observer can serialize begin/acknowledge/read-back, but its injection into the debuggee requires its own reviewed authority, transport, and lifecycle — none is established by this contract.",
    "Until #10098 measures it, the helper proves nothing about package replacement or instance migration beyond what its own read-back observes.",
];

const CLASS_REFRESH_LIMITS: &[&str] = &[
    "Measured compatibility subject only: never bundled, never a product dependency, and never granted authority merely because it is installed or available.",
    "Its own documented scope excludes XS modules, and its `@INC`-deletion-plus-`require` core inherits every `%INC` limitation above.",
    "Delegating to it would move the transaction's honesty obligations into an external module the workspace cannot prove.",
];

/// The frozen comparative record for the four mechanisms.
pub fn mechanism_records() -> Vec<ReloadMechanismRecord> {
    vec![
        ReloadMechanismRecord {
            mechanism: ReloadMechanism::IncDeletionAndRequire,
            limitation_statements: INC_DELETION_LIMITS,
        },
        ReloadMechanismRecord {
            mechanism: ReloadMechanism::DoOrRequireHelper,
            limitation_statements: DO_REQUIRE_LIMITS,
        },
        ReloadMechanismRecord {
            mechanism: ReloadMechanism::WorkspaceRuntimeHelperObserver,
            limitation_statements: WORKSPACE_HELPER_LIMITS,
        },
        ReloadMechanismRecord {
            mechanism: ReloadMechanism::ClassRefreshCompatibilitySubject,
            limitation_statements: CLASS_REFRESH_LIMITS,
        },
    ]
}

/// A claim a proposed mechanism record makes. Used by verification to
/// refuse the forbidden claims.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MechanismClaims {
    /// The mechanism the claims are made about.
    pub mechanism: ReloadMechanism,
    /// The claims under review.
    pub claims: Vec<MechanismClaim>,
}

/// A single claim about a reload mechanism.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MechanismClaim {
    /// The mechanism exists and is available in the environment.
    Available,
    /// FORBIDDEN: engine compile success implies reload success.
    CompileSuccessImpliesReloadSuccess,
    /// FORBIDDEN before #10098 measurement: the mechanism proves package
    /// replacement.
    ProvesPackageReplacement,
    /// FORBIDDEN: availability (for example Class::Refresh being
    /// installed) grants product authority.
    AvailabilityGrantsProductAuthority,
    /// The mechanism observes prompts as acknowledgements.
    PromptIsAcknowledgement,
}

impl MechanismClaim {
    /// All claims in closed order.
    pub const ALL: [MechanismClaim; 5] = [
        MechanismClaim::Available,
        MechanismClaim::CompileSuccessImpliesReloadSuccess,
        MechanismClaim::ProvesPackageReplacement,
        MechanismClaim::AvailabilityGrantsProductAuthority,
        MechanismClaim::PromptIsAcknowledgement,
    ];

    /// Stable closed-vocabulary code used by the `.spec` fixtures.
    pub const fn as_str(self) -> &'static str {
        match self {
            MechanismClaim::Available => "available",
            MechanismClaim::CompileSuccessImpliesReloadSuccess => {
                "compile_success_implies_reload_success"
            }
            MechanismClaim::ProvesPackageReplacement => "proves_package_replacement",
            MechanismClaim::AvailabilityGrantsProductAuthority => {
                "availability_grants_product_authority"
            }
            MechanismClaim::PromptIsAcknowledgement => "prompt_is_acknowledgement",
        }
    }
}

/// Why a set of mechanism claims violates the contract.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MechanismRecordError {
    /// Compile success was claimed as reload success.
    CompileSuccessAsReloadSuccess,
    /// Package replacement was claimed without #10098's executed proof.
    PackageReplacementClaimedWithoutProof,
    /// Availability was claimed as product authority.
    AvailabilityAsProductAuthority,
    /// A prompt observation was claimed as an acknowledgement.
    PromptAsAcknowledgement,
}

impl MechanismRecordError {
    /// All mechanism errors in closed order.
    pub const ALL: [MechanismRecordError; 4] = [
        MechanismRecordError::CompileSuccessAsReloadSuccess,
        MechanismRecordError::PackageReplacementClaimedWithoutProof,
        MechanismRecordError::AvailabilityAsProductAuthority,
        MechanismRecordError::PromptAsAcknowledgement,
    ];

    /// Stable closed-vocabulary code used by the `.spec` fixtures.
    pub const fn code(self) -> &'static str {
        match self {
            MechanismRecordError::CompileSuccessAsReloadSuccess => {
                "compile_success_as_reload_success"
            }
            MechanismRecordError::PackageReplacementClaimedWithoutProof => {
                "package_replacement_claimed_without_proof"
            }
            MechanismRecordError::AvailabilityAsProductAuthority => {
                "availability_as_product_authority"
            }
            MechanismRecordError::PromptAsAcknowledgement => "prompt_as_acknowledgement",
        }
    }
}

/// Verify a set of mechanism claims against the frozen laws.
///
/// Availability alone is legitimate; every forbidden claim fails with its
/// exact code. The frozen precedence order (compile success, package
/// replacement, availability-as-authority, prompt-as-acknowledgement —
/// [`MechanismRecordError::ALL`]) decides which violation is reported, so
/// the result never depends on the caller's claim order.
pub fn verify_mechanism_claims(claims: &MechanismClaims) -> Result<(), MechanismRecordError> {
    let forbidden: [(MechanismClaim, MechanismRecordError); 4] = [
        (
            MechanismClaim::CompileSuccessImpliesReloadSuccess,
            MechanismRecordError::CompileSuccessAsReloadSuccess,
        ),
        (
            MechanismClaim::ProvesPackageReplacement,
            MechanismRecordError::PackageReplacementClaimedWithoutProof,
        ),
        (
            MechanismClaim::AvailabilityGrantsProductAuthority,
            MechanismRecordError::AvailabilityAsProductAuthority,
        ),
        (MechanismClaim::PromptIsAcknowledgement, MechanismRecordError::PromptAsAcknowledgement),
    ];
    for (claim, error) in forbidden {
        if claims.claims.contains(&claim) {
            return Err(error);
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_mechanism_record_states_real_limitations() {
        let records = mechanism_records();
        assert_eq!(records.len(), 4);
        for record in &records {
            assert!(!record.limitation_statements.is_empty());
            let all = record.all_limitations();
            assert!(all.len() > PERL_RUNTIME_LIMITATIONS.len());
            // The shared Perl truths apply to every mechanism.
            for truth in PERL_RUNTIME_LIMITATIONS {
                assert!(all.contains(truth));
            }
        }
    }

    #[test]
    fn no_mechanism_record_claims_compile_success_or_package_replacement() {
        for record in mechanism_records() {
            let claims = MechanismClaims {
                mechanism: record.mechanism,
                claims: vec![MechanismClaim::Available],
            };
            assert!(verify_mechanism_claims(&claims).is_ok());
        }
    }

    #[test]
    fn forbidden_claims_fail_with_their_exact_codes() {
        let cases: Vec<(MechanismClaim, MechanismRecordError)> = vec![
            (
                MechanismClaim::CompileSuccessImpliesReloadSuccess,
                MechanismRecordError::CompileSuccessAsReloadSuccess,
            ),
            (
                MechanismClaim::ProvesPackageReplacement,
                MechanismRecordError::PackageReplacementClaimedWithoutProof,
            ),
            (
                MechanismClaim::AvailabilityGrantsProductAuthority,
                MechanismRecordError::AvailabilityAsProductAuthority,
            ),
            (
                MechanismClaim::PromptIsAcknowledgement,
                MechanismRecordError::PromptAsAcknowledgement,
            ),
        ];
        for (claim, expected) in cases {
            let claims = MechanismClaims {
                mechanism: ReloadMechanism::ClassRefreshCompatibilitySubject,
                claims: vec![MechanismClaim::Available, claim],
            };
            assert_eq!(
                verify_mechanism_claims(&claims),
                Err(expected),
                "claim {} must fail with {}",
                claim.as_str(),
                expected.code()
            );
        }
    }

    #[test]
    fn claim_precedence_is_independent_of_input_order() {
        let reversed = MechanismClaims {
            mechanism: ReloadMechanism::ClassRefreshCompatibilitySubject,
            claims: vec![
                MechanismClaim::PromptIsAcknowledgement,
                MechanismClaim::AvailabilityGrantsProductAuthority,
                MechanismClaim::ProvesPackageReplacement,
                MechanismClaim::CompileSuccessImpliesReloadSuccess,
            ],
        };
        assert_eq!(
            verify_mechanism_claims(&reversed),
            Err(MechanismRecordError::CompileSuccessAsReloadSuccess)
        );
        let without_compile = MechanismClaims {
            mechanism: ReloadMechanism::IncDeletionAndRequire,
            claims: vec![
                MechanismClaim::PromptIsAcknowledgement,
                MechanismClaim::ProvesPackageReplacement,
            ],
        };
        assert_eq!(
            verify_mechanism_claims(&without_compile),
            Err(MechanismRecordError::PackageReplacementClaimedWithoutProof)
        );
    }

    #[test]
    fn mechanism_vocabulary_is_closed() {
        assert_eq!(ReloadMechanism::ALL.len(), 4);
        for mechanism in ReloadMechanism::ALL {
            assert_eq!(ReloadMechanism::parse(mechanism.as_str()), Some(mechanism));
        }
        assert_eq!(ReloadMechanism::parse("class_refresh"), None);
    }
}
