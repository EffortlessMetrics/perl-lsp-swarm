//! Capability sets exchanged in the `peer/hello` handshake.

#[cfg(test)]
use perl_tdd_support::must;
use serde::{Deserialize, Serialize};

use crate::backend::capabilities::{ControlMode, DebugBackendCapabilities};

/// What the peer (e.g. ptkdb) advertises it can do.
///
/// All fields default to `false` so a minimal peer that only reports stops still
/// deserializes cleanly.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PeerReportedCapabilities {
    /// Peer can resume execution on request.
    #[serde(default)]
    pub can_continue: bool,
    /// Peer can step on request.
    #[serde(default)]
    pub can_step: bool,
    /// Peer can asynchronously pause a running debuggee.
    ///
    /// Distinct from [`Self::can_step`]: pausing a running program typically
    /// needs out-of-band signal delivery, a real capability asymmetry from
    /// stepping, so it is negotiated separately (mirror-mode honesty).
    #[serde(default)]
    pub can_pause: bool,
    /// Peer can evaluate expressions.
    #[serde(default)]
    pub can_evaluate: bool,
    /// Peer accepts source breakpoint sets.
    #[serde(default)]
    pub can_set_breakpoints: bool,
    /// Peer accepts function breakpoint sets.
    #[serde(default)]
    pub can_set_function_breakpoints: bool,
    /// Peer supports conditional breakpoints.
    #[serde(default)]
    pub can_condition_breakpoints: bool,
    /// Peer can report a stack trace.
    #[serde(default)]
    pub can_list_stack: bool,
    /// Peer can list variables/scopes.
    #[serde(default)]
    pub can_list_variables: bool,
    /// Peer can report the set of subroutines in a source.
    #[serde(default)]
    pub can_report_subroutines: bool,
    /// Peer can report breakable lines in a source.
    #[serde(default)]
    pub can_report_breakable_lines: bool,
    /// Peer can report a machine-readable `cause` on a `success: false`
    /// response (#14582).
    ///
    /// Unlike the flags above, this does not gate a *request*: it declares that
    /// the peer speaks the [`PeerFailureCause`] vocabulary when it declines. The
    /// host honours [`PeerResponse::cause`] only from a peer that advertised
    /// this, so a `cause` field belonging to some other protocol dialect cannot
    /// silently steer host triage. A peer that does not advertise it is not
    /// degraded — its failures classify exactly as they did before the field
    /// existed.
    ///
    /// This is deliberately absent from
    /// [`Self::to_backend_capabilities`]: [`DebugBackendCapabilities`] feeds DAP
    /// `supportsX` advertisement, and reporting a failure cause is not a DAP
    /// capability. Adding it there would advertise nothing an editor can use.
    ///
    /// [`PeerFailureCause`]: crate::peer_protocol::message::PeerFailureCause
    /// [`PeerResponse::cause`]: crate::peer_protocol::message::PeerResponse::cause
    #[serde(default)]
    pub can_report_failure_cause: bool,
    /// The control mode the peer wants to operate under.
    #[serde(default)]
    pub control_mode: ControlMode,
}

impl PeerReportedCapabilities {
    /// Translate the peer's self-report into the host-side backend capability
    /// view used for DAP negotiation. Anything the peer did not claim is `false`.
    #[must_use]
    pub fn to_backend_capabilities(&self) -> DebugBackendCapabilities {
        DebugBackendCapabilities {
            source_breakpoints: self.can_set_breakpoints,
            conditional_breakpoints: self.can_set_breakpoints && self.can_condition_breakpoints,
            // v1 peers do not negotiate hit conditions / logpoints / data bps.
            hit_conditions: false,
            logpoints: false,
            function_breakpoints: self.can_set_function_breakpoints,
            data_breakpoints: false,
            evaluate: self.can_evaluate,
            variables: self.can_list_variables,
            scopes: self.can_list_variables,
            stack_trace: self.can_list_stack,
            continue_execution: self.can_continue,
            stepping: self.can_step,
            pause: self.can_pause,
            set_variable: false,
            control_mode: self.control_mode,
        }
    }
}

/// What the host (`perl-dap`) advertises it wants from the peer, in its
/// `peer/hello` response.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HostReportedCapabilities {
    /// Host will send breakpoint sets.
    pub wants_breakpoints: bool,
    /// Host wants stack traces.
    pub wants_stack: bool,
    /// Host wants variables.
    pub wants_variables: bool,
    /// Host wants output forwarded.
    pub wants_output: bool,
    /// Host wants static source facts (breakable lines, subroutines).
    pub wants_source_facts: bool,
}

impl Default for HostReportedCapabilities {
    fn default() -> Self {
        Self {
            wants_breakpoints: true,
            wants_stack: true,
            wants_variables: true,
            wants_output: true,
            wants_source_facts: true,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn minimal_peer_report_deserializes_with_defaults() {
        // A peer that only reports stops sends an almost-empty capability map.
        let caps: PeerReportedCapabilities = must(serde_json::from_str("{}"));
        assert!(!caps.can_continue);
        assert!(!caps.can_evaluate);
        assert_eq!(caps.control_mode, ControlMode::Mirror);
    }

    #[test]
    fn conditional_requires_both_flags() {
        let caps = PeerReportedCapabilities {
            can_set_breakpoints: true,
            can_condition_breakpoints: false,
            ..Default::default()
        };
        assert!(!caps.to_backend_capabilities().conditional_breakpoints);

        let caps2 = PeerReportedCapabilities {
            can_set_breakpoints: true,
            can_condition_breakpoints: true,
            ..Default::default()
        };
        assert!(caps2.to_backend_capabilities().conditional_breakpoints);
    }

    #[test]
    fn backend_view_never_invents_unsupported_features() {
        let caps = PeerReportedCapabilities {
            can_set_breakpoints: true,
            can_evaluate: true,
            can_step: true,
            can_list_stack: true,
            can_list_variables: true,
            ..Default::default()
        };
        let b = caps.to_backend_capabilities();
        assert!(!b.logpoints);
        assert!(!b.hit_conditions);
        assert!(!b.data_breakpoints);
        assert!(!b.set_variable);
        // pause must NOT be inferred from can_step — the peer did not advertise
        // can_pause, so pause stays off (mirror-mode honesty).
        assert!(!b.pause, "pause must not be invented from can_step");
        assert!(b.evaluate && b.stepping && b.stack_trace);
    }

    #[test]
    fn continue_and_step_are_negotiated_independently() {
        // A peer that can resume but not single-step must map to
        // continue_execution=true, stepping=false — DAP `continue` stays
        // available even though next/stepIn/stepOut do not.
        let resume_only =
            PeerReportedCapabilities { can_continue: true, can_step: false, ..Default::default() };
        let b = resume_only.to_backend_capabilities();
        assert!(b.continue_execution, "can_continue must enable resume");
        assert!(!b.stepping, "can_step=false must keep stepping off");

        // And the inverse: a stepping-only peer still reports it can continue
        // only if it said so.
        let step_only =
            PeerReportedCapabilities { can_continue: false, can_step: true, ..Default::default() };
        let b2 = step_only.to_backend_capabilities();
        assert!(!b2.continue_execution, "continue must not be invented from can_step");
        assert!(b2.stepping);
    }

    /// A peer built before #14582 sends no such key, and must read as "cannot".
    #[test]
    fn failure_cause_reporting_is_off_for_a_peer_that_never_mentions_it() {
        let caps: PeerReportedCapabilities = must(serde_json::from_str("{}"));
        assert!(!caps.can_report_failure_cause);

        let declared: PeerReportedCapabilities =
            must(serde_json::from_str(r#"{"canReportFailureCause":true}"#));
        assert!(declared.can_report_failure_cause, "camelCase key must bind the flag");
    }

    /// Negative control on the DAP surface.
    ///
    /// `can_report_failure_cause` is a peer-protocol capability about how the
    /// host *classifies* a failure. It must not leak into
    /// [`DebugBackendCapabilities`], which feeds DAP `supportsX` advertisement —
    /// there is no editor-facing promise here, and inventing one would advertise
    /// a capability no editor can consume.
    #[test]
    fn failure_cause_reporting_does_not_move_the_dap_capability_view() {
        let silent = PeerReportedCapabilities {
            can_set_breakpoints: true,
            can_evaluate: true,
            can_list_stack: true,
            can_report_failure_cause: false,
            ..Default::default()
        };
        let declared = PeerReportedCapabilities { can_report_failure_cause: true, ..silent };

        assert_eq!(
            silent.to_backend_capabilities(),
            declared.to_backend_capabilities(),
            "advertising a failure cause must not widen any DAP capability"
        );
    }

    #[test]
    fn host_capabilities_default_wants_everything() {
        let h = HostReportedCapabilities::default();
        assert!(h.wants_breakpoints && h.wants_stack && h.wants_source_facts);
    }
}
