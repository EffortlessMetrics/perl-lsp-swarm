//! Mutation bank for the standalone semantic conformance corpus (#11550).
//!
//! Each entry is one deliberate wrong coordinator/adapter behavior. The
//! bank is load-bearing: `standalone-vectors mutation-check` applies every
//! entry to its registered target vectors and fails unless the derived
//! packet differs from the checked-in golden (or the redaction scanner
//! fires). A mutation that survives means expected and actual would share
//! one blind spot — exactly the negative control #11550 forbids.

use super::oracle::Deviation;

/// One registered mutation: a stable id, the wrongness it models, and the
/// vector ids it must flip.
#[derive(Debug, Clone)]
pub struct MutationSpec {
    pub id: &'static str,
    pub deviation: Deviation,
    pub title: &'static str,
    /// Vectors whose golden packet must change when this deviation applies.
    pub target_vectors: &'static [&'static str],
}

/// All 15 behavioral mutations. Mutation 16 (expected oracle generated from
/// production output) is structural and enforced by the import-boundary
/// integration test, which also forbids subprocess spawning so no production
/// adapter can be executed by this harness.
pub const MUTATION_BANK: &[MutationSpec] = &[
    MutationSpec {
        id: "m01-reresolve-latest",
        deviation: Deviation::ReresolveLatest,
        title: "re-resolve latest/target after subject creation",
        target_vectors: &["v001-archive-pair-success", "v020-latest-drift-detected"],
    },
    MutationSpec {
        id: "m02-trust-wrong-identity",
        deviation: Deviation::TrustWrongIdentity,
        title: "trust wrong subject/predecessor identity",
        target_vectors: &["v009-transport-checksum-subject-mix", "v010-wrong-predecessor-receipt"],
    },
    MutationSpec {
        id: "m03-warn-and-continue",
        deviation: Deviation::WarnAndContinue,
        title: "warn-and-continue after mandatory failure",
        target_vectors: &[
            "v011-missing-mandatory-stage",
            "v012-mandatory-mislabeled-not-applicable",
        ],
    },
    MutationSpec {
        id: "m04-checksum-implies-provenance",
        deviation: Deviation::ChecksumImpliesProvenance,
        title: "treat checksum success as provenance success",
        target_vectors: &["v005-provenance-required-satisfied"],
    },
    MutationSpec {
        id: "m05-extract-before-integrity",
        deviation: Deviation::ExtractBeforeIntegrity,
        title: "extract before the required integrity stage",
        target_vectors: &["v001-archive-pair-success"],
    },
    MutationSpec {
        id: "m06-allow-missing-dap",
        deviation: Deviation::AllowMissingDap,
        title: "accept perllsp without required perl-dap in a pair",
        target_vectors: &["v013-pair-missing-dap"],
    },
    MutationSpec {
        id: "m07-implicit-fallback",
        deviation: Deviation::ImplicitFallback,
        title: "silently switch archive failure to latest registry source",
        target_vectors: &["v008-fallback-forbidden-no-registry-action"],
    },
    MutationSpec {
        id: "m08-source-as-archive-pair",
        deviation: Deviation::SourceAsArchivePair,
        title: "let source-mode proof satisfy archive-pair claims",
        target_vectors: &[
            "v003-registry-source-server-only",
            "v004-local-development-non-authoritative",
        ],
    },
    MutationSpec {
        id: "m09-mutate-destination-policy",
        deviation: Deviation::MutateDestinationPolicy,
        title: "mutate destination/product-unit/PATH policy before promotion",
        target_vectors: &[
            "v002-archive-historical-server-only",
            "v015-path-persisted-fresh-process-fails",
        ],
    },
    MutationSpec {
        id: "m10-mandatory-as-not-applicable",
        deviation: Deviation::MandatoryAsNotApplicable,
        title: "call a mandatory missing stage not_applicable",
        target_vectors: &[
            "v011-missing-mandatory-stage",
            "v012-mandatory-mislabeled-not-applicable",
        ],
    },
    MutationSpec {
        id: "m11-promotion-implies-health",
        deviation: Deviation::PromotionImpliesHealth,
        title: "publication automatically confirms health",
        target_vectors: &["v014-health-failure-rollback"],
    },
    MutationSpec {
        id: "m12-path-as-fresh-process",
        deviation: Deviation::PathPersistenceAsFreshProcess,
        title: "PATH persistence counts as fresh-process success",
        target_vectors: &["v015-path-persisted-fresh-process-fails"],
    },
    MutationSpec {
        id: "m13-erase-prior-attempt",
        deviation: Deviation::ErasePriorAttempt,
        title: "erase prior failed attempt when retry succeeds",
        // v016: retry fails too — erasing drops the attempt count;
        // v022: retry SUCCEEDS — erasing hides the evidence of the
        //        first failure behind a clean-looking success.
        target_vectors: &["v016-retry-stale-completion", "v022-retry-succeeds-erase-prior-attempt"],
    },
    MutationSpec {
        id: "m14-stale-advances-newer",
        deviation: Deviation::StaleAdvancesNewer,
        title: "stale completion advances the newer transaction",
        target_vectors: &["v016-retry-stale-completion"],
    },
    MutationSpec {
        id: "m15-leak-private-path",
        deviation: Deviation::LeakPrivatePath,
        title: "private path/credential leaks into the durable packet",
        target_vectors: &["v019-instrument-failure-redaction"],
    },
];
