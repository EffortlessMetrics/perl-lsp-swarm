//! Backend capability description and DAP capability negotiation.
//!
//! DAP capabilities advertised to the editor must be the **intersection** of
//! what the feature catalog supports and what the selected backend can actually
//! do (decision D6). A ptkdb peer in `mirror` mode, for example, must not cause
//! `perl-dap` to advertise control commands the peer never offered.

use serde::{Deserialize, Serialize};

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

    #[test]
    fn control_mode_defaults_to_mirror() {
        assert_eq!(ControlMode::default(), ControlMode::Mirror);
        assert_eq!(DebugBackendCapabilities::none().control_mode, ControlMode::Mirror);
    }
}
