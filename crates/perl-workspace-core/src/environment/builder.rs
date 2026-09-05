//! Deterministic population compiler for [`ProjectEnvironmentSnapshot`].
//!
//! This module is the #4833 builder residue named by the #2981 environment-epic
//! plan (slice S1). It bridges the environment *sources that already exist on
//! main* — client-configured include roots, admitted external roots, PERL5LIB
//! activation policy, system `@INC` probe outcomes, explicit interpreter
//! selection, bounded build-metadata facts, and default-denied ambient
//! observations — into the immutable snapshot model in [`super`].
//!
//! The compiler is pure: [`WorkspaceEnvironmentDeclaration`] accepts only
//! hand-fed, already-normalized values (owned strings and [`Digest`]
//! fingerprints). It performs no discovery, no filesystem access, and no
//! process execution; later slices own those producers and hand-feed their
//! bounded outcomes here. Selection and precedence semantics live in the
//! parent `environment` module; this module only normalizes declarations and
//! projects receipts.
//!
//! Two contracts are enforced by proof in this module:
//!
//! 1. slot *feed order* never changes the compiled snapshot, while include-root
//!    order *within* one slot stays semantic (it is a search path);
//! 2. [`EnvironmentInputAuthority::precedence_rank`] is the executable
//!    precedence contract, verified as a full pairwise table.
//!
//! Every compiled snapshot keeps rejected candidates visible, and
//! [`EnvironmentSnapshotReceipts`] projects both *selected* and *rejected*
//! facts with typed reasons — the #4833 explainability contract.

use std::sync::Arc;

use super::{
    BuildSystemFactRef, BuildSystemKind, Digest, EnvironmentBuildError, EnvironmentInput,
    EnvironmentInputAuthority, EnvironmentInputId, EnvironmentInputState, EnvironmentLimitation,
    EnvironmentPathRef, IncludeEntry, IncludeEntryRole, InterpreterIdentityRef,
    ProjectEnvironmentSnapshot, ProjectEnvironmentSnapshotBuilder, ToolCandidate,
    ToolCandidateRole, WorkspaceTrust, push_field,
};

// ── Declaration vocabulary ──────────────────────────────────────────────────

/// One normalized path plus its redacted public identity, supplied by the
/// producer so the compiler never has to invent redaction policy.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IncludeRootDeclaration {
    /// Normalized path used by trusted internal consumers.
    pub normalized: String,
    /// Stable redacted identity used by public receipts.
    pub public_id: String,
}

impl IncludeRootDeclaration {
    /// Declare one include-root candidate.
    #[must_use]
    pub fn new(normalized: impl Into<String>, public_id: impl Into<String>) -> Self {
        Self { normalized: normalized.into(), public_id: public_id.into() }
    }
}

/// PERL5LIB activation slot.
///
/// `Enabled` is the explicitly activated case (`WorkspaceConfig`
/// `use_perl5lib = true` with hand-fed entries). `Disabled` is the
/// default-deny case: the activation is configured off, and any observed
/// value fingerprint is retained only as a denied-ambient receipt.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum Perl5LibDeclaration {
    /// No producer supplied the activation state.
    #[default]
    NotSupplied,
    /// Activation is configured off; an observed value is ambient evidence only.
    Disabled {
        /// Fingerprint of the ambient value, when the producer observed one.
        observed_value_fingerprint: Option<Digest>,
    },
    /// Explicitly enabled activation with hand-fed include-root candidates.
    Enabled {
        /// Ordered entries; order is semantic within this slot.
        entries: Vec<IncludeRootDeclaration>,
    },
}

/// System `@INC` slot fed from an already-completed probe outcome
/// (`SystemIncProbeOutcome` on main); the compiler never probes.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum SystemIncDeclaration {
    /// No producer supplied the probe outcome.
    #[default]
    NotSupplied,
    /// System `@INC` probing is configured off for this workspace.
    Disabled,
    /// The probe ran and failed; the producer classifies why.
    ProbeUnavailable {
        /// Stable producer explanation code (for example
        /// `system_inc_probe_timed_out`).
        reason_code: String,
    },
    /// The probe succeeded; hand-fed startup roots in probe order.
    Available {
        /// Ordered startup roots; order is semantic within this slot.
        paths: Vec<IncludeRootDeclaration>,
    },
}

/// Interpreter selection slot fed from an already-completed selection; the
/// compiler never searches `PATH` or executes anything.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum InterpreterDeclaration {
    /// No producer supplied a selection decision.
    #[default]
    NotSupplied,
    /// Selection was attempted and could not obtain an interpreter.
    Unavailable {
        /// Stable producer explanation code (for example
        /// `interpreter_probe_failed`).
        reason_code: String,
    },
    /// A concrete interpreter was selected.
    Selected {
        /// Logical interpreter selection identity.
        logical_id: String,
        /// Normalized executable location.
        normalized_path: String,
        /// Redacted public executable identity.
        public_id: String,
        /// Fingerprint of bounded probe evidence supplied by the producer.
        evidence_fingerprint: Digest,
        /// Whether an explicit user configuration selected this interpreter
        /// (as opposed to a reviewed workspace convention).
        from_explicit_configuration: bool,
    },
}

/// One bounded, non-executing build-metadata fact (for example an
/// ExtUtils::MakeMaker or Carton family fact with its evidence fingerprint).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BuildSystemFactDeclaration {
    /// Reviewed build-system family.
    pub kind: BuildSystemKind,
    /// Fingerprint of the bounded metadata evidence.
    pub fact_fingerprint: Digest,
}

impl BuildSystemFactDeclaration {
    /// Declare one build-system fact.
    #[must_use]
    pub fn new(kind: BuildSystemKind, fact_fingerprint: Digest) -> Self {
        Self { kind, fact_fingerprint }
    }
}

/// One default-denied ambient observation (for example `PERL5OPT` observed
/// but never activated because ambient code-loading inputs are denied).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AmbientEnvironmentObservation {
    /// Stable semantic key, such as `env.PERL5OPT`.
    semantic_key: String,
    /// Stable producer source identity, such as `process_environment`.
    source_id: String,
    /// Fingerprint of the observed value, when one was captured.
    value_fingerprint: Option<Digest>,
    /// Stable explanation code for the denial.
    explanation_code: String,
}

impl AmbientEnvironmentObservation {
    /// Record one denied ambient observation.
    #[must_use]
    pub fn new(
        semantic_key: impl Into<String>,
        source_id: impl Into<String>,
        value_fingerprint: Option<Digest>,
        explanation_code: impl Into<String>,
    ) -> Self {
        Self {
            semantic_key: semantic_key.into(),
            source_id: source_id.into(),
            value_fingerprint,
            explanation_code: explanation_code.into(),
        }
    }
}

/// Hand-fed declaration of every environment source this compiler knows.
///
/// Slots left at their `NotSupplied` default compile to visible `Unavailable`
/// inputs and receipts — never to silent absence. Empty include-root lists
/// compile to no input at all: "nothing configured" is honest absence.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkspaceEnvironmentDeclaration {
    workspace_id: String,
    configuration_generation: u64,
    trust: WorkspaceTrust,
    user_include_roots: Vec<IncludeRootDeclaration>,
    external_include_roots: Vec<IncludeRootDeclaration>,
    perl5lib: Perl5LibDeclaration,
    system_inc: SystemIncDeclaration,
    interpreter: InterpreterDeclaration,
    build_facts: Vec<BuildSystemFactDeclaration>,
    ambient_observations: Vec<AmbientEnvironmentObservation>,
    limitations: Vec<EnvironmentLimitation>,
}

impl WorkspaceEnvironmentDeclaration {
    /// Start a declaration for one workspace identity, configuration
    /// generation, and trust state.
    #[must_use]
    pub fn new(
        workspace_id: impl Into<String>,
        configuration_generation: u64,
        trust: WorkspaceTrust,
    ) -> Self {
        Self {
            workspace_id: workspace_id.into(),
            configuration_generation,
            trust,
            user_include_roots: Vec::new(),
            external_include_roots: Vec::new(),
            perl5lib: Perl5LibDeclaration::NotSupplied,
            system_inc: SystemIncDeclaration::NotSupplied,
            interpreter: InterpreterDeclaration::NotSupplied,
            build_facts: Vec::new(),
            ambient_observations: Vec::new(),
            limitations: Vec::new(),
        }
    }

    /// Feed user/client-configured workspace-relative include roots
    /// (`WorkspaceConfig::include_paths`). Order is semantic.
    #[must_use]
    pub fn with_user_include_roots(
        mut self,
        roots: impl IntoIterator<Item = IncludeRootDeclaration>,
    ) -> Self {
        self.user_include_roots.extend(roots);
        self
    }

    /// Feed admitted machine-scoped external include roots
    /// (`WorkspaceConfig::external_include_paths`). Order is semantic.
    #[must_use]
    pub fn with_external_include_roots(
        mut self,
        roots: impl IntoIterator<Item = IncludeRootDeclaration>,
    ) -> Self {
        self.external_include_roots.extend(roots);
        self
    }

    /// Set the PERL5LIB activation slot.
    #[must_use]
    pub fn with_perl5lib(mut self, perl5lib: Perl5LibDeclaration) -> Self {
        self.perl5lib = perl5lib;
        self
    }

    /// Set the system `@INC` slot.
    #[must_use]
    pub fn with_system_inc(mut self, system_inc: SystemIncDeclaration) -> Self {
        self.system_inc = system_inc;
        self
    }

    /// Set the interpreter selection slot.
    #[must_use]
    pub fn with_interpreter(mut self, interpreter: InterpreterDeclaration) -> Self {
        self.interpreter = interpreter;
        self
    }

    /// Feed one bounded build-metadata fact.
    #[must_use]
    pub fn with_build_fact(mut self, fact: BuildSystemFactDeclaration) -> Self {
        self.build_facts.push(fact);
        self
    }

    /// Feed one denied ambient observation.
    #[must_use]
    pub fn with_ambient_observation(mut self, observation: AmbientEnvironmentObservation) -> Self {
        self.ambient_observations.push(observation);
        self
    }

    /// Feed one explicit limitation.
    #[must_use]
    pub fn with_limitation(mut self, limitation: EnvironmentLimitation) -> Self {
        self.limitations.push(limitation);
        self
    }

    /// Compile the declaration into a precedence-ranked snapshot.
    ///
    /// Deterministic: the same declaration always compiles to an equal
    /// snapshot, independent of the order in which slots were fed.
    pub fn compile(&self) -> Result<ProjectEnvironmentSnapshot, EnvironmentBuildError> {
        let mut inputs: Vec<EnvironmentInput> = Vec::new();
        let mut include_entries: Vec<IncludeEntry> = Vec::new();
        let mut build_systems: Vec<BuildSystemFactRef> = Vec::new();
        let mut tool_candidates: Vec<ToolCandidate> = Vec::new();
        let mut selected_interpreter: Option<InterpreterIdentityRef> = None;
        let mut limitations: Vec<EnvironmentLimitation> = self.limitations.clone();

        self.compile_user_include_roots(&mut inputs, &mut include_entries);
        self.compile_external_include_roots(&mut inputs, &mut include_entries);
        self.compile_perl5lib(&mut inputs, &mut include_entries);
        self.compile_system_inc(&mut inputs, &mut include_entries, &mut limitations);
        self.compile_interpreter(
            &mut inputs,
            &mut tool_candidates,
            &mut selected_interpreter,
            &mut limitations,
        );
        self.compile_build_facts(&mut inputs, &mut build_systems);
        self.compile_ambient_observations(&mut inputs);

        let mut builder = ProjectEnvironmentSnapshotBuilder::new(
            self.workspace_id.clone(),
            self.configuration_generation,
            self.trust,
        );
        for input in inputs {
            builder = builder.with_input(input);
        }
        for entry in include_entries {
            builder = builder.with_include_entry(entry);
        }
        for build_system in build_systems {
            builder = builder.with_build_system(build_system);
        }
        for tool in tool_candidates {
            builder = builder.with_tool_candidate(tool);
        }
        for limitation in limitations {
            builder = builder.with_limitation(limitation);
        }
        if let Some(interpreter) = selected_interpreter {
            builder = builder.with_selected_interpreter(interpreter);
        }
        builder.build()
    }

    fn compile_user_include_roots(
        &self,
        inputs: &mut Vec<EnvironmentInput>,
        include_entries: &mut Vec<IncludeEntry>,
    ) {
        if self.user_include_roots.is_empty() {
            return;
        }
        let fingerprints: Vec<String> =
            self.user_include_roots.iter().map(|root| root.normalized.clone()).collect();
        let input = EnvironmentInput::new(
            "include.configured",
            EnvironmentInputAuthority::UserConfiguration,
            EnvironmentInputState::Accepted,
            "client_settings",
            Some(list_fingerprint("user_include_roots", &fingerprints)),
            "user_include_roots_accepted",
        );
        push_include_entries(
            include_entries,
            &input.id,
            IncludeEntryRole::WorkspaceConfigured,
            &self.user_include_roots,
        );
        inputs.push(input);
    }

    fn compile_external_include_roots(
        &self,
        inputs: &mut Vec<EnvironmentInput>,
        include_entries: &mut Vec<IncludeEntry>,
    ) {
        if self.external_include_roots.is_empty() {
            return;
        }
        let fingerprints: Vec<String> =
            self.external_include_roots.iter().map(|root| root.normalized.clone()).collect();
        let input = EnvironmentInput::new(
            "include.external",
            EnvironmentInputAuthority::UserConfiguration,
            EnvironmentInputState::Accepted,
            "external_include_authority",
            Some(list_fingerprint("external_include_roots", &fingerprints)),
            "external_include_roots_admitted",
        );
        push_include_entries(
            include_entries,
            &input.id,
            IncludeEntryRole::Other,
            &self.external_include_roots,
        );
        inputs.push(input);
    }

    fn compile_perl5lib(
        &self,
        inputs: &mut Vec<EnvironmentInput>,
        include_entries: &mut Vec<IncludeEntry>,
    ) {
        match &self.perl5lib {
            Perl5LibDeclaration::NotSupplied => inputs.push(EnvironmentInput::new(
                "env.PERL5LIB",
                EnvironmentInputAuthority::Ambient,
                EnvironmentInputState::Unavailable,
                "process_environment",
                None,
                "perl5lib_state_not_supplied",
            )),
            Perl5LibDeclaration::Disabled { observed_value_fingerprint } => {
                inputs.push(EnvironmentInput::new(
                    "env.PERL5LIB",
                    EnvironmentInputAuthority::Ambient,
                    EnvironmentInputState::Denied,
                    "process_environment",
                    observed_value_fingerprint.clone(),
                    "perl5lib_denied_by_configuration",
                ));
            }
            Perl5LibDeclaration::Enabled { entries } => {
                // An explicitly enabled activation that resolved to zero entries is a
                // different fact from `NotSupplied`, and the declared slot owes a receipt
                // either way. Emit the input unconditionally; only the include entries
                // are conditional on there being entries to contribute.
                let fingerprints: Vec<String> =
                    entries.iter().map(|root| root.normalized.clone()).collect();
                // Two labels, two different questions.
                //
                // `authority` is the precedence class, and it must stay
                // `ExplicitEnvironment` — #4833 class 6, "explicitly enabled
                // environment activation". It cannot be `Ambient`: the landed model
                // rejects an accepted ambient input outright
                // (`EnvironmentValidationError::AmbientInputAccepted`), so an
                // ambient-classed PERL5LIB could never contribute a search path,
                // which would defeat the very opt-in PLSP-SPEC-0022 permits.
                //
                // `source_id` is provenance, and it is `process_environment` like
                // both sibling arms. The client setting decides *whether* PERL5LIB
                // is honoured; it never supplies the entries. Naming settings here
                // would make the one arm that actually contributes paths the one arm
                // whose receipt hides their ambient origin — exactly the misreport
                // PLSP-SPEC-0022 guards against.
                let input = EnvironmentInput::new(
                    "include.perl5lib",
                    EnvironmentInputAuthority::ExplicitEnvironment,
                    EnvironmentInputState::Accepted,
                    "process_environment",
                    Some(list_fingerprint("perl5lib_entries", &fingerprints)),
                    if entries.is_empty() {
                        "perl5lib_activation_enabled_without_entries"
                    } else {
                        "perl5lib_activation_enabled"
                    },
                );
                push_include_entries(
                    include_entries,
                    &input.id,
                    IncludeEntryRole::Perl5Lib,
                    entries,
                );
                inputs.push(input);
            }
        }
    }

    fn compile_system_inc(
        &self,
        inputs: &mut Vec<EnvironmentInput>,
        include_entries: &mut Vec<IncludeEntry>,
        limitations: &mut Vec<EnvironmentLimitation>,
    ) {
        match &self.system_inc {
            SystemIncDeclaration::NotSupplied => inputs.push(EnvironmentInput::new(
                "include.system_inc",
                EnvironmentInputAuthority::InterpreterEvidence,
                EnvironmentInputState::Unavailable,
                "system_inc_probe",
                None,
                "system_inc_state_not_supplied",
            )),
            SystemIncDeclaration::Disabled => inputs.push(EnvironmentInput::new(
                "include.system_inc",
                EnvironmentInputAuthority::InterpreterEvidence,
                EnvironmentInputState::Denied,
                "client_settings",
                None,
                "system_inc_disabled_by_configuration",
            )),
            SystemIncDeclaration::ProbeUnavailable { reason_code } => {
                limitations.push(EnvironmentLimitation {
                    code: "system_inc_unavailable".to_string(),
                    detail: reason_code.clone(),
                    input_id: None,
                });
                inputs.push(EnvironmentInput::new(
                    "include.system_inc",
                    EnvironmentInputAuthority::InterpreterEvidence,
                    EnvironmentInputState::Unavailable,
                    "system_inc_probe",
                    None,
                    reason_code.clone(),
                ));
            }
            SystemIncDeclaration::Available { paths } => {
                // A probe that ran and returned nothing is a different fact from a probe
                // that was never fed (`NotSupplied`) or that failed (`ProbeUnavailable`).
                // It is available, not unavailable, so it stays `Accepted` and carries its
                // own explanation code rather than vanishing from the receipts.
                let fingerprints: Vec<String> =
                    paths.iter().map(|root| root.normalized.clone()).collect();
                let input = EnvironmentInput::new(
                    "include.system_inc",
                    EnvironmentInputAuthority::InterpreterEvidence,
                    EnvironmentInputState::Accepted,
                    "system_inc_probe",
                    Some(list_fingerprint("system_inc_paths", &fingerprints)),
                    if paths.is_empty() {
                        "system_inc_available_without_paths"
                    } else {
                        "system_inc_available"
                    },
                );
                push_include_entries(
                    include_entries,
                    &input.id,
                    IncludeEntryRole::InterpreterStartup,
                    paths,
                );
                inputs.push(input);
            }
        }
    }

    fn compile_interpreter(
        &self,
        inputs: &mut Vec<EnvironmentInput>,
        tool_candidates: &mut Vec<ToolCandidate>,
        selected_interpreter: &mut Option<InterpreterIdentityRef>,
        limitations: &mut Vec<EnvironmentLimitation>,
    ) {
        match &self.interpreter {
            InterpreterDeclaration::NotSupplied => {
                let input = EnvironmentInput::new(
                    "interpreter.selected",
                    EnvironmentInputAuthority::WorkspaceConvention,
                    EnvironmentInputState::Unavailable,
                    "interpreter_selection",
                    None,
                    "interpreter_selection_not_supplied",
                );
                limitations.push(interpreter_unavailable_limitation(&input.id, "not_supplied"));
                // `NotSupplied` names a placeholder candidate and `Unavailable`
                // names none, on purpose. "No interpreter has been selected yet"
                // mirrors main's default `perl`-on-`PATH` fallback, so naming the
                // candidate is the honest report; "selection was attempted and
                // failed" has no candidate to name, and inventing one would claim
                // a fallback that was already ruled out. The candidate is never
                // active either way — its governing input is `Unavailable` — so
                // this changes what is explained, not what is resolved.
                tool_candidates.push(ToolCandidate::new(
                    ToolCandidateRole::Perl,
                    "perl",
                    EnvironmentPathRef::new("perl", "tool:perl-unresolved"),
                    input.id.clone(),
                ));
                inputs.push(input);
            }
            InterpreterDeclaration::Unavailable { reason_code } => {
                let input = EnvironmentInput::new(
                    "interpreter.selected",
                    EnvironmentInputAuthority::WorkspaceConvention,
                    EnvironmentInputState::Unavailable,
                    "interpreter_selection",
                    None,
                    reason_code.clone(),
                );
                limitations.push(interpreter_unavailable_limitation(&input.id, reason_code));
                inputs.push(input);
            }
            InterpreterDeclaration::Selected {
                logical_id,
                normalized_path,
                public_id,
                evidence_fingerprint,
                from_explicit_configuration,
            } => {
                let (authority, explanation) = if *from_explicit_configuration {
                    (EnvironmentInputAuthority::UserConfiguration, "interpreter_selected_explicit")
                } else {
                    (
                        EnvironmentInputAuthority::WorkspaceConvention,
                        "interpreter_selected_convention",
                    )
                };
                // Two digests describe this selection, and the split is
                // deliberate. The input's `value_fingerprint` hashes the selected
                // path because the behaviour-bearing value for precedence and
                // deterministic-equivalent collapse is *which executable was
                // chosen*: two producers that select the same binary must collapse
                // even when their probe evidence differs, which hashing
                // `evidence_fingerprint` here would wrongly turn into a conflict.
                // The bounded probe's own digest stays on
                // `InterpreterIdentityRef::evidence_fingerprint`, which is the
                // durable record of what the producer actually attested.
                let input = EnvironmentInput::new(
                    "interpreter.selected",
                    authority,
                    EnvironmentInputState::Accepted,
                    "interpreter_selection",
                    Some(Digest::of(normalized_path.as_str())),
                    explanation,
                );
                *selected_interpreter = Some(InterpreterIdentityRef {
                    logical_id: logical_id.clone(),
                    executable: EnvironmentPathRef::new(normalized_path.clone(), public_id.clone()),
                    evidence_fingerprint: evidence_fingerprint.clone(),
                    input_id: input.id.clone(),
                });
                tool_candidates.push(ToolCandidate::new(
                    ToolCandidateRole::Perl,
                    "perl",
                    EnvironmentPathRef::new(normalized_path.clone(), public_id.clone()),
                    input.id.clone(),
                ));
                inputs.push(input);
            }
        }
    }

    fn compile_build_facts(
        &self,
        inputs: &mut Vec<EnvironmentInput>,
        build_systems: &mut Vec<BuildSystemFactRef>,
    ) {
        for fact in &self.build_facts {
            let semantic_key = format!("build.metadata.{}", fact.kind.identity_key());
            let input = EnvironmentInput::new(
                semantic_key,
                EnvironmentInputAuthority::BuildMetadata,
                EnvironmentInputState::Accepted,
                "project_metadata",
                Some(fact.fact_fingerprint.clone()),
                "build_metadata_fact_recorded",
            );
            build_systems.push(BuildSystemFactRef::new(
                fact.kind.clone(),
                fact.fact_fingerprint.clone(),
                input.id.clone(),
            ));
            inputs.push(input);
        }
    }

    fn compile_ambient_observations(&self, inputs: &mut Vec<EnvironmentInput>) {
        for observation in &self.ambient_observations {
            inputs.push(EnvironmentInput::new(
                observation.semantic_key.clone(),
                EnvironmentInputAuthority::Ambient,
                EnvironmentInputState::Denied,
                observation.source_id.clone(),
                observation.value_fingerprint.clone(),
                observation.explanation_code.clone(),
            ));
        }
    }
}

fn interpreter_unavailable_limitation(
    input_id: &EnvironmentInputId,
    detail: &str,
) -> EnvironmentLimitation {
    EnvironmentLimitation {
        code: "interpreter_unavailable".to_string(),
        detail: format!("source-only operation continues ({detail})"),
        input_id: Some(input_id.clone()),
    }
}

fn push_include_entries(
    include_entries: &mut Vec<IncludeEntry>,
    input_id: &EnvironmentInputId,
    role: IncludeEntryRole,
    roots: &[IncludeRootDeclaration],
) {
    for (source_order, root) in roots.iter().enumerate() {
        let source_order = u32::try_from(source_order).unwrap_or(u32::MAX);
        include_entries.push(IncludeEntry::new(
            role,
            EnvironmentPathRef::new(root.normalized.clone(), root.public_id.clone()),
            input_id.clone(),
            source_order,
        ));
    }
}

fn list_fingerprint(domain_tag: &str, items: &[String]) -> Digest {
    let mut material = String::new();
    push_field(&mut material, "list", domain_tag);
    for item in items {
        push_field(&mut material, "item", item);
    }
    Digest::of(&material)
}

// ── Receipts: selected and rejected facts with reasons ──────────────────────

/// Typed rejection reason for one inactive environment input.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EnvironmentRejectionReason {
    /// Trust or policy denied the input.
    DeniedByPolicy,
    /// The input was observed only as ambient/advisory state.
    AmbientObservationOnly,
    /// The input could not be obtained.
    Unavailable,
    /// Equally authoritative candidates disagreed; none is authoritative.
    Conflicting,
    /// A stronger or equivalent candidate won. `None` means the winning group
    /// itself conflicted, so no single winner exists.
    SupersededBy(Option<EnvironmentInputId>),
}

/// One input-level receipt: selected, or rejected with a typed reason.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EnvironmentInputReceipt {
    /// Input identity.
    pub input_id: EnvironmentInputId,
    /// Logical precedence key.
    pub semantic_key: String,
    /// Input authority — the precedence class that decided the outcome.
    pub authority: EnvironmentInputAuthority,
    /// Provenance: the producer-supplied source the value came from.
    ///
    /// Distinct from [`Self::authority`], and carried because the two answer
    /// different questions. PLSP-SPEC-0022 requires PERL5LIB receipts to state
    /// their ambient origin, which an enabled activation cannot express through
    /// authority alone: it is precedence class 6 so that it can be active at
    /// all, while its entries remain process-environment material.
    pub source_id: String,
    /// Stable explanation code carried by the input.
    pub explanation_code: String,
    /// Typed rejection reason, or `None` when selected.
    pub rejection: Option<EnvironmentRejectionReason>,
}

/// Selected-and-rejected snapshot receipts for one snapshot.
///
/// This is the #4833 explainability projection: every input appears exactly
/// once, selected facts carry their winning explanation, and rejected facts
/// carry a typed reason.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EnvironmentSnapshotReceipts {
    selected: Vec<EnvironmentInputReceipt>,
    rejected: Vec<EnvironmentInputReceipt>,
}

impl EnvironmentSnapshotReceipts {
    /// Project receipts for a validated snapshot.
    #[must_use]
    pub fn of(snapshot: &ProjectEnvironmentSnapshot) -> Self {
        let mut selected = Vec::new();
        let mut rejected = Vec::new();
        for input in &snapshot.inputs {
            let receipt = EnvironmentInputReceipt {
                input_id: input.id.clone(),
                semantic_key: input.semantic_key.clone(),
                authority: input.authority,
                source_id: input.source_id.clone(),
                explanation_code: input.explanation_code.clone(),
                rejection: rejection_reason(snapshot, input),
            };
            if receipt.rejection.is_none() {
                selected.push(receipt);
            } else {
                rejected.push(receipt);
            }
        }
        Self { selected, rejected }
    }

    /// Selected inputs, in snapshot (deterministic) order.
    #[must_use]
    pub fn selected(&self) -> &[EnvironmentInputReceipt] {
        &self.selected
    }

    /// Rejected inputs, in snapshot (deterministic) order.
    #[must_use]
    pub fn rejected(&self) -> &[EnvironmentInputReceipt] {
        &self.rejected
    }
}

/// The typed reason one input is not active authority, or `None` when it is.
///
/// Returning `Option` keeps the "only inactive inputs have a rejection reason"
/// precondition in the signature rather than in the caller's guard. The
/// previous shape mapped `Accepted` onto `DeniedByPolicy`, which was
/// unreachable behind the guard but was the most misleading of the five
/// reasons if a later caller ever bypassed it.
fn rejection_reason(
    snapshot: &ProjectEnvironmentSnapshot,
    input: &EnvironmentInput,
) -> Option<EnvironmentRejectionReason> {
    match input.state {
        EnvironmentInputState::Accepted => None,
        EnvironmentInputState::Denied => Some(EnvironmentRejectionReason::DeniedByPolicy),
        EnvironmentInputState::Ambient => Some(EnvironmentRejectionReason::AmbientObservationOnly),
        EnvironmentInputState::Unavailable => Some(EnvironmentRejectionReason::Unavailable),
        EnvironmentInputState::Conflicting => Some(EnvironmentRejectionReason::Conflicting),
        EnvironmentInputState::Superseded => {
            let winner = snapshot.inputs.iter().find(|candidate| {
                candidate.state.is_active() && candidate.semantic_key == input.semantic_key
            });
            Some(EnvironmentRejectionReason::SupersededBy(
                winner.map(|candidate| candidate.id.clone()),
            ))
        }
    }
}

/// One include entry that is present for explainability but not effective.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RejectedIncludeEntryReceipt {
    /// Include-entry identity.
    pub entry_id: String,
    /// Logical include-root role.
    pub role: IncludeEntryRole,
    /// Governing input.
    pub input_id: EnvironmentInputId,
    /// Typed rejection reason inherited from the governing input.
    pub rejection: EnvironmentRejectionReason,
}

/// Project receipts for include entries whose governing input is not active.
///
/// Active entries are reachable through
/// [`ProjectEnvironmentSnapshot::active_include_entries`]; this projection
/// makes the rejected remainder explainable without re-deriving reasons.
#[must_use]
pub fn rejected_include_entries(
    snapshot: &ProjectEnvironmentSnapshot,
) -> Vec<RejectedIncludeEntryReceipt> {
    let receipts = EnvironmentSnapshotReceipts::of(snapshot);
    snapshot
        .include_entries
        .iter()
        .filter_map(|entry| {
            let rejection = receipts
                .rejected
                .iter()
                .find(|receipt| receipt.input_id == entry.input_id)
                .and_then(|receipt| receipt.rejection.clone());
            rejection.map(|rejection| RejectedIncludeEntryReceipt {
                entry_id: entry.id.clone(),
                role: entry.role,
                input_id: entry.input_id.clone(),
                rejection,
            })
        })
        .collect()
}

// ── Generation-tagged snapshot slot (projection seam) ───────────────────────

/// Generation-tagged holder for the current [`ProjectEnvironmentSnapshot`].
///
/// This is the S1 projection seam from the #2981 plan: a small, additive
/// holder that later slices (the S7 invalidation runtime) use as the refresh
/// consumer. Producers install a freshly compiled snapshot tagged with the
/// configuration generation it was compiled from; readers either accept the
/// current tag or require an exact generation match, so stale consumers fail
/// closed instead of silently using a superseded environment.
/// Deliberately not `Clone`. A cloned slot keeps its own `generation` and
/// `Arc`, so installing a newer snapshot into one holder leaves every clone
/// reporting its older snapshot as current — indefinitely, and without ever
/// failing closed, because each clone stays internally coherent. Since the
/// slot's whole purpose is to be the single authority on which snapshot is
/// current, ownership must stay singular. S7 needs multi-reader access it
/// should choose an explicit shared owner rather than inherit copy semantics
/// by accident.
#[derive(Debug, Default)]
pub struct EnvironmentSnapshotSlot {
    generation: u64,
    snapshot: Option<Arc<ProjectEnvironmentSnapshot>>,
}

/// Outcome of an [`EnvironmentSnapshotSlot::install`].
///
/// Installation can decline, so the outcome is returned rather than assumed.
/// Callers that genuinely do not care must say so explicitly.
#[derive(Debug, Clone, PartialEq, Eq)]
#[must_use]
pub enum SnapshotInstallOutcome {
    /// The snapshot became the slot's current environment.
    Installed,
    /// The snapshot did not satisfy [`ProjectEnvironmentSnapshot::validate`]
    /// and was refused. The slot is unchanged.
    Invalid(EnvironmentBuildError),
    /// The snapshot was compiled from an older configuration generation than
    /// the one already installed and was declined. The slot still holds the
    /// newer snapshot.
    Obsolete {
        /// Generation the slot continues to hold.
        installed: u64,
        /// Generation of the declined snapshot.
        declined: u64,
    },
}

impl EnvironmentSnapshotSlot {
    /// Create an empty slot.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Install a snapshot, replacing any previous snapshot from the same or an
    /// older configuration generation.
    ///
    /// The slot's generation tag is derived from the snapshot's own
    /// [`ProjectEnvironmentSnapshot::configuration_generation`], so a snapshot
    /// can never be installed under a tag it was not compiled from.
    ///
    /// The tag is also monotonic. Compilations can finish out of order — a
    /// generation-8 compile that started earlier may complete after a
    /// generation-9 one — and overwriting on arrival order would roll the slot
    /// backward, leaving [`Self::current`] advertising a superseded environment
    /// as authoritative while readers asking for generation 9 lose data that is
    /// still valid. An older snapshot is therefore declined and reported as
    /// [`SnapshotInstallOutcome::Obsolete`] rather than silently dropped, so a
    /// producer can tell "my work was superseded" from "my work landed".
    ///
    /// An equal generation still replaces, so recompiling the same generation
    /// is a refresh rather than a no-op.
    ///
    /// That last rule is also this type's ordering limit, and it is deliberate
    /// rather than overlooked. `configuration_generation` orders snapshots
    /// *across* generations and carries no information to order two snapshots
    /// *within* one — and declarations include probe and ambient facts that can
    /// change with no configuration increment, so two same-generation compiles
    /// can legitimately differ. Equal-generation installs are therefore
    /// last-write-wins, and a producer that issues concurrent refreshes at one
    /// generation must supply its own ordering; the slot cannot recover it.
    ///
    /// Nothing can exercise that today: `install` takes `&mut self` and the slot
    /// is neither `Clone` nor shared, so installs are serialized by ownership.
    /// Whoever gives the slot a shared owner (the S7 refresh runtime) owns the
    /// same-generation ordering contract — a compare-and-swap on the installed
    /// snapshot's identity, or a producer-side sequence — and should choose it
    /// against a real consumer rather than have S1 guess it.
    pub fn install(&mut self, snapshot: Arc<ProjectEnvironmentSnapshot>) -> SnapshotInstallOutcome {
        // Validate before anything else. `ProjectEnvironmentSnapshot` has public
        // fields and no `#[non_exhaustive]`, so a caller can mutate a built
        // snapshot — or assemble one literally — into a state `build()` would
        // never produce: a stale fingerprint, forged `Accepted` authority, an
        // unsupported schema version, a dangling reference. The type's own
        // contract says a failed validation is non-authoritative and its
        // `active_*` APIs must not be consulted, and handing snapshots to
        // exactly those consumers is this slot's entire purpose. Deserialization
        // already re-validates; this closes the in-process construction path.
        if let Err(error) = snapshot.validate() {
            return SnapshotInstallOutcome::Invalid(error);
        }
        let declined = snapshot.configuration_generation;
        if self.snapshot.is_some() && declined < self.generation {
            return SnapshotInstallOutcome::Obsolete { installed: self.generation, declined };
        }
        self.generation = declined;
        self.snapshot = Some(snapshot);
        SnapshotInstallOutcome::Installed
    }

    /// The generation tag of the installed snapshot, if any.
    #[must_use]
    pub fn generation(&self) -> Option<u64> {
        if self.snapshot.is_some() { Some(self.generation) } else { None }
    }

    /// The installed snapshot with its generation tag, if any.
    #[must_use]
    pub fn current(&self) -> Option<(u64, &Arc<ProjectEnvironmentSnapshot>)> {
        self.snapshot.as_ref().map(|snapshot| (self.generation, snapshot))
    }

    /// The installed snapshot only when its generation tag exactly matches
    /// the requested generation.
    #[must_use]
    pub fn current_for_generation(
        &self,
        configuration_generation: u64,
    ) -> Option<&Arc<ProjectEnvironmentSnapshot>> {
        if self.snapshot.is_some() && self.generation == configuration_generation {
            self.snapshot.as_ref()
        } else {
            None
        }
    }
}

// ── Proof ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::{
        AmbientEnvironmentObservation, BuildSystemFactDeclaration, BuildSystemKind, Digest,
        EnvironmentBuildError, EnvironmentInput, EnvironmentInputAuthority, EnvironmentInputState,
        EnvironmentLimitation, EnvironmentPathRef, EnvironmentRejectionReason,
        EnvironmentSnapshotReceipts, EnvironmentSnapshotSlot, IncludeEntry, IncludeEntryRole,
        InterpreterDeclaration, Perl5LibDeclaration, ProjectEnvironmentSnapshotBuilder,
        SnapshotInstallOutcome, SystemIncDeclaration, WorkspaceEnvironmentDeclaration,
        WorkspaceTrust, rejected_include_entries,
    };

    /// Self-source for the deny-fs proof; embedded at compile time so the
    /// proof itself performs no runtime filesystem reads.
    const BUILDER_SOURCE: &str = include_str!("builder.rs");

    fn root(normalized: &str, public_id: &str) -> super::IncludeRootDeclaration {
        super::IncludeRootDeclaration::new(normalized, public_id)
    }

    /// Every unordered pair from a slice, for mutual-distinctness assertions.
    fn pairs<T>(items: &[T]) -> Vec<(&T, &T)> {
        let mut out = Vec::new();
        for (index, left) in items.iter().enumerate() {
            for right in &items[index + 1..] {
                out.push((left, right));
            }
        }
        out
    }

    fn limitation(code: &str, detail: &str) -> EnvironmentLimitation {
        EnvironmentLimitation { code: code.to_string(), detail: detail.to_string(), input_id: None }
    }

    /// The seven slot feeds, each adding exactly one kind of material.
    type Feed = fn(WorkspaceEnvironmentDeclaration) -> WorkspaceEnvironmentDeclaration;

    const FEEDS: [Feed; 7] = [
        |d| d.with_user_include_roots([root("lib", "path:lib"), root("site", "path:site")]),
        |d| {
            d.with_perl5lib(Perl5LibDeclaration::Disabled {
                observed_value_fingerprint: Some(Digest::of("/ambient/perl5lib")),
            })
        },
        |d| {
            d.with_system_inc(SystemIncDeclaration::ProbeUnavailable {
                reason_code: "system_inc_probe_timed_out".to_string(),
            })
        },
        |d| d.with_interpreter(InterpreterDeclaration::NotSupplied),
        |d| {
            d.with_build_fact(BuildSystemFactDeclaration::new(
                BuildSystemKind::ExtUtilsMakeMaker,
                Digest::of("makefile-pl-evidence"),
            ))
        },
        |d| {
            d.with_ambient_observation(AmbientEnvironmentObservation::new(
                "env.PERL5OPT",
                "process_environment",
                Some(Digest::of("-Mlocal::lib")),
                "ambient_code_loading_denied",
            ))
        },
        |d| d.with_limitation(limitation("workspace_note", "reviewed note")),
    ];

    fn feed_by_order(order: &[usize]) -> WorkspaceEnvironmentDeclaration {
        let mut declaration =
            WorkspaceEnvironmentDeclaration::new("workspace:s1", 11, WorkspaceTrust::Trusted);
        for index in order {
            declaration = FEEDS[*index](declaration);
        }
        declaration
    }

    fn permutations(length: usize) -> Vec<Vec<usize>> {
        let mut values: Vec<usize> = (0..length).collect();
        let mut result = Vec::new();
        let mut counters = vec![0_usize; length];
        result.push(values.clone());
        let mut index = 1_usize;
        while index < length {
            if counters[index] < index {
                let swap = if index.is_multiple_of(2) { 0 } else { counters[index] };
                values.swap(swap, index);
                result.push(values.clone());
                counters[index] += 1;
                index = 1;
            } else {
                counters[index] = 0;
                index += 1;
            }
        }
        result
    }

    #[test]
    fn slot_feed_order_never_changes_the_compiled_snapshot() -> Result<(), EnvironmentBuildError> {
        let reference = feed_by_order(&[0, 1, 2, 3, 4, 5, 6]).compile()?;
        for order in permutations(FEEDS.len()) {
            let compiled = feed_by_order(&order).compile()?;
            assert_eq!(compiled, reference, "permutation {order:?} drifted");
        }
        Ok(())
    }

    #[test]
    fn include_root_order_within_a_slot_is_semantic() -> Result<(), EnvironmentBuildError> {
        let forward =
            WorkspaceEnvironmentDeclaration::new("workspace:s1", 1, WorkspaceTrust::Trusted)
                .with_user_include_roots([root("lib", "path:lib"), root("site", "path:site")])
                .compile()?;
        let reversed =
            WorkspaceEnvironmentDeclaration::new("workspace:s1", 1, WorkspaceTrust::Trusted)
                .with_user_include_roots([root("site", "path:site"), root("lib", "path:lib")])
                .compile()?;

        assert_ne!(forward.fingerprint, reversed.fingerprint);
        let forward_roots: Vec<_> =
            forward.active_include_entries().map(|entry| entry.path.normalized.clone()).collect();
        assert_eq!(forward_roots, vec!["lib".to_string(), "site".to_string()]);
        Ok(())
    }

    #[test]
    fn repeated_compilation_is_reproducible() -> Result<(), EnvironmentBuildError> {
        let declaration = feed_by_order(&[6, 5, 4, 3, 2, 1, 0]);
        assert_eq!(declaration.compile()?, declaration.compile()?);
        Ok(())
    }

    #[test]
    fn precedence_rank_contract_table_holds_for_every_authority_pair()
    -> Result<(), EnvironmentBuildError> {
        const AUTHORITIES: [EnvironmentInputAuthority; 7] = [
            EnvironmentInputAuthority::UserConfiguration,
            EnvironmentInputAuthority::TrustedProjectConfiguration,
            EnvironmentInputAuthority::InterpreterEvidence,
            EnvironmentInputAuthority::WorkspaceConvention,
            EnvironmentInputAuthority::BuildMetadata,
            EnvironmentInputAuthority::ExplicitEnvironment,
            EnvironmentInputAuthority::Ambient,
        ];
        for left in AUTHORITIES {
            for right in AUTHORITIES {
                let build = |authority, value: &str, source: &str| {
                    EnvironmentInput::new(
                        "contract.key",
                        authority,
                        EnvironmentInputState::Accepted,
                        source,
                        Some(Digest::of(value)),
                        "contract",
                    )
                };
                let snapshot = ProjectEnvironmentSnapshotBuilder::new(
                    "workspace:table",
                    1,
                    WorkspaceTrust::Trusted,
                )
                .with_input(build(left, "value-left", "source-left"))
                .with_input(build(right, "value-right", "source-right"))
                .build();

                // Ambient authority can never carry Accepted state, at any
                // rank: the fail-closed rule dominates the table.
                if left == EnvironmentInputAuthority::Ambient
                    || right == EnvironmentInputAuthority::Ambient
                {
                    assert!(
                        matches!(snapshot, Err(EnvironmentBuildError::AmbientInputAccepted { .. })),
                        "ambient authority must be rejected for ({left:?}, {right:?})"
                    );
                    continue;
                }

                let snapshot = snapshot?;
                let active: Vec<_> =
                    snapshot.inputs.iter().filter(|item| item.state.is_active()).collect();
                match left.precedence_rank().cmp(&right.precedence_rank()) {
                    std::cmp::Ordering::Less => {
                        assert_eq!(active.len(), 1, "{left:?} must beat {right:?}");
                        assert_eq!(active[0].authority, left);
                        assert!(snapshot.inputs.iter().any(|item| item.authority == right
                            && item.state == EnvironmentInputState::Superseded));
                    }
                    std::cmp::Ordering::Greater => {
                        assert_eq!(active.len(), 1, "{right:?} must beat {left:?}");
                        assert_eq!(active[0].authority, right);
                        assert!(snapshot.inputs.iter().any(|item| item.authority == left
                            && item.state == EnvironmentInputState::Superseded));
                    }
                    std::cmp::Ordering::Equal => {
                        assert!(
                            active.is_empty(),
                            "equal authority with distinct values must conflict"
                        );
                        assert_eq!(
                            snapshot
                                .inputs
                                .iter()
                                .filter(|item| item.state == EnvironmentInputState::Conflicting)
                                .count(),
                            2
                        );
                    }
                }
            }
        }
        Ok(())
    }

    #[test]
    fn equal_authority_equal_value_collapses_to_one_accepted_input()
    -> Result<(), EnvironmentBuildError> {
        let build = |source: &str| {
            EnvironmentInput::new(
                "contract.key",
                EnvironmentInputAuthority::UserConfiguration,
                EnvironmentInputState::Accepted,
                source,
                Some(Digest::of("same-value")),
                "contract",
            )
        };
        let snapshot =
            ProjectEnvironmentSnapshotBuilder::new("workspace:table", 1, WorkspaceTrust::Trusted)
                .with_input(build("source-a"))
                .with_input(build("source-b"))
                .build()?;
        assert_eq!(
            snapshot.inputs.iter().filter(|item| item.state.is_active()).count(),
            1,
            "deterministic equivalent duplicates collapse to one winner"
        );
        assert_eq!(
            snapshot
                .inputs
                .iter()
                .filter(|item| item.state == EnvironmentInputState::Superseded)
                .count(),
            1
        );
        Ok(())
    }

    #[test]
    fn receipts_show_selected_and_rejected_facts_with_reasons()
    -> Result<(), Box<dyn std::error::Error>> {
        let snapshot = feed_by_order(&[0, 1, 2, 3, 4, 5, 6]).compile()?;
        let receipts = EnvironmentSnapshotReceipts::of(&snapshot);

        // Two slots win: the user-configured include roots and the single
        // MakeMaker metadata fact.
        let mut selected_keys: Vec<_> =
            receipts.selected().iter().map(|receipt| receipt.semantic_key.as_str()).collect();
        selected_keys.sort_unstable();
        assert_eq!(selected_keys, vec!["build.metadata.extutils_makemaker", "include.configured"]);
        assert!(receipts.selected().iter().all(|receipt| receipt.rejection.is_none()));
        let configured = receipts
            .selected()
            .iter()
            .find(|receipt| receipt.semantic_key == "include.configured")
            .ok_or("the configured include roots must be selected")?;
        assert_eq!(configured.authority, EnvironmentInputAuthority::UserConfiguration);

        let mut by_key: Vec<(&str, EnvironmentRejectionReason)> = receipts
            .rejected()
            .iter()
            .filter_map(|receipt| {
                receipt.rejection.clone().map(|reason| (receipt.semantic_key.as_str(), reason))
            })
            .collect();
        by_key.sort_by(|left, right| left.0.cmp(right.0));
        let expected: Vec<(&str, EnvironmentRejectionReason)> = vec![
            ("env.PERL5LIB", EnvironmentRejectionReason::DeniedByPolicy),
            ("env.PERL5OPT", EnvironmentRejectionReason::DeniedByPolicy),
            ("include.system_inc", EnvironmentRejectionReason::Unavailable),
            ("interpreter.selected", EnvironmentRejectionReason::Unavailable),
        ];
        assert_eq!(by_key, expected);

        // Selected plus rejected partitions every input exactly once.
        assert_eq!(receipts.selected().len() + receipts.rejected().len(), snapshot.inputs.len());

        // Unavailable interpreter and probe surfaces are named limitations,
        // and source-only operation still compiled a validated snapshot.
        let codes: Vec<_> = snapshot.limitations.iter().map(|item| item.code.as_str()).collect();
        assert!(codes.contains(&"interpreter_unavailable"));
        assert!(codes.contains(&"system_inc_unavailable"));
        snapshot.validate()?;
        Ok(())
    }

    /// Every distinct declared state of the PERL5LIB slot must reach the receipts
    /// as its own fact. The interesting one is `Enabled { entries: [] }`: an
    /// activation that was explicitly switched on and resolved to nothing is not
    /// the same claim as "no activation was supplied", and it must not compile to
    /// silence.
    #[test]
    fn every_declared_perl5lib_state_produces_its_own_receipt()
    -> Result<(), Box<dyn std::error::Error>> {
        let declare = |state: Perl5LibDeclaration| {
            WorkspaceEnvironmentDeclaration::new("workspace:s1", 3, WorkspaceTrust::Trusted)
                .with_perl5lib(state)
        };

        let enabled_empty =
            declare(Perl5LibDeclaration::Enabled { entries: Vec::new() }).compile()?;
        let receipts = EnvironmentSnapshotReceipts::of(&enabled_empty);
        let selected: Vec<_> = receipts
            .selected()
            .iter()
            .map(|receipt| (receipt.semantic_key.as_str(), receipt.explanation_code.as_str()))
            .collect();
        assert_eq!(
            selected,
            vec![("include.perl5lib", "perl5lib_activation_enabled_without_entries")],
            "an explicitly enabled but empty PERL5LIB owes exactly one selected receipt"
        );
        // It is an accepted authority that contributes no search path — the receipt
        // exists precisely so that "enabled and empty" is readable, not inferred.
        assert_eq!(enabled_empty.active_include_entries().count(), 0);
        // An accepted input that contributes no include entry must still be a
        // valid snapshot, not merely a buildable one.
        enabled_empty.validate()?;

        let not_supplied = declare(Perl5LibDeclaration::NotSupplied).compile()?;
        let disabled = declare(Perl5LibDeclaration::Disabled {
            observed_value_fingerprint: Some(Digest::of("/ambient/perl5lib")),
        })
        .compile()?;
        let enabled_populated =
            declare(Perl5LibDeclaration::Enabled { entries: vec![root("lib", "path:lib")] })
                .compile()?;

        // All four declared states are mutually distinguishable in the fingerprint,
        // so none of them can be silently substituted for another.
        let fingerprints = [
            enabled_empty.fingerprint.clone(),
            not_supplied.fingerprint.clone(),
            disabled.fingerprint.clone(),
            enabled_populated.fingerprint.clone(),
        ];
        for (left, right) in pairs(&fingerprints) {
            assert_ne!(left, right, "two distinct PERL5LIB states share a fingerprint");
        }

        // The regression this pins: before the fix, the empty activation compiled to
        // the same zero-input shape as a declaration that fed the slot nothing at all.
        let unfed =
            WorkspaceEnvironmentDeclaration::new("workspace:s1", 3, WorkspaceTrust::Trusted)
                .with_system_inc(SystemIncDeclaration::NotSupplied)
                .compile()?;
        assert_ne!(enabled_empty.fingerprint, unfed.fingerprint);
        Ok(())
    }

    /// The same law for the system `@INC` probe. A probe that ran and returned no
    /// paths is available-and-empty; it is neither "never fed" nor "failed", and
    /// conflating it with either would misreport why the search path is short.
    #[test]
    fn every_declared_system_inc_state_produces_its_own_receipt()
    -> Result<(), Box<dyn std::error::Error>> {
        let declare = |state: SystemIncDeclaration| {
            WorkspaceEnvironmentDeclaration::new("workspace:s1", 5, WorkspaceTrust::Trusted)
                .with_system_inc(state)
        };

        let available_empty =
            declare(SystemIncDeclaration::Available { paths: Vec::new() }).compile()?;
        let receipts = EnvironmentSnapshotReceipts::of(&available_empty);
        let selected: Vec<_> = receipts
            .selected()
            .iter()
            .map(|receipt| (receipt.semantic_key.as_str(), receipt.explanation_code.as_str()))
            .collect();
        assert_eq!(
            selected,
            vec![("include.system_inc", "system_inc_available_without_paths")],
            "a probe that ran and returned nothing owes exactly one selected receipt"
        );
        assert_eq!(available_empty.active_include_entries().count(), 0);
        available_empty.validate()?;
        // A successful-but-empty probe is not a degraded surface, so it must not
        // manufacture a limitation the way `ProbeUnavailable` legitimately does.
        assert!(
            !available_empty.limitations.iter().any(|item| item.code == "system_inc_unavailable"),
            "an available probe must not report itself unavailable"
        );

        let not_supplied = declare(SystemIncDeclaration::NotSupplied).compile()?;
        let disabled = declare(SystemIncDeclaration::Disabled).compile()?;
        let probe_failed = declare(SystemIncDeclaration::ProbeUnavailable {
            reason_code: "system_inc_probe_timed_out".to_string(),
        })
        .compile()?;
        let available_populated =
            declare(SystemIncDeclaration::Available { paths: vec![root("inc", "path:inc")] })
                .compile()?;

        let fingerprints = [
            available_empty.fingerprint.clone(),
            not_supplied.fingerprint.clone(),
            disabled.fingerprint.clone(),
            probe_failed.fingerprint.clone(),
            available_populated.fingerprint.clone(),
        ];
        for (left, right) in pairs(&fingerprints) {
            assert_ne!(left, right, "two distinct system @INC states share a fingerprint");
        }
        Ok(())
    }

    /// Counter-control for the two tests above, and the reason they are not simply
    /// "every empty collection must emit". The plain-`Vec` slots carry no declared
    /// state: an empty `user_include_roots` means nothing was configured, and there
    /// is no explicit fact to preserve. Emitting a receipt there would invent one.
    /// This pins the asymmetry so a later consistency sweep cannot erase it in
    /// either direction.
    #[test]
    fn unfed_plain_collection_slots_stay_silent() -> Result<(), Box<dyn std::error::Error>> {
        let empty_collections =
            WorkspaceEnvironmentDeclaration::new("workspace:s1", 7, WorkspaceTrust::Trusted)
                .with_user_include_roots([])
                .with_external_include_roots([])
                .compile()?;
        let untouched =
            WorkspaceEnvironmentDeclaration::new("workspace:s1", 7, WorkspaceTrust::Trusted)
                .compile()?;

        // Feeding an empty collection is indistinguishable from not feeding it,
        // because in both cases the producer declared no root.
        assert_eq!(empty_collections, untouched);
        let keys: Vec<_> =
            empty_collections.inputs.iter().map(|input| input.semantic_key.as_str()).collect();
        assert!(
            !keys.contains(&"include.configured") && !keys.contains(&"include.external"),
            "an unconfigured include slot must not claim a receipt"
        );
        Ok(())
    }

    /// The slot is the single authority on which snapshot is current, so it must
    /// not be copyable into a second holder that can drift. A clone would keep its
    /// own generation and `Arc`, stay internally coherent, and therefore report a
    /// superseded snapshot as current forever without ever failing closed — the
    /// exact failure the type exists to prevent.
    ///
    /// Proven against the embedded self-source for the same reason the deny-fs
    /// proof is: the property is the absence of a derive, which a value-level
    /// assertion cannot observe.
    #[test]
    fn the_snapshot_slot_is_not_clonable() {
        let derive = BUILDER_SOURCE
            .split("pub struct EnvironmentSnapshotSlot")
            .next()
            .and_then(|before| before.rsplit("#[derive(").next())
            .map(|tail| tail.split(')').next().unwrap_or_default().to_string())
            .unwrap_or_default();
        assert!(!derive.is_empty(), "the slot must carry a derive list to check");
        assert!(
            !derive.contains("Clone"),
            "EnvironmentSnapshotSlot must not derive Clone; found derive({derive})"
        );
    }

    /// Replacement through the single owner is the supported route, and it fails
    /// the superseded generation closed. This is the behaviour a clone would have
    /// silently broken, so it is pinned next to the no-`Clone` proof.
    #[test]
    fn installing_a_newer_snapshot_fails_the_superseded_generation_closed()
    -> Result<(), Box<dyn std::error::Error>> {
        let older =
            WorkspaceEnvironmentDeclaration::new("workspace:s1", 7, WorkspaceTrust::Trusted)
                .compile()?;
        let newer =
            WorkspaceEnvironmentDeclaration::new("workspace:s1", 8, WorkspaceTrust::Trusted)
                .compile()?;

        let mut slot = EnvironmentSnapshotSlot::new();
        assert_eq!(slot.install(std::sync::Arc::new(older)), SnapshotInstallOutcome::Installed);
        assert!(slot.current_for_generation(7).is_some());

        assert_eq!(slot.install(std::sync::Arc::new(newer)), SnapshotInstallOutcome::Installed);
        assert_eq!(slot.generation(), Some(8));
        assert!(
            slot.current_for_generation(7).is_none(),
            "the superseded generation must fail closed after replacement"
        );
        Ok(())
    }

    /// The `NotSupplied` / `Unavailable` interpreter asymmetry is intended, so it
    /// is pinned rather than left to read as an accident: a selection that was
    /// never made names main's default `perl`-on-`PATH` candidate, and one that was
    /// attempted and failed names none. Neither is active authority.
    #[test]
    fn only_an_unmade_interpreter_selection_names_a_fallback_candidate()
    -> Result<(), Box<dyn std::error::Error>> {
        let not_supplied =
            WorkspaceEnvironmentDeclaration::new("workspace:s1", 2, WorkspaceTrust::Trusted)
                .with_interpreter(InterpreterDeclaration::NotSupplied)
                .compile()?;
        let unavailable =
            WorkspaceEnvironmentDeclaration::new("workspace:s1", 2, WorkspaceTrust::Trusted)
                .with_interpreter(InterpreterDeclaration::Unavailable {
                    reason_code: "interpreter_probe_failed".to_string(),
                })
                .compile()?;

        assert_eq!(
            not_supplied.tool_candidates.len(),
            1,
            "an unmade selection names the default fallback candidate"
        );
        assert!(
            unavailable.tool_candidates.is_empty(),
            "a failed selection must not invent a fallback it already ruled out"
        );

        // Both report the same governing state, so the candidate is explanatory
        // only and never resolves to active authority.
        for snapshot in [&not_supplied, &unavailable] {
            let governing = snapshot
                .inputs
                .iter()
                .find(|input| input.semantic_key == "interpreter.selected")
                .ok_or("the interpreter slot must always be reported")?;
            assert!(!governing.state.is_active());
            assert!(
                snapshot.limitations.iter().any(|item| item.code == "interpreter_unavailable"),
                "an unresolved interpreter must be a named limitation"
            );
        }
        Ok(())
    }

    /// `rejection_reason` now encodes its own precondition: an active input has no
    /// rejection reason at all, rather than being mapped onto a misleading one.
    #[test]
    fn an_active_input_has_no_rejection_reason() -> Result<(), Box<dyn std::error::Error>> {
        let snapshot = feed_by_order(&[0, 1, 2, 3, 4, 5, 6]).compile()?;
        for input in &snapshot.inputs {
            assert_eq!(
                super::rejection_reason(&snapshot, input).is_none(),
                input.state.is_active(),
                "rejection reason must be present exactly when the input is inactive"
            );
        }
        Ok(())
    }

    /// Compilations can finish out of order, so arrival order must not decide
    /// which environment is current. A generation-8 compile completing after a
    /// generation-9 one must be declined and reported, not silently installed:
    /// otherwise `current` advertises a superseded environment as authoritative
    /// and readers asking for generation 9 lose data that is still valid.
    #[test]
    fn an_out_of_order_older_snapshot_is_declined_not_installed()
    -> Result<(), Box<dyn std::error::Error>> {
        let newer =
            WorkspaceEnvironmentDeclaration::new("workspace:s1", 9, WorkspaceTrust::Trusted)
                .with_user_include_roots([root("site", "path:site")])
                .compile()?;
        let older =
            WorkspaceEnvironmentDeclaration::new("workspace:s1", 8, WorkspaceTrust::Trusted)
                .with_user_include_roots([root("lib", "path:lib")])
                .compile()?;

        let mut slot = EnvironmentSnapshotSlot::new();
        assert_eq!(slot.install(std::sync::Arc::new(newer)), SnapshotInstallOutcome::Installed);

        // The late arrival is declined, and the producer is told why rather than
        // left to assume its work landed.
        assert_eq!(
            slot.install(std::sync::Arc::new(older)),
            SnapshotInstallOutcome::Obsolete { installed: 9, declined: 8 }
        );

        // All three read paths still report the newer environment.
        assert_eq!(slot.generation(), Some(9));
        let (tag, held) = slot.current().ok_or("the newer snapshot must be retained")?;
        assert_eq!(tag, 9);
        assert_eq!(held.configuration_generation, 9);
        let held_roots: Vec<_> =
            held.active_include_entries().map(|entry| entry.path.normalized.clone()).collect();
        assert_eq!(held_roots, vec!["site".to_string()], "the older roots must not have landed");
        assert!(slot.current_for_generation(9).is_some());
        assert!(
            slot.current_for_generation(8).is_none(),
            "the declined generation must not become readable"
        );
        Ok(())
    }

    /// Monotonic does not mean frozen: recompiling the same generation is a
    /// refresh and must replace, or a producer could never correct a snapshot
    /// without a configuration change it has no reason to make.
    #[test]
    fn reinstalling_the_same_generation_refreshes() -> Result<(), Box<dyn std::error::Error>> {
        let first =
            WorkspaceEnvironmentDeclaration::new("workspace:s1", 4, WorkspaceTrust::Trusted)
                .with_user_include_roots([root("lib", "path:lib")])
                .compile()?;
        let corrected =
            WorkspaceEnvironmentDeclaration::new("workspace:s1", 4, WorkspaceTrust::Trusted)
                .with_user_include_roots([root("site", "path:site")])
                .compile()?;

        let mut slot = EnvironmentSnapshotSlot::new();
        assert_eq!(slot.install(std::sync::Arc::new(first)), SnapshotInstallOutcome::Installed);
        assert_eq!(slot.install(std::sync::Arc::new(corrected)), SnapshotInstallOutcome::Installed);

        let (_, held) = slot.current().ok_or("the refreshed snapshot must be present")?;
        let held_roots: Vec<_> =
            held.active_include_entries().map(|entry| entry.path.normalized.clone()).collect();
        assert_eq!(held_roots, vec!["site".to_string()]);
        Ok(())
    }

    /// PLSP-SPEC-0022 requires every PERL5LIB receipt to carry its ambient
    /// origin. The opt-in decides whether the entries are honoured; it never
    /// supplies them, so provenance stays `process_environment` in every arm —
    /// including, and especially, the one arm that actually contributes paths.
    #[test]
    fn every_perl5lib_receipt_keeps_its_ambient_provenance()
    -> Result<(), Box<dyn std::error::Error>> {
        let states = [
            Perl5LibDeclaration::NotSupplied,
            Perl5LibDeclaration::Disabled {
                observed_value_fingerprint: Some(Digest::of("/ambient/perl5lib")),
            },
            Perl5LibDeclaration::Enabled { entries: Vec::new() },
            Perl5LibDeclaration::Enabled { entries: vec![root("lib", "path:lib")] },
        ];

        for state in states {
            let snapshot =
                WorkspaceEnvironmentDeclaration::new("workspace:s1", 6, WorkspaceTrust::Trusted)
                    .with_perl5lib(state)
                    .compile()?;
            // Assert on the receipt projection, not the raw input: the receipt is
            // what a downstream report reads, and it is the surface
            // PLSP-SPEC-0022 constrains.
            let receipts = EnvironmentSnapshotReceipts::of(&snapshot);
            let perl5lib = receipts
                .selected()
                .iter()
                .chain(receipts.rejected())
                .find(|receipt| {
                    receipt.semantic_key.contains("perl5lib")
                        || receipt.semantic_key.contains("PERL5LIB")
                })
                .ok_or("every PERL5LIB state must reach the receipts")?;
            assert_eq!(
                perl5lib.source_id, "process_environment",
                "PERL5LIB values are ambient in every arm; the client setting only gates them"
            );
        }
        Ok(())
    }

    /// A snapshot that no longer satisfies its own invariants must not become
    /// current. `ProjectEnvironmentSnapshot` exposes public fields and is not
    /// `#[non_exhaustive]`, so a built value can be mutated into a state
    /// `build()` would never emit — here a fingerprint that no longer matches
    /// the content. The type's contract calls such a value non-authoritative,
    /// and the slot is precisely what hands values to `active_*` consumers, so
    /// installation fails closed and reports why.
    #[test]
    fn a_snapshot_that_fails_validation_cannot_become_current()
    -> Result<(), Box<dyn std::error::Error>> {
        let good = WorkspaceEnvironmentDeclaration::new("workspace:s1", 3, WorkspaceTrust::Trusted)
            .with_user_include_roots([root("lib", "path:lib")])
            .compile()?;
        good.validate()?;

        // Mutate content and leave the fingerprint as built: the stored digest
        // no longer describes the value, which is exactly the shape a caller
        // reaches by editing a public field without recompiling.
        let mut tampered = good.clone();
        tampered.workspace_id = "workspace:tampered".to_string();
        assert!(tampered.validate().is_err(), "the tampered snapshot must be invalid");

        // Refused into an empty slot: nothing becomes current.
        let mut slot = EnvironmentSnapshotSlot::new();
        assert!(
            matches!(
                slot.install(std::sync::Arc::new(tampered.clone())),
                SnapshotInstallOutcome::Invalid(_)
            ),
            "an invalid snapshot must be refused"
        );
        assert!(slot.current().is_none(), "a refused snapshot must not become current");
        assert!(slot.generation().is_none());

        // Refused over a good one: the previously installed snapshot survives
        // intact, so a bad install cannot corrupt or clear live state.
        assert_eq!(
            slot.install(std::sync::Arc::new(good.clone())),
            SnapshotInstallOutcome::Installed
        );
        assert!(matches!(
            slot.install(std::sync::Arc::new(tampered)),
            SnapshotInstallOutcome::Invalid(_)
        ));
        let (tag, held) = slot.current().ok_or("the valid snapshot must be retained")?;
        assert_eq!(tag, 3);
        assert_eq!(held.fingerprint, good.fingerprint);
        held.validate()?;
        Ok(())
    }

    #[test]
    fn conflicting_same_family_build_facts_are_both_rejected() -> Result<(), EnvironmentBuildError>
    {
        let snapshot =
            WorkspaceEnvironmentDeclaration::new("workspace:s1", 1, WorkspaceTrust::Trusted)
                .with_build_fact(BuildSystemFactDeclaration::new(
                    BuildSystemKind::ExtUtilsMakeMaker,
                    Digest::of("evidence-a"),
                ))
                .with_build_fact(BuildSystemFactDeclaration::new(
                    BuildSystemKind::ExtUtilsMakeMaker,
                    Digest::of("evidence-b"),
                ))
                .compile()?;
        let receipts = EnvironmentSnapshotReceipts::of(&snapshot);
        assert!(receipts.selected().is_empty());
        assert_eq!(
            receipts
                .rejected()
                .iter()
                .filter(|receipt| {
                    receipt.rejection == Some(EnvironmentRejectionReason::Conflicting)
                })
                .count(),
            2,
            "equally authoritative same-family disagreement stays conflicting"
        );
        assert_eq!(snapshot.build_systems.len(), 2, "rejected facts stay visible");
        Ok(())
    }

    #[test]
    fn superseded_receipts_name_the_winner_when_one_exists()
    -> Result<(), Box<dyn std::error::Error>> {
        let build = |authority, value: &str, source: &str| {
            EnvironmentInput::new(
                "receipt.key",
                authority,
                EnvironmentInputState::Accepted,
                source,
                Some(Digest::of(value)),
                "receipt",
            )
        };
        let snapshot = ProjectEnvironmentSnapshotBuilder::new(
            "workspace:receipts",
            1,
            WorkspaceTrust::Trusted,
        )
        .with_input(build(EnvironmentInputAuthority::UserConfiguration, "strong", "client"))
        .with_input(build(EnvironmentInputAuthority::WorkspaceConvention, "weak", "convention"))
        .build()?;
        let receipts = EnvironmentSnapshotReceipts::of(&snapshot);
        assert_eq!(receipts.selected().len(), 1);
        let weak = receipts
            .rejected()
            .iter()
            .find(|receipt| receipt.authority == EnvironmentInputAuthority::WorkspaceConvention);
        let weak = weak.ok_or("weak input must be rejected")?;
        assert_eq!(
            weak.rejection,
            Some(EnvironmentRejectionReason::SupersededBy(Some(
                receipts.selected()[0].input_id.clone()
            )))
        );
        Ok(())
    }

    #[test]
    fn superseded_receipt_reports_conflicted_group_without_a_winner()
    -> Result<(), Box<dyn std::error::Error>> {
        let build = |value: &str, source: &str| {
            EnvironmentInput::new(
                "receipt.key",
                EnvironmentInputAuthority::UserConfiguration,
                EnvironmentInputState::Accepted,
                source,
                Some(Digest::of(value)),
                "receipt",
            )
        };
        // Two equal-rank candidates disagree (both Conflicting) while a
        // lower-authority candidate loses to the conflicted group: no single
        // winner exists anywhere in the key.
        let weaker = EnvironmentInput::new(
            "receipt.key",
            EnvironmentInputAuthority::WorkspaceConvention,
            EnvironmentInputState::Accepted,
            "weaker-settings",
            Some(Digest::of("weaker")),
            "receipt",
        );
        let weaker_id = weaker.id.clone();
        let snapshot = ProjectEnvironmentSnapshotBuilder::new(
            "workspace:receipts",
            1,
            WorkspaceTrust::Trusted,
        )
        .with_input(build("value-a", "settings-a"))
        .with_input(build("value-b", "settings-b"))
        .with_input(weaker)
        .build()?;
        let receipts = EnvironmentSnapshotReceipts::of(&snapshot);
        assert!(receipts.selected().is_empty());
        let weaker_receipt = receipts
            .rejected()
            .iter()
            .find(|receipt| receipt.input_id == weaker_id)
            .ok_or("the weaker superseded input must have a receipt")?;
        assert_eq!(
            weaker_receipt.rejection,
            Some(EnvironmentRejectionReason::SupersededBy(None)),
            "no single winner exists when the best-rank group conflicts"
        );
        Ok(())
    }

    #[test]
    fn rejected_include_entries_inherit_their_input_reason() -> Result<(), EnvironmentBuildError> {
        let denied = EnvironmentInput::new(
            "env.PERL5LIB",
            EnvironmentInputAuthority::Ambient,
            EnvironmentInputState::Denied,
            "process_environment",
            Some(Digest::of("/ambient/perl5lib")),
            "perl5lib_denied_by_configuration",
        );
        let entry = IncludeEntry::new(
            IncludeEntryRole::Perl5Lib,
            EnvironmentPathRef::new("/ambient/perl5lib", "path:ambient-perl5lib"),
            denied.id.clone(),
            0,
        );
        let snapshot = ProjectEnvironmentSnapshotBuilder::new(
            "workspace:receipts",
            1,
            WorkspaceTrust::Untrusted,
        )
        .with_input(denied)
        .with_include_entry(entry)
        .build()?;

        assert_eq!(snapshot.active_include_entries().count(), 0);
        let rejected = rejected_include_entries(&snapshot);
        assert_eq!(rejected.len(), 1);
        assert_eq!(rejected[0].role, IncludeEntryRole::Perl5Lib);
        assert_eq!(rejected[0].rejection, EnvironmentRejectionReason::DeniedByPolicy);
        Ok(())
    }

    #[test]
    fn fingerprint_moves_on_material_changes() -> Result<(), EnvironmentBuildError> {
        let base = WorkspaceEnvironmentDeclaration::new("workspace:s1", 1, WorkspaceTrust::Trusted)
            .with_user_include_roots([root("lib", "path:lib")])
            .compile()?;
        let untrusted =
            WorkspaceEnvironmentDeclaration::new("workspace:s1", 1, WorkspaceTrust::Unknown)
                .with_user_include_roots([root("lib", "path:lib")])
                .compile()?;
        let next_generation =
            WorkspaceEnvironmentDeclaration::new("workspace:s1", 2, WorkspaceTrust::Trusted)
                .with_user_include_roots([root("lib", "path:lib")])
                .compile()?;
        let changed_roots =
            WorkspaceEnvironmentDeclaration::new("workspace:s1", 1, WorkspaceTrust::Trusted)
                .with_user_include_roots([root("site", "path:site")])
                .compile()?;

        assert_ne!(base.fingerprint, untrusted.fingerprint);
        assert_ne!(base.fingerprint, next_generation.fingerprint);
        assert_ne!(base.fingerprint, changed_roots.fingerprint);
        Ok(())
    }

    #[test]
    fn compiled_snapshot_receipt_redacts_internal_paths() -> Result<(), Box<dyn std::error::Error>>
    {
        let snapshot =
            WorkspaceEnvironmentDeclaration::new("workspace:s1", 1, WorkspaceTrust::Trusted)
                .with_external_include_roots([root(
                    "/home/steven/vendor/perl5",
                    "path:external-vendor",
                )])
                .compile()?;
        let json = serde_json::to_string(&snapshot.public_receipt())?;
        assert!(!json.contains("/home/steven/vendor/perl5"));
        assert!(json.contains("path:external-vendor"));
        Ok(())
    }

    #[test]
    fn populated_slots_rank_active_entries_by_authority() -> Result<(), Box<dyn std::error::Error>>
    {
        let snapshot =
            WorkspaceEnvironmentDeclaration::new("workspace:s1", 1, WorkspaceTrust::Trusted)
                .with_user_include_roots([root("lib", "path:lib")])
                .with_perl5lib(Perl5LibDeclaration::Enabled {
                    entries: vec![root("/activated/perl5lib", "path:perl5lib")],
                })
                .with_system_inc(SystemIncDeclaration::Available {
                    paths: vec![root("/opt/perl/lib", "path:startup")],
                })
                .with_interpreter(InterpreterDeclaration::Selected {
                    logical_id: "perl:explicit".to_string(),
                    normalized_path: "/opt/perl/bin/perl".to_string(),
                    public_id: "tool:perl".to_string(),
                    evidence_fingerprint: Digest::of("bounded-probe-evidence"),
                    from_explicit_configuration: true,
                })
                .compile()?;

        // Authority-ranked snapshot order: user configuration outranks
        // interpreter evidence, which outranks explicit environment
        // activation. Runtime @INC merging stays a consumer concern.
        let roles: Vec<_> = snapshot.active_include_entries().map(|entry| entry.role).collect();
        assert_eq!(
            roles,
            vec![
                IncludeEntryRole::WorkspaceConfigured,
                IncludeEntryRole::InterpreterStartup,
                IncludeEntryRole::Perl5Lib,
            ]
        );
        let interpreter = snapshot
            .selected_interpreter
            .as_ref()
            .ok_or("a selected interpreter must be present")?;
        assert_eq!(interpreter.logical_id, "perl:explicit");
        assert_eq!(interpreter.executable.normalized, "/opt/perl/bin/perl");
        snapshot.validate()?;
        Ok(())
    }

    #[test]
    fn not_supplied_interpreter_permits_source_only_operation()
    -> Result<(), Box<dyn std::error::Error>> {
        let snapshot =
            WorkspaceEnvironmentDeclaration::new("workspace:s1", 1, WorkspaceTrust::Trusted)
                .with_user_include_roots([root("lib", "path:lib")])
                .compile()?;
        assert!(snapshot.selected_interpreter.is_none());
        assert_eq!(snapshot.active_include_entries().count(), 1);
        let codes: Vec<_> = snapshot.limitations.iter().map(|item| item.code.as_str()).collect();
        assert!(codes.contains(&"interpreter_unavailable"));
        snapshot.validate()?;
        Ok(())
    }

    #[test]
    fn slot_requires_exact_generation_for_tagged_reads() -> Result<(), Box<dyn std::error::Error>> {
        let snapshot =
            WorkspaceEnvironmentDeclaration::new("workspace:s1", 7, WorkspaceTrust::Trusted)
                .with_user_include_roots([root("lib", "path:lib")])
                .compile()?;
        let mut slot = EnvironmentSnapshotSlot::new();
        assert!(slot.current().is_none());
        assert!(slot.generation().is_none());

        assert_eq!(slot.install(std::sync::Arc::new(snapshot)), SnapshotInstallOutcome::Installed);
        assert_eq!(slot.generation(), Some(7));
        let (_, installed) = slot.current().ok_or("installed snapshot must be present")?;
        assert_eq!(installed.workspace_id, "workspace:s1");
        assert!(slot.current_for_generation(7).is_some());
        assert!(slot.current_for_generation(8).is_none());

        let refreshed =
            WorkspaceEnvironmentDeclaration::new("workspace:s1", 8, WorkspaceTrust::Trusted)
                .with_user_include_roots([root("site", "path:site")])
                .compile()?;
        assert_eq!(slot.install(std::sync::Arc::new(refreshed)), SnapshotInstallOutcome::Installed);
        assert!(slot.current_for_generation(7).is_none());
        assert!(slot.current_for_generation(8).is_some());
        Ok(())
    }

    /// The slot tag must come from the installed snapshot's own
    /// `configuration_generation`, never from caller belief or the slot's
    /// previous tag: otherwise `install` could present generation-8 data as
    /// generation 7 and a stale read would silently accept it.
    #[test]
    fn slot_derives_tag_from_installed_snapshot() -> Result<(), Box<dyn std::error::Error>> {
        let gen7 = WorkspaceEnvironmentDeclaration::new("workspace:s1", 7, WorkspaceTrust::Trusted)
            .with_user_include_roots([root("lib", "path:lib")])
            .compile()?;
        let gen8 = WorkspaceEnvironmentDeclaration::new("workspace:s1", 8, WorkspaceTrust::Trusted)
            .with_user_include_roots([root("site", "path:site")])
            .compile()?;
        assert_eq!(gen7.configuration_generation, 7);
        assert_eq!(gen8.configuration_generation, 8);

        let mut slot = EnvironmentSnapshotSlot::new();
        assert_eq!(slot.install(std::sync::Arc::new(gen7)), SnapshotInstallOutcome::Installed);
        assert_eq!(slot.generation(), Some(7));

        // Installing a newer snapshot over a slot tagged 7 must retag from
        // the snapshot itself: the reported tag tracks the embedded
        // generation, and the stale generation-7 read fails closed instead
        // of serving generation-8 data under the old tag.
        assert_eq!(slot.install(std::sync::Arc::new(gen8)), SnapshotInstallOutcome::Installed);
        assert_eq!(slot.generation(), Some(8));
        let (tag, installed) = slot.current().ok_or("installed snapshot must be present")?;
        assert_eq!(tag, installed.configuration_generation);
        assert_eq!(installed.configuration_generation, 8);
        assert!(slot.current_for_generation(7).is_none());
        assert!(slot.current_for_generation(8).is_some());
        Ok(())
    }

    #[test]
    fn builder_source_is_free_of_filesystem_and_process_conduct() {
        assert!(!BUILDER_SOURCE.is_empty(), "self-source must be embedded");
        // Every forbidden token is assembled from fragments at runtime so no
        // token appears verbatim in this file; the proof would otherwise
        // match its own table.
        let forbidden: Vec<String> = [
            ("std::", "fs"),
            ("std::", "process"),
            ("std::", "env"),
            ("std::", "net"),
            ("File::", "open"),
            ("Open", "Options"),
            ("read_to_", "string"),
            ("canoni", "calize"),
            ("Command::", "new"),
            ("Tcp", "Stream"),
        ]
        .into_iter()
        .map(|(prefix, suffix)| format!("{prefix}{suffix}"))
        .collect();
        for token in forbidden {
            assert!(
                !BUILDER_SOURCE.contains(token.as_str()),
                "builder source must not reference {token}"
            );
        }
    }
}
