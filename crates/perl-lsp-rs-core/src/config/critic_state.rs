//! One accepted critic configuration authority (#8253).
//!
//! The shipped critic runtime has exactly one accepted state family:
//!
//! ```text
//! EffectiveCriticState::Disabled
//! EffectiveCriticState::Native(EffectiveNativeCriticConfig)
//! ```
//!
//! External Perl::Critic is not a runtime engine (#8253 ruling): deprecated
//! `legacy`/`external`/`perlcritic` inputs are migration observations owned by
//! #9072/#9068 and can never construct an external product state here. The
//! derivation reads no `.perlcriticrc`, no PATH, and no external
//! profile/theme/executable field, so ambient tool state has zero selection
//! authority over the accepted value.
//!
//! This module publishes a read-only derived domain view plus transactional
//! candidate validation at the single existing configuration boundary. It
//! deliberately creates no second mutable effective-state store: the raw
//! fields remain in [`ServerConfig`] until #10857's accepted-store cutover,
//! and downstream consumers receive this immutable object through #9062
//! instead of reconstructing critic state from mutable settings.

use super::{
    CriticEngine, CriticRuleIdSource, ProjectCriticConfig, ProjectDiagnosticsConfig, ServerConfig,
    as_config_u64, normalize_string_list, warn_unknown_rule_ids,
};
use crate::hashing::fnv1a64_hex;
use crate::tooling::perl_critic::NativeCriticProfile;

/// Separator used when serializing accepted-state content into its fingerprint
/// input. Chosen so ordinary setting values cannot collide across fields.
const FINGERPRINT_FIELD_SEPARATOR: &str = "\u{1f}";

/// Version tag binding the fingerprint recipe to this accepted-state shape.
const FINGERPRINT_RECIPE_VERSION: &str = "critic-state-v1";

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
    /// Two states with equal fingerprints carry equal accepted policy;
    /// restart reconstruction from the same inputs reproduces the same value.
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
pub(crate) fn canonical_rule_ids(values: &[String]) -> Vec<String> {
    let mut canonical: Vec<String> = values
        .iter()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .collect();
    canonical.sort();
    canonical.dedup();
    canonical
}

impl ServerConfig {
    /// Derive the accepted critic state for one owning subject (#8253).
    ///
    /// This is the read-only seam #9062 consumes instead of reconstructing
    /// critic policy from mutable settings. Normalization law:
    ///
    /// - `enabled = false` yields [`EffectiveCriticState::Disabled`], which
    ///   carries no live native analysis policy;
    /// - `enabled = true` yields `Native(complete accepted config)`;
    /// - deprecated external engine/profile/theme fields are never read, so
    ///   they cannot select or parameterize product behavior here;
    /// - equal effective inputs always derive an equal accepted value.
    #[must_use]
    pub fn effective_critic_state(&self, owning_root: Option<&str>) -> EffectiveCriticState {
        if !self.perlcritic_enabled {
            return EffectiveCriticState::Disabled;
        }
        EffectiveCriticState::Native(EffectiveNativeCriticConfig {
            profile: NativeCriticProfile::parse(&self.native_critic_profile).unwrap_or_default(),
            severity_threshold: self.perlcritic_severity.clamp(1, 5),
            include: canonical_rule_ids(&self.native_critic_include),
            exclude: canonical_rule_ids(&self.native_critic_exclude),
            owning_root: owning_root.map(ToOwned::to_owned),
        })
    }
}

/// One offending configuration sibling discovered while validating a candidate.
#[derive(Debug)]
struct RejectedSibling {
    setting: &'static str,
    value: String,
}

impl RejectedSibling {
    fn new(setting: &'static str, value: impl std::fmt::Display) -> Self {
        Self { setting, value: value.to_string() }
    }
}

/// A complete critic candidate failed validation; nothing may be applied.
///
/// The prior complete accepted sibling set is retained atomically and exactly
/// one deduplicated condition is emitted for the whole rejected candidate.
#[derive(Debug)]
pub(crate) struct CriticCandidateRejection {
    siblings: Vec<RejectedSibling>,
}

impl CriticCandidateRejection {
    pub(crate) fn emit_single_condition(&self) {
        let Some(first) = self.siblings.first() else {
            return;
        };
        let keys: Vec<&str> = self.siblings.iter().map(|sibling| sibling.setting).collect();
        tracing::warn!(
            target: "perl_lsp::config",
            setting = first.setting,
            value = %first.value,
            rejected_keys = ?keys,
            "rejecting complete critic configuration candidate; \
             every critic sibling retained at its prior accepted value",
        );
    }
}

/// One validated, all-or-nothing critic settings candidate.
///
/// Every present behavior-bearing sibling is parsed and normalized before any
/// mutation happens. An empty candidate applies nothing (the payload named no
/// critic setting).
#[derive(Debug, Default)]
pub(crate) struct CriticSettingsCandidate {
    enabled: Option<bool>,
    severity: Option<u8>,
    profile: Option<NativeCriticProfile>,
    include: Option<Vec<String>>,
    exclude: Option<Vec<String>>,
    /// Interim raw-engine carrier. The accepted state family never represents
    /// an engine (#8253); this field only preserves current raw-field behavior
    /// for pre-migration consumers until #9062 reroutes them and #9068 deletes
    /// the external path.
    engine_raw: Option<CriticEngine>,
}

impl CriticSettingsCandidate {
    pub(crate) fn is_empty(&self) -> bool {
        self.enabled.is_none()
            && self.severity.is_none()
            && self.profile.is_none()
            && self.include.is_none()
            && self.exclude.is_none()
            && self.engine_raw.is_none()
    }

    /// Apply every validated sibling as one accepted transaction.
    ///
    /// Callers must only invoke this after candidate validation succeeded, so
    /// no partial application can ever pair siblings across generations.
    pub(crate) fn apply_to(self, config: &mut ServerConfig) {
        if let Some(enabled) = self.enabled {
            config.perlcritic_enabled = enabled;
        }
        if let Some(severity) = self.severity {
            config.perlcritic_severity = severity;
        }
        if let Some(profile) = self.profile {
            config.native_critic_profile = profile.as_str().to_string();
        }
        if let Some(include) = self.include {
            config.native_critic_include = include;
        }
        if let Some(exclude) = self.exclude {
            config.native_critic_exclude = exclude;
        }
        if let Some(engine) = self.engine_raw {
            config.critic_engine = engine;
        }
    }

    /// Parse one LSP-channel payload (`initializationOptions`,
    /// `didChangeConfiguration`, `workspace/configuration`) into a candidate.
    ///
    /// Legacy `perlcritic.*` keys seed shared enablement/severity and native
    /// `critic.*` keys override them (#3276); both blocks form ONE candidate so
    /// mixed-generation sibling sets are impossible. Deprecated engine aliases
    /// (`legacy`/`external`/`perlcritic`) are recorded as migration
    /// observations and cannot construct any runtime engine state (#9072).
    pub(crate) fn parse_lsp_update(
        settings: &serde_json::Value,
    ) -> Result<Self, CriticCandidateRejection> {
        let mut candidate = Self::default();
        let mut rejected: Vec<RejectedSibling> = Vec::new();

        if let Some(legacy) = settings.get("perlcritic") {
            parse_enabled(legacy, "perlcritic.enabled", &mut candidate, &mut rejected);
            parse_severity(legacy, "perlcritic.severity", &mut candidate, &mut rejected);
        }

        if let Some(native) = settings.get("critic") {
            parse_enabled(native, "critic.enabled", &mut candidate, &mut rejected);
            parse_severity(native, "critic.severity", &mut candidate, &mut rejected);
            match native.get("engine") {
                None => {}
                Some(value) => match value.as_str() {
                    Some("native") => candidate.engine_raw = Some(CriticEngine::Native),
                    Some(alias)
                        if matches!(
                            alias.trim().to_ascii_lowercase().as_str(),
                            "legacy" | "external" | "perlcritic"
                        ) =>
                    {
                        tracing::warn!(
                            target: "perl_lsp::config",
                            setting = "critic.engine",
                            value = %alias,
                            "ignoring deprecated critic.engine from LSP settings channel; \
                             external Perl::Critic is not a runtime engine (#8253), \
                             migration owned by #9072",
                        );
                    }
                    Some(other) => rejected.push(RejectedSibling::new("critic.engine", other)),
                    None => rejected.push(RejectedSibling::new("critic.engine", value.to_string())),
                },
            }
            parse_profile(native, "critic.profile", &mut candidate, &mut rejected);
            parse_rule_list(native, "critic.include", &mut candidate, &mut rejected);
            parse_rule_list(native, "critic.exclude", &mut candidate, &mut rejected);
        }

        finish_candidate(candidate, rejected)
    }

    /// Parse the trusted `.perl-lsp.toml` `[diagnostics]`/`[critic]` sections
    /// into one initialization candidate.
    ///
    /// The trusted project channel may still select the deprecated external
    /// engine until #9072/#9068 remove it; that selection stays confined to
    /// pre-migration raw consumers and can never appear in the accepted state
    /// family.
    pub(crate) fn parse_project_config(
        diagnostics: &ProjectDiagnosticsConfig,
        critic: &ProjectCriticConfig,
    ) -> Result<Self, CriticCandidateRejection> {
        let mut candidate = Self {
            enabled: diagnostics.perlcritic,
            severity: diagnostics.perlcritic_severity.map(|severity| {
                let clamped = severity.clamp(1, 5);
                if clamped != severity {
                    tracing::warn!(
                        target: "perl_lsp::config",
                        setting = "diagnostics.perlcritic_severity",
                        value = severity,
                        valid_range = "1-5",
                        "perlcritic_severity out of range; clamped to {}",
                        clamped,
                    );
                }
                clamped
            }),
            ..Self::default()
        };
        let mut rejected: Vec<RejectedSibling> = Vec::new();

        if let Some(engine) = &critic.engine {
            match crate::config::parse_critic_engine(engine) {
                Some(parsed) => {
                    if parsed == CriticEngine::Legacy {
                        tracing::warn!(
                            target: "perl_lsp::config",
                            setting = "critic.engine",
                            value = %engine,
                            "deprecated critic.engine in .perl-lsp.toml selects the \
                             external compatibility path only; it cannot construct a \
                             product runtime engine state (#8253), migration owned by #9072",
                        );
                    }
                    candidate.engine_raw = Some(parsed);
                }
                None => rejected.push(RejectedSibling::new("critic.engine", engine)),
            }
        }
        if let Some(profile) = &critic.profile {
            match NativeCriticProfile::parse(profile) {
                Some(parsed) => candidate.profile = Some(parsed),
                None => rejected.push(RejectedSibling::new("critic.profile", profile)),
            }
        }
        candidate.include = critic.include.as_ref().map(|ids| {
            let normalized = normalize_string_list(ids);
            warn_unknown_rule_ids(CriticRuleIdSource::ProjectFile, "critic.include", &normalized);
            normalized
        });
        candidate.exclude = critic.exclude.as_ref().map(|ids| {
            let normalized = normalize_string_list(ids);
            warn_unknown_rule_ids(CriticRuleIdSource::ProjectFile, "critic.exclude", &normalized);
            normalized
        });

        finish_candidate(candidate, rejected)
    }
}

/// Return the validated candidate or a whole-candidate rejection.
fn finish_candidate(
    candidate: CriticSettingsCandidate,
    rejected: Vec<RejectedSibling>,
) -> Result<CriticSettingsCandidate, CriticCandidateRejection> {
    if rejected.is_empty() {
        return Ok(candidate);
    }
    Err(CriticCandidateRejection { siblings: rejected })
}

fn parse_enabled(
    block: &serde_json::Value,
    setting: &'static str,
    candidate: &mut CriticSettingsCandidate,
    rejected: &mut Vec<RejectedSibling>,
) {
    match block.get("enabled") {
        None => {}
        Some(value) => match value.as_bool() {
            Some(enabled) => candidate.enabled = Some(enabled),
            None => rejected.push(RejectedSibling::new(setting, value.to_string())),
        },
    }
}

fn parse_severity(
    block: &serde_json::Value,
    setting: &'static str,
    candidate: &mut CriticSettingsCandidate,
    rejected: &mut Vec<RejectedSibling>,
) {
    match block.get("severity") {
        None => {}
        Some(value) => match as_config_u64(value) {
            Some(raw) => {
                let clamped = raw.clamp(1, 5) as u8;
                if u64::from(clamped) != raw {
                    tracing::warn!(
                        target: "perl_lsp::config",
                        setting = setting,
                        value = raw,
                        valid_range = "1-5",
                        "critic severity out of range; clamped to {}",
                        clamped,
                    );
                }
                candidate.severity = Some(clamped);
            }
            None => rejected.push(RejectedSibling::new(setting, value.to_string())),
        },
    }
}

fn parse_profile(
    block: &serde_json::Value,
    setting: &'static str,
    candidate: &mut CriticSettingsCandidate,
    rejected: &mut Vec<RejectedSibling>,
) {
    match block.get("profile") {
        None => {}
        Some(value) => match value.as_str().and_then(NativeCriticProfile::parse) {
            Some(profile) => candidate.profile = Some(profile),
            None => rejected.push(RejectedSibling::new(setting, value.to_string())),
        },
    }
}

fn parse_rule_list(
    block: &serde_json::Value,
    setting: &'static str,
    candidate: &mut CriticSettingsCandidate,
    rejected: &mut Vec<RejectedSibling>,
) {
    let key = setting.rsplit('.').next().unwrap_or(setting);
    match block.get(key) {
        None => {}
        Some(value) => match value.as_array() {
            Some(entries) => {
                let mut ids = Vec::with_capacity(entries.len());
                let mut malformed: Option<String> = None;
                for entry in entries {
                    match entry.as_str() {
                        Some(id) => ids.push(id.to_string()),
                        None => {
                            malformed = Some(entry.to_string());
                            break;
                        }
                    }
                }
                if let Some(entry) = malformed {
                    rejected.push(RejectedSibling::new(setting, entry));
                    return;
                }
                let normalized = normalize_string_list(&ids);
                warn_unknown_rule_ids(CriticRuleIdSource::ClientSettings, setting, &normalized);
                if key == "include" {
                    candidate.include = Some(normalized);
                } else {
                    candidate.exclude = Some(normalized);
                }
            }
            None => rejected.push(RejectedSibling::new(setting, value.to_string())),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::super::ProjectConfig;
    use super::*;

    fn native_config(root: Option<&str>) -> EffectiveNativeCriticConfig {
        EffectiveNativeCriticConfig {
            profile: NativeCriticProfile::Recommended,
            severity_threshold: 3,
            include: vec!["native.testing.require_use_strict".to_string()],
            exclude: Vec::new(),
            owning_root: root.map(ToOwned::to_owned),
        }
    }

    #[test]
    fn disabled_carries_no_policy_or_root() {
        let state = EffectiveCriticState::Disabled;
        assert!(state.owning_root().is_none());
        assert!(
            matches!(state, EffectiveCriticState::Disabled),
            "disabled must be representable without any native policy payload"
        );
    }

    #[test]
    fn fingerprint_is_deterministic_and_content_bound() {
        let a = EffectiveCriticState::Native(native_config(Some("root-a")));
        let b = EffectiveCriticState::Native(native_config(Some("root-a")));
        assert_eq!(a.fingerprint(), b.fingerprint());

        let other_profile = EffectiveNativeCriticConfig {
            profile: NativeCriticProfile::Strict,
            ..native_config(Some("root-a"))
        };
        assert_ne!(a.fingerprint(), EffectiveCriticState::Native(other_profile).fingerprint());
    }

    #[test]
    fn root_identity_binds_the_accepted_subject() {
        let root_a = EffectiveCriticState::Native(native_config(Some("root-a")));
        let root_b = EffectiveCriticState::Native(native_config(Some("root-b")));
        assert_ne!(root_a.fingerprint(), root_b.fingerprint());
        assert_eq!(root_a.owning_root(), Some("root-a"));
        assert_eq!(root_b.owning_root(), Some("root-b"));
        assert_eq!(
            EffectiveCriticState::Native(native_config(None)).owning_root(),
            None,
            "server-global subject stays distinct from every folder subject"
        );
    }

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
    fn accepted_state_serializes_to_one_canonical_representation() {
        let state = EffectiveCriticState::Native(native_config(Some("root-a")));
        let serialized = serde_json::to_string(&state).unwrap_or_else(|error| {
            panic!("accepted state must serialize: {error}");
        });
        let round_tripped: EffectiveCriticState =
            serde_json::from_str(&serialized).unwrap_or_else(|error| {
                panic!("canonical representation must deserialize: {error}");
            });
        assert_eq!(state, round_tripped);
        assert!(
            !serialized.contains("engine"),
            "no engine selector may appear in the canonical runtime representation"
        );

        let disabled = serde_json::to_string(&EffectiveCriticState::Disabled)
            .unwrap_or_else(|error| panic!("disabled must serialize: {error}"));
        assert_eq!(disabled, "\"Disabled\"");
    }

    #[test]
    fn disabled_fingerprint_differs_from_any_native_state() {
        assert_ne!(
            EffectiveCriticState::Disabled.fingerprint(),
            EffectiveCriticState::Native(native_config(None)).fingerprint()
        );
    }

    fn default_native_state(root: Option<&str>) -> EffectiveCriticState {
        EffectiveCriticState::Native(EffectiveNativeCriticConfig {
            profile: NativeCriticProfile::Recommended,
            severity_threshold: 3,
            include: Vec::new(),
            exclude: Vec::new(),
            owning_root: root.map(ToOwned::to_owned),
        })
    }

    fn strict_native_state(root: Option<&str>, severity: u8) -> EffectiveCriticState {
        EffectiveCriticState::Native(EffectiveNativeCriticConfig {
            profile: NativeCriticProfile::Strict,
            severity_threshold: severity,
            include: Vec::new(),
            exclude: Vec::new(),
            owning_root: root.map(ToOwned::to_owned),
        })
    }

    #[test]
    fn default_enabled_state_selects_reviewed_native_default_policy() {
        let config = ServerConfig::default();
        assert_eq!(config.effective_critic_state(None), default_native_state(None));
    }

    #[test]
    fn transition_contract_native_to_disabled_and_back() {
        let mut config = ServerConfig::default();
        let native_a = config.effective_critic_state(Some("root"));

        config.update_from_value(&serde_json::json!({ "critic": { "enabled": false } }));
        let disabled = config.effective_critic_state(Some("root"));
        assert!(matches!(disabled, EffectiveCriticState::Disabled));

        config.update_from_value(&serde_json::json!({
            "critic": { "enabled": true, "profile": "recommended", "severity": 3 }
        }));
        assert_eq!(
            config.effective_critic_state(Some("root")),
            native_a,
            "Disabled → Native(A) reconstructs the same accepted subject"
        );
    }

    #[test]
    fn disabled_state_carries_no_live_native_policy() {
        let mut config = ServerConfig::default();
        config.update_from_value(&serde_json::json!({
            "critic": { "enabled": false }
        }));

        match config.effective_critic_state(Some("root")) {
            EffectiveCriticState::Disabled => {}
            EffectiveCriticState::Native(_) => {
                panic!("disabled state must not expose a native policy object")
            }
        }
        assert!(
            config.effective_critic_state(Some("root")).owning_root().is_none(),
            "disabled carries no policy subject that could run"
        );
    }

    #[test]
    fn transition_contract_native_a_to_b_and_back_is_deterministic() {
        let mut config = ServerConfig::default();
        let native_a = config.effective_critic_state(Some("root"));
        let a_fingerprint = native_a.fingerprint();

        config.update_from_value(&serde_json::json!({
            "critic": { "profile": "strict", "severity": 4 }
        }));
        let native_b = config.effective_critic_state(Some("root"));
        assert_eq!(native_b, strict_native_state(Some("root"), 4));
        assert_ne!(native_b.fingerprint(), a_fingerprint);

        config.update_from_value(&serde_json::json!({
            "critic": { "profile": "recommended", "severity": 3 }
        }));
        assert_eq!(
            config.effective_critic_state(Some("root")).fingerprint(),
            a_fingerprint,
            "Native(B) → Native(A) restores the exact prior policy identity"
        );
    }

    #[test]
    fn invalid_sibling_rejects_the_complete_candidate() {
        let mut config = ServerConfig::default();
        config.update_from_value(&serde_json::json!({
            "critic": { "profile": "strict", "severity": 5 }
        }));
        let before = config.effective_critic_state(Some("root"));

        // Valid sibling paired with an invalid one must not partially mutate.
        config.update_from_value(&serde_json::json!({
            "critic": {
                "include": ["native.testing.require_use_strict"],
                "profile": "recomended"
            }
        }));

        assert_eq!(
            config.effective_critic_state(Some("root")),
            before,
            "one invalid sibling rejects the whole candidate atomically"
        );
        assert!(
            config.native_critic_include.is_empty(),
            "no new-generation filter may pair with the retained profile"
        );
    }

    #[test]
    fn malformed_sibling_type_rejects_the_complete_candidate() {
        let mut config = ServerConfig::default();
        config.update_from_value(&serde_json::json!({
            "critic": { "profile": "strict" }
        }));
        let before = config.effective_critic_state(Some("root"));

        config.update_from_value(&serde_json::json!({
            "critic": {
                "enabled": true,
                "exclude": "native.common.assignment_in_condition"
            }
        }));

        assert_eq!(config.effective_critic_state(Some("root")), before);
        assert!(config.native_critic_exclude.is_empty());
    }

    #[test]
    fn same_effective_update_is_idempotent_across_spellings_and_order() {
        let mut config = ServerConfig::default();
        config.update_from_value(&serde_json::json!({
            "critic": {
                "include": ["native.testing.require_use_warnings", "native.testing.require_use_strict"],
                "severity": 4
            }
        }));
        let first = config.effective_critic_state(Some("root"));

        config.update_from_value(&serde_json::json!({
            "critic": {
                "severity": 4.0,
                "include": ["  native.testing.require_use_strict  ", "native.testing.require_use_warnings"]
            }
        }));
        let second = config.effective_critic_state(Some("root"));

        assert_eq!(first, second, "equivalent effective policy produces no churn");
        assert_eq!(first.fingerprint(), second.fingerprint());

        config.update_from_value(&serde_json::json!({
            "critic": { "severity": 4.0 }
        }));
        assert_eq!(
            config.effective_critic_state(Some("root")),
            second,
            "re-applying the identical effective update changes nothing"
        );
    }

    #[test]
    fn multi_root_contradictory_policies_stay_folder_owned() {
        let global_config = ServerConfig::default();

        let mut root_a_config = ServerConfig::default();
        root_a_config.update_from_value(&serde_json::json!({
            "critic": { "profile": "strict", "severity": 5 }
        }));

        let state_global = global_config.effective_critic_state(Some("root-b"));
        let state_a = root_a_config.effective_critic_state(Some("root-a"));
        let state_b = root_a_config.effective_critic_state(Some("root-b"));

        assert_eq!(state_a.owning_root(), Some("root-a"));
        assert_ne!(
            state_a.fingerprint(),
            state_b.fingerprint(),
            "identical raw inputs on different roots are different accepted subjects"
        );
        assert_ne!(
            state_a.fingerprint(),
            global_config.effective_critic_state(Some("root-a")).fingerprint(),
            "root A cannot be satisfied by another generation's policy"
        );
    }

    #[test]
    fn external_residue_has_zero_selection_authority_over_accepted_state() {
        let clean = ServerConfig::default();
        let mut contaminated = ServerConfig::default();
        contaminated.critic_engine = CriticEngine::Legacy;
        contaminated.perlcritic_profile = Some("/discovered/.perlcriticrc".to_string());
        contaminated.perlcritic_theme = Some("core && !pbp".to_string());

        assert_eq!(
            clean.effective_critic_state(Some("root")),
            contaminated.effective_critic_state(Some("root")),
            ".perlcriticrc paths, themes, and deprecated engine selection cannot \
             alter the accepted implementation or its fingerprint"
        );
    }

    #[test]
    fn deprecated_engine_inputs_cannot_construct_an_external_accepted_state() {
        for alias in ["legacy", "external", "perlcritic"] {
            let mut config = ServerConfig::default();
            config.update_from_value(&serde_json::json!({
                "critic": { "engine": alias }
            }));
            let state = config.effective_critic_state(Some("root"));
            assert!(
                matches!(state, EffectiveCriticState::Disabled | EffectiveCriticState::Native(_)),
                "alias {alias} must stay inside the Disabled|Native family"
            );
            assert_eq!(
                state,
                default_native_state(Some("root")),
                "deprecated alias {alias} must not change accepted product policy"
            );
        }
    }

    #[test]
    fn project_file_initialization_rejects_invalid_candidates_atomically() {
        let mut config = ServerConfig::default();
        config.update_from_value(&serde_json::json!({
            "critic": { "profile": "strict" }
        }));
        let before = config.effective_critic_state(Some("root"));

        let mut project = ProjectConfig::default();
        project.diagnostics.perlcritic_severity = Some(2);
        project.critic.profile = Some("recomended".to_string());
        project.critic.include = Some(vec!["native.testing.require_use_strict".to_string()]);
        project.apply_to_server_config(&mut config);

        assert_eq!(
            config.effective_critic_state(Some("root")),
            before,
            "invalid project initialization retains the complete prior state"
        );
        assert_eq!(config.perlcritic_severity, 3);
        assert!(config.native_critic_include.is_empty());
    }

    #[test]
    fn project_file_trusted_legacy_selection_stays_out_of_accepted_family() {
        let mut config = ServerConfig::default();
        let mut project = ProjectConfig::default();
        project.critic.engine = Some("legacy".to_string());
        project.critic.profile = Some("strict".to_string());
        project.apply_to_server_config(&mut config);

        assert_eq!(config.critic_engine, CriticEngine::Legacy);
        assert_eq!(
            config.effective_critic_state(Some("root")),
            strict_native_state(Some("root"), 3),
            "trusted external selection maps to complete native accepted policy; \
             no external runtime state exists"
        );
    }

    #[test]
    fn restart_reconstruction_yields_identical_canonical_state() {
        let build = || {
            let mut config = ServerConfig::default();
            config.update_from_value(&serde_json::json!({
                "critic": {
                    "enabled": true,
                    "profile": "strict",
                    "severity": 2,
                    "include": ["native.testing.require_use_strict"],
                    "exclude": ["native.common.assignment_in_condition"]
                }
            }));
            config.effective_critic_state(Some("workspace-root"))
        };
        assert_eq!(
            build().fingerprint(),
            build().fingerprint(),
            "restart from the same inputs reproduces the same canonical state"
        );
    }
}
