#!/usr/bin/env python3
"""Apply the bounded #13901 typed OracleSubject identity repair."""

from pathlib import Path


PATH = Path("crates/perl-core-harness/src/critic_oracle.rs")
text = PATH.read_text(encoding="utf-8")


def replace_once(source: str, old: str, new: str, label: str) -> str:
    count = source.count(old)
    if count != 1:
        raise SystemExit(f"{label}: expected exactly one match, found {count}")
    return source.replace(old, new, 1)


identity_anchor = """const MAX_IDENTITY_LENGTH: usize = 256;

/// The immutable identity of one complete Perl::Critic conformance subject.
"""
identity_replacement = """const MAX_IDENTITY_LENGTH: usize = 256;

/// Named, total non-invocation identity for one Perl::Critic oracle subject.
///
/// A struct literal keeps every identity axis visible at the construction site
/// and prevents the string-position swaps possible with the former ten-argument
/// constructor. Deliberately no `Default`: omitting an axis is a compile error.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OracleSubjectIdentity {
    /// Exact redacted Perl executable/build identity.
    pub perl_identity: String,
    /// Exact redacted Perl::Critic installation identity.
    pub critic_identity: String,
    /// Digest of the fixture inventory used by the oracle run.
    pub fixture_digest: String,
    /// Path-free identity of the fixture root.
    pub root_identity: String,
    /// Digest of the source presented to the oracle.
    pub source_digest: String,
    /// Digest of the effective Perl::Critic profile contents.
    pub profile_digest: String,
    /// Digest of the reviewed execution environment.
    pub environment_digest: String,
    /// Process-supervision schema identity.
    pub process_schema: String,
    /// Native-output parser schema identity.
    pub parser_schema: String,
}

/// The immutable identity of one complete Perl::Critic conformance subject.
"""
text = replace_once(text, identity_anchor, identity_replacement, "identity input insertion")

old_constructor = """    pub fn new(
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
"""
new_constructor = """    pub fn new(
        identity: OracleSubjectIdentity,
        invocation: OracleInvocation,
    ) -> Result<Self, OracleSubjectError> {
        let OracleSubjectIdentity {
            perl_identity,
            critic_identity,
            fixture_digest,
            root_identity,
            source_digest,
            profile_digest,
            environment_digest,
            process_schema,
            parser_schema,
        } = identity;
        let subject = Self {
            perl_identity,
            critic_identity,
            fixture_digest,
            root_identity,
            source_digest,
            profile_digest,
            invocation,
            environment_digest,
            process_schema,
            parser_schema,
        };
"""
text = replace_once(text, old_constructor, new_constructor, "OracleSubject constructor")

marker = "#[cfg(test)]\nmod tests {"
if text.count(marker) != 1:
    raise SystemExit(f"test module marker: expected exactly one match, found {text.count(marker)}")
prefix = text.split(marker, 1)[0]

tests = r'''#[cfg(test)]
mod tests {
    use super::*;

    fn identity(root: &str, profile: &str) -> OracleSubjectIdentity {
        OracleSubjectIdentity {
            perl_identity: "perl-5.40.2-build-a".to_string(),
            critic_identity: "perlcritic-1.152".to_string(),
            fixture_digest: "sha256:fixture".to_string(),
            root_identity: root.to_string(),
            source_digest: "sha256:source".to_string(),
            profile_digest: profile.to_string(),
            environment_digest: "sha256:environment".to_string(),
            process_schema: "process-plan.v1".to_string(),
            parser_schema: "perlcritic-parser.v2".to_string(),
        }
    }

    fn invocation() -> OracleInvocation {
        OracleInvocation {
            severity: 3,
            theme: Some("core".to_string()),
            include: vec!["Core".to_string()],
            exclude: vec!["Documentation".to_string()],
            options: vec!["--verbose".to_string()],
        }
    }

    fn subject(root: &str, profile: &str) -> OracleSubject {
        OracleSubject::new(identity(root, profile), invocation())
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
        let baseline_identity = identity("root", "sha256:profile");
        let baseline = OracleSubject::new(baseline_identity.clone(), invocation())
            .expect("baseline subject");
        let variants = [
            OracleSubjectIdentity {
                perl_identity: "perl-5.40.2-build-b".to_string(),
                ..baseline_identity.clone()
            },
            OracleSubjectIdentity {
                critic_identity: "perlcritic-1.153".to_string(),
                ..baseline_identity.clone()
            },
            OracleSubjectIdentity {
                fixture_digest: "sha256:fixture-b".to_string(),
                ..baseline_identity.clone()
            },
            OracleSubjectIdentity {
                root_identity: "root-b".to_string(),
                ..baseline_identity.clone()
            },
            OracleSubjectIdentity {
                source_digest: "sha256:source-b".to_string(),
                ..baseline_identity.clone()
            },
            OracleSubjectIdentity {
                environment_digest: "sha256:environment-b".to_string(),
                ..baseline_identity.clone()
            },
            OracleSubjectIdentity {
                process_schema: "process-plan.v2".to_string(),
                ..baseline_identity.clone()
            },
            OracleSubjectIdentity {
                parser_schema: "perlcritic-parser.v3".to_string(),
                ..baseline_identity
            },
        ];
        for identity in variants {
            let variant = OracleSubject::new(identity, baseline.invocation.clone())
                .expect("identity variant");
            assert_ne!(baseline.digest(), variant.digest());
        }
    }

    #[test]
    fn each_invocation_axis_changes_the_subject() {
        let baseline_identity = identity("root", "sha256:profile");
        let baseline = OracleSubject::new(baseline_identity.clone(), invocation())
            .expect("baseline subject");
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
                OracleSubject::new(baseline_identity.clone(), invocation)
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
            OracleSubjectIdentity {
                environment_digest: "sha256:environment-a".to_string(),
                ..identity("root", "sha256:profile")
            },
            OracleInvocation {
                severity: 3,
                theme: None,
                include: Vec::new(),
                exclude: Vec::new(),
                options: Vec::new(),
            },
        )
        .expect("first subject");
        let second = OracleSubject::new(
            OracleSubjectIdentity {
                perl_identity: "perl-5.40.2-build-b".to_string(),
                critic_identity: "perlcritic-1.153".to_string(),
                environment_digest: "sha256:environment-b".to_string(),
                ..identity("root", "sha256:profile")
            },
            OracleInvocation {
                severity: 3,
                theme: None,
                include: Vec::new(),
                exclude: Vec::new(),
                options: Vec::new(),
            },
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
            OracleSubjectIdentity {
                perl_identity: "perl".to_string(),
                critic_identity: "critic".to_string(),
                fixture_digest: "fixture".to_string(),
                root_identity: "/private/fixture".to_string(),
                source_digest: "source".to_string(),
                profile_digest: "profile".to_string(),
                environment_digest: "environment".to_string(),
                process_schema: "process".to_string(),
                parser_schema: "parser".to_string(),
            },
            OracleInvocation {
                severity: 3,
                theme: None,
                include: Vec::new(),
                exclude: Vec::new(),
                options: Vec::new(),
            },
        )
        .expect_err("private path must be rejected");
        assert_eq!(error, OracleSubjectError::PrivatePath { field: "root_identity" });
    }
}
'''

PATH.write_text(prefix + tests, encoding="utf-8")
print(f"patched {PATH}")
