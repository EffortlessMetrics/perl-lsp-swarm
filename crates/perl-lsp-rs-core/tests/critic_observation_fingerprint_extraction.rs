//! Cross-owner compatibility controls for the native critic extraction (#7410).
//!
//! Keep these tests in the adapter/core test target when policy moves. The
//! native package's own tests must not depend upward on this core helper or on
//! ServerConfig. Equal observation tokens are not equal accepted subjects.

use perl_lsp_rs_core::config::{AcceptedCriticSnapshot, ServerConfig};
use perl_lsp_rs_core::hashing::fnv1a64_hex;
use perl_lsp_rs_core::tooling::perl_critic::{
    EffectiveCriticState, EffectiveNativeCriticConfig, NativeCriticProfile,
};

#[test]
fn native_observation_fingerprint_matches_retained_core_recipe() {
    assert_eq!(
        EffectiveCriticState::Disabled.fingerprint(),
        fnv1a64_hex(b"critic-state-v1"),
    );

    for profile in [NativeCriticProfile::Recommended, NativeCriticProfile::Strict] {
        for severity in [0, 1, 3, 5, u8::MAX] {
            for root in [None, Some(""), Some("root"), Some("根/é")] {
                for (include, exclude) in [
                    (vec![], vec![]),
                    (vec![" rule.b ".into(), "rule.a".into(), "rule.a".into()], vec![]),
                    (vec![], vec!["rule.c".into()]),
                    (vec!["rule.a".into()], vec![" rule.c ".into(), "rule.b".into()]),
                ] {
                    let policy = EffectiveNativeCriticConfig::new(
                        profile, severity, &include, &exclude, root,
                    );
                    let historical_input = format!(
                        "critic-state-v1\u{1f}{}\u{1f}{}\u{1f}{}\u{1f}{}\u{1f}{}",
                        policy.profile.as_str(),
                        policy.severity_threshold,
                        policy.include.join("\u{1f}"),
                        policy.exclude.join("\u{1f}"),
                        policy.owning_root.as_deref().unwrap_or(""),
                    );
                    assert_eq!(
                        policy.fingerprint(),
                        fnv1a64_hex(historical_input.as_bytes()),
                        "the private recipe must preserve the accepted policy's observation token",
                    );
                }
            }
        }
    }
}

#[test]
fn aliased_observation_tokens_do_not_authorize_currentness_or_result_reuse() {
    let a = "native.common.assignment_in_condition".to_string();
    let b = "native.common.printf_format_arity".to_string();
    let c = "native.common.undef_comparison".to_string();
    let first = ServerConfig {
        perlcritic_enabled: true,
        native_critic_profile: "recommended".to_string(),
        perlcritic_severity: 3,
        native_critic_include: vec![a.clone(), b.clone()],
        native_critic_exclude: vec![c.clone()],
        ..ServerConfig::default()
    };
    let second = ServerConfig {
        native_critic_include: vec![a],
        native_critic_exclude: vec![b, c],
        ..first.clone()
    };
    let accepted = AcceptedCriticSnapshot::capture(&first, Some("root"));
    let changed = AcceptedCriticSnapshot::capture(&second, Some("root"));

    // The legacy framing joins both lists with the same separator. This is a
    // compatibility fixture, not a claim that the observation token is unique.
    assert_eq!(accepted.fingerprint(), changed.fingerprint());
    assert_ne!(accepted.state(), changed.state());
    assert!(accepted.is_current(&first));
    assert!(!accepted.is_current(&second));
    assert_ne!(accepted.result_identity_fingerprint(), changed.result_identity_fingerprint());
}
