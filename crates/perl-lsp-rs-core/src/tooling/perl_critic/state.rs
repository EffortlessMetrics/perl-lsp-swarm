//! Immutable native critic policy values (#8253/#7410).
//!
//! Configuration acquisition, validation, root selection, snapshot capture and
//! live currentness checks stay with the server adapter. This module carries
//! only the accepted Disabled/Native values and their observation fingerprint.

use super::NativeCriticProfile;

/// Historical separator for the observation fingerprint's fields and lists.
///
/// Preserve this framing for compatibility. It is not unambiguous: moving a
/// selector across the include/exclude boundary can retain the same bytes.
/// Currentness and reusable result identity must use their separate owners.
const FINGERPRINT_FIELD_SEPARATOR: &str = "\u{1f}";

/// Version tag binding the fingerprint recipe to this accepted-state shape.
const FINGERPRINT_RECIPE_VERSION: &str = "critic-state-v1";

/// Retained observation-only FNV recipe, private to critic policy (#7410).
///
/// Keep the offset, wrapping update, prefix and output width identical to the
/// former core helper. This is not a new public hashing or identity authority.
fn fnv1a64_hex(bytes: &[u8]) -> String {
    let mut hash: u64 = 0xcbf29ce484222325;
    for byte in bytes {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    format!("fnv1a64:{hash:016x}")
}

/// One accepted critic runtime state (#8253).
///
/// Disabled carries no policy payload at all, so a disabled critic cannot
/// accidentally retain live native analysis policy. Native always carries the
/// complete behavior-bearing policy as one accepted transaction.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum EffectiveCriticState {
    /// Critic analysis is disabled. No policy object exists in this state.
    Disabled,
    /// Critic analysis runs with the complete accepted native policy.
    Native(EffectiveNativeCriticConfig),
}

impl EffectiveCriticState {
    /// Deterministic identity of this accepted state.
    ///
    /// This compact FNV observation token predates reusable result identities.
    /// It is deterministic, but it is not a collision-resistant authority and
    /// must not decide whether a client-held diagnostic result ID is reusable.
    #[must_use]
    pub fn fingerprint(&self) -> String {
        match self {
            Self::Disabled => fnv1a64_hex(FINGERPRINT_RECIPE_VERSION.as_bytes()),
            Self::Native(native) => native.fingerprint(),
        }
    }

    /// Owning folder/root identity of this accepted subject, when bound.
    #[must_use]
    pub fn owning_root(&self) -> Option<&str> {
        match self {
            Self::Disabled => None,
            Self::Native(native) => native.owning_root.as_deref(),
        }
    }
}

/// Complete accepted native critic policy (#8253).
///
/// Every behavior-bearing sibling advances as one transaction: consumers can
/// rely on profile, threshold, filters, ownership, and fingerprint describing
/// one configuration generation. No external executable, profile path, theme,
/// engine selector, or process authority is representable here.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct EffectiveNativeCriticConfig {
    /// Canonical selected native rule bundle.
    pub profile: NativeCriticProfile,
    /// Minimum severity threshold reported (1..=5, 5 = most severe).
    pub severity_threshold: u8,
    /// Canonical included rule IDs (trimmed, deduplicated, sorted).
    pub include: Vec<String>,
    /// Canonical excluded rule IDs (trimmed, deduplicated, sorted).
    pub exclude: Vec<String>,
    /// Owning folder/root identity; `None` is the server-global subject.
    pub owning_root: Option<String>,
}

impl EffectiveNativeCriticConfig {
    /// Canonicalize one complete native policy from an adapter's captured inputs.
    ///
    /// Severity is clamped to the supported 1..=5 range. Filter spellings are
    /// trimmed, empty entries removed, and IDs sorted and deduplicated. This
    /// performs no configuration acquisition, rule evaluation or external I/O.
    #[must_use]
    pub fn new(
        profile: NativeCriticProfile,
        severity_threshold: u8,
        include: &[String],
        exclude: &[String],
        owning_root: Option<&str>,
    ) -> Self {
        Self {
            profile,
            severity_threshold: severity_threshold.clamp(1, 5),
            include: canonical_rule_ids(include),
            exclude: canonical_rule_ids(exclude),
            owning_root: owning_root.map(ToOwned::to_owned),
        }
    }

    /// Deterministic fingerprint over the complete canonical policy content.
    #[must_use]
    pub fn fingerprint(&self) -> String {
        let content = format!(
            "{FINGERPRINT_RECIPE_VERSION}{sep}{profile}{sep}{severity}{sep}{include}{sep}{exclude}{sep}{root}",
            sep = FINGERPRINT_FIELD_SEPARATOR,
            profile = self.profile.as_str(),
            severity = self.severity_threshold,
            include = self.include.join(FINGERPRINT_FIELD_SEPARATOR),
            exclude = self.exclude.join(FINGERPRINT_FIELD_SEPARATOR),
            root = self.owning_root.as_deref().unwrap_or(""),
        );
        fnv1a64_hex(content.as_bytes())
    }
}

/// Canonicalize a filter list: trimmed, empties dropped, deduplicated, sorted.
///
/// Sorting makes the accepted policy order-insensitive so equivalent updates
/// spelled in different orders derive one identical accepted value.
fn canonical_rule_ids(values: &[String]) -> Vec<String> {
    let mut canonical: Vec<String> = values
        .iter()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .collect();
    canonical.sort();
    canonical.dedup();
    canonical
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn filter_canonicalization_is_order_and_spelling_insensitive() {
        let spelled_first = canonical_rule_ids(&[
            "native.testing.require_use_warnings ".to_string(),
            "native.testing.require_use_strict".to_string(),
            "".to_string(),
            "native.testing.require_use_strict".to_string(),
        ]);
        let spelled_second = canonical_rule_ids(&[
            "native.testing.require_use_strict".to_string(),
            "native.testing.require_use_warnings".to_string(),
        ]);
        assert_eq!(
            spelled_first,
            vec![
                "native.testing.require_use_strict".to_string(),
                "native.testing.require_use_warnings".to_string()
            ]
        );
        assert_eq!(
            spelled_first, spelled_second,
            "input order must not change the canonical accepted filter set"
        );
    }

    #[test]
    fn canonical_policy_constructor_preserves_supported_thresholds_and_bounds() {
        let include = vec![" rule.b ".into(), "rule.a".into(), "rule.a".into(), " ".into()];
        let exclude = vec![" rule.c ".into(), "rule.c".into()];
        for (input, expected) in [(0, 1), (1, 1), (3, 3), (5, 5), (u8::MAX, 5)] {
            let policy = EffectiveNativeCriticConfig::new(
                NativeCriticProfile::Strict,
                input,
                &include,
                &exclude,
                Some("root"),
            );
            assert_eq!(policy.profile, NativeCriticProfile::Strict);
            assert_eq!(policy.severity_threshold, expected);
            assert_eq!(policy.include, ["rule.a", "rule.b"]);
            assert_eq!(policy.exclude, ["rule.c"]);
            assert_eq!(policy.owning_root.as_deref(), Some("root"));
        }
    }

    #[test]
    fn observation_fingerprint_preserves_known_byte_vectors() {
        for (bytes, expected) in [
            (b"".as_slice(), "fnv1a64:cbf29ce484222325"),
            (b"hello".as_slice(), "fnv1a64:a430d84680aabd0b"),
            (b"ch".as_slice(), "fnv1a64:08a25607b54a22ae"),
            ([0, 127, 128, 255].as_slice(), "fnv1a64:29bdf07e674617ed"),
        ] {
            assert_eq!(fnv1a64_hex(bytes), expected);
        }
    }

    #[test]
    fn accepted_state_fingerprint_preserves_version_and_field_framing() {
        assert_eq!(EffectiveCriticState::Disabled.fingerprint(), "fnv1a64:ba3def9e9270e641");
        let recommended = EffectiveNativeCriticConfig::new(
            NativeCriticProfile::Recommended,
            3,
            &[],
            &[],
            None,
        );
        assert_eq!(recommended.fingerprint(), "fnv1a64:e8a46b38363505a2");

        let strict = EffectiveNativeCriticConfig::new(
            NativeCriticProfile::Strict,
            5,
            &[" rule.b ".into(), "rule.a".into(), "rule.a".into()],
            &["rule.c".into()],
            Some("根/é"),
        );
        assert_eq!(strict.fingerprint(), "fnv1a64:5180b484d3b34cbd");
    }
}
