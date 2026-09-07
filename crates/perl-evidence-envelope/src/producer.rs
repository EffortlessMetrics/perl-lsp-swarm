//! Identity of the producer that emitted an evidence envelope.

use serde::{Deserialize, Serialize};

/// Identity of the tool/process that produced an evidence envelope.
///
/// All four fields are plain, producer-supplied strings. This crate does not
/// validate their internal shape (e.g. that `version` is semver, or that
/// `source_sha` is a real commit) — it only carries them as identity material
/// for the deterministic [`crate::EnvelopeFingerprint`].
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ProducerIdentity {
    /// Producer name, e.g. `"perl-lsp-test-runner"`.
    pub name: String,
    /// Producer version, e.g. a semver string or build tag.
    pub version: String,
    /// The exact source commit SHA the producer was built from.
    pub source_sha: String,
    /// A build-identity string distinguishing otherwise-identical builds
    /// (e.g. a CI job ID, a reproducible-build hash, or a toolchain
    /// fingerprint). Producers with no meaningful build identity should use
    /// an explicit sentinel such as `"unknown"` rather than an empty string.
    pub build_identity: String,
}

impl ProducerIdentity {
    /// Construct a producer identity from its four fields.
    #[must_use]
    pub fn new(
        name: impl Into<String>,
        version: impl Into<String>,
        source_sha: impl Into<String>,
        build_identity: impl Into<String>,
    ) -> Self {
        Self {
            name: name.into(),
            version: version.into(),
            source_sha: source_sha.into(),
            build_identity: build_identity.into(),
        }
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]

    use super::*;

    fn sample() -> ProducerIdentity {
        ProducerIdentity::new("perl-lsp-test-runner", "0.17.0", "abc123", "ci-build-42")
    }

    #[test]
    fn producer_identity_serde_round_trip() {
        let original = sample();
        let json = serde_json::to_string(&original).expect("serialize");
        let back: ProducerIdentity = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(original, back);
    }

    #[test]
    fn producer_identity_distinguishes_each_field() {
        let base = sample();
        let mut with_name = base.clone();
        with_name.name = "other-producer".to_string();
        assert_ne!(base, with_name);

        let mut with_version = base.clone();
        with_version.version = "0.18.0".to_string();
        assert_ne!(base, with_version);

        let mut with_sha = base.clone();
        with_sha.source_sha = "def456".to_string();
        assert_ne!(base, with_sha);

        let mut with_build = base.clone();
        with_build.build_identity = "ci-build-43".to_string();
        assert_ne!(base, with_build);
    }
}
