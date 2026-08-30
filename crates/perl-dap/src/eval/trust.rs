//! Trust boundary for DAP `evaluate` side effects (#9385).
//!
//! The DAP `evaluate` request carries a `context` label describing *why* the
//! client is evaluating: a watch expression, an editor hover, the variables
//! view, or the interactive debug console (`repl`). Those contexts do not
//! deserve the same execution authority.
//!
//! Read-oriented contexts are populated by the editor without a deliberate user
//! action — a hover fires by moving the mouse, and watch expressions re-evaluate
//! on every stop. Executing arbitrary side-effectful Perl on that path would let
//! passive inspection mutate the debuggee. Only the explicit `repl` context
//! represents a user typing an expression they intend to run.
//!
//! This module owns that decision as a pure function so it can be proven
//! exhaustively without a debugger session, and so the admission decision is
//! made *before* any debugger command is constructed.
//!
//! # What this is not
//!
//! Trusted REPL execution is **not** sandboxed. It is not a `Safe.pm`
//! compartment and not an isolated interpreter. When the REPL boundary admits
//! an expression, that expression runs with the debuggee's full authority. The
//! guarantee here is narrow and deliberate: side-effectful evaluation is
//! confined to the one context where the user explicitly asked for it, and
//! cannot be reached from the passive inspection contexts.

use crate::backend::EvaluateContext;

/// Product policy for the trusted REPL execution surface.
///
/// This is process-owned state. It is deliberately not derived from project or
/// workspace configuration: a checked-in project file must never be able to
/// grant broader execution authority to whoever opens the folder (#9385).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ReplTrustPolicy {
    /// The interactive REPL may execute side-effectful Perl in the debuggee.
    ///
    /// This is the default, preserving the established behavior of the debug
    /// console: a user typing at the console is performing a deliberate act.
    #[default]
    TrustedReplEnabled,
    /// No context may execute side-effectful Perl; screening always applies.
    ReplDisabled,
}

/// Typed admission decision for one `evaluate` request.
///
/// Produced before any debugger command is constructed, so a refusal cannot be
/// preceded by a debugger write.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EvaluateAdmission {
    /// Explicit trusted REPL execution: expression screening is bypassed by
    /// request, and the expression executes with the debuggee's authority.
    TrustedReplExecution,
    /// Expression screening applies before the expression reaches the debuggee.
    Screened,
    /// Side effects were requested outside the REPL boundary.
    ///
    /// The request is refused rather than silently downgraded to a screened
    /// evaluation, so a client cannot quietly believe it received side-effect
    /// authority it never had.
    RefusedOutsideRepl,
    /// Side effects were requested in the REPL, but trusted REPL execution is
    /// disabled by product policy.
    RefusedReplDisabled,
}

impl EvaluateAdmission {
    /// The client-visible refusal message, if this decision refuses.
    #[must_use]
    pub fn refusal_message(self) -> Option<String> {
        match self {
            Self::TrustedReplExecution | Self::Screened => None,
            Self::RefusedOutsideRepl => Some(
                "allowSideEffects is only honored for the 'repl' evaluation context; \
                 side-effectful evaluation is not available from watch, hover, \
                 variables, or an unspecified context"
                    .to_string(),
            ),
            Self::RefusedReplDisabled => Some(
                "Side-effectful REPL evaluation is disabled by policy for this session".to_string(),
            ),
        }
    }
}

/// Decide whether one `evaluate` request may execute side-effectful Perl.
///
/// `context` is the parsed DAP evaluate context, or `None` when the client did
/// not send one. A missing context is treated conservatively: it is not the
/// REPL, so it never carries side-effect authority.
#[must_use]
pub fn admit(
    context: Option<&EvaluateContext>,
    allow_side_effects: bool,
    policy: ReplTrustPolicy,
) -> EvaluateAdmission {
    if !allow_side_effects {
        // The ordinary path for every context, including the REPL: the
        // expression is screened before it reaches the debuggee.
        return EvaluateAdmission::Screened;
    }

    if !matches!(context, Some(EvaluateContext::Repl)) {
        return EvaluateAdmission::RefusedOutsideRepl;
    }

    match policy {
        ReplTrustPolicy::TrustedReplEnabled => EvaluateAdmission::TrustedReplExecution,
        ReplTrustPolicy::ReplDisabled => EvaluateAdmission::RefusedReplDisabled,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every context that is not the REPL, including the unknown-label and
    /// absent cases the DAP schema permits.
    fn non_repl_contexts() -> Vec<Option<EvaluateContext>> {
        vec![
            None,
            Some(EvaluateContext::Watch),
            Some(EvaluateContext::Hover),
            Some(EvaluateContext::Variables),
            Some(EvaluateContext::Other("clipboard".to_string())),
            Some(EvaluateContext::Other("".to_string())),
            Some(EvaluateContext::Other("REPL".to_string())),
        ]
    }

    #[test]
    fn side_effects_are_refused_outside_the_repl_context() {
        for context in non_repl_contexts() {
            let decision = admit(context.as_ref(), true, ReplTrustPolicy::TrustedReplEnabled);
            assert_eq!(
                decision,
                EvaluateAdmission::RefusedOutsideRepl,
                "context {context:?} must not carry side-effect authority"
            );
        }
    }

    #[test]
    fn absent_context_is_conservative_rather_than_repl() {
        // Guards the fail-closed default: a client that omits `context` must not
        // inherit REPL authority just because the field is optional.
        assert_eq!(
            admit(None, true, ReplTrustPolicy::TrustedReplEnabled),
            EvaluateAdmission::RefusedOutsideRepl
        );
    }

    #[test]
    fn repl_context_admits_side_effects_when_trusted() {
        assert_eq!(
            admit(Some(&EvaluateContext::Repl), true, ReplTrustPolicy::TrustedReplEnabled),
            EvaluateAdmission::TrustedReplExecution
        );
    }

    #[test]
    fn repl_context_is_refused_when_trusted_repl_is_disabled() {
        assert_eq!(
            admit(Some(&EvaluateContext::Repl), true, ReplTrustPolicy::ReplDisabled),
            EvaluateAdmission::RefusedReplDisabled
        );
    }

    #[test]
    fn screening_applies_to_every_context_when_side_effects_are_not_requested() {
        let mut contexts = non_repl_contexts();
        contexts.push(Some(EvaluateContext::Repl));
        for context in contexts {
            for policy in [ReplTrustPolicy::TrustedReplEnabled, ReplTrustPolicy::ReplDisabled] {
                assert_eq!(
                    admit(context.as_ref(), false, policy),
                    EvaluateAdmission::Screened,
                    "context {context:?} with policy {policy:?} must be screened"
                );
            }
        }
    }

    #[test]
    fn disabling_trusted_repl_cannot_widen_a_non_repl_context() {
        // Negative control: the policy toggle must not become a second way to
        // reach execution authority from a read-oriented context.
        for context in non_repl_contexts() {
            assert_eq!(
                admit(context.as_ref(), true, ReplTrustPolicy::ReplDisabled),
                EvaluateAdmission::RefusedOutsideRepl
            );
        }
    }

    #[test]
    fn only_admitted_decisions_carry_no_refusal_message() {
        assert!(EvaluateAdmission::TrustedReplExecution.refusal_message().is_none());
        assert!(EvaluateAdmission::Screened.refusal_message().is_none());
        assert!(EvaluateAdmission::RefusedOutsideRepl.refusal_message().is_some());
        assert!(EvaluateAdmission::RefusedReplDisabled.refusal_message().is_some());
    }

    #[test]
    fn default_policy_preserves_interactive_repl_execution() {
        assert_eq!(ReplTrustPolicy::default(), ReplTrustPolicy::TrustedReplEnabled);
    }
}
