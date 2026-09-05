//! Exact subject identity for a loaded-module reload transaction.
//!
//! A reload transaction may only name a subject whose identity is exact at
//! every layer the debugger can observe: which session and suspension
//! observed it, which `%INC` key and resolved runtime path it occupies,
//! which observation generation produced the loaded-source view, which
//! saved content digest is the runtime subject, which interpreter loaded
//! it, under which launch authority, and with which classification. The
//! candidate/binding split is load-bearing: [`SubjectCandidate`] is the
//! possibly-partial or wrong description presented at request time, and
//! [`LoadedModuleSubject`] exists only when [`SubjectCandidate::bind`]
//! has refused every insufficient form.

/// Classification of the proposed reload subject.
///
/// The closed set drives the eligibility refusals for module families the
/// initial cohort does not admit (main program, XS/native, source-filter or
/// compile-hook, generated/eval source).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ModuleClassification {
    /// Ordinary source-backed Perl `.pm` module — the only class the
    /// initial cohort can admit.
    SourceBackedPerlModule,
    /// The debuggee's main program (`$0`), not a loadable module.
    MainProgram,
    /// XS or otherwise native-linked module.
    XsOrNative,
    /// Module whose load runs a source filter or compile hook
    /// (for example `Filter::Util::Call` users).
    SourceFilterOrCompileHook,
    /// Source produced by generation or `eval` at runtime, with no stable
    /// on-disk subject of its own.
    GeneratedOrEval,
}

impl ModuleClassification {
    /// All classifications in stable closed order.
    pub const ALL: [ModuleClassification; 5] = [
        ModuleClassification::SourceBackedPerlModule,
        ModuleClassification::MainProgram,
        ModuleClassification::XsOrNative,
        ModuleClassification::SourceFilterOrCompileHook,
        ModuleClassification::GeneratedOrEval,
    ];

    /// Stable closed-vocabulary code used by the `.spec` fixtures.
    pub const fn as_str(self) -> &'static str {
        match self {
            ModuleClassification::SourceBackedPerlModule => "source_backed_perl_module",
            ModuleClassification::MainProgram => "main_program",
            ModuleClassification::XsOrNative => "xs_or_native",
            ModuleClassification::SourceFilterOrCompileHook => "source_filter_or_compile_hook",
            ModuleClassification::GeneratedOrEval => "generated_or_eval",
        }
    }

    /// Parse the closed vocabulary; unknown spellings are refused, never
    /// normalized.
    pub fn parse(code: &str) -> Option<ModuleClassification> {
        ModuleClassification::ALL.into_iter().find(|kind| kind.as_str() == code)
    }
}

/// Identity bindings as presented at request time. May be partial or wrong.
///
/// This is the shape a client-side or research candidate produces; it is
/// deliberately constructible with missing pieces so that binding failures
/// are observable and testable. Basename, package name, or path spelling
/// alone leaves the runtime key/path bindings empty and is refused.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct SubjectCandidate {
    /// Debuggee process/session generation the candidate was observed under.
    pub session_generation: Option<u64>,
    /// Suspension generation (`stopped_generation`) at observation time.
    pub suspension_generation: Option<u64>,
    /// Loaded-source observation generation that produced the `%INC` view.
    pub observation_generation: Option<u64>,
    /// Runtime `%INC` key (for example `App/Core.pm`). Empty when only a
    /// basename or package name is known.
    pub inc_key: String,
    /// Runtime-resolved absolute path the `%INC` entry points at.
    pub resolved_runtime_path: String,
    /// Digest/revision of the saved on-disk source that is the runtime
    /// subject. Empty when the buffer is dirty or unread.
    pub saved_content_digest: String,
    /// Editor logical source identity (URI).
    pub logical_source_uri: String,
    /// Selected Perl interpreter/runtime identity string.
    pub perl_identity: String,
    /// Validated launch root the subject must live under.
    pub launch_root: String,
    /// Classification claim for the subject.
    pub module_classification: Option<ModuleClassification>,
    /// Correlation identity of the proposed operation (non-zero).
    pub operation_identity: u64,
}

/// Reason a candidate cannot be bound to an exact reload subject.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SubjectBindingError {
    /// One or more required identity bindings are missing or empty:
    /// runtime key/path, saved digest, logical source, runtime identity,
    /// launch root, classification, operation identity, or any generation.
    /// Basename/package-only identity is exactly this error.
    InsufficientSubjectIdentity,
}

impl SubjectBindingError {
    /// All binding errors in closed order.
    pub const ALL: [SubjectBindingError; 1] = [SubjectBindingError::InsufficientSubjectIdentity];

    /// Stable closed-vocabulary code used by the `.spec` fixtures.
    pub const fn code(self) -> &'static str {
        match self {
            SubjectBindingError::InsufficientSubjectIdentity => "insufficient_subject_identity",
        }
    }
}

/// The fully bound, exact subject of one reload transaction.
///
/// Constructed only through [`SubjectCandidate::bind`]; every field is
/// non-empty and load-bearing. The struct deliberately has no constructor
/// that could produce a partially identified subject.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LoadedModuleSubject {
    session_generation: u64,
    suspension_generation: u64,
    observation_generation: u64,
    inc_key: String,
    resolved_runtime_path: String,
    saved_content_digest: String,
    logical_source_uri: String,
    perl_identity: String,
    launch_root: String,
    module_classification: ModuleClassification,
    operation_identity: u64,
}

impl SubjectCandidate {
    /// Bind an exact reload subject, refusing every insufficient form.
    ///
    /// Binding requires all of: non-empty `%INC` key and resolved runtime
    /// path, non-empty saved content digest, logical source URI, runtime
    /// identity, and launch root, a known classification, a non-zero
    /// operation identity, and all three generations present. A candidate
    /// carrying only a basename, package name, or path spelling cannot
    /// satisfy this and fails with
    /// [`SubjectBindingError::InsufficientSubjectIdentity`].
    pub fn bind(&self) -> Result<LoadedModuleSubject, SubjectBindingError> {
        let complete = self.session_generation.is_some()
            && self.suspension_generation.is_some()
            && self.observation_generation.is_some()
            && !self.inc_key.trim().is_empty()
            && !self.resolved_runtime_path.trim().is_empty()
            && !self.saved_content_digest.trim().is_empty()
            && !self.logical_source_uri.trim().is_empty()
            && !self.perl_identity.trim().is_empty()
            && !self.launch_root.trim().is_empty()
            && self.module_classification.is_some()
            && self.operation_identity != 0;
        if !complete {
            return Err(SubjectBindingError::InsufficientSubjectIdentity);
        }
        // Every Option is Some here by the `complete` check; `unwrap_or`
        // mirrors the values without any panicking accessor.
        let session_generation = self.session_generation.unwrap_or(0);
        let suspension_generation = self.suspension_generation.unwrap_or(0);
        let observation_generation = self.observation_generation.unwrap_or(0);
        let module_classification =
            self.module_classification.unwrap_or(ModuleClassification::MainProgram);
        Ok(LoadedModuleSubject {
            session_generation,
            suspension_generation,
            observation_generation,
            inc_key: self.inc_key.clone(),
            resolved_runtime_path: self.resolved_runtime_path.clone(),
            saved_content_digest: self.saved_content_digest.clone(),
            logical_source_uri: self.logical_source_uri.clone(),
            perl_identity: self.perl_identity.clone(),
            launch_root: self.launch_root.clone(),
            module_classification,
            operation_identity: self.operation_identity,
        })
    }
}

impl LoadedModuleSubject {
    /// Process/session generation the subject was bound under.
    pub fn session_generation(&self) -> u64 {
        self.session_generation
    }

    /// Suspension generation at bind time.
    pub fn suspension_generation(&self) -> u64 {
        self.suspension_generation
    }

    /// Loaded-source observation generation at bind time.
    pub fn observation_generation(&self) -> u64 {
        self.observation_generation
    }

    /// Runtime `%INC` key of the subject.
    pub fn inc_key(&self) -> &str {
        &self.inc_key
    }

    /// Runtime-resolved absolute path of the subject.
    pub fn resolved_runtime_path(&self) -> &str {
        &self.resolved_runtime_path
    }

    /// Saved on-disk content digest that is the runtime subject.
    pub fn saved_content_digest(&self) -> &str {
        &self.saved_content_digest
    }

    /// Editor logical source identity (URI).
    pub fn logical_source_uri(&self) -> &str {
        &self.logical_source_uri
    }

    /// Selected runtime identity string.
    pub fn perl_identity(&self) -> &str {
        &self.perl_identity
    }

    /// Launch root the subject must live under.
    pub fn launch_root(&self) -> &str {
        &self.launch_root
    }

    /// Classification of the subject.
    pub fn module_classification(&self) -> ModuleClassification {
        self.module_classification
    }

    /// Correlation identity of the operation.
    pub fn operation_identity(&self) -> u64 {
        self.operation_identity
    }

    /// Whether the binding is still current against the given live view.
    ///
    /// A subject is current only when the session has not been replaced,
    /// the suspension and observation generations still match, and the
    /// saved digest on disk still equals the bound digest. Any mismatch
    /// means the exact identity is stale and the transaction must
    /// re-bind before admission.
    pub fn is_current_against(&self, view: &SubjectCurrentnessView) -> bool {
        self.session_generation == view.session_generation
            && self.suspension_generation == view.suspension_generation
            && self.observation_generation == view.observation_generation
            && self.saved_content_digest == view.saved_content_digest
            && self.perl_identity == view.perl_identity
    }
}

/// Live currentness view a bound subject is checked against.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SubjectCurrentnessView {
    /// Current process/session generation (reset on replacement).
    pub session_generation: u64,
    /// Current suspension generation.
    pub suspension_generation: u64,
    /// Current loaded-source observation generation.
    pub observation_generation: u64,
    /// Saved digest readable on disk right now.
    pub saved_content_digest: String,
    /// Runtime identity of the live debuggee.
    pub perl_identity: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    type TestResult = Result<(), Box<dyn std::error::Error>>;

    fn complete_candidate() -> SubjectCandidate {
        SubjectCandidate {
            session_generation: Some(3),
            suspension_generation: Some(11),
            observation_generation: Some(2),
            inc_key: "App/Core.pm".to_string(),
            resolved_runtime_path: "/ws/lib/App/Core.pm".to_string(),
            saved_content_digest: "sha256:9f2c".to_string(),
            logical_source_uri: "file:///ws/lib/App/Core.pm".to_string(),
            perl_identity: "perl 5.42.0".to_string(),
            launch_root: "/ws".to_string(),
            module_classification: Some(ModuleClassification::SourceBackedPerlModule),
            operation_identity: 7,
        }
    }

    #[test]
    fn bind_accepts_a_complete_candidate_and_preserves_every_binding() -> TestResult {
        let subject = complete_candidate().bind().map_err(|_| "complete candidate must bind")?;
        assert_eq!(subject.inc_key(), "App/Core.pm");
        assert_eq!(subject.resolved_runtime_path(), "/ws/lib/App/Core.pm");
        assert_eq!(subject.saved_content_digest(), "sha256:9f2c");
        assert_eq!(subject.session_generation(), 3);
        assert_eq!(subject.suspension_generation(), 11);
        assert_eq!(subject.observation_generation(), 2);
        assert_eq!(subject.operation_identity(), 7);
        assert_eq!(subject.module_classification(), ModuleClassification::SourceBackedPerlModule);
        Ok(())
    }

    #[test]
    fn basename_only_identity_is_refused() {
        let mut candidate = complete_candidate();
        candidate.inc_key = String::new();
        candidate.resolved_runtime_path = String::new();
        candidate.saved_content_digest = String::new();
        assert!(
            candidate.bind().is_err_and(|error| error.code() == "insufficient_subject_identity")
        );
    }

    #[test]
    fn every_missing_binding_is_refused_with_the_same_closed_code() {
        let base = complete_candidate();
        // Field-by-field removals, each of which must fail binding.
        let missing: Vec<SubjectCandidate> = vec![
            SubjectCandidate { session_generation: None, ..base.clone() },
            SubjectCandidate { suspension_generation: None, ..base.clone() },
            SubjectCandidate { observation_generation: None, ..base.clone() },
            SubjectCandidate { inc_key: "  ".to_string(), ..base.clone() },
            SubjectCandidate { resolved_runtime_path: String::new(), ..base.clone() },
            SubjectCandidate { saved_content_digest: String::new(), ..base.clone() },
            SubjectCandidate { logical_source_uri: String::new(), ..base.clone() },
            SubjectCandidate { perl_identity: String::new(), ..base.clone() },
            SubjectCandidate { launch_root: String::new(), ..base.clone() },
            SubjectCandidate { module_classification: None, ..base.clone() },
            SubjectCandidate { operation_identity: 0, ..base.clone() },
        ];
        assert!(missing.iter().all(|candidate| {
            candidate.bind().is_err_and(|error| error.code() == "insufficient_subject_identity")
        }));
    }

    #[test]
    fn currentness_detects_session_replacement_and_digest_change() -> TestResult {
        let subject = complete_candidate().bind().map_err(|_| "complete candidate must bind")?;
        let current = SubjectCurrentnessView {
            session_generation: 3,
            suspension_generation: 11,
            observation_generation: 2,
            saved_content_digest: "sha256:9f2c".to_string(),
            perl_identity: "perl 5.42.0".to_string(),
        };
        assert!(subject.is_current_against(&current));
        let replaced = SubjectCurrentnessView { session_generation: 4, ..current.clone() };
        assert!(!subject.is_current_against(&replaced));
        let redigested =
            SubjectCurrentnessView { saved_content_digest: "sha256:aaaa".to_string(), ..current };
        assert!(!subject.is_current_against(&redigested));
        Ok(())
    }

    #[test]
    fn classification_vocabulary_is_closed() {
        assert_eq!(ModuleClassification::ALL.len(), 5);
        for kind in ModuleClassification::ALL {
            assert_eq!(ModuleClassification::parse(kind.as_str()), Some(kind));
        }
        assert_eq!(ModuleClassification::parse("xs_module"), None);
        assert_eq!(ModuleClassification::parse(""), None);
    }
}
