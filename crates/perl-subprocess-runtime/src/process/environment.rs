//! The environment projection a plan applies to its child.
//!
//! The projection is declarative: it records what the owner decided, not what
//! the ambient process happens to hold. Reading the real environment belongs
//! to the environment-snapshot authority, not here.

use std::collections::{BTreeMap, BTreeSet};

use super::encoding::CanonicalEncoder;
use super::identity::SecretValue;

/// The name of an environment variable.
///
/// Names are public; values are not.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct EnvVarName(String);

impl EnvVarName {
    /// Construct a variable name.
    pub fn new(name: impl Into<String>) -> Self {
        Self(name.into())
    }

    /// Borrow the name.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for EnvVarName {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

/// Variables that let a caller inject code or libraries into the child.
///
/// Admitting one of these is a decision, never a default: the plan must
/// acknowledge it explicitly and a hermetic probe may not admit one at all.
pub const CODE_LOADING_VARIABLES: &[&str] = &[
    "PERL5LIB",
    "PERL5OPT",
    "PERLLIB",
    "PERL5DB",
    "PERL_UNICODE",
    "LD_PRELOAD",
    "LD_LIBRARY_PATH",
    "LD_AUDIT",
    "DYLD_INSERT_LIBRARIES",
    "DYLD_LIBRARY_PATH",
    "PYTHONPATH",
    "RUBYOPT",
    "NODE_OPTIONS",
];

/// Whether a variable name is a known code-loading vector.
pub fn is_code_loading_variable(name: &EnvVarName) -> bool {
    CODE_LOADING_VARIABLES.contains(&name.as_str())
}

/// How the child treats the supervisor's ambient environment.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum AmbientInheritance {
    /// The child starts from an empty environment.
    DenyAll,
    /// Only explicitly allowed names are inherited.
    AllowListedOnly,
    /// Everything except explicitly denied names is inherited.
    ///
    /// The permissive option; profiles that require hermeticity reject it.
    InheritExceptDenied,
}

impl AmbientInheritance {
    pub(crate) fn discriminant(self) -> u16 {
        match self {
            Self::DenyAll => 0,
            Self::AllowListedOnly => 1,
            Self::InheritExceptDenied => 2,
        }
    }
}

/// Whether the plan's owner explicitly accepted code-loading variables.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum CodeLoadingDisposition {
    /// No code-loading variable may be admitted.
    Refused,
    /// The owner explicitly accepted the injection risk.
    AcknowledgedByOwner,
}

impl CodeLoadingDisposition {
    pub(crate) fn discriminant(self) -> u16 {
        match self {
            Self::Refused => 0,
            Self::AcknowledgedByOwner => 1,
        }
    }
}

/// The declarative environment projection applied to a child process.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EnvironmentProjection {
    projection_id: String,
    inheritance: AmbientInheritance,
    allowed: BTreeSet<EnvVarName>,
    denied: BTreeSet<EnvVarName>,
    removed: BTreeSet<EnvVarName>,
    additions: BTreeMap<EnvVarName, SecretValue>,
    code_loading: CodeLoadingDisposition,
}

impl EnvironmentProjection {
    /// Start a projection bound to an environment-snapshot identity.
    ///
    /// The identity is opaque here; the snapshot authority owns its meaning.
    pub fn new(projection_id: impl Into<String>, inheritance: AmbientInheritance) -> Self {
        Self {
            projection_id: projection_id.into(),
            inheritance,
            allowed: BTreeSet::new(),
            denied: BTreeSet::new(),
            removed: BTreeSet::new(),
            additions: BTreeMap::new(),
            code_loading: CodeLoadingDisposition::Refused,
        }
    }

    /// Allow a name to be inherited.
    #[must_use]
    pub fn allow(mut self, name: EnvVarName) -> Self {
        self.allowed.insert(name);
        self
    }

    /// Deny a name.
    #[must_use]
    pub fn deny(mut self, name: EnvVarName) -> Self {
        self.denied.insert(name);
        self
    }

    /// Remove a name that would otherwise be inherited.
    #[must_use]
    pub fn remove(mut self, name: EnvVarName) -> Self {
        self.removed.insert(name);
        self
    }

    /// Set a variable in the child's environment.
    #[must_use]
    pub fn add(mut self, name: EnvVarName, value: SecretValue) -> Self {
        self.additions.insert(name, value);
        self
    }

    /// Record that the owner explicitly accepted code-loading variables.
    #[must_use]
    pub fn acknowledging_code_loading(mut self) -> Self {
        self.code_loading = CodeLoadingDisposition::AcknowledgedByOwner;
        self
    }

    /// The opaque environment-snapshot identity.
    pub fn projection_id(&self) -> &str {
        &self.projection_id
    }

    /// How ambient variables are treated.
    pub fn inheritance(&self) -> AmbientInheritance {
        self.inheritance
    }

    /// Names explicitly allowed.
    pub fn allowed(&self) -> &BTreeSet<EnvVarName> {
        &self.allowed
    }

    /// Names explicitly denied.
    pub fn denied(&self) -> &BTreeSet<EnvVarName> {
        &self.denied
    }

    /// Names explicitly removed.
    pub fn removed(&self) -> &BTreeSet<EnvVarName> {
        &self.removed
    }

    /// Names explicitly set, without their values.
    pub fn addition_names(&self) -> impl Iterator<Item = &EnvVarName> {
        self.additions.keys()
    }

    /// Look up an addition's value.
    ///
    /// The only way to reach a secret value, and deliberately explicit.
    pub fn addition_value(&self, name: &EnvVarName) -> Option<&SecretValue> {
        self.additions.get(name)
    }

    /// Whether any addition carries a value the plan must keep private.
    pub fn carries_private_values(&self) -> bool {
        !self.additions.is_empty()
    }

    /// The disposition toward code-loading variables.
    pub fn code_loading(&self) -> CodeLoadingDisposition {
        self.code_loading
    }

    /// Names admitted into the child that are known code-loading vectors.
    pub fn admitted_code_loading_variables(&self) -> Vec<&EnvVarName> {
        self.allowed
            .iter()
            .chain(self.additions.keys())
            .filter(|name| is_code_loading_variable(name))
            .collect()
    }

    /// Names that appear in contradictory rules.
    pub fn contradictions(&self) -> Vec<&EnvVarName> {
        let mut contradictions: Vec<&EnvVarName> = Vec::new();
        for name in &self.allowed {
            if self.denied.contains(name) {
                contradictions.push(name);
            }
        }
        for name in self.additions.keys() {
            if self.removed.contains(name) || self.denied.contains(name) {
                contradictions.push(name);
            }
        }
        contradictions.sort();
        contradictions.dedup();
        contradictions
    }

    /// Canonically encode the projection.
    ///
    /// Variable **values** are never encoded, not even as a fingerprint: a
    /// fingerprint of a low-entropy secret is a guessable secret. Two plans
    /// that differ only in an addition's value therefore share a semantic
    /// fingerprint, which is a deliberate, documented limitation.
    pub(crate) fn encode(&self, encoder: &mut CanonicalEncoder) {
        encoder.section("environment");
        encoder.text(&self.projection_id);
        encoder.variant(self.inheritance.discriminant());
        encoder.variant(self.code_loading.discriminant());
        for (label, names) in
            [("allowed", &self.allowed), ("denied", &self.denied), ("removed", &self.removed)]
        {
            encoder.section(label);
            encoder.unsigned(names.len() as u64);
            for name in names {
                encoder.text(name.as_str());
            }
        }
        encoder.section("additions");
        encoder.unsigned(self.additions.len() as u64);
        for name in self.additions.keys() {
            encoder.text(name.as_str());
        }
    }
}
