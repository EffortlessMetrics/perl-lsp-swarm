//! Exact subject identity for repository-only Perl::Critic compatibility runs.
//!
//! This module deliberately lives in the private conformance harness rather than
//! in an LSP/runtime crate. A subject is made from redacted identities supplied by
//! the harness; no filesystem path or ambient process environment is accepted.
//! The cache is an ordinary value owned by a harness run, so production requests
//! cannot populate or consult it.

use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::fmt;

use serde::Serialize;
use sha2::{Digest, Sha256};

const MAX_IDENTITY_LENGTH: usize = 256;

/// The immutable identity of one complete Perl::Critic conformance subject.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
pub struct OracleSubject {
    perl_identity: String,
    critic_identity: String,
    fixture_digest: String,
    root_identity: String,
    source_digest: String,
    profile_digest: String,
    invocation: OracleInvocation,
    environment_digest: String,
    process_schema: String,
    parser_schema: String,
}

impl OracleSubject {
    /// Build a subject from redacted/content identities, rejecting private paths
    /// and missing identity components before they can reach a cache or receipt.
    // The oracle subject is an identity bag: all ten strings are distinct
    // pinned identities, so a parameter object would only relocate the
    // enumeration without adding meaning.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        perl_identity: impl Into<String>,
        critic_identity: impl Into<String>,
        fixture_digest: impl Into<String>,
        root_identity: impl Into<String>,
        source_digest: impl Into<String>,
        profile_digest: impl Into<String>,
        invocation: OracleInvocation,
        environment_digest: impl Into<String>,
        process_schema: impl Into<String>,
        parser_schema: impl Into<String>,
    ) -> Result<Self, OracleSubjectError> {
        let subject = Self {
            perl_identity: perl_identity.into(),
            critic_identity: critic_identity.into(),
            fixture_digest: fixture_digest.into(),
            root_identity: root_identity.into(),
            source_digest: source_digest.into(),
            profile_digest: profile_digest.into(),
            invocation,
            environment_digest: environment_digest.into(),
            process_schema: process_schema.into(),
            parser_schema: parser_schema.into(),
        };
        subject.validate()?;
        Ok(subject)
    }

    /// Stable, path-free digest used as the cache key and receipt identity.
    pub fn digest(&self) -> String {
        let mut hasher = Sha256::new();
        for (name, value) in self.fields() {
            hasher.update(name.as_bytes());
            hasher.update((value.len() as u64).to_le_bytes());
            hasher.update(value.as_bytes());
        }
        hasher.update(b"invocation");
        hasher.update(self.invocation.severity.to_le_bytes());
        let invocation_fields: [(&str, Vec<&str>); 4] = [
            ("theme", self.invocation.theme.as_deref().into_iter().collect()),
            ("include", self.invocation.include.iter().map(String::as_str).collect()),
            ("exclude", self.invocation.exclude.iter().map(String::as_str).collect()),
            ("options", self.invocation.options.iter().map(String::as_str).collect()),
        ];
        for (name, values) in invocation_fields {
            hasher.update(name.as_bytes());
            for value in values {
                hasher.update((value.len() as u64).to_le_bytes());
                hasher.update(value.as_bytes());
            }
        }
        format!("sha256:{}", hex_lower(&hasher.finalize()))
    }

    /// Validate all subject components and the redaction boundary.
    pub fn validate(&self) -> Result<(), OracleSubjectError> {
        for (field, value) in self.fields() {
            validate_identity_value(field, value)?;
        }
        if !(1..=5).contains(&self.invocation.severity) {
            return Err(OracleSubjectError::InvalidSeverity { value: self.invocation.severity });
        }
        if let Some(theme) = &self.invocation.theme {
            validate_identity_value("invocation.theme", theme)?;
        }
        for (field, values) in [
            ("invocation.include", &self.invocation.include),
            ("invocation.exclude", &self.invocation.exclude),
            ("invocation.options", &self.invocation.options),
        ] {
            for value in values {
                validate_identity_value(field, value)?;
            }
        }
        Ok(())
    }

    fn fields(&self) -> [(&'static str, &str); 9] {
        [
            ("perl_identity", &self.perl_identity),
            ("critic_identity", &self.critic_identity),
            ("fixture_digest", &self.fixture_digest),
            ("root_identity", &self.root_identity),
            ("source_digest", &self.source_digest),
            ("profile_digest", &self.profile_digest),
            ("environment_digest", &self.environment_digest),
            ("process_schema", &self.process_schema),
            ("parser_schema", &self.parser_schema),
        ]
    }
}

fn validate_identity_value(field: &'static str, value: &str) -> Result<(), OracleSubjectError> {
    if value.is_empty() {
        return Err(OracleSubjectError::Empty { field });
    }
    if value.len() > MAX_IDENTITY_LENGTH {
        return Err(OracleSubjectError::TooLong { field });
    }
    if value.contains('/') || value.contains('\\') || value.contains("..") {
        return Err(OracleSubjectError::PrivatePath { field });
    }
    Ok(())
}

/// Invocation settings that affect Perl::Critic output.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
pub struct OracleInvocation {
    /// Perl::Critic severity threshold.
    pub severity: u8,
    /// Optional theme selected for the oracle.
    pub theme: Option<String>,
    /// Policy include arguments, retaining behavior-bearing order.
    pub include: Vec<String>,
    /// Policy exclude arguments, retaining behavior-bearing order.
    pub exclude: Vec<String>,
    /// Other reviewed invocation arguments, retaining behavior-bearing order.
    pub options: Vec<String>,
}

fn hex_lower(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut output = String::with_capacity(bytes.len() * 2);
    for &byte in bytes {
        output.push(HEX[(byte >> 4) as usize] as char);
        output.push(HEX[(byte & 0x0f) as usize] as char);
    }
    output
}

/// A rejected subject identity.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OracleSubjectError {
    /// A required identity component was absent.
    Empty { field: &'static str },
    /// An identity component exceeded the bounded receipt size.
    TooLong { field: &'static str },
    /// A private or path-shaped value was supplied where a redacted identity was required.
    PrivatePath { field: &'static str },
    /// Perl::Critic severity is outside its reviewed 1–5 range.
    InvalidSeverity { value: u8 },
}

impl fmt::Display for OracleSubjectError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Empty { field } => write!(formatter, "oracle subject field {field} is empty"),
            Self::TooLong { field } => {
                write!(formatter, "oracle subject field {field} is too long")
            }
            Self::PrivatePath { field } => {
                write!(formatter, "oracle subject field {field} contains a private path")
            }
            Self::InvalidSeverity { value } => {
                write!(formatter, "oracle invocation severity {value} is outside 1..=5")
            }
        }
    }
}

impl std::error::Error for OracleSubjectError {}

/// Counters for deterministic cache observability.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
pub struct OracleCacheStats {
    /// Number of exact subject hits.
    pub hits: u64,
    /// Number of exact subject misses.
    pub misses: u64,
    /// Number of explicitly retired subjects.
    pub invalidations: u64,
    /// Number of entries evicted at the configured bound.
    pub evictions: u64,
}

/// A bounded, exact-subject cache for repository-only oracle results.
#[derive(Debug)]
pub struct OracleCache<T> {
    capacity: usize,
    entries: BTreeMap<OracleSubject, T>,
    order: VecDeque<OracleSubject>,
    retired: BTreeSet<OracleSubject>,
    stats: OracleCacheStats,
}

impl<T> OracleCache<T> {
    /// Create a cache with a finite number of reusable complete results.
    pub fn new(capacity: usize) -> Result<Self, OracleCacheError> {
        if capacity == 0 {
            return Err(OracleCacheError::ZeroCapacity);
        }
        Ok(Self {
            capacity,
            entries: BTreeMap::new(),
            order: VecDeque::new(),
            retired: BTreeSet::new(),
            stats: OracleCacheStats::default(),
        })
    }

    /// Return a result only for the exact complete subject.
    pub fn get(&mut self, subject: &OracleSubject) -> Option<&T> {
        if self.entries.contains_key(subject) {
            self.stats.hits = self.stats.hits.saturating_add(1);
            self.touch(subject);
            self.entries.get(subject)
        } else {
            self.stats.misses = self.stats.misses.saturating_add(1);
            None
        }
    }

    /// Insert a complete result unless work for this retired subject arrived late.
    pub fn insert(&mut self, subject: &OracleSubject, value: T) -> Result<(), OracleCacheError> {
        subject.validate().map_err(OracleCacheError::InvalidSubject)?;
        if self.retired.contains(subject) {
            return Err(OracleCacheError::RetiredSubject);
        }
        self.entries.insert(subject.clone(), value);
        self.touch(subject);
        while self.entries.len() > self.capacity {
            if let Some(oldest) = self.order.pop_front() {
                if oldest != *subject && self.entries.remove(&oldest).is_some() {
                    self.stats.evictions = self.stats.evictions.saturating_add(1);
                }
            } else {
                break;
            }
        }
        Ok(())
    }

    /// Retire a subject so late/cancelled work cannot repopulate it.
    pub fn retire(&mut self, subject: &OracleSubject) {
        self.retired.insert(subject.clone());
        self.entries.remove(subject);
        self.order.retain(|candidate| candidate != subject);
        self.stats.invalidations = self.stats.invalidations.saturating_add(1);
    }

    /// Return deterministic cache counters.
    #[must_use]
    pub const fn stats(&self) -> OracleCacheStats {
        self.stats
    }

    fn touch(&mut self, subject: &OracleSubject) {
        self.order.retain(|candidate| candidate != subject);
        self.order.push_back(subject.clone());
    }
}

/// Cache construction or insertion failure.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OracleCacheError {
    /// The cache must have a finite positive bound.
    ZeroCapacity,
    /// The subject crossed the redaction/shape boundary.
    InvalidSubject(OracleSubjectError),
    /// A retired subject cannot be repopulated by late work.
    RetiredSubject,
}

impl fmt::Display for OracleCacheError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ZeroCapacity => formatter.write_str("oracle cache capacity must be positive"),
            Self::InvalidSubject(error) => error.fmt(formatter),
            Self::RetiredSubject => formatter.write_str("oracle subject has been retired"),
        }
    }
}

impl std::error::Error for OracleCacheError {}

#[cfg(test)]
mod tests {
    use super::*;

    fn subject(root: &str, profile: &str) -> OracleSubject {
        OracleSubject::new(
            "perl-5.40.2-build-a",
            "perlcritic-1.152",
            "sha256:fixture",
            root,
            "sha256:source",
            profile,
            OracleInvocation {
                severity: 3,
                theme: Some("core".to_string()),
                include: vec!["Core".to_string()],
                exclude: vec!["Documentation".to_string()],
                options: vec!["--verbose".to_string()],
            },
            "sha256:environment",
            "process-plan.v1",
            "perlcritic-parser.v2",
        )
        .expect("test subject should be valid")
    }

    #[test]
    fn contradictory_roots_and_profiles_never_share_results() {
        let first = subject("root-a", "sha256:profile-a");
        let second = subject("root-b", "sha256:profile-b");
        let mut cache = OracleCache::new(4).expect("positive capacity");
        cache.insert(&first, "a").expect("first result");
        cache.insert(&second, "b").expect("second result");
        assert_eq!(cache.get(&first), Some(&"a"));
        assert_eq!(cache.get(&second), Some(&"b"));
        assert_ne!(first.digest(), second.digest());
    }

    #[test]
    fn profile_content_movement_changes_the_subject() {
        let before = subject("root", "sha256:profile-before");
        let after = subject("root", "sha256:profile-after");
        assert_ne!(before.digest(), after.digest());
    }

    #[test]
    fn each_non_invocation_identity_axis_changes_the_subject() {
        let baseline = subject("root", "sha256:profile");
        let variants = [
            OracleSubject::new(
                "perl-5.40.2-build-b",
                "perlcritic-1.152",
                "sha256:fixture",
                "root",
                "sha256:source",
                "sha256:profile",
                baseline.invocation.clone(),
                "sha256:environment",
                "process-plan.v1",
                "perlcritic-parser.v2",
            )
            .expect("perl variant"),
            OracleSubject::new(
                "perl-5.40.2-build-a",
                "perlcritic-1.153",
                "sha256:fixture",
                "root",
                "sha256:source",
                "sha256:profile",
                baseline.invocation.clone(),
                "sha256:environment",
                "process-plan.v1",
                "perlcritic-parser.v2",
            )
            .expect("critic variant"),
            OracleSubject::new(
                "perl-5.40.2-build-a",
                "perlcritic-1.152",
                "sha256:fixture-b",
                "root",
                "sha256:source",
                "sha256:profile",
                baseline.invocation.clone(),
                "sha256:environment",
                "process-plan.v1",
                "perlcritic-parser.v2",
            )
            .expect("fixture variant"),
            OracleSubject::new(
                "perl-5.40.2-build-a",
                "perlcritic-1.152",
                "sha256:fixture",
                "root-b",
                "sha256:source",
                "sha256:profile",
                baseline.invocation.clone(),
                "sha256:environment",
                "process-plan.v1",
                "perlcritic-parser.v2",
            )
            .expect("root variant"),
            OracleSubject::new(
                "perl-5.40.2-build-a",
                "perlcritic-1.152",
                "sha256:fixture",
                "root",
                "sha256:source-b",
                "sha256:profile",
                baseline.invocation.clone(),
                "sha256:environment",
                "process-plan.v1",
                "perlcritic-parser.v2",
            )
            .expect("source variant"),
            OracleSubject::new(
                "perl-5.40.2-build-a",
                "perlcritic-1.152",
                "sha256:fixture",
                "root",
                "sha256:source",
                "sha256:profile",
                baseline.invocation.clone(),
                "sha256:environment-b",
                "process-plan.v1",
                "perlcritic-parser.v2",
            )
            .expect("environment variant"),
            OracleSubject::new(
                "perl-5.40.2-build-a",
                "perlcritic-1.152",
                "sha256:fixture",
                "root",
                "sha256:source",
                "sha256:profile",
                baseline.invocation.clone(),
                "sha256:environment",
                "process-plan.v2",
                "perlcritic-parser.v2",
            )
            .expect("process schema variant"),
            OracleSubject::new(
                "perl-5.40.2-build-a",
                "perlcritic-1.152",
                "sha256:fixture",
                "root",
                "sha256:source",
                "sha256:profile",
                baseline.invocation.clone(),
                "sha256:environment",
                "process-plan.v1",
                "perlcritic-parser.v3",
            )
            .expect("parser schema variant"),
        ];
        for variant in variants {
            assert_ne!(baseline.digest(), variant.digest());
        }
    }

    #[test]
    fn each_invocation_axis_changes_the_subject() {
        let baseline = subject("root", "sha256:profile");
        let mut variants = Vec::new();
        for invocation in [
            OracleInvocation { severity: 4, ..baseline.invocation.clone() },
            OracleInvocation { theme: Some("full".to_string()), ..baseline.invocation.clone() },
            OracleInvocation { include: vec!["Naming".to_string()], ..baseline.invocation.clone() },
            OracleInvocation {
                exclude: vec!["ValuesAndExpressions".to_string()],
                ..baseline.invocation.clone()
            },
            OracleInvocation {
                options: vec!["--verbose=4".to_string()],
                ..baseline.invocation.clone()
            },
        ] {
            variants.push(
                OracleSubject::new(
                    "perl-5.40.2-build-a",
                    "perlcritic-1.152",
                    "sha256:fixture",
                    "root",
                    "sha256:source",
                    "sha256:profile",
                    invocation,
                    "sha256:environment",
                    "process-plan.v1",
                    "perlcritic-parser.v2",
                )
                .expect("invocation variant"),
            );
        }
        for variant in variants {
            assert_ne!(baseline.digest(), variant.digest());
        }
    }

    #[test]
    fn tool_and_environment_movement_changes_the_subject() {
        let first = OracleSubject::new(
            "perl-5.40.2-build-a",
            "perlcritic-1.152",
            "sha256:fixture",
            "root",
            "sha256:source",
            "sha256:profile",
            OracleInvocation {
                severity: 3,
                theme: None,
                include: Vec::new(),
                exclude: Vec::new(),
                options: Vec::new(),
            },
            "sha256:environment-a",
            "process-plan.v1",
            "perlcritic-parser.v2",
        )
        .expect("first subject");
        let second = OracleSubject::new(
            "perl-5.40.2-build-b",
            "perlcritic-1.153",
            "sha256:fixture",
            "root",
            "sha256:source",
            "sha256:profile",
            OracleInvocation {
                severity: 3,
                theme: None,
                include: Vec::new(),
                exclude: Vec::new(),
                options: Vec::new(),
            },
            "sha256:environment-b",
            "process-plan.v1",
            "perlcritic-parser.v2",
        )
        .expect("second subject");
        assert_ne!(first.digest(), second.digest());
    }

    #[test]
    fn execution_order_does_not_change_results() {
        let first = subject("root-a", "sha256:profile-a");
        let second = subject("root-b", "sha256:profile-b");
        let mut left = OracleCache::new(4).expect("positive capacity");
        left.insert(&first, "a").expect("first result");
        left.insert(&second, "b").expect("second result");
        let mut right = OracleCache::new(4).expect("positive capacity");
        right.insert(&second, "b").expect("second result");
        right.insert(&first, "a").expect("first result");
        assert_eq!(left.get(&first), right.get(&first));
        assert_eq!(left.get(&second), right.get(&second));
    }

    #[test]
    fn retired_subject_rejects_late_completion() {
        let current = subject("root", "sha256:profile");
        let mut cache = OracleCache::new(1).expect("positive capacity");
        cache.retire(&current);
        assert_eq!(cache.insert(&current, "late"), Err(OracleCacheError::RetiredSubject));
        assert_eq!(cache.stats().invalidations, 1);
    }

    #[test]
    fn cache_is_bounded_and_reports_eviction() {
        let first = subject("root-a", "sha256:profile-a");
        let second = subject("root-b", "sha256:profile-b");
        let mut cache = OracleCache::new(1).expect("positive capacity");
        cache.insert(&first, "a").expect("first result");
        cache.insert(&second, "b").expect("second result");
        assert_eq!(cache.get(&first), None);
        assert_eq!(cache.get(&second), Some(&"b"));
        assert_eq!(cache.stats().evictions, 1);
    }

    #[test]
    fn private_paths_are_not_receipt_identities() {
        let error = OracleSubject::new(
            "perl",
            "critic",
            "fixture",
            "/private/fixture",
            "source",
            "profile",
            OracleInvocation {
                severity: 3,
                theme: None,
                include: Vec::new(),
                exclude: Vec::new(),
                options: Vec::new(),
            },
            "environment",
            "process",
            "parser",
        )
        .expect_err("private path must be rejected");
        assert_eq!(error, OracleSubjectError::PrivatePath { field: "root_identity" });
    }
}
