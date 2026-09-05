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

/// The optional breakpoint capabilities fail closed (#9578).
///
/// `supportsFunctionBreakpoints`, `supportsConditionalBreakpoints`,
/// `supportsHitConditionalBreakpoints`, and `supportsLogPoints` name runtime
/// contracts — engine-installed function resolution, condition enforcement,
/// attributed hit counting with serialized auto-continue, and correlated
/// logpoint output — that the native `perl5db` seam has not proven. The
/// repository has parsers, stores, and handler registration for parts of these
/// features, but a syntactically valid name is not runtime resolution, a
/// locally accepted condition is not proof the engine installed and enforced
/// it, and a hit counter is not trustworthy before exact hit attribution.
///
/// These gates deliberately consume no catalog flag (`dap.core`,
/// `dap.breakpoints.*`), no backend capability, no maturity row, and no
/// handler-presence signal. Promoting one capability to `true` requires
/// exactly its own re-enable gate recorded on #9578, from its own exact
/// public behavior receipt: function #8645, conditional #8988, hit-condition
/// #8994, logpoint #9000 (with the #7366 same-session false-path receipts).
/// No capability inherits another's receipt and no combined cell widens a
/// single component — each authority below is bound to exactly one accessor
/// (`optional_breakpoint_authority_binding_is_per_capability` pins this).
pub(crate) const OPTIONAL_FUNCTION_BREAKPOINTS_PROVEN: bool = false;
pub(crate) const OPTIONAL_CONDITIONAL_BREAKPOINTS_PROVEN: bool = false;
pub(crate) const OPTIONAL_HIT_CONDITIONAL_BREAKPOINTS_PROVEN: bool = false;
pub(crate) const OPTIONAL_LOG_POINTS_PROVEN: bool = false;

/// The single authority for the advertised `supportsFunctionBreakpoints` value.
///
/// Derived from [`OPTIONAL_FUNCTION_BREAKPOINTS_PROVEN`] alone (#9578); no
/// catalog or backend signal may widen it, and no sibling receipt
/// (#8988/#8994/#9000) may promote it.
#[must_use]
pub(crate) const fn advertises_function_breakpoints() -> bool {
    OPTIONAL_FUNCTION_BREAKPOINTS_PROVEN
}

/// The single authority for the advertised `supportsConditionalBreakpoints`
/// value.
///
/// Derived from [`OPTIONAL_CONDITIONAL_BREAKPOINTS_PROVEN`] alone (#9578); no
/// catalog or backend signal may widen it, and no sibling receipt
/// (#8645/#8994/#9000) may promote it.
#[must_use]
pub(crate) const fn advertises_conditional_breakpoints() -> bool {
    OPTIONAL_CONDITIONAL_BREAKPOINTS_PROVEN
}

/// The single authority for the advertised `supportsHitConditionalBreakpoints`
/// value.
///
/// Derived from [`OPTIONAL_HIT_CONDITIONAL_BREAKPOINTS_PROVEN`] alone (#9578);
/// no catalog or backend signal may widen it, and no sibling receipt
/// (#8645/#8988/#9000) may promote it.
#[must_use]
pub(crate) const fn advertises_hit_conditional_breakpoints() -> bool {
    OPTIONAL_HIT_CONDITIONAL_BREAKPOINTS_PROVEN
}

/// The single authority for the advertised `supportsLogPoints` value.
///
/// Derived from [`OPTIONAL_LOG_POINTS_PROVEN`] alone (#9578); no catalog or
/// backend signal may widen it, and no sibling receipt
/// (#8645/#8988/#8994) may promote it.
#[must_use]
pub(crate) const fn advertises_log_points() -> bool {
    OPTIONAL_LOG_POINTS_PROVEN
}

/// Deterministic refusal for `setFunctionBreakpoints` while the capability is
/// floored (#9578). Every request shape receives this exact message, and no
/// rejected request resolves a name, writes a debugger command, or mutates the
/// function-breakpoint registry.
pub(crate) const FUNCTION_BREAKPOINTS_UNSUPPORTED_MESSAGE: &str = "setFunctionBreakpoints is not supported: supportsFunctionBreakpoints is advertised \
     false until exact runtime resolution and engine install/hit proof exists (#9578; re-enable gate #8645)";

/// Deterministic per-item refusal for a `setBreakpoints` entry carrying a
/// `condition` while conditional support is floored (#9578). The condition is
/// never silently stripped and an unconditional breakpoint is never installed
/// in its place.
pub(crate) const CONDITION_UNSUPPORTED_MESSAGE: &str = "condition is not supported: supportsConditionalBreakpoints is advertised \
     false until exact condition installation and enforcement proof exists (#9578; re-enable gate #8988)";

/// Deterministic per-item refusal for a `setBreakpoints` entry carrying a
/// `hitCondition` while hit-condition support is floored (#9578). The entry is
/// never installed as an unconditional breakpoint and the expression is never
/// counted locally.
pub(crate) const HIT_CONDITION_UNSUPPORTED_MESSAGE: &str = "hitCondition is not supported: supportsHitConditionalBreakpoints is advertised \
     false until exact attributed-hit counting and serialized auto-continue proof exists (#9578; re-enable gate #8994)";

/// Deterministic per-item refusal for a `setBreakpoints` entry carrying a
/// `logMessage` while logpoint support is floored (#9578). The entry is never
/// converted into an ordinary stopping breakpoint and no output is simulated.
pub(crate) const LOG_MESSAGE_UNSUPPORTED_MESSAGE: &str = "logMessage is not supported: supportsLogPoints is advertised \
     false until exact install/hit/output/continue proof exists (#9578; re-enable gate #9000)";

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

/// Whether the exact setExpression proof-and-promotion gate has passed (#9568).
///
/// DAP's `supportsSetExpression` promises that a `setExpression` request can
/// assign to an arbitrary l-value in the *exact current-frame* location: bounded
/// l-value admission, broker acknowledgement of the write, and read-back
/// currentness. `handle_set_expression` interpolates client text into raw
/// `p {lhs} = {rhs}` debugger commands against the debugger's *current* frame —
/// none of that contract exists yet, so advertising the capability from
/// `dap.core` (the previous wiring) invited editors to route assignments into
/// an unproven mutation path.
///
/// Flipping this to `true` requires the re-enable gate recorded on #9568 with
/// #9570 as the promotion boundary: exact current-frame location identity,
/// bounded l-value admission, broker acknowledgement, read-back currentness,
/// and exact public evidence. Nothing else — not `dap.core`, not
/// `setVariable`/`evaluate` evidence, not handler presence, not a catalog row —
/// may widen it.
pub const SET_EXPRESSION_PROMOTION_PROVEN: bool = false;

/// The single authority for the advertised `supportsSetExpression` value.
///
/// This deliberately consumes no catalog flag, backend flag, or handler-presence
/// signal. Set-expression support is gated on a proof that does not exist yet,
/// so the value is derived from [`SET_EXPRESSION_PROMOTION_PROVEN`] alone
/// (#9568).
#[must_use]
pub const fn advertises_set_expression() -> bool {
    SET_EXPRESSION_PROMOTION_PROVEN
}

/// Refusal message used when a `setExpression` request is declined (#9568).
///
/// Deterministic and input-independent: every rejected request — whatever its
/// expression, value, frame, or format — receives this exact message, so the
/// gate's output cannot leak anything about the input it refused.
pub const SET_EXPRESSION_UNSUPPORTED_MESSAGE: &str = "setExpression is not supported: \
     supportsSetExpression is advertised false because perl-dap has no exact current-frame \
     l-value assignment proof yet (#9568)";

/// Whether a mode must refuse a `setExpression` request as unsupported.
///
/// The invariant every mode holds: **a mode refuses setExpression exactly when
/// it does not advertise it.** `advertised_set_expression` is the value that
/// *this* mode puts on the wire for `supportsSetExpression`, so admission and
/// advertisement can never disagree.
///
/// This matters at promotion time, not just today. If the refusal ignored the
/// advertised value, flipping [`SET_EXPRESSION_PROMOTION_PROVEN`] would
/// advertise the capability while the handler still rejected every request —
/// the same capability-versus-behaviour contradiction #9568 exists to remove,
/// only pointing the other way.
///
/// Modes stay independent: each passes its own advertised value, so promoting
/// the native gate does not silently open an external-peer or mirror path that
/// has no assignment proof of its own.
#[must_use]
pub(crate) fn refuse_set_expression(advertised_set_expression: bool) -> bool {
    !advertised_set_expression
}

/// The `supportsSetExpression` value the external-peer bridge advertises.
///
/// The external-peer seam has no exact current-frame l-value assignment proof
/// of its own (#9568): the bridge can observe and drive a peer debugger, but
/// it performs no brokered, read-back-current assignment. The value is
/// deliberately independent of [`SET_EXPRESSION_PROMOTION_PROVEN`] so
/// promoting the native adapter cannot silently open the peer path (#9573's
/// independence rule, applied to setExpression).
pub(crate) const PEER_BRIDGE_ADVERTISES_SET_EXPRESSION: bool = false;

/// The external-peer bridge's setExpression advertisement, as a pure function.
///
/// `native_set_expression_gate` is accepted and **deliberately not used** —
/// the same independence property [`peer_bridge_hover_admission`] proves for
/// hover (#9573): a test evaluates this under both native values and requires
/// the result to be identical, so re-reading the native gate here fails CI
/// without any source mutation.
#[must_use]
pub(crate) fn peer_bridge_set_expression_admission(
    _native_set_expression_gate: bool,
    peer_set_expression_gate: bool,
) -> bool {
    peer_set_expression_gate
}

/// The `supportsSetExpression` value the static mirror profile advertises.
///
/// Mirror mode is conservative by construction and has no assignment proof of
/// its own, so it stays false independently of the native gate (#9568). Both
/// `static_mirror_capabilities` and the mirror request gate read this, so the
/// profile cannot advertise one thing and enforce another.
pub(crate) const MIRROR_ADVERTISES_SET_EXPRESSION: bool = false;

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

    /// Authority-binding contract (#9578 review): each `advertises_*`
    /// accessor reads exactly its own per-capability proof authority, and the
    /// old shared authority no longer exists. One capability's re-enable
    /// receipt can therefore never promote a sibling. This is a source
    /// contract, not a value assertion: the authorities are compile-time and
    /// every one currently floors at `false`, so promoting any capability
    /// requires flipping exactly its own constant and updating the floor pin
    /// below.
    #[test]
    fn optional_breakpoint_authority_binding_is_per_capability() {
        // Scan only the production half: this test module legitimately names
        // the removed authority in its own assertions.
        let source =
            include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/src/backend/capabilities.rs"));
        let source = source
            .split("#[cfg(test)]")
            .next()
            .expect("module source always has a production half");

        let bindings = [
            ("advertises_function_breakpoints", "OPTIONAL_FUNCTION_BREAKPOINTS_PROVEN"),
            ("advertises_conditional_breakpoints", "OPTIONAL_CONDITIONAL_BREAKPOINTS_PROVEN"),
            (
                "advertises_hit_conditional_breakpoints",
                "OPTIONAL_HIT_CONDITIONAL_BREAKPOINTS_PROVEN",
            ),
            ("advertises_log_points", "OPTIONAL_LOG_POINTS_PROVEN"),
        ];
        for (accessor, authority) in bindings {
            // The body must reduce to exactly its own authority atom after
            // comment stripping, so neither comments nor extra expressions can
            // satisfy the binding.
            let atom = accessor_return_atom(source, accessor);
            assert_eq!(
                atom, *authority,
                "{accessor} must return exactly its own authority {authority}"
            );
        }

        assert!(
            !source.contains("OPTIONAL_BREAKPOINT_CAPABILITIES_PROVEN"),
            "the shared all-capsabilities authority must stay removed; per-capability              authorities are the only promotion path (#9578 review)"
        );
    }

    /// Floor pin (#9578): every per-capability authority floors at `false`.
    /// A promotion flips exactly one constant AND updates this pin together
    /// with its own #9578 receipt, so a receipt can never silently widen a
    /// sibling.
    #[test]
    fn every_optional_breakpoint_authority_floors_at_false() {
        let source =
            include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/src/backend/capabilities.rs"));
        let source = source
            .split("#[cfg(test)]")
            .next()
            .expect("module source always has a production half");
        for authority in [
            "OPTIONAL_FUNCTION_BREAKPOINTS_PROVEN",
            "OPTIONAL_CONDITIONAL_BREAKPOINTS_PROVEN",
            "OPTIONAL_HIT_CONDITIONAL_BREAKPOINTS_PROVEN",
            "OPTIONAL_LOG_POINTS_PROVEN",
        ] {
            let declaration = format!("pub(crate) const {authority}: bool = false;");
            assert!(
                source.contains(&declaration),
                "{authority} must floor at false until its own re-enable receipt lands;                  update this pin in the same commit as the promotion"
            );
        }
    }

    /// Extract the single return atom of one `const fn` accessor from the
    /// module source: body text with comments stripped, the `return` keyword
    /// removed, and whitespace collapsed. Any additional expression in the
    /// body breaks the exact-atom equality.
    fn accessor_return_atom(source: &str, accessor: &str) -> String {
        let signature = format!("fn {accessor}()");
        let start = source
            .find(&signature)
            .unwrap_or_else(|| panic!("{accessor} must stay defined in capabilities.rs"));
        let open = source[start..].find('{').expect("accessor body brace") + start;
        let close = source[open..].find('}').expect("accessor body close") + open;
        let body = &source[open + 1..close];

        let mut stripped = String::with_capacity(body.len());
        let mut in_block_comment = false;
        for line in body.lines() {
            let mut rest = line;
            while !rest.is_empty() {
                let line_comment = rest.find("//");
                let block_comment = rest.find("/*");
                let block_first = block_comment.is_some_and(|b| line_comment.is_none_or(|l| b < l));
                match (block_first, line_comment) {
                    // A block comment starting before any line comment: skip
                    // to its terminator (which may live on a later line).
                    (true, _) => {
                        // `block_first` implies the block marker exists.
                        let block_comment = block_comment.unwrap_or_default();
                        stripped.push_str(&rest[..block_comment]);
                        rest = &rest[block_comment + 2..];
                        in_block_comment = true;
                    }
                    // A line comment (or a line comment preceding a block
                    // comment start): the rest of the line is commentary.
                    (false, Some(line_comment)) => {
                        stripped.push_str(&rest[..line_comment]);
                        rest = "";
                    }
                    (false, None) => {
                        stripped.push_str(rest);
                        rest = "";
                    }
                }
            }
            stripped.push(' ');
        }

        stripped.replace("return", " ").split_whitespace().collect::<String>()
    }

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

    #[test]
    fn control_mode_defaults_to_mirror() {
        assert_eq!(ControlMode::default(), ControlMode::Mirror);
        assert_eq!(DebugBackendCapabilities::none().control_mode, ControlMode::Mirror);
    }

    /// The #9568 floor: the single setExpression authority is closed until the
    /// #9570 promotion boundary passes, and it consumes no catalog signal.
    #[test]
    fn set_expression_authority_is_closed_until_promotion() {
        assert!(
            !advertises_set_expression(),
            "setExpression must stay closed until #9568's re-enable gate passes"
        );
        // The authority is a constant function of the promotion constant only;
        // assert both spellings agree so a future edit cannot decouple them.
        assert_eq!(advertises_set_expression(), SET_EXPRESSION_PROMOTION_PROVEN);
    }

    /// #9568 promotion safety: refusal follows advertisement in BOTH directions.
    ///
    /// The gate constant is `false` today, so testing only the current value
    /// would leave the promotion path unproven. Passing `advertised` explicitly
    /// exercises the flipped state without mutating the constant: when a mode
    /// advertises setExpression it must stop refusing it; when it does not, it
    /// must refuse. Anything else republishes the exact
    /// capability-versus-behaviour contradiction this issue removes.
    #[test]
    fn set_expression_refusal_tracks_the_advertised_capability_in_both_directions() {
        assert!(
            refuse_set_expression(false),
            "a mode that does not advertise setExpression must refuse it"
        );
        assert!(
            !refuse_set_expression(true),
            "a mode that advertises setExpression must stop refusing it, or promotion \
             would advertise a capability the handler still rejects"
        );
    }

    /// #9568: the peer bridge's setExpression decision does not read the
    /// native gate.
    ///
    /// Both gates are false today, so asserting only "the peer is closed"
    /// would pass for a recoupled implementation too. Evaluating the decision
    /// under both native values fails the moment the native gate is read
    /// again — the same non-mutating independence proof hover uses (#9573).
    #[test]
    fn peer_set_expression_admission_is_independent_of_the_native_gate() {
        for peer_gate in [false, true] {
            assert_eq!(
                peer_bridge_set_expression_admission(false, peer_gate),
                peer_bridge_set_expression_admission(true, peer_gate),
                "the peer setExpression decision changed with the native gate \
                 (peer_gate={peer_gate}); promoting native must never open an \
                 external-peer assignment path"
            );
        }

        assert!(
            !peer_bridge_set_expression_admission(true, false),
            "a closed peer gate stays closed even with native promoted"
        );
        assert!(
            peer_bridge_set_expression_admission(false, true),
            "an open peer gate opens on its own authority, not the native one"
        );
    }

    /// #9568: the mirror profile advertises and enforces the same value,
    /// independently of the native gate.
    #[test]
    fn mirror_set_expression_advertisement_and_admission_agree() {
        // The advertised value itself is pinned at runtime by
        // `peer_launch::tests::static_capabilities_match_the_conservative_profile`,
        // which reads it back out of the emitted JSON.
        assert!(
            refuse_set_expression(MIRROR_ADVERTISES_SET_EXPRESSION),
            "mirror advertises setExpression false, so it must refuse it"
        );
    }

    /// #9568: the refusal message is input-independent by construction.
    ///
    /// The gate treats every request identically, so the message must be a
    /// single deterministic string with no interpolation seam.
    #[test]
    fn set_expression_refusal_message_is_deterministic() {
        assert_eq!(
            SET_EXPRESSION_UNSUPPORTED_MESSAGE,
            "setExpression is not supported: supportsSetExpression is advertised false \
             because perl-dap has no exact current-frame l-value assignment proof yet (#9568)"
        );
    }
}
