//! Backend capability description and DAP capability negotiation.
//!
//! DAP capabilities advertised to the editor must be the **intersection** of
//! what the feature catalog supports and what the selected backend can actually
//! do (decision D6). A ptkdb peer in `mirror` mode, for example, must not cause
//! `perl-dap` to advertise control commands the peer never offered.

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Who owns stepping/control in an external-peer session.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ControlMode {
    /// The external UI (ptkdb) owns control; the IDE mirrors state. Friendliest
    /// first integration and the only fully-exercised mode today.
    #[default]
    Mirror,
    /// Both the IDE and the external UI may send control commands.
    Cooperative,
    /// The IDE owns control; the external tool is mostly a UI/data provider.
    DapControlled,
}

/// What a backend can do, after any negotiation with its engine/peer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct DebugBackendCapabilities {
    /// Can set source-line breakpoints.
    pub source_breakpoints: bool,
    /// Supports conditional breakpoints.
    pub conditional_breakpoints: bool,
    /// Supports hit-count conditions.
    pub hit_conditions: bool,
    /// Supports logpoints.
    pub logpoints: bool,
    /// Supports function/subroutine breakpoints.
    pub function_breakpoints: bool,
    /// Supports data/watchpoints.
    pub data_breakpoints: bool,
    /// Supports expression evaluation.
    pub evaluate: bool,
    /// Can list variables.
    pub variables: bool,
    /// Can list scopes.
    pub scopes: bool,
    /// Can produce stack traces.
    pub stack_trace: bool,
    /// Supports resuming execution (DAP `continue`).
    ///
    /// Distinct from [`Self::stepping`]: a peer can be able to resume a stopped
    /// program without supporting single-step, so the two are negotiated
    /// separately (mirror-mode honesty).
    pub continue_execution: bool,
    /// Supports stepping (next/stepIn/stepOut).
    pub stepping: bool,
    /// Supports pause.
    pub pause: bool,
    /// Supports setting variable values.
    pub set_variable: bool,
    /// The control mode this backend/session operates under.
    pub control_mode: ControlMode,
}

impl DebugBackendCapabilities {
    /// Everything on (used by mocks / a fully-capable native engine).
    #[must_use]
    pub fn full() -> Self {
        Self {
            source_breakpoints: true,
            conditional_breakpoints: true,
            hit_conditions: true,
            logpoints: true,
            function_breakpoints: true,
            data_breakpoints: true,
            evaluate: true,
            variables: true,
            scopes: true,
            stack_trace: true,
            continue_execution: true,
            stepping: true,
            pause: true,
            set_variable: true,
            control_mode: ControlMode::DapControlled,
        }
    }

    /// Nothing on. Starting point before a peer negotiates.
    #[must_use]
    pub fn none() -> Self {
        Self {
            source_breakpoints: false,
            conditional_breakpoints: false,
            hit_conditions: false,
            logpoints: false,
            function_breakpoints: false,
            data_breakpoints: false,
            evaluate: false,
            variables: false,
            scopes: false,
            stack_trace: false,
            continue_execution: false,
            stepping: false,
            pause: false,
            set_variable: false,
            control_mode: ControlMode::Mirror,
        }
    }

    /// The realistic capability floor for a `Devel::ptkdb` peer at protocol v1.
    ///
    /// ptkdb documents conditional breakpoints, sub breakpoints, expression
    /// evaluation, stack and variable inspection — but not DAP-style logpoints,
    /// hit conditions, or data breakpoints. This is a *documentation* default;
    /// the live value is whatever the peer negotiates in its `peer/hello`.
    #[must_use]
    pub fn ptkdb_v1_defaults() -> Self {
        Self {
            source_breakpoints: true,
            conditional_breakpoints: true,
            hit_conditions: false,
            logpoints: false,
            function_breakpoints: true,
            data_breakpoints: false,
            evaluate: true,
            variables: true,
            scopes: true,
            stack_trace: true,
            continue_execution: true,
            stepping: true,
            pause: true,
            set_variable: false,
            control_mode: ControlMode::Mirror,
        }
    }
}

/// The catalog-side (feature.toml-derived) view of which DAP features are
/// compiled/advertised. Passed in explicitly so negotiation is unit-testable
/// without the build-time catalog.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CatalogDapFlags {
    /// `dap.core` — core requests (evaluate-for-hovers, function bps, set var).
    pub core: bool,
    /// `dap.breakpoints.basic` — conditional breakpoints, breakpoint locations.
    pub breakpoints_basic: bool,
    /// `dap.breakpoints.hit_condition`.
    pub hit_condition: bool,
    /// `dap.breakpoints.logpoints`.
    pub logpoints: bool,
    /// `dap.watchpoints` — data breakpoints.
    pub watchpoints: bool,
    /// `dap.breakpoints.function`.
    pub function_breakpoints: bool,
}

impl CatalogDapFlags {
    /// Resolve the catalog flags from the build-time feature catalog.
    #[must_use]
    pub fn from_catalog() -> Self {
        use crate::feature_catalog::has_feature;
        Self {
            core: has_feature("dap.core"),
            breakpoints_basic: has_feature("dap.breakpoints.basic"),
            hit_condition: has_feature("dap.breakpoints.hit_condition"),
            logpoints: has_feature("dap.breakpoints.logpoints"),
            watchpoints: has_feature("dap.watchpoints"),
            function_breakpoints: has_feature("dap.breakpoints.function"),
        }
    }
}

/// Whether a pure, selected-frame hover inspection path has been proven (#9573).
///
/// DAP's `supportsEvaluateForHovers` is a promise that an `evaluate` request
/// carrying `context: "hover"` is a *pure inspection* of the frame the client
/// selected. `perl-dap` has no such path yet: `handle_evaluate` issues a raw
/// perl5db command against the debugger's *current* frame, and the custom
/// `allowSideEffects` field can widen the screened expression subset. Until the
/// parser-backed pure inspection path exists, advertising the capability would
/// invite editors to route hover text into a general Perl evaluator whose frame
/// and side-effect semantics are not the claimed feature.
///
/// Flipping this to `true` requires the re-enable gate recorded on #9573:
/// selected-frame identity, an immutable value graph, bounded parser-backed
/// inspection, trust routing that keeps REPL authority out of hover, and exact
/// public-stdio proof. Nothing else — not `dap.core`, not backend `evaluate`,
/// not handler presence, not a successful raw evaluation — may widen it.
pub(crate) const PURE_HOVER_INSPECTION_PROVEN: bool = false;

/// The single authority for the advertised `supportsEvaluateForHovers` value.
///
/// This deliberately consumes no catalog flag, backend flag, or handler-presence
/// signal. Hover support is gated on a proof that does not exist yet, so the
/// value is derived from [`PURE_HOVER_INSPECTION_PROVEN`] alone (#9573).
#[must_use]
pub(crate) const fn advertises_evaluate_for_hovers() -> bool {
    PURE_HOVER_INSPECTION_PROVEN
}

/// The DAP standard `context` value for hover evaluation.
const HOVER_EVALUATE_CONTEXT: &str = "hover";

/// Refusal message used when hover-context evaluation is declined (#9573).
pub(crate) const HOVER_UNSUPPORTED_MESSAGE: &str = "evaluate with context 'hover' is not supported: supportsEvaluateForHovers is advertised \
     false because perl-dap has no pure selected-frame inspection path yet (#9573)";

/// Whether an `evaluate` request's `context` selects hover.
///
/// Matched ASCII-case-insensitively. The DAP-standard spelling is lowercase
/// `hover`; while the capability is closed, accepting case variants only ever
/// *widens the refusal*, so a client sending `Hover` cannot slip past the floor.
///
/// A missing or unrecognised context is deliberately **not** hover. Those keep
/// their own conservative policy rather than being silently reclassified as
/// hover or REPL (#9573).
///
/// Every backend mode routes its hover refusal through this one predicate so
/// native, attach, and external-peer sessions cannot drift apart.
#[must_use]
pub(crate) fn is_hover_evaluate_context(context: Option<&str>) -> bool {
    context.is_some_and(|value| value.eq_ignore_ascii_case(HOVER_EVALUATE_CONTEXT))
}

/// Whether a mode must refuse this `evaluate` request as unsupported hover.
///
/// The invariant every mode holds: **a mode refuses hover exactly when it does
/// not advertise hover.** `advertised_hover` is the value that *this* mode puts
/// on the wire for `supportsEvaluateForHovers`, so admission and advertisement
/// can never disagree.
///
/// This matters at promotion time, not just today. If the refusal ignored the
/// advertised value, flipping [`PURE_HOVER_INSPECTION_PROVEN`] would advertise
/// hover while still rejecting every hover request — the same
/// capability-versus-behaviour contradiction #9573 exists to remove, only
/// pointing the other way.
///
/// Modes stay independent: each passes its own advertised value, so promoting
/// the native gate does not silently open an external-peer path that has no
/// pure inspection of its own.
#[must_use]
pub(crate) fn refuse_hover_evaluation(advertised_hover: bool, context: Option<&str>) -> bool {
    !advertised_hover && is_hover_evaluate_context(context)
}

/// The `supportsEvaluateForHovers` value the external-peer bridge advertises.
///
/// Deliberately **independent of [`PURE_HOVER_INSPECTION_PROVEN`]**, which is
/// the *native* proof gate. An external peer runs its own evaluator and has no
/// pure selected-frame inspection of its own, so promoting the native gate must
/// not silently open a path that routes hover text to a live external debugger.
/// #9573 states this directly: "Keep processId attach, TCP, and external peer
/// modes independently false unless they have their own pure hover
/// implementation and proof."
///
/// Promoting this requires that separate peer-side proof.
pub(crate) const PEER_BRIDGE_ADVERTISES_EVALUATE_FOR_HOVERS: bool = false;

/// The external-peer bridge's hover decision, as a pure function.
///
/// `native_hover_gate` is accepted and **deliberately not used**. That is the
/// property under test, not an oversight: the peer mode must not inherit the
/// native proof gate (#9573). Taking it as a parameter is what makes the
/// independence provable in CI — a test can evaluate this under both possible
/// native values and require the result to be identical, without mutating any
/// constant. If someone later re-reads the native gate here, that test fails.
///
/// The peer's own gate decides, still intersected with whether the peer can
/// evaluate at all, so promoting the peer gate cannot over-advertise against a
/// peer that offered no evaluation.
#[must_use]
pub(crate) fn peer_bridge_hover_admission(
    _native_hover_gate: bool,
    peer_hover_gate: bool,
    backend_can_evaluate: bool,
) -> bool {
    peer_hover_gate && backend_can_evaluate
}

/// The `supportsEvaluateForHovers` value the static mirror profile advertises.
///
/// Mirror mode is conservative by construction and has no pure hover inspection
/// of its own, so it stays false independently of the native gate (#9573). Both
/// `static_mirror_capabilities` and the mirror request gate read this, so the
/// profile cannot advertise one thing and enforce another.
pub(crate) const MIRROR_ADVERTISES_EVALUATE_FOR_HOVERS: bool = false;

/// The #9581 secondary-capability floor: one explicit unsupported disposition
/// per floored request.
///
/// Seven `initialize` capability fields — `supportsCompletionsRequest`,
/// `supportsModulesRequest`, `supportsLoadedSourcesRequest`,
/// `supportsRestartRequest`, `supportsValueFormattingOptions`,
/// `supportsBreakpointLocationsRequest`, and `supportsCancelRequest` — are
/// forced `false` in every mode (native launch, TCP attach, and both mirror
/// peer surfaces) until that field's own exact-behavior receipt passes (#9581).
/// Each row is independent: one field's gate evidence never widens another, and
/// no row is derived from `supports_core`, catalog maturity, handler presence,
/// or another mode's support.
///
/// While a field is floored, its request is rejected by the dispatcher *before*
/// any handler runs, so the floored path can perform no debugger I/O, process
/// action, or state mutation, and a missing/unavailable session can never
/// masquerade as a successful empty result (#9581). Re-enable is per field,
/// owned by the per-feature implementation/proof issues named in each message.
pub(crate) fn secondary_capability_floor_message(command: &str) -> Option<String> {
    let (capability, gate) = match command {
        "completions" => {
            ("supportsCompletionsRequest", "#9021 + #9046 + #9050 + #8581 + #9582 + #9584")
        }
        "modules" => ("supportsModulesRequest", "#8581 + #7667/#8668 + #9585 + #9586"),
        "loadedSources" => ("supportsLoadedSourcesRequest", "#8581 + #7667/#8668 + #9585 + #9586"),
        "restart" => {
            ("supportsRestartRequest", "#9051 + #8691/#8703 + #8974 + #9587 + #8726 + #7568")
        }
        "breakpointLocations" => {
            ("supportsBreakpointLocationsRequest", "#10524 + #2300 + #9021 + #7566")
        }
        "cancel" => ("supportsCancelRequest", "#9074 + #8712 + #7568"),
        _ => return None,
    };
    Some(format!(
        "`{command}` is unsupported: `{capability}` is false for this adapter \
         (#9581 secondary-capability floor; exact semantics unproven, \
         re-enable gate: {gate}). The request was rejected before any debugger \
         interaction, so no state was read or changed."
    ))
}

/// The `ValueFormat` families whose `format` option is floored (#9581):
/// `variables`, `setVariable`, `evaluate`, and `setExpression`.
///
/// Requests without a `format` option (or with a default-equivalent one) keep
/// their independently supported contract; only a *non-default* request
/// (`hex: true`, the pinned schema's single property) is rejected, and it is
/// rejected before any debugger/value mutation (#9581). Re-enable gate:
/// #9050 + #8364 + #9070 + #7342/#7345 + #9588 + #9590.
pub(crate) fn unproven_value_format_requested(command: &str, arguments: Option<&Value>) -> bool {
    matches!(command, "variables" | "setVariable" | "evaluate" | "setExpression")
        && arguments
            .and_then(|arguments| arguments.get("format"))
            .and_then(|format| format.get("hex"))
            .and_then(Value::as_bool)
            == Some(true)
}

fn value_format_unknown_field(command: &str, arguments: Option<&Value>) -> Option<String> {
    if !matches!(command, "variables" | "setVariable" | "evaluate" | "setExpression") {
        return None;
    }
    arguments
        .and_then(|arguments| arguments.get("format"))
        .and_then(Value::as_object)
        .and_then(|format| format.keys().find(|key| key.as_str() != "hex"))
        .cloned()
}

fn value_format_invalid_message(command: &str, field: &str) -> String {
    format!(
        "`{command}`: Invalid arguments: `format` contains unknown field `{field}`; the pinned DAP ValueFormat schema only permits `hex`"
    )
}

/// The explicit unsupported disposition for a floored `format` option (#9581).
pub(crate) fn value_format_unsupported_message(command: &str) -> String {
    format!(
        "`{command}` is unsupported: a non-default `format` option was sent while \
         `supportsValueFormattingOptions` is false for this adapter (#9581 \
         secondary-capability floor; re-enable gate: #9050 + #8364 + #9070 + \
         #7342/#7345 + #9588 + #9590). The request was rejected before any \
         debugger interaction; resend without `format` for the default \
         presentation."
    )
}

/// The one #9581 floor decision for a request, combining both floored
/// families: the six secondary requests and a non-default `format` option on
/// the four ValueFormat families.
///
/// Every production surface — the native dispatch seams
/// (`DebugAdapter::secondary_capability_floor_response`) and both peer
/// frontends (`secondary_floor_response`) — applies this single function at
/// its own sanctioned seam and constructs only its own refusal response from
/// it, so the two families cannot drift apart between surfaces.
pub(crate) fn capability_floor_message(command: &str, arguments: Option<&Value>) -> Option<String> {
    if let Some(message) = secondary_capability_floor_message(command) {
        return Some(message);
    }
    if let Some(field) = value_format_unknown_field(command, arguments) {
        return Some(value_format_invalid_message(command, &field));
    }
    if unproven_value_format_requested(command, arguments) {
        return Some(value_format_unsupported_message(command));
    }
    None
}

/// The negotiated DAP capability flags: catalog ∩ backend.
///
/// Field names mirror the DAP `capabilities` payload keys the frontend emits in
/// `process.rs` so the mapping is obvious.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NegotiatedDapCapabilities {
    /// `supportsConditionalBreakpoints`.
    pub supports_conditional_breakpoints: bool,
    /// `supportsHitConditionalBreakpoints`.
    pub supports_hit_conditional_breakpoints: bool,
    /// `supportsLogPoints`.
    pub supports_log_points: bool,
    /// `supportsFunctionBreakpoints`.
    pub supports_function_breakpoints: bool,
    /// `supportsDataBreakpoints`.
    pub supports_data_breakpoints: bool,
    /// `supportsEvaluateForHovers`.
    ///
    /// Pinned false by the crate-internal hover authority, independently of
    /// [`Self::supports_evaluate`], until a pure selected-frame inspection path
    /// is proven (#9573).
    pub supports_evaluate_for_hovers: bool,
    /// Whether the backend can serve a general `evaluate` request.
    ///
    /// DAP has no `supportsEvaluate` wire capability — `evaluate` is always
    /// requestable — so this is an internal backend fact, not an advertised
    /// flag. It is kept separate from [`Self::supports_evaluate_for_hovers`]
    /// because hover is a narrower promise (pure inspection of the *selected*
    /// frame) than general evaluation, and the two must not imply each other
    /// (#9573).
    #[serde(default)]
    pub supports_evaluate: bool,
    /// `supportsSetVariable`.
    pub supports_set_variable: bool,
}

/// Intersect catalog-advertised DAP features with a backend's capabilities.
///
/// A feature is advertised only if *both* the catalog compiled it in *and* the
/// selected backend can actually do it. This is the honest-negotiation core of
/// decision D6.
#[must_use]
pub fn intersect_dap_capabilities(
    catalog: &CatalogDapFlags,
    backend: &DebugBackendCapabilities,
) -> NegotiatedDapCapabilities {
    NegotiatedDapCapabilities {
        supports_conditional_breakpoints: catalog.breakpoints_basic
            && backend.conditional_breakpoints,
        supports_hit_conditional_breakpoints: catalog.hit_condition && backend.hit_conditions,
        supports_log_points: catalog.logpoints && backend.logpoints,
        supports_function_breakpoints: catalog.function_breakpoints && backend.function_breakpoints,
        supports_data_breakpoints: catalog.watchpoints && backend.data_breakpoints,
        // #9573: hover is gated on a pure selected-frame inspection proof that
        // does not exist yet. The catalog ∩ backend intersection still applies
        // (decision D6) so that re-enabling the gate cannot over-advertise, but
        // neither conjunct can widen the capability on its own.
        supports_evaluate_for_hovers: advertises_evaluate_for_hovers()
            && catalog.core
            && backend.evaluate,
        supports_evaluate: catalog.core && backend.evaluate,
        supports_set_variable: catalog.core && backend.set_variable,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn all_catalog() -> CatalogDapFlags {
        CatalogDapFlags {
            core: true,
            breakpoints_basic: true,
            hit_condition: true,
            logpoints: true,
            watchpoints: true,
            function_breakpoints: true,
        }
    }

    #[test]
    fn full_backend_with_full_catalog_advertises_everything() {
        let n = intersect_dap_capabilities(&all_catalog(), &DebugBackendCapabilities::full());
        assert!(n.supports_conditional_breakpoints);
        assert!(n.supports_hit_conditional_breakpoints);
        assert!(n.supports_log_points);
        assert!(n.supports_function_breakpoints);
        assert!(n.supports_data_breakpoints);
        assert!(n.supports_set_variable);
        // General evaluation is available with a full catalog and backend...
        assert!(n.supports_evaluate);
        // ...but hover stays closed regardless: it is a narrower promise than
        // general evaluation and its pure-inspection proof does not exist
        // (#9573). "Everything" deliberately excludes it.
        assert!(
            !n.supports_evaluate_for_hovers,
            "hover must not ride along with a fully capable catalog and backend"
        );
    }

    #[test]
    fn ptkdb_mirror_does_not_advertise_unsupported_features() {
        // Catalog compiles everything, but a ptkdb v1 peer cannot do logpoints,
        // hit conditions, data breakpoints, or set-variable. Negotiation must
        // NOT advertise them (the mirror-mode honesty guarantee).
        let n = intersect_dap_capabilities(
            &all_catalog(),
            &DebugBackendCapabilities::ptkdb_v1_defaults(),
        );
        assert!(n.supports_conditional_breakpoints, "ptkdb supports conditions");
        assert!(n.supports_function_breakpoints, "ptkdb supports sub breakpoints");
        assert!(n.supports_evaluate, "ptkdb supports evaluate");
        assert!(
            !n.supports_evaluate_for_hovers,
            "a peer that can evaluate still does not get the hover promise (#9573)"
        );
        assert!(!n.supports_log_points, "ptkdb v1 has no logpoints");
        assert!(!n.supports_hit_conditional_breakpoints, "ptkdb v1 has no hit conditions");
        assert!(!n.supports_data_breakpoints, "ptkdb v1 has no data breakpoints");
        assert!(!n.supports_set_variable, "ptkdb v1 does not set variables");
    }

    #[test]
    fn catalog_gate_wins_when_feature_not_compiled() {
        // Backend can evaluate, but catalog didn't compile dap.core.
        let catalog = CatalogDapFlags { core: false, ..all_catalog() };
        let n = intersect_dap_capabilities(&catalog, &DebugBackendCapabilities::full());
        // Discriminate on `supports_evaluate`: asserting `!supports_evaluate_for_hovers`
        // here would be vacuous while the #9573 gate pins hover false, so this
        // test would stop proving that the catalog gate works at all.
        assert!(!n.supports_evaluate);
        assert!(!n.supports_set_variable);
        // A non-core feature is unaffected.
        assert!(n.supports_conditional_breakpoints);
    }

    /// The #9573 floor: no catalog/backend combination may advertise hover.
    #[test]
    fn hover_capability_is_closed_for_every_catalog_and_backend_combination() {
        let catalogs = [
            all_catalog(),
            CatalogDapFlags { core: false, ..all_catalog() },
            CatalogDapFlags {
                core: true,
                breakpoints_basic: false,
                hit_condition: false,
                logpoints: false,
                watchpoints: false,
                function_breakpoints: false,
            },
        ];
        let backends = [
            DebugBackendCapabilities::full(),
            DebugBackendCapabilities::none(),
            DebugBackendCapabilities::ptkdb_v1_defaults(),
            // A backend that can evaluate but is otherwise bare: the exact shape
            // that would tempt `catalog.core && backend.evaluate` into a true.
            DebugBackendCapabilities { evaluate: true, ..DebugBackendCapabilities::none() },
        ];

        for catalog in &catalogs {
            for backend in &backends {
                let n = intersect_dap_capabilities(catalog, backend);
                assert!(
                    !n.supports_evaluate_for_hovers,
                    "hover advertised for catalog {catalog:?} + backend {backend:?}"
                );
            }
        }

        assert!(
            !advertises_evaluate_for_hovers(),
            "the single hover authority must report false until #9573's re-enable gate passes"
        );
    }

    /// #9573 promotion safety: refusal follows advertisement in BOTH directions.
    ///
    /// The gate constant is `false` today, so testing only the current value
    /// would leave the promotion path unproven. Passing `advertised_hover`
    /// explicitly exercises the flipped state without mutating the constant:
    /// when a mode advertises hover, it must stop refusing it; when it does
    /// not, it must refuse. Anything else republishes the exact
    /// capability-versus-behaviour contradiction this issue removes.
    #[test]
    fn hover_refusal_tracks_the_advertised_capability_in_both_directions() {
        // Closed: hover contexts refused, everything else untouched.
        assert!(refuse_hover_evaluation(false, Some("hover")));
        assert!(refuse_hover_evaluation(false, Some("Hover")));
        assert!(!refuse_hover_evaluation(false, Some("watch")));
        assert!(!refuse_hover_evaluation(false, Some("repl")));
        assert!(!refuse_hover_evaluation(false, Some("clipboard")));
        assert!(!refuse_hover_evaluation(false, None));

        // Promoted: hover is admitted, and nothing else changes behaviour.
        assert!(
            !refuse_hover_evaluation(true, Some("hover")),
            "a mode that advertises hover must stop refusing it, or promotion \
             would advertise a capability the handler still rejects"
        );
        assert!(!refuse_hover_evaluation(true, Some("watch")));
        assert!(!refuse_hover_evaluation(true, None));
    }

    /// #9573: the peer bridge's hover decision does not read the native gate.
    ///
    /// Asserting "the peer is closed today" would be vacuous — both gates are
    /// false, so a *recoupled* implementation would pass it too, and CI could
    /// only catch recoupling by mutating production source. This instead
    /// evaluates the decision under **both** native values and requires the
    /// result to be identical, which fails the moment the native gate is read
    /// again, with no mutation needed.
    #[test]
    fn peer_hover_admission_is_independent_of_the_native_gate() {
        for peer_gate in [false, true] {
            for can_evaluate in [false, true] {
                assert_eq!(
                    peer_bridge_hover_admission(false, peer_gate, can_evaluate),
                    peer_bridge_hover_admission(true, peer_gate, can_evaluate),
                    "the peer decision changed with the native gate \
                     (peer_gate={peer_gate}, can_evaluate={can_evaluate}); promoting native \
                     must never open an external-peer hover path"
                );
            }
        }

        // The peer's own gate is what decides, and it still respects whether the
        // peer can evaluate at all.
        assert!(
            !peer_bridge_hover_admission(true, false, true),
            "a closed peer gate stays closed even with native promoted"
        );
        assert!(
            peer_bridge_hover_admission(false, true, true),
            "an open peer gate opens on its own authority, not the native one"
        );
        assert!(
            !peer_bridge_hover_admission(true, true, false),
            "a peer that cannot evaluate is never advertised for hover"
        );
    }

    /// Mirror mode advertises and enforces the same value, independently.
    #[test]
    fn mirror_profile_advertisement_and_admission_agree() {
        // Mirror advertises hover false and must therefore refuse hover. The
        // advertised value itself is pinned at runtime by
        // `peer_launch::tests::static_capabilities_match_the_conservative_profile`,
        // which reads it back out of the emitted JSON.
        assert!(
            refuse_hover_evaluation(MIRROR_ADVERTISES_EVALUATE_FOR_HOVERS, Some("hover")),
            "mirror advertises hover false, so it must refuse hover"
        );
        assert!(
            !refuse_hover_evaluation(MIRROR_ADVERTISES_EVALUATE_FOR_HOVERS, Some("watch")),
            "the mirror gate must stay scoped to hover"
        );
    }

    /// Hover and general evaluation are independent evidence cells.
    #[test]
    fn general_evaluate_survives_the_hover_floor() {
        let n = intersect_dap_capabilities(&all_catalog(), &DebugBackendCapabilities::full());
        assert!(
            n.supports_evaluate,
            "closing hover must not disable general evaluate; watch/repl/clipboard depend on it"
        );
        assert!(!n.supports_evaluate_for_hovers);
    }

    /// #9581: every floored request names its own capability and its own gate,
    /// and the floor never widens beyond the six secondary requests.
    #[test]
    fn secondary_capability_floor_rows_are_independent_and_explicit() {
        let floored = [
            ("completions", "supportsCompletionsRequest"),
            ("modules", "supportsModulesRequest"),
            ("loadedSources", "supportsLoadedSourcesRequest"),
            ("restart", "supportsRestartRequest"),
            ("breakpointLocations", "supportsBreakpointLocationsRequest"),
            ("cancel", "supportsCancelRequest"),
        ];
        for (command, capability) in floored {
            let message = secondary_capability_floor_message(command)
                .unwrap_or_else(|| format!("`{command}` must be floored (#9581)"));
            assert!(
                message.contains(capability),
                "`{command}` disposition must name its own capability row: {message}"
            );
            assert!(
                message.contains("unsupported") && message.contains("#9581"),
                "`{command}` disposition must be explicit unsupported: {message}"
            );
        }

        // Core launch/breakpoint/stack/variable/control families stay outside
        // the floor (#9581 scope).
        for open in [
            "initialize",
            "launch",
            "attach",
            "setBreakpoints",
            "setFunctionBreakpoints",
            "threads",
            "stackTrace",
            "scopes",
            "variables",
            "setVariable",
            "continue",
            "next",
            "stepIn",
            "stepOut",
            "pause",
            "evaluate",
            "configurationDone",
            "disconnect",
            "terminate",
            "source",
        ] {
            assert!(
                secondary_capability_floor_message(open).is_none(),
                "`{open}` must not be floored by the secondary-capability floor"
            );
        }
    }

    /// #9581: only a non-default `format` on the four ValueFormat families is
    /// floored; absent/default-equivalent formats keep the independent
    /// contract, and other requests are never format-floored.
    #[test]
    fn value_format_floor_rejects_only_non_default_format_on_the_four_families() {
        for command in ["variables", "setVariable", "evaluate", "setExpression"] {
            assert!(unproven_value_format_requested(
                command,
                Some(&serde_json::json!({ "format": { "hex": true } }))
            ));
            assert!(!unproven_value_format_requested(command, None));
            assert!(!unproven_value_format_requested(command, Some(&serde_json::json!({}))));
            assert!(!unproven_value_format_requested(
                command,
                Some(&serde_json::json!({ "format": {} }))
            ));
            assert!(!unproven_value_format_requested(
                command,
                Some(&serde_json::json!({ "format": { "hex": false } }))
            ));
        }
        assert!(!unproven_value_format_requested(
            "stackTrace",
            Some(&serde_json::json!({ "format": { "hex": true } }))
        ));

        let message = value_format_unsupported_message("variables");
        assert!(message.contains("supportsValueFormattingOptions"));
        assert!(message.contains("#9581"));
    }

    #[test]
    fn value_format_unknown_fields_are_rejected_before_flooring() {
        let args = serde_json::json!({
            "expression": "$x",
            "format": { "hex": true, "radix": 16 }
        });
        let message = capability_floor_message("evaluate", Some(&args))
            .expect("unknown ValueFormat fields must be rejected");
        assert!(message.contains("Invalid arguments"), "unexpected message: {message}");
        assert!(message.contains("radix"), "unexpected message: {message}");
        assert!(!message.contains("non-default `format` option"));
    }

    /// #9581: the combined floor decision every surface applies is exactly the
    /// composition of the two floored families — a floored secondary row, a
    /// non-default `format` on a ValueFormat family, and nothing else.
    #[test]
    fn capability_floor_message_combines_both_floored_families() {
        assert!(capability_floor_message("restart", None).is_some());
        assert!(
            capability_floor_message(
                "evaluate",
                Some(&serde_json::json!({ "expression": "$x", "format": { "hex": true } }))
            )
            .is_some()
        );
        assert!(capability_floor_message("threads", None).is_none());
        assert!(
            capability_floor_message("evaluate", Some(&serde_json::json!({ "expression": "$x" })))
                .is_none()
        );
    }

    #[test]
    fn control_mode_defaults_to_mirror() {
        assert_eq!(ControlMode::default(), ControlMode::Mirror);
        assert_eq!(DebugBackendCapabilities::none().control_mode, ControlMode::Mirror);
    }
}
