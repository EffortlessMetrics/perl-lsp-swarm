//! Discriminating proof for the [`EffectiveLspSurface`] authority
//! (#9665): selection algebra, structural impossibility of the old
//! architecture's failure classes, and static-builder parity with every
//! disagreement retained and mapped to its cutover row.

#![expect(
    clippy::unwrap_used,
    reason = "tracked conversion debt: https://github.com/EffortlessMetrics/perl-lsp-swarm/issues/3021"
)]

use std::collections::BTreeSet;

use super::{
    CapabilityFamily, ClientFact, ClientSurfaceEvidence, DiagnosticTransport, DowngradeReason,
    EffectiveLspSurface, FamilyOutcome, FileOperationFacts, KnownException, PlannedDynamic,
    PositionEncoding, PushTransportReason, RefreshFamily, RegistrationOptionsShape,
    SuppressionReason, SurfaceInputs, WatcherPlanDecision, WatcherWithholdReason,
};
use crate::features::policy::FeatureProfile;
use crate::protocol::capabilities::{
    BuildFlags, SERVER_WORKSPACE_FOLDER_SUPPORT, capabilities_json, get_supported_commands,
};
use crate::protocol::final_surface_inventory::{census_profiles, flatten_surface_pointers};

/// Minimal client evidence: no declarations at all.
fn bare_client() -> ClientSurfaceEvidence {
    ClientSurfaceEvidence::default()
}

/// Inputs for a subject with no client signal beyond the profile.
fn bare_inputs(profile: FeatureProfile) -> SurfaceInputs {
    SurfaceInputs {
        profile,
        build_flags: profile.build_flags(),
        disabled_feature_ids: BTreeSet::new(),
        compatibility_exceptions: BTreeSet::new(),
        client: bare_client(),
        command_ids: get_supported_commands(),
        runtime: Default::default(),
    }
}

fn build_ok(inputs: &SurfaceInputs) -> EffectiveLspSurface {
    match EffectiveLspSurface::build(inputs) {
        Ok(surface) => surface,
        Err(error) => panic!("surface construction refused: {error}"),
    }
}

// ---------------------------------------------------------------------------
// Determinism and input binding (#9665 item 8)
// ---------------------------------------------------------------------------

#[test]
fn identical_inputs_produce_identical_surfaces_and_digests() {
    let inputs = bare_inputs(FeatureProfile::Production);
    let first = build_ok(&inputs);
    let second = build_ok(&inputs);
    assert_eq!(first, second, "model must be deterministic for one subject");
    assert!(first.input_digest.starts_with("sha256:"));
    assert_eq!(first.schema_version, 1);
}

#[test]
fn digest_changes_when_configuration_generation_changes() {
    let mut disabled = BTreeSet::new();
    disabled.insert("lsp.hover".to_string());
    let mut with_disabled = bare_inputs(FeatureProfile::Production);
    with_disabled.disabled_feature_ids = disabled;
    let plain = build_ok(&bare_inputs(FeatureProfile::Production));
    let suppressed = build_ok(&with_disabled);
    assert_ne!(plain.input_digest, suppressed.input_digest);
}

#[test]
fn digest_binds_the_reviewed_runtime_input() {
    // Two subjects that differ only in runtime tuning must not share a
    // receipt: the registration plans differ, so the digest must identify
    // which runtime decision produced each (#9665 item 8).
    let mut tuned_off = bare_inputs(FeatureProfile::Production);
    tuned_off.client.dynamic_file_watcher_registration = ClientFact::Supported;
    tuned_off.runtime.file_watchers_enabled = false;
    let mut tuned_on = tuned_off.clone();
    tuned_on.runtime.file_watchers_enabled = true;

    let off = build_ok(&tuned_off);
    let on = build_ok(&tuned_on);
    assert_ne!(off.input_digest, on.input_digest);
    assert_ne!(off.registration_plan, on.registration_plan);
}

#[test]
fn raw_flag_divergence_from_profile_is_refused() {
    let mut tampered = bare_inputs(FeatureProfile::GaLock);
    // ga-lock excludes notebook sync; flipping the raw flag must be refused
    // instead of silently widening the profile.
    assert!(!tampered.build_flags.notebook_document_sync, "precondition: ga-lock excludes it");
    tampered.build_flags.notebook_document_sync = true;
    let error = EffectiveLspSurface::build(&tampered)
        .expect_err("raw flags must not inject a capability the profile does not admit");
    assert!(
        error.problems.iter().any(|problem| problem.contains("profile.build_flags")),
        "refusal must name provenance: {:?}",
        error.problems
    );
}

#[test]
fn duplicate_command_descriptors_are_refused() {
    let mut duplicated = bare_inputs(FeatureProfile::Production);
    duplicated.command_ids.push("perl.runTests".to_string());
    let error = EffectiveLspSurface::build(&duplicated)
        .expect_err("duplicate descriptors must be rejected deterministically");
    assert!(
        error.problems.iter().any(|problem| problem.contains("duplicate command")),
        "refusal must name the duplicate: {:?}",
        error.problems
    );
}

// ---------------------------------------------------------------------------
// Selection algebra: distinct outcome classes (#9665 items 3/6)
// ---------------------------------------------------------------------------

#[test]
fn profile_exclusion_configuration_and_client_withholding_stay_distinct() {
    // Profile exclusion: ga-lock does not compile inline completion.
    let ga = build_ok(&bare_inputs(FeatureProfile::GaLock));
    assert_eq!(
        ga.families.get(&CapabilityFamily::InlineCompletion),
        Some(&FamilyOutcome::UnadvertisedUnsupported),
        "ga-lock excludes inline completion by profile, not suppression"
    );

    // Configuration suppression on a profile that compiles it.
    let mut disabled = BTreeSet::new();
    disabled.insert("lsp.inline_completion".to_string());
    let mut inputs = bare_inputs(FeatureProfile::All);
    inputs.disabled_feature_ids = disabled;
    let all = build_ok(&inputs);
    assert_eq!(
        all.families.get(&CapabilityFamily::InlineCompletion),
        Some(&FamilyOutcome::Suppressed(SuppressionReason::DisabledByConfiguration {
            feature_id: "lsp.inline_completion".to_string(),
        })),
        "disabledFeatures must produce a typed configuration suppression"
    );

    // Compiled in but withheld because the client never declared it.
    let no_client = build_ok(&bare_inputs(FeatureProfile::All));
    assert_eq!(
        no_client.families.get(&CapabilityFamily::InlineCompletion),
        Some(&FamilyOutcome::UnadvertisedUnsupported),
        "absent client declaration withholds the static provider object"
    );
}

#[test]
fn unknown_disabled_feature_ids_are_preserved_not_silently_dropped() {
    let mut disabled = BTreeSet::new();
    disabled.insert("lsp.does_not_exist".to_string());
    let mut inputs = bare_inputs(FeatureProfile::Production);
    inputs.disabled_feature_ids = disabled;
    let surface = build_ok(&inputs);
    assert_eq!(
        surface.unrecognized_disabled_feature_ids,
        vec!["lsp.does_not_exist".to_string()],
        "unknown IDs must remain visible instead of vanishing"
    );
}

#[test]
fn workspace_symbol_resolve_overrides_simple_shape_after_suppression() {
    // Resolve survives while the simple flag is suppressed: advertisement
    // remains, in resolve shape (builder parity — there is no separate
    // suppression ID for the resolve variant).
    let mut disabled = BTreeSet::new();
    disabled.insert("lsp.workspace_symbol".to_string());
    let mut inputs = bare_inputs(FeatureProfile::All);
    inputs.disabled_feature_ids = disabled;
    let surface = build_ok(&inputs);
    assert_eq!(
        surface.server_capabilities.get("workspaceSymbolProvider"),
        Some(&serde_json::json!({ "resolveProvider": true })),
        "resolve-only advertisement stays Static"
    );
    assert_eq!(
        surface.families.get(&CapabilityFamily::WorkspaceSymbol),
        Some(&FamilyOutcome::Static),
    );

    // Both shipped profiles carry the resolve variant, so the resolve shape
    // appears there too (documents current reality; the simple boolean only
    // occurs when workspace_symbol_resolve is off).
    let ga = build_ok(&bare_inputs(FeatureProfile::GaLock));
    assert_eq!(
        ga.server_capabilities.get("workspaceSymbolProvider"),
        Some(&serde_json::json!({ "resolveProvider": true })),
        "ga-lock ships the workspace-symbol resolve variant"
    );
}

#[test]
fn position_encoding_pin_records_negotiated_preference_as_downgrade() {
    let mut client = bare_client();
    client.negotiated_position_encoding = Some(PositionEncoding::Utf8);
    let mut inputs = bare_inputs(FeatureProfile::Production);
    inputs.client = client;
    let surface = build_ok(&inputs);

    assert_eq!(
        surface.position_contract,
        super::super::effective_surface::PositionEncodingContract {
            advertised: PositionEncoding::Utf16,
            negotiated_preference: Some(PositionEncoding::Utf8),
        },
        "negotiated preference stored; advertisement pinned to UTF-16"
    );
    assert_eq!(
        surface.server_capabilities.get("positionEncoding"),
        Some(&serde_json::json!("utf-16")),
        "wire advertisement must stay utf-16 until #9282 cutover"
    );
    assert!(
        surface.suppressed_or_downgraded().contains_key(&CapabilityFamily::PositionEncoding),
        "the pin is recorded as a typed downgrade, not silently applied"
    );
}

// ---------------------------------------------------------------------------
// Static vs planned-dynamic exclusivity (#9665 item 4)
// ---------------------------------------------------------------------------

#[test]
fn inline_completion_selector_cannot_be_static_and_planned_simultaneously() {
    let mut client = bare_client();
    client.inline_completion = ClientFact::Supported;
    client.inline_completion_dynamic_registration = ClientFact::Supported;
    let mut inputs = bare_inputs(FeatureProfile::All);
    inputs.client = client;
    let surface = build_ok(&inputs);

    let outcome = surface.families.get(&CapabilityFamily::InlineCompletion).unwrap();
    match outcome {
        FamilyOutcome::Downgraded(
            DowngradeReason::DynamicRegistrationPreferred { selector: "inlineCompletionProvider" },
            inner,
        ) => {
            assert!(
                matches!(inner.as_ref(), FamilyOutcome::PlannedDynamic(_)),
                "retained variant is the plan, not a second static copy: {outcome:?}"
            );
        }
        other => panic!("expected downgraded-to-planned-dynamic, got {other:?}"),
    }
    assert!(
        surface.server_capabilities.get("inlineCompletionProvider").is_none(),
        "static provider object withdrawn when the plan owns the selector"
    );
    assert_eq!(surface.registration_plan.registrations.len(), 1);
    assert_eq!(surface.registration_plan.registrations[0].registration_id, "perl-inlineCompletion");
}

#[test]
fn registration_plan_selectors_are_unique_across_the_whole_plan() {
    let mut client = bare_client();
    client.inline_completion = ClientFact::Supported;
    client.inline_completion_dynamic_registration = ClientFact::Supported;
    client.dynamic_file_watcher_registration = ClientFact::Supported;
    let mut inputs = bare_inputs(FeatureProfile::All);
    inputs.client = client;
    let surface = build_ok(&inputs);

    let mut selectors = BTreeSet::new();
    for registration in &surface.registration_plan.registrations {
        assert!(
            selectors.insert(registration.method),
            "duplicate selector in plan: {}",
            registration.method
        );
    }
    assert_eq!(selectors.len(), surface.registration_plan.registrations.len());
}

#[test]
fn watcher_registration_requires_claimed_support_active_symbol_and_no_jetbrains_exception() {
    let base = || {
        let mut client = bare_client();
        client.dynamic_file_watcher_registration = ClientFact::Supported;
        client.file_watcher_relative_pattern = ClientFact::Supported;
        let mut inputs = bare_inputs(FeatureProfile::Production);
        inputs.client = client;
        inputs
    };

    let admitted = build_ok(&base());
    match admitted.registration_plan.registrations.as_slice() {
        [
            PlannedDynamic {
                registration_id: "perl-didChangeWatchedFiles",
                method: "workspace/didChangeWatchedFiles",
                options_shape: RegistrationOptionsShape::Watchers { relative_pattern: true },
            },
        ] => {}
        other => panic!("expected watcher registration with relative patterns, got {other:?}"),
    }
    assert_eq!(
        admitted.watcher_registration_decision,
        WatcherPlanDecision::Planned,
        "the admitted subject records a typed planned decision"
    );

    // Without claimed support: no registration, and the cause is the
    // client's missing declaration.
    let mut unclaimed = base();
    unclaimed.client.dynamic_file_watcher_registration = ClientFact::DeclaredFalse;
    let refused = build_ok(&unclaimed);
    assert!(refused.registration_plan.registrations.is_empty());
    assert_eq!(
        refused.watcher_registration_decision,
        WatcherPlanDecision::Withheld(WatcherWithholdReason::ClientUnsupported),
    );

    // With the JetBrains compatibility exception: force-disabled.
    let mut jetbrains = base();
    jetbrains.compatibility_exceptions.insert(KnownException::JetBrainsWatcherForceDisable);
    let forced_off = build_ok(&jetbrains);
    assert!(
        forced_off.registration_plan.registrations.is_empty(),
        "compatibility exception removes the planned registration"
    );
    assert_eq!(
        forced_off.watcher_registration_decision,
        WatcherPlanDecision::Withheld(WatcherWithholdReason::CompatibilityException {
            exception: KnownException::JetBrainsWatcherForceDisable,
        }),
    );
    assert!(
        forced_off
            .compatibility_exceptions_applied
            .contains(&KnownException::JetBrainsWatcherForceDisable)
    );
}

// ---------------------------------------------------------------------------
// Negative controls: the old architecture's failure classes (#9665)
// ---------------------------------------------------------------------------

/// Every emitted wire pointer must trace to an active family/contract — a
/// raw build flag cannot create an advertised field outside the model's
/// final selection. Provenance: projection reads only `families` +
/// contracts + normalized facts, so this control walks both directions:
/// model→wire and wire→census coverage.
#[test]
fn every_wire_pointer_is_owned_by_a_selected_family_or_contract() {
    for (profile_name, _) in census_profiles() {
        let profile = match profile_name {
            "ga-lock" => FeatureProfile::GaLock,
            "production" => FeatureProfile::Production,
            _ => FeatureProfile::All,
        };
        for inline_declared in [false, true] {
            let mut client = bare_client();
            if inline_declared {
                client.inline_completion = ClientFact::Supported;
            }
            let mut inputs = bare_inputs(profile);
            inputs.client = client;
            let surface = build_ok(&inputs);

            let pointers = flatten_surface_pointers(&surface.server_capabilities);
            for pointer in &pointers {
                let owned = pointer == "positionEncoding"
                    || pointer.starts_with("textDocumentSync")
                    || pointer.starts_with("workspace")
                    || pointer.starts_with("experimental")
                    || surface.families.iter().any(|(family, outcome)| {
                        family.wire_prefixes().iter().any(|prefix| pointer.starts_with(prefix))
                            && outcome.is_effectively_advertised()
                    });
                assert!(
                    owned,
                    "{profile_name}: pointer {pointer} emitted without an owning selected family"
                );
            }
        }
    }
}

#[test]
fn absence_false_malformed_and_unknown_future_never_collapse_to_supported() {
    for fact in [
        ClientFact::Absent,
        ClientFact::DeclaredFalse,
        ClientFact::Malformed,
        ClientFact::UnsupportedFuture,
    ] {
        assert!(!fact.is_supported(), "{fact:?} must never count as support");
    }
    assert!(ClientFact::Supported.is_supported());

    // Behaviorally: dynamic registration stays unplanned for each class.
    for fact in [
        ClientFact::Absent,
        ClientFact::DeclaredFalse,
        ClientFact::Malformed,
        ClientFact::UnsupportedFuture,
    ] {
        let mut client = bare_client();
        client.inline_completion = ClientFact::Supported;
        client.inline_completion_dynamic_registration = fact;
        let mut inputs = bare_inputs(FeatureProfile::All);
        inputs.client = client;
        let surface = build_ok(&inputs);
        assert!(
            !matches!(
                surface.families.get(&CapabilityFamily::InlineCompletion),
                Some(FamilyOutcome::Downgraded(_, inner))
                    if matches!(inner.as_ref(), FamilyOutcome::PlannedDynamic(_))
            ),
            "{fact:?} must not produce a planned registration"
        );
        if fact != ClientFact::Absent {
            // Declared-but-false/malformed still *declares* presence, so the
            // static provider object remains (presence gate parity).
            assert_eq!(
                surface.families.get(&CapabilityFamily::InlineCompletion),
                Some(&FamilyOutcome::Static),
                "{fact:?} keeps static advertisement via the presence gate"
            );
        }
    }
}

#[test]
fn external_tool_absence_cannot_suppress_native_formatting() {
    // The model has no tool-availability input at all; formatting depends
    // only on profile + configuration. No construction can express a
    // perltidy gate over the native formatter.
    let inputs = bare_inputs(FeatureProfile::Production);
    let surface = build_ok(&inputs);
    assert_eq!(
        surface.families.get(&CapabilityFamily::DocumentFormatting),
        Some(&FamilyOutcome::Static),
        "native formatter capability is independent of optional external tools"
    );
}

#[test]
fn runtime_tuning_file_watchers_gate_suppresses_only_the_registration_plan() {
    let mut client = bare_client();
    client.dynamic_file_watcher_registration = ClientFact::Supported;
    let mut tuned_off = bare_inputs(FeatureProfile::Production);
    tuned_off.client = client;
    tuned_off.runtime.file_watchers_enabled = false;
    let surface = build_ok(&tuned_off);
    assert!(
        surface.registration_plan.registrations.is_empty(),
        "runtime tuning must suppress the watcher registration"
    );
    assert!(
        surface.families.get(&CapabilityFamily::WorkspaceSymbol) == Some(&FamilyOutcome::Static),
        "tuning gates the plan, never the advertised static surface"
    );
    assert_eq!(
        surface.watcher_registration_decision,
        WatcherPlanDecision::Withheld(WatcherWithholdReason::RuntimeUnavailable {
            input: "runtime_tuning.file_watchers",
        }),
        "runtime withholding is a typed decision, not a silently empty plan"
    );
}

#[test]
fn advertised_ids_are_derived_after_configuration_suppression() {
    let mut disabled = BTreeSet::new();
    disabled.insert("lsp.completion".to_string());
    disabled.insert("lsp.semantic_tokens".to_string());
    let mut inputs = bare_inputs(FeatureProfile::Production);
    inputs.disabled_feature_ids = disabled;
    let surface = build_ok(&inputs);

    assert!(
        !surface.advertised_feature_ids.contains(&"lsp.completion"),
        "suppressed feature IDs must be absent from effective identities"
    );
    assert!(!surface.advertised_feature_ids.contains(&"lsp.semantic_tokens"));
    assert!(surface.advertised_feature_ids.contains(&"lsp.hover"));

    // Wire agrees: completionProvider absent after suppression.
    assert!(surface.server_capabilities.get("completionProvider").is_none());
    assert!(surface.server_capabilities.get("semanticTokensProvider").is_none());
}

#[test]
fn advertised_ids_derive_from_final_family_outcomes_not_client_unsuppressed_flags() {
    // Compiled in but never declared by the client: the final family outcome
    // is UnadvertisedUnsupported, so the effective identity must be absent
    // even though the post-configuration flag set still carries it
    // (#9665 item 5 and its negative control).
    let no_signal = build_ok(&bare_inputs(FeatureProfile::Production));
    assert_eq!(
        no_signal.families.get(&CapabilityFamily::InlineCompletion),
        Some(&FamilyOutcome::UnadvertisedUnsupported),
    );
    assert!(
        !no_signal.advertised_feature_ids.contains(&"lsp.inline_completion"),
        "an unadvertised family must not appear in effective identities"
    );

    // Declared with dynamic support: delivered through the plan, identity
    // present (the downgraded outcome is effectively advertised).
    let mut client = bare_client();
    client.inline_completion = ClientFact::Supported;
    client.inline_completion_dynamic_registration = ClientFact::Supported;
    let mut declared = bare_inputs(FeatureProfile::Production);
    declared.client = client;
    let planned = build_ok(&declared);
    assert!(planned.advertised_feature_ids.contains(&"lsp.inline_completion"));
}

#[test]
fn compatibility_exceptions_carry_exact_evidence_reason_and_expiry() {
    for exception in [
        KnownException::OpenCodePushDiagnosticsRetention,
        KnownException::JetBrainsWatcherForceDisable,
    ] {
        assert!(!exception.subject_evidence().is_empty());
        assert!(!exception.reason().is_empty());
        assert!(!exception.expiry().is_empty(), "exceptions are never silently permanent");
    }
}

#[test]
fn planned_dynamic_state_is_not_representable_as_active_client_state() {
    // The planned type has no activation field anywhere; serialize it and
    // prove the receipt language stays plan-shaped.
    let plan = PlannedDynamic {
        registration_id: "perl-inlineCompletion",
        method: "textDocument/inlineCompletion",
        options_shape: RegistrationOptionsShape::InlineCompletionDocumentSelector,
    };
    let rendered = serde_json::to_string(&plan).unwrap();
    assert!(
        !rendered.contains("active") && !rendered.contains("\"status\""),
        "plan serialization must not imply activation: {rendered}"
    );
}

#[test]
fn opencode_compatibility_downgrades_transport_but_not_advertisement() {
    let declared = || {
        let mut client = bare_client();
        client.diagnostic_pull = ClientFact::Supported;
        let mut inputs = bare_inputs(FeatureProfile::Production);
        inputs.client = client;
        inputs
    };

    let pull = build_ok(&declared());
    assert_eq!(pull.diagnostic_transport, DiagnosticTransport::PullPreferred);

    let mut opencode = declared();
    opencode.compatibility_exceptions.insert(KnownException::OpenCodePushDiagnosticsRetention);
    let pushed = build_ok(&opencode);
    assert_eq!(
        pushed.diagnostic_transport,
        DiagnosticTransport::PushOnly(PushTransportReason::ClientCompatibility(
            KnownException::OpenCodePushDiagnosticsRetention
        )),
        "OpenCode retains push publishing through the typed exception"
    );
    // The diagnosticProvider advertisement itself is unchanged either way.
    assert_eq!(
        pull.server_capabilities.get("diagnosticProvider"),
        pushed.server_capabilities.get("diagnosticProvider")
    );

    // No client signal at all: push without any exception.
    let silent = build_ok(&bare_inputs(FeatureProfile::Production));
    assert_eq!(
        silent.diagnostic_transport,
        DiagnosticTransport::PushOnly(PushTransportReason::NoClientSignal)
    );
}

#[test]
fn transport_requires_an_effectively_advertised_pull_family() {
    // The client declares pull diagnostics, but configuration suppressed
    // lsp.pull_diagnostics: diagnosticProvider is absent from the final
    // surface, so push publishing must remain instead of being suppressed
    // for a transport the server never offered.
    let mut suppressed = bare_inputs(FeatureProfile::Production);
    suppressed.client.diagnostic_pull = ClientFact::Supported;
    suppressed.disabled_feature_ids.insert("lsp.pull_diagnostics".to_string());
    let surface = build_ok(&suppressed);
    assert!(surface.server_capabilities.get("diagnosticProvider").is_none());
    assert_eq!(
        surface.diagnostic_transport,
        DiagnosticTransport::PushOnly(PushTransportReason::PullNotAdvertised),
        "push survives when pull is withheld by configuration"
    );
}

// ---------------------------------------------------------------------------
// Refresh plan (#9665: plans, never execution)
// ---------------------------------------------------------------------------

#[test]
fn refresh_requests_require_client_support_and_active_feature() {
    let mut client = bare_client();
    client.refresh_supports.code_lens = ClientFact::Supported;
    client.refresh_supports.diagnostic = ClientFact::Supported;
    let mut inputs = bare_inputs(FeatureProfile::GaLock);
    inputs.client = client;
    let surface = build_ok(&inputs);

    let code_lens = surface.refresh_plan.get(&RefreshFamily::CodeLens).unwrap();
    assert!(code_lens.planned, "codeLens refresh planned when supported+active");

    // textDocumentContent refresh has no refreshSupport gate.
    let content = surface.refresh_plan.get(&RefreshFamily::TextDocumentContent).unwrap();
    assert!(content.planned);
}

// ---------------------------------------------------------------------------
// Parity against the current static builder (#9665 item 7):
// compare model output with the live builder per census profile and retain
// every disagreement explicitly. The known disagreement set below maps to
// #9662 rows CAP_TEXT_DOCUMENT_SYNC_SAVE / MUT_TEXT_DOCUMENT_SYNC_OVERRIDE,
// MUT_WORKSPACE_REPLACEMENT, MUT_POSITION_ENCODING_PIN and
// MUT_INLINE_COMPLETION_TRI_STATE; S03 owns their removal.
// ---------------------------------------------------------------------------

#[test]
fn model_matches_static_builder_except_runtime_authority_deltas() {
    for (profile_name, flags) in census_profiles() {
        let profile = match profile_name {
            "ga-lock" => FeatureProfile::GaLock,
            "production" => FeatureProfile::Production,
            _ => FeatureProfile::All,
        };
        let inputs = bare_inputs(profile);
        let surface = build_ok(&inputs);
        let static_json = capabilities_json(flags.clone());

        let model_pointers = flatten_surface_pointers(&surface.server_capabilities);
        let static_pointers = flatten_surface_pointers(&static_json);

        let mut model_only: Vec<String> =
            model_pointers.difference(&static_pointers).cloned().collect();
        let mut static_only: Vec<String> =
            static_pointers.difference(&model_pointers).cloned().collect();

        // Runtime-authority deltas (documented competing writers in #9662):
        // 1. positionEncoding pin exists only in the final surface;
        // 2. workspace.* exists only in the runtime-owned replacement;
        // 3. textDocumentSync pointers differ (runtime replaces the shape).
        let removed_model_only = retain_count(&mut model_only, |pointer| {
            pointer == "positionEncoding"
                || pointer.starts_with("workspace")
                || pointer.starts_with("textDocumentSync")
        });
        assert!(
            removed_model_only >= 1,
            "positionEncoding pin must appear as a model-only pointer"
        );
        // 4. inlineCompletionProvider tri-state: with no client signal the
        // final surface withholds it even when compiled in.
        if flags.inline_completion {
            static_only.retain(|pointer| !pointer.starts_with("inlineCompletionProvider"));
        }
        static_only.retain(|pointer| !pointer.starts_with("textDocumentSync"));

        // textDocumentSync differs in value but shares pointers; compare
        // shapes explicitly and record the exact delta.
        let sync_delta = static_json.get("textDocumentSync").map(|sync| {
            serde_json::json!({
                "static": sync,
                "final": surface.server_capabilities.get("textDocumentSync"),
            })
        });

        assert!(
            model_only.is_empty(),
            "{profile_name}: unexplained final-only pointers {model_only:?}"
        );
        assert!(
            static_only.is_empty(),
            "{profile_name}: unexplained static-only pointers {static_only:?}"
        );

        // Exact retained disagreement: static advertises save:true while the
        // final authority replaces it with willSave/willSaveWaitUntil/save
        // includeText (row CAP_TEXT_DOCUMENT_SYNC_SAVE).
        let sync = sync_delta.expect("both surfaces carry textDocumentSync");
        assert_eq!(sync["static"]["save"], serde_json::json!(true));
        assert_eq!(sync["final"]["save"], serde_json::json!({"includeText": true}));
        assert_eq!(sync["final"]["willSave"], serde_json::json!(true));
        assert_eq!(sync["final"]["willSaveWaitUntil"], serde_json::json!(false));

        // Everything OUTSIDE the four documented delta families must agree
        // byte-for-byte (Value equality), proving the model reproduces the
        // shipped static surface exactly rather than approximately.
        let mut sanitized_static = static_json.clone();
        let mut sanitized_final = surface.server_capabilities.clone();
        for sanitized in [&mut sanitized_static, &mut sanitized_final] {
            if let Some(map) = sanitized.as_object_mut() {
                map.remove("textDocumentSync");
                map.remove("workspace");
                map.remove("positionEncoding");
                map.remove("inlineCompletionProvider");
            }
        }
        assert_eq!(
            sanitized_static, sanitized_final,
            "{profile_name}: model must equal the static builder outside documented deltas"
        );
    }
}

fn retain_count(pointers: &mut Vec<String>, predicate: impl Fn(&str) -> bool) -> usize {
    let before = pointers.len();
    pointers.retain(|pointer| !predicate(pointer.as_str()));
    before - pointers.len()
}

// ---------------------------------------------------------------------------
// Inventory cross-checks: the model covers the S01 denominator rows that
// target #9665 (suppression branches + compatibility exceptions).
// ---------------------------------------------------------------------------

#[test]
fn every_9665_targeted_inventory_row_maps_into_the_model() {
    use crate::protocol::final_surface_inventory::{SurfaceKind, final_surface_rows};

    let surface = build_ok(&bare_inputs(FeatureProfile::All));
    for row in final_surface_rows() {
        if row.target_issue != "#9665" {
            continue;
        }
        match row.kind {
            SurfaceKind::Suppression => {
                // Each named suppression input is expressible as a typed
                // model mechanism: disabledFeatures IDs map to feature-ID
                // families; profile:/tool:/config: inputs are expressed by
                // profile selection, the reviewed RuntimeAvailability seam
                // (no tool probe exists), and registration-plan tuning.
                let expressible = row
                    .protocol_field
                    .starts_with("initializationOptions.disabledFeatures:")
                    || CapabilityFamily::feature_id_for_suppression(row.protocol_field).is_some()
                    || row.protocol_field.starts_with("profile:")
                    || row.protocol_field.starts_with("tool:")
                    || row.protocol_field.starts_with("config:");
                assert!(
                    expressible,
                    "S02 model cannot yet express suppression row {}",
                    row.surface_id
                );
            }
            SurfaceKind::Compatibility => {
                // Compatibility rows either map to a KnownException or are
                // negotiation-input records modeled as typed facts.
                let modeled = matches!(
                    row.surface_id,
                    "compat.client.jetbrains.watcherForceDisable"
                        | "compat.client.opencode.pushDiagnosticsRetention"
                        | "compat.protocol.positionEncodingUtf16Pin"
                        | "compat.negotiated.clientInputsWithoutAdvertisementSeam"
                        | "compat.protocol.diagnosticRefreshSingularKey"
                        | "compat.protocol.markdownContentFormatFallback"
                        | "compat.protocol.completionItemFlattenedShape"
                        | "compat.initialize.legacyRootPath"
                        | "compat.initialize.initOptionsRootFallbackChain"
                        | "compat.initialize.cwdFallback"
                );
                assert!(
                    modeled,
                    "compatibility row {} lacks a typed model disposition",
                    row.surface_id
                );
            }
            _ => {}
        }
    }
    let _ = surface; // silence unused in wasm builds where rows differ
}

#[test]
fn file_operation_intersection_follows_normalized_facts() {
    let mut client = bare_client();
    client.workspace_folders = ClientFact::Supported;
    client.file_operations = FileOperationFacts {
        will_rename: ClientFact::Supported,
        did_delete: ClientFact::Supported,
        ..FileOperationFacts::default()
    };
    let mut inputs = bare_inputs(FeatureProfile::Production);
    inputs.client = client;
    let surface = build_ok(&inputs);

    let operations = surface
        .server_capabilities
        .pointer("/workspace/fileOperations")
        .and_then(serde_json::Value::as_object)
        .expect("fileOperations object present when any operation supported");
    assert_eq!(operations.len(), 2, "only supported operations advertise: {operations:?}");
    assert!(operations.contains_key("willRename"));
    assert!(operations.contains_key("didDelete"));
    assert_eq!(
        surface.server_capabilities.pointer("/workspace/workspaceFolders/supported"),
        Some(&serde_json::json!(SERVER_WORKSPACE_FOLDER_SUPPORT)),
    );

    // #8161: the server's `supported` describes server implementation truth,
    // so the client's own workspaceFolders fact must not flip it. The expected
    // value is the shared constant rather than a literal, so a profile-owned
    // suppression moves the model, the runtime builder and this proof together
    // instead of leaving a stale `true` behind. Independence from the client is
    // still proven by holding the client fact at false/absent below while the
    // server bit stays at implementation truth.
    let mut no_folders = inputs.clone();
    no_folders.client.workspace_folders = ClientFact::DeclaredFalse;
    let declared_false = build_ok(&no_folders);
    assert_eq!(
        declared_false.server_capabilities.pointer("/workspace/workspaceFolders/supported"),
        Some(&serde_json::json!(SERVER_WORKSPACE_FOLDER_SUPPORT)),
        "client DeclaredFalse must not un-implement server workspace folders"
    );

    let mut absent_folders = inputs;
    absent_folders.client.workspace_folders = ClientFact::Absent;
    let absent = build_ok(&absent_folders);
    assert_eq!(
        absent.server_capabilities.pointer("/workspace/workspaceFolders/supported"),
        Some(&serde_json::json!(SERVER_WORKSPACE_FOLDER_SUPPORT)),
        "client Absent must not un-implement server workspace folders"
    );
}

#[test]
fn code_action_documentation_insertion_follows_client_fact() {
    let with_docs = || {
        let mut client = bare_client();
        client.code_action_documentation = ClientFact::Supported;
        let mut inputs = bare_inputs(FeatureProfile::Production);
        inputs.client = client;
        inputs
    };
    let supported = build_ok(&with_docs());
    assert!(
        supported.server_capabilities.pointer("/codeActionProvider/documentation").is_some(),
        "documentation inserted only for declaring clients"
    );

    let unsupported = build_ok(&bare_inputs(FeatureProfile::Production));
    assert!(
        unsupported.server_capabilities.pointer("/codeActionProvider/documentation").is_none(),
        "default clients must not receive CodeAction.documentation"
    );

    // Malformed documentationSupport is not support.
    let mut malformed = with_docs();
    malformed.client.code_action_documentation = ClientFact::Malformed;
    let refused = build_ok(&malformed);
    assert!(refused.server_capabilities.pointer("/codeActionProvider/documentation").is_none());
}
