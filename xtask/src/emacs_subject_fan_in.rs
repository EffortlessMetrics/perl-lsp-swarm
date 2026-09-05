//! Fan-in validation of the Emacs subject lane (#8755, SUBJ_FAN).
//!
//! The subject lane's builders have landed: the manifest/resolver/cache core
//! (#11744 via #12540), the two external Eglot subjects (#11745 via #12662),
//! and the two external lsp-mode subjects (#11746 via #12669). This module
//! is the fan-in over that lane: it names the complete subject denominator
//! and validates, in one place, that the checked manifest completes it
//! exactly.
//!
//! Contract notes (the node's docs obligation):
//!
//! - *The denominator is the identity of this surface.* It binds the six
//!   subject classes the issue catalog selected — two bundled Eglot
//!   generations, one released and one pinned-source Eglot, one released and
//!   one pinned-source lsp-mode — each by exact id, family, source state,
//!   Emacs release tag/commit pin, host token, and audited version header.
//!   It is deliberately a checked constant, not a derived set: a lane change
//!   (a newer release selecting a new exact subject, a retired generation)
//!   must revise the denominator in a reviewed change, never slip through as
//!   an unnoticed extra row.
//! - *Completeness is exact, not at-least.* A partial denominator is never
//!   rendered complete (a missing slot is a typed failure naming the slot),
//!   and a row outside the denominator is an unbound generation, not
//!   surplus evidence. The manifest and the runner registry's
//!   manifest-bound dispatch set must cover the denominator in both
//!   directions.
//! - *A bound id whose row binds a different generation is stale.* A newer
//!   release re-pinned under an existing subject id, or any tag/token/
//!   header drift under a bound id, is refused until the denominator is
//!   revised — the fan-in never certifies a generation it did not bind.
//! - *The fan-in is strictly non-builder.* Validation is pure over the
//!   declared manifest identity: it creates no subject rows, materializes no
//!   inputs, writes no cache state, and launches nothing. A missing row is a
//!   typed failure, never a materialized one. Whether the declared subjects
//!   actually materialize stays proven by the resolver and the per-family
//!   contract suites; whether a client actually loaded at runtime stays with
//!   the actual host-run receipt.
//! - *Materialization is not a journey claim.* The subjects that resolve
//!   completely but have no driver adapter yet keep their typed launch
//!   refusals; the consumer leaves (#8776/#8795, the observation and root
//!   lanes) earn those claims themselves.

use std::collections::BTreeSet;
use std::fmt;

use crate::editor_client_compat::ClientSourceState;
use crate::emacs_host_run::EmacsClientSubject;
use crate::emacs_subject_manifest::{SubjectClientKind, SubjectManifest};

/// One bound slot of the complete subject denominator: the exact subject
/// class the slot names, bound by every declared identity field the fan-in
/// certifies. Digests stay pinned by the per-family contract suites against
/// the real manifest rows; the denominator binds the generation, not the
/// digest bytes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SubjectDenominatorSlot {
    /// Stable subject id, identical to the manifest row and registry id.
    pub subject_id: &'static str,
    /// Client family of the class.
    pub client_kind: SubjectClientKind,
    /// Source-state class of the slot.
    pub source_state: ClientSourceState,
    /// Exact Emacs release tag (`emacs-29.4`) or 40-hex commit pin this
    /// generation binds.
    pub emacs_release_tag: &'static str,
    /// Host build token the pinned generation requires.
    pub emacs_version_token: &'static str,
    /// Audited client version header of the generation (naming hint).
    pub client_version_hint: &'static str,
}

/// The complete subject denominator of the Emacs subject lane (#8755): the
/// six subject classes the issue catalog selected, as materialized by
/// SUBJ_CORE (#11744), SUBJ_E (#11745), and SUBJ_L (#11746).
///
/// The fan-in is complete exactly when the checked manifest's rows cover
/// these slots and nothing else. Revising this constant is a reviewed
/// lane-shape change (`return_to_issue` on the train manifest), not an
/// incidental row edit.
pub const SUBJECT_DENOMINATOR: [SubjectDenominatorSlot; 6] = [
    // Bundled Eglot 1.12.29 in exact Emacs 29.4 (SUBJ_CORE).
    SubjectDenominatorSlot {
        subject_id: "bundled_eglot_emacs_29_4",
        client_kind: SubjectClientKind::BundledEglot,
        source_state: ClientSourceState::Bundled,
        emacs_release_tag: "emacs-29.4",
        emacs_version_token: "29.4",
        client_version_hint: "1.12.29",
    },
    // Bundled Eglot 1.17.30 in exact Emacs 30.1 (SUBJ_CORE).
    SubjectDenominatorSlot {
        subject_id: "bundled_eglot_emacs_30_1",
        client_kind: SubjectClientKind::BundledEglot,
        source_state: ClientSourceState::Bundled,
        emacs_release_tag: "emacs-30.1",
        emacs_version_token: "30.1",
        client_version_hint: "1.17.30",
    },
    // Released GNU ELPA Eglot 1.24, archive-attested commit (SUBJ_E).
    SubjectDenominatorSlot {
        subject_id: "released_eglot_gnu_elpa_1_24",
        client_kind: SubjectClientKind::ExternalEglot,
        source_state: ClientSourceState::Released,
        emacs_release_tag: "0d67e76b94e1f0af9fe364aed8aa5db1c494c206",
        emacs_version_token: "30.1",
        client_version_hint: "1.24",
    },
    // Pinned upstream-source Eglot at the emacs.git commit (SUBJ_E).
    SubjectDenominatorSlot {
        subject_id: "source_eglot_emacs_c1ad9d27",
        client_kind: SubjectClientKind::ExternalEglot,
        source_state: ClientSourceState::UpstreamSource,
        emacs_release_tag: "c1ad9d27207aff96a22d49ae4c6cab35a2619927",
        emacs_version_token: "30.1",
        client_version_hint: "1.24",
    },
    // Released MELPA Stable lsp-mode 10.0.0, triple-attested commit (SUBJ_L).
    SubjectDenominatorSlot {
        subject_id: "released_lsp_mode_melpa_stable_10_0_0",
        client_kind: SubjectClientKind::LspMode,
        source_state: ClientSourceState::Released,
        emacs_release_tag: "913a6c07f163205cb568bc68d7dfe677dbc358ab",
        emacs_version_token: "30.1",
        client_version_hint: "10.0.0",
    },
    // Pinned upstream-source lsp-mode at the emacs-lsp commit (SUBJ_L).
    SubjectDenominatorSlot {
        subject_id: "source_lsp_mode_github_6bfc593",
        client_kind: SubjectClientKind::LspMode,
        source_state: ClientSourceState::UpstreamSource,
        emacs_release_tag: "6bfc593d7b1bc0dd656f09ffce52cc085ebced05",
        emacs_version_token: "30.1",
        client_version_hint: "10.0.1",
    },
];

/// Typed reasons the subject-lane fan-in refuses to certify completeness. A
/// refusal is never a pass and never a repair: the fan-in executes no
/// missing subject work.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SubjectFanInFailure {
    /// A denominator slot has no manifest row: the denominator is partial
    /// and must not be rendered complete.
    MissingSubject { slot_id: &'static str },
    /// A manifest row cites a subject generation the denominator does not
    /// bind. New subject classes join by revising the denominator in a
    /// reviewed change, not by arriving as surplus rows.
    UnboundGeneration { subject_id: String, reason: String },
    /// A bound subject id carries a row that binds a different generation
    /// than the denominator bound (re-pinned release tag/commit, drifted
    /// host token or version header): the row is stale relative to the
    /// certified denominator.
    StaleSubjectRow { subject_id: String, reason: String },
    /// A manifest binds one row per subject id, but the fan-in was handed
    /// more than one (a duplicate under a bound id can hide a stale copy
    /// behind the first match; the schema-level duplicate rejection is not
    /// assumed here because the fan-in certifies independently of
    /// `SubjectManifest::validate`).
    DuplicateSubjectRow { subject_id: String },
    /// The runner registry and the denominator have drifted: a slot the
    /// registry cannot dispatch through the subject manifest, or a
    /// manifest-bound registry row outside the denominator.
    RegistryDrift { reason: String },
}

impl fmt::Display for SubjectFanInFailure {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MissingSubject { slot_id } => write!(
                formatter,
                "the subject denominator is partial: no manifest row binds slot {slot_id}"
            ),
            Self::UnboundGeneration { subject_id, reason } => write!(
                formatter,
                "subject {subject_id} is a generation the subject denominator does not bind: \
                 {reason}"
            ),
            Self::StaleSubjectRow { subject_id, reason } => write!(
                formatter,
                "subject {subject_id} binds a different generation than its denominator slot: \
                 {reason}"
            ),
            Self::DuplicateSubjectRow { subject_id } => write!(
                formatter,
                "subject {subject_id} binds more than one manifest row; the denominator certifies \
                 exactly one row per bound slot"
            ),
            Self::RegistryDrift { reason } => {
                write!(
                    formatter,
                    "the runner registry disagrees with the subject denominator: {reason}"
                )
            }
        }
    }
}

impl std::error::Error for SubjectFanInFailure {}

/// Validate that one subject manifest completes the subject-lane denominator
/// exactly, and that the runner registry dispatches the denominator through
/// the subject manifest in both directions.
///
/// Pure over the declared identity: no rows are created or repaired, no
/// inputs are materialized, no cache state is written, nothing launches. A
/// failure is a typed refusal carrying what drifted, so a partial, stale, or
/// unbound subject lane stays uncertified (`not_proven`), never
/// fall-back-complete.
pub fn validate_subject_lane_denominator(
    manifest: &SubjectManifest,
) -> Result<(), SubjectFanInFailure> {
    // Completeness: every bound slot has a row, and that row binds exactly
    // the slot's generation. A bound id with a re-pinned or drifted row is
    // stale, not a silent generation swap.
    for slot in SUBJECT_DENOMINATOR {
        let Some(row) = manifest.subjects.iter().find(|row| row.subject_id == slot.subject_id)
        else {
            return Err(SubjectFanInFailure::MissingSubject { slot_id: slot.subject_id });
        };
        let mut drift = Vec::new();
        if row.client_kind != slot.client_kind {
            drift.push(format!("client kind {:?} (bound {:?})", row.client_kind, slot.client_kind));
        }
        if row.source_state != slot.source_state {
            drift.push(format!(
                "source state {:?} (bound {:?})",
                row.source_state, slot.source_state
            ));
        }
        if row.emacs_release_tag != slot.emacs_release_tag {
            drift.push(format!(
                "release tag {} (bound {})",
                row.emacs_release_tag, slot.emacs_release_tag
            ));
        }
        if row.emacs_version_token != slot.emacs_version_token {
            drift.push(format!(
                "host token {} (bound {})",
                row.emacs_version_token, slot.emacs_version_token
            ));
        }
        if row.client_version_hint != slot.client_version_hint {
            drift.push(format!(
                "version header {} (bound {})",
                row.client_version_hint, slot.client_version_hint
            ));
        }
        if !drift.is_empty() {
            return Err(SubjectFanInFailure::StaleSubjectRow {
                subject_id: slot.subject_id.to_string(),
                reason: drift.join("; "),
            });
        }
    }

    // Exactness: every manifest row is a bound slot, and every bound slot
    // exactly once. A row outside the denominator is an unbound generation,
    // and a duplicate row under any id — bound or not — hides a possible
    // stale copy behind the first match, so it is refused independently of
    // the schema-level duplicate rejection the fan-in does not assume.
    let mut seen_rows = BTreeSet::new();
    for row in &manifest.subjects {
        if !seen_rows.insert(row.subject_id.as_str()) {
            return Err(SubjectFanInFailure::DuplicateSubjectRow {
                subject_id: row.subject_id.clone(),
            });
        }
        if !SUBJECT_DENOMINATOR.iter().any(|slot| slot.subject_id == row.subject_id) {
            return Err(SubjectFanInFailure::UnboundGeneration {
                subject_id: row.subject_id.clone(),
                reason: "new subject classes join the lane by revising the denominator in a \
                         reviewed change, never as surplus manifest rows"
                    .to_string(),
            });
        }
    }

    validate_registry_dispatches_the_denominator()
}

/// The registry half of the fan-in law: the runner registry's manifest-bound
/// dispatch set must equal the denominator, slot tokens included. The
/// slice-2 released-Eglot registry row that predates the manifest is not
/// manifest-bound and stays outside this law until superseded.
fn validate_registry_dispatches_the_denominator() -> Result<(), SubjectFanInFailure> {
    for slot in SUBJECT_DENOMINATOR {
        let subject = EmacsClientSubject::from_id(slot.subject_id).map_err(|_| {
            SubjectFanInFailure::RegistryDrift {
                reason: format!(
                    "denominator slot {} is not a known runner registry subject",
                    slot.subject_id
                ),
            }
        })?;
        if !subject.resolves_through_subject_manifest() {
            return Err(SubjectFanInFailure::RegistryDrift {
                reason: format!(
                    "denominator slot {} does not dispatch through the subject manifest",
                    slot.subject_id
                ),
            });
        }
        if subject.pinned_emacs_version_token() != slot.emacs_version_token {
            return Err(SubjectFanInFailure::RegistryDrift {
                reason: format!(
                    "registry token for {} disagrees with the bound token {}",
                    slot.subject_id, slot.emacs_version_token
                ),
            });
        }
    }
    let registry_manifest_bound: BTreeSet<&str> = EmacsClientSubject::known_ids()
        .iter()
        .copied()
        .filter(|id| {
            EmacsClientSubject::from_id(id)
                .is_ok_and(|subject| subject.resolves_through_subject_manifest())
        })
        .collect();
    let denominator_ids: BTreeSet<&str> =
        SUBJECT_DENOMINATOR.iter().map(|slot| slot.subject_id).collect();
    if registry_manifest_bound != denominator_ids {
        return Err(SubjectFanInFailure::RegistryDrift {
            reason: format!(
                "manifest-bound registry rows {registry_manifest_bound:?} and denominator slots \
                 {denominator_ids:?} do not cover each other exactly"
            ),
        });
    }
    Ok(())
}
