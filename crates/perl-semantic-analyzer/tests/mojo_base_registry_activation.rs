#![deny(clippy::map_err_ignore)]
// Cohort C1 activation (#12598): all production rows exact-excepted; new findings move the crate back to non-C1.
//! End-to-end registry-backed Mojo::Base activation/profile proof (#9681).
//!
//! Drives the #6820 checked SDK surface through the Mojo::Base adapter in
//! `perl-semantic-facts::framework_adapters::mojo_base` and the AST
//! activation extractor in
//! `perl_semantic_analyzer::analysis::mojo_base_activation`, including the
//! `test_corpus/real_projects/mojolicious_skeleton` fixture. Every typed
//! outcome and every negative control named in #9681 appears as one named
//! test.

use perl_semantic_analyzer::Parser;
use perl_semantic_analyzer::analysis::mojo_base_activation::extract_mojo_base_activation_sites;
use perl_semantic_facts::framework::{
    AdapterCancellation, AdapterDetectionInput, DetectionOutcome, ModuleObservationReceipt,
    ModuleSelectorEvaluation, ModuleSelectorOutcome,
};
use perl_semantic_facts::framework::{
    DetectionAbsenceReason, DetectionEvidenceClass, ModuleActivationIdentity, ModuleVersionEvidence,
};
use perl_semantic_facts::framework_adapters::mojo_base::{
    MOJO_BASE_VERSION_CONSTRAINT, MojoBaseActivationOutcome, MojoBaseParentSelection,
    detect_mojo_base, mojo_base_activation_facts, mojo_base_descriptor,
    parse_mojo_base_import_args,
};
use perl_semantic_facts::{Confidence, FileId, SourceGeneration};
use perl_tdd_support::{must, must_some};

fn observation(
    evaluations: Vec<ModuleSelectorEvaluation>,
    generation: &str,
) -> ModuleObservationReceipt {
    ModuleObservationReceipt::new(
        "module-resolver.v1",
        "root:fixture",
        "project-environment.v1",
        SourceGeneration::known(generation),
        "sha256:fixture-input",
        evaluations,
    )
}

fn matched_mojo_base(
    version: Option<&str>,
    generation: &str,
    evidence: DetectionEvidenceClass,
) -> ModuleSelectorEvaluation {
    let activation = ModuleActivationIdentity::new(
        "Mojo::Base",
        Some(FileId(7)),
        SourceGeneration::known(generation),
    );
    let activation = match version {
        Some(version) => activation.with_observed_version(ModuleVersionEvidence::new(
            version,
            SourceGeneration::known(generation),
        )),
        None => activation,
    };
    ModuleSelectorEvaluation::new(
        "Mojo::Base",
        ModuleSelectorOutcome::Matched { activation, evidence_class: evidence },
    )
}

fn input(evaluations: Vec<ModuleSelectorEvaluation>, generation: &str) -> AdapterDetectionInput {
    AdapterDetectionInput::new(
        mojo_base_descriptor(),
        observation(evaluations, generation),
        None,
        AdapterCancellation::active(),
    )
}

fn detected_input(version: &str, generation: &str) -> AdapterDetectionInput {
    input(
        vec![matched_mojo_base(Some(version), generation, DetectionEvidenceClass::ResolvedModule)],
        generation,
    )
}

fn sites(
    code: &str,
    generation: &str,
) -> Vec<perl_semantic_analyzer::analysis::mojo_base_activation::MojoBaseActivationSite> {
    let mut parser = Parser::new(code);
    let ast = must(parser.parse());
    extract_mojo_base_activation_sites(&ast, code, FileId(1), SourceGeneration::known(generation))
}

fn first_site(
    code: &str,
    generation: &str,
) -> perl_semantic_analyzer::analysis::mojo_base_activation::MojoBaseActivationSite {
    must_some(sites(code, generation).into_iter().next())
}

fn facts_for(
    code: &str,
    detection_generation: &str,
    site_generation: &str,
) -> perl_semantic_facts::framework_adapters::mojo_base::MojoBaseActivationFacts {
    let detection = detect_mojo_base(&detected_input("9.34", detection_generation));
    let site = first_site(code, site_generation);
    mojo_base_activation_facts(&detection, &site.anchor, &site.evidence)
}

// Positive form 1: `use Mojo::Base -base;` activates exactly once through the
// registry, with no second site alongside a nested-module import.
#[test]
fn base_form_activates_via_registry_exactly_once() {
    let code =
        "package MyApp;\nuse Mojo::Base -base;\nuse Mojo::Base::_RoleBase;\nhas attr => 1;\n";
    let found = sites(code, "gen-1");
    assert_eq!(found.len(), 1, "one activation site: `_RoleBase` is not one");

    let detection = detect_mojo_base(&detected_input("9.34", "gen-1"));
    assert_eq!(
        detection.outcome,
        DetectionOutcome::Detected {
            confidence: Confidence::High,
            framework_version: Some("9.34".to_string())
        },
        "resolved supported Mojo::Base identity activates exactly once"
    );

    let facts = mojo_base_activation_facts(&detection, &found[0].anchor, &found[0].evidence);
    assert!(facts.is_exact());
    assert_eq!(facts.outcome, MojoBaseActivationOutcome::ExactBaseActivation);
    assert_eq!(facts.package.as_deref(), Some("MyApp"));
    assert_eq!(facts.framework_version, "9.34");
    assert_eq!(facts.source_generation, SourceGeneration::known("gen-1"));
    assert_eq!(
        must_some(facts.resolved_module.as_ref()).module_name,
        "Mojo::Base",
        "resolved module/source identity is retained on the profile"
    );
}

// Positive form 2: `use Mojo::Base 'Parent';` retains the literal parent
// spelling and its source range.
#[test]
fn literal_parent_form_retains_spelling_and_range() {
    let code = "package Log;\nuse Mojo::Base 'Mojo::EventEmitter';\n";
    let found = sites(code, "gen-1");
    assert_eq!(found.len(), 1);

    let detection = detect_mojo_base(&detected_input("9.34", "gen-1"));
    let facts = mojo_base_activation_facts(&detection, &found[0].anchor, &found[0].evidence);
    assert_eq!(
        facts.outcome,
        MojoBaseActivationOutcome::ExactLiteralParentActivation {
            parent: "Mojo::EventEmitter".to_string()
        }
    );
    let (start, end) = must_some(found[0].anchor.parent_range);
    assert_eq!(
        &code[(start as usize)..(end as usize)],
        "'Mojo::EventEmitter'",
        "literal parent range must cover the spelling"
    );
    assert_eq!(facts.parent_range, found[0].anchor.parent_range);
}

// Positive form 3: `use Mojo::Base 'Parent', -signatures;` represents the
// reviewed import option in the profile.
#[test]
fn signatures_option_is_represented() {
    let code = "package App;\nuse Mojo::Base 'Parent', -signatures;\n";
    let facts = facts_for(code, "gen-1", "gen-1");
    assert!(matches!(
        facts.outcome,
        MojoBaseActivationOutcome::ExactLiteralParentActivation { .. }
    ));
    assert!(facts.signatures, "`-signatures` must be represented on the profile");
    assert!(facts.unmodeled_options.is_empty());
}

// Corpus round-trip: the mojolicious_skeleton fixture drives an end-to-end
// exact base activation (version evidence read from the fixture).
#[test]
fn mojolicious_skeleton_corpus_drives_end_to_end_activation() {
    let skeleton_mojolicious_pm = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../test_corpus/real_projects/mojolicious_skeleton/lib/Mojolicious.pm"
    );
    let module_source = must(std::fs::read_to_string(skeleton_mojolicious_pm));
    let version = must_some(extract_version(&module_source));
    assert!(
        perl_semantic_facts::framework::version_constraint_matches(
            MOJO_BASE_VERSION_CONSTRAINT,
            &version
        ) == Some(true),
        "fixture version {version} must satisfy the reviewed constraint"
    );

    let mut parser = Parser::new(&module_source);
    let ast = must(parser.parse());
    let found = extract_mojo_base_activation_sites(
        &ast,
        &module_source,
        FileId(11),
        SourceGeneration::known("corpus-gen-1"),
    );
    let site = must_some(found.into_iter().next());
    assert_eq!(site.anchor.package.as_deref(), Some("Mojolicious"));
    assert_eq!(site.evidence.parent, MojoBaseParentSelection::Base);
    assert!(site.evidence.signatures, "fixture uses `-base, -signatures`");

    let detection = detect_mojo_base(&detected_input(&version, "corpus-gen-1"));
    assert!(detection.is_detected());
    let facts = mojo_base_activation_facts(&detection, &site.anchor, &site.evidence);
    assert_eq!(facts.outcome, MojoBaseActivationOutcome::ExactBaseActivation);
    assert_eq!(facts.framework_version, version);
}

fn extract_version(source: &str) -> Option<String> {
    let marker = "our $VERSION = '";
    let start = source.find(marker)? + marker.len();
    let end = source[start..].find('\'')? + start;
    Some(source[start..end].to_string())
}

// Negative control: an ordinary package defining `has` without Mojo::Base
// never activates; a raw module spelling or `require` is not import proof.
#[test]
fn has_calls_and_raw_spellings_are_not_activation_proof() {
    let code = "package App;\nuse strict;\nhas attr => (is => 'ro');\nrequire Mojo::Base;\nmy $x = 'Mojo::Base';\n";
    assert!(
        sites(code, "gen-1").is_empty(),
        "`has`, `require`, and raw spellings are not activation sites"
    );
    let detection = detect_mojo_base(&input(
        vec![ModuleSelectorEvaluation::new("Mojo::Base", ModuleSelectorOutcome::Absent)],
        "gen-1",
    ));
    let facts = mojo_base_activation_facts(
        &detection,
        &perl_semantic_facts::framework_adapters::mojo_base::MojoBaseSiteAnchor::new(
            Some("App".to_string()),
            0,
            1,
            None,
            SourceGeneration::known("gen-1"),
        ),
        &parse_mojo_base_import_args(&[]),
    );
    assert!(matches!(facts.outcome, MojoBaseActivationOutcome::AbsentWithCompleteEvidence { .. }));
}

// Negative control: the Mojo::Base module unresolved (or only name-matched,
// e.g. a same-named local module without resolved identity) cannot activate.
#[test]
fn unresolved_or_name_only_module_is_not_activation() {
    let unresolved = detect_mojo_base(&input(
        vec![ModuleSelectorEvaluation::unresolved("Mojo::Base", "not found in lib tree")],
        "gen-1",
    ));
    let site_anchor = perl_semantic_facts::framework_adapters::mojo_base::MojoBaseSiteAnchor::new(
        Some("App".to_string()),
        0,
        1,
        None,
        SourceGeneration::known("gen-1"),
    );
    let evidence = parse_mojo_base_import_args(&["-base".to_string()]);
    let facts = mojo_base_activation_facts(&unresolved, &site_anchor, &evidence);
    assert!(
        matches!(facts.outcome, MojoBaseActivationOutcome::MissingOrUnavailableModule { .. }),
        "unresolved module must be a typed missing/unavailable outcome, got {:?}",
        facts.outcome
    );

    let name_only = detect_mojo_base(&input(
        vec![matched_mojo_base(Some("9.34"), "gen-1", DetectionEvidenceClass::NameOnly)],
        "gen-1",
    ));
    let facts = mojo_base_activation_facts(&name_only, &site_anchor, &evidence);
    assert!(
        matches!(facts.outcome, MojoBaseActivationOutcome::UnsupportedVersionOrProfile { .. }),
        "a module merely named Mojo::Base must not produce exact facts, got {:?}",
        facts.outcome
    );
}

// Negative control: unsupported version and unsupported import profile.
#[test]
fn unsupported_version_and_profile_do_not_activate() {
    let detection = detect_mojo_base(&detected_input("10.0.0", "gen-1"));
    assert_eq!(
        detection.outcome,
        DetectionOutcome::Absent { reason: DetectionAbsenceReason::VersionConstraintNotSatisfied }
    );
    let facts = facts_for("package App;\nuse Mojo::Base -base;\n", "gen-1", "gen-1").outcome;
    assert_eq!(facts, MojoBaseActivationOutcome::ExactBaseActivation);
    let site = first_site("package App;\nuse Mojo::Base -base;\n", "gen-1");
    let unsupported_version = mojo_base_activation_facts(&detection, &site.anchor, &site.evidence);
    assert!(
        matches!(
            unsupported_version.outcome,
            MojoBaseActivationOutcome::UnsupportedVersionOrProfile { .. }
        ),
        "unsupported version must stay typed non-exact"
    );

    let unreviewed = parse_mojo_base_import_args(&["-base".to_string(), "-future".to_string()]);
    let detection = detect_mojo_base(&detected_input("9.34", "gen-1"));
    let unsupported_profile = mojo_base_activation_facts(&detection, &site.anchor, &unreviewed);
    assert!(
        matches!(
            unsupported_profile.outcome,
            MojoBaseActivationOutcome::UnsupportedVersionOrProfile { .. }
        ),
        "import options outside the reviewed profile must stay typed non-exact"
    );
}

// Negative control: computed parent expression stays a dynamic boundary.
#[test]
fn computed_parent_is_a_dynamic_boundary() {
    let facts = facts_for("package App;\nuse Mojo::Base $parent;\n", "gen-1", "gen-1");
    assert!(
        matches!(facts.outcome, MojoBaseActivationOutcome::DynamicOrUnmodeledParent { .. }),
        "computed parent must not normalize into an exact profile, got {:?}",
        facts.outcome
    );
    assert!(!facts.is_exact());
}

// Negative control: import removed or moved between packages invalidates the
// old activation before reuse.
#[test]
fn removed_or_moved_import_invalidates_old_activation() {
    // Import removed: no site exists at the newer generation.
    let code_v2 = "package App;\nhas attr => 1;\n";
    assert!(sites(code_v2, "gen-2").is_empty());
    let detection = detect_mojo_base(&input(
        vec![ModuleSelectorEvaluation::new("Mojo::Base", ModuleSelectorOutcome::Absent)],
        "gen-2",
    ));
    let stale_anchor = perl_semantic_facts::framework_adapters::mojo_base::MojoBaseSiteAnchor::new(
        Some("App".to_string()),
        13,
        34,
        None,
        SourceGeneration::known("gen-1"),
    );
    let facts = mojo_base_activation_facts(
        &detection,
        &stale_anchor,
        &parse_mojo_base_import_args(&["-base".to_string()]),
    );
    assert!(!facts.is_exact(), "a gen-1 site must not survive into gen-2");

    // Import moved into another package: the activating package is scoped.
    let code = "package App;\nuse Mojo::Base -base;\npackage Other;\nhas attr => 1;\n";
    let found = sites(code, "gen-1");
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].anchor.package.as_deref(), Some("App"));
}

// Negative control: a stale detection finishing after a newer source/module
// generation stays typed stale and cannot be reused.
#[test]
fn stale_detection_cannot_activate_against_newer_source() {
    let facts = facts_for("package App;\nuse Mojo::Base -base;\n", "gen-1", "gen-2");
    assert!(
        matches!(facts.outcome, MojoBaseActivationOutcome::StaleOrIncompleteInput { .. }),
        "site generation ahead of detection must be stale, got {:?}",
        facts.outcome
    );

    // Module evidence older than the detection generation is also stale.
    let stale_module_input = input(
        vec![matched_mojo_base(Some("9.34"), "gen-1", DetectionEvidenceClass::ResolvedModule)],
        "gen-2",
    );
    let detection = detect_mojo_base(&stale_module_input);
    let site = first_site("package App;\nuse Mojo::Base -base;\n", "gen-2");
    let facts = mojo_base_activation_facts(&detection, &site.anchor, &site.evidence);
    assert!(
        matches!(facts.outcome, MojoBaseActivationOutcome::StaleOrIncompleteInput { .. }),
        "module generation behind the detection must be stale, got {:?}",
        facts.outcome
    );
}

// Negative control: malformed/recovered import stays typed malformed.
#[test]
fn recovered_or_malformed_import_cannot_activate() {
    let code = "package App;\nuse Mojo::Base 'Pare;\n";
    let mut parser = Parser::new(code);
    let ast = must(parser.parse());
    let found =
        extract_mojo_base_activation_sites(&ast, code, FileId(1), SourceGeneration::known("gen-1"));
    let site = must_some(found.into_iter().next());
    assert!(
        matches!(site.evidence.parent, MojoBaseParentSelection::Malformed { .. }),
        "recovered quote fragment must classify malformed, got {:?}",
        site.evidence.parent
    );
    let detection = detect_mojo_base(&detected_input("9.34", "gen-1"));
    let facts = mojo_base_activation_facts(&detection, &site.anchor, &site.evidence);
    assert!(
        matches!(facts.outcome, MojoBaseActivationOutcome::RecoveredOrMalformedSource { .. }),
        "malformed source must not normalize into an exact profile"
    );
}

// Negative control: multi-root same-package isolation (distinct input
// identities scope the activations).
#[test]
fn two_roots_with_same_package_stay_isolated() {
    let detection_a = detect_mojo_base(&detected_input("9.34", "gen-a"));
    let detection_b = detect_mojo_base(&detected_input("9.34", "gen-b"));
    let site_a = first_site("package App;\nuse Mojo::Base -base;\n", "gen-a");
    let site_b = first_site("package App;\nuse Mojo::Base -base;\n", "gen-b");
    let facts_a = mojo_base_activation_facts(&detection_a, &site_a.anchor, &site_a.evidence);
    let facts_b = mojo_base_activation_facts(&detection_b, &site_b.anchor, &site_b.evidence);
    assert!(facts_a.is_exact() && facts_b.is_exact());
    assert_ne!(facts_a.source_generation, facts_b.source_generation);
    assert_ne!(
        detection_a.input_identity, detection_b.input_identity,
        "roots are isolated through distinct deterministic input identities"
    );
}

// Review regression: two roots with the SAME generation, package, spans, and
// module/version stay isolated through the carried root/environment identity.
#[test]
fn same_generation_distinct_roots_stay_isolated() {
    fn root_input(root: &str) -> AdapterDetectionInput {
        AdapterDetectionInput::new(
            mojo_base_descriptor(),
            ModuleObservationReceipt::new(
                "module-resolver.v1",
                root,
                "project-environment.v1",
                SourceGeneration::known("gen-1"),
                "sha256:fixture-input",
                vec![matched_mojo_base(
                    Some("9.34"),
                    "gen-1",
                    DetectionEvidenceClass::ResolvedModule,
                )],
            ),
            None,
            AdapterCancellation::active(),
        )
    }
    let code = "package App;\nuse Mojo::Base 'Parent';\n";
    let site_a = first_site(code, "gen-1");
    let site_b = first_site(code, "gen-1");
    let detection_a = detect_mojo_base(&root_input("root:a"));
    let detection_b = detect_mojo_base(&root_input("root:b"));
    let facts_a = mojo_base_activation_facts(&detection_a, &site_a.anchor, &site_a.evidence);
    let facts_b = mojo_base_activation_facts(&detection_b, &site_b.anchor, &site_b.evidence);
    assert!(facts_a.is_exact() && facts_b.is_exact());
    assert_eq!(facts_a.source_generation, facts_b.source_generation);
    assert_ne!(
        facts_a.scope_identity.as_deref(),
        facts_b.scope_identity.as_deref(),
        "the observation's root/scope identity must stay load-bearing on the facts"
    );
    assert_ne!(facts_a, facts_b, "distinct roots produce distinct facts");
}

// Typed outcome: ambiguous module identity.
#[test]
fn ambiguous_module_identity_is_a_conflict() {
    let detection = detect_mojo_base(&input(
        vec![ModuleSelectorEvaluation::new(
            "Mojo::Base",
            ModuleSelectorOutcome::Ambiguous { reason: "two roots define Mojo::Base".to_string() },
        )],
        "gen-1",
    ));
    assert!(matches!(detection.outcome, DetectionOutcome::Conflicting { .. }));
    let site = first_site("package App;\nuse Mojo::Base -base;\n", "gen-1");
    let facts = mojo_base_activation_facts(&detection, &site.anchor, &site.evidence);
    assert!(matches!(
        facts.outcome,
        MojoBaseActivationOutcome::AmbiguousOrConflictingModule { .. }
    ));
}

// Typed outcome: instrument failure (cancelled / budget-exhausted detection).
#[test]
fn cancelled_detection_is_an_instrument_failure() {
    use perl_semantic_facts::framework::AdapterDetectionResult;
    let cancelled = AdapterDetectionResult::new(
        mojo_base_descriptor(),
        SourceGeneration::known("gen-1"),
        DetectionOutcome::Cancelled,
    );
    let site = first_site("package App;\nuse Mojo::Base -base;\n", "gen-1");
    let facts = mojo_base_activation_facts(&cancelled, &site.anchor, &site.evidence);
    assert!(
        matches!(facts.outcome, MojoBaseActivationOutcome::InstrumentFailure { .. }),
        "cancelled detection must be a typed instrument failure"
    );
}

// Review regression: a pre-cancelled admission snapshot fails closed before
// module evidence is evaluated, even with a matched supported module.
#[test]
fn pre_cancelled_input_detection_fails_closed() {
    let mut cancelled_input = detected_input("9.34", "gen-1");
    cancelled_input.cancellation = perl_semantic_facts::framework::AdapterCancellation::cancelled();
    let detection = detect_mojo_base(&cancelled_input);
    assert_eq!(detection.outcome, DetectionOutcome::Cancelled);
    let site = first_site("package App;\nuse Mojo::Base -base;\n", "gen-1");
    let facts = mojo_base_activation_facts(&detection, &site.anchor, &site.evidence);
    assert!(matches!(facts.outcome, MojoBaseActivationOutcome::InstrumentFailure { .. }));
    assert!(!facts.is_exact());
}

// Review regression: interpolated double-quoted parents and falsy quoted
// parents cannot become exact literals.
#[test]
fn interpolated_and_falsy_quoted_parents_cannot_activate() {
    let interpolated = facts_for("package App;\nuse Mojo::Base \"$parent\";\n", "gen-1", "gen-1");
    assert!(
        matches!(interpolated.outcome, MojoBaseActivationOutcome::DynamicOrUnmodeledParent { .. }),
        "interpolated parent must stay dynamic, got {:?}",
        interpolated.outcome
    );

    for code in ["package App;\nuse Mojo::Base '';\n", "package App;\nuse Mojo::Base '0';\n"] {
        let falsy = facts_for(code, "gen-1", "gen-1");
        assert!(
            matches!(falsy.outcome, MojoBaseActivationOutcome::AbsentWithCompleteEvidence { .. }),
            "falsy quoted parent degrades to strict-only, got {:?} for {code:?}",
            falsy.outcome
        );
    }
}

// Review regression: a leading `-signatures` flag occupies the base/parent
// slot of `Mojo::Base::import` and is not a reviewed activation form.
#[test]
fn leading_signatures_flag_is_malformed() {
    let facts = facts_for("package App;\nuse Mojo::Base -signatures;\n", "gen-1", "gen-1");
    assert!(
        matches!(facts.outcome, MojoBaseActivationOutcome::RecoveredOrMalformedSource { .. }),
        "leading `-signatures` must stay malformed, got {:?}",
        facts.outcome
    );
    assert!(!facts.is_exact());
}

// Review regression: a raw Detected result without contributing module and
// version evidence cannot become exact activation.
#[test]
fn raw_detected_result_without_evidence_is_not_exact() {
    use perl_semantic_facts::framework::AdapterDetectionResult;
    let raw = AdapterDetectionResult::new(
        mojo_base_descriptor(),
        SourceGeneration::known("gen-1"),
        DetectionOutcome::Detected {
            confidence: Confidence::High,
            framework_version: Some("9.34".to_string()),
        },
    );
    let site = first_site("package App;\nuse Mojo::Base -base;\n", "gen-1");
    let facts = mojo_base_activation_facts(&raw, &site.anchor, &site.evidence);
    assert!(
        matches!(facts.outcome, MojoBaseActivationOutcome::StaleOrIncompleteInput { .. }),
        "missing contributing evidence must stay incomplete, got {:?}",
        facts.outcome
    );
}

// Review regression: a bareword parent that also occurs inside the module
// name must not capture the module name's range.
#[test]
fn bareword_parent_range_avoids_module_name_capture() {
    let code = "package App;\nuse Mojo::Base Base;\n";
    let found = sites(code, "gen-1");
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].evidence.parent, MojoBaseParentSelection::Literal("Base".to_string()));
    let range = must_some(found[0].anchor.parent_range);
    let (start, end) = range;
    assert_eq!(&code[(start as usize)..(end as usize)], "Base");
}

// Typed outcome: strict-only imports are complete-evidence absence, and a
// `use Mojo::Base;` without arguments never activates.
#[test]
fn strict_only_import_is_absent_with_complete_evidence() {
    for code in ["package App;\nuse Mojo::Base -strict;\n", "package App;\nuse Mojo::Base;\n"] {
        let facts = facts_for(code, "gen-1", "gen-1");
        assert!(
            matches!(facts.outcome, MojoBaseActivationOutcome::AbsentWithCompleteEvidence { .. }),
            "strict-only import must be complete-evidence absence, got {:?} for {code:?}",
            facts.outcome
        );
    }
}

// Typed outcome: missing version evidence stays unsupported, not exact.
#[test]
fn missing_version_evidence_is_not_exact() {
    let detection = detect_mojo_base(&input(
        vec![matched_mojo_base(None, "gen-1", DetectionEvidenceClass::ResolvedModule)],
        "gen-1",
    ));
    assert!(matches!(detection.outcome, DetectionOutcome::Unsupported { .. }));
    let site = first_site("package App;\nuse Mojo::Base -base;\n", "gen-1");
    let facts = mojo_base_activation_facts(&detection, &site.anchor, &site.evidence);
    assert!(matches!(facts.outcome, MojoBaseActivationOutcome::UnsupportedVersionOrProfile { .. }));
}

// Acceptance: results and fingerprints are deterministic.
#[test]
fn results_and_fingerprints_are_deterministic() {
    let code = "package App;\nuse Mojo::Base 'Parent', -signatures;\n";
    let detection_a = detect_mojo_base(&detected_input("9.34", "gen-1"));
    let detection_b = detect_mojo_base(&detected_input("9.34", "gen-1"));
    assert_eq!(detection_a, detection_b, "detection is deterministic");
    assert_eq!(detection_a.input_identity, detection_b.input_identity);

    let site_a = first_site(code, "gen-1");
    let site_b = first_site(code, "gen-1");
    assert_eq!(site_a, site_b, "site extraction is deterministic");

    let facts_a = mojo_base_activation_facts(&detection_a, &site_a.anchor, &site_a.evidence);
    let facts_b = mojo_base_activation_facts(&detection_b, &site_b.anchor, &site_b.evidence);
    assert_eq!(facts_a, facts_b);
    assert!(facts_a.is_exact());
}

// Shadow honesty: the adapter's output cannot become publication authority.
#[test]
fn shadow_adapter_output_cannot_become_authority() {
    use perl_semantic_facts::framework::{
        AdapterAuthorityError, AdapterInput, AdapterSourceScope, FactClass,
    };
    let detection = detect_mojo_base(&detected_input("9.34", "gen-1"));
    let scope = AdapterSourceScope::new(
        FileId(3),
        SourceGeneration::known("gen-1"),
        None,
        None,
        Some("App".to_string()),
    );
    let adapter_input = AdapterInput::new(
        mojo_base_descriptor(),
        scope,
        vec![FactClass::FrameworkImports],
        Vec::new(),
        None,
        AdapterCancellation::active(),
    );
    let result = perl_semantic_facts::framework::AdapterResult::new(
        mojo_base_descriptor(),
        adapter_input.source_scope.clone(),
        SourceGeneration::known("gen-1"),
        perl_semantic_facts::framework::AdapterOutcome::Applied {
            sink: perl_semantic_facts::framework::FactSink::new(
                perl_semantic_facts::framework::FactSinkId(1),
                mojo_base_descriptor().adapter_id,
            ),
            limitations: Vec::new(),
        },
    );
    assert_eq!(
        result.validate_authority_against(&adapter_input),
        Err(AdapterAuthorityError::NonProduction),
        "no live provider is promoted by this shadow adapter"
    );
    assert!(detection.is_detected());
}
